//! `cmux-config` — a serde model of the cmux configuration schema (`cmux.json`).
//!
//! This crate mirrors the canonical JSON Schema at `web/data/cmux.schema.json`.
//! It models the CORE, load-bearing sections that the Settings UI binds to
//! (app, terminal, notifications, sidebar, workspace colors, sidebar
//! appearance, automation, browser, markdown, canvas, file editor, file
//! explorer, diff viewer, shortcuts) as strongly-typed serde structs/enums.
//!
//! Deliberately NOT strict: the top-level [`Config`] does not use
//! `deny_unknown_fields`. Unmodeled top-level sections (`actions`, `ui`,
//! `commands`, `surfaceTabBarButtons`) are captured verbatim in
//! [`Config::extra`] so a decode → encode round-trip does not silently drop
//! them.
//!
//! The `ts` feature gates `ts_rs::TS` derives, exactly mirroring `cmux-core`.
//! It is inert in the default build (ts-rs is an optional dependency).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts")]
use ts_rs::TS;

// ---------------------------------------------------------------------------
// Small shared enums
// ---------------------------------------------------------------------------

/// `system` / `light` / `dark`. Shared by `app.appearance` and `browser.theme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

/// `app.appIcon`: `automatic` / `light` / `dark`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum AppIcon {
    #[default]
    Automatic,
    Light,
    Dark,
}

/// Where new workspaces are inserted in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "camelCase"))]
#[serde(rename_all = "camelCase")]
pub enum NewWorkspacePlacement {
    Top,
    #[default]
    AfterCurrent,
    End,
}

/// `app.forkConversationDefaultDestination`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "camelCase"))]
#[serde(rename_all = "camelCase")]
pub enum ForkDestination {
    #[default]
    Right,
    Left,
    Top,
    Bottom,
    NewTab,
    NewWorkspace,
}

/// `app.confirmQuit`: `always` / `dirty-only` / `never`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "kebab-case"))]
#[serde(rename_all = "kebab-case")]
pub enum ConfirmQuit {
    #[default]
    Always,
    DirtyOnly,
    Never,
}

/// `sidebar.branchLayout`: `vertical` / `inline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum BranchLayout {
    #[default]
    Vertical,
    Inline,
}

/// `notifications.hooksMode`: `append` / `replace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum HooksMode {
    #[default]
    Append,
    Replace,
}

/// `automation.kiroNotificationLevel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum KiroNotificationLevel {
    Minimal,
    #[default]
    Standard,
    Verbose,
}

/// `fileExplorer.doubleClickAction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "camelCase"))]
#[serde(rename_all = "camelCase")]
pub enum DoubleClickAction {
    #[default]
    Preview,
    DefaultEditor,
    PreferredEditor,
}

/// `diffViewer.defaultLayout`: `unified` / `split`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum DiffLayout {
    #[default]
    Unified,
    Split,
}

/// `terminal.resumeCommands[].policy`: `manual` / `prompt` / `auto`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum ResumePolicy {
    #[default]
    Manual,
    Prompt,
    Auto,
}

// ---------------------------------------------------------------------------
// app
// ---------------------------------------------------------------------------

/// `app`: general app preferences from Settings > App.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct AppConfig {
    /// Preferred app language. Kept as a `String` (not an enum) because the
    /// value set is large and locale-tagged (`zh-Hans`, `pt-BR`, …); staying
    /// lenient avoids rejecting future locales.
    pub language: String,
    pub appearance: Appearance,
    #[serde(rename = "appIcon")]
    pub app_icon: AppIcon,
    #[serde(rename = "windowTitleTemplate")]
    pub window_title_template: String,
    #[serde(rename = "menuBarOnly")]
    pub menu_bar_only: bool,
    #[serde(rename = "newWorkspacePlacement")]
    pub new_workspace_placement: NewWorkspacePlacement,
    #[serde(rename = "forkConversationDefaultDestination")]
    pub fork_conversation_default_destination: ForkDestination,
    #[serde(rename = "workspaceInheritWorkingDirectory")]
    pub workspace_inherit_working_directory: bool,
    #[serde(rename = "minimalMode")]
    pub minimal_mode: bool,
    #[serde(rename = "keepWorkspaceOpenWhenClosingLastSurface")]
    pub keep_workspace_open_when_closing_last_surface: bool,
    #[serde(rename = "focusPaneOnFirstClick")]
    pub focus_pane_on_first_click: bool,
    #[serde(rename = "preferredEditor")]
    pub preferred_editor: String,
    #[serde(rename = "openSupportedFilesInCmux")]
    pub open_supported_files_in_cmux: bool,
    #[serde(rename = "openMarkdownInCmuxViewer")]
    pub open_markdown_in_cmux_viewer: bool,
    #[serde(rename = "globalFontMagnification")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub global_font_magnification: i64,
    #[serde(rename = "reorderOnNotification")]
    pub reorder_on_notification: bool,
    #[serde(rename = "iMessageMode")]
    pub i_message_mode: bool,
    #[serde(rename = "sendAnonymousTelemetry")]
    pub send_anonymous_telemetry: bool,
    #[serde(rename = "confirmQuit")]
    pub confirm_quit: ConfirmQuit,
    #[serde(rename = "warnBeforeQuit")]
    pub warn_before_quit: bool,
    #[serde(rename = "warnBeforeClosingTab")]
    pub warn_before_closing_tab: bool,
    #[serde(rename = "warnBeforeClosingTabXButton")]
    pub warn_before_closing_tab_x_button: bool,
    #[serde(rename = "hideTabCloseButton")]
    pub hide_tab_close_button: bool,
    #[serde(rename = "renameSelectsExistingName")]
    pub rename_selects_existing_name: bool,
    #[serde(rename = "commandPaletteSearchesAllSurfaces")]
    pub command_palette_searches_all_surfaces: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            language: "system".to_owned(),
            appearance: Appearance::System,
            app_icon: AppIcon::Automatic,
            window_title_template: String::new(),
            menu_bar_only: false,
            new_workspace_placement: NewWorkspacePlacement::AfterCurrent,
            fork_conversation_default_destination: ForkDestination::Right,
            workspace_inherit_working_directory: true,
            minimal_mode: false,
            keep_workspace_open_when_closing_last_surface: false,
            focus_pane_on_first_click: true,
            preferred_editor: String::new(),
            open_supported_files_in_cmux: true,
            open_markdown_in_cmux_viewer: true,
            global_font_magnification: 100,
            reorder_on_notification: true,
            i_message_mode: false,
            send_anonymous_telemetry: true,
            confirm_quit: ConfirmQuit::Always,
            warn_before_quit: true,
            warn_before_closing_tab: true,
            warn_before_closing_tab_x_button: false,
            hide_tab_close_button: false,
            rename_selects_existing_name: true,
            command_palette_searches_all_surfaces: false,
        }
    }
}

