// Hand-port of `crates/cmux-workspaces/src/render_items.rs` (itself a port of the
// app `Sources/SidebarWorkspaceRenderItem.swift`) — the pure projection from
// ordered rows + `groupsById` into the drawable `[groupHeader | workspace]` list.
//
// Honors collapse, anchor-as-header suppression (an anchor workspace is
// represented exclusively by its group header, never as its own row), and
// header de-duplication when a group's members appear in more than one run.
//
// DIVERGENCE: the Rust `.Workspace` case carries a `WorkspaceRow` value
// snapshot (Swift carries the app `Workspace` god object). The algorithm reads
// only `id` / `groupId`, so this is behavior-preserving. Rust `HashMap<Uuid,_>`
// becomes a TS `Map<string, WorkspaceGroup>`; `Uuid` becomes a `string`.

/// Named collapsible sidebar group containing one or more workspaces.
///
/// Mirrors the Rust `WorkspaceGroup` value shape. The membership relation lives
/// on `WorkspaceRow.groupId`; this holds the group's identity, display name,
/// collapse/pin state, and the explicit anchor workspace rendered implicitly as
/// the header.
export interface WorkspaceGroup {
  /// The group's stable identity (UUID string).
  id: string;
  /// The group's display name.
  name: string;
  /// Whether the group's member rows are collapsed in the sidebar.
  isCollapsed: boolean;
  /// Whether the group is pinned.
  isPinned: boolean;
  /// Identifier of the member workspace that owns this group's lifecycle.
  /// Always points to a workspace whose `groupId === self.id`.
  anchorWorkspaceId: string;
  /// Group-level color override (hex string), or absent for no tint.
  customColor?: string;
  /// SF-symbol name for the header icon, or absent for the host default.
  iconSymbol?: string;
}

/// The pure per-workspace ("tab") facts the projection reads: identity, group
/// membership, and pin state. Mirrors the Rust `WorkspaceRow`.
export interface WorkspaceRow {
  /// The workspace's stable identity (UUID string).
  id: string;
  /// The owning `WorkspaceGroup.id`, or absent when ungrouped.
  groupId?: string;
  /// Whether the workspace is pinned (pinned rows float above unpinned).
  isPinned: boolean;
  /// Workspace-level color override (hex string), or absent for inherited tint.
  customColor?: string;
}

/// Stable identity for a render item. The `kind` keeps group headers and
/// workspace rows from ever colliding on the same UUID.
export type SidebarWorkspaceRenderItemId =
  | { kind: "group"; id: string }
  | { kind: "workspace"; id: string };

/// One drawable item in the workspace sidebar.
export type SidebarWorkspaceRenderItem =
  | {
      kind: "groupHeader";
      /// The group being rendered as a header.
      group: WorkspaceGroup;
      /// Member workspace ids (including the anchor) in tab order.
      memberWorkspaceIds: string[];
    }
  | {
      kind: "workspace";
      /// A plain (ungrouped, or non-anchor grouped) workspace row.
      workspace: WorkspaceRow;
    };

/// The item's stable render identity.
export function renderItemId(
  item: SidebarWorkspaceRenderItem,
): SidebarWorkspaceRenderItemId {
  if (item.kind === "groupHeader") {
    return { kind: "group", id: item.group.id };
  }
  return { kind: "workspace", id: item.workspace.id };
}

/// The workspace id this row stands for (a group header reports its anchor's
/// id, a workspace row its own).
export function rowWorkspaceId(item: SidebarWorkspaceRenderItem): string {
  if (item.kind === "groupHeader") {
    return item.group.anchorWorkspaceId;
  }
  return item.workspace.id;
}

/// Projects ordered `tabs` and `groupsById` into the drawable sidebar list.
export function renderItems(
  tabs: readonly WorkspaceRow[],
  groupsById: ReadonlyMap<string, WorkspaceGroup>,
): SidebarWorkspaceRenderItem[] {
  if (tabs.length === 0) {
    return [];
  }

  const memberWorkspaceIdsByGroupId = new Map<string, string[]>();
  for (const tab of tabs) {
    if (tab.groupId !== undefined) {
      let members = memberWorkspaceIdsByGroupId.get(tab.groupId);
      if (members === undefined) {
        members = [];
        memberWorkspaceIdsByGroupId.set(tab.groupId, members);
      }
      members.push(tab.id);
    }
  }

  const items: SidebarWorkspaceRenderItem[] = [];
  let lastEmittedGroupId: string | undefined = undefined;
  // Distinguishes "no group emitted yet" from "the ungrouped run"; the initial
  // value is a sentinel that no real (grouped-or-ungrouped) tab can equal.
  let haveLastEmittedGroupId = false;
  const emittedHeaders = new Set<string>();
  const collapsedByGroupId = new Map<string, boolean>();
  let skipChildrenUntilNextGroup = false;

  for (const tab of tabs) {
    const groupId = tab.groupId;
    if (!haveLastEmittedGroupId || groupId !== lastEmittedGroupId) {
      lastEmittedGroupId = groupId;
      haveLastEmittedGroupId = true;
      skipChildrenUntilNextGroup = false;
      if (groupId !== undefined) {
        const group = groupsById.get(groupId);
        if (group !== undefined) {
          if (!emittedHeaders.has(groupId)) {
            const memberWorkspaceIds =
              memberWorkspaceIdsByGroupId.get(groupId) ?? [];
            items.push({
              kind: "groupHeader",
              group,
              memberWorkspaceIds,
            });
            emittedHeaders.add(groupId);
            collapsedByGroupId.set(groupId, group.isCollapsed);
          }
          // If legacy reorder paths ever leave a group's members in two runs,
          // keep honoring the same collapse decision.
          skipChildrenUntilNextGroup =
            collapsedByGroupId.get(groupId) ?? false;
        }
      }
    }
    // Anchor workspaces are represented exclusively by the group header.
    if (groupId !== undefined) {
      const group = groupsById.get(groupId);
      if (group !== undefined && group.anchorWorkspaceId === tab.id) {
        continue;
      }
    }
    if (groupId === undefined || !skipChildrenUntilNextGroup) {
      items.push({ kind: "workspace", workspace: tab });
    }
  }
  return items;
}
