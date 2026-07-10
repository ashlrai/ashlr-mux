// Pure, immutable updater over the generated `@cmux/core-types` `Config`.
//
// Every action returns a NEW `Config` and NEVER mutates its input (structural
// sharing: untouched sections keep their reference identity). This is the
// desktop-web counterpart of the Settings mutations the macOS app performs on
// its in-memory `CmuxConfigFile` before persisting `cmux.json`.
//
// Sections on `Config` are all optional and their sub-shapes have required
// fields, so a section that is absent cannot be synthesized from a partial
// value. When an action targets an absent section the reducer is a no-op
// (returns the input unchanged) — the Settings UI only dispatches against a
// loaded config whose relevant section is present.

import type {
  Appearance,
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
  ShortcutBinding,
  SidebarConfig,
  TerminalConfig,
  WorkspaceColorsConfig,
} from "@cmux/core-types";

/** The keys of `T` whose value type is exactly `boolean`. */
type BooleanKeys<T> = {
  [K in keyof T]-?: T[K] extends boolean ? K : never;
}[keyof T];

/** Toggleable boolean flags on `sidebar` (excludes `branchLayout`, widths). */
export type SidebarBoolKey = BooleanKeys<SidebarConfig>;

/** Toggleable boolean flags on `notifications` (excludes sound/command/etc). */
export type NotificationsBoolKey = BooleanKeys<NotificationsConfig>;

/** Editable free-form string fields on `notifications`. */
export type NotificationsStringKey = "sound" | "customSoundFilePath" | "command";

/** Editable constrained-select fields on `notifications`. */
export type NotificationsSelectKey = "hooksMode";

/** Toggleable boolean flags on `app`. */
export type AppBoolKey = BooleanKeys<AppConfig>;

type KeysOfType<T, Value> = {
  [K in keyof T]-?: T[K] extends Value ? K : never;
}[keyof T];

/** Editable free-form string fields on `app`. */
export type AppStringKey = "language" | "windowTitleTemplate" | "preferredEditor";

/** Editable numeric fields on `app`. */
export type AppNumberKey = KeysOfType<AppConfig, number>;

/** Editable constrained-select fields on `app` (appearance has its own action). */
export type AppSelectKey = Exclude<
  KeysOfType<AppConfig, string>,
  AppStringKey | "appearance"
>;

/** Toggleable boolean flags on `automation`. */
export type AutomationBoolKey = BooleanKeys<AutomationConfig>;

/** Editable free-form string fields on `automation` (password is optional). */
export type AutomationStringKey =
  | "socketControlMode"
  | "claudeBinaryPath"
  | "autoNamingAgent"
  | "ripgrepBinaryPath";

/** Editable optional secret fields on `automation`. */
export type AutomationPasswordKey = "socket_password";

/** Editable numeric fields on `automation`. */
export type AutomationNumberKey = KeysOfType<AutomationConfig, number>;

/** Editable constrained-select fields on `automation`. */
export type AutomationSelectKey = "kiroNotificationLevel";

/** Toggleable boolean flags on `browser` (excludes strings/lists/numbers). */
export type BrowserBoolKey = BooleanKeys<BrowserConfig>;

/** Toggleable boolean flags on `terminal` (excludes nested hibernation/renderer). */
export type TerminalBoolKey = BooleanKeys<TerminalConfig>;

/** Editable numeric fields on `terminal` (excludes nested hibernation/renderer). */
export type TerminalNumberKey = KeysOfType<TerminalConfig, number>;

/** Nested terminal settings groups exposed by Settings > Terminal. */
export type TerminalNestedSection = "agentHibernation" | "rendererRealization";

/** Toggleable boolean flags on nested terminal settings groups. */
export type TerminalNestedBoolKey = "enabled";

/** Editable numeric fields on nested terminal settings groups. */
export type TerminalNestedNumberKey =
  | "idleSeconds"
  | "maxLiveTerminals"
  | "maxWarmRenderers";

/** Editable free-form string fields on `markdown`. */
export type MarkdownStringKey = KeysOfType<MarkdownConfig, string>;

/** Editable numeric fields on `markdown`. */
export type MarkdownNumberKey = KeysOfType<MarkdownConfig, number>;

