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
    CallbackResponsible { lease_id: u64, expires_at: u64 },
}

const FAKE_HANDOFF_LEASE_TICKS: u64 = 3;

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
    now: AtomicUsize,
    next_lease_id: AtomicUsize,
}

struct FakeEarlyPublicationCleanup {
    coordinator: Arc<FakeProgrammaticNavigationCoordinator>,
    key: FakeNavigationKey,
}

impl Drop for FakeEarlyPublicationCleanup {
    fn drop(&mut self) {
        let mut handoffs = self.coordinator.handoffs.lock().unwrap();
        if matches!(
            handoffs.get(&self.key),
            Some(FakeHandoffState::NavigateInFlight { .. })
        ) {
            handoffs.remove(&self.key);
        }
        let mut reservations = self.coordinator.reservations.lock().unwrap();
        if reservations.get(&self.key.panel_id)
            == Some(&FakeReservationOwner::Mutation(self.key.runtime_id))
        {
            reservations.remove(&self.key.panel_id);
        }
    }
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
        let expected_owner = matches!(
            handoffs.get(key),
            Some(FakeHandoffState::CallbackResponsible { .. })
        )
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
                let lease_id = self.next_lease_id.fetch_add(1, Ordering::SeqCst) as u64 + 1;
                let expires_at = self.now.load(Ordering::SeqCst) as u64 + FAKE_HANDOFF_LEASE_TICKS;
                handoffs.insert(
                    key.clone(),
                    FakeHandoffState::CallbackResponsible {
                        lease_id,
                        expires_at,
                    },
                );
                while_handoff_locked();
                self.transfer_mutation_to_handoff_while_locked(key);
                self.leave_lock();
            }
            FakeHandoffState::CallbackResponsible { .. } => {
                self.leave_lock();
                return Err("programmatic navigation already delegated".to_string());
            }
        }
        Ok(())
    }

    fn delegated_lease(&self, key: &FakeNavigationKey) -> Option<(u64, u64)> {
        match self.handoffs.lock().unwrap().get(key).copied() {
            Some(FakeHandoffState::CallbackResponsible {
                lease_id,
                expires_at,
            }) => Some((lease_id, expires_at)),
            _ => None,
        }
    }

    fn set_now(&self, now: u64) {
        self.now.store(now as usize, Ordering::SeqCst);
    }

    fn expire_delegated_handoff(&self, key: &FakeNavigationKey, lease_id: u64) -> bool {
        let now = self.now.load(Ordering::SeqCst) as u64;
        let mut handoffs = self.handoffs.lock().unwrap();
        if !matches!(
            handoffs.get(key),
            Some(FakeHandoffState::CallbackResponsible {
                lease_id: current_lease_id,
                expires_at,
            }) if *current_lease_id == lease_id && *expires_at <= now
        ) {
            return false;
        }

        let mut reservations = self.reservations.lock().unwrap();
        if reservations.get(&key.panel_id) != Some(&FakeReservationOwner::Handoff(key.runtime_id)) {
            return false;
        }
        handoffs.remove(key);
        reservations.remove(&key.panel_id);
        true
    }

    fn panic_during_early_publication(self: &Arc<Self>, key: &FakeNavigationKey) -> ! {
        let _cleanup = FakeEarlyPublicationCleanup {
            coordinator: Arc::clone(self),
            key: key.clone(),
        };
        panic!("injected early publication panic");
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
fn queued_navigate_with_later_load_failure_or_no_callback_expires_exact_handoff() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 15, "https://missing-callback.example/path");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert(key.panel_id.clone(), key.runtime_id);
    coordinator.begin(&key);
    // Success acknowledges only that the UI-loop navigate message was queued. A later
    // load failure can still produce no exact native callback.
    coordinator
        .complete(&key, FakeNavigateOutcome::Success, || {})
        .unwrap();
    let (lease_id, expires_at) = coordinator
        .delegated_lease(&key)
        .expect("late success owns a bounded callback lease");

    coordinator.set_now(expires_at - 1);
    assert!(!coordinator.expire_delegated_handoff(&key, lease_id));
    assert!(coordinator.try_detach_or_replace(&key).is_err());

    coordinator.set_now(expires_at);
    assert!(coordinator.expire_delegated_handoff(&key, lease_id));
    assert!(coordinator.handoffs.lock().unwrap().is_empty());
    assert!(coordinator.reservations.lock().unwrap().is_empty());
    coordinator
        .try_detach_or_replace(&key)
        .expect("expiry releases detach and replacement");

    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert(key.panel_id.clone(), key.runtime_id);
    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    coordinator.assert_exactly_once(&key);
}

#[test]
fn mismatched_or_stale_callbacks_cannot_extend_a_delegated_lease() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 16, "https://lease.example/right");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert(key.panel_id.clone(), key.runtime_id);
    coordinator.begin(&key);
    coordinator
        .complete(&key, FakeNavigateOutcome::Success, || {})
        .unwrap();
    let lease = coordinator.delegated_lease(&key).unwrap();

    assert_eq!(
        coordinator.callback(&FakeNavigationKey::new(
            "panel-a",
            15,
            "https://lease.example/right"
        )),
        FakeCallbackOutcome::Ignored
    );
    assert_eq!(
        coordinator.callback(&FakeNavigationKey::new(
            "panel-a",
            16,
            "https://lease.example/wrong"
        )),
        FakeCallbackOutcome::Ignored
    );
    assert_eq!(coordinator.delegated_lease(&key), Some(lease));

    coordinator.set_now(lease.1);
    assert!(coordinator.expire_delegated_handoff(&key, lease.0));
    assert!(coordinator.handoffs.lock().unwrap().is_empty());
    assert!(coordinator.reservations.lock().unwrap().is_empty());
}

