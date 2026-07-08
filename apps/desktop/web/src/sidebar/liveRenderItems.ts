// Pure projection from the live `SessionTabManagerSnapshot` into the drawable
// sidebar `[groupHeader | workspace]` list — the TS mirror of
// `apps/desktop/src-tauri/src/sidebar_render.rs`, which maps the snapshot's
// optional-string fields onto the ported value types and delegates to the
// golden-pinned `renderItems` (`./renderItems.ts` ⇄ `cmux_workspaces`).
//
// Parity: the canonical caller (`ContentView.swift:10395,10425-10428`) passes
// `tabManager.tabs` UNSORTED — pinned-ahead order and group contiguity are
// already baked into the model tab array — so this projection consumes
// `snapshot.workspaces` as-is and MUST NOT re-sort or re-group.
//
// DIVERGENCES (mirroring the Rust helper's documented ones):
// - A workspace whose `workspace_id` is absent or not a canonical hyphenated
//   UUID is skipped (canonical restore mints a fresh UUID once in the stateful
//   restore layer; a stateless projection cannot re-mint per render). cmux
//   always persists ids, so this only affects corrupted / pre-id snapshots.
//   NOTE the Rust helper accepts any `Uuid::parse_str` form (simple/braced/urn);
//   this port pins the Swift `UUID(uuidString:)` hyphenated 36-char form — the
//   only form either side ever persists.
// - A group's anchor resolves with the oracle's 3-tier fallback
//   (`TabManager.swift:6018-6027`): `anchor_member_index` into the members in
//   persisted order, then the stored `anchor_workspace_id` when still a member,
//   then the first member. Member-less groups are dropped; duplicate group ids
//   keep the first occurrence.
//
// Ids are normalized to lowercase so comparisons match both oracles' UUID
// case-insensitivity (Swift uppercases, Rust `Uuid` compares parsed values).
// Consumers (title maps, selection sets) must use {@link normalizedWorkspaceId}
// for their own keys so lookups stay consistent.

import type { SessionTabManagerSnapshot } from "@cmux/core-types";

import {
  renderItems,
  type SidebarWorkspaceRenderItem,
  type WorkspaceGroup,
  type WorkspaceRow,
} from "./renderItems";

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/**
 * The id lowercased when it is a canonical hyphenated UUID, else `undefined`.
 * The shared id-normalization used by the projection and its consumers.
 */
export function normalizedWorkspaceId(
  value: string | null | undefined,
): string | undefined {
  if (typeof value !== "string") {
    return undefined;
  }
  const lower = value.toLowerCase();
  return UUID_PATTERN.test(lower) ? lower : undefined;
}

/** Projects a live tab-manager snapshot into the drawable sidebar list. */
export function snapshotRenderItems(
  snapshot: SessionTabManagerSnapshot,
): SidebarWorkspaceRenderItem[] {
  const rows: WorkspaceRow[] = [];
  for (const workspace of snapshot.workspaces) {
    const id = normalizedWorkspaceId(workspace.workspace_id);
    if (id === undefined) {
      continue;
    }
    rows.push({
      id,
      groupId: normalizedWorkspaceId(workspace.group_id),
      isPinned: workspace.is_pinned ?? false,
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
  for (const group of snapshot.workspace_groups ?? []) {
    const id = normalizedWorkspaceId(group.id);
    if (id === undefined) {
      continue;
    }
    // Drop groups with no restorable members; de-dup by id keeping the first
    // occurrence (TabManager.swift:6010-6012).
    const members = membersByGroupId.get(id);
    if (members === undefined || groupsById.has(id)) {
      continue;
    }
    // 3-tier anchor fallback (TabManager.swift:6018-6027): restore-stable
    // member index, then the stored anchor id when still a member, then the
    // first member — a group with members ALWAYS renders a header.
    const index = group.anchor_member_index;
    const byIndex =
      typeof index === "number" &&
      Number.isInteger(index) &&
      index >= 0 &&
      index < members.length
        ? members[index]
        : undefined;
    const storedAnchor = normalizedWorkspaceId(group.anchor_workspace_id);
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
      customColor: group.custom_color ?? undefined,
      iconSymbol: group.icon_symbol ?? undefined,
    });
  }

  return renderItems(rows, groupsById);
}
