import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

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

import { SettingsPane } from "./SettingsPane";

// ---- Fixtures ---------------------------------------------------------------

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

function makeShortcuts(overrides: Partial<ShortcutsConfig> = {}): ShortcutsConfig {
  return {
    showModifierHoldHints: true,
    bindings: { "workspace.new": "cmd+t", "workspace.close": ["cmd", "w"] },
    when: {},
    ...overrides,
  };
}

const noop = () => {};

/** The full `<input …>` tag carrying the given `data-*` attribute value. */
function tagByAttr(markup: string, attr: string, value: string): string {
  const m = markup.match(new RegExp(`<input[^>]*${attr}="${value}"[^>]*>`));
  return m ? m[0] : "";
}

// ---- Section labels ---------------------------------------------------------

describe("SettingsPane section labels", () => {
  test("renders a heading for every present section", () => {
    const config: Config = {
      sidebar: makeSidebar(),
      notifications: makeNotifications(),
      sidebar_appearance: makeSidebarAppearance(),
      workspace_colors: makeWorkspaceColors(),
      browser: makeBrowser(),
      automation: makeAutomation(),
      terminal: makeTerminal(),
      markdown: makeMarkdown(),
      canvas: makeCanvas(),
      file_editor: makeFileEditor(),
      file_explorer: makeFileExplorer(),
      diff_viewer: makeDiffViewer(),
      app: makeApp(),
      shortcuts: makeShortcuts(),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);
    expect(markup).toContain("<h2>Account</h2>");
    expect(markup).toContain("<h2>Mobile</h2>");
    expect(markup).toContain("<h2>Updates</h2>");
    expect(markup).toContain("<h2>Custom Sidebars</h2>");
    expect(markup).toContain("<h2>Beta Features</h2>");
    expect(markup).toContain("<h2>Automation</h2>");
    expect(markup).toContain("<h2>Browser</h2>");
    expect(markup).toContain("<h2>Import Browser Data</h2>");
    expect(markup).toContain("<h2>Global Hotkey</h2>");
    expect(markup).toContain("<h2>Terminal</h2>");
    expect(markup).toContain("<h2>Workspace Colors</h2>");
    expect(markup).toContain("<h2>Sidebar</h2>");
    expect(markup).toContain("<h2>Notifications</h2>");
    expect(markup).toContain("<h2>App</h2>");
    expect(markup).toContain("<h2>Shortcuts</h2>");
    expect(markup).toContain("<h2>cmux.json</h2>");
    expect(markup).toContain("<h2>Reset</h2>");
  });

  test("omits a section whose config slice is absent", () => {
    const config: Config = { app: makeApp() };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);
    expect(markup).toContain("<h2>Account</h2>");
    expect(markup).toContain("<h2>Mobile</h2>");
    expect(markup).toContain("<h2>Updates</h2>");
    expect(markup).toContain("<h2>Custom Sidebars</h2>");
    expect(markup).toContain("<h2>Beta Features</h2>");
    expect(markup).toContain("<h2>Import Browser Data</h2>");
    expect(markup).toContain("<h2>Global Hotkey</h2>");
    expect(markup).toContain("<h2>App</h2>");
    expect(markup).toContain("<h2>cmux.json</h2>");
    expect(markup).toContain("<h2>Reset</h2>");
    expect(markup).not.toContain("<h2>Browser</h2>");
    expect(markup).not.toContain("<h2>Automation</h2>");
    expect(markup).not.toContain("<h2>Terminal</h2>");
    expect(markup).not.toContain("<h2>Workspace Colors</h2>");
    expect(markup).not.toContain("<h2>Sidebar</h2>");
    expect(markup).not.toContain("<h2>Notifications</h2>");
    expect(markup).not.toContain("<h2>Shortcuts</h2>");
  });

  test("renders settings file and reset actions when callbacks are provided", () => {
    const config: Config = { app: makeApp() };
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={config}
        onChange={noop}
        onOpenSettingsFile={noop}
        onOpenSettingsFileInCmux={noop}
        onResetConfig={noop}
      />,
    );
    expect(markup).toContain("Open in cmux");
    expect(markup).toContain("Open externally");
    expect(markup).toContain("Reset cmux.json");
    expect(markup).toContain('<button type="button">Open in cmux</button>');
    expect(markup).toContain('<button type="button">Open externally</button>');
    expect(markup).toContain('<button type="button">Reset cmux.json</button>');
  });

  test("renders the in-app raw cmux.json editor controls", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        rawSettingsPath="C:\\Users\\User\\.cmux\\cmux.json"
        rawSettingsDraft={'{\n  "app": { "minimal_mode": true }\n}'}
        rawSettingsError="Expected a JSON object"
        rawSettingsStatus="Saved cmux.json"
        onLoadRawSettings={noop}
        onRawSettingsDraftChange={noop}
        onSaveRawSettings={noop}
      />,
    );

    expect(markup).toContain("Edit raw cmux.json in cmux");
    expect(markup).toContain("C:\\\\Users\\\\User\\\\.cmux\\\\cmux.json");
    expect(markup).toContain("Expected a JSON object");
    expect(markup).toContain("Saved cmux.json");
    expect(markup).toContain('aria-label="Raw cmux.json editor"');
    expect(markup).toContain("&quot;minimal_mode&quot;: true");
    expect(markup).toContain('<button type="button">Load</button>');
    expect(markup).toContain('<button type="button">Save</button>');
  });

  test("keeps raw cmux.json save disabled until there is a draft", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        rawSettingsDraft="  "
        onLoadRawSettings={noop}
        onRawSettingsDraftChange={noop}
        onSaveRawSettings={noop}
      />,
    );

    expect(markup).toContain(
      '<button type="button" disabled="">Save</button>',
    );
  });
});

