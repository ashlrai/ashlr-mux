//! Pure CLI command -> v2 control-socket method mapping.
//!
//! The desktop backend already exposes a focused set of workspace/surface v2
//! methods. This module gives the user-facing CLI spellings a real route to
//! those methods instead of falling through to the generic "not yet ported"
//! socket failure.

use crate::invocation::CliError;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CMUX_WORKSPACE_ID_ENV: &str = "CMUX_WORKSPACE_ID";
pub const CMUX_SURFACE_ID_ENV: &str = "CMUX_SURFACE_ID";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlCommand {
    pub method: String,
    pub params: serde_json::Value,
}

impl ControlCommand {
    fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }

    pub fn with_ambient_workspace_id(mut self, workspace_id: Option<&str>) -> Self {
        let Some(workspace_id) = workspace_id.map(str::trim) else {
            return self;
        };
        if !workspace_scoped_method(&self.method) {
            return self;
        }
        let Some(params) = self.params.as_object_mut() else {
            return self;
        };
        if params.contains_key("suppress_ambient_workspace") {
            return self;
        }
        if workspace_id.is_empty() {
            if matches!(self.method.as_str(), "tab.action" | "surface.respawn") {
                params.remove("resolve_current_workspace");
                params.insert("suppress_ambient_workspace".into(), serde_json::json!(true));
            }
            return self;
        }
        if ["workspace_id", "workspace_ref", "workspace_index"]
            .iter()
            .any(|key| params.contains_key(*key))
        {
            return self;
        }
        params.insert("workspace_id".to_string(), serde_json::json!(workspace_id));
        self
    }

    pub fn with_window_id(mut self, window_id: Option<&str>) -> Self {
        if !self.method.starts_with("workspace.group.")
            && !matches!(
                self.method.as_str(),
                "surface.read_text"
                    | "surface.clear_history"
                    | "surface.trigger_flash"
                    | "notification.clear"
                    | "notification.create"
                    | "window.current"
                    | "window.display"
                    | "right_sidebar"
                    | "workspace.list"
                    | "workspace.current"
                    | "workspace.create"
                    | "workspace.close"
                    | "workspace.select"
                    | "workspace.rename"
                    | "workspace.action"
                    | "tab.action"
                    | "surface.respawn"
            )
        {
            return self;
        }
        let Some(window_id) = window_id.map(str::trim) else {
            return self;
        };
        let Some(params) = self.params.as_object_mut() else {
            return self;
        };
        if window_id.is_empty() {
            if matches!(self.method.as_str(), "tab.action" | "surface.respawn") {
                params.insert("suppress_ambient_window".into(), serde_json::json!(true));
            }
            return self;
        }
        if params.contains_key("suppress_ambient_window") {
            return self;
        }
        if ["window_id", "window_ref", "window_index"]
            .iter()
            .any(|key| params.contains_key(*key))
        {
            return self;
        }
        apply_window_selector_value(window_id, params);
        self
    }

    pub fn with_ambient_surface_id(mut self, surface_id: Option<&str>) -> Self {
        let Some(surface_id) = surface_id.map(str::trim) else {
            return self;
        };
        if !self.method.starts_with("workspace.group.")
            && !matches!(
                self.method.as_str(),
                "workspace.list"
                    | "workspace.current"
                    | "workspace.create"
                    | "workspace.rename"
                    | "tab.action"
                    | "surface.respawn"
            )
        {
            return self;
        }
        let Some(params) = self.params.as_object_mut() else {
            return self;
        };
        if params.contains_key("suppress_ambient_surface")
            || params.contains_key("suppress_ambient_workspace")
            || params.contains_key("suppress_ambient_window")
        {
            return self;
        }
        if surface_id.is_empty() && matches!(self.method.as_str(), "tab.action" | "surface.respawn")
        {
            params.insert("suppress_ambient_surface".into(), serde_json::json!(true));
            return self;
        }
        let has_explicit_scope = [
            "window_id",
            "window_ref",
            "window_index",
            "workspace_id",
            "workspace_ref",
            "workspace_index",
            "surface_id",
            "surface_ref",
            "surface_index",
        ]
        .iter()
        .any(|key| params.contains_key(*key));
        if !has_explicit_scope {
            params
                .entry("surface_id")
                .or_insert_with(|| serde_json::json!(surface_id));
        }
        self
    }
}

fn workspace_scoped_method(method: &str) -> bool {
    method.starts_with("workspace.group.")
        || matches!(
            method,
            "workspace.current"
                | "workspace.list"
                | "workspace.create"
                | "workspace.close"
                | "workspace.rename"
                | "workspace.action"
                | "workspace.equalize_splits"
                | "workspace.set_description"
                | "workspace.reset_color"
                | "workspace.set_progress"
                | "workspace.clear_progress"
                | "workspace.set_status"
                | "workspace.clear_status"
                | "workspace.list_status"
                | "workspace.set_agent_pid"
                | "workspace.clear_agent_pid"
                | "workspace.set_agent_lifecycle"
                | "workspace.report_pr"
                | "workspace.report_review"
                | "workspace.clear_pr"
                | "workspace.report_meta"
                | "workspace.clear_meta"
                | "workspace.list_meta"
                | "workspace.report_meta_block"
                | "workspace.clear_meta_block"
                | "workspace.list_meta_blocks"
                | "workspace.reset_sidebar"
                | "workspace.log"
                | "workspace.clear_log"
                | "workspace.list_log"
                | "workspace.sidebar_state"
                | "workspace.set_unread"
                | "workspace.set_pinned"
                | "pane.focus"
                | "pane.list"
                | "pane.surfaces"
                | "surface.list"
                | "surface.split"
                | "surface.new_terminal_tab"
                | "surface.new_tab"
                | "surface.split_browser"
                | "surface.close"
                | "surface.set_type"
                | "surface.set_kind"
                | "surface.rename"
                | "surface.set_title"
                | "surface.set_pinned"
                | "surface.set_unread"
                | "surface.report_ports"
                | "surface.set_ports"
                | "report_ports"
                | "surface.report_tty"
                | "report_tty"
                | "surface.report_shell_state"
                | "report_shell_state"
                | "surface.clear_ports"
                | "clear_ports"
                | "surface.ports_kick"
                | "ports_kick"
                | "surface.focus"
                | "surface.health"
                | "surface.read_text"
                | "surface.clear_history"
                | "surface.trigger_flash"
                | "notification.clear"
                | "notification.create"
                | "right_sidebar"
                | "debug.terminals"
                | "surface.send_text"
                | "surface.send_key"
                | "surface.open_browser"
                | "surface.open_markdown"
                | "surface.open_file"
                | "surface.open_diff"
                | "surface.next"
                | "surface.previous"
                | "surface.toggle_split_zoom"
                | "browser.back"
                | "browser.forward"
                | "browser.reload"
                | "browser.url.get"
                | "browser.focus_webview"
                | "browser.is_webview_focused"
                | "browser.clear_history"
                | "browser.toggle_omnibar"
                | "browser.toggle_focus_mode"
                | "tab.action"
                | "surface.respawn"
                | "browser.toggle_developer_tools"
                | "browser.show_developer_tools"
                | "browser.network.requests"
                | "browser.set_zoom"
        )
}

pub fn control_command_for(
    command: &str,
    args: &[String],
) -> Result<Option<ControlCommand>, CliError> {
    let mapped = match command {
        "ping" => Some(ControlCommand::new("ping", serde_json::json!({}))),
        "capabilities" => Some(ControlCommand::new(
            "system.capabilities",
            serde_json::json!({}),
        )),
        "agent-hibernation" => Some(agent_hibernation_command(args)?),
        "set-agent-lifecycle" | "set_agent_lifecycle" => Some(set_agent_lifecycle_command(args)?),
        "config"
            if args
                .first()
                .is_some_and(|argument| argument.eq_ignore_ascii_case("reload")) =>
        {
            Some(ControlCommand::new(
                "config.reload",
                config_reload_alias_params(args)?,
            ))
        }
        "reload-config" => Some(ControlCommand::new(
            "config.reload",
            reload_config_params(args)?,
        )),
        "refresh-surfaces" => Some(ControlCommand::new(
            "surface.refresh_all",
            serde_json::json!({}),
        )),
        "identify" => Some(ControlCommand::new(
            "system.identify",
            serde_json::json!({}),
        )),
        "list-windows" => Some(ControlCommand::new("window.list", serde_json::json!({}))),
        "current-window" => Some(ControlCommand::new("window.current", serde_json::json!({}))),
        "window" => Some(window_command(args)?),
        "list-notifications" => Some(ControlCommand::new(
            "notification.list",
            serde_json::json!({}),
        )),
        "dismiss-notification" => Some(ControlCommand::new(
            "notification.dismiss",
            dismiss_notification_params(args)?,
        )),
        "mark-notification-read" => Some(ControlCommand::new(
            "notification.mark_read",
            mark_notification_read_params(args)?,
        )),
        "clear-notifications" => Some(ControlCommand::new(
            "notification.clear",
            clear_notifications_params(args)?,
        )),
        "open-notification" => Some(ControlCommand::new(
            "notification.open",
            open_notification_params(args)?,
        )),
        "jump-to-unread" => Some(ControlCommand::new(
            "notification.jump_to_unread",
            serde_json::json!({}),
        )),
        "notify" => Some(ControlCommand::new(
            "notification.create",
            notification_create_params(args)?,
        )),
        "right-sidebar" => Some(ControlCommand::new(
            "right_sidebar",
            right_sidebar_params(args)?,
        )),
        "sidebar-snapshot" | "extension-sidebar-snapshot" => Some(ControlCommand::new(
            "extension.sidebar.snapshot",
            serde_json::json!({}),
        )),
        "sidebar" => Some(sidebar_command(args)?),
        "list-workspaces" => Some(ControlCommand::new(
            "workspace.list",
            legacy_workspace_scope_params(args, "list-workspaces")?,
        )),
        "current-workspace" => Some(ControlCommand::new(
            "workspace.current",
            legacy_workspace_scope_params(args, "current-workspace")?,
        )),
        "new-workspace" => Some(ControlCommand::new(
            "workspace.create",
            legacy_workspace_create_params(args)?,
        )),
        "new-browser-workspace" => Some(ControlCommand::new(
            "workspace.create_browser",
            browser_workspace_params(args)?,
        )),
        "reopen-closed-browser-tab" => Some(ControlCommand::new(
            "browser.reopen_closed",
            serde_json::json!({}),
        )),
        "restore-previous-launch" => Some(ControlCommand::new(
            "session.restore_previous_launch",
            serde_json::json!({}),
        )),
        "restore-session" => Some(ControlCommand::new(
            "session.restore_previous",
            restore_session_params(args)?,
        )),
        "close-workspace" => Some(ControlCommand::new(
            "workspace.close",
            workspace_close_params(args, "close-workspace")?,
        )),
        "close-workspaces" => Some(ControlCommand::new(
            "workspace.close_many",
            workspace_multi_selector_params(args)?,
        )),
        "reorder-workspace" => Some(ControlCommand::new(
            "workspace.reorder",
            canonical_workspace_reorder_params(args)?,
        )),
        "reorder-workspaces" => Some(ControlCommand::new(
            "workspace.reorder_many",
            canonical_workspaces_reorder_params(args)?,
        )),
        "move-workspace-to-window" => Some(ControlCommand::new(
            "workspace.move_to_window",
            move_workspace_to_window_params(args)?,
        )),
        "move-surface" => Some(ControlCommand::new(
            "surface.move",
            canonical_surface_move_params(args)?,
        )),
        "split-off" | "drag-surface-to-split" => Some(ControlCommand::new(
            if command == "split-off" {
                "surface.split_off"
            } else {
                "surface.drag_to_split"
            },
            split_off_params(args, command)?,
        )),
        "swap-pane" => Some(ControlCommand::new("pane.swap", swap_pane_params(args)?)),
        "break-pane" => Some(ControlCommand::new("pane.break", break_pane_params(args)?)),
        "join-pane" => Some(ControlCommand::new("pane.join", join_pane_params(args)?)),
        "last-pane" => Some(ControlCommand::new(
            "pane.last",
            workspace_window_scope_params(args)?,
        )),
        "last-window" => Some(ControlCommand::new(
            "workspace.last",
            window_scope_params(args)?,
        )),
        "resize-pane" => Some(ControlCommand::new(
            "pane.resize",
            resize_pane_params(args)?,
        )),
        "reorder-surface" => Some(ControlCommand::new(
            "surface.reorder",
            canonical_surface_reorder_params(args)?,
        )),
        "select-workspace" => Some(ControlCommand::new(
            "workspace.select",
            legacy_workspace_target_params(args, "select-workspace")?,
        )),
        "rename-workspace" | "rename-window" => Some(ControlCommand::new(
            "workspace.rename",
            legacy_workspace_rename_params(args, command)?,
        )),
        "set-progress" => Some(ControlCommand::new(
            "workspace.set_progress",
            workspace_progress_params(args)?,
        )),
        "clear-progress" => Some(ControlCommand::new(
            "workspace.clear_progress",
            workspace_selector_params(args)?,
        )),
        "set-status" => Some(ControlCommand::new(
            "workspace.set_status",
            workspace_status_params(args)?,
        )),
        "clear-status" => Some(ControlCommand::new(
            "workspace.clear_status",
            workspace_status_key_params(args, "clear-status")?,
        )),
        "list-status" => Some(ControlCommand::new(
            "workspace.list_status",
            workspace_selector_params(args)?,
        )),
        "set-agent-pid" | "set_agent_pid" => Some(ControlCommand::new(
            "workspace.set_agent_pid",
            workspace_agent_pid_params(args, "set-agent-pid")?,
        )),
        "clear-agent-pid" | "clear_agent_pid" => Some(ControlCommand::new(
            "workspace.clear_agent_pid",
            workspace_status_key_params(args, "clear-agent-pid")?,
        )),
        "report-pr" | "report_pr" => Some(ControlCommand::new(
            "workspace.report_pr",
            workspace_pr_params(args, "report-pr")?,
        )),
        "report-review" | "report_review" => Some(ControlCommand::new(
            "workspace.report_review",
            workspace_pr_params(args, "report-review")?,
        )),
        "clear-pr" | "clear_pr" => Some(ControlCommand::new(
            "workspace.clear_pr",
            surface_selector_params(args)?,
        )),
        "report-meta" | "set-meta" => Some(ControlCommand::new(
            "workspace.report_meta",
            workspace_meta_params(args, "report-meta")?,
        )),
        "clear-meta" => Some(ControlCommand::new(
            "workspace.clear_meta",
            workspace_status_key_params(args, "clear-meta")?,
        )),
        "list-meta" => Some(ControlCommand::new(
            "workspace.list_meta",
            workspace_selector_params(args)?,
        )),
        "report-meta-block" | "set-meta-block" => Some(ControlCommand::new(
            "workspace.report_meta_block",
            workspace_meta_block_params(args, "report-meta-block")?,
        )),
        "clear-meta-block" => Some(ControlCommand::new(
            "workspace.clear_meta_block",
            workspace_status_key_params(args, "clear-meta-block")?,
        )),
        "list-meta-blocks" => Some(ControlCommand::new(
            "workspace.list_meta_blocks",
            workspace_selector_params(args)?,
        )),
        "reset-sidebar" => Some(ControlCommand::new(
            "workspace.reset_sidebar",
            workspace_selector_params(args)?,
        )),
        "log" => Some(ControlCommand::new(
            "workspace.log",
            workspace_log_params(args)?,
        )),
        "clear-log" => Some(ControlCommand::new(
            "workspace.clear_log",
            workspace_selector_params(args)?,
        )),
        "list-log" => Some(ControlCommand::new(
            "workspace.list_log",
            workspace_list_log_params(args)?,
        )),
        "sidebar-state" => Some(ControlCommand::new(
            "workspace.sidebar_state",
            workspace_selector_params(args)?,
        )),
        "report-tty" | "report_tty" => Some(ControlCommand::new(
            "surface.report_tty",
            surface_tty_params(args)?,
        )),
        "report-shell-state" | "report_shell_state" => Some(ControlCommand::new(
            "surface.report_shell_state",
            surface_shell_state_params(args)?,
        )),
        "rename-tab" => Some(ControlCommand::new(
            "surface.rename",
            surface_title_params(args)?,
        )),
        "tab-action" => Some(ControlCommand::new("tab.action", tab_action_params(args)?)),
        "workspace-action" => Some(ControlCommand::new(
            "workspace.action",
            workspace_action_params(args)?,
        )),
        "workspace-group" => Some(workspace_group_subcommand(args)?),
        "respawn-pane" => Some(ControlCommand::new(
            "surface.respawn",
            respawn_pane_params(args)?,
        )),
        "move-tab-to-new-workspace" => Some(ControlCommand::new(
            "surface.move_to_new_workspace",
            surface_selector_params(args)?,
        )),
        "workspace" => workspace_subcommand(args)?,
        "list-panes" => Some(ControlCommand::new(
            "pane.list",
            workspace_window_scope_params(args)?,
        )),
        "list-pane-surfaces" => Some(ControlCommand::new(
            "pane.surfaces",
            pane_workspace_window_scope_params(args)?,
        )),
        "list-panels" => Some(ControlCommand::new(
            "surface.list",
            workspace_window_scope_params(args)?,
        )),
        "new-split" => Some(ControlCommand::new(
            "surface.split",
            surface_split_params(args)?,
        )),
        "new-pane" => Some(ControlCommand::new(
            "surface.split",
            surface_split_params(args)?,
        )),
        "new-surface" => Some(ControlCommand::new(
            "surface.new_terminal_tab",
            surface_terminal_tab_params(args)?,
        )),
        "new-terminal-tab" => Some(ControlCommand::new(
            "surface.new_terminal_tab",
            surface_terminal_tab_params(args)?,
        )),
        "split-browser" => Some(ControlCommand::new(
            "surface.split_browser",
            surface_split_browser_params(args)?,
        )),
        "close-surface" => Some(ControlCommand::new(
            "surface.close",
            surface_selector_params(args)?,
        )),
        "focus-pane" => Some(ControlCommand::new("pane.focus", pane_focus_params(args)?)),
        "focus-panel" => Some(ControlCommand::new(
            "surface.focus",
            surface_focus_params(args)?,
        )),
        "surface-health" => Some(ControlCommand::new(
            "surface.health",
            surface_workspace_params(args)?,
        )),
        "read-screen" | "capture-pane" => Some(ControlCommand::new(
            "surface.read_text",
            surface_read_text_params(args, command)?,
        )),
        "clear-history" => Some(ControlCommand::new(
            "surface.clear_history",
            surface_selector_params(args)?,
        )),
        "trigger-flash" => Some(ControlCommand::new(
            "surface.trigger_flash",
            surface_selector_params(args)?,
        )),
        "debug-terminals" => Some(ControlCommand::new(
            "debug.terminals",
            workspace_selector_params(args)?,
        )),
        "send" => Some(ControlCommand::new(
            "surface.send_text",
            surface_send_text_params(args, false)?,
        )),
        "send-panel" => Some(ControlCommand::new(
            "surface.send_text",
            surface_send_text_params(args, true)?,
        )),
        "send-key" => Some(ControlCommand::new(
            "surface.send_key",
            surface_send_key_params(args, false)?,
        )),
        "send-key-panel" => Some(ControlCommand::new(
            "surface.send_key",
            surface_send_key_params(args, true)?,
        )),
        "open-browser" => Some(ControlCommand::new(
            "surface.open_browser",
            surface_open_browser_params(args)?,
        )),
        "navigate" => Some(ControlCommand::new(
            "surface.open_browser",
            surface_open_browser_params(args)?,
        )),
        "browser-back" => Some(ControlCommand::new(
            "browser.back",
            surface_selector_params(args)?,
        )),
        "browser-forward" => Some(ControlCommand::new(
            "browser.forward",
            surface_selector_params(args)?,
        )),
        "browser-reload" => Some(ControlCommand::new(
            "browser.reload",
            surface_selector_params(args)?,
        )),
        "get-url" => Some(ControlCommand::new(
            "browser.url.get",
            surface_selector_params(args)?,
        )),
        "focus-webview" => Some(ControlCommand::new(
            "browser.focus_webview",
            surface_selector_params(args)?,
        )),
        "is-webview-focused" => Some(ControlCommand::new(
            "browser.is_webview_focused",
            surface_selector_params(args)?,
        )),
        "browser" => browser_subcommand(args)?,
        "markdown" => Some(ControlCommand::new(
            "surface.open_markdown",
            surface_open_markdown_params(args)?,
        )),
        "diff" => Some(ControlCommand::new(
            "surface.open_diff",
            surface_open_diff_params(args)?,
        )),
        "surface" => surface_subcommand(args)?,
        _ => None,
    };
    Ok(mapped)
}

fn agent_hibernation_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let [value] = args else {
        return Err(CliError::new(
            "Usage: cmux agent-hibernation <on|off> [--json]",
        ));
    };
    let enabled = match value.to_ascii_lowercase().as_str() {
        "on" | "enable" => true,
        "off" | "disable" => false,
        _ => {
            return Err(CliError::new(
                "Usage: cmux agent-hibernation <on|off> [--json]",
            ));
        }
    };
    Ok(ControlCommand::new(
        "agent_hibernation",
        serde_json::json!({"enabled": enabled}),
    ))
}

fn set_agent_lifecycle_command(args: &[String]) -> Result<ControlCommand, CliError> {
    const USAGE: &str = "cmux set-agent-lifecycle <key> <unknown|running|idle|needsInput> [--tab=<id>] [--panel=<id>]";
    let parsed = ParsedArgs::parse(args)?;
    if parsed.positionals.len() < 2 {
        return Err(CliError::new(format!("Usage: {USAGE}")));
    }
    let raw_lifecycle = &parsed.positionals[1];
    let lifecycle = normalize_agent_lifecycle(raw_lifecycle).ok_or_else(|| {
        CliError::new(format!(
            "Invalid agent lifecycle '{raw_lifecycle}' — usage: {USAGE}"
        ))
    })?;
    let mut params = serde_json::Map::new();
    params.insert("key".to_string(), serde_json::json!(&parsed.positionals[0]));
    params.insert("lifecycle".to_string(), serde_json::json!(lifecycle));
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    Ok(ControlCommand::new(
        "workspace.set_agent_lifecycle",
        serde_json::Value::Object(params),
    ))
}

fn normalize_agent_lifecycle(raw: &str) -> Option<&'static str> {
    let normalized = raw.trim().to_ascii_lowercase().replace('_', "-");
    match normalized.as_str() {
        "unknown" => Some("unknown"),
        "running" => Some("running"),
        "idle" => Some("idle"),
        "needsinput" | "needs-input" => Some("needsInput"),
        _ => None,
    }
}

