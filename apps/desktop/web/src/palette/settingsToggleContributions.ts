import type { Config } from "@cmux/core-types";

import type {
  CommandContribution,
} from "./commandCatalog";
import type {
  ConfigAction,
  AppBoolKey,
  AutomationBoolKey,
  BrowserBoolKey,
  CanvasBoolKey,
  FileEditorBoolKey,
  NotificationsBoolKey,
  SidebarBoolKey,
  TerminalBoolKey,
  TerminalNestedSection,
} from "../settings/configReducer";

interface ToggleDescriptor<K extends string> {
  key: K;
  label: string;
  available?: (config: Config) => boolean;
  disabledTitle?: string;
  extraKeywords?: readonly string[];
}

const APP_TOGGLES: readonly ToggleDescriptor<AppBoolKey>[] = [
  { key: "menuBarOnly", label: "Menu Bar Only" },
  {
    key: "workspaceInheritWorkingDirectory",
    label: "Workspace Inherits Working Directory",
  },
  { key: "minimalMode", label: "Minimal Mode" },
  {
    key: "keepWorkspaceOpenWhenClosingLastSurface",
    label: "Keep Workspace Open When Closing Last Surface",
  },
  { key: "focusPaneOnFirstClick", label: "Focus Pane on First Click" },
  { key: "openSupportedFilesInCmux", label: "Open Supported Files in cmux" },
  { key: "openMarkdownInCmuxViewer", label: "Open Markdown in cmux Viewer" },
  { key: "reorderOnNotification", label: "Reorder on Notification" },
  { key: "iMessageMode", label: "iMessage Mode" },
  { key: "sendAnonymousTelemetry", label: "Anonymous Telemetry" },
  { key: "warnBeforeQuit", label: "Warn Before Quit" },
  { key: "warnBeforeClosingTab", label: "Warn Before Closing Tab" },
  { key: "warnBeforeClosingTabXButton", label: "Warn Before Tab X Button" },
  { key: "hideTabCloseButton", label: "Hide Tab Close Button" },
  { key: "renameSelectsExistingName", label: "Rename Selects Existing Name" },
  {
    key: "commandPaletteSearchesAllSurfaces",
    label: "Command Palette Searches All Surfaces",
  },
];

const AUTOMATION_TOGGLES: readonly ToggleDescriptor<AutomationBoolKey>[] = [
  {
    key: "claudeCodeIntegration",
    label: "Claude Code Integration",
    disabledTitle: "Install Claude Code Integration",
    extraKeywords: ["install", "setup", "hooks", "hook", "claude-code", "agent"],
  },
  {
    key: "codexIntegration",
    label: "Codex Integration",
    disabledTitle: "Install Codex Integration",
    extraKeywords: ["install", "setup", "hooks", "hook", "codex", "agent"],
  },
  {
    key: "opencodeIntegration",
    label: "OpenCode Integration",
    disabledTitle: "Install OpenCode Integration",
    extraKeywords: ["install", "setup", "hooks", "hook", "opencode", "agent"],
  },
  { key: "workspaceAutoNaming", label: "Workspace Auto Naming" },
  {
    key: "suppressSubagentNotifications",
    label: "Suppress Subagent Notifications",
  },
  { key: "ampIntegration", label: "Amp Integration" },
  { key: "cursorIntegration", label: "Cursor Integration" },
  { key: "geminiIntegration", label: "Gemini Integration" },
  { key: "kiroIntegration", label: "Kiro Integration" },
];

const BROWSER_TOGGLES: readonly ToggleDescriptor<BrowserBoolKey>[] = [
  { key: "showSearchSuggestions", label: "Show Search Suggestions" },
  { key: "discardHiddenWebViews", label: "Discard Hidden WebViews" },
  {
    key: "openTerminalLinksInCmuxBrowser",
    label: "Open Terminal Links in cmux Browser",
  },
  {
    key: "interceptTerminalOpenCommandInCmuxBrowser",
    label: "Intercept Terminal Open Commands",
  },
  { key: "showImportHintOnBlankTabs", label: "Show Import Hint on Blank Tabs" },
];

