//! Pure CLI command -> v2 control-socket method mapping.
//!
//! The desktop backend already exposes a focused set of workspace/surface v2
//! methods. This module gives the user-facing CLI spellings a real route to
//! those methods instead of falling through to the generic "not yet ported"
//! socket failure.

use crate::invocation::CliError;
use std::collections::BTreeMap;

mod browser;
mod workspace;

use browser::{
    browser_subcommand, browser_workspace_params, surface_open_browser_params,
    surface_split_browser_params,
};

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
            workspace::legacy_workspace_scope_params(args, "list-workspaces")?,
        )),
        "current-workspace" => Some(ControlCommand::new(
            "workspace.current",
            workspace::legacy_workspace_scope_params(args, "current-workspace")?,
        )),
        "new-workspace" => Some(ControlCommand::new(
            "workspace.create",
            workspace::legacy_workspace_create_params(args)?,
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
            workspace::workspace_close_params(args, "close-workspace")?,
        )),
        "close-workspaces" => Some(ControlCommand::new(
            "workspace.close_many",
            workspace::workspace_multi_selector_params(args)?,
        )),
        "reorder-workspace" => Some(ControlCommand::new(
            "workspace.reorder",
            workspace::canonical_workspace_reorder_params(args)?,
        )),
        "reorder-workspaces" => Some(ControlCommand::new(
            "workspace.reorder_many",
            workspace::canonical_workspaces_reorder_params(args)?,
        )),
        "move-workspace-to-window" => Some(ControlCommand::new(
            "workspace.move_to_window",
            workspace::move_workspace_to_window_params(args)?,
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
        "last-window" | "next-window" | "previous-window" => Some(ControlCommand::new(
            match command {
                "next-window" => "workspace.next",
                "previous-window" => "workspace.previous",
                _ => "workspace.last",
            },
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
            workspace::legacy_workspace_target_params(args, "select-workspace")?,
        )),
        "rename-workspace" | "rename-window" => Some(ControlCommand::new(
            "workspace.rename",
            workspace::legacy_workspace_rename_params(args, command)?,
        )),
        "set-progress" => Some(ControlCommand::new(
            "workspace.set_progress",
            workspace::workspace_progress_params(args)?,
        )),
        "clear-progress" => Some(ControlCommand::new(
            "workspace.clear_progress",
            workspace::workspace_selector_params(args)?,
        )),
        "set-status" => Some(ControlCommand::new(
            "workspace.set_status",
            workspace::workspace_status_params(args)?,
        )),
        "clear-status" => Some(ControlCommand::new(
            "workspace.clear_status",
            workspace::workspace_status_key_params(args, "clear-status")?,
        )),
        "list-status" => Some(ControlCommand::new(
            "workspace.list_status",
            workspace::workspace_selector_params(args)?,
        )),
        "set-agent-pid" | "set_agent_pid" => Some(ControlCommand::new(
            "workspace.set_agent_pid",
            workspace::workspace_agent_pid_params(args, "set-agent-pid")?,
        )),
        "clear-agent-pid" | "clear_agent_pid" => Some(ControlCommand::new(
            "workspace.clear_agent_pid",
            workspace::workspace_status_key_params(args, "clear-agent-pid")?,
        )),
        "report-pr" | "report_pr" => Some(ControlCommand::new(
            "workspace.report_pr",
            workspace::workspace_pr_params(args, "report-pr")?,
        )),
        "report-review" | "report_review" => Some(ControlCommand::new(
            "workspace.report_review",
            workspace::workspace_pr_params(args, "report-review")?,
        )),
        "clear-pr" | "clear_pr" => Some(ControlCommand::new(
            "workspace.clear_pr",
            surface_selector_params(args)?,
        )),
        "report-meta" | "set-meta" => Some(ControlCommand::new(
            "workspace.report_meta",
            workspace::workspace_meta_params(args, "report-meta")?,
        )),
        "clear-meta" => Some(ControlCommand::new(
            "workspace.clear_meta",
            workspace::workspace_status_key_params(args, "clear-meta")?,
        )),
        "list-meta" => Some(ControlCommand::new(
            "workspace.list_meta",
            workspace::workspace_selector_params(args)?,
        )),
        "report-meta-block" | "set-meta-block" => Some(ControlCommand::new(
            "workspace.report_meta_block",
            workspace::workspace_meta_block_params(args, "report-meta-block")?,
        )),
        "clear-meta-block" => Some(ControlCommand::new(
            "workspace.clear_meta_block",
            workspace::workspace_status_key_params(args, "clear-meta-block")?,
        )),
        "list-meta-blocks" => Some(ControlCommand::new(
            "workspace.list_meta_blocks",
            workspace::workspace_selector_params(args)?,
        )),
        "reset-sidebar" => Some(ControlCommand::new(
            "workspace.reset_sidebar",
            workspace::workspace_selector_params(args)?,
        )),
        "log" => Some(ControlCommand::new(
            "workspace.log",
            workspace::workspace_log_params(args)?,
        )),
        "clear-log" => Some(ControlCommand::new(
            "workspace.clear_log",
            workspace::workspace_selector_params(args)?,
        )),
        "list-log" => Some(ControlCommand::new(
            "workspace.list_log",
            workspace::workspace_list_log_params(args)?,
        )),
        "sidebar-state" => Some(ControlCommand::new(
            "workspace.sidebar_state",
            workspace::workspace_selector_params(args)?,
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
            workspace::workspace_action_params(args)?,
        )),
        "workspace-group" => Some(workspace::workspace_group_subcommand(args)?),
        "respawn-pane" => Some(ControlCommand::new(
            "surface.respawn",
            respawn_pane_params(args)?,
        )),
        "move-tab-to-new-workspace" => Some(ControlCommand::new(
            "surface.move_to_new_workspace",
            surface_selector_params(args)?,
        )),
        "workspace" => workspace::workspace_subcommand(args)?,
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
            workspace::workspace_selector_params(args)?,
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

include!("command_forward/arguments.rs");

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

    include!("command_forward/navigation_tests.rs");

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
