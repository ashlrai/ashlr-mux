use super::*;

#[derive(Default)]
struct FakeRemoteWorkspaceRenameController {
    calls: Mutex<Vec<RemoteWorkspaceRenameRequest>>,
}

impl RemoteWorkspaceRenameController for FakeRemoteWorkspaceRenameController {
    fn rename(&self, request: &RemoteWorkspaceRenameRequest) -> Result<(), String> {
        self.calls.lock().unwrap().push(request.clone());
        Ok(())
    }
}
use cmux_core::session_ops::count_leaves;

fn active_layout(snapshot: &AppSessionSnapshot) -> &SessionWorkspaceLayoutSnapshot {
    snapshot.windows[0].tab_manager.workspaces[0]
        .layout
        .as_ref()
        .expect("layout present")
}

fn first_panel_id(workspace: &cmux_core::session::SessionWorkspaceSnapshot) -> Option<&str> {
    match workspace.layout.as_ref()? {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.panel_ids.first().map(String::as_str),
        SessionWorkspaceLayoutSnapshot::Split(_) => None,
    }
}

fn pane_ids_in_layout(layout: &SessionWorkspaceLayoutSnapshot) -> Vec<&str> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => pane.pane_id.as_deref().into_iter().collect(),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let mut ids = pane_ids_in_layout(&split.first);
            ids.extend(pane_ids_in_layout(&split.second));
            ids
        }
    }
}

fn split_ids_in_layout(layout: &SessionWorkspaceLayoutSnapshot) -> Vec<&str> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(_) => Vec::new(),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let mut ids = split.split_id.as_deref().into_iter().collect::<Vec<_>>();
            ids.extend(split_ids_in_layout(&split.first));
            ids.extend(split_ids_in_layout(&split.second));
            ids
        }
    }
}

#[test]
fn initial_snapshot_is_one_window_workspace_pane() {
    let snapshot = initial_snapshot(FIRST_PANEL_ID);
    assert_eq!(snapshot.version, SESSION_SNAPSHOT_SCHEMA_VERSION);
    assert_eq!(snapshot.windows.len(), 1);
    let tabs = &snapshot.windows[0].tab_manager;
    assert_eq!(tabs.selected_workspace_index, Some(0));
    assert_eq!(tabs.workspaces.len(), 1);
    assert_eq!(count_leaves(active_layout(&snapshot)), 1);
}

#[test]
fn snapshot_for_window_projects_requested_tab_manager_first() {
    let mut snapshot = initial_snapshot(FIRST_PANEL_ID);
    snapshot.windows.push(SessionWindowSnapshot {
        window_id: Some("window-2".to_string()),
        selected_workspace_id: None,
        dock: None,
        tab_manager: SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![session_ops::fresh_terminal_workspace("surface-2")],
            workspace_groups: None,
        },
    });

    let projected = snapshot_for_window(&snapshot, "window-2");
    assert_eq!(projected.windows[0].window_id.as_deref(), Some("window-2"));
    assert_eq!(
        first_panel_id(&projected.windows[0].tab_manager.workspaces[0]),
        Some("surface-2")
    );
    // D1: bootstrap windows carry UUID ids ("main" is only the label).
    let id = snapshot.windows[0].window_id.clone().expect("bootstrap id");
    assert!(Uuid::parse_str(&id).is_ok(), "{id}");
}

#[test]
fn initial_snapshot_mints_a_workspace_id() {
    let snapshot = initial_snapshot(FIRST_PANEL_ID);
    let id = snapshot.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .as_deref()
        .expect("workspace_id minted");
    assert!(Uuid::parse_str(id).is_ok(), "not a uuid: {id}");
}

#[test]
fn initial_snapshot_mints_a_pane_id() {
    let snapshot = initial_snapshot(FIRST_PANEL_ID);
    let pane_ids = pane_ids_in_layout(active_layout(&snapshot));
    assert_eq!(pane_ids.len(), 1);
    assert!(
        Uuid::parse_str(pane_ids[0]).is_ok(),
        "not a uuid: {}",
        pane_ids[0]
    );
}

#[test]
fn workspace_focus_history_navigates_back_skips_stale_and_truncates_branches() {
    let mut history = WorkspaceFocusHistory::default();
    history.record("workspace-a");
    history.record("workspace-b");
    history.record("workspace-c");
    let valid = HashSet::from(["workspace-a", "workspace-c"]);
    assert_eq!(
        history.navigate_back(&valid).as_deref(),
        Some("workspace-a")
    );
    assert_eq!(history.entries, ["workspace-a", "workspace-c"]);

    history.record("workspace-a");
    assert_eq!(history.entries, ["workspace-a", "workspace-c"]);
    history.record("workspace-d");
    assert_eq!(history.entries, ["workspace-a", "workspace-d"]);
    assert_eq!(history.index, Some(1));
}

#[test]
fn workspace_focus_history_requires_a_distinct_previous_entry() {
    let mut history = WorkspaceFocusHistory::default();
    history.record("workspace-a");
    history.record("workspace-a");
    assert_eq!(history.navigate_back(&HashSet::from(["workspace-a"])), None);
}

#[test]
fn snapshot_file_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let snapshot = initial_snapshot(FIRST_PANEL_ID);
    write_snapshot_file(&path, &snapshot).unwrap();
    let loaded = load_snapshot_file(&path).expect("snapshot reloads");
    assert_eq!(loaded, snapshot);
}

#[test]
fn next_panel_counter_tracks_the_highest_surface_suffix() {
    let mut snapshot = initial_snapshot(FIRST_PANEL_ID);
    apply_new_workspace(&mut snapshot, "surface-9", None, None, None, None);
    assert_eq!(next_panel_counter(&snapshot), 10);
}

#[test]
fn apply_close_workspaces_closes_requested_indices_in_original_order() {
    let mut snapshot = initial_snapshot(FIRST_PANEL_ID);
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    apply_new_workspace(&mut snapshot, "surface-3", None, None, None, None);
    apply_new_workspace(&mut snapshot, "surface-4", None, None, None, None);

    assert!(apply_close_workspaces(&mut snapshot, &[3, 1]));
    let workspaces = &snapshot.windows[0].tab_manager.workspaces;
    assert_eq!(workspaces.len(), 2);
    assert_eq!(first_panel_id(&workspaces[0]), Some("surface-1"));
    assert_eq!(first_panel_id(&workspaces[1]), Some("surface-3"));
}

#[test]
fn new_workspace_mints_an_id_and_keeps_existing_ids() {
    let mut snapshot = initial_snapshot("surface-1");
    let first_id = snapshot.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone();
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    let tabs = &snapshot.windows[0].tab_manager;
    assert_eq!(tabs.workspaces.len(), 2);
    // Every workspace has a valid uuid id, the pre-existing one unchanged,
    // and the two ids are distinct.
    let ids: Vec<&str> = tabs
        .workspaces
        .iter()
        .map(|ws| ws.workspace_id.as_deref().expect("id minted"))
        .collect();
    assert!(ids.iter().all(|id| Uuid::parse_str(id).is_ok()));
    assert_eq!(tabs.workspaces[0].workspace_id, first_id);
    assert_ne!(ids[0], ids[1]);
}

#[test]
fn apply_split_grows_the_active_layout() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    assert_eq!(count_leaves(active_layout(&snapshot)), 2);
}

#[test]
fn apply_new_terminal_tab_adds_selected_panel_with_startup_input() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_new_terminal_tab(
        &mut snapshot,
        "surface-1",
        "surface-2",
        None,
        Some("codex fork session-1\r\n"),
        None,
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected pane");
    };
    assert_eq!(pane.panel_ids, ["surface-1", "surface-2"]);
    assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-2"));
    let startups = tab_manager(&snapshot).workspaces[0]
        .panel_terminal_startups
        .as_ref()
        .expect("panel startup");
    assert_eq!(startups.len(), 1);
    assert_eq!(startups[0].panel_id, "surface-2");
    assert_eq!(
        startups[0].initial_terminal_input.as_deref(),
        Some("codex fork session-1")
    );
}

#[test]
fn ssh_terminal_command_uses_parser_approved_open_ssh_arguments() {
    let request = parse_ssh_uri(
        "cmux://ssh?host=dev.example.com&user=alice&port=2222&connect-timeout=10&host-key-policy=accept-new",
    )
    .unwrap();

    assert_eq!(
        ssh_terminal_command(&request),
        "ssh -p 2222 -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new alice@dev.example.com"
    );
}

#[test]
fn apply_ssh_url_request_adds_terminal_tab_with_startup_command() {
    let request = parse_ssh_uri("cmux://ssh?host=dev.example.com&user=alice&port=2222").unwrap();
    let mut snapshot = initial_snapshot("surface-1");

    assert!(apply_ssh_url_request(
        &mut snapshot,
        "surface-1",
        "surface-2",
        &request,
    ));

    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected pane");
    };
    assert_eq!(pane.panel_ids, ["surface-1", "surface-2"]);
    assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-2"));
    let startups = tab_manager(&snapshot).workspaces[0]
        .panel_terminal_startups
        .as_ref()
        .expect("panel startup");
    assert_eq!(startups[0].panel_id, "surface-2");
    assert_eq!(
        startups[0].initial_terminal_command.as_deref(),
        Some("ssh -p 2222 alice@dev.example.com")
    );
}

#[test]
fn apply_ssh_url_request_honors_no_focus() {
    let request = parse_ssh_uri("cmux://ssh?host=dev.example.com&no-focus").unwrap();
    let mut snapshot = initial_snapshot("surface-1");

    assert!(apply_ssh_url_request(
        &mut snapshot,
        "surface-1",
        "surface-2",
        &request,
    ));

    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected pane");
    };
    assert_eq!(pane.panel_ids, ["surface-1", "surface-2"]);
    assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-1"));
}

#[test]
fn apply_split_preserves_existing_pane_id_and_mints_a_new_one() {
    let mut snapshot = initial_snapshot("surface-1");
    let before = pane_ids_in_layout(active_layout(&snapshot))
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(before.len(), 1);

    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));

    let after = pane_ids_in_layout(active_layout(&snapshot));
    assert_eq!(after.len(), 2);
    assert!(after.iter().any(|pane_id| *pane_id == before[0].as_str()));
    assert_eq!(
        after
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        2
    );
    for pane_id in after {
        assert!(Uuid::parse_str(pane_id).is_ok(), "not a uuid: {pane_id}");
    }
    let split_ids = split_ids_in_layout(active_layout(&snapshot));
    assert_eq!(split_ids.len(), 1);
    assert!(Uuid::parse_str(split_ids[0]).is_ok());
}

#[test]
fn apply_split_unknown_panel_is_rejected() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(!apply_split(
        &mut snapshot,
        "nope",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    assert_eq!(count_leaves(active_layout(&snapshot)), 1);
}

#[test]
fn apply_close_collapses_back_to_a_single_pane() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );
    assert_eq!(
        apply_close(&mut snapshot, "surface-2"),
        CloseOutcome::Removed
    );
    assert_eq!(count_leaves(active_layout(&snapshot)), 1);
}

#[test]
fn apply_close_emptying_the_last_pane_clears_layout() {
    let mut snapshot = initial_snapshot("surface-1");
    assert_eq!(
        apply_close(&mut snapshot, "surface-1"),
        CloseOutcome::Emptied
    );
    assert!(snapshot.windows[0].tab_manager.workspaces[0]
        .layout
        .is_none());
}

#[test]
fn closed_browser_tab_for_active_panel_captures_disappearing_browser_pane() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-2",
        Some("https://example.com")
    ));

    let tab = closed_browser_tab_for_active_panel(&snapshot, "surface-2")
        .expect("closed browser tab captured");
    assert_eq!(tab.url, "https://example.com");
    assert_eq!(
        closed_browser_tab_for_active_panel(&snapshot, "surface-1"),
        None
    );
}

#[test]
fn closed_browser_tab_for_active_panel_skips_about_blank() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_open_browser_url(&mut snapshot, "surface-1", None);

    assert_eq!(
        closed_browser_tab_for_active_panel(&snapshot, "surface-1"),
        None
    );
}

