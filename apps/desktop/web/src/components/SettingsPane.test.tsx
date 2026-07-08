import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";

import type {
  AppConfig,
  Config,
  NotificationsConfig,
  ShortcutsConfig,
  SidebarConfig,
} from "@cmux/core-types";

import { SettingsPane, SettingsSearchResults } from "./SettingsPane";
import { settingsEntriesMatching } from "../settings/settingsSearch";

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
    const markup = renderToStaticMarkup(<SettingsPane config={config} onAction={noop} />);
    expect(markup).toContain("<h2>Sidebar</h2>");
    expect(markup).toContain("<h2>Notifications</h2>");
    expect(markup).toContain("<h2>Appearance</h2>");
    expect(markup).toContain("<h2>Shortcuts</h2>");
  });

  test("omits a section whose config slice is absent", () => {
    const config: Config = { app: makeApp() };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onAction={noop} />);
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
    const markup = renderToStaticMarkup(<SettingsPane config={config} onAction={noop} />);

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
    const markup = renderToStaticMarkup(<SettingsPane config={config} onAction={noop} />);
    expect(tagByAttr(markup, "data-field", "dockBadge")).toContain("checked");
    expect(tagByAttr(markup, "data-field", "paneFlash")).not.toContain("checked");
    expect(markup).toContain("Unread pane ring");
  });
});

// ---- Appearance radio: only the selected value is checked -------------------

describe("SettingsPane appearance radio", () => {
  test("selects exactly the configured appearance", () => {
    const config: Config = { app: makeApp({ appearance: "dark" }) };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onAction={noop} />);

    expect(tagByAttr(markup, "data-appearance", "dark")).toContain("checked");
    expect(tagByAttr(markup, "data-appearance", "light")).not.toContain("checked");
    expect(tagByAttr(markup, "data-appearance", "system")).not.toContain("checked");
  });

  test("selecting system checks only system", () => {
    const config: Config = { app: makeApp({ appearance: "system" }) };
    const markup = renderToStaticMarkup(<SettingsPane config={config} onAction={noop} />);
    expect(tagByAttr(markup, "data-appearance", "system")).toContain("checked");
    expect(tagByAttr(markup, "data-appearance", "dark")).not.toContain("checked");
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
    const markup = renderToStaticMarkup(<SettingsPane config={config} onAction={noop} />);

    expect(markup).toContain('data-action="workspace.new"');
    expect(markup).toContain("cmd+t");
    expect(markup).toContain('data-action="workspace.close"');
    expect(markup).toContain("cmd w"); // chord joined with a space
    // An explicit null binding renders as "Unbound".
    expect(markup).toContain('data-action="workspace.rename"');
    expect(markup).toContain("Unbound");
  });
});

describe("SettingsSearchResults", () => {
  test("renders ranked canonical entries from the search corpus", () => {
    const entries = settingsEntriesMatching("appearance");
    expect(entries.length).toBeGreaterThan(0);
    const markup = renderToStaticMarkup(
      <SettingsSearchResults entries={entries} />,
    );
    expect(markup).toContain("cmux-settings-search-result");
    expect(markup).toContain("data-target=");
  });

  test("shows the empty state when nothing matches", () => {
    const markup = renderToStaticMarkup(<SettingsSearchResults entries={[]} />);
    expect(markup).toContain("No matching settings");
  });

  test("the pane renders the search input", () => {
    const markup = renderToStaticMarkup(
      <SettingsPane config={{}} onAction={() => {}} />,
    );
    expect(markup).toContain("cmux-settings-search-input");
  });
});

