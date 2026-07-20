// --- Workspace reorder ---

const R_W1: &str = "00000000-0000-0000-0000-000000000001";
const R_W2: &str = "00000000-0000-0000-0000-000000000002";
const R_W3: &str = "00000000-0000-0000-0000-000000000003";
const R_W4: &str = "00000000-0000-0000-0000-000000000004";
const R_G1: &str = "11111111-0000-0000-0000-000000000001";
const R_G2: &str = "11111111-0000-0000-0000-000000000002";
/// Parseable UUID that never appears in `workspace_groups` (dangling).
const R_G_DANGLING: &str = "dddddddd-0000-0000-0000-000000000001";

/// Workspaces with fixed UUID ids; panel id mirrors the position (for
/// `order_of`) and each spec is `(workspace_id, group_id, pinned)`.
fn reorder_tabs(
    specs: &[(&str, Option<&str>, bool)],
    selected: Option<i64>,
) -> SessionTabManagerSnapshot {
    let workspaces = specs
        .iter()
        .enumerate()
        .map(|(i, (id, gid, pinned))| SessionWorkspaceSnapshot {
            workspace_id: Some(id.to_string()),
            group_id: gid.map(str::to_string),
            is_pinned: pinned.then_some(true),
            ..fresh_terminal_workspace(&format!("surface-{i}"))
        })
        .collect();
    SessionTabManagerSnapshot {
        selected_workspace_index: selected,
        workspaces,
        workspace_groups: None,
    }
}

fn ws_id_order(tabs: &SessionTabManagerSnapshot) -> Vec<&str> {
    tabs.workspaces
        .iter()
        .map(|w| w.workspace_id.as_deref().unwrap_or(""))
        .collect()
}

fn uuid(raw: &str) -> Uuid {
    Uuid::parse_str(raw).unwrap()
}

#[test]
fn notification_reorder_moves_unpinned_workspace_to_its_tier_boundary() {
    let mut tabs = reorder_tabs(
        &[
            (R_W1, None, true),
            (R_W2, None, false),
            (R_W3, None, false),
        ],
        Some(0),
    );

    assert!(move_workspace_to_top_for_notification(&mut tabs, 2));
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2]);
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn notification_reorder_is_a_no_op_for_pinned_and_boundary_workspaces() {
    let mut tabs = reorder_tabs(
        &[
            (R_W1, None, true),
            (R_W2, None, false),
            (R_W3, None, false),
        ],
        Some(2),
    );
    let before = serde_json::to_string(&tabs).unwrap();

    assert!(!move_workspace_to_top_for_notification(&mut tabs, 0));
    assert!(!move_workspace_to_top_for_notification(&mut tabs, 1));
    assert_eq!(serde_json::to_string(&tabs).unwrap(), before);
}

#[test]
fn notification_reorder_hoists_an_unpinned_group_as_one_top_level_row() {
    let mut tabs = reorder_tabs(
        &[
            (R_W1, None, true),
            (R_W4, None, false),
            (R_W2, Some(R_G1), false),
            (R_W3, Some(R_G1), false),
        ],
        Some(1),
    );
    tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W2, false)]);

    assert!(move_workspace_to_top_for_notification(&mut tabs, 3));
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W2, R_W3, R_W4]);
    assert_eq!(tabs.selected_workspace_index, Some(3));
}

#[test]
fn create_workspace_group_adopts_eligible_children_at_first_child_slot() {
    let mut tabs = reorder_tabs(
        &[
            (R_W1, None, false),
            (R_W2, None, true),
            (R_W3, None, false),
            (R_W4, None, false), // fresh anchor, appended by the host
        ],
        Some(2),
    );

    let created = create_workspace_group_snapshot(
        &mut tabs,
        uuid(R_G1),
        "  Team  ",
        uuid(R_W4),
        &[uuid(R_W3), uuid(R_W2), uuid(R_W1)],
    )
    .unwrap();

    // Pinned W2 is ineligible. Eligible children retain tab order rather
    // than request order, and the run replaces the first child's slot.
    assert_eq!(ws_id_order(&tabs), [R_W2, R_W4, R_W1, R_W3]);
    assert_eq!(tabs.selected_workspace_index, Some(3));
    assert_eq!(created.name, "Team");
    assert_eq!(created.anchor_workspace_id.as_deref(), Some(R_W4));
    assert_eq!(
        tabs.workspaces
            .iter()
            .map(|workspace| workspace.group_id.as_deref())
            .collect::<Vec<_>>(),
        [None, Some(R_G1), Some(R_G1), Some(R_G1)]
    );
}

