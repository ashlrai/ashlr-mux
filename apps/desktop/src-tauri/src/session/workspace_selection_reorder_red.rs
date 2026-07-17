//! Durable workspace selection and reorder publication policies.

use super::*;
use cmux_core::session::SessionWorkspaceGroupSnapshot;
use std::sync::mpsc;
use std::time::Duration;

const W1: &str = "11111111-1111-4111-8111-111111111111";
const W2: &str = "22222222-2222-4222-8222-222222222222";
const W3: &str = "33333333-3333-4333-8333-333333333333";
const G1: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionError {
    WindowMissing,
}

struct RecordingPublication {
    calls: Vec<&'static str>,
    persist_error: Option<String>,
    baseline: AppSessionSnapshot,
    events: Vec<AppSessionSnapshot>,
}

impl RecordingPublication {
    fn new(initial: &AppSessionSnapshot) -> Self {
        Self {
            calls: Vec::new(),
            persist_error: None,
            baseline: initial.clone(),
            events: Vec::new(),
        }
    }
}

impl SnapshotPublicationOperations for RecordingPublication {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("persist");
        match self.persist_error.take() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls.push("baseline");
        self.baseline = candidate.clone();
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls.push("emit");
        self.events.push(candidate.clone());
        Ok(())
    }
}

fn workspaces() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("a");
    apply_new_workspace(&mut snapshot, "b", None, None, None, None);
    apply_new_workspace(&mut snapshot, "c", None, None, None, None);
    let window = &mut snapshot.windows[0];
    for (workspace, id) in window.tab_manager.workspaces.iter_mut().zip([W1, W2, W3]) {
        workspace.workspace_id = Some(id.into());
    }
    window.tab_manager.selected_workspace_index = Some(0);
    window.selected_workspace_id = Some(W1.into());
    snapshot
}

fn ids(snapshot: &AppSessionSnapshot) -> Vec<&str> {
    snapshot.windows[0]
        .tab_manager
        .workspaces
        .iter()
        .map(|workspace| workspace.workspace_id.as_deref().unwrap())
        .collect()
}

fn assert_publication(publication: &RecordingPublication, committed: &AppSessionSnapshot) {
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(&publication.baseline, committed);
    assert_eq!(publication.events, [committed.clone()]);
}

fn assert_persist_only(
    publication: &RecordingPublication,
    authority: &GatedSnapshot,
    before: &AppSessionSnapshot,
) {
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), *before);
    assert_eq!(publication.baseline, *before);
    assert!(publication.events.is_empty());
}

fn select_in_window_candidate(
    snapshot: &mut AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) -> Result<((), bool), SelectionError> {
    let window = snapshot
        .windows
        .get_mut(window_index)
        .ok_or(SelectionError::WindowMissing)?;
    if workspace_index >= window.tab_manager.workspaces.len() {
        return Ok(((), false));
    }
    let changed = session_ops::select_workspace(&mut window.tab_manager, workspace_index as i64);
    if changed {
        sync_window_selected_workspace_id(window);
    }
    Ok(((), true))
}

fn reorder_many_candidate(
    snapshot: &mut AppSessionSnapshot,
    ordered: &[Uuid],
) -> Result<(Vec<WorkspaceReorderPlanItem>, bool), ReorderWorkspacesManyControlError> {
    let window = snapshot
        .windows
        .first_mut()
        .ok_or(ReorderWorkspacesManyControlError::Unavailable)?;
    let plan = session_ops::reorder_workspaces_many(&mut window.tab_manager, ordered, false)
        .map_err(ReorderWorkspacesManyControlError::Batch)?;
    let changed = plan.iter().any(|item| item.from_index != item.to_index);
    Ok((plan, changed))
}

#[test]
fn first_window_selection_always_publishes_same_invalid_and_missing_window_results() {
    for (before, index) in [
        (workspaces(), 0),
        (workspaces(), 99),
        (
            {
                let mut snapshot = workspaces();
                snapshot.windows.clear();
                snapshot
            },
            0,
        ),
    ] {
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let committed = transact_snapshot_always(&authority, &mut publication, |candidate| {
            apply_select_workspace(candidate, index)
        })
        .unwrap();
        assert_eq!(committed, before);
        assert_publication(&publication, &committed);
    }
}

