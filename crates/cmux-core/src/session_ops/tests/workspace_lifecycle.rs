// --- Tab-manager (workspace) ops ---

fn one_workspace_tabs(panel_id: &str) -> SessionTabManagerSnapshot {
    SessionTabManagerSnapshot {
        selected_workspace_index: Some(0),
        workspaces: vec![fresh_terminal_workspace(panel_id)],
        workspace_groups: None,
    }
}

/// `n` workspaces (`surface-0`..`surface-{n-1}`), the first `pinned` of them
/// pinned (contiguous prefix, as the sidebar guarantees), selected at
/// `selected`.
fn tabs_with(n: usize, pinned: usize, selected: i64) -> SessionTabManagerSnapshot {
    let workspaces = (0..n)
        .map(|i| SessionWorkspaceSnapshot {
            is_pinned: (i < pinned).then_some(true),
            ..fresh_terminal_workspace(&format!("surface-{i}"))
        })
        .collect();
    SessionTabManagerSnapshot {
        selected_workspace_index: Some(selected),
        workspaces,
        workspace_groups: None,
    }
}

#[test]
fn set_process_title_updates_the_owning_workspace() {
    let mut tabs = one_workspace_tabs("surface-1");
    assert!(set_process_title(&mut tabs, "surface-1", "pwsh — ~/proj"));
    assert_eq!(tabs.workspaces[0].process_title, "pwsh — ~/proj");
}

#[test]
fn set_process_title_trims_and_drops_an_empty_title() {
    let mut tabs = one_workspace_tabs("surface-1");
    // Whitespace-only never clobbers the existing title.
    assert!(!set_process_title(&mut tabs, "surface-1", "   \t "));
    assert_eq!(tabs.workspaces[0].process_title, "Terminal");
    // A padded real title is trimmed on both ends.
    assert!(set_process_title(&mut tabs, "surface-1", "  vim  "));
    assert_eq!(tabs.workspaces[0].process_title, "vim");
}

#[test]
fn set_process_title_is_a_no_op_for_an_unknown_panel() {
    let mut tabs = one_workspace_tabs("surface-1");
    assert!(!set_process_title(&mut tabs, "surface-999", "nope"));
    assert_eq!(tabs.workspaces[0].process_title, "Terminal");
}

#[test]
fn set_process_title_reports_no_change_when_identical() {
    let mut tabs = one_workspace_tabs("surface-1");
    assert!(set_process_title(&mut tabs, "surface-1", "npm run dev"));
    // Same title again → false (nothing changed).
    assert!(!set_process_title(&mut tabs, "surface-1", "npm run dev"));
}

#[test]
fn set_process_title_targets_only_the_workspace_owning_the_panel() {
    // surface-0..surface-2 each in their own workspace.
    let mut tabs = tabs_with(3, 0, 0);
    assert!(set_process_title(&mut tabs, "surface-2", "cargo test"));
    assert_eq!(tabs.workspaces[0].process_title, "Terminal");
    assert_eq!(tabs.workspaces[1].process_title, "Terminal");
    assert_eq!(tabs.workspaces[2].process_title, "cargo test");
}

fn install_surface_records(tabs: &mut SessionTabManagerSnapshot, surface_ids: &[&str]) {
    tabs.workspaces[0].surfaces = Some(
        surface_ids
            .iter()
            .map(|surface_id| {
                serde_json::from_value(serde_json::json!({
                    "surface_id": surface_id,
                    "pane_id": "surface-1",
                    "generation": 1,
                    "kind": {"type": "terminal"},
                    "metadata": {}
                }))
                .expect("surface record")
            })
            .collect(),
    );
}

fn runtime_title(tabs: &SessionTabManagerSnapshot, surface_id: &str) -> Option<String> {
    serde_json::to_value(&tabs.workspaces[0].surfaces)
        .expect("serialize surface records")
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["surface_id"] == surface_id)
                .and_then(|row| row["metadata"]["runtime_title"].as_str())
        })
        .map(str::to_owned)
}

#[test]
fn set_process_title_keeps_exact_surface_authority_and_ignores_blank() {
    let mut tabs = one_workspace_tabs("surface-1");
    install_surface_records(&mut tabs, &["surface-1"]);

    assert!(set_process_title(
        &mut tabs,
        "surface-1",
        "  pwsh — C:/repo  "
    ));
    assert_eq!(tabs.workspaces[0].process_title, "pwsh — C:/repo");
    assert_eq!(
        runtime_title(&tabs, "surface-1").as_deref(),
        Some("pwsh — C:/repo")
    );

    assert!(!set_process_title(&mut tabs, "surface-1", " \r\n\t "));
    assert_eq!(
        runtime_title(&tabs, "surface-1").as_deref(),
        Some("pwsh — C:/repo")
    );
    assert!(!set_process_title(&mut tabs, "unknown", "ignored"));
}

#[test]
fn set_process_title_does_not_replace_multi_panel_or_custom_workspace_title() {
    let mut multi = one_workspace_tabs("surface-1");
    assert!(split_pane(
        multi.workspaces[0].layout.as_mut().unwrap(),
        "surface-1",
        SessionSplitOrientation::Horizontal,
        "surface-2",
        false,
    ));
    install_surface_records(&mut multi, &["surface-1", "surface-2"]);
    assert!(set_process_title(&mut multi, "surface-2", "cargo test"));
    assert_eq!(multi.workspaces[0].process_title, "Terminal");
    assert_eq!(
        runtime_title(&multi, "surface-2").as_deref(),
        Some("cargo test")
    );

    let mut custom = one_workspace_tabs("surface-1");
    custom.workspaces[0].custom_title = Some("Pinned name".into());
    install_surface_records(&mut custom, &["surface-1"]);
    assert!(set_process_title(&mut custom, "surface-1", "npm run dev"));
    assert_eq!(custom.workspaces[0].process_title, "Terminal");
    assert_eq!(
        runtime_title(&custom, "surface-1").as_deref(),
        Some("npm run dev")
    );
}

