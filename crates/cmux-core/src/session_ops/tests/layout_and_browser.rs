use super::*;
use crate::session::{SessionCanvasPaneSnapshot, SessionPanelShellActivityStateSnapshot};

fn pane(id: &str) -> Layout {
    single_pane(id)
}

fn split(
    orientation: SessionSplitOrientation,
    divider: f64,
    first: Layout,
    second: Layout,
) -> Layout {
    Layout::Split(SessionSplitLayoutSnapshot {
        split_id: None,
        orientation,
        divider_position: divider,
        first: Box::new(first),
        second: Box::new(second),
    })
}

fn panel_ids(layout: &Layout) -> Vec<String> {
    match layout {
        Layout::Pane(p) => p.panel_ids.clone(),
        Layout::Split(_) => panic!("expected a pane"),
    }
}

#[test]
fn clamp_divider_bounds_and_nan() {
    assert_eq!(clamp_divider(0.5), 0.5);
    assert_eq!(clamp_divider(-1.0), MIN_DIVIDER);
    assert_eq!(clamp_divider(2.0), MAX_DIVIDER);
    assert_eq!(clamp_divider(f64::NAN), 0.5);
}

#[test]
fn count_leaves_walks_the_tree() {
    assert_eq!(count_leaves(&pane("a")), 1);
    let tree = split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        split(SessionSplitOrientation::Vertical, 0.5, pane("b"), pane("c")),
    );
    assert_eq!(count_leaves(&tree), 3);
}

#[test]
fn equalize_weights_by_leaf_count() {
    let two_vs_one = SessionSplitLayoutSnapshot {
        split_id: None,
        orientation: SessionSplitOrientation::Horizontal,
        divider_position: 0.5,
        first: Box::new(split(
            SessionSplitOrientation::Vertical,
            0.5,
            pane("a"),
            pane("b"),
        )),
        second: Box::new(pane("c")),
    };
    assert!((equalize_divider(&two_vs_one) - 2.0 / 3.0).abs() < 1e-9);
}

#[test]
fn equalize_dividers_resets_a_mixed_orientation_tree_to_span_ratios() {
    // H( V(a,b), c ) with skewed dividers; equalize uses orientation-aware
    // span counts, so the horizontal root sees span 1 (the vertical subtree)
    // vs 1 (pane c) → 0.5, and the inner vertical split → 0.5. This diverges
    // from leaf-count weighting (which would give the root 2/3).
    let mut tree = split(
        SessionSplitOrientation::Horizontal,
        0.8,
        split(SessionSplitOrientation::Vertical, 0.2, pane("a"), pane("b")),
        pane("c"),
    );
    assert!(equalize_dividers(&mut tree));
    if let Layout::Split(root) = &tree {
        assert_eq!(root.divider_position, 0.5); // span-weighted, NOT 2/3
        if let Layout::Split(inner) = root.first.as_ref() {
            assert_eq!(inner.divider_position, 0.5);
        } else {
            panic!("expected nested vertical split");
        }
    } else {
        panic!("expected a split");
    }
}

#[test]
fn equalize_dividers_on_a_single_pane_is_a_noop() {
    // Canonical `foundSplit == false` for a lone pane: returns false and
    // leaves the pane byte-identical.
    let mut tree = pane("a");
    assert!(!equalize_dividers(&mut tree));
    assert_eq!(tree, pane("a"));
}

#[test]
fn equalize_dividers_preserves_leaf_count() {
    // Equalize never adds or removes panes; only divider positions change.
    let mut tree = split(
        SessionSplitOrientation::Horizontal,
        0.75,
        pane("a"),
        split(
            SessionSplitOrientation::Horizontal,
            0.15,
            pane("b"),
            pane("c"),
        ),
    );
    let before = count_leaves(&tree);
    assert!(equalize_dividers(&mut tree));
    assert_eq!(count_leaves(&tree), before);
    // Same-axis nesting: root sees span 1 (a) vs 2 (b,c) → 1/3; inner → 0.5.
    if let Layout::Split(root) = &tree {
        assert!((root.divider_position - 1.0 / 3.0).abs() < 1e-9);
        if let Layout::Split(inner) = root.second.as_ref() {
            assert_eq!(inner.divider_position, 0.5);
        } else {
            panic!("expected nested split");
        }
    } else {
        panic!("expected a split");
    }
}

#[test]
fn toggle_split_zoom_sets_and_clears_the_workspace_zoom_target() {
    let mut workspace = fresh_terminal_workspace("a");
    workspace.layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        pane("b"),
    ));
    assert!(toggle_split_zoom(&mut workspace, "b"));
    assert_eq!(workspace.zoomed_panel_id.as_deref(), Some("b"));
    assert!(toggle_split_zoom(&mut workspace, "b"));
    assert_eq!(workspace.zoomed_panel_id, None);
}

