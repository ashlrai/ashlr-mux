//! `cmux-config` — a serde model of the cmux configuration schema (`cmux.json`).
//!
//! This crate mirrors the canonical JSON Schema at `web/data/cmux.schema.json`.
//! It models the CORE, load-bearing sections that the Settings UI binds to
//! (app, terminal, notifications, sidebar, workspace colors, sidebar
//! appearance, automation, browser, markdown, canvas, file editor, file
//! explorer, diff viewer, shortcuts, vault, workspace groups, actions, ui,
//! commands, surface tab bar buttons) as strongly-typed serde structs/enums.
//!
//! Deliberately NOT strict: the top-level [`Config`] does not use
//! `deny_unknown_fields`. Any top-level section this crate does not model is
//! captured verbatim in [`Config::extra`] so a decode → encode round-trip does
//! not silently drop it. The four previously-unmodeled polymorphic sections
//! (`actions`, `ui`, `commands`, `surfaceTabBarButtons`) are now strongly
//! typed, mirroring the canonical Swift decoders in `Sources/CmuxConfig.swift`
//! and `Sources/CmuxConfigUI.swift`.
//!
//! The `ts` feature gates `ts_rs::TS` derives, exactly mirroring `cmux-core`.
//! It is inert in the default build (ts-rs is an optional dependency).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::de::Error as _;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[cfg(feature = "ts")]
use ts_rs::TS;

pub mod json_path;
pub use json_path::{JsonPath, JsonPathError};
pub mod notification_hooks;
pub use notification_hooks::{
    project_root, resolve_notification_hooks, resolved_hooks_for, ActionTrustDescriptor,
    ResolvedNotificationHook, DEFAULT_TIMEOUT_SECONDS,
};
pub mod right_sidebar_width;
pub use right_sidebar_width::RightSidebarWidthSettings;

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
    #[serde(
        rename = "lastUsedAt",
        default,
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        rename = "rightMaxWidth",
        default,
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(rename = "codexIntegration")]
    pub codex_integration: bool,
    #[serde(rename = "opencodeIntegration")]
    pub opencode_integration: bool,
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
            codex_integration: true,
            opencode_integration: true,
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
            bindings: BTreeMap::from([(
                "agent.warmClaudeCode".to_owned(),
                Some(ShortcutBinding::Single("ctrl+alt+c".to_owned())),
            )]),
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
    #[serde(
        rename = "processName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub process_name: Option<String>,
    #[serde(
        rename = "processNames",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub process_names: Option<StringOrStringList>,
    #[serde(
        rename = "argvContains",
        default,
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        rename = "argvOption",
        default,
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        rename = "iconAssetName",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon_asset_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub detect: Option<VaultAgentDetect>,
    #[serde(rename = "sessionIdSource")]
    pub session_id_source: VaultAgentSessionIdSource,
    #[serde(rename = "resumeCommand")]
    pub resume_command: String,
    #[serde(
        rename = "forkCommand",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub fork_command: Option<String>,
    pub cwd: VaultAgentCwd,
    #[serde(
        rename = "sessionDirectory",
        default,
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        rename = "contextMenu",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional, type = "Array<unknown>"))]
    pub context_menu: Option<Vec<serde_json::Value>>,
    /// Per-cwd override for new-workspace placement; falls back to the group's
    /// global default when omitted. Reuses the existing [`NewWorkspacePlacement`]
    /// enum.
    #[serde(
        rename = "newWorkspacePlacement",
        default,
        skip_serializing_if = "Option::is_none"
    )]
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
// Shared leaf types for `actions`, `ui`, `surfaceTabBarButtons`
//
// LENIENCY POLICY (applies to every custom decoder below).
// DIVERGENCE: the canonical Swift decoders trim `whitespacesAndNewlines` off
// every string field and hard-error on blank/required values; this crate keeps
// its established leniency and preserves string values verbatim (no trimming),
// defaulting absent-but-required leaf strings to `""` instead of erroring. Only
// the *structural* rules that are needed to disambiguate a polymorphic union
// (or that the task/Swift schema explicitly enforces — icon `type`,
// action `type`, `pane` vs `direction`, command `workspace`-xor-`command`) are
// reproduced as hard errors, so a malformed config fails to parse exactly as it
// does on macOS. Whitespace trimming is intentionally NOT reproduced because it
// would mutate data and break the lossless round-trip this crate guarantees.
// ---------------------------------------------------------------------------

/// `CmuxConfigTerminalCommandTarget` (`Sources/CmuxConfig.swift:328-333`): where
/// a terminal-command action runs. Swift `String` enum whose raw values are the
/// case names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "camelCase"))]
#[serde(rename_all = "camelCase")]
pub enum CmuxConfigTerminalCommandTarget {
    CurrentTerminal,
    NewTabInCurrentPane,
}

/// `CmuxConfigAgentKind` (`Sources/CmuxConfig.swift:357-409`): the built-in
/// agent an action can launch. Decodes the aliases `claude`/`claudeCode`/
/// `claude-code` → [`ClaudeCode`](Self::ClaudeCode) and encodes canonically
/// (`codex` / `claude`), matching Swift's custom `Codable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum CmuxConfigAgentKind {
    Codex,
    ClaudeCode,
}

impl CmuxConfigAgentKind {
    /// The default terminal command name for this agent
    /// (`Sources/CmuxConfig.swift:361-368`).
    fn command_name(self) -> &'static str {
        match self {
            CmuxConfigAgentKind::Codex => "codex",
            CmuxConfigAgentKind::ClaudeCode => "claude",
        }
    }
}

impl Serialize for CmuxConfigAgentKind {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Swift encodes `.codex` as "codex" and `.claudeCode` as "claude".
        serializer.serialize_str(match self {
            CmuxConfigAgentKind::Codex => "codex",
            CmuxConfigAgentKind::ClaudeCode => "claude",
        })
    }
}

impl<'de> Deserialize<'de> for CmuxConfigAgentKind {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        // Swift trims the token before matching; the alias set is closed.
        match raw.trim() {
            "codex" => Ok(CmuxConfigAgentKind::Codex),
            "claude" | "claudeCode" | "claude-code" => Ok(CmuxConfigAgentKind::ClaudeCode),
            other => Err(D::Error::custom(format!("Unknown agent '{other}'"))),
        }
    }
}

/// `CmuxButtonIcon` (`Sources/CmuxConfig.swift:411-469`): a polymorphic icon
/// discriminated on a `type` key. Accepts the `type` aliases
/// `symbol`/`sfSymbol`/`systemImage` and `image`/`file`; the emoji `scale`
/// defaults to `1` and is omitted on encode when equal to `1`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum CmuxButtonIcon {
    /// SF Symbol name (`{ "type": "symbol", "name": … }`).
    Symbol(String),
    /// Emoji glyph with an optional positive scale
    /// (`{ "type": "emoji", "value": …, "scale"?: … }`).
    Emoji {
        value: String,
        #[cfg_attr(feature = "ts", ts(type = "number"))]
        scale: f64,
    },
    /// Local image path (`{ "type": "image", "path": … }`).
    ImagePath(String),
}

impl Serialize for CmuxButtonIcon {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        match self {
            CmuxButtonIcon::Symbol(name) => {
                map.serialize_entry("type", "symbol")?;
                map.serialize_entry("name", name)?;
            }
            CmuxButtonIcon::Emoji { value, scale } => {
                map.serialize_entry("type", "emoji")?;
                map.serialize_entry("value", value)?;
                // Swift only writes `scale` when it differs from 1.
                if *scale != 1.0 {
                    map.serialize_entry("scale", scale)?;
                }
            }
            CmuxButtonIcon::ImagePath(path) => {
                map.serialize_entry("type", "image")?;
                map.serialize_entry("path", path)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for CmuxButtonIcon {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            r#type: Option<String>,
            name: Option<String>,
            value: Option<String>,
            path: Option<String>,
            scale: Option<f64>,
        }
        let raw = Raw::deserialize(deserializer)?;
        match raw.r#type.as_deref().map(str::trim) {
            Some("symbol") | Some("sfSymbol") | Some("systemImage") => Ok(CmuxButtonIcon::Symbol(
                raw.name.map(|s| s.trim().to_owned()).unwrap_or_default(),
            )),
            Some("emoji") => Ok(CmuxButtonIcon::Emoji {
                value: raw.value.map(|s| s.trim().to_owned()).unwrap_or_default(),
                // DIVERGENCE: Swift rejects non-finite / non-positive scale; we
                // keep the value verbatim (crate leniency) and only default a
                // missing scale to 1.
                scale: raw.scale.unwrap_or(1.0),
            }),
            Some("image") | Some("file") => Ok(CmuxButtonIcon::ImagePath(
                raw.path.map(|s| s.trim().to_owned()).unwrap_or_default(),
            )),
            Some(other) => Err(D::Error::custom(format!("Unknown icon type '{other}'"))),
            None => Err(D::Error::custom("icon requires a 'type'")),
        }
    }
}

/// `CmuxSurfaceTabBarButtonAction` (`Sources/CmuxConfig.swift:1079-1144`): the
/// polymorphic action carried by an `actions` entry or a `surfaceTabBarButtons`
/// entry. Not `Codable` on its own in Swift — it is inlined into its parent's
/// discriminator keys — so (de)serialization is handled by
/// [`CmuxConfigActionDefinition`] and [`CmuxSurfaceTabBarButton`], which encode
/// it differently (hence no `Serialize`/`Deserialize` derive here).
///
/// DIVERGENCE (leniency only): Swift validates the built-in id against the
/// `CmuxSurfaceTabBarBuiltInAction` registry and hard-errors on unknown ids.
/// The registry's closed alias table IS ported ([`builtin_action_canonical_id`])
/// so known aliases canonicalize exactly as on macOS, but
/// [`BuiltIn`](Self::BuiltIn) still holds a lenient `String`: unknown ids are
/// preserved rather than rejected, mirroring how `vault`'s `sessionIdSource`
/// type was kept a lenient `String`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum CmuxSurfaceTabBarButtonAction {
    /// A built-in action referenced by its config id.
    BuiltIn(String),
    /// A raw terminal command string.
    Command(String),
    /// Launch a built-in agent with optional argument string.
    Agent {
        agent: CmuxConfigAgentKind,
        args: Option<String>,
    },
    /// Run a named workspace command.
    WorkspaceCommand(String),
    /// A reference to another action / built-in by identifier.
    ActionReference(String),
}

