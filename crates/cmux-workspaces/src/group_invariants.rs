//! Port of `Model/WorkspacesModel+GroupInvariants.swift` — group-section
//! invariant maintenance.
//!
//! DIVERGENCE: the Swift methods mutate `Tab.groupId` in place and reassign
//! `self.tabs` / `self.workspaceGroups`. Each Rust port is a pure function
//! returning the new ordered `Vec<WorkspaceRow>` (and, where the Swift also
//! reorders groups, the new `Vec<WorkspaceGroup>`).

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::group::WorkspaceGroup;
use crate::ordering::{
    anchor_first, sidebar_top_level_pinned_workspace_ids, sidebar_top_level_workspace_ids,
};
use crate::row::WorkspaceRow;

/// Sets a workspace's group membership, returning the new rows. No-op (returns
/// a clone) when the workspace is missing or already has the requested group.
pub fn assign_group(
    rows: &[WorkspaceRow],
    workspace_id: Uuid,
    group_id: Option<Uuid>,
) -> Vec<WorkspaceRow> {
    let mut out = rows.to_vec();
    if let Some(row) = out.iter_mut().find(|r| r.id == workspace_id) {
        if row.group_id != group_id {
            row.group_id = group_id;
        }
    }
    out
}

/// Drops `group_id`s that don't reference a known group (mirrors the guard the
/// Swift invariant helpers run before reordering).
fn clearing_unknown_group_ids(
    rows: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
) -> Vec<WorkspaceRow> {
    let known: HashSet<Uuid> = groups.iter().map(|g| g.id).collect();
    rows.iter()
        .map(|r| {
            let mut r = *r;
            if r.group_id.map(|gid| !known.contains(&gid)).unwrap_or(false) {
                r.group_id = None;
            }
            r
        })
        .collect()
}

/// Rebuild `rows` by walking a desired top-level workspace order and emitting
/// each workspace group as one contiguous run at its first encountered member.
pub fn normalize_workspace_group_runs_preserving_order(
    rows: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    desired_ids: &[Uuid],
) -> Vec<WorkspaceRow> {
    let by_id: HashMap<Uuid, &WorkspaceGroup> = groups.iter().map(|g| (g.id, g)).collect();
    let cleared = clearing_unknown_group_ids(rows, groups);

    let mut grouped_by_group_id: HashMap<Uuid, Vec<WorkspaceRow>> = HashMap::new();
    for tab in &cleared {
        if let Some(gid) = tab.group_id {
            grouped_by_group_id.entry(gid).or_default().push(*tab);
        }
    }
    let tabs_by_id: HashMap<Uuid, WorkspaceRow> = cleared.iter().map(|t| (t.id, *t)).collect();

    let mut emitted_workspace_ids: HashSet<Uuid> = HashSet::new();
    let mut emitted_group_ids: HashSet<Uuid> = HashSet::new();
    let mut reordered: Vec<WorkspaceRow> = Vec::with_capacity(cleared.len());

    let append = |id: Uuid,
                      reordered: &mut Vec<WorkspaceRow>,
                      emitted_workspace_ids: &mut HashSet<Uuid>,
                      emitted_group_ids: &mut HashSet<Uuid>| {
        let Some(tab) = tabs_by_id.get(&id) else {
            return;
        };
        match tab.group_id.and_then(|gid| by_id.get(&gid).map(|g| (gid, g))) {
            Some((gid, group)) => {
                if emitted_group_ids.insert(gid) {
                    let members = anchor_first(
                        grouped_by_group_id.get(&gid).map(|v| v.as_slice()).unwrap_or(&[]),
                        group.anchor_workspace_id,
                    );
                    for member in members {
                        if emitted_workspace_ids.insert(member.id) {
                            reordered.push(member);
                        }
                    }
                }
            }
            None => {
                if emitted_workspace_ids.insert(tab.id) {
                    reordered.push(*tab);
                }
            }
        }
    };

    for id in desired_ids {
        append(
            *id,
            &mut reordered,
            &mut emitted_workspace_ids,
            &mut emitted_group_ids,
        );
    }
    for tab in &cleared {
        if !emitted_workspace_ids.contains(&tab.id) {
            append(
                tab.id,
                &mut reordered,
                &mut emitted_workspace_ids,
                &mut emitted_group_ids,
            );
        }
    }
    reordered
}

