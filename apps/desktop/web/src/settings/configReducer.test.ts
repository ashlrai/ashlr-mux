import { describe, expect, test } from "bun:test";

import type {
  AppConfig,
  Config,
  NotificationsConfig,
  ShortcutsConfig,
  SidebarConfig,
} from "@cmux/core-types";

import { configReducer } from "./configReducer";

// ---- Fixtures (fully-populated sections; every required field present) ------

function makeSidebar(overrides: Partial<SidebarConfig> = {}): SidebarConfig {
  return {
    hideAllDetails: false,
    wrapWorkspaceTitles: false,
    showWorkspaceDescription: true,
    branchLayout: "vertical",
    showNotificationMessage: true,
    showBranchDirectory: true,
    showPullRequests: true,
    watchGitStatus: true,
    makePullRequestsClickable: true,
    openPullRequestLinksInCmuxBrowser: false,
    openPortLinksInCmuxBrowser: false,
    showSSH: true,
    showPorts: true,
    showLog: false,
    showProgress: true,
    showCustomMetadata: true,
    ...overrides,
  };
}

function makeNotifications(
  overrides: Partial<NotificationsConfig> = {},
): NotificationsConfig {
  return {
    dockBadge: true,
    showInMenuBar: false,
    unreadPaneRing: true,
    paneFlash: false,
    sound: "default",
    customSoundFilePath: "",
    command: "",
    hooksMode: "append",
    hooks: [],
    ...overrides,
  };
}

function makeApp(overrides: Partial<AppConfig> = {}): AppConfig {
  return {
    language: "en",
    appearance: "system",
    appIcon: "automatic",
    windowTitleTemplate: "",
    menuBarOnly: false,
    newWorkspacePlacement: "end",
    forkConversationDefaultDestination: "newTab",
    workspaceInheritWorkingDirectory: true,
    minimalMode: false,
    keepWorkspaceOpenWhenClosingLastSurface: false,
    focusPaneOnFirstClick: false,
    preferredEditor: "",
    openSupportedFilesInCmux: false,
    openMarkdownInCmuxViewer: false,
    globalFontMagnification: 0,
    reorderOnNotification: false,
    iMessageMode: false,
    sendAnonymousTelemetry: false,
    confirmQuit: "always",
    warnBeforeQuit: false,
    warnBeforeClosingTab: false,
    warnBeforeClosingTabXButton: false,
    hideTabCloseButton: false,
    renameSelectsExistingName: false,
    commandPaletteSearchesAllSurfaces: false,
    ...overrides,
  };
}

function makeShortcuts(overrides: Partial<ShortcutsConfig> = {}): ShortcutsConfig {
  return {
    showModifierHoldHints: true,
    bindings: { "workspace.new": "cmd+t" },
    when: {},
    ...overrides,
  };
}

// ---- toggleSidebarFlag ------------------------------------------------------

describe("configReducer / toggleSidebarFlag", () => {
  test("flips the targeted boolean and returns a new Config", () => {
    const config: Config = { sidebar: makeSidebar({ showPorts: true }) };
    const next = configReducer(config, { type: "toggleSidebarFlag", key: "showPorts" });

    expect(next).not.toBe(config);
    expect(next.sidebar).not.toBe(config.sidebar);
    expect(next.sidebar?.showPorts).toBe(false);
  });

  test("never mutates the input", () => {
    const config: Config = { sidebar: makeSidebar({ showLog: false }) };
    configReducer(config, { type: "toggleSidebarFlag", key: "showLog" });
    expect(config.sidebar?.showLog).toBe(false);
  });

  test("leaves sibling flags and other sections untouched (structural sharing)", () => {
    const config: Config = {
      sidebar: makeSidebar({ showPorts: true, showSSH: true }),
      notifications: makeNotifications(),
    };
    const next = configReducer(config, { type: "toggleSidebarFlag", key: "showPorts" });
    expect(next.sidebar?.showSSH).toBe(true);
    // Untouched section keeps its reference identity.
    expect(next.notifications).toBe(config.notifications);
  });

  test("is a no-op when the sidebar section is absent", () => {
    const config: Config = {};
    expect(configReducer(config, { type: "toggleSidebarFlag", key: "showPorts" })).toBe(
      config,
    );
  });
});

// ---- toggleNotificationsFlag ------------------------------------------------