// Case A: append-when-no-groups / no-pins (AfterCurrent, single tab).
#[test]
fn new_workspace_appends_and_selects_it() {
    let mut tabs = one_workspace_tabs("surface-1");
    new_workspace(&mut tabs, "surface-2");
    assert_eq!(tabs.workspaces.len(), 2);
    assert_eq!(tabs.selected_workspace_index, Some(1));
    assert!(matches!(tabs.workspaces[1].layout, Some(Layout::Pane(_))));
    assert_eq!(tabs.workspaces[1].process_title, "Terminal");
}

// Case B: insert-after-selected (AfterCurrent, middle selection).
#[test]
fn new_workspace_after_current_inserts_after_selected() {
    let mut tabs = tabs_with(4, 0, 1);
    new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::AfterCurrent);
    assert_eq!(tabs.workspaces.len(), 5);
    // Lands between old index-1 and old index-2.
    assert_eq!(
        panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()),
        ["new"]
    );
    assert_eq!(tabs.selected_workspace_index, Some(2));
}

// Case C: End placement appends.
#[test]
fn new_workspace_end_appends() {
    let mut tabs = tabs_with(4, 0, 1);
    new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::End);
    assert_eq!(tabs.workspaces.len(), 5);
    assert_eq!(
        panel_ids(tabs.workspaces[4].layout.as_ref().unwrap()),
        ["new"]
    );
    assert_eq!(tabs.selected_workspace_index, Some(4));
}

// Case D: Top placement lands just after the pinned prefix.
#[test]
fn new_workspace_top_inserts_after_pinned_prefix() {
    // 5 ws, first 2 pinned, selected = 3 (unpinned).
    let mut tabs = tabs_with(5, 2, 3);
    new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::Top);
    assert_eq!(tabs.workspaces.len(), 6);
    // At index 2: just after the pinned prefix, ahead of the unpinned tabs.
    assert_eq!(
        panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()),
        ["new"]
    );
    assert_eq!(tabs.selected_workspace_index, Some(2));
    // Not at the end, and not inside the pinned prefix.
    assert!(tabs.workspaces[0].is_pinned == Some(true));
    assert!(tabs.workspaces[1].is_pinned == Some(true));
}

// Case E: pinned selection under AfterCurrent inserts at the pinned boundary,
// not after itself (mirrors placement.rs pinned-selection test).
#[test]
fn new_workspace_after_current_pinned_selection_inserts_at_boundary() {
    // 5 ws, first 2 pinned, selected = 0 (pinned).
    let mut tabs = tabs_with(5, 2, 0);
    new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::AfterCurrent);
    assert_eq!(tabs.workspaces.len(), 6);
    // Inserts at the pinned boundary (2), not after itself (index 1).
    assert_eq!(
        panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()),
        ["new"]
    );
    assert_eq!(tabs.selected_workspace_index, Some(2));
}

// Case F: into-selected-group — flat placement first lands the new ws
// adjacent to the selected group member, then canonical contiguity moves the
// ungrouped workspace out of the middle of the group run.
#[test]
fn new_workspace_into_selected_group_normalizes_contiguity() {
    let group_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let mut tabs = tabs_with(4, 0, 1);
    // Mark the selected ws (index 1) and its neighbour (index 2) as a group.
    tabs.workspaces[1].group_id = Some(group_id.to_string());
    tabs.workspaces[2].group_id = Some(group_id.to_string());
    tabs.workspace_groups = Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
        id: group_id.to_string(),
        name: "G".to_string(),
        ..Default::default()
    }]);
    new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::AfterCurrent);
    // The fresh workspace is ungrouped. Normalization moves it after the
    // contiguous group run and remaps selection to keep following it.
    assert_eq!(
        panel_ids(tabs.workspaces[3].layout.as_ref().unwrap()),
        ["new"]
    );
    assert_eq!(tabs.selected_workspace_index, Some(3));
    assert_eq!(tabs.workspaces[3].group_id, None);
    assert_eq!(tabs.workspaces[1].group_id.as_deref(), Some(group_id));
    assert_eq!(tabs.workspaces[2].group_id.as_deref(), Some(group_id));
}

// The default two-arg wrapper resolves to AfterCurrent.
#[test]
fn new_workspace_defaults_to_after_current() {
    let mut tabs = tabs_with(4, 0, 1);
    new_workspace(&mut tabs, "new");
    assert_eq!(
        panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()),
        ["new"]
    );
    assert_eq!(tabs.selected_workspace_index, Some(2));
}