fn workspace_subcommand(args: &[String]) -> Result<Option<ControlCommand>, CliError> {
    let Some((subcommand, rest)) = split_subcommand(args) else {
        return Ok(Some(ControlCommand::new(
            "workspace.current",
            serde_json::json!({}),
        )));
    };
    let mapped = match subcommand.as_str() {
        "list" | "ls" => ControlCommand::new("workspace.list", serde_json::json!({})),
        "current" | "show" => ControlCommand::new("workspace.current", serde_json::json!({})),
        "new" | "create" => ControlCommand::new("workspace.create", workspace_create_params(rest)?),
        "new-browser" | "create-browser" | "browser" => {
            ControlCommand::new("workspace.create_browser", browser_workspace_params(rest)?)
        }
        "close" => ControlCommand::new(
            "workspace.close",
            workspace_close_params(rest, "workspace close")?,
        ),
        "close-many" | "close-workspaces" => ControlCommand::new(
            "workspace.close_many",
            workspace_multi_selector_params(rest)?,
        ),
        "select" | "focus" => {
            ControlCommand::new("workspace.select", workspace_selector_params(rest)?)
        }
        "reorder" | "move" => {
            ControlCommand::new("workspace.reorder", workspace_reorder_params(rest)?)
        }
        "next" => ControlCommand::new("workspace.next", serde_json::json!({})),
        "previous" | "prev" => ControlCommand::new("workspace.previous", serde_json::json!({})),
        "rename" => ControlCommand::new(
            "workspace.rename",
            workspace_rename_params(rest, "workspace rename")?,
        ),
        "description" | "set-description" => ControlCommand::new(
            "workspace.set_description",
            workspace_description_params(rest)?,
        ),
        "set-progress" | "progress" => {
            ControlCommand::new("workspace.set_progress", workspace_progress_params(rest)?)
        }
        "clear-progress" => {
            ControlCommand::new("workspace.clear_progress", workspace_selector_params(rest)?)
        }
        "set-status" | "status" => {
            ControlCommand::new("workspace.set_status", workspace_status_params(rest)?)
        }
        "clear-status" => ControlCommand::new(
            "workspace.clear_status",
            workspace_status_key_params(rest, "workspace clear-status")?,
        ),
        "list-status" => {
            ControlCommand::new("workspace.list_status", workspace_selector_params(rest)?)
        }
        "set-agent-pid" | "agent-pid" => ControlCommand::new(
            "workspace.set_agent_pid",
            workspace_agent_pid_params(rest, "workspace set-agent-pid")?,
        ),
        "clear-agent-pid" => ControlCommand::new(
            "workspace.clear_agent_pid",
            workspace_status_key_params(rest, "workspace clear-agent-pid")?,
        ),
        "report-pr" | "pr" => ControlCommand::new(
            "workspace.report_pr",
            workspace_pr_params(rest, "workspace report-pr")?,
        ),
        "report-review" | "review" => ControlCommand::new(
            "workspace.report_review",
            workspace_pr_params(rest, "workspace report-review")?,
        ),
        "clear-pr" => ControlCommand::new("workspace.clear_pr", surface_selector_params(rest)?),
        "report-meta" | "set-meta" | "meta" => ControlCommand::new(
            "workspace.report_meta",
            workspace_meta_params(rest, "workspace report-meta")?,
        ),
        "clear-meta" => ControlCommand::new(
            "workspace.clear_meta",
            workspace_status_key_params(rest, "workspace clear-meta")?,
        ),
        "list-meta" => ControlCommand::new("workspace.list_meta", workspace_selector_params(rest)?),
        "report-meta-block" | "set-meta-block" | "meta-block" => ControlCommand::new(
            "workspace.report_meta_block",
            workspace_meta_block_params(rest, "workspace report-meta-block")?,
        ),
        "clear-meta-block" => ControlCommand::new(
            "workspace.clear_meta_block",
            workspace_status_key_params(rest, "workspace clear-meta-block")?,
        ),
        "list-meta-blocks" => ControlCommand::new(
            "workspace.list_meta_blocks",
            workspace_selector_params(rest)?,
        ),
        "reset-sidebar" => {
            ControlCommand::new("workspace.reset_sidebar", workspace_selector_params(rest)?)
        }
        "log" => ControlCommand::new("workspace.log", workspace_log_params(rest)?),
        "clear-log" => ControlCommand::new("workspace.clear_log", workspace_selector_params(rest)?),
        "list-log" => ControlCommand::new("workspace.list_log", workspace_list_log_params(rest)?),
        "sidebar-state" => {
            ControlCommand::new("workspace.sidebar_state", workspace_selector_params(rest)?)
        }
        "reset-color" => {
            ControlCommand::new("workspace.reset_color", workspace_selector_params(rest)?)
        }
        "pin" => ControlCommand::new(
            "workspace.set_pinned",
            workspace_bool_params(rest, "pinned", true)?,
        ),
        "unpin" => ControlCommand::new(
            "workspace.set_pinned",
            workspace_bool_params(rest, "pinned", false)?,
        ),
        "mark-unread" => ControlCommand::new(
            "workspace.set_unread",
            workspace_bool_params(rest, "unread", true)?,
        ),
        "mark-read" => ControlCommand::new(
            "workspace.set_unread",
            workspace_bool_params(rest, "unread", false)?,
        ),
        "group" => workspace_group_subcommand(rest)?,
        "restore-previous-launch" | "restore-previous" | "restore-session" => {
            ControlCommand::new("session.restore_previous_launch", serde_json::json!({}))
        }
        "equalize-splits" | "equalize" => {
            ControlCommand::new("workspace.equalize_splits", serde_json::json!({}))
        }
        _ => return Ok(None),
    };
    Ok(Some(mapped))
}

fn workspace_group_subcommand(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_subcommand(args) else {
        return Err(CliError::new(
            "workspace-group requires a subcommand. Try: list, create, ungroup, delete, rename, collapse, expand, pin, unpin, add, remove, set-anchor, new-workspace, set-color, set-icon, move, focus",
        ));
    };
    let parsed = ParsedArgs::parse(rest)?;
    let mut params = serde_json::Map::new();
    apply_window_scope_selector(&parsed, &mut params);

    let method = match subcommand.as_str() {
        "list" => "workspace.group.list",
        "create" => {
            let name = parsed
                .value(&["--name"])
                .cloned()
                .or_else(|| parsed.first_positional().map(str::to_owned))
                .unwrap_or_default();
            params.insert("name".into(), serde_json::json!(name));
            if let Some(cwd) = parsed.value(&["--cwd"]) {
                params.insert(
                    "cwd".into(),
                    serde_json::json!(resolve_workspace_group_path(cwd)),
                );
            }
            if let Some(from) = parsed.value(&["--from"]) {
                // Swift String.split omits empty subsequences before trimming.
                let child_workspace_ids: Vec<String> = from
                    .split(',')
                    .filter(|part| !part.is_empty())
                    .map(|part| part.trim().to_owned())
                    .collect();
                params.insert(
                    "child_workspace_ids".into(),
                    serde_json::json!(child_workspace_ids),
                );
            }
            "workspace.group.create"
        }
        "ungroup" | "delete" | "collapse" | "expand" | "pin" | "unpin" | "focus" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            match subcommand.as_str() {
                "ungroup" => "workspace.group.ungroup",
                "delete" => "workspace.group.delete",
                "collapse" => "workspace.group.collapse",
                "expand" => "workspace.group.expand",
                "pin" => "workspace.group.pin",
                "unpin" => "workspace.group.unpin",
                "focus" => "workspace.group.focus",
                _ => unreachable!(),
            }
        }
        "rename" => {
            let group_id = workspace_group_id(&parsed, &subcommand)?;
            let name = parsed.value(&["--name"]).cloned().or_else(|| {
                parsed
                    .positionals
                    .iter()
                    .find(|candidate| candidate.as_str() != group_id)
                    .cloned()
            });
            let name = name.ok_or_else(|| CliError::new("rename requires --name <name>"))?;
            params.insert("group_id".into(), serde_json::json!(group_id));
            params.insert("name".into(), serde_json::json!(name));
            "workspace.group.rename"
        }
        "add" => {
            let Some(group_id) = parsed.value(&["--group"]) else {
                return Err(CliError::new("add requires --group <id> --workspace <id>"));
            };
            let Some(workspace_id) = parsed.value(&["--workspace"]) else {
                return Err(CliError::new("add requires --group <id> --workspace <id>"));
            };
            params.insert("group_id".into(), serde_json::json!(group_id));
            params.insert("workspace_id".into(), serde_json::json!(workspace_id));
            "workspace.group.add"
        }
        "remove" => {
            let workspace_id = parsed
                .value(&["--workspace"])
                .cloned()
                .or_else(|| parsed.first_positional().map(str::to_owned))
                .ok_or_else(|| CliError::new("remove requires --workspace <id>"))?;
            params.insert("workspace_id".into(), serde_json::json!(workspace_id));
            "workspace.group.remove"
        }
        "set-anchor" => {
            let Some(group_id) = parsed.value(&["--group"]) else {
                return Err(CliError::new(
                    "set-anchor requires --group <id> --workspace <id>",
                ));
            };
            let Some(workspace_id) = parsed.value(&["--workspace"]) else {
                return Err(CliError::new(
                    "set-anchor requires --group <id> --workspace <id>",
                ));
            };
            params.insert("group_id".into(), serde_json::json!(group_id));
            params.insert("workspace_id".into(), serde_json::json!(workspace_id));
            "workspace.group.set_anchor"
        }
        "new-workspace" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            if let Some(placement) = parsed.value(&["--placement"]) {
                params.insert("placement".into(), serde_json::json!(placement));
            }
            "workspace.group.new_workspace"
        }
        "set-color" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            params.insert(
                "hex".into(),
                serde_json::json!(parsed.value(&["--hex"]).cloned().unwrap_or_default()),
            );
            "workspace.group.set_color"
        }
        "set-icon" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            params.insert(
                "symbol".into(),
                serde_json::json!(parsed.value(&["--symbol"]).cloned().unwrap_or_default()),
            );
            "workspace.group.set_icon"
        }
        "move" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            if let Some(to_index) = parsed.value(&["--to-index"]) {
                let to_index = to_index
                    .parse::<i64>()
                    .map_err(|_| CliError::new("move --to-index must be an integer"))?;
                params.insert("to_index".into(), serde_json::json!(to_index));
            } else if let Some(before) = parsed.value(&["--before"]) {
                params.insert("before_group_id".into(), serde_json::json!(before));
            } else if let Some(after) = parsed.value(&["--after"]) {
                params.insert("after_group_id".into(), serde_json::json!(after));
            } else {
                return Err(CliError::new(
                    "move requires --to-index <n>, --before <group>, or --after <group>",
                ));
            }
            "workspace.group.move"
        }
        _ => {
            return Err(CliError::new(format!(
                "Unknown workspace-group subcommand: {subcommand}"
            )))
        }
    };
    Ok(ControlCommand::new(
        method,
        serde_json::Value::Object(params),
    ))
}

fn workspace_group_id(parsed: &ParsedArgs, subcommand: &str) -> Result<String, CliError> {
    parsed
        .value(&["--group"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_owned))
        .ok_or_else(|| {
            CliError::new(format!(
                "workspace-group {subcommand} requires a group id or --group <id>"
            ))
        })
}

fn resolve_workspace_group_path(raw: &str) -> String {
    let expanded = if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .map(|home| home.join(raw.trim_start_matches('~').trim_start_matches(['/', '\\'])))
            .unwrap_or_else(|| PathBuf::from(raw))
    } else {
        PathBuf::from(raw)
    };
    let resolved = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir().unwrap_or_default().join(expanded)
    };
    resolved.to_string_lossy().into_owned()
}

fn surface_subcommand(args: &[String]) -> Result<Option<ControlCommand>, CliError> {
    let Some((subcommand, rest)) = split_subcommand(args) else {
        return Ok(Some(ControlCommand::new(
            "surface.list",
            serde_json::json!({}),
        )));
    };
    let mapped = match subcommand.as_str() {
        "list" | "ls" => ControlCommand::new("surface.list", serde_json::json!({})),
        "split" => ControlCommand::new("surface.split", surface_split_params(rest)?),
        "new-tab" | "new-terminal-tab" | "tab" => ControlCommand::new(
            "surface.new_terminal_tab",
            surface_terminal_tab_params(rest)?,
        ),
        "close" => ControlCommand::new("surface.close", surface_selector_params(rest)?),
        "rename" => ControlCommand::new("surface.rename", surface_title_params(rest)?),
        "pin" => ControlCommand::new(
            "surface.set_pinned",
            surface_bool_params(rest, "pinned", true)?,
        ),
        "unpin" => ControlCommand::new(
            "surface.set_pinned",
            surface_bool_params(rest, "pinned", false)?,
        ),
        "mark-unread" => ControlCommand::new(
            "surface.set_unread",
            surface_bool_params(rest, "unread", true)?,
        ),
        "mark-read" => ControlCommand::new(
            "surface.set_unread",
            surface_bool_params(rest, "unread", false)?,
        ),
        "report-tty" | "report_tty" => {
            ControlCommand::new("surface.report_tty", surface_tty_params(rest)?)
        }
        "report-shell-state" | "report_shell_state" | "shell-state" => ControlCommand::new(
            "surface.report_shell_state",
            surface_shell_state_params(rest)?,
        ),
        "move-to-new-workspace" => ControlCommand::new(
            "surface.move_to_new_workspace",
            surface_selector_params(rest)?,
        ),
        "terminal" => ControlCommand::new(
            "surface.set_type",
            surface_set_type_params(rest, Some("terminal"))?,
        ),
        "agent" => ControlCommand::new(
            "surface.set_type",
            surface_set_type_params(rest, Some("agent"))?,
        ),
        "browser" => {
            ControlCommand::new("surface.open_browser", surface_open_browser_params(rest)?)
        }
        "split-browser" | "browser-split" => {
            ControlCommand::new("surface.split_browser", surface_split_browser_params(rest)?)
        }
        "markdown" => {
            ControlCommand::new("surface.open_markdown", surface_open_markdown_params(rest)?)
        }
        "diff" => ControlCommand::new("surface.open_diff", surface_open_diff_params(rest)?),
        "next" => ControlCommand::new("surface.next", surface_selector_params(rest)?),
        "previous" | "prev" => {
            ControlCommand::new("surface.previous", surface_selector_params(rest)?)
        }
        "toggle-zoom" | "zoom" => {
            ControlCommand::new("surface.toggle_split_zoom", surface_selector_params(rest)?)
        }
        "set-type" | "set-kind" => {
            ControlCommand::new("surface.set_type", surface_set_type_params(rest, None)?)
        }
        _ => return Ok(None),
    };
    Ok(Some(mapped))
}

fn browser_subcommand(args: &[String]) -> Result<Option<ControlCommand>, CliError> {
    let Some((subcommand, rest_args)) = split_browser_subcommand(args) else {
        return Ok(Some(ControlCommand::new(
            "surface.open_browser",
            serde_json::json!({}),
        )));
    };
    let rest = rest_args.as_slice();
    let mapped = match subcommand.as_str() {
        "open" => ControlCommand::new("surface.open_browser", surface_open_browser_params(rest)?),
        "navigate" | "goto" => {
            ControlCommand::new("browser.navigate", browser_navigate_params(rest)?)
        }
        "split" => {
            ControlCommand::new("surface.split_browser", surface_split_browser_params(rest)?)
        }
        "open-split" | "open_split" => {
            ControlCommand::new("browser.open_split", surface_split_browser_params(rest)?)
        }
        "split-right" => ControlCommand::new(
            "surface.split_browser",
            surface_split_browser_params_with_direction(rest, "right")?,
        ),
        "split-down" => ControlCommand::new(
            "surface.split_browser",
            surface_split_browser_params_with_direction(rest, "down")?,
        ),
        "new-workspace" | "workspace" => {
            ControlCommand::new("workspace.create_browser", browser_workspace_params(rest)?)
        }
        "reopen-closed" | "reopen-closed-tab" | "reopen" => {
            ControlCommand::new("browser.reopen_closed", serde_json::json!({}))
        }
        "back" => ControlCommand::new("browser.back", surface_selector_params(rest)?),
        "forward" => ControlCommand::new("browser.forward", surface_selector_params(rest)?),
        "clear-history" => {
            ControlCommand::new("browser.clear_history", surface_selector_params(rest)?)
        }
        "toggle-omnibar" | "omnibar" => {
            ControlCommand::new("browser.toggle_omnibar", surface_selector_params(rest)?)
        }
        "toggle-focus-mode" | "focus-mode" => {
            ControlCommand::new("browser.toggle_focus_mode", surface_selector_params(rest)?)
        }
        "toggle-devtools" | "toggle-developer-tools" | "devtools" => ControlCommand::new(
            "browser.toggle_developer_tools",
            surface_selector_params(rest)?,
        ),
        "show-devtools" | "show-developer-tools" => ControlCommand::new(
            "browser.show_developer_tools",
            browser_show_developer_tools_params(rest)?,
        ),
        "network" if rest.first().map(String::as_str) == Some("clear") => ControlCommand::new(
            "browser.network.clear",
            browser_network_clear_params(&rest[1..])?,
        ),
        "network" | "network-requests" | "requests" => {
            let rest = browser_network_requests_rest(&subcommand, rest);
            ControlCommand::new(
                "browser.network.requests",
                browser_network_requests_params(&rest)?,
            )
        }
        "network-clear" | "clear-network" | "clear-requests" => {
            ControlCommand::new("browser.network.clear", browser_network_clear_params(rest)?)
        }
        "reload" => ControlCommand::new("browser.reload", surface_selector_params(rest)?),
        "url" | "url.get" | "get-url" | "current-url" => {
            ControlCommand::new("browser.url.get", surface_selector_params(rest)?)
        }
        "snapshot" => ControlCommand::new("browser.snapshot", browser_surface_only_params(rest)?),
        "eval" => ControlCommand::new("browser.eval", browser_eval_params(rest)?),
        "wait" => ControlCommand::new("browser.wait", browser_wait_params(rest)?),
        "click" => ControlCommand::new("browser.click", browser_selector_action_params(rest)?),
        "dblclick" | "double-click" => {
            ControlCommand::new("browser.dblclick", browser_selector_action_params(rest)?)
        }
        "hover" => ControlCommand::new("browser.hover", browser_selector_action_params(rest)?),
        "focus" => ControlCommand::new("browser.focus", browser_selector_action_params(rest)?),
        "type" => ControlCommand::new("browser.type", browser_text_action_params(rest)?),
        "fill" => ControlCommand::new("browser.fill", browser_text_action_params(rest)?),
        "press" | "key" => ControlCommand::new("browser.press", browser_key_action_params(rest)?),
        "keydown" | "key-down" => {
            ControlCommand::new("browser.keydown", browser_key_action_params(rest)?)
        }
        "keyup" | "key-up" => {
            ControlCommand::new("browser.keyup", browser_key_action_params(rest)?)
        }
        "check" => ControlCommand::new("browser.check", browser_selector_action_params(rest)?),
        "uncheck" => ControlCommand::new("browser.uncheck", browser_selector_action_params(rest)?),
        "select" => ControlCommand::new("browser.select", browser_select_action_params(rest)?),
        "scroll" => ControlCommand::new("browser.scroll", browser_scroll_params(rest)?),
        "scrollintoview" | "scrollinto" | "scroll-into-view" => ControlCommand::new(
            "browser.scroll_into_view",
            browser_selector_action_params(rest)?,
        ),
        "screenshot" => ControlCommand::new("browser.screenshot", browser_screenshot_params(rest)?),
        "get" => browser_get_command(rest)?,
        "is" => browser_is_command(rest)?,
        "find" => browser_find_command(rest)?,
        "frame" => browser_frame_command(rest)?,
        "dialog" => browser_dialog_command(rest)?,
        "download" => browser_download_command(rest)?,
        "cookies" => browser_cookies_command(rest)?,
        "storage" => browser_storage_command(rest)?,
        "tab" => browser_tab_command(rest)?,
        "console" => browser_console_command(rest)?,
        "errors" => browser_errors_command(rest)?,
        "highlight" => {
            ControlCommand::new("browser.highlight", browser_selector_action_params(rest)?)
        }
        "state" => browser_state_command(rest)?,
        "addinitscript" | "add-init-script" => ControlCommand::new(
            "browser.addinitscript",
            browser_script_params(rest, "script")?,
        ),
        "addscript" | "add-script" => {
            ControlCommand::new("browser.addscript", browser_script_params(rest, "script")?)
        }
        "addstyle" | "add-style" => {
            ControlCommand::new("browser.addstyle", browser_script_params(rest, "css")?)
        }
        "focus-webview" => {
            ControlCommand::new("browser.focus_webview", surface_selector_params(rest)?)
        }
        "is-webview-focused" | "webview-focused" | "is-focused" => {
            ControlCommand::new("browser.is_webview_focused", surface_selector_params(rest)?)
        }
        "viewport" | "set-viewport" => {
            ControlCommand::new("browser.viewport.set", browser_viewport_set_params(rest)?)
        }
        "geo" | "geolocation" | "set-geolocation" => ControlCommand::new(
            "browser.geolocation.set",
            browser_geolocation_set_params(rest)?,
        ),
        "offline" | "set-offline" => {
            ControlCommand::new("browser.offline.set", browser_offline_set_params(rest)?)
        }
        "trace" => browser_trace_command(rest)?,
        "screencast" => browser_screencast_command(rest)?,
        "input" => browser_input_command(rest)?,
        "zoom" | "set-zoom" => {
            ControlCommand::new("browser.set_zoom", browser_set_zoom_params(rest)?)
        }
        _ => return Ok(None),
    };
    Ok(Some(mapped))
}

fn split_browser_subcommand(args: &[String]) -> Option<(String, Vec<String>)> {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            return None;
        }
        if arg.starts_with("--") {
            index += if arg.contains('=') || !takes_value(arg) {
                1
            } else {
                2
            };
            continue;
        }
        if !is_browser_subcommand(arg)
            && args
                .get(index + 1)
                .is_some_and(|next| is_browser_subcommand(next))
        {
            let mut rest = Vec::with_capacity(args.len().saturating_sub(1));
            rest.extend(args[..index].iter().cloned());
            rest.push("--surface".to_string());
            rest.push(arg.clone());
            rest.extend(args[index + 2..].iter().cloned());
            return Some((args[index + 1].to_ascii_lowercase(), rest));
        }
        let mut rest = Vec::with_capacity(args.len().saturating_sub(1));
        rest.extend(args[..index].iter().cloned());
        rest.extend(args[index + 1..].iter().cloned());
        return Some((arg.to_ascii_lowercase(), rest));
    }
    None
}

fn is_browser_subcommand(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "open"
            | "navigate"
            | "goto"
            | "split"
            | "open-split"
            | "open_split"
            | "split-right"
            | "split-down"
            | "new-workspace"
            | "workspace"
            | "reopen-closed"
            | "reopen-closed-tab"
            | "reopen"
            | "back"
            | "forward"
            | "clear-history"
            | "toggle-omnibar"
            | "omnibar"
            | "toggle-focus-mode"
            | "focus-mode"
            | "toggle-devtools"
            | "toggle-developer-tools"
            | "devtools"
            | "show-devtools"
            | "show-developer-tools"
            | "network"
            | "network-requests"
            | "requests"
            | "network-clear"
            | "clear-network"
            | "clear-requests"
            | "reload"
            | "url"
            | "url.get"
            | "get-url"
            | "current-url"
            | "snapshot"
            | "eval"
            | "wait"
            | "click"
            | "dblclick"
            | "double-click"
            | "hover"
            | "focus"
            | "type"
            | "fill"
            | "press"
            | "key"
            | "keydown"
            | "key-down"
            | "keyup"
            | "key-up"
            | "check"
            | "uncheck"
            | "select"
            | "scroll"
            | "scrollintoview"
            | "scrollinto"
            | "scroll-into-view"
            | "screenshot"
            | "get"
            | "is"
            | "find"
            | "frame"
            | "dialog"
            | "download"
            | "cookies"
            | "storage"
            | "tab"
            | "console"
            | "errors"
            | "highlight"
            | "state"
            | "addinitscript"
            | "add-init-script"
            | "addscript"
            | "add-script"
            | "addstyle"
            | "add-style"
            | "focus-webview"
            | "is-webview-focused"
            | "webview-focused"
            | "is-focused"
            | "viewport"
            | "set-viewport"
            | "geo"
            | "geolocation"
            | "set-geolocation"
            | "offline"
            | "set-offline"
            | "trace"
            | "screencast"
            | "input"
            | "zoom"
            | "set-zoom"
    )
}

fn browser_network_requests_rest(subcommand: &str, rest: &[String]) -> Vec<String> {
    if subcommand != "network" || rest.first().map(String::as_str) != Some("requests") {
        return rest.to_vec();
    }
    rest[1..].to_vec()
}

fn split_subcommand(args: &[String]) -> Option<(String, &[String])> {
    let first = args.first()?;
    if first.starts_with('-') {
        return None;
    }
    Some((first.to_ascii_lowercase(), &args[1..]))
}

fn sidebar_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_subcommand(args) else {
        return Ok(ControlCommand::new("sidebar.list", serde_json::json!({})));
    };
    let (method, requires_name) = match subcommand.as_str() {
        "list" => ("sidebar.list", false),
        "validate" => ("sidebar.validate", false),
        "reload" => ("sidebar.reload", false),
        "select" => ("sidebar.select", true),
        "open" => ("sidebar.open", true),
        other => {
            return Err(CliError::new(format!(
                "unknown sidebar subcommand '{other}'; expected list, validate, reload, select, or open"
            )));
        }
    };
    Ok(ControlCommand::new(
        method,
        sidebar_name_params(rest, requires_name, &format!("sidebar {subcommand}"))?,
    ))
}

fn sidebar_name_params(
    args: &[String],
    requires_name: bool,
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let name = parsed
        .value(&["--name"])
        .map(String::as_str)
        .or_else(|| parsed.first_positional())
        .map(str::trim)
        .filter(|name| !name.is_empty());
    if requires_name && name.is_none() {
        return Err(CliError::new(format!(
            "{command_label} requires a sidebar name"
        )));
    }
    let mut params = serde_json::Map::new();
    if let Some(name) = name {
        params.insert("name".to_string(), serde_json::json!(name));
    }
    Ok(serde_json::Value::Object(params))
}