#[test]
fn apply_reopen_closed_browser_tab_creates_selected_browser_workspace() {
    let mut snapshot = initial_snapshot("surface-1");
    let tab = ClosedBrowserTabSnapshot {
        url: "https://example.com/docs".to_string(),
    };

    assert!(apply_reopen_closed_browser_tab(
        &mut snapshot,
        &tab,
        "surface-2"
    ));

    let tabs = &snapshot.windows[0].tab_manager;
    assert_eq!(tabs.workspaces.len(), 2);
    assert_eq!(tabs.selected_workspace_index, Some(1));
    let restored = tabs.workspaces[1].layout.as_ref().expect("layout present");
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = restored else {
        panic!("expected single browser pane");
    };
    assert_eq!(pane.panel_ids, ["surface-2"]);
    assert_eq!(pane.surface_kind.as_deref(), Some("browser"));
    assert_eq!(
        pane.browser_url.as_deref(),
        Some("https://example.com/docs")
    );
}

#[test]
fn apply_set_divider_updates_the_active_split() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );
    assert!(apply_set_divider(&mut snapshot, &[], 0.25));
    if let SessionWorkspaceLayoutSnapshot::Split(s) = active_layout(&snapshot) {
        assert_eq!(s.divider_position, 0.25);
    } else {
        panic!("expected a split");
    }
}

#[test]
fn apply_equalize_dividers_evens_out_the_active_layout() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );
    // Skew the divider, then equalize a 2-pane same-axis split back to 0.5.
    apply_set_divider(&mut snapshot, &[], 0.85);
    assert!(apply_equalize_dividers(&mut snapshot));
    if let SessionWorkspaceLayoutSnapshot::Split(s) = active_layout(&snapshot) {
        assert_eq!(s.divider_position, 0.5);
    } else {
        panic!("expected a split");
    }
    // Equalize preserves the leaf count (never adds/removes panes).
    assert_eq!(count_leaves(active_layout(&snapshot)), 2);
}

#[test]
fn apply_equalize_dividers_on_a_single_pane_is_a_noop() {
    // Fresh single-pane layout → no split found → false, snapshot unchanged.
    let mut snapshot = initial_snapshot("surface-1");
    let before = snapshot.clone();
    assert!(!apply_equalize_dividers(&mut snapshot));
    assert_eq!(snapshot, before);
}

#[test]
fn apply_equalize_dividers_on_absent_layout_is_a_noop() {
    // Emptied layout slot (None) → no-op.
    let mut snapshot = initial_snapshot("surface-1");
    assert_eq!(
        apply_close(&mut snapshot, "surface-1"),
        CloseOutcome::Emptied
    );
    assert!(!apply_equalize_dividers(&mut snapshot));
}

#[test]
fn apply_toggle_split_zoom_sets_and_clears_workspace_zoom_target() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    assert!(apply_toggle_split_zoom(&mut snapshot, "surface-2"));
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0]
            .zoomed_panel_id
            .as_deref(),
        Some("surface-2")
    );
    assert!(apply_toggle_split_zoom(&mut snapshot, "surface-2"));
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0].zoomed_panel_id,
        None
    );
}

#[test]
fn apply_set_layout_mode_enables_canvas_and_seeds_panes() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));

    assert!(apply_set_layout_mode(&mut snapshot, Some("canvas")));
    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(workspace.layout_mode.as_deref(), Some("canvas"));
    assert_eq!(workspace.canvas_panes.as_ref().map(Vec::len), Some(2));

    let seeded = workspace.canvas_panes.clone();
    assert!(!apply_set_layout_mode(&mut snapshot, Some("canvas")));
    assert_eq!(tab_manager(&snapshot).workspaces[0].canvas_panes, seeded);

    assert!(apply_set_layout_mode(&mut snapshot, None));
    assert_eq!(tab_manager(&snapshot).workspaces[0].layout_mode, None);
    assert_eq!(tab_manager(&snapshot).workspaces[0].canvas_panes, seeded);
}

#[test]
fn apply_set_canvas_pane_frame_updates_active_workspace_canvas_panes() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_layout_mode(&mut snapshot, Some("canvas")));

    assert!(apply_set_canvas_pane_frame(
        &mut snapshot,
        "surface-1",
        24,
        32,
        640,
        360
    ));

    let pane = &tab_manager(&snapshot).workspaces[0]
        .canvas_panes
        .as_ref()
        .unwrap()[0];
    assert_eq!(
        (pane.x, pane.y, pane.width, pane.height),
        (24, 32, 640, 360)
    );
    assert!(!apply_set_canvas_pane_frame(
        &mut snapshot,
        "surface-1",
        24,
        32,
        640,
        360
    ));
}

#[test]
fn apply_canvas_action_updates_active_workspace_canvas_panes() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    assert!(apply_set_layout_mode(&mut snapshot, Some("canvas")));

    assert!(apply_canvas_action(
        &mut snapshot,
        "distributeVertically",
        None
    ));
    let panes = tab_manager(&snapshot).workspaces[0]
        .canvas_panes
        .as_ref()
        .unwrap();
    assert_eq!(panes[0].y, 0);
    assert_eq!(panes[1].y, 816);
    assert!(!apply_canvas_action(&mut snapshot, "doesNotExist", None));
}

#[test]
fn apply_set_surface_kind_marks_the_target_pane() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_surface_kind(
        &mut snapshot,
        "surface-1",
        Some("agent".to_string())
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.surface_kind.as_deref(), Some("agent"));
}

#[test]
fn apply_select_adjacent_panel_updates_the_active_pane_selection() {
    let mut snapshot = initial_snapshot("surface-1");
    let layout = active_layout_slot(&mut snapshot)
        .expect("layout slot")
        .as_mut()
        .expect("layout");
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = layout else {
        panic!("expected a pane");
    };
    pane.panel_ids = vec![
        "surface-1".to_string(),
        "surface-2".to_string(),
        "surface-3".to_string(),
    ];
    pane.selected_panel_id = Some("surface-1".to_string());

    assert!(apply_select_adjacent_panel(
        &mut snapshot,
        "surface-1",
        true
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-2"));

    assert!(apply_select_adjacent_panel(
        &mut snapshot,
        "surface-1",
        false
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-1"));
}

#[test]
fn apply_select_workspace_surface_selects_workspace_and_panel() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);
    let workspace_id = snapshot.windows[0].tab_manager.workspaces[1]
        .workspace_id
        .clone()
        .expect("workspace id");
    let layout = snapshot.windows[0].tab_manager.workspaces[1]
        .layout
        .as_mut()
        .expect("layout");
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = layout else {
        panic!("expected a pane");
    };
    pane.panel_ids = vec!["surface-2".to_string(), "surface-3".to_string()];
    pane.selected_panel_id = Some("surface-2".to_string());

    assert!(apply_select_workspace_surface(
        &mut snapshot,
        &workspace_id,
        "surface-3",
    ));
    assert_eq!(
        snapshot.windows[0].tab_manager.selected_workspace_index,
        Some(1)
    );
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = snapshot.windows[0].tab_manager.workspaces[1]
        .layout
        .as_ref()
        .expect("layout")
    else {
        panic!("expected a pane");
    };
    assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-3"));
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[1]
            .focused_panel_id
            .as_deref(),
        Some("surface-3")
    );

    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);
    assert!(!apply_select_workspace_surface(
        &mut snapshot,
        &workspace_id,
        "missing",
    ));
    assert_eq!(
        snapshot.windows[0].tab_manager.selected_workspace_index,
        Some(0)
    );
}

#[test]
fn apply_focus_pane_selects_workspace_and_preserves_pane_tab() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[1];
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.pane_id = Some("pane-2".into());
    pane.panel_ids.push("surface-3".into());
    pane.selected_panel_id = Some("surface-3".into());

    assert_eq!(apply_focus_pane(&mut snapshot, 0, 1, "pane-2"), Ok(()));
    assert_eq!(
        snapshot.windows[0].tab_manager.selected_workspace_index,
        Some(1)
    );
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[1]
            .focused_panel_id
            .as_deref(),
        Some("surface-3")
    );
    assert_eq!(
        apply_focus_pane(&mut snapshot, 0, 1, "missing"),
        Err(PaneFocusControlError::PaneNotFound)
    );
}

#[test]
fn apply_focus_panel_persists_focus_without_changing_selection() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("surface-1")
    );
    assert!(!apply_focus_panel(&mut snapshot, "surface-1"));
    assert!(apply_focus_panel(&mut snapshot, "surface-2"));
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0]
            .focused_panel_id
            .as_deref(),
        Some("surface-2")
    );
    assert!(!apply_focus_panel(&mut snapshot, "missing"));
    assert_eq!(
        snapshot.windows[0].tab_manager.selected_workspace_index,
        Some(0)
    );
}

#[test]
fn apply_select_workspace_by_id_selects_existing_workspace() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);
    let workspace_id = snapshot.windows[0].tab_manager.workspaces[1]
        .workspace_id
        .clone()
        .expect("workspace id");

    assert!(apply_select_workspace_by_id(&mut snapshot, &workspace_id));
    assert_eq!(
        snapshot.windows[0].tab_manager.selected_workspace_index,
        Some(1)
    );
    assert!(workspace_is_selected(&snapshot, &workspace_id));
    assert!(!apply_select_workspace_by_id(&mut snapshot, &workspace_id));
    assert!(!apply_select_workspace_by_id(&mut snapshot, "missing"));
}

#[test]
fn parse_session_navigation_uri_accepts_workspace_pane_and_surface_links() {
    assert_eq!(
        parse_session_navigation_uri("cmux://workspace/workspace-1").unwrap(),
        SessionNavigationTarget {
            workspace_id: "workspace-1".to_string(),
            panel_id: None,
        }
    );
    assert_eq!(
        parse_session_navigation_uri("cmux://workspace/workspace%201/surface/surface%2F1").unwrap(),
        SessionNavigationTarget {
            workspace_id: "workspace 1".to_string(),
            panel_id: Some("surface/1".to_string()),
        }
    );
    assert_eq!(
        parse_session_navigation_uri("cmux://workspace/workspace-1/pane/pane-1").unwrap(),
        SessionNavigationTarget {
            workspace_id: "workspace-1".to_string(),
            panel_id: Some("pane-1".to_string()),
        }
    );
    assert_eq!(
        parse_session_navigation_uri("cmux-dev://workspace/workspace-1").unwrap(),
        SessionNavigationTarget {
            workspace_id: "workspace-1".to_string(),
            panel_id: None,
        }
    );
    assert_eq!(
        parse_session_navigation_uri("cmux-nightly://workspace/workspace-1").unwrap(),
        SessionNavigationTarget {
            workspace_id: "workspace-1".to_string(),
            panel_id: None,
        }
    );
}

#[test]
fn parse_session_navigation_uri_rejects_bad_scheme_route_or_encoding() {
    assert!(parse_session_navigation_uri("other://workspace/workspace-1").is_err());
    assert!(parse_session_navigation_uri("cmux://notification?id=1").is_err());
    assert!(parse_session_navigation_uri("cmux://workspace/workspace-1/surface/%ZZ").is_err());
}

#[test]
fn apply_open_markdown_file_binds_the_path_and_switches_the_surface() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_open_markdown_file(
        &mut snapshot,
        "surface-1",
        "C:/docs/readme.md"
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.surface_kind.as_deref(), Some("markdown"));
    assert_eq!(
        pane.markdown_file_path.as_deref(),
        Some("C:/docs/readme.md")
    );
}

#[test]
fn apply_open_file_binds_the_path_and_switches_the_surface() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_open_file(
        &mut snapshot,
        "surface-1",
        "C:/docs/notes.txt"
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.surface_kind.as_deref(), Some("file"));
    assert_eq!(pane.file_path.as_deref(), Some("C:/docs/notes.txt"));
}

#[test]
fn apply_open_diff_viewer_binds_the_token_path_and_switches_the_surface() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_open_diff_viewer(
        &mut snapshot,
        "surface-1",
        "tok-abcdef0123456789",
        "/review/index.html"
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.surface_kind.as_deref(), Some("diff"));
    assert_eq!(
        pane.diff_viewer_token.as_deref(),
        Some("tok-abcdef0123456789")
    );
    assert_eq!(
        pane.diff_viewer_request_path.as_deref(),
        Some("/review/index.html")
    );
}

#[test]
fn apply_open_browser_url_binds_the_url_and_switches_the_surface() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://example.com")
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.surface_kind.as_deref(), Some("browser"));
    assert_eq!(pane.browser_url.as_deref(), Some("https://example.com"));
    assert_eq!(pane.browser_back_history, None);
    assert_eq!(pane.browser_forward_history, None);
    assert_eq!(pane.browser_omnibar_visible, None);
    assert_eq!(pane.browser_focus_mode_active, None);
    assert_eq!(pane.browser_developer_tools_visible, None);
    assert_eq!(pane.browser_developer_tools_panel, None);
    assert_eq!(pane.browser_page_zoom, Some(1.0));
}

