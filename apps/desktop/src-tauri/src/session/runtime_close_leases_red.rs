//! Reversible terminal-runtime detachment foundation for durable close.

use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering as AtomicOrdering};

#[derive(Debug)]
struct FakeTerminalRuntime {
    generation: u64,
    kill_failures_remaining: usize,
    kills: Arc<AtomicUsize>,
    registry_lock_held: Arc<AtomicBool>,
}

impl FakeTerminalRuntime {
    fn kill(&mut self) -> Result<(), String> {
        assert!(
            !self.registry_lock_held.load(AtomicOrdering::SeqCst),
            "terminal kill must happen outside the registry lock"
        );
        if self.kill_failures_remaining > 0 {
            self.kill_failures_remaining -= 1;
            return Err(format!("kill generation {} failed", self.generation));
        }
        self.kills.fetch_add(1, AtomicOrdering::SeqCst);
        Ok(())
    }
}

#[derive(Debug)]
struct FakeTerminalPanelRuntimeLease {
    panels: BTreeSet<String>,
    sessions: BTreeMap<u32, (String, FakeTerminalRuntime)>,
}

#[derive(Debug)]
struct FakeRollbackCollision {
    message: String,
    lease: FakeTerminalPanelRuntimeLease,
}

#[derive(Debug)]
struct FakeFinalizeFailure {
    failures: Vec<String>,
    retry: FakeTerminalPanelRuntimeLease,
}

#[derive(Default)]
struct FakeTerminalRegistry {
    sessions: BTreeMap<u32, (String, FakeTerminalRuntime)>,
    reserved_session_ids: BTreeSet<u32>,
    reserved_panel_ids: BTreeSet<String>,
    registry_lock_held: Arc<AtomicBool>,
}

impl FakeTerminalRegistry {
    fn runtime(
        &self,
        generation: u64,
        kill_failures_remaining: usize,
        kills: &Arc<AtomicUsize>,
    ) -> FakeTerminalRuntime {
        FakeTerminalRuntime {
            generation,
            kill_failures_remaining,
            kills: Arc::clone(kills),
            registry_lock_held: Arc::clone(&self.registry_lock_held),
        }
    }

    fn open_or_reuse(
        &mut self,
        id: u32,
        panel_id: &str,
        runtime: FakeTerminalRuntime,
    ) -> Result<(), String> {
        if self.reserved_session_ids.contains(&id) {
            return Err(format!("terminal session {id} is reserved"));
        }
        if self.reserved_panel_ids.contains(panel_id) {
            return Err(format!("terminal panel {panel_id} is reserved"));
        }
        if self.sessions.contains_key(&id) {
            return Err(format!("terminal session {id} is already live"));
        }
        self.sessions.insert(id, (panel_id.into(), runtime));
        Ok(())
    }

    fn detach_panels(
        &mut self,
        panel_ids: impl IntoIterator<Item = String>,
    ) -> Result<FakeTerminalPanelRuntimeLease, String> {
        let panels = panel_ids.into_iter().collect::<BTreeSet<_>>();
        if let Some(panel_id) = panels
            .iter()
            .find(|panel_id| self.reserved_panel_ids.contains(*panel_id))
        {
            return Err(format!("terminal panel {panel_id} is already reserved"));
        }
        let session_ids = self
            .sessions
            .iter()
            .filter_map(|(id, (panel_id, _))| panels.contains(panel_id).then_some(*id))
            .collect::<BTreeSet<_>>();
        if let Some(id) = session_ids
            .iter()
            .find(|id| self.reserved_session_ids.contains(id))
        {
            return Err(format!("terminal session {id} is already reserved"));
        }

        self.registry_lock_held.store(true, AtomicOrdering::SeqCst);
        let mut sessions = BTreeMap::new();
        for id in &session_ids {
            sessions.insert(*id, self.sessions.remove(id).expect("prechecked session"));
        }
        self.reserved_panel_ids.extend(panels.iter().cloned());
        self.reserved_session_ids.extend(session_ids);
        self.registry_lock_held.store(false, AtomicOrdering::SeqCst);
        Ok(FakeTerminalPanelRuntimeLease { panels, sessions })
    }

    fn rollback(
        &mut self,
        lease: FakeTerminalPanelRuntimeLease,
    ) -> Result<(), FakeRollbackCollision> {
        let ownership_lost = lease
            .panels
            .iter()
            .any(|panel| !self.reserved_panel_ids.contains(panel));
        let collision = lease
            .sessions
            .keys()
            .any(|id| self.sessions.contains_key(id));
        if ownership_lost || collision {
            return Err(FakeRollbackCollision {
                message: "terminal rollback collision".into(),
                lease,
            });
        }

        self.registry_lock_held.store(true, AtomicOrdering::SeqCst);
        for (id, session) in lease.sessions {
            self.sessions.insert(id, session);
            self.reserved_session_ids.remove(&id);
        }
        for panel in lease.panels {
            self.reserved_panel_ids.remove(&panel);
        }
        self.registry_lock_held.store(false, AtomicOrdering::SeqCst);
        Ok(())
    }

