// Oracle tests for commandCatalog.ts, pinned to the macOS host build loop
// (`Sources/ContentView.swift:6010-6052`) and the contribution rows
// (`Sources/ContentView.swift:6321-7464`, `ContentView+ViewCommandPalette.swift`,
// `ContentViewIdentifierCopyCommands.swift`, `ContentView+MoveTabToNewWorkspace.swift`,
// `ContentView+AuthCommandPalette.swift`). Expectations are hand-derived from
// the Swift sources cited inline.

import { describe, expect, test } from "bun:test";

import {
  buildCommandCatalog,
  dispatchCommand,
  resolveIntent,
  searchableTexts,
  type CommandContext,
  type CommandContribution,
  type CommandDescriptor,
} from "./commandCatalog";

/** A context in which every gate is false (Swift `snapshot.bool` default). */
const EMPTY_CTX: CommandContext = {};

function byId(catalog: CommandDescriptor[], id: string): CommandDescriptor | undefined {
  return catalog.find((d) => d.id === id);
}

function ids(catalog: CommandDescriptor[]): string[] {
  return catalog.map((d) => d.id);
}

describe("searchableTexts — CommandPaletteCommand.swift:49-51", () => {
  test("is [title, subtitle, ...keywords]", () => {
    // palette.newWorkspace row (ContentView.swift:6323-6329).
    const catalog = buildCommandCatalog(EMPTY_CTX);
    const row = byId(catalog, "palette.newWorkspace");
    expect(row).toBeDefined();
    expect(searchableTexts(row!)).toEqual([
      "New Workspace",
      "Workspace",
      "create",
      "new",
      "workspace",
    ]);
  });
});

describe("when-gate filtering + rank compaction (ContentView.swift:6020/6048)", () => {
  test("ranks are contiguous 0..n-1 in declared order", () => {
    const catalog = buildCommandCatalog(EMPTY_CTX);
    catalog.forEach((d, index) => {
      expect(d.rank).toBe(index);
    });
  });

  test("browserDisabled drops browser workspace/tab rows and keeps ranks contiguous", () => {
    const enabled = buildCommandCatalog({});
    const disabled = buildCommandCatalog({ browserDisabled: true });

    expect(ids(enabled)).toContain("palette.newBrowserWorkspace");
    expect(ids(enabled)).toContain("palette.newBrowserTab");
    expect(ids(disabled)).not.toContain("palette.newBrowserWorkspace");
    expect(ids(disabled)).not.toContain("palette.newBrowserTab");

    // rank is the post-filter index: still contiguous after dropping rows.
    disabled.forEach((d, index) => expect(d.rank).toBe(index));
    // Dropping rows makes the catalog strictly smaller.
    expect(disabled.length).toBeLessThan(enabled.length);
  });
});

describe("mutually-exclusive pairs", () => {
  test("cliInstalledInPATH toggles install/uninstall (ContentView.swift:6354/6363)", () => {
    const notInstalled = ids(buildCommandCatalog({ cliInstalledInPATH: false }));
    expect(notInstalled).toContain("palette.installCLI");
    expect(notInstalled).not.toContain("palette.uninstallCLI");

    const installed = ids(buildCommandCatalog({ cliInstalledInPATH: true }));
    expect(installed).toContain("palette.uninstallCLI");
    expect(installed).not.toContain("palette.installCLI");
  });

  test("workspaceMinimalModeEnabled toggles enable/disable (ContentView.swift:6505/6514)", () => {
    const off = ids(buildCommandCatalog({ workspaceMinimalModeEnabled: false }));
    expect(off).toContain("palette.enableMinimalMode");
    expect(off).not.toContain("palette.disableMinimalMode");

    const on = ids(buildCommandCatalog({ workspaceMinimalModeEnabled: true }));
    expect(on).toContain("palette.disableMinimalMode");
    expect(on).not.toContain("palette.enableMinimalMode");
  });

  test("authSignedIn toggles sign-in/sign-out; authWorking hides both (AuthCommandPalette.swift:20-33)", () => {
    const signedOut = ids(buildCommandCatalog({ authSignedIn: false }));
    expect(signedOut).toContain("palette.auth.signIn");
    expect(signedOut).not.toContain("palette.auth.signOut");

    const signedIn = ids(buildCommandCatalog({ authSignedIn: true }));
    expect(signedIn).toContain("palette.auth.signOut");
    expect(signedIn).not.toContain("palette.auth.signIn");

    const working = ids(buildCommandCatalog({ authSignedIn: true, authWorking: true }));
    expect(working).not.toContain("palette.auth.signIn");
    expect(working).not.toContain("palette.auth.signOut");
  });
});

