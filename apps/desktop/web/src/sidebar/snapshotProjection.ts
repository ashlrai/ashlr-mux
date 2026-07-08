// Pure mapping from the persisted `SessionTabManagerSnapshot` wire shape onto
// the `renderItems` value types — the web twin of the Rust projection in
// `src-tauri/src/sidebar_render.rs` (which delegates to the golden-pinned
// `cmux_workspaces::render_items`; the algorithm itself is ported in
// `./renderItems`). Canonical parity: Swift projects at view-draw time in
// `ContentView.swift:10395,10425-10428`, so the projection lives web-side and
// runs synchronously over the snapshot every render — no IPC round-trip.
//
// Mapping rules (all mirrored from `sidebar_render.rs`, divergences documented
// there):
// - A workspace whose `workspace_id` is absent or not a UUID is skipped; id
//   synthesis belongs to the stateful session layer (`ensure_workspace_ids` in
//   `session.rs`), never to this stateless projection.
// - Ids are lowercase-normalized, matching Rust `Uuid` semantics where parsing
//   erases case so mixed-case ids still match across rows and groups.
// - A group with no member rows is dropped; a duplicate group id keeps the
//   first occurrence.
// - A group's anchor resolves with the oracle's 3-tier fallback
//   (`TabManager.swift:6018-6027`): the restore-stable `anchor_member_index`
//   into the members (in persisted row order), then the stored
//   `anchor_workspace_id` when still a member, then the first member.
// - `workspaces` is consumed in persisted order — pinned-ahead order and group
//   contiguity are invariants of the model array; this MUST NOT re-sort.

import type {
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import {
  renderItems,
  type SidebarWorkspaceRenderItem,
  type WorkspaceGroup,
  type WorkspaceRow,
} from "./renderItems";

/// The hyphenated UUID form — the only form cmux persists (Rust
/// `Uuid::to_string()`, Swift `UUID.uuidString`). Case-insensitive, like both.
const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/// Validates and lowercase-normalizes a persisted workspace/group id, or
/// `undefined` when absent/unparseable — the TS twin of `Uuid::parse_str(..)`
/// over the forms cmux actually persists.
export function workspaceIdKey(id: string | undefined): string | undefined {
  if (id === undefined || !UUID_PATTERN.test(id)) {
    return undefined;
  }
  return id.toLowerCase();
}

/// The label for a workspace row: custom title, else process title, else
/// "Terminal" (same fallback as the canonical sidebar row).
export function workspaceTitle(ws: SessionWorkspaceSnapshot): string {
  return ws.custom_title?.trim() || ws.process_title?.trim() || "Terminal";
}

/// The sidebar-relevant slice of the tab-manager snapshot (readonly view of
/// the generated `SessionTabManagerSnapshot` fields).
export type SidebarTabsSnapshot = {
  workspaces?: readonly SessionWorkspaceSnapshot[];
  workspace_groups?: readonly SessionWorkspaceGroupSnapshot[];
};

/// Projects the snapshot into the drawable `[groupHeader | workspace]` list.
export function projectSidebarItems(
  tabs: SidebarTabsSnapshot | null | undefined,
): SidebarWorkspaceRenderItem[] {
  const workspaces = tabs?.workspaces ?? [];

  const rows: WorkspaceRow[] = [];
  for (const ws of workspaces) {
    const id = workspaceIdKey(ws.workspace_id);
    if (id === undefined) {
      continue;
    }
    rows.push({
      id,
      groupId: workspaceIdKey(ws.group_id),
      isPinned: ws.is_pinned ?? false,
    });
  }

  // Members-by-group in persisted row order — the oracle's
  // `workspaceIdsByGroupId` (TabManager.swift:6000-6008), used both to drop
  // member-less groups and to resolve each group's anchor.
  const membersByGroupId = new Map<string, string[]>();
  for (const row of rows) {
    if (row.groupId !== undefined) {
      const members = membersByGroupId.get(row.groupId);
      if (members === undefined) {
        membersByGroupId.set(row.groupId, [row.id]);
      } else {
        members.push(row.id);
      }
    }
  }

  const groupsById = new Map<string, WorkspaceGroup>();
  for (const group of tabs?.workspace_groups ?? []) {
    const id = workspaceIdKey(group.id);
    if (id === undefined || groupsById.has(id)) {
      continue;
    }
    const members = membersByGroupId.get(id);
    if (members === undefined) {
      continue;
    }
    const index = group.anchor_member_index;
    const byIndex =
      index !== undefined && Number.isInteger(index) && index >= 0
        ? members[index]
        : undefined;
    const storedAnchor = workspaceIdKey(group.anchor_workspace_id);
    const anchorWorkspaceId =
      byIndex ??
      (storedAnchor !== undefined && members.includes(storedAnchor)
        ? storedAnchor
        : undefined) ??
      members[0];
    groupsById.set(id, {
      id,
      name: group.name,
      isCollapsed: group.is_collapsed,
      isPinned: group.is_pinned ?? false,
      anchorWorkspaceId,
      customColor: group.custom_color,
      iconSymbol: group.icon_symbol,
    });
  }

  return renderItems(rows, groupsById);
}

/// Titles keyed by normalized workspace id (first occurrence wins, mirroring
/// the projection's keep-first duplicate policy). Rows the projection skipped
/// (id-less) are absent here too.
export function workspaceTitlesById(
  workspaces: readonly SessionWorkspaceSnapshot[],
): Map<string, string> {
  const titles = new Map<string, string>();
  for (const ws of workspaces) {
    const id = workspaceIdKey(ws.workspace_id);
    if (id !== undefined && !titles.has(id)) {
      titles.set(id, workspaceTitle(ws));
    }
  }
  return titles;
}