#[test]
fn add_workspace_rejects_foreign_anchor_and_expands_for_selected_member() {
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G1), false),
            (R_W2, Some(R_G1), false),
            (R_W3, Some(R_G2), false),
            (R_W4, None, false),
        ],
        Some(3),
    );
    tabs.workspace_groups = Some(vec![
        SessionWorkspaceGroupSnapshot {
            is_collapsed: true,
            ..reorder_group(R_G1, R_W1, false)
        },
        reorder_group(R_G2, R_W3, false),
    ]);
    let before = tabs.clone();
    assert_eq!(
        add_workspace_to_group_snapshot(&mut tabs, uuid(R_G1), uuid(R_W3), None, None),
        Err(WorkspaceGroupMutationError::WorkspaceIsOtherGroupAnchor)
    );
    assert_eq!(tabs, before);

    assert_eq!(
        add_workspace_to_group_snapshot(
            &mut tabs,
            uuid(R_G1),
            uuid(R_W4),
            Some(WorkspaceGroupPlacement::Top),
            None,
        ),
        Ok(true)
    );
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W4, R_W2, R_W3]);
    assert_eq!(tabs.selected_workspace_index, Some(1));
    assert!(!tabs.workspace_groups.as_ref().unwrap()[0].is_collapsed);
}

#[test]
fn remove_member_normalizes_but_remove_anchor_flattens_in_place() {
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G1), false),
            (R_W2, Some(R_G1), false),
            (R_W3, None, false),
        ],
        Some(1),
    );
    tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
    assert_eq!(
        remove_workspace_from_group_snapshot(&mut tabs, uuid(R_W2)),
        Ok(true)
    );
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W2, R_W3]);
    assert_eq!(tabs.workspaces[1].group_id, None);
    assert_eq!(tabs.selected_workspace_index, Some(1));

    tabs.workspaces[1].group_id = Some(R_G1.to_string());
    assert_eq!(
        remove_workspace_from_group_snapshot(&mut tabs, uuid(R_W1)),
        Ok(true)
    );
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W2, R_W3]);
    assert!(tabs
        .workspaces
        .iter()
        .all(|workspace| workspace.group_id.is_none()));
    assert!(tabs.workspace_groups.as_ref().unwrap().is_empty());
}

#[test]
fn group_anchor_pin_metadata_and_slot_move_preserve_selection_identity() {
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G1), false),
            (R_W2, Some(R_G1), false),
            (R_W3, None, false),
            (R_W4, Some(R_G2), false),
        ],
        Some(1),
    );
    tabs.workspace_groups = Some(vec![
        reorder_group(R_G1, R_W1, false),
        reorder_group(R_G2, R_W4, false),
    ]);
    assert_eq!(
        set_workspace_group_anchor_snapshot(&mut tabs, uuid(R_G1), uuid(R_W2)),
        Ok(true)
    );
    assert_eq!(ws_id_order(&tabs), [R_W2, R_W1, R_W3, R_W4]);
    assert_eq!(tabs.selected_workspace_index, Some(0));
    assert_eq!(
        rename_workspace_group_snapshot(&mut tabs, uuid(R_G1), "  Renamed  "),
        Ok(true)
    );
    assert_eq!(
        set_workspace_group_color_snapshot(&mut tabs, uuid(R_G1), Some("#123456".into())),
        Ok(true)
    );
    assert_eq!(
        set_workspace_group_icon_snapshot(&mut tabs, uuid(R_G1), Some("folder.fill".into())),
        Ok(true)
    );

    assert_eq!(
        move_workspace_group_snapshot(&mut tabs, uuid(R_G2), 0),
        Ok(true)
    );
    // The ungrouped W3 retains the only ungrouped top-level slot while G2
    // and G1 exchange the two group slots.
    assert_eq!(ws_id_order(&tabs), [R_W4, R_W3, R_W2, R_W1]);
    assert_eq!(tabs.selected_workspace_index, Some(2));
    assert_eq!(
        tabs.workspace_groups
            .as_ref()
            .unwrap()
            .iter()
            .map(|group| group.id.as_str())
            .collect::<Vec<_>>(),
        [R_G2, R_G1]
    );
}

