#[test]
fn break_surface_to_new_workspace_transfers_state_without_forcing_focus() {
    let mut tabs = one_workspace_tabs("a");
    let Layout::Pane(pane) = tabs.workspaces[0].layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.pane_id = Some("pane-source".into());
    pane.panel_ids.push("b".into());
    pane.selected_panel_id = Some("b".into());
    assert!(set_panel_title(&mut tabs.workspaces[0], "b", "Build"));

    let result = break_surface_to_new_workspace(&mut tabs, 0, "b", false).unwrap();

    assert_eq!(result.surface_id, "b");
    assert_eq!(result.workspace_index, 1);
    assert_eq!(tabs.selected_workspace_index, Some(0));
    assert_eq!(
        panel_ids(tabs.workspaces[0].layout.as_ref().unwrap()),
        ["a"]
    );
    let destination = &tabs.workspaces[1];
    assert_eq!(panel_ids(destination.layout.as_ref().unwrap()), ["b"]);
    let Layout::Pane(destination_pane) = destination.layout.as_ref().unwrap() else {
        unreachable!();
    };
    assert_eq!(destination_pane.pane_id, None);
    assert_eq!(destination.process_title, "Build");
    assert_eq!(destination.panel_titles.as_ref().unwrap()[0].panel_id, "b");
}

#[test]
fn break_surface_transfers_workspace_focus_to_live_surfaces() {
    let mut tabs = one_workspace_tabs("a");
    let Layout::Pane(pane) = tabs.workspaces[0].layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.panel_ids.push("b".into());
    pane.selected_panel_id = Some("b".into());
    tabs.workspaces[0].focused_panel_id = Some("b".into());

    let result = break_surface_to_new_workspace(&mut tabs, 0, "b", false).unwrap();

    assert_eq!(tabs.workspaces[0].focused_panel_id.as_deref(), Some("a"));
    assert_eq!(
        tabs.workspaces[result.workspace_index]
            .focused_panel_id
            .as_deref(),
        Some("b")
    );
}

#[test]
fn break_surface_to_new_workspace_is_atomic_for_missing_surface() {
    let mut tabs = one_workspace_tabs("a");
    let before = tabs.clone();
    assert_eq!(
        break_surface_to_new_workspace(&mut tabs, 0, "missing", true),
        Err(PaneBreakError::SurfaceNotFound)
    );
    assert_eq!(tabs, before);
}

#[test]
fn break_only_surface_leaves_empty_source_and_focuses_new_workspace() {
    let mut tabs = one_workspace_tabs("a");

    let result = break_surface_to_new_workspace(&mut tabs, 0, "a", true).unwrap();

    assert_eq!(result.workspace_index, 1);
    assert_eq!(tabs.workspaces[0].layout, None);
    assert_eq!(
        panel_ids(tabs.workspaces[1].layout.as_ref().unwrap()),
        ["a"]
    );
    assert_eq!(tabs.selected_workspace_index, Some(1));
}

#[test]
fn focus_alternate_pane_selects_first_pane_different_from_focus() {
    let mut first = pane("a");
    let Layout::Pane(first_pane) = &mut first else {
        unreachable!();
    };
    first_pane.pane_id = Some("pane-a".into());
    let mut second = pane("b");
    let Layout::Pane(second_pane) = &mut second else {
        unreachable!();
    };
    second_pane.pane_id = Some("pane-b".into());
    let mut workspace = fresh_terminal_workspace("unused");
    workspace.layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        first,
        second,
    ));

    assert_eq!(
        focus_alternate_pane(&workspace, Some("pane-a")),
        Ok(PaneLastResult {
            pane_id: "pane-b".into(),
            surface_id: Some("b".into()),
        })
    );
    assert_eq!(
        focus_alternate_pane(&workspace, Some("pane-b")),
        Ok(PaneLastResult {
            pane_id: "pane-a".into(),
            surface_id: Some("a".into()),
        })
    );
    assert_eq!(
        focus_alternate_pane(&workspace, None),
        Err(PaneLastError::NoFocusedPane)
    );
}

