import { describe, expect, test } from "bun:test";

import type { Config } from "@cmux/core-types";

import { configMutationFromAction } from "./configMutation";
import { defaultSettingsConfig } from "./defaultConfig";

describe("configMutationFromAction", () => {
  test("sidebar toggles remove the key when the next value equals the default", () => {
    const next = defaultSettingsConfig();
    next.sidebar!.showPorts = true;
    expect(
      configMutationFromAction(next, { type: "toggleSidebarFlag", key: "showPorts" }),
    ).toEqual({ path: "sidebar.showPorts", remove: true });
  });

  test("sidebar toggles set the key when the next value differs from the default", () => {
    const next = defaultSettingsConfig();
    next.sidebar!.showPorts = false;
    expect(
      configMutationFromAction(next, { type: "toggleSidebarFlag", key: "showPorts" }),
    ).toEqual({ path: "sidebar.showPorts", value: false });
  });

  test("notifications toggles follow the same default-aware remove policy", () => {
    const next = defaultSettingsConfig();
    next.notifications!.showInMenuBar = false;
    expect(
      configMutationFromAction(next, {
        type: "toggleNotificationsFlag",
        key: "showInMenuBar",
      }),
    ).toEqual({ path: "notifications.showInMenuBar", value: false });

    next.notifications!.showInMenuBar = true;
    expect(
      configMutationFromAction(next, {
        type: "toggleNotificationsFlag",
        key: "showInMenuBar",
      }),
    ).toEqual({ path: "notifications.showInMenuBar", remove: true });
  });

  test("notification string edits save or remove individual notification keys", () => {
    const next = defaultSettingsConfig();
    next.notifications!.sound = "custom_file";
    expect(
      configMutationFromAction(next, {
        type: "setNotificationsString",
        key: "sound",
        value: "custom_file",
      }),
    ).toEqual({ path: "notifications.sound", value: "custom_file" });

    next.notifications!.sound = "default";
    expect(
      configMutationFromAction(next, {
        type: "setNotificationsString",
        key: "sound",
        value: "default",
      }),
    ).toEqual({ path: "notifications.sound", remove: true });
  });

  test("notification select edits save or remove individual notification keys", () => {
    const next = defaultSettingsConfig();
    next.notifications!.hooksMode = "replace";
    expect(
      configMutationFromAction(next, {
        type: "setNotificationsSelect",
        key: "hooksMode",
        value: "replace",
      }),
    ).toEqual({ path: "notifications.hooksMode", value: "replace" });

    next.notifications!.hooksMode = "append";
    expect(
      configMutationFromAction(next, {
        type: "setNotificationsSelect",
        key: "hooksMode",
        value: "append",
      }),
    ).toEqual({ path: "notifications.hooksMode", remove: true });
  });

  test("app boolean edits save or remove individual app keys", () => {
    const next = defaultSettingsConfig();
    next.app!.minimalMode = true;
    expect(
      configMutationFromAction(next, {
        type: "toggleAppFlag",
        key: "minimalMode",
      }),
    ).toEqual({ path: "app.minimalMode", value: true });

    next.app!.minimalMode = false;
    expect(
      configMutationFromAction(next, {
        type: "toggleAppFlag",
        key: "minimalMode",
      }),
    ).toEqual({ path: "app.minimalMode", remove: true });
  });

  test("app string edits save or remove individual app keys", () => {
    const next = defaultSettingsConfig();
    next.app!.preferredEditor = "code";
    expect(
      configMutationFromAction(next, {
        type: "setAppString",
        key: "preferredEditor",
        value: "code",
      }),
    ).toEqual({ path: "app.preferredEditor", value: "code" });

    next.app!.preferredEditor = "";
    expect(
      configMutationFromAction(next, {
        type: "setAppString",
        key: "preferredEditor",
        value: "",
      }),
    ).toEqual({ path: "app.preferredEditor", remove: true });
  });

  test("app number edits save or remove individual app keys", () => {
    const next = defaultSettingsConfig();
    next.app!.globalFontMagnification = 125;
    expect(
      configMutationFromAction(next, {
        type: "setAppNumber",
        key: "globalFontMagnification",
        value: 125,
      }),
    ).toEqual({ path: "app.globalFontMagnification", value: 125 });

    next.app!.globalFontMagnification = 100;
    expect(
      configMutationFromAction(next, {
        type: "setAppNumber",
        key: "globalFontMagnification",
        value: 100,
      }),
    ).toEqual({ path: "app.globalFontMagnification", remove: true });
  });

  test("app select edits save or remove individual app keys", () => {
    const next = defaultSettingsConfig();
    next.app!.confirmQuit = "never";
    expect(
      configMutationFromAction(next, {
        type: "setAppSelect",
        key: "confirmQuit",
        value: "never",
      }),
    ).toEqual({ path: "app.confirmQuit", value: "never" });

    next.app!.confirmQuit = "always";
    expect(
      configMutationFromAction(next, {
        type: "setAppSelect",
        key: "confirmQuit",
        value: "always",
      }),
    ).toEqual({ path: "app.confirmQuit", remove: true });
  });

  test("automation boolean edits save or remove individual automation keys", () => {
    const next = defaultSettingsConfig();
    next.automation!.workspaceAutoNaming = true;
    expect(
      configMutationFromAction(next, {
        type: "toggleAutomationFlag",
        key: "workspaceAutoNaming",
      }),
    ).toEqual({ path: "automation.workspaceAutoNaming", value: true });

    next.automation!.workspaceAutoNaming = false;
    expect(
      configMutationFromAction(next, {
        type: "toggleAutomationFlag",
        key: "workspaceAutoNaming",
      }),
    ).toEqual({ path: "automation.workspaceAutoNaming", remove: true });
  });

  test("automation string edits save or remove individual automation keys", () => {
    const next = defaultSettingsConfig();
    next.automation!.socketControlMode = "password";
    expect(
      configMutationFromAction(next, {
        type: "setAutomationString",
        key: "socketControlMode",
        value: "password",
      }),
    ).toEqual({ path: "automation.socketControlMode", value: "password" });

    next.automation!.socketControlMode = "cmuxOnly";
    expect(
      configMutationFromAction(next, {
        type: "setAutomationString",
        key: "socketControlMode",
        value: "cmuxOnly",
      }),
    ).toEqual({ path: "automation.socketControlMode", remove: true });
  });

  test("automation password edits use the serde socketPassword key", () => {
    const next = defaultSettingsConfig();
    next.automation!.socket_password = "hunter2";
    expect(
      configMutationFromAction(next, {
        type: "setAutomationPassword",
        key: "socket_password",
        value: "hunter2",
      }),
    ).toEqual({ path: "automation.socketPassword", value: "hunter2" });

    next.automation!.socket_password = undefined;
    expect(
      configMutationFromAction(next, {
        type: "setAutomationPassword",
        key: "socket_password",
        value: null,
      }),
    ).toEqual({ path: "automation.socketPassword", remove: true });
  });

  test("automation number and select edits save or remove keys", () => {
    const next = defaultSettingsConfig();
    next.automation!.portBase = 9200;
    expect(
      configMutationFromAction(next, {
        type: "setAutomationNumber",
        key: "portBase",
        value: 9200,
      }),
    ).toEqual({ path: "automation.portBase", value: 9200 });

    next.automation!.portBase = 9100;
    expect(
      configMutationFromAction(next, {
        type: "setAutomationNumber",
        key: "portBase",
        value: 9100,
      }),
    ).toEqual({ path: "automation.portBase", remove: true });

    next.automation!.kiroNotificationLevel = "verbose";
    expect(
      configMutationFromAction(next, {
        type: "setAutomationSelect",
        key: "kiroNotificationLevel",
        value: "verbose",
      }),
    ).toEqual({ path: "automation.kiroNotificationLevel", value: "verbose" });

    next.automation!.kiroNotificationLevel = "standard";
    expect(
      configMutationFromAction(next, {
        type: "setAutomationSelect",
        key: "kiroNotificationLevel",
        value: "standard",
      }),
    ).toEqual({ path: "automation.kiroNotificationLevel", remove: true });
  });

  test("appearance removes the key when reset to system", () => {
    const next = defaultSettingsConfig();
    next.app!.appearance = "dark";
    expect(
      configMutationFromAction(next, { type: "setAppearance", appearance: "dark" }),
    ).toEqual({ path: "app.appearance", value: "dark" });

    next.app!.appearance = "system";
    expect(
      configMutationFromAction(next, { type: "setAppearance", appearance: "system" }),
    ).toEqual({ path: "app.appearance", remove: true });
  });

  test("minimal mode follows the default-aware remove policy", () => {
    const next = defaultSettingsConfig();
    next.app!.minimalMode = true;
    expect(
      configMutationFromAction(next, { type: "setMinimalMode", enabled: true }),
    ).toEqual({ path: "app.minimalMode", value: true });

    next.app!.minimalMode = false;
    expect(
      configMutationFromAction(next, { type: "setMinimalMode", enabled: false }),
    ).toEqual({ path: "app.minimalMode", remove: true });
  });

  test("match-terminal-background follows the default-aware remove policy", () => {
    const next = defaultSettingsConfig();
    next.sidebar_appearance!.matchTerminalBackground = true;
    expect(
      configMutationFromAction(next, { type: "toggleMatchTerminalBackground" }),
    ).toEqual({
      path: "sidebarAppearance.matchTerminalBackground",
      value: true,
    });

    next.sidebar_appearance!.matchTerminalBackground = false;
    expect(
      configMutationFromAction(next, { type: "toggleMatchTerminalBackground" }),
    ).toEqual({
      path: "sidebarAppearance.matchTerminalBackground",
      remove: true,
    });
  });

  test("sidebar select edits save or remove individual sidebar keys", () => {
    const next = defaultSettingsConfig();
    next.sidebar!.branchLayout = "inline";
    expect(
      configMutationFromAction(next, {
        type: "setSidebarSelect",
        key: "branchLayout",
        value: "inline",
      }),
    ).toEqual({ path: "sidebar.branchLayout", value: "inline" });

    next.sidebar!.branchLayout = "vertical";
    expect(
      configMutationFromAction(next, {
        type: "setSidebarSelect",
        key: "branchLayout",
        value: "vertical",
      }),
    ).toEqual({ path: "sidebar.branchLayout", remove: true });
  });

  test("sidebar appearance string edits save or remove individual appearance keys", () => {
    const next = defaultSettingsConfig();
    next.sidebar_appearance!.tintColor = "#123456";
    expect(
      configMutationFromAction(next, {
        type: "setSidebarAppearanceString",
        key: "tintColor",
        value: "#123456",
      }),
    ).toEqual({ path: "sidebarAppearance.tintColor", value: "#123456" });

    next.sidebar_appearance!.tintColor = "#000000";
    expect(
      configMutationFromAction(next, {
        type: "setSidebarAppearanceString",
        key: "tintColor",
        value: "#000000",
      }),
    ).toEqual({ path: "sidebarAppearance.tintColor", remove: true });
  });

  test("sidebar appearance number edits save or remove individual appearance keys", () => {
    const next = defaultSettingsConfig();
    next.sidebar_appearance!.tintOpacity = 0.2;
    expect(
      configMutationFromAction(next, {
        type: "setSidebarAppearanceNumber",
        key: "tintOpacity",
        value: 0.2,
      }),
    ).toEqual({ path: "sidebarAppearance.tintOpacity", value: 0.2 });

    next.sidebar_appearance!.tintOpacity = 0.03;
    expect(
      configMutationFromAction(next, {
        type: "setSidebarAppearanceNumber",
        key: "tintOpacity",
        value: 0.03,
      }),
    ).toEqual({ path: "sidebarAppearance.tintOpacity", remove: true });
  });

  test("browser enablement saves the full browser section when disabling", () => {
    const next = defaultSettingsConfig();
    next.browser!.openTerminalLinksInCmuxBrowser = false;
    next.browser!.interceptTerminalOpenCommandInCmuxBrowser = false;
    expect(
      configMutationFromAction(next, { type: "setBrowserEnabled", enabled: false }),
    ).toEqual({ path: "browser", value: next.browser });
  });

  test("browser flag toggles save or remove individual browser keys", () => {
    const next = defaultSettingsConfig();
    next.browser!.showSearchSuggestions = false;
    expect(
      configMutationFromAction(next, {
        type: "toggleBrowserFlag",
        key: "showSearchSuggestions",
      }),
    ).toEqual({ path: "browser.showSearchSuggestions", value: false });

    next.browser!.showSearchSuggestions = true;
    expect(
      configMutationFromAction(next, {
        type: "toggleBrowserFlag",
        key: "showSearchSuggestions",
      }),
    ).toEqual({ path: "browser.showSearchSuggestions", remove: true });
  });

  test("browser string edits save or remove individual browser keys", () => {
    const next = defaultSettingsConfig();
    next.browser!.defaultSearchEngine = "duckduckgo";
    expect(
      configMutationFromAction(next, {
        type: "setBrowserString",
        key: "defaultSearchEngine",
        value: "duckduckgo",
      }),
    ).toEqual({ path: "browser.defaultSearchEngine", value: "duckduckgo" });

    next.browser!.defaultSearchEngine = "google";
    expect(
      configMutationFromAction(next, {
        type: "setBrowserString",
        key: "defaultSearchEngine",
        value: "google",
      }),
    ).toEqual({ path: "browser.defaultSearchEngine", remove: true });
  });

  test("browser theme edits save or remove the theme key", () => {
    const next = defaultSettingsConfig();
    next.browser!.theme = "dark";
    expect(
      configMutationFromAction(next, { type: "setBrowserTheme", theme: "dark" }),
    ).toEqual({ path: "browser.theme", value: "dark" });

    next.browser!.theme = "system";
    expect(
      configMutationFromAction(next, { type: "setBrowserTheme", theme: "system" }),
    ).toEqual({ path: "browser.theme", remove: true });
  });

  test("browser number edits save or remove individual browser keys", () => {
    const next = defaultSettingsConfig();
    next.browser!.hiddenWebViewDiscardDelaySeconds = 60;
    expect(
      configMutationFromAction(next, {
        type: "setBrowserNumber",
        key: "hiddenWebViewDiscardDelaySeconds",
        value: 60,
      }),
    ).toEqual({ path: "browser.hiddenWebViewDiscardDelaySeconds", value: 60 });

    next.browser!.hiddenWebViewDiscardDelaySeconds = 300;
    expect(
      configMutationFromAction(next, {
        type: "setBrowserNumber",
        key: "hiddenWebViewDiscardDelaySeconds",
        value: 300,
      }),
    ).toEqual({ path: "browser.hiddenWebViewDiscardDelaySeconds", remove: true });
  });

  test("browser list edits save or remove individual browser keys", () => {
    const next = defaultSettingsConfig();
    next.browser!.hostsToOpenInEmbeddedBrowser = ["*.internal.test"];
    expect(
      configMutationFromAction(next, {
        type: "setBrowserStringList",
        key: "hostsToOpenInEmbeddedBrowser",
        value: ["*.internal.test"],
      }),
    ).toEqual({
      path: "browser.hostsToOpenInEmbeddedBrowser",
      value: ["*.internal.test"],
    });

    next.browser!.hostsToOpenInEmbeddedBrowser = [];
    expect(
      configMutationFromAction(next, {
        type: "setBrowserStringList",
        key: "hostsToOpenInEmbeddedBrowser",
        value: [],
      }),
    ).toEqual({ path: "browser.hostsToOpenInEmbeddedBrowser", remove: true });
  });

  test("browser enablement removes the section when it returns to defaults", () => {
    const next = defaultSettingsConfig();
    expect(
      configMutationFromAction(next, { type: "setBrowserEnabled", enabled: true }),
    ).toEqual({ path: "browser", remove: true });
  });

  test("browser enablement preserves other custom browser settings", () => {
    const next = defaultSettingsConfig();
    next.browser!.defaultSearchEngine = "duckduckgo";
    expect(
      configMutationFromAction(next, { type: "setBrowserEnabled", enabled: true }),
    ).toEqual({ path: "browser", value: next.browser });
  });

  test("terminal flag edits save or remove individual terminal keys", () => {
    const next = defaultSettingsConfig();
    next.terminal!.copyOnSelect = true;
    expect(
      configMutationFromAction(next, {
        type: "toggleTerminalFlag",
        key: "copyOnSelect",
      }),
    ).toEqual({ path: "terminal.copyOnSelect", value: true });

    next.terminal!.copyOnSelect = false;
    expect(
      configMutationFromAction(next, {
        type: "toggleTerminalFlag",
        key: "copyOnSelect",
      }),
    ).toEqual({ path: "terminal.copyOnSelect", remove: true });
  });

  test("terminal number edits save or remove individual terminal keys", () => {
    const next = defaultSettingsConfig();
    next.terminal!.scrollSpeed = 1.5;
    expect(
      configMutationFromAction(next, {
        type: "setTerminalNumber",
        key: "scrollSpeed",
        value: 1.5,
      }),
    ).toEqual({ path: "terminal.scrollSpeed", value: 1.5 });

    next.terminal!.scrollSpeed = 1;
    expect(
      configMutationFromAction(next, {
        type: "setTerminalNumber",
        key: "scrollSpeed",
        value: 1,
      }),
    ).toEqual({ path: "terminal.scrollSpeed", remove: true });
  });

  test("terminal nested flag edits save or remove nested terminal keys", () => {
    const next = defaultSettingsConfig();
    next.terminal!.agentHibernation.enabled = true;
    expect(
      configMutationFromAction(next, {
        type: "toggleTerminalNestedFlag",
        section: "agentHibernation",
        key: "enabled",
      }),
    ).toEqual({
      path: "terminal.agentHibernation.enabled",
      value: true,
    });

    next.terminal!.agentHibernation.enabled = false;
    expect(
      configMutationFromAction(next, {
        type: "toggleTerminalNestedFlag",
        section: "agentHibernation",
        key: "enabled",
      }),
    ).toEqual({
      path: "terminal.agentHibernation.enabled",
      remove: true,
    });
  });

  test("terminal nested number edits save or remove nested terminal keys", () => {
    const next = defaultSettingsConfig();
    next.terminal!.rendererRealization.maxWarmRenderers = 4;
    expect(
      configMutationFromAction(next, {
        type: "setTerminalNestedNumber",
        section: "rendererRealization",
        key: "maxWarmRenderers",
        value: 4,
      }),
    ).toEqual({
      path: "terminal.rendererRealization.maxWarmRenderers",
      value: 4,
    });

    next.terminal!.rendererRealization.maxWarmRenderers = 12;
    expect(
      configMutationFromAction(next, {
        type: "setTerminalNestedNumber",
        section: "rendererRealization",
        key: "maxWarmRenderers",
        value: 12,
      }),
    ).toEqual({
      path: "terminal.rendererRealization.maxWarmRenderers",
      remove: true,
    });
  });

  test("markdown edits save or remove individual markdown keys", () => {
    const next = defaultSettingsConfig();
    next.markdown!.fontFamily = "Georgia";
    expect(
      configMutationFromAction(next, {
        type: "setMarkdownString",
        key: "fontFamily",
        value: "Georgia",
      }),
    ).toEqual({ path: "markdown.fontFamily", value: "Georgia" });

    next.markdown!.fontFamily = "";
    expect(
      configMutationFromAction(next, {
        type: "setMarkdownString",
        key: "fontFamily",
        value: "",
      }),
    ).toEqual({ path: "markdown.fontFamily", remove: true });

    next.markdown!.maxWidth = 720;
    expect(
      configMutationFromAction(next, {
        type: "setMarkdownNumber",
        key: "maxWidth",
        value: 720,
      }),
    ).toEqual({ path: "markdown.maxWidth", value: 720 });

    next.markdown!.maxWidth = 980;
    expect(
      configMutationFromAction(next, {
        type: "setMarkdownNumber",
        key: "maxWidth",
        value: 980,
      }),
    ).toEqual({ path: "markdown.maxWidth", remove: true });
  });

  test("canvas edits save or remove individual canvas keys", () => {
    const next = defaultSettingsConfig();
    next.canvas!.snappingEnabled = false;
    expect(
      configMutationFromAction(next, {
        type: "toggleCanvasFlag",
        key: "snappingEnabled",
      }),
    ).toEqual({ path: "canvas.snappingEnabled", value: false });

    next.canvas!.snappingEnabled = true;
    expect(
      configMutationFromAction(next, {
        type: "toggleCanvasFlag",
        key: "snappingEnabled",
      }),
    ).toEqual({ path: "canvas.snappingEnabled", remove: true });

    next.canvas!.paneGap = 24;
    expect(
      configMutationFromAction(next, {
        type: "setCanvasNumber",
        key: "paneGap",
        value: 24,
      }),
    ).toEqual({ path: "canvas.paneGap", value: 24 });

    next.canvas!.paneGap = 16;
    expect(
      configMutationFromAction(next, {
        type: "setCanvasNumber",
        key: "paneGap",
        value: 16,
      }),
    ).toEqual({ path: "canvas.paneGap", remove: true });
  });

  test("file editor, file explorer, and diff viewer edits save or remove keys", () => {
    const next = defaultSettingsConfig();
    next.file_editor!.wordWrap = true;
    expect(
      configMutationFromAction(next, {
        type: "toggleFileEditorFlag",
        key: "wordWrap",
      }),
    ).toEqual({ path: "fileEditor.wordWrap", value: true });

    next.file_editor!.wordWrap = false;
    expect(
      configMutationFromAction(next, {
        type: "toggleFileEditorFlag",
        key: "wordWrap",
      }),
    ).toEqual({ path: "fileEditor.wordWrap", remove: true });

    next.file_explorer!.doubleClickAction = "preferredEditor";
    expect(
      configMutationFromAction(next, {
        type: "setFileExplorerSelect",
        key: "doubleClickAction",
        value: "preferredEditor",
      }),
    ).toEqual({
      path: "fileExplorer.doubleClickAction",
      value: "preferredEditor",
    });

    next.file_explorer!.doubleClickAction = "preview";
    expect(
      configMutationFromAction(next, {
        type: "setFileExplorerSelect",
        key: "doubleClickAction",
        value: "preview",
      }),
    ).toEqual({ path: "fileExplorer.doubleClickAction", remove: true });

    next.diff_viewer!.defaultLayout = "split";
    expect(
      configMutationFromAction(next, {
        type: "setDiffViewerSelect",
        key: "defaultLayout",
        value: "split",
      }),
    ).toEqual({ path: "diffViewer.defaultLayout", value: "split" });

    next.diff_viewer!.defaultLayout = "unified";
    expect(
      configMutationFromAction(next, {
        type: "setDiffViewerSelect",
        key: "defaultLayout",
        value: "unified",
      }),
    ).toEqual({ path: "diffViewer.defaultLayout", remove: true });
  });

  test("workspace color scalar edits save or remove individual keys", () => {
    const next = defaultSettingsConfig();
    next.workspace_colors!.indicatorStyle = "dot";
    expect(
      configMutationFromAction(next, {
        type: "setWorkspaceColorsString",
        key: "indicatorStyle",
        value: "dot",
      }),
    ).toEqual({ path: "workspaceColors.indicatorStyle", value: "dot" });

    next.workspace_colors!.indicatorStyle = "leftRail";
    expect(
      configMutationFromAction(next, {
        type: "setWorkspaceColorsString",
        key: "indicatorStyle",
        value: "leftRail",
      }),
    ).toEqual({ path: "workspaceColors.indicatorStyle", remove: true });
  });

  test("workspace color nullable edits save or remove individual keys", () => {
    const next = defaultSettingsConfig();
    next.workspace_colors!.selectionColor = "#123456";
    expect(
      configMutationFromAction(next, {
        type: "setWorkspaceColorsNullableString",
        key: "selectionColor",
        value: "#123456",
      }),
    ).toEqual({ path: "workspaceColors.selectionColor", value: "#123456" });

    next.workspace_colors!.selectionColor = null;
    expect(
      configMutationFromAction(next, {
        type: "setWorkspaceColorsNullableString",
        key: "selectionColor",
        value: null,
      }),
    ).toEqual({ path: "workspaceColors.selectionColor", remove: true });
  });

  test("workspace color maps and lists save or remove values", () => {
    const next = defaultSettingsConfig();
    next.workspace_colors!.paletteOverrides = { Blue: "#0f62fe" };
    expect(
      configMutationFromAction(next, {
        type: "setWorkspaceColorsMap",
        key: "paletteOverrides",
        value: { Blue: "#0f62fe" },
      }),
    ).toEqual({
      path: "workspaceColors.paletteOverrides",
      value: { Blue: "#0f62fe" },
    });

    next.workspace_colors!.paletteOverrides = {};
    expect(
      configMutationFromAction(next, {
        type: "setWorkspaceColorsMap",
        key: "paletteOverrides",
        value: {},
      }),
    ).toEqual({ path: "workspaceColors.paletteOverrides", remove: true });

    next.workspace_colors!.customColors = ["#111111"];
    expect(
      configMutationFromAction(next, {
        type: "setWorkspaceColorsList",
        key: "customColors",
        value: ["#111111"],
      }),
    ).toEqual({
      path: "workspaceColors.customColors",
      value: ["#111111"],
    });

    next.workspace_colors!.customColors = [];
    expect(
      configMutationFromAction(next, {
        type: "setWorkspaceColorsList",
        key: "customColors",
        value: [],
      }),
    ).toEqual({ path: "workspaceColors.customColors", remove: true });
  });

  test("shortcut binding edits use an escaped path for literal-dot action ids", () => {
    const next: Config = defaultSettingsConfig();
    next.shortcuts!.bindings["workspace.new"] = "cmd+t";
    expect(
      configMutationFromAction(next, {
        type: "setShortcutBinding",
        action: "workspace.new",
        binding: "cmd+t",
      }),
    ).toEqual({
      path: 'shortcuts.bindings["workspace.new"]',
      value: "cmd+t",
    });
  });

  test("shortcut binding edits preserve explicit null unbindings", () => {
    const next: Config = defaultSettingsConfig();
    next.shortcuts!.bindings["workspace.close"] = null;
    expect(
      configMutationFromAction(next, {
        type: "setShortcutBinding",
        action: "workspace.close",
        binding: null,
      }),
    ).toEqual({
      path: 'shortcuts.bindings["workspace.close"]',
      value: null,
    });
  });

  test("shortcut binding paths JSON-escape unusual action ids", () => {
    const next: Config = defaultSettingsConfig();
    next.shortcuts!.bindings['workspace."quote"'] = "cmd+q";
    expect(
      configMutationFromAction(next, {
        type: "setShortcutBinding",
        action: 'workspace."quote"',
        binding: "cmd+q",
      }),
    ).toEqual({
      path: 'shortcuts.bindings["workspace.\\"quote\\""]',
      value: "cmd+q",
    });
  });
});
