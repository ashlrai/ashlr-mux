//! Fault injection at the direct Tauri Dock/session publication boundary.

use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeRecord {
    surface_id: String,
    generation: u64,
    kind: &'static str,
}

#[derive(Clone, Debug)]
struct TeardownRecord {
    runtime: RuntimeRecord,
    operation: DockRuntimeOperation,
}

#[derive(Default)]
struct HarnessRuntime {
    inventory: Rc<RefCell<BTreeMap<String, RuntimeRecord>>>,
    staged: Option<RuntimeRecord>,
    published: Option<RuntimeRecord>,
    torn_down: Option<TeardownRecord>,
}

impl DockRuntimeEffects for HarnessRuntime {
    fn stage_create(
        &mut self,
        _owner_id: &str,
        operation: &DockRuntimeOperation,
    ) -> Result<(), String> {
        self.staged = Some(runtime_record(operation));
        Ok(())
    }

    fn publish_staged(&mut self) -> Result<(), String> {
        let claim = self.staged.take().expect("runtime claim staged");
        assert!(
            self.inventory
                .borrow_mut()
                .insert(claim.surface_id.clone(), claim.clone())
                .is_none(),
            "runtime claim was published twice"
        );
        self.published = Some(claim);
        Ok(())
    }

    fn rollback_staged(&mut self) -> Result<(), String> {
        if let Some(claim) = self.staged.take() {
            self.inventory.borrow_mut().remove(&claim.surface_id);
        }
        Ok(())
    }

    fn teardown(&mut self, operation: &DockRuntimeOperation) -> Result<(), String> {
        let DockRuntimeOperation::Teardown { surface_id, .. } = operation else {
            return Err("expected teardown operation".into());
        };
        let runtime = self
            .inventory
            .borrow_mut()
            .remove(surface_id)
            .ok_or_else(|| "runtime missing during teardown".to_string())?;
        self.torn_down = Some(TeardownRecord {
            runtime,
            operation: operation.clone(),
        });
        Ok(())
    }
}

fn fixture() -> (AppSessionSnapshot, String) {
    let state = crate::session::SessionState::default();
    let mut snapshot = state.snapshot_for_lifecycle().unwrap();
    let owner_id = Uuid::new_v4().to_string();
    snapshot.windows[0].window_id = Some(owner_id.clone());
    (snapshot, owner_id)
}

fn terminal(title: &str) -> DockCreateRequest {
    DockCreateRequest {
        kind: DockSurfaceKind::Terminal,
        title: Some(title.into()),
        working_directory: Some("C:\\repo".into()),
        command: Some("cargo test".into()),
        focus: true,
        ..DockCreateRequest::default()
    }
}

fn create_candidate(
    before: &AppSessionSnapshot,
    owner_id: &str,
    title: &str,
) -> (AppSessionSnapshot, DockCreateResult, DockRuntimeOperation) {
    let mut candidate = before.clone();
    let mut operation = None;
    let created = DockStore
        .create_transactionally(&mut candidate, owner_id, terminal(title), |effect| {
            operation = Some(effect.clone());
            Ok::<_, String>(())
        })
        .unwrap();
    (candidate, created, operation.unwrap())
}

fn runtime_record(operation: &DockRuntimeOperation) -> RuntimeRecord {
    let DockRuntimeOperation::Create {
        surface_id,
        generation,
        intent,
    } = operation
    else {
        panic!("expected create operation")
    };
    RuntimeRecord {
        surface_id: surface_id.clone(),
        generation: *generation,
        kind: match intent {
            DockRuntimeIntent::Terminal { .. } => "terminal",
            DockRuntimeIntent::Browser { .. } => "browser",
        },
    }
}

#[test]
fn direct_create_persist_failure_rolls_back_published_claim_without_publishing_authority() {
    let (before, owner_id) = fixture();
    let before_dock = DockStore.snapshot(&before, &owner_id);
    let authoritative = before.clone();
    let events = Vec::<DockSnapshot>::new();
    let mut runtime = HarnessRuntime::default();
    let inventory = Rc::clone(&runtime.inventory);
    let mut journal = DirectDockCommitJournal::<RuntimeRecord, TeardownRecord>::default();

    let error = transact_direct_dock_lifecycle(
        &before,
        &mut journal,
        |candidate, journal| {
            let created = create_with_runtime(
                candidate,
                &DockStore,
                &owner_id,
                terminal("candidate"),
                &mut runtime,
            )?;
            journal.stage_claim(runtime.published.take().unwrap());
            Ok(created)
        },
        |_| Err("injected direct Dock persistence failure".to_string()),
        |step| match step {
            DirectDockRollbackStep::RollbackClaim(claim) => {
                assert_eq!(
                    inventory.borrow_mut().remove(&claim.surface_id),
                    Some(claim)
                );
                Ok(())
            }
            DirectDockRollbackStep::RecreateTeardown(_) => unreachable!(),
        },
    )
    .unwrap_err();

    assert_eq!(error, "injected direct Dock persistence failure");
    assert_eq!(authoritative, before);
    assert_eq!(DockStore.snapshot(&authoritative, &owner_id), before_dock);
    assert!(
        inventory.borrow().is_empty(),
        "published runtime claim leaked"
    );
    assert!(events.is_empty(), "failed candidate emitted Dock change");
}