#[test]
fn focus_pane_target_preserves_selected_surface_and_rejects_missing_pane() {
    let mut workspace = fresh_terminal_workspace("a");
    let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.pane_id = Some("pane-a".into());
    pane.panel_ids.push("b".into());
    pane.selected_panel_id = Some("b".into());
    assert_eq!(
        focus_pane_target(&workspace, "pane-a"),
        Ok(PaneLastResult {
            pane_id: "pane-a".into(),
            surface_id: Some("b".into()),
        })
    );
    assert_eq!(
        focus_pane_target(&workspace, "missing"),
        Err(PaneFocusError::PaneNotFound)
    );
}

#[test]
fn focus_alternate_pane_rejects_single_pane() {
    let mut workspace = fresh_terminal_workspace("a");
    let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
        unreachable!();
    };
    pane.pane_id = Some("pane-a".into());
    assert_eq!(
        focus_alternate_pane(&workspace, Some("pane-a")),
        Err(PaneLastError::NoAlternatePane)
    );
}

fn resize_workspace() -> SessionWorkspaceSnapshot {
    let mut a = pane("a");
    let Layout::Pane(a_pane) = &mut a else {
        unreachable!()
    };
    a_pane.pane_id = Some("pane-a".into());
    let mut b = pane("b");
    let Layout::Pane(b_pane) = &mut b else {
        unreachable!()
    };
    b_pane.pane_id = Some("pane-b".into());
    let mut c = pane("c");
    let Layout::Pane(c_pane) = &mut c else {
        unreachable!()
    };
    c_pane.pane_id = Some("pane-c".into());
    let mut inner = split(SessionSplitOrientation::Horizontal, 0.5, a, b);
    let Layout::Split(inner_split) = &mut inner else {
        unreachable!()
    };
    inner_split.split_id = Some("split-inner".into());
    let mut root = split(SessionSplitOrientation::Horizontal, 0.6, inner, c);
    let Layout::Split(root_split) = &mut root else {
        unreachable!()
    };
    root_split.split_id = Some("split-root".into());
    SessionWorkspaceSnapshot {
        layout: Some(root),
        ..fresh_terminal_workspace("unused")
    }
}

#[test]
fn resize_pane_relative_uses_nearest_matching_adjacent_ancestor() {
    let mut workspace = resize_workspace();
    assert_eq!(
        resize_pane_relative(
            &mut workspace,
            "pane-a",
            PaneResizeDirection::Right,
            60,
            1000.0,
            800.0,
        ),
        Ok(PaneResizeResult {
            split_id: "split-inner".into(),
            old_divider_position: 0.5,
            new_divider_position: 0.6,
        })
    );
    let Layout::Split(root) = workspace.layout.as_ref().unwrap() else {
        unreachable!()
    };
    assert_eq!(root.divider_position, 0.6);
    let Layout::Split(inner) = root.first.as_ref() else {
        unreachable!()
    };
    assert!((inner.divider_position - 0.6).abs() < 1e-9);
}

#[test]
fn resize_pane_relative_walks_outward_for_requested_border() {
    let mut workspace = resize_workspace();
    let result = resize_pane_relative(
        &mut workspace,
        "pane-b",
        PaneResizeDirection::Right,
        100,
        1000.0,
        800.0,
    )
    .unwrap();
    assert_eq!(result.split_id, "split-root");
    assert!((result.new_divider_position - 0.7).abs() < 1e-9);

    let before = workspace.clone();
    assert_eq!(
        resize_pane_relative(
            &mut workspace,
            "pane-c",
            PaneResizeDirection::Right,
            10,
            1000.0,
            800.0,
        ),
        Err(PaneResizeError::NoAdjacentBorder)
    );
    assert_eq!(workspace, before);
}