#[test]
fn toggle_split_zoom_is_a_noop_without_a_split_or_matching_panel() {
    let mut workspace = fresh_terminal_workspace("a");
    assert!(!toggle_split_zoom(&mut workspace, "a"));
    assert_eq!(workspace.zoomed_panel_id, None);

    workspace.layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        pane("b"),
    ));
    assert!(!toggle_split_zoom(&mut workspace, "missing"));
    assert_eq!(workspace.zoomed_panel_id, None);
}

#[test]
fn canvas_panes_from_layout_preserves_split_ratios() {
    let tree = split(
        SessionSplitOrientation::Horizontal,
        0.25,
        pane("a"),
        split(
            SessionSplitOrientation::Vertical,
            0.75,
            pane("b"),
            pane("c"),
        ),
    );

    let panes = canvas_panes_from_layout(&tree);

    assert_eq!(panes.len(), 3);
    assert_eq!(panes[0].panel_id, "a");
    assert_eq!(
        (panes[0].x, panes[0].y, panes[0].width, panes[0].height),
        (0, 0, 300, 800)
    );
    assert_eq!(panes[1].panel_id, "b");
    assert_eq!(
        (panes[1].x, panes[1].y, panes[1].width, panes[1].height),
        (300, 0, 900, 600)
    );
    assert_eq!(panes[2].panel_id, "c");
    assert_eq!(
        (panes[2].x, panes[2].y, panes[2].width, panes[2].height),
        (300, 600, 900, 200)
    );
}

#[test]
fn set_layout_mode_toggles_canvas_and_seeds_once() {
    let mut workspace = fresh_terminal_workspace("a");
    workspace.layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        pane("b"),
    ));

    assert!(set_layout_mode(&mut workspace, Some("canvas")));
    assert_eq!(workspace.layout_mode.as_deref(), Some("canvas"));
    assert_eq!(workspace.canvas_panes.as_ref().map(Vec::len), Some(2));

    let seeded = workspace.canvas_panes.clone();
    assert!(!set_layout_mode(&mut workspace, Some("canvas")));
    assert_eq!(workspace.canvas_panes, seeded);

    assert!(set_layout_mode(&mut workspace, Some("split")));
    assert_eq!(workspace.layout_mode, None);
    assert_eq!(workspace.canvas_panes, seeded);
}

#[test]
fn set_layout_mode_unknown_values_fall_back_to_split() {
    let mut workspace = fresh_terminal_workspace("a");
    assert!(set_layout_mode(&mut workspace, Some("canvas")));
    assert!(set_layout_mode(&mut workspace, Some("mystery")));
    assert_eq!(workspace.layout_mode, None);
}

#[test]
fn set_canvas_pane_frame_updates_existing_seeded_pane() {
    let mut workspace = fresh_terminal_workspace("a");
    assert!(set_layout_mode(&mut workspace, Some("canvas")));

    assert!(set_canvas_pane_frame(&mut workspace, "a", 40, 50, 640, 360));
    let pane = &workspace.canvas_panes.as_ref().unwrap()[0];
    assert_eq!(
        (pane.x, pane.y, pane.width, pane.height),
        (40, 50, 640, 360)
    );
    assert!(!set_canvas_pane_frame(
        &mut workspace,
        "a",
        40,
        50,
        640,
        360
    ));
}

#[test]
fn set_canvas_pane_frame_seeds_from_layout_before_updating() {
    let mut workspace = fresh_terminal_workspace("a");
    workspace.layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        pane("b"),
    ));

    assert!(set_canvas_pane_frame(
        &mut workspace,
        "b",
        700,
        20,
        300,
        240
    ));

    let panes = workspace.canvas_panes.as_ref().unwrap();
    assert_eq!(panes.len(), 2);
    let updated = panes.iter().find(|pane| pane.panel_id == "b").unwrap();
    assert_eq!(
        (updated.x, updated.y, updated.width, updated.height),
        (700, 20, 300, 240)
    );
}

#[test]
fn set_canvas_pane_frame_adds_missing_panel_and_rejects_blank_ids() {
    let mut workspace = fresh_terminal_workspace("a");
    assert!(!set_canvas_pane_frame(&mut workspace, " ", 0, 0, 1, 1));
    assert!(set_canvas_pane_frame(&mut workspace, "new", 1, 2, -3, 0));
    let pane = workspace.canvas_panes.as_ref().unwrap().last().unwrap();
    assert_eq!(pane.panel_id, "new");
    assert_eq!((pane.width, pane.height), (1, 1));
}