describe("context-toggled titles (ContentView.swift:6490-6494, 6721-6722, 7005-7008)", () => {
  test("toggleMatchTerminalBackground title flips on sidebarMatchTerminalBackground", () => {
    const off = byId(buildCommandCatalog({}), "palette.toggleMatchTerminalBackground");
    expect(off?.title).toBe("Enable Match Terminal Background");
    const on = byId(
      buildCommandCatalog({ sidebarMatchTerminalBackground: true }),
      "palette.toggleMatchTerminalBackground",
    );
    expect(on?.title).toBe("Disable Match Terminal Background");
  });

  test("toggleWorkspacePin title flips on workspaceShouldPin", () => {
    const pin = byId(
      buildCommandCatalog({ hasWorkspace: true, workspaceShouldPin: true }),
      "palette.toggleWorkspacePin",
    );
    expect(pin?.title).toBe("Pin Workspace");
    const unpin = byId(
      buildCommandCatalog({ hasWorkspace: true, workspaceShouldPin: false }),
      "palette.toggleWorkspacePin",
    );
    expect(unpin?.title).toBe("Unpin Workspace");
  });
});

describe("name-interpolated subtitles (ContentView.swift:6268-6291)", () => {
  test("workspace subtitle uses workspaceName, falling back to 'Workspace'", () => {
    const named = byId(
      buildCommandCatalog({ hasWorkspace: true, workspaceName: "Phoenix" }),
      "palette.renameWorkspace",
    );
    expect(named?.subtitle).toBe("Workspace • Phoenix");
    const fallback = byId(
      buildCommandCatalog({ hasWorkspace: true }),
      "palette.renameWorkspace",
    );
    expect(fallback?.subtitle).toBe("Workspace • Workspace");
  });

  test("terminal subtitle uses panelName, falling back to 'Tab'", () => {
    const row = byId(
      buildCommandCatalog({ panelIsTerminal: true, panelName: "zsh" }),
      "palette.terminalFind",
    );
    expect(row?.subtitle).toBe("Terminal • zsh");
  });
});

describe("enablement gate (ContentView.swift:6774)", () => {
  test("moveWorkspaceUp needs workspaceHasAbove", () => {
    const present = ids(buildCommandCatalog({ hasWorkspace: true, workspaceHasAbove: true }));
    expect(present).toContain("palette.moveWorkspaceUp");
    const absent = ids(buildCommandCatalog({ hasWorkspace: true, workspaceHasAbove: false }));
    expect(absent).not.toContain("palette.moveWorkspaceUp");
  });
});