#[test]
fn apply_open_browser_url_inherits_remote_workspace_proxy_url() {
    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].tab_manager.workspaces[0].remote =
        Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
            transport: "ssh".to_string(),
            destination: "dev.example.com".to_string(),
            port: Some(22),
            local_proxy_port: Some(31337),
            persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
            remote_daemon_path: None,
            remote_daemon_relay_port: None,
            identity_file: None,
            ssh_options: Vec::new(),
            auto_connect: true,
        }));

    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://example.com")
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(
        pane.browser_proxy_url.as_deref(),
        Some("socks5://127.0.0.1:31337")
    );
}

#[test]
fn browser_panels_for_workspace_proxy_url_finds_matching_browser_panel() {
    let mut snapshot = initial_snapshot("surface-1");
    let workspace_id = snapshot.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone()
        .expect("workspace id");
    snapshot.windows[0].tab_manager.workspaces[0].remote =
        Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
            transport: "ssh".to_string(),
            destination: "dev.example.com".to_string(),
            port: Some(22),
            local_proxy_port: Some(31337),
            persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
            remote_daemon_path: None,
            remote_daemon_relay_port: None,
            identity_file: None,
            ssh_options: Vec::new(),
            auto_connect: true,
        }));
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://example.com")
    ));

    assert_eq!(
        browser_panels_for_workspace_proxy_url(
            &snapshot,
            &workspace_id,
            "socks5://127.0.0.1:31337"
        ),
        vec!["surface-1".to_string()]
    );
    assert!(browser_panels_for_workspace_proxy_url(
        &snapshot,
        &workspace_id,
        "socks5://127.0.0.1:31338"
    )
    .is_empty());
}

#[test]
fn browser_panels_for_workspace_collects_existing_browser_panes() {
    let mut snapshot = initial_snapshot("surface-1");
    let workspace_id = snapshot.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone()
        .expect("workspace id");
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    assert!(apply_split(
        &mut snapshot,
        "surface-2",
        SessionSplitOrientation::Vertical,
        "surface-3",
        false,
    ));
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://one.example")
    ));
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-3",
        Some("https://three.example")
    ));

    assert_eq!(
        browser_panels_for_workspace(&snapshot, &workspace_id),
        vec!["surface-1".to_string(), "surface-3".to_string()]
    );
}

fn proxy_http_observation(
    protocol: crate::remote_proxy::ProxyHandshakeProtocol,
    path: &str,
    response_body: &str,
) -> crate::remote_proxy::ProxyTrafficObservation {
    crate::remote_proxy::ProxyTrafficObservation {
        protocol,
        target: crate::remote_proxy::ProxyTarget {
            host: "example.com".to_string(),
            port: 80,
        },
        upstream_prefix: format!(
            "GET {path} HTTP/1.1\r\nHost: example.com\r\nX-Test: observer\r\n\r\n"
        )
        .into_bytes(),
        upstream_truncated: false,
        downstream_prefix: format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n{response_body}"
        )
        .into_bytes(),
        downstream_truncated: false,
        started_at_ms: 10,
        completed_at_ms: 42,
    }
}

fn proxy_opaque_tunnel_observation(
    protocol: crate::remote_proxy::ProxyHandshakeProtocol,
) -> crate::remote_proxy::ProxyTrafficObservation {
    crate::remote_proxy::ProxyTrafficObservation {
        protocol,
        target: crate::remote_proxy::ProxyTarget {
            host: "secure.example".to_string(),
            port: 443,
        },
        upstream_prefix: vec![0x16, 0x03, 0x01, 0x00, 0x2a],
        upstream_truncated: false,
        downstream_prefix: vec![0x16, 0x03, 0x03, 0x00, 0x31],
        downstream_truncated: true,
        started_at_ms: 100,
        completed_at_ms: 155,
    }
}

#[test]
fn panel_proxy_observer_bridge_records_panel_attributed_network_metadata() {
    let browser_state = crate::browser::BrowserWebviewState::default();
    let observation = proxy_http_observation(
        crate::remote_proxy::ProxyHandshakeProtocol::Socks5,
        "/panel",
        "panel ok",
    );

    record_proxy_observation_for_browser_panel(&browser_state, "surface-1", &observation, "panel")
        .unwrap();

    let reply = crate::browser::browser_network_requests_for_control(
        &browser_state,
        "surface-1",
        crate::browser::BrowserNetworkRequestsQuery::default(),
    )
    .unwrap();
    assert_eq!(reply.requests.len(), 1);
    let record = &reply.requests[0];
    assert_eq!(record.source, "proxy-stream-http");
    assert_eq!(record.transport, "socks5");
    assert_eq!(record.proxy_attribution.as_deref(), Some("panel"));
    assert_eq!(record.url, "http://example.com/panel");
    assert_eq!(record.method, "GET");
    assert_eq!(
        record.request_headers.get("x-test").map(String::as_str),
        Some("observer")
    );
    assert_eq!(record.response_status, Some(200));
    assert_eq!(record.response_body.as_deref(), Some("panel ok"));
    assert_eq!(record.response_body_preview_kind, "text");
    assert_eq!(record.duration_ms, Some(32));
    assert_eq!(reply.observer.proxy_attribution_mode, "panel");
}

#[test]
fn panel_proxy_observer_bridge_records_opaque_tunnel_metadata() {
    let browser_state = crate::browser::BrowserWebviewState::default();
    let observation =
        proxy_opaque_tunnel_observation(crate::remote_proxy::ProxyHandshakeProtocol::Socks5);

    record_proxy_observation_for_browser_panel(&browser_state, "surface-1", &observation, "panel")
        .unwrap();

    let reply = crate::browser::browser_network_requests_for_control(
        &browser_state,
        "surface-1",
        crate::browser::BrowserNetworkRequestsQuery::default(),
    )
    .unwrap();
    assert_eq!(reply.requests.len(), 1);
    let record = &reply.requests[0];
    assert_eq!(record.source, "proxy-stream-tunnel");
    assert_eq!(record.transport, "socks5");
    assert_eq!(record.proxy_attribution.as_deref(), Some("panel"));
    assert_eq!(record.url, "https://secure.example/");
    assert_eq!(record.method, "CONNECT");
    assert_eq!(record.response_status, Some(200));
    assert_eq!(record.request_body_preview_kind, "binary");
    assert_eq!(record.response_body_preview_kind, "binary");
    assert!(record.response_body_truncated);
    assert_eq!(record.duration_ms, Some(55));
    assert_eq!(reply.observer.proxy_attribution_mode, "panel");
}

#[test]
fn workspace_proxy_observer_bridge_records_only_unambiguous_browser_panel() {
    let mut snapshot = initial_snapshot("surface-1");
    let workspace_id = snapshot.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone()
        .expect("workspace id");
    snapshot.windows[0].tab_manager.workspaces[0].remote =
        Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
            transport: "ssh".to_string(),
            destination: "dev.example.com".to_string(),
            port: Some(22),
            local_proxy_port: Some(31337),
            persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
            remote_daemon_path: None,
            remote_daemon_relay_port: None,
            identity_file: None,
            ssh_options: Vec::new(),
            auto_connect: true,
        }));
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://example.com")
    ));

    let browser_state = crate::browser::BrowserWebviewState::default();
    let observation = proxy_http_observation(
        crate::remote_proxy::ProxyHandshakeProtocol::HttpConnect,
        "/workspace",
        "workspace ok",
    );
    let recorded_panel = record_workspace_proxy_observation_for_browser_panel(
        &snapshot,
        &browser_state,
        &workspace_id,
        "socks5://127.0.0.1:31337",
        &observation,
    )
    .unwrap();

    assert_eq!(recorded_panel.as_deref(), Some("surface-1"));
    let reply = crate::browser::browser_network_requests_for_control(
        &browser_state,
        "surface-1",
        crate::browser::BrowserNetworkRequestsQuery::default(),
    )
    .unwrap();
    assert_eq!(reply.requests.len(), 1);
    let record = &reply.requests[0];
    assert_eq!(record.transport, "http-connect");
    assert_eq!(record.proxy_attribution.as_deref(), Some("workspace"));
    assert_eq!(record.url, "http://example.com/workspace");
    assert_eq!(record.response_body.as_deref(), Some("workspace ok"));
    assert_eq!(reply.observer.proxy_attribution_mode, "workspace");
}

#[test]
fn workspace_proxy_observer_bridge_skips_ambiguous_shared_proxy_panels() {
    let mut snapshot = initial_snapshot("surface-1");
    let workspace_id = snapshot.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone()
        .expect("workspace id");
    snapshot.windows[0].tab_manager.workspaces[0].remote =
        Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
            transport: "ssh".to_string(),
            destination: "dev.example.com".to_string(),
            port: Some(22),
            local_proxy_port: Some(31337),
            persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
            remote_daemon_path: None,
            remote_daemon_relay_port: None,
            identity_file: None,
            ssh_options: Vec::new(),
            auto_connect: true,
        }));
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://one.example")
    ));
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-2",
        Some("https://two.example")
    ));

    let browser_state = crate::browser::BrowserWebviewState::default();
    let observation = proxy_http_observation(
        crate::remote_proxy::ProxyHandshakeProtocol::Socks5,
        "/ambiguous",
        "ambiguous",
    );
    let recorded_panel = record_workspace_proxy_observation_for_browser_panel(
        &snapshot,
        &browser_state,
        &workspace_id,
        "socks5://127.0.0.1:31337",
        &observation,
    )
    .unwrap();

    assert_eq!(recorded_panel, None);
    for panel_id in ["surface-1", "surface-2"] {
        let reply = crate::browser::browser_network_requests_for_control(
            &browser_state,
            panel_id,
            crate::browser::BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert!(reply.requests.is_empty());
    }
}

#[test]
fn apply_open_browser_url_sets_remote_proxy_only_on_target_pane() {
    fn proxy_for_panel<'a>(
        layout: &'a SessionWorkspaceLayoutSnapshot,
        panel_id: &str,
    ) -> Option<&'a str> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => pane
                .panel_ids
                .iter()
                .any(|candidate| candidate == panel_id)
                .then(|| pane.browser_proxy_url.as_deref())
                .flatten(),
            SessionWorkspaceLayoutSnapshot::Split(split) => proxy_for_panel(&split.first, panel_id)
                .or_else(|| proxy_for_panel(&split.second, panel_id)),
        }
    }

    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].tab_manager.workspaces[0].remote =
        Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
            transport: "ssh".to_string(),
            destination: "dev.example.com".to_string(),
            port: Some(22),
            local_proxy_port: Some(31337),
            persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
            remote_daemon_path: None,
            remote_daemon_relay_port: None,
            identity_file: None,
            ssh_options: Vec::new(),
            auto_connect: true,
        }));
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));

    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-2",
        Some("https://example.com")
    ));

    let layout = active_layout(&snapshot);
    assert_eq!(proxy_for_panel(layout, "surface-1"), None);
    assert_eq!(
        proxy_for_panel(layout, "surface-2"),
        Some("socks5://127.0.0.1:31337")
    );
}

#[test]
fn apply_open_browser_url_tracks_history_without_resetting_zoom() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://one.example")
    ));
    assert!(apply_set_browser_zoom(&mut snapshot, "surface-1", 1.5));
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://two.example")
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_url.as_deref(), Some("https://two.example"));
    assert_eq!(
        pane.browser_back_history.as_deref(),
        Some(["https://one.example/".to_string()].as_slice())
    );
    assert_eq!(pane.browser_forward_history, None);
    assert_eq!(pane.browser_page_zoom, Some(1.5));
}

#[test]
fn apply_browser_back_and_forward_use_persisted_history() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://one.example")
    ));
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://two.example")
    ));

    assert!(apply_browser_go_back(&mut snapshot, "surface-1"));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_url.as_deref(), Some("https://one.example/"));
    assert_eq!(pane.browser_back_history, None);
    assert_eq!(
        pane.browser_forward_history.as_deref(),
        Some(["https://two.example/".to_string()].as_slice())
    );

    assert!(apply_browser_go_forward(&mut snapshot, "surface-1"));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_url.as_deref(), Some("https://two.example/"));
    assert_eq!(
        pane.browser_back_history.as_deref(),
        Some(["https://one.example/".to_string()].as_slice())
    );
    assert_eq!(pane.browser_forward_history, None);
}