/// Reorder `rows` so each group stays contiguous and anchor-first while
/// preserving top-level row order inside the pinned and unpinned tiers, then
/// reorder `groups` to match anchor order. Returns `(rows, groups)`.
pub fn normalize_workspace_group_contiguity(
    rows: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    preferred_top_level_ids: Option<&[Uuid]>,
) -> (Vec<WorkspaceRow>, Vec<WorkspaceGroup>) {
    if rows.is_empty() {
        return (rows.to_vec(), groups.to_vec());
    }
    let cleared = clearing_unknown_group_ids(rows, groups);
    let top_level_ids: Vec<Uuid> = match preferred_top_level_ids {
        Some(ids) => ids.to_vec(),
        None => sidebar_top_level_workspace_ids(&cleared, groups, None),
    };
    let pinned_top_level = sidebar_top_level_pinned_workspace_ids(&cleared, groups);
    let desired_ids: Vec<Uuid> = top_level_ids
        .iter()
        .copied()
        .filter(|id| pinned_top_level.contains(id))
        .chain(
            top_level_ids
                .iter()
                .copied()
                .filter(|id| !pinned_top_level.contains(id)),
        )
        .collect();
    let new_rows = normalize_workspace_group_runs_preserving_order(&cleared, groups, &desired_ids);
    let new_groups = sync_workspace_groups_order_to_anchor_order(&new_rows, groups);
    (new_rows, new_groups)
}

/// Ensure the group containing the newly-selected workspace is expanded so the
/// selected row is visible. Returns the new groups. No-op (clone) when the
/// workspace is ungrouped, its group is already expanded, or it IS the anchor.
pub fn expand_workspace_group_for_selection_if_needed(
    rows: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    selected_tab_id: Option<Uuid>,
) -> Vec<WorkspaceGroup> {
    let mut out = groups.to_vec();
    let Some(selected) = selected_tab_id else {
        return out;
    };
    let Some(group_id) = rows.iter().find(|r| r.id == selected).and_then(|r| r.group_id) else {
        return out;
    };
    let Some(index) = out.iter().position(|g| g.id == group_id) else {
        return out;
    };
    if !out[index].is_collapsed {
        return out;
    }
    // Skip auto-expand when the focused workspace IS the group's anchor.
    if out[index].anchor_workspace_id == selected {
        return out;
    }
    out[index].is_collapsed = false;
    out
}

/// Reorder `groups` so each group's relative position matches the order its
/// anchor occupies in `rows`. Anchors missing from `rows` sort last.
///
/// DIVERGENCE: Swift's `Array.sort` is not guaranteed stable; Rust's `sort_by`
/// is. This only matters for multiple groups whose anchors are all missing from
/// `rows` (an impossible state under the anchor lifecycle), so the observable
/// behavior matches while staying deterministic here.
pub fn sync_workspace_groups_order_to_anchor_order(
    rows: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
) -> Vec<WorkspaceGroup> {
    let anchor_index: HashMap<Uuid, usize> =
        rows.iter().enumerate().map(|(i, r)| (r.id, i)).collect();
    let mut out = groups.to_vec();
    out.sort_by_key(|g| {
        anchor_index
            .get(&g.anchor_workspace_id)
            .copied()
            .unwrap_or(usize::MAX)
    });
    out
}