// ---------------------------------------------------------------------------
// terminal
// ---------------------------------------------------------------------------

/// `terminal.agentHibernation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct AgentHibernation {
    pub enabled: bool,
    #[serde(rename = "idleSeconds")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub idle_seconds: i64,
    #[serde(rename = "maxLiveTerminals")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max_live_terminals: i64,
}

impl Default for AgentHibernation {
    fn default() -> Self {
        Self {
            enabled: false,
            idle_seconds: 5,
            max_live_terminals: 12,
        }
    }
}

/// `terminal.rendererRealization`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct RendererRealization {
    pub enabled: bool,
    #[serde(rename = "idleSeconds")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub idle_seconds: i64,
    #[serde(rename = "maxWarmRenderers")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max_warm_renderers: i64,
}

impl Default for RendererRealization {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_seconds: 30,
            max_warm_renderers: 12,
        }
    }
}

/// A signed command-prefix approval for restoring non-agent terminal surfaces
/// (`terminal.resumeCommands[]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct ResumeCommandApproval {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub version: i64,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name: Option<String>,
    #[serde(rename = "commandPrefix")]
    pub command_prefix: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub environment: Option<BTreeMap<String, String>>,
    #[serde(rename = "environmentKeys")]
    pub environment_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub source: Option<String>,
    pub policy: ResumePolicy,
    #[serde(rename = "createdAt")]
    pub created_at: f64,
    #[serde(rename = "updatedAt")]
    pub updated_at: f64,
    #[serde(rename = "lastUsedAt", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub last_used_at: Option<f64>,
    pub signature: String,
}

impl Default for ResumeCommandApproval {
    fn default() -> Self {
        Self {
            version: 1,
            id: String::new(),
            name: None,
            command_prefix: Vec::new(),
            cwd: None,
            environment: None,
            environment_keys: Vec::new(),
            source: None,
            policy: ResumePolicy::Manual,
            created_at: 0.0,
            updated_at: 0.0,
            last_used_at: None,
            signature: String::new(),
        }
    }
}

/// `terminal`: terminal presentation settings from Settings > Terminal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct TerminalConfig {
    #[serde(rename = "showScrollBar")]
    pub show_scroll_bar: bool,
    #[serde(rename = "scrollSpeed")]
    pub scroll_speed: f64,
    #[serde(rename = "copyOnSelect")]
    pub copy_on_select: bool,
    #[serde(rename = "autoResumeAgentSessions")]
    pub auto_resume_agent_sessions: bool,
    #[serde(rename = "showTextBoxOnNewTerminals")]
    pub show_text_box_on_new_terminals: bool,
    #[serde(rename = "focusTextBoxOnNewTerminals")]
    pub focus_text_box_on_new_terminals: bool,
    #[serde(rename = "agentHibernation")]
    pub agent_hibernation: AgentHibernation,
    #[serde(rename = "rendererRealization")]
    pub renderer_realization: RendererRealization,
    #[serde(rename = "textBoxMaxLines")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub text_box_max_lines: i64,
    #[serde(rename = "resumeCommands")]
    pub resume_commands: Vec<ResumeCommandApproval>,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            show_scroll_bar: true,
            scroll_speed: 1.0,
            copy_on_select: false,
            auto_resume_agent_sessions: true,
            show_text_box_on_new_terminals: false,
            focus_text_box_on_new_terminals: false,
            agent_hibernation: AgentHibernation::default(),
            renderer_realization: RendererRealization::default(),
            text_box_max_lines: 10,
            resume_commands: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// notifications
// ---------------------------------------------------------------------------

/// A composable notification shell hook (`notifications.hooks[]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct NotificationHook {
    pub id: String,
    pub command: String,
    #[serde(rename = "timeoutSeconds")]
    pub timeout_seconds: f64,
    pub enabled: bool,
}

impl Default for NotificationHook {
    fn default() -> Self {
        Self {
            id: String::new(),
            command: String::new(),
            timeout_seconds: 20.0,
            enabled: true,
        }
    }
}

/// `notifications`: notification behavior from Settings > Notifications.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct NotificationsConfig {
    #[serde(rename = "dockBadge")]
    pub dock_badge: bool,
    #[serde(rename = "showInMenuBar")]
    pub show_in_menu_bar: bool,
    #[serde(rename = "unreadPaneRing")]
    pub unread_pane_ring: bool,
    #[serde(rename = "paneFlash")]
    pub pane_flash: bool,
    /// Notification sound preset. Kept as a `String` because the preset set is
    /// large and includes the sentinel `custom_file`.
    pub sound: String,
    #[serde(rename = "customSoundFilePath")]
    pub custom_sound_file_path: String,
    pub command: String,
    #[serde(rename = "hooksMode")]
    pub hooks_mode: HooksMode,
    pub hooks: Vec<NotificationHook>,
}

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            dock_badge: true,
            show_in_menu_bar: true,
            unread_pane_ring: true,
            pane_flash: true,
            sound: "default".to_owned(),
            custom_sound_file_path: String::new(),
            command: String::new(),
            hooks_mode: HooksMode::Append,
            hooks: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// sidebar
// ---------------------------------------------------------------------------

