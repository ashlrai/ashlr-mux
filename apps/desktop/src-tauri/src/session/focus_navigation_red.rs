//! Typed value-bearing change gates for focus and navigation writers.

use super::*;
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
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.workspace_id = Some("workspace-a".into());
    workspace.focused_panel_id = Some("a".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        panic!("pane")
    };
    pane.pane_id = Some("pane-a".into());
    pane.panel_ids = vec!["a".into(), "b".into(), "c".into()];
    pane.selected_panel_id = Some("a".into());
    snapshot.windows[0].selected_workspace_id = Some("workspace-a".into());
    snapshot
}

fn split() -> AppSessionSnapshot {
    let mut snapshot = initial_snapshot("a");
    assert!(apply_split(
        &mut snapshot,
        "a",
        SessionSplitOrientation::Horizontal,
        "b",
        false,
    ));
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.workspace_id = Some("workspace-a".into());
    workspace.focused_panel_id = Some("a".into());
    let SessionWorkspaceLayoutSnapshot::Split(root) = workspace.layout.as_mut().unwrap() else {
        panic!("split")
    };
    let SessionWorkspaceLayoutSnapshot::Pane(first) = root.first.as_mut() else {
        panic!("first")
    };
    first.pane_id = Some("pane-a".into());
    let SessionWorkspaceLayoutSnapshot::Pane(second) = root.second.as_mut() else {
        panic!("second")
    };
    second.pane_id = Some("pane-b".into());
    snapshot.windows[0].selected_workspace_id = Some("workspace-a".into());
    snapshot
}

fn two_workspaces() -> AppSessionSnapshot {
    let mut snapshot = tabbed();
    let mut workspace = session_ops::fresh_terminal_workspace("d");
    workspace.workspace_id = Some("workspace-b".into());
    workspace.focused_panel_id = Some("d".into());
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        panic!("pane")
    };
    pane.pane_id = Some("pane-b".into());
    pane.panel_ids.push("e".into());
    snapshot.windows[0].tab_manager.workspaces.push(workspace);
    snapshot
}

fn assert_publication(publication: &RecordingPublication, committed: &AppSessionSnapshot) {
    assert_eq!(publication.calls, ["persist", "baseline", "emit"]);
    assert_eq!(&publication.baseline, committed);
    assert_eq!(publication.events, [committed.clone()]);
}

fn focus_pane_candidate(
    snapshot: &mut AppSessionSnapshot,
    pane_id: &str,
) -> Result<((), bool), PaneFocusControlError> {
    let before = snapshot.clone();
    apply_focus_pane(snapshot, 0, 0, pane_id)?;
    Ok(((), *snapshot != before))
}

fn focus_last_candidate(
    snapshot: &mut AppSessionSnapshot,
) -> Result<(session_ops::PaneLastResult, bool), PaneLastControlError> {
    let before = snapshot.clone();
    let workspace = snapshot
        .windows
        .get_mut(0)
        .and_then(|window| window.tab_manager.workspaces.get_mut(0))
        .ok_or(PaneLastControlError::WorkspaceNotFound)?;
    let focused_pane_id = workspace.focused_panel_id.as_deref().and_then(|panel_id| {
        session_ops::pane_id_containing_surface(workspace, panel_id).map(str::to_string)
    });
    let focused = session_ops::focus_alternate_pane(workspace, focused_pane_id.as_deref())
        .map_err(PaneLastControlError::Pane)?;
    workspace.focused_panel_id = focused.surface_id.clone();
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);
    sync_window_selected_workspace_id(&mut snapshot.windows[0]);
    Ok((focused, *snapshot != before))
}

