// Presentational, CONTROLLED Settings form. Renders the Sidebar / Notifications
// toggles, the Appearance radio group, and the Shortcuts list straight from a
// `Config` prop, and emits the next `Config` through `onChange`. It owns NO
// state and performs NO IPC — the parent holds the config and persists it. All
// mutations go through the pure `configReducer`, mirroring how the macOS
// Settings panes mutate the in-memory `CmuxConfigFile`. The search box is
// controlled like everything else (`searchQuery` / `onSearchQueryChange`);
// navigation (scroll-to-section) is the parent's side effect via `onNavigate`.

import { useState } from "react";

import type {
  AppConfig,
  AppIcon,
  Appearance,
  BranchLayout,
  Config,
  ConfirmQuit,
  DiffLayout,
  DoubleClickAction,
  ForkDestination,
  HooksMode,
  KiroNotificationLevel,
  NewWorkspacePlacement,
} from "@cmux/core-types";

import {
  type AppBoolKey,
  type AppNumberKey,
  type AppSelectKey,
  type AppStringKey,
  type AutomationBoolKey,
  type AutomationNumberKey,
  type AutomationStringKey,
  type BrowserBoolKey,
  type BrowserNumberKey,
  type BrowserStringKey,
  type BrowserStringListKey,
  type CanvasBoolKey,
  type CanvasNumberKey,
  type ConfigAction,
  type FileEditorBoolKey,
  type MarkdownNumberKey,
  type MarkdownStringKey,
  configReducer,
  type NotificationsBoolKey,
  type NotificationsStringKey,
  type SidebarAppearanceNumberKey,
  type SidebarAppearanceStringKey,
  type SidebarBoolKey,
  type TerminalBoolKey,
  type TerminalNestedBoolKey,
  type TerminalNestedNumberKey,
  type TerminalNestedSection,
  type TerminalNumberKey,
  type WorkspaceColorsListKey,
  type WorkspaceColorsMapKey,
  type WorkspaceColorsNullableStringKey,
} from "../settings/configReducer";
import { shortcutBindingDisplayString } from "../settings/shortcutBinding";
import {
  settingsSearchResults,
  type SettingsPaneSection,
} from "../settings/settingsSearchResults";
import {
  shortcutBindingDraftValue,
  shortcutBindingFromDraft,
  shortcutBindingFromKeyboardEvent,
} from "../settings/shortcutBindingEdit";
import {
  buildBrowserImportStartRequest,
  browserImportScopeFromSelection,
  defaultBrowserImportMode,
  normalizedDestinationProfiles,
  type BrowserImportDestinationProfile,
  type BrowserImportMode,
  type BrowserImportSeparateDestinationChoice,
  type BrowserImportStartRequest,
} from "../settings/browserImportPlan";

export interface SettingsPaneProps {
  config: Config;
  onChange: (next: Config) => void;
  onAction?: (action: ConfigAction, next: Config) => void;
  agentProviderStatus?: AgentProviderStatusView[] | null;
  agentProviderStatusError?: string | null;
  browserImportDestinationProfiles?: BrowserImportDestinationProfileView[] | null;
  browserImportDestinationProfilesError?: string | null;
  browserImportProfiles?: BrowserImportProfileView[] | null;
  browserImportProfilesError?: string | null;
  browserImportInitialWizardStep?: BrowserImportWizardStep;
  browserImportStartStatus?: string | null;
  globalHotkeyStatus?: GlobalHotkeyStatusView | null;
  globalHotkeyStatusError?: string | null;
  mobilePairingStatus?: MobilePairingStatusView | null;
  mobilePairingStatusError?: string | null;
  updaterStatus?: UpdaterStatusView | null;
  updaterStatusError?: string | null;
  cliInstallStatus?: CliInstallStatusView | null;
  cliInstallStatusError?: string | null;
  configExtensionStatus?: ConfigExtensionStatusView | null;
  configExtensionStatusError?: string | null;
  desktopCoreStatus?: DesktopCoreStatusView | null;
  desktopCoreStatusError?: string | null;
  defaultTerminalStatus?: DefaultTerminalStatusView | null;
  defaultTerminalStatusError?: string | null;
  controlSocketStatus?: ControlSocketStatusView | null;
  controlSocketStatusError?: string | null;
  vscodeInlineAvailable?: boolean | null;
  vscodeInlineStatusError?: string | null;
  onRefreshAgentProviderStatus?: () => void;
  onRefreshBrowserImportProfiles?: () => void;
  onStartBrowserImport?: (request: BrowserImportStartRequest) => void;
  onRefreshGlobalHotkeyStatus?: () => void;
  onRefreshMobilePairingStatus?: () => void;
  onRefreshUpdaterStatus?: () => void;
  onInstallCli?: () => void;
  onUninstallCli?: () => void;
  onRefreshCliInstallStatus?: () => void;
  onRefreshConfigExtensionStatus?: () => void;
  onRefreshDesktopCoreStatus?: () => void;
  onMakeDefaultTerminal?: () => void;
  onRefreshDefaultTerminalStatus?: () => void;
  onRefreshVSCodeInlineStatus?: () => void;
  onOpenFolderInVSCodeInline?: () => void;
  onRestartVSCodeServeWeb?: () => void;
  onStopVSCodeServeWeb?: () => void;
  onRefreshControlSocketStatus?: () => void;
  onRestartControlSocket?: () => void;
  onOpenTaskManager?: () => void;
  onRestorePreviousLaunch?: () => void;
  notificationCommandTestStatus?: string | null;
  onTestNotificationCommand?: () => void;
  notificationDeliveryPreviewStatus?: string | null;
  onPreviewNotificationDelivery?: () => void;
  notificationToastSendStatus?: string | null;
  onSendTestNotificationToast?: () => void;
  onOpenSettingsFile?: () => void;
  onOpenSettingsFileInCmux?: () => void;
  onOpenGhosttySettingsFile?: () => void;
  rawSettingsPath?: string | null;
  rawSettingsDraft?: string;
  rawSettingsLoading?: boolean;
  rawSettingsSaving?: boolean;
  rawSettingsError?: string | null;
  rawSettingsStatus?: string | null;
  onLoadRawSettings?: () => void;
  onRawSettingsDraftChange?: (draft: string) => void;
  onSaveRawSettings?: () => void;
  onResetConfig?: () => void;
  searchQuery?: string;
  onSearchQueryChange?: (query: string) => void;
  onNavigate?: (section: SettingsPaneSection) => void;
  rightSidebarBetaSettings?: RightSidebarBetaSettingsView | null;
  onSetRightSidebarBetaFeature?: (
    feature: "feed" | "dock",
    enabled: boolean,
  ) => void;
}

export interface RightSidebarBetaSettingsView {
  feed_enabled: boolean;
  dock_enabled: boolean;
}

export interface AgentProviderStatusView {
  id: string;
  display_name: string;
  executable_name: string;
  transport_kind: string;
  available: boolean;
  executable_path?: string | null;
  searched_directories: string[];
}

export interface BrowserImportProfileView {
  browser_id: string;
  browser_name: string;
  profile_name: string;
  profile_path: string;
  bookmarks_path?: string | null;
  history_path?: string | null;
  cookies_path?: string | null;
  importable_items: string[];
}

export type BrowserImportDestinationProfileView = BrowserImportDestinationProfile;

export type BrowserImportWizardStep = 1 | 2 | 3;

export interface UpdaterStatusView {
  platform: string;
  current_version: string;
  backend: string;
  configured: boolean;
  check_supported: boolean;
  install_supported: boolean;
  reason?: string | null;
}

export interface GlobalHotkeyStatusView {
  platform: string;
  backend: string;
  available: boolean;
  registered: boolean;
  reason?: string | null;
}

export interface MobilePairingStatusView {
  platform: string;
  backend: string;
  available: boolean;
  running: boolean;
  pairing_supported: boolean;
  advertised_endpoint?: string | null;
  reason?: string | null;
}

export interface CliInstallStatusView {
  installed_in_path: boolean;
  shim_path: string;
  shim_directory: string;
  bundled_cli_path?: string | null;
}

export interface ConfigExtensionStatusView {
  config_path: string;
  actions_count: number;
  commands_count: number;
  surface_tab_bar_buttons_count: number;
  ui_configured: boolean;
  workspace_groups_count: number;
  unknown_top_level_keys: string[];
  beta_feature_keys: string[];
}

export interface DesktopCoreStatusView {
  milestone: string;
  platform: string;
  agent_providers: string[];
  ipc_fixture_request: string;
}

export interface DefaultTerminalStatusView {
  is_default: boolean;
  command: string;
}

export interface ControlSocketStatusView {
  pipe: string;
  running: boolean;
}

interface FlagRow<K> {
  key: K;
  label: string;
}

interface TextRow<K> {
  key: K;
  label: string;
  description?: string;
}

type NotificationTextKey = Exclude<NotificationsStringKey, "sound">;

// Order + labels mirror Settings > Sidebar. Only boolean flags are toggles;
// `branchLayout` and `right_max_width` are edited elsewhere.
const SIDEBAR_FLAGS: ReadonlyArray<FlagRow<SidebarBoolKey>> = [
  { key: "hideAllDetails", label: "Hide all details" },
  { key: "wrapWorkspaceTitles", label: "Wrap workspace titles" },
  { key: "showWorkspaceDescription", label: "Show workspace description" },
  { key: "showNotificationMessage", label: "Show notification message" },
  { key: "showBranchDirectory", label: "Show branch directory" },
  { key: "showPullRequests", label: "Show pull requests" },
  { key: "watchGitStatus", label: "Watch git status" },
  { key: "makePullRequestsClickable", label: "Make pull requests clickable" },
  {
    key: "openPullRequestLinksInCmuxBrowser",
    label: "Open pull request links in cmux browser",
  },
  { key: "openPortLinksInCmuxBrowser", label: "Open port links in cmux browser" },
  { key: "showSSH", label: "Show SSH" },
  { key: "showPorts", label: "Show ports" },
  { key: "showLog", label: "Show log" },
  { key: "showProgress", label: "Show progress" },
  { key: "showCustomMetadata", label: "Show custom metadata" },
];

// Order + labels mirror Settings > Notifications (boolean flags only).
const NOTIFICATION_FLAGS: ReadonlyArray<FlagRow<NotificationsBoolKey>> = [
  { key: "dockBadge", label: "Dock badge" },
  { key: "showInMenuBar", label: "Show in menu bar" },
  { key: "unreadPaneRing", label: "Unread pane ring" },
  { key: "paneFlash", label: "Pane flash" },
];

const NOTIFICATION_SOUND_OPTIONS: ReadonlyArray<{ value: string; label: string }> = [
  { value: "default", label: "Default" },
  { value: "Basso", label: "Basso" },
  { value: "Blow", label: "Blow" },
  { value: "Bottle", label: "Bottle" },
  { value: "Frog", label: "Frog" },
  { value: "Funk", label: "Funk" },
  { value: "Glass", label: "Glass" },
  { value: "Hero", label: "Hero" },
  { value: "Morse", label: "Morse" },
  { value: "Ping", label: "Ping" },
  { value: "Pop", label: "Pop" },
  { value: "Purr", label: "Purr" },
  { value: "Sosumi", label: "Sosumi" },
  { value: "Submarine", label: "Submarine" },
  { value: "Tink", label: "Tink" },
  { value: "custom_file", label: "Custom File..." },
  { value: "none", label: "None" },
];