/// `sidebar`: sidebar content and metadata visibility from Settings > Sidebar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct SidebarConfig {
    #[serde(rename = "hideAllDetails")]
    pub hide_all_details: bool,
    #[serde(rename = "wrapWorkspaceTitles")]
    pub wrap_workspace_titles: bool,
    #[serde(rename = "showWorkspaceDescription")]
    pub show_workspace_description: bool,
    #[serde(rename = "branchLayout")]
    pub branch_layout: BranchLayout,
    #[serde(rename = "showNotificationMessage")]
    pub show_notification_message: bool,
    #[serde(rename = "showBranchDirectory")]
    pub show_branch_directory: bool,
    #[serde(rename = "showPullRequests")]
    pub show_pull_requests: bool,
    #[serde(rename = "watchGitStatus")]
    pub watch_git_status: bool,
    #[serde(rename = "makePullRequestsClickable")]
    pub make_pull_requests_clickable: bool,
    #[serde(rename = "openPullRequestLinksInCmuxBrowser")]
    pub open_pull_request_links_in_cmux_browser: bool,
    #[serde(rename = "openPortLinksInCmuxBrowser")]
    pub open_port_links_in_cmux_browser: bool,
    #[serde(rename = "showSSH")]
    pub show_ssh: bool,
    #[serde(rename = "showPorts")]
    pub show_ports: bool,
    #[serde(rename = "showLog")]
    pub show_log: bool,
    #[serde(rename = "showProgress")]
    pub show_progress: bool,
    #[serde(rename = "showCustomMetadata")]
    pub show_custom_metadata: bool,
    #[serde(rename = "rightMaxWidth", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub right_max_width: Option<f64>,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            hide_all_details: false,
            wrap_workspace_titles: false,
            show_workspace_description: true,
            branch_layout: BranchLayout::Vertical,
            show_notification_message: true,
            show_branch_directory: true,
            show_pull_requests: true,
            watch_git_status: true,
            make_pull_requests_clickable: true,
            open_pull_request_links_in_cmux_browser: true,
            open_port_links_in_cmux_browser: true,
            show_ssh: true,
            show_ports: true,
            show_log: true,
            show_progress: true,
            show_custom_metadata: true,
            right_max_width: None,
        }
    }
}

// ---------------------------------------------------------------------------
// workspaceColors
// ---------------------------------------------------------------------------

/// `workspaceColors`: workspace tab and badge colors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct WorkspaceColorsConfig {
    /// Active workspace indicator style. Kept as a `String` because the schema
    /// documents legacy aliases that are accepted and normalized at runtime.
    #[serde(rename = "indicatorStyle")]
    pub indicator_style: String,
    #[serde(rename = "selectionColor")]
    pub selection_color: Option<String>,
    #[serde(rename = "notificationBadgeColor")]
    pub notification_badge_color: Option<String>,
    pub colors: BTreeMap<String, String>,
    #[serde(rename = "paletteOverrides")]
    pub palette_overrides: BTreeMap<String, String>,
    #[serde(rename = "customColors")]
    pub custom_colors: Vec<String>,
}

impl Default for WorkspaceColorsConfig {
    fn default() -> Self {
        Self {
            indicator_style: "leftRail".to_owned(),
            selection_color: None,
            notification_badge_color: None,
            colors: default_workspace_colors(),
            palette_overrides: BTreeMap::new(),
            custom_colors: Vec::new(),
        }
    }
}

/// The built-in named workspace color palette (schema default of
/// `workspaceColors.colors`).
pub fn default_workspace_colors() -> BTreeMap<String, String> {
    [
        ("Red", "#C0392B"),
        ("Crimson", "#922B21"),
        ("Orange", "#A04000"),
        ("Amber", "#7D6608"),
        ("Olive", "#4A5C18"),
        ("Green", "#196F3D"),
        ("Teal", "#006B6B"),
        ("Aqua", "#0E6B8C"),
        ("Blue", "#1565C0"),
        ("Navy", "#1A5276"),
        ("Indigo", "#283593"),
        ("Purple", "#6A1B9A"),
        ("Magenta", "#AD1457"),
        ("Rose", "#880E4F"),
        ("Brown", "#7B3F00"),
        ("Charcoal", "#3E4B5E"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

// ---------------------------------------------------------------------------
// sidebarAppearance
// ---------------------------------------------------------------------------

/// `sidebarAppearance`: sidebar tint settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct SidebarAppearanceConfig {
    #[serde(rename = "matchTerminalBackground")]
    pub match_terminal_background: bool,
    #[serde(rename = "tintColor")]
    pub tint_color: String,
    #[serde(rename = "lightModeTintColor")]
    pub light_mode_tint_color: Option<String>,
    #[serde(rename = "darkModeTintColor")]
    pub dark_mode_tint_color: Option<String>,
    #[serde(rename = "tintOpacity")]
    pub tint_opacity: f64,
}

impl Default for SidebarAppearanceConfig {
    fn default() -> Self {
        Self {
            match_terminal_background: false,
            tint_color: "#000000".to_owned(),
            light_mode_tint_color: None,
            dark_mode_tint_color: None,
            tint_opacity: 0.03,
        }
    }
}

// ---------------------------------------------------------------------------
// automation
// ---------------------------------------------------------------------------

/// `automation`: socket control and automation settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct AutomationConfig {
    /// Socket control mode. Kept as a `String` because the schema documents
    /// legacy aliases that are accepted and normalized at runtime.
    #[serde(rename = "socketControlMode")]
    pub socket_control_mode: String,
    /// Password for password-mode socket access. `null` or `""` clears it.
    #[serde(rename = "socketPassword", skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub socket_password: Option<String>,
    #[serde(rename = "claudeCodeIntegration")]
    pub claude_code_integration: bool,
    #[serde(rename = "claudeBinaryPath")]
    pub claude_binary_path: String,
    #[serde(rename = "workspaceAutoNaming")]
    pub workspace_auto_naming: bool,
    #[serde(rename = "autoNamingAgent")]
    pub auto_naming_agent: String,
    #[serde(rename = "ripgrepBinaryPath")]
    pub ripgrep_binary_path: String,
    #[serde(rename = "suppressSubagentNotifications")]
    pub suppress_subagent_notifications: bool,
    #[serde(rename = "ampIntegration")]
    pub amp_integration: bool,
    #[serde(rename = "cursorIntegration")]
    pub cursor_integration: bool,
    #[serde(rename = "geminiIntegration")]
    pub gemini_integration: bool,
    #[serde(rename = "kiroIntegration")]
    pub kiro_integration: bool,
    #[serde(rename = "kiroNotificationLevel")]
    pub kiro_notification_level: KiroNotificationLevel,
    #[serde(rename = "portBase")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub port_base: i64,
    #[serde(rename = "portRange")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub port_range: i64,
}

impl Default for AutomationConfig {
    fn default() -> Self {
        Self {
            socket_control_mode: "cmuxOnly".to_owned(),
            socket_password: None,
            claude_code_integration: true,
            claude_binary_path: String::new(),
            workspace_auto_naming: false,
            auto_naming_agent: "auto".to_owned(),
            ripgrep_binary_path: String::new(),
            suppress_subagent_notifications: true,
            amp_integration: true,
            cursor_integration: true,
            gemini_integration: true,
            kiro_integration: true,
            kiro_notification_level: KiroNotificationLevel::Standard,
            port_base: 9100,
            port_range: 10,
        }
    }
}

// ---------------------------------------------------------------------------
// browser
// ---------------------------------------------------------------------------

/// `browser`: embedded browser settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct BrowserConfig {
    /// Default search engine. Kept as a `String` because the value set is large
    /// and includes the `custom` sentinel.
    #[serde(rename = "defaultSearchEngine")]
    pub default_search_engine: String,
    #[serde(rename = "customSearchEngineName")]
    pub custom_search_engine_name: String,
    #[serde(rename = "customSearchEngineURLTemplate")]
    pub custom_search_engine_url_template: String,
    #[serde(rename = "showSearchSuggestions")]
    pub show_search_suggestions: bool,
    pub theme: Appearance,
    #[serde(rename = "discardHiddenWebViews")]
    pub discard_hidden_web_views: bool,
    #[serde(rename = "hiddenWebViewDiscardDelaySeconds")]
    pub hidden_web_view_discard_delay_seconds: f64,
    #[serde(rename = "openTerminalLinksInCmuxBrowser")]
    pub open_terminal_links_in_cmux_browser: bool,
    #[serde(rename = "interceptTerminalOpenCommandInCmuxBrowser")]
    pub intercept_terminal_open_command_in_cmux_browser: bool,
    #[serde(rename = "hostsToOpenInEmbeddedBrowser")]
    pub hosts_to_open_in_embedded_browser: Vec<String>,
    #[serde(rename = "urlsToAlwaysOpenExternally")]
    pub urls_to_always_open_externally: Vec<String>,
    #[serde(rename = "insecureHttpHostsAllowedInEmbeddedBrowser")]
    pub insecure_http_hosts_allowed_in_embedded_browser: Vec<String>,
    #[serde(rename = "showImportHintOnBlankTabs")]
    pub show_import_hint_on_blank_tabs: bool,
    #[serde(rename = "reactGrabVersion")]
    pub react_grab_version: String,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            default_search_engine: "google".to_owned(),
            custom_search_engine_name: String::new(),
            custom_search_engine_url_template: "https://www.google.com/search?q={query}".to_owned(),
            show_search_suggestions: true,
            theme: Appearance::System,
            discard_hidden_web_views: true,
            hidden_web_view_discard_delay_seconds: 300.0,
            open_terminal_links_in_cmux_browser: true,
            intercept_terminal_open_command_in_cmux_browser: true,
            hosts_to_open_in_embedded_browser: Vec::new(),
            urls_to_always_open_externally: Vec::new(),
            insecure_http_hosts_allowed_in_embedded_browser: vec![
                "localhost".to_owned(),
                "*.localhost".to_owned(),
                "127.0.0.1".to_owned(),
                "::1".to_owned(),
                "0.0.0.0".to_owned(),
                "*.localtest.me".to_owned(),
            ],
            show_import_hint_on_blank_tabs: true,
            react_grab_version: "0.1.29".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// markdown / canvas / fileEditor / fileExplorer / diffViewer
// ---------------------------------------------------------------------------

/// `markdown`: built-in markdown viewer settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct MarkdownConfig {
    #[serde(rename = "fontSize")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub font_size: i64,
    #[serde(rename = "fontFamily")]
    pub font_family: String,
    #[serde(rename = "maxWidth")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub max_width: i64,
}