impl CmuxSurfaceTabBarButtonAction {
    /// The identifier Swift falls back to when a button omits `id`
    /// (`CmuxSurfaceTabBarButtonAction.defaultId`,
    /// `Sources/CmuxConfig.swift:1086-1099`).
    fn default_id(&self) -> String {
        match self {
            CmuxSurfaceTabBarButtonAction::BuiltIn(id)
            | CmuxSurfaceTabBarButtonAction::ActionReference(id) => id.clone(),
            CmuxSurfaceTabBarButtonAction::Command(command) => {
                format!("command.{}", generated_command_id(command))
            }
            CmuxSurfaceTabBarButtonAction::Agent { agent, .. } => agent.command_name().to_owned(),
            CmuxSurfaceTabBarButtonAction::WorkspaceCommand(name) => {
                format!("workspaceCommand.{}", generated_command_id(name))
            }
        }
    }
}

/// Percent-encode a command string for use in a generated id
/// (`CmuxSurfaceTabBarButtonAction.generatedCommandId`,
/// `Sources/CmuxConfig.swift:1139-1143`).
///
/// DIVERGENCE: Swift's allowed set is the full-Unicode `CharacterSet`
/// `alphanumerics` ∪ `._-`; reproducing Unicode-wide alphanumeric membership in
/// Rust without a table is impractical, so this keeps only ASCII alphanumerics
/// (plus `._-`) and percent-encodes every other byte. This only affects
/// auto-generated ids for buttons that omit `id` and use non-ASCII commands.
fn generated_command_id(command: &str) -> String {
    let mut out = String::new();
    for byte in command.bytes() {
        let ch = byte as char;
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        } else {
            out.push('%');
            out.push_str(&format!("{byte:02X}"));
        }
    }
    if out.is_empty() {
        "command".to_owned()
    } else {
        out
    }
}

/// Port of `CmuxSurfaceTabBarBuiltInAction.init?(configID:)` plus its
/// `configID` raw value (`Sources/CmuxSurfaceTabBarBuiltInAction.swift:4-31`):
/// maps every accepted alias of a built-in surface-tab-bar action to the
/// canonical config id (the Swift enum's raw value). Returns `None` for
/// unknown ids. The alias table is closed and byte-identical to Swift's.
fn builtin_action_canonical_id(config_id: &str) -> Option<&'static str> {
    match config_id {
        "cmux.newWorkspace" | "newWorkspace" => Some("cmux.newWorkspace"),
        "cmux.cloudvm" | "cmux.cloudVM" | "cloudVM" | "cloudvm" | "cmux.newCloudVM"
        | "cmux.newCloudVm" | "newCloudVM" | "newCloudVm" | "cmux.startCloudVM"
        | "cmux.startCloudVm" | "startCloudVM" | "startCloudVm" => Some("cmux.cloudvm"),
        "cmux.newTerminal" | "newTerminal" => Some("cmux.newTerminal"),
        "cmux.newBrowser" | "newBrowser" => Some("cmux.newBrowser"),
        "cmux.splitRight" | "splitRight" => Some("cmux.splitRight"),
        "cmux.splitDown" | "splitDown" => Some("cmux.splitDown"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// actions
// ---------------------------------------------------------------------------

/// A single `actions` map value (`CmuxConfigActionDefinition`,
/// `Sources/CmuxConfig.swift:832-989`).
///
/// The action is inferred from the sibling discriminator keys (`type`, `agent`,
/// `builtin`, `command`, `commandName`/`name`) exactly as Swift does; `action`
/// is `None` when none of those keys are present. `subtitle` decodes from
/// either `subtitle` or `description` (and re-encodes as `subtitle`). The
/// `shortcut` field keeps the raw string-or-array wire form via
/// [`ShortcutBinding`].
///
/// DIVERGENCE: Swift parses `shortcut` into a structured `StoredShortcut` via
/// the keyboard-shortcut layer's `parseConfig` (modifier/key-token parsing) and
/// re-encodes the normalized form; that parser lives outside this crate, so we
/// preserve the raw `string | [string]` wire shape (reusing [`ShortcutBinding`])
/// which round-trips losslessly.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct CmuxConfigActionDefinition {
    #[cfg_attr(feature = "ts", ts(optional))]
    pub action: Option<CmuxSurfaceTabBarButtonAction>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub title: Option<String>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub subtitle: Option<String>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub keywords: Option<Vec<String>>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub palette: Option<bool>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub shortcut: Option<ShortcutBinding>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon: Option<CmuxButtonIcon>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub tooltip: Option<String>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub confirm: Option<bool>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub terminal_command_target: Option<CmuxConfigTerminalCommandTarget>,
}

impl Serialize for CmuxConfigActionDefinition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Mirrors `CmuxConfigActionDefinition.encode` (Swift:959-989).
        let mut map = serializer.serialize_map(None)?;
        if let Some(value) = &self.title {
            map.serialize_entry("title", value)?;
        }
        if let Some(value) = &self.subtitle {
            map.serialize_entry("subtitle", value)?;
        }
        if let Some(value) = &self.keywords {
            map.serialize_entry("keywords", value)?;
        }
        if let Some(value) = &self.palette {
            map.serialize_entry("palette", value)?;
        }
        if let Some(value) = &self.shortcut {
            map.serialize_entry("shortcut", value)?;
        }
        if let Some(value) = &self.icon {
            map.serialize_entry("icon", value)?;
        }
        if let Some(value) = &self.tooltip {
            map.serialize_entry("tooltip", value)?;
        }
        if let Some(value) = &self.confirm {
            map.serialize_entry("confirm", value)?;
        }
        if let Some(value) = &self.terminal_command_target {
            map.serialize_entry("target", value)?;
        }
        match &self.action {
            Some(CmuxSurfaceTabBarButtonAction::BuiltIn(id)) => {
                map.serialize_entry("type", "builtin")?;
                map.serialize_entry("builtin", id)?;
            }
            Some(CmuxSurfaceTabBarButtonAction::Command(command)) => {
                map.serialize_entry("type", "command")?;
                map.serialize_entry("command", command)?;
            }
            Some(CmuxSurfaceTabBarButtonAction::Agent { agent, args }) => {
                map.serialize_entry("type", "agent")?;
                map.serialize_entry("agent", agent)?;
                if let Some(args) = args {
                    map.serialize_entry("args", args)?;
                }
            }
            Some(CmuxSurfaceTabBarButtonAction::WorkspaceCommand(command_name)) => {
                map.serialize_entry("type", "workspaceCommand")?;
                map.serialize_entry("commandName", command_name)?;
            }
            // Swift re-encodes an actionReference as a builtin id.
            Some(CmuxSurfaceTabBarButtonAction::ActionReference(identifier)) => {
                map.serialize_entry("type", "builtin")?;
                map.serialize_entry("builtin", identifier)?;
            }
            None => {}
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for CmuxConfigActionDefinition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            r#type: Option<String>,
            builtin: Option<String>,
            command: Option<String>,
            #[serde(rename = "commandName")]
            command_name: Option<String>,
            name: Option<String>,
            // Buffered raw and parsed ONLY inside the "agent" arm: Swift
            // decodes `agent` solely in `case "agent"` (Swift:931-932) and
            // otherwise only checks key presence, so an invalid agent value
            // next to an explicit non-agent `type` never errors on macOS.
            // DIVERGENCE: the other lazily-decoded Swift keys (`builtin`,
            // `command`, `commandName`, `name`, `args`) are typed
            // `Option<String>` here, so NON-STRING junk under them fails this
            // decode even when an explicit `type` routes elsewhere, while
            // Swift ignores keys the winning arm never reads.
            agent: Option<serde_json::Value>,
            args: Option<String>,
            title: Option<String>,
            subtitle: Option<String>,
            description: Option<String>,
            keywords: Option<Vec<String>>,
            palette: Option<bool>,
            shortcut: Option<ShortcutBinding>,
            icon: Option<CmuxButtonIcon>,
            tooltip: Option<String>,
            confirm: Option<bool>,
            #[serde(rename = "target")]
            terminal_command_target: Option<CmuxConfigTerminalCommandTarget>,
        }
        let raw = Raw::deserialize(deserializer)?;

        // Inference order mirrors Swift:904-915 exactly (type, then agent, then
        // builtin, then command).
        // DIVERGENCE: an explicit JSON `null` under a discriminator key
        // (`agent`, `builtin`, `command`, `type`, …) decodes as `None` here,
        // i.e. exactly like an absent key. Swift's `container.contains(key)` is
        // TRUE for a null value, so `{ "agent": null }` infers type "agent" on
        // macOS and then hard-errors decoding null; here it infers nothing and
        // yields `action: None`. Pinned by
        // `explicit_null_discriminators_are_treated_as_absent`.
        let type_tag = raw
            .r#type
            .as_ref()
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        let inferred = type_tag
            .or_else(|| {
                raw.agent
                    .as_ref()
                    // Explicit null == absent (see the DIVERGENCE note above).
                    .filter(|value| !value.is_null())
                    .map(|_| "agent".to_owned())
            })
            .or_else(|| raw.builtin.as_ref().map(|_| "builtin".to_owned()))
            .or_else(|| raw.command.as_ref().map(|_| "command".to_owned()));

        let action = match inferred.as_deref() {
            // Swift:918-927 rejects unknown built-in ids and stores the
            // canonical enum (so aliases re-encode canonically). Known aliases
            // are canonicalized here too; unknown ids stay verbatim (crate
            // leniency — see the DIVERGENCE note on
            // `CmuxSurfaceTabBarButtonAction`).
            Some("builtin") => {
                let raw_builtin = raw.builtin.clone().unwrap_or_default();
                let builtin = builtin_action_canonical_id(raw_builtin.trim())
                    .map(str::to_owned)
                    .unwrap_or(raw_builtin);
                Some(CmuxSurfaceTabBarButtonAction::BuiltIn(builtin))
            }
            Some("command") => Some(CmuxSurfaceTabBarButtonAction::Command(
                raw.command.clone().unwrap_or_default(),
            )),
            Some("agent") => {
                let agent_value = raw
                    .agent
                    .filter(|value| !value.is_null())
                    .ok_or_else(|| D::Error::custom("agent actions require 'agent'"))?;
                let agent: CmuxConfigAgentKind =
                    serde_json::from_value(agent_value).map_err(D::Error::custom)?;
                Some(CmuxSurfaceTabBarButtonAction::Agent {
                    agent,
                    args: raw.args.clone(),
                })
            }
            Some("workspaceCommand") => {
                let command_name = raw
                    .command_name
                    .clone()
                    .or_else(|| raw.name.clone())
                    .or_else(|| raw.command.clone())
                    .ok_or_else(|| {
                        D::Error::custom("workspaceCommand actions require commandName")
                    })?;
                Some(CmuxSurfaceTabBarButtonAction::WorkspaceCommand(
                    command_name,
                ))
            }
            None => None,
            Some(other) => {
                return Err(D::Error::custom(format!("Unknown action type '{other}'")));
            }
        };

        Ok(CmuxConfigActionDefinition {
            action,
            title: raw.title,
            // Swift: `subtitle ?? description`.
            subtitle: raw.subtitle.or(raw.description),
            keywords: raw.keywords,
            palette: raw.palette,
            shortcut: raw.shortcut,
            icon: raw.icon,
            tooltip: raw.tooltip,
            confirm: raw.confirm,
            terminal_command_target: raw.terminal_command_target,
        })
    }
}