const NOTIFICATION_STRING_FIELDS: ReadonlyArray<TextRow<NotificationTextKey>> = [
  {
    key: "customSoundFilePath",
    label: "Custom sound file",
    description: "Path to a WAV, MP3, CAF, or AIFF file.",
  },
  {
    key: "command",
    label: "Notification command",
    description: "Shell command to run for notification hooks.",
  },
];

const NOTIFICATION_HOOKS_MODE_OPTIONS: ReadonlyArray<{
  value: HooksMode;
  label: string;
}> = [
  { value: "append", label: "Append" },
  { value: "replace", label: "Replace" },
];

const APP_FLAGS: ReadonlyArray<FlagRow<AppBoolKey>> = [
  { key: "menuBarOnly", label: "Menu bar only" },
  { key: "workspaceInheritWorkingDirectory", label: "New workspaces inherit cwd" },
  { key: "minimalMode", label: "Minimal mode" },
  {
    key: "keepWorkspaceOpenWhenClosingLastSurface",
    label: "Keep workspace open when closing last surface",
  },
  { key: "focusPaneOnFirstClick", label: "Focus pane on first click" },
  { key: "openSupportedFilesInCmux", label: "Open supported files in cmux" },
  { key: "openMarkdownInCmuxViewer", label: "Open Markdown in cmux viewer" },
  { key: "reorderOnNotification", label: "Reorder workspaces on notification" },
  { key: "iMessageMode", label: "iMessage mode" },
  { key: "sendAnonymousTelemetry", label: "Send anonymous telemetry" },
  { key: "warnBeforeQuit", label: "Warn before quit" },
  { key: "warnBeforeClosingTab", label: "Warn before closing tab" },
  { key: "warnBeforeClosingTabXButton", label: "Warn before closing with X button" },
  { key: "hideTabCloseButton", label: "Hide tab close button" },
  { key: "renameSelectsExistingName", label: "Rename selects existing name" },
  {
    key: "commandPaletteSearchesAllSurfaces",
    label: "Command palette searches all surfaces",
  },
];

const SIDEBAR_BRANCH_LAYOUT_OPTIONS: ReadonlyArray<{
  value: BranchLayout;
  label: string;
}> = [
  { value: "vertical", label: "Vertical" },
  { value: "inline", label: "Inline" },
];

const SIDEBAR_APPEARANCE_STRING_FIELDS: ReadonlyArray<
  TextRow<SidebarAppearanceStringKey>
> = [
  { key: "tintColor", label: "Tint color" },
  { key: "lightModeTintColor", label: "Light mode tint color" },
  { key: "darkModeTintColor", label: "Dark mode tint color" },
];

const SIDEBAR_APPEARANCE_NUMBER_FIELDS: ReadonlyArray<
  TextRow<SidebarAppearanceNumberKey>
> = [
  {
    key: "tintOpacity",
    label: "Tint opacity",
    description: "0 is transparent; 1 is fully opaque.",
  },
];

const WORKSPACE_COLORS_NULLABLE_FIELDS: ReadonlyArray<
  TextRow<WorkspaceColorsNullableStringKey>
> = [
  {
    key: "selectionColor",
    label: "Selection color",
    description: "Active workspace selection highlight color. Leave blank for default.",
  },
  {
    key: "notificationBadgeColor",
    label: "Notification badge color",
    description: "Unread notification badge color. Leave blank for default.",
  },
];

const WORKSPACE_COLORS_MAP_FIELDS: ReadonlyArray<TextRow<WorkspaceColorsMapKey>> = [
  {
    key: "colors",
    label: "Named color palette",
    description: "One name=value entry per line, for example Blue=#1565C0.",
  },
  {
    key: "paletteOverrides",
    label: "Palette overrides",
    description: "Override named palette entries with one name=value entry per line.",
  },
];

const WORKSPACE_COLORS_LIST_FIELDS: ReadonlyArray<TextRow<WorkspaceColorsListKey>> = [
  {
    key: "customColors",
    label: "Custom colors",
    description: "One color value per line.",
  },
];

const APP_STRING_FIELDS: ReadonlyArray<TextRow<AppStringKey>> = [
  {
    key: "language",
    label: "Language",
    description: "Use system for the OS language, or a locale tag such as en or pt-BR.",
  },
  {
    key: "windowTitleTemplate",
    label: "Window title template",
    description: "Template used for the native window title.",
  },
  {
    key: "preferredEditor",
    label: "Preferred editor",
    description: "Command used when opening files in an external editor.",
  },
];

const APP_NUMBER_FIELDS: ReadonlyArray<TextRow<AppNumberKey>> = [
  {
    key: "globalFontMagnification",
    label: "Global font magnification",
    description: "Percentage scale applied to app text.",
  },
];

const APP_ICON_OPTIONS: ReadonlyArray<{ value: AppIcon; label: string }> = [
  { value: "automatic", label: "Automatic" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const NEW_WORKSPACE_PLACEMENT_OPTIONS: ReadonlyArray<{
  value: NewWorkspacePlacement;
  label: string;
}> = [
  { value: "afterCurrent", label: "After current" },
  { value: "top", label: "Top" },
  { value: "end", label: "End" },
];

const FORK_DESTINATION_OPTIONS: ReadonlyArray<{
  value: ForkDestination;
  label: string;
}> = [
  { value: "right", label: "Right split" },
  { value: "left", label: "Left split" },
  { value: "top", label: "Top split" },
  { value: "bottom", label: "Bottom split" },
  { value: "newTab", label: "New tab" },
  { value: "newWorkspace", label: "New workspace" },
];

const CONFIRM_QUIT_OPTIONS: ReadonlyArray<{ value: ConfirmQuit; label: string }> = [
  { value: "always", label: "Always" },
  { value: "dirty-only", label: "Dirty only" },
  { value: "never", label: "Never" },
];

const AUTOMATION_SOCKET_MODE_OPTIONS: ReadonlyArray<{
  value: string;
  label: string;
}> = [
  { value: "cmuxOnly", label: "cmux only" },
  { value: "password", label: "Password" },
  { value: "allow", label: "Allow" },
  { value: "disabled", label: "Disabled" },
];

const AUTOMATION_KIRO_NOTIFICATION_OPTIONS: ReadonlyArray<{
  value: KiroNotificationLevel;
  label: string;
}> = [
  { value: "minimal", label: "Minimal" },
  { value: "standard", label: "Standard" },
  { value: "verbose", label: "Verbose" },
];

const AUTOMATION_FLAGS: ReadonlyArray<FlagRow<AutomationBoolKey>> = [
  { key: "claudeCodeIntegration", label: "Claude Code integration" },
  { key: "codexIntegration", label: "Codex integration" },
  { key: "opencodeIntegration", label: "OpenCode integration" },
  { key: "workspaceAutoNaming", label: "Workspace auto-naming" },
  { key: "suppressSubagentNotifications", label: "Suppress subagent notifications" },
  { key: "ampIntegration", label: "Amp integration" },
  { key: "cursorIntegration", label: "Cursor integration" },
  { key: "geminiIntegration", label: "Gemini integration" },
  { key: "kiroIntegration", label: "Kiro integration" },
];

const AUTOMATION_STRING_FIELDS: ReadonlyArray<TextRow<AutomationStringKey>> = [
  {
    key: "claudeBinaryPath",
    label: "Claude binary path",
    description: "Leave blank to resolve claude from PATH.",
  },
  {
    key: "autoNamingAgent",
    label: "Auto-naming agent",
    description: "Use auto for the default workspace naming agent.",
  },
  {
    key: "ripgrepBinaryPath",
    label: "Ripgrep binary path",
    description: "Leave blank to resolve rg from PATH.",
  },
];

const AUTOMATION_NUMBER_FIELDS: ReadonlyArray<
  TextRow<AutomationNumberKey> & { min: number; max?: number; step: number }
> = [
  {
    key: "portBase",
    label: "Port base",
    description: "First CMUX_PORT value assigned to workspace processes.",
    min: 1,
    max: 65535,
    step: 1,
  },
  {
    key: "portRange",
    label: "Port range size",
    description: "Number of ports available for workspace process assignment.",
    min: 1,
    step: 1,
  },
];

const BROWSER_FLAGS: ReadonlyArray<FlagRow<BrowserBoolKey>> = [
  {
    key: "openTerminalLinksInCmuxBrowser",
    label: "Open terminal links in cmux browser",
  },
  {
    key: "interceptTerminalOpenCommandInCmuxBrowser",
    label: "Intercept terminal open commands in cmux browser",
  },
  { key: "showSearchSuggestions", label: "Show search suggestions" },
  { key: "discardHiddenWebViews", label: "Discard hidden webviews" },
  { key: "showImportHintOnBlankTabs", label: "Show import hint on blank browser tabs" },
];

const BROWSER_SEARCH_ENGINE_OPTIONS: ReadonlyArray<{ value: string; label: string }> = [
  { value: "google", label: "Google" },
  { value: "duckduckgo", label: "DuckDuckGo" },
  { value: "bing", label: "Bing" },
  { value: "kagi", label: "Kagi" },
  { value: "brave", label: "Brave Search" },
  { value: "startpage", label: "Startpage" },
  { value: "perplexity", label: "Perplexity" },
  { value: "exa", label: "Exa" },
  { value: "yahoo", label: "Yahoo" },
  { value: "ecosia", label: "Ecosia" },
  { value: "qwant", label: "Qwant" },
  { value: "mojeek", label: "Mojeek" },
  { value: "wikipedia", label: "Wikipedia" },
  { value: "github", label: "GitHub" },
  { value: "baidu", label: "Baidu" },
  { value: "yandex", label: "Yandex" },
  { value: "custom", label: "Custom" },
];

const BROWSER_THEME_OPTIONS: ReadonlyArray<{ value: Appearance; label: string }> = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

const BROWSER_STRING_FIELDS: ReadonlyArray<TextRow<BrowserStringKey>> = [
  {
    key: "customSearchEngineName",
    label: "Custom search engine name",
    description: "Shown when the default search engine is set to Custom.",
  },
  {
    key: "customSearchEngineURLTemplate",
    label: "Custom search URL template",
    description: "Use {query} where the escaped search text should be inserted.",
  },
  {
    key: "reactGrabVersion",
    label: "React Grab version",
    description: "Version used by the browser React inspector helper.",
  },
];

const BROWSER_NUMBER_FIELDS: ReadonlyArray<TextRow<BrowserNumberKey>> = [
  {
    key: "hiddenWebViewDiscardDelaySeconds",
    label: "Hidden webview discard delay",
    description: "Seconds to wait before unloading hidden browser webviews.",
  },
];

const BROWSER_LIST_FIELDS: ReadonlyArray<TextRow<BrowserStringListKey>> = [
  {
    key: "hostsToOpenInEmbeddedBrowser",
    label: "Hosts to open in embedded browser",
    description: "One host or wildcard per line.",
  },
  {
    key: "urlsToAlwaysOpenExternally",
    label: "URLs to always open externally",
    description: "One URL pattern per line.",
  },
  {
    key: "insecureHttpHostsAllowedInEmbeddedBrowser",
    label: "HTTP hosts allowed in embedded browser",
    description: "One local or trusted non-HTTPS host per line.",
  },
];

const TERMINAL_FLAGS: ReadonlyArray<FlagRow<TerminalBoolKey>> = [
  { key: "showScrollBar", label: "Show terminal scroll bar" },
  { key: "copyOnSelect", label: "Copy on select" },
  { key: "autoResumeAgentSessions", label: "Auto-resume agent sessions" },
  { key: "showTextBoxOnNewTerminals", label: "Show TextBox on new terminals" },
  { key: "focusTextBoxOnNewTerminals", label: "Focus TextBox on new terminals" },
];

const TERMINAL_NUMBER_FIELDS: ReadonlyArray<
  TextRow<TerminalNumberKey> & { min: number; step: number }
> = [
  {
    key: "scrollSpeed",
    label: "Scroll speed",
    description: "Multiplier applied to terminal wheel and trackpad scrolling.",
    min: 0,
    step: 0.1,
  },
  {
    key: "textBoxMaxLines",
    label: "TextBox max lines",
    description: "Maximum height of the rich input prompt before it scrolls.",
    min: 1,
    step: 1,
  },
];

const TERMINAL_NESTED_FLAGS: ReadonlyArray<{
  section: TerminalNestedSection;
  key: TerminalNestedBoolKey;
  label: string;
}> = [
  {
    section: "agentHibernation",
    key: "enabled",
    label: "Enable agent hibernation",
  },
  {
    section: "rendererRealization",
    key: "enabled",
    label: "Enable renderer realization",
  },
];

const TERMINAL_NESTED_NUMBER_FIELDS: ReadonlyArray<{
  section: TerminalNestedSection;
  key: TerminalNestedNumberKey;
  label: string;
  description?: string;
  min: number;
  step: number;
}> = [
  {
    section: "agentHibernation",
    key: "idleSeconds",
    label: "Agent hibernation idle seconds",
    min: 0,
    step: 1,
  },
  {
    section: "agentHibernation",
    key: "maxLiveTerminals",
    label: "Max live terminals",
    min: 1,
    step: 1,
  },
  {
    section: "rendererRealization",
    key: "idleSeconds",
    label: "Renderer realization idle seconds",
    min: 0,
    step: 1,
  },
  {
    section: "rendererRealization",
    key: "maxWarmRenderers",
    label: "Max warm renderers",
    min: 0,
    step: 1,
  },
];

const MARKDOWN_STRING_FIELDS: ReadonlyArray<TextRow<MarkdownStringKey>> = [
  {
    key: "fontFamily",
    label: "Markdown font family",
    description: "Leave blank to use the viewer default font stack.",
  },
];

const MARKDOWN_NUMBER_FIELDS: ReadonlyArray<
  TextRow<MarkdownNumberKey> & { min: number; step: number }
> = [
  {
    key: "fontSize",
    label: "Markdown font size",
    description: "Font size in points for the built-in markdown viewer.",
    min: 8,
    step: 1,
  },
  {
    key: "maxWidth",
    label: "Markdown max width",
    description: "Maximum reading column width in pixels.",
    min: 320,
    step: 10,
  },
];

const CANVAS_FLAGS: ReadonlyArray<FlagRow<CanvasBoolKey>> = [
  { key: "snappingEnabled", label: "Enable canvas snapping" },
];

const CANVAS_NUMBER_FIELDS: ReadonlyArray<
  TextRow<CanvasNumberKey> & { min: number; step: number }
> = [
  {
    key: "paneGap",
    label: "Canvas pane gap",
    description: "Spacing between panes in freeform canvas layouts.",
    min: 0,
    step: 1,
  },
];

const FILE_EDITOR_FLAGS: ReadonlyArray<FlagRow<FileEditorBoolKey>> = [
  { key: "wordWrap", label: "File editor word wrap" },
];

const FILE_EXPLORER_DOUBLE_CLICK_OPTIONS: ReadonlyArray<{
  value: DoubleClickAction;
  label: string;
}> = [
  { value: "preview", label: "Preview" },
  { value: "defaultEditor", label: "Default editor" },
  { value: "preferredEditor", label: "Preferred editor" },
];

const DIFF_VIEWER_LAYOUT_OPTIONS: ReadonlyArray<{
  value: DiffLayout;
  label: string;
}> = [
  { value: "unified", label: "Unified" },
  { value: "split", label: "Split" },
];

const APPEARANCE_OPTIONS: ReadonlyArray<{ value: Appearance; label: string }> = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

function stringListDraft(values: Array<string>): string {
  return values.join("\n");
}

function stringListFromDraft(draft: string): Array<string> {
  return draft
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line !== "");
}

function stringMapDraft(values: Record<string, string | undefined>): string {
  return Object.entries(values)
    .filter((entry): entry is [string, string] => entry[1] !== undefined)
    .map(([key, value]) => `${key}=${value}`)
    .join("\n");
}

function stringMapFromDraft(draft: string): Record<string, string> {
  const entries: Record<string, string> = {};
  for (const rawLine of draft.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line === "") {
      continue;
    }
    const separatorIndex = line.indexOf("=");
    if (separatorIndex <= 0) {
      continue;
    }
    const key = line.slice(0, separatorIndex).trim();
    const value = line.slice(separatorIndex + 1).trim();
    if (key !== "" && value !== "") {
      entries[key] = value;
    }
  }
  return entries;
}

