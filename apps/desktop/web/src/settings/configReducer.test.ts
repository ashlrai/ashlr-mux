import { describe, expect, test } from "bun:test";

import type {
  AppConfig,
  AutomationConfig,
  BrowserConfig,
  CanvasConfig,
  Config,
  DiffViewerConfig,
  FileEditorConfig,
  FileExplorerConfig,
  MarkdownConfig,
  NotificationsConfig,
  SidebarAppearanceConfig,
  ShortcutsConfig,
  SidebarConfig,
  TerminalConfig,
  WorkspaceColorsConfig,
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

function makeAutomation(
  overrides: Partial<AutomationConfig> = {},
): AutomationConfig {
  return {
    socketControlMode: "cmuxOnly",
    socket_password: undefined,
    claudeCodeIntegration: true,
    codexIntegration: true,
    opencodeIntegration: true,
    claudeBinaryPath: "",
    workspaceAutoNaming: false,
    autoNamingAgent: "auto",
    ripgrepBinaryPath: "",
    suppressSubagentNotifications: true,
    ampIntegration: true,
    cursorIntegration: true,
    geminiIntegration: true,
    kiroIntegration: true,
    kiroNotificationLevel: "standard",
    portBase: 9100,
    portRange: 10,
    ...overrides,
  };
}

function makeBrowser(overrides: Partial<BrowserConfig> = {}): BrowserConfig {
  return {
    defaultSearchEngine: "google",
    customSearchEngineName: "",
    customSearchEngineURLTemplate: "https://www.google.com/search?q={query}",
    showSearchSuggestions: true,
    theme: "system",
    discardHiddenWebViews: true,
    hiddenWebViewDiscardDelaySeconds: 300,
    openTerminalLinksInCmuxBrowser: true,
    interceptTerminalOpenCommandInCmuxBrowser: true,
    hostsToOpenInEmbeddedBrowser: [],
    urlsToAlwaysOpenExternally: [],
    insecureHttpHostsAllowedInEmbeddedBrowser: ["localhost"],
    showImportHintOnBlankTabs: true,
    reactGrabVersion: "0.1.29",
    ...overrides,
  };
}

function makeTerminal(overrides: Partial<TerminalConfig> = {}): TerminalConfig {
  return {
    showScrollBar: true,
    scrollSpeed: 1,
    copyOnSelect: false,
    autoResumeAgentSessions: true,
    showTextBoxOnNewTerminals: false,
    focusTextBoxOnNewTerminals: false,
    agentHibernation: {
      enabled: false,
      idleSeconds: 5,
      maxLiveTerminals: 12,
      confirmationSeconds: 60,
    },
    rendererRealization: {
      enabled: true,
      idleSeconds: 30,
      maxWarmRenderers: 12,
    },
    textBoxMaxLines: 10,
    resumeCommands: [],
    ...overrides,
  };
}

function makeMarkdown(overrides: Partial<MarkdownConfig> = {}): MarkdownConfig {
  return {
    fontSize: 15,
    fontFamily: "",
    maxWidth: 980,
    ...overrides,
  };
}

function makeCanvas(overrides: Partial<CanvasConfig> = {}): CanvasConfig {
  return {
    paneGap: 16,
    snappingEnabled: true,
    ...overrides,
  };
}

function makeFileEditor(
  overrides: Partial<FileEditorConfig> = {},
): FileEditorConfig {
  return {
    wordWrap: false,
    ...overrides,
  };
}

function makeFileExplorer(
  overrides: Partial<FileExplorerConfig> = {},
): FileExplorerConfig {
  return {
    doubleClickAction: "preview",
    ...overrides,
  };
}

function makeDiffViewer(
  overrides: Partial<DiffViewerConfig> = {},
): DiffViewerConfig {
  return {
    defaultLayout: "unified",
    ...overrides,
  };
}

function makeSidebarAppearance(
  overrides: Partial<SidebarAppearanceConfig> = {},
): SidebarAppearanceConfig {
  return {
    matchTerminalBackground: false,
    tintColor: "#000000",
    lightModeTintColor: null,
    darkModeTintColor: null,
    tintOpacity: 0.03,
    ...overrides,
  };
}

function makeWorkspaceColors(
  overrides: Partial<WorkspaceColorsConfig> = {},
): WorkspaceColorsConfig {
  return {
    indicatorStyle: "leftRail",
    selectionColor: null,
    notificationBadgeColor: null,
    colors: {
      Red: "#C0392B",
      Blue: "#1565C0",
    },
    paletteOverrides: {},
    customColors: [],
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

describe("configReducer / notifications editable fields", () => {
  test("setNotificationsString updates notification text fields", () => {
    const config: Config = {
      notifications: makeNotifications({ sound: "default" }),
    };
    const next = configReducer(config, {
      type: "setNotificationsString",
      key: "sound",
      value: "custom_file",
    });
    expect(next.notifications?.sound).toBe("custom_file");
  });

  test("setNotificationsSelect updates notification select fields", () => {
    const config: Config = {
      notifications: makeNotifications({ hooksMode: "append" }),
    };
    const next = configReducer(config, {
      type: "setNotificationsSelect",
      key: "hooksMode",
      value: "replace",
    });
    expect(next.notifications?.hooksMode).toBe("replace");
  });

  test("notifications editable actions are no-ops when notifications are absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, {
        type: "setNotificationsString",
        key: "command",
        value: "notify-send cmux",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setNotificationsSelect",
        key: "hooksMode",
        value: "replace",
      }),
    ).toBe(config);
  });
});