#[test]
fn reorder_workspaces_many_plans_dry_run_and_applies_atomically() {
    let ordered = [
        Uuid::parse_str(R_W3).unwrap(),
        Uuid::parse_str(R_W2).unwrap(),
    ];
    let mut tabs = reorder_tabs(
        &[(R_W1, None, true), (R_W2, None, false), (R_W3, None, false)],
        Some(1),
    );
    let before = tabs.clone();

    let dry_plan = reorder_workspaces_many(&mut tabs, &ordered, true).unwrap();
    assert_eq!(
        dry_plan,
        vec![
            WorkspaceReorderPlanItem::new(ordered[0], 2, 1),
            WorkspaceReorderPlanItem::new(ordered[1], 1, 2),
        ]
    );
    assert_eq!(tabs, before);

    let applied_plan = reorder_workspaces_many(&mut tabs, &ordered, false).unwrap();
    assert_eq!(applied_plan, dry_plan);
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2]);
    assert_eq!(tabs.selected_workspace_index, Some(2));

    let before_duplicate = tabs.clone();
    assert_eq!(
        reorder_workspaces_many(&mut tabs, &[ordered[0], ordered[0]], false),
        Err(WorkspaceBatchReorderError::DuplicateWorkspace(ordered[0]))
    );
    assert_eq!(tabs, before_duplicate);
}

#[test]
fn reorder_workspaces_many_restores_group_contiguity_and_anchor_order() {
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G1), false),
            (R_W2, Some(R_G1), false),
            (R_W3, None, false),
        ],
        Some(0),
    );
    tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
    let ordered = [
        Uuid::parse_str(R_W3).unwrap(),
        Uuid::parse_str(R_W2).unwrap(),
    ];

    reorder_workspaces_many(&mut tabs, &ordered, false).unwrap();
    assert_eq!(ws_id_order(&tabs), [R_W3, R_W1, R_W2]);
    assert_eq!(tabs.selected_workspace_index, Some(1));
}

fn reorder_group(
    id: &str,
    anchor: &str,
    pinned: bool,
) -> crate::session::SessionWorkspaceGroupSnapshot {
    crate::session::SessionWorkspaceGroupSnapshot {
        id: id.to_string(),
        name: "G".to_string(),
        anchor_workspace_id: Some(anchor.to_string()),
        // Some(true)/None convention (byte-stability, matching workspaces).
        is_pinned: pinned.then_some(true),
        ..Default::default()
    }
}

#[test]
fn reorder_unpinned_mover_clamps_below_pinned_prefix() {
    // Cross-tier attempt clamps to pinnedCount, never crosses.
    let mut tabs = reorder_tabs(
        &[
            (R_W1, None, true),
            (R_W2, None, true),
            (R_W3, None, false),
            (R_W4, None, false),
        ],
        Some(0),
    );
    assert!(reorder_workspaces(&mut tabs, 3, 0));
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W2, R_W4, R_W3]);
}

#[test]
fn reorder_pinned_mover_clamps_into_pinned_tier() {
    // Pinned mover dragged past the boundary clamps to pinnedCount-1.
    let mut tabs = reorder_tabs(
        &[
            (R_W1, None, true),
            (R_W2, None, true),
            (R_W3, None, false),
            (R_W4, None, false),
        ],
        Some(0),
    );
    assert!(reorder_workspaces(&mut tabs, 0, 3));
    assert_eq!(ws_id_order(&tabs), [R_W2, R_W1, R_W3, R_W4]);
}

#[test]
fn reorder_boundary_counts_grouped_rows_by_group_pin() {
    // Leading grouped members of a PINNED group (members' own is_pinned is
    // None) count as pinned rows (isGlobalPinnedRow parity, regression
    // sibling of `boundary_counts_grouped_rows_by_group_pin`).
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G1), false),
            (R_W2, Some(R_G1), false),
            (R_W3, None, false),
            (R_W4, None, false),
        ],
        Some(0),
    );
    tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, true)]);
    assert!(reorder_workspaces(&mut tabs, 3, 0));
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W2, R_W4, R_W3]);
}

#[test]
fn reorder_boundary_dangling_group_id_falls_back_to_own_flag() {
    // A leading row whose group_id resolves to NO group counts by its own
    // pin flag (Ordering.swift:201-207 nil-group arm), and its dangling
    // group_id string survives the reorder untouched.
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G_DANGLING), true),
            (R_W2, None, false),
            (R_W3, None, false),
        ],
        Some(0),
    );
    assert!(reorder_workspaces(&mut tabs, 2, 0));
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2]);
    assert_eq!(tabs.workspaces[0].group_id.as_deref(), Some(R_G_DANGLING));
}

