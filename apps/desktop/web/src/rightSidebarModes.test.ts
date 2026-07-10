import { describe, expect, test } from "bun:test";

import {
  rightSidebarModeItems,
  rightSidebarStateFromRemote,
  type RightSidebarState,
} from "./rightSidebarModes";

describe("right sidebar remote state", () => {
  const filesClosed: RightSidebarState = { visible: false, mode: "files" };

  test("applies canonical visibility and mode payloads", () => {
    expect(
      rightSidebarStateFromRemote(filesClosed, {
        visible: true,
        mode: "sessions",
        focus: true,
      }),
    ).toEqual({ visible: true, mode: "sessions" });
  });

  test("preserves current fields omitted by an event", () => {
    expect(
      rightSidebarStateFromRemote(
        { visible: true, mode: "find" },
        { visible: false },
      ),
    ).toEqual({ visible: false, mode: "find" });
  });

  test("accepts Feed events but ignores unimplemented or malformed modes", () => {
    expect(
      rightSidebarStateFromRemote(filesClosed, { visible: true, mode: "feed" }),
    ).toEqual({ visible: true, mode: "feed" });
    expect(
      rightSidebarStateFromRemote(filesClosed, { visible: true, mode: "dock" }),
    ).toEqual({ visible: true, mode: "files" });
    expect(
      rightSidebarStateFromRemote(filesClosed, { visible: true, mode: 42 }),
    ).toEqual({ visible: true, mode: "files" });
  });

  test("adds beta modes only when enabled", () => {
    expect(rightSidebarModeItems({ feedEnabled: false, dockEnabled: false })).toEqual([
      { mode: "files", label: "Files" },
      { mode: "find", label: "Find" },
      { mode: "sessions", label: "Vault" },
    ]);
    expect(rightSidebarModeItems({ feedEnabled: true, dockEnabled: false })).toContainEqual({
      mode: "feed",
      label: "Feed",
    });
  });
});
