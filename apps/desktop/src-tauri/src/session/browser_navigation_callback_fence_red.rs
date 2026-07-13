//! Linear exact-runtime fencing for browser navigation callbacks.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

#[derive(Default)]
struct FakeCallbackState {
    reserved: Mutex<BTreeSet<String>>,
    webviews: Mutex<BTreeMap<String, u64>>,
    records: Mutex<Vec<(String, u64, String)>>,
    events: Mutex<Vec<(String, u64, String)>>,
    lock_order: Mutex<Vec<&'static str>>,
    registry_locks_held: AtomicUsize,
    fail_record: AtomicBool,
    fail_emit: AtomicBool,
}

impl FakeCallbackState {
    fn enter_registry_lock(&self, name: &'static str) {
        self.registry_locks_held.fetch_add(1, Ordering::SeqCst);
        self.lock_order.lock().unwrap().push(name);
    }

    fn leave_registry_lock(&self) {
        self.registry_locks_held.fetch_sub(1, Ordering::SeqCst);
    }

    fn reserve_exact(
        self: &Arc<Self>,
        panel_id: &str,
        runtime_id: u64,
    ) -> Option<FakeCallbackReservation> {
        let mut reserved = self.reserved.lock().unwrap();
        self.enter_registry_lock("reservations");
        if reserved.contains(panel_id) {
            self.leave_registry_lock();
            return None;
        }
        let webviews = self.webviews.lock().unwrap();
        self.enter_registry_lock("webviews");
        let authoritative = webviews.get(panel_id).copied() == Some(runtime_id);
        if authoritative {
            reserved.insert(panel_id.to_string());
        }
        self.leave_registry_lock();
        drop(webviews);
        self.leave_registry_lock();
        drop(reserved);
        authoritative.then(|| FakeCallbackReservation {
            state: Arc::clone(self),
            panel_id: panel_id.to_string(),
        })
    }

    fn record(&self, panel_id: &str, runtime_id: u64, url: &str) -> Result<(), String> {
        if self.fail_record.load(Ordering::SeqCst) {
            return Err("record failed".to_string());
        }
        let mut records = self.records.lock().unwrap();
        self.enter_registry_lock("network_records");
        records.push((panel_id.to_string(), runtime_id, url.to_string()));
        self.leave_registry_lock();
        Ok(())
    }

    fn emit(&self, panel_id: &str, runtime_id: u64, url: &str) -> Result<(), String> {
        assert_eq!(
            self.registry_locks_held.load(Ordering::SeqCst),
            0,
            "emit must be reentrancy-safe and outside every registry mutex"
        );
        if self.fail_emit.load(Ordering::SeqCst) {
            return Err("emit failed".to_string());
        }
        self.events
            .lock()
            .unwrap()
            .push((panel_id.to_string(), runtime_id, url.to_string()));
        Ok(())
    }

    fn try_detach(&self, panel_id: &str) -> bool {
        let reserved = self.reserved.lock().unwrap();
        if reserved.contains(panel_id) {
            return false;
        }
        drop(reserved);
        self.webviews.lock().unwrap().remove(panel_id).is_some()
    }

    fn try_replace(&self, panel_id: &str, runtime_id: u64) -> bool {
        let reserved = self.reserved.lock().unwrap();
        if reserved.contains(panel_id) {
            return false;
        }
        drop(reserved);
        self.webviews
            .lock()
            .unwrap()
            .insert(panel_id.to_string(), runtime_id);
        true
    }

    fn is_reserved(&self, panel_id: &str) -> bool {
        self.reserved.lock().unwrap().contains(panel_id)
    }
}

struct FakeCallbackReservation {
    state: Arc<FakeCallbackState>,
    panel_id: String,
}

impl Drop for FakeCallbackReservation {
    fn drop(&mut self) {
        self.state.reserved.lock().unwrap().remove(&self.panel_id);
    }
}

fn run_callback(
    state: &Arc<FakeCallbackState>,
    panel_id: &str,
    runtime_id: u64,
    url: &str,
    before_emit: impl FnOnce(),
) -> Result<bool, String> {
    let Some(_reservation) = state.reserve_exact(panel_id, runtime_id) else {
        return Ok(false);
    };
    state.record(panel_id, runtime_id, url)?;
    before_emit();
    state.emit(panel_id, runtime_id, url)?;
    Ok(true)
}