#[test]
fn apply_clear_browser_history_preserves_current_url() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://one.example")
    ));
    assert!(apply_open_browser_url(
        &mut snapshot,
        "surface-1",
        Some("https://two.example")
    ));
    assert!(apply_browser_go_back(&mut snapshot, "surface-1"));

    assert!(apply_clear_browser_history(&mut snapshot, "surface-1"));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_url.as_deref(), Some("https://one.example/"));
    assert_eq!(pane.browser_back_history, None);
    assert_eq!(pane.browser_forward_history, None);
    assert!(!apply_clear_browser_history(&mut snapshot, "surface-1"));
}

#[test]
fn apply_toggle_browser_omnibar_flips_visible_default() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_toggle_browser_omnibar(&mut snapshot, "surface-1"));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_omnibar_visible, Some(false));

    assert!(apply_toggle_browser_omnibar(&mut snapshot, "surface-1"));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_omnibar_visible, Some(true));
}

#[test]
fn apply_toggle_browser_focus_mode_flips_inactive_default() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_toggle_browser_focus_mode(&mut snapshot, "surface-1"));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_focus_mode_active, Some(true));

    assert!(apply_toggle_browser_focus_mode(&mut snapshot, "surface-1"));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_focus_mode_active, Some(false));
}

#[test]
fn apply_browser_developer_tools_visibility_and_panel_persist() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_toggle_browser_developer_tools(
        &mut snapshot,
        "surface-1"
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_developer_tools_visible, Some(true));
    assert_eq!(
        pane.browser_developer_tools_panel.as_deref(),
        Some("inspector")
    );

    assert!(apply_show_browser_developer_tools(
        &mut snapshot,
        "surface-1",
        "console"
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_developer_tools_visible, Some(true));
    assert_eq!(
        pane.browser_developer_tools_panel.as_deref(),
        Some("console")
    );

    assert!(apply_toggle_browser_developer_tools(
        &mut snapshot,
        "surface-1"
    ));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_developer_tools_visible, Some(false));
    assert_eq!(
        pane.browser_developer_tools_panel.as_deref(),
        Some("console")
    );
}

#[test]
fn apply_set_browser_zoom_clamps_to_supported_range() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_browser_zoom(&mut snapshot, "surface-1", 12.0));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_page_zoom, Some(3.0));
    assert!(apply_set_browser_zoom(&mut snapshot, "surface-1", f64::NAN));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
        panic!("expected a pane");
    };
    assert_eq!(pane.browser_page_zoom, Some(1.0));
}

fn tab_manager(snapshot: &AppSessionSnapshot) -> &SessionTabManagerSnapshot {
    &snapshot.windows[0].tab_manager
}

// The tab-manager workspace logic is unit-tested in `cmux_core::session_ops`;
// these verify the desktop `apply_*` fns delegate to it against the first
// window of a real `AppSessionSnapshot`.

#[test]
fn restore_remints_only_typed_identities_and_preserves_equal_free_text() {
    const LEGACY_WINDOW_ID: &str = "main";
    const LEGACY_WORKSPACE_ID: &str = "workspace-1";
    const LEGACY_PANE_ID: &str = "pane-1";
    const LEGACY_SURFACE_ID: &str = "surface-1";

    let text = || LEGACY_SURFACE_ID.to_string();
    let same_text_environment = BTreeMap::from([(text(), text())]);
    let mut legacy = initial_snapshot(LEGACY_SURFACE_ID);
    let window = &mut legacy.windows[0];
    window.window_id = Some(LEGACY_WINDOW_ID.to_string());
    window.selected_workspace_id = Some(LEGACY_WORKSPACE_ID.to_string());
    let workspace = &mut window.tab_manager.workspaces[0];
    workspace.workspace_id = Some(LEGACY_WORKSPACE_ID.to_string());
    workspace.custom_title = Some(text());
    workspace.custom_description = Some(text());
    workspace.focused_panel_id = Some(LEGACY_SURFACE_ID.to_string());
    workspace.focused_pane_id = Some(LEGACY_PANE_ID.to_string());
    workspace.surface_resume_bindings = Some(vec![
        cmux_core::session::SessionSurfaceResumeBindingRecordSnapshot {
            surface_id: LEGACY_SURFACE_ID.to_string(),
            binding: cmux_core::session::SessionSurfaceResumeBindingSnapshot {
                name: None,
                kind: None,
                command: text(),
                cwd: Some(text()),
                checkpoint_id: None,
                source: None,
                environment: None,
                auto_resume: false,
                approval_policy: None,
                approval_record_id: None,
                updated_at: 1.0,
            },
        },
    ]);

    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        workspace.layout.as_mut().expect("legacy pane")
    else {
        unreachable!()
    };
    pane.pane_id = Some(LEGACY_PANE_ID.to_string());
    pane.browser_url = Some(text());

    let surface = &mut workspace.surfaces.as_mut().expect("surface records")[0];
    surface.pane_id = LEGACY_PANE_ID.to_string();
    surface.kind = cmux_core::session::SessionSurfaceKindSnapshot::RemoteTerminal {
        remote_session_id: None,
        remote_context: Some(serde_json::json!({
            "notification": {"body": LEGACY_SURFACE_ID},
            "opaque": {
                LEGACY_SURFACE_ID: LEGACY_SURFACE_ID,
                "value": LEGACY_SURFACE_ID
            }
        })),
        arrival_generation: Some(1),
    };
    surface.terminal_startup = Some(cmux_core::session::SessionSurfaceTerminalStartupSnapshot {
        command: Some(text()),
        working_directory: Some(text()),
        initial_input: Some(text()),
        environment: Some(same_text_environment),
        tmux_start_command: Some(text()),
        remote_pty_session_id: None,
        resume_binding: None,
        hibernation: None,
    });

    remint_noncanonical_identities(&mut legacy);

    fn assert_reminted(actual: &str, legacy: &str) {
        assert_ne!(actual, legacy);
        assert!(Uuid::parse_str(actual).is_ok(), "{actual}");
    }
    let window = &legacy.windows[0];
    let window_id = window.window_id.as_deref().expect("reminted window id");
    assert_reminted(window_id, LEGACY_WINDOW_ID);
    let workspace = &window.tab_manager.workspaces[0];
    let workspace_id = workspace
        .workspace_id
        .as_deref()
        .expect("reminted workspace id");
    assert_reminted(workspace_id, LEGACY_WORKSPACE_ID);
    assert_eq!(window.selected_workspace_id.as_deref(), Some(workspace_id));
    let SessionWorkspaceLayoutSnapshot::Pane(pane) =
        workspace.layout.as_ref().expect("reminted pane")
    else {
        unreachable!()
    };
    let pane_id = pane.pane_id.as_deref().expect("reminted pane id");
    assert_reminted(pane_id, LEGACY_PANE_ID);
    assert_eq!(workspace.focused_pane_id.as_deref(), Some(pane_id));
    let surface_id = pane.panel_ids[0].as_str();
    assert_reminted(surface_id, LEGACY_SURFACE_ID);
    assert_eq!(pane.selected_panel_id.as_deref(), Some(surface_id));
    assert_eq!(workspace.focused_panel_id.as_deref(), Some(surface_id));
    let surface = &workspace.surfaces.as_deref().expect("surface records")[0];
    assert_eq!(surface.surface_id, surface_id);
    assert_eq!(surface.pane_id, pane_id);
    assert_eq!(
        workspace.surface_resume_bindings.as_deref().unwrap()[0].surface_id,
        surface_id
    );
    let startup = surface.terminal_startup.as_ref().expect("terminal startup");
    let binding = &workspace.surface_resume_bindings.as_deref().unwrap()[0].binding;
    let cmux_core::session::SessionSurfaceKindSnapshot::RemoteTerminal { remote_context, .. } =
        &surface.kind
    else {
        unreachable!()
    };
    let context = remote_context.as_ref().expect("remote context");
    let environment = startup.environment.as_ref().expect("environment");
    let free_text = [
        ("working_directory", startup.working_directory.as_deref()),
        ("initial_input", startup.initial_input.as_deref()),
        ("tmux_start_command", startup.tmux_start_command.as_deref()),
        (
            "environment_key",
            environment.keys().next().map(String::as_str),
        ),
        (
            "environment_value",
            environment.get(LEGACY_SURFACE_ID).map(String::as_str),
        ),
        ("url", pane.browser_url.as_deref()),
        ("title", workspace.custom_title.as_deref()),
        ("description", workspace.custom_description.as_deref()),
        (
            "notification_body",
            context["notification"]["body"].as_str(),
        ),
        ("command", startup.command.as_deref()),
        ("cwd", binding.cwd.as_deref()),
        (
            "arbitrary_metadata_key",
            context["opaque"]
                .as_object()
                .and_then(|object| object.get(LEGACY_SURFACE_ID))
                .and_then(serde_json::Value::as_str),
        ),
        (
            "arbitrary_metadata_value",
            context["opaque"]["value"].as_str(),
        ),
    ];
    let rewritten: Vec<_> = free_text
        .into_iter()
        .filter_map(|(field, actual)| (actual != Some(LEGACY_SURFACE_ID)).then_some(field))
        .collect();

    assert!(
        rewritten.is_empty(),
        "legacy identity remint rewrote free text fields: {}",
        rewritten.join(", ")
    );
}

#[test]
fn bootstrap_workspace_carries_default_directory_and_surface_startup() {
    // Canonical Workspace init: currentDirectory = requested ?? home
    // (Workspace.swift:2885-2891 at pinned e1825d40d); the first terminal
    // spawns with it, so its requestedWorkingDirectory is present from
    // birth (REMEDIATION.md divergence 6: null-vs-present is capture-pinned).
    let snapshot = initial_snapshot("surface-1");
    let workspace = &snapshot.windows[0].tab_manager.workspaces[0];
    let directory = workspace
        .current_directory
        .clone()
        .expect("bootstrap workspace directory");
    assert!(!directory.trim().is_empty());
    let records = workspace
        .surfaces
        .as_deref()
        .expect("bootstrap surface records");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].surface_id, "surface-1");
    assert_eq!(
        records[0]
            .terminal_startup
            .as_ref()
            .and_then(|startup| startup.working_directory.as_deref()),
        Some(directory.as_str())
    );
}

#[test]
fn new_workspace_seeds_its_initial_surface_directory() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(
        &mut snapshot,
        "surface-2",
        Some("C:/inherited"),
        None,
        None,
        None,
    );
    let tabs = tab_manager(&snapshot);
    let index = usize::try_from(tabs.selected_workspace_index.unwrap()).unwrap();
    let workspace = &tabs.workspaces[index];
    assert_eq!(workspace.current_directory.as_deref(), Some("C:/inherited"));
    let records = workspace.surfaces.as_deref().expect("surface records");
    assert_eq!(records[0].surface_id, "surface-2");
    assert_eq!(
        records[0]
            .terminal_startup
            .as_ref()
            .and_then(|startup| startup.working_directory.as_deref()),
        Some("C:/inherited")
    );
}

#[test]
fn apply_new_workspace_appends_and_selects_it() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    let tabs = tab_manager(&snapshot);
    assert_eq!(tabs.workspaces.len(), 2);
    assert_eq!(tabs.selected_workspace_index, Some(1));
    assert_eq!(count_leaves(active_layout(&snapshot)), 1);
}

#[test]
fn apply_new_workspace_mints_a_pane_id_for_the_new_workspace() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    let layout = tab_manager(&snapshot).workspaces[1]
        .layout
        .as_ref()
        .expect("layout present");
    let pane_ids = pane_ids_in_layout(layout);
    assert_eq!(pane_ids.len(), 1);
    assert!(
        Uuid::parse_str(pane_ids[0]).is_ok(),
        "not a uuid: {}",
        pane_ids[0]
    );
}

#[test]
fn apply_new_workspace_carries_the_requested_directory() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(
        &mut snapshot,
        "surface-2",
        Some("C:/repo"),
        None,
        None,
        None,
    );
    let workspace = &tab_manager(&snapshot).workspaces[1];
    assert_eq!(workspace.current_directory.as_deref(), Some("C:/repo"));
}
#[test]
fn apply_new_workspace_inherits_selected_workspace_directory_when_unspecified() {
    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].tab_manager.workspaces[0].current_directory =
        Some("C:/inherited".to_string());

    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);

    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[1]
            .current_directory
            .as_deref(),
        Some("C:/inherited")
    );
}

