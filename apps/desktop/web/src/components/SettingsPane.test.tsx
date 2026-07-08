import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type {
  AppConfig,
  Config,
  NotificationsConfig,
  ShortcutsConfig,
  SidebarConfig,
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
      app: makeApp(),
      shortcuts: makeShortcuts(),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);
    expect(markup).toContain("<h2>Sidebar</h2>");
    expect(markup).toContain("<h2>Notifications</h2>");
    expect(markup).toContain("<h2>Appearance</h2>");
    expect(markup).toContain("<h2>Shortcuts</h2>");
  });

  test("omits a section whose config slice is absent", () => {
    const config: Config = { app: makeApp() };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);
    expect(markup).toContain("<h2>Appearance</h2>");
    expect(markup).not.toContain("<h2>Sidebar</h2>");
    expect(markup).not.toContain("<h2>Notifications</h2>");
    expect(markup).not.toContain("<h2>Shortcuts</h2>");
  });
});

// ---- Sidebar toggles: checked-state reflects the fixture --------------------

describe("SettingsPane sidebar toggles", () => {
  test("checkbox checked-state mirrors each fixture flag", () => {
    const config: Config = {
      sidebar: makeSidebar({ showPorts: true, showLog: false, hideAllDetails: true }),
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
  });
});

// ---- Notifications toggles --------------------------------------------------

describe("SettingsPane notifications toggles", () => {
  test("checkbox checked-state mirrors each fixture flag", () => {
    const config: Config = {
      notifications: makeNotifications({ dockBadge: true, paneFlash: false }),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);
    expect(tagByAttr(markup, "data-field", "dockBadge")).toContain("checked");
    expect(tagByAttr(markup, "data-field", "paneFlash")).not.toContain("checked");
    expect(markup).toContain("Unread pane ring");
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
    app: makeApp(),
    shortcuts: makeShortcuts(),
  };
  const allHeadings = [
    "<h2>Sidebar</h2>",
    "<h2>Notifications</h2>",
    "<h2>Appearance</h2>",
    "<h2>Shortcuts</h2>",
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

  test("results keep producer order; unmapped targets are disabled", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane config={fullConfig} onChange={noop} searchQuery="copy on select" />,
    );
    const firstResultId = markup.match(/data-result-id="([^"]+)"/)?.[1];
    expect(firstResultId).toBe("setting:terminal:copy-on-select");
    const row = markup.match(
      /<li[^>]*data-result-id="setting:terminal:copy-on-select"[^>]*>.*?<\/li>/,
    )?.[0];
    expect(row).toBeDefined();
    expect(row).toContain("disabled");
  });

  test("stop-words-only query lists the 16 sections without subtitles", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane config={fullConfig} onChange={noop} searchQuery="settings" />,
    );
    expect(markup.match(/<li /g)).toHaveLength(16);
    expect(markup).toContain('data-result-id="section:account"');
    expect(markup).toContain('data-result-id="section:reset"');
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
  test("lists each binding's action id and formatted keys", () => {
    const config: Config = {
      shortcuts: makeShortcuts({
        bindings: {
          "workspace.new": "cmd+t",
          "workspace.close": ["cmd", "w"],
          "workspace.rename": null,
        },
      }),
    };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onChange={noop} />);

    expect(markup).toContain('data-action="workspace.new"');
    expect(markup).toContain("cmd+t");
    expect(markup).toContain('data-action="workspace.close"');
    expect(markup).toContain("cmd w"); // chord joined with a space
    // An explicit null binding renders as "Unbound".
    expect(markup).toContain('data-action="workspace.rename"');
    expect(markup).toContain("Unbound");
  });
});
