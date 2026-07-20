use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use cmux_core::session::{
    AppSessionSnapshot, SessionPaneLayoutSnapshot, SessionPanelShellActivityStateSnapshot,
    SessionPullRequestStatusSnapshot, SessionSplitOrientation, SessionSurfaceKindSnapshot,
    SessionWindowSnapshot, SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot,
};
use cmux_core::session_ops;
use cmux_ipc::{ControlCallResult, ControlRequest, ControlStream, JsonValue};
use cmux_workspaces::{WorkspaceBatchReorderError, WorkspaceReorderPlanItem};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use time::OffsetDateTime;
use uuid::Uuid;

mod browser_control;
mod custom_sidebar_action;
mod event_stream;
mod lifecycle_dispatch;
mod pane_surface_control;
mod pane_surface_lifecycle;
mod payloads;
mod terminal_runtime_v2;
#[cfg(test)]
#[path = "control_socket/unit_tests.rs"]
mod tests;
mod window_lifecycle;
mod workspace_control;
use browser_control::*;
#[cfg(test)]
use custom_sidebar_action::{
    custom_sidebar_action_denied, custom_sidebar_action_reply,
    validate_custom_sidebar_action_schema, CUSTOM_SIDEBAR_ACTION_SCHEMA_VERSION,
};
use custom_sidebar_action::{
    custom_sidebar_action_policy_allows, custom_sidebar_action_schema_catalog,
    invoke_custom_sidebar_action, CUSTOM_SIDEBAR_ACTION_POLICY,
    CUSTOM_SIDEBAR_ALLOWED_ACTION_METHODS,
};
use event_stream::*;
pub(crate) use event_stream::{
    publish_notification_removal_effects, record_manual_restore_window_created,
    record_session_changed_event, replace_session_event_baseline,
};
use lifecycle_dispatch::*;
pub(crate) use lifecycle_dispatch::{
    note_window_focused, ControlActiveWindowState, ControlClosedWindowHistoryState,
};
use pane_surface_control::*;
pub(crate) use payloads::resolve_custom_sidebar_asset_request;
use payloads::*;
use workspace_control::*;

#[cfg(test)]
use crate::browser::strict_browser_runtime_teardown_transaction;
use crate::browser::{
    browser_add_init_script_for_control, browser_attach_webview_for_control,
    browser_clear_network_requests_for_control, browser_close_webview_strict_for_control,
    browser_eval_for_control, browser_has_webview_for_control,
    browser_init_script_count_for_control, browser_network_requests_for_control,
    browser_webview_command_for_control, BrowserNetworkRequestsQuery, BrowserWebviewState,
};
use crate::diff::DiffState;
use crate::dock::{
    publish_runtime_claim, rollback_runtime_claim, runtime_exists_for_control,
    stage_runtime_for_control, teardown_runtime_for_control, DockRuntimeClaim,
    DockRuntimeOperation,
};
use crate::remote_proxy as proxy_runtime;
use crate::session::{
    append_workspace_sidebar_log_for_control, apply_workspace_action_for_control,
    apply_workspace_action_for_control_with_post_commit, browser_go_back_for_control,
    browser_go_forward_for_control, clear_browser_history_for_control,
    clear_workspace_agent_pid_for_control, clear_workspace_panel_pull_request_for_control,
    clear_workspace_remote_for_control, clear_workspace_sidebar_log_for_control,
    clear_workspace_sidebar_metadata_block_for_control,
    clear_workspace_sidebar_metadata_for_control, clear_workspace_sidebar_progress_for_control,
    clear_workspace_sidebar_status_for_control, close_panel_for_control,
    close_workspace_in_window_for_control, close_workspaces_for_control,
    commit_lifecycle_snapshot_for_control, commit_lifecycle_snapshot_for_control_if_current,
    configure_workspace_remote_for_control, current_session_snapshot, default_workspace_directory,
    delete_workspace_group_for_control, equalize_dividers_for_control, focus_last_pane_for_control,
    focus_pane_for_control, move_panel_to_new_workspace_for_control, move_surface_for_control,
    move_workspace_to_window_for_control, new_browser_workspace_for_control,
    new_terminal_tab_for_control, new_workspace_in_window_for_control, open_browser_url_in_panel,
    open_custom_sidebar_in_panel, open_diff_viewer_in_panel, open_file_in_panel,
    open_markdown_file_in_panel, reconnect_workspace_remote_for_control,
    rename_workspace_in_window_for_control, reopen_closed_browser_tab_for_control,
    reopen_closed_workspace_for_control, reorder_surface_for_control,
    reorder_workspaces_in_window_for_control, reorder_workspaces_many_in_window_for_control,
    reset_workspace_color_for_control, reset_workspace_sidebar_metadata_for_control,
    resize_pane_for_control, restore_previous_launch_for_control,
    select_adjacent_panel_for_control, select_last_workspace_for_control,
    select_workspace_in_window_for_control, select_workspace_surface, set_browser_zoom_for_control,
    set_group_collapsed_for_control, set_panel_listening_ports_for_control,
    set_panel_pinned_for_control, set_panel_shell_activity_for_control,
    set_panel_title_for_control, set_panel_tty_for_control, set_panel_unread_for_control,
    set_surface_kind_for_control, set_workspace_agent_listening_ports_for_control,
    set_workspace_agent_pid_for_control, set_workspace_description_for_control,
    set_workspace_panel_pull_request_for_control, set_workspace_pinned_for_control,
    set_workspace_sidebar_metadata_block_for_control, set_workspace_sidebar_metadata_for_control,
    set_workspace_sidebar_progress_for_control, set_workspace_sidebar_status_for_control,
    set_workspace_unread_for_control, show_browser_developer_tools_for_control,
    split_browser_for_control, split_off_surface_for_control, split_panel_for_control,
    start_direct_browser_proxy_for_control, toggle_browser_developer_tools_for_control,
    toggle_browser_focus_mode_for_control, toggle_browser_omnibar_for_control,
    toggle_split_zoom_for_control, transact_value_if_changed_suppressing_derived_events,
    BrowserPanelCreateError, DerivedEventPolicy, MoveWorkspaceToWindowControlError,
    PaneFocusControlError, PaneLastControlError, PaneResizeControlError, PaneResizeControlIntent,
    PaneTopologyControlError, ReorderWorkspacesManyControlError, RestorePreviousLaunchOutcome,
    SessionState, SurfacePositionControlError, TerminalPanelCreateError, WorkspaceLastControlError,
    WorkspaceRemoteControlConfig, WorkspaceSelectControlError,
};
use crate::terminal::{
    materialize_terminal_for_input, request_live_terminal_input, request_terminal_materialization,
    scan_listening_ports_for_root_pid, scan_panel_listening_ports,
    terminal_apply_materialization_events_for_control, terminal_clear_history_panel,
    terminal_grid_size_for_panel, terminal_ids_for_panel_for_control, terminal_open_for_control,
    terminal_read_panel, terminal_remove_id_for_control, terminal_runtime_snapshots,
    terminal_shutdown_id_preserving_authority_for_control, terminal_write_panel,
    TerminalInputOutcome, TerminalMaterializationDemand, TerminalMaterializationSpec,
    TerminalState,
};
use proxy_runtime::{ProxyTarget, RemoteProxyBrokerState};
use terminal_runtime_v2::{
    plan_terminal_request_with_active_window, plan_terminal_set_font_request,
    terminal_create_response_terminal_id, TerminalRequestPlan,
};

const CONTROL_PIPE_BASE_NAME: &str = "cmux";
const CONTROL_EVENTS_CHANGED_EVENT: &str = "cmux://events-changed";
const PANEL_FLASH_EVENT: &str = "cmux://panel-flash";
const SURFACE_REFRESH_EVENT: &str = "cmux://refresh-surfaces";
const CUSTOM_SIDEBAR_RELOAD_EVENT: &str = "cmux://custom-sidebar-reload";
const CUSTOM_SIDEBAR_SELECT_EVENT: &str = "cmux://custom-sidebar-select";
pub struct ControlSocketState {
    listener: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
}

impl Default for ControlSocketState {
    fn default() -> Self {
        Self {
            listener: Mutex::new(None),
        }
    }
}

const EVENT_STREAM_PROTOCOL: &str = "cmux-events";
const EVENT_STREAM_VERSION: u8 = 1;
const EVENT_REPLAY_LIMIT: usize = 4096;
const EVENT_LOG_MAX_BYTES: u64 = 16 * 1024 * 1024;
const EVENT_LOG_FILE_NAME: &str = "events.jsonl";
const EVENT_LOG_ARCHIVE_FILE_NAME: &str = "events.jsonl.1";

pub struct ControlEventState {
    inner: Mutex<ControlEventLog>,
}

#[derive(Default)]
pub struct SidebarSelectionState {
    by_window: Mutex<HashMap<String, Vec<String>>>,
}

impl SidebarSelectionState {
    fn update(&self, window_id: Option<&str>, workspace_ids: Vec<String>) {
        self.by_window
            .lock()
            .expect("sidebar selection mutex poisoned")
            .insert(window_id.unwrap_or_default().to_string(), workspace_ids);
    }

    fn selected_for_window(&self, window_id: Option<&str>) -> Vec<String> {
        self.by_window
            .lock()
            .expect("sidebar selection mutex poisoned")
            .get(window_id.unwrap_or_default())
            .cloned()
            .unwrap_or_default()
    }
}

#[tauri::command]
pub fn sidebar_selection_update(
    state: State<'_, SidebarSelectionState>,
    window_id: Option<String>,
    workspace_ids: Vec<String>,
) {
    state.update(window_id.as_deref(), workspace_ids);
}

#[derive(Default)]
pub struct ControlHandleRegistryState {
    inner: Mutex<ControlHandleRegistry>,
}

#[derive(Default)]
struct ControlHandleRegistry {
    refs: HashMap<(&'static str, String), u64>,
    next: HashMap<&'static str, u64>,
}

impl ControlHandleRegistry {
    fn mint(&mut self, kind: &'static str, id: &str) -> String {
        let key = (kind, id.to_string());
        let number = if let Some(number) = self.refs.get(&key) {
            *number
        } else {
            let next = self.next.entry(kind).or_insert(1);
            let number = *next;
            *next = next.saturating_add(1);
            self.refs.insert(key, number);
            number
        };
        format!("{kind}:{number}")
    }

    /// R5 (differential remediation): canonical drops a closed/respawned
    /// entity from its registry, so rendering it again re-mints a FRESH ref
    /// (capture: close echo surface:17 for a surface previously :16; respawn
    /// surface:21/pane:11). The counter never rewinds.
    fn forget(&mut self, kind: &'static str, id: &str) {
        self.refs.remove(&(kind, id.to_string()));
    }

