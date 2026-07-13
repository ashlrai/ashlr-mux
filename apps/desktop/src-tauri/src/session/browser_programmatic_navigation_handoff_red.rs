//! Deterministic exactly-once handoff for programmatic browser navigation.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FakeNavigationKey {
    panel_id: String,
    runtime_id: u64,
    url: String,
}

impl FakeNavigationKey {
    fn new(panel_id: &str, runtime_id: u64, url: &str) -> Self {
        Self {
            panel_id: panel_id.to_string(),
            runtime_id,
            url: url.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeHandoffState {
    NavigateInFlight { callback_observed: bool },
    CallbackResponsible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeReservationOwner {
    Mutation(u64),
    Callback(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeNavigateOutcome {
    Success,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FakeCallbackOutcome {
    DeferredToCommand,
    Published,
    Ignored,
}

#[derive(Default)]
struct FakeProgrammaticNavigationCoordinator {
    handoffs: Mutex<BTreeMap<FakeNavigationKey, FakeHandoffState>>,
    reservations: Mutex<BTreeMap<String, FakeReservationOwner>>,
    webviews: Mutex<BTreeMap<String, u64>>,
    records: Mutex<Vec<FakeNavigationKey>>,
    events: Mutex<Vec<FakeNavigationKey>>,
    lock_order: Mutex<Vec<&'static str>>,
    registry_locks_held: AtomicUsize,
}

impl FakeProgrammaticNavigationCoordinator {
    fn begin(self: &Arc<Self>, key: &FakeNavigationKey) {
        self.reservations.lock().unwrap().insert(
            key.panel_id.clone(),
            FakeReservationOwner::Mutation(key.runtime_id),
        );
        self.handoffs.lock().unwrap().insert(
            key.clone(),
            FakeHandoffState::NavigateInFlight {
                callback_observed: false,
            },
        );
    }

    fn callback(self: &Arc<Self>, key: &FakeNavigationKey) -> FakeCallbackOutcome {
        self.note_lock("navigation_handoffs");
        let mut handoffs = self.handoffs.lock().unwrap();
        let Some(state) = handoffs.get_mut(key) else {
            self.leave_lock();
            return FakeCallbackOutcome::Ignored;
        };
        if let FakeHandoffState::NavigateInFlight { callback_observed } = state {
            self.note_lock("reservations");
            let reservations = self.reservations.lock().unwrap();
            if reservations.get(&key.panel_id)
                != Some(&FakeReservationOwner::Mutation(key.runtime_id))
            {
                self.leave_lock();
                drop(reservations);
                self.leave_lock();
                return FakeCallbackOutcome::Ignored;
            }
            *callback_observed = true;
            self.leave_lock();
            drop(reservations);
            self.leave_lock();
            return FakeCallbackOutcome::DeferredToCommand;
        }

        self.note_lock("reservations");
        let mut reservations = self.reservations.lock().unwrap();
        self.note_lock("webviews");
        let webviews = self.webviews.lock().unwrap();
        if reservations.contains_key(&key.panel_id)
            || webviews.get(&key.panel_id).copied() != Some(key.runtime_id)
        {
            self.leave_lock();
            drop(webviews);
            self.leave_lock();
            drop(reservations);
            self.leave_lock();
            return FakeCallbackOutcome::Ignored;
        }
        reservations.insert(
            key.panel_id.clone(),
            FakeReservationOwner::Callback(key.runtime_id),
        );
        self.note_lock("network_records");
        self.records.lock().unwrap().push(key.clone());
        self.leave_lock();
        handoffs.remove(key);
        self.leave_lock();
        drop(webviews);
        self.leave_lock();
        drop(reservations);
        self.leave_lock();
        drop(handoffs);

        self.emit(key);
        self.reservations.lock().unwrap().remove(&key.panel_id);
        FakeCallbackOutcome::Published
    }

    fn complete(
        self: &Arc<Self>,
        key: &FakeNavigationKey,
        outcome: FakeNavigateOutcome,
        while_handoff_locked: impl FnOnce(),
    ) -> Result<(), String> {
        self.note_lock("navigation_handoffs");
        let mut handoffs = self.handoffs.lock().unwrap();
        let state = handoffs
            .get(key)
            .copied()
            .ok_or_else(|| "missing programmatic navigation handoff".to_string())?;
        if outcome != FakeNavigateOutcome::Success {
            handoffs.remove(key);
            self.release_mutation_while_handoff_locked(key);
            self.leave_lock();
            return Err(match outcome {
                FakeNavigateOutcome::Error => "navigate failed",
                FakeNavigateOutcome::Cancelled => "navigate cancelled",
                FakeNavigateOutcome::Success => unreachable!(),
            }
            .to_string());
        }

        match state {
            FakeHandoffState::NavigateInFlight {
                callback_observed: true,
            } => {
                self.note_lock("reservations");
                let reservations = self.reservations.lock().unwrap();
                self.note_lock("webviews");
                let webviews = self.webviews.lock().unwrap();
                assert_eq!(
                    reservations.get(&key.panel_id),
                    Some(&FakeReservationOwner::Mutation(key.runtime_id))
                );
                assert_eq!(webviews.get(&key.panel_id), Some(&key.runtime_id));
                self.note_lock("network_records");
                self.records.lock().unwrap().push(key.clone());
                self.leave_lock();
                handoffs.remove(key);
                self.leave_lock();
                drop(webviews);
                self.leave_lock();
                drop(reservations);
                self.leave_lock();
                drop(handoffs);
                self.emit(key);
                self.reservations.lock().unwrap().remove(&key.panel_id);
            }
            FakeHandoffState::NavigateInFlight {
                callback_observed: false,
            } => {
                handoffs.insert(key.clone(), FakeHandoffState::CallbackResponsible);
                while_handoff_locked();
                self.release_mutation_while_handoff_locked(key);
                self.leave_lock();
            }
            FakeHandoffState::CallbackResponsible => {
                self.leave_lock();
                return Err("programmatic navigation already delegated".to_string());
            }
        }
        Ok(())
    }

    fn release_mutation_while_handoff_locked(&self, key: &FakeNavigationKey) {
        self.note_lock("reservations");
        let removed = self.reservations.lock().unwrap().remove(&key.panel_id);
        assert_eq!(
            removed,
            Some(FakeReservationOwner::Mutation(key.runtime_id))
        );
        self.leave_lock();
    }

    fn emit(&self, key: &FakeNavigationKey) {
        assert_eq!(
            self.registry_locks_held.load(Ordering::SeqCst),
            0,
            "event emit must hold no coordinator or registry mutex"
        );
        self.events.lock().unwrap().push(key.clone());
    }

    fn note_lock(&self, name: &'static str) {
        self.registry_locks_held.fetch_add(1, Ordering::SeqCst);
        self.lock_order.lock().unwrap().push(name);
    }

    fn leave_lock(&self) {
        self.registry_locks_held.fetch_sub(1, Ordering::SeqCst);
    }

    fn assert_exactly_once(&self, key: &FakeNavigationKey) {
        assert_eq!(
            self.records
                .lock()
                .unwrap()
                .iter()
                .filter(|candidate| *candidate == key)
                .count(),
            1
        );
        assert_eq!(
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|candidate| *candidate == key)
                .count(),
            1
        );
    }
}

#[test]
fn early_callback_defers_and_command_publishes_exactly_once() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 7, "https://early.example");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert("panel-a".into(), 7);
    coordinator.begin(&key);

    assert_eq!(
        coordinator.callback(&key),
        FakeCallbackOutcome::DeferredToCommand
    );
    assert_eq!(
        coordinator.complete(&key, FakeNavigateOutcome::Success, || {}),
        Ok(())
    );
    coordinator.assert_exactly_once(&key);
    assert!(coordinator.reservations.lock().unwrap().is_empty());
}

#[test]
fn callback_racing_between_navigate_return_and_completion_defers_to_command() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 8, "https://race.example");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert("panel-a".into(), 8);
    coordinator.begin(&key);
    let callback_coordinator = Arc::clone(&coordinator);
    let callback_key = key.clone();
    let callback = thread::spawn(move || callback_coordinator.callback(&callback_key));

    assert_eq!(
        callback.join().unwrap(),
        FakeCallbackOutcome::DeferredToCommand
    );
    assert_eq!(
        coordinator.complete(&key, FakeNavigateOutcome::Success, || {}),
        Ok(())
    );
    coordinator.assert_exactly_once(&key);
}

#[test]
fn late_callback_is_delegated_without_a_release_gap() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 9, "https://late.example");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert("panel-a".into(), 9);
    coordinator.begin(&key);