#[test]
fn stale_expiry_cannot_release_a_newer_runtime_or_handoff_generation() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let old = FakeNavigationKey::new("panel-a", 17, "https://generation.example/old");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert(old.panel_id.clone(), old.runtime_id);
    coordinator.begin(&old);
    coordinator
        .complete(&old, FakeNavigateOutcome::Success, || {})
        .unwrap();
    let old_lease = coordinator.delegated_lease(&old).unwrap();
    coordinator.set_now(old_lease.1);
    assert!(coordinator.expire_delegated_handoff(&old, old_lease.0));

    let replacement =
        FakeNavigationKey::new("panel-a", 18, "https://generation.example/replacement");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert(replacement.panel_id.clone(), replacement.runtime_id);
    coordinator.begin(&replacement);
    assert!(!coordinator.expire_delegated_handoff(&old, old_lease.0));
    assert_eq!(
        coordinator
            .reservations
            .lock()
            .unwrap()
            .get("panel-a")
            .copied(),
        Some(FakeReservationOwner::Mutation(replacement.runtime_id))
    );
    coordinator
        .complete(&replacement, FakeNavigateOutcome::Success, || {})
        .unwrap();
    let replacement_lease = coordinator.delegated_lease(&replacement).unwrap();
    assert_ne!(old_lease.0, replacement_lease.0);

    assert!(!coordinator.expire_delegated_handoff(&old, old_lease.0));
    assert!(!coordinator.expire_delegated_handoff(&replacement, old_lease.0));
    assert_eq!(
        coordinator.delegated_lease(&replacement),
        Some(replacement_lease)
    );
    assert_eq!(
        coordinator
            .reservations
            .lock()
            .unwrap()
            .get("panel-a")
            .copied(),
        Some(FakeReservationOwner::Handoff(replacement.runtime_id))
    );
}

#[test]
fn callback_and_expiry_race_has_one_publication_owner_and_no_emit_gap() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 19, "https://expiry-race.example");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert(key.panel_id.clone(), key.runtime_id);
    coordinator.begin(&key);
    coordinator
        .complete(&key, FakeNavigateOutcome::Success, || {})
        .unwrap();
    let (lease_id, expires_at) = coordinator.delegated_lease(&key).unwrap();
    coordinator.set_now(expires_at);

    let (start_tx, start_rx) = mpsc::channel();
    let callback_coordinator = Arc::clone(&coordinator);
    let callback_key = key.clone();
    let callback = thread::spawn(move || {
        start_rx.recv().unwrap();
        callback_coordinator.callback(&callback_key)
    });
    let expiry_coordinator = Arc::clone(&coordinator);
    let expiry_key = key.clone();
    let expiry = thread::spawn(move || {
        start_tx.send(()).unwrap();
        expiry_coordinator.expire_delegated_handoff(&expiry_key, lease_id)
    });

    let callback_outcome = callback.join().unwrap();
    let _expiry_won = expiry.join().unwrap();
    assert_eq!(callback_outcome, FakeCallbackOutcome::Published);
    coordinator.assert_exactly_once(&key);
    assert!(coordinator.handoffs.lock().unwrap().is_empty());
    assert!(coordinator.reservations.lock().unwrap().is_empty());
    assert_eq!(
        coordinator
            .reentrant_mutation_rejections
            .load(Ordering::SeqCst),
        1,
        "the winning callback must retain a reservation through emit"
    );
}

#[test]
fn exact_callback_consumes_lease_idempotently_without_a_tombstone() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 20, "https://lease-consume.example");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert(key.panel_id.clone(), key.runtime_id);
    coordinator.begin(&key);
    coordinator
        .complete(&key, FakeNavigateOutcome::Success, || {})
        .unwrap();
    let (lease_id, expires_at) = coordinator.delegated_lease(&key).unwrap();

    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    coordinator.set_now(expires_at);
    assert!(!coordinator.expire_delegated_handoff(&key, lease_id));
    assert!(!coordinator.expire_delegated_handoff(&key, lease_id));
    assert!(coordinator.handoffs.lock().unwrap().is_empty());
    assert!(coordinator.reservations.lock().unwrap().is_empty());

    assert_eq!(coordinator.callback(&key), FakeCallbackOutcome::Published);
    coordinator.assert_publication_count(&key, 2);
}

#[test]
fn panic_before_early_publication_cleanup_releases_handoff_and_reservation() {
    let coordinator = Arc::new(FakeProgrammaticNavigationCoordinator::default());
    let key = FakeNavigationKey::new("panel-a", 21, "https://publication-panic.example");
    coordinator
        .webviews
        .lock()
        .unwrap()
        .insert(key.panel_id.clone(), key.runtime_id);
    coordinator.begin(&key);
    assert_eq!(
        coordinator.callback(&key),
        FakeCallbackOutcome::DeferredToCommand
    );

    let unwind = std::panic::catch_unwind({
        let coordinator = Arc::clone(&coordinator);
        let key = key.clone();
        move || coordinator.panic_during_early_publication(&key)
    });
    assert!(unwind.is_err());
    assert!(coordinator.handoffs.lock().unwrap().is_empty());
    assert!(coordinator.reservations.lock().unwrap().is_empty());
    assert!(coordinator.records.lock().unwrap().is_empty());
    assert!(coordinator.events.lock().unwrap().is_empty());
}
