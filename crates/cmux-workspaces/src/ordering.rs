//! Port of `Model/WorkspacesModel+Ordering.swift` — pure sidebar ordering reads
//! and reorder-index clamps.
//!
//! DIVERGENCE: the Swift methods read `self.tabs` / `self.workspaceGroups`; the
//! Rust ports take those as explicit `&[WorkspaceRow]` / `&[WorkspaceGroup]`
//! slices. Index math uses `i64` to mirror Swift's signed `Int` clamping
//! (`max(0, min(...))` with a possibly-`-1` `count - 1`).

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::group::WorkspaceGroup;
use crate::row::WorkspaceRow;

fn groups_by_id(groups: &[WorkspaceGroup]) -> HashMap<Uuid, &WorkspaceGroup> {
    groups.iter().map(|g| (g.id, g)).collect()
}

/// Whether the workspace anchors any group.
pub fn is_workspace_group_anchor(groups: &[WorkspaceGroup], workspace_id: Uuid) -> bool {
    groups.iter().any(|g| g.anchor_workspace_id == workspace_id)
}

/// The top-level sidebar row id for each given workspace (its group's anchor
/// when grouped, itself when ungrouped), deduplicated in order.
pub fn top_level_workspace_ids(
    workspaces: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
) -> Vec<Uuid> {
    let by_id = groups_by_id(groups);
    let mut emitted: HashSet<Uuid> = HashSet::new();
    let mut ids: Vec<Uuid> = Vec::with_capacity(workspaces.len());
    for workspace in workspaces {
        let top_level_id = match workspace.group_id.and_then(|gid| by_id.get(&gid)) {
            Some(group) => group.anchor_workspace_id,
            None => workspace.id,
        };
        if emitted.insert(top_level_id) {
            ids.push(top_level_id);
        }
    }
    ids
}

/// The sidebar's top-level row ids in tab order (group anchors and ungrouped
/// workspaces). Optionally inserts a grouped workspace being promoted to top
/// level right after its group's row.
pub fn sidebar_top_level_workspace_ids(
    tabs: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    promoting_workspace_id: Option<Uuid>,
) -> Vec<Uuid> {
    let by_id = groups_by_id(groups);
    let mut emitted_group_ids: HashSet<Uuid> = HashSet::new();
    let mut ids: Vec<Uuid> = Vec::with_capacity(tabs.len());
    for tab in tabs {
        match tab.group_id.and_then(|gid| by_id.get(&gid).map(|g| (gid, g))) {
            Some((gid, group)) => {
                if emitted_group_ids.insert(gid) {
                    ids.push(group.anchor_workspace_id);
                }
            }
            None => ids.push(tab.id),
        }
    }
    if let Some(promoted) = promoting_workspace_id {
        if !ids.contains(&promoted) {
            if let Some(tab) = tabs.iter().find(|t| t.id == promoted) {
                if let Some(group) = tab.group_id.and_then(|gid| by_id.get(&gid)) {
                    if let Some(group_index) =
                        ids.iter().position(|id| *id == group.anchor_workspace_id)
                    {
                        let insert_at = (group_index + 1).min(ids.len());
                        ids.insert(insert_at, promoted);
                    }
                }
            }
        }
    }
    ids
}

#[allow(clippy::too_many_arguments)]
fn append_top_level_id(
    id: Uuid,
    tabs_by_id: &HashMap<Uuid, &WorkspaceRow>,
    by_id: &HashMap<Uuid, &WorkspaceGroup>,
    emitted_workspace_ids: &mut HashSet<Uuid>,
    emitted_group_ids: &mut HashSet<Uuid>,
    ids: &mut Vec<Uuid>,
) {
    let Some(tab) = tabs_by_id.get(&id) else {
        return;
    };
    if !emitted_workspace_ids.insert(tab.id) {
        return;
    }
    match tab.group_id.and_then(|gid| by_id.get(&gid).map(|g| (gid, g))) {
        Some((gid, group)) => {
            if emitted_group_ids.insert(gid) {
                ids.push(group.anchor_workspace_id);
            }
        }
        None => ids.push(tab.id),
    }
}

