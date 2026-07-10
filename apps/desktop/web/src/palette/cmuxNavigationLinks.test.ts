import { describe, expect, test } from "bun:test";

import { paneLink, surfaceLink, workspaceLink } from "./cmuxNavigationLinks";

describe("cmuxNavigationLinks", () => {
  test("workspaceLink matches the canonical same-session workspace route", () => {
    expect(workspaceLink("a1b2c3d4-e5f6-4789-8abc-def012345678")).toBe(
      "cmux://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678",
    );
  });

  test("workspaceLink can use the active host callback scheme", () => {
    expect(
      workspaceLink(
        "a1b2c3d4-e5f6-4789-8abc-def012345678",
        "cmux-dev-feature",
      ),
    ).toBe("cmux-dev-feature://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678");
  });

  test("surfaceLink matches the canonical same-session surface route", () => {
    expect(
      surfaceLink(
        "a1b2c3d4-e5f6-4789-8abc-def012345678",
        "surface-1",
      ),
    ).toBe(
      "cmux://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678/surface/surface-1",
    );
  });

  test("surfaceLink can use the active host callback scheme", () => {
    expect(
      surfaceLink(
        "a1b2c3d4-e5f6-4789-8abc-def012345678",
        "surface-1",
        "cmux-nightly",
      ),
    ).toBe(
      "cmux-nightly://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678/surface/surface-1",
    );
  });

  test("paneLink matches the canonical same-session pane route", () => {
    expect(
      paneLink(
        "a1b2c3d4-e5f6-4789-8abc-def012345678",
        "b1c2d3e4-f5a6-4789-8abc-def012345678",
      ),
    ).toBe(
      "cmux://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678/pane/b1c2d3e4-f5a6-4789-8abc-def012345678",
    );
  });

  test("paneLink can use the active host callback scheme", () => {
    expect(
      paneLink(
        "a1b2c3d4-e5f6-4789-8abc-def012345678",
        "b1c2d3e4-f5a6-4789-8abc-def012345678",
        "cmux-dev",
      ),
    ).toBe(
      "cmux-dev://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678/pane/b1c2d3e4-f5a6-4789-8abc-def012345678",
    );
  });
});