#[test]
fn apply_canvas_action_rejects_split_mode_and_unknown_actions() {
    let mut workspace = fresh_terminal_workspace("a");
    workspace.canvas_panes = Some(vec![
        SessionCanvasPaneSnapshot {
            panel_id: "a".to_string(),
            x: 20,
            y: 30,
            width: 100,
            height: 80,
            panel_ids: None,
            selected_panel_id: Some("a".to_string()),
        },
        SessionCanvasPaneSnapshot {
            panel_id: "b".to_string(),
            x: 80,
            y: 90,
            width: 200,
            height: 120,
            panel_ids: None,
            selected_panel_id: Some("b".to_string()),
        },
    ]);
    assert!(!apply_canvas_action(&mut workspace, "alignLeft"));
    workspace.layout_mode = Some("canvas".to_string());
    assert!(!apply_canvas_action(&mut workspace, "mystery"));
}

#[test]
fn apply_canvas_action_aligns_and_equalizes_persisted_frames() {
    let mut workspace = fresh_terminal_workspace("a");
    workspace.layout_mode = Some("canvas".to_string());
    workspace.canvas_panes = Some(vec![
        SessionCanvasPaneSnapshot {
            panel_id: "a".to_string(),
            x: 20,
            y: 30,
            width: 100,
            height: 80,
            panel_ids: None,
            selected_panel_id: Some("a".to_string()),
        },
        SessionCanvasPaneSnapshot {
            panel_id: "b".to_string(),
            x: 80,
            y: 90,
            width: 200,
            height: 120,
            panel_ids: None,
            selected_panel_id: Some("b".to_string()),
        },
    ]);

    assert!(apply_canvas_action(&mut workspace, "canvas.alignLeft"));
    let panes = workspace.canvas_panes.as_ref().unwrap();
    assert_eq!(
        panes.iter().map(|pane| pane.x).collect::<Vec<_>>(),
        vec![20, 20]
    );

    assert!(apply_canvas_action(&mut workspace, "equalizeHeights"));
    let panes = workspace.canvas_panes.as_ref().unwrap();
    assert_eq!(
        panes.iter().map(|pane| pane.height).collect::<Vec<_>>(),
        vec![120, 120]
    );
}

#[test]
fn apply_canvas_action_distributes_and_tidies_frames() {
    let mut workspace = fresh_terminal_workspace("a");
    workspace.layout_mode = Some("canvas".to_string());
    workspace.canvas_panes = Some(vec![
        SessionCanvasPaneSnapshot {
            panel_id: "c".to_string(),
            x: 320,
            y: 210,
            width: 50,
            height: 50,
            panel_ids: None,
            selected_panel_id: Some("c".to_string()),
        },
        SessionCanvasPaneSnapshot {
            panel_id: "a".to_string(),
            x: 10,
            y: 20,
            width: 100,
            height: 80,
            panel_ids: None,
            selected_panel_id: Some("a".to_string()),
        },
        SessionCanvasPaneSnapshot {
            panel_id: "b".to_string(),
            x: 240,
            y: 50,
            width: 75,
            height: 60,
            panel_ids: None,
            selected_panel_id: Some("b".to_string()),
        },
    ]);

    assert!(apply_canvas_action(
        &mut workspace,
        "distributeHorizontally"
    ));
    let panes = workspace.canvas_panes.as_ref().unwrap();
    assert_eq!(panes[1].x, 10);
    assert_eq!(panes[2].x, 126);
    assert_eq!(panes[0].x, 217);

    assert!(apply_canvas_action(&mut workspace, "tidy"));
    let panes = workspace.canvas_panes.as_ref().unwrap();
    assert_eq!(
        panes
            .iter()
            .map(|pane| (pane.panel_id.as_str(), pane.x, pane.y))
            .collect::<Vec<_>>(),
        vec![("c", 10, 116), ("a", 10, 20), ("b", 126, 20)]
    );
}

#[test]
fn apply_canvas_action_with_gap_uses_configured_spacing() {
    let mut workspace = fresh_terminal_workspace("a");
    workspace.layout_mode = Some("canvas".to_string());
    workspace.canvas_panes = Some(vec![
        SessionCanvasPaneSnapshot {
            panel_id: "a".to_string(),
            x: 10,
            y: 20,
            width: 100,
            height: 80,
            panel_ids: None,
            selected_panel_id: Some("a".to_string()),
        },
        SessionCanvasPaneSnapshot {
            panel_id: "b".to_string(),
            x: 240,
            y: 50,
            width: 75,
            height: 60,
            panel_ids: None,
            selected_panel_id: Some("b".to_string()),
        },
        SessionCanvasPaneSnapshot {
            panel_id: "c".to_string(),
            x: 320,
            y: 210,
            width: 50,
            height: 50,
            panel_ids: None,
            selected_panel_id: Some("c".to_string()),
        },
    ]);

    assert!(apply_canvas_action_with_gap(
        &mut workspace,
        "distributeHorizontally",
        Some(24)
    ));
    let panes = workspace.canvas_panes.as_ref().unwrap();
    assert_eq!(panes[0].x, 10);
    assert_eq!(panes[1].x, 134);
    assert_eq!(panes[2].x, 233);

    assert!(apply_canvas_action_with_gap(
        &mut workspace,
        "tidy",
        Some(24)
    ));
    let panes = workspace.canvas_panes.as_ref().unwrap();
    assert_eq!(
        panes
            .iter()
            .map(|pane| (pane.panel_id.as_str(), pane.x, pane.y))
            .collect::<Vec<_>>(),
        vec![("a", 10, 20), ("b", 134, 20), ("c", 10, 124)]
    );
}