/// Projects a desired full workspace-id order down to top-level row ids,
/// appending any unmentioned workspaces in tab order.
pub fn top_level_workspace_ids_preserving_order(
    tabs: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    desired_ids: &[Uuid],
) -> Vec<Uuid> {
    let by_id = groups_by_id(groups);
    let tabs_by_id: HashMap<Uuid, &WorkspaceRow> = tabs.iter().map(|t| (t.id, t)).collect();
    let mut emitted_workspace_ids: HashSet<Uuid> = HashSet::new();
    let mut emitted_group_ids: HashSet<Uuid> = HashSet::new();
    let mut ids: Vec<Uuid> = Vec::with_capacity(tabs.len());

    for id in desired_ids {
        append_top_level_id(
            *id,
            &tabs_by_id,
            &by_id,
            &mut emitted_workspace_ids,
            &mut emitted_group_ids,
            &mut ids,
        );
    }
    for tab in tabs {
        if !emitted_workspace_ids.contains(&tab.id) {
            append_top_level_id(
                tab.id,
                &tabs_by_id,
                &by_id,
                &mut emitted_workspace_ids,
                &mut emitted_group_ids,
                &mut ids,
            );
        }
    }
    ids
}

/// The pinned subset of the top-level rows (pinned groups by group pin,
/// ungrouped workspaces by workspace pin).
pub fn sidebar_top_level_pinned_workspace_ids(
    tabs: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
) -> HashSet<Uuid> {
    let groups_by_anchor_id: HashMap<Uuid, &WorkspaceGroup> =
        groups.iter().map(|g| (g.anchor_workspace_id, g)).collect();
    let tabs_by_id: HashMap<Uuid, &WorkspaceRow> = tabs.iter().map(|t| (t.id, t)).collect();
    sidebar_top_level_workspace_ids(tabs, groups, None)
        .into_iter()
        .filter(|id| {
            if let Some(group) = groups_by_anchor_id.get(id) {
                group.is_pinned
            } else {
                tabs_by_id.get(id).map(|t| t.is_pinned) == Some(true)
            }
        })
        .collect()
}

/// Clamps a requested top-level reorder index into the mover's pin tier.
pub fn clamped_top_level_reorder_index(
    tabs: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    workspace_id: Uuid,
    target_index: i64,
    top_level_ids: &[Uuid],
) -> i64 {
    let count = top_level_ids.len() as i64;
    let clamped = target_index.min((count - 1).max(0)).max(0);
    let pinned_ids = sidebar_top_level_pinned_workspace_ids(tabs, groups);
    let pinned_count = top_level_ids
        .iter()
        .filter(|id| pinned_ids.contains(id))
        .count() as i64;
    if pinned_ids.contains(&workspace_id) {
        clamped.min((pinned_count - 1).max(0))
    } else {
        clamped.max(pinned_count)
    }
}

/// Hoist the anchor to the front of its group's member list, then keep pinned
/// member workspaces above unpinned ones while preserving relative order in
/// each tier. No-op when the anchor isn't in the list.
pub fn anchor_first(members: &[WorkspaceRow], anchor_id: Uuid) -> Vec<WorkspaceRow> {
    let Some(anchor_index) = members.iter().position(|m| m.id == anchor_id) else {
        return members.to_vec();
    };
    let anchor = members[anchor_index];
    let non_anchors: Vec<WorkspaceRow> =
        members.iter().copied().filter(|m| m.id != anchor_id).collect();
    let mut out: Vec<WorkspaceRow> = Vec::with_capacity(members.len());
    out.push(anchor);
    out.extend(non_anchors.iter().copied().filter(|m| m.is_pinned));
    out.extend(non_anchors.iter().copied().filter(|m| !m.is_pinned));
    out
}