#[test]
fn typed_value_gate_distinguishes_operation_noop_and_changed_publication() {
    let before = split();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let error = transact_value_if_changed_snapshot(&authority, &mut publication, |_| {
        Err::<((), bool), _>(TestOperationError::Invalid)
    })
    .unwrap_err();
    assert_eq!(
        error,
        PaneTopologyControlError::Operation(TestOperationError::Invalid)
    );
    assert_eq!(*authority.lock().unwrap(), before);
    assert!(publication.calls.is_empty());

    let mut publication = RecordingPublication::new(&before);
    let (value, returned) =
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            focus_pane_candidate(candidate, "pane-a")
        })
        .unwrap();
    assert_eq!(value, ());
    assert_eq!(returned, before);
    assert!(publication.calls.is_empty());

    let mut publication = RecordingPublication::new(&before);
    let (_, committed) =
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            focus_pane_candidate(candidate, "pane-b")
        })
        .unwrap();
    assert_eq!(
        committed.windows[0].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("b")
    );
    assert_publication(&publication, &committed);
}

#[test]
fn focus_pane_domain_errors_are_atomic_and_empty_panes_keep_legacy_success_semantics() {
    for pane_id in ["missing", "pane-a"] {
        let mut before = split();
        if pane_id == "pane-a" {
            before.windows.clear();
        }
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let error = transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            focus_pane_candidate(candidate, pane_id)
        })
        .unwrap_err();
        assert!(matches!(error, PaneTopologyControlError::Operation(_)));
        assert_eq!(*authority.lock().unwrap(), before);
        assert!(publication.calls.is_empty());
    }

    let mut snapshot = split();
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    let SessionWorkspaceLayoutSnapshot::Split(root) = workspace.layout.as_mut().unwrap() else {
        panic!("split")
    };
    let SessionWorkspaceLayoutSnapshot::Pane(empty) = root.second.as_mut() else {
        panic!("empty")
    };
    empty.panel_ids.clear();
    empty.selected_panel_id = None;
    assert_eq!(apply_focus_pane(&mut snapshot, 0, 0, "pane-b"), Ok(()));
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id,
        None
    );
    assert!(matches!(
        focus_last_candidate(&mut snapshot),
        Err(PaneLastControlError::Pane(
            session_ops::PaneLastError::NoFocusedPane
        ))
    ));
}

#[test]
fn pane_last_returns_value_and_changed_snapshot_or_atomic_domain_errors() {
    let before = split();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (focused, committed) =
        transact_value_if_changed_snapshot(&authority, &mut publication, focus_last_candidate)
            .unwrap();
    assert_eq!(focused.pane_id, "pane-b");
    assert_eq!(focused.surface_id.as_deref(), Some("b"));
    assert_eq!(
        committed.windows[0].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("b")
    );
    assert_publication(&publication, &committed);

    for mut before in [tabbed(), {
        let mut snapshot = split();
        snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id = Some("missing".into());
        snapshot
    }] {
        let expected = before.clone();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let error =
            transact_value_if_changed_snapshot(&authority, &mut publication, focus_last_candidate)
                .unwrap_err();
        assert!(matches!(
            error,
            PaneTopologyControlError::Operation(PaneLastControlError::Pane(_))
        ));
        assert_eq!(*authority.lock().unwrap(), expected);
        assert!(publication.calls.is_empty());
        before.windows.clear();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        assert!(matches!(
            transact_value_if_changed_snapshot(&authority, &mut publication, focus_last_candidate),
            Err(PaneTopologyControlError::Operation(
                PaneLastControlError::WorkspaceNotFound
            ))
        ));
        assert!(publication.calls.is_empty());
    }
}