#[test]
fn apply_canvas_action_seeds_from_layout_when_needed() {
    let mut workspace = fresh_terminal_workspace("a");
    workspace.layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        pane("b"),
    ));
    workspace.layout_mode = Some("canvas".to_string());

    assert!(apply_canvas_action(&mut workspace, "distributeVertically"));
    let panes = workspace.canvas_panes.as_ref().unwrap();
    assert_eq!(panes.len(), 2);
    assert_eq!(panes[0].y, 0);
    assert_eq!(panes[1].y, 816);
}

#[test]
fn split_pane_replaces_target_with_a_centered_split() {
    let mut tree = pane("a");
    assert!(split_pane(
        &mut tree,
        "a",
        SessionSplitOrientation::Horizontal,
        "b",
        false,
    ));
    match &tree {
        Layout::Split(s) => {
            assert_eq!(s.divider_position, 0.5);
            assert_eq!(s.orientation, SessionSplitOrientation::Horizontal);
            assert_eq!(panel_ids(&s.first), vec!["a"]); // existing stays first
            assert_eq!(panel_ids(&s.second), vec!["b"]); // new pane second
        }
        Layout::Pane(_) => panic!("expected a split"),
    }
}

#[test]
fn split_pane_insert_first_puts_the_new_pane_first() {
    let mut tree = pane("a");
    assert!(split_pane(
        &mut tree,
        "a",
        SessionSplitOrientation::Vertical,
        "b",
        true
    ));
    if let Layout::Split(s) = &tree {
        assert_eq!(panel_ids(&s.first), vec!["b"]);
        assert_eq!(panel_ids(&s.second), vec!["a"]);
    } else {
        panic!("expected a split");
    }
}

#[test]
fn split_pane_targets_a_nested_pane() {
    let mut tree = split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        pane("b"),
    );
    assert!(split_pane(
        &mut tree,
        "b",
        SessionSplitOrientation::Vertical,
        "c",
        false
    ));
    // The right child became a vertical split of b|c; the root is untouched.
    if let Layout::Split(root) = &tree {
        assert_eq!(count_leaves(&root.second), 2);
        assert_eq!(count_leaves(&root.first), 1);
    } else {
        panic!("expected a split");
    }
    assert_eq!(count_leaves(&tree), 3);
}

#[test]
fn split_pane_returns_false_for_unknown_target() {
    let mut tree = pane("a");
    assert!(!split_pane(
        &mut tree,
        "zzz",
        SessionSplitOrientation::Horizontal,
        "b",
        false
    ));
    assert_eq!(tree, pane("a"));
}

#[test]
fn close_panel_collapses_a_split_into_its_sibling() {
    let mut layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        pane("b"),
    ));
    assert_eq!(close_panel(&mut layout, "a"), CloseOutcome::Removed);
    // The split collapsed to the surviving pane `b`.
    assert_eq!(layout, Some(pane("b")));
}

#[test]
fn close_panel_collapses_deeply_and_preserves_the_far_sibling() {
    // split( a, split( b, c ) ); closing b collapses the inner split to c.
    let mut layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        split(SessionSplitOrientation::Vertical, 0.5, pane("b"), pane("c")),
    ));
    assert_eq!(close_panel(&mut layout, "b"), CloseOutcome::Removed);
    let expected = split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        pane("c"),
    );
    assert_eq!(layout, Some(expected));
}

#[test]
fn close_panel_emptying_the_root_pane_clears_the_layout() {
    let mut layout = Some(pane("a"));
    assert_eq!(close_panel(&mut layout, "a"), CloseOutcome::Emptied);
    assert_eq!(layout, None);
}