function appSelectValue<K extends AppSelectKey>(
  app: AppConfig,
  key: K,
): AppConfig[K] {
  return app[key];
}

export function SettingsPane({
  config,
  onChange,
    onAction,
    agentProviderStatus,
    agentProviderStatusError,
    browserImportDestinationProfiles,
    browserImportDestinationProfilesError,
    browserImportProfiles,
    browserImportProfilesError,
    browserImportInitialWizardStep = 1,
    browserImportStartStatus,
    globalHotkeyStatus,
    globalHotkeyStatusError,
    mobilePairingStatus,
    mobilePairingStatusError,
    updaterStatus,
    updaterStatusError,
    cliInstallStatus,
    cliInstallStatusError,
    configExtensionStatus,
    configExtensionStatusError,
    desktopCoreStatus,
  desktopCoreStatusError,
  defaultTerminalStatus,
  defaultTerminalStatusError,
  controlSocketStatus,
  controlSocketStatusError,
  vscodeInlineAvailable,
  vscodeInlineStatusError,
  onRefreshAgentProviderStatus,
  onRefreshBrowserImportProfiles,
  onStartBrowserImport,
  onRefreshGlobalHotkeyStatus,
  onRefreshMobilePairingStatus,
  onRefreshUpdaterStatus,
  onInstallCli,
  onUninstallCli,
  onRefreshCliInstallStatus,
  onRefreshConfigExtensionStatus,
  onRefreshDesktopCoreStatus,
  onMakeDefaultTerminal,
  onRefreshDefaultTerminalStatus,
  onRefreshVSCodeInlineStatus,
  onOpenFolderInVSCodeInline,
  onRestartVSCodeServeWeb,
  onStopVSCodeServeWeb,
  onRefreshControlSocketStatus,
  onRestartControlSocket,
  onOpenTaskManager,
  onRestorePreviousLaunch,
  notificationCommandTestStatus,
  onTestNotificationCommand,
  notificationDeliveryPreviewStatus,
  onPreviewNotificationDelivery,
  notificationToastSendStatus,
  onSendTestNotificationToast,
  onOpenSettingsFile,
  onOpenSettingsFileInCmux,
  onOpenGhosttySettingsFile,
  rawSettingsPath,
  rawSettingsDraft = "",
  rawSettingsLoading = false,
  rawSettingsSaving = false,
  rawSettingsError,
  rawSettingsStatus,
  onLoadRawSettings,
  onRawSettingsDraftChange,
  onSaveRawSettings,
  onResetConfig,
  searchQuery,
  onSearchQueryChange,
  onNavigate,
  rightSidebarBetaSettings,
  onSetRightSidebarBetaFeature,
}: SettingsPaneProps) {
  const {
    sidebar,
    notifications,
    app,
    automation,
    browser,
    terminal,
    markdown,
    canvas,
    file_editor,
    file_explorer,
    diff_viewer,
    workspace_colors,
    shortcuts,
    sidebar_appearance,
  } = config;
  const hasAppSection =
    app != null ||
    markdown != null ||
    canvas != null ||
    file_editor != null ||
    file_explorer != null ||
    diff_viewer != null;
  const [browserImportSelectedPathsDraft, setBrowserImportSelectedPathsDraft] =
    useState<string[] | null>(null);
  const [browserImportModeDraft, setBrowserImportModeDraft] =
    useState<BrowserImportMode | null>(null);
  const [browserImportWizardStep, setBrowserImportWizardStep] =
    useState<BrowserImportWizardStep>(browserImportInitialWizardStep);
  const [browserImportDestinationProfileIdDraft, setBrowserImportDestinationProfileIdDraft] =
    useState<string | null>(null);
  const [browserImportSeparateDestinationChoices, setBrowserImportSeparateDestinationChoices] =
    useState<Record<string, BrowserImportSeparateDestinationChoice>>({});
  const [browserImportIncludeCookies, setBrowserImportIncludeCookies] =
    useState(true);
  const [browserImportIncludeHistory, setBrowserImportIncludeHistory] =
    useState(true);
  const [browserImportIncludeAdditionalData, setBrowserImportIncludeAdditionalData] =
    useState(false);
  const browserImportProfilePaths =
    browserImportProfiles?.map((profile) => profile.profile_path) ?? [];
  const browserImportSelectedPaths = (
    browserImportSelectedPathsDraft ?? browserImportProfilePaths
  ).filter((path) => browserImportProfilePaths.includes(path));
  const browserImportSelectedProfiles =
    browserImportProfiles?.filter((profile) =>
      browserImportSelectedPaths.includes(profile.profile_path),
    ) ?? [];
  const browserImportBrowserNames = [
    ...new Set(browserImportProfiles?.map((profile) => profile.browser_name) ?? []),
  ];
  const showBrowserImportSourceStep = browserImportWizardStep >= 2;
  const showBrowserImportOptionsStep = browserImportWizardStep >= 3;
  const browserImportMode =
    browserImportModeDraft ?? defaultBrowserImportMode(browserImportSelectedProfiles);
  const browserImportDestinations = normalizedDestinationProfiles(
    browserImportDestinationProfiles,
  );
  const defaultBrowserImportDestinationProfileId =
    browserImportDestinations.find((profile) => profile.is_default)?.id ??
    browserImportDestinations[0].id;
  const browserImportDestinationProfileId =
    browserImportDestinationProfileIdDraft != null &&
    browserImportDestinations.some(
      (profile) => profile.id === browserImportDestinationProfileIdDraft,
    )
      ? browserImportDestinationProfileIdDraft
      : defaultBrowserImportDestinationProfileId;
  const browserImportScope = browserImportScopeFromSelection({
    cookies: browserImportIncludeCookies,
    history: browserImportIncludeHistory,
    additionalData: browserImportIncludeAdditionalData,
  });
  const browserImportStartRequest = buildBrowserImportStartRequest(
    browserImportSelectedProfiles,
    {
      mode: browserImportMode,
      scope: browserImportScope,
      destinationProfiles: browserImportDestinations,
      mergeDestinationProfileId: browserImportDestinationProfileId,
      separateDestinationChoices: browserImportSeparateDestinationChoices,
    },
  );
  // Trimmed-empty restores the plain section state (SettingsWindowScene.swift:143).
  const isSearching = (searchQuery ?? "").trim() !== "";

  const searchInput = (
    <input
      type="search"
      className="cmux-settings-search"
      data-settings-search
      value={searchQuery ?? ""}
      placeholder="Search"
      aria-label="Search settings"
      onChange={(e) => onSearchQueryChange?.(e.target.value)}
    />
  );

  const dispatch = (action: ConfigAction) => {
    const next = configReducer(config, action);
    onChange(next);
    onAction?.(action, next);
  };

  if (isSearching) {
    // The flat pane is both results list and detail: while searching, only the
    // hits render. Canonical lists ALL matches; rows whose target has no web
    // pane section are disabled. Clicking clears the query (restoring the
    // pane), then navigates to the hit's parent section — the web adaptation
    // of `selectSidebarEntry` (SettingsWindowScene.swift:230-245).
    const results = settingsSearchResults(searchQuery ?? "");
    return (
      <div className="cmux-settings-pane">
        {searchInput}
        {results.length === 0 ? (
          <p className="cmux-settings-search-empty">No Results</p>
        ) : (
          <ul className="cmux-settings-search-results">
            {results.map((result) => (
              <li
                key={result.id}
                className="cmux-settings-row"
                data-result-id={result.id}
              >
                <button
                  type="button"
                  disabled={result.paneSection == null}
                  onClick={() => {
                    onSearchQueryChange?.("");
                    if (result.paneSection) {
                      onNavigate?.(result.paneSection);
                    }
                  }}
                >
                  <span className="cmux-settings-search-result-title">
                    {result.title}
                  </span>
                  {result.subtitle != null && (
                    <span className="cmux-settings-search-result-section">
                      {result.subtitle}
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    );
  }

  return (
    <div className="cmux-settings-pane">
      {searchInput}
      <section className="cmux-settings-section" data-section="account">
        <h2>Account</h2>
        <div className="cmux-settings-row">
          <div>
            <span>Agent provider CLIs</span>
            <p className="cmux-settings-row-description">
              {agentProviderStatusError ??
                (agentProviderStatus == null
                  ? "Provider status has not been loaded yet."
                  : "cmux uses your installed provider CLIs and their existing credentials.")}
            </p>
            {agentProviderStatus != null && (
              <ul className="cmux-settings-status-list">
                {agentProviderStatus.map((provider) => (
                  <li key={provider.id}>
                    <span>{provider.display_name}</span>
                    <span>
                      {provider.available
                        ? `Found ${provider.executable_path ?? provider.executable_name}`
                        : `Missing ${provider.executable_name}`}
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <div className="cmux-settings-row-actions">
            <button
              type="button"
              disabled={onRefreshAgentProviderStatus == null}
              onClick={() => onRefreshAgentProviderStatus?.()}
            >
              Refresh
            </button>
          </div>
        </div>
      </section>

      <section className="cmux-settings-section" data-section="mobile">
        <h2>Mobile</h2>
        <div className="cmux-settings-row">
          <div>
            <span>iOS pairing</span>
            <p className="cmux-settings-row-description">
              {mobilePairingStatusError ??
                (mobilePairingStatus == null
                  ? "Status has not been loaded yet."
                  : mobilePairingStatus.available
                    ? "The native mobile pairing listener is available."
                    : (mobilePairingStatus.reason ??
                      "No native mobile pairing listener is configured in this build."))}
            </p>
            {mobilePairingStatus != null && (
              <ul className="cmux-settings-status-list">
                <li>
                  <span>Platform</span>
                  <span>{mobilePairingStatus.platform}</span>
                </li>
                <li>
                  <span>Backend</span>
                  <span>{mobilePairingStatus.backend}</span>
                </li>
                <li>
                  <span>Listener</span>
                  <span>{mobilePairingStatus.running ? "Running" : "Stopped"}</span>
                </li>
                <li>
                  <span>Pairing</span>
                  <span>{mobilePairingStatus.pairing_supported ? "Available" : "Unavailable"}</span>
                </li>
                {mobilePairingStatus.advertised_endpoint != null && (
                  <li>
                    <span>Endpoint</span>
                    <span>{mobilePairingStatus.advertised_endpoint}</span>
                  </li>
                )}
              </ul>
            )}
          </div>
          <div className="cmux-settings-row-actions">
            <button
              type="button"
              disabled={onRefreshMobilePairingStatus == null}
              onClick={() => onRefreshMobilePairingStatus?.()}
            >
              Refresh
            </button>
          </div>
        </div>
      </section>

      <section className="cmux-settings-section" data-section="updates">
        <h2>Updates</h2>
        <div className="cmux-settings-row">
          <div>
            <span>Native updater</span>
            <p className="cmux-settings-row-description">
              {updaterStatusError ??
                (updaterStatus == null
                  ? "Status has not been loaded yet."
                  : updaterStatus.configured
                    ? `Update checks are available through ${updaterStatus.backend}.`
                    : (updaterStatus.reason ??
                      "No native updater backend is configured in this build."))}
            </p>
            {updaterStatus != null && (
              <ul className="cmux-settings-status-list">
                <li>
                  <span>Current version</span>
                  <span>{updaterStatus.current_version}</span>
                </li>
                <li>
                  <span>Platform</span>
                  <span>{updaterStatus.platform}</span>
                </li>
                <li>
                  <span>Backend</span>
                  <span>{updaterStatus.backend}</span>
                </li>
                <li>
                  <span>Check support</span>
                  <span>{updaterStatus.check_supported ? "Available" : "Unavailable"}</span>
                </li>
                <li>
                  <span>Install support</span>
                  <span>{updaterStatus.install_supported ? "Available" : "Unavailable"}</span>
                </li>
              </ul>
            )}
          </div>
          <div className="cmux-settings-row-actions">
            <button
              type="button"
              disabled={onRefreshUpdaterStatus == null}
              onClick={() => onRefreshUpdaterStatus?.()}
            >
              Refresh
            </button>
          </div>
        </div>
      </section>

      <section className="cmux-settings-section" data-section="customSidebars">
        <h2>Custom Sidebars</h2>
        <div className="cmux-settings-row">
          <div>
            <span>Config-backed extension surfaces</span>
            <p className="cmux-settings-row-description">
              {configExtensionStatusError ??
                (configExtensionStatus == null
                  ? "Config extension status has not been loaded yet."
                  : "cmux is reading custom actions, commands, UI placements, and workspace group metadata from cmux.json.")}
            </p>
            {configExtensionStatus != null && (
              <ul className="cmux-settings-status-list">
                <li>
                  <span>Actions</span>
                  <span>{configExtensionStatus.actions_count}</span>
                </li>
                <li>
                  <span>Commands</span>
                  <span>{configExtensionStatus.commands_count}</span>
                </li>
                <li>
                  <span>Surface tab buttons</span>
                  <span>{configExtensionStatus.surface_tab_bar_buttons_count}</span>
                </li>
                <li>
                  <span>UI placements</span>
                  <span>{configExtensionStatus.ui_configured ? "Configured" : "Default"}</span>
                </li>
                <li>
                  <span>Workspace groups</span>
                  <span>{configExtensionStatus.workspace_groups_count}</span>
                </li>
              </ul>
            )}
          </div>
          <div className="cmux-settings-row-actions">
            <button
              type="button"
              disabled={onRefreshConfigExtensionStatus == null}
              onClick={() => onRefreshConfigExtensionStatus?.()}
            >
              Refresh
            </button>
            <button
              type="button"
              disabled={onOpenSettingsFile == null}
              onClick={() => onOpenSettingsFile?.()}
            >
              Open cmux.json
            </button>
          </div>
        </div>
      </section>

      <section className="cmux-settings-section" data-section="betaFeatures">
        <h2>Beta Features</h2>
        <label className="cmux-settings-row">
          <input
            type="checkbox"
            data-field="rightSidebar.beta.feed.enabled"
            checked={rightSidebarBetaSettings?.feed_enabled ?? false}
            disabled={
              rightSidebarBetaSettings == null ||
              onSetRightSidebarBetaFeature == null
            }
            onChange={(event) =>
              onSetRightSidebarBetaFeature?.("feed", event.currentTarget.checked)
            }
          />
          <div>
            <span>Feed in right sidebar</span>
            <p className="cmux-settings-row-description">
              Shows Feed for inline agent decisions, permissions, and questions.
            </p>
          </div>
        </label>
        <div className="cmux-settings-row">
          <div>
            <span>Experimental feature flags</span>
            <p className="cmux-settings-row-description">
              {configExtensionStatusError ??
                (configExtensionStatus == null
                  ? "Config extension status has not been loaded yet."
                  : configExtensionStatus.beta_feature_keys.length === 0
                    ? "No beta or experimental feature blocks were detected in cmux.json."
                    : "cmux found beta or experimental config keys preserved in cmux.json.")}
            </p>
            {configExtensionStatus != null && (
              <ul className="cmux-settings-status-list">
                <li>
                  <span>Beta keys</span>
                  <span>{configExtensionStatus.beta_feature_keys.length}</span>
                </li>
                <li>
                  <span>Preserved unknown keys</span>
                  <span>{configExtensionStatus.unknown_top_level_keys.length}</span>
                </li>
                {configExtensionStatus.beta_feature_keys.slice(0, 5).map((key) => (
                  <li key={key}>
                    <span>{key}</span>
                    <span>cmux.json</span>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <div className="cmux-settings-row-actions">
            <button
              type="button"
              disabled={onRefreshConfigExtensionStatus == null}
              onClick={() => onRefreshConfigExtensionStatus?.()}
            >
              Refresh
            </button>
            <button
              type="button"
              disabled={onOpenSettingsFile == null}
              onClick={() => onOpenSettingsFile?.()}
            >
              Open cmux.json
            </button>
          </div>
        </div>
      </section>

      {automation && (
        <section className="cmux-settings-section" data-section="automation">
          <h2>Automation</h2>
          <div className="cmux-settings-row">
            <div>
              <span>Control socket listener</span>
              <p className="cmux-settings-row-description">
                {controlSocketStatusError ??
                  (controlSocketStatus == null
                    ? "Status has not been loaded yet."
                    : `${controlSocketStatus.running ? "Running" : "Stopped"} at ${controlSocketStatus.pipe}`)}
              </p>
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                disabled={onRefreshControlSocketStatus == null}
                onClick={() => onRefreshControlSocketStatus?.()}
              >
                Refresh
              </button>
              <button
                type="button"
                disabled={onRestartControlSocket == null}
                onClick={() => onRestartControlSocket?.()}
              >
                Restart Listener
              </button>
            </div>
          </div>
          <label className="cmux-settings-row">
            <span>Socket control mode</span>
            <select
              data-automation-field="socketControlMode"
              value={automation.socketControlMode}
              onChange={(event) =>
                dispatch({
                  type: "setAutomationString",
                  key: "socketControlMode",
                  value: event.target.value,
                })
              }
            >
              {AUTOMATION_SOCKET_MODE_OPTIONS.map(({ value, label }) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label className="cmux-settings-row">
            <div>
              <span>Socket password</span>
              <p className="cmux-settings-row-description">
                Used when socket control mode requires password authentication.
              </p>
            </div>
            <input
              type="password"
              data-automation-field="socketPassword"
              value={automation.socket_password ?? ""}
              onChange={(event) =>
                dispatch({
                  type: "setAutomationPassword",
                  key: "socket_password",
                  value:
                    event.target.value === "" ? null : event.target.value,
                })
              }
            />
          </label>
          {AUTOMATION_FLAGS.map(({ key, label }) => (
            <label key={key} className="cmux-settings-row">
              <input
                type="checkbox"
                data-automation-field={key}
                checked={automation[key]}
                onChange={() => dispatch({ type: "toggleAutomationFlag", key })}
              />
              <span>{label}</span>
            </label>
          ))}
          {AUTOMATION_STRING_FIELDS.map(({ key, label, description }) => (
            <label key={key} className="cmux-settings-row">
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <input
                type="text"
                data-automation-field={key}
                value={automation[key]}
                onChange={(event) =>
                  dispatch({
                    type: "setAutomationString",
                    key,
                    value: event.target.value,
                  })
                }
              />
            </label>
          ))}
          <label className="cmux-settings-row">
            <span>Kiro notification level</span>
            <select
              data-automation-field="kiroNotificationLevel"
              value={automation.kiroNotificationLevel}
              onChange={(event) =>
                dispatch({
                  type: "setAutomationSelect",
                  key: "kiroNotificationLevel",
                  value: event.target.value as KiroNotificationLevel,
                })
              }
            >
              {AUTOMATION_KIRO_NOTIFICATION_OPTIONS.map(({ value, label }) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          {AUTOMATION_NUMBER_FIELDS.map(({ key, label, description, min, max, step }) => (
            <label key={key} className="cmux-settings-row">
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <input
                type="number"
                min={min}
                max={max}
                step={step}
                data-automation-field={key}
                value={automation[key]}
                onChange={(event) => {
                  const rawValue = event.currentTarget.valueAsNumber;
                  if (Number.isFinite(rawValue)) {
                    const bounded =
                      max == null
                        ? Math.max(min, rawValue)
                        : Math.min(max, Math.max(min, rawValue));
                    dispatch({
                      type: "setAutomationNumber",
                      key,
                      value: bounded,
                    });
                  }
                }}
              />
            </label>
          ))}
        </section>
      )}

      {workspace_colors && (
        <section className="cmux-settings-section" data-section="workspaceColors">
          <h2>Workspace Colors</h2>
          <label className="cmux-settings-row">
            <div>
              <span>Indicator style</span>
              <p className="cmux-settings-row-description">
                Active workspace indicator style, such as leftRail, stripe, or dot.
              </p>
            </div>
            <input
              type="text"
              data-workspace-colors-field="indicatorStyle"
              value={workspace_colors.indicatorStyle}
              onChange={(event) =>
                dispatch({
                  type: "setWorkspaceColorsString",
                  key: "indicatorStyle",
                  value: event.target.value,
                })
              }
            />
          </label>
          {WORKSPACE_COLORS_NULLABLE_FIELDS.map(({ key, label, description }) => (
            <label key={key} className="cmux-settings-row">
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <input
                type="text"
                data-workspace-colors-field={key}
                value={workspace_colors[key] ?? ""}
                onChange={(event) =>
                  dispatch({
                    type: "setWorkspaceColorsNullableString",
                    key,
                    value: event.target.value === "" ? null : event.target.value,
                  })
                }
              />
            </label>
          ))}
          {WORKSPACE_COLORS_MAP_FIELDS.map(({ key, label, description }) => (
            <label
              key={key}
              className="cmux-settings-row cmux-settings-row-stacked"
            >
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <textarea
                rows={5}
                data-workspace-colors-field={key}
                value={stringMapDraft(workspace_colors[key])}
                onChange={(event) =>
                  dispatch({
                    type: "setWorkspaceColorsMap",
                    key,
                    value: stringMapFromDraft(event.target.value),
                  })
                }
              />
            </label>
          ))}
          {WORKSPACE_COLORS_LIST_FIELDS.map(({ key, label, description }) => (
            <label
              key={key}
              className="cmux-settings-row cmux-settings-row-stacked"
            >
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <textarea
                rows={3}
                data-workspace-colors-field={key}
                value={stringListDraft(workspace_colors[key])}
                onChange={(event) =>
                  dispatch({
                    type: "setWorkspaceColorsList",
                    key,
                    value: stringListFromDraft(event.target.value),
                  })
                }
              />
            </label>
          ))}
        </section>
      )}

      {sidebar && (
        <section className="cmux-settings-section" data-section="sidebar">
          <h2>Sidebar</h2>
          <label className="cmux-settings-row">
            <span>Branch layout</span>
            <select
              data-sidebar-field="branchLayout"
              value={sidebar.branchLayout}
              onChange={(event) =>
                dispatch({
                  type: "setSidebarSelect",
                  key: "branchLayout",
                  value: event.target.value as BranchLayout,
                })
              }
            >
              {SIDEBAR_BRANCH_LAYOUT_OPTIONS.map(({ value, label }) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          {SIDEBAR_FLAGS.map(({ key, label }) => (
            <label key={key} className="cmux-settings-row">
              <input
                type="checkbox"
                data-field={key}
                checked={sidebar[key]}
                onChange={() => dispatch({ type: "toggleSidebarFlag", key })}
              />
              <span>{label}</span>
            </label>
          ))}
          {sidebar_appearance && (
            <>
              <label className="cmux-settings-row">
                <input
                  type="checkbox"
                  data-sidebar-appearance-field="matchTerminalBackground"
                  checked={sidebar_appearance.matchTerminalBackground}
                  onChange={() => dispatch({ type: "toggleMatchTerminalBackground" })}
                />
                <span>Match terminal background</span>
              </label>
              {SIDEBAR_APPEARANCE_STRING_FIELDS.map(({ key, label, description }) => (
                <label key={key} className="cmux-settings-row">
                  <div>
                    <span>{label}</span>
                    {description != null && (
                      <p className="cmux-settings-row-description">{description}</p>
                    )}
                  </div>
                  <input
                    type="text"
                    data-sidebar-appearance-field={key}
                    value={sidebar_appearance[key] ?? ""}
                    onChange={(event) =>
                      dispatch({
                        type: "setSidebarAppearanceString",
                        key,
                        value:
                          key === "tintColor" || event.target.value !== ""
                            ? event.target.value
                            : null,
                      })
                    }
                  />
                </label>
              ))}
              {SIDEBAR_APPEARANCE_NUMBER_FIELDS.map(({ key, label, description }) => (
                <label key={key} className="cmux-settings-row">
                  <div>
                    <span>{label}</span>
                    {description != null && (
                      <p className="cmux-settings-row-description">{description}</p>
                    )}
                  </div>
                  <input
                    type="number"
                    min={0}
                    max={1}
                    step={0.01}
                    data-sidebar-appearance-field={key}
                    value={sidebar_appearance[key]}
                    onChange={(event) => {
                      const value = event.currentTarget.valueAsNumber;
                      if (Number.isFinite(value)) {
                        dispatch({
                          type: "setSidebarAppearanceNumber",
                          key,
                          value: Math.min(1, Math.max(0, value)),
                        });
                      }
                    }}
                  />
                </label>
              ))}
            </>
          )}
        </section>
      )}

      {notifications && (
        <section className="cmux-settings-section" data-section="notifications">
          <h2>Notifications</h2>
          <label className="cmux-settings-row">
            <span>Hooks mode</span>
            <select
              data-notification-field="hooksMode"
              value={notifications.hooksMode}
              onChange={(event) =>
                dispatch({
                  type: "setNotificationsSelect",
                  key: "hooksMode",
                  value: event.target.value as HooksMode,
                })
              }
            >
              {NOTIFICATION_HOOKS_MODE_OPTIONS.map(({ value, label }) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label className="cmux-settings-row">
            <div>
              <span>Notification sound</span>
              <p className="cmux-settings-row-description">
                Windows maps named macOS sounds to the closest toast sound event.
              </p>
            </div>
            <select
              data-notification-field="sound"
              value={notifications.sound}
              onChange={(event) =>
                dispatch({
                  type: "setNotificationsString",
                  key: "sound",
                  value: event.target.value,
                })
              }
            >
              {NOTIFICATION_SOUND_OPTIONS.map(({ value, label }) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <div className="cmux-settings-row">
            <div>
              <span>Windows toast delivery</span>
              <p className="cmux-settings-row-description">
                Preview the native toast payload, replacement tag, sound mapping,
                and custom-command effect for the current settings.
              </p>
              {notificationDeliveryPreviewStatus != null && (
                <p
                  className="cmux-settings-row-description"
                  data-notification-delivery-preview-status
                >
                  {notificationDeliveryPreviewStatus}
                </p>
              )}
              {notificationToastSendStatus != null && (
                <p
                  className="cmux-settings-row-description"
                  data-notification-toast-send-status
                >
                  {notificationToastSendStatus}
                </p>
              )}
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                data-notification-delivery-preview
                disabled={onPreviewNotificationDelivery == null}
                onClick={() => onPreviewNotificationDelivery?.()}
              >
                Preview Toast
              </button>
              <button
                type="button"
                data-notification-toast-send
                disabled={onSendTestNotificationToast == null}
                onClick={() => onSendTestNotificationToast?.()}
              >
                Send Test Toast
              </button>
            </div>
          </div>
          {NOTIFICATION_STRING_FIELDS.map(({ key, label, description }) => (
            <label key={key} className="cmux-settings-row">
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <input
                type="text"
                data-notification-field={key}
                value={notifications[key]}
                onChange={(event) =>
                  dispatch({
                    type: "setNotificationsString",
                    key,
                    value: event.target.value,
                  })
                }
              />
            </label>
          ))}
          <div className="cmux-settings-row">
            <div>
              <span>Custom command test</span>
              <p className="cmux-settings-row-description">
                Runs the configured command with CMUX_NOTIFICATION_TITLE,
                CMUX_NOTIFICATION_SUBTITLE, and CMUX_NOTIFICATION_BODY set.
              </p>
              {notificationCommandTestStatus != null && (
                <p
                  className="cmux-settings-row-description"
                  data-notification-command-status
                >
                  {notificationCommandTestStatus}
                </p>
              )}
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                data-notification-command-test
                disabled={
                  onTestNotificationCommand == null ||
                  notifications.command.trim() === ""
                }
                onClick={() => onTestNotificationCommand?.()}
              >
                Run Test Command
              </button>
            </div>
          </div>
          {NOTIFICATION_FLAGS.map(({ key, label }) => (
            <label key={key} className="cmux-settings-row">
              <input
                type="checkbox"
                data-field={key}
                checked={notifications[key]}
                onChange={() => dispatch({ type: "toggleNotificationsFlag", key })}
              />
              <span>{label}</span>
            </label>
          ))}
        </section>
      )}

      {browser && (
        <section className="cmux-settings-section" data-section="browser">
          <h2>Browser</h2>
          <div className="cmux-settings-row">
            <div>
              <span>VS Code web workspace</span>
              <p className="cmux-settings-row-description">
                Start or reuse VS Code serve-web, then open a selected folder in
                cmux's embedded browser.
              </p>
              <p className="cmux-settings-row-description">
                VS Code command:{" "}
                {vscodeInlineStatusError != null
                  ? vscodeInlineStatusError
                  : vscodeInlineAvailable == null
                    ? "checking..."
                    : vscodeInlineAvailable
                      ? "detected"
                      : "not found"}
              </p>
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                disabled={onRefreshVSCodeInlineStatus == null}
                onClick={() => onRefreshVSCodeInlineStatus?.()}
              >
                Refresh Status
              </button>
              <button
                type="button"
                disabled={
                  onOpenFolderInVSCodeInline == null ||
                  vscodeInlineAvailable === false
                }
                onClick={() => onOpenFolderInVSCodeInline?.()}
              >
                Open Folder in VS Code
              </button>
              <button
                type="button"
                disabled={onRestartVSCodeServeWeb == null}
                onClick={() => onRestartVSCodeServeWeb?.()}
              >
                Restart Server
              </button>
              <button
                type="button"
                disabled={onStopVSCodeServeWeb == null}
                onClick={() => onStopVSCodeServeWeb?.()}
              >
                Stop Server
              </button>
            </div>
          </div>
          <label className="cmux-settings-row">
            <span>Default search engine</span>
            <select
              data-browser-field="defaultSearchEngine"
              value={browser.defaultSearchEngine}
              onChange={(event) =>
                dispatch({
                  type: "setBrowserString",
                  key: "defaultSearchEngine",
                  value: event.target.value,
                })
              }
            >
              {BROWSER_SEARCH_ENGINE_OPTIONS.map(({ value, label }) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <label className="cmux-settings-row">
            <span>Browser theme</span>
            <select
              data-browser-field="theme"
              value={browser.theme}
              onChange={(event) =>
                dispatch({
                  type: "setBrowserTheme",
                  theme: event.target.value as Appearance,
                })
              }
            >
              {BROWSER_THEME_OPTIONS.map(({ value, label }) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          {BROWSER_FLAGS.map(({ key, label }) => (
            <label key={key} className="cmux-settings-row">
              <input
                type="checkbox"
                data-browser-field={key}
                checked={browser[key]}
                onChange={() => dispatch({ type: "toggleBrowserFlag", key })}
              />
              <span>{label}</span>
            </label>
          ))}
          {BROWSER_STRING_FIELDS.map(({ key, label, description }) => (
            <label key={key} className="cmux-settings-row">
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <input
                type="text"
                data-browser-field={key}
                value={browser[key]}
                onChange={(event) =>
                  dispatch({
                    type: "setBrowserString",
                    key,
                    value: event.target.value,
                  })
                }
              />
            </label>
          ))}
          {BROWSER_NUMBER_FIELDS.map(({ key, label, description }) => (
            <label key={key} className="cmux-settings-row">
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <input
                type="number"
                min={0}
                step={1}
                data-browser-field={key}
                value={browser[key]}
                onChange={(event) => {
                  const value = event.currentTarget.valueAsNumber;
                  if (Number.isFinite(value)) {
                    dispatch({
                      type: "setBrowserNumber",
                      key,
                      value: Math.max(0, value),
                    });
                  }
                }}
              />
            </label>
          ))}
          {BROWSER_LIST_FIELDS.map(({ key, label, description }) => (
            <label key={key} className="cmux-settings-row cmux-settings-row-stacked">
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <textarea
                rows={3}
                data-browser-field={key}
                value={stringListDraft(browser[key])}
                onChange={(event) =>
                  dispatch({
                    type: "setBrowserStringList",
                    key,
                    value: stringListFromDraft(event.target.value),
                  })
                }
              />
            </label>
          ))}
        </section>
      )}

      <section className="cmux-settings-section" data-section="browserImport">
        <h2>Import Browser Data</h2>
        <div className="cmux-settings-row">
          <div>
            <span>Detected browser profiles</span>
            <p className="cmux-settings-row-description">
              {browserImportProfilesError ??
                (browserImportProfiles == null
                  ? "Browser profile detection has not been loaded yet."
                  : browserImportProfiles.length === 0
                    ? "No importable Chrome, Edge, Brave, Chromium, or Firefox profiles were detected."
                    : "cmux found browser profiles with importable bookmarks, history, or cookies.")}
            </p>
            {browserImportProfiles != null &&
              browserImportProfiles.length > 0 &&
              !showBrowserImportSourceStep && (
                <div className="cmux-browser-import-wizard" role="group">
                  <p className="cmux-settings-row-description">
                    cmux is ready to import from{" "}
                    {browserImportBrowserNames.join(", ") || "detected browsers"}.
                  </p>
                </div>
              )}
            {browserImportProfiles != null &&
              browserImportProfiles.length > 0 &&
              showBrowserImportSourceStep && (
              <ul className="cmux-settings-status-list">
                {browserImportProfiles.map((profile) => (
                  <li key={`${profile.browser_id}:${profile.profile_path}`}>
                    <label className="cmux-browser-import-source-row">
                      <input
                        type="checkbox"
                        checked={browserImportSelectedPaths.includes(profile.profile_path)}
                        aria-label={profile.profile_name}
                        onChange={(event) => {
                          const selected = new Set(browserImportSelectedPaths);
                          if (event.currentTarget.checked) {
                            selected.add(profile.profile_path);
                          } else {
                            selected.delete(profile.profile_path);
                          }
                          setBrowserImportSelectedPathsDraft([...selected]);
                          setBrowserImportModeDraft(null);
                        }}
                      />
                      <span>
                        {profile.browser_name} - {profile.profile_name}
                      </span>
                    </label>
                    <span>{profile.importable_items.join(", ")}</span>
                  </li>
                ))}
              </ul>
            )}
            {browserImportProfiles != null &&
              browserImportProfiles.length > 0 &&
              showBrowserImportOptionsStep && (
              <div className="cmux-browser-import-wizard" role="group">
                {browserImportDestinationProfilesError != null && (
                  <p className="cmux-settings-row-description">
                    {browserImportDestinationProfilesError}
                  </p>
                )}
                <div className="cmux-settings-inline-options">
                  <label>
                    <input
                      type="checkbox"
                      aria-label="BrowserImportCookiesCheckbox"
                      checked={browserImportIncludeCookies}
                      onChange={(event) =>
                        setBrowserImportIncludeCookies(event.currentTarget.checked)
                      }
                    />
                    Cookies
                  </label>
                  <label>
                    <input
                      type="checkbox"
                      aria-label="BrowserImportHistoryCheckbox"
                      checked={browserImportIncludeHistory}
                      onChange={(event) =>
                        setBrowserImportIncludeHistory(event.currentTarget.checked)
                      }
                    />
                    History
                  </label>
                  <label>
                    <input
                      type="checkbox"
                      aria-label="BrowserImportAdditionalDataCheckbox"
                      checked={browserImportIncludeAdditionalData}
                      onChange={(event) =>
                        setBrowserImportIncludeAdditionalData(event.currentTarget.checked)
                      }
                    />
                    Additional data
                  </label>
                </div>
                {browserImportSelectedProfiles.length > 1 && (
                  <div className="cmux-settings-inline-options">
                    <label>
                      <input
                        type="radio"
                        name="browser-import-mode"
                        checked={browserImportMode === "separateProfiles"}
                        onChange={() => setBrowserImportModeDraft("separateProfiles")}
                      />
                      Separate profiles
                    </label>
                    <label>
                      <input
                        type="radio"
                        name="browser-import-mode"
                        checked={browserImportMode === "mergeIntoOne"}
                        onChange={() => setBrowserImportModeDraft("mergeIntoOne")}
                      />
                      Merge into one
                    </label>
                  </div>
                )}
                {(browserImportMode === "mergeIntoOne" ||
                  browserImportMode === "singleDestination") && (
                  <label className="cmux-settings-inline-field">
                    Destination
                    <select
                      aria-label={
                        browserImportMode === "mergeIntoOne"
                          ? "BrowserImportDestinationPopup-merge"
                          : "BrowserImportDestinationPopup-single"
                      }
                      value={browserImportDestinationProfileId}
                      onChange={(event) =>
                        setBrowserImportDestinationProfileIdDraft(event.currentTarget.value)
                      }
                    >
                      {browserImportDestinations.map((profile) => (
                        <option key={profile.id} value={profile.id}>
                          {profile.display_name}
                        </option>
                      ))}
                    </select>
                  </label>
                )}
                {browserImportMode === "separateProfiles" &&
                  browserImportSelectedProfiles.map((profile) => (
                    <label
                      key={`destination:${profile.profile_path}`}
                      className="cmux-settings-inline-field"
                    >
                      {profile.profile_name} destination
                      <select
                        aria-label={`BrowserImportDestinationPopup-${profile.profile_name}`}
                        value={
                          browserImportSeparateDestinationChoices[profile.profile_path]
                            ?.kind === "existing"
                            ? `existing:${
                                browserImportSeparateDestinationChoices[
                                  profile.profile_path
                                ].destinationProfileId ?? ""
                              }`
                            : "create"
                        }
                        onChange={(event) => {
                          const value = event.currentTarget.value;
                          setBrowserImportSeparateDestinationChoices((choices) => {
                            const next = { ...choices };
                            if (value === "create") {
                              delete next[profile.profile_path];
                            } else {
                              next[profile.profile_path] = {
                                kind: "existing",
                                destinationProfileId: value.replace(/^existing:/, ""),
                              };
                            }
                            return next;
                          });
                        }}
                      >
                        <option value="create">Create "{profile.profile_name}"</option>
                        {browserImportDestinations.map((destination) => (
                          <option
                            key={`${profile.profile_path}:${destination.id}`}
                            value={`existing:${destination.id}`}
                          >
                            Add to {destination.display_name}
                          </option>
                        ))}
                      </select>
                    </label>
                  ))}
                {browserImportStartStatus != null && (
                  <p className="cmux-settings-row-description">
                    {browserImportStartStatus}
                  </p>
                )}
              </div>
            )}
          </div>
          <div className="cmux-settings-row-actions">
            <button
              type="button"
              disabled={onRefreshBrowserImportProfiles == null}
              onClick={() => onRefreshBrowserImportProfiles?.()}
            >
              Refresh
            </button>
            {browserImportProfiles != null &&
              browserImportProfiles.length > 0 &&
              browserImportWizardStep < 3 && (
                <button
                  type="button"
                  onClick={() =>
                    setBrowserImportWizardStep((step) =>
                      step === 1 ? 2 : 3,
                    )
                  }
                >
                  Next
                </button>
              )}
            {showBrowserImportOptionsStep && (
              <button
                type="button"
                disabled={
                  onStartBrowserImport == null ||
                  browserImportStartRequest == null ||
                  (!browserImportIncludeAdditionalData &&
                    !browserImportIncludeCookies &&
                    !browserImportIncludeHistory)
                }
                onClick={() => {
                  if (browserImportStartRequest != null) {
                    onStartBrowserImport?.(browserImportStartRequest);
                  }
                }}
              >
                Start Import
              </button>
            )}
          </div>
        </div>
      </section>

      <section className="cmux-settings-section" data-section="globalHotkey">
        <h2>Global Hotkey</h2>
        <div className="cmux-settings-row">
          <div>
            <span>System-wide shortcut</span>
            <p className="cmux-settings-row-description">
              {globalHotkeyStatusError ??
                (globalHotkeyStatus == null
                  ? "Status has not been loaded yet."
                  : globalHotkeyStatus.available
                    ? "The native shortcut registrar is available for system-wide hotkeys."
                    : (globalHotkeyStatus.reason ??
                      "No native system-wide shortcut registrar is configured in this build."))}
            </p>
            {globalHotkeyStatus != null && (
              <ul className="cmux-settings-status-list">
                <li>
                  <span>Platform</span>
                  <span>{globalHotkeyStatus.platform}</span>
                </li>
                <li>
                  <span>Backend</span>
                  <span>{globalHotkeyStatus.backend}</span>
                </li>
                <li>
                  <span>Registrar</span>
                  <span>{globalHotkeyStatus.available ? "Available" : "Unavailable"}</span>
                </li>
                <li>
                  <span>Registration</span>
                  <span>{globalHotkeyStatus.registered ? "Active" : "Inactive"}</span>
                </li>
              </ul>
            )}
          </div>
          <div className="cmux-settings-row-actions">
            <button
              type="button"
              disabled={onRefreshGlobalHotkeyStatus == null}
              onClick={() => onRefreshGlobalHotkeyStatus?.()}
            >
              Refresh
            </button>
          </div>
        </div>
      </section>

      {terminal && (
        <section className="cmux-settings-section" data-section="terminal">
          <h2>Terminal</h2>
          <div className="cmux-settings-row">
            <div>
              <span>Ghostty config files</span>
              <p className="cmux-settings-row-description">
                Open or create Ghostty's settings file using the native editor
                integration.
              </p>
            </div>
            <button
              type="button"
              disabled={onOpenGhosttySettingsFile == null}
              onClick={() => onOpenGhosttySettingsFile?.()}
            >
              Open Ghostty Settings
            </button>
          </div>
          {TERMINAL_FLAGS.map(({ key, label }) => (
            <label key={key} className="cmux-settings-row">
              <input
                type="checkbox"
                data-terminal-field={key}
                checked={terminal[key]}
                onChange={() => dispatch({ type: "toggleTerminalFlag", key })}
              />
              <span>{label}</span>
            </label>
          ))}
          {TERMINAL_NUMBER_FIELDS.map(({ key, label, description, min, step }) => (
            <label key={key} className="cmux-settings-row">
              <div>
                <span>{label}</span>
                {description != null && (
                  <p className="cmux-settings-row-description">{description}</p>
                )}
              </div>
              <input
                type="number"
                min={min}
                step={step}
                data-terminal-field={key}
                value={terminal[key]}
                onChange={(event) => {
                  const value = event.currentTarget.valueAsNumber;
                  if (Number.isFinite(value)) {
                    dispatch({
                      type: "setTerminalNumber",
                      key,
                      value: Math.max(min, value),
                    });
                  }
                }}
              />
            </label>
          ))}
          {TERMINAL_NESTED_FLAGS.map(({ section, key, label }) => (
            <label key={`${section}.${key}`} className="cmux-settings-row">
              <input
                type="checkbox"
                data-terminal-nested-field={`${section}.${key}`}
                checked={terminal[section][key]}
                onChange={() =>
                  dispatch({ type: "toggleTerminalNestedFlag", section, key })
                }
              />
              <span>{label}</span>
            </label>
          ))}
          {TERMINAL_NESTED_NUMBER_FIELDS.map(
            ({ section, key, label, description, min, step }) => {
              const nested = terminal[section];
              if (!(key in nested)) {
                return null;
              }
              const value = nested[key as keyof typeof nested];
              if (typeof value !== "number") {
                return null;
              }
              return (
                <label key={`${section}.${key}`} className="cmux-settings-row">
                  <div>
                    <span>{label}</span>
                    {description != null && (
                      <p className="cmux-settings-row-description">{description}</p>
                    )}
                  </div>
                  <input
                    type="number"
                    min={min}
                    step={step}
                    data-terminal-nested-field={`${section}.${key}`}
                    value={value}
                    onChange={(event) => {
                      const nextValue = event.currentTarget.valueAsNumber;
                      if (Number.isFinite(nextValue)) {
                        dispatch({
                          type: "setTerminalNestedNumber",
                          section,
                          key,
                          value: Math.max(min, nextValue),
                        });
                      }
                    }}
                  />
                </label>
              );
            },
          )}
          <div className="cmux-settings-row">
            <div>
              <span>Resume command approvals</span>
              <p className="cmux-settings-row-description">
                Signed approvals are managed automatically when terminal sessions are
                restored.
              </p>
            </div>
            <span data-terminal-field="resumeCommands">
              {terminal.resumeCommands.length}
            </span>
          </div>
        </section>
      )}

      {hasAppSection && (
        <section className="cmux-settings-section" data-section="appearance">
          <h2>App</h2>
          <div className="cmux-settings-row">
            <div>
              <span>Desktop core</span>
              <p className="cmux-settings-row-description">
                {desktopCoreStatusError ??
                  (desktopCoreStatus == null
                    ? "Desktop core status has not been loaded yet."
                    : `${desktopCoreStatus.platform} - ${desktopCoreStatus.milestone} - providers: ${desktopCoreStatus.agent_providers.join(", ")}`)}
              </p>
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                disabled={onRefreshDesktopCoreStatus == null}
                onClick={() => onRefreshDesktopCoreStatus?.()}
              >
                Refresh
              </button>
            </div>
          </div>
          <div className="cmux-settings-row">
            <div>
              <span>cmux CLI in PATH</span>
              <p className="cmux-settings-row-description">
                {cliInstallStatusError ??
                  (cliInstallStatus == null
                    ? "CLI install status has not been loaded yet."
                    : cliInstallStatus.installed_in_path
                      ? `Installed at ${cliInstallStatus.shim_path}`
                      : `Not installed. Shim directory: ${cliInstallStatus.shim_directory}`)}
              </p>
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                disabled={onRefreshCliInstallStatus == null}
                onClick={() => onRefreshCliInstallStatus?.()}
              >
                Refresh
              </button>
              {cliInstallStatus?.installed_in_path ? (
                <button
                  type="button"
                  disabled={onUninstallCli == null}
                  onClick={() => onUninstallCli?.()}
                >
                  Uninstall CLI
                </button>
              ) : (
                <button
                  type="button"
                  disabled={onInstallCli == null}
                  onClick={() => onInstallCli?.()}
                >
                  Install CLI
                </button>
              )}
            </div>
          </div>
          <div className="cmux-settings-row">
            <div>
              <span>Default terminal for SSH links</span>
              <p className="cmux-settings-row-description">
                {defaultTerminalStatusError ??
                  (defaultTerminalStatus == null
                    ? "Default terminal status has not been loaded yet."
                    : defaultTerminalStatus.is_default
                      ? `cmux is the SSH handler: ${defaultTerminalStatus.command}`
                      : `cmux is not the SSH handler. Candidate command: ${defaultTerminalStatus.command || "unknown"}`)}
              </p>
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                disabled={onRefreshDefaultTerminalStatus == null}
                onClick={() => onRefreshDefaultTerminalStatus?.()}
              >
                Refresh
              </button>
              <button
                type="button"
                disabled={
                  onMakeDefaultTerminal == null ||
                  defaultTerminalStatus?.is_default === true
                }
                onClick={() => onMakeDefaultTerminal?.()}
              >
                Make Default
              </button>
            </div>
          </div>
          <div className="cmux-settings-row">
            <div>
              <span>Task Manager</span>
              <p className="cmux-settings-row-description">
                Open the native cmux task manager for process and resource
                inspection.
              </p>
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                disabled={onOpenTaskManager == null}
                onClick={() => onOpenTaskManager?.()}
              >
                Open Task Manager
              </button>
            </div>
          </div>
          <div className="cmux-settings-row">
            <div>
              <span>Previous launch</span>
              <p className="cmux-settings-row-description">
                Restore the last saved workspace session snapshot.
              </p>
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                disabled={onRestorePreviousLaunch == null}
                onClick={() => onRestorePreviousLaunch?.()}
              >
                Restore Previous Launch
              </button>
            </div>
          </div>
          {app && (
            <>
              {APPEARANCE_OPTIONS.map(({ value, label }) => (
                <label key={value} className="cmux-settings-row">
                  <input
                    type="radio"
                    name="appearance"
                    data-appearance={value}
                    checked={app.appearance === value}
                    onChange={() => dispatch({ type: "setAppearance", appearance: value })}
                  />
                  <span>{label}</span>
                </label>
              ))}
              <label className="cmux-settings-row">
                <span>App icon</span>
                <select
                  data-app-field="appIcon"
                  value={appSelectValue(app, "appIcon")}
                  onChange={(event) =>
                    dispatch({
                      type: "setAppSelect",
                      key: "appIcon",
                      value: event.target.value as AppIcon,
                    })
                  }
                >
                  {APP_ICON_OPTIONS.map(({ value, label }) => (
                    <option key={value} value={value}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="cmux-settings-row">
                <span>New workspace placement</span>
                <select
                  data-app-field="newWorkspacePlacement"
                  value={appSelectValue(app, "newWorkspacePlacement")}
                  onChange={(event) =>
                    dispatch({
                      type: "setAppSelect",
                      key: "newWorkspacePlacement",
                      value: event.target.value as NewWorkspacePlacement,
                    })
                  }
                >
                  {NEW_WORKSPACE_PLACEMENT_OPTIONS.map(({ value, label }) => (
                    <option key={value} value={value}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="cmux-settings-row">
                <span>Fork conversation destination</span>
                <select
                  data-app-field="forkConversationDefaultDestination"
                  value={appSelectValue(app, "forkConversationDefaultDestination")}
                  onChange={(event) =>
                    dispatch({
                      type: "setAppSelect",
                      key: "forkConversationDefaultDestination",
                      value: event.target.value as ForkDestination,
                    })
                  }
                >
                  {FORK_DESTINATION_OPTIONS.map(({ value, label }) => (
                    <option key={value} value={value}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
              <label className="cmux-settings-row">
                <span>Confirm quit</span>
                <select
                  data-app-field="confirmQuit"
                  value={appSelectValue(app, "confirmQuit")}
                  onChange={(event) =>
                    dispatch({
                      type: "setAppSelect",
                      key: "confirmQuit",
                      value: event.target.value as ConfirmQuit,
                    })
                  }
                >
                  {CONFIRM_QUIT_OPTIONS.map(({ value, label }) => (
                    <option key={value} value={value}>
                      {label}
                    </option>
                  ))}
                </select>
              </label>
              {APP_STRING_FIELDS.map(({ key, label, description }) => (
                <label key={key} className="cmux-settings-row">
                  <div>
                    <span>{label}</span>
                    {description != null && (
                      <p className="cmux-settings-row-description">{description}</p>
                    )}
                  </div>
                  <input
                    type="text"
                    data-app-field={key}
                    value={app[key]}
                    onChange={(event) =>
                      dispatch({
                        type: "setAppString",
                        key,
                        value: event.target.value,
                      })
                    }
                  />
                </label>
              ))}
              {APP_NUMBER_FIELDS.map(({ key, label, description }) => (
                <label key={key} className="cmux-settings-row">
                  <div>
                    <span>{label}</span>
                    {description != null && (
                      <p className="cmux-settings-row-description">{description}</p>
                    )}
                  </div>
                  <input
                    type="number"
                    min={25}
                    step={1}
                    data-app-field={key}
                    value={app[key]}
                    onChange={(event) => {
                      const value = event.currentTarget.valueAsNumber;
                      if (Number.isFinite(value)) {
                        dispatch({
                          type: "setAppNumber",
                          key,
                          value: Math.max(25, value),
                        });
                      }
                    }}
                  />
                </label>
              ))}
              {APP_FLAGS.map(({ key, label }) => (
                <label key={key} className="cmux-settings-row">
                  <input
                    type="checkbox"
                    data-app-field={key}
                    checked={app[key]}
                    onChange={() => dispatch({ type: "toggleAppFlag", key })}
                  />
                  <span>{label}</span>
                </label>
              ))}
            </>
          )}
          {markdown && (
            <>
              {MARKDOWN_STRING_FIELDS.map(({ key, label, description }) => (
                <label key={key} className="cmux-settings-row">
                  <div>
                    <span>{label}</span>
                    {description != null && (
                      <p className="cmux-settings-row-description">{description}</p>
                    )}
                  </div>
                  <input
                    type="text"
                    data-markdown-field={key}
                    value={markdown[key]}
                    onChange={(event) =>
                      dispatch({
                        type: "setMarkdownString",
                        key,
                        value: event.target.value,
                      })
                    }
                  />
                </label>
              ))}
              {MARKDOWN_NUMBER_FIELDS.map(({ key, label, description, min, step }) => (
                <label key={key} className="cmux-settings-row">
                  <div>
                    <span>{label}</span>
                    {description != null && (
                      <p className="cmux-settings-row-description">{description}</p>
                    )}
                  </div>
                  <input
                    type="number"
                    min={min}
                    step={step}
                    data-markdown-field={key}
                    value={markdown[key]}
                    onChange={(event) => {
                      const value = event.currentTarget.valueAsNumber;
                      if (Number.isFinite(value)) {
                        dispatch({
                          type: "setMarkdownNumber",
                          key,
                          value: Math.max(min, value),
                        });
                      }
                    }}
                  />
                </label>
              ))}
            </>
          )}
          {canvas && (
            <>
              {CANVAS_NUMBER_FIELDS.map(({ key, label, description, min, step }) => (
                <label key={key} className="cmux-settings-row">
                  <div>
                    <span>{label}</span>
                    {description != null && (
                      <p className="cmux-settings-row-description">{description}</p>
                    )}
                  </div>
                  <input
                    type="number"
                    min={min}
                    step={step}
                    data-canvas-field={key}
                    value={canvas[key]}
                    onChange={(event) => {
                      const value = event.currentTarget.valueAsNumber;
                      if (Number.isFinite(value)) {
                        dispatch({
                          type: "setCanvasNumber",
                          key,
                          value: Math.max(min, value),
                        });
                      }
                    }}
                  />
                </label>
              ))}
              {CANVAS_FLAGS.map(({ key, label }) => (
                <label key={key} className="cmux-settings-row">
                  <input
                    type="checkbox"
                    data-canvas-field={key}
                    checked={canvas[key]}
                    onChange={() => dispatch({ type: "toggleCanvasFlag", key })}
                  />
                  <span>{label}</span>
                </label>
              ))}
            </>
          )}
          {file_editor && (
            <>
              {FILE_EDITOR_FLAGS.map(({ key, label }) => (
                <label key={key} className="cmux-settings-row">
                  <input
                    type="checkbox"
                    data-file-editor-field={key}
                    checked={file_editor[key]}
                    onChange={() => dispatch({ type: "toggleFileEditorFlag", key })}
                  />
                  <span>{label}</span>
                </label>
              ))}
            </>
          )}
          {file_explorer && (
            <label className="cmux-settings-row">
              <span>File explorer double-click action</span>
              <select
                data-file-explorer-field="doubleClickAction"
                value={file_explorer.doubleClickAction}
                onChange={(event) =>
                  dispatch({
                    type: "setFileExplorerSelect",
                    key: "doubleClickAction",
                    value: event.target.value as DoubleClickAction,
                  })
                }
              >
                {FILE_EXPLORER_DOUBLE_CLICK_OPTIONS.map(({ value, label }) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          )}
          {diff_viewer && (
            <label className="cmux-settings-row">
              <span>Diff viewer default layout</span>
              <select
                data-diff-viewer-field="defaultLayout"
                value={diff_viewer.defaultLayout}
                onChange={(event) =>
                  dispatch({
                    type: "setDiffViewerSelect",
                    key: "defaultLayout",
                    value: event.target.value as DiffLayout,
                  })
                }
              >
                {DIFF_VIEWER_LAYOUT_OPTIONS.map(({ value, label }) => (
                  <option key={value} value={value}>
                    {label}
                  </option>
                ))}
              </select>
            </label>
          )}
        </section>
      )}

      {shortcuts && (
        <section className="cmux-settings-section" data-section="shortcuts">
          <h2>Shortcuts</h2>
          <ul className="cmux-settings-shortcuts">
            {Object.keys(shortcuts.bindings).map((actionId) => (
              <li key={actionId} className="cmux-settings-row" data-action={actionId}>
                <span className="cmux-settings-shortcut-action">{actionId}</span>
                <span className="cmux-settings-shortcut-binding">
                  {shortcutBindingDisplayString(actionId, shortcuts.bindings[actionId])}
                </span>
                <input
                  type="text"
                  className="cmux-settings-shortcut-editor"
                  aria-label={`Shortcut for ${actionId}`}
                  data-shortcut-action={actionId}
                  value={shortcutBindingDraftValue(shortcuts.bindings[actionId])}
                  placeholder="None"
                  onChange={(event) =>
                    dispatch({
                      type: "setShortcutBinding",
                      action: actionId,
                      binding: shortcutBindingFromDraft(event.target.value),
                    })
                  }
                  onKeyDown={(event) => {
                    const binding = shortcutBindingFromKeyboardEvent(event);
                    if (binding === undefined) {
                      return;
                    }
                    event.preventDefault();
                    event.stopPropagation();
                    dispatch({
                      type: "setShortcutBinding",
                      action: actionId,
                      binding,
                    });
                  }}
                />
                <button
                  type="button"
                  className="cmux-settings-shortcut-clear"
                  data-shortcut-clear={actionId}
                  onClick={() =>
                    dispatch({
                      type: "setShortcutBinding",
                      action: actionId,
                      binding: null,
                    })
                  }
                >
                  Clear
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section className="cmux-settings-section" data-section="settingsJSON">
        <h2>cmux.json</h2>
        <div className="cmux-settings-row">
          <div>
            <span>Open the raw settings file</span>
            <p className="cmux-settings-row-description">
              Advanced sections such as actions, commands, UI definitions,
              vault agents, workspace groups, surface tab buttons, and
              experimental flags can be edited directly in cmux.json.
            </p>
          </div>
          <div className="cmux-settings-row-actions">
            <button
              type="button"
              disabled={onOpenSettingsFileInCmux == null}
              onClick={() => onOpenSettingsFileInCmux?.()}
            >
              Open in cmux
            </button>
            <button
              type="button"
              disabled={onOpenSettingsFile == null}
              onClick={() => onOpenSettingsFile?.()}
            >
              Open externally
            </button>
          </div>
        </div>
        <div className="cmux-settings-row cmux-settings-row-stacked">
          <div className="cmux-settings-row-header">
            <div>
              <span>Edit raw cmux.json in cmux</span>
              <p className="cmux-settings-row-description">
                Load the current file, edit JSON directly, and save after
                validation. The typed settings panes update after a successful
                save.
              </p>
              {rawSettingsPath != null ? (
                <p className="cmux-settings-row-description cmux-settings-path">
                  {rawSettingsPath}
                </p>
              ) : null}
            </div>
            <div className="cmux-settings-row-actions">
              <button
                type="button"
                disabled={onLoadRawSettings == null || rawSettingsLoading}
                onClick={() => onLoadRawSettings?.()}
              >
                {rawSettingsLoading ? "Loading..." : "Load"}
              </button>
              <button
                type="button"
                disabled={
                  onSaveRawSettings == null ||
                  rawSettingsSaving ||
                  rawSettingsDraft.trim() === ""
                }
                onClick={() => onSaveRawSettings?.()}
              >
                {rawSettingsSaving ? "Saving..." : "Save"}
              </button>
            </div>
          </div>
          {rawSettingsError != null ? (
            <p
              className="cmux-settings-row-description cmux-settings-error-text"
              role="alert"
            >
              {rawSettingsError}
            </p>
          ) : null}
          {rawSettingsStatus != null ? (
            <p className="cmux-settings-row-description">
              {rawSettingsStatus}
            </p>
          ) : null}
          <textarea
            className="cmux-settings-raw-editor"
            aria-label="Raw cmux.json editor"
            value={rawSettingsDraft}
            spellCheck={false}
            disabled={onRawSettingsDraftChange == null}
            placeholder={'{\n  "app": {}\n}'}
            onChange={(event) =>
              onRawSettingsDraftChange?.(event.currentTarget.value)
            }
          />
        </div>
      </section>

      <section className="cmux-settings-section" data-section="reset">
        <h2>Reset</h2>
        <div className="cmux-settings-row">
          <div>
            <span>Restore default settings</span>
            <p className="cmux-settings-row-description">
              Resetting clears cmux.json back to an empty object. The backend
              then materializes the default settings used by this pane.
            </p>
          </div>
          <button
            type="button"
            disabled={onResetConfig == null}
            onClick={() => onResetConfig?.()}
          >
            Reset cmux.json
          </button>
        </div>
      </section>
    </div>
  );
}