// ---------------------------------------------------------------------------
// surfaceTabBarButtons
// ---------------------------------------------------------------------------

/// A single `surfaceTabBarButtons` entry (`CmuxSurfaceTabBarButton`,
/// `Sources/CmuxConfig.swift:1146-1453`). Decodes either a bare legacy string
/// (→ [`ActionReference`](CmuxSurfaceTabBarButtonAction::ActionReference)) or an
/// object whose action is inferred from the discriminator keys.
///
/// The runtime-only Swift fields `actionSourcePath` / `iconSourcePath` are NOT
/// part of the JSON wire format (Swift sets them to `nil` on decode and never
/// encodes them), so they are intentionally omitted from this model.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct CmuxSurfaceTabBarButton {
    pub id: String,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub title: Option<String>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon: Option<CmuxButtonIcon>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub tooltip: Option<String>,
    pub action: CmuxSurfaceTabBarButtonAction,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub confirm: Option<bool>,
    #[cfg_attr(feature = "ts", ts(optional))]
    pub terminal_command_target: Option<CmuxConfigTerminalCommandTarget>,
}

impl Serialize for CmuxSurfaceTabBarButton {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Mirrors `CmuxSurfaceTabBarButton.encode` (Swift:1430-1453).
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("id", &self.id)?;
        if let Some(value) = &self.title {
            map.serialize_entry("title", value)?;
        }
        if let Some(value) = &self.icon {
            map.serialize_entry("icon", value)?;
        }
        if let Some(value) = &self.tooltip {
            map.serialize_entry("tooltip", value)?;
        }
        if let Some(value) = &self.confirm {
            map.serialize_entry("confirm", value)?;
        }
        if let Some(value) = &self.terminal_command_target {
            map.serialize_entry("target", value)?;
        }
        match &self.action {
            CmuxSurfaceTabBarButtonAction::BuiltIn(id) => {
                map.serialize_entry("builtin", id)?;
            }
            CmuxSurfaceTabBarButtonAction::Command(command) => {
                map.serialize_entry("command", command)?;
            }
            CmuxSurfaceTabBarButtonAction::Agent { agent, args } => {
                map.serialize_entry("agent", agent)?;
                if let Some(args) = args {
                    map.serialize_entry("args", args)?;
                }
            }
            CmuxSurfaceTabBarButtonAction::WorkspaceCommand(command_name) => {
                map.serialize_entry("type", "workspaceCommand")?;
                map.serialize_entry("commandName", command_name)?;
            }
            CmuxSurfaceTabBarButtonAction::ActionReference(identifier) => {
                map.serialize_entry("action", identifier)?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for CmuxSurfaceTabBarButton {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            // Legacy bare-string form (Swift:1284-1299).
            Legacy(String),
            // Boxed to keep the enum variants balanced (clippy::large_enum_variant).
            Object(Box<ObjectRaw>),
        }
        #[derive(Deserialize)]
        struct ObjectRaw {
            id: Option<String>,
            title: Option<String>,
            icon: Option<CmuxButtonIcon>,
            tooltip: Option<String>,
            action: Option<String>,
            builtin: Option<String>,
            command: Option<String>,
            agent: Option<CmuxConfigAgentKind>,
            args: Option<String>,
            r#type: Option<String>,
            #[serde(rename = "commandName")]
            command_name: Option<String>,
            name: Option<String>,
            confirm: Option<bool>,
            #[serde(rename = "target")]
            terminal_command_target: Option<CmuxConfigTerminalCommandTarget>,
        }

        match Raw::deserialize(deserializer)? {
            Raw::Legacy(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(D::Error::custom(
                        "surface tab bar button action must not be blank",
                    ));
                }
                // Swift:1294-1297 canonicalizes a legacy bare string through the
                // built-in registry: both the id and the action reference become
                // the canonical config id ("newTerminal" → "cmux.newTerminal");
                // unknown strings pass through verbatim.
                let id = builtin_action_canonical_id(trimmed).unwrap_or(trimmed);
                Ok(CmuxSurfaceTabBarButton {
                    id: id.to_owned(),
                    title: None,
                    icon: None,
                    tooltip: None,
                    action: CmuxSurfaceTabBarButtonAction::ActionReference(id.to_owned()),
                    confirm: None,
                    terminal_command_target: None,
                })
            }
            Raw::Object(object) => {
                let object = *object;
                // DIVERGENCE: an explicit JSON `null` under any key read here
                // decodes as `None`, i.e. exactly like an absent key. Swift
                // reads these fields behind `container.contains(key)` guards,
                // which are TRUE for null, so e.g. `{ "id": "x", "command": null }`
                // hard-errors on macOS (decoding null as String throws) while
                // here the null key is ignored. Pinned by
                // `explicit_null_discriminators_are_treated_as_absent`.
                //
                // Swift:1319-1333 — at most one action form may be defined.
                let defined = [
                    object.action.is_some(),
                    object.builtin.is_some(),
                    object.command.is_some(),
                    object.agent.is_some(),
                    object.r#type.is_some(),
                ]
                .into_iter()
                .filter(|&present| present)
                .count();
                if defined > 1 {
                    return Err(D::Error::custom(
                        "surfaceTabBarButtons entries must define only one of 'action', 'builtin', 'command', 'agent', or 'type'",
                    ));
                }

                let type_tag = object
                    .r#type
                    .as_ref()
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty());

                let action = if let Some(type_tag) = type_tag {
                    match type_tag.as_str() {
                        "workspaceCommand" => {
                            let command_name = object
                                .command_name
                                .clone()
                                .or_else(|| object.name.clone())
                                .ok_or_else(|| {
                                    D::Error::custom(
                                        "workspaceCommand surface tab bar buttons require commandName",
                                    )
                                })?;
                            CmuxSurfaceTabBarButtonAction::WorkspaceCommand(command_name)
                        }
                        other => {
                            return Err(D::Error::custom(format!(
                                "Unknown surface tab bar button type '{other}'"
                            )));
                        }
                    }
                } else if let Some(command) = object.command.clone() {
                    CmuxSurfaceTabBarButtonAction::Command(command)
                } else if let Some(agent) = object.agent {
                    CmuxSurfaceTabBarButtonAction::Agent {
                        agent,
                        args: object.args.clone(),
                    }
                } else if let Some(builtin) = object.builtin.clone() {
                    // Swift:1358-1366 rejects unknown `builtin` ids and stores
                    // the canonical enum. Known aliases are canonicalized here
                    // too; unknown ids stay verbatim (crate leniency).
                    let builtin = builtin_action_canonical_id(builtin.trim())
                        .map(str::to_owned)
                        .unwrap_or(builtin);
                    CmuxSurfaceTabBarButtonAction::BuiltIn(builtin)
                } else if let Some(action) = object.action.clone() {
                    CmuxSurfaceTabBarButtonAction::ActionReference(action)
                } else if let Some(id) = object.id.clone() {
                    // Swift:1369-1371 treats a bare `id` as a built-in only when
                    // it matches the CmuxSurfaceTabBarBuiltInAction registry
                    // (storing the canonical enum), and otherwise falls through
                    // to the missing-action error below.
                    // DIVERGENCE: an unknown bare id becomes a lenient BuiltIn
                    // here instead of that hard error (it re-encodes as
                    // `{ id, builtin: id }`, matching Swift's output for a real
                    // built-in).
                    let builtin = builtin_action_canonical_id(id.trim())
                        .map(str::to_owned)
                        .unwrap_or(id);
                    CmuxSurfaceTabBarButtonAction::BuiltIn(builtin)
                } else {
                    return Err(D::Error::custom(
                        "surfaceTabBarButtons entries must define 'action', 'builtin', 'command', 'agent', or 'type'",
                    ));
                };

                let id = object.id.clone().unwrap_or_else(|| action.default_id());
                Ok(CmuxSurfaceTabBarButton {
                    id,
                    title: object.title,
                    icon: object.icon,
                    tooltip: object.tooltip,
                    action,
                    confirm: object.confirm,
                    terminal_command_target: object.terminal_command_target,
                })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ui
// ---------------------------------------------------------------------------

/// `ui` (`CmuxConfigUIDefinition`, `Sources/CmuxConfigUI.swift:5-8`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct CmuxConfigUIDefinition {
    #[serde(rename = "newWorkspace", skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub new_workspace: Option<CmuxConfigButtonPlacement>,
    #[serde(rename = "surfaceTabBar", skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub surface_tab_bar: Option<CmuxSurfaceTabBarUIDefinition>,
}

/// `ui.surfaceTabBar` (`CmuxSurfaceTabBarUIDefinition`,
/// `Sources/CmuxConfigUI.swift:10-12`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct CmuxSurfaceTabBarUIDefinition {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub buttons: Option<Vec<CmuxSurfaceTabBarButton>>,
}

/// `ui.newWorkspace` (`CmuxConfigButtonPlacement`,
/// `Sources/CmuxConfigUI.swift:14-75`). `contextMenu` also accepts the legacy
/// alias `rightClick` on decode and always encodes as `contextMenu`.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct CmuxConfigButtonPlacement {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub action: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon: Option<CmuxButtonIcon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub tooltip: Option<String>,
    #[serde(rename = "contextMenu", skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub context_menu: Option<Vec<CmuxContextMenuItem>>,
}