#[test]
fn close_panel_removes_one_of_several_tabs_without_collapsing() {
    let mut layout = Some(Layout::Pane(SessionPaneLayoutSnapshot {
        pane_id: None,
        panel_ids: vec!["a".into(), "b".into()],
        selected_panel_id: Some("a".into()),
        surface_kind: None,
        markdown_file_path: None,
        file_path: None,
        diff_viewer_token: None,
        diff_viewer_request_path: None,
        browser_url: None,
        browser_proxy_url: None,
        browser_back_history: None,
        browser_forward_history: None,
        browser_omnibar_visible: None,
        browser_focus_mode_active: None,
        browser_developer_tools_visible: None,
        browser_developer_tools_panel: None,
        browser_page_zoom: None,
    }));
    assert_eq!(close_panel(&mut layout, "a"), CloseOutcome::Removed);
    // Pane survives with `b`, and selection moved off the closed panel.
    if let Some(Layout::Pane(p)) = &layout {
        assert_eq!(p.panel_ids, vec!["b".to_string()]);
        assert_eq!(p.selected_panel_id.as_deref(), Some("b"));
    } else {
        panic!("expected a surviving pane");
    }
}

#[test]
fn select_adjacent_panel_wraps_within_a_multi_panel_pane() {
    let mut tree = Layout::Pane(SessionPaneLayoutSnapshot {
        pane_id: None,
        panel_ids: vec!["a".into(), "b".into(), "c".into()],
        selected_panel_id: Some("c".into()),
        surface_kind: None,
        markdown_file_path: None,
        file_path: None,
        diff_viewer_token: None,
        diff_viewer_request_path: None,
        browser_url: None,
        browser_proxy_url: None,
        browser_back_history: None,
        browser_forward_history: None,
        browser_omnibar_visible: None,
        browser_focus_mode_active: None,
        browser_developer_tools_visible: None,
        browser_developer_tools_panel: None,
        browser_page_zoom: None,
    });
    assert!(select_adjacent_panel(&mut tree, "b", true));
    if let Layout::Pane(pane) = &tree {
        assert_eq!(pane.selected_panel_id.as_deref(), Some("a"));
    } else {
        panic!("expected pane");
    }
}

#[test]
fn select_adjacent_panel_previous_wraps_backward() {
    let mut tree = Layout::Pane(SessionPaneLayoutSnapshot {
        pane_id: None,
        panel_ids: vec!["a".into(), "b".into(), "c".into()],
        selected_panel_id: Some("a".into()),
        surface_kind: None,
        markdown_file_path: None,
        file_path: None,
        diff_viewer_token: None,
        diff_viewer_request_path: None,
        browser_url: None,
        browser_proxy_url: None,
        browser_back_history: None,
        browser_forward_history: None,
        browser_omnibar_visible: None,
        browser_focus_mode_active: None,
        browser_developer_tools_visible: None,
        browser_developer_tools_panel: None,
        browser_page_zoom: None,
    });
    assert!(select_adjacent_panel(&mut tree, "a", false));
    if let Layout::Pane(pane) = &tree {
        assert_eq!(pane.selected_panel_id.as_deref(), Some("c"));
    } else {
        panic!("expected pane");
    }
}

#[test]
fn select_adjacent_panel_is_noop_for_single_panel_or_unknown_panel() {
    let mut tree = pane("a");
    assert!(!select_adjacent_panel(&mut tree, "a", true));
    assert_eq!(tree, pane("a"));
    assert!(!select_adjacent_panel(&mut tree, "missing", true));
    assert_eq!(tree, pane("a"));
}

#[test]
fn select_panel_sets_the_requested_panel_in_its_pane() {
    let mut tree = Layout::Pane(SessionPaneLayoutSnapshot {
        pane_id: None,
        panel_ids: vec!["a".into(), "b".into(), "c".into()],
        selected_panel_id: Some("a".into()),
        surface_kind: None,
        markdown_file_path: None,
        file_path: None,
        diff_viewer_token: None,
        diff_viewer_request_path: None,
        browser_url: None,
        browser_proxy_url: None,
        browser_back_history: None,
        browser_forward_history: None,
        browser_omnibar_visible: None,
        browser_focus_mode_active: None,
        browser_developer_tools_visible: None,
        browser_developer_tools_panel: None,
        browser_page_zoom: None,
    });

    assert!(select_panel(&mut tree, "c"));
    if let Layout::Pane(pane) = &tree {
        assert_eq!(pane.selected_panel_id.as_deref(), Some("c"));
    } else {
        panic!("expected pane");
    }
    assert!(!select_panel(&mut tree, "c"));
    assert!(!select_panel(&mut tree, "missing"));
}