#[test]
fn resize_pane_absolute_uses_target_child_fraction_and_clamps() {
    let mut workspace = resize_workspace();
    let result = resize_pane_absolute(
        &mut workspace,
        "pane-b",
        SessionSplitOrientation::Horizontal,
        120.0,
        1000.0,
        800.0,
    )
    .unwrap();
    assert_eq!(result.split_id, "split-inner");
    assert!((result.new_divider_position - 0.8).abs() < 1e-9);

    let clamped = resize_pane_absolute(
        &mut workspace,
        "pane-a",
        SessionSplitOrientation::Horizontal,
        1.0,
        1000.0,
        800.0,
    )
    .unwrap();
    assert_eq!(clamped.new_divider_position, MIN_DIVIDER);
}

#[test]
fn resize_pane_rejects_missing_axis_or_identity_without_mutating() {
    let mut workspace = resize_workspace();
    let before = workspace.clone();
    assert_eq!(
        resize_pane_relative(
            &mut workspace,
            "pane-a",
            PaneResizeDirection::Down,
            10,
            1000.0,
            800.0,
        ),
        Err(PaneResizeError::NoOrientationSplitAncestor)
    );
    assert_eq!(workspace, before);

    let Layout::Split(root) = workspace.layout.as_mut().unwrap() else {
        unreachable!()
    };
    let Layout::Split(inner) = root.first.as_mut() else {
        unreachable!()
    };
    inner.split_id = None;
    let before = workspace.clone();
    assert_eq!(
        resize_pane_relative(
            &mut workspace,
            "pane-a",
            PaneResizeDirection::Right,
            10,
            1000.0,
            800.0,
        ),
        Err(PaneResizeError::MissingSplitIdentity)
    );
    assert_eq!(workspace, before);
}

#[test]
fn set_panel_unread_sets_and_clears_panel_unread() {
    let mut workspace = fresh_terminal_workspace("surface-1");

    assert!(set_panel_unread_at(
        &mut workspace,
        "surface-1",
        true,
        Some(123)
    ));
    let unreads = workspace.panel_unreads.as_ref().expect("unread metadata");
    assert_eq!(unreads.len(), 1);
    assert_eq!(unreads[0].panel_id, "surface-1");
    assert!(unreads[0].is_unread);
    assert_eq!(unreads[0].unread_at, Some(123));

    assert!(!set_panel_unread(&mut workspace, "surface-1", true));
    assert!(set_panel_unread(&mut workspace, "surface-1", false));
    assert_eq!(workspace.panel_unreads, None);
    assert!(!set_panel_unread(&mut workspace, "surface-1", false));
}

#[test]
fn set_panel_unread_rejects_missing_layout_or_panel() {
    let mut workspace = fresh_terminal_workspace("surface-1");
    let before = workspace.clone();
    assert!(!set_panel_unread(&mut workspace, "missing", true));
    assert_eq!(workspace, before);

    workspace.layout = None;
    assert!(!set_panel_unread(&mut workspace, "surface-1", true));
    assert_eq!(workspace.panel_unreads, None);
}

#[test]
fn set_workspace_unread_sets_preferred_or_first_panel_and_clears_all() {
    let mut tabs = tabs_with(2, 0, 0);
    tabs.workspaces[0].layout = Some(SessionWorkspaceLayoutSnapshot::Split(
        SessionSplitLayoutSnapshot {
            split_id: None,
            orientation: SessionSplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(single_pane("surface-1")),
            second: Box::new(single_pane("surface-2")),
        },
    ));

    assert!(set_workspace_unread_at(
        &mut tabs,
        0,
        Some("surface-2"),
        true,
        Some(456)
    ));
    let unreads = tabs.workspaces[0]
        .panel_unreads
        .as_ref()
        .expect("unread metadata");
    assert_eq!(unreads.len(), 1);
    assert_eq!(unreads[0].panel_id, "surface-2");
    assert!(unreads[0].is_unread);
    assert_eq!(unreads[0].unread_at, Some(456));

    assert!(!set_workspace_unread(&mut tabs, 0, Some("surface-1"), true));
    assert!(set_workspace_unread(&mut tabs, 0, None, false));
    assert_eq!(tabs.workspaces[0].panel_unreads, None);

    assert!(set_workspace_unread(&mut tabs, 0, Some("missing"), true));
    assert_eq!(
        tabs.workspaces[0].panel_unreads.as_ref().unwrap()[0].panel_id,
        "surface-1"
    );
}