#[test]
fn window_scoped_selection_distinguishes_missing_invalid_same_and_changed() {
    let mut missing = workspaces();
    missing.windows.clear();
    let authority = GatedSnapshot::new(missing.clone());
    let mut publication = RecordingPublication::new(&missing);
    assert_eq!(
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            select_in_window_candidate(candidate, 0, 0)
        })
        .unwrap_err(),
        PaneTopologyControlError::Operation(SelectionError::WindowMissing)
    );
    assert_eq!(*authority.lock().unwrap(), missing);
    assert!(publication.calls.is_empty());

    let before = workspaces();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (_, returned) =
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            select_in_window_candidate(candidate, 0, 99)
        })
        .unwrap();
    assert_eq!(returned, before);
    assert!(publication.calls.is_empty());

    for workspace_index in [0, 2] {
        let before = workspaces();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let (_, committed) =
            transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
                select_in_window_candidate(candidate, 0, workspace_index)
            })
            .unwrap();
        assert_eq!(
            committed.windows[0].tab_manager.selected_workspace_index,
            Some(workspace_index as i64)
        );
        assert_publication(&publication, &committed);
    }
}

#[test]
fn workspace_id_selection_returns_exact_changed_value_and_gates_same_or_missing() {
    let before = workspaces();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (changed, committed) =
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            let changed = apply_select_workspace_by_id(candidate, W3);
            Ok::<_, std::convert::Infallible>((changed, changed))
        })
        .unwrap();
    assert!(changed);
    assert_eq!(
        committed.windows[0].tab_manager.selected_workspace_index,
        Some(2)
    );
    assert_eq!(
        committed.windows[0].selected_workspace_id.as_deref(),
        Some(W3)
    );
    assert_publication(&publication, &committed);

    for workspace_id in [W1, "missing"] {
        let before = workspaces();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let (changed, returned) =
            transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
                let changed = apply_select_workspace_by_id(candidate, workspace_id);
                Ok::<_, std::convert::Infallible>((changed, changed))
            })
            .unwrap();
        assert!(!changed);
        assert_eq!(returned, before);
        assert!(publication.calls.is_empty());
    }
}

#[test]
fn single_reorder_publishes_changes_and_pin_or_group_clamps_are_exact_noops() {
    let before = workspaces();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let committed = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
        apply_reorder_workspaces(candidate, 2, 0, false)
    })
    .unwrap();
    assert_eq!(ids(&committed), [W3, W1, W2]);
    assert_publication(&publication, &committed);

    let mut pinned = workspaces();
    pinned.windows[0].tab_manager.workspaces[0].is_pinned = Some(true);
    let mut grouped = workspaces();
    grouped.windows[0].tab_manager.workspaces[0].group_id = Some(G1.into());
    grouped.windows[0].tab_manager.workspaces[1].group_id = Some(G1.into());
    grouped.windows[0].tab_manager.workspace_groups = Some(vec![SessionWorkspaceGroupSnapshot {
        id: G1.into(),
        name: "G".into(),
        anchor_workspace_id: Some(W1.into()),
        ..Default::default()
    }]);
    for (before, index, target) in [(pinned, 1, 0), (grouped, 1, 0)] {
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let returned = transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_reorder_workspaces(candidate, index, target, false)
        })
        .unwrap();
        assert_eq!(returned, before);
        assert!(publication.calls.is_empty());
    }
}

