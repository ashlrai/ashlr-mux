//! Behavioral fault injection for the production Dock commit boundary.

use super::pane_surface_lifecycle::{
    commit_lifecycle_transition, dispatch_lifecycle_request, LifecycleDispatchContext,
    LifecycleEffect, LifecycleEffectExecutor, LifecycleTransition,
};
use super::*;
use crate::dock::{DockCreateRequest, DockStore, DockSurfaceKind};
use std::collections::BTreeMap;
use std::sync::{mpsc, Arc};
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeRecord {
    generation: u64,
    kind: String,
    owner_id: String,
}

#[derive(Clone, Debug)]
struct TeardownRecord {
    surface_id: String,
    runtime: RuntimeRecord,
    intent: crate::dock::DockRuntimeIntent,
}

#[derive(Default)]
struct ProductionDockFaultHarness {
    authoritative: AppSessionSnapshot,
    previous: Option<AppSessionSnapshot>,
    candidate: Option<AppSessionSnapshot>,
    runtimes: BTreeMap<String, RuntimeRecord>,
    journal: DockCommitJournal<(String, RuntimeRecord), TeardownRecord>,
    fail_dock_publish: bool,
    fail_persist: bool,
    teardown_log: Vec<String>,
    recreation_log: Vec<String>,
    operation_log: Vec<String>,
}

impl ProductionDockFaultHarness {
    fn new(authoritative: AppSessionSnapshot) -> Self {
        Self {
            authoritative,
            ..Self::default()
        }
    }

    fn seed_runtime(&mut self, surface_id: &str, generation: u64, kind: &str, owner_id: &str) {
        self.runtimes.insert(
            surface_id.to_string(),
            RuntimeRecord {
                generation,
                kind: kind.to_string(),
                owner_id: owner_id.to_string(),
            },
        );
    }

    fn persist_candidate(&mut self) -> Result<(), String> {
        self.operation_log.push("persist:candidate".into());
        if self.fail_persist {
            return Err("injected Dock persistence failure".into());
        }
        self.authoritative = self.candidate.clone().expect("candidate prepared");
        Ok(())
    }

    fn publish_dock(&mut self, surface_id: &str) -> Result<(), String> {
        self.operation_log.push(format!("publish:{surface_id}"));
        if self.fail_dock_publish {
            Err("injected Dock publication failure".into())
        } else {
            Ok(())
        }
    }
}

impl LifecycleEffectExecutor for ProductionDockFaultHarness {
    type Error = String;

    fn prepare_transition(&mut self, candidate: &AppSessionSnapshot) -> Result<(), Self::Error> {
        self.previous = Some(self.authoritative.clone());
        self.candidate = Some(candidate.clone());
        Ok(())
    }

