//! Typed change-gated publication for surface reorder and move.

use super::*;
use cmux_core::session::SessionPanelPinSnapshot;
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestOperationError {
    Invalid,
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

fn tabbed() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("a");
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = snapshot.windows[0].tab_manager.workspaces[0]
        .layout
        .as_mut()
        .unwrap()
    else {
        panic!("pane")
    };
    pane.panel_ids = vec!["a".into(), "b".into(), "c".into()];
    pane.selected_panel_id = Some("b".into());
    snapshot.windows[0].tab_manager.workspaces[0].panel_pins =
        Some(vec![SessionPanelPinSnapshot {
            panel_id: "a".into(),
            is_pinned: true,
        }]);
    snapshot
}

fn cross_pane() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("a");
    assert!(apply_split(
        &mut snapshot,
        "a",
        SessionSplitOrientation::Horizontal,
        "b",
        false,
    ));
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Split(split) = workspace.layout.as_mut().unwrap() else {
        panic!("split")
    };
    let SessionWorkspaceLayoutSnapshot::Pane(source) = split.first.as_mut() else {
        panic!("source")
    };
    source.pane_id = Some("pane-source".into());
    let SessionWorkspaceLayoutSnapshot::Pane(target) = split.second.as_mut() else {
        panic!("target")
    };
    target.pane_id = Some("pane-target".into());
    target.panel_ids.push("c".into());
    target.selected_panel_id = Some("b".into());
    workspace.panel_pins = Some(vec![
        SessionPanelPinSnapshot {
            panel_id: "a".into(),
            is_pinned: true,
        },
        SessionPanelPinSnapshot {
            panel_id: "b".into(),
            is_pinned: true,
        },
    ]);
    snapshot
}

fn cross_workspace() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("a");
    let source_workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Pane(source) = source_workspace.layout.as_mut().unwrap()
    else {
        panic!("source")
    };
    source.pane_id = Some("pane-source".into());
    source.panel_ids.push("b".into());
    assert!(session_ops::set_panel_title(source_workspace, "b", "build"));
    assert!(session_ops::set_panel_pinned(source_workspace, "b", true));
    assert!(session_ops::set_panel_unread(source_workspace, "b", true));
    source_workspace.panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
        panel_id: "b".into(),
        ports: vec![3000],
    }]);
    source_workspace.listening_ports = Some(vec![3000]);
    let mut destination = session_ops::fresh_terminal_workspace("c");
    destination.workspace_id = Some(Uuid::new_v4().to_string());
    let SessionWorkspaceLayoutSnapshot::Pane(target) = destination.layout.as_mut().unwrap() else {
        panic!("target")
    };
    target.pane_id = Some("pane-target".into());
    snapshot.windows[0].tab_manager.workspaces.push(destination);
    snapshot
}

fn assert_publication(publication: &RecordingPublication, committed: &AppSessionSnapshot) {
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(&publication.baseline, committed);
    assert_eq!(publication.events, [committed.clone()]);
}

#[test]
fn operation_error_and_false_result_are_exact_zero_operation_outcomes() {
    let before = tabbed();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let error = transact_result_if_changed_snapshot(&authority, &mut publication, |candidate| {
        session_ops::reorder_surface(
            &mut candidate.windows[0].tab_manager.workspaces[0],
            "missing",
            0,
            false,
        )
        .ok_or(TestOperationError::Invalid)
    })
    .unwrap_err();
    assert_eq!(
        error,
        PaneTopologyControlError::Operation(TestOperationError::Invalid)
    );
    assert_eq!(*authority.lock().unwrap(), before);
    assert!(publication.calls.is_empty());

    let mut publication = RecordingPublication::new(&before);
    let returned = transact_result_if_changed_snapshot(&authority, &mut publication, |candidate| {
        session_ops::reorder_surface(
            &mut candidate.windows[0].tab_manager.workspaces[0],
            "b",
            2,
            false,
        )
        .ok_or(TestOperationError::Invalid)
    })
    .unwrap();
    assert_eq!(returned, before);
    assert!(publication.calls.is_empty());
    assert!(publication.events.is_empty());
}