describe("configReducer / toggleNotificationsFlag", () => {
  test("flips the targeted boolean and returns a new Config", () => {
    const config: Config = { notifications: makeNotifications({ dockBadge: true }) };
    const next = configReducer(config, {
      type: "toggleNotificationsFlag",
      key: "dockBadge",
    });

    expect(next).not.toBe(config);
    expect(next.notifications).not.toBe(config.notifications);
    expect(next.notifications?.dockBadge).toBe(false);
  });

  test("does not disturb non-boolean fields", () => {
    const config: Config = {
      notifications: makeNotifications({ paneFlash: false, sound: "ping" }),
    };
    const next = configReducer(config, {
      type: "toggleNotificationsFlag",
      key: "paneFlash",
    });
    expect(next.notifications?.paneFlash).toBe(true);
    expect(next.notifications?.sound).toBe("ping");
  });

  test("is a no-op when the notifications section is absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, { type: "toggleNotificationsFlag", key: "dockBadge" }),
    ).toBe(config);
  });
});

// ---- setAppearance ----------------------------------------------------------

describe("configReducer / setAppearance", () => {
  test("sets app.appearance to each of the 3 config values", () => {
    const config: Config = { app: makeApp({ appearance: "system" }) };

    const toDark = configReducer(config, { type: "setAppearance", appearance: "dark" });
    expect(toDark.app?.appearance).toBe("dark");
    expect(toDark).not.toBe(config);
    expect(toDark.app).not.toBe(config.app);

    const toLight = configReducer(config, { type: "setAppearance", appearance: "light" });
    expect(toLight.app?.appearance).toBe("light");

    const toSystem = configReducer(config, {
      type: "setAppearance",
      appearance: "system",
    });
    expect(toSystem.app?.appearance).toBe("system");
  });

  test("never mutates the input", () => {
    const config: Config = { app: makeApp({ appearance: "light" }) };
    configReducer(config, { type: "setAppearance", appearance: "dark" });
    expect(config.app?.appearance).toBe("light");
  });

  test("is a no-op when the app section is absent", () => {
    const config: Config = {};
    expect(configReducer(config, { type: "setAppearance", appearance: "dark" })).toBe(
      config,
    );
  });
});

// ---- setShortcutBinding -----------------------------------------------------

describe("configReducer / setShortcutBinding", () => {
  test("sets a new binding without disturbing existing ones", () => {
    const config: Config = {
      shortcuts: makeShortcuts({ bindings: { "workspace.new": "cmd+t" } }),
    };
    const next = configReducer(config, {
      type: "setShortcutBinding",
      action: "workspace.close",
      binding: ["cmd", "w"],
    });

    expect(next).not.toBe(config);
    expect(next.shortcuts).not.toBe(config.shortcuts);
    expect(next.shortcuts?.bindings).not.toBe(config.shortcuts?.bindings);
    expect(next.shortcuts?.bindings["workspace.new"]).toBe("cmd+t");
    expect(next.shortcuts?.bindings["workspace.close"]).toEqual(["cmd", "w"]);
  });

  test("overwrites an existing binding", () => {
    const config: Config = {
      shortcuts: makeShortcuts({ bindings: { "workspace.new": "cmd+t" } }),
    };
    const next = configReducer(config, {
      type: "setShortcutBinding",
      action: "workspace.new",
      binding: "cmd+n",
    });
    expect(next.shortcuts?.bindings["workspace.new"]).toBe("cmd+n");
  });

  test("an explicit null unbind is preserved distinctly from an absent key", () => {
    const config: Config = {
      shortcuts: makeShortcuts({ bindings: { "workspace.new": "cmd+t" } }),
    };
    const next = configReducer(config, {
      type: "setShortcutBinding",
      action: "workspace.new",
      binding: null,
    });
    expect(next.shortcuts?.bindings).toHaveProperty(["workspace.new"]);
    expect(next.shortcuts?.bindings["workspace.new"]).toBeNull();
  });

  test("never mutates the input bindings map", () => {
    const config: Config = {
      shortcuts: makeShortcuts({ bindings: { "workspace.new": "cmd+t" } }),
    };
    configReducer(config, {
      type: "setShortcutBinding",
      action: "workspace.new",
      binding: "cmd+n",
    });
    expect(config.shortcuts?.bindings["workspace.new"]).toBe("cmd+t");
  });

  test("is a no-op when the shortcuts section is absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, {
        type: "setShortcutBinding",
        action: "workspace.new",
        binding: "cmd+n",
      }),
    ).toBe(config);
  });
});