#[test]
fn add_panel_to_pane_inserts_after_anchor_and_selects_new_panel() {
    let mut tree = Layout::Pane(SessionPaneLayoutSnapshot {
        pane_id: Some("pane-1".into()),
        panel_ids: vec!["a".into(), "c".into()],
        selected_panel_id: Some("a".into()),
        surface_kind: None,
        markdown_file_path: None,
        file_path: None,
        diff_viewer_token: None,
        diff_viewer_request_path: None,
        browser_url: None,
        browser_proxy_url: None,
        browser_back_history: None,
        browser_forward_history: None,
        browser_omnibar_visible: None,
        browser_focus_mode_active: None,
        browser_developer_tools_visible: None,
        browser_developer_tools_panel: None,
        browser_page_zoom: None,
    });
    assert!(add_panel_to_pane(&mut tree, "a", "b"));
    if let Layout::Pane(pane) = tree {
        assert_eq!(pane.panel_ids, ["a", "b", "c"]);
        assert_eq!(pane.selected_panel_id.as_deref(), Some("b"));
    } else {
        panic!("expected pane");
    }
}

#[test]
fn close_panel_unknown_id_is_not_found_and_leaves_the_tree() {
    let mut layout = Some(split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        pane("b"),
    ));
    let before = layout.clone();
    assert_eq!(close_panel(&mut layout, "zzz"), CloseOutcome::NotFound);
    assert_eq!(layout, before);
}

#[test]
fn set_divider_at_path_updates_root_and_nested() {
    let mut tree = split(
        SessionSplitOrientation::Horizontal,
        0.5,
        pane("a"),
        split(SessionSplitOrientation::Vertical, 0.5, pane("b"), pane("c")),
    );
    assert!(set_divider_at_path(&mut tree, &[], 0.3));
    assert!(set_divider_at_path(&mut tree, &[SplitChild::Second], 5.0));
    if let Layout::Split(root) = &tree {
        assert_eq!(root.divider_position, 0.3);
        if let Layout::Split(inner) = root.second.as_ref() {
            assert_eq!(inner.divider_position, MAX_DIVIDER); // clamped
        } else {
            panic!("expected nested split");
        }
    } else {
        panic!("expected a split");
    }
}

#[test]
fn set_divider_at_path_off_a_leaf_is_a_noop() {
    let mut tree = pane("a");
    assert!(!set_divider_at_path(&mut tree, &[], 0.3));
    assert_eq!(tree, pane("a"));
}

#[test]
fn set_surface_kind_marks_the_pane_and_survives_a_split() {
    let mut tree = pane("a");
    // Unknown panel → no-op.
    assert!(!set_surface_kind(&mut tree, "zzz", Some("agent".into())));
    // Mark pane `a` as an agent surface.
    assert!(set_surface_kind(&mut tree, "a", Some("agent".into())));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.surface_kind.as_deref(), Some("agent"));
    } else {
        panic!("expected a pane");
    }
    // Splitting keeps `a`'s agent kind on its side; the new pane defaults off.
    assert!(split_pane(
        &mut tree,
        "a",
        SessionSplitOrientation::Horizontal,
        "b",
        false
    ));
    if let Layout::Split(s) = &tree {
        if let Layout::Pane(first) = s.first.as_ref() {
            assert_eq!(first.surface_kind.as_deref(), Some("agent"));
        } else {
            panic!("expected pane a first");
        }
        if let Layout::Pane(second) = s.second.as_ref() {
            assert_eq!(second.surface_kind, None);
        } else {
            panic!("expected pane b second");
        }
    } else {
        panic!("expected a split");
    }
    // Clearing it back to a terminal.
    assert!(set_surface_kind(&mut tree, "a", None));
    if let Layout::Split(s) = &tree {
        if let Layout::Pane(first) = s.first.as_ref() {
            assert_eq!(first.surface_kind, None);
        }
    }
}

#[test]
fn set_markdown_file_path_marks_the_pane_and_survives_a_split() {
    let mut tree = pane("a");
    assert!(set_markdown_file_path(
        &mut tree,
        "a",
        Some("C:/docs/readme.md".into())
    ));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.markdown_file_path.as_deref(), Some("C:/docs/readme.md"));
    } else {
        panic!("expected a pane");
    }
    assert!(split_pane(
        &mut tree,
        "a",
        SessionSplitOrientation::Horizontal,
        "b",
        false
    ));
    if let Layout::Split(s) = &tree {
        if let Layout::Pane(first) = s.first.as_ref() {
            assert_eq!(
                first.markdown_file_path.as_deref(),
                Some("C:/docs/readme.md")
            );
        } else {
            panic!("expected the existing pane to survive as first");
        }
        if let Layout::Pane(second) = s.second.as_ref() {
            assert_eq!(second.markdown_file_path, None);
        } else {
            panic!("expected the new pane as second");
        }
    } else {
        panic!("expected a split");
    }
}