fn legacy_workspace_create_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    validate_legacy_flags(
        "new-workspace",
        &parsed,
        &[
            "--name",
            "--description",
            "--cwd",
            "--command",
            "--env",
            "--env-file",
            "--layout",
            "--window",
            "--focus",
            "--group",
            "--group-placement",
            "--group-reference",
        ],
    )?;
    let mut params = serde_json::Map::new();
    if let Some(cwd) = parsed.value(&["--cwd"]) {
        params.insert(
            "working_directory".to_string(),
            serde_json::json!(expand_workspace_path(cwd)?),
        );
    }
    if let Some(title) = parsed.value(&["--name"]) {
        params.insert("title".to_string(), serde_json::json!(title));
    }
    if let Some(description) = parsed.value(&["--description"]) {
        params.insert("description".to_string(), serde_json::json!(description));
    }
    if let Some(command) = parsed.value(&["--command"]) {
        params.insert(
            "__post_create_command".to_string(),
            serde_json::json!(command),
        );
    }
    let mut environment = BTreeMap::new();
    for path in parsed.values(&["--env-file"]) {
        let contents = std::fs::read_to_string(path)
            .map_err(|error| CliError::new(format!("failed to read env file {path}: {error}")))?;
        for raw_line in contents
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let line = raw_line
                .strip_prefix("export ")
                .map(str::trim)
                .unwrap_or(raw_line);
            let (key, value) = parse_environment_assignment(line, &format!("--env-file '{path}'"))?;
            environment.insert(key, unquote_env_file_value(&value));
        }
    }
    for assignment in parsed.values(&["--env"]) {
        let (key, value) = parse_environment_assignment(assignment, "--env")?;
        environment.insert(key, value);
    }
    if !environment.is_empty() {
        params.insert("workspace_env".to_string(), serde_json::json!(environment));
    }
    if let Some(layout) = parsed.value(&["--layout"]) {
        let value: serde_json::Value = serde_json::from_str(layout)
            .map_err(|_| CliError::new("--layout must be a JSON object"))?;
        if !value.is_object() {
            return Err(CliError::new("--layout must be a JSON object"));
        }
        params.insert("layout".to_string(), value);
    }
    if let Some(focus) = parsed.value(&["--focus"]) {
        let focus = match focus.as_str() {
            "true" => true,
            "false" => false,
            _ => return Err(CliError::new("--focus must be true or false")),
        };
        params.insert("focus".to_string(), serde_json::json!(focus));
    }
    if let Some(group) = parsed.value(&["--group"]) {
        params.insert("group_id".into(), serde_json::json!(group));
    }
    if let Some(placement) = parsed.value(&["--group-placement"]) {
        params.insert("group_placement".into(), serde_json::json!(placement));
    }
    if let Some(reference) = parsed.value(&["--group-reference"]) {
        params.insert(
            "group_reference_workspace_id".into(),
            serde_json::json!(reference),
        );
    }
    apply_legacy_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn workspace_create_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    if let Some(cwd) = parsed.value(&["--cwd", "--path", "--directory"]) {
        params.insert("current_directory".to_string(), serde_json::json!(cwd));
    }
    if let Some(command) = parsed.value(&["--command"]) {
        params.insert(
            "initial_terminal_command".to_string(),
            serde_json::json!(command),
        );
    }
    if let Some(input) = parsed.value(&["--input"]) {
        params.insert(
            "initial_terminal_input".to_string(),
            serde_json::json!(input),
        );
    }
    if let Some(environment) = terminal_environment_param(&parsed)? {
        params.insert(
            "initial_terminal_environment".to_string(),
            serde_json::json!(environment),
        );
    }
    Ok(serde_json::Value::Object(params))
}

fn legacy_workspace_scope_params(
    args: &[String],
    command: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    validate_legacy_flags(command, &parsed, &["--window"])?;
    let mut params = serde_json::Map::new();
    apply_legacy_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn workspace_selector_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector(&parsed, &mut params)?;
    Ok(serde_json::Value::Object(params))
}

fn workspace_close_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if command_label == "close-workspace" {
        validate_legacy_flags(command_label, &parsed, &["--workspace", "--window"])?;
        if parsed.value(&["--workspace"]).is_none() {
            return Err(CliError::new("close-workspace requires --workspace"));
        }
    } else if !has_explicit_workspace_selector(&parsed) && parsed.first_positional().is_none() {
        return Err(CliError::new(format!(
            "{command_label} requires a workspace target (workspace:N or workspace id)"
        )));
    }
    let mut params = serde_json::Map::new();
    if command_label == "close-workspace" {
        apply_legacy_workspace_selector(&parsed, &mut params);
        apply_legacy_window_scope_selector(&parsed, &mut params);
    } else {
        apply_workspace_selector(&parsed, &mut params)?;
    }
    Ok(serde_json::Value::Object(params))
}

fn workspace_multi_selector_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if parsed.positionals.is_empty() {
        return Err(CliError::new(
            "close-workspaces requires at least one workspace",
        ));
    }
    let mut refs = Vec::new();
    let mut ids = Vec::new();
    for selector in &parsed.positionals {
        if selector.chars().all(|ch| ch.is_ascii_digit()) {
            refs.push(format!("workspace:{selector}"));
        } else if selector.starts_with("workspace:") {
            refs.push(selector.clone());
        } else {
            ids.push(selector.clone());
        }
    }
    let mut params = serde_json::Map::new();
    if !refs.is_empty() {
        params.insert("workspace_refs".to_string(), serde_json::json!(refs));
    }
    if !ids.is_empty() {
        params.insert("workspace_ids".to_string(), serde_json::json!(ids));
    }
    Ok(serde_json::Value::Object(params))
}

fn workspace_reorder_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if parsed.value(&["--to-index", "--target-index"]).is_some() {
        return Err(CliError::new(
            "workspace reorder requires a before/after workspace ref or id; --to-index is not supported",
        ));
    }
    let mut params = serde_json::Map::new();

    let from = parsed
        .value(&["--from", "--workspace", "--workspace-id", "--id"])
        .cloned()
        .or_else(|| parsed.positionals.first().cloned())
        .ok_or_else(|| CliError::new("workspace reorder requires a workspace"))?;
    if from.chars().all(|ch| ch.is_ascii_digit()) {
        params.insert(
            "workspace_ref".to_string(),
            serde_json::json!(format!("workspace:{from}")),
        );
    } else if from.starts_with("workspace:") {
        params.insert("workspace_ref".to_string(), serde_json::json!(from));
    } else {
        params.insert("workspace_id".to_string(), serde_json::json!(from));
    }

    if let Some(after) = parsed.value(&["--after", "--after-workspace"]) {
        apply_workspace_target_selector(after, "after_workspace", &mut params);
    } else {
        let to = parsed
            .value(&["--to", "--before", "--before-workspace"])
            .cloned()
            .or_else(|| parsed.positionals.get(1).cloned())
            .ok_or_else(|| CliError::new("workspace reorder requires a destination"))?;
        apply_workspace_target_selector(&to, "before_workspace", &mut params);
    }
    if parsed.has_flag("--top-level-rows") {
        params.insert("uses_top_level_rows".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

fn canonical_workspace_reorder_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let workspace = parsed
        .value(&["--workspace"])
        .cloned()
        .or_else(|| parsed.positionals.first().cloned())
        .ok_or_else(|| CliError::new("reorder-workspace requires --workspace <id|ref|index>"))?;
    let mut params = serde_json::Map::new();
    apply_workspace_target_selector(&workspace, "workspace", &mut params);

    if let Some(before) = parsed.value(&["--before", "--before-workspace"]) {
        apply_workspace_target_selector(before, "before_workspace", &mut params);
    }
    if let Some(after) = parsed.value(&["--after", "--after-workspace"]) {
        apply_workspace_target_selector(after, "after_workspace", &mut params);
    }
    if let Some(index) = parsed.value(&["--index"]) {
        let index = index
            .parse::<i64>()
            .map_err(|_| CliError::new("--index must be an integer"))?;
        params.insert("index".to_string(), serde_json::json!(index));
    }
    apply_window_scope_selector(&parsed, &mut params);
    if parsed.has_flag("--dry-run") {
        params.insert("dry_run".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

fn canonical_workspaces_reorder_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let order = parsed.value(&["--order"]).ok_or_else(|| {
        CliError::new("reorder-workspaces requires --order <id|ref|index>,<id|ref|index>,...")
    })?;
    if order.is_empty() {
        return Err(CliError::new(
            "reorder-workspaces requires at least one workspace in --order",
        ));
    }
    let workspace_ids: Vec<String> = order
        .split(',')
        .map(str::trim)
        .map(str::to_string)
        .collect();
    if workspace_ids.iter().any(String::is_empty) {
        return Err(CliError::new(
            "reorder-workspaces --order cannot contain empty workspace refs",
        ));
    }
    let workspace_ids: Vec<String> = workspace_ids
        .into_iter()
        .map(|workspace| {
            if workspace
                .chars()
                .all(|character| character.is_ascii_digit())
            {
                format!("workspace:{workspace}")
            } else {
                workspace
            }
        })
        .collect();
    let mut params = serde_json::Map::new();
    params.insert(
        "workspace_ids".to_string(),
        serde_json::json!(workspace_ids),
    );
    apply_window_scope_selector(&parsed, &mut params);
    if parsed.has_flag("--dry-run") {
        params.insert("dry_run".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

fn canonical_surface_reorder_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let surface = parsed
        .value(&["--surface"])
        .cloned()
        .or_else(|| parsed.positionals.first().cloned())
        .ok_or_else(|| CliError::new("reorder-surface requires --surface <id|ref|index>"))?;
    let mut params = serde_json::Map::new();
    apply_surface_target_selector(&surface, "surface", &mut params);
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    if let Some(before) = parsed.value(&["--before", "--before-surface"]) {
        apply_surface_target_selector(before, "before_surface", &mut params);
    }
    if let Some(after) = parsed.value(&["--after", "--after-surface"]) {
        apply_surface_target_selector(after, "after_surface", &mut params);
    }
    if let Some(index) = parsed.value(&["--index"]) {
        let index = index
            .parse::<i64>()
            .map_err(|_| CliError::new("--index must be an integer"))?;
        params.insert("index".to_string(), serde_json::json!(index));
    }
    let focus_value = parsed
        .value(&["--focus"])
        .map(|value| value.to_ascii_lowercase());
    let focus = match focus_value.as_deref() {
        None => false,
        Some("1" | "true" | "yes" | "on") => true,
        Some("0" | "false" | "no" | "off") => false,
        Some(_) => return Err(CliError::new("--focus must be true|false")),
    };
    params.insert("focus".to_string(), serde_json::json!(focus));
    Ok(serde_json::Value::Object(params))
}

fn canonical_surface_move_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let surface = parsed
        .value(&["--surface"])
        .cloned()
        .or_else(|| parsed.positionals.first().cloned())
        .ok_or_else(|| CliError::new("move-surface requires --surface <id|ref|index>"))?;
    let mut params = serde_json::Map::new();
    apply_surface_target_selector(&surface, "surface", &mut params);
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    if let Some(pane) = parsed.value(&["--pane"]) {
        apply_pane_target_selector(pane, &mut params);
    }
    if let Some(before) = parsed.value(&["--before", "--before-surface"]) {
        apply_surface_target_selector(before, "before_surface", &mut params);
    }
    if let Some(after) = parsed.value(&["--after", "--after-surface"]) {
        apply_surface_target_selector(after, "after_surface", &mut params);
    }
    if let Some(index) = parsed.value(&["--index"]) {
        let index = index
            .parse::<i64>()
            .map_err(|_| CliError::new("--index must be an integer"))?;
        params.insert("index".to_string(), serde_json::json!(index));
    }
    if let Some(value) = parsed.value(&["--focus"]) {
        let focus = match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => return Err(CliError::new("--focus must be true|false")),
        };
        params.insert("focus".to_string(), serde_json::json!(focus));
    }
    Ok(serde_json::Value::Object(params))
}

fn move_workspace_to_window_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let workspace = parsed
        .value(&["--workspace"])
        .ok_or_else(|| CliError::new("move-workspace-to-window requires --workspace"))?;
    let window = parsed
        .value(&["--window"])
        .ok_or_else(|| CliError::new("move-workspace-to-window requires --window"))?;
    let mut params = serde_json::Map::new();
    apply_workspace_target_selector(workspace, "workspace", &mut params);
    apply_window_selector_value(window, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn split_off_params(args: &[String], command_name: &str) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let surface = parsed.value(&["--surface", "--panel"]).ok_or_else(|| {
        CliError::new(format!("{command_name} requires --surface <id|ref|index>"))
    })?;
    let direction = parsed
        .positionals
        .first()
        .filter(|direction| !direction.starts_with("--"))
        .ok_or_else(|| CliError::new(format!("{command_name} requires a direction")))?;
    if !matches!(
        direction.to_ascii_lowercase().as_str(),
        "left" | "right" | "up" | "down" | "l" | "r" | "u" | "d"
    ) {
        return Err(CliError::new(format!(
            "{command_name}: direction must be left|right|up|down"
        )));
    }
    if let Some(unknown) = parsed.flags.first() {
        return Err(CliError::new(format!(
            "{command_name}: unknown flag '{unknown}'"
        )));
    }
    let mut params = serde_json::Map::new();
    apply_surface_target_selector(surface, "surface", &mut params);
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    params.insert("direction".to_string(), serde_json::json!(direction));
    let focus = match parsed
        .value(&["--focus"])
        .map(|value| value.to_ascii_lowercase())
    {
        None => false,
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(value) if matches!(value.as_str(), "0" | "false" | "no" | "off") => false,
        Some(_) => return Err(CliError::new("--focus must be true|false")),
    };
    params.insert("focus".to_string(), serde_json::json!(focus));
    Ok(serde_json::Value::Object(params))
}

fn swap_pane_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let pane = parsed
        .value(&["--pane"])
        .ok_or_else(|| CliError::new("swap-pane requires --pane"))?;
    let target_pane = parsed
        .value(&["--target-pane"])
        .ok_or_else(|| CliError::new("swap-pane requires --target-pane"))?;
    let mut params = serde_json::Map::new();
    apply_named_pane_target_selector(pane, "pane", &mut params);
    apply_named_pane_target_selector(target_pane, "target_pane", &mut params);
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    let focus = match parsed
        .value(&["--focus"])
        .map(|value| value.to_ascii_lowercase())
    {
        None => false,
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(value) if matches!(value.as_str(), "0" | "false" | "no" | "off") => false,
        Some(_) => return Err(CliError::new("--focus must be true|false")),
    };
    params.insert("focus".to_string(), serde_json::json!(focus));
    Ok(serde_json::Value::Object(params))
}

fn break_pane_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    pane_transfer_params(args, false)
}

fn join_pane_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    pane_transfer_params(args, true)
}

fn pane_transfer_params(
    args: &[String],
    require_target_pane: bool,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if parsed.value(&["--focus"]).is_some() && parsed.has_flag("--no-focus") {
        return Err(CliError::new(
            "--focus and --no-focus cannot be used together",
        ));
    }
    let mut params = serde_json::Map::new();
    if require_target_pane {
        let target_pane = parsed
            .value(&["--target-pane"])
            .ok_or_else(|| CliError::new("join-pane requires --target-pane"))?;
        apply_named_pane_target_selector(target_pane, "target_pane", &mut params);
    }
    if let Some(pane) = parsed.value(&["--pane"]) {
        apply_pane_target_selector(pane, &mut params);
    }
    if let Some(surface) = parsed.value(&["--surface"]) {
        apply_surface_target_selector(surface, "surface", &mut params);
    }
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    let focus = match parsed
        .value(&["--focus"])
        .map(|value| value.to_ascii_lowercase())
    {
        None => false,
        Some(value) if matches!(value.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(value) if matches!(value.as_str(), "0" | "false" | "no" | "off") => false,
        Some(_) => return Err(CliError::new("--focus must be true|false")),
    };
    params.insert("focus".to_string(), serde_json::json!(focus));
    Ok(serde_json::Value::Object(params))
}

fn workspace_window_scope_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn window_scope_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn pane_workspace_window_scope_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    if let Some(pane) = parsed.value(&["--pane"]) {
        apply_pane_target_selector(pane, &mut params);
    }
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn resize_pane_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let amount = parsed
        .value(&["--amount"])
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(1);
    if amount <= 0 {
        return Err(CliError::new("--amount must be greater than 0"));
    }
    let direction = if args.iter().any(|arg| arg == "-L") {
        "left"
    } else if args.iter().any(|arg| arg == "-R") {
        "right"
    } else if args.iter().any(|arg| arg == "-U") {
        "up"
    } else if args.iter().any(|arg| arg == "-D") {
        "down"
    } else {
        "right"
    };
    let mut params = serde_json::Map::new();
    params.insert("direction".to_string(), serde_json::json!(direction));
    params.insert("amount".to_string(), serde_json::json!(amount));
    if let Some(pane) = parsed.value(&["--pane"]) {
        apply_pane_target_selector(pane, &mut params);
    }
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn legacy_workspace_rename_params(
    args: &[String],
    command_name: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if command_name == "rename-workspace" || command_name == "rename-window" {
        validate_legacy_flags(
            command_name,
            &parsed,
            &["--workspace", "--window", "--title", "--name"],
        )?;
    }
    let mut params = serde_json::Map::new();
    if parsed.value(&["--workspace"]).is_some() {
        apply_legacy_workspace_selector(&parsed, &mut params);
    } else {
        params.insert("resolve_current_workspace".into(), serde_json::json!(true));
    }
    apply_legacy_window_scope_selector(&parsed, &mut params);
    let title = parsed
        .value(&["--title", "--name"])
        .cloned()
        .or_else(|| {
            let title = parsed.positionals.join(" ");
            (!title.trim().is_empty()).then_some(title)
        })
        .ok_or_else(|| CliError::new(format!("{command_name} requires a title")))?;
    if title.trim().is_empty() {
        return Err(CliError::new(format!("{command_name} requires a title")));
    }
    params.insert("title".to_string(), serde_json::json!(title));
    Ok(serde_json::Value::Object(params))
}

fn workspace_rename_params(
    args: &[String],
    command_name: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_free_text(&parsed, &mut params)?;
    let title = parsed
        .value(&["--title", "--name"])
        .cloned()
        .or_else(|| workspace_free_text_from_positionals(&parsed))
        .ok_or_else(|| CliError::new(format!("{command_name} requires a title")))?;
    params.insert("title".to_string(), serde_json::json!(title));
    Ok(serde_json::Value::Object(params))
}

fn legacy_workspace_target_params(
    args: &[String],
    command: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    validate_legacy_flags(command, &parsed, &["--workspace", "--window"])?;
    if parsed.value(&["--workspace"]).is_none() {
        return Err(CliError::new(format!("{command} requires --workspace")));
    }
    let mut params = serde_json::Map::new();
    apply_legacy_workspace_selector(&parsed, &mut params);
    apply_legacy_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn apply_legacy_workspace_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    let Some(value) = parsed.value(&["--workspace"]) else {
        return;
    };
    if let Ok(index) = value.parse::<usize>() {
        params.insert("workspace_index".into(), serde_json::json!(index));
    } else {
        apply_workspace_selector_value(value, params);
    }
}

fn apply_legacy_window_scope_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    let Some(value) = parsed.value(&["--window"]) else {
        return;
    };
    if let Ok(index) = value.parse::<usize>() {
        params.insert("window_index".into(), serde_json::json!(index));
    } else {
        apply_window_selector_value(value, params);
    }
}

fn apply_workspace_selector_value(
    value: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.starts_with("workspace:") {
        params.insert("workspace_ref".into(), serde_json::json!(value));
    } else {
        params.insert("workspace_id".into(), serde_json::json!(value));
    }
}

fn parse_environment_assignment(
    assignment: &str,
    source: &str,
) -> Result<(String, String), CliError> {
    let (key, value) = assignment.split_once('=').ok_or_else(|| {
        CliError::new(format!(
            "new-workspace: {source} entry '{assignment}' must be in KEY=VALUE form"
        ))
    })?;
    if key.trim().is_empty() {
        return Err(CliError::new(format!(
            "new-workspace: {source} entry '{assignment}' has an empty key"
        )));
    }
    Ok((key.trim().to_string(), value.to_string()))
}

fn unquote_env_file_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn expand_workspace_path(raw: &str) -> Result<String, CliError> {
    let expanded = if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| {
                CliError::new("new-workspace: unable to expand '~' without a home directory")
            })?;
        let suffix = raw.trim_start_matches('~').trim_start_matches(['/', '\\']);
        PathBuf::from(home).join(suffix)
    } else {
        PathBuf::from(raw)
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|error| {
                CliError::new(format!("new-workspace: failed to resolve --cwd: {error}"))
            })?
            .join(expanded)
    };
    let normalized = normalize_path(&absolute).to_string_lossy().into_owned();
    Ok(normalized
        .strip_prefix(r"\\?\")
        .unwrap_or(&normalized)
        .to_string())
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn validate_legacy_flags(
    command: &str,
    parsed: &ParsedArgs,
    allowed: &[&str],
) -> Result<(), CliError> {
    if let Some(flag) = parsed
        .flags
        .iter()
        .find(|flag| !allowed.contains(&flag.as_str()))
    {
        return Err(CliError::new(format!("{command}: unknown flag '{flag}'")));
    }
    if let Some((flag, _)) = parsed
        .values
        .iter()
        .find(|(flag, _)| !allowed.contains(&flag.as_str()))
    {
        return Err(CliError::new(format!("{command}: unknown flag '{flag}'")));
    }
    Ok(())
}

fn restore_session_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    if let Some(unknown) = args.iter().find(|arg| arg.as_str() != "--") {
        return Err(CliError::new(format!(
            "restore-session: unknown flag '{unknown}'"
        )));
    }
    Ok(serde_json::json!({}))
}

fn reload_config_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    if let Some(unexpected) = args.first() {
        return Err(CliError::new(format!(
            "reload-config does not accept arguments. Unexpected argument '{unexpected}'"
        )));
    }
    Ok(serde_json::json!({}))
}

fn config_reload_alias_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    if args.len() != 1 {
        return Err(CliError::new("Usage: cmux config reload"));
    }
    Ok(serde_json::json!({}))
}

fn window_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some(subcommand) = args.first().map(|argument| argument.to_lowercase()) else {
        return Err(CliError::new(
            "window requires a subcommand. Try: display, displays, default-display",
        ));
    };
    match subcommand.as_str() {
        "displays" => Ok(ControlCommand::new(
            "window.displays",
            serde_json::json!({}),
        )),
        "display"
            if args[1..]
                .iter()
                .any(|argument| matches!(argument.as_str(), "--list" | "-l")) =>
        {
            Ok(ControlCommand::new(
                "window.displays",
                serde_json::json!({}),
            ))
        }
        "display" => {
            let display = args[1..]
                .iter()
                .find(|argument| !argument.starts_with('-'))
                .filter(|argument| !argument.is_empty())
                .ok_or_else(|| CliError::new("window display requires a display name. Usage: cmux window display \"LG HDR 4K\"  (list names with: cmux window displays)"))?;
            Ok(ControlCommand::new(
                "window.display",
                serde_json::json!({"display": display}),
            ))
        }
        _ => Err(CliError::new(format!(
            "Unknown window subcommand: {subcommand}. Try: display, displays"
        ))),
    }
}

fn dismiss_notification_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let id = parsed.value(&["--id"]);
    let all_read = parsed.has_flag("--all-read");
    if id.is_some() == all_read {
        return Err(CliError::new(
            "dismiss-notification requires exactly one of --id or --all-read",
        ));
    }
    Ok(match id {
        Some(id) => serde_json::json!({"id": id}),
        None => serde_json::json!({"all_read": true}),
    })
}

fn mark_notification_read_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let id = parsed.value(&["--id"]);
    let workspace = parsed.value(&["--workspace", "--workspace-id", "--workspace-ref"]);
    let surface = parsed.value(&["--surface", "--surface-id", "--surface-ref"]);
    let all = parsed.has_flag("--all");
    if surface.is_some() && workspace.is_none() {
        return Err(CliError::new("--surface requires --workspace"));
    }
    if usize::from(id.is_some()) + usize::from(workspace.is_some()) + usize::from(all) != 1 {
        return Err(CliError::new(
            "mark-notification-read requires exactly one selector: --id, --workspace, or --all",
        ));
    }
    if let Some(id) = id {
        return Ok(serde_json::json!({"id": id}));
    }
    if all {
        return Ok(serde_json::json!({"all": true}));
    }
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn clear_notifications_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn open_notification_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let id = parsed
        .value(&["--id"])
        .ok_or_else(|| CliError::new("open-notification requires --id"))?;
    Ok(serde_json::json!({"id": id}))
}