#[test]
fn move_panel_to_new_workspace_extracts_panel_and_selects_destination() {
    let mut tabs = one_workspace_tabs("surface-1");
    tabs.workspaces[0].layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("surface-1"),
        pane("surface-2"),
    ));
    tabs.workspaces[0].current_directory = Some("C:/repo".to_string());
    assert!(set_panel_title(&mut tabs.workspaces[0], "surface-2", "api"));
    assert!(set_panel_pinned(&mut tabs.workspaces[0], "surface-2", true));
    assert!(set_panel_unread(&mut tabs.workspaces[0], "surface-2", true));
    tabs.workspaces[0].agent_listening_ports = Some(vec![9000]);
    tabs.workspaces[0].listening_ports = Some(vec![3000, 5173, 9000]);
    tabs.workspaces[0].panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
        panel_id: "surface-2".to_string(),
        ports: vec![3000, 5173],
    }]);
    tabs.workspaces[0].panel_shell_activity = Some(vec![SessionPanelShellActivitySnapshot {
        panel_id: "surface-2".to_string(),
        state: SessionPanelShellActivityStateSnapshot::CommandRunning,
        updated_at: 12,
    }]);

    assert!(move_panel_to_new_workspace(&mut tabs, "surface-2"));

    assert_eq!(tabs.workspaces.len(), 2);
    assert_eq!(tabs.selected_workspace_index, Some(1));
    assert_eq!(tabs.workspaces[1].process_title, "api");
    assert_eq!(
        tabs.workspaces[1].current_directory.as_deref(),
        Some("C:/repo")
    );
    assert!(contains_panel(
        tabs.workspaces[0].layout.as_ref().unwrap(),
        "surface-1"
    ));
    assert!(!contains_panel(
        tabs.workspaces[0].layout.as_ref().unwrap(),
        "surface-2"
    ));
    assert!(contains_panel(
        tabs.workspaces[1].layout.as_ref().unwrap(),
        "surface-2"
    ));
    assert_eq!(
        tabs.workspaces[1].panel_titles.as_ref().unwrap()[0]
            .custom_title
            .as_deref(),
        Some("api")
    );
    assert_eq!(tabs.workspaces[0].panel_titles, None);
    assert!(tabs.workspaces[1].panel_pins.as_ref().unwrap()[0].is_pinned);
    assert!(tabs.workspaces[1].panel_unreads.as_ref().unwrap()[0].is_unread);
    assert_eq!(tabs.workspaces[0].listening_ports, Some(vec![9000]));
    assert_eq!(tabs.workspaces[0].agent_listening_ports, Some(vec![9000]));
    assert_eq!(tabs.workspaces[0].panel_listening_ports, None);
    assert_eq!(tabs.workspaces[1].listening_ports, Some(vec![3000, 5173]));
    assert_eq!(tabs.workspaces[1].agent_listening_ports, None);
    assert_eq!(
        tabs.workspaces[1].panel_listening_ports.as_ref().unwrap()[0].ports,
        vec![3000, 5173]
    );
    assert_eq!(tabs.workspaces[0].panel_shell_activity, None);
    assert_eq!(
        tabs.workspaces[1].panel_shell_activity.as_ref().unwrap()[0].state,
        SessionPanelShellActivityStateSnapshot::CommandRunning
    );
}

#[test]
fn move_panel_to_new_workspace_rejects_the_only_panel() {
    let mut tabs = one_workspace_tabs("surface-1");
    let before = tabs.clone();
    assert!(!move_panel_to_new_workspace(&mut tabs, "surface-1"));
    assert_eq!(tabs, before);
}

#[test]
fn select_workspace_ignores_out_of_range() {
    let mut tabs = one_workspace_tabs("surface-1");
    new_workspace(&mut tabs, "surface-2"); // 2 workspaces, selected = 1
    assert!(select_workspace(&mut tabs, 0));
    assert_eq!(tabs.selected_workspace_index, Some(0));
    assert!(!select_workspace(&mut tabs, 9));
    assert!(!select_workspace(&mut tabs, -1));
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn close_workspace_before_selection_shifts_it_left() {
    let mut tabs = one_workspace_tabs("surface-1");
    new_workspace(&mut tabs, "surface-2");
    new_workspace(&mut tabs, "surface-3"); // 3 workspaces, selected = 2
    assert!(close_workspace(&mut tabs, 0));
    assert_eq!(tabs.workspaces.len(), 2);
    assert_eq!(tabs.selected_workspace_index, Some(1));
}

#[test]
fn close_selected_last_workspace_clamps_selection() {
    let mut tabs = one_workspace_tabs("surface-1");
    new_workspace(&mut tabs, "surface-2"); // 2 workspaces, selected = 1 (the last)
    assert!(close_workspace(&mut tabs, 1));
    assert_eq!(tabs.workspaces.len(), 1);
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn close_only_workspace_is_a_noop() {
    // Canonical `guard tabs.count > 1`: the sole workspace cannot be closed.
    let mut tabs = one_workspace_tabs("surface-1");
    assert!(!close_workspace(&mut tabs, 0));
    assert_eq!(tabs.workspaces.len(), 1);
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn close_workspace_out_of_range_is_rejected() {
    let mut tabs = one_workspace_tabs("surface-1");
    new_workspace(&mut tabs, "surface-2");
    assert!(!close_workspace(&mut tabs, 5));
    assert_eq!(tabs.workspaces.len(), 2);
}

#[test]
fn close_workspace_rejects_pinned_workspace_without_mutation() {
    let mut tabs = one_workspace_tabs("surface-1");
    new_workspace(&mut tabs, "surface-2");
    tabs.workspaces[0].is_pinned = Some(true);
    let before = tabs.clone();

    assert!(!close_workspace(&mut tabs, 0));
    assert_eq!(tabs, before);
}

#[test]
fn new_workspace_inherits_selected_workspace_directory() {
    let mut tabs = one_workspace_tabs("surface-1");
    tabs.workspaces[0].current_directory = Some("C:/inherited".to_string());

    new_workspace(&mut tabs, "surface-2");

    assert_eq!(
        tabs.workspaces[1].current_directory.as_deref(),
        Some("C:/inherited")
    );
}

#[test]
fn close_anchor_workspace_dissolves_its_group() {
    let mut tabs = SessionTabManagerSnapshot {
        selected_workspace_index: Some(0),
        workspaces: vec![
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-anchor".to_string()),
                group_id: Some("g".to_string()),
                ..fresh_terminal_workspace("surface-1")
            },
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-member".to_string()),
                group_id: Some("g".to_string()),
                ..fresh_terminal_workspace("surface-2")
            },
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-solo".to_string()),
                ..fresh_terminal_workspace("surface-3")
            },
        ],
        workspace_groups: Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
            id: "g".to_string(),
            name: "Group".to_string(),
            anchor_workspace_id: Some("ws-anchor".to_string()),
            anchor_member_index: Some(0),
            ..Default::default()
        }]),
    };

    assert!(close_workspace(&mut tabs, 0));
    assert_eq!(tabs.workspaces.len(), 2);
    assert_eq!(
        tabs.workspaces[0].workspace_id.as_deref(),
        Some("ws-member")
    );
    assert_eq!(tabs.workspaces[0].group_id, None);
    assert_eq!(tabs.workspace_groups, None);
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn close_group_member_preserves_group_and_reanchors_member_index() {
    let mut tabs = SessionTabManagerSnapshot {
        selected_workspace_index: Some(2),
        workspaces: vec![
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-anchor".to_string()),
                group_id: Some("g".to_string()),
                ..fresh_terminal_workspace("surface-1")
            },
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-member".to_string()),
                group_id: Some("g".to_string()),
                ..fresh_terminal_workspace("surface-2")
            },
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-solo".to_string()),
                ..fresh_terminal_workspace("surface-3")
            },
        ],
        workspace_groups: Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
            id: "g".to_string(),
            name: "Group".to_string(),
            anchor_workspace_id: Some("ws-anchor".to_string()),
            anchor_member_index: Some(1),
            ..Default::default()
        }]),
    };

    assert!(close_workspace(&mut tabs, 1));
    let group = tabs.workspace_groups.as_ref().expect("group survives");
    assert_eq!(group.len(), 1);
    assert_eq!(group[0].anchor_workspace_id.as_deref(), Some("ws-anchor"));
    assert_eq!(group[0].anchor_member_index, Some(0));
    assert_eq!(tabs.selected_workspace_index, Some(1));
}