// ---- Account status ---------------------------------------------------------

describe("SettingsPane account status", () => {
  test("renders agent provider CLI availability from the backend", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        agentProviderStatus={[
          {
            id: "codex",
            display_name: "Codex",
            executable_name: "codex",
            transport_kind: "stdio-jsonrpc",
            available: true,
            executable_path: "C:\\Users\\A\\AppData\\Local\\Codex\\codex.exe",
            searched_directories: [],
          },
          {
            id: "claude",
            display_name: "Claude Code",
            executable_name: "claude",
            transport_kind: "stdio-jsonl",
            available: false,
            executable_path: null,
            searched_directories: ["C:\\Users\\A\\bin"],
          },
        ]}
        onRefreshAgentProviderStatus={noop}
      />,
    );

    expect(markup).toContain("Agent provider CLIs");
    expect(markup).toContain(
      "cmux uses your installed provider CLIs and their existing credentials.",
    );
    expect(markup).toContain("Codex");
    expect(markup).toContain("Found C:\\Users\\A\\AppData\\Local\\Codex\\codex.exe");
    expect(markup).toContain("Claude Code");
    expect(markup).toContain("Missing claude");
    expect(markup).toContain("Refresh");
  });
});

// ---- Mobile status ----------------------------------------------------------

describe("SettingsPane mobile status", () => {
  test("renders mobile pairing listener status from the backend", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        mobilePairingStatus={{
          platform: "windows",
          backend: "none",
          available: false,
          running: false,
          pairing_supported: false,
          advertised_endpoint: null,
          reason: "No native mobile pairing listener is configured.",
        }}
        onRefreshMobilePairingStatus={noop}
      />,
    );

    expect(markup).toContain("iOS pairing");
    expect(markup).toContain("No native mobile pairing listener is configured.");
    expect(markup).toContain("Platform");
    expect(markup).toContain("windows");
    expect(markup).toContain("Backend");
    expect(markup).toContain("none");
    expect(markup).toContain("Listener");
    expect(markup).toContain("Stopped");
    expect(markup).toContain("Pairing");
    expect(markup).toContain("Unavailable");
    expect(markup).toContain("Refresh");
  });
});

// ---- Updates status ---------------------------------------------------------

describe("SettingsPane updates status", () => {
  test("renders native updater status from the backend", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        updaterStatus={{
          platform: "windows",
          current_version: "0.1.0",
          backend: "none",
          configured: false,
          check_supported: false,
          install_supported: false,
          reason: "No native updater plugin is configured.",
        }}
        onRefreshUpdaterStatus={noop}
      />,
    );

    expect(markup).toContain("Native updater");
    expect(markup).toContain("No native updater plugin is configured.");
    expect(markup).toContain("Current version");
    expect(markup).toContain("0.1.0");
    expect(markup).toContain("Platform");
    expect(markup).toContain("windows");
    expect(markup).toContain("Backend");
    expect(markup).toContain("none");
    expect(markup).toContain("Check support");
    expect(markup).toContain("Unavailable");
    expect(markup).toContain("Install support");
    expect(markup).toContain("Refresh");
  });
});

// ---- Global hotkey status ---------------------------------------------------

describe("SettingsPane global hotkey status", () => {
  test("renders system-wide shortcut registrar status from the backend", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        globalHotkeyStatus={{
          platform: "windows",
          backend: "none",
          available: false,
          registered: false,
          reason: "No native system-wide shortcut registrar is configured.",
        }}
        onRefreshGlobalHotkeyStatus={noop}
      />,
    );

    expect(markup).toContain("System-wide shortcut");
    expect(markup).toContain("No native system-wide shortcut registrar is configured.");
    expect(markup).toContain("Platform");
    expect(markup).toContain("windows");
    expect(markup).toContain("Backend");
    expect(markup).toContain("none");
    expect(markup).toContain("Registrar");
    expect(markup).toContain("Unavailable");
    expect(markup).toContain("Registration");
    expect(markup).toContain("Inactive");
    expect(markup).toContain("Refresh");
  });
});

// ---- Config extension status ------------------------------------------------

describe("SettingsPane config extension status", () => {
  test("renders custom sidebar and beta feature summaries from cmux.json", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        configExtensionStatus={{
          config_path: "C:\\Users\\A\\AppData\\Roaming\\cmux\\cmux.json",
          actions_count: 2,
          commands_count: 1,
          surface_tab_bar_buttons_count: 3,
          ui_configured: true,
          workspace_groups_count: 4,
          unknown_top_level_keys: ["betaFeatures", "experimentalThing"],
          beta_feature_keys: ["betaFeatures.feed", "experimentalThing"],
        }}
        onRefreshConfigExtensionStatus={noop}
        onOpenSettingsFile={noop}
      />,
    );

    expect(markup).toContain("Config-backed extension surfaces");
    expect(markup).toContain("Actions");
    expect(markup).toContain(">2</span>");
    expect(markup).toContain("Commands");
    expect(markup).toContain(">1</span>");
    expect(markup).toContain("Surface tab buttons");
    expect(markup).toContain(">3</span>");
    expect(markup).toContain("UI placements");
    expect(markup).toContain("Configured");
    expect(markup).toContain("Workspace groups");
    expect(markup).toContain(">4</span>");
    expect(markup).toContain("cmux found beta or experimental config keys");
    expect(markup).toContain("betaFeatures.feed");
    expect(markup).toContain("experimentalThing");
    expect(markup).toContain("Open cmux.json");
  });

  test("renders empty beta feature detection state", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        configExtensionStatus={{
          config_path: "C:\\Users\\A\\AppData\\Roaming\\cmux\\cmux.json",
          actions_count: 0,
          commands_count: 0,
          surface_tab_bar_buttons_count: 0,
          ui_configured: false,
          workspace_groups_count: 0,
          unknown_top_level_keys: [],
          beta_feature_keys: [],
        }}
        onRefreshConfigExtensionStatus={noop}
      />,
    );

    expect(markup).toContain(
      "No beta or experimental feature blocks were detected in cmux.json.",
    );
    expect(markup).toContain("Default");
    expect(markup).toContain("Beta keys");
    expect(markup).toContain("Preserved unknown keys");
  });

  test("renders persisted Feed beta availability control", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        rightSidebarBetaSettings={{ feed_enabled: true, dock_enabled: false }}
        onSetRightSidebarBetaFeature={noop}
      />,
    );

    expect(markup).toContain("Feed in right sidebar");
    expect(markup).toContain("inline agent decisions");
    expect(markup).toContain('data-field="rightSidebar.beta.feed.enabled"');
    expect(markup).toContain("checked");
  });
});