fn notification_create_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    params.insert(
        "title".to_string(),
        serde_json::json!(parsed
            .value(&["--title"])
            .map(String::as_str)
            .unwrap_or("Notification")),
    );
    params.insert(
        "subtitle".to_string(),
        serde_json::json!(parsed
            .value(&["--subtitle"])
            .map(String::as_str)
            .unwrap_or("")),
    );
    params.insert(
        "body".to_string(),
        serde_json::json!(parsed.value(&["--body"]).map(String::as_str).unwrap_or("")),
    );
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn right_sidebar_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    for (index, arg) in args.iter().enumerate() {
        if arg == "--workspace" && args.get(index + 1).is_none() {
            return Err(CliError::new("right-sidebar: --workspace requires an id"));
        }
        if arg == "--window" && args.get(index + 1).is_none() {
            return Err(CliError::new("right-sidebar: --window requires an id"));
        }
    }
    for arg in args.iter().filter(|arg| arg.starts_with("--")) {
        let name = arg.split_once('=').map_or(arg.as_str(), |(name, _)| name);
        let valid = matches!(name, "--workspace" | "--window")
            || (name == "--no-focus" && arg == "--no-focus");
        if !valid {
            return Err(CliError::new(format!(
                "right-sidebar: unknown flag '{arg}'"
            )));
        }
    }

    let parsed = ParsedArgs::parse(args)?;
    let Some(action) = parsed.first_positional().map(str::to_ascii_lowercase) else {
        return Err(CliError::new("right-sidebar requires a subcommand"));
    };
    let no_focus = parsed.has_flag("--no-focus");
    let mut params = serde_json::Map::new();

    match action.as_str() {
        "toggle" | "show" | "hide" | "focus" | "mode" => {
            if parsed.positionals.len() != 1 {
                return Err(CliError::new(format!(
                    "right-sidebar {action} received unexpected arguments"
                )));
            }
            if no_focus {
                return Err(CliError::new(
                    "right-sidebar: --no-focus is only valid with set",
                ));
            }
            params.insert("action".to_string(), serde_json::json!(action));
        }
        "set" => {
            if parsed.positionals.len() != 2 {
                return Err(CliError::new(
                    "right-sidebar set requires a mode: files, find, vault, sessions, feed, or dock",
                ));
            }
            let raw_mode = &parsed.positionals[1];
            let mode = canonical_right_sidebar_mode(raw_mode)
                .ok_or_else(|| CliError::new(format!("Unknown right-sidebar mode '{raw_mode}'")))?;
            params.insert("action".to_string(), serde_json::json!("set"));
            params.insert("mode".to_string(), serde_json::json!(mode));
            params.insert("focus".to_string(), serde_json::json!(!no_focus));
        }
        "files" | "find" | "vault" | "sessions" | "feed" | "dock" => {
            if parsed.positionals.len() != 1 {
                return Err(CliError::new(format!(
                    "right-sidebar {action} received unexpected arguments"
                )));
            }
            if no_focus {
                return Err(CliError::new(
                    "right-sidebar: --no-focus is only valid with set",
                ));
            }
            params.insert("action".to_string(), serde_json::json!("set"));
            params.insert(
                "mode".to_string(),
                serde_json::json!(canonical_right_sidebar_mode(&action).expect("known mode")),
            );
            params.insert("focus".to_string(), serde_json::json!(true));
        }
        _ => {
            return Err(CliError::new(format!(
                "Unknown right-sidebar command '{action}'"
            )));
        }
    }

    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn canonical_right_sidebar_mode(mode: &str) -> Option<&'static str> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "files" => Some("files"),
        "find" => Some("find"),
        "vault" | "sessions" => Some("sessions"),
        "feed" => Some("feed"),
        "dock" => Some("dock"),
        _ => None,
    }
}

fn workspace_description_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_free_text(&parsed, &mut params)?;
    let description = parsed
        .value(&["--description", "--text", "--body"])
        .cloned()
        .or_else(|| workspace_free_text_from_positionals(&parsed))
        .ok_or_else(|| CliError::new("workspace description requires text"))?;
    params.insert("description".to_string(), serde_json::json!(description));
    Ok(serde_json::Value::Object(params))
}

fn workspace_bool_params(
    args: &[String],
    key: &str,
    value: bool,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector(&parsed, &mut params)?;
    params.insert(key.to_string(), serde_json::json!(value));
    Ok(serde_json::Value::Object(params))
}

fn workspace_progress_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_free_text(&parsed, &mut params)?;
    let raw_value = parsed
        .value(&["--value", "--progress"])
        .cloned()
        .or_else(|| workspace_progress_value_from_positionals(&parsed))
        .ok_or_else(|| CliError::new("set-progress requires a progress value"))?;
    let value = raw_value
        .parse::<f64>()
        .map_err(|_| CliError::new("set-progress requires a numeric progress value"))?;
    params.insert("value".to_string(), serde_json::json!(value));
    if let Some(label) = parsed.value(&["--label", "--text"]) {
        params.insert("label".to_string(), serde_json::json!(label));
    }
    Ok(serde_json::Value::Object(params))
}

fn workspace_status_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    let (key, value) = workspace_key_value_from_args(&parsed, "set-status")?;
    params.insert("key".to_string(), serde_json::json!(key));
    params.insert("value".to_string(), serde_json::json!(value));
    if let Some(priority) = parsed.value(&["--priority"]) {
        let priority = priority
            .parse::<i64>()
            .map_err(|_| CliError::new("set-status --priority requires an integer"))?;
        params.insert("priority".to_string(), serde_json::json!(priority));
    }
    Ok(serde_json::Value::Object(params))
}

fn workspace_agent_pid_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    if !params.contains_key("workspace_ref")
        && !params.contains_key("workspace_id")
        && parsed
            .positionals
            .first()
            .is_some_and(|value| looks_like_workspace_selector(value))
        && (parsed.value(&["--key", "--name"]).is_some()
            || parsed
                .value(&["--pid", "--process-id", "--processId"])
                .is_some())
    {
        apply_workspace_selector(&parsed, &mut params)?;
    }
    let key_positionals = if (params.contains_key("workspace_ref")
        || params.contains_key("workspace_id"))
        && parsed
            .positionals
            .first()
            .is_some_and(|value| looks_like_workspace_selector(value))
    {
        parsed.positionals[1..].to_vec()
    } else {
        workspace_key_positionals(&parsed)
    };
    let key = parsed
        .value(&["--key", "--name"])
        .cloned()
        .or_else(|| key_positionals.first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a key")))?;
    let pid_raw = parsed
        .value(&["--pid", "--process-id", "--processId"])
        .cloned()
        .or_else(|| {
            if parsed.value(&["--key", "--name"]).is_some() {
                key_positionals.first().cloned()
            } else {
                key_positionals.get(1).cloned()
            }
        })
        .ok_or_else(|| CliError::new(format!("{command_label} requires a pid")))?;
    let pid = pid_raw
        .parse::<u32>()
        .map_err(|_| CliError::new(format!("{command_label} pid must be a positive integer")))?;
    if pid == 0 {
        return Err(CliError::new(format!(
            "{command_label} pid must be a positive integer"
        )));
    }
    params.insert("key".to_string(), serde_json::json!(key));
    params.insert("pid".to_string(), serde_json::json!(pid));
    Ok(serde_json::Value::Object(params))
}

fn workspace_meta_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    let (key, value) = workspace_key_value_from_args(&parsed, command_label)?;
    params.insert("key".to_string(), serde_json::json!(key));
    params.insert("value".to_string(), serde_json::json!(value));
    if let Some(icon) = parsed.value(&["--icon"]) {
        params.insert("icon".to_string(), serde_json::json!(icon));
    }
    if let Some(color) = parsed.value(&["--color"]) {
        params.insert("color".to_string(), serde_json::json!(color));
    }
    if let Some(url) = parsed.value(&["--url"]) {
        params.insert("url".to_string(), serde_json::json!(url));
    }
    if let Some(format) = parsed.value(&["--format"]) {
        params.insert("format".to_string(), serde_json::json!(format));
    }
    if let Some(priority) = parsed.value(&["--priority"]) {
        let priority = priority.parse::<i64>().map_err(|_| {
            CliError::new(format!("{command_label} --priority requires an integer"))
        })?;
        params.insert("priority".to_string(), serde_json::json!(priority));
    }
    Ok(serde_json::Value::Object(params))
}

fn workspace_meta_block_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    let key = parsed
        .value(&["--key", "--name"])
        .cloned()
        .or_else(|| workspace_key_positionals(&parsed).first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a key")))?;
    let markdown = parsed
        .value(&["--markdown", "--value", "--text"])
        .cloned()
        .or_else(|| {
            let positionals = workspace_key_positionals(&parsed);
            (positionals.len() > 1).then(|| positionals[1..].join(" "))
        })
        .ok_or_else(|| CliError::new(format!("{command_label} requires markdown")))?;
    params.insert("key".to_string(), serde_json::json!(key));
    params.insert("markdown".to_string(), serde_json::json!(markdown));
    if let Some(priority) = parsed.value(&["--priority"]) {
        let priority = priority.parse::<i64>().map_err(|_| {
            CliError::new(format!("{command_label} --priority requires an integer"))
        })?;
        params.insert("priority".to_string(), serde_json::json!(priority));
    }
    Ok(serde_json::Value::Object(params))
}

fn workspace_pr_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;

    let positional_start = if !params.contains_key("workspace_ref")
        && !params.contains_key("workspace_id")
        && parsed
            .positionals
            .first()
            .is_some_and(|value| value.starts_with("workspace:"))
        && parsed.positionals.len() > 2
    {
        params.insert(
            "workspace_ref".to_string(),
            serde_json::json!(parsed.positionals[0]),
        );
        1
    } else {
        0
    };
    let positionals = &parsed.positionals[positional_start..];
    let number = parsed
        .value(&["--number", "--pr"])
        .cloned()
        .or_else(|| positionals.first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a number")))?;
    let number = number
        .parse::<i64>()
        .map_err(|_| CliError::new(format!("{command_label} number must be an integer")))?;
    params.insert("number".to_string(), serde_json::json!(number));

    let url = parsed
        .value(&["--url", "--href"])
        .cloned()
        .or_else(|| positionals.get(1).cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a URL")))?;
    params.insert("url".to_string(), serde_json::json!(url));

    if let Some(label) = parsed.value(&["--label"]) {
        params.insert("label".to_string(), serde_json::json!(label));
    }
    if let Some(state) = parsed.value(&["--state", "--status"]) {
        params.insert("state".to_string(), serde_json::json!(state));
    }
    if let Some(branch) = parsed.value(&["--branch"]) {
        params.insert("branch".to_string(), serde_json::json!(branch));
    }
    if parsed.has_flag("--stale") {
        params.insert("stale".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

fn workspace_status_key_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    let key = parsed
        .value(&["--key", "--name"])
        .cloned()
        .or_else(|| workspace_key_positionals(&parsed).first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a key")))?;
    params.insert("key".to_string(), serde_json::json!(key));
    Ok(serde_json::Value::Object(params))
}

fn workspace_log_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_free_text(&parsed, &mut params)?;
    let message = parsed
        .value(&["--message", "--text"])
        .cloned()
        .or_else(|| workspace_free_text_from_positionals(&parsed))
        .ok_or_else(|| CliError::new("log requires a message"))?;
    params.insert("message".to_string(), serde_json::json!(message));
    if let Some(level) = parsed.value(&["--level"]) {
        params.insert("level".to_string(), serde_json::json!(level));
    }
    Ok(serde_json::Value::Object(params))
}

fn workspace_list_log_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector(&parsed, &mut params)?;
    if let Some(limit) = parsed.value(&["--limit"]) {
        let limit = limit
            .parse::<usize>()
            .map_err(|_| CliError::new("list-log --limit requires a positive integer"))?;
        params.insert("limit".to_string(), serde_json::json!(limit));
    }
    Ok(serde_json::Value::Object(params))
}

fn apply_workspace_selector_for_free_text(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if has_explicit_workspace_selector(parsed) {
        return apply_workspace_selector(parsed, params);
    }
    if parsed
        .positionals
        .first()
        .is_some_and(|value| looks_like_workspace_selector(value))
        && parsed.positionals.len() > 1
    {
        return apply_workspace_selector(parsed, params);
    }
    Ok(())
}

fn workspace_free_text_from_positionals(parsed: &ParsedArgs) -> Option<String> {
    let first = parsed.positionals.first()?;
    let text_start = if looks_like_workspace_selector(first) && parsed.positionals.len() > 1 {
        1
    } else {
        0
    };
    let text = parsed.positionals[text_start..].join(" ");
    (!text.trim().is_empty()).then_some(text)
}

fn workspace_progress_value_from_positionals(parsed: &ParsedArgs) -> Option<String> {
    let first = parsed.positionals.first()?;
    let value_index = if looks_like_workspace_selector(first) && parsed.positionals.len() > 1 {
        1
    } else {
        0
    };
    parsed.positionals.get(value_index).cloned()
}

fn apply_workspace_selector_for_key_value(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if has_explicit_workspace_selector(parsed) {
        return apply_workspace_selector(parsed, params);
    }
    if parsed
        .positionals
        .first()
        .is_some_and(|value| looks_like_workspace_selector(value))
        && parsed.positionals.len() > 2
    {
        return apply_workspace_selector(parsed, params);
    }
    Ok(())
}

fn workspace_key_positionals(parsed: &ParsedArgs) -> Vec<String> {
    let start = if parsed
        .positionals
        .first()
        .is_some_and(|value| looks_like_workspace_selector(value))
        && parsed.positionals.len() > 2
    {
        1
    } else {
        0
    };
    parsed.positionals[start..].to_vec()
}

fn workspace_key_value_from_args(
    parsed: &ParsedArgs,
    command_label: &str,
) -> Result<(String, String), CliError> {
    let key = parsed
        .value(&["--key", "--name"])
        .cloned()
        .or_else(|| workspace_key_positionals(parsed).first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a key")))?;
    let value = parsed
        .value(&["--value", "--status", "--text"])
        .cloned()
        .or_else(|| {
            let positionals = workspace_key_positionals(parsed);
            (positionals.len() > 1).then(|| positionals[1..].join(" "))
        })
        .ok_or_else(|| CliError::new(format!("{command_label} requires a value")))?;
    Ok((key, value))
}

fn surface_selector_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    Ok(serde_json::Value::Object(params))
}

fn tab_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let (workspace, rem0) = parse_frozen_option(args, "--workspace");
    let (tab, rem1) = parse_frozen_option(&rem0, "--tab");
    let (surface, rem2) = parse_frozen_option(&rem1, "--surface");
    let (action_option, rem3) = parse_frozen_option(&rem2, "--action");
    let (title_option, rem4) = parse_frozen_option(&rem3, "--title");
    let (url, rem5) = parse_frozen_option(&rem4, "--url");
    let (focus, rem6) = parse_frozen_option(&rem5, "--focus");
    let (window, mut positional) = parse_frozen_option(&rem6, "--window");
    let action_raw = if let Some(action) = action_option {
        action
    } else if positional.is_empty() {
        return Err(CliError::new("tab-action requires --action <name>"));
    } else {
        positional.remove(0)
    };
    if let Some(unknown) = positional.iter().find(|value| value.starts_with("--")) {
        return Err(CliError::new(format!(
            "tab-action: unknown flag '{unknown}'"
        )));
    }
    let action = normalize_action_name(&action_raw);
    let title = title_option
        .map(|title| title.trim().to_string())
        .or_else(|| join_then_trim(&positional));
    if action == "rename" && title.as_deref().is_none_or(str::is_empty) {
        return Err(CliError::new(
            "tab-action rename requires --title <text> (or a trailing title)",
        ));
    }

    let mut params = lifecycle_scope_values(workspace.as_deref(), window.as_deref())?;
    params.insert("action".into(), serde_json::json!(action));
    if let Some(tab) = tab.as_deref() {
        apply_validated_surface_selector(tab, true, &mut params)?;
    } else if let Some(surface) = surface.as_deref() {
        apply_validated_surface_selector(surface, false, &mut params)?;
    }
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        params.insert("title".into(), serde_json::json!(title));
    }
    if let Some(url) = url.as_deref().map(str::trim).filter(|url| !url.is_empty()) {
        params.insert("url".into(), serde_json::json!(url));
    }
    params.insert(
        "focus".into(),
        serde_json::json!(parse_optional_bool(focus.as_deref(), "--focus")?.unwrap_or(false)),
    );
    Ok(serde_json::Value::Object(params))
}

fn workspace_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let (workspace, rem0) = parse_frozen_option(args, "--workspace");
    let (action_option, rem1) = parse_frozen_option(&rem0, "--action");
    let (title_option, rem2) = parse_frozen_option(&rem1, "--title");
    let (color_option, rem3) = parse_frozen_option(&rem2, "--color");
    let (description_option, rem4) = parse_frozen_option(&rem3, "--description");
    let (window, mut positional) = parse_frozen_option(&rem4, "--window");
    let action_raw = if let Some(action) = action_option {
        action
    } else if positional.is_empty() {
        return Err(CliError::new("workspace-action requires --action <name>"));
    } else {
        positional.remove(0)
    };
    if let Some(unknown) = positional.iter().find(|value| value.starts_with("--")) {
        return Err(CliError::new(format!(
            "workspace-action: unknown flag '{unknown}'"
        )));
    }
    let action = normalize_action_name(&action_raw);
    let inferred_raw = positional.join(" ");
    let inferred = inferred_raw.trim();
    let title = title_option
        .map(|value| value.trim().to_owned())
        .or_else(|| (action == "rename" && !inferred.is_empty()).then(|| inferred.to_owned()));
    if action == "rename" && title.as_deref().is_none_or(str::is_empty) {
        return Err(CliError::new(
            "workspace-action rename requires --title <text> (or a trailing title)",
        ));
    }
    let color = color_option
        .map(|value| value.trim().to_owned())
        .or_else(|| (action == "set_color" && !inferred.is_empty()).then(|| inferred.to_owned()));
    if action == "set_color" && color.as_deref().is_none_or(str::is_empty) {
        return Err(CliError::new(
            "workspace-action set-color requires --color <name|#hex> (or a trailing color)",
        ));
    }
    let description = description_option
        .map(|value| value.trim().to_owned())
        .or_else(|| {
            (action == "set_description" && !inferred.is_empty())
                .then(|| inferred_raw.trim().to_owned())
        });
    if action == "set_description" && description.as_deref().is_none_or(str::is_empty) {
        return Err(CliError::new(
            "workspace-action set-description requires --description <text> (or trailing text)",
        ));
    }

    let mut params = lifecycle_scope_values(workspace.as_deref(), window.as_deref())?;
    params.insert("action".into(), serde_json::json!(action));
    if let Some(title) = title.filter(|value| !value.is_empty()) {
        params.insert("title".into(), serde_json::json!(title));
    }
    if let Some(color) = color.filter(|value| !value.is_empty()) {
        params.insert("color".into(), serde_json::json!(color));
    }
    if let Some(description) = description.filter(|value| !value.is_empty()) {
        params.insert("description".into(), serde_json::json!(description));
    }
    Ok(serde_json::Value::Object(params))
}

fn respawn_pane_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let (workspace, rem0) = parse_frozen_option(args, "--workspace");
    let (surface, rem1) = parse_frozen_option(&rem0, "--surface");
    let (window, rem2) = parse_frozen_option(&rem1, "--window");
    let (command, remaining) = parse_frozen_option(&rem2, "--command");
    let mut params = lifecycle_scope_values(workspace.as_deref(), window.as_deref())?;
    if let Some(surface) = surface.as_deref() {
        apply_validated_surface_selector(surface, false, &mut params)?;
    }
    let requested = command
        .map(|command| command.trim().to_string())
        .or_else(|| {
            let remaining = if remaining.first().map(String::as_str) == Some("--") {
                &remaining[1..]
            } else {
                &remaining
            };
            join_then_trim(remaining)
        });
    let default_shell = native_windows_shell();
    let start_command = requested
        .filter(|command| !command.is_empty())
        .unwrap_or_else(|| default_shell.clone());
    params.insert(
        "command".into(),
        serde_json::json!(native_shell_wrapper(&default_shell, &start_command)),
    );
    params.insert(
        "tmux_start_command".into(),
        serde_json::json!(start_command),
    );
    Ok(serde_json::Value::Object(params))
}

fn parse_frozen_option(args: &[String], name: &str) -> (Option<String>, Vec<String>) {
    let mut remaining = Vec::new();
    let mut value = None;
    let mut skip_next = false;
    let mut past_terminator = false;
    let prefix = format!("{name}=");
    for (index, argument) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
        } else if argument == "--" {
            past_terminator = true;
            remaining.push(argument.clone());
        } else if !past_terminator && argument.starts_with(&prefix) {
            value = Some(argument[prefix.len()..].to_string());
        } else if !past_terminator && argument == name && index + 1 < args.len() {
            value = Some(args[index + 1].clone());
            skip_next = true;
        } else {
            remaining.push(argument.clone());
        }
    }
    (value, remaining)
}

fn normalize_action_name(action: &str) -> String {
    action.to_lowercase().replace('-', "_")
}

fn is_frozen_handle_ref(value: &str) -> bool {
    value.split_once(':').is_some_and(|(kind, index)| {
        matches!(
            kind.to_ascii_lowercase().as_str(),
            "window" | "workspace" | "pane" | "surface"
        ) && index.parse::<i64>().is_ok()
    })
}

fn apply_lifecycle_workspace_value(
    raw: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    let value = raw.trim();
    if value.is_empty() {
        params.remove("resolve_current_workspace");
        params.insert("suppress_ambient_workspace".into(), serde_json::json!(true));
        return Ok(());
    }
    if let Ok(index) = value.parse::<i64>() {
        params.insert("workspace_index".into(), serde_json::json!(index));
    } else if is_frozen_handle_ref(value) {
        params.insert("workspace_ref".into(), serde_json::json!(value));
    } else if uuid::Uuid::parse_str(value).is_ok() {
        params.insert("workspace_id".into(), serde_json::json!(value));
    } else {
        return Err(CliError::new(format!(
            "Invalid workspace handle: {value} (expected UUID, ref like workspace:1, or index)"
        )));
    }
    Ok(())
}

fn lifecycle_scope_values(
    workspace: Option<&str>,
    window: Option<&str>,
) -> Result<serde_json::Map<String, serde_json::Value>, CliError> {
    let mut params = serde_json::Map::new();
    if let Some(workspace) = workspace {
        apply_lifecycle_workspace_value(workspace, &mut params)?;
    }
    apply_lifecycle_window_value(window, &mut params)?;
    if !params.contains_key("workspace_id")
        && !params.contains_key("workspace_ref")
        && !params.contains_key("workspace_index")
        && !params.contains_key("suppress_ambient_workspace")
    {
        params.insert("resolve_current_workspace".into(), serde_json::json!(true));
    }
    Ok(params)
}

fn apply_lifecycle_window_value(
    raw: Option<&str>,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    let Some(raw) = raw else {
        return Ok(());
    };
    let value = raw.trim();
    if value.is_empty() {
        params.insert("suppress_ambient_window".into(), serde_json::json!(true));
        return Ok(());
    }
    if let Ok(index) = value.parse::<i64>() {
        params.insert("window_index".into(), serde_json::json!(index));
    } else if is_frozen_handle_ref(value) {
        params.insert("window_ref".into(), serde_json::json!(value));
    } else if uuid::Uuid::parse_str(value).is_ok() {
        params.insert("window_id".into(), serde_json::json!(value));
    } else {
        return Err(CliError::new(format!(
            "Invalid window handle: {value} (expected UUID, ref like window:1, or index)"
        )));
    }
    Ok(())
}

fn join_then_trim(parts: &[String]) -> Option<String> {
    let joined = parts.join(" ");
    let trimmed = joined.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn parse_optional_bool(value: Option<&str>, name: &str) -> Result<Option<bool>, CliError> {
    value
        .map(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(CliError::new(format!("{name} must be true|false"))),
        })
        .transpose()
}

fn apply_validated_surface_selector(
    raw: &str,
    allow_tab_ref: bool,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        params.insert("suppress_ambient_surface".into(), serde_json::json!(true));
        return Ok(());
    }
    let value = if allow_tab_ref {
        trimmed
            .split_once(':')
            .filter(|(kind, index)| {
                kind.eq_ignore_ascii_case("tab") && index.parse::<i64>().is_ok()
            })
            .map(|(_, index)| format!("surface:{index}"))
            .unwrap_or_else(|| trimmed.to_string())
    } else {
        trimmed.to_string()
    };
    let valid_reference = is_frozen_handle_ref(&value);
    if value.parse::<i64>().is_ok() || valid_reference {
        if let Ok(index) = value.parse::<i64>() {
            params.insert("surface_index".into(), serde_json::json!(index));
        } else {
            params.insert("surface_ref".into(), serde_json::json!(value));
        }
    } else if uuid::Uuid::parse_str(&value).is_ok() {
        params.insert("surface_id".into(), serde_json::json!(value));
    } else {
        return Err(CliError::new(format!(
            "Invalid surface handle: {trimmed} (expected UUID, ref like surface:1, or index)"
        )));
    }
    Ok(())
}