#[test]
fn cmux_layout_builds_split_tree_and_surface_startups() {
    let layout: CmuxLayoutNode = serde_json::from_value(serde_json::json!({
        "direction": "horizontal",
        "split": 0.4,
        "children": [
            {"pane": {"surfaces": [{"type": "terminal", "command": "cargo test", "env": {"RUST_LOG": "debug"}}]}},
            {"pane": {"surfaces": [{"type": "browser", "url": "https://example.test", "focus": true}]}}
        ]
    }))
    .unwrap();
    let next = AtomicU64::new(10);
    let mut ids = DeferredPanelIds::new(&next);

    let (layout, focused, startups) =
        session_layout_from_cmux(layout, &mut ids).expect("valid canonical layout");

    assert!(matches!(layout, SessionWorkspaceLayoutSnapshot::Split(_)));
    // D2: generated ids are UUIDs.
    let focused_id = focused.clone().expect("focused id");
    assert!(Uuid::parse_str(&focused_id).is_ok(), "{focused_id}");
    assert_eq!(next.load(Ordering::Relaxed), 10);
    assert_eq!(ids.used, 2);
    assert_eq!(startups.len(), 1);
    assert_eq!(
        startups[0].initial_terminal_command.as_deref(),
        Some("cargo test")
    );
    assert_eq!(
        startups[0]
            .initial_terminal_environment
            .as_ref()
            .and_then(|env| env.get("RUST_LOG"))
            .map(String::as_str),
        Some("debug")
    );
}

#[test]
fn workspace_environment_is_inherited_by_later_terminal_surfaces_and_round_trips() {
    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].tab_manager.workspaces[0].workspace_environment = Some(BTreeMap::from([(
        "CMUX_SCOPE".to_string(),
        "workspace".to_string(),
    )]));

    assert!(apply_new_terminal_tab(
        &mut snapshot,
        "surface-1",
        "surface-2",
        None,
        None,
        None,
    ));
    let workspace = &snapshot.windows[0].tab_manager.workspaces[0];
    assert_eq!(
        workspace.panel_terminal_startups.as_ref().unwrap()[0]
            .initial_terminal_environment
            .as_ref()
            .and_then(|env| env.get("CMUX_SCOPE"))
            .map(String::as_str),
        Some("workspace")
    );
    let restored: AppSessionSnapshot =
        serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).unwrap();
    assert_eq!(restored, snapshot);
}

#[test]
fn closed_workspace_history_snapshot_keeps_restorable_state() {
    let mut snapshot = initial_snapshot("surface-1");
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.custom_title = Some("Restorable".to_string());
    workspace.workspace_environment = Some(BTreeMap::from([(
        "TOKEN".to_string(),
        "preserved".to_string(),
    )]));

    let closed = closed_workspace_snapshot(&snapshot, 0, 0).unwrap();

    // D1: bootstrap windows carry UUID ids.
    let closed_window = closed.window_id.clone().expect("closed window id");
    assert!(Uuid::parse_str(&closed_window).is_ok(), "{closed_window}");
    assert_eq!(closed.original_index, 0);
    assert_eq!(closed.workspace.custom_title.as_deref(), Some("Restorable"));
    assert!(closed.workspace.layout.is_some());
    assert_eq!(
        closed
            .workspace
            .workspace_environment
            .as_ref()
            .and_then(|env| env.get("TOKEN"))
            .map(String::as_str),
        Some("preserved")
    );
}

#[test]
fn remote_workspace_rename_intent_is_exactly_once_for_enabled_remote() {
    let mut workspace = session_ops::fresh_terminal_workspace("surface-1");
    workspace.workspace_id = Some("workspace-remote".to_string());
    workspace.remote = Some(SessionWorkspaceRemoteSnapshot {
        enabled: true,
        state: "connected".to_string(),
        connected: true,
        transport: Some("tmux".to_string()),
        destination: Some("remote".to_string()),
        port: None,
        local_proxy_port: None,
        persistent_daemon_slot: None,
        has_ssh_options: false,
        detail: None,
        daemon: None,
        proxy: None,
        detected_ports: Vec::new(),
        forwarded_ports: Vec::new(),
        conflicted_ports: Vec::new(),
        active_terminal_sessions: Some(1),
    });

    assert_eq!(
        remote_workspace_rename_intent(&workspace, "  Build  "),
        Some(("workspace-remote".to_string(), "Build".to_string()))
    );
    assert_eq!(remote_workspace_rename_intent(&workspace, "  \r\n "), None);
    workspace.remote.as_mut().unwrap().transport = Some("ssh".to_string());
    assert_eq!(
        remote_workspace_rename_intent(&workspace, "Build"),
        None,
        "ordinary SSH workspaces are not remote tmux mirrors"
    );
    workspace.remote.as_mut().unwrap().transport = Some("tmux".to_string());
    workspace.remote.as_mut().unwrap().connected = false;
    assert_eq!(
        remote_workspace_rename_intent(&workspace, "Build"),
        None,
        "disconnected tmux mirrors cannot receive control-mode renames"
    );
    workspace.remote.as_mut().unwrap().connected = true;
    workspace.remote.as_mut().unwrap().enabled = false;
    assert_eq!(remote_workspace_rename_intent(&workspace, "Build"), None);
}

#[test]
fn remote_workspace_rename_dispatches_and_acknowledges_exactly_once() {
    let controller = FakeRemoteWorkspaceRenameController::default();
    let request = RemoteWorkspaceRenameRequest {
        workspace_id: "workspace-remote".to_string(),
        destination: "dev.example.com".to_string(),
        port: Some(2222),
        identity_file: None,
        ssh_options: Vec::new(),
        session: Some("cmux-remote".to_string()),
        title: "Build".to_string(),
    };

    dispatch_remote_workspace_rename(&controller, &request).unwrap();

    assert_eq!(controller.calls.lock().unwrap().as_slice(), &[request]);
}

#[test]
fn selected_workspace_identity_witness_survives_stale_index_and_restore() {
    let mut snapshot = initial_snapshot("surface-1");
    let selected_id = snapshot.windows[0]
        .selected_workspace_id
        .clone()
        .expect("selected identity witness");
    snapshot.windows[0].tab_manager.selected_workspace_index = Some(99);

    let restored: AppSessionSnapshot =
        serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).unwrap();

    assert_eq!(restored.windows[0].selected_workspace_id, Some(selected_id));
    assert_eq!(
        restored.windows[0].tab_manager.selected_workspace_index,
        Some(99)
    );
}

#[test]
fn selected_workspace_identity_witness_tracks_create_select_and_close() {
    let mut snapshot = initial_snapshot("surface-1");
    let first_id = snapshot.windows[0].selected_workspace_id.clone().unwrap();

    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    let second_id = snapshot.windows[0].tab_manager.workspaces[1]
        .workspace_id
        .clone()
        .unwrap();
    assert_eq!(
        snapshot.windows[0].selected_workspace_id,
        Some(second_id.clone())
    );

    assert!(apply_select_workspace(&mut snapshot, 0));
    assert_eq!(snapshot.windows[0].selected_workspace_id, Some(first_id));

    assert!(apply_close_workspace(&mut snapshot, 0));
    assert_eq!(snapshot.windows[0].selected_workspace_id, Some(second_id));
}

#[test]
fn close_then_reopen_workspace_restores_complete_snapshot() {
    let mut snapshot = initial_snapshot("surface-1");
    let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
    workspace.custom_title = Some("Restored".to_string());
    workspace.initial_terminal_input = Some("scrollback fixture".to_string());
    workspace.workspace_environment = Some(BTreeMap::from([(
        "TOKEN".to_string(),
        "preserved".to_string(),
    )]));
    let closed = closed_workspace_snapshot(&snapshot, 0, 0).unwrap();
    snapshot.windows[0].tab_manager.workspaces.clear();

    assert!(apply_reopen_closed_workspace(&mut snapshot, closed));
    let reopened = &snapshot.windows[0].tab_manager.workspaces[0];
    assert_eq!(reopened.custom_title.as_deref(), Some("Restored"));
    assert_eq!(
        reopened.initial_terminal_input.as_deref(),
        Some("scrollback fixture")
    );
    assert_eq!(
        reopened
            .workspace_environment
            .as_ref()
            .and_then(|env| env.get("TOKEN"))
            .map(String::as_str),
        Some("preserved")
    );
    assert!(reopened.layout.is_some());
}

#[test]
fn close_teardown_plan_covers_workspace_owned_state() {
    let workspace_id = "workspace-a";
    let mut workspace = initial_snapshot("surface-1").windows[0]
        .tab_manager
        .workspaces[0]
        .clone();
    workspace.remote = Some(SessionWorkspaceRemoteSnapshot {
        enabled: true,
        state: "connected".to_string(),
        connected: true,
        transport: Some("tmux".to_string()),
        destination: Some("remote".to_string()),
        port: None,
        local_proxy_port: None,
        persistent_daemon_slot: None,
        has_ssh_options: false,
        detail: None,
        daemon: None,
        proxy: None,
        detected_ports: Vec::new(),
        forwarded_ports: Vec::new(),
        conflicted_ports: Vec::new(),
        active_terminal_sessions: Some(1),
    });
    assert_eq!(
        workspace_close_teardown_plan(workspace_id, &workspace),
        WorkspaceCloseTeardownPlan {
            workspace_id: workspace_id.to_string(),
            panel_ids: vec!["surface-1".to_string()],
            clear_notifications: true,
            clear_metadata: true,
            clear_focus_history: true,
            stop_remote: true,
        }
    );
}

#[test]
fn same_title_rename_is_resolved_without_model_change() {
    let mut tabs = initial_snapshot("surface-1").windows.remove(0).tab_manager;
    tabs.workspaces[0].custom_title = Some("Build".to_string());
    tabs.workspaces[0].custom_title_source = Some("user".to_string());

    assert_eq!(
        rename_workspace_resolution(&mut tabs, 0, "Build"),
        WorkspaceRenameResolution::ResolvedUnchanged
    );
}

#[test]
fn apply_new_workspace_carries_initial_terminal_startup_metadata() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(
        &mut snapshot,
        "surface-2",
        Some("C:/repo"),
        Some("ssh example.com"),
        Some("echo ready\r"),
        Some(BTreeMap::from([("CMUX_FORK".to_string(), "1".to_string())])),
    );
    let workspace = &tab_manager(&snapshot).workspaces[1];
    assert_eq!(workspace.current_directory.as_deref(), Some("C:/repo"));
    assert_eq!(
        workspace.initial_terminal_command.as_deref(),
        Some("ssh example.com")
    );
    assert_eq!(
        workspace.initial_terminal_input.as_deref(),
        Some("echo ready\r")
    );
    assert_eq!(
        workspace
            .initial_terminal_environment
            .as_ref()
            .and_then(|environment| environment.get("CMUX_FORK"))
            .map(String::as_str),
        Some("1")
    );
}

#[test]
fn apply_move_panel_to_new_workspace_mints_destination_ids() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));

    assert!(apply_move_panel_to_new_workspace(
        &mut snapshot,
        "surface-2"
    ));

    let tabs = tab_manager(&snapshot);
    assert_eq!(tabs.workspaces.len(), 2);
    assert_eq!(tabs.selected_workspace_index, Some(1));
    assert!(Uuid::parse_str(tabs.workspaces[1].workspace_id.as_deref().unwrap()).is_ok());
    let pane_ids = pane_ids_in_layout(tabs.workspaces[1].layout.as_ref().unwrap());
    assert_eq!(pane_ids.len(), 1);
    assert!(Uuid::parse_str(pane_ids[0]).is_ok());
}

#[test]
fn apply_close_workspace_removes_and_reclamps_selection() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    apply_new_workspace(&mut snapshot, "surface-3", None, None, None, None); // 3 workspaces, sel=2
                                                                             // Close the first: selection (2) shifts left to 1.
    assert!(apply_close_workspace(&mut snapshot, 0));
    let tabs = tab_manager(&snapshot);
    assert_eq!(tabs.workspaces.len(), 2);
    assert_eq!(tabs.selected_workspace_index, Some(1));
}