impl<'de> Deserialize<'de> for CmuxConfigButtonPlacement {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            action: Option<String>,
            icon: Option<CmuxButtonIcon>,
            tooltip: Option<String>,
            #[serde(rename = "contextMenu")]
            context_menu: Option<Vec<CmuxContextMenuItem>>,
            // Buffered raw so it is only *parsed* when `contextMenu` is unusable,
            // mirroring Swift's short-circuiting `??` (CmuxConfigUI.swift:45-46):
            // a malformed `rightClick` alongside a valid `contextMenu` is never
            // decoded on macOS and therefore never errors.
            #[serde(rename = "rightClick")]
            right_click: Option<serde_json::Value>,
        }
        let raw = Raw::deserialize(deserializer)?;
        // Swift decodes `contextMenu ?? rightClick` (CmuxConfigUI.swift:45-46):
        // when BOTH keys are present, `contextMenu` wins and `rightClick` is
        // silently ignored — this is NOT a duplicate-key error. An explicit
        // JSON `null` under `contextMenu` behaves exactly like an absent key
        // (`decodeIfPresent` returns nil for null) and falls back to the legacy
        // `rightClick` alias; a null `rightClick` likewise yields no menu.
        // Pinned by `context_menu_and_right_click_both_present_prefers_context_menu`.
        let context_menu = match (raw.context_menu, raw.right_click) {
            (Some(menu), _) => Some(menu),
            (None, Some(value)) if !value.is_null() => Some(
                serde_json::from_value::<Vec<CmuxContextMenuItem>>(value)
                    .map_err(D::Error::custom)?,
            ),
            _ => None,
        };
        Ok(CmuxConfigButtonPlacement {
            action: raw.action,
            icon: raw.icon,
            tooltip: raw.tooltip,
            context_menu,
        })
    }
}

/// A `contextMenu` action entry
/// (`CmuxConfigContextMenuActionItem`, `Sources/CmuxConfigUI.swift:77-139`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct CmuxContextMenuActionItem {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub icon: Option<CmuxButtonIcon>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub tooltip: Option<String>,
}

/// A `contextMenu` item: a separator or an action
/// (`CmuxConfigContextMenuItem`, `Sources/CmuxConfigUI.swift:141-205`). Decodes
/// from a bare string (`"-"` / `"separator"` → separator, else an action id) or
/// an object (`{ "type": "separator" }` → separator, else an action item).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum CmuxContextMenuItem {
    Separator,
    Action(CmuxContextMenuActionItem),
}

impl Serialize for CmuxContextMenuItem {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            CmuxContextMenuItem::Separator => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("type", "separator")?;
                map.end()
            }
            CmuxContextMenuItem::Action(item) => item.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for CmuxContextMenuItem {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Object(Box<ObjectRaw>),
        }
        #[derive(Deserialize)]
        struct ObjectRaw {
            r#type: Option<String>,
            action: Option<String>,
            title: Option<String>,
            icon: Option<CmuxButtonIcon>,
            tooltip: Option<String>,
        }
        match Raw::deserialize(deserializer)? {
            Raw::Text(raw) => {
                let trimmed = raw.trim();
                if trimmed == "-" || trimmed == "separator" {
                    return Ok(CmuxContextMenuItem::Separator);
                }
                if trimmed.is_empty() {
                    return Err(D::Error::custom("contextMenu action must not be blank"));
                }
                Ok(CmuxContextMenuItem::Action(CmuxContextMenuActionItem {
                    action: trimmed.to_owned(),
                    title: None,
                    icon: None,
                    tooltip: None,
                }))
            }
            Raw::Object(object) => {
                if object.r#type.as_deref().map(str::trim) == Some("separator") {
                    return Ok(CmuxContextMenuItem::Separator);
                }
                let action = object
                    .action
                    .ok_or_else(|| D::Error::custom("action is required"))?;
                Ok(CmuxContextMenuItem::Action(CmuxContextMenuActionItem {
                    action,
                    title: object.title,
                    icon: object.icon,
                    tooltip: object.tooltip,
                }))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// commands (+ recursive workspace layout)
// ---------------------------------------------------------------------------

/// `commands[].restart` (`CmuxRestartBehavior`,
/// `Sources/CmuxConfigUI.swift:230-235`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum CmuxRestartBehavior {
    New,
    Recreate,
    Ignore,
    Confirm,
}

/// A recursive workspace layout node (`CmuxLayoutNode`,
/// `Sources/CmuxConfig.swift:1690-1740`): either a leaf `pane` or a `split`.
/// Discriminated by the presence of a `pane` key (leaf) vs a `direction` key
/// (split); the `split` arm is heap-boxed to break the type recursion.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub enum CmuxLayoutNode {
    Pane(CmuxPaneDefinition),
    Split(Box<CmuxSplitDefinition>),
}

impl Serialize for CmuxLayoutNode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            CmuxLayoutNode::Pane(pane) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("pane", pane)?;
                map.end()
            }
            // Swift flattens the split fields at the node level (Swift:1731-1739).
            CmuxLayoutNode::Split(split) => split.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for CmuxLayoutNode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            pane: Option<CmuxPaneDefinition>,
            direction: Option<CmuxSplitDirection>,
            split: Option<f64>,
            children: Option<Vec<CmuxLayoutNode>>,
        }
        let raw = Raw::deserialize(deserializer)?;
        // DIVERGENCE: Swift discriminates via `container.contains` (Swift:
        // 1703-1704), which is TRUE for an explicit JSON null — so
        // `{ "pane": null, "direction": … }` is a both-keys error on macOS and
        // `{ "pane": null }` errors decoding null; serde maps null → None, so
        // a null key counts as absent here (consistent with the pinned
        // null-discriminator divergence on the other decoders).
        match (raw.pane, raw.direction) {
            (Some(_), Some(_)) => Err(D::Error::custom(
                "CmuxLayoutNode must not contain both 'pane' and 'direction' keys",
            )),
            (Some(pane), None) => Ok(CmuxLayoutNode::Pane(pane)),
            (None, Some(direction)) => {
                // Swift decodes `children` non-optionally for a split node.
                let children = raw
                    .children
                    .ok_or_else(|| D::Error::custom("Split node requires 'children'"))?;
                // DIVERGENCE: Swift additionally requires exactly 2 children; we
                // relax that count check (crate leniency) and preserve the array
                // verbatim.
                Ok(CmuxLayoutNode::Split(Box::new(CmuxSplitDefinition {
                    direction,
                    split: raw.split,
                    children,
                })))
            }
            (None, None) => Err(D::Error::custom(
                "CmuxLayoutNode must contain either a 'pane' key or a 'direction' key",
            )),
        }
    }
}

/// A split layout node body (`CmuxSplitDefinition`,
/// `Sources/CmuxConfig.swift:1742-1779`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct CmuxSplitDefinition {
    pub direction: CmuxSplitDirection,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional, type = "number"))]
    pub split: Option<f64>,
    pub children: Vec<CmuxLayoutNode>,
}

/// `split.direction` (`CmuxSplitDirection`, `Sources/CmuxConfig.swift:1781-1784`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum CmuxSplitDirection {
    Horizontal,
    Vertical,
}

/// A leaf pane layout node (`CmuxPaneDefinition`,
/// `Sources/CmuxConfig.swift:1786-1805`).
///
/// DIVERGENCE: Swift requires at least one surface; we relax that (crate
/// leniency) and preserve the array verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct CmuxPaneDefinition {
    pub surfaces: Vec<CmuxSurfaceDefinition>,
}

/// A single surface within a pane (`CmuxSurfaceDefinition`,
/// `Sources/CmuxConfig.swift:1807-1815`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct CmuxSurfaceDefinition {
    #[serde(rename = "type")]
    pub surface_type: CmuxSurfaceType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub focus: Option<bool>,
}

/// `surface.type` (`CmuxSurfaceType`, `Sources/CmuxConfig.swift:1817-1821`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "lowercase"))]
#[serde(rename_all = "lowercase")]
pub enum CmuxSurfaceType {
    Terminal,
    Browser,
    Project,
}

/// A `commands[].workspace` definition (`CmuxWorkspaceDefinition`,
/// `Sources/CmuxWorkspaceDefinition.swift:3-47`).
///
/// DIVERGENCE: Swift normalizes `color` through `WorkspaceTabColorSettings`
/// (hex / named-color resolution against app `UserDefaults`) and errors on an
/// invalid value. That resolver is app-runtime state outside this crate, so
/// `color` is kept as a raw `String` (no normalization / validation).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
#[serde(default)]
pub struct CmuxWorkspaceDefinition {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub env: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout: Option<CmuxLayoutNode>,
}

/// A single `commands` entry (`CmuxCommandDefinition`,
/// `Sources/CmuxConfig.swift:1612-1688`). `name` must be non-blank and exactly
/// one of `workspace` / `command` must be present.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct CmuxCommandDefinition {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub keywords: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub restart: Option<CmuxRestartBehavior>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub workspace: Option<CmuxWorkspaceDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub confirm: Option<bool>,
}

impl<'de> Deserialize<'de> for CmuxCommandDefinition {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            name: String,
            description: Option<String>,
            keywords: Option<Vec<String>>,
            restart: Option<CmuxRestartBehavior>,
            workspace: Option<CmuxWorkspaceDefinition>,
            command: Option<String>,
            confirm: Option<bool>,
        }
        let raw = Raw::deserialize(deserializer)?;
        if raw.name.trim().is_empty() {
            return Err(D::Error::custom("Command name must not be blank"));
        }
        // DIVERGENCE: Swift also errors on a blank (present) `command`; we relax
        // that (crate leniency). The workspace/command mutual-exclusivity is
        // enforced because the task and Swift both require it.
        match (raw.workspace.is_some(), raw.command.is_some()) {
            (true, true) => {
                return Err(D::Error::custom(format!(
                    "Command '{}' must not define both 'workspace' and 'command'",
                    raw.name
                )));
            }
            (false, false) => {
                return Err(D::Error::custom(format!(
                    "Command '{}' must define either 'workspace' or 'command'",
                    raw.name
                )));
            }
            _ => {}
        }
        Ok(CmuxCommandDefinition {
            name: raw.name,
            description: raw.description,
            keywords: raw.keywords,
            restart: raw.restart,
            workspace: raw.workspace,
            command: raw.command,
            confirm: raw.confirm,
        })
    }
}

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