#[test]
fn set_file_path_marks_the_pane_and_survives_a_split() {
    let mut tree = pane("a");
    assert!(set_file_path(
        &mut tree,
        "a",
        Some("C:/docs/notes.txt".into())
    ));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.file_path.as_deref(), Some("C:/docs/notes.txt"));
    } else {
        panic!("expected a pane");
    }
    assert!(split_pane(
        &mut tree,
        "a",
        SessionSplitOrientation::Horizontal,
        "b",
        false
    ));
    if let Layout::Split(s) = &tree {
        if let Layout::Pane(first) = s.first.as_ref() {
            assert_eq!(first.file_path.as_deref(), Some("C:/docs/notes.txt"));
        } else {
            panic!("expected the existing pane to survive as first");
        }
        if let Layout::Pane(second) = s.second.as_ref() {
            assert_eq!(second.file_path, None);
        } else {
            panic!("expected the new pane as second");
        }
    } else {
        panic!("expected a split");
    }
}

#[test]
fn set_diff_viewer_session_marks_the_pane_and_survives_a_split() {
    let mut tree = pane("a");
    assert!(set_diff_viewer_session(
        &mut tree,
        "a",
        Some("tok-abcdef0123456789".into()),
        Some("/index.html".into())
    ));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.diff_viewer_token.as_deref(), Some("tok-abcdef0123456789"));
        assert_eq!(p.diff_viewer_request_path.as_deref(), Some("/index.html"));
    } else {
        panic!("expected a pane");
    }
    assert!(split_pane(
        &mut tree,
        "a",
        SessionSplitOrientation::Horizontal,
        "b",
        false
    ));
    if let Layout::Split(s) = &tree {
        if let Layout::Pane(first) = s.first.as_ref() {
            assert_eq!(
                first.diff_viewer_token.as_deref(),
                Some("tok-abcdef0123456789")
            );
            assert_eq!(
                first.diff_viewer_request_path.as_deref(),
                Some("/index.html")
            );
        } else {
            panic!("expected the existing pane to survive as first");
        }
        if let Layout::Pane(second) = s.second.as_ref() {
            assert_eq!(second.diff_viewer_token, None);
            assert_eq!(second.diff_viewer_request_path, None);
        } else {
            panic!("expected the new pane as second");
        }
    } else {
        panic!("expected a split");
    }
}

#[test]
fn set_browser_state_marks_the_pane_and_survives_a_split() {
    let mut tree = pane("a");
    assert!(set_browser_url(
        &mut tree,
        "a",
        Some("https://example.com".into())
    ));
    assert!(set_browser_page_zoom(&mut tree, "a", Some(1.25)));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_url.as_deref(), Some("https://example.com"));
        assert_eq!(p.browser_page_zoom, Some(1.25));
    } else {
        panic!("expected a pane");
    }
    assert!(split_pane(
        &mut tree,
        "a",
        SessionSplitOrientation::Horizontal,
        "b",
        false
    ));
    if let Layout::Split(s) = &tree {
        if let Layout::Pane(first) = s.first.as_ref() {
            assert_eq!(first.browser_url.as_deref(), Some("https://example.com"));
            assert_eq!(first.browser_page_zoom, Some(1.25));
        } else {
            panic!("expected the existing pane to survive as first");
        }
        if let Layout::Pane(second) = s.second.as_ref() {
            assert_eq!(second.browser_url, None);
            assert_eq!(second.browser_back_history, None);
            assert_eq!(second.browser_forward_history, None);
            assert_eq!(second.browser_omnibar_visible, None);
            assert_eq!(second.browser_focus_mode_active, None);
            assert_eq!(second.browser_developer_tools_visible, None);
            assert_eq!(second.browser_developer_tools_panel, None);
            assert_eq!(second.browser_page_zoom, None);
        } else {
            panic!("expected the new pane as second");
        }
    } else {
        panic!("expected a split");
    }
}

#[test]
fn browser_history_can_be_cleared_without_losing_the_current_url() {
    let mut tree = pane("a");
    assert!(navigate_browser(
        &mut tree,
        "a",
        "https://one.example".into()
    ));
    assert!(navigate_browser(
        &mut tree,
        "a",
        "https://two.example".into()
    ));
    assert!(browser_go_back(&mut tree, "a"));

    assert!(clear_browser_history(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_url.as_deref(), Some("https://one.example/"));
        assert_eq!(p.browser_back_history, None);
        assert_eq!(p.browser_forward_history, None);
    } else {
        panic!("expected a pane");
    }
    assert!(!clear_browser_history(&mut tree, "a"));
}

#[test]
fn browser_omnibar_visibility_toggles_from_visible_default() {
    let mut tree = pane("a");
    assert!(toggle_browser_omnibar_visible(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_omnibar_visible, Some(false));
    } else {
        panic!("expected a pane");
    }
    assert!(toggle_browser_omnibar_visible(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_omnibar_visible, Some(true));
    } else {
        panic!("expected a pane");
    }
}