// ---- automation editable fields ---------------------------------------------

describe("configReducer / automation editable fields", () => {
  test("toggleAutomationFlag flips an automation boolean and preserves siblings", () => {
    const config: Config = {
      automation: makeAutomation({
        workspaceAutoNaming: false,
        autoNamingAgent: "codex",
      }),
    };
    const next = configReducer(config, {
      type: "toggleAutomationFlag",
      key: "workspaceAutoNaming",
    });

    expect(next).not.toBe(config);
    expect(next.automation).not.toBe(config.automation);
    expect(next.automation?.workspaceAutoNaming).toBe(true);
    expect(next.automation?.autoNamingAgent).toBe("codex");
  });

  test("automation string, password, number, and select actions update values", () => {
    const config: Config = { automation: makeAutomation() };
    const socket = configReducer(config, {
      type: "setAutomationString",
      key: "socketControlMode",
      value: "password",
    });
    const password = configReducer(socket, {
      type: "setAutomationPassword",
      key: "socket_password",
      value: "hunter2",
    });
    const port = configReducer(password, {
      type: "setAutomationNumber",
      key: "portBase",
      value: 9200,
    });
    const notification = configReducer(port, {
      type: "setAutomationSelect",
      key: "kiroNotificationLevel",
      value: "verbose",
    });

    expect(socket.automation?.socketControlMode).toBe("password");
    expect(password.automation?.socket_password).toBe("hunter2");
    expect(port.automation?.portBase).toBe(9200);
    expect(notification.automation?.kiroNotificationLevel).toBe("verbose");
    expect(config.automation?.socketControlMode).toBe("cmuxOnly");
  });

  test("setAutomationPassword clears to undefined for null", () => {
    const config: Config = {
      automation: makeAutomation({ socket_password: "hunter2" }),
    };
    const next = configReducer(config, {
      type: "setAutomationPassword",
      key: "socket_password",
      value: null,
    });
    expect(next.automation?.socket_password).toBeUndefined();
  });

  test("automation editable actions are no-ops when automation is absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, {
        type: "toggleAutomationFlag",
        key: "workspaceAutoNaming",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setAutomationString",
        key: "autoNamingAgent",
        value: "codex",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setAutomationPassword",
        key: "socket_password",
        value: "hunter2",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setAutomationNumber",
        key: "portRange",
        value: 20,
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setAutomationSelect",
        key: "kiroNotificationLevel",
        value: "minimal",
      }),
    ).toBe(config);
  });
});

// ---- toggleBrowserFlag / setBrowserEnabled ----------------------------------

describe("configReducer / toggleBrowserFlag", () => {
  test("toggleBrowserFlag flips a browser boolean and preserves sibling settings", () => {
    const config: Config = {
      browser: makeBrowser({
        defaultSearchEngine: "duckduckgo",
        showSearchSuggestions: true,
      }),
    };
    const next = configReducer(config, {
      type: "toggleBrowserFlag",
      key: "showSearchSuggestions",
    });

    expect(next).not.toBe(config);
    expect(next.browser).not.toBe(config.browser);
    expect(next.browser?.showSearchSuggestions).toBe(false);
    expect(next.browser?.defaultSearchEngine).toBe("duckduckgo");
  });

  test("toggleBrowserFlag is a no-op when the browser section is absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, {
        type: "toggleBrowserFlag",
        key: "showSearchSuggestions",
      }),
    ).toBe(config);
  });
});