describe("intent dispatch is single-source (HandlerRegistry parity)", () => {
  test("every cataloged id resolves to a non-null intent", () => {
    // Build with every gate on so the maximum static row set is present.
    const ctx: CommandContext = {
      cliInstalledInPATH: true,
      vscodeInlineOpenTargetAvailable: true,
      workspaceMinimalModeEnabled: true,
      sidebarMatchTerminalBackground: true,
      hasWorkspace: true,
      workspaceHasCustomName: true,
      workspaceHasCustomDescription: true,
      workspaceShouldPin: true,
      workspaceHasAbove: true,
      workspaceHasBelow: true,
      workspaceHasPeers: true,
      workspaceCanMarkRead: true,
      workspaceCanMarkUnread: true,
      workspaceHasPullRequests: true,
      workspaceHasSplits: true,
      hasFocusedPanel: true,
      panelHasCustomName: true,
      panelShouldPin: true,
      panelHasUnread: true,
      panelHasPane: true,
      panelCanMoveToNewWorkspace: true,
      panelIsBrowser: true,
      panelIsMarkdown: true,
      panelIsTerminal: true,
      panelBrowserFocusModeActive: true,
      panelBrowserOmnibarVisible: true,
      panelHasForkableAgent: true,
      defaultTerminalIsDefault: false,
      updateHasAvailable: true,
      authSignedIn: false,
      authWorking: false,
    };
    const catalog = buildCommandCatalog(ctx);
    expect(catalog.length).toBeGreaterThan(0);
    for (const descriptor of catalog) {
      expect(resolveIntent(descriptor.id)).not.toBeNull();
      // The descriptor's own resolved intent matches the registry.
      expect(descriptor.intent).toEqual(resolveIntent(descriptor.id)!);
    }
  });

  test("no two ids map to the same intent kind (1:1 registry)", () => {
    const ctx: CommandContext = {
      cliInstalledInPATH: true,
      hasWorkspace: true,
      hasFocusedPanel: true,
      panelHasPane: true,
      panelIsBrowser: true,
      panelIsMarkdown: true,
      panelIsTerminal: true,
      workspaceHasSplits: true,
      panelHasForkableAgent: true,
    };
    const catalog = buildCommandCatalog(ctx);
    const kinds = catalog.map((d) => d.intent.kind);
    expect(new Set(kinds).size).toBe(kinds.length);
  });

  test("resolveIntent returns null for an unknown id", () => {
    expect(resolveIntent("palette.doesNotExist")).toBeNull();
  });

  test("dispatchCommand returns the descriptor's intent + dismissOnRun", () => {
    // newWorkspace dismisses on run (default true).
    const run = dispatchCommand("palette.newWorkspace", EMPTY_CTX);
    expect(run).toEqual({ intent: { kind: "newWorkspace" }, dismissOnRun: true });

    // renameWorkspace has dismissOnRun:false (ContentView.swift:6680).
    const rename = dispatchCommand("palette.renameWorkspace", { hasWorkspace: true });
    expect(rename).toEqual({ intent: { kind: "renameWorkspace" }, dismissOnRun: false });

    // A filtered-out command dispatches to null.
    expect(dispatchCommand("palette.renameWorkspace", EMPTY_CTX)).toBeNull();
    expect(dispatchCommand("palette.doesNotExist", EMPTY_CTX)).toBeNull();
  });
});

describe("config-override hook (ContentView.swift:6023-6043)", () => {
  // The host only consults cmux.json for the four configurable ids
  // (`commandPaletteConfigActionID`, 6054-6067); `palette.newTerminalTab` is
  // one of them, so it exercises the real override path.
  test("{palette:false} drops the row before rank assignment", () => {
    const withRow = buildCommandCatalog(EMPTY_CTX);
    const withoutRow = buildCommandCatalog(EMPTY_CTX, {
      configResolver: (id) => (id === "palette.newTerminalTab" ? { palette: false } : null),
    });
    expect(ids(withRow)).toContain("palette.newTerminalTab");
    expect(ids(withoutRow)).not.toContain("palette.newTerminalTab");
    // The gap is compacted: ranks stay contiguous.
    withoutRow.forEach((d, index) => expect(d.rank).toBe(index));
    expect(withoutRow.length).toBe(withRow.length - 1);
  });

  test("override replaces title/subtitle/keywords but not id (6037-6043)", () => {
    const catalog = buildCommandCatalog(EMPTY_CTX, {
      configResolver: (id) =>
        id === "palette.newTerminalTab"
          ? { palette: true, title: "X", subtitle: "Y", keywords: ["z"] }
          : null,
    });
    const row = byId(catalog, "palette.newTerminalTab");
    expect(row?.id).toBe("palette.newTerminalTab");
    expect(row?.title).toBe("X");
    expect(row?.subtitle).toBe("Y");
    expect(row?.keywords).toEqual(["z"]);
  });

  test("empty override keywords fall back to the contribution keywords (6041-6042)", () => {
    const catalog = buildCommandCatalog(EMPTY_CTX, {
      configResolver: (id) =>
        id === "palette.newTerminalTab" ? { palette: true, keywords: [] } : null,
    });
    const row = byId(catalog, "palette.newTerminalTab");
    expect(row?.keywords).toEqual(["new", "terminal", "tab"]);
  });

  test("config is NOT consulted for non-configurable ids (6054-6067 gating)", () => {
    // The host never calls resolvedAction for ids outside the four-id switch, so
    // a resolver that would drop / rewrite a non-configurable id is ignored.
    const catalog = buildCommandCatalog(EMPTY_CTX, {
      configResolver: (id) =>
        id === "palette.newWorkspace"
          ? { palette: false, title: "SHOULD-NOT-APPLY" }
          : null,
    });
    const row = byId(catalog, "palette.newWorkspace");
    expect(row).toBeDefined();
    expect(row?.title).toBe("New Workspace");
  });
});