    fn resolve(&self, kind: &'static str, reference: &str) -> Option<String> {
        let number = one_based_ref_index(reference, kind)? as u64 + 1;
        self.refs
            .iter()
            .find_map(|((entry_kind, id), entry_number)| {
                (*entry_kind == kind && *entry_number == number).then(|| id.clone())
            })
    }
}

/// D3: canonical registers the bootstrap window/workspace/pane/surface in
/// the handle registry before the first socket mint, so fixture refs start at
/// :2 per kind (live capture: workspace:2/pane:2 for the fixture workspace,
/// pane:3/surface:4 for the first split). Walk order: window, then per
/// workspace: workspace id, panes in layout order, and authoritative surface
/// records in creation order. Legacy snapshots without records fall back to
/// layout order.
fn bootstrap_registry_seeds(snapshot: &AppSessionSnapshot) -> Vec<(&'static str, String)> {
    fn walk_layout(
        layout: &cmux_core::session::SessionWorkspaceLayoutSnapshot,
        seed_surfaces: bool,
        seeds: &mut Vec<(&'static str, String)>,
    ) {
        match layout {
            cmux_core::session::SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                if let Some(id) = &pane.pane_id {
                    seeds.push(("pane", id.clone()));
                }
                if seed_surfaces {
                    for panel in &pane.panel_ids {
                        seeds.push(("surface", panel.clone()));
                    }
                }
            }
            cmux_core::session::SessionWorkspaceLayoutSnapshot::Split(split) => {
                walk_layout(&split.first, seed_surfaces, seeds);
                walk_layout(&split.second, seed_surfaces, seeds);
            }
        }
    }
    let mut seeds = Vec::new();
    for window in &snapshot.windows {
        if let Some(id) = &window.window_id {
            seeds.push(("window", id.clone()));
        }
        for workspace in &window.tab_manager.workspaces {
            if let Some(id) = &workspace.workspace_id {
                seeds.push(("workspace", id.clone()));
            }
            if let Some(layout) = &workspace.layout {
                walk_layout(layout, workspace.surfaces.is_none(), &mut seeds);
            }
            if let Some(surfaces) = &workspace.surfaces {
                for surface in surfaces {
                    seeds.push(("surface", surface.surface_id.clone()));
                }
            }
        }
        // Canonical mints group refs after the window's workspaces
        // (v2RefreshKnownRefs, TerminalController.swift:3580-3584).
        for group in window
            .tab_manager
            .workspace_groups
            .as_deref()
            .unwrap_or_default()
        {
            seeds.push(("workspace_group", group.id.clone()));
        }
    }
    seeds
}

/// Seed the handle registry with the bootstrap session entities (D3). Runs in
/// app setup after the session bootstrap and before the control listener
/// starts.
pub(crate) fn seed_control_handle_registry(app: &AppHandle) {
    refresh_known_handle_refs(app, &snapshot(app));
}

/// Round 6 item 2 — the twin of canonical `v2RefreshKnownRefs`
/// (TerminalController.swift:3561-3586 at pinned e1825d40d): every control
/// dispatch mints refs for EVERY live window/workspace/pane/surface (and
/// workspace group) in creation order, so entities created between dispatches
/// occupy their ref number even when no response ever renders them (the
/// differential's burned pane:10).
fn refresh_known_handle_refs(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    for (kind, id) in bootstrap_registry_seeds(snapshot) {
        control_handle_ref(app, kind, &id);
    }
}

fn control_handle_ref(app: &AppHandle, kind: &'static str, id: &str) -> String {
    let state = app.state::<ControlHandleRegistryState>();
    let reference = state
        .inner
        .lock()
        .expect("control handle registry mutex poisoned")
        .mint(kind, id);
    reference
}

fn resolve_control_handle_ref(
    app: &AppHandle,
    kind: &'static str,
    reference: &str,
) -> Option<String> {
    app.state::<ControlHandleRegistryState>()
        .inner
        .lock()
        .expect("control handle registry mutex poisoned")
        .resolve(kind, reference)
}

struct ControlEventLog {
    boot_id: String,
    next_seq: u64,
    events: VecDeque<Value>,
    last_session_summaries: BTreeMap<String, SessionEventSummary>,
    subscribers: Vec<EventSubscriber>,
}

impl Default for ControlEventState {
    fn default() -> Self {
        Self {
            inner: Mutex::new(ControlEventLog {
                boot_id: Uuid::new_v4().to_string(),
                next_seq: 1,
                events: VecDeque::new(),
                last_session_summaries: BTreeMap::new(),
                subscribers: Vec::new(),
            }),
        }
    }
}

struct EventSubscriber {
    sender: cmux_ipc::stream_mpsc::UnboundedSender<String>,
    names: Vec<String>,
    categories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
struct SessionEventSummary {
    window_id: Option<String>,
    selected_workspace_id: Option<String>,
    selected_workspace_index: Option<usize>,
    workspaces: Vec<WorkspaceEventSummary>,
}

#[derive(Debug, Clone, PartialEq)]
struct WorkspaceEventSummary {
    key: String,
    id: Option<String>,
    title: String,
    index: usize,
    panes: Vec<PaneEventSummary>,
    surface_ids: Vec<String>,
    selected_surface_id: Option<String>,
    sidebar: WorkspaceSidebarEventSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneEventSummary {
    key: String,
    id: Option<String>,
    index: usize,
    surface_ids: Vec<String>,
    selected_surface_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct WorkspaceSidebarEventSummary {
    progress: Option<Value>,
    status_entries: BTreeMap<String, Value>,
    metadata_entries: BTreeMap<String, Value>,
    metadata_blocks: BTreeMap<String, Value>,
    log_entries: Vec<Value>,
}

#[derive(Debug, Clone)]
struct DerivedEventSpec {
    name: &'static str,
    category: &'static str,
    source: &'static str,
    window_id: Option<String>,
    workspace_id: Option<String>,
    surface_id: Option<String>,
    payload: Value,
}

#[derive(Debug, serde::Serialize, PartialEq, Eq)]
pub struct ControlSocketStatus {
    pub pipe: String,
    pub running: bool,
}

#[tauri::command]
pub fn control_socket_status(state: State<'_, ControlSocketState>) -> ControlSocketStatus {
    control_socket_status_inner(&state)
}

#[tauri::command]
pub fn restart_control_socket_listener(
    app: AppHandle,
    state: State<'_, ControlSocketState>,
) -> Result<ControlSocketStatus, String> {
    restart_control_socket_listener_inner(&app, &state)
}

#[tauri::command]
pub fn custom_sidebar_action_invoke(
    app: AppHandle,
    method: String,
    params: Option<Value>,
    source_path: Option<String>,
) -> Value {
    invoke_custom_sidebar_action(&app, method, params, source_path.as_deref())
}

pub(crate) fn start_control_socket_listener(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<ControlSocketState>();
    restart_control_socket_listener_inner(app, &state).map(|_| ())
}

fn control_socket_status_inner(state: &ControlSocketState) -> ControlSocketStatus {
    let pipe = control_pipe_path();
    let running = state
        .listener
        .lock()
        .expect("control socket listener mutex poisoned")
        .as_ref()
        .is_some();
    ControlSocketStatus { pipe, running }
}

fn restart_control_socket_listener_inner(
    app: &AppHandle,
    state: &ControlSocketState,
) -> Result<ControlSocketStatus, String> {
    let pipe = control_pipe_path();
    let mut guard = state
        .listener
        .lock()
        .expect("control socket listener mutex poisoned");
    if let Some(handle) = guard.take() {
        handle.abort();
    }

    let app = app.clone();
    let server_pipe = pipe.clone();
    let handle = tauri::async_runtime::spawn(async move {
        let factory = move || DesktopControlHandler { app: app.clone() };
        if let Err(error) = cmux_ipc::serve_named_pipe(&server_pipe, factory).await {
            eprintln!("[control-socket] listener stopped: {error}");
        }
    });
    *guard = Some(handle);
    Ok(ControlSocketStatus {
        pipe,
        running: true,
    })
}

fn control_pipe_path() -> String {
    control_pipe_path_for_base(std::env::var("CMUX_CONTROL_PIPE_NAME").ok().as_deref())
}

/// `CMUX_CONTROL_PIPE_NAME` overrides the control pipe base name so test
/// fixtures can isolate the named pipe. Values the pipe-path builder rejects
/// (empty, backslash, over-long) fall back to the default — never panic.
fn control_pipe_path_for_base(override_name: Option<&str>) -> String {
    override_name
        .and_then(|name| cmux_ipc::control_pipe_path(name).ok())
        .unwrap_or_else(|| {
            cmux_ipc::control_pipe_path(CONTROL_PIPE_BASE_NAME)
                .expect("static control pipe base name is valid")
        })
}

#[derive(Clone)]
struct DesktopControlHandler {
    app: AppHandle,
}

const CONTROL_SOCKET_METHODS: &[&str] = &[
    "ping",
    "system.ping",
    "system.identify",
    "system.capabilities",
    "mobile.terminal.create",
    "mobile.terminal.input",
    "mobile.terminal.set_font",
    "terminal.create",
    "terminal.input",
    "config.reload",
    "window.list",
    "window.current",
    "window.displays",
    "window.display",
    "window.create",
    "window.close",
    "window.focus",
    "notification.list",
    "notification.dismiss",
    "notification.mark_read",
    "notification.clear",
    "notification.open",
    "notification.jump_to_unread",
    "notification.create",
    "notification.create_for_caller",
    "notification.create_for_surface",
    "notification.create_for_target",
    "right_sidebar",
    "feed.push",
    "feed.list",
    "feed.permission.reply",
    "feed.question.reply",
    "feed.exit_plan.reply",
    "events.stream",
    "extension.sidebar.snapshot",
    "sidebar.snapshot",
    "sidebar.list",
    "sidebar.validate",
    "sidebar.reload",
    "sidebar.select",
    "sidebar.open",
    "workspace.list",
    "workspace.current",
    "workspace.create",
    "workspace.create_browser",
    "browser.new_workspace",
    "session.restore_previous",
    "session.restore_previous_launch",
    "workspace.restore_previous_launch",
    "workspace.close",
    "workspace.action",
    "workspace.reopen_closed",
    "workspace.close_many",
    "workspace.close_workspaces",
    "workspace.rename",
    "workspace.select",
    "workspace.reorder",
    "workspace.reorder_many",
    "workspace.move",
    "workspace.move_to_window",
    "workspace.next",
    "workspace.previous",
    "workspace.last",
    "workspace.equalize_splits",
    "workspace.set_description",
    "workspace.reset_color",
    "workspace.set_progress",
    "workspace.clear_progress",
    "workspace.set_status",
    "workspace.clear_status",
    "workspace.list_status",
    "workspace.set_agent_pid",
    "workspace.clear_agent_pid",
    "workspace.report_pr",
    "workspace.report_review",
    "workspace.clear_pr",
    "workspace.report_meta",
    "workspace.clear_meta",
    "workspace.list_meta",
    "workspace.report_meta_block",
    "workspace.clear_meta_block",
    "workspace.list_meta_blocks",
    "workspace.reset_sidebar",
    "workspace.log",
    "workspace.clear_log",
    "workspace.list_log",
    "workspace.sidebar_state",
    "workspace.set_unread",
    "workspace.set_pinned",
    "workspace.remote.status",
    "workspace.remote.configure",
    "workspace.remote.disconnect",
    "workspace.remote.clear",
    "workspace.remote.reconnect",
    "workspace.group.list",
    "workspace.group.create",
    "workspace.group.ungroup",
    "workspace.group.delete",
    "workspace.group.rename",
    "workspace.group.collapse",
    "workspace.group.expand",
    "workspace.group.pin",
    "workspace.group.unpin",
    "workspace.group.add",
    "workspace.group.remove",
    "workspace.group.set_anchor",
    "workspace.group.new_workspace",
    "workspace.group.set_color",
    "workspace.group.set_icon",
    "workspace.group.move",
    "workspace.group.focus",
    "workspace.group.set_collapsed",
    "workspace_group.set_collapsed",
    "surface.list",
    "surface.current",
    "surface.create",
    "surface.action",
    "tab.action",
    "surface.report_pwd",
    "surface.respawn",
    "surface.split",
    "surface.new_terminal_tab",
    "surface.new_tab",
    "surface.split_browser",
    "surface.split_off",
    "surface.drag_to_split",
    "surface.close",
    "surface.set_type",
    "surface.set_kind",
    "surface.rename",
    "surface.set_title",
    "surface.set_pinned",
    "surface.set_unread",
    "surface.report_ports",
    "surface.set_ports",
    "report_ports",
    "surface.report_tty",
    "report_tty",
    "report-tty",
    "surface.report_shell_state",
    "surface.move",
    "surface.reorder",
    "report_shell_state",
    "report-shell-state",
    "surface.clear_ports",
    "clear_ports",
    "surface.ports_kick",
    "ports_kick",
    "surface.focus",
    "surface.health",
    "surface.clear_history",
    "surface.trigger_flash",
    "surface.refresh_all",
    "surface.refresh",
    "surface.resume.set",
    "surface.resume.get",
    "surface.resume.clear",
    "surface.read_text",
    "surface.send_text",
    "surface.send_key",
    "surface.move_to_new_workspace",
    "surface.open_browser",
    "surface.open_markdown",
    "surface.open_file",
    "surface.open_diff",
    "surface.next",
    "surface.previous",
    "surface.toggle_split_zoom",
    "pane.swap",
    "pane.create",
    "pane.focus",
    "pane.list",
    "pane.surfaces",
    "pane.break",
    "pane.join",
    "pane.last",
    "pane.resize",
    "browser.open_split",
    "browser.navigate",
    "browser.back",
    "browser.forward",
    "browser.reload",
    "browser.url.get",
    "browser.focus_webview",
    "browser.is_webview_focused",
    "browser.snapshot",
    "browser.eval",
    "browser.wait",
    "browser.click",
    "browser.dblclick",
    "browser.hover",
    "browser.focus",
    "browser.type",
    "browser.fill",
    "browser.press",
    "browser.keydown",
    "browser.keyup",
    "browser.check",
    "browser.uncheck",
    "browser.select",
    "browser.scroll",
    "browser.scroll_into_view",
    "browser.screenshot",
    "browser.get.text",
    "browser.get.html",
    "browser.get.value",
    "browser.get.attr",
    "browser.get.title",
    "browser.get.count",
    "browser.get.box",
    "browser.get.styles",
    "browser.is.visible",
    "browser.is.enabled",
    "browser.is.checked",
    "browser.find.role",
    "browser.find.text",
    "browser.find.label",
    "browser.find.placeholder",
    "browser.find.alt",
    "browser.find.title",
    "browser.find.testid",
    "browser.find.first",
    "browser.find.last",
    "browser.find.nth",
    "browser.frame.select",
    "browser.frame.main",
    "browser.dialog.accept",
    "browser.dialog.dismiss",
    "browser.download.wait",
    "browser.cookies.get",
    "browser.cookies.set",
    "browser.cookies.clear",
    "browser.storage.get",
    "browser.storage.set",
    "browser.storage.clear",
    "browser.tab.new",
    "browser.tab.list",
    "browser.tab.switch",
    "browser.tab.close",
    "browser.console.list",
    "browser.console.clear",
    "browser.errors.list",
    "browser.highlight",
    "browser.state.save",
    "browser.state.load",
    "browser.addinitscript",
    "browser.addscript",
    "browser.addstyle",
    "browser.reopen_closed",
    "browser.reopen_closed_tab",
    "browser.clear_history",
    "browser.toggle_omnibar",
    "browser.toggle_focus_mode",
    "browser.toggle_developer_tools",
    "browser.show_developer_tools",
    "browser.set_zoom",
    "browser.network.requests",
    "browser.network.clear",
    "browser.viewport.set",
    "browser.geolocation.set",
    "browser.offline.set",
    "browser.trace.start",
    "browser.trace.stop",
    "browser.network.route",
    "browser.network.unroute",
    "browser.screencast.start",
    "browser.screencast.stop",
    "browser.input_mouse",
    "browser.input_keyboard",
    "browser.input_touch",
    "debug.browser.start_direct_proxy",
    "debug.browser.attach_webview",
    "debug.terminals",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlRequestRoute {
    PaneSurfaceLifecycle,
    WorkspaceAction,
    WindowLifecycle,
    Legacy,
}

fn control_request_route_for_method(method: &str) -> ControlRequestRoute {
    match method {
        "pane.break" | "pane.create" | "pane.join" | "pane.resize" | "pane.focus" | "pane.last"
        | "pane.swap" | "surface.action" | "tab.action" | "surface.create" | "surface.current"
        | "surface.list" | "surface.report_pwd" | "surface.respawn" | "surface.close"
        | "surface.focus" | "surface.move" | "surface.split" => {
            ControlRequestRoute::PaneSurfaceLifecycle
        }
        "workspace.action" => ControlRequestRoute::WorkspaceAction,
        "window.create"
        | "window.close"
        | "window.focus"
        | "surface.refresh"
        | "surface.resume.set"
        | "surface.resume.get"
        | "surface.resume.clear" => ControlRequestRoute::WindowLifecycle,
        _ => ControlRequestRoute::Legacy,
    }
}

impl cmux_ipc::ControlRequestHandler for DesktopControlHandler {
    fn handle(&mut self, request: ControlRequest) -> ControlCallResult {
        handle_control_request(&self.app, request)
    }

    fn close_without_response(
        &mut self,
        request: &ControlRequest,
        result: &ControlCallResult,
    ) -> bool {
        window_close_terminates_without_response(&self.app, request, result)
    }

    fn handle_stream(&mut self, request: ControlRequest) -> Option<ControlStream> {
        (request.method == "events.stream").then(|| events_live_stream(&self.app, &request.params))
    }

    fn handle_v1_line(&mut self, line: &str) -> Option<String> {
        let command = window_lifecycle::parse_v1_window_command(line)?;
        Some(match window_lifecycle::v1_window_request(&command) {
            Err(early_error) => early_error,
            Ok((method, params)) => {
                let result =
                    handle_control_request(&self.app, ControlRequest::new(None, method, params));
                window_lifecycle::v1_window_reply(&command, &result)
            }
        })
    }
}

struct DeferredRemoteWorkspaceRenameFlush<'a>(&'a SessionState);

impl Drop for DeferredRemoteWorkspaceRenameFlush<'_> {
    fn drop(&mut self) {
        self.0.flush_deferred_remote_workspace_renames();
    }
}

fn run_after_control_mutation_gate<T>(
    guard: parking_lot::ReentrantMutexGuard<'_, ()>,
    operation: impl FnOnce() -> T,
) -> T {
    drop(guard);
    operation()
}

fn handle_control_request(app: &AppHandle, mut request: ControlRequest) -> ControlCallResult {
    // Keep this event-only route outside normalization and the mutation gate.
    if request.method == "mobile.terminal.set_font" {
        return terminal_set_font_control(app, &request.params);
    }
    let session_state = app.state::<SessionState>();
    // Drop order releases the mutation gate before deferred SSH creation.
    let _remote_workspace_rename_flush = DeferredRemoteWorkspaceRenameFlush(session_state.inner());
    let control_guard = match session_state.lock_control_mutation() {
        Ok(guard) => guard,
        Err(message) => {
            return ControlCallResult::Err {
                code: "internal_error".into(),
                message,
                data: None,
            }
        }
    };
    // Canonical dispatch preamble: refresh known refs BEFORE handle-ref
    // resolution and routing (v2RefreshKnownRefs via controlResolveOnMain /
    // the main-lane preamble).
    refresh_known_handle_refs(app, &snapshot(app));
    if request.method.starts_with("workspace.") && request.params.contains_key("window") {
        return ControlCallResult::Err {
            code: "invalid_params".to_string(),
            message: "Unsupported parameter `window`; use `window_id` with a window UUID or ref from `window.list`.".to_string(),
            data: json!({
                "method": request.method,
                "unsupported_param": "window",
                "supported_param": "window_id",
            })
            .try_into()
            .ok(),
        };
    }
    let workspace_action_event_params =
        (request.method == "workspace.action").then(|| request.params.clone());
    // pane.create's split source honors ONLY a raw-UUID surface_id (canonical
    // ControlCommandCoordinator+Pane.swift:300); routing still resolves refs.
    // Preserve the pre-resolution value on a reserved key (stripped first so a
    // caller cannot spoof it) for the lifecycle dispatcher's source selection.
    request.params.remove("__pane_create_raw_surface_id");
    let pane_create_raw_surface_id = (request.method == "pane.create")
        .then(|| request.params.get("surface_id").cloned())
        .flatten();
    resolve_request_handle_refs(app, &mut request.params);
    if let Some(raw) = pane_create_raw_surface_id {
        request
            .params
            .insert("__pane_create_raw_surface_id".into(), raw);
    }
    if matches!(
        request.method.as_str(),
        "terminal.input" | "mobile.terminal.input"
    ) {
        let prepared = match prepare_terminal_input_control(app, &request.method, &request.params) {
            Ok(prepared) => prepared,
            Err(error) => return error,
        };
        return run_after_control_mutation_gate(control_guard, || {
            finish_terminal_input_control(app, prepared)
        });
    }
    match control_request_route_for_method(&request.method) {
        ControlRequestRoute::PaneSurfaceLifecycle => {
            return handle_pane_surface_lifecycle_request(app, &request.method, &request.params);
        }
        ControlRequestRoute::WorkspaceAction => {
            return handle_workspace_action_request(
                app,
                &request.params,
                workspace_action_event_params
                    .as_ref()
                    .expect("workspace.action event params were captured"),
            );
        }
        ControlRequestRoute::WindowLifecycle => {
            return handle_window_lifecycle_request(app, &request.method, request.params);
        }
        ControlRequestRoute::Legacy => {}
    }
    match request.method.as_str() {
        "terminal.create" | "mobile.terminal.create" => {
            terminal_create_control(app, &request.method, &request.params)
        }
        "ping" | "system.ping" => ok(json!("pong")),
        "system.identify" => ok(json!({
            "app": "cmux",
            "platform": cmux_core::CMUX_PLATFORM,
            "milestone": cmux_core::milestone(),
            "control_socket": {
                "transport": "windows-named-pipe",
                "pipe": control_pipe_path(),
            },
        })),
        "system.capabilities" => ok(json!({
            "version": 2,
            "methods": CONTROL_SOCKET_METHODS,
            "transport": "windows-named-pipe",
            "custom_sidebar_actions": {
                "policy": CUSTOM_SIDEBAR_ACTION_POLICY,
                "allowed_methods": CUSTOM_SIDEBAR_ALLOWED_ACTION_METHODS,
                "schema": custom_sidebar_action_schema_catalog(),
            },
        })),
        "config.reload" => config_reload(app),
        "window.list" => window_list(app),
        "window.current" => window_current(app, &request.params),
        "window.displays" => window_displays(app),
        "window.display" => window_display(app, &request.params),
        "notification.list" => notification_list(app),
        "notification.dismiss" => notification_dismiss(app, &request.params),
        "notification.mark_read" => notification_mark_read(app, &request.params),
        "notification.clear" => notification_clear(app, &request.params),
        "notification.open" => notification_open(app, &request.params),
        "notification.jump_to_unread" => notification_jump_to_unread(app),
        "notification.create" => notification_create(app, &request.params),
        "notification.create_for_caller" => notification_create_for_caller(app, &request.params),
        "notification.create_for_surface" => notification_create_for_surface(app, &request.params),
        "notification.create_for_target" => notification_create_for_target(app, &request.params),
        "right_sidebar" => right_sidebar_control(app, &request.params),
        "feed.push" => feed_push(app, &request.params, request.id.is_some()),
        "feed.list" => feed_list(app),
        "feed.permission.reply" => feed_permission_reply(app, &request.params),
        "feed.question.reply" => feed_question_reply(app, &request.params),
        "feed.exit_plan.reply" => feed_exit_plan_reply(app, &request.params),
        "events.stream" => ok(events_snapshot_payload(app, &request.params)),
        "extension.sidebar.snapshot" | "sidebar.snapshot" => {
            let snapshot = snapshot(app);
            ok(extension_sidebar_snapshot_payload_for_app(
                app,
                &snapshot,
                &request.params,
            ))
        }
        "sidebar.list" | "sidebar.validate" => sidebar_validate(&request.params),
        "sidebar.open" => sidebar_open(app, &request.params),
        "sidebar.reload" => sidebar_reload(app, &request.params),
        "sidebar.select" => sidebar_select(app, &request.params),
        "workspace.list" => {
            workspace_list_with_recoverable_active(app, &snapshot(app), &request.params)
        }
        "workspace.current" => {
            workspace_current_from_params_for_app(app, &snapshot(app), &request.params)
        }
        "workspace.create" => workspace_create(app, &request.params),
        "workspace.create_browser" | "browser.new_workspace" => {
            workspace_create_browser(app, &request.params)
        }
        "session.restore_previous"
        | "session.restore_previous_launch"
        | "workspace.restore_previous_launch" => session_restore_previous_launch(app),
        "workspace.close" => workspace_close(app, &request.params),
        "workspace.reopen_closed" => workspace_reopen_closed(app),
        "workspace.close_many" | "workspace.close_workspaces" => {
            workspace_close_many(app, &request.params)
        }
        "workspace.rename" => workspace_rename(app, &request.params),
        "workspace.select" => workspace_select(app, &request.params),
        "workspace.reorder" | "workspace.move" => workspace_reorder(app, &request.params),
        "workspace.move_to_window" => workspace_move_to_window(app, &request.params),
        "workspace.reorder_many" => workspace_reorder_many(app, &request.params),
        "workspace.next" => workspace_select_relative(app, &request.params, 1),
        "workspace.previous" => workspace_select_relative(app, &request.params, -1),
        "workspace.last" => workspace_last(app, &request.params),
        "workspace.equalize_splits" => workspace_equalize_splits(app),
        "workspace.set_description" => workspace_set_description(app, &request.params),
        "workspace.reset_color" => workspace_reset_color(app, &request.params),
        "workspace.set_progress" | "set_progress" | "set-progress" => {
            workspace_set_progress(app, &request.params)
        }
        "workspace.clear_progress" | "clear_progress" | "clear-progress" => {
            workspace_clear_progress(app, &request.params)
        }
        "workspace.set_status" | "set_status" | "set-status" => {
            workspace_set_status(app, &request.params)
        }
        "workspace.clear_status" | "clear_status" | "clear-status" => {
            workspace_clear_status(app, &request.params)
        }
        "workspace.list_status" | "list_status" | "list-status" => {
            workspace_list_status(app, &request.params)
        }
        "workspace.set_agent_pid" | "set_agent_pid" | "set-agent-pid" => {
            workspace_set_agent_pid(app, &request.params)
        }
        "workspace.clear_agent_pid" | "clear_agent_pid" | "clear-agent-pid" => {
            workspace_clear_agent_pid(app, &request.params)
        }
        "workspace.report_pr" | "report_pr" | "report-pr" => {
            workspace_report_pr(app, &request.params, "PR")
        }
        "workspace.report_review" | "report_review" | "report-review" => {
            workspace_report_pr(app, &request.params, "Review")
        }
        "workspace.clear_pr" | "clear_pr" | "clear-pr" => workspace_clear_pr(app, &request.params),
        "workspace.report_meta" | "report_meta" | "report-meta" => {
            workspace_report_meta(app, &request.params)
        }
        "workspace.clear_meta" | "clear_meta" | "clear-meta" => {
            workspace_clear_meta(app, &request.params)
        }
        "workspace.list_meta" | "list_meta" | "list-meta" => {
            workspace_list_meta(app, &request.params)
        }
        "workspace.report_meta_block" | "report_meta_block" | "report-meta-block" => {
            workspace_report_meta_block(app, &request.params)
        }
        "workspace.clear_meta_block" | "clear_meta_block" | "clear-meta-block" => {
            workspace_clear_meta_block(app, &request.params)
        }
        "workspace.list_meta_blocks" | "list_meta_blocks" | "list-meta-blocks" => {
            workspace_list_meta_blocks(app, &request.params)
        }
        "workspace.reset_sidebar" | "reset_sidebar" | "reset-sidebar" => {
            workspace_reset_sidebar(app, &request.params)
        }
        "workspace.log" | "log" => workspace_log(app, &request.params),
        "workspace.clear_log" | "clear_log" | "clear-log" => {
            workspace_clear_log(app, &request.params)
        }
        "workspace.list_log" | "list_log" | "list-log" => workspace_list_log(app, &request.params),
        "workspace.sidebar_state" | "sidebar_state" | "sidebar-state" => {
            workspace_sidebar_state(app, &request.params)
        }
        "workspace.set_unread" => workspace_set_unread(app, &request.params),
        "workspace.set_pinned" => workspace_set_pinned(app, &request.params),
        "workspace.remote.status" => workspace_remote_status(app, &request.params),
        "workspace.remote.configure" => workspace_remote_configure(app, &request.params),
        "workspace.remote.disconnect" | "workspace.remote.clear" => {
            workspace_remote_disconnect(app, &request.params)
        }
        "workspace.remote.reconnect" => workspace_remote_reconnect(app, &request.params),
        method @ ("workspace.group.list"
        | "workspace.group.create"
        | "workspace.group.ungroup"
        | "workspace.group.delete"
        | "workspace.group.rename"
        | "workspace.group.collapse"
        | "workspace.group.expand"
        | "workspace.group.pin"
        | "workspace.group.unpin"
        | "workspace.group.add"
        | "workspace.group.remove"
        | "workspace.group.set_anchor"
        | "workspace.group.new_workspace"
        | "workspace.group.set_color"
        | "workspace.group.set_icon"
        | "workspace.group.move"
        | "workspace.group.focus") => workspace_group_control(app, method, &request.params),
        "workspace.group.set_collapsed" | "workspace_group.set_collapsed" => {
            workspace_group_set_collapsed(app, &request.params)
        }
        "surface.list" => surface_list_from_params(&snapshot(app), &request.params),
        "surface.split" => surface_split(app, &request.params),
        "surface.new_terminal_tab" | "surface.new_tab" => {
            surface_new_terminal_tab(app, &request.params)
        }
        "surface.split_browser" => surface_split_browser(app, &request.params),
        "surface.split_off" | "surface.drag_to_split" => surface_split_off(app, &request.params),
        "browser.open_split" => browser_open_split(app, &request.params),
        "surface.close" => surface_close(app, &request.params),
        "surface.set_type" | "surface.set_kind" => surface_set_kind(app, &request.params),
        "surface.rename" | "surface.set_title" => surface_set_title(app, &request.params),
        "surface.set_pinned" => surface_set_pinned(app, &request.params),
        "surface.set_unread" => surface_set_unread(app, &request.params),
        "surface.move" => surface_move(app, &request.params),
        "surface.reorder" => surface_reorder(app, &request.params),
        "surface.report_ports" | "surface.set_ports" | "report_ports" => {
            surface_report_ports(app, &request.params)
        }
        "surface.report_tty" | "report_tty" | "report-tty" => {
            surface_report_tty(app, &request.params)
        }
        "surface.report_shell_state" | "report_shell_state" | "report-shell-state" => {
            surface_report_shell_state(app, &request.params)
        }
        "surface.clear_ports" | "clear_ports" => surface_clear_ports(app, &request.params),
        "surface.ports_kick" | "ports_kick" => surface_ports_kick(app, &request.params),
        "surface.focus" => surface_focus(app, &request.params),
        "surface.health" => surface_health(app, &request.params),
        "surface.clear_history" => surface_clear_history(app, &request.params),
        "surface.trigger_flash" => surface_trigger_flash(app, &request.params),
        "surface.refresh_all" => surface_refresh_all(app),
        "surface.read_text" => surface_read_text(app, &request.params),
        "surface.send_text" => surface_send_text(app, &request.params),
        "surface.send_key" => surface_send_key(app, &request.params),
        "surface.move_to_new_workspace" => surface_move_to_new_workspace(app, &request.params),
        "surface.open_browser" => surface_open_browser(app, &request.params),
        "surface.open_markdown" => surface_open_markdown(app, &request.params),
        "surface.open_file" => surface_open_file(app, &request.params),
        "surface.open_diff" => surface_open_diff(app, &request.params),
        "surface.next" => surface_select_adjacent(app, &request.params, true),
        "surface.previous" => surface_select_adjacent(app, &request.params, false),
        "surface.toggle_split_zoom" => surface_toggle_split_zoom(app, &request.params),
        "pane.focus" => pane_focus(app, &request.params),
        "pane.list" => pane_list(app, &request.params),
        "pane.surfaces" => pane_surfaces(app, &request.params),
        "pane.last" => pane_last(app, &request.params),
        "pane.resize" => pane_resize(app, &request.params),
        "browser.navigate" => browser_navigate(app, &request.params),
        "browser.back" => browser_back(app, &request.params),
        "browser.forward" => browser_forward(app, &request.params),
        "browser.reload" => browser_reload(app, &request.params),
        "browser.url.get" => browser_url_get(app, &request.params),
        "browser.focus_webview" => browser_focus_webview(app, &request.params),
        "browser.is_webview_focused" => browser_is_webview_focused(app, &request.params),
        "browser.snapshot" => browser_snapshot(app, &request.params),
        "browser.eval" => browser_eval(app, &request.params),
        "browser.wait" => browser_wait(app, &request.params),
        "browser.click" => browser_action(app, &request.params, BrowserAction::Click),
        "browser.dblclick" => browser_action(app, &request.params, BrowserAction::DblClick),
        "browser.hover" => browser_action(app, &request.params, BrowserAction::Hover),
        "browser.focus" => browser_action(app, &request.params, BrowserAction::Focus),
        "browser.type" => browser_action(app, &request.params, BrowserAction::Type),
        "browser.fill" => browser_action(app, &request.params, BrowserAction::Fill),
        "browser.press" => browser_action(app, &request.params, BrowserAction::Press),
        "browser.keydown" => browser_action(app, &request.params, BrowserAction::KeyDown),
        "browser.keyup" => browser_action(app, &request.params, BrowserAction::KeyUp),
        "browser.check" => browser_action(app, &request.params, BrowserAction::Check),
        "browser.uncheck" => browser_action(app, &request.params, BrowserAction::Uncheck),
        "browser.select" => browser_action(app, &request.params, BrowserAction::Select),
        "browser.scroll" => browser_action(app, &request.params, BrowserAction::Scroll),
        "browser.scroll_into_view" => {
            browser_action(app, &request.params, BrowserAction::ScrollIntoView)
        }
        "browser.screenshot" => browser_screenshot(app, &request.params),
        "browser.get.title" => browser_get_title(app, &request.params),
        "browser.get.text" => browser_get_selector_value(app, &request.params, BrowserGetter::Text),
        "browser.get.html" => browser_get_selector_value(app, &request.params, BrowserGetter::Html),
        "browser.get.value" => {
            browser_get_selector_value(app, &request.params, BrowserGetter::Value)
        }
        "browser.get.attr" => browser_get_selector_value(app, &request.params, BrowserGetter::Attr),
        "browser.get.count" => {
            browser_get_selector_value(app, &request.params, BrowserGetter::Count)
        }
        "browser.get.box" => browser_get_selector_value(app, &request.params, BrowserGetter::Box),
        "browser.get.styles" => {
            browser_get_selector_value(app, &request.params, BrowserGetter::Styles)
        }
        "browser.is.visible" => {
            browser_get_selector_value(app, &request.params, BrowserGetter::Visible)
        }
        "browser.is.enabled" => {
            browser_get_selector_value(app, &request.params, BrowserGetter::Enabled)
        }
        "browser.is.checked" => {
            browser_get_selector_value(app, &request.params, BrowserGetter::Checked)
        }
        "browser.find.role" => browser_find(app, &request.params, BrowserLocator::Role),
        "browser.find.text" => browser_find(app, &request.params, BrowserLocator::Text),
        "browser.find.label" => browser_find(app, &request.params, BrowserLocator::Label),
        "browser.find.placeholder" => {
            browser_find(app, &request.params, BrowserLocator::Placeholder)
        }
        "browser.find.alt" => browser_find(app, &request.params, BrowserLocator::Alt),
        "browser.find.title" => browser_find(app, &request.params, BrowserLocator::Title),
        "browser.find.testid" => browser_find(app, &request.params, BrowserLocator::TestId),
        "browser.find.first" => browser_find(app, &request.params, BrowserLocator::First),
        "browser.find.last" => browser_find(app, &request.params, BrowserLocator::Last),
        "browser.find.nth" => browser_find(app, &request.params, BrowserLocator::Nth),
        "browser.frame.select" => browser_frame_select(app, &request.params),
        "browser.frame.main" => browser_frame_main(app, &request.params),
        "browser.dialog.accept" => {
            browser_dialog(app, &request.params, BrowserDialogAction::Accept)
        }
        "browser.dialog.dismiss" => {
            browser_dialog(app, &request.params, BrowserDialogAction::Dismiss)
        }
        "browser.download.wait" => browser_download_wait(app, &request.params),
        "browser.cookies.get" => browser_cookies(app, &request.params, BrowserCookieAction::Get),
        "browser.cookies.set" => browser_cookies(app, &request.params, BrowserCookieAction::Set),
        "browser.cookies.clear" => {
            browser_cookies(app, &request.params, BrowserCookieAction::Clear)
        }
        "browser.storage.get" => browser_storage(app, &request.params, BrowserStorageAction::Get),
        "browser.storage.set" => browser_storage(app, &request.params, BrowserStorageAction::Set),
        "browser.storage.clear" => {
            browser_storage(app, &request.params, BrowserStorageAction::Clear)
        }
        "browser.tab.list" => browser_tab_list(app, &request.params),
        "browser.tab.new" => browser_tab_new(app, &request.params),
        "browser.tab.switch" => browser_tab_switch(app, &request.params),
        "browser.tab.close" => browser_tab_close(app, &request.params),
        "browser.console.list" => browser_console(app, &request.params, BrowserConsoleAction::List),
        "browser.console.clear" => {
            browser_console(app, &request.params, BrowserConsoleAction::Clear)
        }
        "browser.errors.list" => browser_errors_list(app, &request.params),
        "browser.state.save" => browser_state_save(app, &request.params),
        "browser.state.load" => browser_state_load(app, &request.params),
        "browser.highlight" => browser_highlight(app, &request.params),
        "browser.addinitscript" => browser_add_init_script(app, &request.params),
        "browser.addscript" => browser_addscript(app, &request.params),
        "browser.addstyle" => browser_addstyle(app, &request.params),
        method if is_unported_browser_automation_method(method) => not_supported(&format!(
            "{method} is not yet ported to the Windows/Tauri WebView backend"
        )),
        "browser.reopen_closed" | "browser.reopen_closed_tab" => browser_reopen_closed(app),
        "browser.clear_history" => browser_clear_history(app, &request.params),
        "browser.toggle_omnibar" => browser_toggle_omnibar(app, &request.params),
        "browser.toggle_focus_mode" => browser_toggle_focus_mode(app, &request.params),
        "browser.toggle_developer_tools" => browser_toggle_developer_tools(app, &request.params),
        "browser.show_developer_tools" => browser_show_developer_tools(app, &request.params),
        "browser.set_zoom" => browser_set_zoom(app, &request.params),
        "browser.network.requests" => browser_network_requests(app, &request.params),
        "browser.network.clear" => browser_network_clear(app, &request.params),
        "browser.viewport.set" => {
            not_supported("browser viewport override is not supported by WKWebView")
        }
        "browser.geolocation.set" => {
            not_supported("browser geolocation override is not supported by WKWebView")
        }
        "browser.offline.set" => {
            not_supported("browser offline mode override is not supported by WKWebView")
        }
        "browser.trace.start" | "browser.trace.stop" => {
            not_supported("browser tracing is not supported by WKWebView")
        }
        "browser.network.route" | "browser.network.unroute" => {
            not_supported("browser network request interception is not supported by WKWebView")
        }
        "browser.screencast.start" | "browser.screencast.stop" => {
            not_supported("browser screencast streaming is not supported by WKWebView")
        }
        "browser.input_mouse" | "browser.input_keyboard" | "browser.input_touch" => {
            not_supported("raw browser input injection is not supported by WKWebView")
        }
        "debug.browser.start_direct_proxy" => {
            debug_browser_start_direct_proxy(app, &request.params)
        }
        "debug.browser.attach_webview" => debug_browser_attach_webview(app, &request.params),
        "debug.terminals" => debug_terminals(app, &request.params),
        _ => ControlCallResult::Err {
            code: "method_not_found".to_string(),
            message: format!("Unknown method: {}", request.method),
            data: None,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum LifecycleRuntimeKind {
    Terminal,
    Browser,
}

trait LifecycleRuntimeRegistry {
    fn runtime_ids(&self, kind: LifecycleRuntimeKind, surface_id: &str) -> Vec<u32>;
    fn shutdown_runtime(&mut self, kind: LifecycleRuntimeKind, id: u32) -> Result<(), String>;
    fn remove_runtime(
        &mut self,
        kind: LifecycleRuntimeKind,
        surface_id: &str,
        id: u32,
    ) -> Result<(), String>;
    fn restore_runtime(
        &mut self,
        kind: LifecycleRuntimeKind,
        surface_id: &str,
        id: u32,
    ) -> Result<(), String>;
}

fn combine_failures(failures: Vec<String>) -> Result<(), String> {
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn strict_lifecycle_runtime_teardown(
    registry: &mut impl LifecycleRuntimeRegistry,
    kind: LifecycleRuntimeKind,
    surface_id: &str,
) -> Result<(), String> {
    for id in registry.runtime_ids(kind, surface_id) {
        registry.shutdown_runtime(kind, id)?;
        if let Err(remove_error) = registry.remove_runtime(kind, surface_id, id) {
            let mut failures = vec![remove_error];
            if let Err(restore_error) = registry.restore_runtime(kind, surface_id, id) {
                failures.push(restore_error);
            }
            return combine_failures(failures);
        }
    }
    Ok(())
}

fn commit_terminal_runtime_replacement(
    registry: &mut impl LifecycleRuntimeRegistry,
    surface_id: &str,
    replacement_id: u32,
) -> Result<(), String> {
    let old_ids = registry
        .runtime_ids(LifecycleRuntimeKind::Terminal, surface_id)
        .into_iter()
        .filter(|id| *id != replacement_id)
        .collect::<Vec<_>>();
    for old_id in old_ids {
        if let Err(retirement_error) =
            registry.shutdown_runtime(LifecycleRuntimeKind::Terminal, old_id)
        {
            let mut failures = vec![retirement_error];
            failures.extend(
                compensate_terminal_replacement(registry, surface_id, replacement_id).err(),
            );
            return combine_failures(failures);
        }
        if let Err(remove_error) =
            registry.remove_runtime(LifecycleRuntimeKind::Terminal, surface_id, old_id)
        {
            let mut failures = vec![remove_error];
            failures.extend(
                registry
                    .restore_runtime(LifecycleRuntimeKind::Terminal, surface_id, old_id)
                    .err(),
            );
            failures.extend(
                compensate_terminal_replacement(registry, surface_id, replacement_id).err(),
            );
            return combine_failures(failures);
        }
    }
    Ok(())
}

fn compensate_terminal_replacement(
    registry: &mut impl LifecycleRuntimeRegistry,
    surface_id: &str,
    replacement_id: u32,
) -> Result<(), String> {
    let shutdown_error = registry
        .shutdown_runtime(LifecycleRuntimeKind::Terminal, replacement_id)
        .err();
    let remove_error = registry
        .remove_runtime(LifecycleRuntimeKind::Terminal, surface_id, replacement_id)
        .err();
    combine_failures(shutdown_error.into_iter().chain(remove_error).collect())
}

#[derive(Default)]
struct LifecycleRollbackPlan {
    terminal_ids: Vec<u32>,
    remote_targets: Vec<String>,
    browser_surface_ids: Vec<String>,
}

trait LifecycleRollbackOperations {
    fn rollback_dock(&mut self) -> Result<(), String>;
    fn cleanup_terminal(&mut self, id: u32) -> Result<(), String>;
    fn cleanup_remote(&mut self, target: &str) -> Result<(), String>;
    fn cleanup_browser(&mut self, surface_id: &str) -> Result<(), String>;
}

fn run_lifecycle_rollback_cleanup(
    plan: &LifecycleRollbackPlan,
    operations: &mut impl LifecycleRollbackOperations,
) -> Result<(), String> {
    let mut failures = Vec::new();
    if let Err(error) = operations.rollback_dock() {
        failures.push(error);
    }
    for id in &plan.terminal_ids {
        if let Err(error) = operations.cleanup_terminal(*id) {
            failures.push(error);
        }
    }
    for target in &plan.remote_targets {
        if let Err(error) = operations.cleanup_remote(target) {
            failures.push(error);
        }
    }
    for surface_id in &plan.browser_surface_ids {
        if let Err(error) = operations.cleanup_browser(surface_id) {
            failures.push(error);
        }
    }
    combine_failures(failures)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalCreateRuntimePolicy {
    Eager,
    Deferred,
}

struct ProductionLifecycleExecutor<'a> {
    app: &'a AppHandle,
    terminal_create_runtime_policy: TerminalCreateRuntimePolicy,
    candidate: Option<AppSessionSnapshot>,
    previous: Option<AppSessionSnapshot>,
    staged: Vec<pane_surface_lifecycle::LifecycleEffect>,
    staged_terminals: Vec<(String, u32, bool)>,
    staged_remote_creations: Vec<(StagedRemoteCreation, u64)>,
    deferred_remote_reconciliations: Vec<(StagedRemoteCreation, u64)>,
    deferred_remote_departures: Vec<String>,
    staged_browsers: Vec<(String, String, Option<String>)>,
    dock_journal: DockCommitJournal<DockRuntimeClaim, DockTeardownCompensation>,
}

struct DockTeardownCompensation {
    owner_id: String,
    operation: DockRuntimeOperation,
}

struct ProductionLifecycleRuntimeRegistry<'a> {
    terminal: &'a TerminalState,
    browser: &'a BrowserWebviewState,
    browser_surface_id: Option<&'a str>,
}

impl LifecycleRuntimeRegistry for ProductionLifecycleRuntimeRegistry<'_> {
    fn runtime_ids(&self, kind: LifecycleRuntimeKind, surface_id: &str) -> Vec<u32> {
        match kind {
            LifecycleRuntimeKind::Terminal => {
                terminal_ids_for_panel_for_control(self.terminal, surface_id)
            }
            LifecycleRuntimeKind::Browser => (self.browser_surface_id == Some(surface_id)
                && browser_has_webview_for_control(self.browser, surface_id).unwrap_or(true))
            .then_some(0)
            .into_iter()
            .collect(),
        }
    }

    fn shutdown_runtime(&mut self, kind: LifecycleRuntimeKind, id: u32) -> Result<(), String> {
        match kind {
            LifecycleRuntimeKind::Terminal => {
                terminal_shutdown_id_preserving_authority_for_control(self.terminal, id)
            }
            LifecycleRuntimeKind::Browser => {
                let surface_id = self
                    .browser_surface_id
                    .ok_or_else(|| "browser runtime identity is unavailable".to_string())?;
                browser_close_webview_strict_for_control(self.browser, surface_id)
            }
        }
    }

    fn remove_runtime(
        &mut self,
        kind: LifecycleRuntimeKind,
        surface_id: &str,
        id: u32,
    ) -> Result<(), String> {
        match kind {
            LifecycleRuntimeKind::Terminal => terminal_remove_id_for_control(self.terminal, id),
            LifecycleRuntimeKind::Browser => {
                let _ = (surface_id, id);
                Ok(())
            }
        }
    }

    fn restore_runtime(
        &mut self,
        _kind: LifecycleRuntimeKind,
        _surface_id: &str,
        _id: u32,
    ) -> Result<(), String> {
        // Production removal is the final infallible registry step. If it
        // reports an error, the authoritative entry was not removed.
        Ok(())
    }
}

struct DockCommitJournal<C, T> {
    claims: Vec<C>,
    teardowns: Vec<T>,
    snapshot_committed: bool,
}

enum DockRollbackStep<C, T> {
    RestoreSnapshot,
    RollbackClaim(C),
    RecreateTeardown(T),
}

#[derive(Debug)]
struct DockRollbackErrors<E>(Vec<E>);

impl<E: std::fmt::Display> std::fmt::Display for DockRollbackErrors<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let messages = self.0.iter().map(ToString::to_string).collect::<Vec<_>>();
        write!(formatter, "{}", messages.join("; "))
    }
}

impl<C, T> Default for DockCommitJournal<C, T> {
    fn default() -> Self {
        Self {
            claims: Vec::new(),
            teardowns: Vec::new(),
            snapshot_committed: false,
        }
    }
}

impl<C, T> DockCommitJournal<C, T> {
    fn stage_claim(&mut self, claim: C) {
        self.claims.push(claim);
    }