describe("configReducer / browser editable fields", () => {
  test("setBrowserString updates a string field and preserves sibling settings", () => {
    const config: Config = {
      browser: makeBrowser({
        defaultSearchEngine: "google",
        showSearchSuggestions: true,
      }),
    };
    const next = configReducer(config, {
      type: "setBrowserString",
      key: "defaultSearchEngine",
      value: "duckduckgo",
    });

    expect(next).not.toBe(config);
    expect(next.browser).not.toBe(config.browser);
    expect(next.browser?.defaultSearchEngine).toBe("duckduckgo");
    expect(next.browser?.showSearchSuggestions).toBe(true);
  });

  test("setBrowserTheme accepts the constrained appearance values", () => {
    const config: Config = { browser: makeBrowser({ theme: "system" }) };
    const next = configReducer(config, {
      type: "setBrowserTheme",
      theme: "dark",
    });
    expect(next.browser?.theme).toBe("dark");
  });

  test("setBrowserNumber updates numeric browser settings", () => {
    const config: Config = {
      browser: makeBrowser({ hiddenWebViewDiscardDelaySeconds: 300 }),
    };
    const next = configReducer(config, {
      type: "setBrowserNumber",
      key: "hiddenWebViewDiscardDelaySeconds",
      value: 45,
    });
    expect(next.browser?.hiddenWebViewDiscardDelaySeconds).toBe(45);
  });

  test("setBrowserStringList replaces lists without mutating the action payload", () => {
    const value = ["localhost", "*.internal.test"];
    const config: Config = {
      browser: makeBrowser({ hostsToOpenInEmbeddedBrowser: ["old.test"] }),
    };
    const next = configReducer(config, {
      type: "setBrowserStringList",
      key: "hostsToOpenInEmbeddedBrowser",
      value,
    });

    expect(next.browser?.hostsToOpenInEmbeddedBrowser).toEqual([
      "localhost",
      "*.internal.test",
    ]);
    expect(next.browser?.hostsToOpenInEmbeddedBrowser).not.toBe(value);
    expect(config.browser?.hostsToOpenInEmbeddedBrowser).toEqual(["old.test"]);
  });

  test("browser editable actions are no-ops when the browser section is absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, {
        type: "setBrowserString",
        key: "defaultSearchEngine",
        value: "kagi",
      }),
    ).toBe(config);
    expect(configReducer(config, { type: "setBrowserTheme", theme: "light" })).toBe(
      config,
    );
    expect(
      configReducer(config, {
        type: "setBrowserNumber",
        key: "hiddenWebViewDiscardDelaySeconds",
        value: 1,
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setBrowserStringList",
        key: "urlsToAlwaysOpenExternally",
        value: ["https://example.test"],
      }),
    ).toBe(config);
  });
});

describe("configReducer / setBrowserEnabled", () => {
  test("sets both browser routing flags and preserves sibling browser settings", () => {
    const config: Config = {
      browser: makeBrowser({
        defaultSearchEngine: "duckduckgo",
        openTerminalLinksInCmuxBrowser: true,
        interceptTerminalOpenCommandInCmuxBrowser: true,
      }),
    };
    const next = configReducer(config, {
      type: "setBrowserEnabled",
      enabled: false,
    });

    expect(next).not.toBe(config);
    expect(next.browser).not.toBe(config.browser);
    expect(next.browser?.openTerminalLinksInCmuxBrowser).toBe(false);
    expect(next.browser?.interceptTerminalOpenCommandInCmuxBrowser).toBe(false);
    expect(next.browser?.defaultSearchEngine).toBe("duckduckgo");
  });

  test("never mutates the input browser config", () => {
    const config: Config = { browser: makeBrowser() };
    configReducer(config, { type: "setBrowserEnabled", enabled: false });
    expect(config.browser?.openTerminalLinksInCmuxBrowser).toBe(true);
    expect(config.browser?.interceptTerminalOpenCommandInCmuxBrowser).toBe(true);
  });

  test("is a no-op when the browser section is absent", () => {
    const config: Config = {};
    expect(configReducer(config, { type: "setBrowserEnabled", enabled: false })).toBe(
      config,
    );
  });
});