#[test]
fn set_workspace_unread_rejects_invalid_workspace_or_empty_layout() {
    let mut tabs = tabs_with(1, 0, 0);
    let before = tabs.clone();
    assert!(!set_workspace_unread(&mut tabs, -1, None, true));
    assert!(!set_workspace_unread(&mut tabs, 2, None, true));
    assert_eq!(tabs, before);

    tabs.workspaces[0].layout = None;
    assert!(!set_workspace_unread(&mut tabs, 0, None, true));
    assert_eq!(tabs.workspaces[0].panel_unreads, None);
}

// --- Workspace pin/unpin ---

/// The `panel_ids` of each workspace's layout — a stable identity for order
/// assertions (all `tabs_with` workspaces share the "Terminal" title).
fn order_of(tabs: &SessionTabManagerSnapshot) -> Vec<String> {
    tabs.workspaces
        .iter()
        .map(|w| panel_ids(w.layout.as_ref().unwrap())[0].clone())
        .collect()
}

#[test]
fn pin_moves_workspace_to_end_of_pinned_prefix() {
    let mut tabs = tabs_with(4, 2, 0);
    assert!(set_workspace_pinned(&mut tabs, 3, true));
    // The newly pinned tab lands at the END of the pinned prefix (index 2);
    // the unpinned remainder keeps its relative order.
    assert_eq!(
        order_of(&tabs),
        ["surface-0", "surface-1", "surface-3", "surface-2"]
    );
    assert_eq!(tabs.workspaces[2].is_pinned, Some(true));
}

#[test]
fn pin_first_of_all_unpinned_stays_at_index_0() {
    let mut tabs = tabs_with(3, 0, 0);
    assert!(set_workspace_pinned(&mut tabs, 0, true));
    assert_eq!(order_of(&tabs), ["surface-0", "surface-1", "surface-2"]);
    assert_eq!(tabs.workspaces[0].is_pinned, Some(true));
}

#[test]
fn pin_middle_moves_to_front() {
    let mut tabs = tabs_with(3, 0, 0);
    assert!(set_workspace_pinned(&mut tabs, 1, true));
    assert_eq!(order_of(&tabs), ["surface-1", "surface-0", "surface-2"]);
    assert_eq!(tabs.workspaces[0].is_pinned, Some(true));
}

#[test]
fn unpin_inserts_at_front_of_unpinned_segment() {
    let mut tabs = tabs_with(3, 2, 0);
    assert!(set_workspace_pinned(&mut tabs, 0, false));
    // The unpinned tab lands at the FRONT of the unpinned segment.
    assert_eq!(order_of(&tabs), ["surface-1", "surface-0", "surface-2"]);
    // Unpin stores None (omit-key), never Some(false) — golden
    // byte-stability: the serialized object must not carry the key at all.
    assert_eq!(tabs.workspaces[1].is_pinned, None);
    let object = serde_json::to_value(&tabs.workspaces[1]).unwrap();
    assert!(!object.as_object().unwrap().contains_key("is_pinned"));
}

#[test]
fn pin_already_pinned_is_no_change() {
    let mut tabs = tabs_with(3, 2, 0);
    let before = tabs.clone();
    assert!(!set_workspace_pinned(&mut tabs, 0, true));
    assert_eq!(tabs, before);
}

#[test]
fn unpin_never_pinned_is_no_change() {
    let mut tabs = tabs_with(3, 2, 0);
    let before = tabs.clone();
    assert!(!set_workspace_pinned(&mut tabs, 2, false));
    assert_eq!(tabs, before);
}

#[test]
fn set_workspace_pinned_out_of_range_and_negative_index_are_no_ops() {
    let mut tabs = tabs_with(2, 0, 0);
    let before = tabs.clone();
    assert!(!set_workspace_pinned(&mut tabs, 2, true));
    assert!(!set_workspace_pinned(&mut tabs, -1, true));
    assert_eq!(tabs, before);
}