#[test]
fn reorder_grouped_member_confined_to_section() {
    // Unpinned member section clamp: [firstIndex+1 .. lastIndex].
    let specs: &[(&str, Option<&str>, bool)] = &[
        (R_W1, Some(R_G1), false),
        (R_W2, Some(R_G1), false),
        (R_W3, Some(R_G1), false),
        (R_W4, None, false),
    ];
    // Toward 0: clamps to firstIndex+1 == from → no-op, byte-identical
    // (must not normalize, Coordinator:111-118).
    let mut tabs = reorder_tabs(specs, Some(0));
    tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
    let before = serde_json::to_string(&tabs).unwrap();
    assert!(!reorder_workspaces(&mut tabs, 1, 0));
    assert_eq!(serde_json::to_string(&tabs).unwrap(), before);
    // Toward 999: clamps to lastIndex (2), stays inside the section.
    assert!(reorder_workspaces(&mut tabs, 1, 999));
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2, R_W4]);
}

#[test]
fn reorder_pinned_member_clamps_into_pinned_subtier() {
    // Pinned member sub-tier: [firstIndex+1 .. firstIndex+pinnedMemberCount].
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G1), false), // anchor
            (R_W2, Some(R_G1), true),  // pinned member
            (R_W3, Some(R_G1), true),  // pinned member
            (R_W4, Some(R_G1), false), // unpinned member
        ],
        Some(0),
    );
    tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
    // Pinned member dragged to 999 clamps to firstIndex+pinnedMemberCount (2).
    assert!(reorder_workspaces(&mut tabs, 1, 999));
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2, R_W4]);
}

#[test]
fn reorder_anchor_moves_whole_group_and_syncs_group_order() {
    // Router: an anchor mover takes the TOP-LEVEL path (`to_index` is a
    // top-level row index) and relocates ALL members contiguously,
    // anchor-first, with relative member order preserved; the groups array
    // syncs to the new anchor order.
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G1), false), // anchor of g1
            (R_W2, Some(R_G1), false),
            (R_W3, Some(R_G2), false), // anchor of g2
            (R_W4, Some(R_G2), false),
        ],
        Some(0),
    );
    tabs.workspace_groups = Some(vec![
        reorder_group(R_G1, R_W1, false),
        reorder_group(R_G2, R_W3, false),
    ]);
    // Mover = tabs index 2 (anchor R_W3); target = top-level index 0.
    assert!(reorder_workspaces(&mut tabs, 2, 0));
    assert_eq!(ws_id_order(&tabs), [R_W3, R_W4, R_W1, R_W2]);
    let groups = tabs.workspace_groups.as_ref().unwrap();
    assert_eq!(
        groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
        [R_G2, R_G1]
    );
}

#[test]
fn reorder_grouped_child_with_top_level_rows_promotes_it_out_of_the_group() {
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G1), false), // anchor
            (R_W2, Some(R_G1), false), // grouped child
            (R_W3, None, false),       // ungrouped
        ],
        Some(1),
    );
    tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);

    // Top-level drag lane: the grouped child is promoted to top-level row
    // space, then moved after the ungrouped row.
    assert!(reorder_workspaces_with_mode(&mut tabs, 1, 2, true));
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2]);
    assert_eq!(tabs.workspaces[2].group_id, None);
    assert_eq!(tabs.selected_workspace_index, Some(2));
    // The surviving group keeps its anchor and order.
    let groups = tabs.workspace_groups.as_ref().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].anchor_workspace_id.as_deref(), Some(R_W1));
}

#[test]
fn reorder_selection_follows_mover_and_displaced_rows() {
    // Selection on the mover follows it to its landing index.
    let mut tabs = reorder_tabs(
        &[
            (R_W1, None, false),
            (R_W2, None, false),
            (R_W3, None, false),
        ],
        Some(2),
    );
    assert!(reorder_workspaces(&mut tabs, 2, 0));
    assert_eq!(ws_id_order(&tabs), [R_W3, R_W1, R_W2]);
    assert_eq!(tabs.selected_workspace_index, Some(0));

    // Selection on a displaced neighbor keeps pointing at the same
    // workspace after it shifts.
    let mut tabs = reorder_tabs(
        &[
            (R_W1, None, false),
            (R_W2, None, false),
            (R_W3, None, false),
        ],
        Some(0),
    );
    assert!(reorder_workspaces(&mut tabs, 2, 0));
    assert_eq!(tabs.selected_workspace_index, Some(1));

    // None / out-of-range selection stays untouched.
    let mut tabs = reorder_tabs(&[(R_W1, None, false), (R_W2, None, false)], None);
    assert!(reorder_workspaces(&mut tabs, 1, 0));
    assert_eq!(tabs.selected_workspace_index, None);
    let mut tabs = reorder_tabs(&[(R_W1, None, false), (R_W2, None, false)], Some(99));
    assert!(reorder_workspaces(&mut tabs, 1, 0));
    assert_eq!(tabs.selected_workspace_index, Some(99));
}

