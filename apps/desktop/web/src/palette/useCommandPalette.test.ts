import { describe, expect, test } from "bun:test";

import type { SessionWorkspaceSnapshot } from "@cmux/core-types";

import {
  markUnreadAndJumpNextTarget,
  nextUnreadTarget,
  oldestUnreadTarget,
} from "./useCommandPalette";

function workspace(
  panelIds: readonly string[],
  unreadPanelIds: readonly string[] = [],
  unreadAtByPanelId: Readonly<Record<string, number>> = {},
): SessionWorkspaceSnapshot {
  return {
    process_title: "Workspace",
    layout: {
      type: "pane",
      pane: {
        panel_ids: [...panelIds],
        selected_panel_id: panelIds[0],
      },
    },
    panel_unreads:
      unreadPanelIds.length === 0
        ? undefined
        : unreadPanelIds.map((panel_id) => ({
            panel_id,
            is_unread: true,
            unread_at: unreadAtByPanelId[panel_id],
          })),
  };
}

describe("nextUnreadTarget", () => {
  test("returns the selected workspace when it already has unread state", () => {
    expect(
      nextUnreadTarget(
        [workspace(["surface-1"]), workspace(["surface-2"], ["surface-2"])],
        1,
      ),
    ).toEqual({ workspaceIndex: 1, panelId: "surface-2" });
  });

  test("wraps from the selected workspace to the next unread workspace", () => {
    expect(
      nextUnreadTarget(
        [
          workspace(["surface-1"], ["surface-1"]),
          workspace(["surface-2"]),
          workspace(["surface-3"], ["surface-3"]),
        ],
        1,
      ),
    ).toEqual({ workspaceIndex: 2, panelId: "surface-3" });
  });

  test("falls back to a stale unread id only when no live unread id exists", () => {
    expect(
      nextUnreadTarget([workspace(["surface-1"], ["missing"])], 0),
    ).toEqual({ workspaceIndex: 0, panelId: "missing" });
  });

  test("returns null when there are no unread workspaces", () => {
    expect(nextUnreadTarget([workspace(["surface-1"])], 0)).toBeNull();
  });
});

describe("oldestUnreadTarget", () => {
  test("uses unread timestamps when every unread entry is stamped", () => {
    expect(
      oldestUnreadTarget(
        [
          workspace(["surface-1"], ["surface-1"], { "surface-1": 30 }),
          workspace(["surface-2"], ["surface-2"], { "surface-2": 10 }),
          workspace(["surface-3"], ["surface-3"], { "surface-3": 20 }),
        ],
        0,
      ),
    ).toEqual({ workspaceIndex: 1, panelId: "surface-2" });
  });

  test("targets the timestamp-winning panel inside a multi-unread workspace", () => {
    expect(
      oldestUnreadTarget(
        [
          workspace(["surface-1", "surface-2"], ["surface-1", "surface-2"], {
            "surface-1": 30,
            "surface-2": 10,
          }),
        ],
        0,
      ),
    ).toEqual({ workspaceIndex: 0, panelId: "surface-2" });
  });

  test("falls back to visible next-unread order for legacy unstamped entries", () => {
    expect(
      oldestUnreadTarget(
        [
          workspace(["surface-1"], ["surface-1"]),
          workspace(["surface-2"]),
          workspace(["surface-3"], ["surface-3"]),
        ],
        1,
      ),
    ).toEqual({ workspaceIndex: 2, panelId: "surface-3" });
  });
});

describe("markUnreadAndJumpNextTarget", () => {
  test("marks the oldest timestamped unread target", () => {
    expect(
      markUnreadAndJumpNextTarget(
        [
          workspace(["surface-1"], ["surface-1"], { "surface-1": 30 }),
          workspace(["surface-2"], ["surface-2"], { "surface-2": 10 }),
          workspace(["surface-3"], ["surface-3"], { "surface-3": 20 }),
        ],
        0,
      ),
    ).toEqual({
      marked: { workspaceIndex: 1, panelId: "surface-2" },
      next: { workspaceIndex: 2, panelId: "surface-3" },
    });
  });

  test("marks the next unread target and jumps to the following unread workspace", () => {
    expect(
      markUnreadAndJumpNextTarget(
        [
          workspace(["surface-1"], ["surface-1"]),
          workspace(["surface-2"], ["surface-2"]),
          workspace(["surface-3"], ["surface-3"]),
        ],
        1,
      ),
    ).toEqual({
      marked: { workspaceIndex: 1, panelId: "surface-2" },
      next: { workspaceIndex: 2, panelId: "surface-3" },
    });
  });

  test("wraps after marking the current unread target read", () => {
    expect(
      markUnreadAndJumpNextTarget(
        [
          workspace(["surface-1"], ["surface-1"]),
          workspace(["surface-2"], ["surface-2"]),
        ],
        1,
      ),
    ).toEqual({
      marked: { workspaceIndex: 1, panelId: "surface-2" },
      next: { workspaceIndex: 0, panelId: "surface-1" },
    });
  });

  test("marks the only unread target and has no jump target", () => {
    expect(
      markUnreadAndJumpNextTarget(
        [workspace(["surface-1"]), workspace(["surface-2"], ["surface-2"])],
        0,
      ),
    ).toEqual({
      marked: { workspaceIndex: 1, panelId: "surface-2" },
      next: null,
    });
  });
});