    assert_eq!(
        coordinator.complete(&key, FakeNavigateOutcome::Success, || {}),
        Ok(())
    );
    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    coordinator.assert_exactly_once(&key);
    assert!(coordinator.reservations.lock().unwrap().is_empty());
}

#[test]
fn callback_waiting_at_completion_observes_callback_responsibility_after_release() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 10, "https://immediate.example");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert("panel-a".into(), 10);
    coordinator.begin(&key);
    let (start_tx, start_rx) = mpsc::channel();
    let (entered_tx, entered_rx) = mpsc::channel();
    let callback_coordinator = Arc::clone(&coordinator);
    let callback_key = key.clone();
    let callback = thread::spawn(move || {
        start_rx.recv().unwrap();
        entered_tx.send(()).unwrap();
        callback_coordinator.callback(&callback_key)
    });

    assert_eq!(
        coordinator.complete(&key, FakeNavigateOutcome::Success, || {
            start_tx.send(()).unwrap();
            entered_rx.recv().unwrap();
        }),
        Ok(())
    );
    assert_eq!(callback.join().unwrap(), FakeCallbackOutcome::Published);
    coordinator.assert_exactly_once(&key);
}

#[test]
fn stale_runtime_and_url_mismatch_cannot_claim_the_handoff_or_duplicate() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 11, "https://right.example");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert("panel-a".into(), 11);
    coordinator.begin(&key);

    assert_eq!(
        coordinator.callback(&FakeNavigationKey::new(
            "panel-a",
            10,
            "https://right.example"
        )),
        FakeCallbackOutcome::Ignored
    );
    assert_eq!(
        coordinator.callback(&FakeNavigationKey::new(
            "panel-a",
            11,
            "https://wrong.example"
        )),
        FakeCallbackOutcome::Ignored
    );
    coordinator
        .complete(&key, FakeNavigateOutcome::Success, || {})
        .unwrap();
    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Ignored);
    coordinator.assert_exactly_once(&key);
}