#[test]
fn adjacent_wraps_and_singleton_or_missing_targets_are_exact_noops() {
    let mut before = tabbed();
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = before.windows[0].tab_manager.workspaces[0]
        .layout
        .as_mut()
        .unwrap()
    else {
        panic!("pane")
    };
    pane.selected_panel_id = Some("c".into());
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (_, committed) =
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            let changed = apply_select_adjacent_panel(candidate, "c", true);
            Ok::<_, TestOperationError>(((), changed))
        })
        .unwrap();
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = committed.windows[0].tab_manager.workspaces[0]
        .layout
        .as_ref()
        .unwrap()
    else {
        panic!("pane")
    };
    assert_eq!(pane.selected_panel_id.as_deref(), Some("a"));
    assert_publication(&publication, &committed);

    for (before, panel_id) in [(initial_snapshot("only"), "only"), (tabbed(), "missing")] {
        let expected = before.clone();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let (_, returned) =
            transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
                let changed = apply_select_adjacent_panel(candidate, panel_id, true);
                Ok::<_, TestOperationError>(((), changed))
            })
            .unwrap();
        assert_eq!(returned, expected);
        assert!(publication.calls.is_empty());
    }
}

#[test]
fn workspace_surface_returns_exact_changed_flag_for_changed_noop_and_missing_targets() {
    let before = two_workspaces();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    let (changed, committed) =
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            let changed = apply_select_workspace_surface(candidate, "workspace-b", "e");
            Ok::<_, TestOperationError>((changed, changed))
        })
        .unwrap();
    assert!(changed);
    let window = &committed.windows[0];
    assert_eq!(window.tab_manager.selected_workspace_index, Some(1));
    assert_eq!(window.selected_workspace_id.as_deref(), Some("workspace-b"));
    assert_eq!(
        window.tab_manager.workspaces[1].focused_panel_id.as_deref(),
        Some("e")
    );
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        window.tab_manager.workspaces[1].layout.as_ref().unwrap()
    else {
        panic!("pane")
    };
    assert_eq!(pane.selected_panel_id.as_deref(), Some("e"));
    assert_publication(&publication, &committed);

    for (workspace_id, panel_id) in [
        ("workspace-b", "e"),
        ("missing", "e"),
        ("workspace-b", "missing"),
    ] {
        let before = committed.clone();
        let authority = GatedSnapshot::new(before.clone());
        let mut publication = RecordingPublication::new(&before);
        let (changed, returned) =
            transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
                let changed = apply_select_workspace_surface(candidate, workspace_id, panel_id);
                Ok::<_, TestOperationError>((changed, changed))
            })
            .unwrap();
        assert!(!changed);
        assert_eq!(returned, before);
        assert!(publication.calls.is_empty());
    }
}