#[test]
fn apply_close_only_workspace_is_a_noop() {
    // Canonical `guard tabs.count > 1`: closing the sole workspace does
    // nothing (no replace-with-fresh).
    let mut snapshot = initial_snapshot("surface-1");
    assert!(!apply_close_workspace(&mut snapshot, 0));
    let tabs = tab_manager(&snapshot);
    assert_eq!(tabs.workspaces.len(), 1);
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn apply_rename_workspace_sets_custom_title_and_user_source() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_rename_workspace(&mut snapshot, 0, "X"));
    let ws = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(ws.custom_title.as_deref(), Some("X"));
    assert_eq!(ws.custom_title_source.as_deref(), Some("user"));
    // Identical title again → false (drives the emit gate).
    assert!(!apply_rename_workspace(&mut snapshot, 0, "X"));
}

#[test]
fn apply_rename_workspace_empty_title_clears() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_rename_workspace(&mut snapshot, 0, "X"));
    assert!(apply_rename_workspace(&mut snapshot, 0, ""));
    let ws = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(ws.custom_title, None);
    assert_eq!(ws.custom_title_source, None);
}

#[test]
fn apply_rename_workspace_out_of_range_index_is_a_noop() {
    let mut snapshot = initial_snapshot("surface-1");
    let before = snapshot.clone();
    assert!(!apply_rename_workspace(&mut snapshot, 5, "nope"));
    assert_eq!(snapshot, before);
}

#[test]
fn apply_set_workspace_description_normalizes_and_clears() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_workspace_description(
        &mut snapshot,
        0,
        "alpha\r\nbeta\rgamma"
    ));
    assert_eq!(
        tab_manager(&snapshot).workspaces[0]
            .custom_description
            .as_deref(),
        Some("alpha\nbeta\ngamma")
    );
    assert!(apply_set_workspace_description(&mut snapshot, 0, " \n\t "));
    assert_eq!(
        tab_manager(&snapshot).workspaces[0].custom_description,
        None
    );
}

#[test]
fn apply_set_workspace_description_out_of_range_index_is_a_noop() {
    let mut snapshot = initial_snapshot("surface-1");
    let before = snapshot.clone();
    assert!(!apply_set_workspace_description(&mut snapshot, 5, "nope"));
    assert_eq!(snapshot, before);
}

#[test]
fn apply_reset_workspace_color_clears_custom_color() {
    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].tab_manager.workspaces[0].custom_color = Some("#C0392B".to_string());
    assert!(apply_reset_workspace_color(&mut snapshot, 0));
    assert_eq!(tab_manager(&snapshot).workspaces[0].custom_color, None);
    assert!(!apply_reset_workspace_color(&mut snapshot, 0));
}

#[test]
fn apply_reset_workspace_color_out_of_range_index_is_a_noop() {
    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].tab_manager.workspaces[0].custom_color = Some("#C0392B".to_string());
    let before = snapshot.clone();
    assert!(!apply_reset_workspace_color(&mut snapshot, 5));
    assert_eq!(snapshot, before);
}

#[test]
fn apply_set_panel_title_sets_and_clears_active_workspace_panel_title() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_panel_title(
        &mut snapshot,
        "surface-1",
        " api logs "
    ));
    let titles = tab_manager(&snapshot).workspaces[0]
        .panel_titles
        .as_ref()
        .expect("title metadata");
    assert_eq!(titles[0].panel_id, "surface-1");
    assert_eq!(titles[0].custom_title.as_deref(), Some("api logs"));

    assert!(!apply_set_panel_title(
        &mut snapshot,
        "surface-1",
        "api logs"
    ));
    assert!(apply_set_panel_title(&mut snapshot, "surface-1", ""));
    assert_eq!(tab_manager(&snapshot).workspaces[0].panel_titles, None);
}

#[test]
fn apply_set_panel_title_missing_panel_is_a_noop() {
    let mut snapshot = initial_snapshot("surface-1");
    let before = snapshot.clone();
    assert!(!apply_set_panel_title(&mut snapshot, "missing", "api logs"));
    assert_eq!(snapshot, before);
}

#[test]
fn apply_set_panel_pinned_sets_and_clears_active_workspace_panel_pin() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_panel_pinned(&mut snapshot, "surface-1", true));
    let pins = tab_manager(&snapshot).workspaces[0]
        .panel_pins
        .as_ref()
        .expect("pin metadata");
    assert_eq!(pins[0].panel_id, "surface-1");
    assert!(pins[0].is_pinned);

    assert!(!apply_set_panel_pinned(&mut snapshot, "surface-1", true));
    assert!(apply_set_panel_pinned(&mut snapshot, "surface-1", false));
    assert_eq!(tab_manager(&snapshot).workspaces[0].panel_pins, None);
}

#[test]
fn apply_set_panel_pinned_missing_panel_is_a_noop() {
    let mut snapshot = initial_snapshot("surface-1");
    let before = snapshot.clone();
    assert!(!apply_set_panel_pinned(&mut snapshot, "missing", true));
    assert_eq!(snapshot, before);
}

#[test]
fn apply_set_panel_unread_sets_and_clears_active_workspace_panel_unread() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_panel_unread_at(
        &mut snapshot,
        "surface-1",
        true,
        123
    ));
    let unreads = tab_manager(&snapshot).workspaces[0]
        .panel_unreads
        .as_ref()
        .expect("unread metadata");
    assert_eq!(unreads[0].panel_id, "surface-1");
    assert!(unreads[0].is_unread);
    assert_eq!(unreads[0].unread_at, Some(123));

    assert!(!apply_set_panel_unread(&mut snapshot, "surface-1", true));
    assert!(apply_set_panel_unread(&mut snapshot, "surface-1", false));
    assert_eq!(tab_manager(&snapshot).workspaces[0].panel_unreads, None);
}

#[test]
fn apply_set_panel_unread_missing_panel_is_a_noop() {
    let mut snapshot = initial_snapshot("surface-1");
    let before = snapshot.clone();
    assert!(!apply_set_panel_unread(&mut snapshot, "missing", true));
    assert_eq!(snapshot, before);
}

#[test]
fn apply_set_panel_listening_ports_updates_panel_and_workspace_aggregate() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );

    assert!(apply_set_panel_listening_ports(
        &mut snapshot,
        0,
        "surface-2",
        &[5173, 3000, 5173],
    ));
    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(workspace.listening_ports, Some(vec![3000, 5173]));
    assert_eq!(
        workspace
            .panel_listening_ports
            .as_ref()
            .unwrap()
            .iter()
            .find(|entry| entry.panel_id == "surface-2")
            .map(|entry| entry.ports.clone()),
        Some(vec![3000, 5173])
    );
}

#[test]
fn apply_set_panel_listening_ports_clears_and_recomputes_aggregate() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );
    assert!(apply_set_panel_listening_ports(
        &mut snapshot,
        0,
        "surface-1",
        &[3000],
    ));
    assert!(apply_set_panel_listening_ports(
        &mut snapshot,
        0,
        "surface-2",
        &[5173],
    ));
    assert!(apply_set_panel_listening_ports(
        &mut snapshot,
        0,
        "surface-1",
        &[],
    ));

    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(workspace.listening_ports, Some(vec![5173]));
    assert_eq!(
        workspace
            .panel_listening_ports
            .as_ref()
            .unwrap()
            .iter()
            .map(|entry| entry.panel_id.as_str())
            .collect::<Vec<_>>(),
        vec!["surface-2"]
    );
}

#[test]
fn apply_set_panel_tty_upserts_and_sorts_by_panel() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );

    assert!(apply_set_panel_tty(
        &mut snapshot,
        0,
        "surface-2",
        "ttys002"
    ));
    assert!(apply_set_panel_tty(
        &mut snapshot,
        0,
        "surface-1",
        "ttys001"
    ));
    assert!(apply_set_panel_tty(
        &mut snapshot,
        0,
        "surface-2",
        "/dev/pts/7"
    ));
    assert!(!apply_set_panel_tty(&mut snapshot, 0, "missing", "ttys009"));
    assert!(!apply_set_panel_tty(&mut snapshot, 0, "surface-1", " "));

    let ttys = tab_manager(&snapshot).workspaces[0]
        .panel_ttys
        .as_ref()
        .expect("panel ttys set");
    assert_eq!(
        ttys.iter()
            .map(|entry| (entry.panel_id.as_str(), entry.tty.as_str()))
            .collect::<Vec<_>>(),
        vec![("surface-1", "ttys001"), ("surface-2", "/dev/pts/7")]
    );
}

#[test]
fn apply_set_panel_shell_activity_upserts_and_sorts_by_panel() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );

    assert!(apply_set_panel_shell_activity(
        &mut snapshot,
        0,
        "surface-2",
        SessionPanelShellActivityStateSnapshot::CommandRunning,
    ));
    assert!(apply_set_panel_shell_activity(
        &mut snapshot,
        0,
        "surface-1",
        SessionPanelShellActivityStateSnapshot::PromptIdle,
    ));
    assert!(apply_set_panel_shell_activity(
        &mut snapshot,
        0,
        "surface-2",
        SessionPanelShellActivityStateSnapshot::Unknown,
    ));
    assert!(!apply_set_panel_shell_activity(
        &mut snapshot,
        0,
        "missing",
        SessionPanelShellActivityStateSnapshot::PromptIdle,
    ));

    let activity = tab_manager(&snapshot).workspaces[0]
        .panel_shell_activity
        .as_ref()
        .expect("panel shell activity set");
    assert_eq!(
        activity
            .iter()
            .map(|entry| (entry.panel_id.as_str(), entry.state.clone()))
            .collect::<Vec<_>>(),
        vec![
            (
                "surface-1",
                SessionPanelShellActivityStateSnapshot::PromptIdle
            ),
            ("surface-2", SessionPanelShellActivityStateSnapshot::Unknown),
        ]
    );
}

#[test]
fn apply_set_workspace_agent_listening_ports_unions_with_panel_ports() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_panel_listening_ports(
        &mut snapshot,
        0,
        "surface-1",
        &[5173, 3000],
    ));
    assert!(apply_set_workspace_agent_listening_ports(
        &mut snapshot,
        0,
        &[7000, 3000, 7000],
    ));

    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(workspace.agent_listening_ports, Some(vec![3000, 7000]));
    assert_eq!(workspace.listening_ports, Some(vec![3000, 5173, 7000]));

    assert!(apply_set_workspace_agent_listening_ports(
        &mut snapshot,
        0,
        &[],
    ));
    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(workspace.agent_listening_ports, None);
    assert_eq!(workspace.listening_ports, Some(vec![3000, 5173]));
}

#[test]
fn apply_set_and_clear_workspace_agent_pid_updates_ownership_facts() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_workspace_agent_pid(
        &mut snapshot,
        0,
        "codex.session-2",
        2222,
    ));
    assert!(apply_set_workspace_agent_pid(
        &mut snapshot,
        0,
        "codex.session-1",
        1111,
    ));
    assert!(apply_set_workspace_agent_pid(
        &mut snapshot,
        0,
        "codex.session-2",
        3333,
    ));

    let pids = tab_manager(&snapshot).workspaces[0]
        .agent_pids
        .as_ref()
        .expect("agent pids set");
    assert_eq!(
        pids.iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        vec!["codex.session-1", "codex.session-2"]
    );
    assert_eq!(pids[1].pid, 3333);

    assert!(apply_clear_workspace_agent_pid(
        &mut snapshot,
        0,
        "codex.session-2",
    ));
    let pids = tab_manager(&snapshot).workspaces[0]
        .agent_pids
        .as_ref()
        .expect("one pid remains");
    assert_eq!(pids.len(), 1);
    assert_eq!(pids[0].key, "codex.session-1");

    assert!(apply_clear_workspace_agent_pid(
        &mut snapshot,
        0,
        "codex.session-1",
    ));
    assert_eq!(tab_manager(&snapshot).workspaces[0].agent_pids, None);
}

