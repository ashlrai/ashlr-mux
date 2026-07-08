//! Pure projection from a persisted [`SessionTabManagerSnapshot`] into the
//! drawable sidebar `[groupHeader | workspace]` list, by mapping the snapshot's
//! optional-string fields onto the already-ported `cmux-workspaces` value types
//! and delegating to [`cmux_workspaces::render_items`].
//!
//! Parity: the canonical caller (`ContentView.swift:10395,10425-10428`) passes
//! `tabManager.tabs` to `SidebarWorkspaceRenderItem.renderItems` **unsorted** —
//! pinned-ahead order and group contiguity are already baked into the model tab
//! array by the invariant helpers. `SessionTabManagerSnapshot.workspaces` is
//! persisted in that same authoritative order, so this projection consumes it
//! **as-is** and MUST NOT re-sort or re-group. All ordering / collapse / anchor
//! suppression / header de-dup logic lives in the ported `render_items` and is
//! golden-pinned there; this module only owns the snapshot → value-type mapping.
//!
//! DIVERGENCES from the Swift model (forced by the snapshot's optional-string
//! encoding of fields the crate types require non-optional):
//! - A workspace snapshot whose `workspace_id` is `None` or unparseable is
//!   **skipped**. The canonical restore (`TabManager.swift:5960-5975`) instead
//!   keeps every workspace, minting a fresh UUID — but that id must be assigned
//!   once by a stateful restore step, not re-minted on every render of this
//!   stateless projection, so id synthesis is deferred to the restore layer.
//!   cmux always persists `workspace_id`, so this only affects corrupted /
//!   pre-id snapshots.
//! - A group's anchor is resolved with the oracle's 3-tier fallback
//!   (`TabManager.swift:6009-6037`): prefer the restore-stable
//!   `anchor_member_index` into the group's members (in persisted tab order),
//!   then the stored `anchor_workspace_id` when it is still a member, then the
//!   first member. A group with at least one member row therefore ALWAYS renders
//!   a header — it is never dropped for a missing/stale anchor. A group with no
//!   member rows is dropped (it would emit nothing anyway), and a duplicate
//!   group id keeps the first occurrence.
//! - `selected_workspace_index` is ignored here: the render item carries no
//!   selection (Swift applies selection downstream at row-draw time). Resolving
//!   the selected row id is A4's concern.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use cmux_core::session::SessionTabManagerSnapshot;
use cmux_workspaces::{
    render_items as crate_render_items, SidebarWorkspaceRenderItem, WorkspaceGroup, WorkspaceRow,
};

