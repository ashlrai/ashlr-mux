import { describe, expect, test } from "bun:test";

import {
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

  test("ignores unavailable or malformed remote modes", () => {
    expect(
      rightSidebarStateFromRemote(filesClosed, { visible: true, mode: "dock" }),
    ).toEqual({ visible: true, mode: "files" });
    expect(
      rightSidebarStateFromRemote(filesClosed, { visible: true, mode: 42 }),
    ).toEqual({ visible: true, mode: "files" });
  });
});