#[test]
fn apply_set_workspace_git_facts_sets_and_clears_badge_metadata() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );

    assert!(apply_set_workspace_git_facts(
        &mut snapshot,
        0,
        Some(SessionGitBranchSnapshot {
            branch: "feature/api".to_string(),
            is_dirty: true,
        }),
        vec![
            SessionPanelGitBranchSnapshot {
                panel_id: "surface-2".to_string(),
                branch: "feature/api".to_string(),
                is_dirty: true,
            },
            SessionPanelGitBranchSnapshot {
                panel_id: "surface-1".to_string(),
                branch: "feature/api".to_string(),
                is_dirty: true,
            },
        ],
        vec![SessionPanelPullRequestSnapshot {
            panel_id: "surface-2".to_string(),
            number: 42,
            label: "manaflow-ai/cmux".to_string(),
            url: "https://github.com/manaflow-ai/cmux/pull/42".to_string(),
            status: cmux_core::session::SessionPullRequestStatusSnapshot::Open,
            branch: Some("feature/api".to_string()),
            is_stale: false,
        }],
    ));

    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(
        workspace.git_branch,
        Some(SessionGitBranchSnapshot {
            branch: "feature/api".to_string(),
            is_dirty: true,
        })
    );
    assert_eq!(
        workspace
            .panel_git_branches
            .as_ref()
            .unwrap()
            .iter()
            .map(|entry| entry.panel_id.as_str())
            .collect::<Vec<_>>(),
        vec!["surface-1", "surface-2"]
    );
    assert_eq!(
        workspace.panel_pull_requests.as_ref().unwrap()[0].url,
        "https://github.com/manaflow-ai/cmux/pull/42"
    );

    assert!(apply_set_workspace_git_facts(
        &mut snapshot,
        0,
        None,
        Vec::new(),
        Vec::new(),
    ));
    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(workspace.git_branch, None);
    assert_eq!(workspace.panel_git_branches, None);
    assert_eq!(workspace.panel_pull_requests, None);
}

#[test]
fn apply_workspace_panel_pull_request_upserts_and_clears_one_panel() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );

    assert!(apply_set_workspace_panel_pull_request(
        &mut snapshot,
        0,
        "surface-2",
        42,
        "MR",
        "https://gitlab.example/project/-/merge_requests/42",
        SessionPullRequestStatusSnapshot::Open,
        Some("feature/api".to_string()),
        false,
    ));
    assert!(apply_set_workspace_panel_pull_request(
        &mut snapshot,
        0,
        "surface-1",
        7,
        "PR",
        "https://github.com/manaflow-ai/cmux/pull/7",
        SessionPullRequestStatusSnapshot::Merged,
        None,
        false,
    ));
    assert!(apply_set_workspace_panel_pull_request(
        &mut snapshot,
        0,
        "surface-2",
        43,
        "Review",
        "https://example.test/reviews/43",
        SessionPullRequestStatusSnapshot::Closed,
        None,
        true,
    ));

    let requests = tab_manager(&snapshot).workspaces[0]
        .panel_pull_requests
        .as_ref()
        .expect("pull requests set");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].panel_id, "surface-1");
    assert_eq!(requests[1].panel_id, "surface-2");
    assert_eq!(requests[1].number, 43);
    assert_eq!(requests[1].label, "Review");
    assert_eq!(requests[1].status, SessionPullRequestStatusSnapshot::Closed);
    assert!(requests[1].is_stale);

    assert!(apply_clear_workspace_panel_pull_request(
        &mut snapshot,
        0,
        "surface-2"
    ));
    let requests = tab_manager(&snapshot).workspaces[0]
        .panel_pull_requests
        .as_ref()
        .expect("one pull request remains");
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].panel_id, "surface-1");

    assert!(apply_clear_workspace_panel_pull_request(
        &mut snapshot,
        0,
        "surface-1"
    ));
    assert_eq!(
        tab_manager(&snapshot).workspaces[0].panel_pull_requests,
        None
    );
}

#[test]
fn apply_close_prunes_panel_listening_ports() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );
    assert!(apply_set_panel_listening_ports(
        &mut snapshot,
        0,
        "surface-1",
        &[3000],
    ));
    assert!(apply_set_panel_listening_ports(
        &mut snapshot,
        0,
        "surface-2",
        &[5173],
    ));

    assert_eq!(
        apply_close(&mut snapshot, "surface-2"),
        CloseOutcome::Removed
    );
    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(workspace.listening_ports, Some(vec![3000]));
    assert_eq!(
        workspace
            .panel_listening_ports
            .as_ref()
            .unwrap()
            .iter()
            .map(|entry| entry.panel_id.as_str())
            .collect::<Vec<_>>(),
        vec!["surface-1"]
    );
}

#[test]
fn apply_close_prunes_panel_ttys() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );
    assert!(apply_set_panel_tty(
        &mut snapshot,
        0,
        "surface-1",
        "ttys001"
    ));
    assert!(apply_set_panel_tty(
        &mut snapshot,
        0,
        "surface-2",
        "ttys002"
    ));

    assert_eq!(
        apply_close(&mut snapshot, "surface-2"),
        CloseOutcome::Removed
    );
    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(
        workspace
            .panel_ttys
            .as_ref()
            .unwrap()
            .iter()
            .map(|entry| (entry.panel_id.as_str(), entry.tty.as_str()))
            .collect::<Vec<_>>(),
        vec![("surface-1", "ttys001")]
    );
}

#[test]
fn apply_close_prunes_panel_shell_activity() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    );
    assert!(apply_set_panel_shell_activity(
        &mut snapshot,
        0,
        "surface-1",
        SessionPanelShellActivityStateSnapshot::PromptIdle,
    ));
    assert!(apply_set_panel_shell_activity(
        &mut snapshot,
        0,
        "surface-2",
        SessionPanelShellActivityStateSnapshot::CommandRunning,
    ));

    assert_eq!(
        apply_close(&mut snapshot, "surface-2"),
        CloseOutcome::Removed
    );
    let workspace = &tab_manager(&snapshot).workspaces[0];
    assert_eq!(
        workspace
            .panel_shell_activity
            .as_ref()
            .unwrap()
            .iter()
            .map(|entry| (entry.panel_id.as_str(), entry.state.clone()))
            .collect::<Vec<_>>(),
        vec![(
            "surface-1",
            SessionPanelShellActivityStateSnapshot::PromptIdle
        )]
    );
}

#[test]
fn restorable_agent_snapshot_upserts_for_the_scoped_panel() {
    let mut snapshot = initial_snapshot("surface-1");
    let workspace_id = tab_manager(&snapshot).workspaces[0]
        .workspace_id
        .clone()
        .expect("workspace id");
    let started = StartedAgentSessionSnapshot {
        panel_id: "surface-1".to_string(),
        workspace_id: Some(workspace_id.clone()),
        provider_id: "codex".to_string(),
        session_id: "codex-session-1".to_string(),
        executable_path: "C:\\Program Files\\Codex\\codex.exe".to_string(),
        arguments: vec!["app-server".to_string()],
        working_directory: Some("C:\\repo".to_string()),
    };
    let restorable = restorable_snapshot_from_started(&started);
    assert!(apply_restorable_agent_snapshot(
        &mut snapshot,
        started.workspace_id.as_deref(),
        &started.panel_id,
        restorable.clone(),
    ));
    let entries = tab_manager(&snapshot).workspaces[0]
        .restorable_agent_snapshots
        .as_ref()
        .expect("agent snapshot");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].panel_id, "surface-1");
    assert_eq!(entries[0].snapshot.kind, "codex");
    assert_eq!(entries[0].snapshot.session_id, "codex-session-1");
    assert_eq!(
        entries[0].snapshot.fork_command.as_deref(),
        Some("& 'C:\\Program Files\\Codex\\codex.exe' fork 'codex-session-1'")
    );

    assert!(!apply_restorable_agent_snapshot(
        &mut snapshot,
        Some(&workspace_id),
        "surface-1",
        restorable,
    ));
}

#[test]
fn restorable_agent_snapshot_rejects_wrong_workspace_scope() {
    let mut snapshot = initial_snapshot("surface-1");
    let started = StartedAgentSessionSnapshot {
        panel_id: "surface-1".to_string(),
        workspace_id: Some("workspace-missing".to_string()),
        provider_id: "claude".to_string(),
        session_id: "claude-session-1".to_string(),
        executable_path: "claude".to_string(),
        arguments: Vec::new(),
        working_directory: None,
    };
    assert!(!apply_restorable_agent_snapshot(
        &mut snapshot,
        started.workspace_id.as_deref(),
        &started.panel_id,
        restorable_snapshot_from_started(&started),
    ));
    assert_eq!(
        tab_manager(&snapshot).workspaces[0].restorable_agent_snapshots,
        None
    );
}

#[test]
fn apply_set_workspace_unread_sets_and_clears_selected_workspace() {
    let mut snapshot = initial_snapshot("surface-1");

    assert!(apply_set_workspace_unread_at(
        &mut snapshot,
        0,
        Some("surface-1"),
        true,
        456
    ));
    let unreads = tab_manager(&snapshot).workspaces[0]
        .panel_unreads
        .as_ref()
        .expect("unread metadata");
    assert_eq!(unreads[0].panel_id, "surface-1");
    assert!(unreads[0].is_unread);
    assert_eq!(unreads[0].unread_at, Some(456));

    assert!(!apply_set_workspace_unread(
        &mut snapshot,
        0,
        Some("surface-1"),
        true
    ));
    assert!(apply_set_workspace_unread(&mut snapshot, 0, None, false));
    assert_eq!(tab_manager(&snapshot).workspaces[0].panel_unreads, None);
}

#[test]
fn apply_set_workspace_unread_missing_workspace_is_a_noop() {
    let mut snapshot = initial_snapshot("surface-1");
    let before = snapshot.clone();
    assert!(!apply_set_workspace_unread(
        &mut snapshot,
        2,
        Some("surface-1"),
        true
    ));
    assert_eq!(snapshot, before);
}

#[test]
fn apply_set_workspace_pinned_reorders_and_selection_follows() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None); // 2 workspaces, sel=1
    assert!(apply_set_workspace_pinned(&mut snapshot, 1, true));
    let tabs = tab_manager(&snapshot);
    // The pinned workspace floats to the top; selection follows it.
    assert_eq!(tabs.workspaces[0].is_pinned, Some(true));
    assert_eq!(tabs.workspaces[1].is_pinned, None);
    assert_eq!(tabs.selected_workspace_index, Some(0));
    assert_eq!(count_leaves(active_layout(&snapshot)), 1);
}

#[test]
fn apply_set_workspace_pinned_already_at_value_is_a_noop() {
    let mut snapshot = initial_snapshot("surface-1");
    assert!(apply_set_workspace_pinned(&mut snapshot, 0, true));
    let before = snapshot.clone();
    // Already pinned → false (drives the emit gate), snapshot unchanged.
    assert!(!apply_set_workspace_pinned(&mut snapshot, 0, true));
    assert_eq!(snapshot, before);
}

#[test]
fn apply_reorder_workspaces_moves_and_selection_follows() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    apply_new_workspace(&mut snapshot, "surface-3", None, None, None, None); // 3 workspaces, sel=2
    assert!(apply_reorder_workspaces(&mut snapshot, 2, 0, false));
    let tabs = tab_manager(&snapshot);
    // The mover lands at index 0 and the index-based selection follows it.
    if let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = &tabs.workspaces[0].layout {
        assert_eq!(pane.panel_ids, ["surface-3"]);
    } else {
        panic!("expected a pane");
    }
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn apply_reorder_workspaces_no_op_is_gated() {
    // Out-of-range mover / clamp-back-to-place both report false (drives
    // the emit gate) and leave the snapshot untouched.
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    let before = snapshot.clone();
    assert!(!apply_reorder_workspaces(&mut snapshot, 5, 0, false));
    assert!(!apply_reorder_workspaces(&mut snapshot, 1, 999, false)); // clamps to 1
    assert_eq!(snapshot, before);
}

#[test]
fn apply_set_group_collapsed_flips_the_flag() {
    let mut snapshot = initial_snapshot("surface-1");
    snapshot.windows[0].tab_manager.workspace_groups =
        Some(vec![cmux_core::session::SessionWorkspaceGroupSnapshot {
            id: "g".to_string(),
            name: "G".to_string(),
            ..Default::default()
        }]);
    assert!(apply_set_group_collapsed(&mut snapshot, "g", true));
    let groups = tab_manager(&snapshot).workspace_groups.as_ref().unwrap();
    assert!(groups[0].is_collapsed);
    // Already at the requested value → false (drives the emit gate).
    assert!(!apply_set_group_collapsed(&mut snapshot, "g", true));
}

#[test]
fn apply_set_group_collapsed_unknown_group_is_a_noop() {
    let mut snapshot = initial_snapshot("surface-1");
    let before = snapshot.clone();
    assert!(!apply_set_group_collapsed(&mut snapshot, "g", true));
    assert_eq!(snapshot, before);
}