describe("dynamic-contribution insertion points (contentsOf splice parity)", () => {
  test("injected canvas rows splice between View commands and showNotifications", () => {
    const canvasRow: CommandContribution = {
      commandId: "palette.canvas.toggleLayout",
      title: () => "Toggle Canvas Layout",
      subtitle: () => "Canvas",
      keywords: ["canvas", "layout"],
      dismissOnRun: true,
      when: (c) => c.hasWorkspace === true,
      enablement: () => true,
      intent: { kind: "toggleSidebar" }, // stand-in kind for the injected row
    };
    const catalog = ids(
      buildCommandCatalog(
        { hasWorkspace: true },
        { dynamicContributions: { canvas: [canvasRow] } },
      ),
    );
    const canvasIndex = catalog.indexOf("palette.canvas.toggleLayout");
    const taskManagerIndex = catalog.indexOf("palette.openTaskManager");
    const notificationsIndex = catalog.indexOf("palette.showNotifications");
    expect(canvasIndex).toBeGreaterThan(taskManagerIndex);
    expect(canvasIndex).toBeLessThan(notificationsIndex);
  });

  test("the default catalog omits the runtime sub-lists", () => {
    const catalog = ids(buildCommandCatalog({ hasWorkspace: true }));
    expect(catalog.some((id) => id.startsWith("palette.canvas."))).toBe(false);
    expect(catalog.some((id) => id.startsWith("palette.toggleSetting."))).toBe(false);
  });
});

describe("declared-order golden (id, rank) under the empty context", () => {
  test("the first rows match the Swift contribution order (ContentView.swift:6323-6469)", () => {
    const catalog = buildCommandCatalog(EMPTY_CTX);
    // In the empty context browserDisabled is false (browser rows survive),
    // cliInstalledInPATH is false (installCLI survives, uninstallCLI drops), and
    // vscodeInlineOpenTargetAvailable is false (openFolderInVSCodeInline drops).
    // This locks the surviving head-of-list order + rank.
    expect(catalog.slice(0, 14).map((d) => ({ id: d.id, rank: d.rank }))).toEqual([
      { id: "palette.newWorkspace", rank: 0 },
      { id: "palette.newBrowserWorkspace", rank: 1 },
      { id: "palette.newWindow", rank: 2 },
      { id: "palette.installCLI", rank: 3 },
      { id: "palette.openFolder", rank: 4 },
      { id: "palette.reopenPreviousSession", rank: 5 },
      { id: "palette.newTerminalTab", rank: 6 },
      { id: "palette.newBrowserTab", rank: 7 },
      { id: "palette.closeTab", rank: 8 },
      { id: "palette.closeWorkspace", rank: 9 },
      { id: "palette.closeWindow", rank: 10 },
      { id: "palette.toggleFullScreen", rank: 11 },
      { id: "palette.reopenClosedBrowserTab", rank: 12 },
      { id: "palette.toggleSidebar", rank: 13 },
    ]);
  });
});