describe("configReducer / terminal editable fields", () => {
  test("toggleTerminalFlag flips a terminal boolean and preserves sibling settings", () => {
    const config: Config = {
      terminal: makeTerminal({ copyOnSelect: false, scrollSpeed: 1.5 }),
    };
    const next = configReducer(config, {
      type: "toggleTerminalFlag",
      key: "copyOnSelect",
    });

    expect(next).not.toBe(config);
    expect(next.terminal).not.toBe(config.terminal);
    expect(next.terminal?.copyOnSelect).toBe(true);
    expect(next.terminal?.scrollSpeed).toBe(1.5);
  });

  test("setTerminalNumber updates top-level numeric terminal settings", () => {
    const config: Config = { terminal: makeTerminal({ textBoxMaxLines: 10 }) };
    const next = configReducer(config, {
      type: "setTerminalNumber",
      key: "textBoxMaxLines",
      value: 14,
    });
    expect(next.terminal?.textBoxMaxLines).toBe(14);
  });

  test("nested terminal actions update hibernation and renderer settings immutably", () => {
    const config: Config = { terminal: makeTerminal() };
    const toggled = configReducer(config, {
      type: "toggleTerminalNestedFlag",
      section: "agentHibernation",
      key: "enabled",
    });
    const resized = configReducer(toggled, {
      type: "setTerminalNestedNumber",
      section: "rendererRealization",
      key: "maxWarmRenderers",
      value: 4,
    });

    expect(toggled.terminal?.agentHibernation.enabled).toBe(true);
    expect(toggled.terminal?.agentHibernation).not.toBe(
      config.terminal?.agentHibernation,
    );
    expect(resized.terminal?.rendererRealization.maxWarmRenderers).toBe(4);
    expect(config.terminal?.rendererRealization.maxWarmRenderers).toBe(12);
  });

  test("terminal editable actions are no-ops when terminal is absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, {
        type: "toggleTerminalFlag",
        key: "copyOnSelect",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setTerminalNumber",
        key: "scrollSpeed",
        value: 2,
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "toggleTerminalNestedFlag",
        section: "rendererRealization",
        key: "enabled",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setTerminalNestedNumber",
        section: "agentHibernation",
        key: "idleSeconds",
        value: 60,
      }),
    ).toBe(config);
  });
});

describe("configReducer / viewer and editor editable fields", () => {
  test("markdown actions update text and numeric fields", () => {
    const config: Config = { markdown: makeMarkdown() };
    const withFont = configReducer(config, {
      type: "setMarkdownString",
      key: "fontFamily",
      value: "Georgia",
    });
    const withWidth = configReducer(withFont, {
      type: "setMarkdownNumber",
      key: "maxWidth",
      value: 720,
    });

    expect(withFont.markdown?.fontFamily).toBe("Georgia");
    expect(withFont.markdown).not.toBe(config.markdown);
    expect(withWidth.markdown?.maxWidth).toBe(720);
    expect(config.markdown?.maxWidth).toBe(980);
  });

  test("canvas actions update snapping and pane gap", () => {
    const config: Config = { canvas: makeCanvas({ snappingEnabled: true }) };
    const toggled = configReducer(config, {
      type: "toggleCanvasFlag",
      key: "snappingEnabled",
    });
    const resized = configReducer(toggled, {
      type: "setCanvasNumber",
      key: "paneGap",
      value: 24,
    });

    expect(toggled.canvas?.snappingEnabled).toBe(false);
    expect(resized.canvas?.paneGap).toBe(24);
    expect(config.canvas?.paneGap).toBe(16);
  });

  test("file editor, file explorer, and diff viewer actions update their slices", () => {
    const config: Config = {
      file_editor: makeFileEditor({ wordWrap: false }),
      file_explorer: makeFileExplorer({ doubleClickAction: "preview" }),
      diff_viewer: makeDiffViewer({ defaultLayout: "unified" }),
    };
    const wrapped = configReducer(config, {
      type: "toggleFileEditorFlag",
      key: "wordWrap",
    });
    const opened = configReducer(wrapped, {
      type: "setFileExplorerSelect",
      key: "doubleClickAction",
      value: "preferredEditor",
    });
    const split = configReducer(opened, {
      type: "setDiffViewerSelect",
      key: "defaultLayout",
      value: "split",
    });

    expect(wrapped.file_editor?.wordWrap).toBe(true);
    expect(opened.file_explorer?.doubleClickAction).toBe("preferredEditor");
    expect(split.diff_viewer?.defaultLayout).toBe("split");
    expect(config.diff_viewer?.defaultLayout).toBe("unified");
  });

  test("viewer/editor actions are no-ops when their sections are absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, {
        type: "setMarkdownString",
        key: "fontFamily",
        value: "Georgia",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setMarkdownNumber",
        key: "fontSize",
        value: 18,
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "toggleCanvasFlag",
        key: "snappingEnabled",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setCanvasNumber",
        key: "paneGap",
        value: 24,
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "toggleFileEditorFlag",
        key: "wordWrap",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setFileExplorerSelect",
        key: "doubleClickAction",
        value: "defaultEditor",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setDiffViewerSelect",
        key: "defaultLayout",
        value: "split",
      }),
    ).toBe(config);
  });
});

