// Oracle tests for reorder.ts, pinned to Swift semantics.
//
// The first block ports all 4 Swift tests verbatim from
// `Packages/macOS/CmuxWorkspaces/Tests/CmuxWorkspacesTests/WorkspaceReorderPlannerTests.swift:1-75`
// (UUIDs become canonical-lowercase opaque strings); the remaining tables are
// hand-derived from `WorkspaceReorderPlanner.swift:12-67`.

import { describe, expect, test } from "bun:test";

import {
  batchReorderFinalIds,
  batchReorderPlan,
  type WorkspaceOrderSnapshot,
} from "./reorder";

function unpinned(ids: string[]): WorkspaceOrderSnapshot[] {
  return ids.map((id) => ({ id, isPinned: false }));
}

describe("batchReorderPlan — Swift oracle (WorkspaceReorderPlannerTests.swift)", () => {
  test("moves requested unpinned workspaces ahead of unmentioned ones", () => {
    const all = ["a", "b", "c"];
    const current = unpinned(all);

    const result = batchReorderPlan(["c", "a"], current);

    expect(result).toEqual({
      ok: true,
      plan: [
        { workspaceId: "c", fromIndex: 2, toIndex: 0 },
        { workspaceId: "a", fromIndex: 0, toIndex: 1 },
      ],
    });
    expect(batchReorderFinalIds(["c", "a"], current)).toEqual(["c", "a", "b"]);
  });

  test("keeps pinned workspaces ahead of unpinned", () => {
    const current: WorkspaceOrderSnapshot[] = [
      { id: "pinned-a", isPinned: true },
      { id: "pinned-b", isPinned: true },
      { id: "unpinned-a", isPinned: false },
      { id: "unpinned-b", isPinned: false },
    ];

    // Request interleaves an unpinned ahead of a pinned; the final order
    // still puts every pinned id ahead of every unpinned id.
    expect(batchReorderFinalIds(["unpinned-b", "pinned-b"], current)).toEqual([
      "pinned-b",
      "pinned-a",
      "unpinned-b",
      "unpinned-a",
    ]);
  });

  test("rejects duplicates before unknown workspaces", () => {
    const current = unpinned(["known"]);

    expect(batchReorderPlan(["known", "known", "unknown"], current)).toEqual({
      ok: false,
      error: { kind: "duplicateWorkspace", workspaceId: "known" },
    });

    expect(batchReorderPlan(["unknown"], current)).toEqual({
      ok: false,
      error: { kind: "workspaceNotFound", workspaceId: "unknown" },
    });
  });

  test("empty request plans no moves", () => {
    const all = ["a", "b"];
    const current = unpinned(all);

    expect(batchReorderPlan([], current)).toEqual({ ok: true, plan: [] });
    expect(batchReorderFinalIds([], current)).toEqual(all);
  });
});

describe("batchReorderPlan — edges", () => {
  test("duplicate detection reports the first repeated id in scan order", () => {
    const current = unpinned(["a", "b", "c"]);
    // "b" is the first id whose insert fails (its second occurrence comes
    // before "a"'s), so it is reported, not "a".
    expect(batchReorderPlan(["a", "b", "b", "a"], current)).toEqual({
      ok: false,
      error: { kind: "duplicateWorkspace", workspaceId: "b" },
    });
  });

  test("workspaceNotFound reports the first missing id in request order", () => {
    const current = unpinned(["a"]);
    expect(batchReorderPlan(["a", "x", "y"], current)).toEqual({
      ok: false,
      error: { kind: "workspaceNotFound", workspaceId: "x" },
    });
  });

  test("duplicate ids in `current` throw (mirrors the Swift Dictionary trap)", () => {
    const current = unpinned(["a", "a"]);
    expect(() => batchReorderPlan(["a"], current)).toThrow(
      "duplicate workspace id in current order: a",
    );
  });

  test("single-element no-op request produces a 0→0 plan item", () => {
    const current = unpinned(["a", "b"]);
    expect(batchReorderPlan(["a"], current)).toEqual({
      ok: true,
      plan: [{ workspaceId: "a", fromIndex: 0, toIndex: 0 }],
    });
  });

  test("request mentioning every id reverses fully", () => {
    const current = unpinned(["a", "b", "c"]);
    expect(batchReorderPlan(["c", "b", "a"], current)).toEqual({
      ok: true,
      plan: [
        { workspaceId: "c", fromIndex: 2, toIndex: 0 },
        { workspaceId: "b", fromIndex: 1, toIndex: 1 },
        { workspaceId: "a", fromIndex: 0, toIndex: 2 },
      ],
    });
  });
});

describe("batchReorderFinalIds — unvalidated edges", () => {
  test("unknown requested id silently drops from the output", () => {
    // undefined !== true and undefined !== false, so an id absent from
    // `current` fails both pinned filters — exactly like Swift's optional
    // chaining against the snapshot dictionary.
    const current = unpinned(["a", "b"]);
    expect(batchReorderFinalIds(["ghost", "b"], current)).toEqual(["b", "a"]);
  });

  test("duplicate ids in `current` throw (mirrors the Swift Dictionary trap)", () => {
    // Swift builds `snapshotsById` with `Dictionary(uniqueKeysWithValues:)`
    // (WorkspaceReorderPlanner.swift:56), which TRAPS on a duplicate id. The
    // standalone port must throw rather than silently take last-wins — a plain
    // `new Map` would keep the last duplicate and diverge from Swift's crash.
    const current = unpinned(["a", "a"]);
    expect(() => batchReorderFinalIds(["a"], current)).toThrow(
      "duplicate workspace id in current order: a",
    );
  });

  test("mixed pinned interleavings keep group-internal request/current order", () => {
    const current: WorkspaceOrderSnapshot[] = [
      { id: "p1", isPinned: true },
      { id: "u1", isPinned: false },
      { id: "p2", isPinned: true },
      { id: "u2", isPinned: false },
      { id: "p3", isPinned: true },
    ];
    // Requested pinned in request order, remaining pinned in current order,
    // then requested unpinned, then remaining unpinned.
    expect(batchReorderFinalIds(["u2", "p3", "p1"], current)).toEqual([
      "p3",
      "p1",
      "p2",
      "u2",
      "u1",
    ]);
  });
});