fn native_windows_shell() -> String {
    std::env::var("ComSpec")
        .ok()
        .map(|shell| shell.trim().to_string())
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| r"C:\Windows\System32\cmd.exe".to_string())
}

fn native_shell_wrapper(shell: &str, command: &str) -> String {
    format!(
        "& '{}' /d /s /c '{}'",
        powershell_single_quoted_content(shell),
        powershell_single_quoted_content(command)
    )
}

fn powershell_single_quoted_content(value: &str) -> String {
    value.replace('\'', "''")
}

fn surface_read_text_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if !parsed.positionals.is_empty() {
        return Err(CliError::new(format!(
            "{command_label}: unexpected arguments: {}",
            parsed.positionals.join(" ")
        )));
    }
    if let Some(flag) = parsed
        .flags
        .iter()
        .find(|flag| flag.as_str() != "--scrollback")
    {
        return Err(CliError::new(format!(
            "{command_label}: unexpected argument: {flag}"
        )));
    }

    let mut params = serde_json::Map::new();
    apply_window_scope_selector(&parsed, &mut params);
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if parsed.has_flag("--scrollback") {
        params.insert("scrollback".to_string(), serde_json::json!(true));
    }
    if let Some(lines) = parsed.value(&["--lines"]) {
        let lines = lines
            .parse::<usize>()
            .map_err(|_| CliError::new("--lines must be greater than 0"))?;
        if lines == 0 {
            return Err(CliError::new("--lines must be greater than 0"));
        }
        params.insert("lines".to_string(), serde_json::json!(lines));
        params.insert("scrollback".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

fn surface_tty_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;

    let tty = parsed
        .value(&["--tty", "--tty-name", "--ttyName", "--name"])
        .cloned()
        .or_else(|| {
            if has_surface_selector(&params) {
                return parsed.first_positional().map(str::to_string);
            }
            match parsed.positionals.as_slice() {
                [first, second, ..] if looks_like_surface_selector(first) => {
                    apply_surface_selector_value(first, "panel_id", &mut params);
                    Some(second.clone())
                }
                [first, ..] => Some(first.clone()),
                [] => None,
            }
        })
        .ok_or_else(|| CliError::new("report-tty requires a TTY name"))?;
    params.insert("tty".to_string(), serde_json::json!(tty));
    Ok(serde_json::Value::Object(params))
}

fn surface_shell_state_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;

    let state = parsed
        .value(&["--state", "--shell-state", "--shellState", "--activity"])
        .cloned()
        .or_else(|| {
            if has_surface_selector(&params) {
                return parsed.first_positional().map(str::to_string);
            }
            match parsed.positionals.as_slice() {
                [first, second, ..] if looks_like_surface_selector(first) => {
                    apply_surface_selector_value(first, "panel_id", &mut params);
                    Some(second.clone())
                }
                [first, ..] => Some(first.clone()),
                [] => None,
            }
        })
        .ok_or_else(|| CliError::new("report-shell-state requires a state"))?;
    params.insert("state".to_string(), serde_json::json!(state));
    Ok(serde_json::Value::Object(params))
}

fn surface_split_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(direction) = parsed.value(&["--direction", "--orientation", "--layout"]) {
        params.insert("direction".to_string(), serde_json::json!(direction));
    }
    if parsed.has_flag("--before") {
        params.insert("insert_first".to_string(), serde_json::json!(true));
    }
    apply_terminal_startup(&parsed, &mut params)?;
    Ok(serde_json::Value::Object(params))
}

fn surface_terminal_tab_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    apply_terminal_startup(&parsed, &mut params)?;
    Ok(serde_json::Value::Object(params))
}

fn surface_split_browser_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    surface_split_browser_params_inner(args, None)
}

fn surface_split_browser_params_with_direction(
    args: &[String],
    direction: &str,
) -> Result<serde_json::Value, CliError> {
    surface_split_browser_params_inner(args, Some(direction))
}