#[test]
fn navigate_error_or_cancel_clears_handoff_without_record_event_or_reservation() {
    for (runtime_id, outcome) in [
        (12, FakeNavigateOutcome::Error),
        (13, FakeNavigateOutcome::Cancelled),
    ] {
        let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
        let key = FakeNavigationKey::new("panel-a", runtime_id, "https://failed.example");
        coordinator
            .webviews
            .lock()
            .unwrap()
            .insert("panel-a".into(), runtime_id);
        coordinator.begin(&key);
        assert_eq!(
            coordinator.callback(&key),
            FakeCallbackOutcome::DeferredToCommand
        );

        assert!(coordinator.complete(&key, outcome, || {}).is_err());
        assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Ignored);
        assert!(coordinator.records.lock().unwrap().is_empty());
        assert!(coordinator.events.lock().unwrap().is_empty());
        assert!(coordinator.reservations.lock().unwrap().is_empty());
        assert!(coordinator.handoffs.lock().unwrap().is_empty());
    }
}

#[test]
fn handoff_uses_fixed_lock_order_and_emit_is_mutex_free() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 14, "https://order.example");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert("panel-a".into(), 14);
    coordinator.begin(&key);
    coordinator.callback(&key);
    coordinator
        .complete(&key, FakeNavigateOutcome::Success, || {})
        .unwrap();

    let order = coordinator.lock_order.lock().unwrap();
    let publish = order
        .windows(4)
        .find(|window| {
            *window
                == [
                    "navigation_handoffs",
                    "reservations",
                    "webviews",
                    "network_records",
                ]
        })
        .expect("command publication follows the fixed lock order");
    assert_eq!(
        publish,
        [
            "navigation_handoffs",
            "reservations",
            "webviews",
            "network_records",
        ]
    );
}