// ---- Automation settings ----------------------------------------------------

describe("SettingsPane automation settings", () => {
  test("renders editable controls for automation config", () => {
    const config: Config = {
      automation: makeAutomation({
        socketControlMode: "password",
        socket_password: "hunter2",
        claudeCodeIntegration: false,
        codexIntegration: false,
        opencodeIntegration: true,
        claudeBinaryPath: "C:\\tools\\claude.exe",
        workspaceAutoNaming: true,
        autoNamingAgent: "codex",
        ripgrepBinaryPath: "C:\\tools\\rg.exe",
        suppressSubagentNotifications: false,
        ampIntegration: false,
        cursorIntegration: true,
        geminiIntegration: false,
        kiroIntegration: true,
        kiroNotificationLevel: "verbose",
        portBase: 9200,
        portRange: 20,
      }),
    };
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={config}
        onChange={noop}
        controlSocketStatus={{
          pipe: "\\\\.\\pipe\\cmux",
          running: true,
        }}
        onRefreshControlSocketStatus={noop}
        onRestartControlSocket={noop}
      />,
    );

    expect(markup).toContain("<h2>Automation</h2>");
    expect(markup).toContain("Control socket listener");
    expect(markup).toContain("Running at \\\\.\\pipe\\cmux");
    expect(markup).toContain("Refresh");
    expect(markup).toContain("Restart Listener");
    expect(markup).toContain('data-automation-field="socketControlMode"');
    expect(markup).toContain('<option value="password" selected="">Password</option>');
    expect(tagByAttr(markup, "data-automation-field", "socketPassword")).toContain(
      'type="password"',
    );
    expect(tagByAttr(markup, "data-automation-field", "socketPassword")).toContain(
      'value="hunter2"',
    );
    expect(
      tagByAttr(markup, "data-automation-field", "claudeCodeIntegration"),
    ).not.toContain("checked");
    expect(tagByAttr(markup, "data-automation-field", "codexIntegration")).not.toContain(
      "checked",
    );
    expect(tagByAttr(markup, "data-automation-field", "opencodeIntegration")).toContain(
      "checked",
    );
    expect(
      tagByAttr(markup, "data-automation-field", "workspaceAutoNaming"),
    ).toContain("checked");
    expect(
      tagByAttr(markup, "data-automation-field", "suppressSubagentNotifications"),
    ).not.toContain("checked");
    expect(tagByAttr(markup, "data-automation-field", "claudeBinaryPath")).toContain(
      'value="C:\\tools\\claude.exe"',
    );
    expect(tagByAttr(markup, "data-automation-field", "autoNamingAgent")).toContain(
      'value="codex"',
    );
    expect(tagByAttr(markup, "data-automation-field", "ripgrepBinaryPath")).toContain(
      'value="C:\\tools\\rg.exe"',
    );
    expect(markup).toContain('data-automation-field="kiroNotificationLevel"');
    expect(markup).toContain('<option value="verbose" selected="">Verbose</option>');
    expect(tagByAttr(markup, "data-automation-field", "portBase")).toContain(
      'value="9200"',
    );
    expect(tagByAttr(markup, "data-automation-field", "portRange")).toContain(
      'value="20"',
    );
  });
});

// ---- Workspace color settings ----------------------------------------------

describe("SettingsPane workspace color settings", () => {
  test("renders editable controls for workspace color config", () => {
    const config: Config = {
      workspace_colors: makeWorkspaceColors({
        indicatorStyle: "dot",
        selectionColor: "#123456",
        notificationBadgeColor: "#abcdef",
        colors: {
          Red: "#C0392B",
          Blue: "#1565C0",
        },
        paletteOverrides: {
          Blue: "#0f62fe",
        },
        customColors: ["#111111", "#222222"],
      }),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);

    expect(markup).toContain("<h2>Workspace Colors</h2>");
    expect(
      tagByAttr(markup, "data-workspace-colors-field", "indicatorStyle"),
    ).toContain('value="dot"');
    expect(
      tagByAttr(markup, "data-workspace-colors-field", "selectionColor"),
    ).toContain('value="#123456"');
    expect(
      tagByAttr(markup, "data-workspace-colors-field", "notificationBadgeColor"),
    ).toContain('value="#abcdef"');
    expect(markup).toContain('data-workspace-colors-field="colors"');
    expect(markup).toContain("Red=#C0392B");
    expect(markup).toContain("Blue=#1565C0");
    expect(markup).toContain('data-workspace-colors-field="paletteOverrides"');
    expect(markup).toContain("Blue=#0f62fe");
    expect(markup).toContain('data-workspace-colors-field="customColors"');
    expect(markup).toContain("#111111\n#222222");
  });
});

// ---- Sidebar toggles: checked-state reflects the fixture --------------------