/// The top-level `cmux.json` document.
///
/// Modeled sections are strongly typed and optional (absent sections stay
/// absent on re-serialize). Every other top-level key not modeled here is
/// captured verbatim in [`Config::extra`] so a round-trip is non-lossy.
///
/// Deserialization goes through the private `ConfigShadow` so the
/// document-level validation Swift performs inside `CmuxConfigFile.init(from:)`
/// (`Sources/CmuxConfig.swift:46-152`) — blank / duplicate / alias-colliding
/// `actions` keys and duplicate surface-tab-bar button ids — fails the decode
/// exactly as it does on macOS.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
#[cfg_attr(feature = "ts", derive(TS), ts(export))]
pub struct Config {
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub schema: Option<String>,
    #[serde(
        rename = "schemaVersion",
        default,
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        rename = "workspaceColors",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub workspace_colors: Option<WorkspaceColorsConfig>,
    #[serde(
        rename = "sidebarAppearance",
        default,
        skip_serializing_if = "Option::is_none"
    )]
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
    #[serde(
        rename = "fileEditor",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub file_editor: Option<FileEditorConfig>,
    #[serde(
        rename = "fileExplorer",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub file_explorer: Option<FileExplorerConfig>,
    #[serde(
        rename = "diffViewer",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub diff_viewer: Option<DiffViewerConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub shortcuts: Option<ShortcutsConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub vault: Option<VaultConfig>,
    #[serde(
        rename = "workspaceGroups",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub workspace_groups: Option<WorkspaceGroupsConfig>,
    #[serde(
        rename = "newWorkspaceCommand",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub new_workspace_command: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub actions: Option<BTreeMap<String, CmuxConfigActionDefinition>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub ui: Option<CmuxConfigUIDefinition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub commands: Option<Vec<CmuxCommandDefinition>>,
    #[serde(
        rename = "surfaceTabBarButtons",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub surface_tab_bar_buttons: Option<Vec<CmuxSurfaceTabBarButton>>,
    /// Any top-level key not modeled above, preserved verbatim for lossless
    /// round-trips. Excluded from the TS bindings (opaque JSON).
    #[serde(flatten)]
    #[cfg_attr(feature = "ts", ts(skip))]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Deserialization shadow of [`Config`]: identical wire shape, zero
/// validation. [`Config`]'s `Deserialize` decodes this first and then applies
/// the document-level validation Swift runs during `CmuxConfigFile.init(from:)`
/// (`Sources/CmuxConfig.swift:46-152`); see `TryFrom<ConfigShadow> for Config`.
#[derive(Deserialize, Default)]
struct ConfigShadow {
    #[serde(rename = "$schema", default)]
    schema: Option<String>,
    #[serde(rename = "schemaVersion", default)]
    schema_version: Option<i64>,
    #[serde(default)]
    app: Option<AppConfig>,
    #[serde(default)]
    terminal: Option<TerminalConfig>,
    #[serde(default)]
    notifications: Option<NotificationsConfig>,
    #[serde(default)]
    sidebar: Option<SidebarConfig>,
    #[serde(rename = "workspaceColors", default)]
    workspace_colors: Option<WorkspaceColorsConfig>,
    #[serde(rename = "sidebarAppearance", default)]
    sidebar_appearance: Option<SidebarAppearanceConfig>,
    #[serde(default)]
    automation: Option<AutomationConfig>,
    #[serde(default)]
    browser: Option<BrowserConfig>,
    #[serde(default)]
    markdown: Option<MarkdownConfig>,
    #[serde(default)]
    canvas: Option<CanvasConfig>,
    #[serde(rename = "fileEditor", default)]
    file_editor: Option<FileEditorConfig>,
    #[serde(rename = "fileExplorer", default)]
    file_explorer: Option<FileExplorerConfig>,
    #[serde(rename = "diffViewer", default)]
    diff_viewer: Option<DiffViewerConfig>,
    #[serde(default)]
    shortcuts: Option<ShortcutsConfig>,
    #[serde(default)]
    vault: Option<VaultConfig>,
    #[serde(rename = "workspaceGroups", default)]
    workspace_groups: Option<WorkspaceGroupsConfig>,
    #[serde(rename = "newWorkspaceCommand", default)]
    new_workspace_command: Option<String>,
    #[serde(default)]
    actions: Option<BTreeMap<String, CmuxConfigActionDefinition>>,
    #[serde(default)]
    ui: Option<CmuxConfigUIDefinition>,
    #[serde(default)]
    commands: Option<Vec<CmuxCommandDefinition>>,
    #[serde(rename = "surfaceTabBarButtons", default)]
    surface_tab_bar_buttons: Option<Vec<CmuxSurfaceTabBarButton>>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let shadow = ConfigShadow::deserialize(deserializer)?;
        Config::try_from(shadow).map_err(D::Error::custom)
    }
}

impl TryFrom<ConfigShadow> for Config {
    type Error = String;

    /// The document-level validation `CmuxConfigFile.init(from:)` performs
    /// during decode (`Sources/CmuxConfig.swift:46-95`). Field values are moved
    /// across untouched — Swift's *rejection* rules are reproduced exactly,
    /// while its trimming normalizations stay unreproduced per the crate
    /// leniency policy above.
    fn try_from(shadow: ConfigShadow) -> Result<Self, Self::Error> {
        if let Some(actions) = &shadow.actions {
            validate_action_keys(actions)?;
        }
        // Swift:59-72 — a present-but-blank `newWorkspaceCommand` fails the
        // decode with this exact message (checked after `actions`, matching
        // Swift's statement order).
        // DIVERGENCE (representation only): Swift stores the TRIMMED command
        // back into the model; this crate preserves the value verbatim
        // (lossless round-trip policy) while reproducing the rejection.
        if let Some(command) = &shadow.new_workspace_command {
            if command.trim().is_empty() {
                return Err("newWorkspaceCommand must not be blank".to_owned());
            }
        }
        // Swift:74-88 — `ui.surfaceTabBar.buttons` takes precedence over the
        // root `surfaceTabBarButtons` key and ONLY the winning (configured)
        // list is validated: duplicate ids in the root list are accepted
        // whenever the ui-level list is present. Mirrored exactly; pinned by
        // `surface_tab_bar_button_duplicate_ids_rejected`.
        // DIVERGENCE (representation only): Swift *stores* the winning list
        // back into its `surfaceTabBarButtons` property (Swift:80), so a
        // re-encode on macOS writes the ui-level list under the root key too;
        // this crate keeps both fields verbatim where they appeared (lossless
        // round-trip policy) and consumers apply the ui-over-root precedence
        // themselves.
        let configured_buttons = shadow
            .ui
            .as_ref()
            .and_then(|ui| ui.surface_tab_bar.as_ref())
            .and_then(|bar| bar.buttons.as_deref())
            .or(shadow.surface_tab_bar_buttons.as_deref());
        if let Some(buttons) = configured_buttons {
            validate_surface_tab_bar_buttons(buttons)?;
        }
        Ok(Config {
            schema: shadow.schema,
            schema_version: shadow.schema_version,
            app: shadow.app,
            terminal: shadow.terminal,
            notifications: shadow.notifications,
            sidebar: shadow.sidebar,
            workspace_colors: shadow.workspace_colors,
            sidebar_appearance: shadow.sidebar_appearance,
            automation: shadow.automation,
            browser: shadow.browser,
            markdown: shadow.markdown,
            canvas: shadow.canvas,
            file_editor: shadow.file_editor,
            file_explorer: shadow.file_explorer,
            diff_viewer: shadow.diff_viewer,
            shortcuts: shadow.shortcuts,
            vault: shadow.vault,
            workspace_groups: shadow.workspace_groups,
            new_workspace_command: shadow.new_workspace_command,
            actions: shadow.actions,
            ui: shadow.ui,
            commands: shadow.commands,
            surface_tab_bar_buttons: shadow.surface_tab_bar_buttons,
            extra: shadow.extra,
        })
    }
}

/// Port of `CmuxConfigFile.normalizedActions`
/// (`Sources/CmuxConfig.swift:97-134`): rejects blank (empty-after-trim)
/// `actions` keys, keys that collide after trimming, and two keys that are
/// aliases of the same built-in action (via [`builtin_action_canonical_id`]),
/// with Swift's exact error messages.
///
/// DIVERGENCE (representation only): Swift *stores* the trimmed key back into
/// the dictionary; this crate preserves keys verbatim (lossless round-trip
/// policy above) while applying the identical accept/reject rules, so a config
/// decodes or fails exactly as on macOS. Two notes: (1) exact-duplicate raw
/// keys in the JSON text are collapsed last-wins by serde_json before this
/// runs and cannot be detected here; (2) Swift iterates its dictionary in
/// nondeterministic order, so *which* violation a multi-violation config
/// reports is arbitrary there — here iteration is over sorted keys
/// (deterministic). The decode outcome (Ok vs Err) is identical either way.
fn validate_action_keys(
    actions: &BTreeMap<String, CmuxConfigActionDefinition>,
) -> Result<(), String> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut canonical_ids: BTreeMap<&str, &str> = BTreeMap::new();
    for raw_id in actions.keys() {
        let id = raw_id.trim();
        if id.is_empty() {
            return Err("actions keys must not be blank".to_owned());
        }
        if !seen.insert(id) {
            return Err("actions must not contain duplicate ids".to_owned());
        }
        let canonical = builtin_action_canonical_id(id).unwrap_or(id);
        if let Some(existing) = canonical_ids.get(canonical) {
            return Err(format!(
                "actions must not contain duplicate aliases for '{canonical}' (found '{existing}' and '{id}')"
            ));
        }
        canonical_ids.insert(canonical, id);
    }
    Ok(())
}

/// Port of `CmuxConfigFile.validatedSurfaceTabBarButtons`
/// (`Sources/CmuxConfig.swift:136-152`): rejects duplicate button ids with
/// Swift's exact error message.
///
/// Ids are compared *trimmed*: Swift trims every explicit id (and
/// canonicalizes legacy bare strings) during button decode before its
/// `Set.insert`, while this crate stores explicit ids verbatim — trimming at
/// comparison time reproduces Swift's accept/reject outcome. Residual edge
/// (covered by the crate leniency DIVERGENCE above): an auto-generated id from
/// an untrimmed command (`"ls "` → `command.ls%20`) differs from Swift's
/// (`command.ls`), so a pathological pair like `"ls"` / `"ls "` collides on
/// macOS but not here.
fn validate_surface_tab_bar_buttons(buttons: &[CmuxSurfaceTabBarButton]) -> Result<(), String> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for button in buttons {
        if !seen.insert(button.id.trim()) {
            return Err("surface tab bar buttons must not contain duplicate ids".to_owned());
        }
    }
    Ok(())
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