/** Toggleable boolean flags on `canvas`. */
export type CanvasBoolKey = BooleanKeys<CanvasConfig>;

/** Editable numeric fields on `canvas`. */
export type CanvasNumberKey = KeysOfType<CanvasConfig, number>;

/** Toggleable boolean flags on `file_editor`. */
export type FileEditorBoolKey = BooleanKeys<FileEditorConfig>;

/** Editable constrained-select fields on `file_explorer`. */
export type FileExplorerSelectKey = "doubleClickAction";

/** Editable constrained-select fields on `diff_viewer`. */
export type DiffViewerSelectKey = "defaultLayout";

/** Editable free-form string fields on `workspace_colors`. */
export type WorkspaceColorsStringKey = "indicatorStyle";

/** Editable nullable string/color fields on `workspace_colors`. */
export type WorkspaceColorsNullableStringKey =
  | "selectionColor"
  | "notificationBadgeColor";

/** Editable string maps on `workspace_colors`. */
export type WorkspaceColorsMapKey = "colors" | "paletteOverrides";

/** Editable string-list fields on `workspace_colors`. */
export type WorkspaceColorsListKey = "customColors";

/** Editable constrained-select fields on `sidebar`. */
export type SidebarSelectKey = "branchLayout";

/** Editable string fields on `sidebar_appearance`. */
export type SidebarAppearanceStringKey =
  | "tintColor"
  | "lightModeTintColor"
  | "darkModeTintColor";

/** Editable numeric fields on `sidebar_appearance`. */
export type SidebarAppearanceNumberKey = "tintOpacity";

/** Editable plain string fields on `browser` (theme is constrained separately). */
export type BrowserStringKey = Exclude<KeysOfType<BrowserConfig, string>, "theme">;

/** Editable numeric fields on `browser`. */
export type BrowserNumberKey = KeysOfType<BrowserConfig, number>;

/** Editable string-list fields on `browser`. */
export type BrowserStringListKey = KeysOfType<BrowserConfig, Array<string>>;

/**
 * A Settings mutation. Discriminated on `type` so `configReducer` is total and
 * the Settings UI can `dispatch` without knowing the update mechanics.
 */