#[test]
fn reorder_no_op_cases_return_false_and_snapshot_is_byte_identical() {
    let specs: &[(&str, Option<&str>, bool)] =
        &[(R_W1, None, true), (R_W2, None, false), (R_W3, None, false)];
    let mut tabs = reorder_tabs(specs, Some(1));
    let before = serde_json::to_string(&tabs).unwrap();
    // Same index (clamps to itself).
    assert!(!reorder_workspaces(&mut tabs, 2, 2));
    // Out-of-range / negative mover index.
    assert!(!reorder_workspaces(&mut tabs, 3, 0));
    assert!(!reorder_workspaces(&mut tabs, -1, 0));
    // Unpinned mover at the boundary asked past it clamps back to `from`
    // (pinnedCount = 1, mover already at index 1).
    assert!(!reorder_workspaces(&mut tabs, 1, 0));
    assert_eq!(serde_json::to_string(&tabs).unwrap(), before);

    // Single workspace.
    let mut solo = reorder_tabs(&[(R_W1, None, false)], Some(0));
    let before = serde_json::to_string(&solo).unwrap();
    assert!(!reorder_workspaces(&mut solo, 0, 0));
    assert_eq!(serde_json::to_string(&solo).unwrap(), before);
}

#[test]
fn reorder_reverted_by_normalization_returns_false() {
    // An ungrouped row nudged into the middle of a group's section snaps
    // back out via normalization — the changed-gate reports false and the
    // snapshot stays byte-identical.
    let mut tabs = reorder_tabs(
        &[
            (R_W1, Some(R_G1), false), // anchor
            (R_W2, Some(R_G1), false), // member
            (R_W3, None, false),       // ungrouped
        ],
        Some(0),
    );
    tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
    let before = serde_json::to_string(&tabs).unwrap();
    assert!(!reorder_workspaces(&mut tabs, 2, 1));
    assert_eq!(serde_json::to_string(&tabs).unwrap(), before);
}

#[test]
fn reorder_id_less_rows_move_positionally() {
    // workspace_id None rows get MINTED mirror ids (positional identity)
    // and still reorder; their serialized objects stay untouched (no id is
    // written back).
    let mut tabs = SessionTabManagerSnapshot {
        selected_workspace_index: Some(0),
        workspaces: (0..3)
            .map(|i| fresh_terminal_workspace(&format!("surface-{i}")))
            .collect(),
        workspace_groups: None,
    };
    let before: Vec<String> = tabs
        .workspaces
        .iter()
        .map(|w| serde_json::to_string(w).unwrap())
        .collect();
    assert!(reorder_workspaces(&mut tabs, 2, 0));
    assert_eq!(order_of(&tabs), ["surface-2", "surface-0", "surface-1"]);
    let after: Vec<String> = tabs
        .workspaces
        .iter()
        .map(|w| serde_json::to_string(w).unwrap())
        .collect();
    assert_eq!(
        after,
        [before[2].clone(), before[0].clone(), before[1].clone()]
    );
    assert!(tabs.workspaces.iter().all(|w| w.workspace_id.is_none()));
}