describe("SettingsPane sidebar toggles", () => {
  test("checkbox checked-state mirrors each fixture flag", () => {
    const config: Config = {
      sidebar: makeSidebar({
        branchLayout: "inline",
        showPorts: true,
        showLog: false,
        hideAllDetails: true,
      }),
      sidebar_appearance: makeSidebarAppearance({
        matchTerminalBackground: true,
        tintColor: "#123456",
        lightModeTintColor: "#ffffff",
        darkModeTintColor: "#111111",
        tintOpacity: 0.2,
      }),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);

    // A checked flag serializes with the `checked` attribute...
    expect(tagByAttr(markup, "data-field", "showPorts")).toContain("checked");
    expect(tagByAttr(markup, "data-field", "hideAllDetails")).toContain("checked");
    // ...an unchecked one does not.
    expect(tagByAttr(markup, "data-field", "showLog")).not.toContain("checked");

    // Every sidebar flag row is rendered (15 boolean flags).
    expect(markup).toContain('data-field="showCustomMetadata"');
    expect(markup).toContain("Show ports");
    expect(markup).toContain('data-sidebar-field="branchLayout"');
    expect(markup).toContain('<option value="inline" selected="">Inline</option>');
    expect(
      tagByAttr(markup, "data-sidebar-appearance-field", "matchTerminalBackground"),
    ).toContain("checked");
    expect(tagByAttr(markup, "data-sidebar-appearance-field", "tintColor")).toContain(
      'value="#123456"',
    );
    expect(
      tagByAttr(markup, "data-sidebar-appearance-field", "lightModeTintColor"),
    ).toContain('value="#ffffff"');
    expect(
      tagByAttr(markup, "data-sidebar-appearance-field", "darkModeTintColor"),
    ).toContain('value="#111111"');
    expect(
      tagByAttr(markup, "data-sidebar-appearance-field", "tintOpacity"),
    ).toContain('value="0.2"');
  });
});

// ---- Notifications toggles --------------------------------------------------

describe("SettingsPane notifications toggles", () => {
  test("checkbox checked-state mirrors each fixture flag", () => {
    const config: Config = {
      notifications: makeNotifications({
        dockBadge: true,
        paneFlash: false,
        sound: "custom_file",
        customSoundFilePath: "C:\\sounds\\cmux.wav",
        command: "notify-send cmux",
        hooksMode: "replace",
      }),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);
    expect(tagByAttr(markup, "data-field", "dockBadge")).toContain("checked");
    expect(tagByAttr(markup, "data-field", "paneFlash")).not.toContain("checked");
    expect(markup).toContain("Unread pane ring");
    expect(markup).toContain('data-notification-field="hooksMode"');
    expect(markup).toContain('<option value="replace" selected="">Replace</option>');
    expect(markup).toContain('data-notification-field="sound"');
    expect(markup).toContain('<option value="custom_file" selected="">Custom File...</option>');
    expect(
      tagByAttr(markup, "data-notification-field", "customSoundFilePath"),
    ).toContain('value="C:\\sounds\\cmux.wav"');
    expect(tagByAttr(markup, "data-notification-field", "command")).toContain(
      'value="notify-send cmux"',
    );
    expect(markup).toContain("Custom command test");
    expect(markup).toContain("Run Test Command");
    expect(markup).toContain("Windows toast delivery");
    expect(markup).toContain("Preview Toast");
    expect(markup).toContain("Send Test Toast");
  });

  test("custom command test row reflects callback and status", () => {
    const config: Config = {
      notifications: makeNotifications({ command: "echo cmux" }),
    };
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={config}
        onChange={noop}
        notificationCommandTestStatus="Notification command launched."
        onTestNotificationCommand={noop}
      />,
    );

    const button = markup.match(/<button[^>]*data-notification-command-test[^>]*>/)?.[0] ?? "";
    expect(button).not.toContain("disabled");
    expect(markup).toContain("Notification command launched.");
    expect(markup).toContain("CMUX_NOTIFICATION_TITLE");
  });

  test("toast delivery preview row reflects callback and status", () => {
    const config: Config = {
      notifications: makeNotifications({ sound: "Ping" }),
    };
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={config}
        onChange={noop}
        notificationDeliveryPreviewStatus="Desktop toast ready: tag notifications, group settings."
        onPreviewNotificationDelivery={noop}
        notificationToastSendStatus="Windows test toast sent. Tag notifications, group settings."
        onSendTestNotificationToast={noop}
      />,
    );

    const previewButton =
      markup.match(/<button[^>]*data-notification-delivery-preview[^>]*>/)?.[0] ?? "";
    const sendButton =
      markup.match(/<button[^>]*data-notification-toast-send[^>]*>/)?.[0] ?? "";
    expect(previewButton).not.toContain("disabled");
    expect(sendButton).not.toContain("disabled");
    expect(markup).toContain("Desktop toast ready: tag notifications, group settings.");
    expect(markup).toContain("Windows test toast sent. Tag notifications, group settings.");
  });
});

// ---- Browser toggles -------------------------------------------------------