const TERMINAL_TOGGLES: readonly ToggleDescriptor<TerminalBoolKey>[] = [
  { key: "showScrollBar", label: "Show Scroll Bar" },
  { key: "copyOnSelect", label: "Copy on Select" },
  { key: "autoResumeAgentSessions", label: "Auto Resume Agent Sessions" },
  { key: "showTextBoxOnNewTerminals", label: "Show TextBox on New Terminals" },
  { key: "focusTextBoxOnNewTerminals", label: "Focus TextBox on New Terminals" },
];

const TERMINAL_NESTED_TOGGLES: readonly ToggleDescriptor<TerminalNestedSection>[] = [
  { key: "agentHibernation", label: "Agent Hibernation" },
  { key: "rendererRealization", label: "Renderer Realization" },
];

const CANVAS_TOGGLES: readonly ToggleDescriptor<CanvasBoolKey>[] = [
  { key: "snappingEnabled", label: "Canvas Snapping" },
];

const FILE_EDITOR_TOGGLES: readonly ToggleDescriptor<FileEditorBoolKey>[] = [
  { key: "wordWrap", label: "File Editor Word Wrap" },
];

const SIDEBAR_TOGGLES: readonly ToggleDescriptor<SidebarBoolKey>[] = [
  { key: "hideAllDetails", label: "Hide All Details" },
  { key: "wrapWorkspaceTitles", label: "Wrap Workspace Titles" },
  { key: "showWorkspaceDescription", label: "Show Workspace Description" },
  {
    key: "showNotificationMessage",
    label: "Show Notification Message",
    available: sidebarDetailsAvailable,
  },
  {
    key: "showBranchDirectory",
    label: "Show Branch Directory",
    available: sidebarDetailsAvailable,
  },
  {
    key: "showPullRequests",
    label: "Show Pull Requests",
    available: sidebarDetailsAvailable,
  },
  {
    key: "watchGitStatus",
    label: "Watch Git Status",
    available: sidebarDetailsAvailable,
  },
  {
    key: "makePullRequestsClickable",
    label: "Make Pull Requests Clickable",
    available: sidebarDetailsAvailable,
  },
  {
    key: "openPullRequestLinksInCmuxBrowser",
    label: "Open Pull Request Links in cmux Browser",
    available: sidebarPullRequestLinksAvailable,
  },
  {
    key: "openPortLinksInCmuxBrowser",
    label: "Open Port Links in cmux Browser",
    available: sidebarPortLinksAvailable,
  },
  { key: "showSSH", label: "Show SSH", available: sidebarDetailsAvailable },
  { key: "showPorts", label: "Show Ports", available: sidebarDetailsAvailable },
  { key: "showLog", label: "Show Log", available: sidebarDetailsAvailable },
  {
    key: "showProgress",
    label: "Show Progress",
    available: sidebarDetailsAvailable,
  },
  {
    key: "showCustomMetadata",
    label: "Show Custom Metadata",
    available: sidebarDetailsAvailable,
  },
];

const NOTIFICATIONS_TOGGLES: readonly ToggleDescriptor<NotificationsBoolKey>[] = [
  { key: "dockBadge", label: "Dock Badge" },
  { key: "showInMenuBar", label: "Show in Menu Bar" },
  { key: "unreadPaneRing", label: "Unread Pane Ring" },
  { key: "paneFlash", label: "Pane Flash" },
];

function sidebarDetailsAvailable(config: Config): boolean {
  return config.sidebar?.hideAllDetails !== true;
}

function sidebarPullRequestLinksAvailable(config: Config): boolean {
  return (
    sidebarDetailsAvailable(config) &&
    config.sidebar?.showPullRequests === true &&
    config.sidebar?.makePullRequestsClickable === true
  );
}

function sidebarPortLinksAvailable(config: Config): boolean {
  return (
    sidebarDetailsAvailable(config) &&
    config.sidebar?.showPorts === true
  );
}

function titleForToggle(
  label: string,
  enabled: boolean,
  disabledTitle?: string,
): string {
  if (!enabled && disabledTitle !== undefined) {
    return disabledTitle;
  }
  return `${enabled ? "Disable" : "Enable"} ${label}`;
}

function subtitleForToggle(sectionTitle: string, enabled: boolean): string {
  return `${sectionTitle} • ${enabled ? "On" : "Off"}`;
}