#[test]
fn delete_workspace_group_candidate_closes_exact_members() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    apply_new_workspace(&mut snapshot, "surface-3", None, None, None, None);
    let group_id = Uuid::new_v4().to_string();
    let anchor_id = snapshot.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone()
        .expect("anchor id");
    for workspace in &mut snapshot.windows[0].tab_manager.workspaces[..2] {
        workspace.group_id = Some(group_id.clone());
    }
    snapshot.windows[0].tab_manager.workspace_groups =
        Some(vec![cmux_core::session::SessionWorkspaceGroupSnapshot {
            id: group_id.clone(),
            name: "Group".to_string(),
            anchor_workspace_id: Some(anchor_id.clone()),
            ..Default::default()
        }]);
    let survivor_id = snapshot.windows[0].tab_manager.workspaces[2]
        .workspace_id
        .clone();

    let (artifacts, changed) = apply_delete_workspace_group_candidate(&mut snapshot, 0, &group_id);

    let artifacts = artifacts.expect("group existed");
    assert!(changed);
    assert_eq!(artifacts.closed_count, 2);
    assert_eq!(artifacts.closed_workspaces.len(), 2);
    assert_eq!(
        artifacts
            .closed_workspaces
            .last()
            .and_then(|closed| closed.workspace.workspace_id.as_deref()),
        Some(anchor_id.as_str())
    );
    assert_eq!(snapshot.windows[0].tab_manager.workspaces.len(), 1);
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0].workspace_id,
        survivor_id
    );
    assert!(snapshot.windows[0]
        .tab_manager
        .workspace_groups
        .as_deref()
        .unwrap_or_default()
        .is_empty());
}

#[test]
fn delete_final_workspace_group_seeds_one_ungrouped_replacement() {
    let mut snapshot = initial_snapshot("surface-1");
    apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
    let group_id = Uuid::new_v4().to_string();
    let anchor_id = snapshot.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone()
        .expect("anchor id");
    snapshot.windows[0].tab_manager.workspaces[0].current_directory = Some("C:/repo".to_string());
    let removed_ids = snapshot.windows[0]
        .tab_manager
        .workspaces
        .iter_mut()
        .map(|workspace| {
            workspace.group_id = Some(group_id.clone());
            workspace.workspace_id.clone().expect("workspace id")
        })
        .collect::<HashSet<_>>();
    snapshot.windows[0].tab_manager.workspace_groups =
        Some(vec![cmux_core::session::SessionWorkspaceGroupSnapshot {
            id: group_id.clone(),
            name: "Only group".to_string(),
            anchor_workspace_id: Some(anchor_id),
            ..Default::default()
        }]);

    let (artifacts, changed) = apply_delete_workspace_group_candidate(&mut snapshot, 0, &group_id);

    let artifacts = artifacts.expect("group existed");
    let tabs = &snapshot.windows[0].tab_manager;
    assert!(changed);
    assert_eq!(artifacts.closed_count, 2);
    assert_eq!(tabs.workspaces.len(), 1);
    assert_eq!(tabs.selected_workspace_index, Some(0));
    assert_eq!(tabs.workspaces[0].group_id, None);
    assert_eq!(
        tabs.workspaces[0].current_directory.as_deref(),
        Some("C:/repo")
    );
    assert!(!removed_ids.contains(
        tabs.workspaces[0]
            .workspace_id
            .as_deref()
            .expect("replacement id")
    ));
    assert!(tabs
        .workspace_groups
        .as_deref()
        .unwrap_or_default()
        .is_empty());
}

#[test]
fn delete_memberless_workspace_group_removes_the_persisted_record() {
    let mut snapshot = initial_snapshot("surface-1");
    let original_workspace_id = snapshot.windows[0].tab_manager.workspaces[0]
        .workspace_id
        .clone();
    let group_id = Uuid::new_v4().to_string();
    snapshot.windows[0].tab_manager.workspace_groups =
        Some(vec![cmux_core::session::SessionWorkspaceGroupSnapshot {
            id: group_id.clone(),
            name: "Orphan".to_string(),
            anchor_workspace_id: Some(Uuid::new_v4().to_string()),
            ..Default::default()
        }]);

    let (artifacts, changed) = apply_delete_workspace_group_candidate(&mut snapshot, 0, &group_id);

    assert!(changed);
    assert_eq!(artifacts.expect("group existed").closed_count, 0);
    assert_eq!(snapshot.windows[0].tab_manager.workspaces.len(), 1);
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0].workspace_id,
        original_workspace_id
    );
    assert!(snapshot.windows[0]
        .tab_manager
        .workspace_groups
        .as_deref()
        .unwrap_or_default()
        .is_empty());
}

#[test]
fn snapshot_serializes_with_the_session_changed_shape() {
    // The web bridge parses this exact JSON; assert the round-trip holds and
    // the layout union uses the `{type, pane|split}` wire shape.
    let mut snapshot = initial_snapshot("surface-1");
    apply_split(
        &mut snapshot,
        "surface-1",
        SessionSplitOrientation::Vertical,
        "surface-2",
        false,
    );
    let json = serde_json::to_string(&snapshot).expect("serialize");
    assert!(json.contains("\"type\":\"split\""));
    assert!(json.contains("\"orientation\":\"vertical\""));
    let round: AppSessionSnapshot = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(round, snapshot);
}

#[test]
fn lifecycle_snapshot_replace_overwrites_existing_and_preserves_it_on_failure() {
    let directory = tempfile::tempdir().unwrap();
    let current = directory.path().join("session.json");
    let staged = directory.path().join("session.staged");
    std::fs::write(&current, b"old").unwrap();
    std::fs::write(&staged, b"new").unwrap();
    replace_file_atomically(&staged, &current).unwrap();
    assert_eq!(std::fs::read(&current).unwrap(), b"new");
    assert!(!staged.exists());

    let missing = directory.path().join("missing.staged");
    assert!(replace_file_atomically(&missing, &current).is_err());
    assert_eq!(std::fs::read(&current).unwrap(), b"new");
}

struct TestSnapshotPublicationOperations {
    name: &'static str,
    calls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    persist_entered: Option<std::sync::mpsc::Sender<()>>,
    persist_release: Option<std::sync::mpsc::Receiver<()>>,
    persist_error: Option<String>,
    event_baseline: String,
}

impl SnapshotPublicationOperations for TestSnapshotPublicationOperations {
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:persist", self.name));
        if let Some(entered) = self.persist_entered.take() {
            entered.send(()).unwrap();
        }
        if let Some(release) = self.persist_release.take() {
            release.recv().unwrap();
        }
        if let Some(error) = self.persist_error.take() {
            return Err(error);
        }
        Ok(())
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:baseline", self.name));
        self.event_baseline = candidate.windows[0].tab_manager.workspaces[0]
            .current_directory
            .clone()
            .unwrap_or_default();
    }

    fn emit(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("{}:emit", self.name));
        Ok(())
    }
}

fn publication_candidate(base: &AppSessionSnapshot, marker: &str) -> AppSessionSnapshot {
    let mut candidate = base.clone();
    candidate.windows[0].tab_manager.workspaces[0].current_directory = Some(marker.into());
    candidate
}

#[test]
fn snapshot_publication_gate_covers_persist_authority_baseline_and_emit() {
    let initial = initial_snapshot("surface-1");
    let older = publication_candidate(&initial, "older");
    let newer = publication_candidate(&older, "newer");
    let authority = std::sync::Arc::new(GatedSnapshot::new(initial.clone()));
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (older_entered_tx, older_entered_rx) = std::sync::mpsc::channel();
    let (release_older_tx, release_older_rx) = std::sync::mpsc::channel();
    let (newer_entered_tx, newer_entered_rx) = std::sync::mpsc::channel();

    let older_authority = authority.clone();
    let older_calls = calls.clone();
    let older_initial = initial.clone();
    let older_thread = std::thread::spawn(move || {
        let mut operations = TestSnapshotPublicationOperations {
            name: "older",
            calls: older_calls,
            persist_entered: Some(older_entered_tx),
            persist_release: Some(release_older_rx),
            persist_error: None,
            event_baseline: "initial".into(),
        };
        publish_snapshot_transaction(
            &older_authority,
            Some(&older_initial),
            &older,
            &mut operations,
        )
        .unwrap();
    });
    older_entered_rx.recv().unwrap();

    let newer_authority = authority.clone();
    let newer_calls = calls.clone();
    let newer_expected = publication_candidate(&initial, "older");
    let newer_thread = std::thread::spawn(move || {
        let mut operations = TestSnapshotPublicationOperations {
            name: "newer",
            calls: newer_calls,
            persist_entered: Some(newer_entered_tx),
            persist_release: None,
            persist_error: None,
            event_baseline: "older".into(),
        };
        publish_snapshot_transaction(
            &newer_authority,
            Some(&newer_expected),
            &newer,
            &mut operations,
        )
        .unwrap();
    });

    assert!(matches!(
        newer_entered_rx.recv_timeout(std::time::Duration::from_millis(100)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    release_older_tx.send(()).unwrap();
    older_thread.join().unwrap();
    newer_entered_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    newer_thread.join().unwrap();

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        [
            "older:persist",
            "older:baseline",
            "older:emit",
            "newer:persist",
            "newer:baseline",
            "newer:emit",
        ]
    );
    assert_eq!(
        authority.lock().unwrap().windows[0].tab_manager.workspaces[0]
            .current_directory
            .as_deref(),
        Some("newer")
    );
}

#[test]
fn snapshot_persistence_failure_preserves_authority_and_event_baseline() {
    let initial = initial_snapshot("surface-1");
    let candidate = publication_candidate(&initial, "candidate");
    let authority = GatedSnapshot::new(initial.clone());
    let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut operations = TestSnapshotPublicationOperations {
        name: "failed",
        calls: calls.clone(),
        persist_entered: None,
        persist_release: None,
        persist_error: Some("snapshot write failed".into()),
        event_baseline: "initial".into(),
    };

    assert_eq!(
        publish_snapshot_transaction(&authority, Some(&initial), &candidate, &mut operations)
            .unwrap_err(),
        "snapshot write failed"
    );
    assert_eq!(*authority.lock().unwrap(), initial);
    assert_eq!(operations.event_baseline, "initial");
    assert_eq!(calls.lock().unwrap().as_slice(), ["failed:persist"]);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TestSnapshotWriterFailure {
    CreateDirectory,
    WriteStaged,
    AtomicReplace,
}

struct TestSnapshotFileOperations {
    failure: TestSnapshotWriterFailure,
    calls: Vec<&'static str>,
}

impl SnapshotFileOperations for TestSnapshotFileOperations {
    fn create_parent(&mut self, _parent: &Path) -> Result<(), String> {
        self.calls.push("mkdir");
        (self.failure != TestSnapshotWriterFailure::CreateDirectory)
            .then_some(())
            .ok_or_else(|| "mkdir failed".into())
    }

    fn write_staged(&mut self, _path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.calls.push("write");
        assert!(!bytes.is_empty());
        (self.failure != TestSnapshotWriterFailure::WriteStaged)
            .then_some(())
            .ok_or_else(|| "write failed".into())
    }

    fn atomic_replace(&mut self, _staged: &Path, _destination: &Path) -> Result<(), String> {
        self.calls.push("replace");
        (self.failure != TestSnapshotWriterFailure::AtomicReplace)
            .then_some(())
            .ok_or_else(|| "replace failed".into())
    }
}

#[test]
fn strict_snapshot_writer_surfaces_directory_write_and_replace_failures() {
    let snapshot = initial_snapshot("surface-1");
    let path = Path::new("state/session.json");
    for (failure, expected_error, expected_calls) in [
        (
            TestSnapshotWriterFailure::CreateDirectory,
            "mkdir failed",
            &["mkdir"][..],
        ),
        (
            TestSnapshotWriterFailure::WriteStaged,
            "write failed",
            &["mkdir", "write"][..],
        ),
        (
            TestSnapshotWriterFailure::AtomicReplace,
            "replace failed",
            &["mkdir", "write", "replace"][..],
        ),
    ] {
        let mut operations = TestSnapshotFileOperations {
            failure,
            calls: Vec::new(),
        };
        assert_eq!(
            write_snapshot_file_strict(path, &snapshot, &mut operations).unwrap_err(),
            expected_error
        );
        assert_eq!(operations.calls, expected_calls);
    }
}