describe("configReducer / workspace color editable fields", () => {
  test("workspace color scalar and nullable fields update immutably", () => {
    const config: Config = {
      workspace_colors: makeWorkspaceColors({ indicatorStyle: "leftRail" }),
    };
    const indicator = configReducer(config, {
      type: "setWorkspaceColorsString",
      key: "indicatorStyle",
      value: "dot",
    });
    const selected = configReducer(indicator, {
      type: "setWorkspaceColorsNullableString",
      key: "selectionColor",
      value: "#123456",
    });

    expect(indicator.workspace_colors?.indicatorStyle).toBe("dot");
    expect(indicator.workspace_colors).not.toBe(config.workspace_colors);
    expect(selected.workspace_colors?.selectionColor).toBe("#123456");
    expect(config.workspace_colors?.selectionColor).toBeNull();
  });

  test("workspace color map and list fields replace values without mutation", () => {
    const colors = { Red: "#C0392B", Blue: "#1565C0" };
    const customColors = ["#111111", "#222222"];
    const config: Config = { workspace_colors: makeWorkspaceColors() };
    const withColors = configReducer(config, {
      type: "setWorkspaceColorsMap",
      key: "colors",
      value: colors,
    });
    const withCustom = configReducer(withColors, {
      type: "setWorkspaceColorsList",
      key: "customColors",
      value: customColors,
    });

    expect(withColors.workspace_colors?.colors).toEqual(colors);
    expect(withColors.workspace_colors?.colors).not.toBe(colors);
    expect(withCustom.workspace_colors?.customColors).toEqual(customColors);
    expect(withCustom.workspace_colors?.customColors).not.toBe(customColors);
    expect(config.workspace_colors?.customColors).toEqual([]);
  });

  test("workspace color editable actions are no-ops when section is absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, {
        type: "setWorkspaceColorsString",
        key: "indicatorStyle",
        value: "dot",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setWorkspaceColorsNullableString",
        key: "selectionColor",
        value: "#123456",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setWorkspaceColorsMap",
        key: "paletteOverrides",
        value: { Blue: "#0f62fe" },
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setWorkspaceColorsList",
        key: "customColors",
        value: ["#111111"],
      }),
    ).toBe(config);
  });
});

// ---- generic app field actions ---------------------------------------------

describe("configReducer / app editable fields", () => {
  test("toggleAppFlag flips an app boolean and preserves sibling settings", () => {
    const config: Config = {
      app: makeApp({ minimalMode: false, appearance: "dark" }),
    };
    const next = configReducer(config, {
      type: "toggleAppFlag",
      key: "minimalMode",
    });

    expect(next).not.toBe(config);
    expect(next.app).not.toBe(config.app);
    expect(next.app?.minimalMode).toBe(true);
    expect(next.app?.appearance).toBe("dark");
  });

  test("setAppString updates a string field", () => {
    const config: Config = { app: makeApp({ preferredEditor: "" }) };
    const next = configReducer(config, {
      type: "setAppString",
      key: "preferredEditor",
      value: "code",
    });
    expect(next.app?.preferredEditor).toBe("code");
  });

  test("setAppNumber updates a numeric app field", () => {
    const config: Config = { app: makeApp({ globalFontMagnification: 100 }) };
    const next = configReducer(config, {
      type: "setAppNumber",
      key: "globalFontMagnification",
      value: 125,
    });
    expect(next.app?.globalFontMagnification).toBe(125);
  });

  test("setAppSelect updates constrained app fields", () => {
    const config: Config = { app: makeApp({ newWorkspacePlacement: "afterCurrent" }) };
    const next = configReducer(config, {
      type: "setAppSelect",
      key: "newWorkspacePlacement",
      value: "top",
    });
    expect(next.app?.newWorkspacePlacement).toBe("top");
  });

  test("app editable actions are no-ops when the app section is absent", () => {
    const config: Config = {};
    expect(configReducer(config, { type: "toggleAppFlag", key: "minimalMode" })).toBe(
      config,
    );
    expect(
      configReducer(config, {
        type: "setAppString",
        key: "language",
        value: "en",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setAppNumber",
        key: "globalFontMagnification",
        value: 110,
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setAppSelect",
        key: "confirmQuit",
        value: "never",
      }),
    ).toBe(config);
  });
});

