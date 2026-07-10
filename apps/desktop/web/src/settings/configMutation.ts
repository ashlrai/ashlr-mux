import type { BrowserConfig, Config } from "@cmux/core-types";

import type {
  AppBoolKey,
  AppNumberKey,
  AppSelectKey,
  AppStringKey,
  AutomationBoolKey,
  AutomationNumberKey,
  AutomationPasswordKey,
  AutomationSelectKey,
  AutomationStringKey,
  BrowserBoolKey,
  BrowserNumberKey,
  BrowserStringKey,
  BrowserStringListKey,
  CanvasBoolKey,
  CanvasNumberKey,
  ConfigAction,
  DiffViewerSelectKey,
  FileEditorBoolKey,
  FileExplorerSelectKey,
  MarkdownNumberKey,
  MarkdownStringKey,
  NotificationsBoolKey,
  NotificationsSelectKey,
  NotificationsStringKey,
  SidebarAppearanceNumberKey,
  SidebarAppearanceStringKey,
  SidebarBoolKey,
  SidebarSelectKey,
  TerminalBoolKey,
  TerminalNestedBoolKey,
  TerminalNestedNumberKey,
  TerminalNestedSection,
  TerminalNumberKey,
  WorkspaceColorsListKey,
  WorkspaceColorsMapKey,
  WorkspaceColorsNullableStringKey,
  WorkspaceColorsStringKey,
} from "./configReducer";
import {
  DEFAULT_APP_CONFIG,
  DEFAULT_AUTOMATION_CONFIG,
  DEFAULT_BROWSER_CONFIG,
  DEFAULT_CANVAS_CONFIG,
  DEFAULT_DIFF_VIEWER_CONFIG,
  DEFAULT_FILE_EDITOR_CONFIG,
  DEFAULT_FILE_EXPLORER_CONFIG,
  DEFAULT_MARKDOWN_CONFIG,
  DEFAULT_NOTIFICATIONS_CONFIG,
  DEFAULT_SIDEBAR_APPEARANCE_CONFIG,
  DEFAULT_SIDEBAR_CONFIG,
  DEFAULT_TERMINAL_CONFIG,
  DEFAULT_WORKSPACE_COLORS_CONFIG,
} from "./defaultConfig";

export interface ConfigMutation {
  path: string;
  value?: unknown;
  remove?: boolean;
}

function sidebarDefault(key: SidebarBoolKey): boolean {
  return DEFAULT_SIDEBAR_CONFIG[key];
}

function notificationsDefault(key: NotificationsBoolKey): boolean {
  return DEFAULT_NOTIFICATIONS_CONFIG[key];
}

function notificationsStringDefault(key: NotificationsStringKey): string {
  return DEFAULT_NOTIFICATIONS_CONFIG[key];
}

function notificationsSelectDefault(
  key: NotificationsSelectKey,
): (typeof DEFAULT_NOTIFICATIONS_CONFIG)[NotificationsSelectKey] {
  return DEFAULT_NOTIFICATIONS_CONFIG[key];
}

function sidebarSelectDefault(
  key: SidebarSelectKey,
): (typeof DEFAULT_SIDEBAR_CONFIG)[SidebarSelectKey] {
  return DEFAULT_SIDEBAR_CONFIG[key];
}

function sidebarAppearanceStringDefault(
  key: SidebarAppearanceStringKey,
): string | null {
  return DEFAULT_SIDEBAR_APPEARANCE_CONFIG[key];
}

function sidebarAppearanceNumberDefault(key: SidebarAppearanceNumberKey): number {
  return DEFAULT_SIDEBAR_APPEARANCE_CONFIG[key];
}

function appDefault(key: keyof typeof DEFAULT_APP_CONFIG): unknown {
  return DEFAULT_APP_CONFIG[key];
}

function appBoolDefault(key: AppBoolKey): boolean {
  return DEFAULT_APP_CONFIG[key];
}

function appStringDefault(key: AppStringKey): string {
  return DEFAULT_APP_CONFIG[key];
}

function appNumberDefault(key: AppNumberKey): number {
  return DEFAULT_APP_CONFIG[key];
}