/// The in-group clamp for a non-anchor member reorder, or `None` when the
/// workspace is ungrouped or its group's anchor.
pub fn clamped_grouped_member_reorder_index(
    tabs: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    workspace: &WorkspaceRow,
    clamped_target_index: i64,
) -> Option<i64> {
    let group_id = workspace.group_id?;
    let group = groups.iter().find(|g| g.id == group_id)?;
    if workspace.id == group.anchor_workspace_id {
        return None;
    }
    let member_indices: Vec<usize> = tabs
        .iter()
        .enumerate()
        .filter(|(_, t)| t.group_id == Some(group_id))
        .map(|(i, _)| i)
        .collect();
    let first_index = *member_indices.first()? as i64;
    let last_index = *member_indices.last()? as i64;
    let pinned_member_count = member_indices
        .iter()
        .filter(|&&i| tabs[i].id != group.anchor_workspace_id && tabs[i].is_pinned)
        .count() as i64;
    let (lower_bound, upper_bound) = if workspace.is_pinned {
        let lower = (first_index + 1).min(last_index);
        let upper = (first_index + pinned_member_count).max(lower);
        (lower, upper)
    } else {
        let lower = (first_index + 1 + pinned_member_count).min(last_index);
        (lower, last_index)
    };
    Some(clamped_target_index.max(lower_bound).min(upper_bound))
}

/// Clamps a requested reorder index for a workspace into its legal range
/// (group section for grouped members, pin tier globally).
pub fn clamped_reorder_index(
    tabs: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    workspace: &WorkspaceRow,
    target_index: i64,
) -> i64 {
    let clamped = target_index.min(tabs.len() as i64 - 1).max(0);
    if let Some(group_clamp) =
        clamped_grouped_member_reorder_index(tabs, groups, workspace, clamped)
    {
        return group_clamp;
    }
    let pinned_count = leading_global_pinned_row_count(tabs, groups);
    if workspace.is_pinned {
        clamped.min((pinned_count - 1).max(0))
    } else {
        clamped.max(pinned_count)
    }
}

/// The number of leading rows in tab order that render as pinned.
pub fn leading_global_pinned_row_count(tabs: &[WorkspaceRow], groups: &[WorkspaceGroup]) -> i64 {
    let mut count = 0;
    for tab in tabs {
        if !is_global_pinned_row(groups, tab) {
            break;
        }
        count += 1;
    }
    count
}