/// Read the shared DEBUG-window display default from `app.devWindowDisplay`.
/// Missing/empty files and missing/blank values resolve to `None`; malformed
/// JSONC is reported so callers never silently overwrite a corrupt config.
pub fn dev_window_display_at(path: &Path) -> Result<Option<String>, String> {
    let root = read_jsonc_object(path)?;
    Ok(root
        .get("app")
        .and_then(serde_json::Value::as_object)
        .and_then(|app| app.get("devWindowDisplay"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string))
}

/// Set or clear `app.devWindowDisplay` in the shared JSONC config.
///
/// Writes sorted, pretty JSON just like canonical `JSONConfigStore`, follows a
/// configured symlink to preserve it, atomically replaces the resolved target,
/// and prunes `app` when clearing its final child.
pub fn set_dev_window_display_at(
    path: &Path,
    value: Option<&str>,
) -> Result<Option<String>, String> {
    let normalized = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let mut root = read_jsonc_object(path)?;
    if let Some(value) = &normalized {
        let app = root
            .entry("app".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !app.is_object() {
            *app = serde_json::json!({});
        }
        if let Some(app) = app.as_object_mut() {
            app.insert("devWindowDisplay".into(), serde_json::json!(value));
        }
    } else if let Some(app) = root
        .get_mut("app")
        .and_then(serde_json::Value::as_object_mut)
    {
        app.remove("devWindowDisplay");
        if app.is_empty() {
            root.remove("app");
        }
    }
    write_json_object_atomically(path, &root)?;
    Ok(normalized)
}

fn read_jsonc_object(path: &Path) -> Result<serde_json::Map<String, serde_json::Value>, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Default::default()),
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    if bytes.is_empty() {
        return Ok(Default::default());
    }
    let sanitized = cmux_jsonc::preprocess(&bytes)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    let value: serde_json::Value = serde_json::from_slice(&sanitized)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| format!("{} must contain a top-level JSON object", path.display()))
}

fn write_json_object_atomically(
    path: &Path,
    root: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    use std::io::Write;

    let write_path = resolved_config_write_path(path);
    let parent = write_path
        .parent()
        .ok_or_else(|| format!("config path {} has no parent", write_path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        format!(
            "failed to create temporary config in {}: {error}",
            parent.display()
        )
    })?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), root)
        .map_err(|error| format!("failed to encode {}: {error}", write_path.display()))?;
    temporary
        .as_file_mut()
        .write_all(b"\n")
        .map_err(|error| format!("failed to write {}: {error}", write_path.display()))?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|error| format!("failed to sync {}: {error}", write_path.display()))?;
    temporary.persist(&write_path).map_err(|error| {
        format!(
            "failed to replace {}: {}",
            write_path.display(),
            error.error
        )
    })?;
    Ok(())
}

fn resolved_config_write_path(path: &Path) -> PathBuf {
    let Ok(destination) = std::fs::read_link(path) else {
        return path.to_path_buf();
    };
    let target = if destination.is_absolute() {
        destination
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new(""))
            .join(destination)
    };
    std::fs::canonicalize(&target).unwrap_or(target)
}

/// Ordered Ghostty configuration candidates shared by the desktop and CLI.
///
/// XDG-style paths take precedence. Windows also falls back to Local AppData;
/// macOS also checks Ghostty's Application Support directory.
pub fn ghostty_config_candidates_from(
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
    local_app_data: Option<&Path>,
) -> Vec<PathBuf> {
    #[cfg(not(target_os = "windows"))]
    let _ = local_app_data;
    let mut candidates = Vec::new();
    let xdg_base = xdg_config_home
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".config")));
    if let Some(base) = xdg_base {
        candidates.push(base.join("ghostty").join("config.ghostty"));
        candidates.push(base.join("ghostty").join("config"));
    }

    #[cfg(target_os = "macos")]
    if let Some(home) = home {
        let base = home
            .join("Library")
            .join("Application Support")
            .join("com.mitchellh.ghostty");
        candidates.push(base.join("config.ghostty"));
        candidates.push(base.join("config"));
    }

    #[cfg(target_os = "windows")]
    if let Some(base) = local_app_data {
        candidates.push(base.join("ghostty").join("config.ghostty"));
        candidates.push(base.join("ghostty").join("config"));
    }

    candidates
}