#[test]
fn browser_focus_mode_toggles_from_inactive_default() {
    let mut tree = pane("a");
    assert!(toggle_browser_focus_mode(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_focus_mode_active, Some(true));
    } else {
        panic!("expected a pane");
    }
    assert!(toggle_browser_focus_mode(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_focus_mode_active, Some(false));
    } else {
        panic!("expected a pane");
    }
}

#[test]
fn browser_developer_tools_toggle_and_panel_selection_persist() {
    let mut tree = pane("a");
    assert!(toggle_browser_developer_tools(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_developer_tools_visible, Some(true));
        assert_eq!(
            p.browser_developer_tools_panel.as_deref(),
            Some("inspector")
        );
    } else {
        panic!("expected a pane");
    }

    assert!(show_browser_developer_tools(&mut tree, "a", "console"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_developer_tools_visible, Some(true));
        assert_eq!(p.browser_developer_tools_panel.as_deref(), Some("console"));
    } else {
        panic!("expected a pane");
    }

    assert!(toggle_browser_developer_tools(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_developer_tools_visible, Some(false));
        assert_eq!(p.browser_developer_tools_panel.as_deref(), Some("console"));
    } else {
        panic!("expected a pane");
    }
}

#[test]
fn browser_navigation_history_round_trips_back_and_forward() {
    let mut tree = pane("a");
    assert!(navigate_browser(
        &mut tree,
        "a",
        "https://one.example".into()
    ));
    assert!(navigate_browser(
        &mut tree,
        "a",
        "https://two.example".into()
    ));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_url.as_deref(), Some("https://two.example"));
        assert_eq!(
            p.browser_back_history.as_deref(),
            Some(["https://one.example/".to_string()].as_slice())
        );
        assert_eq!(p.browser_forward_history, None);
    } else {
        panic!("expected a pane");
    }

    assert!(browser_go_back(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_url.as_deref(), Some("https://one.example/"));
        assert_eq!(p.browser_back_history, None);
        assert_eq!(
            p.browser_forward_history.as_deref(),
            Some(["https://two.example/".to_string()].as_slice())
        );
    } else {
        panic!("expected a pane");
    }

    assert!(browser_go_forward(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_url.as_deref(), Some("https://two.example/"));
        assert_eq!(
            p.browser_back_history.as_deref(),
            Some(["https://one.example/".to_string()].as_slice())
        );
        assert_eq!(p.browser_forward_history, None);
    } else {
        panic!("expected a pane");
    }
}

#[test]
fn browser_navigation_history_uses_session_history_sanitizer() {
    let mut tree = pane("a");
    assert!(navigate_browser(
        &mut tree,
        "a",
        "cmux-diff-viewer://tok-abcdef0123456789/index.html".into()
    ));
    assert!(navigate_browser(
        &mut tree,
        "a",
        "https://two.example/path".into()
    ));
    assert!(navigate_browser(&mut tree, "a", "about:blank".into()));
    if let Layout::Pane(p) = &mut tree {
        assert_eq!(
            p.browser_back_history.as_deref(),
            Some(["https://two.example/path".to_string()].as_slice())
        );
        p.browser_back_history = Some(vec![
            "about:blank".to_string(),
            "http://cmux-diff-viewer.localhost/tok-abcdef0123456789/index.html".to_string(),
            "https://valid.example".to_string(),
        ]);
        p.browser_forward_history = Some(vec![
            "cmux-remote-image://img?url=https://x.test/a.png".to_string(),
        ]);
    } else {
        panic!("expected a pane");
    }

    let Layout::Pane(p) = &tree else {
        panic!("expected a pane");
    };
    assert_eq!(
        browser_navigation_availability(
            p.browser_back_history.as_deref(),
            p.browser_forward_history.as_deref(),
        ),
        NavigationAvailability::new(true, false)
    );

    assert!(browser_go_back(&mut tree, "a"));
    if let Layout::Pane(p) = &tree {
        assert_eq!(p.browser_url.as_deref(), Some("https://valid.example/"));
        assert_eq!(p.browser_back_history, None);
    } else {
        panic!("expected a pane");
    }
}

#[test]
fn split_child_serializes_lowercase_matching_the_web_path() {
    assert_eq!(
        serde_json::to_string(&SplitChild::First).unwrap(),
        "\"first\""
    );
    assert_eq!(
        serde_json::to_string(&SplitChild::Second).unwrap(),
        "\"second\""
    );
    let path: Vec<SplitChild> = serde_json::from_str("[\"first\",\"second\"]").unwrap();
    assert_eq!(path, vec![SplitChild::First, SplitChild::Second]);
}