export type ConfigAction =
  | { type: "toggleSidebarFlag"; key: SidebarBoolKey }
  | { type: "toggleNotificationsFlag"; key: NotificationsBoolKey }
  | { type: "setNotificationsString"; key: NotificationsStringKey; value: string }
  | {
      type: "setNotificationsSelect";
      key: NotificationsSelectKey;
      value: NotificationsConfig[NotificationsSelectKey];
    }
  | { type: "toggleAppFlag"; key: AppBoolKey }
  | { type: "setAppString"; key: AppStringKey; value: string }
  | { type: "setAppNumber"; key: AppNumberKey; value: number }
  | { type: "setAppSelect"; key: AppSelectKey; value: AppConfig[AppSelectKey] }
  | { type: "toggleAutomationFlag"; key: AutomationBoolKey }
  | { type: "setAutomationString"; key: AutomationStringKey; value: string }
  | {
      type: "setAutomationPassword";
      key: AutomationPasswordKey;
      value: string | null;
    }
  | { type: "setAutomationNumber"; key: AutomationNumberKey; value: number }
  | {
      type: "setAutomationSelect";
      key: AutomationSelectKey;
      value: AutomationConfig[AutomationSelectKey];
    }
  | { type: "toggleBrowserFlag"; key: BrowserBoolKey }
  | { type: "setBrowserString"; key: BrowserStringKey; value: string }
  | { type: "setBrowserTheme"; theme: Appearance }
  | { type: "setBrowserNumber"; key: BrowserNumberKey; value: number }
  | { type: "setBrowserStringList"; key: BrowserStringListKey; value: Array<string> }
  | { type: "setBrowserEnabled"; enabled: boolean }
  | { type: "toggleTerminalFlag"; key: TerminalBoolKey }
  | { type: "setTerminalNumber"; key: TerminalNumberKey; value: number }
  | {
      type: "toggleTerminalNestedFlag";
      section: TerminalNestedSection;
      key: TerminalNestedBoolKey;
    }
  | {
      type: "setTerminalNestedNumber";
      section: TerminalNestedSection;
      key: TerminalNestedNumberKey;
      value: number;
    }
  | { type: "setMarkdownString"; key: MarkdownStringKey; value: string }
  | { type: "setMarkdownNumber"; key: MarkdownNumberKey; value: number }
  | { type: "toggleCanvasFlag"; key: CanvasBoolKey }
  | { type: "setCanvasNumber"; key: CanvasNumberKey; value: number }
  | { type: "toggleFileEditorFlag"; key: FileEditorBoolKey }
  | {
      type: "setFileExplorerSelect";
      key: FileExplorerSelectKey;
      value: FileExplorerConfig[FileExplorerSelectKey];
    }
  | {
      type: "setDiffViewerSelect";
      key: DiffViewerSelectKey;
      value: DiffViewerConfig[DiffViewerSelectKey];
    }
  | { type: "setWorkspaceColorsString"; key: WorkspaceColorsStringKey; value: string }
  | {
      type: "setWorkspaceColorsNullableString";
      key: WorkspaceColorsNullableStringKey;
      value: string | null;
    }
  | {
      type: "setWorkspaceColorsMap";
      key: WorkspaceColorsMapKey;
      value: Record<string, string>;
    }
  | {
      type: "setWorkspaceColorsList";
      key: WorkspaceColorsListKey;
      value: Array<string>;
    }
  | { type: "setSidebarSelect"; key: SidebarSelectKey; value: SidebarConfig[SidebarSelectKey] }
  | {
      type: "setSidebarAppearanceString";
      key: SidebarAppearanceStringKey;
      value: string | null;
    }
  | {
      type: "setSidebarAppearanceNumber";
      key: SidebarAppearanceNumberKey;
      value: number;
    }
  | { type: "setMinimalMode"; enabled: boolean }
  | { type: "toggleMatchTerminalBackground" }
  | { type: "setAppearance"; appearance: Appearance }
  // `binding: null` explicitly unbinds the action (preserved distinctly from an
  // absent key), mirroring `ShortcutsConfig.bindings`' `Option<ShortcutBinding>`.
  | { type: "setShortcutBinding"; action: string; binding: ShortcutBinding | null };

/**
 * Applies `action` to `config`, returning a new `Config`. The input is never
 * mutated. When the targeted section is absent the input is returned unchanged.
 */