#[test]
fn close_workspaces_targets_original_indices_in_tab_order() {
    let mut tabs = SessionTabManagerSnapshot {
        selected_workspace_index: Some(2),
        workspaces: vec![
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-0".to_string()),
                ..fresh_terminal_workspace("surface-0")
            },
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-1".to_string()),
                ..fresh_terminal_workspace("surface-1")
            },
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-2".to_string()),
                ..fresh_terminal_workspace("surface-2")
            },
            SessionWorkspaceSnapshot {
                workspace_id: Some("ws-3".to_string()),
                ..fresh_terminal_workspace("surface-3")
            },
        ],
        workspace_groups: None,
    };

    assert!(close_workspaces(&mut tabs, &[3, 1, 1, 99, -1]));
    assert_eq!(
        tabs.workspaces
            .iter()
            .map(|workspace| workspace.workspace_id.as_deref().unwrap_or(""))
            .collect::<Vec<_>>(),
        ["ws-0", "ws-2"]
    );
    assert_eq!(tabs.selected_workspace_index, Some(1));
}

// --- Workspace rename (custom title) ---

#[test]
fn rename_workspace_sets_custom_title_and_user_source() {
    let mut tabs = one_workspace_tabs("surface-1");
    assert!(rename_workspace(&mut tabs, 0, "Fix auth"));
    let ws = &tabs.workspaces[0];
    assert_eq!(ws.custom_title.as_deref(), Some("Fix auth"));
    assert_eq!(ws.custom_title_source.as_deref(), Some("user"));
    // The process-title fallback is never touched by a rename.
    assert_eq!(ws.process_title, "Terminal");
}

#[test]
fn rename_workspace_trims_padding() {
    let mut tabs = one_workspace_tabs("surface-1");
    assert!(rename_workspace(&mut tabs, 0, "  Fix auth  "));
    assert_eq!(tabs.workspaces[0].custom_title.as_deref(), Some("Fix auth"));
}

#[test]
fn rename_workspace_empty_title_clears_both_fields() {
    let mut tabs = one_workspace_tabs("surface-1");
    tabs.workspaces[0].custom_title = Some("Named".to_string());
    tabs.workspaces[0].custom_title_source = Some("user".to_string());
    assert!(rename_workspace(&mut tabs, 0, ""));
    assert_eq!(tabs.workspaces[0].custom_title, None);
    assert_eq!(tabs.workspaces[0].custom_title_source, None);
}

#[test]
fn rename_workspace_whitespace_only_clears_when_previously_set() {
    let mut tabs = one_workspace_tabs("surface-1");
    tabs.workspaces[0].custom_title = Some("Named".to_string());
    tabs.workspaces[0].custom_title_source = Some("user".to_string());
    assert!(rename_workspace(&mut tabs, 0, "   \t "));
    assert_eq!(tabs.workspaces[0].custom_title, None);
    assert_eq!(tabs.workspaces[0].custom_title_source, None);
}

#[test]
fn rename_workspace_clearing_an_already_clear_title_is_no_change() {
    let mut tabs = one_workspace_tabs("surface-1");
    assert!(!rename_workspace(&mut tabs, 0, ""));
    assert_eq!(tabs.workspaces[0].custom_title, None);
}

#[test]
fn rename_workspace_identical_title_is_no_change() {
    let mut tabs = one_workspace_tabs("surface-1");
    assert!(rename_workspace(&mut tabs, 0, "Fix auth"));
    assert!(!rename_workspace(&mut tabs, 0, "Fix auth"));
}

#[test]
fn rename_workspace_same_title_flips_auto_source_to_user() {
    // An OSC/auto-stamped title renamed to the very same text still counts
    // as a change: the source flips "auto" → "user".
    let mut tabs = one_workspace_tabs("surface-1");
    tabs.workspaces[0].custom_title = Some("Fix auth".to_string());
    tabs.workspaces[0].custom_title_source = Some("auto".to_string());
    assert!(rename_workspace(&mut tabs, 0, "Fix auth"));
    assert_eq!(tabs.workspaces[0].custom_title.as_deref(), Some("Fix auth"));
    assert_eq!(
        tabs.workspaces[0].custom_title_source.as_deref(),
        Some("user")
    );
}

#[test]
fn rename_workspace_out_of_range_and_negative_index_are_no_ops() {
    let mut tabs = tabs_with(2, 0, 0);
    let before = tabs.clone();
    assert!(!rename_workspace(&mut tabs, 2, "nope"));
    assert!(!rename_workspace(&mut tabs, -1, "nope"));
    assert_eq!(tabs, before);
}

