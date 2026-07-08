// Canonical default values for the config sections the Settings UI edits,
// transcribed from `web/data/cmux.schema.json` (the schema's `default` fields
// are the canonical source — `configDefaults.test.ts` re-parses the schema and
// fails on any drift, so these constants can never silently diverge).
//
// Why materialize defaults at all: `cmux.json` holds ONLY overrides (a fresh
// install is `{}`), the generated section types have required fields, and the
// pure `configReducer` no-ops on absent sections. The Settings UI therefore
// operates on an EFFECTIVE view where absent sections are default-filled —
// while writes stay single dotted-path deltas (`configDelta.ts`), so the
// defaults never reach disk and the file remains overrides-only (canonical).

import type {
  AppConfig,
  Config,
  NotificationsConfig,
  SidebarConfig,
} from "@cmux/core-types";

/** Settings > Sidebar defaults (schema `sidebar.*.default`). */
export const DEFAULT_SIDEBAR: SidebarConfig = {
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
  // `right_max_width` (schema `rightMaxWidth`) defaults to null → left absent.
};

/** Settings > Notifications defaults (schema `notifications.*.default`). */
export const DEFAULT_NOTIFICATIONS: NotificationsConfig = {
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

/** Settings > App defaults (schema `app.*.default`). */
export const DEFAULT_APP: AppConfig = {
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

/**
 * The effective Settings view of `config`: absent edited sections are filled
 * with canonical defaults, PRESENT sections are merged field-by-field over the
 * defaults (a partial on-disk section — legal JSON the strict typed decode
 * would reject — still displays effective values for its missing fields).
 */
export function withSettingsDefaults(config: Config): Config {
  return {
    ...config,
    sidebar: { ...DEFAULT_SIDEBAR, ...(config.sidebar ?? {}) },
    notifications: { ...DEFAULT_NOTIFICATIONS, ...(config.notifications ?? {}) },
    app: { ...DEFAULT_APP, ...(config.app ?? {}) },
  };
}
