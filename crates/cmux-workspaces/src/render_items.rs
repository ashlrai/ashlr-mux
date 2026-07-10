//! Port of app `Sources/SidebarWorkspaceRenderItem.swift` — the pure projection
//! from ordered rows + `groupsById` into the drawable
//! `[groupHeader | workspace]` list.
//!
//! Honors collapse, anchor-as-header suppression (an anchor workspace is
//! represented exclusively by its group header, never as its own row), and
//! header de-duplication when a group's members appear in more than one run.
//!
//! DIVERGENCE: Swift's `.workspace` case carries the app `Workspace` god
//! object; the port carries a [`WorkspaceRow`] value snapshot instead. The
//! algorithm reads only `id` / `groupId`, so this is behavior-preserving.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::group::WorkspaceGroup;
use crate::row::WorkspaceRow;

/// Stable identity for a [`SidebarWorkspaceRenderItem`]. The variant keeps group
/// headers and workspace rows from ever colliding on the same UUID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarWorkspaceRenderItemId {
    /// A group header row, keyed by group id.
    Group(Uuid),
    /// A workspace row, keyed by workspace id.
    Workspace(Uuid),
}

/// One drawable item in the workspace sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidebarWorkspaceRenderItem {
    /// A group header, standing in for the group's anchor workspace, carrying
    /// the member workspace ids in tab order.
    GroupHeader {
        /// The group being rendered as a header.
        group: WorkspaceGroup,
        /// Member workspace ids (including the anchor) in tab order.
        member_workspace_ids: Vec<Uuid>,
    },
    /// A plain (ungrouped, or non-anchor grouped) workspace row.
    Workspace(WorkspaceRow),
}

impl SidebarWorkspaceRenderItem {
    /// The item's stable render identity.
    pub fn id(&self) -> SidebarWorkspaceRenderItemId {
        match self {
            SidebarWorkspaceRenderItem::GroupHeader { group, .. } => {
                SidebarWorkspaceRenderItemId::Group(group.id)
            }
            SidebarWorkspaceRenderItem::Workspace(workspace) => {
                SidebarWorkspaceRenderItemId::Workspace(workspace.id)
            }
        }
    }

    /// The workspace id this row stands for (a group header reports its
    /// anchor's id, a workspace row its own).
    pub fn row_workspace_id(&self) -> Uuid {
        match self {
            SidebarWorkspaceRenderItem::GroupHeader { group, .. } => group.anchor_workspace_id,
            SidebarWorkspaceRenderItem::Workspace(workspace) => workspace.id,
        }
    }
}