impl Default for MarkdownConfig {
    fn default() -> Self {
        Self {
            font_size: 15,
            font_family: String::new(),
            max_width: 980,
        }
    }
}

/// `canvas`: freeform canvas workspace layout settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct CanvasConfig {
    #[serde(rename = "paneGap")]
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub pane_gap: i64,
    #[serde(rename = "snappingEnabled")]
    pub snapping_enabled: bool,
}

impl Default for CanvasConfig {
    fn default() -> Self {
        Self {
            pane_gap: 16,
            snapping_enabled: true,
        }
    }
}

/// `fileEditor`: built-in plain-text file editor settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct FileEditorConfig {
    #[serde(rename = "wordWrap")]
    pub word_wrap: bool,
}

/// `fileExplorer`: right-sidebar file explorer settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct FileExplorerConfig {
    #[serde(rename = "doubleClickAction")]
    pub double_click_action: DoubleClickAction,
}

/// `diffViewer`: built-in diff viewer settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct DiffViewerConfig {
    #[serde(rename = "defaultLayout")]
    pub default_layout: DiffLayout,
}

// ---------------------------------------------------------------------------
// shortcuts
// ---------------------------------------------------------------------------

/// A single keyboard shortcut binding value: a single stroke, a chord, or an
/// unbinding sentinel string. Deserializes leniently from a string or array.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(untagged)]
pub enum ShortcutBinding {
    /// A single stroke (`"cmd+n"`) or an unbinding sentinel
    /// (`""`, `none`, `clear`, `unbound`, `disabled`).
    Single(String),
    /// A chorded shortcut such as `["ctrl+b", "c"]`.
    Chord(Vec<String>),
}

/// `shortcuts`: keyboard shortcut settings.
///
/// `bindings` values are `Option<ShortcutBinding>` so an explicit JSON `null`
/// (which unbinds an action) is preserved distinctly from an absent key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct ShortcutsConfig {
    #[serde(rename = "showModifierHoldHints")]
    pub show_modifier_hold_hints: bool,
    pub bindings: BTreeMap<String, Option<ShortcutBinding>>,
    pub when: BTreeMap<String, String>,
}