/// Whether the row renders as pinned: group pin for grouped members, workspace
/// pin otherwise.
pub fn is_global_pinned_row(groups: &[WorkspaceGroup], tab: &WorkspaceRow) -> bool {
    if let Some(group) = tab.group_id.and_then(|gid| groups.iter().find(|g| g.id == gid)) {
        group.is_pinned
    } else {
        tab.is_pinned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: Uuid, group: Option<Uuid>, pinned: bool) -> WorkspaceRow {
        WorkspaceRow::new(id, group, pinned)
    }

    fn group(id: Uuid, anchor: Uuid, pinned: bool) -> WorkspaceGroup {
        WorkspaceGroup::new(id, "G".into(), false, pinned, anchor, None, None)
    }

    #[test]
    fn top_level_maps_grouped_members_to_anchor_and_dedups() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let member = Uuid::new_v4();
        let solo = Uuid::new_v4();
        let rows = vec![
            row(anchor, Some(gid), false),
            row(member, Some(gid), false),
            row(solo, None, false),
        ];
        let groups = vec![group(gid, anchor, false)];
        assert_eq!(top_level_workspace_ids(&rows, &groups), vec![anchor, solo]);
    }

    #[test]
    fn unknown_group_membership_falls_back_to_self() {
        let solo = Uuid::new_v4();
        let rows = vec![row(solo, Some(Uuid::new_v4()), false)];
        assert_eq!(top_level_workspace_ids(&rows, &[]), vec![solo]);
        assert_eq!(
            sidebar_top_level_workspace_ids(&rows, &[], None),
            vec![solo]
        );
    }

    #[test]
    fn sidebar_promotes_member_after_its_group_row() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let member = Uuid::new_v4();
        let solo = Uuid::new_v4();
        let rows = vec![
            row(anchor, Some(gid), false),
            row(member, Some(gid), false),
            row(solo, None, false),
        ];
        let groups = vec![group(gid, anchor, false)];
        assert_eq!(
            sidebar_top_level_workspace_ids(&rows, &groups, Some(member)),
            vec![anchor, member, solo]
        );
    }

    #[test]
    fn preserving_order_projects_then_appends_unmentioned() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let rows = vec![row(a, None, false), row(b, None, false), row(c, None, false)];
        assert_eq!(
            top_level_workspace_ids_preserving_order(&rows, &[], &[c, a]),
            vec![c, a, b]
        );
    }

    #[test]
    fn pinned_top_level_ids_honor_group_pin() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let solo = Uuid::new_v4();
        let rows = vec![row(anchor, Some(gid), false), row(solo, None, true)];
        let groups = vec![group(gid, anchor, true)];
        let pinned = sidebar_top_level_pinned_workspace_ids(&rows, &groups);
        assert!(pinned.contains(&anchor));
        assert!(pinned.contains(&solo));
        assert_eq!(pinned.len(), 2);
    }

    #[test]
    fn clamped_top_level_keeps_unpinned_below_pinned_prefix() {
        let pinned = Uuid::new_v4();
        let plain1 = Uuid::new_v4();
        let plain2 = Uuid::new_v4();
        let rows = vec![
            row(pinned, None, true),
            row(plain1, None, false),
            row(plain2, None, false),
        ];
        let top = vec![pinned, plain1, plain2];
        // Unpinned dragged to 0 clamps to pinnedCount (1).
        assert_eq!(
            clamped_top_level_reorder_index(&rows, &[], plain2, 0, &top),
            1
        );
        // Pinned dragged past boundary clamps to pinnedCount-1 (0).
        assert_eq!(
            clamped_top_level_reorder_index(&rows, &[], pinned, 2, &top),
            0
        );
    }

    #[test]
    fn anchor_first_hoists_anchor_then_pinned_then_unpinned() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let pinned = Uuid::new_v4();
        let plain = Uuid::new_v4();
        let members = vec![
            row(plain, Some(gid), false),
            row(pinned, Some(gid), true),
            row(anchor, Some(gid), false),
        ];
        let out = anchor_first(&members, anchor);
        assert_eq!(
            out.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![anchor, pinned, plain]
        );
    }

    #[test]
    fn clamped_reorder_index_clamps_unpinned_above_pinned_boundary() {
        // Matches WorkspaceCoordinatorTests.reorderWorkspaceClampsUnpinnedAbovePinnedBoundary.
        let pinned = Uuid::new_v4();
        let plain1 = Uuid::new_v4();
        let plain2 = Uuid::new_v4();
        let rows = vec![
            row(pinned, None, true),
            row(plain1, None, false),
            row(plain2, None, false),
        ];
        let mover = row(plain2, None, false);
        // Dragging plain2 to index 0 clamps to pinnedCount (1).
        assert_eq!(clamped_reorder_index(&rows, &[], &mover, 0), 1);
    }

    #[test]
    fn grouped_member_clamp_confines_to_section() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let m1 = Uuid::new_v4();
        let m2 = Uuid::new_v4();
        let outside = Uuid::new_v4();
        let rows = vec![
            row(anchor, Some(gid), false),
            row(m1, Some(gid), false),
            row(m2, Some(gid), false),
            row(outside, None, false),
        ];
        let groups = vec![group(gid, anchor, false)];
        let mover = row(m1, Some(gid), false);
        // Section spans indices 0..=2; unpinned members clamp to [1, 2].
        assert_eq!(
            clamped_grouped_member_reorder_index(&rows, &groups, &mover, 0),
            Some(1)
        );
        assert_eq!(
            clamped_grouped_member_reorder_index(&rows, &groups, &mover, 9),
            Some(2)
        );
        // Anchor itself has no in-group clamp.
        let anchor_mover = row(anchor, Some(gid), false);
        assert_eq!(
            clamped_grouped_member_reorder_index(&rows, &groups, &anchor_mover, 0),
            None
        );
    }

    #[test]
    fn leading_pinned_count_stops_at_first_unpinned() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let rows = vec![
            row(Uuid::new_v4(), None, true),
            row(anchor, Some(gid), false),
            row(Uuid::new_v4(), None, false),
        ];
        let groups = vec![group(gid, anchor, true)];
        // Two leading pinned rows (a pinned solo + a pinned group), then stop.
        assert_eq!(leading_global_pinned_row_count(&rows, &groups), 2);
    }

    #[test]
    fn is_workspace_group_anchor_detects_anchor() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let groups = vec![group(gid, anchor, false)];
        assert!(is_workspace_group_anchor(&groups, anchor));
        assert!(!is_workspace_group_anchor(&groups, Uuid::new_v4()));
    }
}