#[test]
fn set_workspace_description_normalizes_line_endings() {
    let mut tabs = tabs_with(1, 0, 0);
    assert!(set_workspace_description(
        &mut tabs,
        0,
        "alpha\r\nbeta\rgamma"
    ));
    assert_eq!(
        tabs.workspaces[0].custom_description.as_deref(),
        Some("alpha\nbeta\ngamma")
    );
}

#[test]
fn set_workspace_description_preserves_nonempty_edge_whitespace() {
    let mut tabs = tabs_with(1, 0, 0);
    assert!(set_workspace_description(&mut tabs, 0, "  notes  "));
    assert_eq!(
        tabs.workspaces[0].custom_description.as_deref(),
        Some("  notes  ")
    );
}

#[test]
fn set_workspace_description_empty_or_whitespace_clears() {
    let mut tabs = tabs_with(1, 0, 0);
    tabs.workspaces[0].custom_description = Some("Named".to_string());
    assert!(set_workspace_description(&mut tabs, 0, " \r\n\t "));
    assert_eq!(tabs.workspaces[0].custom_description, None);
}

#[test]
fn set_workspace_description_identical_value_is_no_change() {
    let mut tabs = tabs_with(1, 0, 0);
    assert!(set_workspace_description(&mut tabs, 0, "alpha\nbeta"));
    assert!(!set_workspace_description(&mut tabs, 0, "alpha\nbeta"));
}

#[test]
fn set_workspace_description_out_of_range_and_negative_index_are_no_ops() {
    let mut tabs = tabs_with(1, 0, 0);
    let before = tabs.clone();
    assert!(!set_workspace_description(&mut tabs, 2, "nope"));
    assert!(!set_workspace_description(&mut tabs, -1, "nope"));
    assert_eq!(tabs, before);
}

#[test]
fn reset_workspace_color_clears_custom_color() {
    let mut tabs = tabs_with(1, 0, 0);
    tabs.workspaces[0].custom_color = Some("#C0392B".to_string());
    assert!(reset_workspace_color(&mut tabs, 0));
    assert_eq!(tabs.workspaces[0].custom_color, None);
}

#[test]
fn reset_workspace_color_no_ops_when_clear_or_out_of_range() {
    let mut tabs = tabs_with(1, 0, 0);
    let before = tabs.clone();
    assert!(!reset_workspace_color(&mut tabs, 0));
    assert!(!reset_workspace_color(&mut tabs, 2));
    assert!(!reset_workspace_color(&mut tabs, -1));
    assert_eq!(tabs, before);
}

#[test]
fn set_panel_title_sets_trims_and_clears_custom_title() {
    let mut workspace = fresh_terminal_workspace("surface-1");
    assert!(set_panel_title(&mut workspace, "surface-1", "  api logs  "));
    let titles = workspace.panel_titles.as_ref().expect("title metadata");
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0].panel_id, "surface-1");
    assert_eq!(titles[0].custom_title.as_deref(), Some("api logs"));

    assert!(!set_panel_title(&mut workspace, "surface-1", "api logs"));
    assert!(set_panel_title(&mut workspace, "surface-1", ""));
    assert_eq!(workspace.panel_titles, None);
}

#[test]
fn set_panel_title_rejects_missing_layout_or_panel() {
    let mut workspace = fresh_terminal_workspace("surface-1");
    let before = workspace.clone();
    assert!(!set_panel_title(&mut workspace, "missing", "api logs"));
    assert_eq!(workspace, before);

    workspace.layout = None;
    assert!(!set_panel_title(&mut workspace, "surface-1", "api logs"));
    assert_eq!(workspace.panel_titles, None);
}

#[test]
fn set_panel_pinned_sets_and_clears_panel_pin() {
    let mut workspace = fresh_terminal_workspace("surface-1");

    assert!(set_panel_pinned(&mut workspace, "surface-1", true));
    let pins = workspace.panel_pins.as_ref().expect("pin metadata");
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].panel_id, "surface-1");
    assert!(pins[0].is_pinned);

    assert!(!set_panel_pinned(&mut workspace, "surface-1", true));
    assert!(set_panel_pinned(&mut workspace, "surface-1", false));
    assert_eq!(workspace.panel_pins, None);
    assert!(!set_panel_pinned(&mut workspace, "surface-1", false));
}

#[test]
fn set_panel_pinned_rejects_missing_layout_or_panel() {
    let mut workspace = fresh_terminal_workspace("surface-1");
    let before = workspace.clone();
    assert!(!set_panel_pinned(&mut workspace, "missing", true));
    assert_eq!(workspace, before);

    workspace.layout = None;
    assert!(!set_panel_pinned(&mut workspace, "surface-1", true));
    assert_eq!(workspace.panel_pins, None);
}

#[test]
fn reorder_surface_matches_bonsplit_offsets_pin_tiers_and_focus_policy() {
    let mut workspace = fresh_terminal_workspace("a");
    let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        panic!("expected pane");
    };
    pane.panel_ids = vec!["a".into(), "b".into(), "c".into()];
    pane.selected_panel_id = Some("b".into());
    workspace.panel_pins = Some(vec![SessionPanelPinSnapshot {
        panel_id: "a".into(),
        is_pinned: true,
    }]);

    assert_eq!(reorder_surface(&mut workspace, "c", 0, false), Some(true));
    assert_eq!(
        panel_ids(workspace.layout.as_ref().unwrap()),
        ["a", "c", "b"]
    );
    let Layout::Pane(pane) = workspace.layout.as_ref().unwrap() else {
        panic!("expected pane");
    };
    assert_eq!(pane.selected_panel_id.as_deref(), Some("b"));

    assert_eq!(reorder_surface(&mut workspace, "c", 3, true), Some(true));
    assert_eq!(
        panel_ids(workspace.layout.as_ref().unwrap()),
        ["a", "b", "c"]
    );
    let Layout::Pane(pane) = workspace.layout.as_ref().unwrap() else {
        panic!("expected pane");
    };
    assert_eq!(pane.selected_panel_id.as_deref(), Some("c"));
    assert_eq!(reorder_surface(&mut workspace, "missing", 0, false), None);
}