impl Default for ShortcutsConfig {
    fn default() -> Self {
        Self {
            show_modifier_hold_hints: true,
            bindings: BTreeMap::new(),
            when: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Vault session-restore agents (`vault`)
// ---------------------------------------------------------------------------

/// A schema `oneOf(string, array-of-string)` value: either a single string or a
/// list of strings. Mirrors the schema's `argvContains` shape and Vault's
/// one-or-many decoding (`CmuxVaultAgentDetectRule.decodeOneOrManyStrings`,
/// `Sources/VaultAgentRegistry.swift:229-242`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(untagged)]
pub enum StringOrStringList {
    /// A single value (`"pi"`).
    One(String),
    /// A list of values (`["pi", "pi-agent"]`).
    Many(Vec<String>),
}

/// `vault.agents[].cwd`: whether Vault `cd`s to the saved working directory
/// before running `resumeCommand`. Schema enum `["preserve", "ignore"]`,
/// default `preserve` (`web/data/cmux.schema.json:150-155`;
/// `CmuxVaultAgentCWDPolicy`, `Sources/VaultAgentRegistry.swift:37,102`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum VaultAgentCwd {
    #[default]
    Preserve,
    Ignore,
}

/// `vault.agents[].detect`: rules for detecting a running agent process inside
/// a cmux terminal.
///
/// The published schema (`web/data/cmux.schema.json:69-89`) is
/// `additionalProperties:false` and declares only `processName` + `argvContains`.
/// DIVERGENCE: `processNames` is modeled here to match the canonical Swift
/// `CmuxVaultAgentDetectRule` (`Sources/VaultAgentRegistry.swift:190-193`), which
/// accepts `processNames` (and `alternateArgvContains`) beyond the schema.
/// Kept strict (no `#[serde(flatten)]`) per the schema's closed object, but not
/// `deny_unknown_fields` so unknown keys are dropped rather than hard-erroring
/// (crate leniency).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct VaultAgentDetect {
    #[serde(rename = "processName", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub process_name: Option<String>,
    #[serde(rename = "processNames", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub process_names: Option<StringOrStringList>,
    #[serde(rename = "argvContains", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub argv_contains: Option<StringOrStringList>,
}

/// The object arm of `vault.agents[].sessionIdSource`
/// (`{ "type": ..., "argvOption"?: ... }`, `web/data/cmux.schema.json:96-130`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct VaultAgentSessionIdSourceObject {
    /// Schema restricts this to the enum `["argvOption", "piSessionFile"]`.
    /// DIVERGENCE: kept as a lenient `String` (not a closed enum) because the
    /// canonical Swift `CmuxVaultAgentSessionIDSource` has itself diverged from
    /// the schema, adding variants and aliases (`grokSessionDirectory`,
    /// `argv-option`, `pi-session-file`; `Sources/VaultAgentRegistry.swift:245-317`).
    /// A `String` preserves unknown `type` values losslessly instead of
    /// hard-erroring, matching this crate's leniency; we stay faithful to the
    /// SCHEMA's two-arm `oneOf` wire shape (string | object).
    pub r#type: String,
    #[serde(rename = "argvOption", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub argv_option: Option<String>,
}

/// `vault.agents[].sessionIdSource`: where cmux reads the native session id.
///
/// Schema `oneOf` (`web/data/cmux.schema.json:90-133`): a bare string
/// (`"piSessionFile"`, or an argv option like `"--session"`) OR an object
/// `{ type, argvOption? }`. Modeled as a `#[serde(untagged)]` two-arm enum with
/// the string arm first so bare strings decode to [`Named`](Self::Named).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(untagged)]
pub enum VaultAgentSessionIdSource {
    /// Bare-string form (`"piSessionFile"`, `"--session"`).
    Named(String),
    /// Structured form (`{ "type": "argvOption", "argvOption": "--session" }`).
    Structured(VaultAgentSessionIdSourceObject),
}

impl Default for VaultAgentSessionIdSource {
    fn default() -> Self {
        // Lenient placeholder so a `vault.agents[]` entry missing the
        // (schema-required) `sessionIdSource` falls back to a default instead of
        // hard-erroring, consistent with the rest of this crate.
        Self::Named(String::new())
    }
}

/// A single `vault.agents[]` entry: a custom coding agent that Vault can detect,
/// list, and resume (`web/data/cmux.schema.json:48-163`;
/// `CmuxVaultAgentRegistration`, `Sources/VaultAgentRegistry.swift:12-49`).
///
/// Schema items are `additionalProperties:true` (`web/data/cmux.schema.json:53`),
/// so unknown per-agent keys are captured verbatim in [`VaultAgent::extra`] for
/// lossless round-trips. The schema-required keys (`id`, `name`,
/// `sessionIdSource`, `resumeCommand`) are modeled non-optional, but the struct
/// is `#[serde(default)]` so a partial/absent entry falls back to defaults
/// rather than hard-erroring (crate leniency; mirrors [`ResumeCommandApproval`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct VaultAgent {
    pub id: String,
    pub name: String,
    #[serde(rename = "iconAssetName", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon_asset_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub detect: Option<VaultAgentDetect>,
    #[serde(rename = "sessionIdSource")]
    pub session_id_source: VaultAgentSessionIdSource,
    #[serde(rename = "resumeCommand")]
    pub resume_command: String,
    #[serde(rename = "forkCommand", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fork_command: Option<String>,
    pub cwd: VaultAgentCwd,
    #[serde(rename = "sessionDirectory", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub session_directory: Option<String>,
    /// Unknown per-agent keys (schema `additionalProperties:true`), preserved
    /// verbatim for lossless round-trips. Excluded from the TS bindings.
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Default for VaultAgent {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            icon_asset_name: None,
            detect: None,
            session_id_source: VaultAgentSessionIdSource::default(),
            resume_command: String::new(),
            fork_command: None,
            cwd: VaultAgentCwd::Preserve,
            session_directory: None,
            extra: serde_json::Map::new(),
        }
    }
}

/// `vault`: Vault session-restore agent registrations
/// (`web/data/cmux.schema.json:42-165`). `additionalProperties:false`; the sole
/// property is `agents` (default `[]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct VaultConfig {
    pub agents: Vec<VaultAgent>,
}

// ---------------------------------------------------------------------------
// Workspace groups (`workspaceGroups`)
// ---------------------------------------------------------------------------

/// A `workspaceGroups.byCwd` entry: per-cwd customization for a sidebar
/// workspace group (`web/data/cmux.schema.json:186-215`;
/// `CmuxConfigWorkspaceGroupEntry`, `Sources/CmuxConfig.swift:166-178`). Schema
/// is `additionalProperties:false`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct WorkspaceGroupEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon: Option<String>,
    /// Right-click menu items on the group's `+` button. Schema items are
    /// `oneOf(string, object)` (`web/data/cmux.schema.json:201-206`); kept opaque
    /// as raw JSON values (matching the untyped action objects) rather than
    /// modeling each action shape.
    #[serde(rename = "contextMenu", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "Array<unknown>"))]
    pub context_menu: Option<Vec<serde_json::Value>>,
    /// Per-cwd override for new-workspace placement; falls back to the group's
    /// global default when omitted. Reuses the existing [`NewWorkspacePlacement`]
    /// enum.
    #[serde(rename = "newWorkspacePlacement", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub new_workspace_placement: Option<NewWorkspacePlacement>,
}