fn surface_split_browser_params_inner(
    args: &[String],
    default_direction: Option<&str>,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(direction) = parsed
        .value(&["--direction", "--orientation", "--layout"])
        .map(String::as_str)
        .or(default_direction)
    {
        params.insert("direction".to_string(), serde_json::json!(direction));
    }
    if parsed.has_flag("--before") {
        params.insert("insert_first".to_string(), serde_json::json!(true));
    }
    if let Some(url) = parsed
        .value(&["--url"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
    {
        params.insert("url".to_string(), serde_json::json!(url));
    }
    Ok(serde_json::Value::Object(params))
}

fn surface_open_browser_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(url) = parsed
        .value(&["--url"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
    {
        params.insert("url".to_string(), serde_json::json!(url));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_workspace_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    if let Some(url) = parsed
        .value(&["--url"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
    {
        params.insert("url".to_string(), serde_json::json!(url));
    }
    Ok(serde_json::Value::Object(params))
}

fn surface_open_markdown_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    let path = parsed
        .value(&["--path", "--file"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
        .ok_or_else(|| CliError::new("markdown requires a file path"))?;
    params.insert("file_path".to_string(), serde_json::json!(path));
    Ok(serde_json::Value::Object(params))
}

fn surface_title_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    let title = parsed
        .value(&["--title", "--name"])
        .cloned()
        .or_else(|| surface_title_from_positionals(&parsed))
        .ok_or_else(|| CliError::new("surface rename requires a title"))?;
    params.insert("title".to_string(), serde_json::json!(title));
    Ok(serde_json::Value::Object(params))
}

fn surface_bool_params(
    args: &[String],
    key: &str,
    value: bool,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    params.insert(key.to_string(), serde_json::json!(value));
    Ok(serde_json::Value::Object(params))
}

fn surface_open_diff_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(token) = parsed.value(&["--token", "--diff-token"]) {
        params.insert("token".to_string(), serde_json::json!(token));
    }
    if let Some(request_path) = parsed.value(&["--request-path", "--diff-path"]) {
        params.insert("request_path".to_string(), serde_json::json!(request_path));
    }
    Ok(serde_json::Value::Object(params))
}

fn surface_set_type_params(
    args: &[String],
    fixed_type: Option<&str>,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    let surface_type = fixed_type
        .map(str::to_string)
        .or_else(|| parsed.value(&["--type", "--kind"]).cloned())
        .or_else(|| parsed.first_positional().map(str::to_string))
        .ok_or_else(|| CliError::new("surface set-type requires a type"))?;
    params.insert("type".to_string(), serde_json::json!(surface_type));
    Ok(serde_json::Value::Object(params))
}

fn browser_show_developer_tools_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(panel) = parsed.value(&["--devtools-panel", "--tool-panel"]) {
        params.insert("panel".to_string(), serde_json::json!(panel));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_network_requests_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    if let Some(value) = parsed.value(&["--url-contains", "--urlContains", "--url"]) {
        params.insert("url_contains".to_string(), serde_json::json!(value));
    }
    if let Some(value) = parsed.value(&["--method"]) {
        params.insert("method".to_string(), serde_json::json!(value));
    }
    if let Some(value) = parsed.value(&["--since-id", "--sinceId", "--after-id", "--afterId"]) {
        params.insert("since_id".to_string(), serde_json::json!(value));
    }
    if let Some(value) = parsed.value(&["--limit"]) {
        let limit = value.parse::<usize>().map_err(|_| {
            CliError::new(format!(
                "browser network --limit requires a non-negative integer: {value}"
            ))
        })?;
        params.insert("limit".to_string(), serde_json::json!(limit));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_network_clear_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    Ok(serde_json::Value::Object(params))
}

fn browser_navigate_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let url = parsed
        .value(&["--url"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser navigate requires a URL"))?;
    params.insert("url".to_string(), serde_json::json!(url));
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_surface_only_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let params = browser_automation_base_params(&parsed)?;
    Ok(serde_json::Value::Object(params))
}

fn browser_eval_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let script = parsed
        .value(&["--script", "--expression"])
        .cloned()
        .or_else(|| (!payload.is_empty()).then(|| payload.join(" ")))
        .ok_or_else(|| CliError::new("browser eval requires a script"))?;
    params.insert("script".to_string(), serde_json::json!(script));
    Ok(serde_json::Value::Object(params))
}

fn browser_wait_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if let Some(selector) = parsed
        .value(&["--selector"])
        .cloned()
        .or_else(|| payload.first().cloned())
    {
        params.insert("selector".to_string(), serde_json::json!(selector));
    }
    if let Some(text) = parsed.value(&["--text-contains", "--text"]) {
        params.insert("text_contains".to_string(), serde_json::json!(text));
    }
    if let Some(function) = parsed.value(&["--function", "--expression"]) {
        params.insert("function".to_string(), serde_json::json!(function));
    }
    if let Some(load_state) = parsed.value(&["--load-state", "--loadState"]) {
        params.insert("load_state".to_string(), serde_json::json!(load_state));
    }
    if let Some(url_contains) = parsed.value(&["--url-contains", "--urlContains"]) {
        params.insert("url_contains".to_string(), serde_json::json!(url_contains));
    }
    if let Some(timeout) = parsed.value(&["--timeout-ms", "--timeout"]) {
        params.insert("timeout_ms".to_string(), serde_json::json!(timeout));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_selector_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if let Some(selector) = parsed
        .value(&["--selector"])
        .cloned()
        .or_else(|| payload.first().cloned())
    {
        params.insert("selector".to_string(), serde_json::json!(selector));
    }
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_text_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if let Some(selector) = parsed
        .value(&["--selector"])
        .cloned()
        .or_else(|| payload.first().cloned())
    {
        params.insert("selector".to_string(), serde_json::json!(selector));
    }
    if let Some(text) = parsed
        .value(&["--text", "--value"])
        .cloned()
        .or_else(|| payload.get(1).cloned())
    {
        params.insert("text".to_string(), serde_json::json!(text));
    }
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_key_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let key = parsed
        .value(&["--key"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser key command requires a key"))?;
    params.insert("key".to_string(), serde_json::json!(key));
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_select_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let selector = parsed
        .value(&["--selector"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser select requires a selector"))?;
    let value = parsed
        .value(&["--value"])
        .cloned()
        .or_else(|| payload.get(1).cloned())
        .ok_or_else(|| CliError::new("browser select requires a value"))?;
    params.insert("selector".to_string(), serde_json::json!(selector));
    params.insert("value".to_string(), serde_json::json!(value));
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_scroll_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_selector_action_params(args)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(dx) = parsed.value(&["--dx"]) {
        params.insert("dx".to_string(), serde_json::json!(dx));
    }
    if let Some(dy) = parsed.value(&["--dy"]) {
        params.insert("dy".to_string(), serde_json::json!(dy));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_screenshot_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    if let Some(path) = parsed.value(&["--path", "--out", "--file"]) {
        params.insert("path".to_string(), serde_json::json!(path));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_get_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(
        args,
        &[
            "text", "html", "value", "attr", "url", "title", "count", "box", "styles",
        ],
    ) else {
        return Err(CliError::new(
            "browser get requires text, html, value, attr, url, title, count, box, or styles",
        ));
    };
    if subcommand == "url" {
        return Ok(ControlCommand::new(
            "browser.url.get",
            browser_surface_only_params(&rest)?,
        ));
    }
    let method = format!("browser.get.{subcommand}");
    Ok(ControlCommand::new(
        method,
        browser_get_params(&subcommand, &rest)?,
    ))
}

fn browser_get_params(kind: &str, args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if !matches!(kind, "title") {
        if let Some(selector) = parsed
            .value(&["--selector"])
            .cloned()
            .or_else(|| payload.first().cloned())
        {
            params.insert("selector".to_string(), serde_json::json!(selector));
        }
    }
    if kind == "attr" {
        if let Some(attr) = parsed
            .value(&["--attr", "--attribute", "--name"])
            .cloned()
            .or_else(|| payload.get(1).cloned())
        {
            params.insert("attr".to_string(), serde_json::json!(attr));
        }
    }
    if kind == "styles" {
        if let Some(property) = parsed
            .value(&["--property", "--name"])
            .cloned()
            .or_else(|| payload.get(1).cloned())
        {
            params.insert("property".to_string(), serde_json::json!(property));
        }
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_is_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) =
        split_browser_nested_subcommand(args, &["visible", "enabled", "checked"])
    else {
        return Err(CliError::new(
            "browser is requires visible, enabled, or checked",
        ));
    };
    Ok(ControlCommand::new(
        format!("browser.is.{subcommand}"),
        browser_selector_action_params(&rest)?,
    ))
}

fn browser_find_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(
        args,
        &[
            "role",
            "text",
            "label",
            "placeholder",
            "alt",
            "title",
            "testid",
            "first",
            "last",
            "nth",
        ],
    ) else {
        return Err(CliError::new("browser find requires a locator kind"));
    };
    Ok(ControlCommand::new(
        format!("browser.find.{subcommand}"),
        browser_find_params(&subcommand, &rest)?,
    ))
}

fn browser_find_params(kind: &str, args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    match kind {
        "role" => {
            if let Some(role) = parsed
                .value(&["--role"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("role".to_string(), serde_json::json!(role));
            }
            if let Some(name) = parsed
                .value(&["--name"])
                .cloned()
                .or_else(|| payload.get(1).cloned())
            {
                params.insert("name".to_string(), serde_json::json!(name));
            }
        }
        "text" | "label" | "placeholder" | "alt" | "title" | "testid" => {
            let key = if kind == "testid" { "testid" } else { kind };
            if let Some(value) = parsed
                .value(&[&format!("--{key}")])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert(key.to_string(), serde_json::json!(value));
            }
        }
        "first" | "last" => {
            if let Some(selector) = parsed
                .value(&["--selector"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("selector".to_string(), serde_json::json!(selector));
            }
        }
        "nth" => {
            if let Some(selector) = parsed
                .value(&["--selector"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("selector".to_string(), serde_json::json!(selector));
            }
            if let Some(index) = parsed
                .value(&["--index"])
                .cloned()
                .or_else(|| payload.get(1).cloned())
            {
                params.insert("index".to_string(), serde_json::json!(index));
            }
        }
        _ => {}
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_frame_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if payload
        .first()
        .is_some_and(|value| value.eq_ignore_ascii_case("main"))
    {
        return Ok(ControlCommand::new(
            "browser.frame.main",
            browser_surface_only_params(args)?,
        ));
    }
    Ok(ControlCommand::new(
        "browser.frame.select",
        browser_selector_action_params(args)?,
    ))
}

fn browser_dialog_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["accept", "dismiss"])
    else {
        return Err(CliError::new("browser dialog requires accept or dismiss"));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    if let Some(text) = parsed.value(&["--text"]).cloned().or_else(|| {
        browser_payload_positionals_after_optional_surface(&parsed)
            .first()
            .cloned()
    }) {
        params.insert("text".to_string(), serde_json::json!(text));
    }
    Ok(ControlCommand::new(
        format!("browser.dialog.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_download_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["wait"]) else {
        return Err(CliError::new("browser download requires wait"));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let path = parsed
        .value(&["--path", "--file"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser download wait requires a path"))?;
    params.insert("path".to_string(), serde_json::json!(path));
    if let Some(timeout) = parsed.value(&["--timeout-ms", "--timeout"]) {
        params.insert("timeout_ms".to_string(), serde_json::json!(timeout));
    }
    Ok(ControlCommand::new(
        format!("browser.download.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_cookies_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["get", "set", "clear"])
    else {
        return Ok(ControlCommand::new(
            "browser.cookies.get",
            browser_surface_only_params(args)?,
        ));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if let Some(name) = parsed
        .value(&["--name"])
        .cloned()
        .or_else(|| payload.first().cloned())
    {
        params.insert("name".to_string(), serde_json::json!(name));
    }
    if let Some(value) = parsed
        .value(&["--value"])
        .cloned()
        .or_else(|| payload.get(1).cloned())
    {
        params.insert("value".to_string(), serde_json::json!(value));
    }
    Ok(ControlCommand::new(
        format!("browser.cookies.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_storage_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let mut cursor = 0;
    let storage_type = payload
        .get(cursor)
        .filter(|value| matches!(value.as_str(), "local" | "session"))
        .cloned()
        .unwrap_or_else(|| "local".to_string());
    if matches!(
        payload.get(cursor).map(String::as_str),
        Some("local" | "session")
    ) {
        cursor += 1;
    }
    let action = payload
        .get(cursor)
        .map(String::as_str)
        .unwrap_or("get")
        .to_ascii_lowercase();
    if matches!(action.as_str(), "get" | "set" | "clear") {
        cursor += 1;
    }
    let mut params = browser_automation_base_params(&parsed)?;
    params.insert("type".to_string(), serde_json::json!(storage_type));
    if let Some(key) = parsed
        .value(&["--key"])
        .cloned()
        .or_else(|| payload.get(cursor).cloned())
    {
        params.insert("key".to_string(), serde_json::json!(key));
    }
    if let Some(value) = parsed
        .value(&["--value"])
        .cloned()
        .or_else(|| payload.get(cursor + 1).cloned())
    {
        params.insert("value".to_string(), serde_json::json!(value));
    }
    Ok(ControlCommand::new(
        format!(
            "browser.storage.{}",
            if matches!(action.as_str(), "get" | "set" | "clear") {
                action
            } else {
                "get".to_string()
            }
        ),
        serde_json::Value::Object(params),
    ))
}

fn browser_tab_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) =
        split_browser_nested_subcommand(args, &["new", "list", "switch", "close"])
    else {
        return Ok(ControlCommand::new(
            "browser.tab.list",
            browser_surface_only_params(args)?,
        ));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    match subcommand.as_str() {
        "new" => {
            if let Some(url) = parsed
                .value(&["--url"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("url".to_string(), serde_json::json!(url));
            }
        }
        "switch" | "close" => {
            if let Some(target) = parsed
                .value(&["--target", "--tab"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("target_surface_id".to_string(), serde_json::json!(target));
            }
        }
        _ => {}
    }
    Ok(ControlCommand::new(
        format!("browser.tab.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_console_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["list", "clear"]) else {
        return Ok(ControlCommand::new(
            "browser.console.list",
            browser_surface_only_params(args)?,
        ));
    };
    Ok(ControlCommand::new(
        format!("browser.console.{subcommand}"),
        browser_surface_only_params(&rest)?,
    ))
}

fn browser_errors_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["list"]) else {
        return Ok(ControlCommand::new(
            "browser.errors.list",
            browser_surface_only_params(args)?,
        ));
    };
    Ok(ControlCommand::new(
        format!("browser.errors.{subcommand}"),
        browser_surface_only_params(&rest)?,
    ))
}

fn browser_state_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["save", "load"]) else {
        return Err(CliError::new("browser state requires save or load"));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let path = parsed
        .value(&["--path", "--file"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser state requires a path"))?;
    params.insert("path".to_string(), serde_json::json!(path));
    Ok(ControlCommand::new(
        format!("browser.state.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_script_params(args: &[String], key: &str) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let value = parsed
        .value(&["--script", "--css", "--text"])
        .cloned()
        .or_else(|| (!payload.is_empty()).then(|| payload.join(" ")))
        .ok_or_else(|| CliError::new(format!("browser {key} command requires a payload")))?;
    params.insert(key.to_string(), serde_json::json!(value));
    Ok(serde_json::Value::Object(params))
}

fn browser_automation_base_params(
    parsed: &ParsedArgs,
) -> Result<serde_json::Map<String, serde_json::Value>, CliError> {
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(parsed, &mut params);
    apply_surface_selector(parsed, &mut params)?;
    if !has_surface_selector(&params) {
        if let Some(value) = parsed
            .positionals
            .first()
            .filter(|value| looks_like_surface_selector(value))
        {
            apply_surface_selector_value(value, "panel_id", &mut params);
        }
    }
    Ok(params)
}

fn browser_payload_positionals_after_optional_surface(parsed: &ParsedArgs) -> Vec<String> {
    if has_surface_selector_from_args(parsed) {
        return parsed.positionals.clone();
    }
    let Some(first) = parsed.positionals.first() else {
        return Vec::new();
    };
    if looks_like_surface_selector(first) {
        return parsed.positionals[1..].to_vec();
    }
    parsed.positionals.clone()
}

fn looks_like_surface_selector(value: &str) -> bool {
    value.starts_with("surface:") || value.starts_with("surface-")
}

fn insert_snapshot_after_flag(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if parsed.has_flag("--snapshot-after") {
        params.insert("snapshot_after".to_string(), serde_json::json!(true));
    }
}

fn browser_viewport_set_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    let positionals = browser_payload_positionals_after_surface(&parsed);
    if let Some(width) = parsed
        .value(&["--width"])
        .cloned()
        .or_else(|| positionals.first().cloned())
    {
        params.insert("width".to_string(), serde_json::json!(width));
    }
    if let Some(height) = parsed
        .value(&["--height"])
        .cloned()
        .or_else(|| positionals.get(1).cloned())
    {
        params.insert("height".to_string(), serde_json::json!(height));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_geolocation_set_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    let positionals = browser_payload_positionals_after_surface(&parsed);
    if let Some(latitude) = parsed
        .value(&["--latitude", "--lat"])
        .cloned()
        .or_else(|| positionals.first().cloned())
    {
        params.insert("latitude".to_string(), serde_json::json!(latitude));
    }
    if let Some(longitude) = parsed
        .value(&["--longitude", "--lon", "--lng"])
        .cloned()
        .or_else(|| positionals.get(1).cloned())
    {
        params.insert("longitude".to_string(), serde_json::json!(longitude));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_offline_set_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    let positionals = browser_payload_positionals_after_surface(&parsed);
    if let Some(enabled) = parsed
        .value(&["--enabled"])
        .cloned()
        .or_else(|| positionals.first().cloned())
    {
        params.insert("enabled".to_string(), serde_json::json!(enabled));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_trace_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["start", "stop"]) else {
        return Err(CliError::new("browser trace requires start or stop"));
    };
    let method = match subcommand.as_str() {
        "start" => "browser.trace.start",
        "stop" => "browser.trace.stop",
        _ => {
            return Err(CliError::new(format!(
                "unknown browser trace command: {subcommand}"
            )))
        }
    };
    Ok(ControlCommand::new(
        method,
        browser_passthrough_params(&rest)?,
    ))
}

fn browser_screencast_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["start", "stop"]) else {
        return Err(CliError::new("browser screencast requires start or stop"));
    };
    let method = match subcommand.as_str() {
        "start" => "browser.screencast.start",
        "stop" => "browser.screencast.stop",
        _ => {
            return Err(CliError::new(format!(
                "unknown browser screencast command: {subcommand}"
            )))
        }
    };
    Ok(ControlCommand::new(
        method,
        browser_passthrough_params(&rest)?,
    ))
}

fn browser_input_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) =
        split_browser_nested_subcommand(args, &["mouse", "keyboard", "touch"])
    else {
        return Err(CliError::new(
            "browser input requires mouse, keyboard, or touch",
        ));
    };
    let method = match subcommand.as_str() {
        "mouse" => "browser.input_mouse",
        "keyboard" => "browser.input_keyboard",
        "touch" => "browser.input_touch",
        _ => {
            return Err(CliError::new(format!(
                "unknown browser input command: {subcommand}"
            )))
        }
    };
    let mut params = browser_passthrough_params(&rest)?;
    if let Some(object) = params.as_object_mut() {
        object.insert("args".to_string(), serde_json::json!(rest));
    }
    Ok(ControlCommand::new(method, params))
}

fn split_browser_nested_subcommand(
    args: &[String],
    allowed: &[&str],
) -> Option<(String, Vec<String>)> {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            return None;
        }
        if arg.starts_with("--") {
            index += if arg.contains('=') || !takes_value(arg) {
                1
            } else {
                2
            };
            continue;
        }
        let lowered = arg.to_ascii_lowercase();
        if allowed.contains(&lowered.as_str()) {
            let mut rest = Vec::with_capacity(args.len().saturating_sub(1));
            rest.extend(args[..index].iter().cloned());
            rest.extend(args[index + 1..].iter().cloned());
            return Some((lowered, rest));
        }
        if args
            .get(index + 1)
            .map(|value| value.to_ascii_lowercase())
            .is_some_and(|next| allowed.contains(&next.as_str()))
        {
            let mut rest = Vec::with_capacity(args.len().saturating_sub(1));
            rest.extend(args[..index].iter().cloned());
            rest.push(arg.clone());
            rest.extend(args[index + 2..].iter().cloned());
            return Some((args[index + 1].to_ascii_lowercase(), rest));
        }
        return None;
    }
    None
}

fn browser_passthrough_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    if !parsed.positionals.is_empty() {
        params.insert(
            "args".to_string(),
            serde_json::json!(browser_payload_positionals_after_surface(&parsed)),
        );
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_payload_positionals_after_surface(parsed: &ParsedArgs) -> Vec<String> {
    if has_surface_selector_from_args(parsed) {
        return parsed.positionals.clone();
    }
    let Some(first) = parsed.positionals.first() else {
        return Vec::new();
    };
    if first.starts_with("surface") && parsed.positionals.len() > 1 {
        return parsed.positionals[1..].to_vec();
    }
    parsed.positionals.clone()
}

fn browser_set_zoom_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    let zoom = parsed
        .value(&["--zoom", "--scale"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
        .ok_or_else(|| CliError::new("browser zoom requires a zoom value"))?;
    params.insert("zoom".to_string(), serde_json::json!(zoom));
    Ok(serde_json::Value::Object(params))
}

fn surface_workspace_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn surface_focus_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    if !has_surface_selector(&params) {
        return Err(CliError::new("focus-panel requires --panel"));
    }
    Ok(serde_json::Value::Object(params))
}

fn pane_focus_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    let pane = parsed
        .value(&["--pane"])
        .map(String::as_str)
        .or_else(|| parsed.first_positional())
        .ok_or_else(|| CliError::new("focus-pane requires --pane <id|ref>"))?;
    apply_pane_target_selector(pane, &mut params);
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn surface_send_text_params(
    args: &[String],
    require_panel: bool,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if require_panel && !has_surface_selector(&params) {
        return Err(CliError::new("send-panel requires --panel"));
    }
    let text = parsed.positionals.join(" ");
    if text.is_empty() {
        return Err(CliError::new(if require_panel {
            "send-panel requires text"
        } else {
            "send requires text"
        }));
    }
    params.insert(
        "text".to_string(),
        serde_json::json!(unescape_send_text(&text)),
    );
    Ok(serde_json::Value::Object(params))
}

fn surface_send_key_params(
    args: &[String],
    require_panel: bool,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if require_panel && !has_surface_selector(&params) {
        return Err(CliError::new("send-key-panel requires --panel"));
    }
    let key = parsed
        .positionals
        .first()
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CliError::new(if require_panel {
                "send-key-panel requires a key"
            } else {
                "send-key requires a key"
            })
        })?;
    params.insert("key".to_string(), serde_json::json!(key));
    Ok(serde_json::Value::Object(params))
}

fn apply_workspace_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if parsed.value(&["--index"]).is_some() {
        return Err(CliError::new(
            "workspace selectors require workspace:N or workspace id; --index is not supported",
        ));
    }
    if let Some(reference) = parsed.value(&["--ref", "--workspace-ref"]) {
        params.insert("workspace_ref".to_string(), serde_json::json!(reference));
        return Ok(());
    }
    if let Some(id) = parsed.value(&["--workspace", "--workspace-id", "--tab", "--id"]) {
        params.insert("workspace_id".to_string(), serde_json::json!(id));
        return Ok(());
    }
    if let Some(positional) = parsed.first_positional() {
        if positional.chars().all(|ch| ch.is_ascii_digit()) {
            params.insert(
                "workspace_ref".to_string(),
                serde_json::json!(format!("workspace:{positional}")),
            );
        } else if positional.starts_with("workspace:") {
            params.insert("workspace_ref".to_string(), serde_json::json!(positional));
        } else {
            params.insert("workspace_id".to_string(), serde_json::json!(positional));
        }
    }
    Ok(())
}

fn apply_workspace_target_selector(
    value: &str,
    key_prefix: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        params.insert(
            format!("{key_prefix}_ref"),
            serde_json::json!(format!("workspace:{value}")),
        );
    } else if value.starts_with("workspace:") {
        params.insert(format!("{key_prefix}_ref"), serde_json::json!(value));
    } else {
        params.insert(format!("{key_prefix}_id"), serde_json::json!(value));
    }
}

fn apply_workspace_scope_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if let Some(reference) = parsed.value(&["--workspace-ref"]) {
        params.insert("workspace_ref".to_string(), serde_json::json!(reference));
        return;
    }
    if let Some(value) = parsed.value(&["--workspace", "--workspace-id", "--tab"]) {
        if value.chars().all(|ch| ch.is_ascii_digit()) {
            params.insert(
                "workspace_ref".to_string(),
                serde_json::json!(format!("workspace:{value}")),
            );
        } else if value.starts_with("workspace:") {
            params.insert("workspace_ref".to_string(), serde_json::json!(value));
        } else {
            params.insert("workspace_id".to_string(), serde_json::json!(value));
        }
    }
}

fn apply_window_scope_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    let Some(value) = parsed.value(&["--window", "--window-id"]) else {
        return;
    };
    apply_window_selector_value(value, params);
}

fn apply_window_selector_value(
    value: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        params.insert(
            "window_ref".to_string(),
            serde_json::json!(format!("window:{value}")),
        );
    } else if value.starts_with("window:") {
        params.insert("window_ref".to_string(), serde_json::json!(value));
    } else {
        params.insert("window_id".to_string(), serde_json::json!(value));
    }
}

fn has_explicit_workspace_selector(parsed: &ParsedArgs) -> bool {
    parsed.value(&["--ref", "--workspace-ref"]).is_some()
        || parsed
            .value(&["--workspace", "--workspace-id", "--tab", "--id"])
            .is_some()
}

fn apply_surface_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if parsed.value(&["--index"]).is_some() {
        return Err(CliError::new(
            "surface selectors require surface:N or surface id; --index is not supported",
        ));
    }
    if let Some(reference) = parsed.value(&["--ref", "--surface-ref"]) {
        params.insert("surface_ref".to_string(), serde_json::json!(reference));
        return Ok(());
    }
    if let Some(value) = parsed.value(&["--surface"]) {
        apply_surface_selector_value(value, "surface_id", params);
        return Ok(());
    }
    if let Some(id) = parsed.value(&["--surface-id"]) {
        params.insert("surface_id".to_string(), serde_json::json!(id));
        return Ok(());
    }
    if let Some(value) = parsed.value(&["--panel"]) {
        apply_surface_selector_value(value, "panel_id", params);
        return Ok(());
    }
    if let Some(id) = parsed.value(&["--panel-id", "--id"]) {
        params.insert("panel_id".to_string(), serde_json::json!(id));
        return Ok(());
    }
    Ok(())
}

fn apply_surface_selector_value(
    value: &str,
    actual_id_key: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        params.insert(
            "surface_ref".to_string(),
            serde_json::json!(format!("surface:{value}")),
        );
    } else if value.starts_with("surface:") {
        params.insert("surface_ref".to_string(), serde_json::json!(value));
    } else {
        params.insert(actual_id_key.to_string(), serde_json::json!(value));
    }
}

fn apply_surface_target_selector(
    value: &str,
    key_prefix: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|character| character.is_ascii_digit()) {
        params.insert(
            format!("{key_prefix}_ref"),
            serde_json::json!(format!("surface:{value}")),
        );
    } else if value.starts_with("surface:") {
        params.insert(format!("{key_prefix}_ref"), serde_json::json!(value));
    } else {
        params.insert(format!("{key_prefix}_id"), serde_json::json!(value));
    }
}

fn apply_pane_target_selector(
    value: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    apply_named_pane_target_selector(value, "pane", params);
}

fn apply_named_pane_target_selector(
    value: &str,
    key_prefix: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|character| character.is_ascii_digit()) {
        params.insert(
            format!("{key_prefix}_ref"),
            serde_json::json!(format!("pane:{value}")),
        );
    } else if value.starts_with("pane:") {
        params.insert(format!("{key_prefix}_ref"), serde_json::json!(value));
    } else {
        params.insert(format!("{key_prefix}_id"), serde_json::json!(value));
    }
}

fn apply_surface_selector_or_positional(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    apply_surface_selector(parsed, params)?;
    if has_surface_selector(params) {
        return Ok(());
    }
    if let Some(value) = parsed.first_positional() {
        apply_surface_selector_value(value, "panel_id", params);
    }
    Ok(())
}

fn has_surface_selector(params: &serde_json::Map<String, serde_json::Value>) -> bool {
    params.contains_key("surface_ref")
        || params.contains_key("surface_id")
        || params.contains_key("panel_id")
}

fn has_surface_selector_from_args(parsed: &ParsedArgs) -> bool {
    parsed.value(&["--ref", "--surface-ref"]).is_some()
        || parsed.value(&["--surface", "--surface-id"]).is_some()
        || parsed.value(&["--panel", "--panel-id", "--id"]).is_some()
}

fn apply_terminal_startup(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if let Some(command) = parsed.value(&["--command"]) {
        params.insert(
            "initial_terminal_command".to_string(),
            serde_json::json!(command),
        );
    }
    if let Some(input) = parsed.value(&["--input"]) {
        params.insert(
            "initial_terminal_input".to_string(),
            serde_json::json!(input),
        );
    }
    if let Some(environment) = terminal_environment_param(parsed)? {
        params.insert(
            "initial_terminal_environment".to_string(),
            serde_json::json!(environment),
        );
    }
    Ok(())
}

fn terminal_environment_param(
    parsed: &ParsedArgs,
) -> Result<Option<BTreeMap<String, String>>, CliError> {
    let mut environment = BTreeMap::new();
    for assignment in parsed.values(&["--env", "--environment"]) {
        let (key, value) = assignment.split_once('=').ok_or_else(|| {
            CliError::new(format!(
                "invalid environment assignment: {assignment} (expected KEY=VALUE)"
            ))
        })?;
        let key = key.trim();
        if key.is_empty() {
            return Err(CliError::new(format!(
                "invalid environment assignment: {assignment} (key cannot be empty)"
            )));
        }
        environment.insert(key.to_string(), value.to_string());
    }
    Ok((!environment.is_empty()).then_some(environment))
}

fn surface_title_from_positionals(parsed: &ParsedArgs) -> Option<String> {
    let title = parsed.positionals.join(" ");
    (!title.trim().is_empty()).then_some(title)
}

fn unescape_send_text(text: &str) -> String {
    let mut output = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some('t') => output.push('\t'),
            Some('\\') => output.push('\\'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }
    output
}

fn looks_like_workspace_selector(value: &str) -> bool {
    value.starts_with("workspace:") || value.chars().all(|ch| ch.is_ascii_digit())
}

#[derive(Debug, Default)]
struct ParsedArgs {
    values: Vec<(String, String)>,
    flags: Vec<String>,
    positionals: Vec<String>,
}

impl ParsedArgs {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        let mut parsed = ParsedArgs::default();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if arg == "--" {
                parsed.positionals.extend(args[index + 1..].iter().cloned());
                break;
            }
            if let Some((key, value)) = arg.split_once('=') {
                if key.starts_with("--") {
                    parsed.values.push((key.to_string(), value.to_string()));
                    index += 1;
                    continue;
                }
            }
            if takes_value(arg) {
                let value = args
                    .get(index + 1)
                    .cloned()
                    .ok_or_else(|| CliError::new(format!("{arg} requires a value")))?;
                parsed.values.push((arg.clone(), value));
                index += 2;
            } else if arg.starts_with("--") {
                parsed.flags.push(arg.clone());
                index += 1;
            } else {
                parsed.positionals.push(arg.clone());
                index += 1;
            }
        }
        Ok(parsed)
    }

    fn value(&self, names: &[&str]) -> Option<&String> {
        self.values
            .iter()
            .rev()
            .find_map(|(name, value)| names.contains(&name.as_str()).then_some(value))
    }

    fn values(&self, names: &[&str]) -> Vec<&String> {
        self.values
            .iter()
            .filter_map(|(name, value)| names.contains(&name.as_str()).then_some(value))
            .collect()
    }

    fn has_flag(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }

    fn first_positional(&self) -> Option<&str> {
        self.positionals.first().map(String::as_str)
    }
}

fn takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "--after"
            | "--after-surface"
            | "--after-workspace"
            | "--action"
            | "--amount"
            | "--before"
            | "--before-surface"
            | "--before-workspace"
            | "--branch"
            | "--command"
            | "--color"
            | "--css"
            | "--cwd"
            | "--direction"
            | "--directory"
            | "--devtools-panel"
            | "--diff-path"
            | "--diff-token"
            | "--dx"
            | "--dy"
            | "--description"
            | "--body"
            | "--env"
            | "--env-file"
            | "--environment"
            | "--file"
            | "--format"
            | "--focus"
            | "--from"
            | "--function"
            | "--group"
            | "--group-id"
            | "--group-placement"
            | "--group-reference"
            | "--hex"
            | "--height"
            | "--href"
            | "--id"
            | "--icon"
            | "--index"
            | "--input"
            | "--key"
            | "--kind"
            | "--label"
            | "--lat"
            | "--latitude"
            | "--layout"
            | "--load-state"
            | "--loadState"
            | "--limit"
            | "--lines"
            | "--level"
            | "--lng"
            | "--lon"
            | "--longitude"
            | "--markdown"
            | "--method"
            | "--message"
            | "--name"
            | "--number"
            | "--orientation"
            | "--order"
            | "--out"
            | "--pane"
            | "--panel"
            | "--panel-id"
            | "--path"
            | "--placement"
            | "--pid"
            | "--property"
            | "--process-id"
            | "--processId"
            | "--priority"
            | "--pr"
            | "--ref"
            | "--request-path"
            | "--scale"
            | "--script"
            | "--selector"
            | "--state"
            | "--subtitle"
            | "--surface"
            | "--surface-id"
            | "--surface-ref"
            | "--symbol"
            | "--status"
            | "--title"
            | "--timeout"
            | "--timeout-ms"
            | "--token"
            | "--tool-panel"
            | "--tty"
            | "--tty-name"
            | "--ttyName"
            | "--text"
            | "--text-contains"
            | "--to"
            | "--to-index"
            | "--type"
            | "--target-index"
            | "--url"
            | "--url-contains"
            | "--urlContains"
            | "--since-id"
            | "--sinceId"
            | "--after-id"
            | "--afterId"
            | "--workspace"
            | "--workspace-id"
            | "--workspace-ref"
            | "--window"
            | "--window-id"
            | "--width"
            | "--zoom"
            | "--enabled"
            | "--attr"
            | "--attribute"
            | "--expression"
            | "--role"
            | "--target"
            | "--target-pane"
            | "--tab"
            | "--value"
            | "--progress"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|token| (*token).to_owned()).collect()
    }

    fn mapped(command: &str, tokens: &[&str]) -> ControlCommand {
        control_command_for(command, &args(tokens))
            .expect("valid")
            .expect("mapped")
    }

    #[test]
    fn agent_hibernation_maps_canonical_aliases_and_rejects_invalid_usage() {
        for (value, enabled) in [
            ("on", true),
            ("ENABLE", true),
            ("off", false),
            ("Disable", false),
        ] {
            let command = mapped("agent-hibernation", &[value]);
            assert_eq!(command.method, "agent_hibernation");
            assert_eq!(command.params, serde_json::json!({"enabled": enabled}));
        }

        for invalid in [vec![], vec!["enabled"], vec!["on", "extra"]] {
            let error = control_command_for("agent-hibernation", &args(&invalid))
                .expect_err("invalid hibernation CLI usage must fail");
            assert_eq!(
                error.message,
                "Usage: cmux agent-hibernation <on|off> [--json]"
            );
        }
    }

    #[test]
    fn set_agent_lifecycle_normalizes_state_and_preserves_scope() {
        for value in ["needsInput", "needsinput", "needs-input", "needs_input"] {
            let command = mapped(
                "set-agent-lifecycle",
                &[
                    "codex",
                    value,
                    "ignored-extra-positional",
                    "--tab=workspace-2",
                    "--panel=surface-3",
                ],
            );
            assert_eq!(command.method, "workspace.set_agent_lifecycle");
            assert_eq!(
                command.params,
                serde_json::json!({
                    "key": "codex",
                    "lifecycle": "needsInput",
                    "workspace_id": "workspace-2",
                    "panel_id": "surface-3",
                })
            );
        }

        let alias = mapped(
            "set_agent_lifecycle",
            &["claude_code", "IDLE", "--tab", "2", "--panel", "panel-id"],
        );
        assert_eq!(alias.method, "workspace.set_agent_lifecycle");
        assert_eq!(
            alias.params,
            serde_json::json!({
                "key": "claude_code",
                "lifecycle": "idle",
                "workspace_ref": "workspace:2",
                "panel_id": "panel-id",
            })
        );

        let invalid = control_command_for(
            "set-agent-lifecycle",
            &args(&["codex", "waiting", "--tab", "workspace-2"]),
        )
        .expect_err("unknown lifecycle must fail before dispatch");
        assert_eq!(
            invalid.message,
            "Invalid agent lifecycle 'waiting' — usage: cmux set-agent-lifecycle <key> <unknown|running|idle|needsInput> [--tab=<id>] [--panel=<id>]"
        );

        let missing = control_command_for("set-agent-lifecycle", &args(&["codex"]))
            .expect_err("missing lifecycle must fail");
        assert_eq!(
            missing.message,
            "Usage: cmux set-agent-lifecycle <key> <unknown|running|idle|needsInput> [--tab=<id>] [--panel=<id>]"
        );
    }

    #[test]
    fn ambient_workspace_id_is_added_to_scoped_surface_commands() {
        let command = mapped("new-split", &["--direction", "down"])
            .with_ambient_workspace_id(Some(" workspace-2 "));
        assert_eq!(command.method, "surface.split");
        assert_eq!(
            command.params,
            serde_json::json!({"direction": "down", "workspace_id": "workspace-2"})
        );
    }

    #[test]
    fn ambient_workspace_id_does_not_override_explicit_workspace_selector() {
        let command = mapped("close-workspace", &["--workspace", "workspace-1"])
            .with_ambient_workspace_id(Some("workspace-2"));
        assert_eq!(command.method, "workspace.close");
        assert_eq!(
            command.params,
            serde_json::json!({"workspace_id": "workspace-1"})
        );
    }

    #[test]
    fn window_scope_is_normalized_without_overriding_explicit_params() {
        let command = mapped("read-screen", &[]).with_window_id(Some("2"));
        assert_eq!(command.params["window_ref"], serde_json::json!("window:2"));

        let command = mapped("clear-history", &[]).with_window_id(Some("2"));
        assert_eq!(command.params["window_ref"], serde_json::json!("window:2"));

        let command = mapped("trigger-flash", &[]).with_window_id(Some("2"));
        assert_eq!(command.params["window_ref"], serde_json::json!("window:2"));

        let command = mapped("clear-notifications", &[]).with_window_id(Some("2"));
        assert_eq!(command.params["window_ref"], serde_json::json!("window:2"));

        let command = mapped("current-window", &[]).with_window_id(Some("2"));
        assert_eq!(command.params["window_ref"], serde_json::json!("window:2"));

        let command =
            mapped("read-screen", &["--window", "window:3"]).with_window_id(Some("window:2"));
        assert_eq!(command.params["window_ref"], serde_json::json!("window:3"));
    }

    #[test]
    fn ambient_workspace_id_routes_legacy_workspace_list() {
        let command = mapped("list-workspaces", &[]).with_ambient_workspace_id(Some("workspace-2"));
        assert_eq!(command.method, "workspace.list");
        assert_eq!(
            command.params,
            serde_json::json!({"workspace_id":"workspace-2"})
        );
    }

    #[test]
    fn workspace_group_commands_carry_canonical_caller_context() {
        let command = mapped("workspace-group", &["create", "Build"])
            .with_ambient_surface_id(Some("surface:2"))
            .with_ambient_workspace_id(Some("workspace:2"));
        assert_eq!(command.method, "workspace.group.create");
        assert_eq!(
            command.params,
            serde_json::json!({
                "name":"Build",
                "surface_id":"surface:2",
                "workspace_id":"workspace:2"
            })
        );
    }

    #[test]
    fn maps_system_commands() {
        assert_eq!(mapped("ping", &[]).method, "ping");
        assert_eq!(mapped("capabilities", &[]).method, "system.capabilities");
        assert_eq!(mapped("identify", &[]).method, "system.identify");
        assert_eq!(mapped("reload-config", &[]).method, "config.reload");
        let error = control_command_for("reload-config", &args(&["extra"])).unwrap_err();
        assert_eq!(
            error.message,
            "reload-config does not accept arguments. Unexpected argument 'extra'"
        );
        assert_eq!(mapped("list-windows", &[]).method, "window.list");
        assert_eq!(mapped("current-window", &[]).method, "window.current");
        assert_eq!(
            mapped("list-notifications", &[]).method,
            "notification.list"
        );
        assert_eq!(
            mapped("refresh-surfaces", &[]).method,
            "surface.refresh_all"
        );
    }

    #[test]
    fn maps_window_display_namespace_and_global_window_target() {
        let displays = mapped("window", &["displays"]);
        assert_eq!(displays.method, "window.displays");
        assert_eq!(displays.params, serde_json::json!({}));

        let display = mapped("window", &["display", "LG HDR 4K"]).with_window_id(Some("2"));
        assert_eq!(display.method, "window.display");
        assert_eq!(
            display.params,
            serde_json::json!({"display":"LG HDR 4K","window_ref":"window:2"})
        );
        assert_eq!(
            mapped("window", &["display", "--list"]).method,
            "window.displays"
        );

        let error = control_command_for("window", &args(&["display"])).unwrap_err();
        assert_eq!(error.message, "window display requires a display name. Usage: cmux window display \"LG HDR 4K\"  (list names with: cmux window displays)");
    }

    #[test]
    fn maps_notification_mutation_commands() {
        assert_eq!(
            mapped("dismiss-notification", &["--id", "notification-1"]).params,
            serde_json::json!({"id": "notification-1"})
        );
        assert_eq!(
            mapped("dismiss-notification", &["--all-read"]).params,
            serde_json::json!({"all_read": true})
        );
        for tokens in [vec![], vec!["--id", "notification-1", "--all-read"]] {
            assert_eq!(
                control_command_for("dismiss-notification", &args(&tokens))
                    .unwrap_err()
                    .message,
                "dismiss-notification requires exactly one of --id or --all-read"
            );
        }
        assert_eq!(
            mapped("mark-notification-read", &["--id", "notification-1"]).params,
            serde_json::json!({"id": "notification-1"})
        );
        assert_eq!(
            mapped(
                "mark-notification-read",
                &["--workspace", "workspace:2", "--surface", "surface:3"]
            )
            .params,
            serde_json::json!({"workspace_ref": "workspace:2", "surface_ref": "surface:3"})
        );
        assert_eq!(
            mapped("mark-notification-read", &["--all"]).params,
            serde_json::json!({"all": true})
        );
        assert_eq!(
            control_command_for("mark-notification-read", &args(&["--surface", "surface:1"]))
                .unwrap_err()
                .message,
            "--surface requires --workspace"
        );
        assert_eq!(
            mapped("clear-notifications", &["--workspace", "workspace:2"]).params,
            serde_json::json!({"workspace_ref": "workspace:2"})
        );
        assert_eq!(
            mapped("clear-notifications", &[]).params,
            serde_json::json!({})
        );
        assert_eq!(
            mapped("open-notification", &["--id", "notification-1"]).params,
            serde_json::json!({"id": "notification-1"})
        );
        assert_eq!(
            control_command_for("open-notification", &[])
                .unwrap_err()
                .message,
            "open-notification requires --id"
        );
        assert_eq!(
            mapped("jump-to-unread", &[]).method,
            "notification.jump_to_unread"
        );
        let notify = mapped(
            "notify",
            &[
                "--title",
                "Build",
                "--subtitle",
                "Agent",
                "--body",
                "Needs input",
                "--workspace",
                "workspace:2",
                "--surface",
                "surface:3",
            ],
        );
        assert_eq!(notify.method, "notification.create");
        assert_eq!(
            notify.params,
            serde_json::json!({
                "title": "Build",
                "subtitle": "Agent",
                "body": "Needs input",
                "workspace_ref": "workspace:2",
                "surface_ref": "surface:3"
            })
        );
        assert_eq!(
            mapped("notify", &[]).params,
            serde_json::json!({"title": "Notification", "subtitle": "", "body": ""})
        );
    }

    #[test]
    fn maps_right_sidebar_commands_with_canonical_validation() {
        assert_eq!(
            mapped("right-sidebar", &["toggle"]).params,
            serde_json::json!({"action": "toggle"})
        );
        assert_eq!(
            mapped("right-sidebar", &["mode"]).params,
            serde_json::json!({"action": "mode"})
        );
        assert_eq!(
            mapped("right-sidebar", &["vault"]).params,
            serde_json::json!({"action": "set", "mode": "sessions", "focus": true})
        );
        assert_eq!(
            mapped("right-sidebar", &["set", "find", "--no-focus"]).params,
            serde_json::json!({"action": "set", "mode": "find", "focus": false})
        );
        assert_eq!(
            mapped(
                "right-sidebar",
                &["set", "files", "--workspace", "2", "--window", "window:3"]
            )
            .params,
            serde_json::json!({
                "action": "set",
                "mode": "files",
                "focus": true,
                "workspace_ref": "workspace:2",
                "window_ref": "window:3"
            })
        );

        for (tokens, message) in [
            (vec![], "right-sidebar requires a subcommand"),
            (
                vec!["set"],
                "right-sidebar set requires a mode: files, find, vault, sessions, feed, or dock",
            ),
            (vec!["set", "bogus"], "Unknown right-sidebar mode 'bogus'"),
            (
                vec!["show", "extra"],
                "right-sidebar show received unexpected arguments",
            ),
            (
                vec!["hide", "--no-focus"],
                "right-sidebar: --no-focus is only valid with set",
            ),
            (
                vec!["mode", "--unknown"],
                "right-sidebar: unknown flag '--unknown'",
            ),
            (
                vec!["show", "--workspace"],
                "right-sidebar: --workspace requires an id",
            ),
            (
                vec!["show", "--window"],
                "right-sidebar: --window requires an id",
            ),
            (
                vec!["set", "files", "--no-focus=false"],
                "right-sidebar: unknown flag '--no-focus=false'",
            ),
        ] {
            assert_eq!(
                control_command_for("right-sidebar", &args(&tokens))
                    .unwrap_err()
                    .message,
                message
            );
        }
    }

    #[test]
    fn p0_workspace_scoped_cli_aliases_accept_ambient_workspace_id() {
        for (command, tokens) in [
            ("send", vec!["hello"]),
            ("send-key", vec!["enter"]),
            ("send-panel", vec!["--panel", "surface:1", "hello"]),
            ("send-key-panel", vec!["--panel", "surface:1", "enter"]),
            ("new-split", vec![]),
            ("new-pane", vec![]),
            ("new-surface", vec![]),
            ("close-surface", vec![]),
            ("list-panes", vec![]),
            ("list-pane-surfaces", vec![]),
            ("list-panels", vec![]),
            ("focus-pane", vec!["surface:1"]),
            ("focus-panel", vec!["--panel", "surface:1"]),
            ("surface-health", vec![]),
            ("clear-history", vec![]),
            ("trigger-flash", vec![]),
            ("clear-notifications", vec![]),
        ] {
            let control = mapped(command, &tokens).with_ambient_workspace_id(Some("workspace-2"));
            assert_eq!(
                control.params.get("workspace_id"),
                Some(&serde_json::json!("workspace-2")),
                "{command} should carry ambient workspace scope"
            );
        }
    }

    #[test]
    fn pane_list_commands_route_to_distinct_canonical_methods() {
        let panes = mapped("list-panes", &["--workspace", "2", "--window", "window:1"]);
        assert_eq!(panes.method, "pane.list");
        assert_eq!(
            panes.params,
            serde_json::json!({"workspace_ref":"workspace:2", "window_ref":"window:1"})
        );

        let surfaces = mapped(
            "list-pane-surfaces",
            &["--workspace", "2", "--pane", "3", "--window", "1"],
        );
        assert_eq!(surfaces.method, "pane.surfaces");
        assert_eq!(
            surfaces.params,
            serde_json::json!({
                "workspace_ref":"workspace:2", "pane_ref":"pane:3", "window_ref":"window:1"
            })
        );
        assert_eq!(mapped("list-panels", &[]).method, "surface.list");
    }

    #[test]
    fn maps_workspace_list_and_current() {
        assert_eq!(mapped("list-workspaces", &[]).method, "workspace.list");
        assert_eq!(
            mapped("sidebar-snapshot", &[]).method,
            "extension.sidebar.snapshot"
        );
        assert_eq!(
            mapped("extension-sidebar-snapshot", &[]).method,
            "extension.sidebar.snapshot"
        );
        assert_eq!(
            mapped("workspace", &["current"]).method,
            "workspace.current"
        );
        assert_eq!(
            mapped("restore-previous-launch", &[]).method,
            "session.restore_previous_launch"
        );
        assert_eq!(
            mapped("workspace", &["restore-previous"]).method,
            "session.restore_previous_launch"
        );
        assert_eq!(
            mapped("restore-session", &[]).method,
            "session.restore_previous"
        );
        let error = control_command_for("restore-session", &args(&["--force"])).unwrap_err();
        assert_eq!(error.message, "restore-session: unknown flag '--force'");
    }

    #[test]
    fn maps_custom_sidebar_subcommands() {
        assert_eq!(mapped("sidebar", &[]).method, "sidebar.list");
        assert_eq!(mapped("sidebar", &["list"]).method, "sidebar.list");
        assert_eq!(
            mapped("sidebar", &["validate", "status-board"]).params,
            serde_json::json!({"name": "status-board"})
        );
        assert_eq!(
            mapped("sidebar", &["reload", "--name", "status-board"]).method,
            "sidebar.reload"
        );
        assert_eq!(
            mapped("sidebar", &["select", "status-board"]).method,
            "sidebar.select"
        );
        assert_eq!(
            mapped("sidebar", &["open", "status-board"]).params,
            serde_json::json!({"name": "status-board"})
        );
        assert_eq!(
            control_command_for("sidebar", &args(&["open"]))
                .unwrap_err()
                .message,
            "sidebar open requires a sidebar name"
        );
    }

    #[test]
    fn maps_workspace_create_options() {
        let command = mapped(
            "new-workspace",
            &["--cwd", "C:/repo", "--command=git status", "--env", "CI=1"],
        );
        assert_eq!(command.method, "workspace.create");
        assert_eq!(
            command.params,
            serde_json::json!({
                "working_directory": "C:/repo",
                "__post_create_command": "git status",
                "workspace_env": { "CI": "1" },
            })
        );
    }

    #[test]
    fn legacy_workspace_verbs_match_canonical_argument_contract() {
        assert_eq!(
            mapped("list-workspaces", &["--window", "window:2"]).params,
            serde_json::json!({"window_ref":"window:2"})
        );
        assert_eq!(
            mapped("current-workspace", &["--window", "window-id"]).params,
            serde_json::json!({"window_id":"window-id"})
        );
        assert_eq!(
            mapped(
                "close-workspace",
                &["--workspace", "0", "--window", "window:2"]
            )
            .params,
            serde_json::json!({"workspace_index":0,"window_ref":"window:2"})
        );
        assert_eq!(
            mapped(
                "select-workspace",
                &["--workspace", "workspace:3", "--window", "window-id"]
            )
            .params,
            serde_json::json!({"workspace_ref":"workspace:3","window_id":"window-id"})
        );
        assert_eq!(
            mapped(
                "rename-workspace",
                &["--window", "window:2", "--", "2", "Build"]
            )
            .params,
            serde_json::json!({"window_ref":"window:2","resolve_current_workspace":true,"title":"2 Build"})
        );
        assert_eq!(
            mapped(
                "new-workspace",
                &[
                    "--name",
                    "Build",
                    "--description",
                    "Review",
                    "--cwd",
                    "C:/repo",
                    "--env",
                    "A=file",
                    "--layout",
                    "{\"type\":\"terminal\"}",
                    "--focus",
                    "false",
                    "--group",
                    "workspace_group:2",
                    "--group-placement",
                    "end",
                    "--window",
                    "window:2",
                    "ignored"
                ]
            )
            .params,
            serde_json::json!({
                "title":"Build", "description":"Review", "working_directory":"C:/repo",
                "workspace_env":{"A":"file"}, "layout":{"type":"terminal"}, "focus":false,
                "group_id":"workspace_group:2", "group_placement":"end", "window_ref":"window:2"
            })
        );
    }

    #[test]
    fn legacy_workspace_required_flags_and_values_fail_before_socket() {
        assert_eq!(
            control_command_for("close-workspace", &args(&["workspace:1"]))
                .unwrap_err()
                .message,
            "close-workspace requires --workspace"
        );
        assert_eq!(
            control_command_for("select-workspace", &args(&["workspace:1"]))
                .unwrap_err()
                .message,
            "select-workspace requires --workspace"
        );
        assert_eq!(
            control_command_for("rename-workspace", &args(&[" "]))
                .unwrap_err()
                .message,
            "rename-workspace requires a title"
        );
        assert_eq!(
            control_command_for("new-workspace", &args(&["--focus", "yes"]))
                .unwrap_err()
                .message,
            "--focus must be true or false"
        );
        assert_eq!(
            control_command_for("new-workspace", &args(&["--layout", "[]"]))
                .unwrap_err()
                .message,
            "--layout must be a JSON object"
        );
        assert_eq!(
            control_command_for("new-workspace", &args(&["--bogus"]))
                .unwrap_err()
                .message,
            "new-workspace: unknown flag '--bogus'"
        );
    }

    #[test]
    fn verifier_create_expands_cwd_and_parses_dotenv_canonically() {
        let temp = tempfile::tempdir().unwrap();
        let env_path = temp.path().join("workspace.env");
        std::fs::write(&env_path, "export A='from file'\nB=\"two words\"\nEMPTY=\n").unwrap();
        let command = mapped(
            "new-workspace",
            &[
                "--cwd",
                ".",
                "--env-file",
                env_path.to_str().unwrap(),
                "--env",
                "A=override=value",
            ],
        );
        assert_eq!(
            command.params["working_directory"],
            serde_json::json!(std::env::current_dir().unwrap().to_string_lossy())
        );
        assert_eq!(
            command.params["workspace_env"],
            serde_json::json!({
                "A":"override=value", "B":"two words", "EMPTY":""
            })
        );
        assert_eq!(
            control_command_for("new-workspace", &args(&["--env", "BROKEN"]))
                .unwrap_err()
                .message,
            "new-workspace: --env entry 'BROKEN' must be in KEY=VALUE form"
        );
    }

    #[test]
    fn verifier_rename_selectors_preserve_canonical_intent() {
        assert_eq!(
            mapped("rename-workspace", &["--workspace", "uuid-a", "Build"]).params,
            serde_json::json!({"workspace_id":"uuid-a","title":"Build"})
        );
        assert_eq!(
            mapped("rename-workspace", &["--workspace", "workspace:2", "Build"]).params,
            serde_json::json!({"workspace_ref":"workspace:2","title":"Build"})
        );
        assert_eq!(
            mapped("rename-workspace", &["--workspace", "0", "Build"]).params,
            serde_json::json!({"workspace_index":0,"title":"Build"})
        );
        assert_eq!(
            mapped("rename-workspace", &["--window", "window:2", "Build"]).params,
            serde_json::json!({"window_ref":"window:2","resolve_current_workspace":true,"title":"Build"})
        );
    }

    #[test]
    fn bare_workspace_number_is_a_zero_based_index() {
        let command = mapped("select-workspace", &["--workspace", "2"]);
        assert_eq!(command.method, "workspace.select");
        assert_eq!(command.params, serde_json::json!({"workspace_index": 2}));
    }

    #[test]
    fn legacy_select_requires_workspace_flag() {
        assert_eq!(
            control_command_for("select-workspace", &args(&["--index", "0"]))
                .unwrap_err()
                .message,
            "select-workspace: unknown flag '--index'"
        );
        assert_eq!(
            mapped("select-workspace", &["--workspace", "workspace:1"]).params,
            serde_json::json!({"workspace_ref": "workspace:1"})
        );
    }

    #[test]
    fn explicit_surface_index_is_rejected_in_favor_of_refs() {
        assert_eq!(
            control_command_for("close-surface", &args(&["--index", "0"]))
                .unwrap_err()
                .message,
            "surface selectors require surface:N or surface id; --index is not supported"
        );
        assert_eq!(
            mapped("close-surface", &["surface:1"]).params,
            serde_json::json!({"surface_ref": "surface:1"})
        );
    }

    #[test]
    fn close_workspace_requires_explicit_target() {
        assert_eq!(
            control_command_for("close-workspace", &[])
                .unwrap_err()
                .message,
            "close-workspace requires --workspace"
        );
        assert_eq!(
            control_command_for("workspace", &args(&["close"]))
                .unwrap_err()
                .message,
            "workspace close requires a workspace target (workspace:N or workspace id)"
        );
        assert_eq!(
            control_command_for("close-workspace", &args(&["--index", "0"]))
                .unwrap_err()
                .message,
            "close-workspace: unknown flag '--index'"
        );
        assert_eq!(
            mapped("close-workspace", &["--workspace", "workspace:2"]).params,
            serde_json::json!({"workspace_ref": "workspace:2"})
        );
        assert_eq!(
            mapped("workspace", &["close", "--workspace", "workspace-2"]).params,
            serde_json::json!({"workspace_id": "workspace-2"})
        );
    }

    #[test]
    fn workspace_rename_requires_and_sends_title() {
        assert_eq!(
            mapped(
                "rename-workspace",
                &["--workspace", "ws-1", "--title", "Build"]
            )
            .params,
            serde_json::json!({"workspace_id": "ws-1", "title": "Build"})
        );
        assert_eq!(
            mapped("rename-workspace", &["2", "Build", "Lane"]).params,
            serde_json::json!({"resolve_current_workspace":true,"title": "2 Build Lane"})
        );
        assert_eq!(
            mapped("rename-workspace", &["Build"]).params,
            serde_json::json!({"resolve_current_workspace":true,"title": "Build"})
        );
        let error = control_command_for("rename-workspace", &[]).unwrap_err();
        assert_eq!(error.message, "rename-workspace requires a title");

        let alias = mapped("rename-window", &["2", "Build", "Lane"]);
        assert_eq!(alias.method, "workspace.rename");
        assert_eq!(
            alias.params,
            serde_json::json!({"resolve_current_workspace":true,"title": "2 Build Lane"})
        );
        let error = control_command_for("rename-window", &[]).unwrap_err();
        assert_eq!(error.message, "rename-window requires a title");
    }

    #[test]
    fn maps_workspace_metadata_commands() {
        assert_eq!(
            mapped("workspace", &["description", "2", "Needs", "review"]).params,
            serde_json::json!({"workspace_ref": "workspace:2", "description": "Needs review"})
        );
        assert_eq!(
            mapped("workspace", &["pin", "2"]).params,
            serde_json::json!({"workspace_ref": "workspace:2", "pinned": true})
        );
        assert_eq!(
            mapped("workspace", &["mark-read", "--workspace", "ws-1"]).params,
            serde_json::json!({"workspace_id": "ws-1", "unread": false})
        );
        assert_eq!(
            mapped("workspace", &["reset-color"]).method,
            "workspace.reset_color"
        );
        assert_eq!(
            mapped(
                "set-status",
                &[
                    "deploy",
                    "v1.2.3",
                    "--workspace",
                    "ws-1",
                    "--priority",
                    "80"
                ]
            )
            .params,
            serde_json::json!({
                "workspace_id": "ws-1",
                "key": "deploy",
                "value": "v1.2.3",
                "priority": 80,
            })
        );
        assert_eq!(
            mapped("clear-status", &["deploy", "--workspace", "ws-1"]).params,
            serde_json::json!({"workspace_id": "ws-1", "key": "deploy"})
        );
        assert_eq!(
            mapped("list-status", &["--workspace", "ws-1"]).method,
            "workspace.list_status"
        );
        assert_eq!(
            mapped(
                "set-agent-pid",
                &["codex.session-1", "1234", "--tab", "ws-1"]
            )
            .params,
            serde_json::json!({
                "workspace_id": "ws-1",
                "key": "codex.session-1",
                "pid": 1234,
            })
        );
        assert_eq!(
            mapped(
                "workspace",
                &[
                    "set-agent-pid",
                    "workspace:2",
                    "--key",
                    "codex.session-2",
                    "--pid",
                    "4321",
                ]
            )
            .params,
            serde_json::json!({
                "workspace_ref": "workspace:2",
                "key": "codex.session-2",
                "pid": 4321,
            })
        );
        assert_eq!(
            mapped(
                "clear-agent-pid",
                &["codex.session-1", "--workspace", "ws-1"]
            )
            .params,
            serde_json::json!({"workspace_id": "ws-1", "key": "codex.session-1"})
        );
        assert_eq!(
            mapped(
                "report-tty",
                &["ttys777", "--tab", "ws-1", "--panel", "surface-2"]
            )
            .params,
            serde_json::json!({
                "workspace_id": "ws-1",
                "panel_id": "surface-2",
                "tty": "ttys777",
            })
        );
        assert_eq!(
            mapped("surface", &["report-tty", "surface:2", "/dev/pts/7"]).params,
            serde_json::json!({
                "surface_ref": "surface:2",
                "tty": "/dev/pts/7",
            })
        );
        assert_eq!(
            mapped(
                "report-pr",
                &[
                    "42",
                    "https://github.com/manaflow-ai/cmux/pull/42",
                    "--tab",
                    "ws-1",
                    "--panel",
                    "surface-2",
                    "--label",
                    "PR",
                    "--state",
                    "merged",
                    "--branch",
                    "feature/api",
                    "--stale",
                ]
            )
            .params,
            serde_json::json!({
                "workspace_id": "ws-1",
                "panel_id": "surface-2",
                "number": 42,
                "url": "https://github.com/manaflow-ai/cmux/pull/42",
                "label": "PR",
                "state": "merged",
                "branch": "feature/api",
                "stale": true,
            })
        );
        assert_eq!(
            mapped(
                "workspace",
                &[
                    "report-review",
                    "workspace:2",
                    "7",
                    "https://gitlab.example/project/-/merge_requests/7",
                    "--label",
                    "MR",
                ]
            )
            .params,
            serde_json::json!({
                "workspace_ref": "workspace:2",
                "number": 7,
                "url": "https://gitlab.example/project/-/merge_requests/7",
                "label": "MR",
            })
        );
        assert_eq!(
            mapped("clear-pr", &["--tab", "ws-1", "--panel", "surface-2"]).params,
            serde_json::json!({"workspace_id": "ws-1", "panel_id": "surface-2"})
        );
        assert_eq!(
            mapped(
                "report-meta",
                &[
                    "task",
                    "review",
                    "--workspace",
                    "ws-1",
                    "--icon",
                    "text:CTX",
                    "--color",
                    "blue",
                    "--url",
                    "https://example.test/pr",
                    "--format",
                    "markdown",
                    "--priority",
                    "50",
                ]
            )
            .params,
            serde_json::json!({
                "workspace_id": "ws-1",
                "key": "task",
                "value": "review",
                "icon": "text:CTX",
                "color": "blue",
                "url": "https://example.test/pr",
                "format": "markdown",
                "priority": 50,
            })
        );
        assert_eq!(
            mapped("workspace", &["clear-meta", "task", "--workspace", "ws-1"]).params,
            serde_json::json!({"workspace_id": "ws-1", "key": "task"})
        );
        assert_eq!(
            mapped("list-meta", &["--workspace", "ws-1"]).method,
            "workspace.list_meta"
        );
        assert_eq!(
            mapped(
                "workspace",
                &[
                    "report-meta-block",
                    "notes",
                    "--workspace",
                    "ws-1",
                    "--priority",
                    "10",
                    "--",
                    "**Ready**",
                    "to",
                    "ship",
                ]
            )
            .params,
            serde_json::json!({
                "workspace_id": "ws-1",
                "key": "notes",
                "markdown": "**Ready** to ship",
                "priority": 10,
            })
        );
        assert_eq!(
            mapped("clear-meta-block", &["notes", "--workspace", "ws-1"]).params,
            serde_json::json!({"workspace_id": "ws-1", "key": "notes"})
        );
        assert_eq!(
            mapped("workspace", &["list-meta-blocks", "--workspace", "ws-1"]).method,
            "workspace.list_meta_blocks"
        );
        assert_eq!(
            mapped("reset-sidebar", &["--workspace", "ws-1"]).method,
            "workspace.reset_sidebar"
        );
        assert_eq!(
            mapped(
                "log",
                &["--workspace", "ws-1", "--level", "warn", "--", "ship it"]
            )
            .params,
            serde_json::json!({"workspace_id": "ws-1", "level": "warn", "message": "ship it"})
        );
        assert_eq!(
            mapped("list-log", &["--workspace", "ws-1", "--limit", "5"]).params,
            serde_json::json!({"workspace_id": "ws-1", "limit": 5})
        );
        assert_eq!(
            mapped("workspace", &["clear-log", "--workspace", "ws-1"]).method,
            "workspace.clear_log"
        );
    }

    #[test]
    fn maps_workspace_bulk_close_and_reorder_commands() {
        assert_eq!(
            mapped("close-workspaces", &["1", "workspace:3", "ws-2"]).params,
            serde_json::json!({
                "workspace_refs": ["workspace:1", "workspace:3"],
                "workspace_ids": ["ws-2"],
            })
        );
        let reorder = mapped("workspace", &["reorder", "3", "1", "--top-level-rows"]);
        assert_eq!(reorder.method, "workspace.reorder");
        assert_eq!(
            reorder.params,
            serde_json::json!({
                "workspace_ref": "workspace:3",
                "before_workspace_ref": "workspace:1",
                "uses_top_level_rows": true,
            })
        );
        assert_eq!(
            mapped("workspace", &["move", "--from", "ws-2", "--to", "2"]).params,
            serde_json::json!({"workspace_id": "ws-2", "before_workspace_ref": "workspace:2"})
        );
        assert_eq!(
            mapped(
                "workspace",
                &["move", "workspace:1", "--after", "workspace:3"]
            )
            .params,
            serde_json::json!({
                "workspace_ref": "workspace:1",
                "after_workspace_ref": "workspace:3",
            })
        );
        assert_eq!(
            control_command_for("workspace", &args(&["move", "workspace:1", "--to-index", "0"]))
                .unwrap_err()
                .message,
            "workspace reorder requires a before/after workspace ref or id; --to-index is not supported"
        );
    }

    #[test]
    fn maps_canonical_reorder_workspace_command() {
        let reorder = mapped(
            "reorder-workspace",
            &[
                "--workspace",
                "workspace:3",
                "--index",
                "0",
                "--window",
                "window:1",
                "--dry-run",
            ],
        );
        assert_eq!(reorder.method, "workspace.reorder");
        assert_eq!(
            reorder.params,
            serde_json::json!({
                "workspace_ref": "workspace:3",
                "index": 0,
                "window_ref": "window:1",
                "dry_run": true,
            })
        );
        assert_eq!(
            mapped(
                "reorder-workspace",
                &["ws-2", "--after-workspace", "workspace:1"]
            )
            .params,
            serde_json::json!({
                "workspace_id": "ws-2",
                "after_workspace_ref": "workspace:1",
            })
        );
    }

    #[test]
    fn maps_canonical_reorder_workspaces_command() {
        let reorder = mapped(
            "reorder-workspaces",
            &[
                "--order",
                "workspace:3, ws-1,2",
                "--window",
                "window:1",
                "--dry-run",
            ],
        );
        assert_eq!(reorder.method, "workspace.reorder_many");
        assert_eq!(
            reorder.params,
            serde_json::json!({
                "workspace_ids": ["workspace:3", "ws-1", "workspace:2"],
                "window_ref": "window:1",
                "dry_run": true,
            })
        );
        assert_eq!(
            control_command_for("reorder-workspaces", &args(&[]))
                .unwrap_err()
                .message,
            "reorder-workspaces requires --order <id|ref|index>,<id|ref|index>,..."
        );
        assert_eq!(
            control_command_for("reorder-workspaces", &args(&["--order="]))
                .unwrap_err()
                .message,
            "reorder-workspaces requires at least one workspace in --order"
        );
        assert_eq!(
            control_command_for("reorder-workspaces", &args(&["--order", "1,,2"]))
                .unwrap_err()
                .message,
            "reorder-workspaces --order cannot contain empty workspace refs"
        );
    }

    #[test]
    fn maps_canonical_reorder_surface_command() {
        let reorder = mapped(
            "reorder-surface",
            &[
                "surface:3",
                "--before-surface",
                "surface:1",
                "--workspace",
                "workspace:2",
                "--window",
                "window:1",
                "--focus",
                "true",
            ],
        );
        assert_eq!(reorder.method, "surface.reorder");
        assert_eq!(
            reorder.params,
            serde_json::json!({
                "surface_ref": "surface:3",
                "before_surface_ref": "surface:1",
                "workspace_ref": "workspace:2",
                "window_ref": "window:1",
                "focus": true,
            })
        );
    }

    #[test]
    fn maps_canonical_move_surface_command() {
        let moved = mapped(
            "move-surface",
            &[
                "surface:3",
                "--pane",
                "pane:2",
                "--workspace",
                "workspace:2",
                "--window",
                "window:1",
                "--after-surface",
                "surface:1",
                "--index",
                "9",
                "--focus",
                "yes",
            ],
        );
        assert_eq!(moved.method, "surface.move");
        assert_eq!(
            moved.params,
            serde_json::json!({
                "surface_ref": "surface:3",
                "pane_ref": "pane:2",
                "workspace_ref": "workspace:2",
                "window_ref": "window:1",
                "after_surface_ref": "surface:1",
                "index": 9,
                "focus": true,
            })
        );
        assert_eq!(
            control_command_for("move-surface", &args(&["surface:1", "--focus", "maybe"]))
                .unwrap_err()
                .message,
            "--focus must be true|false"
        );
    }

    #[test]
    fn maps_canonical_move_workspace_to_window_command() {
        let moved = mapped(
            "move-workspace-to-window",
            &["--workspace", "2", "--window", "window:3"],
        );
        assert_eq!(moved.method, "workspace.move_to_window");
        assert_eq!(
            moved.params,
            serde_json::json!({
                "workspace_ref": "workspace:2",
                "window_ref": "window:3",
            })
        );
        assert_eq!(
            control_command_for("move-workspace-to-window", &args(&["--window", "window:1"]))
                .unwrap_err()
                .message,
            "move-workspace-to-window requires --workspace"
        );
        assert_eq!(
            control_command_for(
                "move-workspace-to-window",
                &args(&["--workspace", "workspace:1"])
            )
            .unwrap_err()
            .message,
            "move-workspace-to-window requires --window"
        );
    }

    #[test]
    fn maps_canonical_split_off_command() {
        let split = mapped(
            "split-off",
            &[
                "--surface",
                "surface:2",
                "--workspace",
                "workspace:3",
                "--window",
                "window:1",
                "right",
            ],
        );
        assert_eq!(split.method, "surface.split_off");
        assert_eq!(
            split.params,
            serde_json::json!({
                "surface_ref": "surface:2",
                "workspace_ref": "workspace:3",
                "window_ref": "window:1",
                "direction": "right",
                "focus": false,
            })
        );
        assert_eq!(
            control_command_for("split-off", &args(&["--surface", "surface:1"]))
                .unwrap_err()
                .message,
            "split-off requires a direction"
        );
        assert_eq!(
            control_command_for("split-off", &args(&["--surface", "surface:1", "diagonal"]))
                .unwrap_err()
                .message,
            "split-off: direction must be left|right|up|down"
        );
    }

    #[test]
    fn maps_canonical_drag_surface_to_split_command() {
        let drag = mapped(
            "drag-surface-to-split",
            &["--panel", "surface:2", "u", "--focus", "true"],
        );
        assert_eq!(drag.method, "surface.drag_to_split");
        assert_eq!(
            drag.params,
            serde_json::json!({
                "surface_ref": "surface:2",
                "direction": "u",
                "focus": true,
            })
        );
    }

    #[test]
    fn maps_canonical_swap_pane_command() {
        let swap = mapped(
            "swap-pane",
            &[
                "--pane",
                "pane:1",
                "--target-pane",
                "2",
                "--workspace",
                "workspace:3",
                "--window",
                "window:1",
                "--focus",
                "true",
            ],
        );
        assert_eq!(swap.method, "pane.swap");
        assert_eq!(
            swap.params,
            serde_json::json!({
                "pane_ref": "pane:1",
                "target_pane_ref": "pane:2",
                "workspace_ref": "workspace:3",
                "window_ref": "window:1",
                "focus": true,
            })
        );
        assert_eq!(
            control_command_for("swap-pane", &args(&["--target-pane", "pane:2"]))
                .unwrap_err()
                .message,
            "swap-pane requires --pane"
        );
    }

    #[test]
    fn maps_canonical_break_pane_command_and_focus_alias() {
        let broken = mapped(
            "break-pane",
            &[
                "--pane",
                "2",
                "--surface",
                "surface:3",
                "--workspace",
                "workspace:1",
                "--window",
                "window:2",
                "--no-focus",
            ],
        );
        assert_eq!(broken.method, "pane.break");
        assert_eq!(
            broken.params,
            serde_json::json!({
                "pane_ref": "pane:2",
                "surface_ref": "surface:3",
                "workspace_ref": "workspace:1",
                "window_ref": "window:2",
                "focus": false,
            })
        );
        assert_eq!(
            mapped("break-pane", &[]).params,
            serde_json::json!({"focus": false})
        );
        assert_eq!(
            control_command_for("break-pane", &args(&["--focus", "true", "--no-focus"]),)
                .unwrap_err()
                .message,
            "--focus and --no-focus cannot be used together"
        );
    }

    #[test]
    fn maps_canonical_join_pane_command_and_requires_target() {
        let joined = mapped(
            "join-pane",
            &[
                "--target-pane",
                "pane:3",
                "--pane",
                "2",
                "--surface",
                "surface-id",
                "--workspace",
                "1",
                "--focus",
                "true",
            ],
        );
        assert_eq!(joined.method, "pane.join");
        assert_eq!(
            joined.params,
            serde_json::json!({
                "target_pane_ref": "pane:3",
                "pane_ref": "pane:2",
                "surface_id": "surface-id",
                "workspace_ref": "workspace:1",
                "focus": true,
            })
        );
        assert_eq!(
            control_command_for("join-pane", &args(&[]))
                .unwrap_err()
                .message,
            "join-pane requires --target-pane"
        );
    }

    #[test]
    fn maps_canonical_last_pane_command() {
        let last = mapped("last-pane", &["--workspace", "2", "--window", "window:1"]);
        assert_eq!(last.method, "pane.last");
        assert_eq!(
            last.params,
            serde_json::json!({
                "workspace_ref": "workspace:2",
                "window_ref": "window:1",
            })
        );
    }

    #[test]
    fn maps_canonical_last_window_command() {
        let command = mapped("last-window", &["--window", "2"]);
        assert_eq!(command.method, "workspace.last");
        assert_eq!(command.params, serde_json::json!({"window_ref":"window:2"}));
    }

    #[test]
    fn maps_canonical_resize_pane_command_and_direction_precedence() {
        let resized = mapped(
            "resize-pane",
            &[
                "--pane",
                "2",
                "--workspace",
                "workspace:3",
                "--window",
                "window:1",
                "-D",
                "-L",
                "--amount",
                "12",
            ],
        );
        assert_eq!(resized.method, "pane.resize");
        assert_eq!(
            resized.params,
            serde_json::json!({
                "pane_ref": "pane:2",
                "workspace_ref": "workspace:3",
                "window_ref": "window:1",
                "direction": "left",
                "amount": 12,
            })
        );
        assert_eq!(
            mapped("resize-pane", &["--amount", "not-a-number"]).params,
            serde_json::json!({"direction": "right", "amount": 1})
        );
        assert_eq!(
            control_command_for("resize-pane", &args(&["--amount", "0"]))
                .unwrap_err()
                .message,
            "--amount must be greater than 0"
        );
    }

    #[test]
    fn maps_workspace_group_collapse_commands() {
        let collapse = mapped("workspace", &["group", "collapse", "group-1"]);
        assert_eq!(collapse.method, "workspace.group.collapse");
        assert_eq!(collapse.params, serde_json::json!({"group_id": "group-1"}));
        let expand = mapped("workspace", &["group", "expand", "--group", "group-1"]);
        assert_eq!(expand.method, "workspace.group.expand");
        assert_eq!(expand.params, serde_json::json!({"group_id": "group-1"}));
    }

    #[test]
    fn maps_surface_browser_and_markdown() {
        assert_eq!(
            mapped(
                "open-browser",
                &["--panel", "surface-1", "https://example.com"]
            )
            .params,
            serde_json::json!({"panel_id": "surface-1", "url": "https://example.com"})
        );
        assert_eq!(
            mapped("markdown", &["--path", "README.md"]).params,
            serde_json::json!({"file_path": "README.md"})
        );
    }

    #[test]
    fn maps_diff_surface_open() {
        assert_eq!(mapped("diff", &[]).method, "surface.open_diff");
        assert_eq!(mapped("diff", &[]).params, serde_json::json!({}));
        assert_eq!(
            mapped(
                "surface",
                &[
                    "diff",
                    "--surface",
                    "surface-1",
                    "--token",
                    "tok-abcdef0123456789",
                    "--request-path",
                    "/review/index.html",
                ],
            )
            .params,
            serde_json::json!({
                "surface_id": "surface-1",
                "token": "tok-abcdef0123456789",
                "request_path": "/review/index.html",
            })
        );
    }

    #[test]
    fn maps_surface_metadata_commands() {
        assert_eq!(
            mapped("rename-tab", &["API", "logs"]).params,
            serde_json::json!({"title": "API logs"})
        );
        assert_eq!(
            mapped(
                "surface",
                &["rename", "--surface", "surface-1", "--title", "Runner"]
            )
            .params,
            serde_json::json!({"surface_id": "surface-1", "title": "Runner"})
        );
        assert_eq!(
            mapped("surface", &["pin", "--surface", "surface-1"]).params,
            serde_json::json!({"surface_id": "surface-1", "pinned": true})
        );
        assert_eq!(
            mapped("surface", &["mark-read", "--surface", "surface-1"]).params,
            serde_json::json!({"surface_id": "surface-1", "unread": false})
        );
        assert_eq!(
            mapped("move-tab-to-new-workspace", &["--surface", "surface-1"]).method,
            "surface.move_to_new_workspace"
        );
    }

    #[test]
    fn maps_surface_split_and_type() {
        assert_eq!(
            mapped(
                "surface",
                &[
                    "split",
                    "--surface",
                    "surface-1",
                    "--direction",
                    "down",
                    "--command",
                    "npm test",
                    "--input",
                    "hello",
                    "--env",
                    "CI=1",
                ]
            )
            .params,
            serde_json::json!({
                "surface_id": "surface-1",
                "direction": "down",
                "initial_terminal_command": "npm test",
                "initial_terminal_input": "hello",
                "initial_terminal_environment": {
                    "CI": "1",
                },
            })
        );
        assert_eq!(
            mapped(
                "new-terminal-tab",
                &[
                    "--surface",
                    "surface-1",
                    "--input",
                    "codex\n",
                    "--env",
                    "FEATURE=enabled",
                ]
            )
            .params,
            serde_json::json!({
                "surface_id": "surface-1",
                "initial_terminal_input": "codex\n",
                "initial_terminal_environment": {
                    "FEATURE": "enabled",
                },
            })
        );
        assert_eq!(
            mapped(
                "surface",
                &[
                    "split-browser",
                    "--surface",
                    "surface-1",
                    "--direction",
                    "right",
                    "--url",
                    "https://example.com",
                ]
            )
            .params,
            serde_json::json!({
                "surface_id": "surface-1",
                "direction": "right",
                "url": "https://example.com",
            })
        );
        assert_eq!(
            mapped(
                "surface",
                &["set-type", "browser", "--surface", "surface-1"]
            )
            .params,
            serde_json::json!({"surface_id": "surface-1", "type": "browser"})
        );
    }

    #[test]
    fn maps_legacy_surface_aliases() {
        assert_eq!(mapped("list-panes", &[]).method, "pane.list");
        assert_eq!(mapped("focus-pane", &["pane:2"]).method, "pane.focus");
        assert_eq!(
            mapped(
                "focus-pane",
                &["--pane", "2", "--workspace", "workspace:3", "--window", "1"]
            )
            .params,
            serde_json::json!({
                "pane_ref": "pane:2",
                "workspace_ref": "workspace:3",
                "window_ref": "window:1",
            })
        );
        assert_eq!(mapped("new-pane", &[]).method, "surface.split");
        assert_eq!(
            mapped("new-surface", &[]).method,
            "surface.new_terminal_tab"
        );
        assert_eq!(mapped("surface-health", &[]).method, "surface.health");
        assert_eq!(
            mapped("debug-terminals", &["--workspace", "workspace-1"]).params,
            serde_json::json!({"workspace_id": "workspace-1"})
        );
        assert_eq!(
            mapped(
                "report-shell-state",
                &[
                    "running",
                    "--workspace",
                    "workspace-1",
                    "--panel",
                    "surface-2"
                ]
            )
            .params,
            serde_json::json!({
                "workspace_id": "workspace-1",
                "panel_id": "surface-2",
                "state": "running"
            })
        );
        assert_eq!(
            mapped("surface", &["report-shell-state", "surface:2", "prompt"]).params,
            serde_json::json!({
                "surface_ref": "surface:2",
                "state": "prompt"
            })
        );
        assert_eq!(
            mapped("focus-panel", &["--panel", "surface:2"]).params,
            serde_json::json!({"surface_ref": "surface:2"})
        );
        assert_eq!(
            mapped("focus-pane", &["surface-2"]).params,
            serde_json::json!({"pane_id": "surface-2"})
        );
    }

    #[test]
    fn maps_send_commands() {
        assert_eq!(
            mapped("send", &["hello\\nthere"]).params,
            serde_json::json!({"text": "hello\nthere"})
        );
        assert_eq!(
            mapped("send-key", &["ctrl+c"]).params,
            serde_json::json!({"key": "ctrl+c"})
        );
        assert_eq!(
            mapped(
                "send-panel",
                &["--workspace", "2", "--panel", "surface:3", "hello"]
            )
            .params,
            serde_json::json!({
                "workspace_ref": "workspace:2",
                "surface_ref": "surface:3",
                "text": "hello",
            })
        );
        assert_eq!(
            mapped(
                "send-key-panel",
                &[
                    "--workspace",
                    "workspace-2",
                    "--panel",
                    "surface-3",
                    "enter"
                ]
            )
            .params,
            serde_json::json!({
                "workspace_id": "workspace-2",
                "panel_id": "surface-3",
                "key": "enter",
            })
        );
    }

    #[test]
    fn send_panel_commands_require_panel_and_payload() {
        assert_eq!(
            control_command_for("send-panel", &args(&["hello"]))
                .unwrap_err()
                .message,
            "send-panel requires --panel"
        );
        assert_eq!(
            control_command_for("send-key-panel", &args(&["--panel", "surface:2"]))
                .unwrap_err()
                .message,
            "send-key-panel requires a key"
        );
    }

    #[test]
    fn rejects_invalid_terminal_environment_assignment() {
        let error = control_command_for("new-terminal-tab", &args(&["--env", "CI"]))
            .expect_err("invalid env assignment should fail");
        assert_eq!(
            error.message,
            "invalid environment assignment: CI (expected KEY=VALUE)"
        );
    }

    #[test]
    fn maps_browser_top_level_commands() {
        assert_eq!(
            mapped("browser-back", &["--surface", "surface-1"]).method,
            "browser.back"
        );
        assert_eq!(
            mapped("browser-forward", &["--surface", "surface-1"]).params,
            serde_json::json!({"surface_id": "surface-1"})
        );
        for (command, method) in [
            ("browser-reload", "browser.reload"),
            ("get-url", "browser.url.get"),
            ("focus-webview", "browser.focus_webview"),
            ("is-webview-focused", "browser.is_webview_focused"),
        ] {
            let control = mapped(command, &["--panel", "surface:1"]);
            assert_eq!(control.method, method);
            assert_eq!(
                control.params,
                serde_json::json!({"surface_ref": "surface:1"})
            );
        }
        assert_eq!(
            mapped("navigate", &["https://example.com"]).params,
            serde_json::json!({"url": "https://example.com"})
        );
        assert_eq!(
            mapped("new-browser-workspace", &["https://example.com"]).params,
            serde_json::json!({"url": "https://example.com"})
        );
        assert_eq!(
            mapped("new-browser-workspace", &["https://example.com"]).method,
            "workspace.create_browser"
        );
        assert_eq!(
            mapped("reopen-closed-browser-tab", &[]).method,
            "browser.reopen_closed"
        );
    }

    #[test]
    fn maps_browser_subcommands() {
        assert_eq!(
            mapped(
                "browser",
                &["new-workspace", "--url", "https://example.com"]
            )
            .params,
            serde_json::json!({"url": "https://example.com"})
        );
        assert_eq!(
            mapped("browser", &["reopen-closed"]).method,
            "browser.reopen_closed"
        );
        assert_eq!(
            mapped("browser", &["split-right", "https://example.com"]).params,
            serde_json::json!({"direction": "right", "url": "https://example.com"})
        );
        assert_eq!(
            mapped(
                "browser",
                &["navigate", "--surface", "surface-1", "https://example.com"]
            )
            .method,
            "browser.navigate"
        );
        assert_eq!(
            mapped("browser", &["open-split", "https://example.com"]).method,
            "browser.open_split"
        );
        assert_eq!(
            mapped("browser", &["clear-history", "--surface", "surface-1"]).method,
            "browser.clear_history"
        );
        assert_eq!(
            mapped("browser", &["show-devtools", "--devtools-panel", "console"]).params,
            serde_json::json!({"panel": "console"})
        );
        assert_eq!(
            mapped(
                "browser",
                &[
                    "network",
                    "surface:2",
                    "--workspace",
                    "workspace:1",
                    "--url-contains",
                    "example.com",
                    "--method",
                    "POST",
                    "--since-id",
                    "browser-network-4",
                    "--limit",
                    "25",
                ]
            )
            .params,
            serde_json::json!({
                "workspace_ref": "workspace:1",
                "surface_ref": "surface:2",
                "url_contains": "example.com",
                "method": "POST",
                "since_id": "browser-network-4",
                "limit": 25,
            })
        );
        assert_eq!(
            mapped("browser", &["requests", "--panel", "surface-1"]).method,
            "browser.network.requests"
        );
        assert_eq!(
            mapped(
                "browser",
                &["--surface", "surface-1", "network", "requests"]
            )
            .params,
            serde_json::json!({"surface_id": "surface-1"})
        );
        assert_eq!(
            mapped("browser", &["network", "requests"]).params,
            serde_json::json!({})
        );
        assert_eq!(
            mapped("browser", &["network", "clear", "surface:2"]).method,
            "browser.network.clear"
        );
        assert_eq!(
            mapped(
                "browser",
                &[
                    "network-clear",
                    "--workspace",
                    "workspace:1",
                    "--surface",
                    "surface:2"
                ]
            )
            .params,
            serde_json::json!({
                "workspace_ref": "workspace:1",
                "surface_ref": "surface:2",
            })
        );
        assert_eq!(
            mapped(
                "browser",
                &["zoom", "--surface", "surface-1", "--scale", "1.25"]
            )
            .params,
            serde_json::json!({"surface_id": "surface-1", "zoom": "1.25"})
        );
        assert_eq!(
            mapped("browser", &["surface-1", "reload"]).method,
            "browser.reload"
        );
        assert_eq!(
            mapped("browser", &["surface-1", "url"]).params,
            serde_json::json!({"surface_id": "surface-1"})
        );
        assert_eq!(
            mapped("browser", &["focus-webview", "--surface", "surface-1"]).method,
            "browser.focus_webview"
        );
        assert_eq!(
            mapped("browser", &["surface-1", "is-webview-focused"]).method,
            "browser.is_webview_focused"
        );
        assert_eq!(
            control_command_for("browser", &args(&["network", "--limit", "abc"]))
                .unwrap_err()
                .message,
            "browser network --limit requires a non-negative integer: abc"
        );
        assert_eq!(
            mapped("browser", &["surface-1", "viewport", "800", "600"]).params,
            serde_json::json!({
                "surface_id": "surface-1",
                "width": "800",
                "height": "600",
            })
        );
        assert_eq!(
            mapped(
                "browser",
                &["--surface", "surface-1", "viewport", "800", "600"]
            )
            .method,
            "browser.viewport.set"
        );
        assert_eq!(
            mapped("browser", &["surface-1", "geo", "37.7", "-122.4"]).params,
            serde_json::json!({
                "surface_id": "surface-1",
                "latitude": "37.7",
                "longitude": "-122.4",
            })
        );
        assert_eq!(
            mapped("browser", &["--surface", "surface-1", "offline", "true"]).method,
            "browser.offline.set"
        );
        assert_eq!(
            mapped("browser", &["--surface", "surface-1", "trace", "start"]).method,
            "browser.trace.start"
        );
        assert_eq!(
            mapped("browser", &["surface-1", "screencast", "stop"]).method,
            "browser.screencast.stop"
        );
        assert_eq!(
            mapped(
                "browser",
                &["--surface", "surface-1", "input", "keyboard", "Enter"]
            )
            .method,
            "browser.input_keyboard"
        );
    }

    #[test]
    fn maps_agent_browser_style_cli_families() {
        assert_eq!(
            mapped(
                "browser",
                &["surface:1", "wait", "--load-state", "complete"]
            )
            .params,
            serde_json::json!({"surface_ref": "surface:1", "load_state": "complete"})
        );
        assert_eq!(
            mapped(
                "browser",
                &[
                    "surface:1",
                    "goto",
                    "https://example.com",
                    "--snapshot-after"
                ]
            )
            .params,
            serde_json::json!({
                "surface_ref": "surface:1",
                "url": "https://example.com",
                "snapshot_after": true,
            })
        );
        assert_eq!(
            mapped("browser", &["surface:1", "eval", "1 + 2"]).method,
            "browser.eval"
        );
        assert_eq!(
            mapped("browser", &["surface:1", "click", "#submit"]).params,
            serde_json::json!({"surface_ref": "surface:1", "selector": "#submit"})
        );
        assert_eq!(
            mapped("browser", &["018f-cmux-surface", "click", "#submit"]).params,
            serde_json::json!({
                "surface_id": "018f-cmux-surface",
                "selector": "#submit",
            })
        );
        assert_eq!(
            mapped(
                "browser",
                &[
                    "surface:1",
                    "fill",
                    "#name",
                    "--text",
                    "",
                    "--snapshot-after"
                ]
            )
            .params,
            serde_json::json!({
                "surface_ref": "surface:1",
                "selector": "#name",
                "text": "",
                "snapshot_after": true,
            })
        );
        assert_eq!(
            mapped("browser", &["surface:1", "get", "attr", "#name", "data-id"]).params,
            serde_json::json!({
                "surface_ref": "surface:1",
                "selector": "#name",
                "attr": "data-id",
            })
        );
        assert_eq!(
            mapped("browser", &["surface:1", "find", "text", "hello"]).method,
            "browser.find.text"
        );
        assert_eq!(
            mapped("browser", &["surface:1", "frame", "main"]).method,
            "browser.frame.main"
        );
        assert_eq!(
            mapped(
                "browser",
                &["surface:1", "dialog", "accept", "--text", "ok"]
            )
            .params,
            serde_json::json!({"surface_ref": "surface:1", "text": "ok"})
        );
        assert_eq!(
            mapped(
                "browser",
                &["surface:1", "download", "wait", "C:/tmp/file.txt"]
            )
            .method,
            "browser.download.wait"
        );
        assert_eq!(
            mapped(
                "browser",
                &["surface:1", "cookies", "set", "cmux", "cookie"]
            )
            .params,
            serde_json::json!({
                "surface_ref": "surface:1",
                "name": "cmux",
                "value": "cookie",
            })
        );
        assert_eq!(
            mapped(
                "browser",
                &["surface:1", "storage", "local", "set", "alpha", "one"]
            )
            .params,
            serde_json::json!({
                "surface_ref": "surface:1",
                "type": "local",
                "key": "alpha",
                "value": "one",
            })
        );
        assert_eq!(
            mapped("browser", &["surface:1", "tab", "switch", "surface:2"]).params,
            serde_json::json!({
                "surface_ref": "surface:1",
                "target_surface_id": "surface:2",
            })
        );
        assert_eq!(
            mapped("browser", &["surface:1", "addscript", "1 + 2"]).params,
            serde_json::json!({"surface_ref": "surface:1", "script": "1 + 2"})
        );
        assert_eq!(
            mapped(
                "browser",
                &["surface:1", "addstyle", "body { color: red; }"]
            )
            .params,
            serde_json::json!({"surface_ref": "surface:1", "css": "body { color: red; }"})
        );
    }

    #[test]
    fn maps_terminal_text_capture_commands() {
        for command in ["read-screen", "capture-pane"] {
            let control = mapped(
                command,
                &[
                    "--workspace",
                    "workspace:2",
                    "--surface",
                    "surface:3",
                    "--window",
                    "window:1",
                    "--scrollback",
                    "--lines",
                    "200",
                ],
            );
            assert_eq!(control.method, "surface.read_text");
            assert_eq!(
                control.params,
                serde_json::json!({
                    "workspace_ref": "workspace:2",
                    "surface_ref": "surface:3",
                    "window_ref": "window:1",
                    "scrollback": true,
                    "lines": 200,
                })
            );
        }

        let error = control_command_for("read-screen", &args(&["--lines", "0"]))
            .expect_err("zero line count must fail");
        assert!(error.message.contains("greater than 0"));

        let clear = mapped(
            "clear-history",
            &["--workspace", "workspace:2", "--surface", "surface:3"],
        );
        assert_eq!(clear.method, "surface.clear_history");
        assert_eq!(
            clear.params,
            serde_json::json!({"workspace_ref": "workspace:2", "surface_ref": "surface:3"})
        );

        let flash = mapped(
            "trigger-flash",
            &["--workspace", "workspace:2", "--surface", "surface:3"],
        );
        assert_eq!(flash.method, "surface.trigger_flash");
        assert_eq!(
            flash.params,
            serde_json::json!({"workspace_ref": "workspace:2", "surface_ref": "surface:3"})
        );
    }
}