#[test]
fn move_surface_cross_pane_collapses_source_and_honors_pin_tier() {
    let mut tabs = one_workspace_tabs("a");
    let mut source = pane("a");
    let Layout::Pane(source_pane) = &mut source else {
        unreachable!();
    };
    source_pane.pane_id = Some("pane-source".into());
    let mut target = pane("b");
    let Layout::Pane(target_pane) = &mut target else {
        unreachable!();
    };
    target_pane.pane_id = Some("pane-target".into());
    target_pane.panel_ids.push("c".into());
    target_pane.selected_panel_id = Some("b".into());
    tabs.workspaces[0].layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        source,
        target,
    ));
    tabs.workspaces[0].panel_pins = Some(vec![
        SessionPanelPinSnapshot {
            panel_id: "a".into(),
            is_pinned: true,
        },
        SessionPanelPinSnapshot {
            panel_id: "b".into(),
            is_pinned: true,
        },
    ]);

    assert_eq!(
        move_surface(&mut tabs, 0, "a", 0, "pane-target", Some(99), false),
        Some(true)
    );
    let Layout::Pane(target) = tabs.workspaces[0].layout.as_ref().unwrap() else {
        panic!("emptied source split should collapse to the target pane");
    };
    assert_eq!(target.pane_id.as_deref(), Some("pane-target"));
    assert_eq!(target.panel_ids, ["b", "a", "c"]);
    assert_eq!(target.selected_panel_id.as_deref(), Some("b"));
}

#[test]
fn move_surface_cross_workspace_transfers_metadata_and_focuses_destination() {
    let mut tabs = one_workspace_tabs("a");
    let Layout::Pane(source) = tabs.workspaces[0].layout.as_mut().unwrap() else {
        unreachable!();
    };
    source.pane_id = Some("pane-source".into());
    source.panel_ids.push("b".into());
    assert!(set_panel_title(&mut tabs.workspaces[0], "b", "build"));
    assert!(set_panel_pinned(&mut tabs.workspaces[0], "b", true));
    assert!(set_panel_unread(&mut tabs.workspaces[0], "b", true));
    tabs.workspaces[0].listening_ports = Some(vec![3000]);
    tabs.workspaces[0].panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
        panel_id: "b".into(),
        ports: vec![3000],
    }]);
    let mut destination = fresh_terminal_workspace("c");
    destination.workspace_id = Some("workspace-destination".into());
    let Layout::Pane(target) = destination.layout.as_mut().unwrap() else {
        unreachable!();
    };
    target.pane_id = Some("pane-target".into());
    tabs.workspaces.push(destination);

    assert_eq!(
        move_surface(&mut tabs, 0, "b", 1, "pane-target", None, true),
        Some(true)
    );
    assert_eq!(tabs.selected_workspace_index, Some(1));
    assert_eq!(tabs.workspaces[0].panel_titles, None);
    assert_eq!(tabs.workspaces[0].panel_pins, None);
    assert_eq!(tabs.workspaces[0].panel_unreads, None);
    assert_eq!(tabs.workspaces[0].listening_ports, None);
    assert_eq!(tabs.workspaces[0].panel_listening_ports, None);
    let target = match tabs.workspaces[1].layout.as_ref().unwrap() {
        Layout::Pane(target) => target,
        Layout::Split(_) => panic!("expected destination pane"),
    };
    assert_eq!(target.panel_ids, ["b", "c"]);
    assert_eq!(target.selected_panel_id.as_deref(), Some("b"));
    assert_eq!(
        tabs.workspaces[1].panel_titles.as_ref().unwrap()[0]
            .custom_title
            .as_deref(),
        Some("build")
    );
    assert!(tabs.workspaces[1].panel_pins.as_ref().unwrap()[0].is_pinned);
    assert!(tabs.workspaces[1].panel_unreads.as_ref().unwrap()[0].is_unread);
    assert_eq!(tabs.workspaces[1].listening_ports, Some(vec![3000]));
    assert_eq!(
        tabs.workspaces[1].panel_listening_ports.as_ref().unwrap()[0].ports,
        [3000]
    );
}

#[test]
fn move_surface_invalid_destination_is_atomic() {
    let mut tabs = one_workspace_tabs("a");
    let before = tabs.clone();
    assert_eq!(
        move_surface(&mut tabs, 0, "a", 0, "missing-pane", Some(0), false),
        None
    );
    assert_eq!(tabs, before);
}

fn workspace_with_id(id: &str, panel_id: &str) -> SessionWorkspaceSnapshot {
    let mut workspace = fresh_terminal_workspace(panel_id);
    workspace.workspace_id = Some(id.to_string());
    workspace
}