/// Resolve the Ghostty config the desktop and CLI should inspect or edit.
/// Prefers the first existing candidate, falling back to the first candidate
/// when no config exists yet.
pub fn ghostty_config_path() -> Option<PathBuf> {
    fn nonempty_env_path(name: &str) -> Option<PathBuf> {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    let xdg = nonempty_env_path("XDG_CONFIG_HOME");
    let home = nonempty_env_path("HOME").or_else(|| nonempty_env_path("USERPROFILE"));
    let local_app_data = nonempty_env_path("LOCALAPPDATA");
    let candidates =
        ghostty_config_candidates_from(xdg.as_deref(), home.as_deref(), local_app_data.as_deref());
    candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
        .or_else(|| candidates.into_iter().next())
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
            browser
                .insecure_http_hosts_allowed_in_embedded_browser
                .len(),
            6
        );

        let colors = WorkspaceColorsConfig::default();
        assert_eq!(colors.indicator_style, "leftRail");
        assert_eq!(colors.colors.len(), 16);
        assert_eq!(
            colors.colors.get("Blue").map(String::as_str),
            Some("#1565C0")
        );

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
        assert_eq!(
            DiffViewerConfig::default().default_layout,
            DiffLayout::Unified
        );
        assert!(ShortcutsConfig::default().show_modifier_hold_hints);
        assert_eq!(
            ShortcutsConfig::default()
                .bindings
                .get("agent.warmClaudeCode"),
            Some(&Some(ShortcutBinding::Single("ctrl+alt+c".to_owned())))
        );

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

        let app = value
            .get("app")
            .and_then(|v| v.as_object())
            .expect("app obj");
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
        assert!(automation.contains_key("codexIntegration"));
        assert!(automation.contains_key("opencodeIntegration"));
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
        assert_eq!(
            terminal.resume_commands[0].command_prefix,
            ["npm", "run", "dev"]
        );

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
            "someFutureSection": { "foo": 1 },
            "newWorkspaceCommand": "build",
            "totallyUnknownKey": 42
        }"#;

        let config = decode_config(json).expect("decode");
        assert!(config.app.as_ref().unwrap().minimal_mode);
        // Still-unmodeled sections land in `extra`, not dropped.
        assert!(config.extra.contains_key("someFutureSection"));
        assert!(config.extra.contains_key("totallyUnknownKey"));
        // `newWorkspaceCommand` is a typed top-level field, no longer extra.
        assert!(!config.extra.contains_key("newWorkspaceCommand"));
        assert_eq!(config.new_workspace_command.as_deref(), Some("build"));

        // And they survive a round-trip.
        let encoded = encode_config(&config).expect("encode");
        let redecoded = decode_config(&encoded).expect("re-decode");
        assert_eq!(config, redecoded);
        assert_eq!(redecoded.extra.get("totallyUnknownKey").unwrap(), 42);
    }

    #[test]
    fn newly_typed_sections_leave_extra() {
        // The four sections that used to fall into `extra` are now typed.
        let json = r##"{
            "actions": { "greet": { "title": "Greet", "type": "command", "command": "echo hi" } },
            "ui": { "surfaceTabBar": { "buttons": [ "newTerminal" ] } },
            "commands": [ { "name": "build", "command": "make" } ],
            "surfaceTabBarButtons": [ { "command": "ls" } ]
        }"##;
        let config = decode_config(json).expect("decode");
        assert!(config.actions.is_some());
        assert!(config.ui.is_some());
        assert!(config.commands.is_some());
        assert!(config.surface_tab_bar_buttons.is_some());
        // None of them remain in `extra`.
        assert!(!config.extra.contains_key("actions"));
        assert!(!config.extra.contains_key("ui"));
        assert!(!config.extra.contains_key("commands"));
        assert!(!config.extra.contains_key("surfaceTabBarButtons"));
    }

    #[test]
    fn button_icon_variants_round_trip() {
        let symbol: CmuxButtonIcon =
            serde_json::from_str(r#"{ "type": "sfSymbol", "name": "terminal" }"#).expect("symbol");
        assert_eq!(symbol, CmuxButtonIcon::Symbol("terminal".to_owned()));
        // sfSymbol alias re-encodes canonically as "symbol".
        assert_eq!(
            serde_json::to_value(&symbol).unwrap(),
            serde_json::json!({ "type": "symbol", "name": "terminal" })
        );

        // Emoji without scale defaults to 1 and omits `scale` on encode.
        let emoji: CmuxButtonIcon =
            serde_json::from_str(r#"{ "type": "emoji", "value": "🚀" }"#).expect("emoji");
        assert_eq!(
            emoji,
            CmuxButtonIcon::Emoji {
                value: "🚀".to_owned(),
                scale: 1.0
            }
        );
        assert_eq!(
            serde_json::to_value(&emoji).unwrap(),
            serde_json::json!({ "type": "emoji", "value": "🚀" })
        );

        // Emoji with a non-default scale keeps it.
        let scaled: CmuxButtonIcon =
            serde_json::from_str(r#"{ "type": "emoji", "value": "🐛", "scale": 1.5 }"#)
                .expect("scaled");
        assert_eq!(
            serde_json::to_value(&scaled).unwrap(),
            serde_json::json!({ "type": "emoji", "value": "🐛", "scale": 1.5 })
        );

        // image/file aliases both map to ImagePath and re-encode as "image".
        let image: CmuxButtonIcon =
            serde_json::from_str(r#"{ "type": "file", "path": "~/x.png" }"#).expect("image");
        assert_eq!(image, CmuxButtonIcon::ImagePath("~/x.png".to_owned()));
        assert_eq!(
            serde_json::to_value(&image).unwrap(),
            serde_json::json!({ "type": "image", "path": "~/x.png" })
        );

        // Unknown icon type is a hard error, matching Swift.
        assert!(serde_json::from_str::<CmuxButtonIcon>(r#"{ "type": "bogus" }"#).is_err());
    }

    #[test]
    fn actions_action_type_tags_round_trip() {
        let json = r##"{
            "runBuild": {
                "type": "command",
                "command": "make",
                "title": "Build",
                "description": "Compile the project",
                "keywords": ["compile"],
                "palette": true,
                "shortcut": "cmd+b",
                "icon": { "type": "symbol", "name": "hammer" },
                "confirm": true,
                "target": "currentTerminal"
            },
            "startCodex": { "type": "agent", "agent": "claude-code", "args": "--yolo" },
            "openThing": { "type": "builtin", "builtin": "newTerminal" },
            "wsCmd": { "type": "workspaceCommand", "commandName": "layout1" },
            "noAction": { "title": "Just a label" }
        }"##;
        let map: BTreeMap<String, CmuxConfigActionDefinition> =
            serde_json::from_str(json).expect("decode actions");

        let build = &map["runBuild"];
        assert_eq!(
            build.action,
            Some(CmuxSurfaceTabBarButtonAction::Command("make".to_owned()))
        );
        // subtitle falls back to `description`.
        assert_eq!(build.subtitle.as_deref(), Some("Compile the project"));
        assert_eq!(
            build.shortcut,
            Some(ShortcutBinding::Single("cmd+b".to_owned()))
        );
        assert_eq!(
            build.terminal_command_target,
            Some(CmuxConfigTerminalCommandTarget::CurrentTerminal)
        );

        // claude-code alias normalizes to ClaudeCode and re-encodes as "claude".
        assert_eq!(
            map["startCodex"].action,
            Some(CmuxSurfaceTabBarButtonAction::Agent {
                agent: CmuxConfigAgentKind::ClaudeCode,
                args: Some("--yolo".to_owned())
            })
        );
        // Known builtin aliases canonicalize exactly as Swift's enum does.
        assert_eq!(
            map["openThing"].action,
            Some(CmuxSurfaceTabBarButtonAction::BuiltIn(
                "cmux.newTerminal".to_owned()
            ))
        );
        assert_eq!(
            map["wsCmd"].action,
            Some(CmuxSurfaceTabBarButtonAction::WorkspaceCommand(
                "layout1".to_owned()
            ))
        );
        // No discriminator keys → action is None.
        assert_eq!(map["noAction"].action, None);

        // Encode → decode is a fixed point once the aliases are canonicalized.
        let encoded = serde_json::to_string(&map).expect("encode");
        let redecoded: BTreeMap<String, CmuxConfigActionDefinition> =
            serde_json::from_str(&encoded).expect("re-decode");
        // agent re-encodes as "claude"; verify the canonical form re-decodes equal.
        assert_eq!(map, redecoded);
    }

    #[test]
    fn surface_tab_bar_buttons_string_and_object_forms() {
        let json = r##"[
            "newTerminal",
            { "command": "ls -la" },
            { "id": "myAgent", "agent": "codex", "args": "-q", "title": "Codex" },
            { "type": "workspaceCommand", "commandName": "grid" },
            { "action": "somethingCustom", "icon": { "type": "emoji", "value": "✨" } }
        ]"##;
        let buttons: Vec<CmuxSurfaceTabBarButton> =
            serde_json::from_str(json).expect("decode buttons");
        assert_eq!(buttons.len(), 5);

        // Bare string → actionReference; a built-in alias canonicalizes to its
        // registry id (Swift:1294-1297).
        assert_eq!(buttons[0].id, "cmux.newTerminal");
        assert_eq!(
            buttons[0].action,
            CmuxSurfaceTabBarButtonAction::ActionReference("cmux.newTerminal".to_owned())
        );

        // command form; id defaults from the command via generatedCommandId.
        assert_eq!(
            buttons[1].action,
            CmuxSurfaceTabBarButtonAction::Command("ls -la".to_owned())
        );
        assert_eq!(buttons[1].id, "command.ls%20-la");

        // explicit id preserved; agent + args.
        assert_eq!(buttons[2].id, "myAgent");
        assert_eq!(
            buttons[2].action,
            CmuxSurfaceTabBarButtonAction::Agent {
                agent: CmuxConfigAgentKind::Codex,
                args: Some("-q".to_owned())
            }
        );
        assert_eq!(
            buttons[3].action,
            CmuxSurfaceTabBarButtonAction::WorkspaceCommand("grid".to_owned())
        );
        assert_eq!(
            buttons[4].action,
            CmuxSurfaceTabBarButtonAction::ActionReference("somethingCustom".to_owned())
        );

        // Defining two action forms in one object is rejected.
        assert!(serde_json::from_str::<CmuxSurfaceTabBarButton>(
            r#"{ "command": "ls", "agent": "codex" }"#
        )
        .is_err());
    }

    #[test]
    fn ui_right_click_alias_and_context_menu_items() {
        // `rightClick` decodes as `contextMenu` and re-encodes as `contextMenu`.
        let json = r##"{
            "newWorkspace": {
                "action": "newWorkspace",
                "icon": { "type": "symbol", "name": "plus" },
                "rightClick": [
                    "-",
                    "newTerminal",
                    { "type": "separator" },
                    { "action": "cloudVM", "title": "Cloud", "icon": { "type": "symbol", "name": "cloud" } }
                ]
            },
            "surfaceTabBar": { "buttons": [ "splitRight" ] }
        }"##;
        let ui: CmuxConfigUIDefinition = serde_json::from_str(json).expect("decode ui");
        let placement = ui.new_workspace.as_ref().expect("newWorkspace");
        let menu = placement.context_menu.as_ref().expect("contextMenu");
        assert_eq!(menu.len(), 4);
        assert_eq!(menu[0], CmuxContextMenuItem::Separator);
        assert_eq!(
            menu[1],
            CmuxContextMenuItem::Action(CmuxContextMenuActionItem {
                action: "newTerminal".to_owned(),
                title: None,
                icon: None,
                tooltip: None,
            })
        );
        assert_eq!(menu[2], CmuxContextMenuItem::Separator);
        assert!(matches!(&menu[3], CmuxContextMenuItem::Action(item) if item.action == "cloudVM"));

        // Re-encode uses `contextMenu`, not `rightClick`.
        let value = serde_json::to_value(&ui).expect("encode ui");
        assert!(value["newWorkspace"].get("contextMenu").is_some());
        assert!(value["newWorkspace"].get("rightClick").is_none());

        assert_eq!(
            ui.surface_tab_bar
                .as_ref()
                .and_then(|s| s.buttons.as_ref())
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn actions_keys_blank_and_duplicate_rules() {
        // Oracle: CmuxConfigFile.normalizedActions (Sources/CmuxConfig.swift:97-134).
        // Blank (whitespace-only) key → decode error.
        let blank = r#"{ "actions": { "   ": { "command": "ls" } } }"#;
        let err = decode_config(blank).unwrap_err().to_string();
        assert!(err.contains("actions keys must not be blank"), "{err}");

        // Two keys that collide after trimming → decode error.
        let dup =
            r#"{ "actions": { "deploy": { "command": "a" }, "deploy ": { "command": "b" } } }"#;
        let err = decode_config(dup).unwrap_err().to_string();
        assert!(
            err.contains("actions must not contain duplicate ids"),
            "{err}"
        );

        // Two aliases of the same built-in action → decode error naming the
        // canonical id and both offending keys.
        let alias = r#"{ "actions": { "newTerminal": {}, "cmux.newTerminal": {} } }"#;
        let err = decode_config(alias).unwrap_err().to_string();
        assert!(
            err.contains(
                "actions must not contain duplicate aliases for 'cmux.newTerminal' (found 'cmux.newTerminal' and 'newTerminal')"
            ),
            "{err}"
        );

        // Distinct, non-aliased keys decode; the verbatim (untrimmed) key is
        // preserved (see the DIVERGENCE note on validate_action_keys — Swift
        // would store the trimmed key).
        let ok = r#"{ "actions": { " deploy ": { "command": "a" }, "test": { "command": "b" } } }"#;
        let config = decode_config(ok).expect("decode");
        assert!(config.actions.as_ref().unwrap().contains_key(" deploy "));
    }

    #[test]
    fn surface_tab_bar_button_duplicate_ids_rejected() {
        // Oracle: CmuxConfigFile.validatedSurfaceTabBarButtons
        // (Sources/CmuxConfig.swift:136-152).
        // Root-level duplicates, including via alias canonicalization of legacy
        // bare strings ("newTerminal" and "cmux.newTerminal" share one id).
        let dup = r#"{ "surfaceTabBarButtons": ["newTerminal", "cmux.newTerminal"] }"#;
        let err = decode_config(dup).unwrap_err().to_string();
        assert!(
            err.contains("surface tab bar buttons must not contain duplicate ids"),
            "{err}"
        );

        // ui-level duplicates are rejected the same way.
        let ui_dup =
            r#"{ "ui": { "surfaceTabBar": { "buttons": ["splitRight", "splitRight"] } } }"#;
        assert!(decode_config(ui_dup).is_err());

        // Swift validates ONLY the configured (winning) list: ui buttons take
        // precedence, so root duplicates are accepted when
        // ui.surfaceTabBar.buttons is present (Sources/CmuxConfig.swift:74-88).
        let root_dup_ui_present = r#"{
            "surfaceTabBarButtons": ["newTerminal", "newTerminal"],
            "ui": { "surfaceTabBar": { "buttons": ["splitRight"] } }
        }"#;
        decode_config(root_dup_ui_present)
            .expect("root duplicates ignored when ui buttons configured");

        // Explicit ids are compared trimmed, exactly like Swift's decode-time
        // trimming.
        let trimmed_dup = r#"{ "surfaceTabBarButtons": [
            { "id": "b1", "command": "ls" },
            { "id": " b1 ", "command": "pwd" }
        ] }"#;
        assert!(decode_config(trimmed_dup).is_err());

        // Unique ids pass.
        let ok = r#"{ "surfaceTabBarButtons": ["newTerminal", "newBrowser"] }"#;
        decode_config(ok).expect("unique ids decode");
    }

    #[test]
    fn legacy_string_buttons_canonicalize_builtin_aliases() {
        // Swift maps a legacy bare-string button through the built-in registry
        // (Sources/CmuxConfig.swift:1294-1297): id and reference become the
        // canonical id; unknown strings pass through verbatim.
        let buttons: Vec<CmuxSurfaceTabBarButton> =
            serde_json::from_str(r#"["splitDown", "custom.thing"]"#).expect("decode");
        assert_eq!(buttons[0].id, "cmux.splitDown");
        assert_eq!(
            buttons[0].action,
            CmuxSurfaceTabBarButtonAction::ActionReference("cmux.splitDown".to_owned())
        );
        assert_eq!(buttons[1].id, "custom.thing");
        assert_eq!(
            buttons[1].action,
            CmuxSurfaceTabBarButtonAction::ActionReference("custom.thing".to_owned())
        );
    }

    #[test]
    fn context_menu_and_right_click_both_present_prefers_context_menu() {
        // Swift decodes `contextMenu ?? rightClick` (CmuxConfigUI.swift:45-46):
        // both keys present is NOT an error — contextMenu wins, rightClick is
        // ignored.
        let both = r#"{
            "contextMenu": ["newTerminal"],
            "rightClick": ["newBrowser"]
        }"#;
        let placement: CmuxConfigButtonPlacement = serde_json::from_str(both).expect("decode");
        let menu = placement.context_menu.expect("contextMenu wins");
        assert_eq!(menu.len(), 1);
        assert!(
            matches!(&menu[0], CmuxContextMenuItem::Action(item) if item.action == "newTerminal")
        );

        // Because Swift's `??` short-circuits, a malformed rightClick next to a
        // valid contextMenu is never parsed (and never errors). Mirrored via
        // lazy parsing of the buffered rightClick value.
        let lazy = r#"{ "contextMenu": ["newTerminal"], "rightClick": 42 }"#;
        assert!(serde_json::from_str::<CmuxConfigButtonPlacement>(lazy).is_ok());

        // Explicit JSON null under contextMenu behaves like an absent key
        // (Swift decodeIfPresent) and falls back to rightClick.
        let null_fallback = r#"{ "contextMenu": null, "rightClick": ["newBrowser"] }"#;
        let placement: CmuxConfigButtonPlacement =
            serde_json::from_str(null_fallback).expect("decode");
        let menu = placement.context_menu.expect("falls back to rightClick");
        assert!(
            matches!(&menu[0], CmuxContextMenuItem::Action(item) if item.action == "newBrowser")
        );

        // A null rightClick alone yields no menu.
        let null_rc = r#"{ "rightClick": null }"#;
        let placement: CmuxConfigButtonPlacement = serde_json::from_str(null_rc).expect("decode");
        assert!(placement.context_menu.is_none());

        // A malformed rightClick WITHOUT a usable contextMenu is a decode
        // error, exactly as decodeIfPresent throws on macOS.
        let bad = r#"{ "rightClick": 42 }"#;
        assert!(serde_json::from_str::<CmuxConfigButtonPlacement>(bad).is_err());
    }

    #[test]
    fn explicit_null_discriminators_are_treated_as_absent() {
        // DIVERGENCE pin (see the decoder comments): Swift's
        // `container.contains(key)` is TRUE for an explicit JSON null, so
        // `{ "id": "x", "command": null }` hard-errors on macOS (decoding null
        // as String throws). serde maps null → None, so this crate treats the
        // key as absent instead: the bare-id fallback applies here.
        let button: CmuxSurfaceTabBarButton =
            serde_json::from_str(r#"{ "id": "cmux.newTerminal", "command": null }"#)
                .expect("decode");
        assert_eq!(
            button.action,
            CmuxSurfaceTabBarButtonAction::BuiltIn("cmux.newTerminal".to_owned())
        );

        // Same for an action definition: `{ "agent": null }` infers type
        // "agent" on macOS (key present) and errors decoding null; here no
        // action is inferred at all.
        let action: CmuxConfigActionDefinition =
            serde_json::from_str(r#"{ "agent": null }"#).expect("decode");
        assert_eq!(action.action, None);
    }

    #[test]
    fn blank_new_workspace_command_rejected() {
        // Oracle: CmuxConfigFile.init(from:) (Sources/CmuxConfig.swift:59-72).
        let err = decode_config(r#"{ "newWorkspaceCommand": "   " }"#)
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("newWorkspaceCommand must not be blank"),
            "{err}"
        );
        assert!(decode_config(r#"{ "newWorkspaceCommand": "" }"#).is_err());

        // Non-blank decodes; the value is preserved VERBATIM (Swift stores it
        // trimmed — see the DIVERGENCE note in TryFrom;
        // cmuxTests/CmuxConfigTests.swift:139-147 shows Swift's trimming).
        let config = decode_config(r#"{ "newWorkspaceCommand": "  build  " }"#).expect("decode");
        assert_eq!(config.new_workspace_command.as_deref(), Some("  build  "));

        // Explicit null behaves like an absent key (decodeIfPresent).
        let config = decode_config(r#"{ "newWorkspaceCommand": null }"#).expect("decode");
        assert!(config.new_workspace_command.is_none());
    }

    #[test]
    fn action_agent_key_is_lazily_decoded() {
        // Swift decodes `agent` ONLY inside the "agent" arm
        // (Sources/CmuxConfig.swift:931-932); elsewhere it merely checks key
        // presence (Swift:907), so an invalid agent value next to an explicit
        // non-agent `type` never errors on macOS.
        let action: CmuxConfigActionDefinition =
            serde_json::from_str(r#"{ "type": "command", "command": "ls", "agent": "bogus" }"#)
                .expect("agent ignored when an explicit type routes elsewhere");
        assert_eq!(
            action.action,
            Some(CmuxSurfaceTabBarButtonAction::Command("ls".to_owned()))
        );

        // When the agent arm IS taken, the invalid agent still errors, as on
        // macOS (Unknown agent).
        assert!(
            serde_json::from_str::<CmuxConfigActionDefinition>(r#"{ "agent": "bogus" }"#).is_err()
        );
        assert!(serde_json::from_str::<CmuxConfigActionDefinition>(
            r#"{ "type": "agent", "agent": "bogus" }"#
        )
        .is_err());
        // Missing agent under an explicit agent type errors, as on macOS
        // (keyNotFound there; null == absent per the pinned divergence).
        assert!(serde_json::from_str::<CmuxConfigActionDefinition>(
            r#"{ "type": "agent", "agent": null }"#
        )
        .is_err());
    }

    #[test]
    fn commands_recursive_workspace_layout() {
        let json = r##"[
            {
                "name": "split-dev",
                "description": "Editor beside a shell",
                "keywords": ["dev"],
                "restart": "recreate",
                "workspace": {
                    "name": "Dev",
                    "cwd": "~/proj",
                    "layout": {
                        "direction": "horizontal",
                        "split": 0.4,
                        "children": [
                            { "pane": { "surfaces": [ { "type": "terminal", "command": "vim" } ] } },
                            {
                                "direction": "vertical",
                                "children": [
                                    { "pane": { "surfaces": [ { "type": "terminal" } ] } },
                                    { "pane": { "surfaces": [ { "type": "browser", "url": "http://localhost:3000" } ] } }
                                ]
                            }
                        ]
                    }
                }
            },
            { "name": "run-tests", "command": "cargo test" }
        ]"##;
        let commands: Vec<CmuxCommandDefinition> =
            serde_json::from_str(json).expect("decode commands");
        assert_eq!(commands.len(), 2);

        let split = &commands[0];
        assert_eq!(split.restart, Some(CmuxRestartBehavior::Recreate));
        let layout = split
            .workspace
            .as_ref()
            .and_then(|w| w.layout.as_ref())
            .expect("layout");
        let CmuxLayoutNode::Split(root) = layout else {
            panic!("expected split root");
        };
        assert_eq!(root.direction, CmuxSplitDirection::Horizontal);
        assert_eq!(root.split, Some(0.4));
        assert_eq!(root.children.len(), 2);
        // First child is a leaf pane; second is a nested vertical split.
        assert!(matches!(root.children[0], CmuxLayoutNode::Pane(_)));
        let CmuxLayoutNode::Split(nested) = &root.children[1] else {
            panic!("expected nested split");
        };
        assert_eq!(nested.direction, CmuxSplitDirection::Vertical);
        assert_eq!(nested.children.len(), 2);

        assert_eq!(commands[1].command.as_deref(), Some("cargo test"));

        // Recursive layout survives a round-trip.
        let encoded = serde_json::to_string(&commands).expect("encode");
        let redecoded: Vec<CmuxCommandDefinition> =
            serde_json::from_str(&encoded).expect("re-decode");
        assert_eq!(commands, redecoded);
    }

    #[test]
    fn commands_and_layout_structural_errors() {
        // Neither workspace nor command → error.
        assert!(serde_json::from_str::<CmuxCommandDefinition>(r#"{ "name": "x" }"#).is_err());
        // Both workspace and command → error.
        assert!(serde_json::from_str::<CmuxCommandDefinition>(
            r#"{ "name": "x", "command": "a", "workspace": {} }"#
        )
        .is_err());
        // Blank name → error.
        assert!(serde_json::from_str::<CmuxCommandDefinition>(
            r#"{ "name": "  ", "command": "a" }"#
        )
        .is_err());
        // Layout node with both pane and direction → error.
        assert!(serde_json::from_str::<CmuxLayoutNode>(
            r#"{ "pane": { "surfaces": [] }, "direction": "horizontal", "children": [] }"#
        )
        .is_err());
        // Layout node with neither → error.
        assert!(serde_json::from_str::<CmuxLayoutNode>(r#"{ "split": 0.5 }"#).is_err());
    }

    #[test]
    fn full_config_with_new_sections_round_trips() {
        let json = r##"{
            "actions": { "a1": { "type": "command", "command": "echo hi" } },
            "surfaceTabBarButtons": [ "newTerminal", { "id": "b2", "command": "ls" } ],
            "ui": {
                "newWorkspace": { "action": "newWorkspace", "contextMenu": [ "newTerminal" ] }
            },
            "commands": [ { "name": "build", "command": "make" } ],
            "app": { "minimalMode": true },
            "futureThing": { "x": 1 }
        }"##;
        let config = decode_config(json).expect("decode");
        let encoded = encode_config(&config).expect("encode");
        let redecoded = decode_config(&encoded).expect("re-decode");
        assert_eq!(config, redecoded);
        // Unknown key still preserved.
        assert!(redecoded.extra.contains_key("futureThing"));
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

    #[test]
    fn ghostty_config_candidates_prefer_xdg_config_home() {
        let xdg = Path::new("C:/xdg");
        let home = Path::new("C:/Users/example");
        let local = Path::new("C:/Users/example/AppData/Local");
        let candidates = ghostty_config_candidates_from(Some(xdg), Some(home), Some(local));
        assert_eq!(candidates[0], xdg.join("ghostty").join("config.ghostty"));
        assert_eq!(candidates[1], xdg.join("ghostty").join("config"));
        #[cfg(target_os = "windows")]
        assert_eq!(candidates[2], local.join("ghostty").join("config.ghostty"));
    }

    #[test]
    fn ghostty_config_candidates_default_xdg_to_home_config() {
        let home = Path::new("C:/Users/example");
        let candidates = ghostty_config_candidates_from(None, Some(home), None);
        assert_eq!(
            candidates[0],
            home.join(".config").join("ghostty").join("config.ghostty")
        );
        assert_eq!(
            candidates[1],
            home.join(".config").join("ghostty").join("config")
        );
    }

    #[test]
    fn dev_window_display_round_trips_jsonc_and_prunes_empty_app() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("cmux-default-display-{unique}"));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("cmux.json");
        std::fs::write(
            &path,
            b"{ // keep readable\n  \"app\": { \"keep\": true, \"devWindowDisplay\": \"Old\", },\n  \"browser\": {},\n}",
        )
        .unwrap();

        assert_eq!(
            dev_window_display_at(&path).unwrap().as_deref(),
            Some("Old")
        );
        assert_eq!(
            set_dev_window_display_at(&path, Some("  Display 2  ")).unwrap(),
            Some("Display 2".to_string())
        );
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["app"]["devWindowDisplay"], "Display 2");
        assert_eq!(value["app"]["keep"], true);
        assert_eq!(value["browser"], serde_json::json!({}));

        set_dev_window_display_at(&path, None).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(value["app"].get("devWindowDisplay").is_none());
        assert_eq!(value["app"]["keep"], true);

        std::fs::write(&path, b"{\"app\":{\"devWindowDisplay\":\"Only\"}}").unwrap();
        set_dev_window_display_at(&path, Some(" ")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}\n");

        std::fs::remove_dir_all(root).unwrap();
    }
}
