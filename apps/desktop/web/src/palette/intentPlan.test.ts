import { describe, expect, test } from "bun:test";

import { hasExecutablePlan, planIntent, type IntentPlanContext } from "./intentPlan";

const BASE: IntentPlanContext = {
  selectedWorkspaceIndex: 0,
  workspaceCount: 3,
  activePanelId: "surface-1",
  activeSurfaceRef: "surface:1",
  activePanelTitle: "api logs",
  activePanelIsPinned: false,
  activePanelHasUnread: false,
  activePaneId: "b1c2d3e4-f5a6-4789-8abc-def012345678",
  activePaneRef: "pane:1",
  adjacentPanelIds: {
    left: "surface-left",
    right: "surface-right",
    up: "surface-up",
    down: "surface-down",
  },
  selectedWorkspaceId: "a1b2c3d4-e5f6-4789-8abc-def012345678",
  selectedWorkspaceTitle: "Phoenix",
  selectedWorkspaceDescription: "Investigate auth flow",
  selectedWorkspaceIsUnread: false,
};

describe("planIntent", () => {
  test("hasExecutablePlan rejects only unresolved parity gaps", () => {
    expect(hasExecutablePlan({ type: "unhandled" })).toBe(false);
    expect(hasExecutablePlan({ type: "none" })).toBe(true);
    expect(hasExecutablePlan({ type: "newWorkspace" })).toBe(true);
  });

  test("newWorkspace plans unconditionally", () => {
    expect(planIntent("newWorkspace", BASE)).toEqual({ type: "newWorkspace" });
    expect(planIntent("newTerminalTab", BASE)).toEqual({ type: "newWorkspace" });
    expect(
      planIntent("newWorkspace", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "newWorkspace" });
  });

  test("warmClaudeCode targets the selected workspace directory", () => {
    expect(
      planIntent("warmClaudeCode", {
        ...BASE,
        selectedWorkspaceCurrentDirectory: "C:\\work\\phoenix",
      }),
    ).toEqual({
      type: "warmClaudeCode",
      currentDirectory: "C:\\work\\phoenix",
    });
    expect(
      planIntent("warmClaudeCode", {
        selectedWorkspaceIndex: 0,
        workspaceCount: 0,
      }),
    ).toEqual({ type: "none" });
  });

  test("closeWorkspace targets the selected index; no-op without workspaces", () => {
    expect(planIntent("closeWorkspace", { ...BASE, selectedWorkspaceIndex: 2 })).toEqual({
      type: "closeWorkspace",
      index: 2,
    });
    expect(planIntent("closeTab", { ...BASE, selectedWorkspaceIndex: 1 })).toEqual({
      type: "closeWorkspace",
      index: 1,
    });
    expect(
      planIntent("closeWorkspace", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
    expect(planIntent("closeTab", { selectedWorkspaceIndex: 0, workspaceCount: 0 })).toEqual({
      type: "none",
    });
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

  test("jumpUnread is handled when a workspace is selected", () => {
    expect(planIntent("jumpUnread", BASE)).toEqual({ type: "jumpUnread" });
    expect(
      planIntent("jumpUnread", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
    expect(planIntent("markOldestUnreadAndJumpNext", BASE)).toEqual({
      type: "markOldestUnreadAndJumpNext",
    });
    expect(
      planIntent("markOldestUnreadAndJumpNext", {
        selectedWorkspaceIndex: 0,
        workspaceCount: 0,
      }),
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

  test("fork conversation intents launch the restorable agent fork command", () => {
    const ctx = {
      ...BASE,
      activeForkCommand: "codex fork session-1",
      activeForkWorkingDirectory: "C:/repo",
    };
    expect(planIntent("forkAgentConversationRight", ctx)).toEqual({
      type: "split",
      panelId: "surface-1",
      orientation: "horizontal",
      insertFirst: false,
      initialTerminalInput: "codex fork session-1\r\n",
      currentDirectory: "C:/repo",
    });
    expect(planIntent("forkAgentConversationTop", ctx)).toEqual({
      type: "split",
      panelId: "surface-1",
      orientation: "vertical",
      insertFirst: true,
      initialTerminalInput: "codex fork session-1\r\n",
      currentDirectory: "C:/repo",
    });
    expect(planIntent("forkAgentConversationNewWorkspace", ctx)).toEqual({
      type: "forkAgentNewWorkspace",
      initialTerminalInput: "codex fork session-1\r\n",
      currentDirectory: "C:/repo",
    });
    expect(planIntent("forkAgentConversationNewTab", ctx)).toEqual({
      type: "forkAgentNewTab",
      panelId: "surface-1",
      initialTerminalInput: "codex fork session-1\r\n",
    });
    expect(planIntent("forkAgentConversationRight", BASE)).toEqual({ type: "none" });
  });

  test("diff viewer intents switch the active pane to the diff surface", () => {
    expect(planIntent("openWorkspacePullRequests", BASE)).toEqual({
      type: "openWorkspacePullRequests",
    });
    expect(
      planIntent("openWorkspacePullRequests", {
        selectedWorkspaceIndex: 0,
        workspaceCount: 0,
      }),
    ).toEqual({ type: "none" });
    expect(planIntent("openDiffViewer", BASE)).toEqual({
      type: "setSurfaceKind",
      panelId: "surface-1",
      kind: "diff",
    });
    expect(planIntent("openDirectoryDiffViewer", BASE)).toEqual({
      type: "setSurfaceKind",
      panelId: "surface-1",
      kind: "diff",
    });
    expect(planIntent("openDiffViewer", { ...BASE, activePanelId: undefined })).toEqual({
      type: "none",
    });
  });

  test("markdown zoom intents target the active panel", () => {
    expect(planIntent("markdownZoomIn", BASE)).toEqual({
      type: "markdownZoomIn",
      panelId: "surface-1",
    });
    expect(planIntent("markdownZoomOut", BASE)).toEqual({
      type: "markdownZoomOut",
      panelId: "surface-1",
    });
    expect(planIntent("markdownZoomReset", BASE)).toEqual({
      type: "markdownZoomReset",
      panelId: "surface-1",
    });
    expect(planIntent("markdownZoomIn", { ...BASE, activePanelId: undefined })).toEqual({
      type: "none",
    });
  });

  test("equalizeSplits, toggleSidebar, and openSettings plan directly", () => {
    expect(planIntent("equalizeSplits", BASE)).toEqual({ type: "equalizeDividers" });
    expect(planIntent("toggleCanvasLayout", BASE)).toEqual({
      type: "setLayoutMode",
      mode: "canvas",
    });
    expect(
      planIntent("toggleCanvasLayout", {
        ...BASE,
        selectedWorkspaceLayoutMode: "canvas",
      }),
    ).toEqual({ type: "setLayoutMode", mode: null });
    expect(
      planIntent("toggleCanvasLayout", {
        selectedWorkspaceIndex: 0,
        workspaceCount: 0,
      }),
    ).toEqual({ type: "none" });
    expect(planIntent("openFolder", BASE)).toEqual({ type: "openFolder" });
    expect(planIntent("openFolderInVSCodeInline", BASE)).toEqual({
      type: "openFolderInVSCodeInline",
    });
    expect(planIntent("vscodeServeWebStop", BASE)).toEqual({
      type: "vscodeServeWebStop",
    });
    expect(planIntent("vscodeServeWebRestart", BASE)).toEqual({
      type: "vscodeServeWebRestart",
    });
    expect(planIntent("installCLI", BASE)).toEqual({ type: "installCLI" });
    expect(planIntent("uninstallCLI", BASE)).toEqual({ type: "uninstallCLI" });
    expect(planIntent("makeDefaultTerminal", BASE)).toEqual({
      type: "makeDefaultTerminal",
    });
    expect(planIntent("reopenPreviousSession", BASE)).toEqual({
      type: "restorePreviousLaunch",
    });
    expect(planIntent("reopenClosedBrowserTab", BASE)).toEqual({
      type: "reopenClosedBrowserTab",
    });
    expect(planIntent("newWindow", BASE)).toEqual({ type: "newWindow" });
    expect(planIntent("toggleSidebar", BASE)).toEqual({ type: "toggleSidebar" });
    expect(planIntent("toggleFileExplorer", BASE)).toEqual({
      type: "toggleFileExplorer",
    });
    expect(
      planIntent("toggleFileExplorer", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
    expect(planIntent("openSettings", BASE)).toEqual({ type: "openSettings" });
    expect(planIntent("mobileConnect", BASE)).toEqual({
      type: "openSettings",
      section: "mobile",
    });
    expect(planIntent("authSignIn", BASE)).toEqual({
      type: "openSettings",
      section: "account",
    });
    expect(planIntent("authSignOut", BASE)).toEqual({
      type: "openSettings",
      section: "account",
    });
    expect(planIntent("checkForUpdates", BASE)).toEqual({
      type: "openSettings",
      section: "updates",
    });
    expect(planIntent("attemptUpdate", BASE)).toEqual({
      type: "openSettings",
      section: "updates",
    });
    expect(planIntent("applyUpdateIfAvailable", BASE)).toEqual({
      type: "openSettings",
      section: "updates",
    });
    expect(planIntent("showNotifications", BASE)).toEqual({ type: "showNotifications" });
    expect(planIntent("findInDirectory", BASE)).toEqual({
      type: "openFindInDirectory",
    });
    expect(
      planIntent("findInDirectory", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
    expect(planIntent("openCmuxSettingsFile", BASE)).toEqual({
      type: "openCmuxSettingsFile",
    });
    expect(planIntent("openGhosttySettings", BASE)).toEqual({
      type: "openGhosttySettingsFile",
    });
    expect(planIntent("closeWindow", BASE)).toEqual({ type: "closeWindow" });
    expect(planIntent("toggleFullScreen", BASE)).toEqual({ type: "toggleFullScreen" });
    expect(planIntent("openTaskManager", BASE)).toEqual({ type: "openTaskManager" });
    expect(planIntent("restartSocketListener", BASE)).toEqual({
      type: "restartControlSocketListener",
    });
  });

  test("browser enablement intents route through settings actions", () => {
    expect(planIntent("disableBrowser", BASE)).toEqual({
      type: "setBrowserEnabled",
      enabled: false,
    });
    expect(planIntent("enableBrowser", BASE)).toEqual({
      type: "setBrowserEnabled",
      enabled: true,
    });
  });

  test("global settings intents route through settings actions", () => {
    expect(planIntent("enableMinimalMode", BASE)).toEqual({
      type: "setMinimalMode",
      enabled: true,
    });
    expect(planIntent("disableMinimalMode", BASE)).toEqual({
      type: "setMinimalMode",
      enabled: false,
    });
    expect(planIntent("toggleMatchTerminalBackground", BASE)).toEqual({
      type: "toggleMatchTerminalBackground",
    });
  });

  test("copyWorkspaceID copies the canonical single line, id verbatim", () => {
    // Exact canonical shape (WorkspaceSurfaceIdentifierClipboardText.swift:85-92):
    // one line, no trailing newline, lowercase port id preserved as-is.
    expect(planIntent("copyWorkspaceID", BASE)).toEqual({
      type: "copyText",
      text: "workspace_id=a1b2c3d4-e5f6-4789-8abc-def012345678",
    });
  });

  test("copyWorkspaceLink copies the canonical same-session deep link", () => {
    expect(planIntent("copyWorkspaceLink", BASE)).toEqual({
      type: "copyText",
      text: "cmux://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678",
    });
  });

  test("copy link intents use the host-provided active callback scheme", () => {
    const ctx = { ...BASE, navigationScheme: "cmux-dev-feature" };
    expect(planIntent("copyWorkspaceLink", ctx)).toEqual({
      type: "copyText",
      text: "cmux-dev-feature://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678",
    });
    expect(planIntent("copyPaneLink", ctx)).toEqual({
      type: "copyText",
      text: "cmux-dev-feature://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678/pane/b1c2d3e4-f5a6-4789-8abc-def012345678",
    });
    expect(planIntent("copySurfaceLink", ctx)).toEqual({
      type: "copyText",
      text: "cmux-dev-feature://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678/surface/surface-1",
    });
  });

  test("copyWorkspaceID is a no-op without a workspace id or workspaces", () => {
    expect(
      planIntent("copyWorkspaceID", { ...BASE, selectedWorkspaceId: undefined }),
    ).toEqual({ type: "none" });
    expect(planIntent("copyWorkspaceID", { ...BASE, workspaceCount: 0 })).toEqual({
      type: "none",
    });
    expect(planIntent("copyWorkspaceLink", { ...BASE, selectedWorkspaceId: undefined })).toEqual({
      type: "none",
    });
  });

  test("copyPaneID copies the focused pane id", () => {
    expect(planIntent("copyPaneID", BASE)).toEqual({
      type: "copyText",
      text: "pane_id=b1c2d3e4-f5a6-4789-8abc-def012345678",
    });
    expect(planIntent("copyPaneID", { ...BASE, activePaneId: undefined })).toEqual({
      type: "none",
    });
  });

  test("copyPaneLink copies the canonical same-session deep link", () => {
    expect(planIntent("copyPaneLink", BASE)).toEqual({
      type: "copyText",
      text: "cmux://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678/pane/b1c2d3e4-f5a6-4789-8abc-def012345678",
    });
    expect(planIntent("copyPaneLink", { ...BASE, activePaneId: undefined })).toEqual({
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

  test("copySurfaceLink copies the canonical same-session deep link", () => {
    expect(planIntent("copySurfaceLink", BASE)).toEqual({
      type: "copyText",
      text: "cmux://workspace/a1b2c3d4-e5f6-4789-8abc-def012345678/surface/surface-1",
    });
    expect(planIntent("copySurfaceLink", { ...BASE, activePanelId: undefined })).toEqual({
      type: "none",
    });
  });

  test("copyIdentifiers copies the canonical workspace/pane/surface block", () => {
    const withoutRefs = { ...BASE };
    delete withoutRefs.activePaneRef;
    delete withoutRefs.activeSurfaceRef;

    expect(planIntent("copyIdentifiers", BASE)).toEqual({
      type: "copyText",
      text:
        "workspace_id=a1b2c3d4-e5f6-4789-8abc-def012345678\n" +
        "workspace_ref=workspace:1\n" +
        "pane_id=b1c2d3e4-f5a6-4789-8abc-def012345678\n" +
        "pane_ref=pane:1\n" +
        "surface_id=surface-1\n" +
        "surface_ref=surface:1",
    });
    expect(planIntent("copyIdentifiers", withoutRefs)).toEqual({
      type: "copyText",
      text:
        "workspace_id=a1b2c3d4-e5f6-4789-8abc-def012345678\n" +
        "workspace_ref=workspace:1\n" +
        "pane_id=b1c2d3e4-f5a6-4789-8abc-def012345678\n" +
        "surface_id=surface-1",
    });
    expect(planIntent("copyIdentifiers", { ...BASE, activePanelId: undefined })).toEqual({
      type: "none",
    });
  });

  test("toggleWorkspacePin plans the selected workspace's inverted flag", () => {
    // WorkspaceActionDispatcher.swift:75 parity: pinned = !anchorWorkspace.isPinned.
    expect(
      planIntent("toggleWorkspacePin", { ...BASE, selectedWorkspaceIsPinned: false }),
    ).toEqual({ type: "setWorkspacePinned", index: 0, pinned: true });
    expect(
      planIntent("toggleWorkspacePin", {
        ...BASE,
        selectedWorkspaceIndex: 2,
        selectedWorkspaceIsPinned: true,
      }),
    ).toEqual({ type: "setWorkspacePinned", index: 2, pinned: false });
  });

  test("toggleWorkspacePin treats an absent pin flag as unpinned", () => {
    // Snapshot is_pinned is Some(true)|None (A7 golden-stability): undefined
    // MUST plan pinned: true, not false.
    expect(planIntent("toggleWorkspacePin", BASE)).toEqual({
      type: "setWorkspacePinned",
      index: 0,
      pinned: true,
    });
  });

  test("toggleWorkspacePin is a no-op without workspaces or a valid selection", () => {
    expect(
      planIntent("toggleWorkspacePin", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
    expect(
      planIntent("toggleWorkspacePin", { selectedWorkspaceIndex: 5, workspaceCount: 3 }),
    ).toEqual({ type: "none" });
  });

  test("workspace read/unread intents target the selected workspace", () => {
    expect(planIntent("markWorkspaceRead", BASE)).toEqual({
      type: "setWorkspaceUnread",
      index: 0,
      unread: false,
      preferredPanelId: "surface-1",
    });
    expect(planIntent("markWorkspaceUnread", { ...BASE, selectedWorkspaceIndex: 2 })).toEqual({
      type: "setWorkspaceUnread",
      index: 2,
      unread: true,
      preferredPanelId: "surface-1",
    });
    expect(planIntent("toggleUnread", BASE)).toEqual({
      type: "setWorkspaceUnread",
      index: 0,
      unread: true,
      preferredPanelId: "surface-1",
    });
    expect(planIntent("toggleUnread", { ...BASE, selectedWorkspaceIsUnread: true })).toEqual({
      type: "setWorkspaceUnread",
      index: 0,
      unread: false,
      preferredPanelId: "surface-1",
    });
  });

  test("workspace read/unread intents are no-ops without a workspace", () => {
    expect(
      planIntent("markWorkspaceRead", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
    expect(
      planIntent("markWorkspaceUnread", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
    expect(planIntent("toggleUnread", { selectedWorkspaceIndex: 0, workspaceCount: 0 })).toEqual({
      type: "none",
    });
  });

  test("workspace batch-close intents target original indices", () => {
    expect(
      planIntent("closeOtherWorkspaces", { ...BASE, selectedWorkspaceIndex: 1, workspaceCount: 4 }),
    ).toEqual({ type: "closeWorkspaces", indexes: [0, 2, 3] });
    expect(
      planIntent("closeWorkspacesAbove", { ...BASE, selectedWorkspaceIndex: 2, workspaceCount: 4 }),
    ).toEqual({ type: "closeWorkspaces", indexes: [0, 1] });
    expect(
      planIntent("closeWorkspacesBelow", { ...BASE, selectedWorkspaceIndex: 1, workspaceCount: 4 }),
    ).toEqual({ type: "closeWorkspaces", indexes: [2, 3] });
  });

  test("workspace batch-close intents are no-ops without closable peers", () => {
    expect(planIntent("closeOtherWorkspaces", BASE)).toEqual({
      type: "closeWorkspaces",
      indexes: [1, 2],
    });
    expect(planIntent("closeWorkspacesAbove", BASE)).toEqual({ type: "none" });
    expect(
      planIntent("closeWorkspacesBelow", { ...BASE, selectedWorkspaceIndex: 2 }),
    ).toEqual({ type: "none" });
    expect(
      planIntent("closeOtherWorkspaces", { selectedWorkspaceIndex: 0, workspaceCount: 1 }),
    ).toEqual({ type: "none" });
  });

  test("workspace group collapse intents target the selected workspace group", () => {
    expect(
      planIntent("collapseWorkspaceGroup", {
        ...BASE,
        selectedWorkspaceGroupId: "group-1",
        selectedWorkspaceGroupIsCollapsed: false,
      }),
    ).toEqual({
      type: "setWorkspaceGroupCollapsed",
      groupId: "group-1",
      collapsed: true,
    });
    expect(
      planIntent("expandWorkspaceGroup", {
        ...BASE,
        selectedWorkspaceGroupId: "group-1",
        selectedWorkspaceGroupIsCollapsed: true,
      }),
    ).toEqual({
      type: "setWorkspaceGroupCollapsed",
      groupId: "group-1",
      collapsed: false,
    });
    expect(planIntent("collapseWorkspaceGroup", BASE)).toEqual({ type: "none" });
    expect(
      planIntent("expandWorkspaceGroup", {
        ...BASE,
        selectedWorkspaceGroupId: "group-1",
        selectedWorkspaceGroupIsCollapsed: false,
      }),
    ).toEqual({ type: "none" });
  });

  test("workspace move intents map to reorder commands", () => {
    expect(planIntent("moveWorkspaceUp", { ...BASE, selectedWorkspaceIndex: 2 })).toEqual({
      type: "reorderWorkspace",
      index: 2,
      toIndex: 1,
    });
    expect(planIntent("moveWorkspaceDown", { ...BASE, selectedWorkspaceIndex: 1 })).toEqual({
      type: "reorderWorkspace",
      index: 1,
      toIndex: 2,
    });
    expect(planIntent("moveWorkspaceToTop", { ...BASE, selectedWorkspaceIndex: 2 })).toEqual({
      type: "reorderWorkspace",
      index: 2,
      toIndex: 0,
    });
  });

  test("workspace move intents are no-ops at the boundaries or without workspaces", () => {
    expect(planIntent("moveWorkspaceUp", BASE)).toEqual({ type: "none" });
    expect(
      planIntent("moveWorkspaceDown", { ...BASE, selectedWorkspaceIndex: 2 }),
    ).toEqual({ type: "none" });
    expect(planIntent("moveWorkspaceToTop", BASE)).toEqual({ type: "none" });
    expect(
      planIntent("moveWorkspaceToTop", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
  });

  test("clearWorkspaceName plans rename-to-empty (TabManager.swift:1700-1702)", () => {
    expect(planIntent("clearWorkspaceName", { ...BASE, selectedWorkspaceIndex: 1 })).toEqual({
      type: "renameWorkspace",
      index: 1,
      title: "",
    });
    expect(
      planIntent("clearWorkspaceName", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
  });

  test("renameWorkspace and editWorkspaceDescription start edit flows", () => {
    expect(planIntent("renameWorkspace", BASE)).toEqual({
      type: "beginRenameWorkspace",
      workspaceId: "a1b2c3d4-e5f6-4789-8abc-def012345678",
      title: "Phoenix",
    });
    expect(planIntent("editWorkspaceDescription", BASE)).toEqual({
      type: "beginWorkspaceDescriptionEdit",
      workspaceId: "a1b2c3d4-e5f6-4789-8abc-def012345678",
      title: "Phoenix",
      description: "Investigate auth flow",
    });
  });

  test("clearWorkspaceDescription plans set-to-empty", () => {
    expect(planIntent("clearWorkspaceDescription", { ...BASE, selectedWorkspaceIndex: 1 })).toEqual({
      type: "setWorkspaceDescription",
      index: 1,
      description: "",
    });
    expect(
      planIntent("clearWorkspaceDescription", {
        selectedWorkspaceIndex: 0,
        workspaceCount: 0,
      }),
    ).toEqual({ type: "none" });
  });

  test("resetWorkspaceColor targets the selected workspace", () => {
    expect(planIntent("resetWorkspaceColor", { ...BASE, selectedWorkspaceIndex: 1 })).toEqual({
      type: "resetWorkspaceColor",
      index: 1,
    });
    expect(
      planIntent("resetWorkspaceColor", { selectedWorkspaceIndex: 0, workspaceCount: 0 }),
    ).toEqual({ type: "none" });
  });

  test("renameTab and clearTabName target the active panel", () => {
    expect(planIntent("renameTab", BASE)).toEqual({
      type: "beginRenameTab",
      panelId: "surface-1",
      title: "api logs",
    });
    expect(planIntent("clearTabName", BASE)).toEqual({
      type: "renameTab",
      panelId: "surface-1",
      title: "",
    });
    expect(planIntent("toggleTabPin", BASE)).toEqual({
      type: "setPanelPinned",
      panelId: "surface-1",
      pinned: true,
    });
    expect(planIntent("toggleTabPin", { ...BASE, activePanelIsPinned: true })).toEqual({
      type: "setPanelPinned",
      panelId: "surface-1",
      pinned: false,
    });
    expect(planIntent("toggleTabUnread", BASE)).toEqual({
      type: "setPanelUnread",
      panelId: "surface-1",
      unread: true,
    });
    expect(planIntent("toggleTabUnread", { ...BASE, activePanelHasUnread: true })).toEqual({
      type: "setPanelUnread",
      panelId: "surface-1",
      unread: false,
    });
    expect(planIntent("renameTab", { selectedWorkspaceIndex: 0, workspaceCount: 1 })).toEqual({
      type: "none",
    });
    expect(planIntent("toggleTabPin", { selectedWorkspaceIndex: 0, workspaceCount: 1 })).toEqual({
      type: "none",
    });
    expect(
      planIntent("toggleTabUnread", { selectedWorkspaceIndex: 0, workspaceCount: 1 }),
    ).toEqual({
      type: "none",
    });
  });

  test("copyWorkspaceIDAndRef copies the workspace id plus control ref", () => {
    expect(planIntent("copyWorkspaceIDAndRef", BASE)).toEqual({
      type: "copyText",
      text:
        "workspace_id=a1b2c3d4-e5f6-4789-8abc-def012345678\n" +
        "workspace_ref=workspace:1",
    });
    expect(
      planIntent("copyWorkspaceIDAndRef", {
        ...BASE,
        selectedWorkspaceIndex: 2,
      }),
    ).toEqual({
      type: "copyText",
      text:
        "workspace_id=a1b2c3d4-e5f6-4789-8abc-def012345678\n" +
        "workspace_ref=workspace:3",
    });
    expect(
      planIntent("copyWorkspaceIDAndRef", {
        ...BASE,
        selectedWorkspaceId: undefined,
      }),
    ).toEqual({ type: "none" });
  });

  test("moveTabToNewWorkspace targets the active panel", () => {
    expect(planIntent("moveTabToNewWorkspace", BASE)).toEqual({
      type: "movePanelToNewWorkspace",
      panelId: "surface-1",
    });
    expect(
      planIntent("moveTabToNewWorkspace", {
        selectedWorkspaceIndex: 0,
        workspaceCount: 1,
      }),
    ).toEqual({
      type: "none",
    });
  });

  test("browser commands target the active pane", () => {
    expect(planIntent("browserReload", BASE)).toEqual({
      type: "browserCommand",
      panelId: "surface-1",
      command: "reload",
    });
    expect(planIntent("browserFocusAddressBar", BASE)).toEqual({
      type: "browserCommand",
      panelId: "surface-1",
      command: "focusAddressBar",
    });
    expect(planIntent("browserToggleOmnibar", BASE)).toEqual({
      type: "toggleBrowserOmnibar",
      panelId: "surface-1",
    });
    expect(planIntent("browserFocusMode", BASE)).toEqual({
      type: "toggleBrowserFocusMode",
      panelId: "surface-1",
    });
    expect(planIntent("browserToggleDevTools", BASE)).toEqual({
      type: "toggleBrowserDeveloperTools",
      panelId: "surface-1",
    });
    expect(planIntent("browserConsole", BASE)).toEqual({
      type: "showBrowserDeveloperTools",
      panelId: "surface-1",
      panel: "console",
    });
    expect(planIntent("browserReactGrab", BASE)).toEqual({
      type: "showBrowserDeveloperTools",
      panelId: "surface-1",
      panel: "react",
    });
    expect(planIntent("browserNetwork", BASE)).toEqual({
      type: "showBrowserDeveloperTools",
      panelId: "surface-1",
      panel: "network",
    });
    expect(planIntent("browserNetworkClear", BASE)).toEqual({
      type: "clearBrowserNetworkRecords",
      panelId: "surface-1",
    });
    expect(planIntent("browserClearHistory", BASE)).toEqual({
      type: "clearBrowserHistory",
      panelId: "surface-1",
    });
  });

  test("browser creation and split intents route through browser plans", () => {
    expect(planIntent("newBrowserWorkspace", BASE)).toEqual({
      type: "newBrowserWorkspace",
    });
    expect(planIntent("newBrowserTab", BASE)).toEqual({
      type: "openBrowser",
      panelId: "surface-1",
    });
    expect(planIntent("terminalSplitBrowserRight", BASE)).toEqual({
      type: "splitBrowser",
      panelId: "surface-1",
      orientation: "horizontal",
      insertFirst: false,
    });
  });

  test("terminal direct commands target the active pane", () => {
    expect(planIntent("terminalFind", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "find",
    });
    expect(planIntent("terminalFindNext", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "findNext",
    });
    expect(planIntent("terminalFindPrevious", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "findPrevious",
    });
    expect(planIntent("terminalHideFind", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "hideFind",
    });
    expect(planIntent("terminalUseSelectionForFind", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "useSelectionForFind",
    });
    expect(planIntent("terminalToggleTextBoxInput", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "toggleTextBoxInput",
    });
    expect(planIntent("terminalFocusTextBoxInput", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "focusTextBoxInput",
    });
    expect(planIntent("terminalAttachTextBoxFile", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "attachTextBoxFile",
    });
    expect(planIntent("terminalSendCtrlF", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "sendCtrlF",
    });
    expect(planIntent("terminalClearScreenKeepScrollback", BASE)).toEqual({
      type: "terminalCommand",
      panelId: "surface-1",
      command: "clearScreenKeepScrollback",
    });
    expect(
      planIntent("terminalSendCtrlF", { ...BASE, activePanelId: undefined }),
    ).toEqual({ type: "none" });
  });

  test("pane tab navigation targets the active pane", () => {
    expect(planIntent("nextTabInPane", BASE)).toEqual({
      type: "selectAdjacentPanel",
      panelId: "surface-1",
      next: true,
    });
    expect(planIntent("previousTabInPane", BASE)).toEqual({
      type: "selectAdjacentPanel",
      panelId: "surface-1",
      next: false,
    });
    expect(planIntent("nextTabInPane", { ...BASE, activePanelId: undefined })).toEqual({
      type: "none",
    });
  });

  test("directional focus targets adjacent pane geometry results", () => {
    expect(planIntent("focusLeft", BASE)).toEqual({
      type: "focusPanel",
      panelId: "surface-left",
    });
    expect(planIntent("focusRight", BASE)).toEqual({
      type: "focusPanel",
      panelId: "surface-right",
    });
    expect(planIntent("focusUp", BASE)).toEqual({
      type: "focusPanel",
      panelId: "surface-up",
    });
    expect(planIntent("focusDown", BASE)).toEqual({
      type: "focusPanel",
      panelId: "surface-down",
    });
  });

  test("directional focus is a no-op when there is no pane in that direction", () => {
    expect(planIntent("focusLeft", { ...BASE, adjacentPanelIds: {} })).toEqual({
      type: "none",
    });
  });

  test("split zoom targets the active pane", () => {
    expect(planIntent("toggleSplitZoom", BASE)).toEqual({
      type: "toggleSplitZoom",
      panelId: "surface-1",
    });
    expect(planIntent("toggleSplitZoom", { ...BASE, activePanelId: undefined })).toEqual({
      type: "none",
    });
  });

  test("triggerFlash targets the active pane", () => {
    expect(planIntent("triggerFlash", BASE)).toEqual({
      type: "triggerPanelFlash",
      panelId: "surface-1",
    });
    expect(planIntent("triggerFlash", { ...BASE, activePanelId: undefined })).toEqual({
      type: "none",
    });
  });
});