// ---- setMinimalMode ---------------------------------------------------------

describe("configReducer / setMinimalMode", () => {
  test("sets app.minimalMode without disturbing other app settings", () => {
    const config: Config = { app: makeApp({ minimalMode: false, appearance: "dark" }) };
    const next = configReducer(config, { type: "setMinimalMode", enabled: true });

    expect(next).not.toBe(config);
    expect(next.app).not.toBe(config.app);
    expect(next.app?.minimalMode).toBe(true);
    expect(next.app?.appearance).toBe("dark");
  });

  test("is a no-op when the app section is absent", () => {
    const config: Config = {};
    expect(configReducer(config, { type: "setMinimalMode", enabled: true })).toBe(
      config,
    );
  });
});

// ---- toggleMatchTerminalBackground -----------------------------------------

describe("configReducer / toggleMatchTerminalBackground", () => {
  test("flips sidebar_appearance.matchTerminalBackground", () => {
    const config: Config = {
      sidebar_appearance: makeSidebarAppearance({
        matchTerminalBackground: false,
        tintColor: "#123456",
      }),
    };
    const next = configReducer(config, {
      type: "toggleMatchTerminalBackground",
    });

    expect(next).not.toBe(config);
    expect(next.sidebar_appearance).not.toBe(config.sidebar_appearance);
    expect(next.sidebar_appearance?.matchTerminalBackground).toBe(true);
    expect(next.sidebar_appearance?.tintColor).toBe("#123456");
  });

  test("is a no-op when the sidebar appearance section is absent", () => {
    const config: Config = {};
    expect(configReducer(config, { type: "toggleMatchTerminalBackground" })).toBe(
      config,
    );
  });
});

describe("configReducer / sidebar editable fields", () => {
  test("setSidebarSelect updates branchLayout", () => {
    const config: Config = { sidebar: makeSidebar({ branchLayout: "vertical" }) };
    const next = configReducer(config, {
      type: "setSidebarSelect",
      key: "branchLayout",
      value: "inline",
    });
    expect(next.sidebar?.branchLayout).toBe("inline");
  });

  test("setSidebarAppearanceString updates nullable tint fields", () => {
    const config: Config = {
      sidebar_appearance: makeSidebarAppearance({ lightModeTintColor: null }),
    };
    const next = configReducer(config, {
      type: "setSidebarAppearanceString",
      key: "lightModeTintColor",
      value: "#ffffff",
    });
    expect(next.sidebar_appearance?.lightModeTintColor).toBe("#ffffff");
  });

  test("setSidebarAppearanceNumber updates tintOpacity", () => {
    const config: Config = {
      sidebar_appearance: makeSidebarAppearance({ tintOpacity: 0.03 }),
    };
    const next = configReducer(config, {
      type: "setSidebarAppearanceNumber",
      key: "tintOpacity",
      value: 0.2,
    });
    expect(next.sidebar_appearance?.tintOpacity).toBe(0.2);
  });

  test("sidebar editable actions are no-ops when their sections are absent", () => {
    const config: Config = {};
    expect(
      configReducer(config, {
        type: "setSidebarSelect",
        key: "branchLayout",
        value: "inline",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setSidebarAppearanceString",
        key: "tintColor",
        value: "#123456",
      }),
    ).toBe(config);
    expect(
      configReducer(config, {
        type: "setSidebarAppearanceNumber",
        key: "tintOpacity",
        value: 0.2,
      }),
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
