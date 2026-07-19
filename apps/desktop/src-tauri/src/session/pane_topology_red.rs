//! Typed durable transaction contract for pane topology operations.

use super::*;
use std::sync::mpsc;
use std::time::Duration;

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

fn tabbed() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_new_terminal_tab(
        &mut snapshot,
        "surface-1",
        "surface-2",
        Some("cargo test"),
        Some("echo ready"),
        Some(BTreeMap::from([("RUST_LOG".into(), "debug".into())])),
    ));
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    assert!(session_ops::set_panel_title(
        workspace,
        "surface-2",
        "moved"
    ));
    assert!(session_ops::set_panel_pinned(workspace, "surface-2", true));
    snapshot
}

fn split() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    assert!(session_ops::set_panel_title(workspace, "surface-1", "left"));
    assert!(session_ops::set_panel_title(
        workspace,
        "surface-2",
        "right"
    ));
    assert!(session_ops::set_panel_unread(workspace, "surface-2", true));
    snapshot
}

fn pane_ids(layout: &SessionWorkspaceLayoutSnapshot, ids: &mut Vec<String>) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            ids.push(pane.pane_id.clone().expect("pane id"));
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            pane_ids(&split.first, ids);
            pane_ids(&split.second, ids);
        }
    }
}

fn assert_publication(publication: &RecordingPublication, committed: &AppSessionSnapshot) {
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(&publication.baseline, committed);
    assert_eq!(publication.events, [committed.clone()]);
}