/// `workspaceGroups`: per-cwd customization for sidebar workspace groups
/// (`web/data/cmux.schema.json:170-218`). `additionalProperties:false`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct WorkspaceGroupsConfig {
    /// Global default for where new workspaces land within a group. Reuses the
    /// existing [`NewWorkspacePlacement`] enum; schema default `afterCurrent`.
    #[serde(rename = "newWorkspacePlacement")]
    pub new_workspace_placement: NewWorkspacePlacement,
    /// Map of cwd pattern → group customization. Empty when omitted.
    #[serde(rename = "byCwd")]
    pub by_cwd: BTreeMap<String, WorkspaceGroupEntry>,
}

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

/// The top-level `cmux.json` document.
///
/// Modeled sections are strongly typed and optional (absent sections stay
/// absent on re-serialize). Every other top-level key — including sections this
/// crate deliberately does not model (`actions`, `ui`, `commands`,
/// `surfaceTabBarButtons`) — is captured verbatim in [`Config::extra`] so a
/// round-trip is non-lossy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct Config {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub schema: Option<String>,
    #[serde(rename = "schemaVersion", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub schema_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub app: Option<AppConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub terminal: Option<TerminalConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub notifications: Option<NotificationsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sidebar: Option<SidebarConfig>,
    #[serde(rename = "workspaceColors", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub workspace_colors: Option<WorkspaceColorsConfig>,
    #[serde(rename = "sidebarAppearance", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub sidebar_appearance: Option<SidebarAppearanceConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub automation: Option<AutomationConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub browser: Option<BrowserConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub markdown: Option<MarkdownConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub canvas: Option<CanvasConfig>,
    #[serde(rename = "fileEditor", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub file_editor: Option<FileEditorConfig>,
    #[serde(rename = "fileExplorer", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub file_explorer: Option<FileExplorerConfig>,
    #[serde(rename = "diffViewer", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub diff_viewer: Option<DiffViewerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub shortcuts: Option<ShortcutsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub vault: Option<VaultConfig>,
    #[serde(rename = "workspaceGroups", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub workspace_groups: Option<WorkspaceGroupsConfig>,
    #[serde(rename = "newWorkspaceCommand", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub new_workspace_command: Option<String>,
    /// Any top-level key not modeled above, preserved verbatim for lossless
    /// round-trips. Excluded from the TS bindings (opaque JSON).
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// ---------------------------------------------------------------------------
// Decode / encode helpers
// ---------------------------------------------------------------------------

/// Parse a `cmux.json` string into a [`Config`]. Unknown keys are preserved in
/// [`Config::extra`] rather than rejected.
pub fn decode_config(json: &str) -> Result<Config, serde_json::Error> {
    serde_json::from_str(json)
}

/// Serialize a [`Config`] back to a compact JSON string.
pub fn encode_config(config: &Config) -> Result<String, serde_json::Error> {
    serde_json::to_string(config)
}

/// Serialize a [`Config`] back to a pretty-printed JSON string (2-space indent),
/// suitable for writing `cmux.json` to disk.
pub fn encode_config_pretty(config: &Config) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(config)
}

// ---------------------------------------------------------------------------
// Config path resolution
// ---------------------------------------------------------------------------

/// The two path components appended to the platform config directory.
const CONFIG_DIR_NAME: &str = "cmux";
const CONFIG_FILE_NAME: &str = "cmux.json";

/// Resolve the global `cmux.json` path under an explicit base config directory.
/// Pure and platform-independent so it can be unit-tested; used by
/// [`config_path`].
pub fn config_path_in(config_dir: &Path) -> PathBuf {
    config_dir.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME)
}