    fn stage(&mut self, effect: &LifecycleEffect) -> Result<(), Self::Error> {
        match effect {
            LifecycleEffect::DockCreate {
                owner_id,
                dock_surface_id,
                generation,
                kind,
                ..
            } => {
                self.operation_log
                    .push(format!("stage_create:{dock_surface_id}"));
                self.journal.stage_claim((
                    dock_surface_id.clone(),
                    RuntimeRecord {
                        generation: *generation,
                        kind: kind.clone(),
                        owner_id: owner_id.clone(),
                    },
                ));
            }
            LifecycleEffect::RuntimeTeardown {
                surface_id,
                owner_id,
                dock_intent,
                ..
            } => {
                if let Some(runtime) = self.runtimes.remove(surface_id) {
                    assert_eq!(&runtime.owner_id, owner_id);
                    self.teardown_log.push(surface_id.clone());
                    self.operation_log.push(format!("teardown:{surface_id}"));
                    self.journal.stage_teardown(TeardownRecord {
                        surface_id: surface_id.clone(),
                        runtime,
                        intent: dock_intent.clone().expect("Dock teardown intent"),
                    });
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn commit_staged(&mut self) -> Result<(), Self::Error> {
        let mut journal = std::mem::take(&mut self.journal);
        let result = journal
            .commit_snapshot(|| self.persist_candidate())
            .and_then(|_| {
                journal.publish_claims(|(surface_id, runtime)| {
                    self.publish_dock(surface_id)?;
                    self.runtimes.insert(surface_id.clone(), runtime.clone());
                    Ok(())
                })
            });
        if result.is_ok() {
            journal.finish();
        }
        self.journal = journal;
        result
    }

    fn rollback_staged(&mut self) -> Result<(), Self::Error> {
        let previous = self.previous.take();
        let rollback = std::mem::take(&mut self.journal).rollback(|step| {
            match step {
                DockRollbackStep::RestoreSnapshot => {
                    self.operation_log.push("restore:previous".into());
                    self.authoritative = previous
                        .as_ref()
                        .expect("previous snapshot prepared")
                        .clone();
                }
                DockRollbackStep::RollbackClaim((surface_id, _)) => {
                    self.operation_log
                        .push(format!("rollback_claim:{surface_id}"));
                    self.runtimes.remove(&surface_id);
                    self.teardown_log.push(surface_id);
                }
                DockRollbackStep::RecreateTeardown(teardown) => {
                    let expected_kind = match teardown.intent {
                        crate::dock::DockRuntimeIntent::Terminal { .. } => "terminal",
                        crate::dock::DockRuntimeIntent::Browser { .. } => "browser",
                    };
                    assert_eq!(teardown.runtime.kind, expected_kind);
                    let surface_id = teardown.surface_id;
                    self.operation_log
                        .push(format!("recreate_teardown:{surface_id}"));
                    self.runtimes.insert(surface_id.clone(), teardown.runtime);
                    self.recreation_log.push(surface_id);
                }
            }
            Ok::<_, String>(())
        });
        self.candidate = None;
        rollback.map_err(|errors| errors.0.into_iter().next().unwrap())
    }

    fn rollback_committed(&mut self) -> Result<(), Self::Error> {
        self.rollback_staged()
    }
}

fn context() -> LifecycleDispatchContext {
    LifecycleDispatchContext::new(true, true, None)
}

fn owner(snapshot: &AppSessionSnapshot) -> String {
    snapshot.windows[0]
        .window_id
        .clone()
        .unwrap_or_else(|| "main".into())
}

fn transition(snapshot: &AppSessionSnapshot, method: &str, params: Value) -> LifecycleTransition {
    dispatch_lifecycle_request(snapshot, method, params.as_object().unwrap(), &context())
}

#[test]
fn stale_snapshot_fence_rejects_intervening_authority_without_clobbering_it() {
    let before = test_snapshot();
    let mut intervening = before.clone();
    intervening.windows[0].selected_workspace_id = Some("intervening-workspace".into());
    let preserved = intervening.clone();

    assert_eq!(
        crate::session::ensure_lifecycle_snapshot_current(&intervening, &before),
        Err("Stale lifecycle transition".into())
    );
    assert_eq!(intervening, preserved);
}

#[test]
fn session_control_mutation_gate_serializes_complete_transactions() {
    let state = Arc::new(SessionState::default());
    let first = state.lock_control_mutation().unwrap();
    assert_eq!(
        state.snapshot_for_lifecycle().unwrap().windows.len(),
        1,
        "a control request must be able to re-enter the gated snapshot on its thread"
    );
    let (ready_tx, ready_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let contender = Arc::clone(&state);
    let thread = std::thread::spawn(move || {
        ready_tx.send(()).unwrap();
        let _guard = contender.lock_control_mutation().unwrap();
        acquired_tx.send(()).unwrap();
    });

    ready_rx.recv().unwrap();
    assert!(acquired_rx.recv_timeout(Duration::from_millis(50)).is_err());
    drop(first);
    acquired_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    thread.join().unwrap();
}

#[test]
fn representative_ui_session_writers_share_the_control_mutation_gate() {
    use crate::session::TestUiWriterCategory;

    let state = Arc::new(SessionState::default());
    let control_transaction = state.lock_control_mutation().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let mut threads = Vec::new();
    for category in [
        TestUiWriterCategory::Structural,
        TestUiWriterCategory::Focus,
        TestUiWriterCategory::Metadata,
    ] {
        let state = Arc::clone(&state);
        let ready = ready_tx.clone();
        let completed = completed_tx.clone();
        threads.push(std::thread::spawn(move || {
            ready.send(category).unwrap();
            state.exercise_ui_writer_for_test(category).unwrap();
            completed.send(category).unwrap();
        }));
    }
    drop(ready_tx);
    drop(completed_tx);
    for _ in 0..3 {
        ready_rx.recv().unwrap();
    }

    let mut early = Vec::new();
    for _ in 0..3 {
        if let Ok(category) = completed_rx.recv_timeout(Duration::from_millis(50)) {
            early.push(category);
        }
    }
    drop(control_transaction);
    for thread in threads {
        thread.join().unwrap();
    }
    assert!(
        early.is_empty(),
        "UI session writers bypassed the control mutation gate: {early:?}"
    );
}

#[test]
fn lifecycle_commit_error_preserves_primary_failure_and_appends_rollback_failure() {
    struct PrimaryAndRollbackFailure;

    impl LifecycleEffectExecutor for PrimaryAndRollbackFailure {
        type Error = String;

        fn stage(&mut self, _effect: &LifecycleEffect) -> Result<(), Self::Error> {
            Ok(())
        }

        fn commit_staged(&mut self) -> Result<(), Self::Error> {
            Err("primary commit failure".into())
        }

        fn rollback_staged(&mut self) -> Result<(), Self::Error> {
            Err("rollback compensation failure".into())
        }
    }

    let before = test_snapshot();
    let transition = transition(
        &before,
        "surface.create",
        json!({"type":"terminal", "focus":false}),
    );
    let mut published = before.clone();
    let error =
        commit_lifecycle_transition(&mut published, transition, &mut PrimaryAndRollbackFailure)
            .unwrap_err();
    assert!(
        error.contains("primary commit failure") && error.contains("rollback compensation failure"),
        "transaction error lost one failure: {error}"
    );
    assert_eq!(published, before);
}

#[test]
fn stale_restore_keeps_intervening_authority_and_runtime_inventory_coherent() {
    // This interleaving is reachable in production: named_pipe::accept_loop
    // serves every accepted connection in its own tokio::spawn task. A second
    // client can therefore commit after this candidate is persisted and before
    // its staged browser publication reports failure.
    let before = test_snapshot();
    let owner_id = owner(&before);
    let created = transition(
        &before,
        "surface.create",
        json!({
            "placement":"dock",
            "window_id":owner_id,
            "type":"browser",
            "url":"https://candidate.example",
            "focus":false,
        }),
    );
    let candidate = created.snapshot.clone();
    let surface_id = created
        .effects
        .iter()
        .find_map(|effect| match effect {
            LifecycleEffect::DockCreate {
                dock_surface_id, ..
            } => Some(dock_surface_id.clone()),
            _ => None,
        })
        .expect("Dock create effect");

    let mut authoritative = before.clone();
    let mut runtimes = BTreeMap::from([(
        surface_id.clone(),
        RuntimeRecord {
            generation: 1,
            kind: "browser".into(),
            owner_id: owner_id.clone(),
        },
    )]);
    let mut journal = DockCommitJournal::<String, String>::default();
    journal.stage_claim(surface_id.clone());
    journal
        .commit_snapshot(|| {
            authoritative = candidate.clone();
            Ok::<_, &'static str>(())
        })
        .unwrap();

    let publish = journal.publish_claims(|_| {
        DockStore
            .create(
                &mut authoritative,
                &owner_id,
                DockCreateRequest {
                    kind: DockSurfaceKind::Terminal,
                    focus: false,
                    ..DockCreateRequest::default()
                },
            )
            .expect("intervening authoritative commit");
        Err::<(), _>("injected browser publication failure")
    });
    assert_eq!(publish, Err("injected browser publication failure"));
    assert_ne!(authoritative, candidate);

    let rollback = journal.rollback(|step| match step {
        DockRollbackStep::RestoreSnapshot => {
            crate::session::ensure_lifecycle_snapshot_current(&authoritative, &candidate)?;
            authoritative = before.clone();
            Ok::<(), String>(())
        }
        DockRollbackStep::RollbackClaim(claim) => {
            runtimes.remove(&claim);
            Ok(())
        }
        DockRollbackStep::RecreateTeardown(_) => unreachable!(),
    });
    assert!(
        format!("{rollback:?}").contains("Stale lifecycle transition"),
        "stale compensation must be observable: {rollback:?}"
    );

    let authoritative_has_surface =
        cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(&authoritative)
            .unwrap()
            .owner_of_surface(&surface_id)
            .is_some();
    let runtime_exists = runtimes.contains_key(&surface_id);
    assert_eq!(
        authoritative_has_surface, runtime_exists,
        "stale snapshot restoration must serialize or reconcile runtime rollback with the intervening authority"
    );
}

#[test]
fn rollback_reports_snapshot_restore_and_runtime_recreation_failures() {
    let mut restore = DockCommitJournal::<String, String>::default();
    restore
        .commit_snapshot(|| Ok::<_, &'static str>(()))
        .unwrap();
    let restore_outcome = restore.rollback(|step| match step {
        DockRollbackStep::RestoreSnapshot => Err::<(), _>("snapshot restore failed"),
        _ => Ok(()),
    });
    let restore_report = format!("{restore_outcome:?}");

    let mut recreate = DockCommitJournal::<String, String>::default();
    recreate.stage_teardown("closed-dock-runtime".into());
    let recreate_outcome = recreate.rollback(|step| match step {
        DockRollbackStep::RecreateTeardown(_) => Err::<(), _>("runtime recreation failed"),
        _ => Ok(()),
    });
    let recreate_report = format!("{recreate_outcome:?}");
    let mut violations = Vec::new();
    if !restore_report.contains("snapshot restore failed") {
        violations.push(format!(
            "snapshot compensation failure was silently discarded: {restore_report}"
        ));
    }
    if !recreate_report.contains("runtime recreation failed") {
        violations.push(format!(
            "runtime compensation failure was silently discarded: {recreate_report}"
        ));
    }
    assert!(violations.is_empty(), "{}", violations.join("; "));
}

#[test]
fn post_persist_dock_publish_failure_restores_authority_and_tears_down_staged_runtime() {
    let before = test_snapshot();
    let created = transition(
        &before,
        "surface.create",
        json!({
            "placement":"dock",
            "window_id":owner(&before),
            "type":"browser",
            "url":"https://example.com",
            "focus":false,
        }),
    );
    let surface_id = created
        .effects
        .iter()
        .find_map(|effect| match effect {
            LifecycleEffect::DockCreate {
                dock_surface_id, ..
            } => Some(dock_surface_id.clone()),
            _ => None,
        })
        .expect("Dock create effect");
    let mut published = before.clone();
    let mut production = ProductionDockFaultHarness::new(before.clone());
    production.fail_dock_publish = true;

    let result = commit_lifecycle_transition(&mut published, created, &mut production);
    let mut violations = Vec::new();
    if result != Err("injected Dock publication failure".to_string()) {
        violations.push(format!("publication failure was swallowed: {result:?}"));
    }
    if production.authoritative != before {
        violations.push("authoritative snapshot retained the failed Dock create".to_string());
    }
    if production.runtimes.contains_key(&surface_id) {
        violations.push("staged Dock runtime remained live after rollback".to_string());
    }
    if !production.teardown_log.contains(&surface_id) {
        violations.push("staged Dock runtime teardown was not observed".to_string());
    }
    if published != before {
        violations.push("caller-visible snapshot changed after failure".to_string());
    }
    assert_eq!(
        production.operation_log,
        vec![
            format!("stage_create:{surface_id}"),
            "persist:candidate".into(),
            format!("publish:{surface_id}"),
            "restore:previous".into(),
            format!("rollback_claim:{surface_id}"),
        ]
    );
    assert!(violations.is_empty(), "{}", violations.join("; "));
}

#[test]
fn dock_close_persist_failure_recreates_original_runtime_and_keeps_snapshot_unchanged() {
    let mut before = test_snapshot();
    let owner_id = owner(&before);
    let created = DockStore
        .create(
            &mut before,
            &owner_id,
            DockCreateRequest {
                kind: DockSurfaceKind::Terminal,
                focus: true,
                ..DockCreateRequest::default()
            },
        )
        .unwrap();
    let surface_id = created.surface_id.to_string();
    let closed = transition(&before, "surface.close", json!({"surface_id":surface_id}));
    let mut published = before.clone();
    let mut production = ProductionDockFaultHarness::new(before.clone());
    production.seed_runtime(&surface_id, created.generation, "terminal", &owner_id);
    production.fail_persist = true;

    let result = commit_lifecycle_transition(&mut published, closed, &mut production);
    let mut violations = Vec::new();
    if result != Err("injected Dock persistence failure".to_string()) {
        violations.push(format!(
            "persistence failure was not propagated: {result:?}"
        ));
    }
    if production.authoritative != before || published != before {
        violations.push("snapshot changed despite failed Dock close persistence".to_string());
    }
    if !production.runtimes.contains_key(&surface_id) {
        violations.push("original Dock runtime was not recreated after rollback".to_string());
    }
    if !production.recreation_log.contains(&surface_id) {
        violations.push("Dock runtime compensation was not observed".to_string());
    }
    assert_eq!(production.runtimes[&surface_id].owner_id, owner_id);
    assert_eq!(
        production.runtimes[&surface_id].generation,
        created.generation
    );
    assert_eq!(
        production.operation_log,
        vec![
            format!("teardown:{surface_id}"),
            "persist:candidate".into(),
            format!("recreate_teardown:{surface_id}"),
        ]
    );
    assert!(violations.is_empty(), "{}", violations.join("; "));
}