/// Projects ordered `tabs` and `groups_by_id` into the drawable sidebar list.
pub fn render_items(
    tabs: &[WorkspaceRow],
    groups_by_id: &HashMap<Uuid, WorkspaceGroup>,
) -> Vec<SidebarWorkspaceRenderItem> {
    if tabs.is_empty() {
        return Vec::new();
    }

    let mut member_workspace_ids_by_group_id: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for tab in tabs {
        if let Some(gid) = tab.group_id {
            member_workspace_ids_by_group_id
                .entry(gid)
                .or_default()
                .push(tab.id);
        }
    }

    let mut items: Vec<SidebarWorkspaceRenderItem> =
        Vec::with_capacity(tabs.len() + groups_by_id.len());
    let mut last_emitted_group_id: Option<Uuid> = None;
    let mut emitted_headers: HashSet<Uuid> = HashSet::new();
    let mut collapsed_by_group_id: HashMap<Uuid, bool> = HashMap::new();
    let mut skip_children_until_next_group = false;

    for tab in tabs {
        let group_id = tab.group_id;
        if group_id != last_emitted_group_id {
            last_emitted_group_id = group_id;
            skip_children_until_next_group = false;
            if let Some(gid) = group_id {
                if let Some(group) = groups_by_id.get(&gid) {
                    if !emitted_headers.contains(&gid) {
                        let member_workspace_ids = member_workspace_ids_by_group_id
                            .get(&gid)
                            .cloned()
                            .unwrap_or_default();
                        items.push(SidebarWorkspaceRenderItem::GroupHeader {
                            group: group.clone(),
                            member_workspace_ids,
                        });
                        emitted_headers.insert(gid);
                        collapsed_by_group_id.insert(gid, group.is_collapsed);
                    }
                    // If legacy reorder paths ever leave a group's members in
                    // two runs, keep honoring the same collapse decision.
                    skip_children_until_next_group =
                        collapsed_by_group_id.get(&gid).copied().unwrap_or(false);
                }
            }
        }
        // Anchor workspaces are represented exclusively by the group header.
        if let Some(gid) = group_id {
            if let Some(group) = groups_by_id.get(&gid) {
                if group.anchor_workspace_id == tab.id {
                    continue;
                }
            }
        }
        if group_id.is_none() || !skip_children_until_next_group {
            items.push(SidebarWorkspaceRenderItem::Workspace(*tab));
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: Uuid, group: Option<Uuid>, pinned: bool) -> WorkspaceRow {
        WorkspaceRow::new(id, group, pinned)
    }

    fn group(id: Uuid, anchor: Uuid, collapsed: bool) -> WorkspaceGroup {
        WorkspaceGroup::new(id, "G".into(), collapsed, false, anchor, None, None)
    }

    fn groups_map(groups: Vec<WorkspaceGroup>) -> HashMap<Uuid, WorkspaceGroup> {
        groups.into_iter().map(|g| (g.id, g)).collect()
    }

    #[test]
    fn empty_tabs_render_nothing() {
        assert!(render_items(&[], &HashMap::new()).is_empty());
    }

    #[test]
    fn anchor_is_suppressed_and_header_carries_members() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let member = Uuid::new_v4();
        let solo = Uuid::new_v4();
        let tabs = vec![
            row(anchor, Some(gid), false),
            row(member, Some(gid), false),
            row(solo, None, false),
        ];
        let groups = groups_map(vec![group(gid, anchor, false)]);
        let items = render_items(&tabs, &groups);
        assert_eq!(items.len(), 3);
        match &items[0] {
            SidebarWorkspaceRenderItem::GroupHeader {
                group,
                member_workspace_ids,
            } => {
                assert_eq!(group.id, gid);
                // Anchor + member, in tab order.
                assert_eq!(member_workspace_ids, &vec![anchor, member]);
            }
            other => panic!("expected header, got {other:?}"),
        }
        assert_eq!(items[0].id(), SidebarWorkspaceRenderItemId::Group(gid));
        assert_eq!(items[0].row_workspace_id(), anchor);
        assert_eq!(items[1], SidebarWorkspaceRenderItem::Workspace(tabs[1]));
        assert_eq!(items[2], SidebarWorkspaceRenderItem::Workspace(tabs[2]));
    }

    #[test]
    fn collapsed_group_hides_non_anchor_members() {
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let member = Uuid::new_v4();
        let tabs = vec![row(anchor, Some(gid), false), row(member, Some(gid), false)];
        let groups = groups_map(vec![group(gid, anchor, true)]); // collapsed
        let items = render_items(&tabs, &groups);
        // Only the header survives; the collapsed member row is suppressed.
        assert_eq!(items.len(), 1);
        assert!(matches!(
            items[0],
            SidebarWorkspaceRenderItem::GroupHeader { .. }
        ));
    }

    #[test]
    fn split_group_runs_dedup_header_and_reuse_collapse_decision() {
        // A member appearing in a second run must not re-emit the header, and
        // must honor the same collapse decision recorded at the first run.
        let gid = Uuid::new_v4();
        let anchor = Uuid::new_v4();
        let member1 = Uuid::new_v4();
        let member2 = Uuid::new_v4();
        let outside = Uuid::new_v4();
        let tabs = vec![
            row(anchor, Some(gid), false),
            row(member1, Some(gid), false),
            row(outside, None, false),
            row(member2, Some(gid), false),
        ];
        let groups = groups_map(vec![group(gid, anchor, true)]); // collapsed
        let items = render_items(&tabs, &groups);
        // Header (once) + the ungrouped outside row; both collapsed members hidden.
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id(), SidebarWorkspaceRenderItemId::Group(gid));
        assert_eq!(items[1], SidebarWorkspaceRenderItem::Workspace(tabs[2]));
    }

    #[test]
    fn missing_group_metadata_renders_member_as_plain_row() {
        // group_id points at a group not in the map → rendered as a plain row.
        let gid = Uuid::new_v4();
        let orphan = Uuid::new_v4();
        let tabs = vec![row(orphan, Some(gid), false)];
        let items = render_items(&tabs, &HashMap::new());
        assert_eq!(items, vec![SidebarWorkspaceRenderItem::Workspace(tabs[0])]);
    }
}