#[test]
fn reorder_preserves_each_object_byte_for_byte() {
    // Only ARRAY ORDER may change: every workspace/group object's own
    // serialization must equal its pre-move serialization (is_pinned
    // Some(true)/None convention and dangling group_id strings untouched).
    let mut tabs = reorder_tabs(
        &[
            (R_W1, None, true),
            (R_W2, Some(R_G1), false), // anchor
            (R_W3, Some(R_G1), false), // member
            (R_W4, Some(R_G_DANGLING), false),
        ],
        Some(0),
    );
    tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W2, false)]);
    let ws_before: std::collections::HashMap<String, String> = tabs
        .workspaces
        .iter()
        .map(|w| {
            (
                w.workspace_id.clone().unwrap(),
                serde_json::to_string(w).unwrap(),
            )
        })
        .collect();
    let group_before =
        serde_json::to_string(&tabs.workspace_groups.as_ref().unwrap()[0]).unwrap();
    // Move the dangling-group row (index 3) up; clamps to the unpinned
    // boundary (1).
    assert!(reorder_workspaces(&mut tabs, 3, 1));
    assert_eq!(ws_id_order(&tabs), [R_W1, R_W4, R_W2, R_W3]);
    for w in &tabs.workspaces {
        assert_eq!(
            serde_json::to_string(w).unwrap(),
            ws_before[w.workspace_id.as_deref().unwrap()]
        );
    }
    assert_eq!(
        serde_json::to_string(&tabs.workspace_groups.as_ref().unwrap()[0]).unwrap(),
        group_before
    );
}

// --- Workspace-group collapse ---

fn group(id: &str, is_collapsed: bool) -> crate::session::SessionWorkspaceGroupSnapshot {
    crate::session::SessionWorkspaceGroupSnapshot {
        id: id.to_string(),
        name: id.to_uppercase(),
        is_collapsed,
        ..Default::default()
    }
}

fn tabs_with_group(is_collapsed: bool) -> SessionTabManagerSnapshot {
    let mut tabs = one_workspace_tabs("surface-1");
    tabs.workspace_groups = Some(vec![group("g", is_collapsed)]);
    tabs
}

#[test]
fn set_group_collapsed_collapses_a_group() {
    let mut tabs = tabs_with_group(false);
    assert!(set_group_collapsed(&mut tabs, "g", true));
    assert!(tabs.workspace_groups.as_ref().unwrap()[0].is_collapsed);
    assert_eq!(tabs.selected_workspace_index, Some(0));
}

#[test]
fn set_group_collapsed_expands_a_group() {
    let mut tabs = tabs_with_group(true);
    assert!(set_group_collapsed(&mut tabs, "g", false));
    assert!(!tabs.workspace_groups.as_ref().unwrap()[0].is_collapsed);
}

#[test]
fn set_group_collapsed_same_value_is_a_no_op() {
    let mut tabs = tabs_with_group(false);
    let before = tabs.clone();
    assert!(!set_group_collapsed(&mut tabs, "g", false));
    assert_eq!(tabs, before);
}

#[test]
fn set_group_collapsed_unknown_group_is_a_no_op() {
    let mut tabs = tabs_with_group(false);
    let before = tabs.clone();
    assert!(!set_group_collapsed(&mut tabs, "nope", true));
    assert_eq!(tabs, before);
}

#[test]
fn set_group_collapsed_with_no_groups_is_a_no_op() {
    // `None` groups must stay `None` — never materialize `Some(vec![])`.
    let mut tabs = one_workspace_tabs("surface-1");
    assert!(!set_group_collapsed(&mut tabs, "g", true));
    assert_eq!(tabs.workspace_groups, None);
}

#[test]
fn set_group_collapsed_targets_only_the_named_group() {
    let mut tabs = one_workspace_tabs("surface-1");
    tabs.workspace_groups = Some(vec![group("g1", false), group("g2", false)]);
    assert!(set_group_collapsed(&mut tabs, "g2", true));
    let groups = tabs.workspace_groups.as_ref().unwrap();
    assert!(!groups[0].is_collapsed);
    assert!(groups[1].is_collapsed);
}

// Pins the canonical pure-data contract (WorkspaceGroupCoordinator.swift:405-407)
// against drift toward the UI toggle's anchor-select semantics: collapsing a
// group whose selected member is a NON-anchor must not move selection.
#[test]
fn set_group_collapsed_never_moves_selection() {
    let mut tabs = tabs_with(2, 0, 1);
    tabs.workspaces[0].workspace_id = Some("ws-anchor".to_string());
    tabs.workspaces[0].group_id = Some("g".to_string());
    tabs.workspaces[1].workspace_id = Some("ws-member".to_string());
    tabs.workspaces[1].group_id = Some("g".to_string());
    tabs.workspace_groups = Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
        anchor_workspace_id: Some("ws-anchor".to_string()),
        ..group("g", false)
    }]);
    assert!(set_group_collapsed(&mut tabs, "g", true));
    // Selection stays on the (now hidden-in-UI) non-anchor member.
    assert_eq!(tabs.selected_workspace_index, Some(1));
}