#[test]
fn batch_reorder_typed_errors_non_dry_change_and_noop_and_dry_simulation_are_atomic() {
    let duplicate = Uuid::parse_str(W1).unwrap();
    let missing_id = Uuid::parse_str("99999999-9999-4999-8999-999999999999").unwrap();
    for (before, ordered, expected) in [
        (
            {
                let mut snapshot = workspaces();
                snapshot.windows.clear();
                snapshot
            },
            vec![duplicate],
            "unavailable",
        ),
        (workspaces(), vec![duplicate, duplicate], "duplicate"),
        (workspaces(), vec![missing_id], "missing"),
    ] {
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let error = transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            reorder_many_candidate(candidate, &ordered)
        })
        .unwrap_err();
        match (expected, error) {
            (
                "unavailable",
                PaneTopologyControlError::Operation(ReorderWorkspacesManyControlError::Unavailable),
            )
            | (
                "duplicate",
                PaneTopologyControlError::Operation(ReorderWorkspacesManyControlError::Batch(
                    WorkspaceBatchReorderError::DuplicateWorkspace(_),
                )),
            )
            | (
                "missing",
                PaneTopologyControlError::Operation(ReorderWorkspacesManyControlError::Batch(
                    WorkspaceBatchReorderError::WorkspaceNotFound(_),
                )),
            ) => {}
            _ => panic!("unexpected {expected} error"),
        }
        assert_eq!(*authority.lock().unwrap(), before);
        assert!(publication.calls.is_empty());
    }

    let before = workspaces();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let ordered = [Uuid::parse_str(W3).unwrap()];
    let (plan, committed) =
        match transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            reorder_many_candidate(candidate, &ordered)
        }) {
            Ok(result) => result,
            Err(_) => panic!("changed batch reorder"),
        };
    assert!(plan.iter().any(|item| item.from_index != item.to_index));
    assert_eq!(ids(&committed), [W3, W1, W2]);
    assert_publication(&publication, &committed);

    let ordered = [
        Uuid::parse_str(W1).unwrap(),
        Uuid::parse_str(W2).unwrap(),
        Uuid::parse_str(W3).unwrap(),
    ];
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (_, returned) =
        match transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            reorder_many_candidate(candidate, &ordered)
        }) {
            Ok(result) => result,
            Err(_) => panic!("no-op batch reorder"),
        };
    assert_eq!(returned, before);
    assert!(publication.calls.is_empty());

    let authority = GatedSnapshot::new(before.clone());
    let mut simulated = authority.lock().unwrap().clone();
    let plan = session_ops::reorder_workspaces_many(
        &mut simulated.windows[0].tab_manager,
        &[Uuid::parse_str(W3).unwrap()],
        false,
    )
    .unwrap();
    assert!(plan.iter().any(|item| item.from_index != item.to_index));
    assert_eq!(ids(&simulated), [W3, W1, W2]);
    assert_eq!(*authority.lock().unwrap(), before);
}

#[test]
fn persistence_failures_leak_nothing_and_selection_reorder_writers_serialize() {
    let before = workspaces();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("selection persistence failure".into());
    assert_eq!(
        transact_snapshot_always(&authority, &mut publication, |candidate| {
            apply_select_workspace(candidate, 2)
        })
        .unwrap_err(),
        "selection persistence failure"
    );
    assert_persist_only(&publication, &authority, &before);

    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("window selection persistence failure".into());
    assert!(matches!(
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            select_in_window_candidate(candidate, 0, 2)
        }),
        Err(PaneTopologyControlError::Publication(error))
            if error == "window selection persistence failure"
    ));
    assert_persist_only(&publication, &authority, &before);

    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("id selection persistence failure".into());
    assert!(matches!(
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            let changed = apply_select_workspace_by_id(candidate, W3);
            Ok::<_, std::convert::Infallible>((changed, changed))
        }),
        Err(PaneTopologyControlError::Publication(error))
            if error == "id selection persistence failure"
    ));
    assert_persist_only(&publication, &authority, &before);

    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("single reorder persistence failure".into());
    assert_eq!(
        transact_snapshot_if_changed(&authority, &mut publication, |candidate| {
            apply_reorder_workspaces(candidate, 2, 0, false)
        })
        .unwrap_err(),
        "single reorder persistence failure"
    );
    assert_persist_only(&publication, &authority, &before);

    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("batch persistence failure".into());
    let ordered = [Uuid::parse_str(W3).unwrap()];
    assert!(matches!(
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            reorder_many_candidate(candidate, &ordered)
        }),
        Err(PaneTopologyControlError::Publication(error)) if error == "batch persistence failure"
    ));
    assert_persist_only(&publication, &authority, &before);

    let authority = Arc::new(GatedSnapshot::new(before));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (second_tx, second_rx) = mpsc::channel();
    let first_authority = Arc::clone(&authority);
    let first = std::thread::spawn(move || {
        let initial = first_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_snapshot_always(&first_authority, &mut publication, |candidate| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            apply_select_workspace(candidate, 2)
        })
        .unwrap();
    });
    entered_rx.recv().unwrap();
    let second_authority = Arc::clone(&authority);
    let second = std::thread::spawn(move || {
        let initial = second_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_snapshot_if_changed(&second_authority, &mut publication, |candidate| {
            second_tx.send(()).unwrap();
            apply_reorder_workspaces(candidate, 2, 0, false)
        })
        .unwrap();
    });
    assert!(second_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert_eq!(ids(&authority.lock().unwrap()), [W3, W1, W2]);
}
