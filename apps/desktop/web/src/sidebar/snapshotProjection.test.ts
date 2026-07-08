// Ports the `src-tauri/src/sidebar_render.rs` test suite so the web twin of
// the snapshot → (rows, groups) mapping stays behavior-identical: skip
// invalid/absent ids, drop member-less groups, de-dup group ids keep-first,
// and the 3-tier anchor fallback (member index → stored anchor → first member).

import { describe, expect, test } from "bun:test";

import type {
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import { rowWorkspaceId } from "./renderItems";
import {
  projectSidebarItems,
  workspaceIdKey,
  workspaceTitle,
  workspaceTitlesById,
} from "./snapshotProjection";

const GID = "11111111-1111-1111-1111-111111111111";
const ANCHOR = "22222222-2222-2222-2222-222222222222";
const MEMBER = "33333333-3333-3333-3333-333333333333";
const SOLO = "44444444-4444-4444-4444-444444444444";

function workspace(
  id: string | undefined,
  groupId?: string,
  isPinned?: boolean,
): SessionWorkspaceSnapshot {
  return {
    workspace_id: id,
    group_id: groupId,
    is_pinned: isPinned,
    process_title: "Terminal",
    layout: null,
  };
}

function group(
  id: string,
  anchor: string | undefined,
  isCollapsed: boolean,
  anchorMemberIndex?: number,
): SessionWorkspaceGroupSnapshot {
  return {
    id,
    name: "G",
    is_collapsed: isCollapsed,
    anchor_workspace_id: anchor,
    anchor_member_index: anchorMemberIndex,
  };
}

describe("projectSidebarItems", () => {
  test("empty snapshot renders nothing", () => {
    expect(projectSidebarItems({ workspaces: [] })).toEqual([]);
    expect(projectSidebarItems(null)).toEqual([]);
    expect(projectSidebarItems(undefined)).toEqual([]);
  });

  test("anchor is suppressed and header carries members", () => {
    const items = projectSidebarItems({
      workspaces: [
        workspace(ANCHOR, GID),
        workspace(MEMBER, GID),
        workspace(SOLO),
      ],
      workspace_groups: [group(GID, ANCHOR, false)],
    });
    expect(items).toHaveLength(3);
    const header = items[0];
    if (header.kind !== "groupHeader") {
      throw new Error("expected header");
    }
    expect(header.group.id).toBe(GID);
    expect(header.memberWorkspaceIds).toEqual([ANCHOR, MEMBER]);
    expect(rowWorkspaceId(items[0])).toBe(ANCHOR);
    expect(rowWorkspaceId(items[1])).toBe(MEMBER);
    expect(rowWorkspaceId(items[2])).toBe(SOLO);
  });

  test("collapsed group hides non-anchor members", () => {
    const items = projectSidebarItems({
      workspaces: [workspace(ANCHOR, GID), workspace(MEMBER, GID)],
      workspace_groups: [group(GID, ANCHOR, true)],
    });
    expect(items).toHaveLength(1);
    expect(items[0].kind).toBe("groupHeader");
  });

  test("persisted order is honored as given (no re-sort)", () => {
    // Deliberately "wrong" order (unpinned first) to guard against an
    // accidental pinned-ahead sort creeping into the projection.
    const items = projectSidebarItems({
      workspaces: [workspace(MEMBER, undefined, false), workspace(SOLO, undefined, true)],
    });
    expect(items.map(rowWorkspaceId)).toEqual([MEMBER, SOLO]);
  });

  test("is_pinned absent maps to false", () => {
    const items = projectSidebarItems({ workspaces: [workspace(SOLO)] });
    const item = items[0];
    if (item.kind !== "workspace") {
      throw new Error("expected workspace row");
    }
    expect(item.workspace.isPinned).toBe(false);
  });

  test("missing anchor falls back to first member (group still renders)", () => {
    const items = projectSidebarItems({
      workspaces: [workspace(ANCHOR, GID), workspace(MEMBER, GID)],
      workspace_groups: [group(GID, undefined, false)],
    });
    expect(items).toHaveLength(2);
    expect(items[0].kind).toBe("groupHeader");
    expect(rowWorkspaceId(items[0])).toBe(ANCHOR); // members[0]
    expect(rowWorkspaceId(items[1])).toBe(MEMBER);
  });

  test("stale anchor (not a member) re-anchors to first member", () => {
    const items = projectSidebarItems({
      workspaces: [workspace(ANCHOR, GID), workspace(MEMBER, GID)],
      workspace_groups: [group(GID, SOLO, false)],
    });
    expect(items).toHaveLength(2);
    expect(rowWorkspaceId(items[0])).toBe(ANCHOR); // members[0], not SOLO
    expect(rowWorkspaceId(items[1])).toBe(MEMBER);
  });

  test("anchor_member_index is preferred over the stored anchor", () => {
    const items = projectSidebarItems({
      workspaces: [workspace(ANCHOR, GID), workspace(MEMBER, GID)],
      workspace_groups: [group(GID, ANCHOR, false, /* index */ 1)],
    });
    expect(items).toHaveLength(2);
    // Header anchored to MEMBER (index 1) -> ANCHOR survives as a plain row.
    expect(rowWorkspaceId(items[0])).toBe(MEMBER);
    expect(rowWorkspaceId(items[1])).toBe(ANCHOR);
  });

  test("out-of-range anchor_member_index falls through to the stored anchor", () => {
    const items = projectSidebarItems({
      workspaces: [workspace(ANCHOR, GID), workspace(MEMBER, GID)],
      workspace_groups: [group(GID, MEMBER, false, /* index */ 9)],
    });
    expect(items).toHaveLength(2);
    expect(rowWorkspaceId(items[0])).toBe(MEMBER); // stored anchor
    expect(rowWorkspaceId(items[1])).toBe(ANCHOR);
  });

  test("absent and invalid workspace ids are skipped", () => {
    const items = projectSidebarItems({
      workspaces: [
        workspace(undefined),
        workspace("not-a-uuid"),
        workspace(SOLO),
      ],
    });
    expect(items.map(rowWorkspaceId)).toEqual([SOLO]);
  });

  test("member-less groups are dropped; duplicate group ids keep the first", () => {
    const first = group(GID, ANCHOR, false);
    const dupCollapsed = group(GID, ANCHOR, true);
    const memberless = group(SOLO, undefined, false);
    const items = projectSidebarItems({
      workspaces: [workspace(ANCHOR, GID), workspace(MEMBER, GID)],
      workspace_groups: [first, dupCollapsed, memberless],
    });
    // First (expanded) wins: header + member row both render.
    expect(items).toHaveLength(2);
    expect(items[0].kind).toBe("groupHeader");
  });

  test("no groups renders all as plain rows", () => {
    const items = projectSidebarItems({
      workspaces: [workspace(ANCHOR, GID), workspace(SOLO)],
    });
    expect(items).toHaveLength(2);
    expect(items.every((item) => item.kind === "workspace")).toBe(true);
  });

  test("mixed-case ids normalize and still match (Uuid parse parity)", () => {
    const upper = ANCHOR.toUpperCase();
    const items = projectSidebarItems({
      workspaces: [workspace(upper, GID.toUpperCase()), workspace(MEMBER, GID)],
      workspace_groups: [group(GID, upper, false)],
    });
    // Header (anchor suppressed) + the non-anchor member row.
    expect(items).toHaveLength(2);
    expect(items[0].kind).toBe("groupHeader");
    expect(rowWorkspaceId(items[0])).toBe(ANCHOR); // lowercased
    expect(rowWorkspaceId(items[1])).toBe(MEMBER);
  });
});

describe("workspaceIdKey", () => {
  test("validates the hyphenated uuid form and lowercases", () => {
    expect(workspaceIdKey(ANCHOR.toUpperCase())).toBe(ANCHOR);
    expect(workspaceIdKey(undefined)).toBeUndefined();
    expect(workspaceIdKey("not-a-uuid")).toBeUndefined();
    expect(workspaceIdKey("22222222222222222222222222222222")).toBeUndefined();
  });
});

describe("workspaceTitlesById", () => {
  test("keys normalized ids to custom/process/Terminal fallback titles", () => {
    const titles = workspaceTitlesById([
      { ...workspace(ANCHOR.toUpperCase()), custom_title: " Custom " },
      { ...workspace(MEMBER), process_title: "zsh" },
      { ...workspace(SOLO), process_title: "  " },
      workspace(undefined),
    ]);
    expect(titles.get(ANCHOR)).toBe("Custom");
    expect(titles.get(MEMBER)).toBe("zsh");
    expect(titles.get(SOLO)).toBe("Terminal");
    expect(titles.size).toBe(3);
  });

  test("workspaceTitle trims and falls back", () => {
    expect(workspaceTitle({ process_title: " zsh ", layout: null })).toBe("zsh");
  });
});