#[test]
fn move_workspace_to_window_detaches_group_and_preserves_unfocused_selection() {
    let group_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let mut source_tabs = SessionTabManagerSnapshot {
        selected_workspace_index: Some(0),
        workspaces: vec![
            workspace_with_id("workspace-a", "a"),
            workspace_with_id("workspace-b", "b"),
        ],
        workspace_groups: Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
            id: group_id.to_string(),
            name: "group".to_string(),
            anchor_workspace_id: Some("workspace-a".to_string()),
            ..Default::default()
        }]),
    };
    source_tabs.workspaces[0].group_id = Some(group_id.to_string());
    source_tabs.workspaces[1].group_id = Some(group_id.to_string());
    source_tabs.workspaces[0].is_pinned = Some(true);
    let destination_tabs = SessionTabManagerSnapshot {
        selected_workspace_index: Some(0),
        workspaces: vec![workspace_with_id("workspace-c", "c")],
        workspace_groups: None,
    };
    let mut snapshot = crate::session::AppSessionSnapshot {
        windows: vec![
            crate::session::SessionWindowSnapshot {
                window_id: Some("window-a".to_string()),
                selected_workspace_id: None,
                dock: None,
                tab_manager: source_tabs,
            },
            crate::session::SessionWindowSnapshot {
                window_id: Some("window-b".to_string()),
                selected_workspace_id: None,
                dock: None,
                tab_manager: destination_tabs,
            },
        ],
        ..Default::default()
    };

    assert_eq!(
        move_workspace_to_window(
            &mut snapshot,
            "workspace-a",
            "window-b",
            workspace_with_id("bootstrap", "bootstrap-panel"),
            false,
        ),
        Ok(())
    );
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .as_deref(),
        Some("workspace-b")
    );
    assert_eq!(snapshot.windows[0].tab_manager.workspaces[0].group_id, None);
    assert_eq!(snapshot.windows[0].tab_manager.workspace_groups, None);
    assert_eq!(
        snapshot.windows[0].tab_manager.selected_workspace_index,
        Some(0)
    );
    assert_eq!(
        snapshot.windows[1].tab_manager.selected_workspace_index,
        Some(1)
    );
    assert_eq!(
        snapshot.windows[1]
            .tab_manager
            .workspaces
            .iter()
            .map(|workspace| workspace.workspace_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["workspace-a", "workspace-c"]
    );
    assert_eq!(snapshot.windows[1].tab_manager.workspaces[0].group_id, None);
}

#[test]
fn move_workspace_to_window_bootstraps_empty_source_and_focuses_destination() {
    let mut snapshot = crate::session::AppSessionSnapshot {
        windows: vec![
            crate::session::SessionWindowSnapshot {
                window_id: Some("window-a".to_string()),
                selected_workspace_id: None,
                dock: None,
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index: Some(0),
                    workspaces: vec![workspace_with_id("workspace-a", "a")],
                    workspace_groups: None,
                },
            },
            crate::session::SessionWindowSnapshot {
                window_id: Some("window-b".to_string()),
                selected_workspace_id: None,
                dock: None,
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index: Some(0),
                    workspaces: vec![workspace_with_id("workspace-b", "b")],
                    workspace_groups: None,
                },
            },
        ],
        ..Default::default()
    };

    assert_eq!(
        move_workspace_to_window(
            &mut snapshot,
            "workspace-a",
            "window-b",
            workspace_with_id("bootstrap", "bootstrap-panel"),
            true,
        ),
        Ok(())
    );
    assert_eq!(
        snapshot.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .as_deref(),
        Some("bootstrap")
    );
    assert_eq!(
        snapshot.windows[0].tab_manager.selected_workspace_index,
        Some(0)
    );
    assert_eq!(
        snapshot.windows[1].tab_manager.selected_workspace_index,
        Some(1)
    );
    assert_eq!(
        snapshot.windows[1].tab_manager.workspaces[1]
            .workspace_id
            .as_deref(),
        Some("workspace-a")
    );
}

#[test]
fn move_workspace_to_same_window_uses_detach_attach_semantics() {
    let mut snapshot = crate::session::AppSessionSnapshot {
        windows: vec![crate::session::SessionWindowSnapshot {
            window_id: Some("window-a".to_string()),
            selected_workspace_id: None,
            dock: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![workspace_with_id("workspace-a", "a")],
                workspace_groups: None,
            },
        }],
        ..Default::default()
    };

    assert_eq!(
        move_workspace_to_window(
            &mut snapshot,
            "workspace-a",
            "window-a",
            workspace_with_id("bootstrap", "bootstrap-panel"),
            false,
        ),
        Ok(())
    );
    assert_eq!(
        snapshot.windows[0]
            .tab_manager
            .workspaces
            .iter()
            .map(|workspace| workspace.workspace_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["bootstrap", "workspace-a"]
    );
    assert_eq!(
        snapshot.windows[0].tab_manager.selected_workspace_index,
        Some(0)
    );
}

#[test]
fn move_workspace_to_window_rejects_missing_targets_atomically() {
    let mut snapshot = crate::session::AppSessionSnapshot {
        windows: vec![crate::session::SessionWindowSnapshot {
            window_id: Some("window-a".to_string()),
            selected_workspace_id: None,
            dock: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![workspace_with_id("workspace-a", "a")],
                workspace_groups: None,
            },
        }],
        ..Default::default()
    };
    let before = snapshot.clone();
    assert_eq!(
        move_workspace_to_window(
            &mut snapshot,
            "workspace-a",
            "missing-window",
            workspace_with_id("bootstrap", "bootstrap-panel"),
            false,
        ),
        Err(MoveWorkspaceToWindowError::WindowNotFound)
    );
    assert_eq!(snapshot, before);
}

#[test]
fn split_off_surface_moves_tab_into_adjacent_pane() {
    let mut workspace = fresh_terminal_workspace("a");
    let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.pane_id = Some("pane-source".to_string());
    pane.panel_ids = vec!["a".into(), "b".into(), "c".into()];
    pane.selected_panel_id = Some("a".into());

    assert_eq!(
        split_off_surface(
            &mut workspace,
            "b",
            SessionSplitOrientation::Horizontal,
            false,
        ),
        Ok(())
    );
    let Layout::Split(split) = workspace.layout.as_ref().unwrap() else {
        panic!("expected split");
    };
    assert_eq!(split.orientation, SessionSplitOrientation::Horizontal);
    let Layout::Pane(source) = split.first.as_ref() else {
        panic!("expected source pane first");
    };
    let Layout::Pane(moved) = split.second.as_ref() else {
        panic!("expected moved pane second");
    };
    assert_eq!(source.pane_id.as_deref(), Some("pane-source"));
    assert_eq!(source.panel_ids, ["a", "c"]);
    assert_eq!(source.selected_panel_id.as_deref(), Some("a"));
    assert_eq!(moved.pane_id, None);
    assert_eq!(moved.panel_ids, ["b"]);
    assert_eq!(moved.selected_panel_id.as_deref(), Some("b"));
}