#[test]
fn successful_topology_matrix_publishes_exact_results_metadata_focus_and_uuids() {
    let before = tabbed();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (_, split_committed) = transact_pane_topology_snapshot(
        &authority,
        &mut publication,
        |candidate| -> Result<_, session_ops::SplitOffSurfaceError> {
            let workspace = &mut candidate.windows[0].tab_manager.workspaces[0];
            session_ops::split_off_surface(
                workspace,
                "surface-2",
                SessionSplitOrientation::Vertical,
                false,
            )?;
            candidate.windows[0].tab_manager.selected_workspace_index = Some(0);
            sync_window_selected_workspace_id(&mut candidate.windows[0]);
            ensure_pane_ids(candidate);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(*authority.lock().unwrap(), split_committed);
    assert_publication(&publication, &split_committed);
    let mut ids = Vec::new();
    pane_ids(
        split_committed.windows[0].tab_manager.workspaces[0]
            .layout
            .as_ref()
            .unwrap(),
        &mut ids,
    );
    assert_eq!(ids.len(), 2);
    assert!(ids.iter().all(|id| Uuid::parse_str(id).is_ok()));
    assert_eq!(
        split_committed.windows[0].tab_manager.workspaces[0]
            .panel_titles
            .as_ref()
            .unwrap()
            .iter()
            .find(|entry| entry.panel_id == "surface-2")
            .unwrap()
            .custom_title
            .as_deref(),
        Some("moved")
    );

    let before = split();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (swap, swap_committed) = transact_pane_topology_snapshot(
        &authority,
        &mut publication,
        |candidate| -> Result<_, session_ops::PaneSwapError> {
            let workspace = &mut candidate.windows[0].tab_manager.workspaces[0];
            let pane_ids = {
                let mut ids = Vec::new();
                pane_ids(workspace.layout.as_ref().unwrap(), &mut ids);
                ids
            };
            let swap =
                session_ops::swap_selected_pane_surfaces(workspace, &pane_ids[0], &pane_ids[1])?;
            candidate.windows[0].tab_manager.selected_workspace_index = Some(0);
            sync_window_selected_workspace_id(&mut candidate.windows[0]);
            Ok(swap)
        },
    )
    .unwrap();
    assert_eq!(swap.source_surface_id, "surface-1");
    assert_eq!(swap.target_surface_id, "surface-2");
    assert_publication(&publication, &swap_committed);
    assert_eq!(
        swap_committed.windows[0]
            .tab_manager
            .selected_workspace_index,
        Some(0)
    );

    let before = split();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (broken, break_committed) = transact_pane_topology_snapshot(
        &authority,
        &mut publication,
        |candidate| -> Result<_, session_ops::PaneBreakError> {
            let result = session_ops::break_surface_to_new_workspace(
                &mut candidate.windows[0].tab_manager,
                0,
                "surface-2",
                true,
            )?;
            ensure_workspace_ids(candidate);
            ensure_pane_ids(candidate);
            Ok(result)
        },
    )
    .unwrap();
    assert_eq!(broken.surface_id, "surface-2");
    assert_eq!(
        break_committed.windows[0]
            .tab_manager
            .selected_workspace_index,
        Some(broken.workspace_index as i64)
    );
    let destination = &break_committed.windows[0].tab_manager.workspaces[broken.workspace_index];
    assert!(Uuid::parse_str(destination.workspace_id.as_deref().unwrap()).is_ok());
    let mut destination_panes = Vec::new();
    pane_ids(destination.layout.as_ref().unwrap(), &mut destination_panes);
    assert_eq!(destination_panes.len(), 1);
    assert!(Uuid::parse_str(&destination_panes[0]).is_ok());
    assert_eq!(
        destination.panel_titles.as_ref().unwrap()[0]
            .custom_title
            .as_deref(),
        Some("right")
    );
    assert!(destination.panel_unreads.as_ref().unwrap()[0].is_unread);
    assert_publication(&publication, &break_committed);
}

#[test]
fn break_surface_moves_persisted_owner_to_the_new_pane() {
    let mut before = split();
    let workspace = &mut before.windows[0].tab_manager.workspaces[0];
    let mut source_panes = Vec::new();
    pane_ids(workspace.layout.as_ref().unwrap(), &mut source_panes);
    workspace.surfaces.get_or_insert_with(Vec::new).push(
        serde_json::from_value(serde_json::json!({
            "surface_id": "surface-2",
            "pane_id": source_panes[1],
            "generation": 1,
            "kind": {"type": "terminal"},
            "metadata": {}
        }))
        .expect("surface record"),
    );
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);

    let (broken, committed) = transact_pane_topology_snapshot(
        &authority,
        &mut publication,
        |candidate| -> Result<_, session_ops::PaneBreakError> {
            let result = session_ops::break_surface_to_new_workspace(
                &mut candidate.windows[0].tab_manager,
                0,
                "surface-2",
                true,
            )?;
            ensure_workspace_ids(candidate);
            ensure_pane_ids(candidate);
            Ok(result)
        },
    )
    .unwrap();

    let workspaces = &committed.windows[0].tab_manager.workspaces;
    assert!(workspaces[0]
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .all(|surface| surface.surface_id != "surface-2"));
    let destination = &workspaces[broken.workspace_index];
    let mut destination_panes = Vec::new();
    pane_ids(destination.layout.as_ref().unwrap(), &mut destination_panes);
    let moved = destination
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|surface| surface.surface_id == "surface-2")
        .expect("moved surface record");
    assert_eq!(moved.pane_id, destination_panes[0]);
    cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(&committed)
        .expect("broken surface ownership remains valid");
}

fn assert_operation_error<E: Clone + std::fmt::Debug + PartialEq>(
    before: &AppSessionSnapshot,
    expected: E,
) {
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(before);
    let error = transact_pane_topology_snapshot(&authority, &mut publication, |_| {
        Err::<(), _>(expected.clone())
    })
    .unwrap_err();
    assert_eq!(error, PaneTopologyControlError::Operation(expected));
    assert_eq!(*authority.lock().unwrap(), *before);
    assert_eq!(publication.baseline, *before);
    assert!(publication.calls.is_empty());
    assert!(publication.events.is_empty());
}