describe("SettingsPane browser settings", () => {
  test("renders editable controls for every browser config shape", () => {
    const config: Config = {
      browser: makeBrowser({
        defaultSearchEngine: "duckduckgo",
        customSearchEngineName: "My Search",
        customSearchEngineURLTemplate: "https://search.test?q={query}",
        theme: "dark",
        showSearchSuggestions: false,
        discardHiddenWebViews: true,
        hiddenWebViewDiscardDelaySeconds: 120,
        openTerminalLinksInCmuxBrowser: true,
        hostsToOpenInEmbeddedBrowser: ["*.internal.test", "docs.test"],
        urlsToAlwaysOpenExternally: ["https://external.test/*"],
        insecureHttpHostsAllowedInEmbeddedBrowser: ["localhost", "*.localtest.me"],
        reactGrabVersion: "0.2.0",
      }),
    };
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={config}
        onChange={noop}
        vscodeInlineAvailable
        onOpenFolderInVSCodeInline={noop}
        onRefreshVSCodeInlineStatus={noop}
        onRestartVSCodeServeWeb={noop}
        onStopVSCodeServeWeb={noop}
      />,
    );

    expect(markup).toContain("VS Code web workspace");
    expect(markup).toContain("VS Code command:");
    expect(markup).toContain("detected");
    expect(markup).toContain("Refresh Status");
    expect(markup).toContain("Open Folder in VS Code");
    expect(markup).toContain("Restart Server");
    expect(markup).toContain("Stop Server");
    expect(tagByAttr(markup, "data-browser-field", "showSearchSuggestions")).not.toContain(
      "checked",
    );
    expect(tagByAttr(markup, "data-browser-field", "discardHiddenWebViews")).toContain(
      "checked",
    );
    expect(
      tagByAttr(markup, "data-browser-field", "openTerminalLinksInCmuxBrowser"),
    ).toContain("checked");
    expect(markup).toContain('data-browser-field="defaultSearchEngine"');
    expect(markup).toContain('<option value="duckduckgo" selected="">DuckDuckGo</option>');
    expect(markup).toContain('data-browser-field="theme"');
    expect(markup).toContain('<option value="dark" selected="">Dark</option>');
    expect(tagByAttr(markup, "data-browser-field", "customSearchEngineName")).toContain(
      'value="My Search"',
    );
    expect(
      tagByAttr(markup, "data-browser-field", "customSearchEngineURLTemplate"),
    ).toContain('value="https://search.test?q={query}"');
    expect(
      tagByAttr(markup, "data-browser-field", "hiddenWebViewDiscardDelaySeconds"),
    ).toContain('value="120"');
    expect(tagByAttr(markup, "data-browser-field", "reactGrabVersion")).toContain(
      'value="0.2.0"',
    );
    expect(markup).toContain("*.internal.test\n");
    expect(markup).toContain("https://external.test/*");
    expect(markup).toContain("*.localtest.me");
  });

  test("disables VS Code folder open when the backend probe is unavailable", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ browser: makeBrowser() }}
        onChange={noop}
        vscodeInlineAvailable={false}
        onOpenFolderInVSCodeInline={noop}
        onRefreshVSCodeInlineStatus={noop}
      />,
    );

    expect(markup).toContain("not found");
    expect(markup).toMatch(
      /<button[^>]*disabled=""[^>]*>Open Folder in VS Code<\/button>/,
    );
  });
});

// ---- Browser import ---------------------------------------------------------

describe("SettingsPane browser import", () => {
  test("renders detected browser import profiles from the backend", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        browserImportProfiles={[
          {
            browser_id: "chrome",
            browser_name: "Google Chrome",
            profile_name: "Default",
            profile_path: "C:\\Users\\A\\AppData\\Local\\Google\\Chrome\\User Data\\Default",
            bookmarks_path: "C:\\Chrome\\Default\\Bookmarks",
            history_path: "C:\\Chrome\\Default\\History",
            cookies_path: null,
            importable_items: ["bookmarks", "history"],
          },
        ]}
        browserImportInitialWizardStep={3}
        onRefreshBrowserImportProfiles={noop}
        onStartBrowserImport={noop}
      />,
    );

    expect(markup).toContain("<h2>Import Browser Data</h2>");
    expect(markup).toContain("Detected browser profiles");
    expect(markup).toContain(
      "cmux found browser profiles with importable bookmarks, history, or cookies.",
    );
    expect(markup).toContain("Google Chrome - Default");
    expect(markup).toContain("bookmarks, history");
    expect(markup).toContain("BrowserImportCookiesCheckbox");
    expect(markup).toContain("BrowserImportHistoryCheckbox");
    expect(markup).toContain("BrowserImportAdditionalDataCheckbox");
    expect(markup).toContain("Start Import");
    expect(markup).toContain("Refresh");
  });

  test("renders separate/merge controls for multiple source profiles", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        browserImportProfiles={[
          {
            browser_id: "chrome",
            browser_name: "Google Chrome",
            profile_name: "You",
            profile_path: "C:\\Chrome\\You",
            importable_items: ["bookmarks", "history", "cookies"],
          },
          {
            browser_id: "chrome",
            browser_name: "Google Chrome",
            profile_name: "austin",
            profile_path: "C:\\Chrome\\austin",
            importable_items: ["bookmarks", "history", "cookies"],
          },
        ]}
        browserImportDestinationProfiles={[
          { id: "default", display_name: "Default", is_default: true },
          { id: "work", display_name: "Work" },
        ]}
        browserImportInitialWizardStep={3}
        onRefreshBrowserImportProfiles={noop}
        onStartBrowserImport={noop}
      />,
    );

    expect(markup).toContain("Separate profiles");
    expect(markup).toContain("Merge into one");
    expect(markup).toContain("BrowserImportDestinationPopup-You");
    expect(markup).toContain("BrowserImportDestinationPopup-austin");
    expect(markup).toContain("Add to Work");
  });

  test("renders backend destination profiles in single-destination controls", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        browserImportProfiles={[
          {
            browser_id: "chrome",
            browser_name: "Google Chrome",
            profile_name: "Default",
            profile_path: "C:\\Chrome\\Default",
            importable_items: ["bookmarks", "history", "cookies"],
          },
        ]}
        browserImportDestinationProfiles={[
          { id: "personal", display_name: "Personal", is_default: true },
          { id: "work", display_name: "Work" },
        ]}
        browserImportInitialWizardStep={3}
        onRefreshBrowserImportProfiles={noop}
        onStartBrowserImport={noop}
      />,
    );

    expect(markup).toContain("BrowserImportDestinationPopup-single");
    expect(markup).toContain('<option value="personal" selected="">Personal</option>');
    expect(markup).toContain('<option value="work">Work</option>');
  });

  test("opens browser import as a Next-driven wizard before source selection", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        browserImportProfiles={[
          {
            browser_id: "helium",
            browser_name: "Helium",
            profile_name: "You",
            profile_path: "cmux-ui-test://browser-import/helium/You",
            importable_items: ["bookmarks", "history", "cookies"],
          },
        ]}
        onRefreshBrowserImportProfiles={noop}
        onStartBrowserImport={noop}
      />,
    );

    expect(markup).toContain("cmux is ready to import from");
    expect(markup).toContain("Helium");
    expect(markup).toContain(">Next</button>");
    expect(markup).not.toContain("BrowserImportCookiesCheckbox");
    expect(markup).not.toContain("Start Import");
  });

  test("renders an empty browser import detection state", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={{ app: makeApp() }}
        onChange={noop}
        browserImportProfiles={[]}
        onRefreshBrowserImportProfiles={noop}
      />,
    );

    expect(markup).toContain(
      "No importable Chrome, Edge, Brave, Chromium, or Firefox profiles were detected.",
    );
  });
});