#[test]
fn split_off_surface_rejects_single_tab_pane_atomically() {
    let mut workspace = fresh_terminal_workspace("a");
    let before = workspace.clone();
    assert_eq!(
        split_off_surface(&mut workspace, "a", SessionSplitOrientation::Vertical, true,),
        Err(SplitOffSurfaceError::WouldEmptySourcePane)
    );
    assert_eq!(workspace, before);
}

#[test]
fn swap_selected_pane_surfaces_matches_canonical_move_order() {
    let mut source = pane("a");
    let Layout::Pane(source_pane) = &mut source else {
        unreachable!();
    };
    source_pane.pane_id = Some("pane-source".into());
    source_pane.panel_ids.push("b".into());
    source_pane.selected_panel_id = Some("b".into());
    let mut target = pane("c");
    let Layout::Pane(target_pane) = &mut target else {
        unreachable!();
    };
    target_pane.pane_id = Some("pane-target".into());
    target_pane.panel_ids.push("d".into());
    target_pane.selected_panel_id = Some("c".into());
    let mut workspace = fresh_terminal_workspace("unused");
    workspace.layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        source,
        target,
    ));
    workspace.panel_pins = Some(vec![
        SessionPanelPinSnapshot {
            panel_id: "b".into(),
            is_pinned: true,
        },
        SessionPanelPinSnapshot {
            panel_id: "d".into(),
            is_pinned: true,
        },
    ]);

    assert_eq!(
        swap_selected_pane_surfaces(&mut workspace, "pane-source", "pane-target"),
        Ok(PaneSwapResult {
            source_surface_id: "b".into(),
            target_surface_id: "c".into(),
        })
    );
    let Layout::Split(split) = workspace.layout.as_ref().unwrap() else {
        panic!("expected split");
    };
    let Layout::Pane(source) = split.first.as_ref() else {
        panic!("expected source pane");
    };
    let Layout::Pane(target) = split.second.as_ref() else {
        panic!("expected target pane");
    };
    assert_eq!(source.pane_id.as_deref(), Some("pane-source"));
    assert_eq!(target.pane_id.as_deref(), Some("pane-target"));
    assert_eq!(source.panel_ids, ["a", "c"]);
    assert_eq!(target.panel_ids, ["d", "b"]);
    assert_eq!(source.selected_panel_id.as_deref(), Some("a"));
    assert_eq!(target.selected_panel_id.as_deref(), Some("d"));
}

#[test]
fn swap_selected_singleton_panes_preserves_identities_and_selection() {
    let mut source = pane("a");
    let Layout::Pane(source_pane) = &mut source else {
        unreachable!();
    };
    source_pane.pane_id = Some("pane-source".into());
    let mut target = pane("b");
    let Layout::Pane(target_pane) = &mut target else {
        unreachable!();
    };
    target_pane.pane_id = Some("pane-target".into());
    let mut workspace = fresh_terminal_workspace("unused");
    workspace.layout = Some(split(
        SessionSplitOrientation::Vertical,
        0.5,
        source,
        target,
    ));

    assert!(swap_selected_pane_surfaces(&mut workspace, "pane-source", "pane-target").is_ok());
    let Layout::Split(split) = workspace.layout.as_ref().unwrap() else {
        panic!("expected split");
    };
    let Layout::Pane(source) = split.first.as_ref() else {
        panic!("expected source pane");
    };
    let Layout::Pane(target) = split.second.as_ref() else {
        panic!("expected target pane");
    };
    assert_eq!(source.panel_ids, ["b"]);
    assert_eq!(target.panel_ids, ["a"]);
    assert_eq!(source.selected_panel_id.as_deref(), Some("b"));
    assert_eq!(target.selected_panel_id.as_deref(), Some("a"));
}

#[test]
fn swap_selected_pane_surfaces_moves_persisted_surface_ownership() {
    let mut source = pane("a");
    let Layout::Pane(source_pane) = &mut source else {
        unreachable!();
    };
    source_pane.pane_id = Some("pane-source".into());
    let mut target = pane("b");
    let Layout::Pane(target_pane) = &mut target else {
        unreachable!();
    };
    target_pane.pane_id = Some("pane-target".into());
    let mut workspace = fresh_terminal_workspace("unused");
    workspace.focused_panel_id = Some("a".into());
    workspace.layout = Some(split(
        SessionSplitOrientation::Vertical,
        0.5,
        source,
        target,
    ));
    workspace.surfaces = Some(
        [("a", "pane-source"), ("b", "pane-target")]
            .into_iter()
            .map(|(surface_id, pane_id)| {
                serde_json::from_value(serde_json::json!({
                    "surface_id": surface_id,
                    "pane_id": pane_id,
                    "generation": 1,
                    "kind": {"type": "terminal"},
                    "metadata": {}
                }))
                .expect("surface record")
            })
            .collect(),
    );

    swap_selected_pane_surfaces(&mut workspace, "pane-source", "pane-target").unwrap();

    let records = workspace.surfaces.as_ref().unwrap();
    assert_eq!(records[0].pane_id, "pane-target");
    assert_eq!(records[1].pane_id, "pane-source");
    let tabs = SessionTabManagerSnapshot {
        selected_workspace_index: Some(0),
        workspaces: vec![workspace],
        workspace_groups: None,
    };
    SurfaceLifecycleModel::from_session_snapshot("window-1", &tabs)
        .expect("swapped surface ownership remains valid");
}

#[test]
fn swap_selected_pane_surfaces_rejects_invalid_target_atomically() {
    let mut workspace = fresh_terminal_workspace("a");
    let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.pane_id = Some("pane-source".into());
    let before = workspace.clone();

    assert_eq!(
        swap_selected_pane_surfaces(&mut workspace, "pane-source", "pane-missing"),
        Err(PaneSwapError::TargetPaneNotFound)
    );
    assert_eq!(workspace, before);
}