function appSelectDefault(key: AppSelectKey): (typeof DEFAULT_APP_CONFIG)[AppSelectKey] {
  return DEFAULT_APP_CONFIG[key];
}

function automationBoolDefault(key: AutomationBoolKey): boolean {
  return DEFAULT_AUTOMATION_CONFIG[key];
}

function automationStringDefault(key: AutomationStringKey): string {
  return DEFAULT_AUTOMATION_CONFIG[key];
}

function automationPasswordDefault(
  key: AutomationPasswordKey,
): string | undefined {
  return DEFAULT_AUTOMATION_CONFIG[key];
}

function automationNumberDefault(key: AutomationNumberKey): number {
  return DEFAULT_AUTOMATION_CONFIG[key];
}

function automationSelectDefault(
  key: AutomationSelectKey,
): (typeof DEFAULT_AUTOMATION_CONFIG)[AutomationSelectKey] {
  return DEFAULT_AUTOMATION_CONFIG[key];
}

function automationPath(key: string): string {
  return key === "socket_password"
    ? "automation.socketPassword"
    : `automation.${key}`;
}

function sidebarAppearancePath(key: string): string {
  return `sidebarAppearance.${key}`;
}

function fileEditorPath(key: string): string {
  return `fileEditor.${key}`;
}

function fileExplorerPath(key: string): string {
  return `fileExplorer.${key}`;
}

function diffViewerPath(key: string): string {
  return `diffViewer.${key}`;
}

function browserDefault(key: BrowserBoolKey): boolean {
  return DEFAULT_BROWSER_CONFIG[key];
}

function browserStringDefault(key: BrowserStringKey): string {
  return DEFAULT_BROWSER_CONFIG[key];
}

function browserNumberDefault(key: BrowserNumberKey): number {
  return DEFAULT_BROWSER_CONFIG[key];
}

function browserStringListDefault(key: BrowserStringListKey): Array<string> {
  return DEFAULT_BROWSER_CONFIG[key];
}

function terminalBoolDefault(key: TerminalBoolKey): boolean {
  return DEFAULT_TERMINAL_CONFIG[key];
}

function terminalNumberDefault(key: TerminalNumberKey): number {
  return DEFAULT_TERMINAL_CONFIG[key];
}

function terminalNestedBoolDefault(
  section: TerminalNestedSection,
  key: TerminalNestedBoolKey,
): boolean {
  return DEFAULT_TERMINAL_CONFIG[section][key];
}

function terminalNestedNumberDefault(
  section: TerminalNestedSection,
  key: TerminalNestedNumberKey,
): number | undefined {
  const defaults = DEFAULT_TERMINAL_CONFIG[section];
  const value = key in defaults ? defaults[key as keyof typeof defaults] : undefined;
  return typeof value === "number" ? value : undefined;
}

function markdownStringDefault(key: MarkdownStringKey): string {
  return DEFAULT_MARKDOWN_CONFIG[key];
}

function markdownNumberDefault(key: MarkdownNumberKey): number {
  return DEFAULT_MARKDOWN_CONFIG[key];
}

function canvasBoolDefault(key: CanvasBoolKey): boolean {
  return DEFAULT_CANVAS_CONFIG[key];
}

function canvasNumberDefault(key: CanvasNumberKey): number {
  return DEFAULT_CANVAS_CONFIG[key];
}

function fileEditorBoolDefault(key: FileEditorBoolKey): boolean {
  return DEFAULT_FILE_EDITOR_CONFIG[key];
}

function fileExplorerSelectDefault(
  key: FileExplorerSelectKey,
): (typeof DEFAULT_FILE_EXPLORER_CONFIG)[FileExplorerSelectKey] {
  return DEFAULT_FILE_EXPLORER_CONFIG[key];
}

function diffViewerSelectDefault(
  key: DiffViewerSelectKey,
): (typeof DEFAULT_DIFF_VIEWER_CONFIG)[DiffViewerSelectKey] {
  return DEFAULT_DIFF_VIEWER_CONFIG[key];
}

function workspaceColorsStringDefault(key: WorkspaceColorsStringKey): string {
  return DEFAULT_WORKSPACE_COLORS_CONFIG[key];
}