#[test]
fn selection_follows_the_pinned_workspace() {
    let mut tabs = tabs_with(3, 0, 2);
    assert!(set_workspace_pinned(&mut tabs, 2, true));
    // ws2 moved to index 0; the selection follows it there.
    assert_eq!(order_of(&tabs), ["surface-2", "surface-0", "surface-1"]);
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn selection_stays_on_unmoved_workspace() {
    let mut tabs = tabs_with(3, 0, 1);
    assert!(set_workspace_pinned(&mut tabs, 2, true));
    // ws2 moved to index 0, shifting ws1 to index 2 — selection follows.
    assert_eq!(order_of(&tabs), ["surface-2", "surface-0", "surface-1"]);
    assert_eq!(tabs.selected_workspace_index, Some(2));
}

#[test]
fn unpin_selection_follows() {
    // Selected tab is the one being unpinned: follows it to index 1.
    let mut tabs = tabs_with(3, 2, 0);
    assert!(set_workspace_pinned(&mut tabs, 0, false));
    assert_eq!(tabs.selected_workspace_index, Some(1));

    // Selected tab is the OTHER pinned tab (p1): unpinning index 0 moves
    // p1 up to index 0 — selection follows.
    let mut tabs = tabs_with(3, 2, 1);
    assert!(set_workspace_pinned(&mut tabs, 0, false));
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn grouped_workspace_pin_preserves_membership_and_normalizes_contiguity() {
    let group_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let mut tabs = tabs_with(4, 0, 1);
    tabs.workspaces[0].group_id = Some(group_id.to_string());
    tabs.workspaces[2].group_id = Some(group_id.to_string());
    tabs.workspace_groups = Some(vec![group(group_id, false)]);
    assert!(set_workspace_pinned(&mut tabs, 2, true));
    // Group membership is preserved, but the canonical normalization tail
    // repairs the broken group run and selection follows the shifted row.
    assert_eq!(
        order_of(&tabs),
        ["surface-0", "surface-2", "surface-1", "surface-3"]
    );
    assert_eq!(tabs.workspaces[1].is_pinned, Some(true));
    assert_eq!(tabs.workspaces[1].group_id.as_deref(), Some(group_id));
    assert_eq!(tabs.selected_workspace_index, Some(2));
}

#[test]
fn boundary_counts_grouped_rows_by_group_pin() {
    // Leading grouped members of a PINNED group (members' own is_pinned is
    // None) followed by unpinned rows; pinning a trailing ungrouped ws must
    // insert AFTER the grouped pinned run (isGlobalPinnedRow parity:
    // grouped rows count by their group's pin, Ordering.swift:201-207).
    let mut tabs = tabs_with(4, 0, 0);
    tabs.workspaces[0].group_id = Some("g".to_string());
    tabs.workspaces[1].group_id = Some("g".to_string());
    tabs.workspace_groups = Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
        is_pinned: Some(true),
        ..group("g", false)
    }]);
    assert!(set_workspace_pinned(&mut tabs, 3, true));
    assert_eq!(
        order_of(&tabs),
        ["surface-0", "surface-1", "surface-3", "surface-2"]
    );
    assert_eq!(tabs.workspaces[2].is_pinned, Some(true));
}

#[test]
fn boundary_dangling_group_id_falls_back_to_own_pin() {
    // A leading row whose group_id resolves to NO group still counts by
    // its own is_pinned (the oracle's isGlobalPinnedRow nil-group arm,
    // Ordering.swift:201-207) — it must not be treated as unpinned.
    let mut tabs = tabs_with(3, 0, 0);
    tabs.workspaces[0].is_pinned = Some(true);
    tabs.workspaces[0].group_id = Some("gone".to_string());
    tabs.workspace_groups = None;
    assert!(set_workspace_pinned(&mut tabs, 2, true));
    // surface-2 lands AFTER the dangling-group pinned row, not before it.
    assert_eq!(order_of(&tabs), ["surface-0", "surface-2", "surface-1"]);
}