#[test]
fn every_domain_operation_error_is_zero_operation_and_preserves_authority() {
    let before = split();
    for error in [
        session_ops::SplitOffSurfaceError::SurfaceNotFound,
        session_ops::SplitOffSurfaceError::WouldEmptySourcePane,
    ] {
        assert_operation_error(&before, error);
    }
    for error in [
        session_ops::PaneSwapError::SamePane,
        session_ops::PaneSwapError::SourcePaneNotFound,
        session_ops::PaneSwapError::TargetPaneNotFound,
        session_ops::PaneSwapError::BothPanesNeedSurface,
    ] {
        assert_operation_error(&before, error);
    }
    for error in [
        session_ops::PaneBreakError::WorkspaceNotFound,
        session_ops::PaneBreakError::SurfaceNotFound,
        session_ops::PaneBreakError::DetachFailed,
    ] {
        assert_operation_error(&before, error);
    }
}

#[test]
fn persistence_failure_matrix_only_persists_and_leaks_no_candidate_uuid() {
    assert_persist_failure(tabbed(), |candidate| {
        session_ops::split_off_surface(
            &mut candidate.windows[0].tab_manager.workspaces[0],
            "surface-2",
            SessionSplitOrientation::Horizontal,
            false,
        )?;
        ensure_pane_ids(candidate);
        Ok::<_, session_ops::SplitOffSurfaceError>(())
    });
    assert_persist_failure(split(), |candidate| {
        let workspace = &mut candidate.windows[0].tab_manager.workspaces[0];
        let mut ids = Vec::new();
        pane_ids(workspace.layout.as_ref().unwrap(), &mut ids);
        session_ops::swap_selected_pane_surfaces(workspace, &ids[0], &ids[1])?;
        Ok::<_, session_ops::PaneSwapError>(())
    });
    assert_persist_failure(split(), |candidate| {
        session_ops::break_surface_to_new_workspace(
            &mut candidate.windows[0].tab_manager,
            0,
            "surface-2",
            true,
        )?;
        ensure_workspace_ids(candidate);
        ensure_pane_ids(candidate);
        Ok::<_, session_ops::PaneBreakError>(())
    });
}

fn assert_persist_failure<E>(
    before: AppSessionSnapshot,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<(), E>,
) where
    E: std::fmt::Debug + PartialEq,
{
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("injected topology persistence failure".into());
    let error =
        transact_pane_topology_snapshot(&authority, &mut publication, mutation).unwrap_err();
    assert_eq!(
        error,
        PaneTopologyControlError::Publication("injected topology persistence failure".into())
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), before);
    assert_eq!(publication.baseline, before);
    assert!(publication.events.is_empty());
}

#[test]
fn concurrent_topology_writers_are_serialized_by_the_transaction_gate() {
    let before = split();
    let authority = Arc::new(GatedSnapshot::new(before));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (second_mutated_tx, second_mutated_rx) = mpsc::channel();
    let first_authority = Arc::clone(&authority);
    let first = std::thread::spawn(move || {
        let initial = first_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_pane_topology_snapshot(
            &first_authority,
            &mut publication,
            |candidate| -> Result<(), session_ops::PaneBreakError> {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                candidate.windows[0].tab_manager.workspaces[0].process_title = "first".into();
                Ok(())
            },
        )
        .unwrap();
    });
    entered_rx.recv().unwrap();
    let second_authority = Arc::clone(&authority);
    let second = std::thread::spawn(move || {
        let initial = second_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_pane_topology_snapshot(
            &second_authority,
            &mut publication,
            |candidate| -> Result<(), session_ops::PaneBreakError> {
                second_mutated_tx.send(()).unwrap();
                candidate.windows[0].tab_manager.workspaces[0].process_title = "second".into();
                Ok(())
            },
        )
        .unwrap();
    });
    assert!(second_mutated_rx
        .recv_timeout(Duration::from_millis(50))
        .is_err());
    release_tx.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert_eq!(
        authority.lock().unwrap().windows[0].tab_manager.workspaces[0].process_title,
        "second"
    );
}