/// Resolve the global `cmux.json` path for the current user.
///
/// Uses [`dirs::config_dir`] joined with `cmux/cmux.json`. On Windows that is
/// `%APPDATA%\cmux\cmux.json` (Roaming AppData), the platform analog of the
/// macOS `~/.config/cmux/cmux.json` location cmux uses. Returns `None` only
/// when the platform config directory cannot be determined.
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| config_path_in(&dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_schema() {
        let app = AppConfig::default();
        assert_eq!(app.language, "system");
        assert_eq!(app.appearance, Appearance::System);
        assert_eq!(app.app_icon, AppIcon::Automatic);
        assert_eq!(app.global_font_magnification, 100);
        assert!(app.workspace_inherit_working_directory);
        assert_eq!(app.confirm_quit, ConfirmQuit::Always);
        assert_eq!(
            app.fork_conversation_default_destination,
            ForkDestination::Right
        );
        assert_eq!(
            app.new_workspace_placement,
            NewWorkspacePlacement::AfterCurrent
        );

        let terminal = TerminalConfig::default();
        assert!(terminal.show_scroll_bar);
        assert_eq!(terminal.scroll_speed, 1.0);
        assert_eq!(terminal.text_box_max_lines, 10);
        assert_eq!(terminal.agent_hibernation.idle_seconds, 5);
        assert_eq!(terminal.agent_hibernation.max_live_terminals, 12);
        assert!(!terminal.agent_hibernation.enabled);
        assert!(terminal.renderer_realization.enabled);
        assert_eq!(terminal.renderer_realization.idle_seconds, 30);

        let automation = AutomationConfig::default();
        assert_eq!(automation.socket_control_mode, "cmuxOnly");
        assert_eq!(automation.port_base, 9100);
        assert_eq!(automation.port_range, 10);
        assert_eq!(
            automation.kiro_notification_level,
            KiroNotificationLevel::Standard
        );

        let browser = BrowserConfig::default();
        assert_eq!(browser.default_search_engine, "google");
        assert_eq!(browser.theme, Appearance::System);
        assert_eq!(
            browser.custom_search_engine_url_template,
            "https://www.google.com/search?q={query}"
        );
        assert_eq!(
            browser.insecure_http_hosts_allowed_in_embedded_browser.len(),
            6
        );

        let colors = WorkspaceColorsConfig::default();
        assert_eq!(colors.indicator_style, "leftRail");
        assert_eq!(colors.colors.len(), 16);
        assert_eq!(colors.colors.get("Blue").map(String::as_str), Some("#1565C0"));

        let sidebar_appearance = SidebarAppearanceConfig::default();
        assert_eq!(sidebar_appearance.tint_color, "#000000");
        assert_eq!(sidebar_appearance.tint_opacity, 0.03);

        assert_eq!(MarkdownConfig::default().font_size, 15);
        assert_eq!(MarkdownConfig::default().max_width, 980);
        assert_eq!(CanvasConfig::default().pane_gap, 16);
        assert!(CanvasConfig::default().snapping_enabled);
        assert!(!FileEditorConfig::default().word_wrap);
        assert_eq!(
            FileExplorerConfig::default().double_click_action,
            DoubleClickAction::Preview
        );
        assert_eq!(DiffViewerConfig::default().default_layout, DiffLayout::Unified);
        assert!(ShortcutsConfig::default().show_modifier_hold_hints);

        // The top-level Config default is empty (all sections absent).
        let config = Config::default();
        assert!(config.app.is_none());
        assert!(config.extra.is_empty());
    }

    #[test]
    fn key_casing_serializes_exactly() {
        let config = Config {
            app: Some(AppConfig::default()),
            automation: Some(AutomationConfig::default()),
            ..Config::default()
        };
        let value = serde_json::to_value(&config).expect("serialize");

        let app = value.get("app").and_then(|v| v.as_object()).expect("app obj");
        assert!(app.contains_key("globalFontMagnification"));
        assert!(app.contains_key("forkConversationDefaultDestination"));
        assert!(app.contains_key("appIcon"));
        assert!(app.contains_key("iMessageMode"));
        assert!(app.contains_key("warnBeforeClosingTabXButton"));
        // camelCase enum value.
        assert_eq!(app.get("newWorkspacePlacement").unwrap(), "afterCurrent");
        // kebab-case enum value round-trips through the rename.
        assert_eq!(app.get("confirmQuit").unwrap(), "always");

        let automation = value
            .get("automation")
            .and_then(|v| v.as_object())
            .expect("automation obj");
        assert!(automation.contains_key("socketControlMode"));
        assert!(automation.contains_key("claudeCodeIntegration"));
        assert!(automation.contains_key("portBase"));
        // socketPassword is None → skipped.
        assert!(!automation.contains_key("socketPassword"));
    }

    #[test]
    fn confirm_quit_kebab_case_round_trips() {
        let json = r#"{ "app": { "confirmQuit": "dirty-only" } }"#;
        let config = decode_config(json).expect("decode");
        assert_eq!(
            config.app.as_ref().unwrap().confirm_quit,
            ConfirmQuit::DirtyOnly
        );
        let value = serde_json::to_value(&config).expect("serialize");
        assert_eq!(value["app"]["confirmQuit"], "dirty-only");
    }

    #[test]
    fn representative_subset_round_trips() {
        let json = r#"{
            "$schema": "https://raw.githubusercontent.com/manaflow-ai/cmux/main/web/data/cmux.schema.json",
            "schemaVersion": 1,
            "app": {
                "appearance": "dark",
                "globalFontMagnification": 120,
                "confirmQuit": "never",
                "newWorkspacePlacement": "top"
            },
            "terminal": {
                "scrollSpeed": 1.5,
                "agentHibernation": { "enabled": true, "idleSeconds": 60 },
                "resumeCommands": [
                    {
                        "version": 1,
                        "id": "abc",
                        "commandPrefix": ["npm", "run", "dev"],
                        "environmentKeys": ["PATH"],
                        "policy": "prompt",
                        "createdAt": 1.0,
                        "updatedAt": 2.0,
                        "signature": "sig"
                    }
                ]
            },
            "notifications": {
                "sound": "Ping",
                "hooks": [ { "id": "h1", "command": "echo hi", "timeoutSeconds": 5 } ]
            },
            "shortcuts": {
                "bindings": {
                    "newTab": "cmd+t",
                    "toggleTerminalCopyMode": ["ctrl+b", "c"],
                    "closeTab": null
                },
                "when": { "selectWorkspaceByNumber": "!sidebarFocus" }
            }
        }"#;

        let config = decode_config(json).expect("decode");

        let app = config.app.as_ref().unwrap();
        assert_eq!(app.appearance, Appearance::Dark);
        assert_eq!(app.global_font_magnification, 120);
        assert_eq!(app.confirm_quit, ConfirmQuit::Never);
        // Unspecified field falls back to schema default.
        assert!(app.workspace_inherit_working_directory);

        let terminal = config.terminal.as_ref().unwrap();
        assert_eq!(terminal.scroll_speed, 1.5);
        assert!(terminal.agent_hibernation.enabled);
        assert_eq!(terminal.agent_hibernation.idle_seconds, 60);
        // Nested default still applied where unspecified.
        assert_eq!(terminal.agent_hibernation.max_live_terminals, 12);
        assert_eq!(terminal.resume_commands.len(), 1);
        assert_eq!(terminal.resume_commands[0].policy, ResumePolicy::Prompt);
        assert_eq!(terminal.resume_commands[0].command_prefix, ["npm", "run", "dev"]);

        let shortcuts = config.shortcuts.as_ref().unwrap();
        assert_eq!(
            shortcuts.bindings.get("newTab"),
            Some(&Some(ShortcutBinding::Single("cmd+t".to_owned())))
        );
        assert_eq!(
            shortcuts.bindings.get("toggleTerminalCopyMode"),
            Some(&Some(ShortcutBinding::Chord(vec![
                "ctrl+b".to_owned(),
                "c".to_owned()
            ])))
        );
        // Explicit null preserved as Some(None): key present, value null.
        assert_eq!(shortcuts.bindings.get("closeTab"), Some(&None));

        // Full re-decode stability: encode → decode must be a fixed point.
        let encoded = encode_config(&config).expect("encode");
        let redecoded = decode_config(&encoded).expect("re-decode");
        assert_eq!(config, redecoded);
    }

    #[test]
    fn unknown_sections_preserved_in_extra() {
        let json = r#"{
            "app": { "minimalMode": true },
            "actions": { "custom": { "title": "Do thing" } },
            "commands": [ { "name": "build" } ],
            "newWorkspaceCommand": "build",
            "totallyUnknownKey": 42
        }"#;

        let config = decode_config(json).expect("decode");
        assert!(config.app.as_ref().unwrap().minimal_mode);
        // Unmodeled sections land in `extra`, not dropped.
        assert!(config.extra.contains_key("actions"));
        assert!(config.extra.contains_key("commands"));
        assert!(config.extra.contains_key("totallyUnknownKey"));
        // `newWorkspaceCommand` is now a typed top-level field, no longer extra.
        assert!(!config.extra.contains_key("newWorkspaceCommand"));
        assert_eq!(config.new_workspace_command.as_deref(), Some("build"));

        // And they survive a round-trip.
        let encoded = encode_config(&config).expect("encode");
        let redecoded = decode_config(&encoded).expect("re-decode");
        assert_eq!(config, redecoded);
        assert_eq!(redecoded.extra.get("totallyUnknownKey").unwrap(), 42);
    }

    #[test]
    fn empty_object_decodes_to_empty_config() {
        let config = decode_config("{}").expect("decode");
        assert_eq!(config, Config::default());
        // And an empty Config serializes back to "{}".
        assert_eq!(encode_config(&config).unwrap(), "{}");
    }

    #[test]
    fn vault_and_workspace_group_defaults() {
        assert!(VaultConfig::default().agents.is_empty());
        assert_eq!(VaultAgentCwd::default(), VaultAgentCwd::Preserve);
        assert_eq!(VaultAgent::default().cwd, VaultAgentCwd::Preserve);

        let groups = WorkspaceGroupsConfig::default();
        assert_eq!(
            groups.new_workspace_placement,
            NewWorkspacePlacement::AfterCurrent
        );
        assert!(groups.by_cwd.is_empty());
    }

    #[test]
    fn vault_workspace_groups_and_new_workspace_command_round_trip() {
        // `r##"..."##` because the JSON contains a `"#7A4FD8"` hex color, whose
        // `"#` would otherwise terminate a plain `r#"..."#` raw string.
        let json = r##"{
            "vault": {
                "agents": [
                    {
                        "id": "pi",
                        "name": "Pi",
                        "iconAssetName": "AgentIcons/Pi",
                        "detect": { "processName": "pi", "argvContains": ["pi"] },
                        "sessionIdSource": "piSessionFile",
                        "resumeCommand": "{{executable}} --session {{sessionId}}",
                        "forkCommand": "{{executable}} --session {{sessionId}} --fork",
                        "cwd": "preserve",
                        "sessionDirectory": "~/.pi/agent/sessions",
                        "customExtra": { "note": "kept" }
                    },
                    {
                        "id": "antigravity",
                        "name": "Antigravity",
                        "detect": { "processNames": ["agy", "antigravity"] },
                        "sessionIdSource": { "type": "argvOption", "argvOption": "--session" },
                        "resumeCommand": "{{executable}} --conversation {{sessionId}}",
                        "cwd": "ignore"
                    }
                ]
            },
            "workspaceGroups": {
                "newWorkspacePlacement": "top",
                "byCwd": {
                    "~/work/*": {
                        "color": "#7A4FD8",
                        "icon": "ladybug.fill",
                        "contextMenu": [ "sep", { "action": "newTerminal" } ],
                        "newWorkspacePlacement": "end"
                    }
                }
            },
            "newWorkspaceCommand": "build"
        }"##;

        let config = decode_config(json).expect("decode");

        let vault = config.vault.as_ref().expect("vault");
        assert_eq!(vault.agents.len(), 2);

        // Bare-string sessionIdSource arm.
        let pi = &vault.agents[0];
        assert_eq!(pi.id, "pi");
        assert_eq!(pi.cwd, VaultAgentCwd::Preserve);
        assert_eq!(
            pi.session_id_source,
            VaultAgentSessionIdSource::Named("piSessionFile".to_owned())
        );
        // `additionalProperties:true` → unknown agent keys preserved in `extra`.
        assert!(pi.extra.contains_key("customExtra"));

        // Structured object sessionIdSource arm.
        let agy = &vault.agents[1];
        assert_eq!(agy.cwd, VaultAgentCwd::Ignore);
        assert_eq!(
            agy.session_id_source,
            VaultAgentSessionIdSource::Structured(VaultAgentSessionIdSourceObject {
                r#type: "argvOption".to_owned(),
                argv_option: Some("--session".to_owned()),
            })
        );
        assert_eq!(
            agy.detect.as_ref().and_then(|d| d.process_names.clone()),
            Some(StringOrStringList::Many(vec![
                "agy".to_owned(),
                "antigravity".to_owned()
            ]))
        );

        let groups = config.workspace_groups.as_ref().expect("workspaceGroups");
        assert_eq!(groups.new_workspace_placement, NewWorkspacePlacement::Top);
        let entry = groups.by_cwd.get("~/work/*").expect("byCwd entry");
        assert_eq!(entry.color.as_deref(), Some("#7A4FD8"));
        assert_eq!(entry.icon.as_deref(), Some("ladybug.fill"));
        assert_eq!(
            entry.new_workspace_placement,
            Some(NewWorkspacePlacement::End)
        );
        // contextMenu stays opaque JSON (string | object items).
        assert_eq!(entry.context_menu.as_ref().map(Vec::len), Some(2));

        assert_eq!(config.new_workspace_command.as_deref(), Some("build"));
        // newWorkspaceCommand is now typed, not captured in `extra`.
        assert!(!config.extra.contains_key("newWorkspaceCommand"));

        // Encode → decode must be a fixed point.
        let encoded = encode_config(&config).expect("encode");
        let redecoded = decode_config(&encoded).expect("re-decode");
        assert_eq!(config, redecoded);
    }

    #[test]
    fn config_path_shape() {
        let base = Path::new("/some/base/config");
        let resolved = config_path_in(base);
        assert_eq!(
            resolved.file_name().and_then(|s| s.to_str()),
            Some(CONFIG_FILE_NAME)
        );
        assert_eq!(
            resolved
                .parent()
                .and_then(Path::file_name)
                .and_then(|s| s.to_str()),
            Some(CONFIG_DIR_NAME)
        );

        // On this platform dirs::config_dir() resolves, and the tail is stable.
        if let Some(path) = config_path() {
            assert_eq!(
                path.file_name().and_then(|s| s.to_str()),
                Some(CONFIG_FILE_NAME)
            );
            assert_eq!(
                path.parent()
                    .and_then(Path::file_name)
                    .and_then(|s| s.to_str()),
                Some(CONFIG_DIR_NAME)
            );
        }
    }
}