/// Hoist promoted (non-anchor) members to the front of their group's member
/// run, right after the anchor, preserving each group's position. Returns the
/// new rows (a clone when nothing qualifies).
pub fn move_workspace_group_members_after_anchors(
    rows: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    workspace_ids: &[Uuid],
) -> Vec<WorkspaceRow> {
    let by_id: HashMap<Uuid, &WorkspaceGroup> = groups.iter().map(|g| (g.id, g)).collect();
    let tabs_by_id: HashMap<Uuid, WorkspaceRow> = rows.iter().map(|t| (t.id, *t)).collect();

    let mut promoted_ids_by_group_id: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for workspace_id in workspace_ids {
        let Some(tab) = tabs_by_id.get(workspace_id) else {
            continue;
        };
        let Some(gid) = tab.group_id else { continue };
        let Some(group) = by_id.get(&gid) else {
            continue;
        };
        if tab.id == group.anchor_workspace_id {
            continue;
        }
        promoted_ids_by_group_id.entry(gid).or_default().push(*workspace_id);
    }
    if promoted_ids_by_group_id.is_empty() {
        return rows.to_vec();
    }

    let mut replacement_members_by_group_id: HashMap<Uuid, Vec<WorkspaceRow>> = HashMap::new();
    for (gid, promoted_ids) in &promoted_ids_by_group_id {
        let Some(group) = by_id.get(gid) else { continue };
        let group_members: Vec<WorkspaceRow> =
            rows.iter().copied().filter(|r| r.group_id == Some(*gid)).collect();
        let ordered_members = anchor_first(&group_members, group.anchor_workspace_id);
        let Some(anchor) = ordered_members
            .iter()
            .copied()
            .find(|m| m.id == group.anchor_workspace_id)
        else {
            continue;
        };
        let mut emitted_promoted_ids: HashSet<Uuid> = HashSet::new();
        let promoted_members: Vec<WorkspaceRow> = promoted_ids
            .iter()
            .filter(|id| emitted_promoted_ids.insert(**id))
            .filter_map(|id| tabs_by_id.get(id).copied())
            .collect();
        let promoted_id_set: HashSet<Uuid> = promoted_members.iter().map(|m| m.id).collect();
        let remaining_members: Vec<WorkspaceRow> = ordered_members
            .iter()
            .copied()
            .filter(|m| m.id != group.anchor_workspace_id && !promoted_id_set.contains(&m.id))
            .collect();
        let mut replacement = Vec::with_capacity(ordered_members.len());
        replacement.push(anchor);
        replacement.extend(promoted_members);
        replacement.extend(remaining_members);
        replacement_members_by_group_id.insert(*gid, replacement);
    }
    if replacement_members_by_group_id.is_empty() {
        return rows.to_vec();
    }

    let mut emitted_group_ids: HashSet<Uuid> = HashSet::new();
    let mut reordered: Vec<WorkspaceRow> = Vec::with_capacity(rows.len());
    for tab in rows {
        match tab.group_id.and_then(|gid| {
            replacement_members_by_group_id
                .get(&gid)
                .map(|members| (gid, members))
        }) {
            Some((gid, members)) => {
                if emitted_group_ids.insert(gid) {
                    reordered.extend(members.iter().copied());
                }
            }
            None => reordered.push(*tab),
        }
    }
    reordered
}

