// Mirrors the Rust test suite in `apps/desktop/src-tauri/src/sidebar_render.rs`
// (same fixtures, same expectations) so the two snapshot→items projections
// cannot drift apart silently.

import { describe, expect, test } from "bun:test";

import type {
  SessionTabManagerSnapshot,
  SessionWorkspaceGroupSnapshot,
  SessionWorkspaceSnapshot,
} from "@cmux/core-types";

import {
  normalizedWorkspaceId,
  snapshotRenderItems,
} from "./liveRenderItems";
import { rowWorkspaceId } from "./renderItems";

const GID = "11111111-1111-1111-1111-111111111111";
const ANCHOR = "22222222-2222-2222-2222-222222222222";
const MEMBER = "33333333-3333-3333-3333-333333333333";
const SOLO = "44444444-4444-4444-4444-444444444444";

function workspace(
  id: string | null,
  groupId?: string,
  isPinned?: boolean,
): SessionWorkspaceSnapshot {
  return {
    workspace_id: id ?? undefined,
    group_id: groupId,
    is_pinned: isPinned,
  } as SessionWorkspaceSnapshot;
}

function group(
  id: string,
  anchor: string | undefined,
  isCollapsed: boolean,
): SessionWorkspaceGroupSnapshot {
  return {
    id,
    name: "G",
    is_collapsed: isCollapsed,
    anchor_workspace_id: anchor,
  } as SessionWorkspaceGroupSnapshot;
}

function tabs(
  workspaces: SessionWorkspaceSnapshot[],
  groups?: SessionWorkspaceGroupSnapshot[],
): SessionTabManagerSnapshot {
  return {
    workspaces,
    workspace_groups: groups,
  } as SessionTabManagerSnapshot;
}