#[test]
fn production_has_an_exact_programmatic_navigation_handoff() {
    let browser = include_str!("../browser.rs");
    for required in [
        "BrowserProgrammaticNavigationHandoff",
        "programmatic_navigation_handoffs",
        "begin_browser_programmatic_navigation",
        "complete_browser_programmatic_navigation",
        "observe_browser_programmatic_navigation_callback",
        "BROWSER_NAVIGATION_HANDOFF_LOCK_ORDER",
    ] {
        assert!(
            browser.contains(required),
            "missing navigation handoff: {required}"
        );
    }

    let handoff = source_item(browser, "struct BrowserProgrammaticNavigationHandoff");
    for exact in ["panel_id", "runtime_id", "url"] {
        assert!(handoff.contains(exact), "handoff key missing {exact}");
    }

    let complete = source_item(browser, "fn complete_browser_programmatic_navigation(");
    assert!(complete.contains("callback_observed"));
    assert!(complete.contains("CallbackResponsible"));
    let handoff_lock = complete.find("programmatic_navigation_handoffs").unwrap();
    let release = complete[handoff_lock..].find("reserved_panel_ids").unwrap() + handoff_lock;
    assert!(
        handoff_lock < release,
        "handoff lock must span mutation release"
    );

    let callback = callback_body(browser);
    assert!(callback.contains("observe_browser_programmatic_navigation_callback"));
    assert!(callback.contains("runtime_id"));
    assert!(callback.contains("url.as_str()"));

    let upsert = source_item(browser, "fn upsert_browser_webview");
    let navigate = upsert.find(".navigate(").expect("programmatic navigate");
    let after_navigate = &upsert[navigate..];
    assert!(after_navigate.contains("complete_browser_programmatic_navigation"));
    assert!(
        !after_navigate.contains("record_observed_navigation(state, &panel_id"),
        "programmatic navigation must not unconditionally record by panel after release"
    );

    let publish = source_item(browser, "fn publish_browser_programmatic_navigation(");
    let order = [
        "programmatic_navigation_handoffs",
        "reserved_panel_ids",
        "webviews",
        "network_records",
    ];
    let mut cursor = 0usize;
    for lock in order {
        let position = publish[cursor..]
            .find(lock)
            .unwrap_or_else(|| panic!("publish missing ordered lock: {lock}"))
            + cursor;
        cursor = position + lock.len();
    }
    assert!(!publish.contains(".emit("));
}

fn callback_body(source: &str) -> &str {
    let start = source
        .find(".on_navigation(move |url|")
        .expect("browser navigation callback");
    source_item(&source[start..], ".on_navigation(move |url|")
}

fn source_item<'a>(source: &'a str, marker: &str) -> &'a str {
    let start = source
        .find(marker)
        .unwrap_or_else(|| panic!("missing source marker: {marker}"));
    let brace = source[start..]
        .find('{')
        .map(|offset| start + offset)
        .unwrap_or_else(|| panic!("missing opening brace after: {marker}"));
    let mut depth = 0usize;
    for (offset, byte) in source[brace..].bytes().enumerate() {
        match byte {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &source[start..=brace + offset];
                }
            }
            _ => {}
        }
    }
    panic!("unclosed source item: {marker}")
}