    fn finalize(
        &mut self,
        lease: FakeTerminalPanelRuntimeLease,
    ) -> Result<(), FakeFinalizeFailure> {
        let mut failures = Vec::new();
        let mut retry_sessions = BTreeMap::new();
        for (id, (panel_id, mut runtime)) in lease.sessions {
            match runtime.kill() {
                Ok(()) => {
                    self.reserved_session_ids.remove(&id);
                }
                Err(message) => {
                    failures.push(message);
                    retry_sessions.insert(id, (panel_id, runtime));
                }
            }
        }
        if retry_sessions.is_empty() {
            for panel in lease.panels {
                self.reserved_panel_ids.remove(&panel);
            }
            Ok(())
        } else {
            Err(FakeFinalizeFailure {
                failures,
                retry: FakeTerminalPanelRuntimeLease {
                    panels: lease.panels,
                    sessions: retry_sessions,
                },
            })
        }
    }
}

fn fixture() -> (FakeTerminalRegistry, Arc<AtomicUsize>) {
    let kills = Arc::new(AtomicUsize::new(0));
    let mut registry = FakeTerminalRegistry::default();
    registry
        .sessions
        .insert(3, ("panel-b".into(), registry.runtime(30, 0, &kills)));
    registry
        .sessions
        .insert(1, ("panel-a".into(), registry.runtime(10, 0, &kills)));
    registry
        .sessions
        .insert(2, ("panel-a".into(), registry.runtime(20, 0, &kills)));
    (registry, kills)
}