export function configReducer(config: Config, action: ConfigAction): Config {
  switch (action.type) {
    case "toggleSidebarFlag": {
      const sidebar = config.sidebar;
      if (!sidebar) {
        return config;
      }
      const nextSidebar: SidebarConfig = { ...sidebar };
      nextSidebar[action.key] = !sidebar[action.key];
      return { ...config, sidebar: nextSidebar };
    }
    case "toggleNotificationsFlag": {
      const notifications = config.notifications;
      if (!notifications) {
        return config;
      }
      const nextNotifications: NotificationsConfig = { ...notifications };
      nextNotifications[action.key] = !notifications[action.key];
      return { ...config, notifications: nextNotifications };
    }
    case "setNotificationsString": {
      const notifications = config.notifications;
      if (!notifications) {
        return config;
      }
      return {
        ...config,
        notifications: { ...notifications, [action.key]: action.value },
      };
    }
    case "setNotificationsSelect": {
      const notifications = config.notifications;
      if (!notifications) {
        return config;
      }
      return {
        ...config,
        notifications: { ...notifications, [action.key]: action.value },
      };
    }
    case "toggleAppFlag": {
      const app = config.app;
      if (!app) {
        return config;
      }
      const nextApp: AppConfig = { ...app };
      nextApp[action.key] = !app[action.key];
      return { ...config, app: nextApp };
    }
    case "setAppString": {
      const app = config.app;
      if (!app) {
        return config;
      }
      return { ...config, app: { ...app, [action.key]: action.value } };
    }
    case "setAppNumber": {
      const app = config.app;
      if (!app) {
        return config;
      }
      return { ...config, app: { ...app, [action.key]: action.value } };
    }
    case "setAppSelect": {
      const app = config.app;
      if (!app) {
        return config;
      }
      return { ...config, app: { ...app, [action.key]: action.value } };
    }
    case "toggleAutomationFlag": {
      const automation = config.automation;
      if (!automation) {
        return config;
      }
      const nextAutomation: AutomationConfig = { ...automation };
      nextAutomation[action.key] = !automation[action.key];
      return { ...config, automation: nextAutomation };
    }
    case "setAutomationString": {
      const automation = config.automation;
      if (!automation) {
        return config;
      }
      return {
        ...config,
        automation: { ...automation, [action.key]: action.value },
      };
    }
    case "setAutomationPassword": {
      const automation = config.automation;
      if (!automation) {
        return config;
      }
      return {
        ...config,
        automation: {
          ...automation,
          [action.key]: action.value == null ? undefined : action.value,
        },
      };
    }
    case "setAutomationNumber": {
      const automation = config.automation;
      if (!automation) {
        return config;
      }
      return {
        ...config,
        automation: { ...automation, [action.key]: action.value },
      };
    }
    case "setAutomationSelect": {
      const automation = config.automation;
      if (!automation) {
        return config;
      }
      return {
        ...config,
        automation: { ...automation, [action.key]: action.value },
      };
    }
    case "toggleBrowserFlag": {
      const browser = config.browser;
      if (!browser) {
        return config;
      }
      const nextBrowser: BrowserConfig = { ...browser };
      nextBrowser[action.key] = !browser[action.key];
      return { ...config, browser: nextBrowser };
    }
    case "setBrowserString": {
      const browser = config.browser;
      if (!browser) {
        return config;
      }
      return {
        ...config,
        browser: { ...browser, [action.key]: action.value },
      };
    }
    case "setBrowserTheme": {
      const browser = config.browser;
      if (!browser) {
        return config;
      }
      return { ...config, browser: { ...browser, theme: action.theme } };
    }
    case "setBrowserNumber": {
      const browser = config.browser;
      if (!browser) {
        return config;
      }
      return {
        ...config,
        browser: { ...browser, [action.key]: action.value },
      };
    }
    case "setBrowserStringList": {
      const browser = config.browser;
      if (!browser) {
        return config;
      }
      return {
        ...config,
        browser: { ...browser, [action.key]: [...action.value] },
      };
    }
    case "setBrowserEnabled": {
      const browser = config.browser;
      if (!browser) {
        return config;
      }
      const nextBrowser: BrowserConfig = {
        ...browser,
        openTerminalLinksInCmuxBrowser: action.enabled,
        interceptTerminalOpenCommandInCmuxBrowser: action.enabled,
      };
      return { ...config, browser: nextBrowser };
    }
    case "toggleTerminalFlag": {
      const terminal = config.terminal;
      if (!terminal) {
        return config;
      }
      const nextTerminal: TerminalConfig = { ...terminal };
      nextTerminal[action.key] = !terminal[action.key];
      return { ...config, terminal: nextTerminal };
    }
    case "setTerminalNumber": {
      const terminal = config.terminal;
      if (!terminal) {
        return config;
      }
      return {
        ...config,
        terminal: { ...terminal, [action.key]: action.value },
      };
    }
    case "toggleTerminalNestedFlag": {
      const terminal = config.terminal;
      if (!terminal) {
        return config;
      }
      const nested = terminal[action.section];
      return {
        ...config,
        terminal: {
          ...terminal,
          [action.section]: {
            ...nested,
            [action.key]: !nested[action.key],
          },
        },
      };
    }
    case "setTerminalNestedNumber": {
      const terminal = config.terminal;
      if (!terminal) {
        return config;
      }
      const nested = terminal[action.section];
      if (!(action.key in nested)) {
        return config;
      }
      return {
        ...config,
        terminal: {
          ...terminal,
          [action.section]: {
            ...nested,
            [action.key]: action.value,
          },
        },
      };
    }
    case "setMarkdownString": {
      const markdown = config.markdown;
      if (!markdown) {
        return config;
      }
      return { ...config, markdown: { ...markdown, [action.key]: action.value } };
    }
    case "setMarkdownNumber": {
      const markdown = config.markdown;
      if (!markdown) {
        return config;
      }
      return { ...config, markdown: { ...markdown, [action.key]: action.value } };
    }
    case "toggleCanvasFlag": {
      const canvas = config.canvas;
      if (!canvas) {
        return config;
      }
      const nextCanvas: CanvasConfig = { ...canvas };
      nextCanvas[action.key] = !canvas[action.key];
      return { ...config, canvas: nextCanvas };
    }
    case "setCanvasNumber": {
      const canvas = config.canvas;
      if (!canvas) {
        return config;
      }
      return { ...config, canvas: { ...canvas, [action.key]: action.value } };
    }
    case "toggleFileEditorFlag": {
      const fileEditor = config.file_editor;
      if (!fileEditor) {
        return config;
      }
      const nextFileEditor: FileEditorConfig = { ...fileEditor };
      nextFileEditor[action.key] = !fileEditor[action.key];
      return { ...config, file_editor: nextFileEditor };
    }
    case "setFileExplorerSelect": {
      const fileExplorer = config.file_explorer;
      if (!fileExplorer) {
        return config;
      }
      return {
        ...config,
        file_explorer: { ...fileExplorer, [action.key]: action.value },
      };
    }
    case "setDiffViewerSelect": {
      const diffViewer = config.diff_viewer;
      if (!diffViewer) {
        return config;
      }
      return {
        ...config,
        diff_viewer: { ...diffViewer, [action.key]: action.value },
      };
    }
    case "setWorkspaceColorsString": {
      const workspaceColors = config.workspace_colors;
      if (!workspaceColors) {
        return config;
      }
      return {
        ...config,
        workspace_colors: { ...workspaceColors, [action.key]: action.value },
      };
    }
    case "setWorkspaceColorsNullableString": {
      const workspaceColors = config.workspace_colors;
      if (!workspaceColors) {
        return config;
      }
      return {
        ...config,
        workspace_colors: { ...workspaceColors, [action.key]: action.value },
      };
    }
    case "setWorkspaceColorsMap": {
      const workspaceColors = config.workspace_colors;
      if (!workspaceColors) {
        return config;
      }
      const nextWorkspaceColors: WorkspaceColorsConfig = {
        ...workspaceColors,
        [action.key]: { ...action.value },
      };
      return { ...config, workspace_colors: nextWorkspaceColors };
    }
    case "setWorkspaceColorsList": {
      const workspaceColors = config.workspace_colors;
      if (!workspaceColors) {
        return config;
      }
      return {
        ...config,
        workspace_colors: { ...workspaceColors, [action.key]: [...action.value] },
      };
    }
    case "setSidebarSelect": {
      const sidebar = config.sidebar;
      if (!sidebar) {
        return config;
      }
      return { ...config, sidebar: { ...sidebar, [action.key]: action.value } };
    }
    case "setSidebarAppearanceString": {
      const sidebarAppearance = config.sidebar_appearance;
      if (!sidebarAppearance) {
        return config;
      }
      return {
        ...config,
        sidebar_appearance: {
          ...sidebarAppearance,
          [action.key]: action.value,
        },
      };
    }
    case "setSidebarAppearanceNumber": {
      const sidebarAppearance = config.sidebar_appearance;
      if (!sidebarAppearance) {
        return config;
      }
      return {
        ...config,
        sidebar_appearance: {
          ...sidebarAppearance,
          [action.key]: action.value,
        },
      };
    }
    case "setMinimalMode": {
      const app = config.app;
      if (!app) {
        return config;
      }
      return { ...config, app: { ...app, minimalMode: action.enabled } };
    }
    case "toggleMatchTerminalBackground": {
      const sidebarAppearance = config.sidebar_appearance;
      if (!sidebarAppearance) {
        return config;
      }
      const nextSidebarAppearance: SidebarAppearanceConfig = {
        ...sidebarAppearance,
        matchTerminalBackground: !sidebarAppearance.matchTerminalBackground,
      };
      return { ...config, sidebar_appearance: nextSidebarAppearance };
    }
    case "setAppearance": {
      const app = config.app;
      if (!app) {
        return config;
      }
      return { ...config, app: { ...app, appearance: action.appearance } };
    }
    case "setShortcutBinding": {
      const shortcuts = config.shortcuts;
      if (!shortcuts) {
        return config;
      }
      return {
        ...config,
        shortcuts: {
          ...shortcuts,
          bindings: { ...shortcuts.bindings, [action.action]: action.binding },
        },
      };
    }
  }
}