#[test]
fn direct_close_persist_failure_recreates_the_exact_runtime_once() {
    let (before_empty, owner_id) = fixture();
    let (before, created, create) = create_candidate(&before_empty, &owner_id, "live");
    let runtime = runtime_record(&create);
    let before_dock = DockStore.snapshot(&before, &owner_id);
    let authoritative = before.clone();
    let mut runtime_effects = HarnessRuntime::default();
    runtime_effects
        .inventory
        .borrow_mut()
        .insert(runtime.surface_id.clone(), runtime.clone());
    let inventory = Rc::clone(&runtime_effects.inventory);
    let events = Vec::<DockSnapshot>::new();
    let recreations = Rc::new(RefCell::new(Vec::new()));
    let recreation_log = Rc::clone(&recreations);
    let mut journal = DirectDockCommitJournal::<RuntimeRecord, TeardownRecord>::default();

    transact_direct_dock_lifecycle(
        &before,
        &mut journal,
        |candidate, journal| {
            close_with_runtime(
                candidate,
                &DockStore,
                &owner_id,
                created.surface_id,
                &mut runtime_effects,
            )?;
            journal.stage_teardown(runtime_effects.torn_down.take().unwrap());
            Ok(())
        },
        |_| Err("injected direct Dock persistence failure".to_string()),
        |step| match step {
            DirectDockRollbackStep::RollbackClaim(_) => unreachable!(),
            DirectDockRollbackStep::RecreateTeardown(teardown) => {
                let DockRuntimeOperation::Teardown {
                    surface_id,
                    generation,
                    ..
                } = &teardown.operation
                else {
                    panic!("expected teardown operation")
                };
                assert_eq!(surface_id, &teardown.runtime.surface_id);
                assert_eq!(generation, teardown.runtime.generation);
                assert!(
                    inventory
                        .borrow_mut()
                        .insert(surface_id.clone(), teardown.runtime.clone())
                        .is_none(),
                    "runtime generation was duplicated"
                );
                recreation_log
                    .borrow_mut()
                    .push((surface_id.clone(), generation));
                Ok(())
            }
        },
    )
    .unwrap_err();

    assert_eq!(authoritative, before);
    assert_eq!(DockStore.snapshot(&authoritative, &owner_id), before_dock);
    assert_eq!(inventory.borrow().get(&runtime.surface_id), Some(&runtime));
    assert_eq!(
        *recreations.borrow(),
        vec![(runtime.surface_id.clone(), runtime.generation)]
    );
    assert!(events.is_empty(), "failed close emitted Dock change");

    journal
        .rollback(|_| Err::<(), _>("completed compensation ran twice".to_string()))
        .unwrap();
    assert_eq!(recreations.borrow().len(), 1);
}

#[test]
fn direct_persistence_and_compensation_failures_are_both_reported() {
    let (before, owner_id) = fixture();
    let mut runtime = HarnessRuntime::default();
    let mut journal = DirectDockCommitJournal::<RuntimeRecord, TeardownRecord>::default();

    let error = transact_direct_dock_lifecycle(
        &before,
        &mut journal,
        |candidate, journal| {
            create_with_runtime(
                candidate,
                &DockStore,
                &owner_id,
                terminal("failure aggregation"),
                &mut runtime,
            )?;
            journal.stage_claim(runtime.published.take().unwrap());
            Ok(())
        },
        |_| Err("primary persistence failure".to_string()),
        |_| Err("runtime rollback failure".to_string()),
    )
    .unwrap_err();

    assert!(error.contains("primary persistence failure"), "{error}");
    assert!(error.contains("runtime rollback failure"), "{error}");
}

#[test]
fn successful_direct_transaction_finalizes_journal_and_returns_exact_committed_snapshot() {
    let (before, owner_id) = fixture();
    let mut authoritative = before.clone();
    let mut runtime = HarnessRuntime::default();
    let mut events = Vec::new();
    let mut journal = DirectDockCommitJournal::<RuntimeRecord, TeardownRecord>::default();

    let (committed_create, committed) = transact_direct_dock_lifecycle(
        &before,
        &mut journal,
        |candidate, journal| {
            let created = create_with_runtime(
                candidate,
                &DockStore,
                &owner_id,
                terminal("committed"),
                &mut runtime,
            )?;
            journal.stage_claim(runtime.published.take().unwrap());
            Ok(created)
        },
        |candidate| {
            authoritative = candidate.clone();
            Ok(candidate.clone())
        },
        |_| Err::<(), _>("successful transaction rolled back".to_string()),
    )
    .unwrap();
    let committed_event = DockStore.snapshot(&committed, &owner_id);

    let (intervening, _, _) = create_candidate(&authoritative, &owner_id, "intervening");
    authoritative = intervening;
    let response = DockStore.snapshot(&committed, &owner_id);
    events.push(response.clone());

    assert_eq!(response, committed_event);
    assert_eq!(events, vec![committed_event]);
    assert_ne!(response, DockStore.snapshot(&authoritative, &owner_id));
    assert_eq!(
        response
            .surface(committed_create.surface_id)
            .unwrap()
            .generation,
        committed_create.generation
    );

    journal
        .rollback(|_| Err::<(), _>("finalized journal was not empty".to_string()))
        .unwrap();
}
