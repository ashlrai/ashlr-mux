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
    Handoff(u64),
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
    reentrant_mutation_rejections: AtomicUsize,
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
        if let Some(FakeHandoffState::NavigateInFlight { callback_observed }) =
            handoffs.get_mut(key)
        {
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
        let expected_owner = handoffs
            .contains_key(key)
            .then_some(FakeReservationOwner::Handoff(key.runtime_id));
        if webviews.get(&key.panel_id).copied() != Some(key.runtime_id)
            || reservations.get(&key.panel_id).copied() != expected_owner
        {
            self.leave_lock();
            drop(webviews);
            self.leave_lock();
            drop(reservations);
            self.leave_lock();
            return FakeCallbackOutcome::Ignored;
        }
        let previous_owner = reservations.insert(
            key.panel_id.clone(),
            FakeReservationOwner::Callback(key.runtime_id),
        );
        assert_eq!(previous_owner, expected_owner);
        self.note_lock("network_records");
        self.records.lock().unwrap().push(key.clone());
        self.leave_lock();
        self.leave_lock();
        drop(webviews);
        self.leave_lock();
        drop(reservations);
        self.leave_lock();
        drop(handoffs);

        let delegated = expected_owner.is_some();
        self.emit(key, delegated);
        self.finish_publication(
            key,
            delegated,
            FakeReservationOwner::Callback(key.runtime_id),
        );
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
                self.leave_lock();
                drop(webviews);
                self.leave_lock();
                drop(reservations);
                self.leave_lock();
                drop(handoffs);
                self.emit(key, true);
                self.finish_publication(key, true, FakeReservationOwner::Mutation(key.runtime_id));
            }
            FakeHandoffState::NavigateInFlight {
                callback_observed: false,
            } => {
                handoffs.insert(key.clone(), FakeHandoffState::CallbackResponsible);
                while_handoff_locked();
                self.transfer_mutation_to_handoff_while_locked(key);
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

    fn transfer_mutation_to_handoff_while_locked(&self, key: &FakeNavigationKey) {
        self.note_lock("reservations");
        let replaced = self.reservations.lock().unwrap().insert(
            key.panel_id.clone(),
            FakeReservationOwner::Handoff(key.runtime_id),
        );
        assert_eq!(
            replaced,
            Some(FakeReservationOwner::Mutation(key.runtime_id))
        );
        self.leave_lock();
    }

    fn try_detach_or_replace(&self, key: &FakeNavigationKey) -> Result<(), String> {
        let reservations = self.reservations.lock().unwrap();
        if reservations.contains_key(&key.panel_id) {
            return Err(format!("browser panel {} is reserved", key.panel_id));
        }
        drop(reservations);
        self.webviews.lock().unwrap().remove(&key.panel_id);
        Ok(())
    }

    fn emit(&self, key: &FakeNavigationKey, handoff_expected: bool) {
        assert_eq!(
            self.registry_locks_held.load(Ordering::SeqCst),
            0,
            "event emit must hold no coordinator or registry mutex"
        );
        assert!(
            matches!(
                self.reservations
                    .lock()
                    .unwrap()
                    .get(&key.panel_id)
                    .copied(),
                Some(FakeReservationOwner::Mutation(runtime_id))
                    | Some(FakeReservationOwner::Callback(runtime_id))
                    if runtime_id == key.runtime_id
            ),
            "emit must retain the exact runtime's publishing reservation"
        );
        assert_eq!(
            self.handoffs.lock().unwrap().contains_key(key),
            handoff_expected,
            "the exact delegated handoff must remain owned through emit"
        );
        assert!(
            self.try_detach_or_replace(key).is_err(),
            "exact logical reservation must reject reentrant detach/replacement through emit"
        );
        self.reentrant_mutation_rejections
            .fetch_add(1, Ordering::SeqCst);
        self.events.lock().unwrap().push(key.clone());
    }

    fn finish_publication(
        &self,
        key: &FakeNavigationKey,
        handoff_expected: bool,
        expected_owner: FakeReservationOwner,
    ) {
        let mut handoffs = self.handoffs.lock().unwrap();
        assert_eq!(handoffs.remove(key).is_some(), handoff_expected);
        let mut reservations = self.reservations.lock().unwrap();
        assert_eq!(reservations.remove(&key.panel_id), Some(expected_owner));
    }

    fn note_lock(&self, name: &'static str) {
        self.registry_locks_held.fetch_add(1, Ordering::SeqCst);
        self.lock_order.lock().unwrap().push(name);
    }

    fn leave_lock(&self) {
        self.registry_locks_held.fetch_sub(1, Ordering::SeqCst);
    }

    fn assert_exactly_once(&self, key: &FakeNavigationKey) {
        self.assert_publication_count(key, 1);
    }

    fn assert_publication_count(&self, key: &FakeNavigationKey, expected: usize) {
        assert_eq!(
            self.records
                .lock()
                .unwrap()
                .iter()
                .filter(|candidate| *candidate == key)
                .count(),
            expected
        );
        assert_eq!(
            self.events
                .lock()
                .unwrap()
                .iter()
                .filter(|candidate| *candidate == key)
                .count(),
            expected
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
    assert!(coordinator.handoffs.lock().unwrap().is_empty());
    assert_eq!(
        coordinator
            .reentrant_mutation_rejections
            .load(Ordering::SeqCst),
        1
    );
}

#[test]
fn early_success_does_not_suppress_a_later_legitimate_same_url_navigation() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 71, "https://same.example/path");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert("panel-a".into(), 71);
    coordinator.begin(&key);
    assert_eq!(
        coordinator.callback(&key),
        FakeCallbackOutcome::DeferredToCommand
    );
    coordinator
        .complete(&key, FakeNavigateOutcome::Success, || {})
        .unwrap();

    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    coordinator.assert_publication_count(&key, 2);
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
    assert_eq!(
        coordinator
            .reservations
            .lock()
            .unwrap()
            .get("panel-a")
            .copied(),
        Some(FakeReservationOwner::Handoff(9))
    );
    assert!(coordinator.try_detach_or_replace(&key).is_err());
    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    coordinator.assert_exactly_once(&key);
    assert!(coordinator.reservations.lock().unwrap().is_empty());
}