// ---- Terminal settings ------------------------------------------------------

describe("SettingsPane terminal settings", () => {
  test("renders editable controls for terminal and TextBox config", () => {
    const config: Config = {
      terminal: makeTerminal({
        showScrollBar: true,
        scrollSpeed: 1.75,
        copyOnSelect: true,
        autoResumeAgentSessions: false,
        showTextBoxOnNewTerminals: true,
        focusTextBoxOnNewTerminals: true,
        agentHibernation: {
          enabled: true,
          idleSeconds: 60,
          maxLiveTerminals: 3,
        },
        rendererRealization: {
          enabled: false,
          idleSeconds: 45,
          maxWarmRenderers: 2,
        },
        textBoxMaxLines: 12,
        resumeCommands: [
          {
            version: 1,
            id: "approval-1",
            commandPrefix: ["pnpm", "dev"],
            environmentKeys: [],
            policy: "manual",
            createdAt: 1,
            updatedAt: 1,
            signature: "sig",
          },
        ],
      }),
    };
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={config}
        onChange={noop}
        onOpenGhosttySettingsFile={noop}
      />,
    );

    expect(markup).toContain("<h2>Terminal</h2>");
    expect(markup).toContain("Ghostty config files");
    expect(markup).toContain(
      '<button type="button">Open Ghostty Settings</button>',
    );
    expect(tagByAttr(markup, "data-terminal-field", "showScrollBar")).toContain(
      "checked",
    );
    expect(tagByAttr(markup, "data-terminal-field", "copyOnSelect")).toContain(
      "checked",
    );
    expect(
      tagByAttr(markup, "data-terminal-field", "autoResumeAgentSessions"),
    ).not.toContain("checked");
    expect(
      tagByAttr(markup, "data-terminal-field", "showTextBoxOnNewTerminals"),
    ).toContain("checked");
    expect(
      tagByAttr(markup, "data-terminal-field", "focusTextBoxOnNewTerminals"),
    ).toContain("checked");
    expect(tagByAttr(markup, "data-terminal-field", "scrollSpeed")).toContain(
      'value="1.75"',
    );
    expect(tagByAttr(markup, "data-terminal-field", "textBoxMaxLines")).toContain(
      'value="12"',
    );
    expect(
      tagByAttr(markup, "data-terminal-nested-field", "agentHibernation.enabled"),
    ).toContain("checked");
    expect(
      tagByAttr(markup, "data-terminal-nested-field", "rendererRealization.enabled"),
    ).not.toContain("checked");
    expect(
      tagByAttr(markup, "data-terminal-nested-field", "agentHibernation.idleSeconds"),
    ).toContain('value="60"');
    expect(
      tagByAttr(markup, "data-terminal-nested-field", "agentHibernation.maxLiveTerminals"),
    ).toContain('value="3"');
    expect(
      tagByAttr(markup, "data-terminal-nested-field", "rendererRealization.idleSeconds"),
    ).toContain('value="45"');
    expect(
      tagByAttr(markup, "data-terminal-nested-field", "rendererRealization.maxWarmRenderers"),
    ).toContain('value="2"');
    expect(markup).toContain('data-terminal-field="resumeCommands"');
    expect(markup).toContain(">1</span>");
  });
});

// ---- App controls -----------------------------------------------------------