#[test]
fn persistence_failure_only_persists_and_concurrent_value_writers_serialize() {
    let before = two_workspaces();
    let authority = GatedSnapshot::new(before.clone());
    let mut publication = RecordingPublication::new(&before);
    publication.persist_error = Some("injected focus persistence failure".into());
    assert_eq!(
        transact_value_if_changed_snapshot(&authority, &mut publication, |candidate| {
            let changed = apply_select_workspace_surface(candidate, "workspace-b", "e");
            Ok::<_, TestOperationError>((changed, changed))
        })
        .unwrap_err(),
        PaneTopologyControlError::Publication("injected focus persistence failure".into())
    );
    assert_eq!(publication.calls, ["persist"]);
    assert_eq!(*authority.lock().unwrap(), before);
    assert_eq!(publication.baseline, before);
    assert!(publication.events.is_empty());

    let authority = Arc::new(GatedSnapshot::new(tabbed()));
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (second_tx, second_rx) = mpsc::channel();
    let first_authority = Arc::clone(&authority);
    let first = std::thread::spawn(move || {
        let initial = first_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_value_if_changed_snapshot(&first_authority, &mut publication, |candidate| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            candidate.windows[0].tab_manager.workspaces[0].process_title = "first".into();
            Ok::<_, TestOperationError>(((), true))
        })
        .unwrap();
    });
    entered_rx.recv().unwrap();
    let second_authority = Arc::clone(&authority);
    let second = std::thread::spawn(move || {
        let initial = second_authority.lock().unwrap().clone();
        let mut publication = RecordingPublication::new(&initial);
        transact_value_if_changed_snapshot(&second_authority, &mut publication, |candidate| {
            second_tx.send(()).unwrap();
            candidate.windows[0].tab_manager.workspaces[0].process_title = "second".into();
            Ok::<_, TestOperationError>(((), true))
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

fn function_source<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("missing {signature}"));
    let tail = &source[start..];
    let body_start = tail.find('{').unwrap();
    let mut depth = 0usize;
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
fn production_helpers_and_every_live_caller_use_results_without_focus_side_effect_bypasses() {
    let session = include_str!("../session.rs");
    for signature in [
        "pub(crate) fn focus_pane_for_control(",
        "pub(crate) fn focus_last_pane_for_control(",
        "pub(crate) fn select_adjacent_panel_for_control(",
        "pub(crate) fn select_workspace_surface(",
    ] {
        let body = function_source(session, signature);
        assert!(body.contains("PaneTopologyControlError<"), "{signature}");
        assert!(
            body.contains("state.transact_value_if_changed(app,"),
            "{signature}"
        );
        assert!(!body.contains("snapshot.lock()") && !body.contains("notify_session_changed("));
    }
    let public_adjacent = function_source(session, "pub fn session_select_adjacent_panel(");
    assert!(public_adjacent.contains("select_adjacent_panel_for_control("));
    assert!(!public_adjacent.contains("transact_snapshot_if_changed("));
    assert!(public_adjacent.contains('?'));
    for signature in [
        "pub fn session_select_workspace_surface(",
        "pub fn session_handle_navigation_uri(",
    ] {
        let body = function_source(session, signature);
        assert!(body.contains("select_workspace_surface("));
        assert!(
            body.contains("?"),
            "{signature} must propagate publication failure"
        );
        assert!(!body.contains(".unwrap(") && !body.contains(".expect("));
    }
    let notifications = include_str!("../notifications.rs");
    let activation = function_source(notifications, "fn route_notification_activation(");
    assert!(activation.contains("select_workspace_surface("));
    assert!(activation.contains("?"));
    assert!(!activation.contains(".unwrap(") && !activation.contains(".expect("));

    let socket = include_str!("../control_socket.rs");
    for signature in [
        "fn pane_focus(",
        "fn pane_last(",
        "fn surface_focus(",
        "fn surface_select_adjacent(",
        "fn browser_focus_webview(",
        "fn notification_open_selected(",
    ] {
        let body = function_source(socket, signature);
        assert!(
            body.contains("PaneTopologyControlError::Publication"),
            "{signature}"
        );
        assert!(body.contains("\"internal\""), "{signature}");
    }
    let tab_switch = function_source(socket, "fn browser_tab_switch(");
    assert!(tab_switch.contains("surface_focus(app, &focus_params)"));
    assert!(!tab_switch.contains("select_workspace_surface("));
    assert!(
        socket.contains("\"surface.next\" => surface_select_adjacent(app, &request.params, true)")
    );
    assert!(socket
        .contains("\"surface.previous\" => surface_select_adjacent(app, &request.params, false)"));
    assert!(socket.contains("\"pane.last\" => pane_last(app, &request.params)"));

    for signature in ["fn pane_focus(", "fn pane_last("] {
        let body = function_source(socket, signature);
        let publication = body.find("PaneTopologyControlError::Publication").unwrap();
        let os_focus = body.find("window.set_focus()").unwrap();
        assert!(
            os_focus > publication,
            "{signature} focuses before helper success"
        );
        assert!(
            !body.contains("if changed"),
            "{signature} skips valid no-op focus"
        );
    }
    let browser_focus = function_source(socket, "fn browser_focus_webview(");
    let publication = browser_focus
        .find("PaneTopologyControlError::Publication")
        .unwrap();
    let webview_focus = browser_focus.find("\"focus\"").unwrap();
    assert!(
        webview_focus > publication,
        "webview focus must follow committed/no-op success"
    );
    assert!(!browser_focus.contains("if changed"));
}