#[test]
fn late_success_consumes_handoff_then_allows_same_url_user_navigation() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 91, "https://same-late.example/path");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert("panel-a".into(), 91);
    coordinator.begin(&key);
    coordinator
        .complete(&key, FakeNavigateOutcome::Success, || {})
        .unwrap();

    assert!(coordinator.try_detach_or_replace(&key).is_err());
    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    assert!(coordinator.handoffs.lock().unwrap().is_empty());
    assert!(coordinator.reservations.lock().unwrap().is_empty());
    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    coordinator.assert_publication_count(&key, 2);
}

#[test]
fn callback_waiting_at_completion_adopts_transferred_handoff_reservation() {
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
fn stale_runtime_and_url_mismatch_cannot_claim_the_handoff() {
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
        assert!(coordinator.records.lock().unwrap().is_empty());
        assert!(coordinator.events.lock().unwrap().is_empty());
        assert!(coordinator.reservations.lock().unwrap().is_empty());
        assert!(coordinator.handoffs.lock().unwrap().is_empty());

        assert_eq!(
            coordinator.callback(&key),
            FakeCallbackOutcome::Published,
            "without a suppression tombstone, a later native callback is a real navigation"
        );
        coordinator.assert_exactly_once(&key);
    }
}

#[test]
fn error_without_native_callback_cannot_suppress_later_same_url_user_navigation() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 131, "https://retry.example/path");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert("panel-a".into(), 131);
    coordinator.begin(&key);
    assert!(coordinator
        .complete(&key, FakeNavigateOutcome::Error, || {})
        .is_err());
    assert!(coordinator.handoffs.lock().unwrap().is_empty());
    assert!(coordinator.reservations.lock().unwrap().is_empty());

    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    coordinator.assert_exactly_once(&key);
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

#[test]
fn production_removes_completed_and_cancelled_handoffs_without_a_tombstone() {
    let browser = include_str!("../browser.rs");
    let phase = source_item(browser, "enum BrowserProgrammaticNavigationPhase");
    assert!(
        !phase.contains("SuppressCallback"),
        "a persistent suppression phase can swallow a later legitimate same-URL navigation"
    );

    let cancel = source_item(browser, "fn cancel_browser_programmatic_navigation(");
    assert!(
        cancel.contains(".remove("),
        "navigate error/cancel must remove its handoff immediately"
    );

    let callback = source_item(
        browser,
        "fn observe_browser_programmatic_navigation_callback",
    );
    assert!(
        !callback.contains("SuppressCallback"),
        "late callback consumption must not leave a suppression tombstone"
    );
    assert!(
        callback.contains(".remove(")
            || callback.contains("consume_browser_programmatic_navigation")
            || callback.contains("complete_browser_programmatic_navigation_callback"),
        "late callback adoption must consume the matching handoff"
    );
}

#[test]
fn production_early_command_emit_retains_exact_logical_reservation() {
    let browser = include_str!("../browser.rs");
    let complete = source_item(browser, "fn complete_browser_programmatic_navigation(");
    let early_start = complete
        .find("callback_observed: true")
        .expect("early-observed completion branch");
    let late_start = complete[early_start..]
        .find("callback_observed: false")
        .map(|offset| early_start + offset)
        .expect("late completion branch");
    let early = &complete[early_start..late_start];
    let emit = early.find("app.emit(").expect("early command event emit");
    let release = early.find("mutation_reservation.release()");
    assert!(
        release.is_none_or(|release| emit < release),
        "early command publication must emit while its exact logical reservation remains active"
    );
}

#[test]
fn production_late_delegation_keeps_detach_and_replacement_excluded() {
    let browser = include_str!("../browser.rs");
    let complete = source_item(browser, "fn complete_browser_programmatic_navigation(");
    let late_start = complete
        .find("callback_observed: false")
        .expect("late completion branch");
    let delegated_start = complete[late_start..]
        .find("BrowserProgrammaticNavigationPhase::CallbackResponsible")
        .map(|offset| late_start + offset)
        .expect("already-delegated completion branch");
    let late = &complete[late_start..delegated_start];
    let detach = source_item(browser, "pub(crate) fn detach_browser_panels_for_control(");
    let retains_registry_reservation = !late.contains("reserved_panel_ids.remove(");
    let detach_honors_handoff_reservation = detach.contains("CallbackResponsible")
        || detach.contains("programmatic_navigation_is_reserved")
        || detach.contains("navigation_handoff_reservation");
    assert!(
        retains_registry_reservation || detach_honors_handoff_reservation,
        "late delegation must transfer, not drop, logical reservation ownership"
    );

    let callback = source_item(
        browser,
        "fn observe_browser_programmatic_navigation_callback",
    );
    let callback_responsible = callback
        .find("CallbackResponsible")
        .map(|start| &callback[start..])
        .expect("callback-responsible adoption branch");
    assert!(
        callback_responsible.contains("adopt")
            || callback_responsible.contains("transfer")
            || !callback_responsible.contains("reserve_browser_navigation_callback("),
        "late callback must atomically adopt the existing handoff reservation"
    );
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