describe("SettingsPane app settings", () => {
  test("renders editable controls for app preferences beyond appearance", () => {
    const config: Config = {
      app: makeApp({
        language: "pt-BR",
        appIcon: "dark",
        windowTitleTemplate: "{workspace}",
        newWorkspacePlacement: "top",
        forkConversationDefaultDestination: "newWorkspace",
        minimalMode: true,
        preferredEditor: "code",
        globalFontMagnification: 125,
        confirmQuit: "dirty-only",
        warnBeforeClosingTab: true,
      }),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);

    expect(markup).toContain("<h2>App</h2>");
    expect(markup).toContain('data-app-field="appIcon"');
    expect(markup).toContain('<option value="dark" selected="">Dark</option>');
    expect(markup).toContain('data-app-field="newWorkspacePlacement"');
    expect(markup).toContain('<option value="top" selected="">Top</option>');
    expect(markup).toContain('data-app-field="forkConversationDefaultDestination"');
    expect(markup).toContain(
      '<option value="newWorkspace" selected="">New workspace</option>',
    );
    expect(markup).toContain('data-app-field="confirmQuit"');
    expect(markup).toContain('<option value="dirty-only" selected="">Dirty only</option>');
    expect(tagByAttr(markup, "data-app-field", "language")).toContain(
      'value="pt-BR"',
    );
    expect(tagByAttr(markup, "data-app-field", "windowTitleTemplate")).toContain(
      'value="{workspace}"',
    );
    expect(tagByAttr(markup, "data-app-field", "preferredEditor")).toContain(
      'value="code"',
    );
    expect(tagByAttr(markup, "data-app-field", "globalFontMagnification")).toContain(
      'value="125"',
    );
    expect(tagByAttr(markup, "data-app-field", "minimalMode")).toContain("checked");
    expect(tagByAttr(markup, "data-app-field", "warnBeforeClosingTab")).toContain(
      "checked",
    );
  });

  test("renders native app integration status actions", () => {
    const config: Config = { app: makeApp() };
    const markup = renderToStaticMarkup(
      <SettingsPane
        config={config}
        onChange={noop}
        cliInstallStatus={{
          installed_in_path: true,
          shim_path: "C:\\Users\\A\\AppData\\Local\\cmux\\bin\\cmux.cmd",
          shim_directory: "C:\\Users\\A\\AppData\\Local\\cmux\\bin",
          bundled_cli_path: "C:\\Program Files\\cmux\\cmux.exe",
        }}
        desktopCoreStatus={{
          milestone: "M1",
          platform: "windows-m1-core",
          agent_providers: ["claude", "codex", "opencode"],
          ipc_fixture_request: '{"id":2,"method":"ping","params":{}}\n',
        }}
        defaultTerminalStatus={{
          is_default: false,
          command: '"C:\\Program Files\\cmux\\cmux.exe" "%1"',
        }}
        onRefreshCliInstallStatus={noop}
        onRefreshDesktopCoreStatus={noop}
        onUninstallCli={noop}
        onRefreshDefaultTerminalStatus={noop}
        onMakeDefaultTerminal={noop}
        onOpenTaskManager={noop}
        onRestorePreviousLaunch={noop}
      />,
    );

    expect(markup).toContain("Desktop core");
    expect(markup).toContain("windows-m1-core - M1");
    expect(markup).toContain("providers: claude, codex, opencode");
    expect(markup).toContain("cmux CLI in PATH");
    expect(markup).toContain("Installed at");
    expect(markup).toContain("cmux.cmd");
    expect(markup).toContain("Uninstall CLI");
    expect(markup).toContain("Default terminal for SSH links");
    expect(markup).toContain("cmux is not the SSH handler");
    expect(markup).toContain("Make Default");
    expect(markup).toContain("Task Manager");
    expect(markup).toContain("Open Task Manager");
    expect(markup).toContain("Previous launch");
    expect(markup).toContain("Restore Previous Launch");
  });

  test("renders editable controls for viewer and editor config slices", () => {
    const config: Config = {
      markdown: makeMarkdown({
        fontSize: 18,
        fontFamily: "Georgia",
        maxWidth: 720,
      }),
      canvas: makeCanvas({ paneGap: 24, snappingEnabled: false }),
      file_editor: makeFileEditor({ wordWrap: true }),
      file_explorer: makeFileExplorer({ doubleClickAction: "preferredEditor" }),
      diff_viewer: makeDiffViewer({ defaultLayout: "split" }),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);

    expect(markup).toContain("<h2>App</h2>");
    expect(tagByAttr(markup, "data-markdown-field", "fontFamily")).toContain(
      'value="Georgia"',
    );
    expect(tagByAttr(markup, "data-markdown-field", "fontSize")).toContain(
      'value="18"',
    );
    expect(tagByAttr(markup, "data-markdown-field", "maxWidth")).toContain(
      'value="720"',
    );
    expect(tagByAttr(markup, "data-canvas-field", "paneGap")).toContain(
      'value="24"',
    );
    expect(tagByAttr(markup, "data-canvas-field", "snappingEnabled")).not.toContain(
      "checked",
    );
    expect(tagByAttr(markup, "data-file-editor-field", "wordWrap")).toContain(
      "checked",
    );
    expect(markup).toContain('data-file-explorer-field="doubleClickAction"');
    expect(markup).toContain(
      '<option value="preferredEditor" selected="">Preferred editor</option>',
    );
    expect(markup).toContain('data-diff-viewer-field="defaultLayout"');
    expect(markup).toContain('<option value="split" selected="">Split</option>');
  });
});

// ---- Appearance radio: only the selected value is checked -------------------

describe("SettingsPane appearance radio", () => {
  test("selects exactly the configured appearance", () => {
    const config: Config = { app: makeApp({ appearance: "dark" }) };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);

    expect(tagByAttr(markup, "data-appearance", "dark")).toContain("checked");
    expect(tagByAttr(markup, "data-appearance", "light")).not.toContain("checked");
    expect(tagByAttr(markup, "data-appearance", "system")).not.toContain("checked");
  });

  test("selecting system checks only system", () => {
    const config: Config = { app: makeApp({ appearance: "system" }) };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);
    expect(tagByAttr(markup, "data-appearance", "system")).toContain("checked");
    expect(tagByAttr(markup, "data-appearance", "dark")).not.toContain("checked");
  });
});

// ---- Search -------------------------------------------------------------------