function keywordsFor(
  sectionKey: string,
  key: string,
  label: string,
  extraKeywords: readonly string[] = [],
): string[] {
  return [
    "settings",
    "toggle",
    `${sectionKey}.${key}`,
    ...extraKeywords,
    ...label
      .toLowerCase()
      .split(/[^a-z0-9]+/i)
      .filter((part) => part.length > 0),
  ];
}

function toggleRow(
  commandId: string,
  label: string,
  sectionTitle: string,
  enabled: boolean,
  keywords: string[],
  action: ConfigAction,
  disabledTitle?: string,
): CommandContribution {
  return {
    commandId,
    title: () => titleForToggle(label, enabled, disabledTitle),
    subtitle: () => subtitleForToggle(sectionTitle, enabled),
    keywords,
    dismissOnRun: true,
    when: () => true,
    enablement: () => true,
    intent: { kind: "toggleSetting", action },
  };
}

function appendToggleRows<K extends string>(
  rows: CommandContribution[],
  config: Config,
  sectionKey: string,
  sectionTitle: string,
  values: Record<K, boolean>,
  descriptors: readonly ToggleDescriptor<K>[],
  actionFor: (key: K) => ConfigAction,
): void {
  for (const descriptor of descriptors) {
    if (descriptor.available && !descriptor.available(config)) {
      continue;
    }
    rows.push(
      toggleRow(
        `palette.toggleSetting.${sectionKey}.${descriptor.key}`,
        descriptor.label,
        sectionTitle,
        values[descriptor.key],
        keywordsFor(
          sectionKey,
          descriptor.key,
          descriptor.label,
          descriptor.extraKeywords,
        ),
        actionFor(descriptor.key),
        descriptor.disabledTitle,
      ),
    );
  }
}

export function buildSettingsToggleContributions(
  config: Config | null | undefined,
): CommandContribution[] {
  if (config == null) {
    return [];
  }

  const rows: CommandContribution[] = [];

  if (config.app) {
    appendToggleRows(
      rows,
      config,
      "app",
      "App",
      config.app,
      APP_TOGGLES,
      (key) => ({ type: "toggleAppFlag", key }),
    );
  }

  if (config.automation) {
    appendToggleRows(
      rows,
      config,
      "automation",
      "Automation",
      config.automation,
      AUTOMATION_TOGGLES,
      (key) => ({ type: "toggleAutomationFlag", key }),
    );
  }

  if (config.browser) {
    appendToggleRows(
      rows,
      config,
      "browser",
      "Browser",
      config.browser,
      BROWSER_TOGGLES,
      (key) => ({ type: "toggleBrowserFlag", key }),
    );
  }

  if (config.terminal) {
    appendToggleRows(
      rows,
      config,
      "terminal",
      "Terminal",
      config.terminal,
      TERMINAL_TOGGLES,
      (key) => ({ type: "toggleTerminalFlag", key }),
    );

    for (const descriptor of TERMINAL_NESTED_TOGGLES) {
      rows.push(
        toggleRow(
          `palette.toggleSetting.terminal.${descriptor.key}.enabled`,
          descriptor.label,
          "Terminal",
          config.terminal[descriptor.key].enabled,
          keywordsFor("terminal", `${descriptor.key}.enabled`, descriptor.label),
          {
            type: "toggleTerminalNestedFlag",
            section: descriptor.key,
            key: "enabled",
          },
        ),
      );
    }
  }

  if (config.canvas) {
    appendToggleRows(
      rows,
      config,
      "canvas",
      "Canvas",
      config.canvas,
      CANVAS_TOGGLES,
      (key) => ({ type: "toggleCanvasFlag", key }),
    );
  }

  if (config.file_editor) {
    appendToggleRows(
      rows,
      config,
      "fileEditor",
      "File Editor",
      config.file_editor,
      FILE_EDITOR_TOGGLES,
      (key) => ({ type: "toggleFileEditorFlag", key }),
    );
  }

  if (config.sidebar) {
    appendToggleRows(
      rows,
      config,
      "sidebar",
      "Sidebar",
      config.sidebar,
      SIDEBAR_TOGGLES,
      (key) => ({ type: "toggleSidebarFlag", key }),
    );
  }

  if (config.notifications) {
    appendToggleRows(
      rows,
      config,
      "notifications",
      "Notifications",
      config.notifications,
      NOTIFICATIONS_TOGGLES,
      (key) => ({ type: "toggleNotificationsFlag", key }),
    );
  }

  return rows;
}