#[test]
fn callback_reservation_excludes_detach_and_replacement_through_event_decision() {
    let state = Arc::new(FakeCallbackState::default());
    state.webviews.lock().unwrap().insert("panel-a".into(), 7);
    let (reserved_tx, reserved_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let callback_state = Arc::clone(&state);
    let callback = thread::spawn(move || {
        run_callback(
            &callback_state,
            "panel-a",
            7,
            "https://seven.example",
            || {
                reserved_tx.send(()).unwrap();
                resume_rx.recv().unwrap();
            },
        )
    });

    reserved_rx.recv().unwrap();
    assert!(state.is_reserved("panel-a"));
    assert!(!state.try_detach("panel-a"));
    assert!(!state.try_replace("panel-a", 8));
    resume_tx.send(()).unwrap();
    assert_eq!(callback.join().unwrap(), Ok(true));
    assert!(!state.is_reserved("panel-a"));
    assert_eq!(state.webviews.lock().unwrap()["panel-a"], 7);
    assert_eq!(state.records.lock().unwrap().len(), 1);
    assert_eq!(state.events.lock().unwrap().len(), 1);
}

#[test]
fn stale_or_previous_generation_callback_records_and_emits_nothing() {
    let state = Arc::new(FakeCallbackState::default());
    state.webviews.lock().unwrap().insert("panel-a".into(), 8);

    assert_eq!(
        run_callback(&state, "panel-a", 7, "https://stale.example", || {}),
        Ok(false)
    );
    assert!(state.records.lock().unwrap().is_empty());
    assert!(state.events.lock().unwrap().is_empty());
    assert!(!state.is_reserved("panel-a"));
}

#[test]
fn record_failure_emits_nothing_and_releases_the_guard() {
    let state = Arc::new(FakeCallbackState::default());
    state.webviews.lock().unwrap().insert("panel-a".into(), 9);
    state.fail_record.store(true, Ordering::SeqCst);

    assert_eq!(
        run_callback(&state, "panel-a", 9, "https://record.example", || {}),
        Err("record failed".to_string())
    );
    assert!(state.records.lock().unwrap().is_empty());
    assert!(state.events.lock().unwrap().is_empty());
    assert!(!state.is_reserved("panel-a"));
    assert!(state.try_replace("panel-a", 10));
}

#[test]
fn emit_failure_releases_guard_and_emit_holds_no_registry_mutex() {
    let state = Arc::new(FakeCallbackState::default());
    state.webviews.lock().unwrap().insert("panel-a".into(), 11);
    state.fail_emit.store(true, Ordering::SeqCst);

    assert_eq!(
        run_callback(&state, "panel-a", 11, "https://emit.example", || {}),
        Err("emit failed".to_string())
    );
    assert_eq!(state.records.lock().unwrap().len(), 1);
    assert!(state.events.lock().unwrap().is_empty());
    assert!(!state.is_reserved("panel-a"));
}

#[test]
fn exact_reservation_uses_fixed_order_and_emit_is_reentrant_safe() {
    let state = Arc::new(FakeCallbackState::default());
    state.webviews.lock().unwrap().insert("panel-a".into(), 12);
    let reentrant_state = Arc::clone(&state);

    assert_eq!(
        run_callback(
            &state,
            "panel-a",
            12,
            "https://reentrant.example",
            move || {
                assert_eq!(
                    reentrant_state.registry_locks_held.load(Ordering::SeqCst),
                    0
                );
                assert!(!reentrant_state.try_detach("panel-a"));
            },
        ),
        Ok(true)
    );
    assert_eq!(
        state.lock_order.lock().unwrap().as_slice(),
        ["reservations", "webviews", "network_records"]
    );
}

#[test]
fn production_callback_uses_a_linear_exact_runtime_reservation() {
    let browser = include_str!("../browser.rs");
    for required in [
        "BrowserNavigationCallbackReservation",
        "reserve_browser_navigation_callback",
        "record_observed_navigation_for_runtime",
    ] {
        assert!(
            browser.contains(required),
            "missing callback fence: {required}"
        );
    }

    let reservation = source_item(browser, "fn reserve_browser_navigation_callback(");
    let reserved = reservation.find("reserved_panel_ids").unwrap();
    let webviews = reservation[reserved..].find("webviews").unwrap() + reserved;
    assert!(reserved < webviews);
    assert!(reservation.contains("runtime_id"));
    assert!(reservation.contains("child.runtime_id"));
    assert!(reservation.contains("insert("));

    let guard = source_item(
        browser,
        "impl Drop for BrowserNavigationCallbackReservation",
    );
    assert!(guard.contains("reserved_panel_ids"));
    assert!(guard.contains("remove("));

    let record_helper = source_item(browser, "fn record_observed_navigation_for_runtime(");
    assert!(record_helper.contains("BrowserNavigationCallbackReservation"));
    assert!(record_helper.contains("runtime_id"));
    assert!(record_helper.contains("network_records"));
    assert!(!record_helper.contains(".emit("));

    let callback = callback_body(browser);
    assert!(!callback.contains("browser_child_is_authoritative"));
    let reserve = callback
        .find("reserve_browser_navigation_callback")
        .expect("callback reserves exact runtime");
    let record = callback[reserve..]
        .find("record_observed_navigation_for_runtime")
        .expect("callback records under reservation")
        + reserve;
    let emit = callback[record..]
        .find(".emit(")
        .expect("callback emits only after record succeeds")
        + record;
    assert!(reserve < record && record < emit);
    assert!(
        !callback.contains("let _ = record_observed_navigation_for_runtime")
            && (callback.contains("if let Err")
                || callback.contains(".is_err()")
                || callback.contains("match record_observed_navigation_for_runtime")),
        "record failure must prevent event emission"
    );
    assert!(!callback.contains("reserved_panel_ids.lock"));
    assert!(!callback.contains("webviews.lock"));
    assert!(!callback.contains("network_records.lock"));
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
