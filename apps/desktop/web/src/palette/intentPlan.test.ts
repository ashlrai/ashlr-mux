import { describe, expect, test } from "bun:test";

import { planIntent, type IntentPlanContext } from "./intentPlan";

const BASE: IntentPlanContext = {
  selectedWorkspaceIndex: 0,
  workspaceCount: 3,
  activePanelId: "surface-1",
  selectedWorkspaceId: "a1b2c3d4-e5f6-4789-8abc-def012345678",
};

describe("planIntent", () => {
  test("newWorkspace plans unconditionally", () => {
    expect(planIntent("newWorkspace", BASE)).toEqual({ type: "newWorkspace" });
    expect(
      planIntent("newWorkspace", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "newWorkspace" });
  });

  test("closeWorkspace targets the selected index; no-op without workspaces", () => {
    expect(planIntent("closeWorkspace", { ...BASE, selectedWorkspaceIndex: 2 })).toEqual({
      type: "closeWorkspace",
      index: 2,
    });
    expect(
      planIntent("closeWorkspace", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
  });

  test("nextWorkspace wraps at the end (TabManager.swift:3454 parity)", () => {
    expect(planIntent("nextWorkspace", { ...BASE, selectedWorkspaceIndex: 1 })).toEqual({
      type: "selectWorkspace",
      index: 2,
    });
    expect(planIntent("nextWorkspace", { ...BASE, selectedWorkspaceIndex: 2 })).toEqual({
      type: "selectWorkspace",
      index: 0,
    });
  });

  test("previousWorkspace wraps at the start (TabManager.swift:3474 parity)", () => {
    expect(
      planIntent("previousWorkspace", { ...BASE, selectedWorkspaceIndex: 0 }),
    ).toEqual({ type: "selectWorkspace", index: 2 });
    expect(
      planIntent("previousWorkspace", { ...BASE, selectedWorkspaceIndex: 2 }),
    ).toEqual({ type: "selectWorkspace", index: 1 });
  });

  test("next/previous are no-ops with no workspaces or an invalid selection", () => {
    expect(
      planIntent("nextWorkspace", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
    expect(
      planIntent("previousWorkspace", { selectedWorkspaceIndex: 5, workspaceCount: 3 }),
    ).toEqual({ type: "none" });
  });

  test("split intents map right→horizontal-second, down→vertical-second", () => {
    expect(planIntent("terminalSplitRight", BASE)).toEqual({
      type: "split",
      panelId: "surface-1",
      orientation: "horizontal",
      insertFirst: false,
    });
    expect(planIntent("terminalSplitDown", BASE)).toEqual({
      type: "split",
      panelId: "surface-1",
      orientation: "vertical",
      insertFirst: false,
    });
  });

  test("split intents are no-ops without a target pane", () => {
    const noPane = { ...BASE, activePanelId: undefined };
    expect(planIntent("terminalSplitRight", noPane)).toEqual({ type: "none" });
    expect(planIntent("terminalSplitDown", noPane)).toEqual({ type: "none" });
  });

  test("equalizeSplits and toggleSidebar plan directly", () => {
    expect(planIntent("equalizeSplits", BASE)).toEqual({ type: "equalizeDividers" });
    expect(planIntent("toggleSidebar", BASE)).toEqual({ type: "toggleSidebar" });
  });

  test("copyWorkspaceID copies the canonical single line, id verbatim", () => {
    // Exact canonical shape (WorkspaceSurfaceIdentifierClipboardText.swift:85-92):
    // one line, no trailing newline, lowercase port id preserved as-is.
    expect(planIntent("copyWorkspaceID", BASE)).toEqual({
      type: "copyText",
      text: "workspace_id=a1b2c3d4-e5f6-4789-8abc-def012345678",
    });
  });

  test("copyWorkspaceID is a no-op without a workspace id or workspaces", () => {
    expect(
      planIntent("copyWorkspaceID", { ...BASE, selectedWorkspaceId: undefined }),
    ).toEqual({ type: "none" });
    expect(planIntent("copyWorkspaceID", { ...BASE, workspaceCount: 0 })).toEqual({
      type: "none",
    });
  });

  test("copySurfaceID copies the focused panel id as the surface id", () => {
    // Port panel id ≡ canonical surface id
    // (ContentViewIdentifierCopyCommands.swift:123, clipboard text :36-43).
    expect(planIntent("copySurfaceID", BASE)).toEqual({
      type: "copyText",
      text: "surface_id=surface-1",
    });
    expect(planIntent("copySurfaceID", { ...BASE, activePanelId: undefined })).toEqual({
      type: "none",
    });
  });

  test("ref/pane/window copy kinds stay unhandled (no port-side source)", () => {
    expect(planIntent("copyWorkspaceIDAndRef", BASE)).toEqual({ type: "unhandled" });
    expect(planIntent("copyPaneID", BASE)).toEqual({ type: "unhandled" });
    expect(planIntent("copyIdentifiers", BASE)).toEqual({ type: "unhandled" });
    expect(planIntent("copySurfaceLink", BASE)).toEqual({ type: "unhandled" });
    expect(planIntent("toggleFullScreen", BASE)).toEqual({ type: "unhandled" });
    expect(planIntent("newWindow", BASE)).toEqual({ type: "unhandled" });
  });

  test("unmapped kinds report unhandled", () => {
    expect(planIntent("openSettings", BASE)).toEqual({ type: "unhandled" });
    expect(planIntent("browserReload", BASE)).toEqual({ type: "unhandled" });
  });
});
