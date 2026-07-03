// Mirrors the `#[cfg(test)] mod tests` fixtures in
// `crates/cmux-workspaces/src/render_items.rs` 1:1. Rust uses `Uuid::new_v4()`;
// the port substitutes fixed UUID strings for determinism.

import { describe, expect, test } from "bun:test";

import {
  renderItemId,
  renderItems,
  rowWorkspaceId,
  type SidebarWorkspaceRenderItem,
  type WorkspaceGroup,
  type WorkspaceRow,
} from "./renderItems";

// Fixed UUIDs (deterministic stand-ins for Uuid::new_v4()).
const UUID = {
  gid: "11111111-1111-1111-1111-111111111111",
  anchor: "22222222-2222-2222-2222-222222222222",
  member: "33333333-3333-3333-3333-333333333333",
  solo: "44444444-4444-4444-4444-444444444444",
  member1: "55555555-5555-5555-5555-555555555555",
  member2: "66666666-6666-6666-6666-666666666666",
  outside: "77777777-7777-7777-7777-777777777777",
  orphan: "88888888-8888-8888-8888-888888888888",
} as const;

function row(
  id: string,
  group: string | undefined,
  pinned: boolean,
): WorkspaceRow {
  return { id, groupId: group, isPinned: pinned };
}

function group(
  id: string,
  anchor: string,
  collapsed: boolean,
): WorkspaceGroup {
  return {
    id,
    name: "G",
    isCollapsed: collapsed,
    isPinned: false,
    anchorWorkspaceId: anchor,
  };
}

function groupsMap(groups: WorkspaceGroup[]): Map<string, WorkspaceGroup> {
  return new Map(groups.map((g) => [g.id, g]));
}

describe("render_items", () => {
  test("empty_tabs_render_nothing", () => {
    expect(renderItems([], new Map())).toEqual([]);
  });

  test("anchor_is_suppressed_and_header_carries_members", () => {
    const { gid, anchor, member, solo } = UUID;
    const tabs = [
      row(anchor, gid, false),
      row(member, gid, false),
      row(solo, undefined, false),
    ];
    const groups = groupsMap([group(gid, anchor, false)]);
    const items = renderItems(tabs, groups);
    expect(items.length).toBe(3);

    const header = items[0];
    if (header.kind !== "groupHeader") {
      throw new Error(`expected header, got ${JSON.stringify(header)}`);
    }
    expect(header.group.id).toBe(gid);
    // Anchor + member, in tab order.
    expect(header.memberWorkspaceIds).toEqual([anchor, member]);

    expect(renderItemId(items[0])).toEqual({ kind: "group", id: gid });
    expect(rowWorkspaceId(items[0])).toBe(anchor);
    expect(items[1]).toEqual({ kind: "workspace", workspace: tabs[1] });
    expect(items[2]).toEqual({ kind: "workspace", workspace: tabs[2] });
  });

  test("collapsed_group_hides_non_anchor_members", () => {
    const { gid, anchor, member } = UUID;
    const tabs = [row(anchor, gid, false), row(member, gid, false)];
    const groups = groupsMap([group(gid, anchor, true)]); // collapsed
    const items = renderItems(tabs, groups);
    // Only the header survives; the collapsed member row is suppressed.
    expect(items.length).toBe(1);
    expect(items[0].kind).toBe("groupHeader");
  });

  test("split_group_runs_dedup_header_and_reuse_collapse_decision", () => {
    // A member appearing in a second run must not re-emit the header, and must
    // honor the same collapse decision recorded at the first run.
    const { gid, anchor, member1, member2, outside } = UUID;
    const tabs = [
      row(anchor, gid, false),
      row(member1, gid, false),
      row(outside, undefined, false),
      row(member2, gid, false),
    ];
    const groups = groupsMap([group(gid, anchor, true)]); // collapsed
    const items = renderItems(tabs, groups);
    // Header (once) + the ungrouped outside row; both collapsed members hidden.
    expect(items.length).toBe(2);
    expect(renderItemId(items[0])).toEqual({ kind: "group", id: gid });
    expect(items[1]).toEqual({ kind: "workspace", workspace: tabs[2] });
  });

  test("missing_group_metadata_renders_member_as_plain_row", () => {
    // groupId points at a group not in the map → rendered as a plain row.
    const { gid, orphan } = UUID;
    const tabs = [row(orphan, gid, false)];
    const items = renderItems(tabs, new Map());
    expect(items).toEqual([
      { kind: "workspace", workspace: tabs[0] } as SidebarWorkspaceRenderItem,
    ]);
  });
});