#[test]
fn reorder_success_preserves_pin_tier_order_and_focus_and_publishes_strictly() {
    let before = tabbed();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let committed =
        transact_result_if_changed_snapshot(&authority, &mut publication, |candidate| {
            let workspace = &mut candidate.windows[0].tab_manager.workspaces[0];
            session_ops::reorder_surface(workspace, "c", 0, false)
                .ok_or(TestOperationError::Invalid)?;
            let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_ref().unwrap()
            else {
                panic!("pane")
            };
            assert_eq!(pane.panel_ids, ["a", "c", "b"]);
            assert_eq!(pane.selected_panel_id.as_deref(), Some("b"));
            session_ops::reorder_surface(workspace, "c", 3, true).ok_or(TestOperationError::Invalid)
        })
        .unwrap();
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = committed.windows[0].tab_manager.workspaces[0]
        .layout
        .as_ref()
        .unwrap()
    else {
        panic!("pane")
    };
    assert_eq!(pane.panel_ids, ["a", "b", "c"]);
    assert_eq!(pane.selected_panel_id.as_deref(), Some("c"));
    assert_publication(&publication, &committed);
}

#[test]
fn move_success_matrix_collapses_cross_pane_and_preserves_cross_workspace_metadata_focus() {
    let before = cross_pane();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let committed =
        transact_result_if_changed_snapshot(&authority, &mut publication, |candidate| {
            session_ops::move_surface(
                &mut candidate.windows[0].tab_manager,
                0,
                "a",
                0,
                "pane-target",
                Some(99),
                false,
            )
            .ok_or(TestOperationError::Invalid)
        })
        .unwrap();
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = committed.windows[0].tab_manager.workspaces[0]
        .layout
        .as_ref()
        .unwrap()
    else {
        panic!("collapsed pane")
    };
    assert_eq!(pane.pane_id.as_deref(), Some("pane-target"));
    assert_eq!(pane.panel_ids, ["b", "a", "c"]);
    assert_eq!(pane.selected_panel_id.as_deref(), Some("b"));
    assert_publication(&publication, &committed);

    let before = cross_workspace();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let committed =
        transact_result_if_changed_snapshot(&authority, &mut publication, |candidate| {
            session_ops::move_surface(
                &mut candidate.windows[0].tab_manager,
                0,
                "b",
                1,
                "pane-target",
                None,
                true,
            )
            .ok_or(TestOperationError::Invalid)
        })
        .unwrap();
    let tabs = &committed.windows[0].tab_manager;
    assert_eq!(tabs.selected_workspace_index, Some(1));
    assert_eq!(tabs.workspaces[0].panel_titles, None);
    assert_eq!(tabs.workspaces[0].panel_pins, None);
    assert_eq!(tabs.workspaces[0].panel_unreads, None);
    assert_eq!(tabs.workspaces[0].panel_listening_ports, None);
    assert_eq!(tabs.workspaces[0].listening_ports, None);
    let destination = &tabs.workspaces[1];
    assert_eq!(
        destination.panel_titles.as_ref().unwrap()[0]
            .custom_title
            .as_deref(),
        Some("build")
    );
    assert!(destination.panel_pins.as_ref().unwrap()[0].is_pinned);
    assert!(destination.panel_unreads.as_ref().unwrap()[0].is_unread);
    assert_eq!(
        destination.panel_listening_ports.as_ref().unwrap()[0].ports,
        [3000]
    );
    assert_eq!(
        destination.listening_ports.as_deref(),
        Some([3000].as_slice())
    );
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = destination.layout.as_ref().unwrap() else {
        panic!("destination")
    };
    assert_eq!(pane.panel_ids, ["b", "c"]);
    assert_eq!(pane.selected_panel_id.as_deref(), Some("b"));
    assert_publication(&publication, &committed);
}