describe("SettingsPane search", () => {
  const fullConfig: Config = {
    sidebar: makeSidebar(),
    notifications: makeNotifications(),
    automation: makeAutomation(),
    browser: makeBrowser(),
    terminal: makeTerminal(),
    workspace_colors: makeWorkspaceColors(),
    markdown: makeMarkdown(),
    canvas: makeCanvas(),
    file_editor: makeFileEditor(),
    file_explorer: makeFileExplorer(),
    diff_viewer: makeDiffViewer(),
    app: makeApp(),
    shortcuts: makeShortcuts(),
  };
  const allHeadings = [
    "<h2>Account</h2>",
    "<h2>Mobile</h2>",
    "<h2>Updates</h2>",
    "<h2>Custom Sidebars</h2>",
    "<h2>Beta Features</h2>",
    "<h2>Automation</h2>",
    "<h2>Browser</h2>",
    "<h2>Import Browser Data</h2>",
    "<h2>Global Hotkey</h2>",
    "<h2>Terminal</h2>",
    "<h2>Workspace Colors</h2>",
    "<h2>Sidebar</h2>",
    "<h2>Notifications</h2>",
    "<h2>App</h2>",
    "<h2>Shortcuts</h2>",
    "<h2>cmux.json</h2>",
    "<h2>Reset</h2>",
  ];

  test.each([undefined, "", "   "])(
    "query %p renders the search input and the normal sections",
    (searchQuery) => {
      const markup = renderToStaticMarkup(
        <SettingsPane config={fullConfig} onChange={noop} searchQuery={searchQuery} />,
      );
      expect(markup).toContain("data-settings-search");
      for (const heading of allHeadings) {
        expect(markup).toContain(heading);
      }
      expect(markup).not.toContain("cmux-settings-search-results");
    },
  );

  test("a matching query replaces the sections with results", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane config={fullConfig} onChange={noop} searchQuery="dock badge" />,
    );
    expect(markup).toContain("cmux-settings-search-results");
    expect(markup).toContain('data-result-id="setting:app:dock-badge"');
    for (const heading of allHeadings) {
      expect(markup).not.toContain(heading);
    }
    // The hit renders its parent-section subtitle and a clickable button.
    const row = markup.match(
      /<li[^>]*data-result-id="setting:app:dock-badge"[^>]*>.*?<\/li>/,
    )?.[0];
    expect(row).toBeDefined();
    expect(row).toContain(
      '<span class="cmux-settings-search-result-section">App</span>',
    );
    expect(row).not.toContain("disabled");
  });

  test("results keep producer order; terminal targets are navigable", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane config={fullConfig} onChange={noop} searchQuery="copy on select" />,
    );
    const firstResultId = markup.match(/data-result-id="([^"]+)"/)?.[1];
    expect(firstResultId).toBe("setting:terminal:copy-on-select");
    const row = markup.match(
      /<li[^>]*data-result-id="setting:terminal:copy-on-select"[^>]*>.*?<\/li>/,
    )?.[0];
    expect(row).toBeDefined();
    expect(row).not.toContain("disabled");
  });

  test("stop-words-only query lists the 16 sections without subtitles", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane config={fullConfig} onChange={noop} searchQuery="settings" />,
    );
    expect(markup.match(/<li /g)).toHaveLength(16);
    expect(markup).toContain('data-result-id="section:account"');
    expect(markup.match(/data-result-id="section:account"[^>]*>.*?<button/g)?.[0]).not.toContain(
      "disabled",
    );
    expect(markup).toContain('data-result-id="section:reset"');
    expect(markup.match(/data-result-id="section:reset"[^>]*>.*?<button/g)?.[0]).not.toContain(
      "disabled",
    );
    expect(markup).not.toContain("cmux-settings-search-result-section");
  });

  test("no matches renders the canonical No Results text", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane config={fullConfig} onChange={noop} searchQuery="zzzzqqqq" />,
    );
    expect(markup).toContain("No Results");
    expect(markup).not.toContain("<li ");
  });
});

// ---- Shortcuts list ---------------------------------------------------------

describe("SettingsPane shortcuts list", () => {
  /** The rendered binding span for the given action id. */
  function bindingSpan(markup: string, actionId: string): string | undefined {
    return markup.match(
      new RegExp(
        `<li[^>]*data-action="${actionId}"[^>]*>.*?` +
          `<span class="cmux-settings-shortcut-binding">(.*?)</span>`,
      ),
    )?.[1];
  }

  test("lists each binding's action id and canonical display string", () => {
    const config: Config = {
      shortcuts: makeShortcuts({
        bindings: {
          "workspace.new": "cmd+t",
          "workspace.close": ["cmd", "w"],
          "workspace.rename": null,
          selectSurfaceByNumber: "ctrl+1",
        },
      }),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);

    expect(bindingSpan(markup, "workspace.new")).toBe("⌘T");
    // ["cmd", "w"] is canonically INVALID (each array element is one whole
    // stroke; "cmd" alone is not a key) → raw fallback joined with a space.
    expect(bindingSpan(markup, "workspace.close")).toBe("cmd w");
    // An explicit null binding is the canonical unbound marker.
    expect(bindingSpan(markup, "workspace.rename")).toBe("None");
    // Numbered actions render the digit as the 1…9 range.
    expect(bindingSpan(markup, "selectSurfaceByNumber")).toBe("⌃1…9");
  });

  test("renders editable shortcut controls backed by raw config values", () => {
    const config: Config = {
      shortcuts: makeShortcuts({
        bindings: {
          "workspace.new": "cmd+t",
          "workspace.close": ["cmd+k", "cmd+w"],
          "workspace.rename": null,
        },
      }),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);

    expect(markup).toContain('data-shortcut-action="workspace.new"');
    expect(tagByAttr(markup, "data-shortcut-action", "workspace.new")).toContain(
      'value="cmd+t"',
    );
    expect(tagByAttr(markup, "data-shortcut-action", "workspace.close")).toContain(
      'value="cmd+k, cmd+w"',
    );
    expect(tagByAttr(markup, "data-shortcut-action", "workspace.rename")).toContain(
      'placeholder="None"',
    );
    expect(markup).toContain('data-shortcut-clear="workspace.new"');
  });
});