describe("snapshotRenderItems", () => {
  test("empty snapshot renders nothing", () => {
    expect(snapshotRenderItems(tabs([]))).toEqual([]);
  });

  test("anchor is suppressed and header carries members", () => {
    const items = snapshotRenderItems(
      tabs(
        [workspace(ANCHOR, GID), workspace(MEMBER, GID), workspace(SOLO)],
        [group(GID, ANCHOR, false)],
      ),
    );
    expect(items).toHaveLength(3);
    const header = items[0]!;
    if (header.kind !== "groupHeader") {
      throw new Error(`expected header, got ${header.kind}`);
    }
    expect(header.group.id).toBe(GID);
    expect(header.memberWorkspaceIds).toEqual([ANCHOR, MEMBER]);
    expect(rowWorkspaceId(items[0]!)).toBe(ANCHOR);
    expect(rowWorkspaceId(items[1]!)).toBe(MEMBER);
    expect(rowWorkspaceId(items[2]!)).toBe(SOLO);
  });

  test("collapsed group hides non-anchor members", () => {
    const items = snapshotRenderItems(
      tabs(
        [workspace(ANCHOR, GID), workspace(MEMBER, GID)],
        [group(GID, ANCHOR, true)],
      ),
    );
    expect(items).toHaveLength(1);
    expect(items[0]!.kind).toBe("groupHeader");
  });

  test("pinned-ahead order is honored as given", () => {
    const items = snapshotRenderItems(
      tabs([workspace(SOLO, undefined, true), workspace(MEMBER, undefined, false)]),
    );
    expect(items.map(rowWorkspaceId)).toEqual([SOLO, MEMBER]);
  });

  test("unpinned before pinned is not resorted", () => {
    const items = snapshotRenderItems(
      tabs([workspace(MEMBER, undefined, false), workspace(SOLO, undefined, true)]),
    );
    expect(items.map(rowWorkspaceId)).toEqual([MEMBER, SOLO]);
  });

  test("is_pinned absent maps to false", () => {
    const items = snapshotRenderItems(tabs([workspace(SOLO)]));
    expect(items).toHaveLength(1);
    const row = items[0]!;
    if (row.kind !== "workspace") {
      throw new Error(`expected workspace row, got ${row.kind}`);
    }
    expect(row.workspace.isPinned).toBe(false);
  });

  test("missing anchor falls back to first member", () => {
    const items = snapshotRenderItems(
      tabs(
        [workspace(ANCHOR, GID), workspace(MEMBER, GID)],
        [group(GID, undefined, false)],
      ),
    );
    expect(items).toHaveLength(2);
    expect(items[0]!.kind).toBe("groupHeader");
    expect(rowWorkspaceId(items[0]!)).toBe(ANCHOR); // members[0]
    expect(rowWorkspaceId(items[1]!)).toBe(MEMBER);
  });

  test("stale anchor not a member re-anchors to first member", () => {
    const items = snapshotRenderItems(
      tabs(
        [workspace(ANCHOR, GID), workspace(MEMBER, GID)],
        [group(GID, SOLO, false)],
      ),
    );
    expect(items).toHaveLength(2);
    expect(rowWorkspaceId(items[0]!)).toBe(ANCHOR); // members[0], not SOLO
    expect(rowWorkspaceId(items[1]!)).toBe(MEMBER);
  });

  test("anchor_member_index is preferred over stored anchor", () => {
    const g = { ...group(GID, ANCHOR, false), anchor_member_index: 1 };
    const items = snapshotRenderItems(
      tabs([workspace(ANCHOR, GID), workspace(MEMBER, GID)], [g]),
    );
    expect(items).toHaveLength(2);
    // Header anchored to MEMBER (index 1) → MEMBER suppressed as a row,
    // ANCHOR (index 0) survives as a plain row.
    expect(rowWorkspaceId(items[0]!)).toBe(MEMBER);
    expect(rowWorkspaceId(items[1]!)).toBe(ANCHOR);
  });

  test("out-of-range anchor_member_index falls through to stored anchor", () => {
    const g = { ...group(GID, MEMBER, false), anchor_member_index: 9 };
    const items = snapshotRenderItems(
      tabs([workspace(ANCHOR, GID), workspace(MEMBER, GID)], [g]),
    );
    expect(items).toHaveLength(2);
    expect(rowWorkspaceId(items[0]!)).toBe(MEMBER); // stored anchor
    expect(rowWorkspaceId(items[1]!)).toBe(ANCHOR);
  });

  test("absent and invalid workspace ids are skipped", () => {
    const items = snapshotRenderItems(
      tabs([workspace(null), workspace("not-a-uuid"), workspace(SOLO)]),
    );
    expect(items.map(rowWorkspaceId)).toEqual([SOLO]);
  });

  test("workspace_groups absent renders all as plain rows", () => {
    const items = snapshotRenderItems(
      tabs([workspace(ANCHOR, GID), workspace(SOLO)]),
    );
    expect(items).toHaveLength(2);
    expect(items.every((item) => item.kind === "workspace")).toBe(true);
  });

  test("uppercase ids normalize so anchor matching stays case-insensitive", () => {
    // Swift `UUID(uuidString:)` uppercases; the projection lowercases both
    // sides so a stored uppercase anchor still matches its member row.
    const items = snapshotRenderItems(
      tabs(
        [workspace(ANCHOR.toUpperCase(), GID), workspace(MEMBER, GID.toUpperCase())],
        [group(GID, ANCHOR.toUpperCase(), false)],
      ),
    );
    expect(items).toHaveLength(2);
    expect(items[0]!.kind).toBe("groupHeader");
    expect(rowWorkspaceId(items[0]!)).toBe(ANCHOR);
    expect(rowWorkspaceId(items[1]!)).toBe(MEMBER);
  });
});

describe("normalizedWorkspaceId", () => {
  test("accepts hyphenated UUIDs case-insensitively", () => {
    expect(normalizedWorkspaceId(SOLO)).toBe(SOLO);
    expect(normalizedWorkspaceId(SOLO.toUpperCase())).toBe(SOLO);
  });

  test("rejects absent and non-UUID values", () => {
    expect(normalizedWorkspaceId(undefined)).toBeUndefined();
    expect(normalizedWorkspaceId(null)).toBeUndefined();
    expect(normalizedWorkspaceId("not-a-uuid")).toBeUndefined();
    // Simple (unhyphenated) form: accepted by Rust `Uuid::parse_str`, but the
    // Swift oracle `UUID(uuidString:)` rejects it — pinned to the Swift form.
    expect(
      normalizedWorkspaceId("44444444444444444444444444444444"),
    ).toBeUndefined();
  });
});