function workspaceColorsNullableStringDefault(
  key: WorkspaceColorsNullableStringKey,
): string | null {
  return DEFAULT_WORKSPACE_COLORS_CONFIG[key];
}

function workspaceColorsMapDefault(
  key: WorkspaceColorsMapKey,
): Record<string, string | undefined> {
  return DEFAULT_WORKSPACE_COLORS_CONFIG[key];
}

function workspaceColorsListDefault(
  key: WorkspaceColorsListKey,
): Array<string> {
  return DEFAULT_WORKSPACE_COLORS_CONFIG[key];
}

function browserConfigEquals(
  a: BrowserConfig | undefined,
  b: BrowserConfig,
): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function valuesEqual(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

function jsonPathQuotedComponent(component: string): string {
  return `[${JSON.stringify(component)}]`;
}

/**
 * Map one supported Settings action to a raw-config mutation path.
 *
 * Plain setting ids use dotted paths. Shortcut actions can contain literal dots
 * (`workspace.new`), so they use the backend's bracket-quoted path component.
 */
export function configMutationFromAction(
  nextConfig: Config,
  action: ConfigAction,
): ConfigMutation | null {
  switch (action.type) {
    case "toggleSidebarFlag": {
      const nextValue = nextConfig.sidebar?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === sidebarDefault(action.key)
        ? { path: `sidebar.${action.key}`, remove: true }
        : { path: `sidebar.${action.key}`, value: nextValue };
    }
    case "toggleNotificationsFlag": {
      const nextValue = nextConfig.notifications?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === notificationsDefault(action.key)
        ? { path: `notifications.${action.key}`, remove: true }
        : { path: `notifications.${action.key}`, value: nextValue };
    }
    case "setNotificationsString": {
      const nextValue = nextConfig.notifications?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === notificationsStringDefault(action.key)
        ? { path: `notifications.${action.key}`, remove: true }
        : { path: `notifications.${action.key}`, value: nextValue };
    }
    case "setNotificationsSelect": {
      const nextValue = nextConfig.notifications?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === notificationsSelectDefault(action.key)
        ? { path: `notifications.${action.key}`, remove: true }
        : { path: `notifications.${action.key}`, value: nextValue };
    }
    case "toggleAppFlag": {
      const nextValue = nextConfig.app?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === appBoolDefault(action.key)
        ? { path: `app.${action.key}`, remove: true }
        : { path: `app.${action.key}`, value: nextValue };
    }
    case "setAppString": {
      const nextValue = nextConfig.app?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === appStringDefault(action.key)
        ? { path: `app.${action.key}`, remove: true }
        : { path: `app.${action.key}`, value: nextValue };
    }
    case "setAppNumber": {
      const nextValue = nextConfig.app?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === appNumberDefault(action.key)
        ? { path: `app.${action.key}`, remove: true }
        : { path: `app.${action.key}`, value: nextValue };
    }
    case "setAppSelect": {
      const nextValue = nextConfig.app?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === appSelectDefault(action.key)
        ? { path: `app.${action.key}`, remove: true }
        : { path: `app.${action.key}`, value: nextValue };
    }
    case "toggleAutomationFlag": {
      const nextValue = nextConfig.automation?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === automationBoolDefault(action.key)
        ? { path: automationPath(action.key), remove: true }
        : { path: automationPath(action.key), value: nextValue };
    }
    case "setAutomationString": {
      const nextValue = nextConfig.automation?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === automationStringDefault(action.key)
        ? { path: automationPath(action.key), remove: true }
        : { path: automationPath(action.key), value: nextValue };
    }
    case "setAutomationPassword": {
      const nextValue = nextConfig.automation?.[action.key];
      if (
        nextValue === undefined ||
        nextValue === "" ||
        nextValue === automationPasswordDefault(action.key)
      ) {
        return { path: automationPath(action.key), remove: true };
      }
      return { path: automationPath(action.key), value: nextValue };
    }
    case "setAutomationNumber": {
      const nextValue = nextConfig.automation?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === automationNumberDefault(action.key)
        ? { path: automationPath(action.key), remove: true }
        : { path: automationPath(action.key), value: nextValue };
    }
    case "setAutomationSelect": {
      const nextValue = nextConfig.automation?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === automationSelectDefault(action.key)
        ? { path: automationPath(action.key), remove: true }
        : { path: automationPath(action.key), value: nextValue };
    }
    case "toggleBrowserFlag": {
      const nextValue = nextConfig.browser?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === browserDefault(action.key)
        ? { path: `browser.${action.key}`, remove: true }
        : { path: `browser.${action.key}`, value: nextValue };
    }
    case "setBrowserString": {
      const nextValue = nextConfig.browser?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === browserStringDefault(action.key)
        ? { path: `browser.${action.key}`, remove: true }
        : { path: `browser.${action.key}`, value: nextValue };
    }
    case "setBrowserTheme": {
      const nextValue = nextConfig.browser?.theme;
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === DEFAULT_BROWSER_CONFIG.theme
        ? { path: "browser.theme", remove: true }
        : { path: "browser.theme", value: nextValue };
    }
    case "setBrowserNumber": {
      const nextValue = nextConfig.browser?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === browserNumberDefault(action.key)
        ? { path: `browser.${action.key}`, remove: true }
        : { path: `browser.${action.key}`, value: nextValue };
    }
    case "setBrowserStringList": {
      const nextValue = nextConfig.browser?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return valuesEqual(nextValue, browserStringListDefault(action.key))
        ? { path: `browser.${action.key}`, remove: true }
        : { path: `browser.${action.key}`, value: nextValue };
    }
    case "setAppearance": {
      const nextValue = nextConfig.app?.appearance;
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === appDefault("appearance")
        ? { path: "app.appearance", remove: true }
        : { path: "app.appearance", value: nextValue };
    }
    case "setMinimalMode": {
      const nextValue = nextConfig.app?.minimalMode;
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === DEFAULT_APP_CONFIG.minimalMode
        ? { path: "app.minimalMode", remove: true }
        : { path: "app.minimalMode", value: nextValue };
    }
    case "toggleMatchTerminalBackground": {
      const nextValue = nextConfig.sidebar_appearance?.matchTerminalBackground;
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === DEFAULT_SIDEBAR_APPEARANCE_CONFIG.matchTerminalBackground
        ? { path: sidebarAppearancePath("matchTerminalBackground"), remove: true }
        : {
            path: sidebarAppearancePath("matchTerminalBackground"),
            value: nextValue,
          };
    }
    case "setBrowserEnabled": {
      const nextBrowser = nextConfig.browser;
      if (nextBrowser === undefined) {
        return null;
      }
      return browserConfigEquals(nextBrowser, DEFAULT_BROWSER_CONFIG)
        ? { path: "browser", remove: true }
        : { path: "browser", value: nextBrowser };
    }
    case "toggleTerminalFlag": {
      const nextValue = nextConfig.terminal?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === terminalBoolDefault(action.key)
        ? { path: `terminal.${action.key}`, remove: true }
        : { path: `terminal.${action.key}`, value: nextValue };
    }
    case "setTerminalNumber": {
      const nextValue = nextConfig.terminal?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === terminalNumberDefault(action.key)
        ? { path: `terminal.${action.key}`, remove: true }
        : { path: `terminal.${action.key}`, value: nextValue };
    }
    case "toggleTerminalNestedFlag": {
      const nextValue = nextConfig.terminal?.[action.section][action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === terminalNestedBoolDefault(action.section, action.key)
        ? { path: `terminal.${action.section}.${action.key}`, remove: true }
        : { path: `terminal.${action.section}.${action.key}`, value: nextValue };
    }
    case "setTerminalNestedNumber": {
      const nested = nextConfig.terminal?.[action.section];
      if (nested === undefined || !(action.key in nested)) {
        return null;
      }
      const nextValue = nested[action.key as keyof typeof nested];
      const defaultValue = terminalNestedNumberDefault(action.section, action.key);
      if (defaultValue === undefined) {
        return null;
      }
      return nextValue === defaultValue
        ? { path: `terminal.${action.section}.${action.key}`, remove: true }
        : { path: `terminal.${action.section}.${action.key}`, value: nextValue };
    }
    case "setMarkdownString": {
      const nextValue = nextConfig.markdown?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === markdownStringDefault(action.key)
        ? { path: `markdown.${action.key}`, remove: true }
        : { path: `markdown.${action.key}`, value: nextValue };
    }
    case "setMarkdownNumber": {
      const nextValue = nextConfig.markdown?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === markdownNumberDefault(action.key)
        ? { path: `markdown.${action.key}`, remove: true }
        : { path: `markdown.${action.key}`, value: nextValue };
    }
    case "toggleCanvasFlag": {
      const nextValue = nextConfig.canvas?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === canvasBoolDefault(action.key)
        ? { path: `canvas.${action.key}`, remove: true }
        : { path: `canvas.${action.key}`, value: nextValue };
    }
    case "setCanvasNumber": {
      const nextValue = nextConfig.canvas?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === canvasNumberDefault(action.key)
        ? { path: `canvas.${action.key}`, remove: true }
        : { path: `canvas.${action.key}`, value: nextValue };
    }
    case "toggleFileEditorFlag": {
      const nextValue = nextConfig.file_editor?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === fileEditorBoolDefault(action.key)
        ? { path: fileEditorPath(action.key), remove: true }
        : { path: fileEditorPath(action.key), value: nextValue };
    }
    case "setFileExplorerSelect": {
      const nextValue = nextConfig.file_explorer?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === fileExplorerSelectDefault(action.key)
        ? { path: fileExplorerPath(action.key), remove: true }
        : { path: fileExplorerPath(action.key), value: nextValue };
    }
    case "setDiffViewerSelect": {
      const nextValue = nextConfig.diff_viewer?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === diffViewerSelectDefault(action.key)
        ? { path: diffViewerPath(action.key), remove: true }
        : { path: diffViewerPath(action.key), value: nextValue };
    }
    case "setWorkspaceColorsString": {
      const nextValue = nextConfig.workspace_colors?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === workspaceColorsStringDefault(action.key)
        ? { path: `workspaceColors.${action.key}`, remove: true }
        : { path: `workspaceColors.${action.key}`, value: nextValue };
    }
    case "setWorkspaceColorsNullableString": {
      const nextValue = nextConfig.workspace_colors?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === workspaceColorsNullableStringDefault(action.key)
        ? { path: `workspaceColors.${action.key}`, remove: true }
        : { path: `workspaceColors.${action.key}`, value: nextValue };
    }
    case "setWorkspaceColorsMap": {
      const nextValue = nextConfig.workspace_colors?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return valuesEqual(nextValue, workspaceColorsMapDefault(action.key))
        ? { path: `workspaceColors.${action.key}`, remove: true }
        : { path: `workspaceColors.${action.key}`, value: nextValue };
    }
    case "setWorkspaceColorsList": {
      const nextValue = nextConfig.workspace_colors?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return valuesEqual(nextValue, workspaceColorsListDefault(action.key))
        ? { path: `workspaceColors.${action.key}`, remove: true }
        : { path: `workspaceColors.${action.key}`, value: nextValue };
    }
    case "setSidebarSelect": {
      const nextValue = nextConfig.sidebar?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === sidebarSelectDefault(action.key)
        ? { path: `sidebar.${action.key}`, remove: true }
        : { path: `sidebar.${action.key}`, value: nextValue };
    }
    case "setSidebarAppearanceString": {
      const nextValue = nextConfig.sidebar_appearance?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === sidebarAppearanceStringDefault(action.key)
        ? { path: sidebarAppearancePath(action.key), remove: true }
        : { path: sidebarAppearancePath(action.key), value: nextValue };
    }
    case "setSidebarAppearanceNumber": {
      const nextValue = nextConfig.sidebar_appearance?.[action.key];
      if (nextValue === undefined) {
        return null;
      }
      return nextValue === sidebarAppearanceNumberDefault(action.key)
        ? { path: sidebarAppearancePath(action.key), remove: true }
        : { path: sidebarAppearancePath(action.key), value: nextValue };
    }
    case "setShortcutBinding": {
      if (!nextConfig.shortcuts) {
        return null;
      }
      return {
        path: `shortcuts.bindings${jsonPathQuotedComponent(action.action)}`,
        value: action.binding,
      };
    }
  }
}
