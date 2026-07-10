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

export const DEFAULT_APP_CONFIG: AppConfig = {
  language: "system",
  appearance: "system",
  appIcon: "automatic",
  windowTitleTemplate: "",
  menuBarOnly: false,
  newWorkspacePlacement: "afterCurrent",
  forkConversationDefaultDestination: "right",
  workspaceInheritWorkingDirectory: true,
  minimalMode: false,
  keepWorkspaceOpenWhenClosingLastSurface: false,
  focusPaneOnFirstClick: true,
  preferredEditor: "",
  openSupportedFilesInCmux: true,
  openMarkdownInCmuxViewer: true,
  globalFontMagnification: 100,
  reorderOnNotification: true,
  iMessageMode: false,
  sendAnonymousTelemetry: true,
  confirmQuit: "always",
  warnBeforeQuit: true,
  warnBeforeClosingTab: true,
  warnBeforeClosingTabXButton: false,
  hideTabCloseButton: false,
  renameSelectsExistingName: true,
  commandPaletteSearchesAllSurfaces: false,
};

export const DEFAULT_AUTOMATION_CONFIG: AutomationConfig = {
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
};

export const DEFAULT_NOTIFICATIONS_CONFIG: NotificationsConfig = {
  dockBadge: true,
  showInMenuBar: true,
  unreadPaneRing: true,
  paneFlash: true,
  sound: "default",
  customSoundFilePath: "",
  command: "",
  hooksMode: "append",
  hooks: [],
};

export const DEFAULT_SIDEBAR_CONFIG: SidebarConfig = {
  hideAllDetails: false,
  wrapWorkspaceTitles: false,
  showWorkspaceDescription: true,
  branchLayout: "vertical",
  showNotificationMessage: true,
  showBranchDirectory: true,
  showPullRequests: true,
  watchGitStatus: true,
  makePullRequestsClickable: true,
  openPullRequestLinksInCmuxBrowser: true,
  openPortLinksInCmuxBrowser: true,
  showSSH: true,
  showPorts: true,
  showLog: true,
  showProgress: true,
  showCustomMetadata: true,
};

export const DEFAULT_WORKSPACE_COLORS_CONFIG: WorkspaceColorsConfig = {
  indicatorStyle: "leftRail",
  selectionColor: null,
  notificationBadgeColor: null,
  colors: {
    Red: "#C0392B",
    Crimson: "#922B21",
    Orange: "#A04000",
    Amber: "#7D6608",
    Olive: "#4A5C18",
    Green: "#196F3D",
    Teal: "#006B6B",
    Aqua: "#0E6B8C",
    Blue: "#1565C0",
    Navy: "#1A5276",
    Indigo: "#283593",
    Purple: "#6A1B9A",
    Magenta: "#AD1457",
    Rose: "#880E4F",
    Brown: "#7B3F00",
    Charcoal: "#3E4B5E",
  },
  paletteOverrides: {},
  customColors: [],
};

export const DEFAULT_SIDEBAR_APPEARANCE_CONFIG: SidebarAppearanceConfig = {
  matchTerminalBackground: false,
  tintColor: "#000000",
  lightModeTintColor: null,
  darkModeTintColor: null,
  tintOpacity: 0.03,
};

export const DEFAULT_BROWSER_CONFIG: BrowserConfig = {
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
  insecureHttpHostsAllowedInEmbeddedBrowser: [
    "localhost",
    "*.localhost",
    "127.0.0.1",
    "::1",
    "0.0.0.0",
    "*.localtest.me",
  ],
  showImportHintOnBlankTabs: true,
  reactGrabVersion: "0.1.29",
};

export const DEFAULT_TERMINAL_CONFIG: TerminalConfig = {
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
};

export const DEFAULT_MARKDOWN_CONFIG: MarkdownConfig = {
  fontSize: 15,
  fontFamily: "",
  maxWidth: 980,
};

export const DEFAULT_CANVAS_CONFIG: CanvasConfig = {
  paneGap: 16,
  snappingEnabled: true,
};

export const DEFAULT_FILE_EDITOR_CONFIG: FileEditorConfig = {
  wordWrap: false,
};

export const DEFAULT_FILE_EXPLORER_CONFIG: FileExplorerConfig = {
  doubleClickAction: "preview",
};

export const DEFAULT_DIFF_VIEWER_CONFIG: DiffViewerConfig = {
  defaultLayout: "unified",
};

export const DEFAULT_SHORTCUTS_CONFIG: ShortcutsConfig = {
  showModifierHoldHints: true,
  bindings: {
    "agent.warmClaudeCode": "ctrl+alt+c",
  },
  when: {},
};

export function defaultSettingsConfig(): Config {
  return {
    app: { ...DEFAULT_APP_CONFIG },
    automation: { ...DEFAULT_AUTOMATION_CONFIG },
    browser: {
      ...DEFAULT_BROWSER_CONFIG,
      hostsToOpenInEmbeddedBrowser: [...DEFAULT_BROWSER_CONFIG.hostsToOpenInEmbeddedBrowser],
      urlsToAlwaysOpenExternally: [...DEFAULT_BROWSER_CONFIG.urlsToAlwaysOpenExternally],
      insecureHttpHostsAllowedInEmbeddedBrowser: [
        ...DEFAULT_BROWSER_CONFIG.insecureHttpHostsAllowedInEmbeddedBrowser,
      ],
    },
    terminal: {
      ...DEFAULT_TERMINAL_CONFIG,
      agentHibernation: { ...DEFAULT_TERMINAL_CONFIG.agentHibernation },
      rendererRealization: { ...DEFAULT_TERMINAL_CONFIG.rendererRealization },
      resumeCommands: [...DEFAULT_TERMINAL_CONFIG.resumeCommands],
    },
    markdown: { ...DEFAULT_MARKDOWN_CONFIG },
    canvas: { ...DEFAULT_CANVAS_CONFIG },
    file_editor: { ...DEFAULT_FILE_EDITOR_CONFIG },
    file_explorer: { ...DEFAULT_FILE_EXPLORER_CONFIG },
    diff_viewer: { ...DEFAULT_DIFF_VIEWER_CONFIG },
    notifications: { ...DEFAULT_NOTIFICATIONS_CONFIG },
    sidebar: { ...DEFAULT_SIDEBAR_CONFIG },
    workspace_colors: {
      ...DEFAULT_WORKSPACE_COLORS_CONFIG,
      colors: { ...DEFAULT_WORKSPACE_COLORS_CONFIG.colors },
      paletteOverrides: { ...DEFAULT_WORKSPACE_COLORS_CONFIG.paletteOverrides },
      customColors: [...DEFAULT_WORKSPACE_COLORS_CONFIG.customColors],
    },
    sidebar_appearance: { ...DEFAULT_SIDEBAR_APPEARANCE_CONFIG },
    shortcuts: {
      ...DEFAULT_SHORTCUTS_CONFIG,
      bindings: { ...DEFAULT_SHORTCUTS_CONFIG.bindings },
      when: { ...DEFAULT_SHORTCUTS_CONFIG.when },
    },
  };
}