    fn stage_teardown(&mut self, teardown: T) {
        self.teardowns.push(teardown);
    }

    fn commit_snapshot<E>(&mut self, commit: impl FnOnce() -> Result<(), E>) -> Result<(), E> {
        commit()?;
        self.snapshot_committed = true;
        Ok(())
    }

    fn publish_claims<E>(&self, mut publish: impl FnMut(&C) -> Result<(), E>) -> Result<(), E> {
        for claim in &self.claims {
            publish(claim)?;
        }
        Ok(())
    }

    fn rollback<E>(
        mut self,
        mut apply: impl FnMut(DockRollbackStep<C, T>) -> Result<(), E>,
    ) -> Result<(), DockRollbackErrors<E>> {
        if self.snapshot_committed {
            if let Err(error) = apply(DockRollbackStep::RestoreSnapshot) {
                return Err(DockRollbackErrors(vec![error]));
            }
        }
        let mut errors = Vec::new();
        for claim in self.claims.drain(..) {
            if let Err(error) = apply(DockRollbackStep::RollbackClaim(claim)) {
                errors.push(error);
            }
        }
        for teardown in self.teardowns.drain(..).rev() {
            if let Err(error) = apply(DockRollbackStep::RecreateTeardown(teardown)) {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(DockRollbackErrors(errors))
        }
    }

    fn finish(&mut self) {
        self.claims.clear();
        self.teardowns.clear();
        self.snapshot_committed = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemoteTmuxTarget {
    Pane,
    Window,
}

const REMOTE_RUNTIME_LEASE_TTL: Duration = Duration::from_secs(30);
const REMOTE_RUNTIME_CLEANUP_MAX_RETRIES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RemoteRuntimeLeaseScope {
    endpoint: String,
    session: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteRuntimeSourceWitness {
    window_id: String,
    workspace_id: String,
    pane_id: String,
    surface_id: String,
    surface_generation: u64,
    remote_token: String,
    move_generation: u64,
    restore_epoch: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteRuntimeLeaseDisposition {
    AwaitCommand,
    AwaitCallback,
    ReconcileObservation,
    Publishing,
    Compensate,
    CleanupRetry,
}

#[derive(Debug, Clone)]
struct RemoteRuntimeLease {
    id: u64,
    scope: RemoteRuntimeLeaseScope,
    source: RemoteRuntimeSourceWitness,
    reserved_surface_id: String,
    reserved_pane_id: String,
    target: RemoteTmuxTarget,
    topology_before: Option<Vec<RemoteTmuxTopologyEntry>>,
    remote_target_token: Option<String>,
    expires_at: Instant,
    disposition: RemoteRuntimeLeaseDisposition,
    cleanup_attempts: usize,
}

#[derive(Default)]
struct RemoteRuntimeLeaseRegistry {
    next_id: u64,
    restore_epoch: u64,
    leases: BTreeMap<u64, RemoteRuntimeLease>,
}

#[derive(Default)]
pub struct RemoteRuntimeLeaseRegistryState {
    registry: Mutex<RemoteRuntimeLeaseRegistry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteRuntimeCommandOutcome {
    Succeeded,
    Failed,
    Unknown,
}

fn remote_runtime_process_error_outcome(child_started: bool) -> RemoteRuntimeCommandOutcome {
    if child_started {
        RemoteRuntimeCommandOutcome::Unknown
    } else {
        RemoteRuntimeCommandOutcome::Failed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteRuntimeCallbackClaim {
    Publish,
    Duplicate,
    Stale,
}

impl RemoteRuntimeLeaseRegistry {
    fn reserve(
        &mut self,
        scope: RemoteRuntimeLeaseScope,
        mut source: RemoteRuntimeSourceWitness,
        reserved_surface_id: String,
        reserved_pane_id: String,
        target: RemoteTmuxTarget,
    ) -> u64 {
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("remote runtime lease id space exhausted");
        source.restore_epoch = self.restore_epoch;
        let id = self.next_id;
        self.leases.insert(
            id,
            RemoteRuntimeLease {
                id,
                scope,
                source,
                reserved_surface_id,
                reserved_pane_id,
                target,
                topology_before: None,
                remote_target_token: None,
                expires_at: Instant::now() + REMOTE_RUNTIME_LEASE_TTL,
                disposition: RemoteRuntimeLeaseDisposition::AwaitCommand,
                cleanup_attempts: 0,
            },
        );
        id
    }

    fn record_command_outcome(
        &mut self,
        id: u64,
        outcome: RemoteRuntimeCommandOutcome,
        token: Option<String>,
    ) -> bool {
        let Some(lease) = self.leases.get_mut(&id) else {
            return false;
        };
        if lease.disposition != RemoteRuntimeLeaseDisposition::AwaitCommand {
            if outcome == RemoteRuntimeCommandOutcome::Succeeded
                && matches!(
                    lease.disposition,
                    RemoteRuntimeLeaseDisposition::Compensate
                        | RemoteRuntimeLeaseDisposition::CleanupRetry
                )
                && token.is_some()
            {
                lease.remote_target_token = token;
            }
            return false;
        }
        if outcome == RemoteRuntimeCommandOutcome::Failed {
            self.leases.remove(&id);
            return true;
        }
        if token.is_some() {
            lease.remote_target_token = token;
        }
        lease.disposition = match outcome {
            RemoteRuntimeCommandOutcome::Succeeded => RemoteRuntimeLeaseDisposition::AwaitCallback,
            RemoteRuntimeCommandOutcome::Failed => unreachable!("handled before lease lookup"),
            RemoteRuntimeCommandOutcome::Unknown => {
                RemoteRuntimeLeaseDisposition::ReconcileObservation
            }
        };
        true
    }

    fn claim_callback(
        &mut self,
        id: u64,
        scope: &RemoteRuntimeLeaseScope,
        source: &RemoteRuntimeSourceWitness,
        reserved_surface_id: &str,
        reserved_pane_id: &str,
    ) -> RemoteRuntimeCallbackClaim {
        let Some(lease) = self.leases.get_mut(&id) else {
            return RemoteRuntimeCallbackClaim::Stale;
        };
        if lease.disposition == RemoteRuntimeLeaseDisposition::Publishing {
            return RemoteRuntimeCallbackClaim::Duplicate;
        }
        let exact = lease.id == id
            && &lease.scope == scope
            && &lease.source == source
            && lease.source.restore_epoch == self.restore_epoch
            && lease.reserved_surface_id == reserved_surface_id
            && lease.reserved_pane_id == reserved_pane_id
            && Instant::now() < lease.expires_at
            && matches!(
                lease.disposition,
                RemoteRuntimeLeaseDisposition::AwaitCallback
                    | RemoteRuntimeLeaseDisposition::ReconcileObservation
            );
        if !exact {
            return RemoteRuntimeCallbackClaim::Stale;
        }
        lease.disposition = RemoteRuntimeLeaseDisposition::Publishing;
        lease.expires_at = Instant::now() + REMOTE_RUNTIME_LEASE_TTL;
        RemoteRuntimeCallbackClaim::Publish
    }

    fn record_callback_result(&mut self, id: u64, succeeded: bool) {
        let Some(lease) = self.leases.get(&id) else {
            return;
        };
        if lease.disposition != RemoteRuntimeLeaseDisposition::Publishing {
            return;
        }
        if succeeded {
            self.leases.remove(&id);
        } else if let Some(lease) = self.leases.get_mut(&id) {
            lease.disposition = RemoteRuntimeLeaseDisposition::AwaitCallback;
        }
    }

    fn expire(&mut self, id: u64, now: Instant) -> Option<RemoteRuntimeLease> {
        let lease = self.leases.get(&id)?;
        if now < lease.expires_at {
            return None;
        }
        if lease.disposition == RemoteRuntimeLeaseDisposition::Publishing {
            return None;
        }
        let lease = self.leases.get_mut(&id)?;
        lease.disposition = RemoteRuntimeLeaseDisposition::Compensate;
        Some(lease.clone())
    }

    fn compensate(&mut self, id: u64) -> Option<RemoteRuntimeLease> {
        let lease = self.leases.get_mut(&id)?;
        lease.disposition = RemoteRuntimeLeaseDisposition::Compensate;
        Some(lease.clone())
    }

    fn record_cleanup_result(&mut self, id: u64, succeeded: bool) -> Option<usize> {
        if succeeded {
            self.leases.remove(&id);
            return None;
        }
        let lease = self.leases.get_mut(&id)?;
        lease.cleanup_attempts = lease.cleanup_attempts.saturating_add(1);
        let attempts = lease.cleanup_attempts;
        if attempts > REMOTE_RUNTIME_CLEANUP_MAX_RETRIES {
            self.leases.remove(&id);
            return None;
        }
        lease.disposition = RemoteRuntimeLeaseDisposition::CleanupRetry;
        Some(attempts)
    }
}

fn reserve_remote_runtime_lease(
    state: &RemoteRuntimeLeaseRegistryState,
    scope: RemoteRuntimeLeaseScope,
    source: RemoteRuntimeSourceWitness,
    reserved_surface_id: String,
    reserved_pane_id: String,
    target: RemoteTmuxTarget,
    topology_before: Vec<RemoteTmuxTopologyEntry>,
) -> Result<u64, String> {
    state
        .registry
        .lock()
        .map_err(|_| "remote runtime lease registry lock poisoned".to_string())
        .and_then(|mut registry| {
            if registry
                .leases
                .values()
                .any(|lease| lease.scope == scope && lease.remote_target_token.is_none())
            {
                return Err(
                    "remote runtime topology reconciliation is already pending for this session"
                        .into(),
                );
            }
            let id = registry.reserve(scope, source, reserved_surface_id, reserved_pane_id, target);
            if !topology_before.is_empty() {
                if let Some(lease) = registry.leases.get_mut(&id) {
                    lease.topology_before = Some(topology_before);
                }
            }
            Ok(id)
        })
}

fn expire_remote_runtime_lease(app: &AppHandle, id: u64) {
    let state = app.state::<RemoteRuntimeLeaseRegistryState>();
    let expired = state
        .registry
        .lock()
        .ok()
        .and_then(|mut registry| registry.expire(id, Instant::now()));
    let Some(lease) = expired else {
        return;
    };
    settle_remote_runtime_cleanup(app, lease);
}

fn compensate_remote_runtime_lease(app: &AppHandle, id: u64) -> bool {
    let state = app.state::<RemoteRuntimeLeaseRegistryState>();
    let lease = state
        .registry
        .lock()
        .ok()
        .and_then(|mut registry| registry.compensate(id));
    let Some(lease) = lease else {
        return false;
    };
    settle_remote_runtime_cleanup(app, lease)
}

fn settle_remote_runtime_cleanup(app: &AppHandle, remote: RemoteRuntimeLease) -> bool {
    let observed_token = remote_runtime_compensation_token(&remote, &[]);
    let cleaned = observed_token.as_deref().is_some_and(|token| {
        remote_tmux_kill_command(remote.target, token)
            .and_then(|command| run_remote_tmux_command(&remote.scope.endpoint, command))
            .is_ok()
    });
    let retry = app
        .state::<RemoteRuntimeLeaseRegistryState>()
        .registry
        .lock()
        .ok()
        .and_then(|mut registry| registry.record_cleanup_result(remote.id, cleaned));
    if let Some(attempt) = retry.filter(|attempt| *attempt <= REMOTE_RUNTIME_CLEANUP_MAX_RETRIES) {
        schedule_remote_runtime_cleanup_retry(app, remote.id, attempt);
    }
    cleaned
}

fn schedule_remote_runtime_cleanup_retry(app: &AppHandle, id: u64, attempt: usize) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let exponent = attempt.saturating_sub(1).min(3) as u32;
        thread::sleep(Duration::from_millis(250 * 2_u64.pow(exponent)));
        let _ = compensate_remote_runtime_lease(&app, id);
    });
}

fn schedule_remote_runtime_lease_watchdog(app: &AppHandle, id: u64) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        thread::sleep(REMOTE_RUNTIME_LEASE_TTL);
        expire_remote_runtime_lease(&app, id);
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
enum RemoteTmuxSplitDirection {
    #[serde(rename = "-h")]
    Horizontal,
    #[serde(rename = "-v")]
    Vertical,
}

impl RemoteTmuxSplitDirection {
    fn tmux_flag(self) -> &'static str {
        match self {
            Self::Horizontal => "-h",
            Self::Vertical => "-v",
        }
    }
}

fn tmux_split_flag(split_direction: RemoteTmuxSplitDirection) -> &'static str {
    split_direction.tmux_flag()
}

impl RemoteTmuxTarget {
    fn for_create(operation: &str) -> Result<Self, String> {
        match operation {
            "split-window" => Ok(Self::Pane),
            "new-window" => Ok(Self::Window),
            _ => Err(format!("unsupported remote tmux operation: {operation}")),
        }
    }

    fn rollback_operation(self) -> &'static str {
        match self {
            Self::Pane => "kill-pane",
            Self::Window => "kill-window",
        }
    }

    #[cfg(test)]
    fn permits_immediate_arrival(self, arrival_policy: &str) -> bool {
        self == Self::Pane && arrival_policy == "runtime-pane-add"
    }

    fn identity_prefix(self) -> char {
        match self {
            Self::Pane => '%',
            Self::Window => '@',
        }
    }
}

fn remote_tmux_kill_command(target: RemoteTmuxTarget, token: &str) -> Result<Vec<String>, String> {
    if !valid_tmux_identity(token, target.identity_prefix()) {
        return Err("invalid tmux rollback identity".into());
    }
    Ok(vec![format!(
        "tmux {} -t {}",
        target.rollback_operation(),
        shell_quote_remote(token)?,
    )])
}

struct RemoteTmuxCreateSpec<'a> {
    operation: &'a str,
    focus: bool,
    source_target: Option<&'a str>,
    working_directory: Option<&'a str>,
}

#[cfg(test)]
fn remote_tmux_create_argv(spec: &RemoteTmuxCreateSpec<'_>) -> Result<Vec<String>, String> {
    remote_tmux_create_argv_with_split(spec, None)
}

fn remote_tmux_create_argv_with_split(
    spec: &RemoteTmuxCreateSpec<'_>,
    split_direction: Option<RemoteTmuxSplitDirection>,
) -> Result<Vec<String>, String> {
    let target = RemoteTmuxTarget::for_create(spec.operation)?;
    let mut parts = vec!["tmux".to_string(), spec.operation.to_string()];
    if target == RemoteTmuxTarget::Window && !spec.focus {
        parts.push("-d".into());
    }
    if target == RemoteTmuxTarget::Window {
        let source = spec.source_target.unwrap_or("{end}");
        if source != "{end}" && !valid_tmux_identity(source, '@') {
            return Err("invalid tmux source window identity".into());
        }
        parts.extend(["-a".into(), "-t".into(), shell_quote_remote(source)?]);
        if spec.source_target.is_some() {
            if let Some(directory) = usable_remote_working_directory(spec.working_directory) {
                parts.extend(["-c".into(), shell_quote_remote(directory)?]);
            }
        }
    } else if let Some(source) = spec.source_target {
        let valid_source = valid_tmux_split_target(source);
        if !valid_source && split_direction.is_some() {
            return Err("invalid tmux split target".into());
        }
        if valid_source {
            let direction = split_direction.unwrap_or(RemoteTmuxSplitDirection::Horizontal);
            parts.extend([
                tmux_split_flag(direction).into(),
                "-t".into(),
                shell_quote_remote(source)?,
            ]);
        }
    }
    parts.extend([
        "-P".into(),
        "-F".into(),
        quote_tmux_format(match target {
            RemoteTmuxTarget::Pane => "#{pane_id}",
            RemoteTmuxTarget::Window => "#{window_id}\t#{pane_id}",
        }),
    ]);
    Ok(vec![parts.join(" ")])
}

fn usable_remote_working_directory(value: Option<&str>) -> Option<&str> {
    let value = value?.trim();
    (!value.is_empty() && !value.chars().any(|character| character.is_control())).then_some(value)
}

fn remote_tmux_source_window_command(pane_token: &str) -> Result<Vec<String>, String> {
    if !valid_tmux_identity(pane_token, '%') {
        return Err("invalid tmux source pane identity".into());
    }
    Ok(vec![format!(
        "tmux display-message -p -t {} {}",
        shell_quote_remote(pane_token)?,
        quote_tmux_format("#{window_id}"),
    )])
}

fn remote_tmux_rename_window_command(
    window_token: &str,
    title: &str,
) -> Result<Vec<String>, String> {
    if !valid_tmux_identity(window_token, '@') {
        return Err("invalid tmux window identity".into());
    }
    Ok(vec![format!(
        "tmux rename-window -t {} {}",
        shell_quote_remote(window_token)?,
        shell_quote_remote(title)?,
    )])
}

fn remote_tmux_list_windows_command() -> Vec<String> {
    vec!["tmux list-windows -F '#{window_id}'".into()]
}

fn parse_remote_tmux_window_ids(output: &str) -> Result<Vec<String>, String> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    let normalized = output.replace("\r\n", "\n");
    if normalized.contains('\r') || !normalized.ends_with('\n') {
        return Err("invalid tmux window list output".into());
    }
    normalized
        .lines()
        .map(|line| {
            valid_tmux_identity(line, '@')
                .then(|| line.to_string())
                .ok_or_else(|| "invalid tmux window list identity".into())
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingRemoteWindowDeparture {
    destination: String,
    remote_window_id: String,
    departure: pane_surface_lifecycle::RuntimeDeparture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteWindowPresenceObservation {
    Present,
    Absent,
    QueryFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteTmuxCommandOutput {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

fn classify_remote_tmux_window_presence(
    remote_window_id: &str,
    output: Result<RemoteTmuxCommandOutput, String>,
) -> RemoteWindowPresenceObservation {
    let Ok(output) = output else {
        return RemoteWindowPresenceObservation::QueryFailed;
    };
    if output.exit_code == 0 {
        return parse_remote_tmux_window_ids(&output.stdout)
            .map(|window_ids| {
                if window_ids
                    .iter()
                    .any(|window_id| window_id == remote_window_id)
                {
                    RemoteWindowPresenceObservation::Present
                } else {
                    RemoteWindowPresenceObservation::Absent
                }
            })
            .unwrap_or(RemoteWindowPresenceObservation::QueryFailed);
    }
    let no_server = output.stderr.strip_suffix('\n').unwrap_or(&output.stderr);
    if output.exit_code == 1
        && output.stdout.is_empty()
        && no_server
            .strip_prefix("no server running on ")
            .is_some_and(|socket| !socket.is_empty() && !socket.contains(['\r', '\n']))
    {
        return RemoteWindowPresenceObservation::Absent;
    }
    RemoteWindowPresenceObservation::QueryFailed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteWindowDepartureAction {
    RetainAndPoll,
    CommitDeparture,
}

#[derive(Default)]
struct RemoteWindowDepartureRegistry {
    next_key: u64,
    pending: BTreeMap<String, PendingRemoteWindowDeparture>,
    identities: BTreeMap<String, RemoteWindowDepartureIdentity>,
    retry: BTreeMap<String, RemoteWindowDepartureRetryState>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RemoteWindowDepartureIdentity {
    scope: RemoteRuntimeLeaseScope,
    surface_id: String,
    generation: u64,
}

#[derive(Default)]
struct RemoteWindowDepartureRetryState {
    attempts: usize,
    next_failure_report_at: Option<Instant>,
}

impl RemoteWindowDepartureRegistry {
    #[cfg(test)]
    fn register(&mut self, pending: PendingRemoteWindowDeparture) -> String {
        let session = pending.remote_window_id.clone();
        self.register_scoped(pending, session)
    }

    fn register_scoped(
        &mut self,
        pending: PendingRemoteWindowDeparture,
        session: String,
    ) -> String {
        let identity = RemoteWindowDepartureIdentity {
            scope: RemoteRuntimeLeaseScope {
                endpoint: pending.destination.clone(),
                session,
            },
            surface_id: pending.departure.surface_id.clone(),
            generation: pending.departure.generation,
        };
        if let Some((key, _)) = self
            .identities
            .iter()
            .find(|(_, current)| *current == &identity)
        {
            return key.clone();
        }
        self.next_key = self.next_key.saturating_add(1);
        let key = format!("remote-window-departure-{}", self.next_key);
        self.pending.insert(key.clone(), pending);
        self.identities.insert(key.clone(), identity);
        self.retry.insert(key.clone(), Default::default());
        key
    }

    fn get(&self, key: &str) -> Option<&PendingRemoteWindowDeparture> {
        self.pending.get(key)
    }

    #[cfg(test)]
    fn contains(&self, key: &str) -> bool {
        self.pending.contains_key(key)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.pending.len()
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    fn record_observation(
        &mut self,
        key: &str,
        observation: RemoteWindowPresenceObservation,
    ) -> RemoteWindowDepartureAction {
        if !self.pending.contains_key(key) {
            return RemoteWindowDepartureAction::RetainAndPoll;
        }
        match observation {
            RemoteWindowPresenceObservation::Absent => RemoteWindowDepartureAction::CommitDeparture,
            RemoteWindowPresenceObservation::Present
            | RemoteWindowPresenceObservation::QueryFailed => {
                if let Some(retry) = self.retry.get_mut(key) {
                    retry.attempts = retry.attempts.saturating_add(1);
                }
                RemoteWindowDepartureAction::RetainAndPoll
            }
        }
    }

    fn record_commit_result(
        &mut self,
        key: &str,
        result: Result<RuntimeDepartureCommitOutcome, String>,
    ) -> bool {
        if result.is_ok() {
            self.identities.remove(key);
            self.retry.remove(key);
            self.pending.remove(key).is_some()
        } else {
            if let Some(retry) = self.retry.get_mut(key) {
                retry.attempts = retry.attempts.saturating_add(1);
            }
            false
        }
    }

    fn retry_attempts(&self, key: &str) -> Option<usize> {
        self.retry.get(key).map(|retry| retry.attempts)
    }

    fn retry_delay(&self, key: &str) -> Option<Duration> {
        let attempts = self.retry_attempts(key)?.min(6) as u32;
        Some(Duration::from_millis(250 * 2_u64.pow(attempts)))
    }

    fn take_failure_report_permit(&mut self, key: &str) -> bool {
        let now = Instant::now();
        let Some(retry) = self.retry.get_mut(key) else {
            return false;
        };
        if retry.next_failure_report_at.is_some_and(|next| now < next) {
            return false;
        }
        retry.next_failure_report_at = Some(now + Duration::from_secs(30));
        true
    }

    fn pending_failure_payload(&self, key: &str, message: &str) -> Option<Value> {
        let pending = self.pending.get(key)?;
        Some(json!({
            "surface_id": pending.departure.surface_id,
            "remote_window_id": pending.remote_window_id,
            "message": message,
            "pending_reconciliation": true,
        }))
    }
}

#[cfg(test)]
fn retain_remote_window_departure_after_kill(
    registry: &mut RemoteWindowDepartureRegistry,
    destination: &str,
    remote_window_id: &str,
    departure: pane_surface_lifecycle::RuntimeDeparture,
) -> Result<String, String> {
    retain_scoped_remote_window_departure_after_kill(
        registry,
        destination,
        remote_window_id,
        remote_window_id,
        departure,
    )
}

fn retain_scoped_remote_window_departure_after_kill(
    registry: &mut RemoteWindowDepartureRegistry,
    destination: &str,
    remote_session_id: &str,
    remote_window_id: &str,
    departure: pane_surface_lifecycle::RuntimeDeparture,
) -> Result<String, String> {
    if !valid_tmux_identity(remote_window_id, '@') {
        return Err("invalid killed tmux window identity".into());
    }
    Ok(registry.register_scoped(
        PendingRemoteWindowDeparture {
            destination: destination.into(),
            remote_window_id: remote_window_id.into(),
            departure,
        },
        remote_session_id.into(),
    ))
}

#[cfg(test)]
fn execute_remote_window_kill_and_register<F>(
    registry: &Mutex<RemoteWindowDepartureRegistry>,
    destination: &str,
    source_pane: &str,
    departure: pane_surface_lifecycle::RuntimeDeparture,
    kill: F,
) -> Result<String, String>
where
    F: FnOnce(&str, &str) -> Result<String, String>,
{
    execute_scoped_remote_window_kill_and_register(
        registry,
        destination,
        destination,
        source_pane,
        departure,
        kill,
    )
}

fn execute_scoped_remote_window_kill_and_register<F>(
    registry: &Mutex<RemoteWindowDepartureRegistry>,
    destination: &str,
    remote_session_id: &str,
    source_pane: &str,
    departure: pane_surface_lifecycle::RuntimeDeparture,
    kill: F,
) -> Result<String, String>
where
    F: FnOnce(&str, &str) -> Result<String, String>,
{
    if !valid_tmux_identity(source_pane, '%') {
        return Err("invalid remote tmux pane identity".into());
    }
    let remote_window_id = kill(destination, source_pane)?;
    let mut registry = registry
        .lock()
        .map_err(|_| "remote departure registry lock poisoned".to_string())?;
    retain_scoped_remote_window_departure_after_kill(
        &mut registry,
        destination,
        remote_session_id,
        &remote_window_id,
        departure,
    )
}

#[derive(Default)]
pub struct RemoteWindowDepartureRegistryState {
    registry: Mutex<RemoteWindowDepartureRegistry>,
}

fn run_remote_tmux_command_output(
    destination: &str,
    command: Vec<String>,
) -> Result<RemoteTmuxCommandOutput, String> {
    let output = Command::new("ssh")
        .args(["-T", "-o", "BatchMode=yes", destination])
        .args(command)
        .output()
        .map_err(|error| error.to_string())?;
    Ok(RemoteTmuxCommandOutput {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    })
}

fn run_remote_tmux_command(destination: &str, command: Vec<String>) -> Result<String, String> {
    let output = run_remote_tmux_command_output(destination, command)?;
    if output.exit_code != 0 {
        return Err(format!(
            "remote tmux command exited with {}",
            output.exit_code
        ));
    }
    Ok(output.stdout)
}

fn remote_tmux_window_for_pane(destination: &str, pane_token: &str) -> Result<String, String> {
    let output =
        run_remote_tmux_command(destination, remote_tmux_source_window_command(pane_token)?)?;
    let window_token = output.trim().to_string();
    valid_tmux_identity(&window_token, '@')
        .then_some(window_token)
        .ok_or_else(|| "invalid remote tmux window observation".into())
}

fn execute_remote_tmux_window_mutation(
    destination: &str,
    pane_token: &str,
    command: impl FnOnce(&str) -> Result<Vec<String>, String>,
) -> Result<String, String> {
    let window_token = remote_tmux_window_for_pane(destination, pane_token)?;
    run_remote_tmux_command(destination, command(&window_token)?)?;
    Ok(window_token)
}

fn shell_quote_remote(value: &str) -> Result<String, String> {
    if value.chars().any(|character| character.is_control()) {
        return Err("remote shell value contains control characters".into());
    }
    Ok(format!("'{}'", value.replace('\'', r#"'"'"'"#)))
}

fn quote_tmux_format(value: &str) -> String {
    format!("'{value}'")
}

fn valid_tmux_identity(value: &str, prefix: char) -> bool {
    value.strip_prefix(prefix).is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

fn valid_tmux_split_target(value: &str) -> bool {
    value.split_once('.').is_some_and(|(window, pane)| {
        valid_tmux_identity(window, '@') && valid_tmux_identity(pane, '%')
    })
}

fn parse_remote_tmux_pane_observation(output: &str) -> Result<String, String> {
    let line = output
        .strip_suffix("\r\n")
        .or_else(|| output.strip_suffix('\n'))
        .unwrap_or(output);
    if line.contains(['\r', '\n']) || !valid_tmux_identity(line, '%') {
        return Err("remote tmux split returned invalid pane observation".into());
    }
    Ok(line.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteTmuxObservation {
    window_token: String,
    pane_token: String,
}

fn parse_remote_tmux_observation(output: &str) -> Result<RemoteTmuxObservation, String> {
    let line = output
        .strip_suffix("\r\n")
        .or_else(|| output.strip_suffix('\n'))
        .unwrap_or(output);
    if line.is_empty()
        || line.contains(['\r', '\n'])
        || line
            .chars()
            .any(|character| character.is_control() && character != '\t')
    {
        return Err("remote tmux create returned invalid window observation".into());
    }
    let (window_token, pane_token) = line.split_once('\t').ok_or_else(|| {
        "remote tmux create returned no authoritative window observation".to_string()
    })?;
    if !valid_tmux_identity(window_token, '@') || !valid_tmux_identity(pane_token, '%') {
        return Err("remote tmux create returned invalid window observation".into());
    }
    Ok(RemoteTmuxObservation {
        window_token: window_token.into(),
        pane_token: pane_token.into(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteTmuxTopologyEntry {
    window_id: String,
    pane_id: String,
}

fn remote_tmux_topology_command() -> Vec<String> {
    vec!["tmux list-panes -a -F '#{window_id}\t#{pane_id}'".into()]
}

fn parse_remote_tmux_topology(output: &str) -> Result<Vec<RemoteTmuxTopologyEntry>, String> {
    if output.is_empty() {
        return Ok(Vec::new());
    }
    let normalized = output.replace("\r\n", "\n");
    if normalized.contains('\r') || !normalized.ends_with('\n') {
        return Err("invalid remote tmux topology observation".into());
    }
    normalized
        .lines()
        .map(|line| {
            let (window_id, pane_id) = line
                .split_once('\t')
                .ok_or_else(|| "invalid remote tmux topology observation".to_string())?;
            if !valid_tmux_identity(window_id, '@') || !valid_tmux_identity(pane_id, '%') {
                return Err("invalid remote tmux topology observation".into());
            }
            Ok(RemoteTmuxTopologyEntry {
                window_id: window_id.into(),
                pane_id: pane_id.into(),
            })
        })
        .collect()
}

fn observe_remote_tmux_topology(destination: &str) -> Result<Vec<RemoteTmuxTopologyEntry>, String> {
    let output = run_remote_tmux_command(destination, remote_tmux_topology_command())?;
    parse_remote_tmux_topology(&output)
}

fn created_remote_tmux_topology_entry(
    target: RemoteTmuxTarget,
    before: &[RemoteTmuxTopologyEntry],
    after: &[RemoteTmuxTopologyEntry],
) -> Result<RemoteTmuxTopologyEntry, String> {
    let created = after
        .iter()
        .filter(|candidate| {
            !before.iter().any(|existing| match target {
                RemoteTmuxTarget::Pane => existing.pane_id == candidate.pane_id,
                RemoteTmuxTarget::Window => existing.window_id == candidate.window_id,
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    match created.as_slice() {
        [entry] => Ok(entry.clone()),
        _ => Err("remote command outcome unknown after topology reconciliation".into()),
    }
}

fn remote_runtime_compensation_token(
    remote: &RemoteRuntimeLease,
    _topology_after: &[RemoteTmuxTopologyEntry],
) -> Option<String> {
    remote.remote_target_token.clone()
}

fn resolve_remote_tmux_create_observation<F>(
    target: RemoteTmuxTarget,
    output: &str,
    topology_before: &[RemoteTmuxTopologyEntry],
    observe: F,
) -> Result<(Option<RemoteTmuxObservation>, Option<String>), String>
where
    F: FnOnce() -> Result<Vec<RemoteTmuxTopologyEntry>, String>,
{
    let raw_output = output.to_string();
    let parsed = match target {
        RemoteTmuxTarget::Window => {
            parse_remote_tmux_observation(&raw_output).map(|observation| (Some(observation), None))
        }
        RemoteTmuxTarget::Pane => {
            parse_remote_tmux_pane_observation(&raw_output).map(|pane| (None, Some(pane)))
        }
    };
    if let Ok(parsed) = parsed {
        return Ok(parsed);
    }
    let created = created_remote_tmux_topology_entry(target, topology_before, &observe()?)?;
    Ok(match target {
        RemoteTmuxTarget::Window => (
            Some(RemoteTmuxObservation {
                window_token: created.window_id,
                pane_token: created.pane_id,
            }),
            None,
        ),
        RemoteTmuxTarget::Pane => (None, Some(created.pane_id)),
    })
}

const REMOTE_OBSERVATION_MAX_RETRIES: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemoteObservationAction {
    Reconcile,
    RetainAndRetry,
    CompensateKillWindow,
}

fn remote_observation_action(
    source_exists: bool,
    commit_error: Option<&str>,
    attempts: usize,
) -> RemoteObservationAction {
    if !source_exists
        || attempts > REMOTE_OBSERVATION_MAX_RETRIES
        || (commit_error.is_some() && attempts >= REMOTE_OBSERVATION_MAX_RETRIES)
    {
        RemoteObservationAction::CompensateKillWindow
    } else if commit_error.is_some_and(|error| error.contains("command outcome unknown")) {
        RemoteObservationAction::Reconcile
    } else if commit_error.is_some() {
        RemoteObservationAction::RetainAndRetry
    } else {
        RemoteObservationAction::Reconcile
    }
}

fn lifecycle_snapshot_changed(
    candidate: &AppSessionSnapshot,
    previous: &AppSessionSnapshot,
) -> bool {
    candidate != previous
}

#[cfg(test)]
fn immediate_remote_arrival(
    target: RemoteTmuxTarget,
    arrival_policy: &str,
    window_id: &str,
    workspace_id: &str,
    token: &str,
) -> Option<pane_surface_lifecycle::RuntimeArrival> {
    target.permits_immediate_arrival(arrival_policy).then(|| {
        pane_surface_lifecycle::RuntimeArrival::remote(
            window_id,
            workspace_id,
            Uuid::new_v4().to_string(),
            Uuid::new_v4().to_string(),
            token,
            1,
        )
    })
}

#[derive(Clone)]
struct StagedRemoteCreation {
    target: RemoteTmuxTarget,
    token: String,
    window_id: String,
    workspace_id: String,
    target_pane_id: Option<String>,
    source_surface_id: Option<String>,
    source_pane_id: Option<String>,
    split_orientation: Option<SessionSplitOrientation>,
    focus: bool,
    observation: Option<RemoteTmuxObservation>,
    pane_observation: Option<String>,
    arrival: Option<pane_surface_lifecycle::RuntimeArrival>,
}

fn remote_runtime_source_witness(
    snapshot: &AppSessionSnapshot,
    window_id: &str,
    workspace_id: &str,
    source_surface_id: &str,
    restore_epoch: u64,
) -> Option<RemoteRuntimeSourceWitness> {
    let window = snapshot
        .windows
        .iter()
        .find(|window| window.window_id.as_deref() == Some(window_id))?;
    let workspace = window
        .tab_manager
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))?;
    let surface = workspace
        .surfaces
        .as_ref()?
        .iter()
        .find(|surface| surface.surface_id == source_surface_id)?;
    let SessionSurfaceKindSnapshot::RemoteTerminal {
        remote_session_id: Some(remote_token),
        ..
    } = &surface.kind
    else {
        return None;
    };
    Some(RemoteRuntimeSourceWitness {
        window_id: window_id.into(),
        workspace_id: workspace_id.into(),
        pane_id: surface.pane_id.clone(),
        surface_id: source_surface_id.into(),
        surface_generation: surface.generation,
        remote_token: remote_token.clone(),
        move_generation: surface.generation,
        restore_epoch,
    })
}

fn remote_runtime_source_is_current(
    snapshot: &AppSessionSnapshot,
    witness: &RemoteRuntimeSourceWitness,
) -> bool {
    remote_runtime_source_witness(
        snapshot,
        &witness.window_id,
        &witness.workspace_id,
        &witness.surface_id,
        witness.restore_epoch,
    )
    .as_ref()
        == Some(witness)
}

fn validate_remote_runtime_mutation_fence(
    registry: &mut RemoteRuntimeLeaseRegistry,
    id: u64,
    snapshot: &AppSessionSnapshot,
) -> Result<(), String> {
    let Some(lease) = registry.leases.get(&id) else {
        return Err("remote runtime lease expired before mutation".into());
    };
    if lease.disposition == RemoteRuntimeLeaseDisposition::AwaitCommand
        && lease.source.restore_epoch == registry.restore_epoch
        && remote_runtime_source_is_current(snapshot, &lease.source)
    {
        return Ok(());
    }
    registry.leases.remove(&id);
    Err("remote runtime source changed before mutation".into())
}

fn observed_remote_window_arrival(
    remote: &StagedRemoteCreation,
    pane_token: &str,
) -> Option<pane_surface_lifecycle::RuntimeArrival> {
    if remote.target != RemoteTmuxTarget::Window || remote.arrival.is_some() {
        return None;
    }
    Some(pane_surface_lifecycle::RuntimeArrival::remote_tab(
        &remote.window_id,
        &remote.workspace_id,
        remote.target_pane_id.as_ref()?,
        Uuid::new_v4().to_string(),
        pane_token,
        1,
        remote.source_surface_id.as_ref()?,
        remote.focus,
    ))
}

fn staged_remote_arrival(
    remote: &StagedRemoteCreation,
) -> Option<pane_surface_lifecycle::RuntimeArrival> {
    if let Some(arrival) = &remote.arrival {
        return Some(arrival.clone());
    }
    if let Some(observation) = &remote.observation {
        return observed_remote_window_arrival(remote, &observation.pane_token);
    }
    let pane_token = remote.pane_observation.as_ref()?;
    let mut arrival = pane_surface_lifecycle::RuntimeArrival::remote(
        &remote.window_id,
        &remote.workspace_id,
        Uuid::new_v4().to_string(),
        Uuid::new_v4().to_string(),
        pane_token,
        1,
    );
    arrival.anchor_surface_id = remote.source_surface_id.clone();
    arrival.source_pane_id = remote.source_pane_id.clone();
    arrival.split_orientation = remote.split_orientation.clone();
    arrival.focused = remote.focus;
    Some(arrival)
}

fn schedule_remote_window_reconciliation(
    app: &AppHandle,
    remote: StagedRemoteCreation,
    lease_id: u64,
) {
    let compensation_token = remote.token.clone();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(arrival) = staged_remote_arrival(&remote) else {
            return;
        };
        let lease = app
            .state::<RemoteRuntimeLeaseRegistryState>()
            .registry
            .lock()
            .ok()
            .and_then(|registry| registry.leases.get(&lease_id).cloned());
        let Some(lease) = lease else {
            return;
        };
        let mut attempt = 0;
        let failure = loop {
            let current = snapshot(&app);
            if !remote_runtime_source_is_current(&current, &lease.source) {
                break "Remote source tab disappeared before runtime arrival".to_string();
            }
            let claim = app
                .state::<RemoteRuntimeLeaseRegistryState>()
                .registry
                .lock()
                .map(|mut registry| {
                    registry.claim_callback(
                        lease.id,
                        &lease.scope,
                        &lease.source,
                        &lease.reserved_surface_id,
                        &lease.reserved_pane_id,
                    )
                })
                .unwrap_or(RemoteRuntimeCallbackClaim::Stale);
            match claim {
                RemoteRuntimeCallbackClaim::Duplicate => return,
                RemoteRuntimeCallbackClaim::Stale => {
                    break "Remote runtime arrival lease is stale".to_string()
                }
                RemoteRuntimeCallbackClaim::Publish => {}
            }
            schedule_remote_runtime_lease_watchdog(&app, lease.id);
            let commit = commit_runtime_arrival_for_control(&app, arrival.clone());
            let committed = matches!(
                commit,
                Ok(RuntimeArrivalCommitOutcome::Committed)
                    | Ok(RuntimeArrivalCommitOutcome::DuplicateOrStale)
            );
            if let Ok(mut registry) = app
                .state::<RemoteRuntimeLeaseRegistryState>()
                .registry
                .lock()
            {
                registry.record_callback_result(lease.id, committed);
            }
            match commit {
                Ok(outcome) => match outcome {
                    RuntimeArrivalCommitOutcome::Committed => {
                        if remote.target == RemoteTmuxTarget::Window
                            && should_focus_window_after_remote_arrival(remote.focus, true)
                        {
                            let _ = crate::window::activate_control_window(&app, &remote.window_id);
                        }
                        return;
                    }
                    RuntimeArrivalCommitOutcome::DuplicateOrStale => return,
                    RuntimeArrivalCommitOutcome::SourceMissing => {
                        let action = RemoteObservationAction::CompensateKillWindow;
                        debug_assert_eq!(action, RemoteObservationAction::CompensateKillWindow);
                        break "Remote source tab disappeared before window arrival".to_string();
                    }
                },
                Err(error) => {
                    if remote_observation_action(true, Some(&error), attempt)
                        != RemoteObservationAction::RetainAndRetry
                    {
                        break format!("Remote window reconciliation failed after retry: {error}");
                    }
                    attempt += 1;
                }
            }
        };
        let compensation = compensate_remote_runtime_lease(&app, lease.id);
        record_event(
            &app,
            "surface.create_failed",
            "surface",
            "workspace.lifecycle",
            Some(remote.window_id),
            Some(remote.workspace_id),
            remote.target_pane_id,
            None,
            json!({
                "message": failure,
                "remote_target_id": compensation_token,
                "compensated": compensation,
            }),
        );
    });
}

fn schedule_remote_window_departure_reconciliation(app: &AppHandle, key: String) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<RemoteWindowDepartureRegistryState>();
        let pending = match state.registry.lock() {
            Ok(registry) => registry.get(&key).cloned(),
            Err(_) => None,
        };
        let Some(pending) = pending else {
            return;
        };
        let observation = classify_remote_tmux_window_presence(
            &pending.remote_window_id,
            run_remote_tmux_command_output(
                &pending.destination,
                remote_tmux_list_windows_command(),
            ),
        );
        let action = state
            .registry
            .lock()
            .map(|mut registry| registry.record_observation(&key, observation))
            .unwrap_or(RemoteWindowDepartureAction::RetainAndPoll);
        let failure = match action {
            RemoteWindowDepartureAction::RetainAndPoll => (observation
                == RemoteWindowPresenceObservation::QueryFailed)
                .then_some("Failed to query remote window departure".to_string()),
            RemoteWindowDepartureAction::CommitDeparture => {
                let result = commit_runtime_departure_for_control(&app, pending.departure.clone());
                let error = result.as_ref().err().cloned();
                let completed = state
                    .registry
                    .lock()
                    .map(|mut registry| registry.record_commit_result(&key, result))
                    .unwrap_or(false);
                if completed {
                    return;
                }
                error.or_else(|| Some("Failed to commit remote window departure".into()))
            }
        };
        if let Some(message) = failure {
            let payload = state.registry.lock().ok().and_then(|mut registry| {
                registry
                    .take_failure_report_permit(&key)
                    .then(|| registry.pending_failure_payload(&key, &message))
                    .flatten()
            });
            if let Some(payload) = payload {
                record_event(
                    &app,
                    "surface.close_failed",
                    "surface",
                    "workspace.lifecycle",
                    Some(pending.departure.window_id.clone()),
                    Some(pending.departure.workspace_id.clone()),
                    Some(pending.departure.pane_id.clone()),
                    Some(pending.departure.surface_id.clone()),
                    payload,
                );
            }
        }
        let retry_delay = state
            .registry
            .lock()
            .ok()
            .and_then(|registry| registry.retry_delay(&key))
            .unwrap_or(Duration::from_millis(250));
        thread::sleep(retry_delay);
        schedule_remote_window_departure_reconciliation(&app, key);
    });
}

fn should_focus_window_after_remote_arrival(requested: bool, committed: bool) -> bool {
    requested && committed
}

#[cfg(windows)]
fn open_external_url_checked(url: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let operation: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let target: Vec<u16> = std::ffi::OsStr::new(url)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(target.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    shell_execute_succeeded(result.0 as isize)
        .then_some(())
        .ok_or_else(|| "Failed to open URL externally".to_string())
}

#[cfg(not(windows))]
fn open_external_url_checked(url: &str) -> Result<(), String> {
    Command::new("xdg-open")
        .arg(url)
        .status()
        .map_err(|_| "Failed to open URL externally".to_string())?
        .success()
        .then_some(())
        .ok_or_else(|| "Failed to open URL externally".to_string())
}

fn shell_execute_succeeded(code: isize) -> bool {
    code > 32
}

struct ProductionLifecycleRollbackOperations<'a> {
    app: &'a AppHandle,
    previous: Option<AppSessionSnapshot>,
    candidate: Option<AppSessionSnapshot>,
    dock_journal: DockCommitJournal<DockRuntimeClaim, DockTeardownCompensation>,
    remote_creations: Vec<(StagedRemoteCreation, u64)>,
}

impl LifecycleRollbackOperations for ProductionLifecycleRollbackOperations<'_> {
    fn rollback_dock(&mut self) -> Result<(), String> {
        let app = self.app;
        let previous = self.previous.as_ref();
        let candidate = self.candidate.as_ref();
        std::mem::take(&mut self.dock_journal)
            .rollback(|step| match step {
                DockRollbackStep::RestoreSnapshot => {
                    let previous = previous.ok_or_else(|| {
                        "previous lifecycle snapshot was not prepared".to_string()
                    })?;
                    let candidate = candidate
                        .ok_or_else(|| "lifecycle candidate was not prepared".to_string())?;
                    commit_lifecycle_snapshot_for_control_if_current(
                        app,
                        app.state::<SessionState>().inner(),
                        candidate,
                        previous,
                        false,
                    )
                    .map(|_| ())
                }
                DockRollbackStep::RollbackClaim(claim) => rollback_runtime_claim(app, claim),
                DockRollbackStep::RecreateTeardown(teardown) => {
                    ProductionLifecycleExecutor::compensate_dock_teardown(app, teardown)
                }
            })
            .map_err(|errors| format!("Lifecycle rollback failed: {errors}"))
    }

    fn cleanup_terminal(&mut self, id: u32) -> Result<(), String> {
        let state = self
            .app
            .try_state::<TerminalState>()
            .ok_or_else(|| "terminal runtime state is unavailable".to_string())?;
        terminal_shutdown_id_preserving_authority_for_control(state.inner(), id)?;
        terminal_remove_id_for_control(state.inner(), id)
    }

    fn cleanup_remote(&mut self, target: &str) -> Result<(), String> {
        let index = self
            .remote_creations
            .iter()
            .position(|(remote, _)| remote.token == target)
            .ok_or_else(|| format!("remote rollback target {target} is unavailable"))?;
        let (_, lease_id) = self.remote_creations.remove(index);
        compensate_remote_runtime_lease(self.app, lease_id)
            .then_some(())
            .ok_or_else(|| format!("remote rollback for {target} remains retryable"))
    }

    fn cleanup_browser(&mut self, surface_id: &str) -> Result<(), String> {
        let state = self
            .app
            .try_state::<BrowserWebviewState>()
            .ok_or_else(|| "browser runtime state is unavailable".to_string())?;
        browser_close_webview_strict_for_control(state.inner(), surface_id)
    }
}

impl ProductionLifecycleExecutor<'_> {
    fn flush_deferred_remote_reconciliations(&mut self) {
        for (remote, lease_id) in self.deferred_remote_reconciliations.drain(..) {
            schedule_remote_window_reconciliation(self.app, remote, lease_id);
        }
    }

    fn flush_deferred_remote_departures(&mut self) {
        for departure in self.deferred_remote_departures.drain(..) {
            schedule_remote_window_departure_reconciliation(self.app, departure);
        }
    }

    fn compensate_dock_teardown(
        app: &AppHandle,
        compensation: DockTeardownCompensation,
    ) -> Result<(), String> {
        let DockRuntimeOperation::Teardown {
            surface_id,
            generation,
            intent,
        } = compensation.operation
        else {
            return Err("Expected Dock teardown compensation".into());
        };
        let operation = DockRuntimeOperation::Create {
            surface_id,
            generation,
            intent,
        };
        let claim = stage_runtime_for_control(app, &compensation.owner_id, &operation)?;
        if let Err(error) = publish_runtime_claim(app, &claim) {
            return match rollback_runtime_claim(app, claim) {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(format!(
                    "{error}; failed to roll back replacement runtime: {rollback_error}"
                )),
            };
        }
        Ok(())
    }

    fn rollback_resources(&mut self) -> Result<(), String> {
        let plan = LifecycleRollbackPlan {
            terminal_ids: self
                .staged_terminals
                .drain(..)
                .map(|(_, id, _)| id)
                .collect(),
            remote_targets: self
                .staged_remote_creations
                .iter()
                .map(|(remote, _)| remote.token.clone())
                .collect(),
            browser_surface_ids: self
                .staged_browsers
                .drain(..)
                .map(|(_, surface_id, _)| surface_id)
                .collect(),
        };
        let mut operations = ProductionLifecycleRollbackOperations {
            app: self.app,
            previous: self.previous.take(),
            candidate: self.candidate.take(),
            dock_journal: std::mem::take(&mut self.dock_journal),
            remote_creations: self.staged_remote_creations.drain(..).collect(),
        };
        let rollback = run_lifecycle_rollback_cleanup(&plan, &mut operations);
        self.deferred_remote_reconciliations.clear();
        for key in self.deferred_remote_departures.drain(..) {
            schedule_remote_window_departure_reconciliation(self.app, key);
        }
        self.staged.clear();
        rollback
    }
}

impl pane_surface_lifecycle::LifecycleEffectExecutor for ProductionLifecycleExecutor<'_> {
    type Error = String;

    fn prepare_transition(&mut self, candidate: &AppSessionSnapshot) -> Result<(), Self::Error> {
        cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(candidate)
            .and_then(|model| model.validate_indexes())
            .map_err(|error| error.to_string())?;
        if self.previous.is_none() {
            return Err("previous lifecycle snapshot was not prepared".to_string());
        }
        self.candidate = Some(candidate.clone());
        Ok(())
    }

    fn stage(
        &mut self,
        effect: &pane_surface_lifecycle::LifecycleEffect,
    ) -> Result<(), Self::Error> {
        let terminal_state = self.app.state::<TerminalState>();
        match effect {
            pane_surface_lifecycle::LifecycleEffect::DockCreate {
                owner_id,
                dock_surface_id,
                generation,
                intent,
                ..
            } => {
                let operation = DockRuntimeOperation::Create {
                    surface_id: dock_surface_id.clone(),
                    generation: *generation,
                    intent: intent.clone(),
                };
                let claim = stage_runtime_for_control(self.app, owner_id, &operation)?;
                self.dock_journal.stage_claim(claim);
            }
            pane_surface_lifecycle::LifecycleEffect::TerminalCreate {
                surface_id,
                command,
                working_directory,
                startup_environment,
                ..
            } => {
                if self.terminal_create_runtime_policy == TerminalCreateRuntimePolicy::Deferred {
                    return Ok(());
                }
                let id = terminal_open_for_control(
                    self.app,
                    terminal_state.inner(),
                    Some(surface_id),
                    working_directory.as_deref(),
                    command.as_deref(),
                    None,
                    startup_environment.clone(),
                    None,
                    None,
                )?;
                self.staged_terminals.push((surface_id.clone(), id, false));
            }
            pane_surface_lifecycle::LifecycleEffect::TerminalReplace {
                surface_id,
                command,
                working_directory,
                ..
            } => {
                let id = terminal_open_for_control(
                    self.app,
                    terminal_state.inner(),
                    Some(surface_id),
                    working_directory.as_deref(),
                    Some(command),
                    None,
                    None,
                    None,
                    None,
                )?;
                self.staged_terminals.push((surface_id.clone(), id, true));
            }
            pane_surface_lifecycle::LifecycleEffect::RemoteCreate {
                reserved_surface_id,
                reserved_pane_id,
                remote_session_id,
                destination,
                window_id,
                workspace_id,
                tmux_operation,
                arrival_policy: _,
                target_pane_id,
                source_surface_id,
                source_remote_pane_id,
                source_pane_id,
                split_direction,
                split_orientation,
                focus,
                working_directory,
                ..
            } => {
                let target = RemoteTmuxTarget::for_create(tmux_operation)?;
                let source_window = source_remote_pane_id.as_ref().and_then(|pane_token| {
                    let command = remote_tmux_source_window_command(pane_token).ok()?;
                    let output = Command::new("ssh")
                        .args(["-T", "-o", "BatchMode=yes", destination])
                        .args(command)
                        .output()
                        .ok()?;
                    if !output.status.success() {
                        return None;
                    }
                    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    valid_tmux_identity(&token, '@').then_some(token)
                });
                let source_target = match target {
                    RemoteTmuxTarget::Window => source_window,
                    RemoteTmuxTarget::Pane => source_window
                        .zip(source_remote_pane_id.clone())
                        .map(|(window, pane)| format!("{window}.{pane}")),
                };
                if target == RemoteTmuxTarget::Pane && source_target.is_none() {
                    return Err("remote tmux split source is unavailable".into());
                }
                let argv = remote_tmux_create_argv_with_split(
                    &RemoteTmuxCreateSpec {
                        operation: tmux_operation,
                        focus: *focus,
                        source_target: source_target.as_deref(),
                        working_directory: working_directory.as_deref(),
                    },
                    *split_direction,
                )?;
                let lease_state = self.app.state::<RemoteRuntimeLeaseRegistryState>();
                let restore_epoch = lease_state
                    .registry
                    .lock()
                    .map_err(|_| "remote runtime lease registry lock poisoned".to_string())?
                    .restore_epoch;
                let source_id = source_surface_id
                    .as_deref()
                    .ok_or_else(|| "remote runtime source identity is unavailable".to_string())?;
                let source = remote_runtime_source_witness(
                    self.previous.as_ref().ok_or_else(|| {
                        "previous lifecycle snapshot was not prepared".to_string()
                    })?,
                    window_id,
                    workspace_id,
                    source_id,
                    restore_epoch,
                )
                .ok_or_else(|| "remote runtime source witness is unavailable".to_string())?;
                let scope = RemoteRuntimeLeaseScope {
                    endpoint: destination.clone(),
                    session: remote_session_id.clone(),
                };
                let lease_id = reserve_remote_runtime_lease(
                    lease_state.inner(),
                    scope.clone(),
                    source.clone(),
                    reserved_surface_id.clone(),
                    reserved_pane_id.clone(),
                    target,
                    Vec::new(),
                )?;
                schedule_remote_runtime_lease_watchdog(self.app, lease_id);
                let topology_before = match observe_remote_tmux_topology(destination) {
                    Ok(topology) => topology,
                    Err(error) => {
                        if let Ok(mut registry) = lease_state.registry.lock() {
                            registry.record_command_outcome(
                                lease_id,
                                RemoteRuntimeCommandOutcome::Failed,
                                None,
                            );
                        }
                        return Err(error);
                    }
                };
                lease_state
                    .registry
                    .lock()
                    .map_err(|_| "remote runtime lease registry lock poisoned".to_string())?
                    .leases
                    .get_mut(&lease_id)
                    .ok_or_else(|| "remote runtime lease expired before mutation".to_string())?
                    .topology_before = Some(topology_before.clone());
                let current = snapshot(self.app);
                let mut lease_registry = lease_state
                    .registry
                    .lock()
                    .map_err(|_| "remote runtime lease registry lock poisoned".to_string())?;
                validate_remote_runtime_mutation_fence(&mut lease_registry, lease_id, &current)?;
                drop(lease_registry);
                let child = match Command::new("ssh")
                    .args(["-T", "-o", "BatchMode=yes", destination])
                    .args(&argv)
                    .spawn()
                {
                    Ok(child) => child,
                    Err(error) => {
                        if let Ok(mut registry) = lease_state.registry.lock() {
                            registry.record_command_outcome(
                                lease_id,
                                remote_runtime_process_error_outcome(false),
                                None,
                            );
                        }
                        return Err(format!("failed to launch remote tmux create: {error}"));
                    }
                };
                let output = match child.wait_with_output() {
                    Ok(output) => output,
                    Err(error) => {
                        if let Ok(mut registry) = lease_state.registry.lock() {
                            registry.record_command_outcome(
                                lease_id,
                                remote_runtime_process_error_outcome(true),
                                None,
                            );
                        }
                        return Err(format!("remote tmux create completion is unknown: {error}"));
                    }
                };
                if !output.status.success() {
                    if let Ok(mut registry) = lease_state.registry.lock() {
                        registry.record_command_outcome(
                            lease_id,
                            RemoteRuntimeCommandOutcome::Failed,
                            None,
                        );
                    }
                    return Err(format!("remote tmux create exited with {}", output.status));
                }
                let raw_output = String::from_utf8_lossy(&output.stdout);
                let (observation, pane_observation) = match resolve_remote_tmux_create_observation(
                    target,
                    &raw_output,
                    &topology_before,
                    || observe_remote_tmux_topology(destination),
                ) {
                    Ok(observation) => observation,
                    Err(error) => {
                        if let Ok(mut registry) = lease_state.registry.lock() {
                            registry.record_command_outcome(
                                lease_id,
                                RemoteRuntimeCommandOutcome::Unknown,
                                None,
                            );
                        }
                        return Err(error);
                    }
                };
                let token = observation
                    .as_ref()
                    .map(|observation| observation.window_token.clone())
                    .or_else(|| pane_observation.clone())
                    .unwrap_or_default();
                if token.is_empty() {
                    if let Ok(mut registry) = lease_state.registry.lock() {
                        registry.record_command_outcome(
                            lease_id,
                            RemoteRuntimeCommandOutcome::Unknown,
                            None,
                        );
                    }
                    return Err("remote tmux create returned no target identity".to_string());
                }
                let retained = lease_state
                    .registry
                    .lock()
                    .map_err(|_| "remote runtime lease registry lock poisoned".to_string())?
                    .record_command_outcome(
                        lease_id,
                        RemoteRuntimeCommandOutcome::Succeeded,
                        Some(token.clone()),
                    );
                if !retained {
                    return Err("remote runtime lease expired before command completion".into());
                }
                let mut arrival = if target == RemoteTmuxTarget::Window {
                    observation.as_ref().and_then(|observation| {
                        Some(pane_surface_lifecycle::RuntimeArrival::remote_tab(
                            window_id,
                            workspace_id,
                            target_pane_id.as_ref()?,
                            reserved_surface_id,
                            &observation.pane_token,
                            1,
                            source_id,
                            *focus,
                        ))
                    })
                } else {
                    pane_observation.as_ref().map(|pane_token| {
                        let mut arrival = pane_surface_lifecycle::RuntimeArrival::remote(
                            window_id,
                            workspace_id,
                            reserved_pane_id,
                            reserved_surface_id,
                            pane_token,
                            1,
                        );
                        arrival.anchor_surface_id = Some(source_id.into());
                        arrival.source_pane_id = source_pane_id.clone();
                        arrival.split_orientation = split_orientation.clone();
                        arrival.focused = *focus;
                        arrival
                    })
                };
                if let Some(arrival) = &mut arrival {
                    arrival.expected_source_generation = Some(source.surface_generation);
                }
                self.staged_remote_creations.push((
                    StagedRemoteCreation {
                        target,
                        token,
                        window_id: window_id.clone(),
                        workspace_id: workspace_id.clone(),
                        target_pane_id: target_pane_id.clone(),
                        source_surface_id: source_surface_id.clone(),
                        source_pane_id: source_pane_id.clone(),
                        split_orientation: split_orientation.clone(),
                        focus: *focus,
                        observation,
                        pane_observation,
                        arrival,
                    },
                    lease_id,
                ));
            }
            pane_surface_lifecycle::LifecycleEffect::BrowserAttach {
                surface_id, url, ..
            } => {
                let window_label =
                    webview_label_for_surface_owner(self.app, self.candidate.as_ref(), surface_id);
                let state = self.app.state::<BrowserWebviewState>();
                browser_attach_webview_for_control(
                    self.app,
                    state.inner(),
                    &window_label,
                    surface_id,
                    url.as_deref(),
                    None,
                    false,
                )?;
                self.staged_browsers
                    .push((window_label, surface_id.clone(), url.clone()));
            }
            pane_surface_lifecycle::LifecycleEffect::RuntimeTeardown {
                surface_id,
                generation,
                owner_id,
                dock_intent,
                must_succeed,
                failure_message,
                phase,
                ..
            } => {
                if *phase == "commit" {
                    self.staged.push(effect.clone());
                    return Ok(());
                }
                if let Some(intent) = dock_intent {
                    let operation = DockRuntimeOperation::Teardown {
                        surface_id: surface_id.clone(),
                        generation: *generation,
                        intent: intent.clone(),
                    };
                    let exists = runtime_exists_for_control(self.app, &operation);
                    if *must_succeed && exists.is_err() {
                        return Err((*failure_message).to_string());
                    }
                    if exists.unwrap_or(false) {
                        self.dock_journal.stage_teardown(DockTeardownCompensation {
                            owner_id: owner_id.clone(),
                            operation: operation.clone(),
                        });
                    }
                    let teardown = teardown_runtime_for_control(self.app, &operation);
                    if *must_succeed && teardown.is_err() {
                        return Err((*failure_message).to_string());
                    }
                } else {
                    let browser_state = self.app.state::<BrowserWebviewState>();
                    let kind = self.previous.as_ref().and_then(|previous| {
                        cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(
                            previous,
                        )
                        .ok()
                        .and_then(|model| {
                            model.surface(surface_id).map(|record| record.kind.clone())
                        })
                    });
                    let mut registry = ProductionLifecycleRuntimeRegistry {
                        terminal: terminal_state.inner(),
                        browser: browser_state.inner(),
                        browser_surface_id: Some(surface_id),
                    };
                    let teardown = match kind {
                        Some(SessionSurfaceKindSnapshot::Browser { .. }) => {
                            strict_lifecycle_runtime_teardown(
                                &mut registry,
                                LifecycleRuntimeKind::Browser,
                                surface_id,
                            )
                        }
                        _ => strict_lifecycle_runtime_teardown(
                            &mut registry,
                            LifecycleRuntimeKind::Terminal,
                            surface_id,
                        ),
                    };
                    if *must_succeed && teardown.is_err() {
                        return Err((*failure_message).to_string());
                    }
                }
                self.app
                    .state::<RemoteProxyBrokerState>()
                    .stop_panel_broker(surface_id);
            }
            _ => {}
        }
        self.staged.push(effect.clone());
        Ok(())
    }

    fn commit_staged(&mut self) -> Result<(), Self::Error> {
        let candidate = self
            .candidate
            .as_ref()
            .ok_or_else(|| "lifecycle candidate was not prepared".to_string())?;
        let browser_state = self.app.state::<BrowserWebviewState>();
        let state = self.app.state::<SessionState>();
        let previous = self
            .previous
            .as_ref()
            .ok_or_else(|| "previous lifecycle snapshot was not prepared".to_string())?;
        if lifecycle_snapshot_changed(candidate, previous) {
            self.dock_journal.commit_snapshot(|| {
                commit_lifecycle_snapshot_for_control_if_current(
                    self.app,
                    state.inner(),
                    previous,
                    candidate,
                    false,
                )
                .map(|_| ())
            })?;
        }
        self.dock_journal
            .publish_claims(|claim| publish_runtime_claim(self.app, claim))?;
        for (window_id, surface_id, url) in &self.staged_browsers {
            browser_attach_webview_for_control(
                self.app,
                browser_state.inner(),
                window_id,
                surface_id,
                url.as_deref(),
                None,
                true,
            )?;
        }
        let terminal_state = self.app.state::<TerminalState>();
        let replacements = self
            .staged_terminals
            .iter()
            .filter(|(_, _, replace)| *replace)
            .cloned()
            .collect::<Vec<_>>();
        for (surface_id, id, _) in replacements {
            let mut registry = ProductionLifecycleRuntimeRegistry {
                terminal: terminal_state.inner(),
                browser: browser_state.inner(),
                browser_surface_id: None,
            };
            if let Err(error) = commit_terminal_runtime_replacement(&mut registry, &surface_id, id)
            {
                self.staged_terminals
                    .retain(|(_, staged_id, _)| *staged_id != id);
                return Err(error);
            }
        }
        for effect in &self.staged {
            match effect {
                pane_surface_lifecycle::LifecycleEffect::ActivateWindow { window_id } => {
                    let _ = crate::window::activate_control_window(self.app, window_id);
                }
                pane_surface_lifecycle::LifecycleEffect::RuntimeTeardown {
                    surface_id,
                    generation,
                    dock_intent,
                    phase,
                    must_succeed,
                    failure_message,
                    ..
                } if *phase == "commit" => {
                    if let Some(intent) = dock_intent {
                        let operation = DockRuntimeOperation::Teardown {
                            surface_id: surface_id.clone(),
                            generation: *generation,
                            intent: intent.clone(),
                        };
                        let teardown = teardown_runtime_for_control(self.app, &operation);
                        if *must_succeed && teardown.is_err() {
                            return Err((*failure_message).to_string());
                        }
                    } else {
                        let kind = self.previous.as_ref().and_then(|previous| {
                            cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(
                                previous,
                            )
                            .ok()
                            .and_then(|model| {
                                model.surface(surface_id).map(|record| record.kind.clone())
                            })
                        });
                        let mut registry = ProductionLifecycleRuntimeRegistry {
                            terminal: terminal_state.inner(),
                            browser: browser_state.inner(),
                            browser_surface_id: Some(surface_id),
                        };
                        let teardown = match kind {
                            Some(SessionSurfaceKindSnapshot::Browser { .. }) => {
                                strict_lifecycle_runtime_teardown(
                                    &mut registry,
                                    LifecycleRuntimeKind::Browser,
                                    surface_id,
                                )
                            }
                            _ => strict_lifecycle_runtime_teardown(
                                &mut registry,
                                LifecycleRuntimeKind::Terminal,
                                surface_id,
                            ),
                        };
                        if *must_succeed && teardown.is_err() {
                            return Err((*failure_message).to_string());
                        }
                    }
                    self.app
                        .state::<RemoteProxyBrokerState>()
                        .stop_panel_broker(surface_id);
                }
                pane_surface_lifecycle::LifecycleEffect::RuntimeTeardown { .. } => {}
                pane_surface_lifecycle::LifecycleEffect::TerminalCreate { .. }
                | pane_surface_lifecycle::LifecycleEffect::TerminalReplace { .. }
                | pane_surface_lifecycle::LifecycleEffect::BrowserAttach { .. }
                | pane_surface_lifecycle::LifecycleEffect::UiSurfaceAttach { .. } => {}
                pane_surface_lifecycle::LifecycleEffect::BrowserReload { surface_id, .. } => {
                    browser_webview_command_for_control(
                        browser_state.inner(),
                        surface_id,
                        "reload",
                    )?;
                }
                pane_surface_lifecycle::LifecycleEffect::ExternalBrowserOpen { url, .. } => {
                    open_external_url_checked(url)?;
                }
                pane_surface_lifecycle::LifecycleEffect::RemoteWindowRename {
                    destination,
                    source_remote_pane_id,
                    title,
                    must_succeed,
                    ..
                } => {
                    let result = execute_remote_tmux_window_mutation(
                        destination,
                        source_remote_pane_id,
                        |window_token| remote_tmux_rename_window_command(window_token, title),
                    );
                    if *must_succeed {
                        result?;
                    }
                }
                pane_surface_lifecycle::LifecycleEffect::RemoteWindowClose {
                    destination,
                    remote_session_id,
                    window_id,
                    workspace_id,
                    pane_id,
                    surface_id,
                    generation,
                    source_remote_pane_id,
                    must_succeed,
                    ..
                } => {
                    let registry_state = self.app.state::<RemoteWindowDepartureRegistryState>();
                    let result = execute_scoped_remote_window_kill_and_register(
                        &registry_state.registry,
                        destination,
                        remote_session_id,
                        source_remote_pane_id,
                        pane_surface_lifecycle::RuntimeDeparture {
                            window_id: window_id.clone(),
                            workspace_id: workspace_id.clone(),
                            pane_id: pane_id.clone(),
                            surface_id: surface_id.clone(),
                            generation: *generation,
                        },
                        |destination, source_pane| {
                            execute_remote_tmux_window_mutation(
                                destination,
                                source_pane,
                                |window_token| {
                                    remote_tmux_kill_command(RemoteTmuxTarget::Window, window_token)
                                },
                            )
                        },
                    );
                    if let Ok(key) = &result {
                        self.deferred_remote_departures.push(key.clone());
                    } else if *must_succeed {
                        result?;
                    }
                }
                pane_surface_lifecycle::LifecycleEffect::RemoteCreate { .. } => {}
                pane_surface_lifecycle::LifecycleEffect::DockCreate { .. } => {}
                pane_surface_lifecycle::LifecycleEffect::DockReveal { owner_id } => {
                    let change = self
                        .app
                        .state::<crate::right_sidebar::RightSidebarState>()
                        .reveal_dock_for_control();
                    if let Ok(Some(change)) = change {
                        let _ = self
                            .app
                            .emit(crate::right_sidebar::RIGHT_SIDEBAR_CHANGED_EVENT, change);
                    }
                    let _ = crate::window::activate_control_window(self.app, owner_id);
                }
                pane_surface_lifecycle::LifecycleEffect::DockChanged { owner_id, .. } => {
                    let snapshot = self
                        .app
                        .state::<crate::dock::DockStore>()
                        .snapshot(candidate, owner_id);
                    let _ = self.app.emit(crate::dock::DOCK_CHANGED_EVENT, snapshot);
                }
                pane_surface_lifecycle::LifecycleEffect::PersistSession => {}
            }
        }
        self.deferred_remote_reconciliations.extend(
            self.staged_remote_creations
                .iter()
                .filter(|(remote, _)| {
                    remote.observation.is_some() || remote.pane_observation.is_some()
                })
                .cloned(),
        );
        self.staged.clear();
        self.staged_terminals.clear();
        self.staged_remote_creations.clear();
        self.staged_browsers.clear();
        self.dock_journal.finish();
        self.previous = None;
        self.candidate = None;
        Ok(())
    }

    fn rollback_staged(&mut self) -> Result<(), Self::Error> {
        self.rollback_resources()
    }

    fn rollback_committed(&mut self) -> Result<(), Self::Error> {
        self.rollback_resources()
    }
}