/// If `closed_workspace_id` anchored any group, dissolve those groups:
/// remaining members lose their `group_id` and stay as ungrouped workspaces,
/// then rows/groups are renormalized. Returns `(rows, groups)`. Caller is
/// responsible for having already removed the closed workspace from `rows`.
pub fn dissolve_groups_anchored_by(
    rows: &[WorkspaceRow],
    groups: &[WorkspaceGroup],
    closed_workspace_id: Uuid,
) -> (Vec<WorkspaceRow>, Vec<WorkspaceGroup>) {
    let dissolved_group_ids: HashSet<Uuid> = groups
        .iter()
        .filter(|g| g.anchor_workspace_id == closed_workspace_id)
        .map(|g| g.id)
        .collect();
    if dissolved_group_ids.is_empty() {
        return (rows.to_vec(), groups.to_vec());
    }
    let ungrouped: Vec<WorkspaceRow> = rows
        .iter()
        .map(|r| {
            let mut r = *r;
            if r.group_id.map(|gid| dissolved_group_ids.contains(&gid)).unwrap_or(false) {
                r.group_id = None;
            }
            r
        })
        .collect();
    let surviving_groups: Vec<WorkspaceGroup> = groups
        .iter()
        .filter(|g| !dissolved_group_ids.contains(&g.id))
        .cloned()
        .collect();
    normalize_workspace_group_contiguity(&ungrouped, &surviving_groups, None)
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
    fn assign_group_sets_membership_and_is_noop_when_unchanged() {
        let a = Uuid::new_v4();
        let gid = Uuid::new_v4();
        let rows = vec![row(a, None, false)];
        let out = assign_group(&rows, a, Some(gid));
        assert_eq!(out[0].group_id, Some(gid));
        // Already assigned → identical.
        assert_eq!(assign_group(&out, a, Some(gid)), out);
        // Missing workspace → clone.
        assert_eq!(assign_group(&rows, Uuid::new_v4(), Some(gid)), rows);
    }

    #[test]
    fn runs_preserving_order_emits_group_as_contiguous_anchor_first_run() {
        // Mirrors createWorkspaceGroupAdoptsChildrenAndKeepsSectionContiguous:
        // group run materializes at its first member's slot, anchor-first.
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let child1 = Uuid::new_v4();
        let child2 = Uuid::new_v4();
        let other = Uuid::new_v4();
        // Layout after group creation inserts anchor at the top: anchor, child1, other, child2.
        let rows = vec![
            row(anchor, Some(gid), false),
            row(child1, Some(gid), false),
            row(other, None, false),
            row(child2, Some(gid), false),
        ];
        let groups = vec![group(gid, anchor, false)];
        let out = normalize_workspace_group_runs_preserving_order(
            &rows,
            &groups,
            &[anchor, child1, other, child2],
        );
        assert_eq!(
            out.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![anchor, child1, child2, other]
        );
    }

    #[test]
    fn contiguity_moves_newly_ungrouped_members_into_unpinned_tier() {
        // A row that lost its group (unknown group id) is unpinned, so it must
        // slide BELOW a surviving PINNED group even though it started first in
        // tab order. (With an unpinned group the algorithm preserves top-level
        // order and would leave `stale` first — the tier split is what moves it.)
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let member = Uuid::new_v4();
        let stale = Uuid::new_v4();
        let stale_gid = Uuid::new_v4(); // not in groups → cleared to ungrouped
        let rows = vec![
            row(stale, Some(stale_gid), false),
            row(anchor, Some(gid), false),
            row(member, Some(gid), false),
        ];
        let groups = vec![group(gid, anchor, true)]; // pinned group
        let (new_rows, _) = normalize_workspace_group_contiguity(&rows, &groups, None);
        // Pinned group (anchor-first) leads the pinned tier; the newly-ungrouped
        // unpinned row trails in the unpinned tier.
        assert_eq!(
            new_rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![anchor, member, stale]
        );
        assert_eq!(new_rows.iter().find(|r| r.id == stale).unwrap().group_id, None);
    }

    #[test]
    fn contiguity_orders_pinned_top_level_rows_before_unpinned() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let unpinned = Uuid::new_v4();
        let pinned_solo = Uuid::new_v4();
        let rows = vec![
            row(unpinned, None, false),
            row(anchor, Some(gid), false),
            row(pinned_solo, None, true),
        ];
        let groups = vec![group(gid, anchor, true)]; // pinned group
        let (new_rows, _) = normalize_workspace_group_contiguity(&rows, &groups, None);
        let ids: Vec<Uuid> = new_rows.iter().map(|r| r.id).collect();
        // Both pinned top-level rows precede the unpinned one.
        let unpinned_pos = ids.iter().position(|id| *id == unpinned).unwrap();
        let anchor_pos = ids.iter().position(|id| *id == anchor).unwrap();
        let pinned_pos = ids.iter().position(|id| *id == pinned_solo).unwrap();
        assert!(anchor_pos < unpinned_pos);
        assert!(pinned_pos < unpinned_pos);
    }

    #[test]
    fn expand_group_uncollapses_unless_anchor_selected() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let member = Uuid::new_v4();
        let rows = vec![row(anchor, Some(gid), false), row(member, Some(gid), false)];
        let groups = vec![WorkspaceGroup::new(
            gid,
            "G".into(),
            true, // collapsed
            false,
            anchor,
            None,
            None,
        )];
        // Selecting a non-anchor member expands.
        let out = expand_workspace_group_for_selection_if_needed(&rows, &groups, Some(member));
        assert!(!out[0].is_collapsed);
        // Selecting the anchor keeps it collapsed.
        let out2 = expand_workspace_group_for_selection_if_needed(&rows, &groups, Some(anchor));
        assert!(out2[0].is_collapsed);
    }

    #[test]
    fn sync_groups_order_follows_anchor_order() {
        let g1 = Uuid::new_v4();
        let a1 = Uuid::new_v4();
        let g2 = Uuid::new_v4();
        let a2 = Uuid::new_v4();
        // Rows place a2 before a1.
        let rows = vec![row(a2, Some(g2), false), row(a1, Some(g1), false)];
        let groups = vec![group(g1, a1, false), group(g2, a2, false)];
        let out = sync_workspace_groups_order_to_anchor_order(&rows, &groups);
        assert_eq!(out.iter().map(|g| g.id).collect::<Vec<_>>(), vec![g2, g1]);
    }

    #[test]
    fn move_members_after_anchors_hoists_promoted_behind_anchor() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let m1 = Uuid::new_v4();
        let m2 = Uuid::new_v4();
        let rows = vec![
            row(anchor, Some(gid), false),
            row(m1, Some(gid), false),
            row(m2, Some(gid), false),
        ];
        let groups = vec![group(gid, anchor, false)];
        let out = move_workspace_group_members_after_anchors(&rows, &groups, &[m2]);
        assert_eq!(
            out.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![anchor, m2, m1]
        );
        // Anchor cannot be promoted → no change.
        assert_eq!(
            move_workspace_group_members_after_anchors(&rows, &groups, &[anchor]),
            rows
        );
    }

    #[test]
    fn dissolve_groups_anchored_by_ungroups_members_and_renormalizes() {
        // Edge case: closing the anchor dissolves the group; members become
        // ungrouped and slide to the ungrouped tier.
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let member = Uuid::new_v4();
        let outside = Uuid::new_v4();
        // Anchor already removed from rows by the caller.
        let rows = vec![row(member, Some(gid), false), row(outside, None, false)];
        let groups = vec![group(gid, anchor, false)];
        let (new_rows, new_groups) = dissolve_groups_anchored_by(&rows, &groups, anchor);
        assert!(new_groups.is_empty());
        assert!(new_rows.iter().all(|r| r.group_id.is_none()));
        assert_eq!(new_rows.len(), 2);
    }

    #[test]
    fn dissolve_groups_anchored_by_noop_when_not_anchor() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let member = Uuid::new_v4();
        let rows = vec![row(anchor, Some(gid), false), row(member, Some(gid), false)];
        let groups = vec![group(gid, anchor, false)];
        // Closing a non-anchor member leaves the group intact.
        let (new_rows, new_groups) = dissolve_groups_anchored_by(&rows, &groups, member);
        assert_eq!(new_rows, rows);
        assert_eq!(new_groups, groups);
    }
}