#[test]
fn multi_panel_detach_is_atomic_deterministic_and_non_destructive() {
    let (mut registry, kills) = fixture();
    let lease = registry
        .detach_panels(["panel-b".into(), "panel-a".into(), "panel-a".into()])
        .unwrap();
    assert_eq!(
        lease.panels.into_iter().collect::<Vec<_>>(),
        ["panel-a", "panel-b"]
    );
    assert_eq!(
        lease.sessions.keys().copied().collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(registry.sessions.is_empty());
    assert_eq!(registry.reserved_session_ids, BTreeSet::from([1, 2, 3]));
    assert_eq!(kills.load(AtomicOrdering::SeqCst), 0);

    let (mut registry, kills) = fixture();
    registry.reserved_panel_ids.insert("panel-b".into());
    let before_ids = registry.sessions.keys().copied().collect::<Vec<_>>();
    assert!(registry
        .detach_panels(["panel-a".into(), "panel-b".into()])
        .is_err());
    assert_eq!(
        registry.sessions.keys().copied().collect::<Vec<_>>(),
        before_ids
    );
    assert!(!registry.reserved_panel_ids.contains("panel-a"));
    assert_eq!(kills.load(AtomicOrdering::SeqCst), 0);
}

#[test]
fn reserved_terminal_ids_and_panels_block_open_or_reuse_until_rollback() {
    let (mut registry, kills) = fixture();
    let lease = registry.detach_panels(["panel-a".into()]).unwrap();
    assert!(registry
        .open_or_reuse(1, "replacement", registry.runtime(99, 0, &kills))
        .is_err());
    assert!(registry
        .open_or_reuse(99, "panel-a", registry.runtime(99, 0, &kills))
        .is_err());
    registry.rollback(lease).unwrap();
    assert_eq!(registry.sessions[&1].1.generation, 10);
    assert!(registry.reserved_panel_ids.is_empty());
    assert!(registry.reserved_session_ids.is_empty());
}

#[test]
fn rollback_prechecks_every_identity_and_returns_the_complete_lease_on_collision() {
    let (mut registry, kills) = fixture();
    let lease = registry.detach_panels(["panel-a".into()]).unwrap();
    // Simulate an out-of-band stale replacement that bypassed the public
    // reservation check. Rollback must perform no partial reinsertion.
    registry
        .sessions
        .insert(2, ("panel-a".into(), registry.runtime(200, 0, &kills)));
    let collision = registry.rollback(lease).unwrap_err();
    assert_eq!(collision.message, "terminal rollback collision");
    assert_eq!(
        collision.lease.sessions.keys().copied().collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(!registry.sessions.contains_key(&1));
    assert_eq!(registry.sessions[&2].1.generation, 200);
    assert!(registry.reserved_session_ids.contains(&1));
    assert!(registry.reserved_session_ids.contains(&2));

    registry.sessions.remove(&2);
    registry.rollback(collision.lease).unwrap();
    assert_eq!(registry.sessions[&1].1.generation, 10);
    assert_eq!(registry.sessions[&2].1.generation, 20);
}

#[test]
fn finalize_kills_outside_lock_and_retains_failed_sessions_for_retry() {
    let kills = Arc::new(AtomicUsize::new(0));
    let mut registry = FakeTerminalRegistry::default();
    registry
        .sessions
        .insert(1, ("panel-a".into(), registry.runtime(10, 0, &kills)));
    registry
        .sessions
        .insert(2, ("panel-a".into(), registry.runtime(20, 1, &kills)));
    let lease = registry.detach_panels(["panel-a".into()]).unwrap();
    let mut finalize_queue = vec![lease];
    assert_eq!(kills.load(AtomicOrdering::SeqCst), 0);
    let failed = registry
        .finalize(finalize_queue.pop().unwrap())
        .unwrap_err();
    assert_eq!(failed.failures, ["kill generation 20 failed"]);
    assert_eq!(kills.load(AtomicOrdering::SeqCst), 1);
    assert!(!registry.reserved_session_ids.contains(&1));
    assert!(registry.reserved_session_ids.contains(&2));
    assert!(registry.reserved_panel_ids.contains("panel-a"));
    assert_eq!(
        failed.retry.sessions.keys().copied().collect::<Vec<_>>(),
        [2]
    );

    registry.finalize(failed.retry).unwrap();
    assert_eq!(kills.load(AtomicOrdering::SeqCst), 2);
    assert!(registry.reserved_session_ids.is_empty());
    assert!(registry.reserved_panel_ids.is_empty());
}

fn function_source<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("missing {signature}"));
    let tail = &source[start..];
    let body_start = tail.find('{').unwrap();
    let mut depth = 0;
    for (offset, character) in tail[body_start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[..body_start + offset + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated {signature}")
}

#[test]
fn production_terminal_lease_api_has_fixed_reservation_rollback_and_finalize_wiring() {
    let terminal = include_str!("../terminal.rs");
    assert!(terminal.contains("struct TerminalRuntimeRegistry"));
    assert!(terminal.contains("struct TerminalPanelRuntimeLease"));
    assert!(terminal.contains("struct TerminalPanelRollbackError"));
    assert!(terminal.contains("struct TerminalPanelFinalizeError"));
    assert!(terminal.contains("reserved_session_ids"));
    assert!(terminal.contains("reserved_panel_ids"));

    let detach = function_source(terminal, "fn detach_terminal_panels_for_control(");
    assert!(detach.contains("BTreeSet"));
    assert!(detach.contains("reserved_session_ids"));
    assert!(detach.contains("reserved_panel_ids"));
    assert!(!detach.contains(".kill("));

    let open = function_source(terminal, "fn terminal_open_with_policy(");
    assert!(open.contains("reserved_session_ids") || open.contains("reserve_terminal"));
    assert!(open.contains("reserved_panel_ids") || open.contains("reserve_terminal"));
    assert!(open.find("reserved_panel_ids").unwrap_or(0) < open.find("ConPty::spawn").unwrap());

    let rollback = function_source(terminal, "fn rollback_terminal_panels_for_control(");
    assert!(rollback.contains("TerminalPanelRuntimeLease"));
    assert!(rollback.contains("TerminalPanelRollbackError"));
    assert!(rollback.contains("Result<"));
    assert!(rollback.contains("contains_key"));
    let precheck = rollback.find("contains_key").unwrap();
    let insert = rollback.find("insert(").unwrap();
    assert!(precheck < insert);

    let finalize = function_source(terminal, "fn finalize_terminal_panels_for_control(");
    assert!(finalize.contains("TerminalPanelRuntimeLease"));
    assert!(finalize.contains("TerminalPanelFinalizeError"));
    assert!(finalize.contains("retry"));
    assert!(finalize.contains("reserved_session_ids"));
    assert!(finalize.contains("reserved_panel_ids"));
    assert!(finalize.contains("drop(sessions)") || finalize.contains("drop(guard)"));
    assert!(finalize.find("drop(").unwrap() < finalize.find(".kill(").unwrap());

    // Foundation only: session close notifiers must not consume the lease yet.
    let session = include_str!("../session.rs");
    for signature in [
        "pub(crate) fn close_panel_for_control(",
        "pub(crate) fn close_workspace_in_window_for_control(",
    ] {
        let body = function_source(session, signature);
        assert!(!body.contains("TerminalPanelRuntimeLease"));
        assert!(!body.contains("detach_terminal_panels_for_control("));
    }
}