#[test]
fn invalid_move_and_same_pane_noop_have_zero_operations() {
    let before = cross_workspace();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let error = transact_result_if_changed_snapshot(&authority, &mut publication, |candidate| {
        session_ops::move_surface(
            &mut candidate.windows[0].tab_manager,
            0,
            "b",
            1,
            "missing",
            None,
            false,
        )
        .ok_or(TestOperationError::Invalid)
    })
    .unwrap_err();
    assert_eq!(
        error,
        PaneTopologyControlError::Operation(TestOperationError::Invalid)
    );
    assert_eq!(*authority.lock().unwrap(), before);
    assert!(publication.calls.is_empty());

    let before = tabbed();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let pane_id = match before.windows[0].tab_manager.workspaces[0]
        .layout
        .as_ref()
        .unwrap()
    {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.pane_id.clone().unwrap(),
        _ => unreachable!(),
    };
    let returned = transact_result_if_changed_snapshot(&authority, &mut publication, |candidate| {
        session_ops::move_surface(
            &mut candidate.windows[0].tab_manager,
            0,
            "b",
            0,
            &pane_id,
            Some(2),
            false,
        )
        .ok_or(TestOperationError::Invalid)
    })
    .unwrap();
    assert_eq!(returned, before);
    assert!(publication.calls.is_empty());
}

fn assert_persist_failure(
    before: AppSessionSnapshot,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<bool, TestOperationError>,
) {
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("injected surface mutation persistence failure".into());
    assert_eq!(
        transact_result_if_changed_snapshot(&authority, &mut publication, mutation).unwrap_err(),
        PaneTopologyControlError::Publication(
            "injected surface mutation persistence failure".into()
        )
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), before);
    assert_eq!(publication.baseline, before);
    assert!(publication.events.is_empty());
}

#[test]
fn reorder_and_move_persistence_failures_only_persist_and_leak_nothing() {
    assert_persist_failure(tabbed(), |candidate| {
        session_ops::reorder_surface(
            &mut candidate.windows[0].tab_manager.workspaces[0],
            "c",
            0,
            false,
        )
        .ok_or(TestOperationError::Invalid)
    });
    assert_persist_failure(cross_workspace(), |candidate| {
        session_ops::move_surface(
            &mut candidate.windows[0].tab_manager,
            0,
            "b",
            1,
            "pane-target",
            None,
            true,
        )
        .ok_or(TestOperationError::Invalid)
    });
}

#[test]
fn concurrent_result_writers_serialize_before_mutation() {
    let authority = Arc::new(GatedSnapshot::new(tabbed()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (second_tx, second_rx) = mpsc::channel();
    let first_authority = Arc::clone(&authority);
    let first = std::thread::spawn(move || {
        let initial = first_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_result_if_changed_snapshot(&first_authority, &mut publication, |candidate| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            candidate.windows[0].tab_manager.workspaces[0].process_title = "first".into();
            Ok::<_, TestOperationError>(true)
        })
        .unwrap();
    });
    entered_rx.recv().unwrap();
    let second_authority = Arc::clone(&authority);
    let second = std::thread::spawn(move || {
        let initial = second_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_result_if_changed_snapshot(&second_authority, &mut publication, |candidate| {
            second_tx.send(()).unwrap();
            candidate.windows[0].tab_manager.workspaces[0].process_title = "second".into();
            Ok::<_, TestOperationError>(true)
        })
        .unwrap();
    });
    assert!(second_rx.recv_timeout(Duration::from_millis(50)).is_err());
    release_tx.send(()).unwrap();
    first.join().unwrap();
    second.join().unwrap();
    assert_eq!(
        authority.lock().unwrap().windows[0].tab_manager.workspaces[0].process_title,
        "second"
    );
}