/// Projects a persisted tab-manager snapshot into the drawable sidebar list.
///
/// Consumes `snapshot.workspaces` in persisted order (no re-sort); see the
/// module doc-comment for the mapping decisions and divergences.
// Not consumed by non-test code: A4 landed the projection WEB-SIDE
// (`web/src/sidebar/snapshotProjection.ts`, a tested twin of this mapping),
// matching the canonical draw-time call site (`ContentView.swift:10395`). This
// Rust projection stays as the golden-pinned oracle twin and for future
// native consumers (e.g. a CLI `list-workspaces` renderer).
#[allow(dead_code)]
pub fn render_items(snapshot: &SessionTabManagerSnapshot) -> Vec<SidebarWorkspaceRenderItem> {
    let rows: Vec<WorkspaceRow> = snapshot
        .workspaces
        .iter()
        .filter_map(|workspace| {
            let id = workspace
                .workspace_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok())?;
            let group_id = workspace
                .group_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok());
            let is_pinned = workspace.is_pinned.unwrap_or(false);
            Some(WorkspaceRow::new(id, group_id, is_pinned))
        })
        .collect();

    // Members-by-group in persisted row order — the oracle's
    // `workspaceIdsByGroupId` (TabManager.swift:6000-6008), used both to drop
    // member-less groups and to resolve each group's anchor.
    let mut members_by_group_id: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for row in &rows {
        if let Some(gid) = row.group_id {
            members_by_group_id.entry(gid).or_default().push(row.id);
        }
    }

    let mut groups_by_id: HashMap<Uuid, WorkspaceGroup> = HashMap::new();
    let mut seen: HashSet<Uuid> = HashSet::new();
    if let Some(group_snapshots) = snapshot.workspace_groups.as_ref() {
        for group in group_snapshots {
            let Ok(id) = Uuid::parse_str(&group.id) else {
                continue;
            };
            // Drop groups with no restorable members, and de-dup by id keeping
            // the first occurrence (TabManager.swift:6010-6012).
            let Some(members) = members_by_group_id.get(&id) else {
                continue;
            };
            if !seen.insert(id) {
                continue;
            }
            // Resolve the anchor with the oracle's 3-tier fallback
            // (TabManager.swift:6018-6027): prefer the restore-stable member
            // index, then the stored anchor id when it is still a member, then
            // the first member. A group with members is thus never dropped.
            let stored_anchor = group
                .anchor_workspace_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok());
            let anchor_workspace_id = group
                .anchor_member_index
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| members.get(i).copied())
                .or_else(|| stored_anchor.filter(|a| members.contains(a)))
                .unwrap_or(members[0]);
            groups_by_id.insert(
                id,
                WorkspaceGroup::new(
                    id,
                    group.name.clone(),
                    group.is_collapsed,
                    group.is_pinned.unwrap_or(false),
                    anchor_workspace_id,
                    group.custom_color.clone(),
                    group.icon_symbol.clone(),
                ),
            );
        }
    }

    crate_render_items(&rows, &groups_by_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_core::session::{SessionWorkspaceGroupSnapshot, SessionWorkspaceSnapshot};
    use cmux_workspaces::SidebarWorkspaceRenderItemId;

    fn workspace(id: &str, group_id: Option<&str>, is_pinned: Option<bool>) -> SessionWorkspaceSnapshot {
        SessionWorkspaceSnapshot {
            workspace_id: Some(id.to_string()),
            group_id: group_id.map(|s| s.to_string()),
            is_pinned,
            ..Default::default()
        }
    }

    fn group(id: &str, anchor: Option<&str>, is_collapsed: bool) -> SessionWorkspaceGroupSnapshot {
        SessionWorkspaceGroupSnapshot {
            id: id.to_string(),
            name: "G".to_string(),
            is_collapsed,
            anchor_workspace_id: anchor.map(|s| s.to_string()),
            anchor_member_index: None,
            is_pinned: None,
            custom_color: None,
            icon_symbol: None,
        }
    }

    // Fixed UUID strings so the tests can assert on exact ids.
    const GID: &str = "11111111-1111-1111-1111-111111111111";
    const ANCHOR: &str = "22222222-2222-2222-2222-222222222222";
    const MEMBER: &str = "33333333-3333-3333-3333-333333333333";
    const SOLO: &str = "44444444-4444-4444-4444-444444444444";

    fn uuid(s: &str) -> Uuid {
        Uuid::parse_str(s).unwrap()
    }

    #[test]
    fn empty_snapshot_renders_nothing() {
        let snapshot = SessionTabManagerSnapshot::default();
        assert!(render_items(&snapshot).is_empty());
    }

    #[test]
    fn anchor_is_suppressed_and_header_carries_members() {
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                workspace(ANCHOR, Some(GID), None),
                workspace(MEMBER, Some(GID), None),
                workspace(SOLO, None, None),
            ],
            workspace_groups: Some(vec![group(GID, Some(ANCHOR), false)]),
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 3);
        match &items[0] {
            SidebarWorkspaceRenderItem::GroupHeader {
                group,
                member_workspace_ids,
            } => {
                assert_eq!(group.id, uuid(GID));
                assert_eq!(member_workspace_ids, &vec![uuid(ANCHOR), uuid(MEMBER)]);
            }
            other => panic!("expected header, got {other:?}"),
        }
        assert_eq!(items[0].id(), SidebarWorkspaceRenderItemId::Group(uuid(GID)));
        assert_eq!(items[0].row_workspace_id(), uuid(ANCHOR));
        assert_eq!(items[1].row_workspace_id(), uuid(MEMBER));
        assert_eq!(items[2].row_workspace_id(), uuid(SOLO));
    }

    #[test]
    fn collapsed_group_hides_non_anchor_members() {
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                workspace(ANCHOR, Some(GID), None),
                workspace(MEMBER, Some(GID), None),
            ],
            workspace_groups: Some(vec![group(GID, Some(ANCHOR), true)]),
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 1);
        assert!(matches!(
            items[0],
            SidebarWorkspaceRenderItem::GroupHeader { .. }
        ));
    }

    #[test]
    fn pinned_ahead_order_is_honored_as_given() {
        // Order as persisted: pinned solo, then unpinned member. The projection
        // must preserve it verbatim (no re-sort).
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                workspace(SOLO, None, Some(true)),
                workspace(MEMBER, None, Some(false)),
            ],
            workspace_groups: None,
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].row_workspace_id(), uuid(SOLO));
        assert_eq!(items[1].row_workspace_id(), uuid(MEMBER));
    }

    #[test]
    fn unpinned_before_pinned_is_not_resorted() {
        // Deliberately "wrong" order (unpinned first) to guard against an
        // accidental pinned-ahead sort creeping into the projection.
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                workspace(MEMBER, None, Some(false)),
                workspace(SOLO, None, Some(true)),
            ],
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].row_workspace_id(), uuid(MEMBER));
        assert_eq!(items[1].row_workspace_id(), uuid(SOLO));
    }

    #[test]
    fn is_pinned_none_maps_to_false() {
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![workspace(SOLO, None, None)],
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 1);
        match &items[0] {
            SidebarWorkspaceRenderItem::Workspace(row) => assert!(!row.is_pinned),
            other => panic!("expected workspace row, got {other:?}"),
        }
    }

    #[test]
    fn missing_anchor_falls_back_to_first_member() {
        // No stored anchor and no index → the oracle re-anchors to members[0]
        // (TabManager.swift:6026), so the group STILL renders a header — it is
        // not dropped. The first member becomes the (suppressed) anchor.
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                workspace(ANCHOR, Some(GID), None),
                workspace(MEMBER, Some(GID), None),
            ],
            workspace_groups: Some(vec![group(GID, None, false)]),
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 2);
        assert!(matches!(
            items[0],
            SidebarWorkspaceRenderItem::GroupHeader { .. }
        ));
        assert_eq!(items[0].row_workspace_id(), uuid(ANCHOR)); // members[0]
        assert_eq!(items[1].row_workspace_id(), uuid(MEMBER));
    }

    #[test]
    fn stale_anchor_not_a_member_re_anchors_to_first_member() {
        // Stored anchor points at a workspace that is not among the group's
        // members (e.g. the anchor was closed / non-restorable). The oracle
        // ignores the stale id and re-anchors to members[0]; it must NOT emit a
        // header for a phantom workspace (TabManager.swift:6023-6026).
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                workspace(ANCHOR, Some(GID), None),
                workspace(MEMBER, Some(GID), None),
            ],
            workspace_groups: Some(vec![group(GID, Some(SOLO), false)]),
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id(), SidebarWorkspaceRenderItemId::Group(uuid(GID)));
        assert_eq!(items[0].row_workspace_id(), uuid(ANCHOR)); // members[0], not SOLO
        assert_eq!(items[1].row_workspace_id(), uuid(MEMBER));
    }

    #[test]
    fn anchor_member_index_is_preferred_over_stored_anchor() {
        // Index 1 selects MEMBER as the anchor even though the stored anchor id
        // names ANCHOR (index 0). Mirrors the restore-stable preference at
        // TabManager.swift:6019-6022.
        let mut g = group(GID, Some(ANCHOR), false);
        g.anchor_member_index = Some(1);
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                workspace(ANCHOR, Some(GID), None),
                workspace(MEMBER, Some(GID), None),
            ],
            workspace_groups: Some(vec![g]),
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 2);
        // Header anchored to MEMBER (index 1) → MEMBER is suppressed as a row,
        // ANCHOR (index 0) survives as a plain row.
        assert_eq!(items[0].row_workspace_id(), uuid(MEMBER));
        assert_eq!(items[1].row_workspace_id(), uuid(ANCHOR));
    }

    #[test]
    fn out_of_range_anchor_member_index_falls_through_to_stored_anchor() {
        // An index past the members array is ignored (indices.contains == false),
        // falling through to the stored anchor id when it is a member.
        let mut g = group(GID, Some(MEMBER), false);
        g.anchor_member_index = Some(9);
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                workspace(ANCHOR, Some(GID), None),
                workspace(MEMBER, Some(GID), None),
            ],
            workspace_groups: Some(vec![g]),
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].row_workspace_id(), uuid(MEMBER)); // stored anchor
        assert_eq!(items[1].row_workspace_id(), uuid(ANCHOR));
    }

    #[test]
    fn absent_and_invalid_workspace_id_are_skipped() {
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                SessionWorkspaceSnapshot {
                    workspace_id: None,
                    ..Default::default()
                },
                SessionWorkspaceSnapshot {
                    workspace_id: Some("not-a-uuid".to_string()),
                    ..Default::default()
                },
                workspace(SOLO, None, None),
            ],
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].row_workspace_id(), uuid(SOLO));
    }

    #[test]
    fn workspace_groups_none_renders_all_as_plain_rows() {
        let snapshot = SessionTabManagerSnapshot {
            workspaces: vec![
                workspace(ANCHOR, Some(GID), None),
                workspace(SOLO, None, None),
            ],
            workspace_groups: None,
            ..Default::default()
        };
        let items = render_items(&snapshot);
        assert_eq!(items.len(), 2);
        assert!(items
            .iter()
            .all(|item| matches!(item, SidebarWorkspaceRenderItem::Workspace(_))));
    }
}
