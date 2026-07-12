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
    SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot,
};
use cmux_core::session_ops;
use cmux_ipc::{ControlCallResult, ControlRequest, ControlStream, JsonValue};
use cmux_workspaces::{WorkspaceBatchReorderError, WorkspaceReorderPlanItem};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};
use time::OffsetDateTime;
use uuid::Uuid;

mod pane_surface_lifecycle;

use crate::browser::{
    browser_add_init_script_for_control, browser_attach_webview_for_control,
    browser_clear_network_requests_for_control, browser_close_webview_strict_for_control,
    browser_eval_for_control, browser_has_webview_for_control,
    browser_network_requests_for_control, browser_webview_command_for_control,
    strict_browser_runtime_teardown_transaction, BrowserNetworkRequestsQuery, BrowserWebviewState,
};
use crate::diff::DiffState;
use crate::dock::{
    publish_runtime_claim, rollback_runtime_claim, runtime_exists_for_control,
    stage_runtime_for_control, teardown_runtime_for_control, DockRuntimeClaim,
    DockRuntimeOperation,
};
use crate::session::{
    append_workspace_sidebar_log_for_control, break_pane_for_control, browser_go_back_for_control,
    browser_go_forward_for_control, clear_browser_history_for_control,
    clear_workspace_agent_pid_for_control, clear_workspace_panel_pull_request_for_control,
    clear_workspace_remote_for_control, clear_workspace_sidebar_log_for_control,
    clear_workspace_sidebar_metadata_block_for_control,
    clear_workspace_sidebar_metadata_for_control, clear_workspace_sidebar_progress_for_control,
    clear_workspace_sidebar_status_for_control, close_panel_for_control,
    close_workspace_in_window_for_control, close_workspaces_for_control,
    commit_lifecycle_snapshot_for_control, commit_lifecycle_snapshot_for_control_if_current,
    configure_workspace_remote_for_control, current_session_snapshot,
    equalize_dividers_for_control, focus_last_pane_for_control, focus_pane_for_control,
    move_panel_to_new_workspace_for_control, move_surface_for_control,
    move_workspace_to_window_for_control, new_browser_workspace_for_control,
    new_terminal_tab_for_control, new_workspace_in_window_for_control, open_browser_url_in_panel,
    open_custom_sidebar_in_panel, open_diff_viewer_in_panel, open_file_in_panel,
    open_markdown_file_in_panel, reconnect_workspace_remote_for_control,
    register_window_for_control, rename_workspace_in_window_for_control,
    reopen_closed_browser_tab_for_control, reopen_closed_workspace_for_control,
    reorder_surface_for_control, reorder_workspaces_for_control,
    reorder_workspaces_many_for_control, reset_workspace_color_for_control,
    reset_workspace_sidebar_metadata_for_control, resize_pane_for_control,
    restore_previous_launch_for_control, select_adjacent_panel_for_control,
    select_last_workspace_for_control, select_workspace_for_control,
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
    start_direct_browser_proxy_for_control, swap_panes_for_control,
    toggle_browser_developer_tools_for_control, toggle_browser_focus_mode_for_control,
    toggle_browser_omnibar_for_control, toggle_split_zoom_for_control, PaneFocusControlError,
    PaneLastControlError, PaneResizeControlError, PaneResizeControlIntent,
    PaneTopologyControlError, ReorderWorkspacesManyControlError, SessionState,
    SurfacePositionControlError, TerminalPanelCreateError, WorkspaceLastControlError,
    WorkspaceRemoteControlConfig, WorkspaceRenameResolution, WorkspaceSelectControlError,
};
use crate::terminal::{
    scan_listening_ports_for_root_pid, scan_panel_listening_ports, terminal_clear_history_panel,
    terminal_grid_size_for_panel, terminal_ids_for_panel_for_control, terminal_open_for_control,
    terminal_read_panel, terminal_remove_id_for_control, terminal_runtime_snapshots,
    terminal_shutdown_id_preserving_authority_for_control, terminal_write_panel, TerminalState,
};

const CONTROL_PIPE_BASE_NAME: &str = "cmux";
const CONTROL_EVENTS_CHANGED_EVENT: &str = "cmux://events-changed";
const PANEL_FLASH_EVENT: &str = "cmux://panel-flash";
const SURFACE_REFRESH_EVENT: &str = "cmux://refresh-surfaces";
const CUSTOM_SIDEBAR_RELOAD_EVENT: &str = "cmux://custom-sidebar-reload";
const CUSTOM_SIDEBAR_SELECT_EVENT: &str = "cmux://custom-sidebar-select";
const CUSTOM_SIDEBAR_ACTION_POLICY: &str = "cmux-custom-sidebar-safe-default";
const CUSTOM_SIDEBAR_ACTION_SCHEMA_VERSION: u32 = 1;
const CUSTOM_SIDEBAR_ALLOWED_ACTION_METHODS: &[&str] = &[
    "extension.sidebar.snapshot",
    "sidebar.snapshot",
    "sidebar.list",
    "sidebar.validate",
    "sidebar.reload",
    "sidebar.select",
    "sidebar.open",
    "workspace.list",
    "workspace.current",
    "workspace.select",
    "workspace.next",
    "workspace.previous",
    "workspace.last",
    "workspace.sidebar_state",
    "workspace.list_status",
    "workspace.list_log",
    "workspace.list_meta",
    "workspace.list_meta_blocks",
    "workspace.set_progress",
    "workspace.clear_progress",
    "workspace.set_status",
    "workspace.clear_status",
    "workspace.report_meta",
    "workspace.clear_meta",
    "workspace.report_meta_block",
    "workspace.clear_meta_block",
    "workspace.log",
    "workspace.clear_log",
    "workspace.reset_sidebar",
    "surface.list",
    "surface.focus",
    "surface.next",
    "surface.previous",
    "set_progress",
    "set-progress",
    "clear_progress",
    "clear-progress",
    "set_status",
    "set-status",
    "clear_status",
    "clear-status",
    "list_status",
    "list-status",
    "report_meta",
    "report-meta",
    "clear_meta",
    "clear-meta",
    "list_meta",
    "list-meta",
    "report_meta_block",
    "report-meta-block",
    "clear_meta_block",
    "clear-meta-block",
    "list_meta_blocks",
    "list-meta-blocks",
    "log",
    "clear_log",
    "clear-log",
    "reset_sidebar",
    "reset-sidebar",
];

const CUSTOM_SIDEBAR_WORKSPACE_SELECTOR_KEYS: &[&str] =
    &["workspace_id", "id", "workspace_ref", "ref"];
const CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS: &[&str] =
    &["surface_id", "panel_id", "id", "surface_ref", "ref"];

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

    fn resolve(&self, kind: &'static str, reference: &str) -> Option<String> {
        let number = one_based_ref_index(reference, kind)? as u64 + 1;
        self.refs
            .iter()
            .find_map(|((entry_kind, id), entry_number)| {
                (*entry_kind == kind && *entry_number == number).then(|| id.clone())
            })
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
    custom_sidebar_action_reply(handle_custom_sidebar_action_request(
        &app,
        method,
        params.unwrap_or(Value::Object(serde_json::Map::new())),
        source_path.as_deref(),
    ))
}

fn handle_custom_sidebar_action_request(
    app: &AppHandle,
    method: String,
    params: Value,
    source_path: Option<&str>,
) -> ControlCallResult {
    let method = method.trim();
    if method.is_empty() {
        return invalid_params("Missing custom sidebar action method");
    }
    let Value::Object(params) = params else {
        return invalid_params("Custom sidebar action params must be a JSON object");
    };
    if !custom_sidebar_action_policy_allows(method) {
        return custom_sidebar_action_denied(
            method,
            custom_sidebar_manifest_for_source(source_path),
        );
    }
    if let Err(error) = validate_custom_sidebar_action_schema(method, &params) {
        return error;
    }
    handle_control_request(app, ControlRequest::new(None, method.to_string(), params))
}

fn custom_sidebar_action_policy_allows(method: &str) -> bool {
    CUSTOM_SIDEBAR_ALLOWED_ACTION_METHODS.contains(&method)
}

fn custom_sidebar_action_denied(method: &str, manifest: Option<Value>) -> ControlCallResult {
    ControlCallResult::Err {
        code: "custom_sidebar_capability_denied".to_string(),
        message: format!("Custom sidebar action '{method}' is outside the safe capability scope"),
        data: JsonValue::try_from(json!({
            "method": method,
            "policy": CUSTOM_SIDEBAR_ACTION_POLICY,
            "allowed_methods": CUSTOM_SIDEBAR_ALLOWED_ACTION_METHODS,
            "manifest": manifest,
        }))
        .ok(),
    }
}

fn validate_custom_sidebar_action_schema(
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<(), ControlCallResult> {
    match custom_sidebar_action_schema_method(method) {
        "sidebar.select" | "sidebar.open" => {
            require_custom_sidebar_string_param(method, params, "name", &["name", "sidebar"])
        }
        "workspace.select" => require_custom_sidebar_workspace_selector(method, params),
        "workspace.set_progress" => require_custom_sidebar_finite_number_param(
            method,
            params,
            "value",
            &["value", "progress"],
        ),
        "workspace.set_status" => {
            require_custom_sidebar_string_param(method, params, "key", &["key", "name"])?;
            require_custom_sidebar_raw_string_param(
                method,
                params,
                "value",
                &["value", "status", "text"],
            )?;
            validate_custom_sidebar_optional_i64_param(method, params, "priority", &["priority"])
        }
        "workspace.clear_status" | "workspace.clear_meta" | "workspace.clear_meta_block" => {
            require_custom_sidebar_string_param(method, params, "key", &["key", "name"])
        }
        "workspace.report_meta" => {
            require_custom_sidebar_string_param(method, params, "key", &["key", "name"])?;
            require_custom_sidebar_raw_string_param(
                method,
                params,
                "value",
                &["value", "text", "markdown"],
            )?;
            validate_custom_sidebar_optional_i64_param(method, params, "priority", &["priority"])
        }
        "workspace.report_meta_block" => {
            require_custom_sidebar_string_param(method, params, "key", &["key", "name"])?;
            require_custom_sidebar_raw_string_param(
                method,
                params,
                "markdown",
                &["markdown", "value", "text"],
            )?;
            validate_custom_sidebar_optional_i64_param(method, params, "priority", &["priority"])
        }
        "workspace.log" => {
            require_custom_sidebar_raw_string_param(method, params, "message", &["message", "text"])
        }
        "surface.focus" => validate_custom_sidebar_optional_surface_selector(method, params),
        "workspace.list_log" => {
            validate_custom_sidebar_optional_usize_param(method, params, "limit", &["limit"])
        }
        method => validate_custom_sidebar_optional_common_params(method, params),
    }
}

fn custom_sidebar_action_schema_method<'a>(method: &'a str) -> &'a str {
    match method {
        "set_progress" | "set-progress" => "workspace.set_progress",
        "clear_progress" | "clear-progress" => "workspace.clear_progress",
        "set_status" | "set-status" => "workspace.set_status",
        "clear_status" | "clear-status" => "workspace.clear_status",
        "list_status" | "list-status" => "workspace.list_status",
        "report_meta" | "report-meta" => "workspace.report_meta",
        "clear_meta" | "clear-meta" => "workspace.clear_meta",
        "list_meta" | "list-meta" => "workspace.list_meta",
        "report_meta_block" | "report-meta-block" => "workspace.report_meta_block",
        "clear_meta_block" | "clear-meta-block" => "workspace.clear_meta_block",
        "list_meta_blocks" | "list-meta-blocks" => "workspace.list_meta_blocks",
        "log" => "workspace.log",
        "clear_log" | "clear-log" => "workspace.clear_log",
        "reset_sidebar" | "reset-sidebar" => "workspace.reset_sidebar",
        other => match other {
            "workspace.set_progress"
            | "workspace.clear_progress"
            | "workspace.set_status"
            | "workspace.clear_status"
            | "workspace.list_status"
            | "workspace.report_meta"
            | "workspace.clear_meta"
            | "workspace.list_meta"
            | "workspace.report_meta_block"
            | "workspace.clear_meta_block"
            | "workspace.list_meta_blocks"
            | "workspace.log"
            | "workspace.clear_log"
            | "workspace.reset_sidebar"
            | "workspace.select"
            | "surface.focus"
            | "workspace.list_log"
            | "sidebar.select"
            | "sidebar.open" => other,
            _ => other,
        },
    }
}

fn require_custom_sidebar_workspace_selector(
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<(), ControlCallResult> {
    if string_param(params, &["workspace_id", "id"]).is_some()
        || string_param(params, &["workspace_ref", "ref"]).is_some()
    {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        "workspace",
        "non-empty workspace_id/id or workspace_ref/ref string",
        CUSTOM_SIDEBAR_WORKSPACE_SELECTOR_KEYS,
    ))
}

fn validate_custom_sidebar_optional_surface_selector(
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<(), ControlCallResult> {
    if !contains_any_param(params, CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS) {
        return Ok(());
    }
    if string_param(params, &["surface_id", "panel_id", "id"]).is_some()
        || string_param(params, &["surface_ref", "ref"]).is_some()
    {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        "surface",
        "non-empty surface_id/panel_id/id or surface_ref/ref string",
        CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS,
    ))
}

fn validate_custom_sidebar_optional_common_params(
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<(), ControlCallResult> {
    validate_custom_sidebar_optional_i64_param(method, params, "priority", &["priority"])?;
    validate_custom_sidebar_optional_usize_param(method, params, "limit", &["limit"])?;
    Ok(())
}

fn require_custom_sidebar_string_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if string_param(params, keys).is_some() {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        field,
        "non-empty string",
        keys,
    ))
}

fn require_custom_sidebar_raw_string_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if raw_string_param(params, keys).is_some() {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method, field, "string", keys,
    ))
}

fn require_custom_sidebar_finite_number_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if f64_param(params, keys).is_some_and(f64::is_finite) {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        field,
        "finite number or numeric string",
        keys,
    ))
}

fn validate_custom_sidebar_optional_i64_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if !contains_any_param(params, keys) || i64_param(params, keys).is_some() {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        field,
        "integer or integer string",
        keys,
    ))
}

fn validate_custom_sidebar_optional_usize_param(
    method: &str,
    params: &serde_json::Map<String, Value>,
    field: &str,
    keys: &'static [&'static str],
) -> Result<(), ControlCallResult> {
    if !contains_any_param(params, keys) || usize_param(params, keys).is_some() {
        return Ok(());
    }
    Err(custom_sidebar_action_schema_invalid(
        method,
        field,
        "non-negative integer or integer string",
        keys,
    ))
}

fn contains_any_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().any(|key| params.contains_key(*key))
}

fn custom_sidebar_action_schema_invalid(
    method: &str,
    field: &str,
    expected: &str,
    accepted_keys: &'static [&'static str],
) -> ControlCallResult {
    ControlCallResult::Err {
        code: "custom_sidebar_action_schema_invalid".to_string(),
        message: format!(
            "Custom sidebar action '{method}' has invalid params: {field} must be {expected}"
        ),
        data: JsonValue::try_from(json!({
            "method": method,
            "schema_method": custom_sidebar_action_schema_method(method),
            "schema_version": CUSTOM_SIDEBAR_ACTION_SCHEMA_VERSION,
            "field": field,
            "expected": expected,
            "accepted_keys": accepted_keys,
        }))
        .ok(),
    }
}

fn custom_sidebar_action_schema_catalog() -> Value {
    json!({
        "version": CUSTOM_SIDEBAR_ACTION_SCHEMA_VERSION,
        "selector_keys": {
            "workspace": CUSTOM_SIDEBAR_WORKSPACE_SELECTOR_KEYS,
            "surface": CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS,
        },
        "methods": [
            {
                "method": "workspace.select",
                "required": [
                    {
                        "field": "workspace",
                        "accepted_keys": CUSTOM_SIDEBAR_WORKSPACE_SELECTOR_KEYS,
                        "expected": "non-empty workspace_id/id or workspace_ref/ref string",
                    },
                ],
            },
            {
                "method": "surface.focus",
                "optional": [
                    {
                        "field": "surface",
                        "accepted_keys": CUSTOM_SIDEBAR_SURFACE_SELECTOR_KEYS,
                        "expected": "non-empty surface_id/panel_id/id or surface_ref/ref string",
                    },
                ],
            },
            {
                "method": "sidebar.select",
                "required": [
                    {
                        "field": "name",
                        "accepted_keys": ["name", "sidebar"],
                        "expected": "non-empty string",
                    },
                ],
            },
            {
                "method": "sidebar.open",
                "required": [
                    {
                        "field": "name",
                        "accepted_keys": ["name", "sidebar"],
                        "expected": "non-empty string",
                    },
                ],
            },
            {
                "method": "workspace.set_progress",
                "aliases": ["set_progress", "set-progress"],
                "required": [
                    {
                        "field": "value",
                        "accepted_keys": ["value", "progress"],
                        "expected": "finite number or numeric string",
                    },
                ],
            },
            {
                "method": "workspace.set_status",
                "aliases": ["set_status", "set-status"],
                "required": [
                    {
                        "field": "key",
                        "accepted_keys": ["key", "name"],
                        "expected": "non-empty string",
                    },
                    {
                        "field": "value",
                        "accepted_keys": ["value", "status", "text"],
                        "expected": "string",
                    },
                ],
                "optional": [
                    {
                        "field": "priority",
                        "accepted_keys": ["priority"],
                        "expected": "integer or integer string",
                    },
                ],
            },
            {
                "method": "workspace.clear_status",
                "aliases": ["clear_status", "clear-status"],
                "required": [
                    {
                        "field": "key",
                        "accepted_keys": ["key", "name"],
                        "expected": "non-empty string",
                    },
                ],
            },
            {
                "method": "workspace.report_meta",
                "aliases": ["report_meta", "report-meta"],
                "required": [
                    {
                        "field": "key",
                        "accepted_keys": ["key", "name"],
                        "expected": "non-empty string",
                    },
                    {
                        "field": "value",
                        "accepted_keys": ["value", "text", "markdown"],
                        "expected": "string",
                    },
                ],
            },
            {
                "method": "workspace.report_meta_block",
                "aliases": ["report_meta_block", "report-meta-block"],
                "required": [
                    {
                        "field": "key",
                        "accepted_keys": ["key", "name"],
                        "expected": "non-empty string",
                    },
                    {
                        "field": "markdown",
                        "accepted_keys": ["markdown", "value", "text"],
                        "expected": "string",
                    },
                ],
            },
            {
                "method": "workspace.log",
                "aliases": ["log"],
                "required": [
                    {
                        "field": "message",
                        "accepted_keys": ["message", "text"],
                        "expected": "string",
                    },
                ],
            },
        ],
    })
}

fn custom_sidebar_action_reply(result: ControlCallResult) -> Value {
    match result {
        ControlCallResult::Ok(value) => json!({
            "ok": true,
            "value": Value::from(value),
        }),
        ControlCallResult::Err {
            code,
            message,
            data,
        } => json!({
            "ok": false,
            "error": {
                "code": code,
                "userMessage": message,
                "data": data.map(Value::from),
            },
        }),
    }
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
    cmux_ipc::control_pipe_path(CONTROL_PIPE_BASE_NAME)
        .expect("static control pipe base name is valid")
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
    "config.reload",
    "window.list",
    "window.current",
    "window.displays",
    "window.display",
    "notification.list",
    "notification.dismiss",
    "notification.mark_read",
    "notification.clear",
    "notification.open",
    "notification.jump_to_unread",
    "notification.create",
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
    Legacy,
}

fn control_request_route_for_method(method: &str) -> ControlRequestRoute {
    match method {
        "pane.create" | "pane.resize" | "pane.focus" | "surface.action" | "tab.action"
        | "surface.create" | "surface.current" | "surface.list" | "surface.report_pwd"
        | "surface.respawn" | "surface.close" | "surface.focus" | "surface.move"
        | "surface.split" => ControlRequestRoute::PaneSurfaceLifecycle,
        _ => ControlRequestRoute::Legacy,
    }
}

impl cmux_ipc::ControlRequestHandler for DesktopControlHandler {
    fn handle(&mut self, request: ControlRequest) -> ControlCallResult {
        handle_control_request(&self.app, request)
    }

    fn handle_stream(&mut self, request: ControlRequest) -> Option<ControlStream> {
        (request.method == "events.stream").then(|| events_live_stream(&self.app, &request.params))
    }
}

fn handle_control_request(app: &AppHandle, mut request: ControlRequest) -> ControlCallResult {
    let session_state = app.state::<SessionState>();
    let _control_guard = match session_state.lock_control_mutation() {
        Ok(guard) => guard,
        Err(message) => {
            return ControlCallResult::Err {
                code: "internal_error".into(),
                message,
                data: None,
            }
        }
    };
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
    resolve_request_handle_refs(app, &mut request.params);
    if control_request_route_for_method(&request.method)
        == ControlRequestRoute::PaneSurfaceLifecycle
    {
        return handle_pane_surface_lifecycle_request(app, &request.method, &request.params);
    }
    match request.method.as_str() {
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
            workspace_list_from_params_for_app(app, &snapshot(app), &request.params)
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
        "workspace.next" => workspace_select_relative(app, 1),
        "workspace.previous" => workspace_select_relative(app, -1),
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
        "pane.swap" => pane_swap(app, &request.params),
        "pane.focus" => pane_focus(app, &request.params),
        "pane.list" => pane_list(app, &request.params),
        "pane.surfaces" => pane_surfaces(app, &request.params),
        "pane.break" => pane_break(app, &request.params),
        "pane.join" => pane_join(app, &request.params),
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

struct ProductionLifecycleExecutor<'a> {
    app: &'a AppHandle,
    candidate: Option<AppSessionSnapshot>,
    previous: Option<AppSessionSnapshot>,
    staged: Vec<pane_surface_lifecycle::LifecycleEffect>,
    staged_terminals: Vec<(String, u32, bool)>,
    staged_remote_creations: Vec<StagedRemoteCreation>,
    deferred_remote_reconciliations: Vec<StagedRemoteCreation>,
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
    retry: BTreeMap<String, RemoteWindowDepartureRetryState>,
}

#[derive(Default)]
struct RemoteWindowDepartureRetryState {
    attempts: usize,
    next_failure_report_at: Option<Instant>,
}

impl RemoteWindowDepartureRegistry {
    fn register(&mut self, pending: PendingRemoteWindowDeparture) -> String {
        self.next_key = self.next_key.saturating_add(1);
        let key = format!("remote-window-departure-{}", self.next_key);
        self.pending.insert(key.clone(), pending);
        self.retry.insert(key.clone(), Default::default());
        key
    }

    fn get(&self, key: &str) -> Option<&PendingRemoteWindowDeparture> {
        self.pending.get(key)
    }

    fn contains(&self, key: &str) -> bool {
        self.pending.contains_key(key)
    }

    fn len(&self) -> usize {
        self.pending.len()
    }

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

fn retain_remote_window_departure_after_kill(
    registry: &mut RemoteWindowDepartureRegistry,
    destination: &str,
    remote_window_id: &str,
    departure: pane_surface_lifecycle::RuntimeDeparture,
) -> Result<String, String> {
    if !valid_tmux_identity(remote_window_id, '@') {
        return Err("invalid killed tmux window identity".into());
    }
    Ok(registry.register(PendingRemoteWindowDeparture {
        destination: destination.into(),
        remote_window_id: remote_window_id.into(),
        departure,
    }))
}

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
    if !valid_tmux_identity(source_pane, '%') {
        return Err("invalid remote tmux pane identity".into());
    }
    let remote_window_id = kill(destination, source_pane)?;
    let mut registry = registry
        .lock()
        .map_err(|_| "remote departure registry lock poisoned".to_string())?;
    retain_remote_window_departure_after_kill(
        &mut registry,
        destination,
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
    if !source_exists || (commit_error.is_some() && attempts >= REMOTE_OBSERVATION_MAX_RETRIES) {
        RemoteObservationAction::CompensateKillWindow
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
    destination: String,
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

fn schedule_remote_window_reconciliation(app: &AppHandle, remote: StagedRemoteCreation) {
    let compensation_token = remote.token.clone();
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(arrival) = staged_remote_arrival(&remote) else {
            return;
        };
        let mut attempt = 0;
        let failure = loop {
            match commit_runtime_arrival_for_control(&app, arrival.clone()) {
                Ok(outcome) => match outcome {
                    RuntimeArrivalCommitOutcome::Committed => {
                        if remote.target == RemoteTmuxTarget::Window
                            && should_focus_window_after_remote_arrival(remote.focus, true)
                        {
                            if let Some(window) = app.get_webview_window(&remote.window_id) {
                                let _ = window.set_focus();
                            }
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
        let compensation = remote_tmux_kill_command(remote.target, &compensation_token)
            .and_then(|command| {
                Command::new("ssh")
                    .args(["-T", "-o", "BatchMode=yes", &remote.destination])
                    .args(command)
                    .status()
                    .map_err(|error| error.to_string())
            })
            .map(|status| status.success())
            .unwrap_or(false);
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
    remote_creations: Vec<StagedRemoteCreation>,
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
            .position(|remote| remote.token == target)
            .ok_or_else(|| format!("remote rollback target {target} is unavailable"))?;
        let remote = self.remote_creations.remove(index);
        let command = remote_tmux_kill_command(remote.target, target)?;
        let status = Command::new("ssh")
            .args(["-T", "-o", "BatchMode=yes", &remote.destination])
            .args(command)
            .status()
            .map_err(|error| format!("failed to launch remote rollback: {error}"))?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("remote rollback exited with {status}"))
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
        for remote in self.deferred_remote_reconciliations.drain(..) {
            schedule_remote_window_reconciliation(self.app, remote);
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
                .map(|remote| remote.token.clone())
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
                ..
            } => {
                let id = terminal_open_for_control(
                    self.app,
                    terminal_state.inner(),
                    Some(surface_id),
                    working_directory.as_deref(),
                    command.as_deref(),
                    None,
                    None,
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
                destination,
                window_id,
                workspace_id,
                tmux_operation,
                arrival_policy,
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
                let output = Command::new("ssh")
                    .args(["-T", "-o", "BatchMode=yes", destination])
                    .args(&argv)
                    .output()
                    .map_err(|error| format!("failed to launch remote tmux create: {error}"))?;
                if !output.status.success() {
                    return Err(format!("remote tmux create exited with {}", output.status));
                }
                let raw_output = String::from_utf8_lossy(&output.stdout);
                let observation = (target == RemoteTmuxTarget::Window)
                    .then(|| parse_remote_tmux_observation(&raw_output))
                    .transpose()?;
                let pane_observation = (target == RemoteTmuxTarget::Pane)
                    .then(|| parse_remote_tmux_pane_observation(&raw_output))
                    .transpose()?;
                let token = observation
                    .as_ref()
                    .map(|observation| observation.window_token.clone())
                    .or_else(|| pane_observation.clone())
                    .unwrap_or_default();
                if token.is_empty() {
                    return Err("remote tmux create returned no target identity".to_string());
                }
                let arrival = if target == RemoteTmuxTarget::Pane {
                    None
                } else {
                    immediate_remote_arrival(
                        target,
                        arrival_policy,
                        window_id,
                        workspace_id,
                        &token,
                    )
                };
                self.staged_remote_creations.push(StagedRemoteCreation {
                    destination: destination.clone(),
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
                });
            }
            pane_surface_lifecycle::LifecycleEffect::BrowserAttach {
                surface_id, url, ..
            } => {
                let window_id = self
                    .candidate
                    .as_ref()
                    .and_then(|candidate| {
                        cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(
                            candidate,
                        )
                        .ok()
                    })
                    .and_then(|model| {
                        model
                            .owner_of_surface(surface_id)
                            .map(|owner| owner.window_id.clone())
                    })
                    .unwrap_or_else(|| "main".into());
                let state = self.app.state::<BrowserWebviewState>();
                browser_attach_webview_for_control(
                    self.app,
                    state.inner(),
                    &window_id,
                    surface_id,
                    url.as_deref(),
                    None,
                    false,
                )?;
                self.staged_browsers
                    .push((window_id, surface_id.clone(), url.clone()));
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
                    .state::<crate::remote_proxy::RemoteProxyBrokerState>()
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
                    if let Some(window) = self.app.get_webview_window(window_id) {
                        let _ = window.set_focus();
                    }
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
                        .state::<crate::remote_proxy::RemoteProxyBrokerState>()
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
                    let result = execute_remote_window_kill_and_register(
                        &registry_state.registry,
                        destination,
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
                    if let Some(window) = self.app.get_webview_window(owner_id) {
                        let _ = window.set_focus();
                    }
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
                .filter(|remote| remote.observation.is_some() || remote.pane_observation.is_some())
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

fn handle_pane_surface_lifecycle_request(
    app: &AppHandle,
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let viewport_size = app.webview_windows().values().find_map(|window| {
        window
            .inner_size()
            .ok()
            .map(|size| (f64::from(size.width), f64::from(size.height)))
    });
    let active_window_id = app.webview_windows().iter().find_map(|(label, window)| {
        (window.is_focused().ok() == Some(true)).then(|| label.clone())
    });
    let mut transition = pane_surface_lifecycle::dispatch_lifecycle_request(
        &current,
        method,
        params,
        &pane_surface_lifecycle::LifecycleDispatchContext {
            viewport_size,
            browser_enabled: app.try_state::<BrowserWebviewState>().is_some(),
            dock_available: app
                .try_state::<crate::right_sidebar::RightSidebarState>()
                .is_some_and(|state| state.beta_settings().dock_enabled),
            active_window_id,
        },
    );
    if let Some(decorated) = decorate_lifecycle_result_refs(app, &mut transition.result) {
        for event in &mut transition.events {
            if let Some(result) = event.payload.get_mut("result") {
                *result = decorated.clone();
            }
        }
    }
    if !transition.changed && transition.effects.is_empty() {
        return transition.result;
    }
    let external_url = transition.effects.iter().find_map(|effect| match effect {
        pane_surface_lifecycle::LifecycleEffect::ExternalBrowserOpen { url, .. } => {
            Some(url.clone())
        }
        _ => None,
    });
    let lifecycle_failure = transition.effects.iter().find_map(|effect| match effect {
        pane_surface_lifecycle::LifecycleEffect::DockCreate {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::RuntimeTeardown {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::TerminalCreate {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::BrowserAttach {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::BrowserReload {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::ExternalBrowserOpen {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::RemoteCreate {
            failure_code,
            failure_message,
            ..
        }
        | pane_surface_lifecycle::LifecycleEffect::RemoteWindowClose {
            failure_code,
            failure_message,
            ..
        } => Some(((*failure_code).to_string(), (*failure_message).to_string())),
        _ => None,
    });
    let publish_snapshot = lifecycle_snapshot_changed(&transition.snapshot, &current);
    let completion_events = transition.events.clone();
    let previous = current.clone();
    let mut target = current;
    let mut executor = ProductionLifecycleExecutor {
        app,
        candidate: None,
        previous: Some(previous),
        staged: Vec::new(),
        staged_terminals: Vec::new(),
        staged_remote_creations: Vec::new(),
        deferred_remote_reconciliations: Vec::new(),
        deferred_remote_departures: Vec::new(),
        staged_browsers: Vec::new(),
        dock_journal: DockCommitJournal::default(),
    };
    let result =
        pane_surface_lifecycle::commit_lifecycle_transition(&mut target, transition, &mut executor)
            .unwrap_or_else(|message| {
                if message == "Failed to open URL externally" {
                    ControlCallResult::Err {
                        code: "external_open_failed".into(),
                        message,
                        data: external_url
                            .and_then(|url| JsonValue::try_from(json!({"url":url})).ok()),
                    }
                } else if message.contains("Lifecycle rollback failed:") {
                    ControlCallResult::Err {
                        code: "internal_error".into(),
                        message,
                        data: None,
                    }
                } else {
                    let (code, mapped_message) = lifecycle_failure
                        .clone()
                        .unwrap_or_else(|| ("internal_error".into(), message.clone()));
                    ControlCallResult::Err {
                        code,
                        message: mapped_message,
                        data: None,
                    }
                }
            });
    if matches!(result, ControlCallResult::Ok(_)) {
        if publish_snapshot {
            let suppressed = completion_events
                .iter()
                .filter(|event| event.source == "workspace.lifecycle")
                .map(|event| event.name)
                .collect::<HashSet<_>>();
            record_session_changed_event_suppressing(app, &target, &suppressed);
        }
        for completion in completion_events {
            record_event(
                app,
                completion.name,
                completion.category,
                completion.source,
                completion.window_id,
                completion.workspace_id,
                completion.pane_id,
                completion.surface_id,
                completion.payload,
            );
        }
        executor.flush_deferred_remote_reconciliations();
        executor.flush_deferred_remote_departures();
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeArrivalCommitOutcome {
    Committed,
    DuplicateOrStale,
    SourceMissing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeDepartureCommitOutcome {
    Committed,
    DuplicateOrStale,
}

fn commit_runtime_arrival_for_control(
    app: &AppHandle,
    arrival: pane_surface_lifecycle::RuntimeArrival,
) -> Result<RuntimeArrivalCommitOutcome, String> {
    let state = app.state::<SessionState>();
    let _control_guard = state.lock_control_mutation()?;
    let current = current_session_snapshot(&state);
    let model = cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(&current)
        .map_err(|error| error.to_string())?;
    if arrival.generation == 0 || model.surface(&arrival.surface_id).is_some() {
        return Ok(RuntimeArrivalCommitOutcome::DuplicateOrStale);
    }
    let scope_exists =
        current.windows.iter().any(|window| {
            window.window_id.as_deref() == Some(&arrival.window_id)
                && window.tab_manager.workspaces.iter().any(|workspace| {
                    workspace.workspace_id.as_deref() == Some(&arrival.workspace_id)
                })
        });
    if !scope_exists {
        return Ok(RuntimeArrivalCommitOutcome::SourceMissing);
    }
    if !arrival.creates_pane {
        let source_matches = arrival.anchor_surface_id.as_ref().is_some_and(|source| {
            model.owner_of_surface(source).is_some_and(|owner| {
                owner.window_id == arrival.window_id
                    && owner.workspace_id == arrival.workspace_id
                    && owner.pane_id == arrival.pane_id
            })
        });
        if !source_matches {
            return Ok(RuntimeArrivalCommitOutcome::SourceMissing);
        }
    } else if let Some(source) = &arrival.anchor_surface_id {
        let source_matches = model.owner_of_surface(source).is_some_and(|owner| {
            owner.window_id == arrival.window_id
                && owner.workspace_id == arrival.workspace_id
                && arrival.source_pane_id.as_deref() == Some(owner.pane_id.as_str())
        });
        if !source_matches {
            return Ok(RuntimeArrivalCommitOutcome::SourceMissing);
        }
    }
    let reconciled = pane_surface_lifecycle::reconcile_runtime_arrival(&current, arrival.clone());
    if reconciled.snapshot == current {
        return Ok(RuntimeArrivalCommitOutcome::DuplicateOrStale);
    }
    commit_lifecycle_snapshot_for_control(app, state.inner(), &reconciled.snapshot, false)?;
    record_session_changed_event_suppressing(
        app,
        &reconciled.snapshot,
        &HashSet::from(["pane.created", "surface.created"]),
    );
    let (emit_pane_created, origin) = runtime_arrival_event_semantics(&arrival);
    if emit_pane_created {
        record_event(
            app,
            "pane.created",
            "pane",
            "workspace.lifecycle",
            Some(arrival.window_id.clone()),
            Some(arrival.workspace_id.clone()),
            Some(arrival.pane_id.clone()),
            Some(arrival.surface_id.clone()),
            json!({"pane_id":arrival.pane_id,"source_pane_id":arrival.source_pane_id,"orientation":arrival.split_orientation,"surface_id":arrival.surface_id,"origin":"terminal_split"}),
        );
    }
    record_event(
        app,
        "surface.created",
        "surface",
        "workspace.lifecycle",
        Some(arrival.window_id),
        Some(arrival.workspace_id),
        Some(arrival.pane_id.clone()),
        Some(arrival.surface_id.clone()),
        json!({"surface_id":arrival.surface_id,"pane_id":arrival.pane_id,"kind":"terminal","origin":origin,"focused":arrival.focused}),
    );
    Ok(RuntimeArrivalCommitOutcome::Committed)
}

fn commit_runtime_departure_for_control(
    app: &AppHandle,
    departure: pane_surface_lifecycle::RuntimeDeparture,
) -> Result<RuntimeDepartureCommitOutcome, String> {
    let state = app.state::<SessionState>();
    let _control_guard = state.lock_control_mutation()?;
    let current = current_session_snapshot(&state);
    let model = cmux_core::surface_lifecycle::SurfaceLifecycleModel::from_app_session(&current)
        .map_err(|error| error.to_string())?;
    let Some(record) = model.surface(&departure.surface_id) else {
        return Ok(RuntimeDepartureCommitOutcome::DuplicateOrStale);
    };
    let owner_matches = model
        .owner_of_surface(&departure.surface_id)
        .is_some_and(|owner| {
            owner.window_id == departure.window_id
                && owner.workspace_id == departure.workspace_id
                && owner.pane_id == departure.pane_id
        });
    if record.generation != departure.generation || !owner_matches {
        return Ok(RuntimeDepartureCommitOutcome::DuplicateOrStale);
    }
    let reconciled = pane_surface_lifecycle::reconcile_runtime_departure(&current, &departure);
    if reconciled.snapshot == current {
        return Err("remote departure reconciliation made no progress".into());
    }
    commit_lifecycle_snapshot_for_control(app, state.inner(), &reconciled.snapshot, false)?;
    record_session_changed_event_suppressing(
        app,
        &reconciled.snapshot,
        &HashSet::from(["surface.closed"]),
    );
    record_event(
        app,
        "surface.closed",
        "surface",
        "workspace.lifecycle",
        Some(departure.window_id),
        Some(departure.workspace_id),
        Some(departure.pane_id),
        Some(departure.surface_id.clone()),
        json!({"surface_id":departure.surface_id,"origin":"remote_window_close"}),
    );
    Ok(RuntimeDepartureCommitOutcome::Committed)
}

fn runtime_arrival_event_semantics(
    arrival: &pane_surface_lifecycle::RuntimeArrival,
) -> (bool, &'static str) {
    if arrival.creates_pane {
        (true, "terminal_split")
    } else {
        (false, "terminal_tab")
    }
}

const LIFECYCLE_ID_REF_FIELDS: [(&str, &str, &str); 10] = [
    ("window_id", "window_ref", "window"),
    ("source_window_id", "source_window_ref", "window"),
    ("workspace_id", "workspace_ref", "workspace"),
    ("source_workspace_id", "source_workspace_ref", "workspace"),
    ("created_workspace_id", "created_workspace_ref", "workspace"),
    ("pane_id", "pane_ref", "pane"),
    ("surface_id", "surface_ref", "surface"),
    ("created_surface_id", "created_surface_ref", "surface"),
    ("tab_id", "tab_ref", "surface"),
    ("created_tab_id", "created_tab_ref", "surface"),
];

fn decorate_lifecycle_result_refs(
    app: &AppHandle,
    result: &mut ControlCallResult,
) -> Option<Value> {
    let success = matches!(result, ControlCallResult::Ok(_));
    let payload = match result {
        ControlCallResult::Ok(payload) => payload,
        ControlCallResult::Err {
            data: Some(data), ..
        } => data,
        ControlCallResult::Err { data: None, .. } => return None,
    };
    let mut value = Value::from(payload.clone());
    decorate_lifecycle_value_refs(&mut value, &mut |kind, id| {
        control_handle_ref(app, kind, id)
    });
    if let Ok(decorated) = JsonValue::try_from(value) {
        *payload = decorated;
    }
    success.then(|| Value::from(payload.clone()))
}

fn decorate_lifecycle_value_refs(
    value: &mut Value,
    mint: &mut impl FnMut(&'static str, &str) -> String,
) {
    fn decorate(
        value: &mut Value,
        row_is_surface: bool,
        mint: &mut impl FnMut(&'static str, &str) -> String,
    ) {
        match value {
            Value::Array(values) => {
                for value in values {
                    decorate(value, true, mint);
                }
            }
            Value::Object(object) => {
                for (id_key, ref_key, kind) in LIFECYCLE_ID_REF_FIELDS {
                    if let Some(id) = object.get(id_key) {
                        let reference = id.as_str().map(|id| {
                            let reference = mint(kind, id);
                            if matches!(ref_key, "tab_ref" | "created_tab_ref") {
                                tab_ref_from_surface_ref(&reference)
                            } else {
                                reference
                            }
                        });
                        object.entry(ref_key).or_insert_with(|| json!(reference));
                    }
                }
                if row_is_surface {
                    if let Some(id) = object.get("id").and_then(Value::as_str).map(str::to_owned) {
                        object
                            .entry("ref")
                            .or_insert_with(|| json!(mint("surface", &id)));
                    }
                }
                for child in object.values_mut() {
                    decorate(child, false, mint);
                }
            }
            _ => {}
        }
    }
    decorate(value, false, mint);
}

fn resolve_request_handle_refs(app: &AppHandle, params: &mut serde_json::Map<String, Value>) {
    for (key, kind) in [
        ("window_id", "window"),
        ("group_id", "workspace_group"),
        ("workspace_id", "workspace"),
        ("group_reference_workspace_id", "workspace"),
        ("reference_workspace_id", "workspace"),
        ("surface_id", "surface"),
        ("terminal_id", "surface"),
        ("tab_id", "surface"),
        ("pane_id", "pane"),
    ] {
        let Some(reference) = params.get(key).and_then(Value::as_str) else {
            continue;
        };
        let normalized = (key == "tab_id")
            .then(|| surface_ref_from_tab_ref(reference))
            .flatten();
        if let Some(id) =
            resolve_control_handle_ref(app, kind, normalized.as_deref().unwrap_or(reference))
        {
            params.insert(key.to_string(), json!(id));
        }
    }
}

fn tab_ref_from_surface_ref(reference: &str) -> String {
    reference
        .strip_prefix("surface:")
        .map_or_else(|| reference.to_string(), |suffix| format!("tab:{suffix}"))
}

fn surface_ref_from_tab_ref(reference: &str) -> Option<String> {
    reference
        .strip_prefix("tab:")
        .map(|suffix| format!("surface:{suffix}"))
}

pub(crate) fn record_session_changed_event(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    record_session_changed_event_suppressing(app, snapshot, &HashSet::new());
}

fn record_session_changed_event_suppressing(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    suppressed_names: &HashSet<&str>,
) {
    let Some(state) = app.try_state::<ControlEventState>() else {
        return;
    };
    let current = session_event_summaries(snapshot);
    let previous = {
        let mut guard = state
            .inner
            .lock()
            .expect("control event log mutex poisoned");
        let previous = guard.last_session_summaries.clone();
        guard.last_session_summaries = current.clone();
        previous
    };
    for (key, summary) in &current {
        for event in derived_session_event_specs(previous.get(key), summary) {
            if suppressed_names.contains(event.name) {
                continue;
            }
            let pane_id = event
                .payload
                .get("pane_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            record_event(
                app,
                event.name,
                event.category,
                event.source,
                event.window_id,
                event.workspace_id,
                pane_id,
                event.surface_id,
                event.payload,
            );
        }
    }
    for (key, previous_summary) in &previous {
        if current.contains_key(key) {
            continue;
        }
        let empty = SessionEventSummary {
            window_id: previous_summary.window_id.clone(),
            selected_workspace_id: None,
            selected_workspace_index: None,
            workspaces: Vec::new(),
        };
        for event in derived_session_event_specs(Some(previous_summary), &empty) {
            if suppressed_names.contains(event.name) {
                continue;
            }
            let pane_id = event
                .payload
                .get("pane_id")
                .and_then(Value::as_str)
                .map(str::to_owned);
            record_event(
                app,
                event.name,
                event.category,
                event.source,
                event.window_id,
                event.workspace_id,
                pane_id,
                event.surface_id,
                event.payload,
            );
        }
    }
}

fn session_event_summaries(snapshot: &AppSessionSnapshot) -> BTreeMap<String, SessionEventSummary> {
    snapshot
        .windows
        .iter()
        .enumerate()
        .map(|(index, window)| {
            let key = window
                .window_id
                .clone()
                .unwrap_or_else(|| format!("window-index:{index}"));
            (key, session_event_summary_for_window(window, index))
        })
        .collect()
}

fn session_event_summary_for_window(
    window: &cmux_core::session::SessionWindowSnapshot,
    window_index: usize,
) -> SessionEventSummary {
    if window.tab_manager.workspaces.is_empty() {
        return SessionEventSummary {
            window_id: window.window_id.clone(),
            selected_workspace_id: None,
            selected_workspace_index: None,
            workspaces: Vec::new(),
        };
    }
    let selected_index = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < window.tab_manager.workspaces.len());
    let workspaces: Vec<WorkspaceEventSummary> = window
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .map(|(index, workspace)| {
            let panes = pane_event_summaries(workspace);
            let surfaces = surfaces_for_workspace(workspace);
            let surface_ids: Vec<String> = surfaces
                .iter()
                .filter_map(|surface| surface.get("id").and_then(Value::as_str))
                .map(str::to_string)
                .collect();
            let selected_surface_id = surfaces
                .iter()
                .find(|surface| {
                    surface
                        .get("focused")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                })
                .or_else(|| surfaces.first())
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
            WorkspaceEventSummary {
                key: workspace
                    .workspace_id
                    .clone()
                    .unwrap_or_else(|| format!("{}:{index}", window_ref(window_index))),
                id: workspace.workspace_id.clone(),
                title: workspace_display_name(workspace),
                index,
                panes,
                surface_ids,
                selected_surface_id,
                sidebar: sidebar_event_summary(workspace),
            }
        })
        .collect();
    let selected_workspace_id = selected_index
        .and_then(|index| workspaces.get(index))
        .and_then(|workspace| workspace.id.clone());
    SessionEventSummary {
        window_id: window.window_id.clone(),
        selected_workspace_id,
        selected_workspace_index: selected_index,
        workspaces,
    }
}

fn derived_session_event_specs(
    previous: Option<&SessionEventSummary>,
    current: &SessionEventSummary,
) -> Vec<DerivedEventSpec> {
    let mut events = vec![session_changed_event_spec(current)];
    match previous {
        Some(previous) => {
            append_workspace_diff_events(&mut events, previous, current);
            append_pane_diff_events(&mut events, previous, current);
            append_surface_diff_events(&mut events, previous, current);
            append_sidebar_diff_events(&mut events, previous, current);
        }
        None => {
            for workspace in &current.workspaces {
                events.push(workspace_event_spec(
                    "workspace.created",
                    current,
                    workspace,
                    None,
                ));
                for pane in &workspace.panes {
                    events.push(pane_event_spec(
                        "pane.created",
                        current,
                        workspace,
                        pane,
                        None,
                    ));
                    if pane.selected_surface_id.is_some() {
                        events.push(pane_event_spec(
                            "pane.focused",
                            current,
                            workspace,
                            pane,
                            None,
                        ));
                    }
                }
                for surface_id in &workspace.surface_ids {
                    events.push(surface_event_spec(
                        "surface.created",
                        current,
                        workspace,
                        surface_id,
                        None,
                    ));
                }
            }
            if let Some(workspace) = selected_workspace(current) {
                events.push(workspace_event_spec(
                    "workspace.selected",
                    current,
                    workspace,
                    None,
                ));
                if let Some(surface_id) = workspace.selected_surface_id.as_deref() {
                    events.push(surface_event_spec(
                        "surface.selected",
                        current,
                        workspace,
                        surface_id,
                        None,
                    ));
                }
            }
        }
    }
    events
}

fn sidebar_event_summary(workspace: &SessionWorkspaceSnapshot) -> WorkspaceSidebarEventSummary {
    WorkspaceSidebarEventSummary {
        progress: workspace
            .sidebar_progress
            .as_ref()
            .map(json_value_for_event),
        status_entries: workspace
            .sidebar_status_entries
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|entry| (entry.key.clone(), json_value_for_event(entry)))
            .collect(),
        metadata_entries: workspace
            .sidebar_metadata_entries
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|entry| (entry.key.clone(), json_value_for_event(entry)))
            .collect(),
        metadata_blocks: workspace
            .sidebar_metadata_blocks
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|entry| (entry.key.clone(), json_value_for_event(entry)))
            .collect(),
        log_entries: workspace
            .sidebar_log_entries
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(json_value_for_event)
            .collect(),
    }
}

fn json_value_for_event<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn pane_event_summaries(workspace: &SessionWorkspaceSnapshot) -> Vec<PaneEventSummary> {
    let mut panes = Vec::new();
    if let Some(layout) = workspace.layout.as_ref() {
        collect_pane_event_summaries(layout, &mut panes);
    }
    panes
}

fn collect_pane_event_summaries(
    layout: &SessionWorkspaceLayoutSnapshot,
    panes: &mut Vec<PaneEventSummary>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            let index = panes.len();
            let selected_surface_id = pane
                .selected_panel_id
                .clone()
                .or_else(|| pane.panel_ids.first().cloned());
            panes.push(PaneEventSummary {
                key: pane.pane_id.clone().unwrap_or_else(|| pane_ref(index)),
                id: pane.pane_id.clone(),
                index,
                surface_ids: pane.panel_ids.clone(),
                selected_surface_id,
            });
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            collect_pane_event_summaries(&split.first, panes);
            collect_pane_event_summaries(&split.second, panes);
        }
    }
}

fn append_workspace_diff_events(
    events: &mut Vec<DerivedEventSpec>,
    previous: &SessionEventSummary,
    current: &SessionEventSummary,
) {
    let previous_by_key: HashMap<&str, &WorkspaceEventSummary> = previous
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    let current_by_key: HashMap<&str, &WorkspaceEventSummary> = current
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();

    if previous.selected_workspace_id != current.selected_workspace_id
        || previous.selected_workspace_index != current.selected_workspace_index
    {
        if let Some(workspace) = selected_workspace(current) {
            events.push(workspace_event_spec(
                "workspace.selected",
                current,
                workspace,
                previous.selected_workspace_id.as_deref(),
            ));
        }
    }

    for workspace in &current.workspaces {
        match previous_by_key.get(workspace.key.as_str()) {
            Some(previous_workspace) => {
                if previous_workspace.title != workspace.title {
                    events.push(workspace_renamed_event_spec(
                        current,
                        workspace,
                        &previous_workspace.title,
                    ));
                }
            }
            None => events.push(workspace_event_spec(
                "workspace.created",
                current,
                workspace,
                None,
            )),
        }
    }

    for workspace in &previous.workspaces {
        if !current_by_key.contains_key(workspace.key.as_str()) {
            events.push(workspace_event_spec(
                "workspace.closed",
                current,
                workspace,
                None,
            ));
        }
    }

    let previous_keys: Vec<&str> = previous
        .workspaces
        .iter()
        .map(|workspace| workspace.key.as_str())
        .collect();
    let current_keys: Vec<&str> = current
        .workspaces
        .iter()
        .map(|workspace| workspace.key.as_str())
        .collect();
    let previous_set: HashSet<&str> = previous_keys.iter().copied().collect();
    let current_set: HashSet<&str> = current_keys.iter().copied().collect();
    if previous_keys != current_keys && previous_set == current_set {
        let moved_workspace_ids: Vec<String> = current
            .workspaces
            .iter()
            .filter(|workspace| {
                previous_by_key
                    .get(workspace.key.as_str())
                    .is_some_and(|previous_workspace| previous_workspace.index != workspace.index)
            })
            .map(workspace_event_identifier)
            .collect();
        events.push(DerivedEventSpec {
            name: "workspace.reordered",
            category: "workspace",
            source: "session.model",
            window_id: current.window_id.clone(),
            workspace_id: current.selected_workspace_id.clone(),
            surface_id: None,
            payload: json!({
                "window_id": current.window_id,
                "workspace_id": current.selected_workspace_id,
                "workspace_count": current.workspaces.len(),
                "selected_workspace_index": current.selected_workspace_index,
                "workspace_ids": current.workspaces.iter().map(workspace_event_identifier).collect::<Vec<_>>(),
                "moved_workspace_ids": moved_workspace_ids,
                "count": current.workspaces.len(),
                "origin": "session.changed",
            }),
        });
    }
}

fn append_surface_diff_events(
    events: &mut Vec<DerivedEventSpec>,
    previous: &SessionEventSummary,
    current: &SessionEventSummary,
) {
    let previous_by_key: HashMap<&str, &WorkspaceEventSummary> = previous
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    let current_by_key: HashMap<&str, &WorkspaceEventSummary> = current
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    let previous_owners = previous
        .workspaces
        .iter()
        .flat_map(|workspace| {
            workspace
                .surface_ids
                .iter()
                .map(move |id| (id.as_str(), workspace))
        })
        .collect::<HashMap<_, _>>();
    let current_owners = current
        .workspaces
        .iter()
        .flat_map(|workspace| {
            workspace
                .surface_ids
                .iter()
                .map(move |id| (id.as_str(), workspace))
        })
        .collect::<HashMap<_, _>>();
    let moved = current_owners
        .iter()
        .filter_map(|(surface_id, destination)| {
            previous_owners
                .get(surface_id)
                .filter(|source| source.key != destination.key)
                .map(|source| ((*surface_id).to_string(), *source, *destination))
        })
        .collect::<Vec<_>>();
    let moved_ids = moved
        .iter()
        .map(|(surface_id, _, _)| surface_id.as_str())
        .collect::<HashSet<_>>();
    for workspace in &current.workspaces {
        let Some(previous_workspace) = previous_by_key.get(workspace.key.as_str()) else {
            for surface_id in &workspace.surface_ids {
                events.push(surface_event_spec(
                    "surface.created",
                    current,
                    workspace,
                    surface_id,
                    None,
                ));
            }
            continue;
        };
        let previous_surfaces: HashSet<&str> = previous_workspace
            .surface_ids
            .iter()
            .map(String::as_str)
            .collect();
        let current_surfaces: HashSet<&str> =
            workspace.surface_ids.iter().map(String::as_str).collect();
        for surface_id in &workspace.surface_ids {
            if !moved_ids.contains(surface_id.as_str())
                && !previous_surfaces.contains(surface_id.as_str())
            {
                events.push(surface_event_spec(
                    "surface.created",
                    current,
                    workspace,
                    surface_id,
                    None,
                ));
            }
        }
        for surface_id in &previous_workspace.surface_ids {
            if !moved_ids.contains(surface_id.as_str())
                && !current_surfaces.contains(surface_id.as_str())
            {
                events.push(surface_event_spec(
                    "surface.closed",
                    current,
                    workspace,
                    surface_id,
                    None,
                ));
            }
        }
        if previous_workspace.selected_surface_id != workspace.selected_surface_id {
            if let Some(surface_id) = workspace.selected_surface_id.as_deref() {
                events.push(surface_event_spec(
                    "surface.selected",
                    current,
                    workspace,
                    surface_id,
                    previous_workspace.selected_surface_id.as_deref(),
                ));
            }
        }
    }
    for workspace in &previous.workspaces {
        if current_by_key.contains_key(workspace.key.as_str()) {
            continue;
        }
        for surface_id in &workspace.surface_ids {
            if moved_ids.contains(surface_id.as_str()) {
                continue;
            }
            events.push(surface_event_spec(
                "surface.closed",
                current,
                workspace,
                surface_id,
                None,
            ));
        }
    }
}

fn append_pane_diff_events(
    events: &mut Vec<DerivedEventSpec>,
    previous: &SessionEventSummary,
    current: &SessionEventSummary,
) {
    let previous_by_key: HashMap<&str, &WorkspaceEventSummary> = previous
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    for workspace in &current.workspaces {
        let Some(previous_workspace) = previous_by_key.get(workspace.key.as_str()) else {
            for pane in &workspace.panes {
                events.push(pane_event_spec(
                    "pane.created",
                    current,
                    workspace,
                    pane,
                    None,
                ));
                if pane.selected_surface_id.is_some() {
                    events.push(pane_event_spec(
                        "pane.focused",
                        current,
                        workspace,
                        pane,
                        None,
                    ));
                }
            }
            continue;
        };
        let previous_panes: HashMap<&str, &PaneEventSummary> = previous_workspace
            .panes
            .iter()
            .map(|pane| (pane.key.as_str(), pane))
            .collect();
        let current_panes: HashMap<&str, &PaneEventSummary> = workspace
            .panes
            .iter()
            .map(|pane| (pane.key.as_str(), pane))
            .collect();
        for pane in &workspace.panes {
            match previous_panes.get(pane.key.as_str()) {
                Some(previous_pane) => {
                    if previous_pane.selected_surface_id != pane.selected_surface_id {
                        events.push(pane_event_spec(
                            "pane.focused",
                            current,
                            workspace,
                            pane,
                            previous_pane.selected_surface_id.as_deref(),
                        ));
                    }
                }
                None => {
                    events.push(pane_event_spec(
                        "pane.created",
                        current,
                        workspace,
                        pane,
                        None,
                    ));
                    if pane.selected_surface_id.is_some() {
                        events.push(pane_event_spec(
                            "pane.focused",
                            current,
                            workspace,
                            pane,
                            None,
                        ));
                    }
                }
            }
        }
        for pane in &previous_workspace.panes {
            if !current_panes.contains_key(pane.key.as_str()) {
                events.push(pane_event_spec(
                    "pane.closed",
                    current,
                    workspace,
                    pane,
                    None,
                ));
            }
        }
    }
    let current_by_key: HashMap<&str, &WorkspaceEventSummary> = current
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    for workspace in &previous.workspaces {
        if current_by_key.contains_key(workspace.key.as_str()) {
            continue;
        }
        for pane in &workspace.panes {
            events.push(pane_event_spec(
                "pane.closed",
                current,
                workspace,
                pane,
                None,
            ));
        }
    }
}

fn append_sidebar_diff_events(
    events: &mut Vec<DerivedEventSpec>,
    previous: &SessionEventSummary,
    current: &SessionEventSummary,
) {
    let previous_by_key: HashMap<&str, &WorkspaceEventSummary> = previous
        .workspaces
        .iter()
        .map(|workspace| (workspace.key.as_str(), workspace))
        .collect();
    for workspace in &current.workspaces {
        let Some(previous_workspace) = previous_by_key.get(workspace.key.as_str()) else {
            continue;
        };
        append_sidebar_progress_diff(events, current, previous_workspace, workspace);
        append_sidebar_metadata_collection_diff(
            events,
            current,
            previous_workspace,
            workspace,
            "status",
            &previous_workspace.sidebar.status_entries,
            &workspace.sidebar.status_entries,
        );
        append_sidebar_metadata_collection_diff(
            events,
            current,
            previous_workspace,
            workspace,
            "metadata",
            &previous_workspace.sidebar.metadata_entries,
            &workspace.sidebar.metadata_entries,
        );
        append_sidebar_metadata_collection_diff(
            events,
            current,
            previous_workspace,
            workspace,
            "metadata_block",
            &previous_workspace.sidebar.metadata_blocks,
            &workspace.sidebar.metadata_blocks,
        );
        append_sidebar_log_diff(events, current, previous_workspace, workspace);
    }
}

fn append_sidebar_progress_diff(
    events: &mut Vec<DerivedEventSpec>,
    current: &SessionEventSummary,
    previous_workspace: &WorkspaceEventSummary,
    workspace: &WorkspaceEventSummary,
) {
    match (
        previous_workspace.sidebar.progress.as_ref(),
        workspace.sidebar.progress.as_ref(),
    ) {
        (previous, Some(value)) if previous != Some(value) => {
            events.push(sidebar_event_spec(
                "sidebar.progress.updated",
                current,
                workspace,
                json!({
                    "kind": "progress",
                    "value": value,
                    "previous_value": previous.cloned(),
                }),
            ));
        }
        (Some(previous), None) => {
            events.push(sidebar_event_spec(
                "sidebar.progress.cleared",
                current,
                workspace,
                json!({
                    "kind": "progress",
                    "previous_value": previous,
                }),
            ));
        }
        _ => {}
    }
}

fn append_sidebar_metadata_collection_diff(
    events: &mut Vec<DerivedEventSpec>,
    current: &SessionEventSummary,
    _previous_workspace: &WorkspaceEventSummary,
    workspace: &WorkspaceEventSummary,
    kind: &'static str,
    previous_entries: &BTreeMap<String, Value>,
    current_entries: &BTreeMap<String, Value>,
) {
    for (key, value) in current_entries {
        let previous_value = previous_entries.get(key);
        if previous_value != Some(value) {
            events.push(sidebar_event_spec(
                "sidebar.metadata.updated",
                current,
                workspace,
                json!({
                    "kind": kind,
                    "key": key,
                    "value": value,
                    "previous_value": previous_value.cloned(),
                    "count": current_entries.len(),
                }),
            ));
        }
    }
    for (key, previous_value) in previous_entries {
        if !current_entries.contains_key(key) {
            events.push(sidebar_event_spec(
                "sidebar.metadata.cleared",
                current,
                workspace,
                json!({
                    "kind": kind,
                    "key": key,
                    "previous_value": previous_value,
                    "count": current_entries.len(),
                }),
            ));
        }
    }
}

fn append_sidebar_log_diff(
    events: &mut Vec<DerivedEventSpec>,
    current: &SessionEventSummary,
    previous_workspace: &WorkspaceEventSummary,
    workspace: &WorkspaceEventSummary,
) {
    let previous_entries = &previous_workspace.sidebar.log_entries;
    let current_entries = &workspace.sidebar.log_entries;
    if previous_entries == current_entries {
        return;
    }
    if current_entries.is_empty() {
        if !previous_entries.is_empty() {
            events.push(sidebar_event_spec(
                "sidebar.log.cleared",
                current,
                workspace,
                json!({
                    "kind": "log",
                    "previous_count": previous_entries.len(),
                    "count": 0,
                }),
            ));
        }
        return;
    }
    let appended = if current_entries.len() >= previous_entries.len()
        && current_entries.starts_with(previous_entries)
    {
        current_entries[previous_entries.len()..].to_vec()
    } else {
        current_entries.clone()
    };
    events.push(sidebar_event_spec(
        "sidebar.log.appended",
        current,
        workspace,
        json!({
            "kind": "log",
            "entries": appended,
            "appended_count": current_entries.len().saturating_sub(previous_entries.len()),
            "previous_count": previous_entries.len(),
            "count": current_entries.len(),
            "replaced": !current_entries.starts_with(previous_entries),
        }),
    ));
}

fn session_changed_event_spec(current: &SessionEventSummary) -> DerivedEventSpec {
    DerivedEventSpec {
        name: "session.changed",
        category: "session",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: current.selected_workspace_id.clone(),
        surface_id: selected_workspace(current)
            .and_then(|workspace| workspace.selected_surface_id.clone()),
        payload: json!({
            "window_id": current.window_id,
            "workspace_id": current.selected_workspace_id,
            "workspace_count": current.workspaces.len(),
            "selected_workspace_index": current.selected_workspace_index,
            "surface_id": selected_workspace(current).and_then(|workspace| workspace.selected_surface_id.clone()),
            "origin": "session.changed",
        }),
    }
}

fn workspace_event_spec(
    name: &'static str,
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    previous_workspace_id: Option<&str>,
) -> DerivedEventSpec {
    let mut payload = workspace_event_payload(current, workspace);
    if let Some(previous_workspace_id) = previous_workspace_id {
        payload["previous_workspace_id"] = json!(previous_workspace_id);
    }
    DerivedEventSpec {
        name,
        category: "workspace",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: None,
        payload,
    }
}

fn workspace_renamed_event_spec(
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    previous_title: &str,
) -> DerivedEventSpec {
    let mut payload = workspace_event_payload(current, workspace);
    payload["previous_title"] = json!(previous_title);
    DerivedEventSpec {
        name: "workspace.renamed",
        category: "workspace",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: None,
        payload,
    }
}

fn workspace_event_payload(
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
) -> Value {
    json!({
        "window_id": current.window_id,
        "workspace_id": workspace.id,
        "workspace_ref": workspace_ref(workspace.index),
        "workspace_key": workspace.key,
        "workspace_count": current.workspaces.len(),
        "selected_workspace_index": current.selected_workspace_index,
        "index": workspace.index,
        "title": workspace.title,
        "tab_count": workspace.surface_ids.len(),
        "origin": "session.changed",
    })
}

fn sidebar_event_spec(
    name: &'static str,
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    detail: Value,
) -> DerivedEventSpec {
    let mut payload = workspace_event_payload(current, workspace);
    if let (Some(payload), Some(detail)) = (payload.as_object_mut(), detail.as_object()) {
        for (key, value) in detail {
            payload.insert(key.clone(), value.clone());
        }
    }
    DerivedEventSpec {
        name,
        category: "sidebar",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: None,
        payload,
    }
}

fn pane_event_spec(
    name: &'static str,
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    pane: &PaneEventSummary,
    previous_surface_id: Option<&str>,
) -> DerivedEventSpec {
    let mut payload = json!({
        "window_id": current.window_id,
        "workspace_id": workspace.id,
        "workspace_ref": workspace_ref(workspace.index),
        "pane_id": pane.id,
        "pane_ref": pane_ref(pane.index),
        "pane_key": pane.key,
        "index": pane.index,
        "surface_ids": pane.surface_ids,
        "selected_surface_id": pane.selected_surface_id,
        "surface_id": pane.selected_surface_id,
        "origin": "session.changed",
    });
    if let Some(previous_surface_id) = previous_surface_id {
        payload["previous_surface_id"] = json!(previous_surface_id);
    }
    DerivedEventSpec {
        name,
        category: "pane",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: pane.selected_surface_id.clone(),
        payload,
    }
}

fn surface_event_spec(
    name: &'static str,
    current: &SessionEventSummary,
    workspace: &WorkspaceEventSummary,
    surface_id: &str,
    previous_surface_id: Option<&str>,
) -> DerivedEventSpec {
    let mut payload = json!({
        "window_id": current.window_id,
        "workspace_id": workspace.id,
        "workspace_ref": workspace_ref(workspace.index),
        "surface_id": surface_id,
        "surface_ref": surface_ref_for_summary(workspace, surface_id),
        "selected_surface_id": workspace.selected_surface_id,
        "index": workspace.surface_ids.iter().position(|id| id == surface_id),
        "tab_count": workspace.surface_ids.len(),
        "focused": workspace.selected_surface_id.as_deref() == Some(surface_id),
        "origin": "session.changed",
    });
    if let Some(previous_surface_id) = previous_surface_id {
        payload["previous_surface_id"] = json!(previous_surface_id);
    }
    DerivedEventSpec {
        name,
        category: "surface",
        source: "session.model",
        window_id: current.window_id.clone(),
        workspace_id: workspace.id.clone(),
        surface_id: Some(surface_id.to_string()),
        payload,
    }
}

fn selected_workspace(current: &SessionEventSummary) -> Option<&WorkspaceEventSummary> {
    current
        .selected_workspace_index
        .and_then(|index| current.workspaces.get(index))
}

fn workspace_event_identifier(workspace: &WorkspaceEventSummary) -> String {
    workspace
        .id
        .clone()
        .unwrap_or_else(|| workspace.key.clone())
}

fn surface_ref_for_summary(workspace: &WorkspaceEventSummary, surface_id: &str) -> Option<String> {
    workspace
        .surface_ids
        .iter()
        .position(|id| id == surface_id)
        .map(surface_ref)
}

fn record_event(
    app: &AppHandle,
    name: &str,
    category: &str,
    source: &str,
    window_id: Option<String>,
    workspace_id: Option<String>,
    pane_id: Option<String>,
    surface_id: Option<String>,
    payload: Value,
) {
    let Some(state) = app.try_state::<ControlEventState>() else {
        return;
    };
    let mut guard = state
        .inner
        .lock()
        .expect("control event log mutex poisoned");
    let seq = guard.next_seq;
    guard.next_seq = guard.next_seq.saturating_add(1);
    let boot_id = guard.boot_id.clone();
    let event = json!({
        "type": "event",
        "protocol": EVENT_STREAM_PROTOCOL,
        "version": EVENT_STREAM_VERSION,
        "boot_id": boot_id,
        "seq": seq,
        "id": format!("{boot_id}-{seq}"),
        "name": name,
        "category": category,
        "source": source,
        "occurred_at": event_timestamp(),
        "workspace_id": workspace_id,
        "surface_id": surface_id,
        "pane_id": pane_id,
        "window_id": window_id,
        "payload": payload,
    });
    let frame = serde_json::to_string(&event).ok();
    guard.events.push_back(event);
    while guard.events.len() > EVENT_REPLAY_LIMIT {
        guard.events.pop_front();
    }
    if let Some(frame) = frame {
        let event = guard.events.back().cloned().unwrap_or(Value::Null);
        fan_out_event_to_subscribers(&mut guard.subscribers, &event, &frame);
    }
    let event = guard.events.back().cloned();
    drop(guard);
    if let Some(event) = event {
        let _ = app.emit(CONTROL_EVENTS_CHANGED_EVENT, event.clone());
        append_event_to_disk(&event);
    }
}

fn append_event_to_disk(event: &Value) {
    let Some(home) = event_log_home_directory() else {
        return;
    };
    let Ok(line) = serde_json::to_string(event) else {
        return;
    };
    let dir = home.join(".cmuxterm");
    if let Err(error) = append_event_line_to_dir(&dir, &line, EVENT_LOG_MAX_BYTES) {
        eprintln!("[events] failed to append durable event log: {error}");
    }
}

fn fan_out_event_to_subscribers(
    subscribers: &mut Vec<EventSubscriber>,
    event: &Value,
    frame: &str,
) {
    subscribers.retain(|subscriber| {
        if !event_matches_filters(event, &subscriber.names, &subscriber.categories) {
            return true;
        }
        subscriber.sender.send(frame.to_string()).is_ok()
    });
}

fn event_log_home_directory() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn append_event_line_to_dir(dir: &Path, line: &str, max_bytes: u64) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let current = dir.join(EVENT_LOG_FILE_NAME);
    let archive = dir.join(EVENT_LOG_ARCHIVE_FILE_NAME);
    let additional_bytes = line.len() as u64 + 1;
    if current
        .metadata()
        .map(|metadata| metadata.len().saturating_add(additional_bytes) > max_bytes)
        .unwrap_or(false)
    {
        match fs::remove_file(&archive) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        fs::rename(&current, &archive)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(current)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

fn events_snapshot_payload(app: &AppHandle, params: &serde_json::Map<String, Value>) -> Value {
    let (ack, events, heartbeat) = events_payload_parts(app, params);
    json!({
        "ack": ack,
        "events": events,
        "heartbeat": heartbeat,
    })
}

fn events_live_stream(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlStream {
    let (ack, events, heartbeat, receiver) = events_live_stream_parts(app, params);
    let mut frames = event_stream_initial_frames(ack, events, heartbeat);
    if frames.is_empty() {
        frames.push("{}".to_string());
    }
    ControlStream::Live {
        initial_frames: frames,
        receiver,
    }
}

fn event_stream_initial_frames(ack: Value, events: Vec<Value>, heartbeat: Value) -> Vec<String> {
    let mut frames = Vec::with_capacity(events.len() + 2);
    frames.push(serde_json::to_string(&ack).unwrap_or_else(|_| "{}".to_string()));
    frames.extend(
        events
            .into_iter()
            .map(|event| serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string())),
    );
    if !heartbeat.is_null() {
        frames.push(serde_json::to_string(&heartbeat).unwrap_or_else(|_| "{}".to_string()));
    }
    frames
}

fn events_live_stream_parts(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> (
    Value,
    Vec<Value>,
    Value,
    cmux_ipc::stream_mpsc::UnboundedReceiver<String>,
) {
    let names = string_vec_param(params, &["names", "name"]).unwrap_or_default();
    let categories = string_vec_param(params, &["categories", "category"]).unwrap_or_default();
    let requested_after_seq = i64_param(params, &["after_seq", "after"])
        .unwrap_or(0)
        .max(0) as u64;
    let limit = usize_param(params, &["limit"]).unwrap_or(EVENT_REPLAY_LIMIT);
    let include_heartbeats = bool_param(params, &["include_heartbeats", "heartbeat"])
        .unwrap_or_else(|| !bool_param(params, &["no_heartbeat", "no-heartbeat"]).unwrap_or(false));
    let (sender, receiver) = cmux_ipc::stream_mpsc::unbounded_channel();
    let heartbeat_sender = include_heartbeats.then(|| sender.clone());

    let (boot_id, next_seq, retained_events) = {
        let state = app.state::<ControlEventState>();
        let mut guard = state
            .inner
            .lock()
            .expect("control event log mutex poisoned");
        let retained_events = guard.events.iter().cloned().collect::<Vec<_>>();
        guard.subscribers.push(EventSubscriber {
            sender: sender.clone(),
            names: names.clone(),
            categories: categories.clone(),
        });
        (guard.boot_id.clone(), guard.next_seq, retained_events)
    };
    let (ack, events, heartbeat) = events_parts_from_retained(
        boot_id,
        next_seq,
        retained_events,
        requested_after_seq,
        limit,
        include_heartbeats,
        names,
        categories,
    );
    if let Some(sender) = heartbeat_sender {
        spawn_event_heartbeat_task(app.clone(), sender, ack["subscription_id"].clone());
    }
    (ack, events, heartbeat, receiver)
}

fn spawn_event_heartbeat_task(
    app: AppHandle,
    sender: cmux_ipc::stream_mpsc::UnboundedSender<String>,
    subscription_id: Value,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            cmux_ipc::stream_sleep(Duration::from_secs(15)).await;
            let Some(state) = app.try_state::<ControlEventState>() else {
                break;
            };
            let (boot_id, latest_seq) = {
                let guard = state
                    .inner
                    .lock()
                    .expect("control event log mutex poisoned");
                (guard.boot_id.clone(), guard.next_seq.saturating_sub(1))
            };
            let heartbeat = json!({
                "type": "heartbeat",
                "protocol": EVENT_STREAM_PROTOCOL,
                "version": EVENT_STREAM_VERSION,
                "boot_id": boot_id,
                "subscription_id": subscription_id,
                "latest_seq": latest_seq,
                "occurred_at": event_timestamp(),
            });
            let Ok(frame) = serde_json::to_string(&heartbeat) else {
                continue;
            };
            if sender.send(frame).is_err() {
                break;
            }
        }
    });
}

fn events_payload_parts(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> (Value, Vec<Value>, Value) {
    let names = string_vec_param(params, &["names", "name"]).unwrap_or_default();
    let categories = string_vec_param(params, &["categories", "category"]).unwrap_or_default();
    let requested_after_seq = i64_param(params, &["after_seq", "after"])
        .unwrap_or(0)
        .max(0) as u64;
    let limit = usize_param(params, &["limit"]).unwrap_or(EVENT_REPLAY_LIMIT);
    let include_heartbeats = bool_param(params, &["include_heartbeats", "heartbeat"])
        .unwrap_or_else(|| !bool_param(params, &["no_heartbeat", "no-heartbeat"]).unwrap_or(false));

    let (boot_id, next_seq, retained_events) = {
        let state = app.state::<ControlEventState>();
        let guard = state
            .inner
            .lock()
            .expect("control event log mutex poisoned");
        (
            guard.boot_id.clone(),
            guard.next_seq,
            guard.events.iter().cloned().collect::<Vec<_>>(),
        )
    };
    events_parts_from_retained(
        boot_id,
        next_seq,
        retained_events,
        requested_after_seq,
        limit,
        include_heartbeats,
        names,
        categories,
    )
}

fn events_parts_from_retained(
    boot_id: String,
    next_seq: u64,
    retained_events: Vec<Value>,
    requested_after_seq: u64,
    limit: usize,
    include_heartbeats: bool,
    names: Vec<String>,
    categories: Vec<String>,
) -> (Value, Vec<Value>, Value) {
    let latest_seq = next_seq.saturating_sub(1);
    let oldest_seq = retained_events
        .first()
        .and_then(|event| event.get("seq"))
        .and_then(Value::as_u64)
        .unwrap_or(next_seq);
    let gap = (requested_after_seq > latest_seq)
        || (!retained_events.is_empty() && requested_after_seq.saturating_add(1) < oldest_seq);
    let events: Vec<Value> = retained_events
        .into_iter()
        .filter(|event| {
            event.get("seq").and_then(Value::as_u64).unwrap_or(0) > requested_after_seq
                && event_matches_filters(event, &names, &categories)
        })
        .take(limit)
        .collect();
    let ack = json!({
        "type": "ack",
        "protocol": EVENT_STREAM_PROTOCOL,
        "version": EVENT_STREAM_VERSION,
        "boot_id": boot_id,
        "subscription_id": Uuid::new_v4().to_string(),
        "heartbeat_interval_seconds": 15,
        "replay_count": events.len(),
        "resume": {
            "after_seq": requested_after_seq,
            "requested_after_seq": requested_after_seq,
            "oldest_seq": oldest_seq,
            "latest_seq": latest_seq,
            "next_seq": next_seq,
            "gap": gap,
        },
        "filters": {
            "names": names,
            "categories": categories,
        }
    });
    let heartbeat = if include_heartbeats {
        json!({
            "type": "heartbeat",
            "protocol": EVENT_STREAM_PROTOCOL,
            "version": EVENT_STREAM_VERSION,
            "boot_id": boot_id,
            "subscription_id": ack["subscription_id"].clone(),
            "latest_seq": latest_seq,
            "occurred_at": event_timestamp(),
        })
    } else {
        Value::Null
    };
    (ack, events, heartbeat)
}

fn event_matches_filters(event: &Value, names: &[String], categories: &[String]) -> bool {
    let name_matches = names.is_empty()
        || event
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| names.iter().any(|filter| filter == name));
    let category_matches = categories.is_empty()
        || event
            .get("category")
            .and_then(Value::as_str)
            .is_some_and(|category| categories.iter().any(|filter| filter == category));
    name_matches && category_matches
}

fn event_timestamp() -> String {
    OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn workspace_create(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window_index) = workspace_routed_window_index(&current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let inherited_directory = current.windows[window_index]
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| {
            current.windows[window_index]
                .tab_manager
                .workspaces
                .get(index)
        })
        .and_then(|workspace| workspace.current_directory.as_deref());
    let current_directory = match workspace_create_cwd_param(params, inherited_directory) {
        Ok(directory) => directory,
        Err(()) => return invalid_params("cwd must be a string"),
    };
    let initial_command = string_param(params, &["initial_command"]);
    let initial_environment = workspace_create_initial_env(params);
    let workspace_environment = workspace_create_workspace_env(params);
    let title = string_param(params, &["title"]);
    let description = raw_string_param(params, &["description"]);
    let group_id = string_param(params, &["group_id"]);
    if params.contains_key("group_id")
        && group_id
            .as_deref()
            .is_none_or(|group_id| Uuid::parse_str(group_id).is_err())
    {
        return invalid_params("Missing or invalid group_id");
    }
    if let Some(group_id) = group_id.as_deref() {
        let group_exists = current.windows[window_index]
            .tab_manager
            .workspace_groups
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|group| group.id == group_id);
        if !group_exists {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Group not found".to_string(),
                data: json!({"group_id": group_id}).try_into().ok(),
            };
        }
    }
    if params
        .get("layout")
        .is_some_and(|layout| !layout.is_object())
    {
        return invalid_params("layout must be a valid JSON object");
    }
    let layout = match params.get("layout") {
        Some(layout) => match serde_json::from_value::<cmux_config::CmuxLayoutNode>(layout.clone()) {
            Ok(layout) if canonical_layout_is_valid(&layout) => Some(layout),
            Ok(_) => return invalid_params("Invalid layout: every split requires exactly two children and every pane requires a surface"),
            Err(error) => return invalid_params(&format!("Invalid layout: {error}")),
        },
        None => None,
    };
    let placement = raw_string_param(params, &["group_placement", "placement"]);
    if params.contains_key("group_placement") || params.contains_key("placement") {
        if string_param(params, &["group_id"]).is_none() {
            return invalid_params("group_id is required for group placement");
        }
        let placement_value = placement.clone().unwrap_or_default();
        if !matches!(placement_value.as_str(), "afterCurrent" | "top" | "end") {
            return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: "Invalid group_placement".to_string(),
                data: json!({"group_placement": placement_value}).try_into().ok(),
            };
        }
    }
    let reference_key_present = params.contains_key("group_reference_workspace_id")
        || params.contains_key("reference_workspace_id");
    let reference_selector = raw_string_param(
        params,
        &["group_reference_workspace_id", "reference_workspace_id"],
    );
    if reference_key_present && reference_selector.is_none() {
        return invalid_params("Missing or invalid group_reference_workspace_id");
    }
    if reference_selector
        .as_deref()
        .is_some_and(|reference| Uuid::parse_str(reference).is_err())
    {
        return invalid_params("Missing or invalid group_reference_workspace_id");
    }
    let reference_index = reference_selector.as_deref().and_then(|selector| {
        resolve_workspace_identity_in_window(app, &current, window_index, selector)
    });
    if let (Some(group_id), Some(reference_selector)) =
        (group_id.as_deref(), reference_selector.as_deref())
    {
        let belongs = reference_index.is_some_and(|index| {
            current.windows[window_index].tab_manager.workspaces[index]
                .group_id
                .as_deref()
                == Some(group_id)
        });
        if !belongs {
            return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: "Reference workspace must be a member of the target group".to_string(),
                data: json!({"group_reference_workspace_id": reference_selector})
                    .try_into()
                    .ok(),
            };
        }
    }
    if placement.as_deref() == Some("afterCurrent") && reference_index.is_none() {
        return invalid_params("Missing or invalid group_reference_workspace_id");
    }
    let group_insert_index = group_id.as_deref().and_then(|group_id| {
        workspace_group_insert_index(
            &current.windows[window_index].tab_manager,
            group_id,
            placement.as_deref().unwrap_or("top"),
            reference_index,
        )
    });
    let state = app.state::<SessionState>();
    let Some((result, created_index)) = new_workspace_in_window_for_control(
        app,
        &state,
        window_index,
        current_directory.as_deref(),
        layout
            .is_none()
            .then_some(initial_command)
            .flatten()
            .as_deref(),
        layout
            .is_none()
            .then_some(initial_environment)
            .filter(|env| !env.is_empty()),
        title.as_deref(),
        description.as_deref(),
        (!workspace_environment.is_empty()).then_some(workspace_environment),
        group_id.as_deref(),
        layout,
        group_insert_index,
        false,
    ) else {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: "Failed to create workspace".to_string(),
            data: None,
        };
    };
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[created_index];
    let surface_id = surfaces_for_workspace(workspace)
        .first()
        .and_then(|surface| surface.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_deref().map(|id| control_handle_ref(app, "window", id)),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace.workspace_id.as_deref().map(|id| control_handle_ref(app, "workspace", id)),
        "group_id": workspace.group_id,
        "group_ref": workspace.group_id.as_deref().map(|id| control_handle_ref(app, "workspace_group", id)),
        "surface_id": surface_id,
        "surface_ref": surface_id.as_str().map(|id| control_handle_ref(app, "surface", id)),
    }))
}

fn record_resolved_workspace_rename_event(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) {
    let Some(event) = resolved_workspace_rename_event_spec(snapshot, window_index, workspace_index)
    else {
        return;
    };
    record_event(
        app,
        event.name,
        event.category,
        event.source,
        event.window_id,
        event.workspace_id,
        None,
        event.surface_id,
        event.payload,
    );
}

fn resolved_workspace_rename_event_spec(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) -> Option<DerivedEventSpec> {
    let Some(window) = snapshot.windows.get(window_index) else {
        return None;
    };
    let summaries = session_event_summaries(snapshot);
    let key = window
        .window_id
        .clone()
        .unwrap_or_else(|| format!("window-{window_index}"));
    let Some(current) = summaries.get(&key) else {
        return None;
    };
    let Some(workspace) = current.workspaces.get(workspace_index) else {
        return None;
    };
    Some(workspace_renamed_event_spec(
        current,
        workspace,
        &workspace.title,
    ))
}

fn workspace_create_cwd_param(
    params: &serde_json::Map<String, Value>,
    inherited: Option<&str>,
) -> Result<Option<String>, ()> {
    let working_directory = params
        .get("working_directory")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if working_directory.is_some() {
        return Ok(working_directory);
    }
    match params.get("cwd") {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(()),
        None => Ok(inherited.map(str::to_string)),
    }
}

fn workspace_create_initial_env(
    params: &serde_json::Map<String, Value>,
) -> BTreeMap<String, String> {
    raw_string_map_param(params, "initial_env")
        .into_iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            (!key.is_empty()).then(|| (key.to_string(), value))
        })
        .collect()
}

fn workspace_create_workspace_env(
    params: &serde_json::Map<String, Value>,
) -> BTreeMap<String, String> {
    raw_string_map_param(params, "workspace_env")
        .into_iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            (!key.is_empty()
                && !value.is_empty()
                && !key.contains('\0')
                && !key.contains('=')
                && !value.contains('\0'))
            .then(|| (key.to_string(), value))
        })
        .collect()
}

fn raw_string_map_param(
    params: &serde_json::Map<String, Value>,
    key: &str,
) -> BTreeMap<String, String> {
    params
        .get(key)
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect()
}

fn canonical_layout_is_valid(layout: &cmux_config::CmuxLayoutNode) -> bool {
    match layout {
        cmux_config::CmuxLayoutNode::Pane(pane) => !pane.surfaces.is_empty(),
        cmux_config::CmuxLayoutNode::Split(split) => {
            split.children.len() == 2 && split.children.iter().all(canonical_layout_is_valid)
        }
    }
}

fn resolve_workspace_identity_in_window(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    selector: &str,
) -> Option<usize> {
    let id = if one_based_ref_index(selector, "workspace").is_some() {
        resolve_control_handle_ref(app, "workspace", selector)?
    } else {
        selector.to_string()
    };
    snapshot
        .windows
        .get(window_index)?
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(id.as_str()))
}

fn workspace_group_insert_index(
    tabs: &cmux_core::session::SessionTabManagerSnapshot,
    group_id: &str,
    placement: &str,
    reference_index: Option<usize>,
) -> Option<usize> {
    let members: Vec<usize> = tabs
        .workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| {
            (workspace.group_id.as_deref() == Some(group_id)).then_some(index)
        })
        .collect();
    match placement {
        "top" => members.first().copied(),
        "end" => members.last().map(|index| index + 1),
        "afterCurrent" => reference_index.map(|index| index + 1),
        _ => None,
    }
}

fn workspace_create_browser(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let url = raw_string_param(params, &["url"]);
    let state = app.state::<SessionState>();
    workspace_current(&new_browser_workspace_for_control(
        app,
        &state,
        url.as_deref(),
    ))
}

fn config_reload(app: &AppHandle) -> ControlCallResult {
    match crate::config::reload_config_for_control(app) {
        Ok(payload) => ok(json!({
            "path": payload.path,
            "reloaded": true,
        })),
        Err(message) => ControlCallResult::Err {
            code: "config_reload_failed".to_string(),
            message,
            data: None,
        },
    }
}

fn window_list(app: &AppHandle) -> ControlCallResult {
    let session = snapshot(app);
    let windows = crate::window::control_window_summaries(app);
    ok(json!({"windows": windows
        .into_iter()
        .enumerate()
        .map(|(index, window)| {
            let session_window = session.windows.iter().find(|session_window| {
                session_window.window_id.as_deref() == Some(window.identity.label.as_str())
            }).or_else(|| {
                if window.identity.label == "main" {
                    session.windows.first()
                } else {
                    None
                }
            });
            let tab_manager = session_window.map(|window| &window.tab_manager);
            let selected_index = tab_manager
                .and_then(|tab_manager| tab_manager.selected_workspace_index)
                .unwrap_or_default()
                .max(0) as usize;
            let selected_workspace_id = tab_manager
                .and_then(|tab_manager| tab_manager.workspaces.get(selected_index))
                .and_then(|workspace| workspace.workspace_id.clone());
            let window_reference = control_handle_ref(app, "window", &window.identity.label);
            let selected_workspace_reference = selected_workspace_id
                .as_deref()
                .map(|id| control_handle_ref(app, "workspace", id));
            json!({
                "index": index,
                "id": window.identity.id,
                "ref": window_reference,
                "key": window.is_key,
                "visible": window.is_visible,
                "workspace_count": tab_manager.map_or(0, |tab_manager| tab_manager.workspaces.len()),
                "selected_workspace_id": selected_workspace_id,
                "selected_workspace_ref": selected_workspace_reference,
            })
        })
        .collect::<Vec<_>>() }))
}

fn window_current(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let selector = raw_string_param(params, &["window_id", "window_ref"]);
    let Some(window) = crate::window::current_control_window(app, selector.as_deref()) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Current window not found".to_string(),
            data: None,
        };
    };
    ok(json!({
        "window_id": window.id,
        "window_ref": window.reference,
    }))
}

fn window_displays(app: &AppHandle) -> ControlCallResult {
    match crate::window::available_displays(app) {
        Ok(displays) => ok(json!({
            "displays": displays.into_iter().map(|display| json!({
                "name": display.name,
                "index": display.index,
                "display_id": Value::Null,
                "main": display.is_main,
                "frame": {
                    "x": display.x,
                    "y": display.y,
                    "width": display.width,
                    "height": display.height,
                },
            })).collect::<Vec<_>>(),
        })),
        Err(message) => ControlCallResult::Err {
            code: "window_displays_failed".into(),
            message,
            data: None,
        },
    }
}

fn window_display(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(display) = raw_string_param(params, &["display"]) else {
        return invalid_params("Missing or invalid display");
    };
    let selector = raw_string_param(params, &["window_id", "window_ref"]);
    match crate::window::move_control_windows_to_display(app, &display, selector.as_deref()) {
        Ok(result) => {
            let mut payload = serde_json::Map::from_iter([
                ("display".into(), json!(result.display)),
                (
                    "moved".into(),
                    json!(result
                        .moved
                        .iter()
                        .map(|identity| identity.id.clone())
                        .collect::<Vec<_>>()),
                ),
            ]);
            if selector.is_some() {
                if let Some(identity) = result.moved.first() {
                    payload.insert("window_id".into(), json!(identity.id));
                    payload.insert("window_ref".into(), json!(identity.reference));
                }
            }
            ok(Value::Object(payload))
        }
        Err(crate::window::WindowDisplayMoveError::WindowNotFound(selector)) => {
            let data = if selector.starts_with("window:") {
                json!({"window_ref": selector})
            } else {
                json!({"window_id": selector})
            };
            ControlCallResult::Err {
                code: "not_found".into(),
                message: "Window not found".into(),
                data: JsonValue::try_from(data).ok(),
            }
        }
        Err(crate::window::WindowDisplayMoveError::DisplayNotFound {
            requested,
            available,
        }) => ControlCallResult::Err {
            code: "not_found".into(),
            message: format!("Display not found: {requested}"),
            data: JsonValue::try_from(json!({
                "requested": requested,
                "available": available,
            }))
            .ok(),
        },
        Err(crate::window::WindowDisplayMoveError::Internal(message)) => ControlCallResult::Err {
            code: "window_display_failed".into(),
            message,
            data: None,
        },
    }
}

fn right_sidebar_control(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if !right_sidebar_target_exists(&current, params) {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Right sidebar target not found".to_string(),
            data: None,
        };
    }

    let Some(action) = string_param(params, &["action"]) else {
        return invalid_params("right_sidebar requires action");
    };
    let mode = string_param(params, &["mode"]);
    let focus = bool_param(params, &["focus"]).unwrap_or(true);
    let state = app.state::<crate::right_sidebar::RightSidebarState>();
    match state.apply_control(&action, mode.as_deref(), focus) {
        Ok(crate::right_sidebar::RightSidebarControlOutcome::State(snapshot)) => {
            ok(json!(snapshot))
        }
        Ok(crate::right_sidebar::RightSidebarControlOutcome::Changed(change)) => {
            if let Err(error) = app.emit(
                crate::right_sidebar::RIGHT_SIDEBAR_CHANGED_EVENT,
                change.clone(),
            ) {
                return ControlCallResult::Err {
                    code: "right_sidebar_unavailable".to_string(),
                    message: error.to_string(),
                    data: None,
                };
            }
            ok(json!({"ok": true}))
        }
        Err(message) => invalid_params(&message),
    }
}

fn right_sidebar_target_exists(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> bool {
    let has_workspace_target =
        params.contains_key("workspace_ref") || params.contains_key("workspace_id");
    if has_workspace_target && workspace_index_from_params(snapshot, params).is_none() {
        return false;
    }

    let has_window_target = params.contains_key("window_ref") || params.contains_key("window_id");
    if !has_window_target {
        return true;
    }
    let Some(window) = snapshot.windows.first() else {
        return false;
    };
    if let Some(reference) = string_param(params, &["window_ref"]) {
        return one_based_ref_index(&reference, "window") == Some(0);
    }
    string_param(params, &["window_id"])
        .is_some_and(|id| window.window_id.as_deref() == Some(id.as_str()))
}

fn feed_push(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    has_request_id: bool,
) -> ControlCallResult {
    let Some(event_value) = params.get("event") else {
        return invalid_params("feed.push requires an event object");
    };
    let event: cmux_agent::WorkstreamEvent = match serde_json::from_value(event_value.clone()) {
        Ok(event) => event,
        Err(error) => return invalid_params(&format!("feed.push event failed to decode: {error}")),
    };
    let wait_timeout = match params.get("wait_timeout_seconds") {
        None => 0.0,
        Some(value) => match value.as_f64() {
            Some(value) => value,
            None => return invalid_params("feed.push wait_timeout_seconds must be numeric"),
        },
    };
    if !(0.0..=120.0).contains(&wait_timeout) {
        return invalid_params("feed.push wait_timeout_seconds must be between 0 and 120");
    }
    if !has_request_id && wait_timeout > 0.0 {
        return invalid_params("feed.push without an id requires wait_timeout_seconds 0");
    }
    let state = app.state::<crate::feed::FeedState>();
    let insert = match state.ingest(event) {
        Ok(insert) => insert,
        Err(message) => {
            return ControlCallResult::Err {
                code: "feed_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    if let Ok(reply) = state.list() {
        let _ = app.emit(crate::feed::FEED_CHANGED_EVENT, &reply);
    }
    match state.wait_for_decision(&insert, Duration::from_secs_f64(wait_timeout)) {
        Ok(reply) => {
            if reply.status == "timed_out" {
                if let Ok(list) = state.list() {
                    let _ = app.emit(crate::feed::FEED_CHANGED_EVENT, &list);
                }
            }
            ok(json!(reply))
        }
        Err(message) => ControlCallResult::Err {
            code: "feed_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn feed_list(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<crate::feed::FeedState>();
    match state.list() {
        Ok(reply) => ok(json!(reply)),
        Err(message) => ControlCallResult::Err {
            code: "feed_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn feed_permission_reply(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(request_id) = string_param(params, &["request_id"]) else {
        return invalid_params("feed.permission.reply requires request_id");
    };
    let Some(mode) = string_param(params, &["mode"]) else {
        return invalid_params("feed.permission.reply requires mode");
    };
    let decision = match crate::feed::permission_decision(&mode) {
        Ok(decision) => decision,
        Err(message) => return invalid_params(&format!("feed.permission.reply {message}")),
    };
    feed_resolve_control(app, &request_id, decision)
}

fn feed_question_reply(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(request_id) = string_param(params, &["request_id"]) else {
        return invalid_params("feed.question.reply requires request_id");
    };
    let Some(selections) = string_vec_param(params, &["selections"]) else {
        return invalid_params("feed.question.reply requires selections: [string]");
    };
    feed_resolve_control(app, &request_id, crate::feed::question_decision(selections))
}

fn feed_exit_plan_reply(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(request_id) = string_param(params, &["request_id"]) else {
        return invalid_params("feed.exit_plan.reply requires request_id");
    };
    let Some(mode) = string_param(params, &["mode"]) else {
        return invalid_params("feed.exit_plan.reply requires mode");
    };
    let decision = match crate::feed::exit_plan_decision(&mode, string_param(params, &["feedback"]))
    {
        Ok(decision) => decision,
        Err(message) => return invalid_params(&format!("feed.exit_plan.reply {message}")),
    };
    feed_resolve_control(app, &request_id, decision)
}

fn feed_resolve_control(
    app: &AppHandle,
    request_id: &str,
    decision: cmux_agent::WorkstreamDecision,
) -> ControlCallResult {
    let state = app.state::<crate::feed::FeedState>();
    match state.resolve(request_id, decision) {
        Ok(reply) => {
            let _ = app.emit(crate::feed::FEED_CHANGED_EVENT, &reply);
            ok(json!({"status": "resolved", "request_id": request_id}))
        }
        Err(message) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message,
            data: None,
        },
    }
}

fn notification_list(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<crate::notifications::NotificationCommandState>();
    match crate::notifications::notification_list_for_control(state.inner()) {
        Ok(reply) => ok(json!(reply
            .notifications
            .into_iter()
            .map(|item| json!({
                "id": item.id,
                "workspace_id": item.workspace_id,
                "surface_id": item.surface_id,
                "panel_id": item.panel_id,
                "is_read": item.is_read,
                "title": item.title,
                "subtitle": item.subtitle,
                "body": item.body,
                "created_at": item.created_at,
                "tab_title": Value::Null,
            }))
            .collect::<Vec<_>>())),
        Err(message) => ControlCallResult::Err {
            code: "notification_store_failed".to_string(),
            message,
            data: None,
        },
    }
}

fn notification_mutation_result(
    result: Result<crate::notifications::NotificationCenterReply, String>,
) -> ControlCallResult {
    match result {
        Ok(reply) => ok(json!({
            "remaining_count": reply.total_count,
            "unread_count": reply.unread_count,
        })),
        Err(message) => ControlCallResult::Err {
            code: "notification_store_failed".to_string(),
            message,
            data: None,
        },
    }
}

fn notification_dismiss(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let id = string_param(params, &["id"]);
    let all_read = bool_param(params, &["all_read"]).unwrap_or(false);
    if id.is_some() == all_read {
        return invalid_params("notification.dismiss requires exactly one of id or all_read");
    }
    let state = app.state::<crate::notifications::NotificationCommandState>();
    notification_mutation_result(crate::notifications::notification_dismiss_for_control(
        state.inner(),
        id.as_deref(),
        all_read,
    ))
}

fn notification_mark_read(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let id = string_param(params, &["id"]);
    let all = bool_param(params, &["all"]).unwrap_or(false);
    let current = snapshot(app);
    let workspace_index = workspace_index_from_params(&current, params);
    if usize::from(id.is_some()) + usize::from(workspace_index.is_some()) + usize::from(all) != 1 {
        return invalid_params("notification.mark_read requires exactly one selector");
    }
    let workspace_id = workspace_index.and_then(|index| {
        current.windows[0].tab_manager.workspaces[index]
            .workspace_id
            .clone()
    });
    let has_surface = ["surface_ref", "surface_id", "panel_id"]
        .iter()
        .any(|key| params.contains_key(*key));
    let surface_id = if has_surface {
        let Some(index) = workspace_index else {
            return invalid_params("surface selector requires workspace selector");
        };
        match surface_id_from_params_or_workspace_focused(&current, index, params) {
            Some(id) => Some(id),
            None => return invalid_params("Missing or invalid surface selector"),
        }
    } else {
        None
    };
    let state = app.state::<crate::notifications::NotificationCommandState>();
    notification_mutation_result(crate::notifications::notification_mark_read_for_control(
        state.inner(),
        id.as_deref(),
        workspace_id.as_deref(),
        surface_id.as_deref(),
        all,
    ))
}

fn notification_clear(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let workspace_id =
        if params.contains_key("workspace_id") || params.contains_key("workspace_ref") {
            let Some(index) = workspace_index_from_params(&current, params) else {
                return invalid_params("Missing or invalid workspace selector");
            };
            current.windows[0].tab_manager.workspaces[index]
                .workspace_id
                .clone()
        } else {
            None
        };
    let state = app.state::<crate::notifications::NotificationCommandState>();
    notification_mutation_result(crate::notifications::notification_clear_for_control(
        state.inner(),
        workspace_id.as_deref(),
    ))
}

fn notification_create(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace_id) = current.windows[0].tab_manager.workspaces[workspace_index]
        .workspace_id
        .clone()
    else {
        return invalid_params("Workspace has no stable id");
    };
    let Some(surface_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let title = raw_string_param(params, &["title"]).unwrap_or_else(|| "Notification".to_string());
    let subtitle = raw_string_param(params, &["subtitle"]).unwrap_or_default();
    let body = raw_string_param(params, &["body"]).unwrap_or_default();
    let state = app.state::<crate::notifications::NotificationCommandState>();
    match crate::notifications::notification_create_for_control(
        state.inner(),
        workspace_id.clone(),
        surface_id.clone(),
        title,
        subtitle,
        body,
    ) {
        Ok(notification) => ok(json!({
            "id": notification.id,
            "workspace_id": workspace_id,
            "workspace_ref": workspace_ref(workspace_index),
            "surface_id": surface_id,
            "surface_ref": surface_ref_for_panel(&current, workspace_index, &surface_id),
            "title": notification.title,
            "subtitle": notification.subtitle,
            "body": notification.body,
            "created_at": notification.created_at,
            "is_read": false,
            "delivered": true,
        })),
        Err(message) => ControlCallResult::Err {
            code: "notification_delivery_failed".to_string(),
            message,
            data: None,
        },
    }
}

fn notification_open(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(id) = string_param(params, &["id"]) else {
        return invalid_params("notification.open requires id");
    };
    notification_open_selected(app, Some(&id), false)
}

fn notification_jump_to_unread(app: &AppHandle) -> ControlCallResult {
    notification_open_selected(app, None, true)
}

fn notification_open_selected(
    app: &AppHandle,
    id: Option<&str>,
    allow_empty: bool,
) -> ControlCallResult {
    let notification_state = app.state::<crate::notifications::NotificationCommandState>();
    let notification = match crate::notifications::notification_open_target_for_control(
        notification_state.inner(),
        id,
    ) {
        Ok(Some(notification)) => notification,
        Ok(None) if allow_empty => return ok(json!({"opened": false})),
        Ok(None) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Notification not found".to_string(),
                data: id.map(|id| json!({"id": id}).try_into().unwrap_or(JsonValue::Null)),
            };
        }
        Err(message) => {
            return ControlCallResult::Err {
                code: "notification_store_failed".to_string(),
                message,
                data: None,
            };
        }
    };
    let session_state = app.state::<SessionState>();
    let target_surface = notification
        .surface_id
        .clone()
        .or(notification.panel_id.clone());
    let opened = if let Some(surface_id) = target_surface.as_deref() {
        let (changed, snapshot) = match crate::session::select_workspace_surface(
            app,
            &session_state,
            &notification.tab_id,
            surface_id,
        ) {
            Ok(result) => result,
            Err(PaneTopologyControlError::Operation(error)) => match error {},
            Err(PaneTopologyControlError::Publication(message)) => {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
        };
        changed
            || crate::session::workspace_surface_is_selected(
                &snapshot,
                &notification.tab_id,
                surface_id,
            )
    } else {
        let current = snapshot(app);
        match workspace_index_for_id(&current, &notification.tab_id) {
            Some(index) => match select_workspace_for_control(app, &session_state, index as i64) {
                Ok(_) => true,
                Err(message) => {
                    return ControlCallResult::Err {
                        code: "internal".to_string(),
                        message,
                        data: None,
                    };
                }
            },
            None => false,
        }
    };
    if !opened {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Notification target not found".to_string(),
            data: Some(
                json!({"id": notification.id})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }
    let current = snapshot(app);
    let workspace_index = workspace_index_for_id(&current, &notification.tab_id);
    let workspace_ref_value = workspace_index.map(workspace_ref);
    let surface_ref_value = workspace_index.and_then(|index| {
        target_surface
            .as_deref()
            .and_then(|surface| surface_ref_for_panel(&current, index, surface))
    });
    ok(json!({
        "id": notification.id,
        "workspace_id": notification.tab_id,
        "workspace_ref": workspace_ref_value,
        "surface_id": target_surface,
        "surface_ref": surface_ref_value,
        "title": notification.title,
        "subtitle": notification.subtitle,
        "body": notification.body,
        "created_at": notification.created_at,
        "is_read": true,
        "opened": true,
    }))
}

fn session_restore_previous_launch(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<SessionState>();
    workspace_current(&restore_previous_launch_for_control(app, &state))
}

fn workspace_close(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window_index) = workspace_routed_window_index(&current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    if raw_string_param(params, &["workspace_id"])
        .as_deref()
        .is_none_or(|workspace_id| Uuid::parse_str(workspace_id).is_err())
    {
        return invalid_params("Missing or invalid workspace_id");
    }
    let Some(index) = canonical_workspace_target_index(&current, window_index, params) else {
        return workspace_not_found(app, params);
    };
    let window = &current.windows[window_index];
    let workspace = &window.tab_manager.workspaces[index];
    let workspace_id = workspace.workspace_id.clone().unwrap_or_default();
    let identity = workspace_identity_payload(app, window, &workspace_id);
    if workspace.is_pinned == Some(true) {
        let mut data = identity;
        data["pinned"] = json!(true);
        return ControlCallResult::Err {
            code: "protected".to_string(),
            message: "Pinned workspaces can't be closed while pinned. Unpin the workspace first."
                .to_string(),
            data: data.try_into().ok(),
        };
    }
    let state = app.state::<SessionState>();
    let Some((_snapshot, _changed)) =
        close_workspace_in_window_for_control(app, &state, window_index, index)
    else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    ok(identity)
}

fn workspace_reopen_closed(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<SessionState>();
    let Some(snapshot) = reopen_closed_workspace_for_control(app, &state) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No recently closed workspace".to_string(),
            data: None,
        };
    };
    workspace_current(&snapshot)
}

fn workspace_close_many(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(indices) = workspace_indices_from_params(&current, params) else {
        return invalid_params("Missing or invalid workspace selectors");
    };
    let state = app.state::<SessionState>();
    workspace_current(&close_workspaces_for_control(app, &state, &indices))
}

fn workspace_rename(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window_index) = workspace_routed_window_index(&current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    if raw_string_param(params, &["workspace_id"])
        .as_deref()
        .is_none_or(|workspace_id| Uuid::parse_str(workspace_id).is_err())
    {
        return invalid_params("Missing or invalid workspace_id");
    }
    let Some(title) = string_param(params, &["title"]) else {
        return invalid_params("Missing or invalid title");
    };
    let Some(index) = canonical_workspace_target_index(&current, window_index, params) else {
        return workspace_not_found(app, params);
    };
    let workspace_id = current.windows[window_index].tab_manager.workspaces[index]
        .workspace_id
        .clone()
        .unwrap_or_default();
    let state = app.state::<SessionState>();
    let rename_result =
        rename_workspace_in_window_for_control(app, &state, window_index, index, &title);
    let Some((result, resolution)) = (match rename_result {
        Ok(result) => result,
        Err(message) => {
            return ControlCallResult::Err {
                code: "remote_rename_failed".to_string(),
                message,
                data: None,
            }
        }
    }) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    if resolution == WorkspaceRenameResolution::ResolvedUnchanged {
        record_resolved_workspace_rename_event(app, &result, window_index, index);
    }
    let mut payload = workspace_identity_payload(app, &result.windows[window_index], &workspace_id);
    payload["title"] = json!(title);
    ok(payload)
}

fn workspace_select(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window_index) = workspace_routed_window_index(&current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    if raw_string_param(params, &["workspace_id"])
        .as_deref()
        .is_none_or(|workspace_id| Uuid::parse_str(workspace_id).is_err())
    {
        return invalid_params("Missing or invalid workspace_id");
    }
    let Some(index) = canonical_workspace_target_index(&current, window_index, params) else {
        return workspace_not_found(app, params);
    };
    let workspace_id = current.windows[window_index].tab_manager.workspaces[index]
        .workspace_id
        .clone()
        .unwrap_or_default();
    let state = app.state::<SessionState>();
    let result = match select_workspace_in_window_for_control(app, &state, window_index, index) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(WorkspaceSelectControlError::WindowNotFound)) => {
            return ControlCallResult::Err {
                code: "unavailable".to_string(),
                message: "TabManager not available".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    if let Some(window_id) = workspace_select_focus_selector(&result, window_index) {
        let _ = crate::window::focus_control_window(app, window_id);
    }
    ok(workspace_identity_payload(
        app,
        &result.windows[window_index],
        &workspace_id,
    ))
}

fn workspace_select_focus_selector(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
) -> Option<&str> {
    snapshot.windows.get(window_index)?.window_id.as_deref()
}

fn workspace_not_found(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let workspace_id = raw_string_param(params, &["workspace_id"]).unwrap_or_default();
    let workspace_reference = if one_based_ref_index(&workspace_id, "workspace").is_some() {
        workspace_id.clone()
    } else {
        control_handle_ref(app, "workspace", &workspace_id)
    };
    workspace_not_found_with_ref(&workspace_id, &workspace_reference)
}

fn workspace_not_found_with_ref(
    workspace_id: &str,
    workspace_reference: &str,
) -> ControlCallResult {
    ControlCallResult::Err {
        code: "not_found".to_string(),
        message: "Workspace not found".to_string(),
        data: json!({
            "workspace_id": workspace_id,
            "workspace_ref": workspace_reference,
        })
        .try_into()
        .ok(),
    }
}

fn workspace_identity_payload(
    app: &AppHandle,
    window: &cmux_core::session::SessionWindowSnapshot,
    workspace_id: &str,
) -> Value {
    json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_deref().map(|id| control_handle_ref(app, "window", id)),
        "workspace_id": workspace_id,
        "workspace_ref": control_handle_ref(app, "workspace", workspace_id),
    })
}

fn workspace_id_for_window_move(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<String> {
    if let Some(reference) = string_param(params, &["workspace_ref"]) {
        let index = one_based_ref_index(&reference, "workspace")?;
        return snapshot
            .windows
            .first()?
            .tab_manager
            .workspaces
            .get(index)?
            .workspace_id
            .clone();
    }
    let workspace_id = string_param(params, &["workspace_id"])?;
    snapshot
        .windows
        .iter()
        .flat_map(|window| &window.tab_manager.workspaces)
        .any(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
        .then_some(workspace_id)
}

fn workspace_move_to_window(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if !params.contains_key("workspace_ref") && !params.contains_key("workspace_id") {
        return invalid_params("Missing or invalid workspace_id");
    }
    let Some(workspace_id) = workspace_id_for_window_move(&current, params) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: string_param(params, &["workspace_id"])
                .and_then(|workspace_id| json!({"workspace_id": workspace_id}).try_into().ok()),
        };
    };
    let Some(window_selector) = raw_string_param(params, &["window_ref", "window_id"]) else {
        return invalid_params("Missing or invalid window_id");
    };
    let Some(window_identity) =
        crate::window::current_control_window(app, Some(window_selector.as_str()))
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Window not found".to_string(),
            data: Some(
                json!({"window_id": window_selector})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    };
    let state = app.state::<SessionState>();
    register_window_for_control(app, &state, &window_identity.label);
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let result = match move_workspace_to_window_for_control(
        app,
        &state,
        &workspace_id,
        &window_identity.label,
        focus,
    ) {
        Ok(result) => result,
        Err(session_ops::MoveWorkspaceToWindowError::WorkspaceNotFound) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Workspace not found".to_string(),
                data: Some(
                    json!({"workspace_id": workspace_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        Err(session_ops::MoveWorkspaceToWindowError::WindowNotFound) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Window not found".to_string(),
                data: Some(
                    json!({"window_id": window_identity.id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    if focus {
        if let Some(window) = app.get_webview_window(&window_identity.label) {
            let _ = window.set_focus();
        }
    }
    let Some(target_window) = result
        .windows
        .iter()
        .find(|window| window.window_id.as_deref() == Some(window_identity.label.as_str()))
    else {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: "Failed to move workspace".to_string(),
            data: None,
        };
    };
    let workspace_ref_value = target_window
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
        .map(workspace_ref);
    ok(json!({
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref_value,
        "window_id": window_identity.id,
        "window_ref": window_identity.reference,
    }))
}

fn workspace_reorder(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if !workspace_reorder_window_matches(&current, params) {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    }
    let Some(index) = workspace_index_from_params(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace_id) = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(index))
        .and_then(|workspace| workspace.workspace_id.clone())
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(requested_index) = workspace_reorder_destination_index(&current, params, index) else {
        return invalid_params(
            "Specify exactly one target: index, before_workspace_id, or after_workspace_id",
        );
    };
    let uses_top_level_rows =
        bool_param(params, &["uses_top_level_rows", "top_level_rows"]).unwrap_or(false);
    let mut planned = current.clone();
    if let Some(window) = planned.windows.first_mut() {
        session_ops::reorder_workspaces_with_mode(
            &mut window.tab_manager,
            index as i64,
            requested_index,
            uses_top_level_rows,
        );
    }
    let Some(to_index) = workspace_index_for_id(&planned, &workspace_id) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let dry_run = bool_param(params, &["dry_run"]).unwrap_or(false);
    let result = if dry_run {
        planned
    } else {
        let state = app.state::<SessionState>();
        match reorder_workspaces_for_control(
            app,
            &state,
            index as i64,
            requested_index,
            uses_top_level_rows,
        ) {
            Ok(result) => result,
            Err(message) => {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
        }
    };
    let window = result.windows.first();
    let window_id = window.and_then(|window| window.window_id.clone());
    let window_ref = window_id.as_ref().map(|_| "window:1");
    let workspace_ref = workspace_ref(to_index);
    let plan = json!({
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref,
        "window_id": window_id,
        "window_ref": window_ref,
        "from_index": index,
        "to_index": to_index,
    });
    let events = if !dry_run && index != to_index {
        vec![plan.clone()]
    } else {
        Vec::new()
    };
    ok(json!({
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref,
        "window_id": window_id,
        "window_ref": window_ref,
        "from_index": index,
        "to_index": to_index,
        "index": to_index,
        "dry_run": dry_run,
        "plan": [plan],
        "events": events,
    }))
}

fn workspace_reorder_many(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if !workspace_reorder_window_matches(&current, params) {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    }
    let ordered_workspace_ids = match workspace_reorder_many_order(&current, params) {
        Ok(ids) => ids,
        Err(WorkspaceReorderManyOrderError::Missing) => {
            return invalid_params("Missing workspace_ids");
        }
        Err(WorkspaceReorderManyOrderError::Invalid(workspace)) => {
            return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: "Invalid workspace id or ref".to_string(),
                data: Some(
                    json!({"workspace": workspace})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    let dry_run = bool_param(params, &["dry_run"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    let (plan, result) =
        match reorder_workspaces_many_for_control(app, &state, &ordered_workspace_ids, dry_run) {
            Ok(result) => result,
            Err(PaneTopologyControlError::Operation(
                ReorderWorkspacesManyControlError::Unavailable,
            )) => {
                return ControlCallResult::Err {
                    code: "unavailable".to_string(),
                    message: "TabManager not available".to_string(),
                    data: None,
                };
            }
            Err(PaneTopologyControlError::Operation(ReorderWorkspacesManyControlError::Batch(
                WorkspaceBatchReorderError::DuplicateWorkspace(workspace_id),
            ))) => {
                return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: "Duplicate workspace in order".to_string(),
                data: Some(
                    json!({
                        "workspace_id": workspace_id,
                        "workspace_ref": workspace_index_for_id(&current, &workspace_id.to_string())
                            .map(workspace_ref),
                    })
                    .try_into()
                    .unwrap_or(JsonValue::Null),
                ),
            };
            }
            Err(PaneTopologyControlError::Operation(ReorderWorkspacesManyControlError::Batch(
                WorkspaceBatchReorderError::WorkspaceNotFound(workspace_id),
            ))) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "Workspace not found".to_string(),
                    data: Some(
                        json!({
                            "workspace_id": workspace_id,
                            "workspace_ref": Value::Null,
                        })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                    ),
                };
            }
            Err(PaneTopologyControlError::Publication(message)) => {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
        };
    let plan_payloads: Vec<Value> = plan
        .iter()
        .map(|item| workspace_reorder_plan_payload(&result, item))
        .collect();
    let events: Vec<Value> = if dry_run {
        Vec::new()
    } else {
        plan.iter()
            .zip(&plan_payloads)
            .filter_map(|(item, payload)| {
                (item.from_index != item.to_index).then(|| payload.clone())
            })
            .collect()
    };
    let window_id = result
        .windows
        .first()
        .and_then(|window| window.window_id.clone());
    ok(json!({
        "window_id": window_id,
        "window_ref": window_id.as_ref().map(|_| "window:1"),
        "dry_run": dry_run,
        "plan": plan_payloads,
        "events": events,
    }))
}

fn workspace_reorder_plan_payload(
    snapshot: &AppSessionSnapshot,
    item: &WorkspaceReorderPlanItem,
) -> Value {
    let workspace_id = item.workspace_id.to_string();
    let workspace_ref_value = workspace_index_for_id(snapshot, &workspace_id).map(workspace_ref);
    let window_id = snapshot
        .windows
        .first()
        .and_then(|window| window.window_id.clone());
    json!({
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref_value,
        "window_id": window_id,
        "window_ref": window_id.as_ref().map(|_| "window:1"),
        "from_index": item.from_index,
        "to_index": item.to_index,
    })
}

#[derive(Debug)]
enum WorkspaceReorderManyOrderError {
    Missing,
    Invalid(String),
}

fn workspace_reorder_many_order(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<Vec<Uuid>, WorkspaceReorderManyOrderError> {
    let values: Vec<&str> = if let Some(raw) = params.get("workspace_ids") {
        match raw {
            Value::Array(values) => values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| WorkspaceReorderManyOrderError::Invalid(value.to_string()))
                })
                .collect::<Result<_, _>>()?,
            Value::String(value) => vec![value],
            value => {
                return Err(WorkspaceReorderManyOrderError::Invalid(value.to_string()));
            }
        }
    } else if let Some(raw) = params.get("order") {
        match raw {
            Value::String(value) => value.split(',').collect(),
            value => {
                return Err(WorkspaceReorderManyOrderError::Invalid(value.to_string()));
            }
        }
    } else {
        return Err(WorkspaceReorderManyOrderError::Missing);
    };
    if values.is_empty() {
        return Err(WorkspaceReorderManyOrderError::Missing);
    }

    values
        .into_iter()
        .map(|raw| {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(WorkspaceReorderManyOrderError::Invalid(raw.to_string()));
            }
            let workspace_id = if let Some(index) = one_based_ref_index(raw, "workspace") {
                snapshot
                    .windows
                    .first()
                    .and_then(|window| window.tab_manager.workspaces.get(index))
                    .and_then(|workspace| workspace.workspace_id.as_deref())
                    .ok_or_else(|| WorkspaceReorderManyOrderError::Invalid(raw.to_string()))?
            } else if workspace_index_for_id(snapshot, raw).is_some()
                || Uuid::parse_str(raw).is_ok()
            {
                raw
            } else {
                return Err(WorkspaceReorderManyOrderError::Invalid(raw.to_string()));
            };
            Uuid::parse_str(workspace_id)
                .map_err(|_| WorkspaceReorderManyOrderError::Invalid(raw.to_string()))
        })
        .collect()
}

fn workspace_select_relative(app: &AppHandle, delta: i64) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window) = current.windows.first() else {
        return workspace_current(&current);
    };
    let count = window.tab_manager.workspaces.len();
    if count == 0 {
        return workspace_current(&current);
    }
    let selected = selected_workspace_index(&current).min(count - 1);
    let next = (selected as i64 + delta).rem_euclid(count as i64);
    let state = app.state::<SessionState>();
    match select_workspace_for_control(app, &state, next) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_last(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or_else(|| {
        crate::window::current_control_window(app, None)
            .and_then(|identity| {
                current
                    .windows
                    .iter()
                    .position(|window| window.window_id.as_deref() == Some(identity.label.as_str()))
            })
            .unwrap_or(0)
    });
    let state = app.state::<SessionState>();
    let (workspace_id, result) = match select_last_workspace_for_control(app, &state, window_index)
    {
        Ok(result) => result,
        Err(WorkspaceLastControlError::TabManagerUnavailable) => {
            return ControlCallResult::Err {
                code: "unavailable".to_string(),
                message: "TabManager not available".to_string(),
                data: None,
            };
        }
        Err(WorkspaceLastControlError::NoPreviousWorkspace) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "No previous workspace in history".to_string(),
                data: None,
            };
        }
    };
    let window = &result.windows[window_index];
    let window_label = window.window_id.as_deref().unwrap_or("main");
    let identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window_label)
        .map(|summary| summary.identity);
    if let Some(window) = app.get_webview_window(window_label) {
        let _ = window.set_focus();
    }
    let workspace_index = window
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
        .unwrap_or(0);
    ok(json!({
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "window_id": identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": identity.as_ref().map(|identity| identity.reference.clone()),
    }))
}

fn workspace_equalize_splits(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<SessionState>();
    match equalize_dividers_for_control(app, &state) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_set_description(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(description) = raw_string_param(params, &["description", "text", "body"]) else {
        return invalid_params("Missing workspace description");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match set_workspace_description_for_control(app, &state, index as i64, &description) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_reset_color(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match reset_workspace_color_for_control(app, &state, index as i64) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_set_progress(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(value) = f64_param(params, &["value", "progress"]) else {
        return invalid_params("Missing or invalid workspace progress value");
    };
    if !value.is_finite() {
        return invalid_params("Workspace progress value must be finite");
    }
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let label = raw_string_param(params, &["label", "text"]);
    let state = app.state::<SessionState>();
    match set_workspace_sidebar_progress_for_control(
        app,
        &state,
        index as i64,
        value,
        label.as_deref(),
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_clear_progress(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_progress_for_control(app, &state, index as i64) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_set_status(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar status key");
    };
    let Some(value) = raw_string_param(params, &["value", "status", "text"]) else {
        return invalid_params("Missing sidebar status value");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let priority = i64_param(params, &["priority"]);
    let state = app.state::<SessionState>();
    match set_workspace_sidebar_status_for_control(
        app,
        &state,
        index as i64,
        &key,
        &value,
        priority,
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_clear_status(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar status key");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_status_for_control(app, &state, index as i64, &key) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_list_status(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace) = workspace_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    ok(json!({
        "status_entries": workspace.sidebar_status_entries.clone().unwrap_or_default(),
        "status_count": workspace.sidebar_status_entries.as_ref().map_or(0, Vec::len),
    }))
}

fn workspace_set_agent_pid(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing agent PID key");
    };
    let Some(pid) = u32_param(params, &["pid", "process_id", "processId"]) else {
        return invalid_params("Missing or invalid agent PID");
    };
    if pid == 0 {
        return invalid_params("Agent PID must be positive");
    }
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match set_workspace_agent_pid_for_control(app, &state, index, &key, pid) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_clear_agent_pid(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing agent PID key");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    let snapshot = match clear_workspace_agent_pid_for_control(app, &state, index, &key) {
        Ok(snapshot) => snapshot,
        Err(message) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let snapshot = refresh_workspace_agent_ports(app, &snapshot, index).unwrap_or(snapshot);
    workspace_current(&snapshot)
}

fn workspace_report_pr(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    default_label: &str,
) -> ControlCallResult {
    let Some(number) = i64_param(params, &["number", "pr", "id"]) else {
        return invalid_params("Missing or invalid pull request number");
    };
    if number <= 0 {
        return invalid_params("Pull request number must be positive");
    }
    let Some(url) = string_param(params, &["url", "href"]) else {
        return invalid_params("Missing pull request URL");
    };
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(status) = pull_request_status_param(params) else {
        return invalid_params("Missing or invalid pull request state");
    };
    let label = raw_string_param(params, &["label"])
        .and_then(|label| (!label.trim().is_empty()).then(|| label.trim().to_string()))
        .unwrap_or_else(|| default_label.to_string());
    let branch = raw_string_param(params, &["branch"])
        .and_then(|branch| (!branch.trim().is_empty()).then(|| branch.trim().to_string()));
    let is_stale = bool_param(params, &["stale", "is_stale", "isStale"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    match set_workspace_panel_pull_request_for_control(
        app,
        &state,
        workspace_index,
        &panel_id,
        number,
        &label,
        &url,
        status,
        branch,
        is_stale,
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_clear_pr(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_panel_pull_request_for_control(app, &state, workspace_index, &panel_id) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_report_meta(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar metadata key");
    };
    let Some(value) = raw_string_param(params, &["value", "text", "markdown"]) else {
        return invalid_params("Missing sidebar metadata value");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let priority = i64_param(params, &["priority"]);
    let icon = string_param(params, &["icon"]);
    let color = string_param(params, &["color"]);
    let url = string_param(params, &["url", "href"]);
    let format = string_param(params, &["format"]);
    let state = app.state::<SessionState>();
    match set_workspace_sidebar_metadata_for_control(
        app,
        &state,
        index as i64,
        &key,
        &value,
        icon.as_deref(),
        color.as_deref(),
        url.as_deref(),
        priority,
        format.as_deref(),
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_clear_meta(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar metadata key");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_metadata_for_control(app, &state, index as i64, &key) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_list_meta(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace) = workspace_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    ok(json!({
        "metadata_entries": workspace.sidebar_metadata_entries.clone().unwrap_or_default(),
        "metadata_count": workspace.sidebar_metadata_entries.as_ref().map_or(0, Vec::len),
    }))
}

fn workspace_report_meta_block(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar metadata block key");
    };
    let Some(markdown) = raw_string_param(params, &["markdown", "value", "text"]) else {
        return invalid_params("Missing sidebar metadata block markdown");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let priority = i64_param(params, &["priority"]);
    let state = app.state::<SessionState>();
    match set_workspace_sidebar_metadata_block_for_control(
        app,
        &state,
        index as i64,
        &key,
        &markdown,
        priority,
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_clear_meta_block(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar metadata block key");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_metadata_block_for_control(app, &state, index as i64, &key) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_list_meta_blocks(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace) = workspace_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    ok(json!({
        "metadata_blocks": workspace.sidebar_metadata_blocks.clone().unwrap_or_default(),
        "metadata_block_count": workspace.sidebar_metadata_blocks.as_ref().map_or(0, Vec::len),
    }))
}

fn workspace_reset_sidebar(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match reset_workspace_sidebar_metadata_for_control(app, &state, index as i64) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_log(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(message) = raw_string_param(params, &["message", "text"]) else {
        return invalid_params("Missing sidebar log message");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let level = string_param(params, &["level"]).unwrap_or_else(|| "info".to_string());
    let state = app.state::<SessionState>();
    match append_workspace_sidebar_log_for_control(app, &state, index as i64, &message, &level) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_clear_log(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_log_for_control(app, &state, index as i64) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_list_log(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace) = workspace_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let limit = usize_param(params, &["limit"]).unwrap_or(20);
    let entries = recent_sidebar_log_entries(workspace, limit);
    ok(json!({
        "log_entries": entries,
        "log_count": workspace.sidebar_log_entries.as_ref().map_or(0, Vec::len),
    }))
}

fn workspace_sidebar_state(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window) = current.windows.first() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(index),
        "ports": workspace_listening_ports(workspace),
        "agent_listening_ports": workspace.agent_listening_ports.clone().unwrap_or_default(),
        "agent_pids": workspace.agent_pids.clone().unwrap_or_default(),
        "agent_pid_count": workspace.agent_pids.as_ref().map_or(0, Vec::len),
        "panel_ttys": workspace.panel_ttys.clone().unwrap_or_default(),
        "tty_count": workspace.panel_ttys.as_ref().map_or(0, Vec::len),
        "panel_shell_activity": workspace.panel_shell_activity.clone().unwrap_or_default(),
        "shell_activity_count": workspace.panel_shell_activity.as_ref().map_or(0, Vec::len),
        "progress": workspace.sidebar_progress,
        "status_entries": workspace.sidebar_status_entries.clone().unwrap_or_default(),
        "status_count": workspace.sidebar_status_entries.as_ref().map_or(0, Vec::len),
        "metadata_entries": workspace.sidebar_metadata_entries.clone().unwrap_or_default(),
        "metadata_count": workspace.sidebar_metadata_entries.as_ref().map_or(0, Vec::len),
        "metadata_blocks": workspace.sidebar_metadata_blocks.clone().unwrap_or_default(),
        "metadata_block_count": workspace.sidebar_metadata_blocks.as_ref().map_or(0, Vec::len),
        "log_entries": recent_sidebar_log_entries(workspace, usize_param(params, &["limit"]).unwrap_or(20)),
        "log_count": workspace.sidebar_log_entries.as_ref().map_or(0, Vec::len),
    }))
}

fn workspace_set_unread(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(unread) = bool_param(params, &["unread", "is_unread"]) else {
        return invalid_params("Missing or invalid workspace unread flag");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let preferred_panel_id =
        string_param(params, &["preferred_panel_id", "panel_id", "surface_id"]);
    let state = app.state::<SessionState>();
    match set_workspace_unread_for_control(
        app,
        &state,
        index as i64,
        preferred_panel_id.as_deref(),
        unread,
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_set_pinned(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(pinned) = bool_param(params, &["pinned", "is_pinned"]) else {
        return invalid_params("Missing or invalid workspace pinned flag");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match set_workspace_pinned_for_control(app, &state, index as i64, pinned) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn workspace_remote_status(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window) = current.windows.first() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(index),
        "remote": workspace_remote_payload(workspace),
    }))
}

fn workspace_remote_configure(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(destination) = string_param(params, &["destination", "host"]) else {
        return invalid_params("Missing destination");
    };
    let Some(port) = optional_u16_param(params, "port") else {
        return invalid_params("port must be 1-65535");
    };
    let Some(local_proxy_port) = optional_u16_param(params, "local_proxy_port") else {
        return invalid_params("local_proxy_port must be 1-65535");
    };
    let transport = string_param(params, &["transport"])
        .unwrap_or_else(|| "ssh".to_string())
        .to_ascii_lowercase();
    if !matches!(transport.as_str(), "ssh" | "websocket") {
        return invalid_params("transport must be ssh or websocket");
    }
    let auto_connect = bool_param(params, &["auto_connect"]).unwrap_or(true);
    let persistent_daemon_slot =
        string_param(params, &["persistent_daemon_slot", "persistentDaemonSlot"]);
    let remote_daemon_path = string_param(params, &["remote_daemon_path", "remoteDaemonPath"]);
    let Some(remote_daemon_relay_port) = optional_u16_param(params, "remote_daemon_relay_port")
    else {
        return invalid_params("remote_daemon_relay_port must be 1-65535");
    };
    let remote_daemon_relay_port = match remote_daemon_relay_port {
        Some(port) => Some(port),
        None => match optional_u16_param(params, "remoteDaemonRelayPort") {
            Some(value) => value,
            None => return invalid_params("remoteDaemonRelayPort must be 1-65535"),
        },
    };
    let identity_file = string_param(params, &["identity_file", "identityFile"]);
    let ssh_options = string_vec_param(params, &["ssh_options", "sshOptions"]).unwrap_or_default();
    let config = WorkspaceRemoteControlConfig {
        transport,
        destination,
        port,
        local_proxy_port,
        persistent_daemon_slot,
        remote_daemon_path,
        remote_daemon_relay_port,
        identity_file,
        ssh_options,
        auto_connect,
    };
    let state = app.state::<SessionState>();
    let Some(snapshot) = configure_workspace_remote_for_control(app, &state, index, config) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    workspace_remote_status_from_snapshot(&snapshot, index)
}

fn workspace_remote_disconnect(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    let Some(snapshot) = clear_workspace_remote_for_control(app, &state, index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    workspace_remote_status_from_snapshot(&snapshot, index)
}

fn workspace_remote_reconnect(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match reconnect_workspace_remote_for_control(app, &state, index) {
        Ok(Some(snapshot)) => workspace_remote_status_from_snapshot(&snapshot, index),
        Ok(None) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        },
        Err(message) => ControlCallResult::Err {
            code: "invalid_state".to_string(),
            message: format!("Remote workspace is not configured: {message}"),
            data: None,
        },
    }
}

fn workspace_remote_status_from_snapshot(
    snapshot: &AppSessionSnapshot,
    index: usize,
) -> ControlCallResult {
    let Some(window) = snapshot.windows.first() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(index),
        "remote": workspace_remote_payload(workspace),
    }))
}

fn workspace_group_set_collapsed(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(group_id) = string_param(params, &["group_id", "id"]) else {
        return invalid_params("Missing workspace group id");
    };
    let Some(collapsed) = bool_param(params, &["collapsed", "is_collapsed"]) else {
        return invalid_params("Missing or invalid collapsed flag");
    };
    let state = app.state::<SessionState>();
    match set_group_collapsed_for_control(app, &state, &group_id, collapsed) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_split(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(orientation) = split_orientation_from_params(params) else {
        return invalid_params("Invalid split orientation");
    };
    let insert_first = insert_first_param(params);
    let (initial_terminal_command, initial_terminal_input, initial_terminal_environment) =
        terminal_startup_params(params);
    let state = app.state::<SessionState>();
    match split_panel_for_control(
        app,
        &state,
        &panel_id,
        orientation,
        insert_first,
        initial_terminal_command.as_deref(),
        initial_terminal_input.as_deref(),
        initial_terminal_environment,
    ) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(TerminalPanelCreateError::NotFound(message)) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message,
            data: None,
        },
        Err(TerminalPanelCreateError::Publication(message)) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_new_terminal_tab(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let (initial_terminal_command, initial_terminal_input, initial_terminal_environment) =
        terminal_startup_params(params);
    let state = app.state::<SessionState>();
    match new_terminal_tab_for_control(
        app,
        &state,
        &panel_id,
        initial_terminal_command.as_deref(),
        initial_terminal_input.as_deref(),
        initial_terminal_environment,
    ) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(TerminalPanelCreateError::NotFound(message)) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message,
            data: None,
        },
        Err(TerminalPanelCreateError::Publication(message)) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_split_browser(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(orientation) = split_orientation_from_params(params) else {
        return invalid_params("Invalid split orientation");
    };
    let url = raw_string_param(params, &["url"]);
    let state = app.state::<SessionState>();
    match split_browser_for_control(
        app,
        &state,
        &panel_id,
        orientation,
        insert_first_param(params),
        url.as_deref(),
    ) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message,
            data: None,
        },
    }
}

fn global_surface_location(
    snapshot: &AppSessionSnapshot,
    panel_id: &str,
) -> Option<(usize, usize)> {
    snapshot
        .windows
        .iter()
        .enumerate()
        .find_map(|(window_index, window)| {
            window
                .tab_manager
                .workspaces
                .iter()
                .enumerate()
                .find(|(_, workspace)| {
                    surfaces_for_workspace(workspace)
                        .iter()
                        .any(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id))
                })
                .map(|(workspace_index, _)| (window_index, workspace_index))
        })
}

fn split_off_window_index(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<Option<usize>> {
    let Some(selector) = raw_string_param(params, &["window_ref", "window_id"]) else {
        return Some(None);
    };
    let identity = crate::window::current_control_window(app, Some(&selector))?;
    snapshot
        .windows
        .iter()
        .position(|window| window.window_id.as_deref() == Some(identity.label.as_str()))
        .map(Some)
}

fn split_off_workspace_index(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    window_index: usize,
) -> Option<Option<usize>> {
    let window = snapshot.windows.get(window_index)?;
    if let Some(reference) = string_param(params, &["workspace_ref"]) {
        return one_based_ref_index(&reference, "workspace")
            .filter(|index| *index < window.tab_manager.workspaces.len())
            .map(Some);
    }
    if let Some(workspace_id) = string_param(params, &["workspace_id"]) {
        return window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
            .map(Some);
    }
    Some(None)
}

fn surface_split_off(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(direction) = string_param(params, &["direction"]) else {
        return invalid_params("Missing or invalid direction (left|right|up|down)");
    };
    let Some(orientation) = split_orientation_from_params(params) else {
        return invalid_params("Missing or invalid direction (left|right|up|down)");
    };
    let normalized_direction = direction.to_ascii_lowercase();
    let insert_first = matches!(normalized_direction.as_str(), "left" | "up" | "l" | "u");
    let current = snapshot(app);
    let requested_window_resolution = split_off_window_index(app, &current, params);
    let requested_window_index = requested_window_resolution.flatten();

    let direct_panel_id = string_param(params, &["surface_id", "panel_id"]);
    let (window_index, workspace_index, panel_id) = if let Some(panel_id) = direct_panel_id {
        let Some((window_index, workspace_index)) = global_surface_location(&current, &panel_id)
        else {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: Some(
                    json!({"surface_id": panel_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        };
        (window_index, workspace_index, panel_id)
    } else if let Some(surface_reference) = string_param(params, &["surface_ref"]) {
        if requested_window_resolution.is_none() {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found in window".to_string(),
                data: None,
            };
        }
        let window_index = requested_window_index.unwrap_or(0);
        let requested_workspace_index =
            match split_off_workspace_index(&current, params, window_index) {
                Some(index) => index,
                None => {
                    return ControlCallResult::Err {
                        code: "not_found".to_string(),
                        message: "Surface not found in workspace".to_string(),
                        data: None,
                    };
                }
            };
        let window = &current.windows[window_index];
        let workspace_index = requested_workspace_index.unwrap_or_else(|| {
            window
                .tab_manager
                .selected_workspace_index
                .and_then(|index| usize::try_from(index).ok())
                .unwrap_or(0)
        });
        let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found in workspace".to_string(),
                data: None,
            };
        };
        let Some(surface_index) = one_based_ref_index(&surface_reference, "surface") else {
            return invalid_params("Missing or invalid surface_id");
        };
        let Some(panel_id) = surfaces_for_workspace(workspace)
            .get(surface_index)
            .and_then(|surface| surface.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: None,
            };
        };
        (window_index, workspace_index, panel_id)
    } else {
        return invalid_params("Missing or invalid surface_id");
    };

    if requested_window_resolution.is_none()
        || requested_window_index.is_some_and(|requested| requested != window_index)
    {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Surface not found in window".to_string(),
            data: None,
        };
    }
    let requested_workspace_index = match split_off_workspace_index(
        &current,
        params,
        requested_window_index.unwrap_or(window_index),
    ) {
        Some(index) => index,
        None => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found in workspace".to_string(),
                data: None,
            };
        }
    };
    if requested_workspace_index.is_some_and(|requested| requested != workspace_index) {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Surface not found in workspace".to_string(),
            data: None,
        };
    }

    let workspace = &current.windows[window_index].tab_manager.workspaces[workspace_index];
    let Some((_, source_pane_id, _)) = surface_pane_details(workspace, &panel_id) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Source pane not found".to_string(),
            data: None,
        };
    };
    let state = app.state::<SessionState>();
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let result = match split_off_surface_for_control(
        app,
        &state,
        window_index,
        workspace_index,
        &panel_id,
        orientation,
        insert_first,
        focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(
            session_ops::SplitOffSurfaceError::WouldEmptySourcePane,
        )) => {
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message: "splitting off would leave the source pane empty".to_string(),
                data: Some(
                    json!({"surface_id": panel_id, "pane_id": source_pane_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        Err(PaneTopologyControlError::Operation(
            session_ops::SplitOffSurfaceError::SurfaceNotFound,
        )) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let Some((pane_index, pane_id, _)) = surface_pane_details(workspace, &panel_id) else {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: "Failed to split pane".to_string(),
            data: None,
        };
    };
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window.window_id.as_deref().unwrap_or("main"))
        .map(|summary| summary.identity);
    if focus {
        if let Some(identity) = window_identity.as_ref() {
            if let Some(window) = app.get_webview_window(&identity.label) {
                let _ = window.set_focus();
            }
        }
    }
    let surface_ref_value = surfaces_for_workspace(workspace)
        .iter()
        .position(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id.as_str()))
        .map(surface_ref);
    ok(json!({
        "window_id": window_identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": window_identity.as_ref().map(|identity| identity.reference.clone()),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_value,
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
    }))
}

fn global_pane_location(
    snapshot: &AppSessionSnapshot,
    pane_id: &str,
) -> Option<(usize, usize, usize)> {
    snapshot
        .windows
        .iter()
        .enumerate()
        .find_map(|(window_index, window)| {
            window.tab_manager.workspaces.iter().enumerate().find_map(
                |(workspace_index, workspace)| {
                    pane_index_by_id(workspace, pane_id)
                        .map(|pane_index| (window_index, workspace_index, pane_index))
                },
            )
        })
}

#[derive(Debug, Clone, Copy)]
struct PanePixelFrame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

fn pane_frames(
    layout: &SessionWorkspaceLayoutSnapshot,
    frame: PanePixelFrame,
    rows: &mut Vec<(SessionPaneLayoutSnapshot, PanePixelFrame)>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => rows.push((pane.clone(), frame)),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let divider = if split.divider_position.is_finite() {
                split.divider_position.clamp(0.1, 0.9)
            } else {
                0.5
            };
            let (first, second) = match split.orientation {
                SessionSplitOrientation::Horizontal => {
                    let first_width = frame.width * divider;
                    (
                        PanePixelFrame {
                            width: first_width,
                            ..frame
                        },
                        PanePixelFrame {
                            x: frame.x + first_width,
                            width: frame.width - first_width,
                            ..frame
                        },
                    )
                }
                SessionSplitOrientation::Vertical => {
                    let first_height = frame.height * divider;
                    (
                        PanePixelFrame {
                            height: first_height,
                            ..frame
                        },
                        PanePixelFrame {
                            y: frame.y + first_height,
                            height: frame.height - first_height,
                            ..frame
                        },
                    )
                }
            };
            pane_frames(&split.first, first, rows);
            pane_frames(&split.second, second, rows);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneFocusResolveError {
    WorkspaceNotFound,
    PaneNotFound,
}

fn resolve_pane_focus_target(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    window_index: usize,
) -> Result<(usize, usize, String), PaneFocusResolveError> {
    let window = snapshot
        .windows
        .get(window_index)
        .ok_or(PaneFocusResolveError::WorkspaceNotFound)?;
    let workspace_index = split_off_workspace_index(snapshot, params, window_index)
        .ok_or(PaneFocusResolveError::WorkspaceNotFound)?
        .or_else(|| {
            window
                .tab_manager
                .selected_workspace_index
                .and_then(|index| usize::try_from(index).ok())
        })
        .unwrap_or(0);
    let workspace = window
        .tab_manager
        .workspaces
        .get(workspace_index)
        .ok_or(PaneFocusResolveError::WorkspaceNotFound)?;
    if let Some(pane_id) = string_param(params, &["pane_id"]) {
        return pane_index_by_id(workspace, &pane_id)
            .map(|pane_index| (workspace_index, pane_index, pane_id))
            .ok_or(PaneFocusResolveError::PaneNotFound);
    }
    let pane_index = string_param(params, &["pane_ref"])
        .and_then(|reference| one_based_ref_index(&reference, "pane"))
        .ok_or(PaneFocusResolveError::PaneNotFound)?;
    let (_, pane_id) =
        pane_at_index(workspace, pane_index).ok_or(PaneFocusResolveError::PaneNotFound)?;
    Ok((workspace_index, pane_index, pane_id))
}

fn pane_focus(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    if current.windows.is_empty() {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    }
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    if !params.contains_key("pane_id") && !params.contains_key("pane_ref") {
        return invalid_params("Missing or invalid pane_id");
    }
    let window_index = requested_window.unwrap_or_else(|| {
        crate::window::current_control_window(app, None)
            .and_then(|identity| {
                current
                    .windows
                    .iter()
                    .position(|window| window.window_id.as_deref() == Some(identity.label.as_str()))
            })
            .unwrap_or(0)
    });
    let (workspace_index, pane_index, pane_id) =
        match resolve_pane_focus_target(&current, params, window_index) {
            Ok(target) => target,
            Err(PaneFocusResolveError::WorkspaceNotFound) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "Workspace not found".to_string(),
                    data: None,
                };
            }
            Err(PaneFocusResolveError::PaneNotFound) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "Pane not found".to_string(),
                    data: Some(
                        json!({"pane_id": string_param(params, &["pane_id", "pane_ref"])})
                            .try_into()
                            .unwrap_or(JsonValue::Null),
                    ),
                };
            }
        };
    let state = app.state::<SessionState>();
    let result = match focus_pane_for_control(app, &state, window_index, workspace_index, &pane_id)
    {
        Ok(snapshot) => snapshot,
        Err(PaneTopologyControlError::Operation(PaneFocusControlError::WorkspaceNotFound)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Workspace not found".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneFocusControlError::PaneNotFound)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Pane not found".to_string(),
                data: Some(
                    json!({"pane_id": pane_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let window_label = window.window_id.as_deref().unwrap_or("main");
    let identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window_label)
        .map(|summary| summary.identity);
    if let Some(identity) = identity.as_ref() {
        if let Some(window) = app.get_webview_window(&identity.label) {
            let _ = window.set_focus();
        }
    }
    ok(json!({
        "window_id": identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": identity.as_ref().map(|identity| identity.reference.clone()),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
    }))
}

fn pane_list(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or(0);
    let Some(requested_workspace) = split_off_workspace_index(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(window) = current.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let workspace_index = requested_workspace.unwrap_or_else(|| {
        window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(0)
    });
    let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let window_label = window.window_id.as_deref().unwrap_or("main");
    let (width, height) = app
        .get_webview_window(window_label)
        .and_then(|window| window.inner_size().ok())
        .map(|size| (f64::from(size.width), f64::from(size.height)))
        .unwrap_or((1.0, 1.0));
    let Some(layout) = workspace.layout.as_ref() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let mut pane_rows = Vec::new();
    pane_frames(
        layout,
        PanePixelFrame {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        &mut pane_rows,
    );
    let terminal_state = app.state::<TerminalState>();
    let workspace_surface_ids = surfaces_for_workspace(workspace)
        .into_iter()
        .filter_map(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    let panes = pane_rows
        .into_iter()
        .enumerate()
        .map(|(index, (pane, frame))| {
            let selected = pane
                .selected_panel_id
                .as_deref()
                .filter(|panel_id| pane.panel_ids.iter().any(|id| id == panel_id));
            let mut row = json!({
                "id": pane.pane_id,
                "ref": pane_ref(index),
                "index": index,
                "focused": workspace.focused_panel_id.as_ref().is_some_and(|focused| pane.panel_ids.contains(focused)),
                "surface_ids": pane.panel_ids,
                "surface_refs": pane.panel_ids.iter().filter_map(|panel_id| workspace_surface_ids.iter().position(|id| id == panel_id).map(surface_ref)).collect::<Vec<_>>(),
                "selected_surface_id": selected,
                "selected_surface_ref": selected.and_then(|selected| workspace_surface_ids.iter().position(|id| id == selected)).map(surface_ref),
                "surface_count": pane.panel_ids.len(),
                "pixel_frame": {"x": frame.x, "y": frame.y, "width": frame.width, "height": frame.height},
            });
            if let Some(size) = selected.and_then(|panel_id| {
                terminal_grid_size_for_panel(terminal_state.inner(), panel_id)
            }) {
                if let Some(object) = row.as_object_mut() {
                    object.insert("columns".to_string(), json!(size.columns));
                    object.insert("rows".to_string(), json!(size.screen_lines));
                    object.insert(
                        "cell_width_px".to_string(),
                        json!((frame.width / size.columns.max(1) as f64).round().max(1.0) as u64),
                    );
                    object.insert(
                        "cell_height_px".to_string(),
                        json!((frame.height / size.screen_lines.max(1) as f64).round().max(1.0) as u64),
                    );
                }
            }
            row
        })
        .collect::<Vec<_>>();
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window_label)
        .map(|summary| summary.identity);
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "panes": panes,
        "window_id": window_identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": window_identity.as_ref().map(|identity| identity.reference.clone()),
        "container_frame": {"width": width, "height": height},
    }))
}

fn pane_snapshot_at_index(
    workspace: &SessionWorkspaceSnapshot,
    target_index: usize,
) -> Option<&SessionPaneLayoutSnapshot> {
    fn visit<'a>(
        layout: &'a SessionWorkspaceLayoutSnapshot,
        target_index: usize,
        index: &mut usize,
    ) -> Option<&'a SessionPaneLayoutSnapshot> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                let current = *index;
                *index += 1;
                (current == target_index).then_some(pane)
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit(&split.first, target_index, index)
                    .or_else(|| visit(&split.second, target_index, index))
            }
        }
    }
    let mut index = 0;
    visit(workspace.layout.as_ref()?, target_index, &mut index)
}

fn resolve_pane_surfaces_target(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    window_index: usize,
) -> Option<(usize, usize, String)> {
    let window = snapshot.windows.get(window_index)?;
    let requested_workspace = split_off_workspace_index(snapshot, params, window_index)?;
    let selected_workspace = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .unwrap_or(0);
    if let Some(pane_id) = string_param(params, &["pane_id"]) {
        if let Some(workspace_index) = requested_workspace {
            let workspace = window.tab_manager.workspaces.get(workspace_index)?;
            return pane_index_by_id(workspace, &pane_id)
                .map(|pane_index| (workspace_index, pane_index, pane_id));
        }
        return window.tab_manager.workspaces.iter().enumerate().find_map(
            |(workspace_index, workspace)| {
                pane_index_by_id(workspace, &pane_id)
                    .map(|pane_index| (workspace_index, pane_index, pane_id.clone()))
            },
        );
    }
    let workspace_index = requested_workspace.unwrap_or(selected_workspace);
    let workspace = window.tab_manager.workspaces.get(workspace_index)?;
    if let Some(reference) = string_param(params, &["pane_ref"]) {
        let pane_index = one_based_ref_index(&reference, "pane")?;
        let (_, pane_id) = pane_at_index(workspace, pane_index)?;
        return Some((workspace_index, pane_index, pane_id));
    }
    workspace
        .focused_panel_id
        .as_deref()
        .and_then(|panel_id| surface_pane_details(workspace, panel_id))
        .and_then(|(pane_index, pane_id, _)| {
            pane_id.map(|pane_id| (workspace_index, pane_index, pane_id))
        })
}

fn pane_surfaces(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let lifecycle = match session_ops::read_surface_lifecycle(&current) {
        Ok(model) => model,
        Err(error) => {
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message: format!("Invalid surface lifecycle: {error}"),
                data: None,
            };
        }
    };
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or_else(|| {
        crate::window::current_control_window(app, None)
            .and_then(|identity| {
                current
                    .windows
                    .iter()
                    .position(|window| window.window_id.as_deref() == Some(identity.label.as_str()))
            })
            .unwrap_or(0)
    });
    let Some((workspace_index, pane_index, pane_id)) =
        resolve_pane_surfaces_target(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Pane or workspace not found".to_string(),
            data: None,
        };
    };
    let window = &current.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let pane = pane_snapshot_at_index(workspace, pane_index)
        .expect("resolved pane index remains in the immutable snapshot");
    let workspace_surface_ids = surfaces_for_workspace(workspace)
        .into_iter()
        .filter_map(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    let surfaces = pane
        .panel_ids
        .iter()
        .enumerate()
        .map(|(index, panel_id)| {
            let surface_type = lifecycle
                .surface(panel_id)
                .map(|surface| surface_kind_label(&surface.kind))
                .unwrap_or_else(|| pane.surface_kind.as_deref().unwrap_or("terminal"));
            json!({
                "id": panel_id,
                "ref": workspace_surface_ids.iter().position(|id| id == panel_id).map(surface_ref),
                "index": index,
                "title": lifecycle.surface(panel_id).and_then(|surface| surface.metadata.custom_title.clone()).or_else(|| panel_title(&workspace.panel_titles, panel_id)).unwrap_or_else(|| surface_type.to_string()),
                "type": surface_type,
                "selected": pane.selected_panel_id.as_deref() == Some(panel_id.as_str()),
            })
        })
        .collect::<Vec<_>>();
    let window_label = window.window_id.as_deref().unwrap_or("main");
    let identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window_label)
        .map(|summary| summary.identity);
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
        "surfaces": surfaces,
        "window_id": identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": identity.as_ref().map(|identity| identity.reference.clone()),
    }))
}

fn pane_swap(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    if !params.contains_key("pane_id") && !params.contains_key("pane_ref") {
        return invalid_params("Missing or invalid pane_id");
    }
    if !params.contains_key("target_pane_id") && !params.contains_key("target_pane_ref") {
        return invalid_params("Missing or invalid target_pane_id");
    }
    let current = snapshot(app);
    let source = if let Some(pane_id) = string_param(params, &["pane_id"]) {
        global_pane_location(&current, &pane_id).map(|location| (location, pane_id))
    } else {
        (|| {
            let requested_window = split_off_window_index(app, &current, params)?;
            let window_index = requested_window.unwrap_or(0);
            let requested_workspace = split_off_workspace_index(&current, params, window_index)?;
            let workspace_index = requested_workspace.unwrap_or_else(|| {
                current.windows[window_index]
                    .tab_manager
                    .selected_workspace_index
                    .and_then(|index| usize::try_from(index).ok())
                    .unwrap_or(0)
            });
            let pane_index = string_param(params, &["pane_ref"])
                .and_then(|reference| one_based_ref_index(&reference, "pane"))?;
            let workspace = current
                .windows
                .get(window_index)?
                .tab_manager
                .workspaces
                .get(workspace_index)?;
            pane_at_index(workspace, pane_index)
                .map(|(_, pane_id)| ((window_index, workspace_index, pane_index), pane_id))
        })()
    };
    let Some(((window_index, workspace_index, source_pane_index), source_pane_id)) = source else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Source pane not found".to_string(),
            data: None,
        };
    };
    let workspace = &current.windows[window_index].tab_manager.workspaces[workspace_index];
    let target = if let Some(target_pane_id) = string_param(params, &["target_pane_id"]) {
        global_pane_location(&current, &target_pane_id)
            .filter(|(target_window, target_workspace, _)| {
                *target_window == window_index && *target_workspace == workspace_index
            })
            .map(|(_, _, pane_index)| (pane_index, target_pane_id))
    } else {
        string_param(params, &["target_pane_ref"])
            .and_then(|reference| one_based_ref_index(&reference, "pane"))
            .and_then(|pane_index| pane_at_index(workspace, pane_index))
    };
    let Some((target_pane_index, target_pane_id)) = target else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Target pane not found in source workspace".to_string(),
            data: None,
        };
    };
    if source_pane_id == target_pane_id {
        return invalid_params("pane_id and target_pane_id must be different");
    }
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    let (swap, result) = match swap_panes_for_control(
        app,
        &state,
        window_index,
        workspace_index,
        &source_pane_id,
        &target_pane_id,
        focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(session_ops::PaneSwapError::SamePane)) => {
            return invalid_params("pane_id and target_pane_id must be different");
        }
        Err(PaneTopologyControlError::Operation(
            session_ops::PaneSwapError::SourcePaneNotFound,
        )) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Source pane not found".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(
            session_ops::PaneSwapError::TargetPaneNotFound,
        )) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Target pane not found in source workspace".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(
            session_ops::PaneSwapError::BothPanesNeedSurface,
        )) => {
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message: "Both panes must have a selected surface".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let surface_ref_value = |panel_id: &str| {
        surfaces_for_workspace(workspace)
            .iter()
            .position(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id))
            .map(surface_ref)
    };
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window.window_id.as_deref().unwrap_or("main"))
        .map(|summary| summary.identity);
    if focus {
        if let Some(identity) = window_identity.as_ref() {
            if let Some(window) = app.get_webview_window(&identity.label) {
                let _ = window.set_focus();
            }
        }
    }
    ok(json!({
        "window_id": window_identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": window_identity.as_ref().map(|identity| identity.reference.clone()),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": source_pane_id,
        "pane_ref": pane_ref(source_pane_index),
        "target_pane_id": target_pane_id,
        "target_pane_ref": pane_ref(target_pane_index),
        "source_surface_id": swap.source_surface_id,
        "source_surface_ref": surface_ref_value(&swap.source_surface_id),
        "target_surface_id": swap.target_surface_id,
        "target_surface_ref": surface_ref_value(&swap.target_surface_id),
    }))
}

fn pane_break(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or(0);
    let Some(requested_workspace) = split_off_workspace_index(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(window) = current.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let workspace_index = requested_workspace.unwrap_or_else(|| {
        window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(0)
    });
    let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let surfaces = surfaces_for_workspace(workspace);
    let source_pane_id = if let Some(pane_id) = string_param(params, &["pane_id"]) {
        pane_index_by_id(workspace, &pane_id).map(|_| pane_id)
    } else if let Some(pane_reference) = string_param(params, &["pane_ref"]) {
        one_based_ref_index(&pane_reference, "pane")
            .and_then(|pane_index| pane_at_index(workspace, pane_index))
            .map(|(_, pane_id)| pane_id)
    } else {
        first_or_focused_pane(workspace).map(|(_, pane_id)| pane_id)
    };
    let explicit_surface_id = string_param(params, &["surface_id"]);
    let panel_id = if let Some(panel_id) = explicit_surface_id.as_ref() {
        surfaces
            .iter()
            .any(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id.as_str()))
            .then(|| panel_id.clone())
    } else if let Some(surface_reference) = string_param(params, &["surface_ref"]) {
        one_based_ref_index(&surface_reference, "surface").and_then(|surface_index| {
            surfaces
                .get(surface_index)
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
    } else if let Some(pane_id) = source_pane_id.as_deref() {
        surfaces
            .iter()
            .find(|surface| {
                surface.get("pane_id").and_then(Value::as_str) == Some(pane_id)
                    && surface
                        .get("selected_in_pane")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
            })
            .and_then(|surface| surface.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        surfaces
            .iter()
            .find(|surface| {
                surface
                    .get("focused")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .or_else(|| surfaces.first())
            .and_then(|surface| surface.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let Some(panel_id) = panel_id else {
        if let Some(surface_id) = explicit_surface_id {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: Some(
                    json!({"surface_id": surface_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No source surface to break".to_string(),
            data: None,
        };
    };
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    let (broken, result) = match break_pane_for_control(
        app,
        &state,
        window_index,
        workspace_index,
        &panel_id,
        focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(
            session_ops::PaneBreakError::WorkspaceNotFound,
        )) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Workspace not found".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(session_ops::PaneBreakError::SurfaceNotFound)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: Some(
                    json!({"surface_id": panel_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        Err(PaneTopologyControlError::Operation(session_ops::PaneBreakError::DetachFailed)) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: "Failed to detach source surface".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[broken.workspace_index];
    let pane_id = surface_pane_details(workspace, &broken.surface_id)
        .and_then(|(_, pane_id, _)| pane_id)
        .expect("state layer mints destination pane ids");
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window.window_id.as_deref().unwrap_or("main"))
        .map(|summary| summary.identity);
    if focus {
        if let Some(identity) = window_identity.as_ref() {
            if let Some(window) = app.get_webview_window(&identity.label) {
                let _ = window.set_focus();
            }
        }
    }
    ok(json!({
        "window_id": window_identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": window_identity.as_ref().map(|identity| identity.reference.clone()),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(broken.workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(0),
        "surface_id": broken.surface_id,
        "surface_ref": surface_ref(0),
    }))
}

#[derive(Debug, PartialEq, Eq)]
enum PaneJoinSourceError {
    Missing,
    SourcePaneUnresolved(String),
}

fn resolve_pane_join_source(
    current: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<String, PaneJoinSourceError> {
    if let Some(surface_id) = string_param(params, &["surface_id"]) {
        return Ok(surface_id);
    }
    if params.contains_key("surface_ref") {
        let workspace_index = workspace_index_from_workspace_scope_or_selected(current, params)
            .ok_or(PaneJoinSourceError::Missing)?;
        let workspace = current
            .windows
            .first()
            .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
            .ok_or(PaneJoinSourceError::Missing)?;
        return surface_id_from_selector_keys(workspace, params, &["surface_ref"], &["surface_id"])
            .ok_or(PaneJoinSourceError::Missing);
    }
    let (workspace_index, pane_id) = if let Some(pane_id) = string_param(params, &["pane_id"]) {
        pane_location_by_id(current, &pane_id)
            .map(|(workspace_index, _, pane_id)| (workspace_index, pane_id))
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_id))?
    } else if let Some(pane_reference) = string_param(params, &["pane_ref"]) {
        let workspace_index = workspace_index_from_workspace_scope_or_selected(current, params)
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_reference.clone()))?;
        let workspace = current
            .windows
            .first()
            .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_reference.clone()))?;
        let pane_index = one_based_ref_index(&pane_reference, "pane")
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_reference.clone()))?;
        pane_at_index(workspace, pane_index)
            .map(|(_, pane_id)| (workspace_index, pane_id))
            .ok_or(PaneJoinSourceError::SourcePaneUnresolved(pane_reference))?
    } else {
        return Err(PaneJoinSourceError::Missing);
    };
    let workspace = &current.windows[0].tab_manager.workspaces[workspace_index];
    surfaces_for_workspace(workspace)
        .into_iter()
        .find(|surface| {
            surface.get("pane_id").and_then(Value::as_str) == Some(pane_id.as_str())
                && surface
                    .get("selected_in_pane")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .and_then(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or(PaneJoinSourceError::SourcePaneUnresolved(pane_id))
}

fn pane_join(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    if !params.contains_key("target_pane_id") && !params.contains_key("target_pane_ref") {
        return invalid_params("Missing or invalid target_pane_id");
    }
    let current = snapshot(app);
    let source_panel_id = match resolve_pane_join_source(&current, params) {
        Ok(panel_id) => panel_id,
        Err(PaneJoinSourceError::Missing) => {
            return invalid_params("Missing surface_id (or pane_id with selected surface)");
        }
        Err(PaneJoinSourceError::SourcePaneUnresolved(pane_id)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Unable to resolve selected surface in source pane".to_string(),
                data: Some(
                    json!({"pane_id": pane_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    let mut move_params = serde_json::Map::new();
    move_params.insert("surface_id".to_string(), json!(source_panel_id));
    for key in [
        "target_pane_id",
        "target_pane_ref",
        "workspace_id",
        "workspace_ref",
        "window_id",
        "window_ref",
        "focus",
    ] {
        if let Some(value) = params.get(key) {
            let move_key = key.strip_prefix("target_").unwrap_or(key);
            move_params.insert(move_key.to_string(), value.clone());
        }
    }
    surface_move(app, &move_params)
}

fn pane_last(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or(0);
    let Some(requested_workspace) = split_off_workspace_index(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(window) = current.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let workspace_index = requested_workspace.unwrap_or_else(|| {
        window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(0)
    });
    if window.tab_manager.workspaces.get(workspace_index).is_none() {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    }
    let state = app.state::<SessionState>();
    let (focused, result) =
        match focus_last_pane_for_control(app, &state, window_index, workspace_index) {
            Ok(result) => result,
            Err(PaneTopologyControlError::Operation(PaneLastControlError::WorkspaceNotFound)) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "Workspace not found".to_string(),
                    data: None,
                };
            }
            Err(PaneTopologyControlError::Operation(PaneLastControlError::Pane(
                session_ops::PaneLastError::NoFocusedPane,
            ))) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "No focused pane".to_string(),
                    data: None,
                };
            }
            Err(PaneTopologyControlError::Operation(PaneLastControlError::Pane(
                session_ops::PaneLastError::NoAlternatePane,
            ))) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "No alternate pane available".to_string(),
                    data: None,
                };
            }
            Err(PaneTopologyControlError::Publication(message)) => {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
        };
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window.window_id.as_deref().unwrap_or("main"))
        .map(|summary| summary.identity);
    if let Some(identity) = window_identity.as_ref() {
        if let Some(window) = app.get_webview_window(&identity.label) {
            let _ = window.set_focus();
        }
    }
    let surface_ref_value = focused.surface_id.as_deref().and_then(|surface_id| {
        surfaces_for_workspace(workspace)
            .iter()
            .position(|surface| surface.get("id").and_then(Value::as_str) == Some(surface_id))
            .map(surface_ref)
    });
    let pane_index = pane_index_by_id(workspace, &focused.pane_id).unwrap_or(0);
    ok(json!({
        "window_id": window_identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": window_identity.as_ref().map(|identity| identity.reference.clone()),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": focused.pane_id,
        "pane_ref": pane_ref(pane_index),
        "surface_id": focused.surface_id,
        "surface_ref": surface_ref_value,
    }))
}

fn pane_resize(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let intent = if params.contains_key("absolute_axis") || params.contains_key("target_pixels") {
        let axis = match string_param(params, &["absolute_axis"]).as_deref() {
            Some("horizontal") => SessionSplitOrientation::Horizontal,
            Some("vertical") => SessionSplitOrientation::Vertical,
            _ => return invalid_params("absolute_axis must be 'horizontal' or 'vertical'"),
        };
        let Some(target_pixels) =
            f64_param(params, &["target_pixels"]).filter(|value| value.is_finite() && *value > 0.0)
        else {
            return invalid_params("target_pixels must be > 0");
        };
        PaneResizeControlIntent::Absolute {
            axis,
            target_pixels,
        }
    } else {
        let direction = match string_param(params, &["direction"]).as_deref() {
            Some("left") => session_ops::PaneResizeDirection::Left,
            Some("right") => session_ops::PaneResizeDirection::Right,
            Some("up") => session_ops::PaneResizeDirection::Up,
            Some("down") => session_ops::PaneResizeDirection::Down,
            _ => {
                return invalid_params(
                    "direction must be one of left|right|up|down and amount must be > 0",
                );
            }
        };
        let Some(amount) = i64_param(params, &["amount"])
            .filter(|amount| *amount > 0)
            .and_then(|amount| u64::try_from(amount).ok())
        else {
            return invalid_params(
                "direction must be one of left|right|up|down and amount must be > 0",
            );
        };
        PaneResizeControlIntent::Relative { direction, amount }
    };

    let current = snapshot(app);
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or(0);
    let Some(requested_workspace) = split_off_workspace_index(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(window) = current.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let workspace_index = requested_workspace.unwrap_or_else(|| {
        window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(0)
    });
    let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some((pane_index, pane_id)) = resolve_resize_pane(workspace, params) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Pane not found".to_string(),
            data: None,
        };
    };

    let window_label = window.window_id.as_deref().unwrap_or("main");
    let (width, height) = app
        .get_webview_window(window_label)
        .and_then(|window| window.inner_size().ok())
        .map(|size| (f64::from(size.width), f64::from(size.height)))
        .unwrap_or((1.0, 1.0));
    let state = app.state::<SessionState>();
    let (resized, result) = match resize_pane_for_control(
        app,
        &state,
        window_index,
        workspace_index,
        &pane_id,
        intent.clone(),
        width,
        height,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::WorkspaceNotFound)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Workspace not found".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
            session_ops::PaneResizeError::PaneNotFoundInTree,
        ))) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Pane not found in split tree".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
            session_ops::PaneResizeError::NoOrientationSplitAncestor,
        ))) => {
            let message = match intent {
                PaneResizeControlIntent::Absolute { .. } => {
                    "No split ancestor for absolute pane resize".to_string()
                }
                PaneResizeControlIntent::Relative { direction, .. } => format!(
                    "No {} split ancestor for pane",
                    match direction {
                        session_ops::PaneResizeDirection::Left
                        | session_ops::PaneResizeDirection::Right => "horizontal",
                        session_ops::PaneResizeDirection::Up
                        | session_ops::PaneResizeDirection::Down => "vertical",
                    }
                ),
            };
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message,
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
            session_ops::PaneResizeError::NoAdjacentBorder,
        ))) => {
            let direction = match intent {
                PaneResizeControlIntent::Relative { direction, .. } => match direction {
                    session_ops::PaneResizeDirection::Left => "left",
                    session_ops::PaneResizeDirection::Right => "right",
                    session_ops::PaneResizeDirection::Up => "up",
                    session_ops::PaneResizeDirection::Down => "down",
                },
                PaneResizeControlIntent::Absolute { .. } => unreachable!(),
            };
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message: format!("Pane has no adjacent border in direction {direction}"),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
            session_ops::PaneResizeError::MissingSplitIdentity,
        ))) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: "Failed to resize pane".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window_label)
        .map(|summary| summary.identity);
    let mut payload = json!({
        "window_id": window_identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": window_identity.as_ref().map(|identity| identity.reference.clone()),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
        "split_id": resized.split_id,
        "old_divider_position": resized.old_divider_position,
        "new_divider_position": resized.new_divider_position,
    });
    if let Some(object) = payload.as_object_mut() {
        match intent {
            PaneResizeControlIntent::Relative { direction, amount } => {
                let direction = match direction {
                    session_ops::PaneResizeDirection::Left => "left",
                    session_ops::PaneResizeDirection::Right => "right",
                    session_ops::PaneResizeDirection::Up => "up",
                    session_ops::PaneResizeDirection::Down => "down",
                };
                object.insert("direction".to_string(), json!(direction));
                object.insert("amount".to_string(), json!(amount));
            }
            PaneResizeControlIntent::Absolute {
                axis,
                target_pixels,
            } => {
                object.insert(
                    "absolute_axis".to_string(),
                    json!(match axis {
                        SessionSplitOrientation::Horizontal => "horizontal",
                        SessionSplitOrientation::Vertical => "vertical",
                    }),
                );
                object.insert("target_pixels".to_string(), json!(target_pixels));
            }
        }
    }
    ok(payload)
}

fn surface_close(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    surface_list_from_params(&close_panel_for_control(app, &state, &panel_id), params)
}

fn surface_set_kind(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let kind = surface_kind_from_params(params);
    if matches!(kind.as_deref(), Some("invalid")) {
        return invalid_params("Invalid surface type");
    }
    let state = app.state::<SessionState>();
    match set_surface_kind_for_control(app, &state, &panel_id, kind) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_set_title(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(title) = raw_string_param(params, &["title", "name"]) else {
        return invalid_params("Missing surface title");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_title_for_control(app, &state, &panel_id, &title) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_set_pinned(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(pinned) = bool_param(params, &["pinned", "is_pinned"]) else {
        return invalid_params("Missing or invalid surface pinned flag");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_pinned_for_control(app, &state, &panel_id, pinned) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_set_unread(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(unread) = bool_param(params, &["unread", "is_unread"]) else {
        return invalid_params("Missing or invalid surface unread flag");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_unread_for_control(app, &state, &panel_id, unread) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
struct SurfaceMoveResolution {
    source_workspace_index: usize,
    panel_id: String,
    target_workspace_index: usize,
    target_pane_id: String,
    destination_index: Option<i64>,
    focus: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum SurfaceMoveResolveError {
    ConflictingAnchors,
    SourceNotFound,
    DestinationNotFound,
}

fn pane_at_index(
    workspace: &SessionWorkspaceSnapshot,
    target_index: usize,
) -> Option<(usize, String)> {
    fn visit(
        layout: &SessionWorkspaceLayoutSnapshot,
        target_index: usize,
        index: &mut usize,
    ) -> Option<(usize, String)> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                let current = *index;
                *index += 1;
                (current == target_index)
                    .then(|| pane.pane_id.clone().map(|id| (current, id)))
                    .flatten()
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit(&split.first, target_index, index)
                    .or_else(|| visit(&split.second, target_index, index))
            }
        }
    }
    let mut index = 0;
    visit(workspace.layout.as_ref()?, target_index, &mut index)
}

fn resolve_resize_pane(
    workspace: &SessionWorkspaceSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<(usize, String)> {
    if let Some(pane_id) = string_param(params, &["pane_id"]) {
        return pane_index_by_id(workspace, &pane_id).map(|index| (index, pane_id));
    }
    if let Some(reference) = string_param(params, &["pane_ref"]) {
        return one_based_ref_index(&reference, "pane")
            .and_then(|index| pane_at_index(workspace, index));
    }
    workspace
        .focused_panel_id
        .as_deref()
        .and_then(|panel_id| surface_pane_details(workspace, panel_id))
        .and_then(|(index, pane_id, _)| pane_id.map(|pane_id| (index, pane_id)))
}

fn pane_index_by_id(workspace: &SessionWorkspaceSnapshot, pane_id: &str) -> Option<usize> {
    fn visit(
        layout: &SessionWorkspaceLayoutSnapshot,
        pane_id: &str,
        index: &mut usize,
    ) -> Option<usize> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                let current = *index;
                *index += 1;
                (pane.pane_id.as_deref() == Some(pane_id)).then_some(current)
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit(&split.first, pane_id, index).or_else(|| visit(&split.second, pane_id, index))
            }
        }
    }
    let mut index = 0;
    visit(workspace.layout.as_ref()?, pane_id, &mut index)
}

fn pane_location_by_id(
    snapshot: &AppSessionSnapshot,
    pane_id: &str,
) -> Option<(usize, usize, String)> {
    snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .find_map(|(workspace_index, workspace)| {
            pane_index_by_id(workspace, pane_id)
                .map(|pane_index| (workspace_index, pane_index, pane_id.to_string()))
        })
}

fn first_or_focused_pane(workspace: &SessionWorkspaceSnapshot) -> Option<(usize, String)> {
    if let Some(panel_id) = workspace.zoomed_panel_id.as_deref() {
        if let Some((pane_index, Some(pane_id), _)) = surface_pane_details(workspace, panel_id) {
            return Some((pane_index, pane_id));
        }
    }
    pane_at_index(workspace, 0)
}

fn surface_location_by_id(
    snapshot: &AppSessionSnapshot,
    panel_id: &str,
) -> Option<(usize, String)> {
    snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .find(|(_, workspace)| {
            surfaces_for_workspace(workspace)
                .iter()
                .any(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id))
        })
        .map(|(workspace_index, _)| (workspace_index, panel_id.to_string()))
}

fn surface_location_from_keys(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    ref_keys: &[&str],
    id_keys: &[&str],
    reference_workspace_index: usize,
) -> Option<(usize, String)> {
    if string_param(params, ref_keys).is_some() {
        let workspace = snapshot
            .windows
            .first()?
            .tab_manager
            .workspaces
            .get(reference_workspace_index)?;
        return surface_id_from_selector_keys(workspace, params, ref_keys, id_keys)
            .map(|panel_id| (reference_workspace_index, panel_id));
    }
    let panel_id = string_param(params, id_keys)?;
    surface_location_by_id(snapshot, &panel_id)
}

fn resolve_surface_move(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<SurfaceMoveResolution, SurfaceMoveResolveError> {
    let window = snapshot
        .windows
        .first()
        .ok_or(SurfaceMoveResolveError::SourceNotFound)?;
    let selected_workspace_index = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < window.tab_manager.workspaces.len())
        .unwrap_or(0);
    let (source_workspace_index, panel_id) = surface_location_from_keys(
        snapshot,
        params,
        &["surface_ref"],
        &["surface_id", "panel_id"],
        selected_workspace_index,
    )
    .ok_or(SurfaceMoveResolveError::SourceNotFound)?;
    let source_workspace = &window.tab_manager.workspaces[source_workspace_index];
    let (source_pane_index, source_pane_id, _) = surface_pane_details(source_workspace, &panel_id)
        .ok_or(SurfaceMoveResolveError::SourceNotFound)?;
    let source_pane_id = source_pane_id.ok_or(SurfaceMoveResolveError::DestinationNotFound)?;

    let before_specified = ["before_surface_ref", "before_surface_id"]
        .iter()
        .any(|key| params.contains_key(*key));
    let after_specified = ["after_surface_ref", "after_surface_id"]
        .iter()
        .any(|key| params.contains_key(*key));
    if before_specified && after_specified {
        return Err(SurfaceMoveResolveError::ConflictingAnchors);
    }

    let has_workspace = params.contains_key("workspace_ref") || params.contains_key("workspace_id");
    let requested_workspace_index = has_workspace
        .then(|| workspace_index_from_workspace_scope_or_selected(snapshot, params))
        .flatten();
    let anchor_reference_workspace = requested_workspace_index.unwrap_or(source_workspace_index);
    let anchor = if before_specified {
        surface_location_from_keys(
            snapshot,
            params,
            &["before_surface_ref"],
            &["before_surface_id"],
            anchor_reference_workspace,
        )
        .map(|location| (location, false))
    } else if after_specified {
        surface_location_from_keys(
            snapshot,
            params,
            &["after_surface_ref"],
            &["after_surface_id"],
            anchor_reference_workspace,
        )
        .map(|location| (location, true))
    } else {
        None
    };

    let (target_workspace_index, _target_pane_index, target_pane_id, destination_index) =
        if before_specified || after_specified {
            let ((workspace_index, anchor_id), after_anchor) =
                anchor.ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            let workspace = &window.tab_manager.workspaces[workspace_index];
            let (pane_index, pane_id, anchor_index) =
                surface_pane_details(workspace, &anchor_id)
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            (
                workspace_index,
                pane_index,
                pane_id.ok_or(SurfaceMoveResolveError::DestinationNotFound)?,
                Some(anchor_index as i64 + i64::from(after_anchor)),
            )
        } else if params.contains_key("pane_ref") || params.contains_key("pane_id") {
            if let Some(pane_id) = string_param(params, &["pane_id"]) {
                let (workspace_index, pane_index, pane_id) =
                    pane_location_by_id(snapshot, &pane_id)
                        .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                (
                    workspace_index,
                    pane_index,
                    pane_id,
                    i64_param(params, &["index"]),
                )
            } else {
                let reference = string_param(params, &["pane_ref"])
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                let pane_index = one_based_ref_index(&reference, "pane")
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                let workspace_index = requested_workspace_index.unwrap_or(source_workspace_index);
                let workspace = window
                    .tab_manager
                    .workspaces
                    .get(workspace_index)
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                let (pane_index, pane_id) = pane_at_index(workspace, pane_index)
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                (
                    workspace_index,
                    pane_index,
                    pane_id,
                    i64_param(params, &["index"]),
                )
            }
        } else if has_workspace {
            let workspace_index =
                requested_workspace_index.ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            let workspace = &window.tab_manager.workspaces[workspace_index];
            let (pane_index, pane_id) = first_or_focused_pane(workspace)
                .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            (
                workspace_index,
                pane_index,
                pane_id,
                i64_param(params, &["index"]),
            )
        } else if params.contains_key("window_ref") || params.contains_key("window_id") {
            if !workspace_reorder_window_matches(snapshot, params) {
                return Err(SurfaceMoveResolveError::DestinationNotFound);
            }
            let workspace = &window.tab_manager.workspaces[selected_workspace_index];
            let (pane_index, pane_id) = first_or_focused_pane(workspace)
                .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            (
                selected_workspace_index,
                pane_index,
                pane_id,
                i64_param(params, &["index"]),
            )
        } else {
            (
                source_workspace_index,
                source_pane_index,
                source_pane_id,
                i64_param(params, &["index"]),
            )
        };

    Ok(SurfaceMoveResolution {
        source_workspace_index,
        panel_id,
        target_workspace_index,
        target_pane_id,
        destination_index,
        focus: bool_param(params, &["focus"]).unwrap_or(false),
    })
}

fn surface_move(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let resolution = match resolve_surface_move(&current, params) {
        Ok(resolution) => resolution,
        Err(SurfaceMoveResolveError::ConflictingAnchors) => {
            return invalid_params("Specify at most one of before_surface_id or after_surface_id");
        }
        Err(SurfaceMoveResolveError::SourceNotFound) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: None,
            };
        }
        Err(SurfaceMoveResolveError::DestinationNotFound) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Destination pane not found".to_string(),
                data: None,
            };
        }
    };
    let state = app.state::<SessionState>();
    let result = match move_surface_for_control(
        app,
        &state,
        resolution.source_workspace_index,
        &resolution.panel_id,
        resolution.target_workspace_index,
        &resolution.target_pane_id,
        resolution.destination_index,
        resolution.focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(SurfacePositionControlError::InvalidRequest)) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: "Failed to move surface".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let window = &result.windows[0];
    let workspace = &window.tab_manager.workspaces[resolution.target_workspace_index];
    let Some((pane_index, pane_id, _)) = surface_pane_details(workspace, &resolution.panel_id)
    else {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: "Moved surface unavailable".to_string(),
            data: None,
        };
    };
    let surface_ref_value = surfaces_for_workspace(workspace)
        .iter()
        .position(|surface| {
            surface.get("id").and_then(Value::as_str) == Some(resolution.panel_id.as_str())
        })
        .map(surface_ref);
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(resolution.target_workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
        "surface_id": resolution.panel_id,
        "surface_ref": surface_ref_value,
    }))
}

fn surface_reorder(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    if !workspace_reorder_window_matches(&current, params) {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    }
    let Some((workspace_index, panel_id)) = surface_reorder_source(&current, params) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Surface not found".to_string(),
            data: None,
        };
    };
    let workspace = &current.windows[0].tab_manager.workspaces[workspace_index];
    let Some((pane_index, pane_id, _source_index)) = surface_pane_details(workspace, &panel_id)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Surface not found".to_string(),
            data: None,
        };
    };
    let index = i64_param(params, &["index"]);
    let before = surface_id_from_selector_keys(
        workspace,
        params,
        &["before_surface_ref"],
        &["before_surface_id"],
    );
    let after = surface_id_from_selector_keys(
        workspace,
        params,
        &["after_surface_ref"],
        &["after_surface_id"],
    );
    let target_count =
        usize::from(index.is_some()) + usize::from(before.is_some()) + usize::from(after.is_some());
    if target_count != 1 {
        return invalid_params(
            "Specify exactly one of index, before_surface_id, or after_surface_id",
        );
    }
    let destination_index = if let Some(index) = index {
        index
    } else {
        let (anchor, after_anchor) = match (before.as_deref(), after.as_deref()) {
            (Some(anchor), None) => (anchor, false),
            (None, Some(anchor)) => (anchor, true),
            _ => {
                return invalid_params(
                    "Specify exactly one of index, before_surface_id, or after_surface_id",
                );
            }
        };
        let Some((anchor_pane_index, _, anchor_index)) = surface_pane_details(workspace, anchor)
        else {
            return invalid_params("Anchor surface must be in the same pane");
        };
        if anchor_pane_index != pane_index {
            return invalid_params("Anchor surface must be in the same pane");
        }
        anchor_index as i64 + i64::from(after_anchor)
    };
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    let result = match reorder_surface_for_control(
        app,
        &state,
        workspace_index,
        &panel_id,
        destination_index,
        focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(SurfacePositionControlError::InvalidRequest)) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: "Failed to reorder surface".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let window = &result.windows[0];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let surface_ref_value = surfaces_for_workspace(workspace)
        .iter()
        .position(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id.as_str()))
        .map(surface_ref);
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_value,
    }))
}

fn surface_reorder_source(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<(usize, String)> {
    let window = snapshot.windows.first()?;
    let has_workspace_scope =
        params.contains_key("workspace_id") || params.contains_key("workspace_ref");
    if has_workspace_scope || params.contains_key("surface_ref") {
        let workspace_index = workspace_index_from_workspace_scope_or_selected(snapshot, params)?;
        let workspace = window.tab_manager.workspaces.get(workspace_index)?;
        return surface_id_from_selector_keys(
            workspace,
            params,
            &["surface_ref"],
            &["surface_id", "panel_id"],
        )
        .map(|panel_id| (workspace_index, panel_id));
    }
    let surface_id = string_param(params, &["surface_id", "panel_id"])?;
    window
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .find(|(_, workspace)| {
            surfaces_for_workspace(workspace).iter().any(|surface| {
                surface.get("id").and_then(Value::as_str) == Some(surface_id.as_str())
            })
        })
        .map(|(index, _)| (index, surface_id))
}

fn surface_id_from_selector_keys(
    workspace: &SessionWorkspaceSnapshot,
    params: &serde_json::Map<String, Value>,
    ref_keys: &[&str],
    id_keys: &[&str],
) -> Option<String> {
    let surfaces = surfaces_for_workspace(workspace);
    if let Some(reference) = string_param(params, ref_keys) {
        let index = one_based_ref_index(&reference, "surface")?;
        return surfaces
            .get(index)
            .and_then(|surface| surface.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    let id = string_param(params, id_keys)?;
    surfaces
        .iter()
        .any(|surface| surface.get("id").and_then(Value::as_str) == Some(id.as_str()))
        .then_some(id)
}

fn surface_pane_details(
    workspace: &SessionWorkspaceSnapshot,
    panel_id: &str,
) -> Option<(usize, Option<String>, usize)> {
    fn visit(
        layout: &SessionWorkspaceLayoutSnapshot,
        panel_id: &str,
        pane_index: &mut usize,
    ) -> Option<(usize, Option<String>, usize)> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                let current = *pane_index;
                *pane_index += 1;
                pane.panel_ids
                    .iter()
                    .position(|id| id == panel_id)
                    .map(|index| (current, pane.pane_id.clone(), index))
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit(&split.first, panel_id, pane_index)
                    .or_else(|| visit(&split.second, panel_id, pane_index))
            }
        }
    }
    let mut pane_index = 0;
    visit(workspace.layout.as_ref()?, panel_id, &mut pane_index)
}

fn surface_report_ports(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(ports) = ports_param(params) else {
        return invalid_params("Missing or invalid listening ports");
    };
    surface_set_ports(app, params, &ports)
}

fn surface_report_tty(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(tty) = string_param(params, &["tty", "tty_name", "ttyName", "name"]) else {
        return invalid_params("Missing TTY name");
    };
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_tty_for_control(app, &state, workspace_index, &panel_id, &tty) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_report_shell_state(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(shell_activity) = shell_activity_param(params) else {
        return invalid_params("state must be prompt, running, or unknown");
    };
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_shell_activity_for_control(
        app,
        &state,
        workspace_index,
        &panel_id,
        shell_activity,
    ) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_clear_ports(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    surface_set_ports(app, params, &[])
}

fn surface_set_ports(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    ports: &[u16],
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_listening_ports_for_control(app, &state, workspace_index, &panel_id, ports) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_ports_kick(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let snapshot = snapshot(app);
    let (workspace_index, panel_id) = match surface_ports_kick_target(&snapshot, params) {
        Ok(target) => target,
        Err(message) => return invalid_params(message),
    };
    let terminal_state = app.state::<TerminalState>();
    let session_state = app.state::<SessionState>();
    let scan = scan_panel_listening_ports(
        app,
        terminal_state.inner(),
        session_state.inner(),
        &panel_id,
    );
    let (scanner, ports, error) = match scan {
        Ok(result) => ("pid-tree", result.ports, None),
        Err(error) => ("unavailable", Vec::new(), Some(error)),
    };
    let agent_refresh = refresh_workspace_agent_ports(app, &snapshot, workspace_index);
    let (agent_ports, agent_error) = match agent_refresh {
        Ok(snapshot) => {
            let ports = snapshot
                .windows
                .first()
                .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
                .and_then(|workspace| workspace.agent_listening_ports.clone())
                .unwrap_or_default();
            (ports, None)
        }
        Err(error) => (Vec::new(), Some(error)),
    };
    ok(json!({
        "accepted": true,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "reason": string_param(params, &["reason"]).unwrap_or_else(|| "command".to_string()),
        "scanner": scanner,
        "listening_ports": ports,
        "agent_listening_ports": agent_ports,
        "error": error,
        "agent_error": agent_error,
    }))
}

fn refresh_workspace_agent_ports(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
) -> Result<AppSessionSnapshot, String> {
    let workspace = snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
        .ok_or_else(|| "workspace not found".to_string())?;
    let root_pids: Vec<u32> = workspace
        .agent_pids
        .as_ref()
        .into_iter()
        .flat_map(|entries| entries.iter())
        .map(|entry| entry.pid)
        .collect();
    let mut ports = Vec::new();
    for root_pid in root_pids {
        ports.extend(scan_listening_ports_for_root_pid(root_pid)?);
    }
    ports.sort_unstable();
    ports.dedup();
    let state = app.state::<SessionState>();
    set_workspace_agent_listening_ports_for_control(app, &state, workspace_index, &ports)
}

fn surface_ports_kick_target(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<(usize, String), &'static str> {
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(snapshot, params)
    else {
        return Err("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(snapshot, workspace_index, params)
    else {
        return Err("Missing or invalid surface selector");
    };
    Ok((workspace_index, panel_id))
}

fn surface_focus(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(workspace_id) = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
        .and_then(|workspace| workspace.workspace_id.clone())
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    let (changed, _snapshot) = match select_workspace_surface(app, &state, &workspace_id, &panel_id)
    {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(error)) => match error {},
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    ok(json!({
        "accepted": true,
        "changed": changed,
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
    }))
}

fn surface_health(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window) = current.windows.first() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let surfaces = surfaces_for_workspace(workspace)
        .into_iter()
        .map(|surface| {
            let mut surface = surface.as_object().cloned().unwrap_or_default();
            surface.insert("in_window".to_string(), json!(true));
            surface.insert("healthy".to_string(), json!(true));
            Value::Object(surface)
        })
        .collect::<Vec<_>>();
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surfaces": surfaces,
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

fn terminal_workspace_index(
    current: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<usize, ControlCallResult> {
    if current.windows.is_empty() {
        return Err(ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        });
    }
    workspace_index_from_workspace_scope_or_selected(current, params).ok_or_else(|| {
        ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        }
    })
}

fn terminal_panel_id(
    current: &AppSessionSnapshot,
    workspace_index: usize,
    params: &serde_json::Map<String, Value>,
) -> Result<String, ControlCallResult> {
    let panel_id = surface_id_from_params_or_workspace_focused(current, workspace_index, params)
        .ok_or_else(|| ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No focused surface".to_string(),
            data: None,
        })?;
    if !surface_is_terminal(current, workspace_index, &panel_id) {
        return Err(ControlCallResult::Err {
            code: "invalid_params".to_string(),
            message: "Surface is not a terminal".to_string(),
            data: Some(
                json!({"surface_id": panel_id})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        });
    }
    Ok(panel_id)
}

fn surface_read_text(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let workspace_index = match terminal_workspace_index(&current, params) {
        Ok(index) => index,
        Err(error) => return error,
    };
    let line_limit = if params.contains_key("lines") {
        match usize_param(params, &["lines"]) {
            Some(lines) if lines > 0 => Some(lines),
            _ => return invalid_params("lines must be greater than 0"),
        }
    } else {
        None
    };
    let panel_id = match terminal_panel_id(&current, workspace_index, params) {
        Ok(panel_id) => panel_id,
        Err(error) => return error,
    };
    let window = &current.windows[0];
    let include_scrollback =
        bool_param(params, &["scrollback"]).unwrap_or(false) || line_limit.is_some();
    let terminal_state = app.state::<TerminalState>();
    let text = match terminal_read_panel(
        terminal_state.inner(),
        &panel_id,
        include_scrollback,
        line_limit,
    ) {
        Ok(text) => text,
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: Some(
                    json!({"surface_id": panel_id.clone()})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    ok(json!({
        "text": text,
        "base64": BASE64_STANDARD.encode(text.as_bytes()),
        "workspace_id": window.tab_manager.workspaces[workspace_index].workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

fn surface_clear_history(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let workspace_index = match terminal_workspace_index(&current, params) {
        Ok(index) => index,
        Err(error) => return error,
    };
    let panel_id = match terminal_panel_id(&current, workspace_index, params) {
        Ok(panel_id) => panel_id,
        Err(error) => return error,
    };
    let window = &current.windows[0];
    let terminal_state = app.state::<TerminalState>();
    if let Err(message) = terminal_clear_history_panel(terminal_state.inner(), &panel_id) {
        return ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: Some(
                json!({"surface_id": panel_id.clone()})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }
    ok(json!({
        "workspace_id": window.tab_manager.workspaces[workspace_index].workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

fn surface_trigger_flash(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window) = current.windows.first() else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let payload = json!({"panelId": panel_id.clone()});
    if let Err(error) = app.emit(PANEL_FLASH_EVENT, payload) {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Failed to emit panel flash event: {error}"),
            data: None,
        };
    }
    ok(json!({
        "workspace_id": window.tab_manager.workspaces[workspace_index].workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

fn surface_refresh_all(app: &AppHandle) -> ControlCallResult {
    if let Err(error) = app.emit(SURFACE_REFRESH_EVENT, json!({"refresh": true})) {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Failed to emit surface refresh event: {error}"),
            data: None,
        };
    }
    ok(json!({
        "accepted": true,
        "event": SURFACE_REFRESH_EVENT,
    }))
}

fn surface_send_text(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(text) = raw_string_param(params, &["text"]) else {
        return invalid_params("Missing text");
    };
    surface_send_input(app, params, &text)
}

fn surface_send_key(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(key) = string_param(params, &["key"]) else {
        return invalid_params("Missing key");
    };
    let Some(sequence) = terminal_key_sequence(&key) else {
        return ControlCallResult::Err {
            code: "invalid_params".to_string(),
            message: "Unknown key".to_string(),
            data: Some(json!({"key": key}).try_into().unwrap_or(JsonValue::Null)),
        };
    };
    surface_send_input(app, params, sequence)
}

fn surface_send_input(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    data: &str,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_terminal(&current, workspace_index, &panel_id) {
        return ControlCallResult::Err {
            code: "invalid_params".to_string(),
            message: "Surface is not a terminal".to_string(),
            data: Some(
                json!({"surface_id": panel_id.clone()})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }
    let terminal_state = app.state::<TerminalState>();
    if let Err(message) = terminal_write_panel(terminal_state.inner(), &panel_id, data) {
        return ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: Some(
                json!({"surface_id": panel_id.clone()})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }
    ok(json!({
        "workspace_id": current.windows[0].tab_manager.workspaces[workspace_index].workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
        "queued": false,
        "window_id": current.windows[0].window_id,
        "window_ref": current.windows[0].window_id.as_ref().map(|_| "window:1"),
    }))
}

fn surface_move_to_new_workspace(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match move_panel_to_new_workspace_for_control(app, &state, &panel_id) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn surface_open_browser(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let url = string_param(params, &["url"]);
    let state = app.state::<SessionState>();
    match open_browser_url_in_panel(app, &state, &panel_id, url.as_deref()) {
        Some(snapshot) => surface_list_from_params(&snapshot, params),
        None => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to open browser in pane {panel_id}"),
            data: None,
        },
    }
}

fn browser_open_split(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(orientation) = split_orientation_from_params(params) else {
        return invalid_params("Invalid split orientation");
    };
    let Some(workspace) = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let before_surface_ids = surfaces_for_workspace(workspace)
        .iter()
        .filter_map(|surface| surface.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    let url = raw_string_param(params, &["url"]);
    let state = app.state::<SessionState>();
    match split_browser_for_control(
        app,
        &state,
        &panel_id,
        orientation,
        insert_first_param(params),
        url.as_deref(),
    ) {
        Ok(snapshot) => {
            let Some(new_panel_id) =
                new_browser_surface_id(&snapshot, workspace_index, &before_surface_ids)
            else {
                return surface_list_from_params(&snapshot, params);
            };
            match browser_surface_payload(&snapshot, workspace_index, &new_panel_id) {
                Some(payload) => ok(payload),
                None => surface_list_from_params(&snapshot, params),
            }
        }
        Err(message) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_navigate(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(url) = raw_string_param(params, &["url"]) else {
        return invalid_params("Missing browser URL");
    };
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.navigate requires a browser surface");
    }
    let state = app.state::<SessionState>();
    match open_browser_url_in_panel(app, &state, &panel_id, Some(&url)) {
        Some(snapshot) => match browser_surface_payload(&snapshot, workspace_index, &panel_id) {
            Some(payload) => ok(payload),
            None => invalid_params("Missing or invalid surface selector"),
        },
        None => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to navigate browser surface {panel_id}"),
            data: None,
        },
    }
}

fn surface_open_markdown(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(file_path) = string_param(params, &["file_path", "path"]) else {
        return invalid_params("Missing markdown file path");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match open_markdown_file_in_panel(app, &state, &panel_id, &file_path) {
        Some(snapshot) => surface_list_from_params(&snapshot, params),
        None => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to open markdown file in pane {panel_id}"),
            data: None,
        },
    }
}

fn surface_open_file(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(file_path) = string_param(params, &["file_path", "path"]) else {
        return invalid_params("Missing file path");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match open_file_in_panel(app, &state, &panel_id, &file_path) {
        Some(snapshot) => surface_list_from_params(&snapshot, params),
        None => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to open file in pane {panel_id}"),
            data: None,
        },
    }
}

fn surface_open_diff(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let diff_state = app.state::<DiffState>();
    let (token, request_path) = match string_param(params, &["token", "diff_token"]) {
        Some(token) => (
            token,
            string_param(params, &["request_path", "path"])
                .unwrap_or_else(|| "/index.html".to_string()),
        ),
        None => match diff_state.create_starter_session(SystemTime::now()) {
            Ok(created) => (created.token, created.request_path),
            Err(message) => {
                return ControlCallResult::Err {
                    code: "internal_error".to_string(),
                    message,
                    data: None,
                }
            }
        },
    };
    let state = app.state::<SessionState>();
    match open_diff_viewer_in_panel(app, &state, &diff_state, &panel_id, &token, &request_path) {
        Some(snapshot) => surface_list_from_params(&snapshot, params),
        None => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to open diff viewer in pane {panel_id}"),
            data: None,
        },
    }
}

fn surface_select_adjacent(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    next: bool,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    let snapshot = match select_adjacent_panel_for_control(app, &state, &panel_id, next) {
        Ok(snapshot) => snapshot,
        Err(PaneTopologyControlError::Operation(error)) => match error {},
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    surface_list_from_params(&snapshot, params)
}

fn surface_toggle_split_zoom(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match toggle_split_zoom_for_control(app, &state, &panel_id) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_back(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    surface_list_from_params(&browser_go_back_for_control(app, &state, &panel_id), params)
}

fn browser_forward(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    surface_list_from_params(
        &browser_go_forward_for_control(app, &state, &panel_id),
        params,
    )
}

fn browser_reload(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.reload requires a browser surface");
    }
    let state = app.state::<BrowserWebviewState>();
    let reply = match browser_webview_command_for_control(state.inner(), &panel_id, "reload") {
        Ok(reply) => reply,
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    let Some(mut payload) = browser_surface_payload(&current, workspace_index, &panel_id)
        .and_then(|value| value.as_object().cloned())
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    payload.insert("reloaded".to_string(), json!(true));
    payload.insert("webview_attached".to_string(), json!(reply.attached));
    ok(Value::Object(payload))
}

fn browser_url_get(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.url.get requires a browser surface");
    }
    match browser_surface_payload(&current, workspace_index, &panel_id) {
        Some(payload) => ok(payload),
        None => invalid_params("Missing or invalid surface selector"),
    }
}

fn browser_focus_webview(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.focus_webview requires a browser surface");
    }
    let Some(workspace_id) = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
        .and_then(|workspace| workspace.workspace_id.clone())
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let session_state = app.state::<SessionState>();
    let (_changed, snapshot) =
        match select_workspace_surface(app, &session_state, &workspace_id, &panel_id) {
            Ok(result) => result,
            Err(PaneTopologyControlError::Operation(error)) => match error {},
            Err(PaneTopologyControlError::Publication(message)) => {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
        };
    let browser_state = app.state::<BrowserWebviewState>();
    let reply = match browser_webview_command_for_control(browser_state.inner(), &panel_id, "focus")
    {
        Ok(reply) => reply,
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    let Some(mut payload) = browser_surface_payload(&snapshot, workspace_index, &panel_id)
        .and_then(|value| value.as_object().cloned())
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    payload.insert("focused".to_string(), json!(true));
    payload.insert("webview_attached".to_string(), json!(reply.attached));
    payload.insert("focus_scope".to_string(), json!("webview"));
    ok(Value::Object(payload))
}

fn browser_is_webview_focused(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.is_webview_focused requires a browser surface");
    }
    let Some(mut payload) = browser_surface_payload(&current, workspace_index, &panel_id)
        .and_then(|value| value.as_object().cloned())
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let focused = payload
        .get("surface")
        .and_then(Value::as_object)
        .and_then(|surface| surface.get("focused"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    payload.insert("focused".to_string(), json!(focused));
    payload.insert("focus_scope".to_string(), json!("surface"));
    payload.insert("webview_focus_verified".to_string(), json!(false));
    ok(Value::Object(payload))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserGetter {
    Text,
    Html,
    Value,
    Attr,
    Title,
    Count,
    Box,
    Styles,
    Visible,
    Enabled,
    Checked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserAction {
    Click,
    DblClick,
    Hover,
    Focus,
    Type,
    Fill,
    Press,
    KeyDown,
    KeyUp,
    Check,
    Uncheck,
    Select,
    Scroll,
    ScrollIntoView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserLocator {
    Role,
    Text,
    Label,
    Placeholder,
    Alt,
    Title,
    TestId,
    First,
    Last,
    Nth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserCookieAction {
    Get,
    Set,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserStorageAction {
    Get,
    Set,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserConsoleAction {
    List,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserDialogAction {
    Accept,
    Dismiss,
}

fn browser_snapshot(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.snapshot") else {
        return invalid_params("browser.snapshot requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, browser_snapshot_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => {
                let snapshot = value
                    .get("snapshot")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let refs = value.get("refs").cloned().unwrap_or_else(|| json!({}));
                ok(json!({
                    "surface_id": panel_id,
                    "panel_id": panel_id,
                    "snapshot": snapshot,
                    "refs": refs,
                }))
            }
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_screenshot(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.screenshot") else {
        return invalid_params("browser.screenshot requires a browser surface");
    };
    let value = match run_browser_eval_script(app, &panel_id, browser_screenshot_probe_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => value,
            Err(message) => {
                return ControlCallResult::Err {
                    code: "javascript_error".to_string(),
                    message,
                    data: None,
                }
            }
        },
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    let width = value
        .get("width")
        .and_then(Value::as_u64)
        .unwrap_or(1024)
        .clamp(1, 2048) as u32;
    let height = value
        .get("height")
        .and_then(Value::as_u64)
        .unwrap_or(768)
        .clamp(1, 2048) as u32;
    let rgb = [
        value
            .get("r")
            .and_then(Value::as_u64)
            .unwrap_or(255)
            .min(255) as u8,
        value
            .get("g")
            .and_then(Value::as_u64)
            .unwrap_or(255)
            .min(255) as u8,
        value
            .get("b")
            .and_then(Value::as_u64)
            .unwrap_or(255)
            .min(255) as u8,
    ];
    let png = solid_png(width, height, rgb);
    let png_base64 = BASE64_STANDARD.encode(&png);
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    payload.insert("png_base64".to_string(), json!(png_base64));
    payload.insert("mime".to_string(), json!("image/png"));
    payload.insert("width".to_string(), json!(width));
    payload.insert("height".to_string(), json!(height));
    payload.insert("captureMode".to_string(), json!("dom-background-raster"));
    if let Some(path) = raw_string_param(params, &["path", "out", "file_path", "filePath"]) {
        if let Err(error) = fs::write(&path, &png) {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Could not write browser screenshot to {path}: {error}"),
                data: None,
            };
        }
        payload.insert("path".to_string(), json!(path));
    }
    ok(Value::Object(payload))
}

fn browser_eval(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(script) = raw_string_param(params, &["script", "expression"]) else {
        return invalid_params("browser.eval requires a script");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.eval") else {
        return invalid_params("browser.eval requires a browser surface");
    };
    let wrapped_script = browser_eval_wrapper_script(&script);
    match run_browser_eval_script(app, &panel_id, &wrapped_script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "value": value,
            })),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_add_init_script(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(script) = raw_string_param(params, &["script"]) else {
        return invalid_params("browser.addinitscript requires a script");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.addinitscript") else {
        return invalid_params("browser.addinitscript requires a browser surface");
    };
    let browser_state = app.state::<BrowserWebviewState>();
    match browser_add_init_script_for_control(app, browser_state.inner(), &panel_id, &script) {
        Ok(reply) => ok(json!({
            "surface_id": panel_id,
            "panel_id": panel_id,
            "added": true,
            "webview": reply,
        })),
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_wait(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.wait") else {
        return invalid_params("browser.wait requires a browser surface");
    };
    let timeout_ms = usize_param(params, &["timeout_ms", "timeoutMs"])
        .unwrap_or(5_000)
        .clamp(1, 60_000) as u64;
    let condition = if let Some(selector) = raw_string_param(params, &["selector"]) {
        browser_wait_selector_script(&selector)
    } else if let Some(text) = raw_string_param(params, &["text_contains", "textContains", "text"])
    {
        browser_wait_text_script(&text)
    } else if let Some(expression) = raw_string_param(params, &["function", "expression"]) {
        browser_wait_function_script(&expression)
    } else if let Some(load_state) = string_param(params, &["load_state", "loadState"]) {
        browser_wait_load_state_script(&load_state)
    } else if let Some(url_contains) = raw_string_param(params, &["url_contains", "urlContains"]) {
        browser_wait_url_script(&url_contains)
    } else {
        return invalid_params(
            "browser.wait requires selector, text_contains, function, load_state, or url_contains",
        );
    };
    let deadline = SystemTime::now() + Duration::from_millis(timeout_ms);
    let mut last_error = String::new();
    while SystemTime::now() < deadline {
        match run_browser_eval_script(app, &panel_id, &condition) {
            Ok(value) => match unwrap_browser_eval_result(value) {
                Ok(value) if value.as_bool().unwrap_or(false) => {
                    return ok(json!({
                        "surface_id": panel_id,
                        "panel_id": panel_id,
                        "value": true,
                    }));
                }
                Ok(_) => {}
                Err(message) => last_error = message,
            },
            Err(message) => {
                return ControlCallResult::Err {
                    code: "surface_unavailable".to_string(),
                    message,
                    data: None,
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    ControlCallResult::Err {
        code: "timeout".to_string(),
        message: if last_error.is_empty() {
            format!("browser.wait timed out after {timeout_ms}ms")
        } else {
            format!("browser.wait timed out after {timeout_ms}ms: {last_error}")
        },
        data: None,
    }
}

fn browser_action(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_action_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_action_method(action)
        ));
    };
    let selector = raw_string_param(params, &["selector"]);
    if browser_action_requires_selector(action) && selector.as_deref().is_none_or(str::is_empty) {
        return invalid_params(&format!(
            "{} requires a selector",
            browser_action_method(action)
        ));
    }
    let text = raw_string_param(params, &["text", "value"]).unwrap_or_default();
    let value = raw_string_param(params, &["value"]).unwrap_or_default();
    let key = raw_string_param(params, &["key"]).unwrap_or_default();
    let dx = f64_param(params, &["dx"]).unwrap_or(0.0);
    let dy = f64_param(params, &["dy"]).unwrap_or(0.0);
    let script = browser_action_script(action, selector.as_deref(), &text, &value, &key, dx, dy);
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => {
                let mut payload = serde_json::Map::new();
                payload.insert("surface_id".to_string(), json!(panel_id));
                payload.insert("panel_id".to_string(), json!(panel_id));
                payload.insert("value".to_string(), value);
                if bool_param(params, &["snapshot_after", "snapshotAfter"]).unwrap_or(false) {
                    if let Ok(snapshot) =
                        run_browser_eval_script(app, &panel_id, browser_snapshot_script())
                            .and_then(unwrap_browser_eval_result)
                    {
                        payload.insert("post_action_snapshot".to_string(), snapshot);
                    }
                }
                ok(Value::Object(payload))
            }
            Err(message) => ControlCallResult::Err {
                code: if message.contains("No element matches selector") {
                    "not_found".to_string()
                } else {
                    "javascript_error".to_string()
                },
                message: browser_not_found_message(&message),
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_find(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    locator: BrowserLocator,
) -> ControlCallResult {
    let panel_id = if locator == BrowserLocator::Nth {
        let mut surface_params = params.clone();
        surface_params.remove("index");
        browser_automation_panel_id(app, &surface_params, browser_locator_method(locator))
    } else {
        browser_automation_panel_id(app, params, browser_locator_method(locator))
    };
    let Some(panel_id) = panel_id else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_locator_method(locator)
        ));
    };
    let script = match browser_locator_script(locator, params) {
        Ok(script) => script,
        Err(message) => return invalid_params(&message),
    };
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_locator_payload(&panel_id, value)),
            Err(message) => ControlCallResult::Err {
                code: "not_found".to_string(),
                message: browser_not_found_message(&message),
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_frame_select(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(selector) = raw_string_param(params, &["selector"]) else {
        return invalid_params("browser.frame.select requires a selector");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.frame.select") else {
        return invalid_params("browser.frame.select requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_frame_select_script(&selector)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "selected": true,
                "frame": value,
            })),
            Err(message) => ControlCallResult::Err {
                code: "not_found".to_string(),
                message: browser_not_found_message(&message),
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_frame_main(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.frame.main") else {
        return invalid_params("browser.frame.main requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, browser_frame_main_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(_) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "selected": false,
            })),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_dialog(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserDialogAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_dialog_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_dialog_method(action)
        ));
    };
    let text = raw_string_param(params, &["text"]);
    match run_browser_eval_script(
        app,
        &panel_id,
        &browser_dialog_script(action, text.as_deref()),
    ) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_dialog_payload(&panel_id, action, value)),
            Err(message) => ControlCallResult::Err {
                code: "not_found".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_download_wait(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(path) = raw_string_param(params, &["path", "file_path", "filePath"]) else {
        return invalid_params("browser.download.wait requires a path");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.download.wait") else {
        return invalid_params("browser.download.wait requires a browser surface");
    };
    let timeout_ms = usize_param(params, &["timeout_ms", "timeoutMs"])
        .unwrap_or(30_000)
        .clamp(1, 300_000) as u64;
    let deadline = SystemTime::now() + Duration::from_millis(timeout_ms);
    while SystemTime::now() < deadline {
        if fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            return ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "path": path,
                "downloaded": true,
            }));
        }
        thread::sleep(Duration::from_millis(100));
    }
    ControlCallResult::Err {
        code: "timeout".to_string(),
        message: format!("browser.download.wait timed out after {timeout_ms}ms for {path}"),
        data: None,
    }
}

fn browser_addscript(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(script) = raw_string_param(params, &["script"]) else {
        return invalid_params("browser.addscript requires a script");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.addscript") else {
        return invalid_params("browser.addscript requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_eval_wrapper_script(&script)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "value": value,
            })),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_addstyle(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(css) = raw_string_param(params, &["css", "style"]) else {
        return invalid_params("browser.addstyle requires css");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.addstyle") else {
        return invalid_params("browser.addstyle requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_addstyle_script(&css)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "value": value,
                "added": value.as_bool().unwrap_or(true),
            })),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_cookies(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserCookieAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_cookie_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_cookie_method(action)
        ));
    };
    let name = string_param(params, &["name"]);
    let value = raw_string_param(params, &["value"]);
    if matches!(
        action,
        BrowserCookieAction::Set | BrowserCookieAction::Clear
    ) && name.is_none()
    {
        return invalid_params(&format!(
            "{} requires a cookie name",
            browser_cookie_method(action)
        ));
    }
    if action == BrowserCookieAction::Set && value.is_none() {
        return invalid_params("browser.cookies.set requires a cookie value");
    }
    let script = browser_cookie_script(action, name.as_deref(), value.as_deref());
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_cookie_payload(&panel_id, action, value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_storage(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserStorageAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_storage_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_storage_method(action)
        ));
    };
    let storage_type = string_param(params, &["type", "storage"])
        .unwrap_or_else(|| "local".to_string())
        .to_ascii_lowercase();
    if !matches!(storage_type.as_str(), "local" | "session") {
        return invalid_params("browser.storage type must be local or session");
    }
    let key = raw_string_param(params, &["key"]);
    let value = params.get("value").cloned().unwrap_or(Value::Null);
    if matches!(
        action,
        BrowserStorageAction::Get | BrowserStorageAction::Set
    ) && key.is_none()
    {
        return invalid_params(&format!(
            "{} requires a key",
            browser_storage_method(action)
        ));
    }
    if action == BrowserStorageAction::Set && value.is_null() {
        return invalid_params("browser.storage.set requires a value");
    }
    let script = browser_storage_script(action, &storage_type, key.as_deref(), &value);
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_storage_payload(&panel_id, action, value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_tab_list(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let tabs = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
        .map(surfaces_for_workspace)
        .unwrap_or_default()
        .into_iter()
        .filter(|surface| {
            surface
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "browser")
        })
        .map(|surface| {
            json!({
                "id": surface.get("id").cloned().unwrap_or(Value::Null),
                "surface_id": surface.get("id").cloned().unwrap_or(Value::Null),
                "title": surface.get("title").cloned().unwrap_or(Value::Null),
                "url": surface.get("browser_url").cloned().unwrap_or(Value::Null),
                "focused": surface.get("focused").cloned().unwrap_or(json!(false)),
            })
        })
        .collect::<Vec<_>>();
    ok(json!({
        "tabs": tabs,
        "count": tabs.len(),
    }))
}

fn browser_tab_new(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let mut open_params = params.clone();
    if !open_params.contains_key("url") {
        open_params.insert("url".to_string(), json!("about:blank"));
    }
    browser_open_split(app, &open_params)
}

fn browser_tab_switch(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(target_surface_id) = string_param(
        params,
        &[
            "target_surface_id",
            "targetSurfaceId",
            "target",
            "tab_id",
            "tabId",
        ],
    ) else {
        return invalid_params("browser.tab.switch requires a target surface id");
    };
    let mut focus_params = params.clone();
    focus_params.insert("surface_id".to_string(), json!(target_surface_id));
    surface_focus(app, &focus_params)
}

fn browser_tab_close(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(target_surface_id) = string_param(
        params,
        &[
            "target_surface_id",
            "targetSurfaceId",
            "target",
            "tab_id",
            "tabId",
        ],
    )
    .or_else(|| string_param(params, &["surface_id", "panel_id", "id"])) else {
        return invalid_params("browser.tab.close requires a target surface id");
    };
    let mut close_params = params.clone();
    close_params.insert("surface_id".to_string(), json!(target_surface_id));
    surface_close(app, &close_params)
}

fn browser_console(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserConsoleAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_console_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_console_method(action)
        ));
    };
    let script = match action {
        BrowserConsoleAction::List => browser_console_list_script(),
        BrowserConsoleAction::Clear => browser_console_clear_script(),
    };
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_console_payload(&panel_id, action, value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_errors_list(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.errors.list") else {
        return invalid_params("browser.errors.list requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_errors_list_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_errors_payload(&panel_id, value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_state_save(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(path) = raw_string_param(params, &["path", "file_path", "filePath"]) else {
        return invalid_params("browser.state.save requires a path");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.state.save") else {
        return invalid_params("browser.state.save requires a browser surface");
    };
    let state = match run_browser_eval_script(app, &panel_id, browser_state_capture_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => value,
            Err(message) => {
                return ControlCallResult::Err {
                    code: "javascript_error".to_string(),
                    message,
                    data: None,
                }
            }
        },
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    let encoded = match serde_json::to_string_pretty(&state) {
        Ok(encoded) => encoded,
        Err(error) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: format!("Could not serialize browser state: {error}"),
                data: None,
            }
        }
    };
    if let Err(error) = fs::write(&path, encoded) {
        return ControlCallResult::Err {
            code: "io_error".to_string(),
            message: format!("Could not write browser state to {path}: {error}"),
            data: None,
        };
    }
    ok(browser_state_payload(&panel_id, &path, "saved", state))
}

fn browser_state_load(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(path) = raw_string_param(params, &["path", "file_path", "filePath"]) else {
        return invalid_params("browser.state.load requires a path");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.state.load") else {
        return invalid_params("browser.state.load requires a browser surface");
    };
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) => {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Could not read browser state from {path}: {error}"),
                data: None,
            }
        }
    };
    let state: Value = match serde_json::from_str(&raw) {
        Ok(state) => state,
        Err(error) => {
            return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: format!("Browser state file is not valid JSON: {error}"),
                data: None,
            }
        }
    };
    match run_browser_eval_script(app, &panel_id, &browser_state_restore_script(&state)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_state_payload(&panel_id, &path, "loaded", value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_highlight(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(selector) = raw_string_param(params, &["selector"]) else {
        return invalid_params("browser.highlight requires a selector");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.highlight") else {
        return invalid_params("browser.highlight requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_highlight_script(&selector)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "highlighted": value.as_bool().unwrap_or(true),
                "value": value,
            })),
            Err(message) => ControlCallResult::Err {
                code: "not_found".to_string(),
                message: browser_not_found_message(&message),
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_get_title(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    browser_get_selector_value(app, params, BrowserGetter::Title)
}

fn browser_get_selector_value(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    getter: BrowserGetter,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_getter_method(getter))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_getter_method(getter)
        ));
    };
    let selector = raw_string_param(params, &["selector"]);
    if matches!(
        getter,
        BrowserGetter::Text
            | BrowserGetter::Html
            | BrowserGetter::Value
            | BrowserGetter::Attr
            | BrowserGetter::Box
            | BrowserGetter::Styles
            | BrowserGetter::Visible
            | BrowserGetter::Enabled
            | BrowserGetter::Checked
    ) && selector.as_deref().is_none_or(str::is_empty)
    {
        return invalid_params(&format!(
            "{} requires a selector",
            browser_getter_method(getter)
        ));
    }
    if getter == BrowserGetter::Count && selector.as_deref().is_none_or(str::is_empty) {
        return invalid_params("browser.get.count requires a selector");
    }
    let attr = if getter == BrowserGetter::Attr {
        match string_param(params, &["attribute", "attr", "name"]) {
            Some(attr) => Some(attr),
            None => return invalid_params("browser.get.attr requires an attribute name"),
        }
    } else if getter == BrowserGetter::Styles {
        string_param(params, &["property", "name"])
    } else {
        None
    };
    let script = browser_getter_script(getter, selector.as_deref(), attr.as_deref());
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_getter_payload(&panel_id, getter, value)),
            Err(message) => ControlCallResult::Err {
                code: if message.contains("No element matches selector") {
                    "not_found".to_string()
                } else {
                    "javascript_error".to_string()
                },
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_automation_panel_id(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    _method: &str,
) -> Option<String> {
    let current = snapshot(app);
    let workspace_index = workspace_index_from_workspace_scope_or_selected(&current, params)?;
    let panel_id = surface_id_from_params_or_workspace_focused(&current, workspace_index, params)?;
    surface_is_browser(&current, workspace_index, &panel_id).then_some(panel_id)
}

fn run_browser_eval_script(app: &AppHandle, panel_id: &str, script: &str) -> Result<Value, String> {
    let state = app.state::<BrowserWebviewState>();
    browser_eval_for_control(state.inner(), panel_id, script)
}

fn browser_eval_wrapper_script(script: &str) -> String {
    let encoded = serde_json::to_string(script).expect("serializing JS source cannot fail");
    let dialog_hook =
        serde_json::to_string(browser_dialog_hook_script()).expect("dialog hook JSON");
    format!(
        r#"(() => {{
  {context}
  (0, eval)({dialog_hook});
  const __cmuxScript = {encoded};
  try {{
    const value = Function('window', 'document', `return (function() {{ return eval(${{JSON.stringify(__cmuxScript)}}); }}).call(window);`)(window, document);
    return {{ ok: true, value }};
  }} catch (error) {{
    return {{
      ok: false,
      error: String((error && (error.stack || error.message)) || error)
    }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

fn browser_js_context_prelude() -> &'static str {
    r#"const __cmuxTopWindow = globalThis;
  const __cmuxFrameSelector = __cmuxTopWindow.__cmuxSelectedFrameSelector || '';
  const __cmuxFrameElement = __cmuxFrameSelector ? __cmuxTopWindow.document.querySelector(__cmuxFrameSelector) : null;
  const window = (__cmuxFrameElement && __cmuxFrameElement.contentWindow) || __cmuxTopWindow;
  const document = window.document;"#
}

fn browser_dialog_hook_script() -> &'static str {
    r#"(() => {
  if (!globalThis.__cmuxDialogCapture) {
    const state = { queue: [] };
    const push = (type, message, defaultValue) => {
      state.queue.push({ type, message: String(message || ''), defaultValue: defaultValue == null ? null : String(defaultValue), timestamp: Date.now() });
    };
    globalThis.alert = (message) => { push('alert', message, null); };
    globalThis.confirm = (message) => { push('confirm', message, null); return true; };
    globalThis.prompt = (message, defaultValue) => { push('prompt', message, defaultValue); return defaultValue == null ? '' : String(defaultValue); };
    globalThis.__cmuxDialogCapture = state;
  }
  return globalThis.__cmuxDialogCapture;
})()"#
}

fn browser_screenshot_probe_script() -> &'static str {
    r#"(() => {
  const __cmuxTopWindow = globalThis;
  const __cmuxFrameSelector = __cmuxTopWindow.__cmuxSelectedFrameSelector || '';
  const __cmuxFrameElement = __cmuxFrameSelector ? __cmuxTopWindow.document.querySelector(__cmuxFrameSelector) : null;
  const window = (__cmuxFrameElement && __cmuxFrameElement.contentWindow) || __cmuxTopWindow;
  const document = window.document;
  try {
    const parseColor = (value) => {
      const match = String(value || '').match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
      if (!match) { return null; }
      return { r: Number(match[1]), g: Number(match[2]), b: Number(match[3]) };
    };
    let element = document.body || document.documentElement;
    let color = null;
    while (element && !color) {
      color = parseColor(window.getComputedStyle(element).backgroundColor);
      if (color && (color.r !== 0 || color.g !== 0 || color.b !== 0)) { break; }
      element = element.parentElement;
    }
    color = color || { r: 255, g: 255, b: 255 };
    return {
      ok: true,
      value: {
        width: Math.max(1, Math.round(window.innerWidth || document.documentElement.clientWidth || 1024)),
        height: Math.max(1, Math.round(window.innerHeight || document.documentElement.clientHeight || 768)),
        r: color.r,
        g: color.g,
        b: color.b
      }
    };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
}

fn browser_frame_select_script(selector: &str) -> String {
    let encoded = serde_json::to_string(selector).expect("frame selector JSON is infallible");
    format!(
        r#"(() => {{
  const selector = {encoded};
  try {{
    const frame = document.querySelector(selector);
    if (!frame || !frame.contentWindow || !frame.contentWindow.document) {{
      throw new Error(`No accessible frame matches selector: ${{selector}}`);
    }}
    window.__cmuxSelectedFrameSelector = selector;
    return {{ ok: true, value: {{ selector, url: String(frame.contentWindow.location.href || '') }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

fn browser_frame_main_script() -> &'static str {
    r#"(() => {
  try {
    delete window.__cmuxSelectedFrameSelector;
    return { ok: true, value: true };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
}

fn browser_dialog_script(action: BrowserDialogAction, text: Option<&str>) -> String {
    let hook = serde_json::to_string(browser_dialog_hook_script()).expect("dialog hook JSON");
    let encoded_text = serde_json::to_string(text.unwrap_or_default()).expect("dialog text JSON");
    let accepted = action == BrowserDialogAction::Accept;
    format!(
        r#"(() => {{
  try {{
    (0, eval)({hook});
    const text = {encoded_text};
    const queue = globalThis.__cmuxDialogCapture.queue;
    const dialog = queue.shift();
    if (!dialog) {{ throw new Error('No pending browser dialog'); }}
    return {{ ok: true, value: {{ ...dialog, accepted: {accepted}, text }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

fn browser_snapshot_script() -> &'static str {
    r#"(() => {
  try {
    const refs = {};
    const lines = ['- document ' + JSON.stringify(document.title || '')];
    const cssPath = (element) => {
      if (!element || element.nodeType !== 1) { return ''; }
      if (element.id) { return '#' + CSS.escape(element.id); }
      const parts = [];
      let current = element;
      while (current && current.nodeType === 1 && current !== document.documentElement) {
        let part = current.tagName.toLowerCase();
        if (current.classList && current.classList.length) {
          part += '.' + Array.from(current.classList).slice(0, 2).map((name) => CSS.escape(name)).join('.');
        }
        const parent = current.parentElement;
        if (parent) {
          const siblings = Array.from(parent.children).filter((child) => child.tagName === current.tagName);
          if (siblings.length > 1) {
            part += `:nth-of-type(${siblings.indexOf(current) + 1})`;
          }
        }
        parts.unshift(part);
        current = parent;
      }
      return parts.join(' > ');
    };
    const describe = (element) => {
      const tag = element.tagName.toLowerCase();
      const id = element.id ? `#${element.id}` : '';
      const label = element.getAttribute('aria-label') || element.getAttribute('title') || element.getAttribute('placeholder') || (element.innerText || element.value || '').trim();
      return `${tag}${id}${label ? ' ' + JSON.stringify(label.slice(0, 80)) : ''}`;
    };
    let index = 1;
    for (const element of Array.from(document.querySelectorAll('a,button,input,select,textarea,label,[role],[data-testid],h1,h2,h3,p,div,span')).slice(0, 120)) {
      const ref = `e${index++}`;
      const selector = cssPath(element);
      refs[ref] = { selector, tag: element.tagName.toLowerCase(), text: (element.innerText || element.value || '').trim().slice(0, 200) };
      lines.push(`  - ${ref} ${describe(element)}`);
    }
    window.__cmuxSnapshotRefs = refs;
    return { ok: true, value: { snapshot: lines.join('\n'), refs } };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
}

fn browser_locator_script(
    locator: BrowserLocator,
    params: &serde_json::Map<String, Value>,
) -> Result<String, String> {
    let (kind, primary, secondary, index) = match locator {
        BrowserLocator::Role => (
            "role",
            string_param(params, &["role"])
                .ok_or_else(|| "browser.find.role requires a role".to_string())?,
            raw_string_param(params, &["name"]).unwrap_or_default(),
            0usize,
        ),
        BrowserLocator::Text => (
            "text",
            raw_string_param(params, &["text"])
                .ok_or_else(|| "browser.find.text requires text".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Label => (
            "label",
            raw_string_param(params, &["label", "text"])
                .ok_or_else(|| "browser.find.label requires a label".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Placeholder => (
            "placeholder",
            raw_string_param(params, &["placeholder"])
                .ok_or_else(|| "browser.find.placeholder requires a placeholder".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Alt => (
            "alt",
            raw_string_param(params, &["alt", "text"])
                .ok_or_else(|| "browser.find.alt requires alt text".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Title => (
            "title",
            raw_string_param(params, &["title"])
                .ok_or_else(|| "browser.find.title requires a title".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::TestId => (
            "testid",
            raw_string_param(params, &["testid", "test_id", "testId"])
                .ok_or_else(|| "browser.find.testid requires a testid".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::First => (
            "first",
            raw_string_param(params, &["selector"])
                .ok_or_else(|| "browser.find.first requires a selector".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Last => (
            "last",
            raw_string_param(params, &["selector"])
                .ok_or_else(|| "browser.find.last requires a selector".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Nth => (
            "nth",
            raw_string_param(params, &["selector"])
                .ok_or_else(|| "browser.find.nth requires a selector".to_string())?,
            String::new(),
            usize_param(params, &["index", "nth"]).unwrap_or(0),
        ),
    };
    let encoded_kind = serde_json::to_string(kind).expect("locator kind JSON is infallible");
    let encoded_primary =
        serde_json::to_string(&primary).expect("locator value JSON is infallible");
    let encoded_secondary =
        serde_json::to_string(&secondary).expect("locator secondary JSON is infallible");
    Ok(format!(
        r#"(() => {{
  {context}
  const kind = {encoded_kind};
  const primary = {encoded_primary};
  const secondary = {encoded_secondary};
  const nthIndex = {index};
  try {{
    const normalize = (value) => String(value || '').trim().toLowerCase();
    const includes = (value, needle) => normalize(value).includes(normalize(needle));
    const cssPath = (element) => {{
      if (!element || element.nodeType !== 1) {{ return ''; }}
      if (element.id) {{ return '#' + CSS.escape(element.id); }}
      const parts = [];
      let current = element;
      while (current && current.nodeType === 1 && current !== document.documentElement) {{
        let part = current.tagName.toLowerCase();
        const parent = current.parentElement;
        if (parent) {{
          const siblings = Array.from(parent.children).filter((child) => child.tagName === current.tagName);
          if (siblings.length > 1) {{
            part += `:nth-of-type(${{siblings.indexOf(current) + 1}})`;
          }}
        }}
        parts.unshift(part);
        current = parent;
      }}
      return parts.join(' > ');
    }};
    const implicitRole = (element) => {{
      const tag = element.tagName.toLowerCase();
      const type = normalize(element.getAttribute('type'));
      if (element.getAttribute('role')) {{ return normalize(element.getAttribute('role')); }}
      if (tag === 'button') {{ return 'button'; }}
      if (tag === 'a' && element.hasAttribute('href')) {{ return 'link'; }}
      if (tag === 'select') {{ return 'combobox'; }}
      if (tag === 'textarea') {{ return 'textbox'; }}
      if (tag === 'input') {{
        if (type === 'checkbox') {{ return 'checkbox'; }}
        if (type === 'radio') {{ return 'radio'; }}
        if (type === 'submit' || type === 'button') {{ return 'button'; }}
        return 'textbox';
      }}
      return '';
    }};
    const accessibleName = (element) => {{
      const aria = element.getAttribute('aria-label');
      if (aria) {{ return aria; }}
      const labelledBy = element.getAttribute('aria-labelledby');
      if (labelledBy) {{
        return labelledBy.split(/\s+/).map((id) => document.getElementById(id)?.innerText || '').join(' ').trim();
      }}
      if (element.id) {{
        const label = document.querySelector(`label[for="${{CSS.escape(element.id)}}"]`);
        if (label) {{ return label.innerText || label.textContent || ''; }}
      }}
      return element.innerText || element.value || element.getAttribute('title') || element.getAttribute('alt') || '';
    }};
    let candidates = [];
    if (kind === 'role') {{
      candidates = Array.from(document.querySelectorAll('*')).filter((element) => {{
        if (implicitRole(element) !== normalize(primary)) {{ return false; }}
        return !secondary || includes(accessibleName(element), secondary);
      }});
    }} else if (kind === 'text') {{
      candidates = Array.from(document.querySelectorAll('body *')).filter((element) => includes(element.innerText || element.textContent || '', primary));
    }} else if (kind === 'label') {{
      const labels = Array.from(document.querySelectorAll('label')).filter((label) => includes(label.innerText || label.textContent || '', primary));
      candidates = labels.map((label) => label.htmlFor ? document.getElementById(label.htmlFor) : label.querySelector('input,textarea,select,button')).filter(Boolean);
    }} else if (kind === 'placeholder') {{
      candidates = Array.from(document.querySelectorAll('[placeholder]')).filter((element) => includes(element.getAttribute('placeholder'), primary));
    }} else if (kind === 'alt') {{
      candidates = Array.from(document.querySelectorAll('[alt]')).filter((element) => includes(element.getAttribute('alt'), primary));
    }} else if (kind === 'title') {{
      candidates = Array.from(document.querySelectorAll('[title]')).filter((element) => includes(element.getAttribute('title'), primary));
    }} else if (kind === 'testid') {{
      candidates = Array.from(document.querySelectorAll('[data-testid], [data-test-id], [data-test]')).filter((element) => [element.getAttribute('data-testid'), element.getAttribute('data-test-id'), element.getAttribute('data-test')].some((value) => normalize(value) === normalize(primary)));
    }} else if (kind === 'first' || kind === 'last' || kind === 'nth') {{
      candidates = Array.from(document.querySelectorAll(primary));
    }}
    const element = kind === 'last' ? candidates[candidates.length - 1] : candidates[kind === 'nth' ? nthIndex : 0];
    if (!element) {{ throw new Error(`No element matches locator: ${{kind}} ${{primary}}`); }}
    window.__cmuxSnapshotRefs = window.__cmuxSnapshotRefs || {{}};
    const ref = `e${{Object.keys(window.__cmuxSnapshotRefs).length + 1}}`;
    const selector = cssPath(element);
    window.__cmuxSnapshotRefs[ref] = {{ selector, tag: element.tagName.toLowerCase(), text: (element.innerText || element.value || '').trim().slice(0, 200) }};
    return {{ ok: true, value: {{ element_ref: '@' + ref, selector, ref, tag: element.tagName.toLowerCase(), text: (element.innerText || element.value || '').trim() }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    ))
}

fn browser_addstyle_script(css: &str) -> String {
    let encoded_css = serde_json::to_string(css).expect("CSS JSON is infallible");
    format!(
        r#"(() => {{
  const css = {encoded_css};
  try {{
    const style = document.createElement('style');
    style.setAttribute('data-cmux-added-style', 'true');
    style.textContent = css;
    (document.head || document.documentElement).appendChild(style);
    return {{ ok: true, value: true }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

fn browser_cookie_script(
    action: BrowserCookieAction,
    name: Option<&str>,
    value: Option<&str>,
) -> String {
    let encoded_name = serde_json::to_string(name.unwrap_or_default()).expect("cookie name JSON");
    let encoded_value =
        serde_json::to_string(value.unwrap_or_default()).expect("cookie value JSON");
    let action_name = match action {
        BrowserCookieAction::Get => "get",
        BrowserCookieAction::Set => "set",
        BrowserCookieAction::Clear => "clear",
    };
    let encoded_action = serde_json::to_string(action_name).expect("cookie action JSON");
    format!(
        r#"(() => {{
  const action = {encoded_action};
  const name = {encoded_name};
  const value = {encoded_value};
  try {{
    const parseCookies = () => String(document.cookie || '').split(';').map((part) => part.trim()).filter(Boolean).map((part) => {{
      const split = part.indexOf('=');
      const rawName = split >= 0 ? part.slice(0, split) : part;
      const rawValue = split >= 0 ? part.slice(split + 1) : '';
      return {{ name: decodeURIComponent(rawName), value: decodeURIComponent(rawValue) }};
    }});
    if (action === 'set') {{
      document.cookie = `${{encodeURIComponent(name)}}=${{encodeURIComponent(value)}}; path=/`;
      return {{ ok: true, value: {{ cookies: parseCookies().filter((cookie) => !name || cookie.name === name), set: true }} }};
    }}
    if (action === 'clear') {{
      document.cookie = `${{encodeURIComponent(name)}}=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT`;
      return {{ ok: true, value: {{ cookies: parseCookies().filter((cookie) => !name || cookie.name === name), cleared: true }} }};
    }}
    const cookies = parseCookies().filter((cookie) => !name || cookie.name === name);
    return {{ ok: true, value: {{ cookies }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

fn browser_storage_script(
    action: BrowserStorageAction,
    storage_type: &str,
    key: Option<&str>,
    value: &Value,
) -> String {
    let encoded_type = serde_json::to_string(storage_type).expect("storage type JSON");
    let encoded_key = serde_json::to_string(key.unwrap_or_default()).expect("storage key JSON");
    let encoded_value = serde_json::to_string(value).expect("storage value JSON");
    let action_name = match action {
        BrowserStorageAction::Get => "get",
        BrowserStorageAction::Set => "set",
        BrowserStorageAction::Clear => "clear",
    };
    let encoded_action = serde_json::to_string(action_name).expect("storage action JSON");
    format!(
        r#"(() => {{
  const action = {encoded_action};
  const storageType = {encoded_type};
  const key = {encoded_key};
  const encodedValue = {encoded_value};
  try {{
    const store = storageType === 'session' ? window.sessionStorage : window.localStorage;
    const decode = (raw) => {{
      if (raw === null || raw === undefined) {{ return null; }}
      try {{ return JSON.parse(raw); }} catch (_error) {{ return raw; }}
    }};
    if (action === 'set') {{
      store.setItem(key, JSON.stringify(encodedValue));
      return {{ ok: true, value: {{ type: storageType, key, value: encodedValue, set: true }} }};
    }}
    if (action === 'clear') {{
      if (key) {{ store.removeItem(key); }} else {{ store.clear(); }}
      return {{ ok: true, value: {{ type: storageType, key: key || null, cleared: true }} }};
    }}
    return {{ ok: true, value: {{ type: storageType, key, value: decode(store.getItem(key)) }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

fn browser_console_hook_script() -> &'static str {
    r#"(() => {
  try {
    if (!window.__cmuxConsoleCapture) {
      const state = {
        entries: [],
        errors: [],
        originals: {
          log: console.log.bind(console),
          info: console.info.bind(console),
          warn: console.warn.bind(console),
          error: console.error.bind(console),
          debug: console.debug.bind(console),
        },
      };
      const stringify = (value) => {
        try {
          if (typeof value === 'string') { return value; }
          if (value instanceof Error) { return value.stack || value.message || String(value); }
          return JSON.stringify(value);
        } catch (_error) {
          return String(value);
        }
      };
      const pushEntry = (level, args) => {
        state.entries.push({
          level,
          text: Array.from(args).map(stringify).join(' '),
          args: Array.from(args).map(stringify),
          timestamp: Date.now(),
        });
      };
      for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
        console[level] = function (...args) {
          pushEntry(level, args);
          return state.originals[level](...args);
        };
      }
      window.addEventListener('error', (event) => {
        state.errors.push({
          message: String(event.message || ''),
          source: event.filename || null,
          line: event.lineno || null,
          column: event.colno || null,
          stack: event.error && event.error.stack ? String(event.error.stack) : null,
          timestamp: Date.now(),
        });
      });
      window.addEventListener('unhandledrejection', (event) => {
        state.errors.push({
          message: stringify(event.reason),
          source: 'unhandledrejection',
          line: null,
          column: null,
          stack: event.reason && event.reason.stack ? String(event.reason.stack) : null,
          timestamp: Date.now(),
        });
      });
      window.__cmuxConsoleCapture = state;
    }
    return window.__cmuxConsoleCapture;
  } catch (error) {
    return null;
  }
})()"#
}

fn browser_console_list_script() -> String {
    let hook = serde_json::to_string(browser_console_hook_script()).expect("console hook JSON");
    r#"(() => {
  try {
    (0, eval)(__CMUX_CONSOLE_HOOK__);
    const entries = (window.__cmuxConsoleCapture && window.__cmuxConsoleCapture.entries) || [];
    return { ok: true, value: { entries, count: entries.length } };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
        .replace("__CMUX_CONSOLE_HOOK__", &hook)
}

fn browser_console_clear_script() -> String {
    let hook = serde_json::to_string(browser_console_hook_script()).expect("console hook JSON");
    r#"(() => {
  try {
    (0, eval)(__CMUX_CONSOLE_HOOK__);
    const capture = window.__cmuxConsoleCapture;
    const cleared = capture ? capture.entries.length : 0;
    if (capture) { capture.entries = []; }
    return { ok: true, value: { entries: [], count: 0, cleared } };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
        .replace("__CMUX_CONSOLE_HOOK__", &hook)
}

fn browser_errors_list_script() -> String {
    let hook = serde_json::to_string(browser_console_hook_script()).expect("console hook JSON");
    r#"(() => {
  try {
    (0, eval)(__CMUX_CONSOLE_HOOK__);
    const errors = (window.__cmuxConsoleCapture && window.__cmuxConsoleCapture.errors) || [];
    return { ok: true, value: { errors, count: errors.length } };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
        .replace("__CMUX_CONSOLE_HOOK__", &hook)
}

fn browser_state_capture_script() -> &'static str {
    r#"(() => {
  try {
    const entries = (store) => {
      const values = {};
      for (let index = 0; index < store.length; index += 1) {
        const key = store.key(index);
        values[key] = store.getItem(key);
      }
      return values;
    };
    const cookies = String(document.cookie || '').split(';').map((part) => part.trim()).filter(Boolean).map((part) => {
      const split = part.indexOf('=');
      const rawName = split >= 0 ? part.slice(0, split) : part;
      const rawValue = split >= 0 ? part.slice(split + 1) : '';
      return { name: decodeURIComponent(rawName), value: decodeURIComponent(rawValue) };
    });
    const value = {
      version: 1,
      url: String(window.location.href || ''),
      savedAt: Date.now(),
      localStorage: entries(window.localStorage),
      sessionStorage: entries(window.sessionStorage),
      cookies,
    };
    return { ok: true, value };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
}

fn browser_state_restore_script(state: &Value) -> String {
    let encoded_state = serde_json::to_string(state).expect("browser state JSON is infallible");
    format!(
        r#"(() => {{
  const state = {encoded_state};
  try {{
    const restoreStore = (store, values) => {{
      store.clear();
      for (const [key, value] of Object.entries(values || {{}})) {{
        store.setItem(key, String(value));
      }}
    }};
    restoreStore(window.localStorage, state.localStorage);
    restoreStore(window.sessionStorage, state.sessionStorage);
    for (const cookie of Array.from(String(document.cookie || '').split(';')).map((part) => part.trim()).filter(Boolean)) {{
      const split = cookie.indexOf('=');
      const rawName = split >= 0 ? cookie.slice(0, split) : cookie;
      document.cookie = `${{rawName}}=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT`;
    }}
    for (const cookie of state.cookies || []) {{
      if (!cookie || !cookie.name) {{ continue; }}
      document.cookie = `${{encodeURIComponent(cookie.name)}}=${{encodeURIComponent(cookie.value || '')}}; path=/`;
    }}
    return {{
      ok: true,
      value: {{
        version: state.version || 1,
        localStorageCount: Object.keys(state.localStorage || {{}}).length,
        sessionStorageCount: Object.keys(state.sessionStorage || {{}}).length,
        cookieCount: Array.isArray(state.cookies) ? state.cookies.length : 0
      }}
    }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

fn browser_highlight_script(selector: &str) -> String {
    let encoded = serde_json::to_string(selector).expect("highlight selector JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const raw = {encoded};
  const ref = String(raw || '').startsWith('@') ? String(raw).slice(1) : raw;
  const selector = (window.__cmuxSnapshotRefs && window.__cmuxSnapshotRefs[ref] && window.__cmuxSnapshotRefs[ref].selector) || raw;
  try {{
    const element = document.querySelector(selector);
    if (!element) {{ throw new Error(`No element matches selector: ${{selector}}`); }}
    const previous = document.querySelector('[data-cmux-highlight-overlay="true"]');
    if (previous) {{ previous.remove(); }}
    const rect = element.getBoundingClientRect();
    const overlay = document.createElement('div');
    overlay.setAttribute('data-cmux-highlight-overlay', 'true');
    Object.assign(overlay.style, {{
      position: 'fixed',
      pointerEvents: 'none',
      zIndex: '2147483647',
      left: `${{rect.left}}px`,
      top: `${{rect.top}}px`,
      width: `${{rect.width}}px`,
      height: `${{rect.height}}px`,
      border: '3px solid #00A3FF',
      boxShadow: '0 0 0 9999px rgba(0, 163, 255, 0.10)',
      borderRadius: '6px'
    }});
    document.documentElement.appendChild(overlay);
    return {{ ok: true, value: true }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

fn browser_wait_selector_script(selector: &str) -> String {
    let encoded = serde_json::to_string(selector).expect("selector JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const raw = {encoded};
  const ref = String(raw || '').startsWith('@') ? String(raw).slice(1) : raw;
  const selector = (window.__cmuxSnapshotRefs && window.__cmuxSnapshotRefs[ref] && window.__cmuxSnapshotRefs[ref].selector) || raw;
  return {{ ok: true, value: Boolean(document.querySelector(selector)) }};
}})()"#,
        context = browser_js_context_prelude(),
    )
}

fn browser_wait_text_script(text: &str) -> String {
    let encoded = serde_json::to_string(text).expect("text JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const needle = {encoded};
  return {{ ok: true, value: String(document.body ? (document.body.innerText || document.body.textContent || '') : '').includes(needle) }};
}})()"#,
        context = browser_js_context_prelude(),
    )
}

fn browser_wait_function_script(expression: &str) -> String {
    let encoded = serde_json::to_string(expression).expect("expression JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const expression = {encoded};
  try {{
    return {{ ok: true, value: Boolean((0, eval)(expression)) }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

fn browser_wait_load_state_script(load_state: &str) -> String {
    let normalized = load_state.trim().to_ascii_lowercase();
    let target = match normalized.as_str() {
        "domcontentloaded" | "interactive" => "interactive",
        "complete" | "load" | "loaded" => "complete",
        _ => "complete",
    };
    let encoded = serde_json::to_string(target).expect("load state JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const target = {encoded};
  const state = document.readyState;
  const value = target === 'interactive' ? (state === 'interactive' || state === 'complete') : state === 'complete';
  return {{ ok: true, value }};
}})()"#,
        context = browser_js_context_prelude(),
    )
}

fn browser_wait_url_script(url_contains: &str) -> String {
    let encoded = serde_json::to_string(url_contains).expect("URL token JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const needle = {encoded};
  return {{ ok: true, value: String(window.location.href || '').includes(needle) }};
}})()"#,
        context = browser_js_context_prelude(),
    )
}

fn browser_action_script(
    action: BrowserAction,
    selector: Option<&str>,
    text: &str,
    value: &str,
    key: &str,
    dx: f64,
    dy: f64,
) -> String {
    let encoded_selector =
        serde_json::to_string(selector.unwrap_or_default()).expect("selector JSON is infallible");
    let encoded_text = serde_json::to_string(text).expect("text JSON is infallible");
    let encoded_value = serde_json::to_string(value).expect("value JSON is infallible");
    let encoded_key = serde_json::to_string(key).expect("key JSON is infallible");
    let body = match action {
        BrowserAction::Click => "element.click(); return { ok: true, value: true };",
        BrowserAction::DblClick => "element.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true, view: window })); return { ok: true, value: true };",
        BrowserAction::Hover => "for (const type of ['mouseover', 'mouseenter', 'mousemove']) { element.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, view: window })); } return { ok: true, value: true };",
        BrowserAction::Focus => "element.focus(); return { ok: true, value: true };",
        BrowserAction::Type => "element.focus(); element.value = String(element.value || '') + __cmuxText; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: element.value ?? true };",
        BrowserAction::Fill => "element.focus(); element.value = __cmuxText; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: element.value ?? true };",
        BrowserAction::Press => "const pressTarget = document.activeElement || document.body; for (const type of ['keydown', 'keypress', 'keyup']) { pressTarget.dispatchEvent(new KeyboardEvent(type, { key: __cmuxKey, bubbles: true, cancelable: true })); } return { ok: true, value: true };",
        BrowserAction::KeyDown => "const downTarget = document.activeElement || document.body; downTarget.dispatchEvent(new KeyboardEvent('keydown', { key: __cmuxKey, bubbles: true, cancelable: true })); return { ok: true, value: true };",
        BrowserAction::KeyUp => "const upTarget = document.activeElement || document.body; upTarget.dispatchEvent(new KeyboardEvent('keyup', { key: __cmuxKey, bubbles: true, cancelable: true })); return { ok: true, value: true };",
        BrowserAction::Check => "element.checked = true; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: Boolean(element.checked) };",
        BrowserAction::Uncheck => "element.checked = false; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: Boolean(element.checked) };",
        BrowserAction::Select => "element.value = __cmuxValue; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: element.value };",
        BrowserAction::Scroll => "const target = element || window; if (target === window) { window.scrollBy(__cmuxDx, __cmuxDy); } else { target.scrollBy(__cmuxDx, __cmuxDy); } return { ok: true, value: true };",
        BrowserAction::ScrollIntoView => "element.scrollIntoView({ block: 'center', inline: 'nearest' }); return { ok: true, value: true };",
    };
    let needs_element = browser_action_requires_selector(action);
    let element_lookup = if needs_element {
        "const element = document.querySelector(__cmuxSelector); if (!element) { throw new Error(`No element matches selector: ${__cmuxSelector}`); }"
    } else if action == BrowserAction::Scroll {
        "const element = __cmuxSelector ? document.querySelector(__cmuxSelector) : null; if (__cmuxSelector && !element) { throw new Error(`No element matches selector: ${__cmuxSelector}`); }"
    } else {
        "const element = null;"
    };
    format!(
        r#"(() => {{
  {context}
  const __cmuxRawSelector = {encoded_selector};
  const __cmuxRef = String(__cmuxRawSelector || '').startsWith('@') ? String(__cmuxRawSelector).slice(1) : __cmuxRawSelector;
  const __cmuxSelector = (window.__cmuxSnapshotRefs && window.__cmuxSnapshotRefs[__cmuxRef] && window.__cmuxSnapshotRefs[__cmuxRef].selector) || __cmuxRawSelector;
  const __cmuxText = {encoded_text};
  const __cmuxValue = {encoded_value};
  const __cmuxKey = {encoded_key};
  const __cmuxDx = {dx};
  const __cmuxDy = {dy};
  try {{
    {element_lookup}
    {body}
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

fn browser_action_requires_selector(action: BrowserAction) -> bool {
    matches!(
        action,
        BrowserAction::Click
            | BrowserAction::DblClick
            | BrowserAction::Hover
            | BrowserAction::Focus
            | BrowserAction::Type
            | BrowserAction::Fill
            | BrowserAction::Check
            | BrowserAction::Uncheck
            | BrowserAction::Select
            | BrowserAction::ScrollIntoView
    )
}

fn browser_action_method(action: BrowserAction) -> &'static str {
    match action {
        BrowserAction::Click => "browser.click",
        BrowserAction::DblClick => "browser.dblclick",
        BrowserAction::Hover => "browser.hover",
        BrowserAction::Focus => "browser.focus",
        BrowserAction::Type => "browser.type",
        BrowserAction::Fill => "browser.fill",
        BrowserAction::Press => "browser.press",
        BrowserAction::KeyDown => "browser.keydown",
        BrowserAction::KeyUp => "browser.keyup",
        BrowserAction::Check => "browser.check",
        BrowserAction::Uncheck => "browser.uncheck",
        BrowserAction::Select => "browser.select",
        BrowserAction::Scroll => "browser.scroll",
        BrowserAction::ScrollIntoView => "browser.scroll_into_view",
    }
}

fn browser_locator_method(locator: BrowserLocator) -> &'static str {
    match locator {
        BrowserLocator::Role => "browser.find.role",
        BrowserLocator::Text => "browser.find.text",
        BrowserLocator::Label => "browser.find.label",
        BrowserLocator::Placeholder => "browser.find.placeholder",
        BrowserLocator::Alt => "browser.find.alt",
        BrowserLocator::Title => "browser.find.title",
        BrowserLocator::TestId => "browser.find.testid",
        BrowserLocator::First => "browser.find.first",
        BrowserLocator::Last => "browser.find.last",
        BrowserLocator::Nth => "browser.find.nth",
    }
}

fn browser_locator_payload(panel_id: &str, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            payload.insert(key.clone(), value.clone());
        }
        if let Some(element_ref) = object
            .get("element_ref")
            .or_else(|| object.get("elementRef"))
        {
            payload.insert("elementRef".to_string(), element_ref.clone());
        }
    } else {
        payload.insert("value".to_string(), value);
    }
    Value::Object(payload)
}

fn browser_cookie_method(action: BrowserCookieAction) -> &'static str {
    match action {
        BrowserCookieAction::Get => "browser.cookies.get",
        BrowserCookieAction::Set => "browser.cookies.set",
        BrowserCookieAction::Clear => "browser.cookies.clear",
    }
}

fn browser_storage_method(action: BrowserStorageAction) -> &'static str {
    match action {
        BrowserStorageAction::Get => "browser.storage.get",
        BrowserStorageAction::Set => "browser.storage.set",
        BrowserStorageAction::Clear => "browser.storage.clear",
    }
}

fn browser_console_method(action: BrowserConsoleAction) -> &'static str {
    match action {
        BrowserConsoleAction::List => "browser.console.list",
        BrowserConsoleAction::Clear => "browser.console.clear",
    }
}

fn browser_dialog_method(action: BrowserDialogAction) -> &'static str {
    match action {
        BrowserDialogAction::Accept => "browser.dialog.accept",
        BrowserDialogAction::Dismiss => "browser.dialog.dismiss",
    }
}

fn browser_cookie_payload(panel_id: &str, action: BrowserCookieAction, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    if let Some(cookies) = value.get("cookies") {
        payload.insert("cookies".to_string(), cookies.clone());
    } else {
        payload.insert("cookies".to_string(), json!([]));
    }
    match action {
        BrowserCookieAction::Set => {
            payload.insert("set".to_string(), json!(true));
        }
        BrowserCookieAction::Clear => {
            payload.insert("cleared".to_string(), json!(true));
        }
        BrowserCookieAction::Get => {}
    }
    Value::Object(payload)
}

fn browser_dialog_payload(panel_id: &str, action: BrowserDialogAction, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            payload.insert(key.clone(), value.clone());
        }
    }
    payload.insert(
        "accepted".to_string(),
        json!(action == BrowserDialogAction::Accept),
    );
    Value::Object(payload)
}

fn browser_console_payload(panel_id: &str, action: BrowserConsoleAction, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    let entries = value.get("entries").cloned().unwrap_or_else(|| json!([]));
    let count = value
        .get("count")
        .cloned()
        .unwrap_or_else(|| json!(entries.as_array().map_or(0, Vec::len)));
    payload.insert("entries".to_string(), entries);
    payload.insert("count".to_string(), count);
    if action == BrowserConsoleAction::Clear {
        payload.insert(
            "cleared".to_string(),
            value.get("cleared").cloned().unwrap_or_else(|| json!(true)),
        );
    }
    Value::Object(payload)
}

fn browser_errors_payload(panel_id: &str, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    let errors = value.get("errors").cloned().unwrap_or_else(|| json!([]));
    let count = value
        .get("count")
        .cloned()
        .unwrap_or_else(|| json!(errors.as_array().map_or(0, Vec::len)));
    payload.insert("errors".to_string(), errors);
    payload.insert("count".to_string(), count);
    Value::Object(payload)
}

fn browser_state_payload(panel_id: &str, path: &str, verb: &str, state: Value) -> Value {
    let local_storage_count = state
        .get("localStorage")
        .and_then(Value::as_object)
        .map_or_else(
            || {
                state
                    .get("localStorageCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize
            },
            serde_json::Map::len,
        );
    let session_storage_count = state
        .get("sessionStorage")
        .and_then(Value::as_object)
        .map_or_else(
            || {
                state
                    .get("sessionStorageCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize
            },
            serde_json::Map::len,
        );
    let cookie_count = state.get("cookies").and_then(Value::as_array).map_or_else(
        || {
            state
                .get("cookieCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize
        },
        Vec::len,
    );
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    payload.insert("path".to_string(), json!(path));
    payload.insert(verb.to_string(), json!(true));
    payload.insert("localStorageCount".to_string(), json!(local_storage_count));
    payload.insert(
        "sessionStorageCount".to_string(),
        json!(session_storage_count),
    );
    payload.insert("cookieCount".to_string(), json!(cookie_count));
    Value::Object(payload)
}

fn solid_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let width = width.max(1);
    let height = height.max(1);
    let row_len = 1usize + width as usize * 3;
    let mut raw = Vec::with_capacity(row_len * height as usize);
    for _ in 0..height {
        raw.push(0);
        for _ in 0..width {
            raw.extend_from_slice(&rgb);
        }
    }

    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(&mut png, b"IDAT", &zlib_store(&raw));
    png_chunk(&mut png, b"IEND", &[]);
    png
}

fn zlib_store(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut offset = 0usize;
    while offset < raw.len() {
        let remaining = raw.len() - offset;
        let len = remaining.min(u16::MAX as usize);
        let final_block = offset + len >= raw.len();
        out.push(if final_block { 0x01 } else { 0x00 });
        let len_u16 = len as u16;
        out.extend_from_slice(&len_u16.to_le_bytes());
        out.extend_from_slice(&(!len_u16).to_le_bytes());
        out.extend_from_slice(&raw[offset..offset + len]);
        offset += len;
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(kind.len() + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

fn browser_storage_payload(panel_id: &str, action: BrowserStorageAction, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            payload.insert(key.clone(), value.clone());
        }
    } else {
        payload.insert("value".to_string(), value);
    }
    match action {
        BrowserStorageAction::Set => {
            payload.insert("set".to_string(), json!(true));
        }
        BrowserStorageAction::Clear => {
            payload.insert("cleared".to_string(), json!(true));
        }
        BrowserStorageAction::Get => {}
    }
    Value::Object(payload)
}

fn browser_not_found_message(message: &str) -> String {
    if message.contains("No element matches selector") {
        format!("{message}; hint: verify the selector or refresh browser.snapshot refs; snapshot unavailable in this error path")
    } else {
        message.to_string()
    }
}

fn browser_getter_script(
    getter: BrowserGetter,
    selector: Option<&str>,
    attr: Option<&str>,
) -> String {
    let encoded_selector =
        serde_json::to_string(selector.unwrap_or_default()).expect("selector JSON is infallible");
    let encoded_attr =
        serde_json::to_string(attr.unwrap_or_default()).expect("attribute JSON is infallible");
    let body = match getter {
        BrowserGetter::Text => {
            "const value = element.innerText ?? element.textContent ?? ''; return { ok: true, value };"
        }
        BrowserGetter::Html => {
            "const value = element.outerHTML ?? element.innerHTML ?? ''; return { ok: true, value };"
        }
        BrowserGetter::Value => {
            "const value = element.value ?? element.getAttribute('value') ?? ''; return { ok: true, value };"
        }
        BrowserGetter::Attr => {
            "const attr = __cmuxAttr; const value = element.getAttribute(attr); return { ok: true, value };"
        }
        BrowserGetter::Title => "return { ok: true, value: document.title || '' };",
        BrowserGetter::Count => {
            "return { ok: true, value: document.querySelectorAll(__cmuxSelector).length };"
        }
        BrowserGetter::Box => {
            "const rect = element.getBoundingClientRect(); const value = { x: rect.x, y: rect.y, top: rect.top, right: rect.right, bottom: rect.bottom, left: rect.left, width: rect.width, height: rect.height }; return { ok: true, value };"
        }
        BrowserGetter::Styles => {
            "const style = window.getComputedStyle(element); if (__cmuxAttr) { return { ok: true, value: style.getPropertyValue(__cmuxAttr) || style[__cmuxAttr] || '' }; } const value = {}; for (const name of style) { value[name] = style.getPropertyValue(name); } return { ok: true, value };"
        }
        BrowserGetter::Visible => {
            "const style = window.getComputedStyle(element); const rect = element.getBoundingClientRect(); const value = style.visibility !== 'hidden' && style.display !== 'none' && Number(style.opacity || '1') !== 0 && rect.width > 0 && rect.height > 0; return { ok: true, value };"
        }
        BrowserGetter::Enabled => {
            "const ariaDisabled = String(element.getAttribute('aria-disabled') || '').toLowerCase(); const value = !element.disabled && ariaDisabled !== 'true'; return { ok: true, value };"
        }
        BrowserGetter::Checked => {
            "const value = Boolean(element.checked); return { ok: true, value };"
        }
    };
    let needs_element = !matches!(getter, BrowserGetter::Title | BrowserGetter::Count);
    let element_lookup = if needs_element {
        "const element = document.querySelector(__cmuxSelector); if (!element) { throw new Error(`No element matches selector: ${__cmuxSelector}`); }"
    } else {
        ""
    };
    format!(
        r#"(() => {{
  {context}
  const __cmuxRawSelector = {encoded_selector};
  const __cmuxRef = String(__cmuxRawSelector || '').startsWith('@') ? String(__cmuxRawSelector).slice(1) : __cmuxRawSelector;
  const __cmuxSelector = (window.__cmuxSnapshotRefs && window.__cmuxSnapshotRefs[__cmuxRef] && window.__cmuxSnapshotRefs[__cmuxRef].selector) || __cmuxRawSelector;
  const __cmuxAttr = {encoded_attr};
  try {{
    {element_lookup}
    {body}
  }} catch (error) {{
    return {{
      ok: false,
      error: String((error && (error.stack || error.message)) || error)
    }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

fn unwrap_browser_eval_result(value: Value) -> Result<Value, String> {
    let Some(object) = value.as_object() else {
        return Ok(value);
    };
    match object.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(object.get("value").cloned().unwrap_or(Value::Null)),
        Some(false) => Err(object
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("browser JavaScript execution failed")
            .to_string()),
        None => Ok(value),
    }
}

fn browser_getter_method(getter: BrowserGetter) -> &'static str {
    match getter {
        BrowserGetter::Text => "browser.get.text",
        BrowserGetter::Html => "browser.get.html",
        BrowserGetter::Value => "browser.get.value",
        BrowserGetter::Attr => "browser.get.attr",
        BrowserGetter::Title => "browser.get.title",
        BrowserGetter::Count => "browser.get.count",
        BrowserGetter::Box => "browser.get.box",
        BrowserGetter::Styles => "browser.get.styles",
        BrowserGetter::Visible => "browser.is.visible",
        BrowserGetter::Enabled => "browser.is.enabled",
        BrowserGetter::Checked => "browser.is.checked",
    }
}

fn browser_getter_payload(panel_id: &str, getter: BrowserGetter, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    payload.insert("value".to_string(), value.clone());
    match getter {
        BrowserGetter::Text => {
            payload.insert("text".to_string(), value);
        }
        BrowserGetter::Html => {
            payload.insert("html".to_string(), value);
        }
        BrowserGetter::Title => {
            payload.insert("title".to_string(), value);
        }
        BrowserGetter::Count => {
            payload.insert("count".to_string(), value);
        }
        BrowserGetter::Box => {
            payload.insert("box".to_string(), value);
        }
        BrowserGetter::Styles => {
            payload.insert("styles".to_string(), value);
        }
        BrowserGetter::Attr
        | BrowserGetter::Value
        | BrowserGetter::Visible
        | BrowserGetter::Enabled
        | BrowserGetter::Checked => {}
    }
    Value::Object(payload)
}

fn debug_terminals(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let selected_workspace = selected_workspace_index(&current);
    let workspace_filter = workspace_index_from_params(&current, params)
        .or_else(|| params.contains_key("workspace_id").then_some(usize::MAX));
    let terminal_state = app.state::<TerminalState>();
    let runtime_by_panel: BTreeMap<String, Value> =
        terminal_runtime_snapshots(terminal_state.inner())
            .into_iter()
            .filter_map(|runtime| {
                let panel_id = runtime.panel_id.clone()?;
                Some((
                    panel_id,
                    json!({
                        "terminal_id": runtime.id,
                        "root_pid": runtime.root_pid,
                        "process_root_pid": runtime.root_pid,
                        "descendant_pids": runtime.descendant_pids,
                        "child_pids": runtime.child_pids,
                        "process_count": runtime.process_count,
                        "foreground_pid": runtime.foreground_pid,
                        "foreground_process_name": runtime.foreground_process_name,
                        "foreground_process_source": runtime.foreground_process_source,
                        "process_error": runtime.process_error,
                    }),
                ))
            })
            .collect();
    let mut terminals = Vec::new();
    let Some(window) = current.windows.first() else {
        return ok(json!({"terminals": terminals}));
    };
    for (workspace_index, workspace) in window.tab_manager.workspaces.iter().enumerate() {
        if workspace_filter.is_some_and(|filter| filter != workspace_index) {
            continue;
        }
        for (surface_index, surface) in surfaces_for_workspace(workspace).into_iter().enumerate() {
            let surface_type = surface
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("terminal");
            if surface_type != "terminal" {
                continue;
            }
            let panel_id = surface
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let runtime = runtime_by_panel.get(&panel_id);
            terminals.push(json!({
                "workspace_id": workspace.workspace_id,
                "workspace_ref": workspace_ref(workspace_index),
                "workspace_selected": workspace_index == selected_workspace,
                "surface_id": panel_id,
                "panel_id": panel_id,
                "surface_ref": surface_ref(surface_index),
                "type": surface_type,
                "title": surface.get("title").cloned().unwrap_or(Value::Null),
                "tty": surface.get("tty").cloned().unwrap_or(Value::Null),
                "tty_name": surface.get("tty_name").cloned().unwrap_or(Value::Null),
                "runtime_surface_ready": runtime.is_some(),
                "terminal_id": runtime.and_then(|value| value.get("terminal_id")).cloned().unwrap_or(Value::Null),
                "root_pid": runtime.and_then(|value| value.get("root_pid")).cloned().unwrap_or(Value::Null),
                "process_root_pid": runtime.and_then(|value| value.get("process_root_pid")).cloned().unwrap_or(Value::Null),
                "descendant_pids": runtime.and_then(|value| value.get("descendant_pids")).cloned().unwrap_or_else(|| json!([])),
                "child_pids": runtime.and_then(|value| value.get("child_pids")).cloned().unwrap_or_else(|| json!([])),
                "process_count": runtime.and_then(|value| value.get("process_count")).cloned().unwrap_or(Value::Null),
                "foreground_pid": runtime.and_then(|value| value.get("foreground_pid")).cloned().unwrap_or(Value::Null),
                "foreground_process_name": runtime.and_then(|value| value.get("foreground_process_name")).cloned().unwrap_or(Value::Null),
                "foreground_process_source": runtime.and_then(|value| value.get("foreground_process_source")).cloned().unwrap_or(Value::Null),
                "process_error": runtime.and_then(|value| value.get("process_error")).cloned().unwrap_or(Value::Null),
                "running": runtime.is_some() || !surface.get("tty").unwrap_or(&Value::Null).is_null(),
            }));
        }
    }
    ok(json!({"terminals": terminals}))
}

fn debug_browser_start_direct_proxy(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("debug.browser.start_direct_proxy requires a browser surface");
    }

    let target_host = raw_string_param(params, &["target_host", "targetHost"])
        .map(|host| host.trim().to_string())
        .filter(|host| !host.is_empty());
    let target_port = optional_u16_param(params, "target_port")
        .flatten()
        .or_else(|| optional_u16_param(params, "targetPort").flatten());
    let target_override = match (target_host, target_port) {
        (Some(host), Some(port)) => Some(crate::remote_proxy::ProxyTarget { host, port }),
        (None, None) => None,
        _ => return invalid_params(
            "debug.browser.start_direct_proxy target override requires targetHost and targetPort",
        ),
    };

    let state = app.state::<SessionState>();
    match start_direct_browser_proxy_for_control(app, &state, &current, &panel_id, target_override)
    {
        Ok((_snapshot, proxy_url)) => ok(json!({
            "surface_id": panel_id,
            "panel_id": panel_id,
            "proxy_url": proxy_url,
            "proxyUrl": proxy_url,
        })),
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn debug_browser_attach_webview(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("debug.browser.attach_webview requires a browser surface");
    }

    let Some(surface) = browser_surface_payload(&current, workspace_index, &panel_id) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let current_url = surface
        .get("surface")
        .and_then(Value::as_object)
        .and_then(|surface| surface.get("browser_url"))
        .and_then(Value::as_str)
        .or_else(|| surface.get("url").and_then(Value::as_str))
        .unwrap_or("about:blank");
    let current_proxy_url = surface
        .get("surface")
        .and_then(Value::as_object)
        .and_then(|surface| surface.get("browser_proxy_url"))
        .and_then(Value::as_str);
    let url = raw_string_param(params, &["url"]).unwrap_or_else(|| current_url.to_string());
    let proxy_url = raw_string_param(params, &["proxy_url", "proxyUrl"])
        .or_else(|| current_proxy_url.map(str::to_string));
    let visible = bool_param(params, &["visible"]).unwrap_or(true);

    let browser_state = app.state::<BrowserWebviewState>();
    match browser_attach_webview_for_control(
        app,
        browser_state.inner(),
        "main",
        &panel_id,
        Some(&url),
        proxy_url.as_deref(),
        visible,
    ) {
        Ok(reply) => ok(json!(reply)),
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_reopen_closed(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<SessionState>();
    workspace_current(&reopen_closed_browser_tab_for_control(app, &state))
}

fn browser_clear_history(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    surface_list_from_params(
        &clear_browser_history_for_control(app, &state, &panel_id),
        params,
    )
}

fn browser_toggle_omnibar(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    surface_list_from_params(
        &toggle_browser_omnibar_for_control(app, &state, &panel_id),
        params,
    )
}

fn browser_toggle_focus_mode(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    surface_list_from_params(
        &toggle_browser_focus_mode_for_control(app, &state, &panel_id),
        params,
    )
}

fn browser_toggle_developer_tools(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    surface_list_from_params(
        &toggle_browser_developer_tools_for_control(app, &state, &panel_id),
        params,
    )
}

fn browser_show_developer_tools(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let panel = string_param(params, &["panel"]).unwrap_or_else(|| "inspector".to_string());
    let state = app.state::<SessionState>();
    surface_list_from_params(
        &show_browser_developer_tools_for_control(app, &state, &panel_id, &panel),
        params,
    )
}

fn browser_set_zoom(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(zoom) = f64_param(params, &["zoom", "scale"]) else {
        return invalid_params("Missing or invalid browser zoom");
    };
    let state = app.state::<SessionState>();
    surface_list_from_params(
        &set_browser_zoom_for_control(app, &state, &panel_id, zoom),
        params,
    )
}

fn browser_network_requests(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.network.requests requires a browser surface");
    }
    let query = BrowserNetworkRequestsQuery {
        url_contains: raw_string_param(params, &["url_contains", "urlContains", "url"]),
        method: raw_string_param(params, &["method"]),
        since_id: raw_string_param(params, &["since_id", "sinceId", "after_id", "afterId"]),
        limit: usize_param(params, &["limit"]).map(|value| value.min(200)),
    };
    let state = app.state::<BrowserWebviewState>();
    match browser_network_requests_for_control(state.inner(), &panel_id, query) {
        Ok(reply) => ok(json!(reply)),
        Err(message) => ControlCallResult::Err {
            code: "internal_error".to_string(),
            message,
            data: None,
        },
    }
}

fn browser_network_clear(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.network.clear requires a browser surface");
    }
    let state = app.state::<BrowserWebviewState>();
    match browser_clear_network_requests_for_control(state.inner(), &panel_id) {
        Ok(reply) => ok(json!(reply)),
        Err(message) => ControlCallResult::Err {
            code: "internal_error".to_string(),
            message,
            data: None,
        },
    }
}

fn snapshot(app: &AppHandle) -> AppSessionSnapshot {
    let state = app.state::<SessionState>();
    current_session_snapshot(&state)
}

fn invalid_params(message: &str) -> ControlCallResult {
    ControlCallResult::Err {
        code: "invalid_params".to_string(),
        message: message.to_string(),
        data: None,
    }
}

fn not_supported(message: &str) -> ControlCallResult {
    ControlCallResult::Err {
        code: "not_supported".to_string(),
        message: message.to_string(),
        data: None,
    }
}

fn is_unported_browser_automation_method(_method: &str) -> bool {
    false
}

fn ok(value: Value) -> ControlCallResult {
    match JsonValue::try_from(value) {
        Ok(value) => ControlCallResult::Ok(value),
        Err(error) => ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Could not encode control response: {error}"),
            data: None,
        },
    }
}

#[cfg(test)]
fn workspace_list_payload(snapshot: &AppSessionSnapshot) -> Value {
    let Some(window) = snapshot.windows.first() else {
        return Value::Null;
    };
    let selected = selected_workspace_index(snapshot);
    json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspaces": window.tab_manager.workspaces.iter().enumerate()
            .map(|(index, workspace)| workspace_summary(workspace, index, index == selected))
            .collect::<Vec<_>>(),
        "workspace_groups": workspace_group_summaries(
            &window.tab_manager.workspaces,
            &window.tab_manager.workspace_groups,
        ),
    })
}

fn workspace_list_from_params_for_app(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(window_index) = workspace_routed_window_index(snapshot, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let mut payload = workspace_list_payload_for_window(snapshot, window_index);
    apply_workspace_handle_refs(app, &mut payload);
    ok(payload)
}

fn workspace_list_payload_for_window(snapshot: &AppSessionSnapshot, window_index: usize) -> Value {
    let Some(window) = snapshot.windows.get(window_index) else {
        return Value::Null;
    };
    let selected = selected_workspace_index_for_window(snapshot, window_index);
    json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| window_ref(window_index)),
        "workspaces": window
            .tab_manager
            .workspaces
            .iter()
            .enumerate()
            .map(|(index, workspace)| canonical_workspace_summary(workspace, index, Some(index) == selected))
            .collect::<Vec<_>>(),
    })
}

fn extension_sidebar_snapshot_payload_for_app(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Value {
    extension_sidebar_snapshot_payload_with_events(
        snapshot,
        extension_sidebar_events_context(app),
        custom_sidebar_asset_map_from_params(params),
    )
}

#[allow(dead_code)]
fn extension_sidebar_snapshot_payload(snapshot: &AppSessionSnapshot) -> Value {
    extension_sidebar_snapshot_payload_with_events(
        snapshot,
        extension_sidebar_empty_events_context(),
        json!({}),
    )
}

fn extension_sidebar_snapshot_payload_with_events(
    snapshot: &AppSessionSnapshot,
    events: Value,
    assets: Value,
) -> Value {
    let latest_seq = events
        .get("latest_seq")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let Some(window) = snapshot.windows.first() else {
        let data = extension_sidebar_data_context(Vec::new(), None, None, 0, events.clone());
        return json!({
            "protocol": "cmux-extension-sidebar-snapshot",
            "version": 1,
            "window_id": Value::Null,
            "window_ref": Value::Null,
            "selected_workspace_id": Value::Null,
            "selected_workspace_ref": Value::Null,
            "selectedId": Value::Null,
            "selectedTitle": Value::Null,
            "workspace_count": 0,
            "workspaceCount": 0,
            "unread_total": 0,
            "unreadTotal": 0,
            "seq": latest_seq,
            "latest_seq": latest_seq,
            "events": events,
            "assets": assets,
            "data": data,
            "workspaces": [],
            "workspace_groups": [],
        });
    };

    let selected = selected_workspace_index(snapshot);
    let workspaces: Vec<Value> = window
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .map(|(index, workspace)| extension_sidebar_workspace(workspace, index, index == selected))
        .collect();
    let selected_workspace = window.tab_manager.workspaces.get(selected);

    let selected_workspace_id =
        selected_workspace.and_then(|workspace| workspace.workspace_id.clone());
    let selected_title = selected_workspace.map(workspace_display_name);
    let unread_total = workspaces
        .iter()
        .filter_map(|workspace| workspace.get("unread").and_then(Value::as_u64))
        .sum::<u64>();
    let data = extension_sidebar_data_context(
        window
            .tab_manager
            .workspaces
            .iter()
            .enumerate()
            .map(|(index, workspace)| {
                extension_sidebar_data_workspace(workspace, index, index == selected)
            })
            .collect(),
        selected_workspace_id.as_deref(),
        selected_title.as_deref(),
        unread_total,
        events.clone(),
    );

    json!({
        "protocol": "cmux-extension-sidebar-snapshot",
        "version": 1,
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "selected_workspace_id": selected_workspace_id.clone(),
        "selected_workspace_ref": selected_workspace.map(|_| workspace_ref(selected)),
        "selectedId": selected_workspace_id,
        "selected_title": selected_title.clone(),
        "selectedTitle": selected_title,
        "workspace_count": window.tab_manager.workspaces.len(),
        "workspaceCount": window.tab_manager.workspaces.len(),
        "unread_total": unread_total,
        "unreadTotal": unread_total,
        "seq": latest_seq,
        "latest_seq": latest_seq,
        "events": events,
        "assets": assets,
        "data": data,
        "workspaces": workspaces,
        "workspace_groups": workspace_group_summaries(&window.tab_manager.workspaces, &window.tab_manager.workspace_groups),
    })
}

fn extension_sidebar_events_context(app: &AppHandle) -> Value {
    let Some(state) = app.try_state::<ControlEventState>() else {
        return extension_sidebar_empty_events_context();
    };
    let (boot_id, next_seq, retained_events) = {
        let guard = state
            .inner
            .lock()
            .expect("control event log mutex poisoned");
        (
            guard.boot_id.clone(),
            guard.next_seq,
            guard.events.iter().cloned().collect::<Vec<_>>(),
        )
    };
    extension_sidebar_events_context_from_retained(boot_id, next_seq, retained_events)
}

fn extension_sidebar_empty_events_context() -> Value {
    extension_sidebar_events_context_from_retained(String::new(), 1, Vec::new())
}

fn extension_sidebar_events_context_from_retained(
    boot_id: String,
    next_seq: u64,
    retained_events: Vec<Value>,
) -> Value {
    let latest_seq = next_seq.saturating_sub(1);
    let oldest_seq = retained_events
        .first()
        .and_then(|event| event.get("seq"))
        .and_then(Value::as_u64)
        .unwrap_or(next_seq);
    let latest = retained_events.last().cloned().unwrap_or(Value::Null);
    let mut category_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut name_counts: BTreeMap<String, u64> = BTreeMap::new();
    for event in &retained_events {
        if let Some(category) = event.get("category").and_then(Value::as_str) {
            *category_counts.entry(category.to_string()).or_default() += 1;
        }
        if let Some(name) = event.get("name").and_then(Value::as_str) {
            *name_counts.entry(name.to_string()).or_default() += 1;
        }
    }
    let retained_count = retained_events.len();
    let recent_start = retained_events.len().saturating_sub(50);
    let recent: Vec<Value> = retained_events.into_iter().skip(recent_start).collect();
    json!({
        "protocol": EVENT_STREAM_PROTOCOL,
        "version": EVENT_STREAM_VERSION,
        "boot_id": if boot_id.is_empty() { Value::Null } else { json!(boot_id) },
        "latest_seq": latest_seq,
        "seq": latest_seq,
        "next_seq": next_seq,
        "oldest_seq": oldest_seq,
        "retained_count": retained_count,
        "latest": latest,
        "recent": recent,
        "counts": category_counts,
        "category_counts": category_counts,
        "name_counts": name_counts,
    })
}

fn sidebar_validate(params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let name = sidebar_name_param(params);
    validate_custom_sidebars_in_dir(&custom_sidebar_directory(), name.as_deref())
}

fn sidebar_open(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(name) = sidebar_name_param(params) else {
        return invalid_params("Missing custom sidebar name");
    };
    let dir = custom_sidebar_directory();
    let candidate = match custom_sidebar_candidate_for_name(&dir, &name) {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: format!("Custom sidebar '{name}' was not found in {}", dir.display()),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            }
        }
        Err(error) => {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Failed to inspect custom sidebars: {error}"),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            }
        }
    };
    let validation = validate_custom_sidebar_candidate(&candidate);
    if !validation
        .get("valid")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return ControlCallResult::Err {
            code: "invalid_sidebar".to_string(),
            message: format!("Custom sidebar '{name}' did not validate"),
            data: Some(validation.try_into().unwrap_or(JsonValue::Null)),
        };
    }
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    let path = candidate.path.to_string_lossy().to_string();
    match open_custom_sidebar_in_panel(app, &state, &panel_id, &path) {
        Some(snapshot) => {
            let surface = surface_list_from_params(&snapshot, params);
            ok(json!({
                "accepted": true,
                "name": candidate.name,
                "kind": candidate.kind,
                "path": path,
                "surface_id": panel_id,
                "surface": match surface {
                    ControlCallResult::Ok(value) => Value::from(value),
                    _ => Value::Null,
                },
                "warnings": validation.get("warnings").cloned().unwrap_or_else(|| json!([])),
            }))
        }
        None => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to open custom sidebar in pane {panel_id}"),
            data: None,
        },
    }
}

fn sidebar_reload(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let name = sidebar_name_param(params);
    let validation =
        match validate_custom_sidebars_in_dir(&custom_sidebar_directory(), name.as_deref()) {
            ControlCallResult::Ok(value) => Value::from(value),
            error => return error,
        };

    if name.is_some()
        && !validation
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return ControlCallResult::Err {
            code: "invalid_sidebar".to_string(),
            message: format!(
                "Custom sidebar '{}' did not validate",
                name.as_deref().unwrap_or_default()
            ),
            data: Some(validation.try_into().unwrap_or(JsonValue::Null)),
        };
    }

    let payload = custom_sidebar_reload_payload(name.as_deref(), &validation);
    if let Err(error) = app.emit(CUSTOM_SIDEBAR_RELOAD_EVENT, payload.clone()) {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Failed to emit custom sidebar reload event: {error}"),
            data: None,
        };
    }

    ok(json!({
        "accepted": true,
        "event": CUSTOM_SIDEBAR_RELOAD_EVENT,
        "name": name,
        "all": payload.get("all").cloned().unwrap_or(Value::Bool(false)),
        "paths": payload.get("paths").cloned().unwrap_or_else(|| json!([])),
        "sidebars": payload.get("sidebars").cloned().unwrap_or_else(|| json!([])),
        "validation": validation,
    }))
}

fn custom_sidebar_reload_payload(name: Option<&str>, validation: &Value) -> Value {
    let sidebars: Vec<Value> = validation
        .get("sidebars")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|sidebar| {
            sidebar
                .get("valid")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    let paths: Vec<Value> = sidebars
        .iter()
        .filter_map(|sidebar| sidebar.get("path").and_then(Value::as_str))
        .map(|path| json!(path))
        .collect();
    json!({
        "protocol": "cmux-custom-sidebar-reload",
        "version": 1,
        "event": CUSTOM_SIDEBAR_RELOAD_EVENT,
        "name": name,
        "all": name.is_none(),
        "paths": paths,
        "sidebars": sidebars,
    })
}

fn sidebar_select(app: &AppHandle, params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let Some(name) = sidebar_name_param(params) else {
        return invalid_params("Missing custom sidebar name");
    };
    let dir = custom_sidebar_directory();
    let candidate = match custom_sidebar_candidate_for_name(&dir, &name) {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: format!("Custom sidebar '{name}' was not found in {}", dir.display()),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        Err(error) => {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Failed to inspect custom sidebars: {error}"),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    let validation = validate_custom_sidebar_candidate(&candidate);
    if !validation
        .get("valid")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return ControlCallResult::Err {
            code: "invalid_sidebar".to_string(),
            message: format!("Custom sidebar '{name}' did not validate"),
            data: Some(validation.try_into().unwrap_or(JsonValue::Null)),
        };
    }

    let payload = custom_sidebar_select_payload(&candidate, &validation);
    if let Err(error) = app.emit(CUSTOM_SIDEBAR_SELECT_EVENT, payload.clone()) {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Failed to emit custom sidebar select event: {error}"),
            data: None,
        };
    }

    ok(payload)
}

fn custom_sidebar_select_payload(candidate: &CustomSidebarCandidate, validation: &Value) -> Value {
    let path = candidate.path.to_string_lossy().to_string();
    json!({
        "accepted": true,
        "protocol": "cmux-custom-sidebar-select",
        "version": 1,
        "event": CUSTOM_SIDEBAR_SELECT_EVENT,
        "name": candidate.name,
        "kind": candidate.kind,
        "path": path,
        "sidebar": validation,
        "warnings": validation.get("warnings").cloned().unwrap_or_else(|| json!([])),
    })
}

fn sidebar_name_param(params: &serde_json::Map<String, Value>) -> Option<String> {
    string_param(params, &["name", "sidebar", "id"])
        .map(|name| {
            Path::new(name.trim())
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_else(|| name.trim())
                .to_string()
        })
        .filter(|name| !name.is_empty())
}

pub(crate) fn custom_sidebar_directory() -> PathBuf {
    std::env::var_os("CMUX_SIDEBARS_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            event_log_home_directory()
                .map(|home| home.join(".config").join("cmux").join("sidebars"))
        })
        .unwrap_or_else(|| {
            PathBuf::from(".")
                .join(".config")
                .join("cmux")
                .join("sidebars")
        })
}

fn custom_sidebar_asset_map_from_params(params: &serde_json::Map<String, Value>) -> Value {
    let source_path = string_param(params, &["source_path", "sourcePath", "path"]);
    custom_sidebar_asset_map_for_source(source_path.as_deref())
}

fn custom_sidebar_asset_map_for_source(source_path: Option<&str>) -> Value {
    let Some(source_path) = canonical_custom_sidebar_source(source_path) else {
        return json!({});
    };
    let Some(asset_root) = custom_sidebar_asset_root_for_source(&source_path) else {
        return json!({});
    };
    let sidebar_name = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("sidebar");
    let mut assets = serde_json::Map::new();
    collect_custom_sidebar_assets(
        &source_path,
        &asset_root,
        &asset_root,
        sidebar_name,
        &mut assets,
        0,
    );
    Value::Object(assets)
}

fn canonical_custom_sidebar_source(source_path: Option<&str>) -> Option<PathBuf> {
    let source_path = PathBuf::from(source_path?.trim());
    let source_path = fs::canonicalize(source_path).ok()?;
    if !source_path.is_file() {
        return None;
    }
    let sidebar_dir = fs::canonicalize(custom_sidebar_directory()).ok()?;
    source_path.starts_with(sidebar_dir).then_some(source_path)
}

fn custom_sidebar_asset_root_for_source(source_path: &Path) -> Option<PathBuf> {
    let dir = source_path.parent()?;
    let stem = source_path.file_stem()?.to_str()?;
    let asset_root = dir.join(format!("{stem}.assets"));
    let asset_root = fs::canonicalize(asset_root).ok()?;
    asset_root.is_dir().then_some(asset_root)
}

fn collect_custom_sidebar_assets(
    source_path: &Path,
    asset_root: &Path,
    current_dir: &Path,
    sidebar_name: &str,
    assets: &mut serde_json::Map<String, Value>,
    depth: usize,
) {
    if depth > 4 || assets.len() >= 256 {
        return;
    }
    let Ok(entries) = fs::read_dir(current_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if assets.len() >= 256 {
            return;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_custom_sidebar_assets(
                source_path,
                asset_root,
                &path,
                sidebar_name,
                assets,
                depth + 1,
            );
            continue;
        }
        if !path.is_file() || custom_sidebar_asset_mime(&path).is_none() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(asset_root) else {
            continue;
        };
        let relative_name = relative.to_string_lossy().replace('\\', "/");
        if relative_name.is_empty() || relative_name.contains("..") {
            continue;
        }
        let url = custom_sidebar_asset_url(source_path, sidebar_name, &relative_name);
        assets.insert(relative_name.clone(), json!(url));
        if let Some(stem) = relative_name.rsplit_once('.').map(|(stem, _)| stem) {
            assets.entry(stem.to_string()).or_insert_with(|| json!(url));
        }
    }
}

fn custom_sidebar_asset_url(source_path: &Path, sidebar_name: &str, relative_name: &str) -> String {
    format!(
        "cmux-sidebar-asset://{}/{}?source={}",
        percent_encode_component(sidebar_name),
        relative_name
            .split('/')
            .map(percent_encode_component)
            .collect::<Vec<_>>()
            .join("/"),
        percent_encode_component(&source_path.to_string_lossy()),
    )
}

pub(crate) fn resolve_custom_sidebar_asset_request(uri: &str) -> Option<(PathBuf, String)> {
    let rest = strip_custom_sidebar_asset_scheme(uri)?;
    let (path_part, query) = rest.split_once('?')?;
    if path_part.contains('#') || query.contains('#') {
        return None;
    }
    let source_path = query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "source").then(|| percent_decode_component(value))
    })?;
    let source_path = canonical_custom_sidebar_source(Some(&source_path))?;
    let asset_root = custom_sidebar_asset_root_for_source(&source_path)?;
    let relative = path_part
        .split_once('/')
        .map(|(_, relative)| relative)
        .unwrap_or_default();
    if relative.is_empty() || relative.contains("..") || relative.contains('\\') {
        return None;
    }
    let decoded_segments: Vec<String> = relative
        .split('/')
        .map(percent_decode_component)
        .filter(|segment| !segment.is_empty() && segment != "." && segment != "..")
        .collect();
    if decoded_segments.is_empty() {
        return None;
    }
    let mut candidate = asset_root.clone();
    for segment in decoded_segments {
        candidate.push(segment);
    }
    let candidate = fs::canonicalize(candidate).ok()?;
    if !candidate.starts_with(&asset_root) || !candidate.is_file() {
        return None;
    }
    let mime = custom_sidebar_asset_mime(&candidate)?;
    Some((candidate, mime.to_string()))
}

fn strip_custom_sidebar_asset_scheme(uri: &str) -> Option<&str> {
    if let Some(rest) = uri.strip_prefix("cmux-sidebar-asset://") {
        return Some(rest);
    }
    let after = uri
        .strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))?;
    after.strip_prefix("cmux-sidebar-asset.localhost/")
}

fn custom_sidebar_asset_mime(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "avif" => Some("image/avif"),
        "ico" => Some("image/x-icon"),
        _ => None,
    }
}

fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        let b = *byte;
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    encoded
}

fn percent_decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                decoded.push(hex);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).to_string()
}

fn validate_custom_sidebars_in_dir(dir: &Path, name: Option<&str>) -> ControlCallResult {
    let normalized_name = name.map(str::trim).filter(|name| !name.is_empty());
    let candidates = match discover_custom_sidebars(dir, normalized_name) {
        Ok(candidates) => candidates,
        Err(error) => {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Failed to inspect custom sidebars: {error}"),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };

    if normalized_name.is_some() && candidates.is_empty() {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!(
                "Custom sidebar '{}' was not found in {}",
                normalized_name.unwrap_or_default(),
                dir.display()
            ),
            data: Some(
                json!({ "sidebar_dir": dir })
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }

    let sidebars: Vec<Value> = candidates
        .iter()
        .map(|candidate| validate_custom_sidebar_candidate(candidate))
        .collect();
    let valid_count = sidebars
        .iter()
        .filter(|sidebar| {
            sidebar
                .get("valid")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let invalid_count = sidebars.len().saturating_sub(valid_count);
    ok(json!({
        "protocol": "cmux-custom-sidebar-validation",
        "version": 1,
        "sidebar_dir": dir,
        "exists": dir.is_dir(),
        "name": normalized_name,
        "ok": invalid_count == 0,
        "valid_count": valid_count,
        "invalid_count": invalid_count,
        "sidebars": sidebars,
    }))
}

#[derive(Debug, Clone)]
struct CustomSidebarCandidate {
    name: String,
    path: PathBuf,
    kind: String,
    shadowed_json_path: Option<PathBuf>,
    manifest_path: Option<PathBuf>,
}

fn discover_custom_sidebars(
    dir: &Path,
    name: Option<&str>,
) -> std::io::Result<Vec<CustomSidebarCandidate>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut by_name: BTreeMap<String, CustomSidebarCandidate> = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(extension) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
        else {
            continue;
        };
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".manifest.json"))
        {
            continue;
        }
        if extension != "swift" && extension != "json" {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if name.is_some_and(|name| name != stem) {
            continue;
        }
        let candidate = CustomSidebarCandidate {
            name: stem.to_string(),
            kind: extension.clone(),
            path: path.clone(),
            shadowed_json_path: None,
            manifest_path: custom_sidebar_manifest_path(dir, stem),
        };
        match by_name.get_mut(stem) {
            Some(existing) if existing.kind == "swift" && extension == "json" => {
                existing.shadowed_json_path = Some(path);
            }
            Some(existing) if existing.kind == "json" && extension == "swift" => {
                let shadowed_json_path = Some(existing.path.clone());
                *existing = CustomSidebarCandidate {
                    shadowed_json_path,
                    ..candidate
                };
            }
            Some(_) => {}
            None => {
                by_name.insert(stem.to_string(), candidate);
            }
        }
    }
    Ok(by_name.into_values().collect())
}

fn custom_sidebar_candidate_for_name(
    dir: &Path,
    name: &str,
) -> std::io::Result<Option<CustomSidebarCandidate>> {
    Ok(discover_custom_sidebars(dir, Some(name))?
        .into_iter()
        .next())
}

fn validate_custom_sidebar_candidate(candidate: &CustomSidebarCandidate) -> Value {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let manifest = candidate
        .manifest_path
        .as_ref()
        .map(|path| custom_sidebar_manifest_summary(path));
    if manifest
        .as_ref()
        .and_then(|manifest| manifest.get("valid"))
        .and_then(Value::as_bool)
        == Some(false)
    {
        warnings.push("custom sidebar capability manifest is invalid".to_string());
    }
    match fs::read_to_string(&candidate.path) {
        Ok(source) if source.trim().is_empty() => {
            errors.push("sidebar file is empty".to_string());
        }
        Ok(source) if candidate.kind == "json" => {
            if let Err(error) = serde_json::from_str::<Value>(&source) {
                errors.push(format!("invalid JSON: {error}"));
            }
        }
        Ok(_) if candidate.kind == "swift" => {
            warnings.push(
                "SwiftUI syntax interpretation is not yet available on Windows/Tauri".to_string(),
            );
        }
        Ok(_) => {}
        Err(error) => {
            errors.push(format!("failed to read sidebar file: {error}"));
        }
    }
    json!({
        "name": candidate.name,
        "kind": candidate.kind,
        "path": candidate.path,
        "valid": errors.is_empty(),
        "errors": errors,
        "warnings": warnings,
        "shadowed_json_path": candidate.shadowed_json_path,
        "manifest_path": candidate.manifest_path,
        "manifest": manifest,
    })
}

fn custom_sidebar_manifest_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let path = dir.join(format!("{name}.manifest.json"));
    path.is_file().then_some(path)
}

fn custom_sidebar_manifest_for_source(source_path: Option<&str>) -> Option<Value> {
    let source_path = PathBuf::from(source_path?.trim());
    let dir = source_path.parent()?;
    let stem = source_path.file_stem()?.to_str()?;
    let manifest_path = custom_sidebar_manifest_path(dir, stem)?;
    Some(custom_sidebar_manifest_summary(&manifest_path))
}

fn custom_sidebar_manifest_summary(path: &Path) -> Value {
    let mut errors = Vec::new();
    let mut requested_methods = Vec::new();
    let mut trusted = false;

    match fs::read_to_string(path) {
        Ok(source) => match serde_json::from_str::<Value>(&source) {
            Ok(Value::Object(manifest)) => {
                trusted = manifest
                    .get("trusted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                for key in ["capabilities", "allowed_methods", "methods"] {
                    if let Some(Value::Array(values)) = manifest.get(key) {
                        requested_methods.extend(values.iter().filter_map(|value| {
                            value
                                .as_str()
                                .map(str::trim)
                                .filter(|method| !method.is_empty())
                                .map(str::to_string)
                        }));
                    }
                }
            }
            Ok(_) => errors.push("manifest must be a JSON object".to_string()),
            Err(error) => errors.push(format!("invalid manifest JSON: {error}")),
        },
        Err(error) => errors.push(format!("failed to read manifest: {error}")),
    }

    requested_methods.sort();
    requested_methods.dedup();
    let allowed_requested_methods: Vec<String> = requested_methods
        .iter()
        .filter(|method| custom_sidebar_action_policy_allows(method))
        .cloned()
        .collect();
    let denied_requested_methods: Vec<String> = requested_methods
        .iter()
        .filter(|method| !custom_sidebar_action_policy_allows(method))
        .cloned()
        .collect();

    json!({
        "path": path,
        "valid": errors.is_empty(),
        "errors": errors,
        "trusted": trusted,
        "requested_methods": requested_methods,
        "allowed_requested_methods": allowed_requested_methods,
        "denied_requested_methods": denied_requested_methods,
        "policy": CUSTOM_SIDEBAR_ACTION_POLICY,
        "enforced": true,
    })
}

fn extension_sidebar_data_context(
    workspaces: Vec<Value>,
    selected_workspace_id: Option<&str>,
    selected_title: Option<&str>,
    unread_total: u64,
    events: Value,
) -> Value {
    json!({
        "workspaces": workspaces,
        "workspaceCount": workspaces.len(),
        "selectedTitle": selected_title.unwrap_or(""),
        "selectedId": selected_workspace_id.unwrap_or(""),
        "unreadTotal": unread_total,
        "clock": extension_sidebar_clock_context(),
        "events": events,
    })
}

fn extension_sidebar_clock_context() -> Value {
    let now = OffsetDateTime::now_utc();
    json!({
        "time": format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second()),
        "hour": now.hour(),
        "minute": now.minute(),
        "second": now.second(),
        "weekday": now.weekday().number_from_sunday(),
        "epoch": now.unix_timestamp(),
    })
}

fn extension_sidebar_data_workspace(
    workspace: &SessionWorkspaceSnapshot,
    index: usize,
    selected: bool,
) -> Value {
    let tabs = extension_sidebar_data_tabs(workspace);
    let ports = workspace_listening_ports(workspace);
    let unread = tabs
        .iter()
        .filter(|tab| tab.get("unread").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let pull_requests = extension_sidebar_pull_requests(workspace);
    let (branch, dirty) = extension_sidebar_branch_summary(workspace);
    let mut object = serde_json::Map::new();

    if let Some(id) = workspace.workspace_id.as_deref() {
        object.insert("id".to_string(), json!(id));
    }
    object.insert(
        "title".to_string(),
        json!(workspace_display_name(workspace)),
    );
    object.insert("selected".to_string(), json!(selected));
    object.insert(
        "pinned".to_string(),
        json!(workspace.is_pinned.unwrap_or(false)),
    );
    object.insert("index".to_string(), json!(index));
    object.insert(
        "directory".to_string(),
        json!(workspace.current_directory.clone().unwrap_or_default()),
    );
    object.insert("ports".to_string(), json!(ports));
    object.insert("portCount".to_string(), json!(ports.len()));
    object.insert("unread".to_string(), json!(unread));
    object.insert("tabs".to_string(), json!(tabs));
    object.insert(
        "tabCount".to_string(),
        json!(surfaces_for_workspace(workspace).len()),
    );

    insert_non_empty_string(
        &mut object,
        "description",
        workspace.custom_description.as_deref(),
    );
    insert_non_empty_string(&mut object, "color", workspace.custom_color.as_deref());
    if let Some(branch) = branch {
        object.insert("branch".to_string(), json!(branch));
        object.insert("dirty".to_string(), json!(dirty));
    }
    if let Some(first_pull_request) = pull_requests.first() {
        object.insert("pr".to_string(), first_pull_request.clone());
        object.insert("prs".to_string(), json!(pull_requests));
    }
    if let Some(progress) = workspace.sidebar_progress.as_ref() {
        let mut progress_object = serde_json::Map::new();
        progress_object.insert("value".to_string(), json!(progress.value));
        insert_non_empty_string(&mut progress_object, "label", progress.label.as_deref());
        object.insert("progress".to_string(), Value::Object(progress_object));
    }
    if let Some(remote) = workspace.remote.as_ref() {
        let target = remote
            .destination
            .as_deref()
            .or(remote.detail.as_deref())
            .unwrap_or("");
        object.insert(
            "remote".to_string(),
            json!({
                "target": target,
                "state": remote.state,
                "connected": remote.connected,
            }),
        );
    }

    Value::Object(object)
}

fn extension_sidebar_data_tabs(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    surfaces_for_workspace(workspace)
        .into_iter()
        .map(|surface| {
            let panel_id = surface.get("id").and_then(Value::as_str);
            let mut object = serde_json::Map::new();
            if let Some(panel_id) = panel_id {
                object.insert("id".to_string(), json!(panel_id));
            }
            object.insert(
                "title".to_string(),
                surface
                    .get("title")
                    .cloned()
                    .unwrap_or_else(|| json!("terminal")),
            );
            object.insert(
                "focused".to_string(),
                json!(surface
                    .get("focused")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)),
            );
            object.insert(
                "pinned".to_string(),
                json!(surface
                    .get("pinned")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)),
            );
            insert_non_empty_string(
                &mut object,
                "directory",
                workspace.current_directory.as_deref(),
            );
            if let Some(panel_id) = panel_id {
                if let Some((branch, dirty)) = extension_sidebar_panel_branch(workspace, panel_id) {
                    object.insert("branch".to_string(), json!(branch));
                    object.insert("dirty".to_string(), json!(dirty));
                }
                let ports = panel_listening_ports(workspace, panel_id);
                if !ports.is_empty() {
                    object.insert("ports".to_string(), json!(ports));
                }
            }
            object.insert(
                "unread".to_string(),
                json!(surface
                    .get("unread")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)),
            );
            Value::Object(object)
        })
        .collect()
}

fn insert_non_empty_string(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        object.insert(key.to_string(), json!(value));
    }
}

fn extension_sidebar_workspace(
    workspace: &SessionWorkspaceSnapshot,
    index: usize,
    selected: bool,
) -> Value {
    let mut summary = workspace_summary(workspace, index, selected);
    let tabs = extension_sidebar_tabs(workspace);
    let ports = workspace_listening_ports(workspace);
    let unread = tabs
        .iter()
        .filter(|tab| tab.get("unread").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let pull_requests = extension_sidebar_pull_requests(workspace);
    let first_pull_request = pull_requests.first().cloned();
    let (branch, dirty) = extension_sidebar_branch_summary(workspace);

    if let Some(object) = summary.as_object_mut() {
        object.insert("directory".to_string(), json!(workspace.current_directory));
        object.insert("root_path".to_string(), json!(workspace.current_directory));
        object.insert(
            "project_root_path".to_string(),
            json!(workspace.current_directory),
        );
        object.insert("ports".to_string(), json!(ports));
        object.insert("port_count".to_string(), json!(ports.len()));
        object.insert("portCount".to_string(), json!(ports.len()));
        object.insert("tabs".to_string(), json!(tabs));
        object.insert(
            "tab_count".to_string(),
            json!(surfaces_for_workspace(workspace).len()),
        );
        object.insert(
            "tabCount".to_string(),
            json!(surfaces_for_workspace(workspace).len()),
        );
        object.insert("unread".to_string(), json!(unread));
        object.insert("branch".to_string(), json!(branch));
        object.insert("dirty".to_string(), json!(dirty));
        object.insert(
            "branch_summary".to_string(),
            json!(branch.map(|branch| {
                if dirty {
                    format!("{branch}*")
                } else {
                    branch
                }
            })),
        );
        object.insert("pr".to_string(), first_pull_request.unwrap_or(Value::Null));
        object.insert("prs".to_string(), json!(pull_requests));
        object.insert(
            "pull_request_urls".to_string(),
            json!(extension_sidebar_pull_request_urls(workspace)),
        );
        object.insert(
            "panel_directories".to_string(),
            json!(extension_sidebar_panel_directories(workspace)),
        );
        object.insert(
            "git_branches".to_string(),
            json!(workspace.panel_git_branches.clone().unwrap_or_default()),
        );
        object.insert("progress".to_string(), json!(workspace.sidebar_progress));
        object.insert("latestMessage".to_string(), Value::Null);
        object.insert("latestPrompt".to_string(), Value::Null);
        object.insert("latestAt".to_string(), Value::Null);
    }

    summary
}

fn extension_sidebar_tabs(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    surfaces_for_workspace(workspace)
        .into_iter()
        .map(|mut surface| {
            let panel_id = surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(object) = surface.as_object_mut() {
                object.insert(
                    "directory".to_string(),
                    json!(workspace.current_directory.clone()),
                );
                object.insert(
                    "ports".to_string(),
                    panel_id
                        .as_deref()
                        .map(|panel_id| json!(panel_listening_ports(workspace, panel_id)))
                        .unwrap_or_else(|| json!([])),
                );
                object.insert(
                    "branch".to_string(),
                    panel_id
                        .as_deref()
                        .and_then(|panel_id| extension_sidebar_panel_branch(workspace, panel_id))
                        .map(|(branch, _dirty)| branch)
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                );
                object.insert(
                    "dirty".to_string(),
                    json!(panel_id.as_deref().is_some_and(|panel_id| {
                        extension_sidebar_panel_branch(workspace, panel_id)
                            .map(|(_branch, dirty)| dirty)
                            .unwrap_or(false)
                    })),
                );
            }
            surface
        })
        .collect()
}

fn extension_sidebar_branch_summary(
    workspace: &SessionWorkspaceSnapshot,
) -> (Option<String>, bool) {
    if let Some(entry) = workspace
        .panel_git_branches
        .as_ref()
        .and_then(|branches| branches.first())
    {
        return (Some(entry.branch.clone()), entry.is_dirty);
    }
    workspace
        .git_branch
        .as_ref()
        .map(|branch| (Some(branch.branch.clone()), branch.is_dirty))
        .unwrap_or((None, false))
}

fn extension_sidebar_panel_branch(
    workspace: &SessionWorkspaceSnapshot,
    panel_id: &str,
) -> Option<(String, bool)> {
    workspace
        .panel_git_branches
        .as_ref()
        .and_then(|branches| {
            branches
                .iter()
                .find(|entry| entry.panel_id == panel_id)
                .map(|entry| (entry.branch.clone(), entry.is_dirty))
        })
        .or_else(|| {
            workspace
                .git_branch
                .as_ref()
                .map(|entry| (entry.branch.clone(), entry.is_dirty))
        })
}

fn extension_sidebar_pull_requests(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    workspace
        .panel_pull_requests
        .as_ref()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    json!({
                        "panel_id": entry.panel_id,
                        "number": entry.number,
                        "label": entry.label,
                        "url": entry.url,
                        "status": entry.status,
                        "stale": entry.is_stale,
                        "is_stale": entry.is_stale,
                        "branch": entry.branch,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extension_sidebar_pull_request_urls(workspace: &SessionWorkspaceSnapshot) -> Vec<String> {
    workspace
        .panel_pull_requests
        .as_ref()
        .map(|entries| entries.iter().map(|entry| entry.url.clone()).collect())
        .unwrap_or_default()
}

fn extension_sidebar_panel_directories(
    workspace: &SessionWorkspaceSnapshot,
) -> BTreeMap<String, String> {
    workspace
        .current_directory
        .as_ref()
        .map(|directory| {
            surfaces_for_workspace(workspace)
                .into_iter()
                .filter_map(|surface| {
                    surface
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|panel_id| (panel_id.to_string(), directory.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn workspace_current(snapshot: &AppSessionSnapshot) -> ControlCallResult {
    let params = serde_json::Map::new();
    workspace_current_from_params(snapshot, &params)
}

fn workspace_current_from_params(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(window_index) = workspace_routed_window_index(snapshot, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let Some(window) = snapshot.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let Some(index) = selected_workspace_index_for_window(snapshot, window_index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No workspace selected".to_string(),
            data: None,
        };
    };
    let workspace = window.tab_manager.workspaces.get(index);
    let identity_id = workspace
        .and_then(|workspace| workspace.workspace_id.clone())
        .or_else(|| window.selected_workspace_id.clone());
    let Some(identity_id) = identity_id else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No workspace selected".to_string(),
            data: None,
        };
    };
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| window_ref(window_index)),
        "workspace_id": identity_id,
        "workspace_ref": workspace_ref(index),
        "workspace": workspace.map(|workspace| canonical_workspace_summary(workspace, index, true)),
    }))
}

fn workspace_current_from_params_for_app(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    match workspace_current_from_params(snapshot, params) {
        ControlCallResult::Ok(value) => {
            let mut payload: Value = value.into();
            apply_workspace_handle_refs(app, &mut payload);
            ok(payload)
        }
        error => error,
    }
}

fn apply_workspace_handle_refs(app: &AppHandle, payload: &mut Value) {
    if let Some(window_id) = payload.get("window_id").and_then(Value::as_str) {
        payload["window_ref"] = json!(control_handle_ref(app, "window", window_id));
    }
    if let Some(workspaces) = payload.get_mut("workspaces").and_then(Value::as_array_mut) {
        for workspace in workspaces {
            if let Some(id) = workspace
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                workspace["ref"] = json!(control_handle_ref(app, "workspace", &id));
            }
        }
    }
    if let Some(workspace) = payload.get_mut("workspace") {
        if let Some(id) = workspace
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            workspace["ref"] = json!(control_handle_ref(app, "workspace", &id));
        }
    }
    if let Some(id) = payload
        .get("workspace_id")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        payload["workspace_ref"] = json!(control_handle_ref(app, "workspace", &id));
    }
}

fn workspace_from_params_or_selected<'a>(
    snapshot: &'a AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<&'a SessionWorkspaceSnapshot> {
    let index = workspace_index_from_params_or_selected(snapshot, params)?;
    snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(index))
}

fn recent_sidebar_log_entries(workspace: &SessionWorkspaceSnapshot, limit: usize) -> Vec<Value> {
    let entries = workspace.sidebar_log_entries.as_deref().unwrap_or_default();
    let start = entries.len().saturating_sub(limit);
    entries[start..]
        .iter()
        .rev()
        .map(|entry| json!(entry))
        .collect()
}

#[allow(dead_code)]
fn surface_list(snapshot: &AppSessionSnapshot) -> ControlCallResult {
    let params = serde_json::Map::new();
    surface_list_from_params(snapshot, &params)
}

fn surface_list_from_params(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(window) = snapshot.windows.first() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(index) = workspace_index_from_workspace_scope_or_selected(snapshot, params) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(index),
        "surfaces": surfaces_for_workspace(workspace),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

fn selected_workspace_index(snapshot: &AppSessionSnapshot) -> usize {
    snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.selected_workspace_index)
        .and_then(|index| usize::try_from(index).ok())
        .unwrap_or(0)
}

fn selected_workspace_index_for_window(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
) -> Option<usize> {
    snapshot
        .windows
        .get(window_index)?
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
}

fn window_ref(index: usize) -> String {
    format!("window:{}", index + 1)
}

/// Resolve the v2 routing selectors to a tab manager without changing focus.
/// An explicit window selector is authoritative: an invalid value never falls
/// through to a workspace/surface in another window.
fn workspace_routed_window_index(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    let has_non_null_window_selector = params
        .get("window_id")
        .or_else(|| params.get("window_ref"))
        .is_some_and(|value| !value.is_null());
    if has_non_null_window_selector {
        let selector = raw_string_param(params, &["window_id", "window_ref"])?;
        if let Some(index) = one_based_ref_index(&selector, "window") {
            return (index < snapshot.windows.len()).then_some(index);
        }
        return snapshot
            .windows
            .iter()
            .position(|window| window.window_id.as_deref() == Some(selector.as_str()));
    }

    if let Some(group_id) = string_param(params, &["group_id"]) {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window
                .tab_manager
                .workspaces
                .iter()
                .any(|workspace| workspace.group_id.as_deref() == Some(group_id.as_str()))
        }) {
            return Some(index);
        }
    }

    if let Some(workspace_id) = string_param(params, &["workspace_id"]) {
        if one_based_ref_index(&workspace_id, "workspace").is_none() {
            if let Some(index) = snapshot.windows.iter().position(|window| {
                window.tab_manager.workspaces.iter().any(|workspace| {
                    workspace.workspace_id.as_deref() == Some(workspace_id.as_str())
                })
            }) {
                return Some(index);
            }
        }
    }

    if let Some(surface_id) = string_param(params, &["surface_id", "terminal_id", "tab_id"]) {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window.tab_manager.workspaces.iter().any(|workspace| {
                surfaces_for_workspace(workspace).iter().any(|surface| {
                    surface.get("id").and_then(Value::as_str) == Some(surface_id.as_str())
                })
            })
        }) {
            return Some(index);
        }
    }

    if let Some(pane_id) = string_param(params, &["pane_id"]) {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window.tab_manager.workspaces.iter().any(|workspace| {
                pane_event_summaries(workspace)
                    .iter()
                    .any(|pane| pane.id.as_deref() == Some(pane_id.as_str()))
            })
        }) {
            return Some(index);
        }
    }

    (!snapshot.windows.is_empty()).then_some(0)
}

fn canonical_workspace_target_index(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    let selector = raw_string_param(params, &["workspace_id"])?;
    let workspaces = &snapshot.windows.get(window_index)?.tab_manager.workspaces;
    if let Some(index) = one_based_ref_index(&selector, "workspace") {
        return (index < workspaces.len()).then_some(index);
    }
    workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(selector.as_str()))
}

fn workspace_index_for_id(snapshot: &AppSessionSnapshot, workspace_id: &str) -> Option<usize> {
    snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
}

fn workspace_index_from_params(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    if let Some(workspace_ref) = string_param(params, &["workspace_ref", "ref"]) {
        if let Some(index) = one_based_ref_index(&workspace_ref, "workspace") {
            if snapshot
                .windows
                .first()
                .is_some_and(|window| index < window.tab_manager.workspaces.len())
            {
                return Some(index);
            }
            return None;
        }
    }

    let workspace_id = params
        .get("workspace_id")
        .or_else(|| params.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    workspace_index_for_id(snapshot, workspace_id)
}

fn workspace_reorder_destination_index(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    from_index: usize,
) -> Option<i64> {
    if params.contains_key("to_index")
        || params.contains_key("to")
        || params.contains_key("target_index")
    {
        return None;
    }

    let index = i64_param(params, &["index"]);
    let before = workspace_index_from_selector_keys(
        snapshot,
        params,
        &["before_workspace_ref", "before_ref"],
        &["before_workspace_id", "before_workspace"],
    );
    let after = workspace_index_from_selector_keys(
        snapshot,
        params,
        &["after_workspace_ref", "after_ref"],
        &["after_workspace_id", "after_workspace"],
    );
    match (index, before, after) {
        (Some(index), None, None) => Some(index),
        (None, Some(target), None) => {
            let destination = if from_index < target {
                target.saturating_sub(1)
            } else {
                target
            };
            Some(destination as i64)
        }
        (None, None, Some(target)) => {
            let destination = if from_index < target {
                target
            } else {
                target.saturating_add(1)
            };
            Some(destination as i64)
        }
        _ => None,
    }
}

fn workspace_reorder_window_matches(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> bool {
    let Some(window) = snapshot.windows.first() else {
        return false;
    };
    let reference_matches = string_param(params, &["window_ref"])
        .map(|reference| one_based_ref_index(&reference, "window") == Some(0))
        .unwrap_or(true);
    let id_matches = string_param(params, &["window_id"])
        .map(|id| window.window_id.as_deref() == Some(id.as_str()))
        .unwrap_or(true);
    reference_matches && id_matches
}

fn workspace_index_from_selector_keys(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    ref_keys: &[&str],
    id_keys: &[&str],
) -> Option<usize> {
    if let Some(workspace_ref) = string_param(params, ref_keys) {
        let index = one_based_ref_index(&workspace_ref, "workspace")?;
        if snapshot
            .windows
            .first()
            .is_some_and(|window| index < window.tab_manager.workspaces.len())
        {
            return Some(index);
        }
        return None;
    }

    let workspace_id = string_param(params, id_keys)?;
    snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
}

fn workspace_indices_from_params(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<Vec<i64>> {
    let mut indices = Vec::new();

    if let Some(values) = params
        .get("workspace_refs")
        .or_else(|| params.get("refs"))
        .and_then(Value::as_array)
    {
        let parsed: Option<Vec<i64>> = values
            .iter()
            .map(|value| {
                let workspace_ref = value.as_str()?.trim();
                let index = one_based_ref_index(workspace_ref, "workspace")?;
                if !snapshot
                    .windows
                    .first()
                    .is_some_and(|window| index < window.tab_manager.workspaces.len())
                {
                    return None;
                }
                Some(index as i64)
            })
            .collect();
        indices.extend(parsed?);
    }

    if let Some(values) = params
        .get("workspace_ids")
        .or_else(|| params.get("ids"))
        .and_then(Value::as_array)
    {
        let window = snapshot.windows.first()?;
        let parsed: Option<Vec<i64>> = values
            .iter()
            .map(|value| {
                let workspace_id = value.as_str()?.trim();
                window
                    .tab_manager
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
                    .map(|index| index as i64)
            })
            .collect();
        indices.extend(parsed?);
    }

    if !indices.is_empty() {
        return Some(indices);
    }

    workspace_index_from_params(snapshot, params).map(|index| vec![index as i64])
}

fn workspace_index_from_params_or_selected(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    if params.is_empty()
        || !params.contains_key("workspace_id")
            && !params.contains_key("id")
            && !params.contains_key("workspace_ref")
            && !params.contains_key("ref")
    {
        let selected = selected_workspace_index(snapshot);
        if snapshot
            .windows
            .first()
            .is_some_and(|window| selected < window.tab_manager.workspaces.len())
        {
            return Some(selected);
        }
        return None;
    }
    workspace_index_from_params(snapshot, params)
}

fn workspace_index_from_workspace_scope_or_selected(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    if let Some(workspace_ref) = string_param(params, &["workspace_ref"]) {
        if let Some(index) = one_based_ref_index(&workspace_ref, "workspace") {
            if snapshot
                .windows
                .first()
                .is_some_and(|window| index < window.tab_manager.workspaces.len())
            {
                return Some(index);
            }
        }
        return None;
    }
    if let Some(workspace_id) = string_param(params, &["workspace_id"]) {
        return snapshot
            .windows
            .first()?
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| {
                workspace.workspace_id.as_deref() == Some(workspace_id.as_str())
            });
    }
    let selected = selected_workspace_index(snapshot);
    if snapshot
        .windows
        .first()
        .is_some_and(|window| selected < window.tab_manager.workspaces.len())
    {
        return Some(selected);
    }
    None
}

fn string_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        params
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

fn raw_string_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| params.get(*key).and_then(Value::as_str).map(str::to_owned))
}

fn string_vec_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<Vec<String>> {
    keys.iter().find_map(|key| {
        let value = params.get(*key)?;
        match value {
            Value::Array(values) => {
                let entries: Vec<String> = values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect();
                Some(entries)
            }
            Value::String(raw) => Some(
                raw.lines()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect(),
            ),
            _ => None,
        }
    })
}

fn string_map_param(
    params: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<BTreeMap<String, String>> {
    for key in keys {
        let Some(object) = params.get(*key).and_then(Value::as_object) else {
            continue;
        };
        let map: BTreeMap<String, String> = object
            .iter()
            .filter_map(|(key, value)| {
                let key = key.trim();
                if key.is_empty() {
                    return None;
                }
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| (key.to_string(), value.to_string()))
            })
            .collect();
        if !map.is_empty() {
            return Some(map);
        }
    }
    None
}

fn first_present_trimmed_string_map_param(
    params: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<BTreeMap<String, String>> {
    keys.iter().find_map(|key| {
        let object = params.get(*key)?.as_object()?;
        Some(
            object
                .iter()
                .filter_map(|(key, value)| {
                    let key = key.trim();
                    (!key.is_empty()).then(|| {
                        value
                            .as_str()
                            .map(|value| (key.to_string(), value.to_string()))
                    })?
                })
                .collect(),
        )
    })
}

fn bool_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| {
            value
                .as_bool()
                .or_else(|| value.as_f64().map(|number| number != 0.0))
                .or_else(|| {
                    let normalized = value.as_str()?.trim().to_ascii_lowercase();
                    match normalized.as_str() {
                        "1" | "true" | "yes" | "on" => Some(true),
                        "0" | "false" | "no" | "off" => Some(false),
                        _ => None,
                    }
                })
        })
    })
}

fn usize_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<usize> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| match value {
            Value::Number(number) => number
                .as_u64()
                .and_then(|value| usize::try_from(value).ok()),
            Value::String(raw) => raw.trim().parse::<usize>().ok(),
            _ => None,
        })
    })
}

fn u32_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u32> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| match value {
            Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
            Value::String(raw) => raw.trim().parse::<u32>().ok(),
            _ => None,
        })
    })
}

fn i64_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| match value {
            Value::Number(number) => number.as_i64(),
            Value::String(raw) => raw.trim().parse::<i64>().ok(),
            _ => None,
        })
    })
}

fn optional_u16_param(params: &serde_json::Map<String, Value>, key: &str) -> Option<Option<u16>> {
    let Some(value) = params.get(key) else {
        return Some(None);
    };
    if value.is_null() {
        return Some(None);
    }
    let parsed = match value {
        Value::Number(number) => number.as_u64(),
        Value::String(raw) => raw.trim().parse::<u64>().ok(),
        _ => None,
    }?;
    if (1..=u16::MAX as u64).contains(&parsed) {
        Some(Some(parsed as u16))
    } else {
        None
    }
}

fn pull_request_status_param(
    params: &serde_json::Map<String, Value>,
) -> Option<SessionPullRequestStatusSnapshot> {
    let raw = string_param(params, &["status", "state"]).unwrap_or_else(|| "open".to_string());
    match raw.trim().to_ascii_lowercase().as_str() {
        "open" | "opened" => Some(SessionPullRequestStatusSnapshot::Open),
        "merged" | "merge" => Some(SessionPullRequestStatusSnapshot::Merged),
        "closed" | "close" => Some(SessionPullRequestStatusSnapshot::Closed),
        _ => None,
    }
}

fn shell_activity_param(
    params: &serde_json::Map<String, Value>,
) -> Option<SessionPanelShellActivityStateSnapshot> {
    let raw = string_param(params, &["state", "shell_state", "shellState", "activity"])?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "prompt" | "idle" | "promptidle" | "prompt_idle" | "prompt-idle" => {
            Some(SessionPanelShellActivityStateSnapshot::PromptIdle)
        }
        "running" | "busy" | "command" | "commandrunning" | "command_running"
        | "command-running" => Some(SessionPanelShellActivityStateSnapshot::CommandRunning),
        "unknown" | "clear" => Some(SessionPanelShellActivityStateSnapshot::Unknown),
        _ => None,
    }
}

fn insert_first_param(params: &serde_json::Map<String, Value>) -> bool {
    bool_param(params, &["insert_first", "before"]).unwrap_or(false)
}

fn terminal_startup_params(
    params: &serde_json::Map<String, Value>,
) -> (
    Option<String>,
    Option<String>,
    Option<BTreeMap<String, String>>,
) {
    (
        string_param(
            params,
            &["initial_terminal_command", "initialCommand", "command"],
        ),
        string_param(params, &["initial_terminal_input", "initialInput", "input"]),
        string_map_param(
            params,
            &["initial_terminal_environment", "environment", "env"],
        ),
    )
}

fn f64_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| {
            value.as_f64().or_else(|| {
                value
                    .as_str()
                    .map(str::trim)
                    .and_then(|value| value.parse::<f64>().ok())
            })
        })
    })
}

fn v2_double_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = params.get(*key)?;
        value
            .as_bool()
            .map(|value| if value { 1.0 } else { 0.0 })
            .or_else(|| value.as_f64())
            .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
            .filter(|value| value.is_finite())
    })
}

fn initial_divider_position_param(
    params: &serde_json::Map<String, Value>,
) -> Result<Option<f64>, ()> {
    match params.get("initial_divider_position") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => v2_double_param(params, &["initial_divider_position"])
            .map(|value| Some(value.clamp(0.1, 0.9)))
            .ok_or(()),
    }
}

fn ports_param(params: &serde_json::Map<String, Value>) -> Option<Vec<u16>> {
    let value = params
        .get("ports")
        .or_else(|| params.get("listening_ports"))
        .or_else(|| params.get("listeningPorts"))
        .or_else(|| params.get("port"))?;
    let raw_ports: Vec<i64> = if let Some(values) = value.as_array() {
        values
            .iter()
            .map(|value| {
                value.as_i64().or_else(|| {
                    value
                        .as_str()
                        .map(str::trim)
                        .and_then(|raw| raw.parse::<i64>().ok())
                })
            })
            .collect::<Option<Vec<_>>>()?
    } else if let Some(port) = value.as_i64() {
        vec![port]
    } else {
        value
            .as_str()?
            .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
            .filter(|part| !part.trim().is_empty())
            .map(|part| part.trim().parse::<i64>().ok())
            .collect::<Option<Vec<_>>>()?
    };
    let mut ports = Vec::new();
    for port in raw_ports {
        if !(1..=65535).contains(&port) {
            return None;
        }
        let port = port as u16;
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    ports.sort_unstable();
    Some(ports)
}

fn one_based_ref_index(value: &str, prefix: &str) -> Option<usize> {
    let (actual_prefix, raw_index) = value.trim().split_once(':')?;
    if actual_prefix != prefix {
        return None;
    }
    let index = raw_index.trim().parse::<usize>().ok()?;
    index.checked_sub(1)
}

fn split_orientation_from_params(
    params: &serde_json::Map<String, Value>,
) -> Option<SessionSplitOrientation> {
    match string_param(params, &["orientation", "direction"])
        .unwrap_or_else(|| "horizontal".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "horizontal" | "h" | "right" | "left" | "r" | "l" => {
            Some(SessionSplitOrientation::Horizontal)
        }
        "vertical" | "v" | "down" | "up" | "d" | "u" => Some(SessionSplitOrientation::Vertical),
        _ => None,
    }
}

fn surface_kind_from_params(params: &serde_json::Map<String, Value>) -> Option<String> {
    let Some(kind) = string_param(params, &["type", "kind"]) else {
        return Some("invalid".to_string());
    };
    match kind.to_ascii_lowercase().as_str() {
        "terminal" | "shell" => None,
        "agent" | "browser" | "markdown" | "file" | "diff" | "custom-sidebar" => {
            Some(kind.to_ascii_lowercase())
        }
        _ => Some("invalid".to_string()),
    }
}

fn surface_id_from_params_or_focused(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<String> {
    let workspace_index = workspace_index_from_workspace_scope_or_selected(snapshot, params)?;
    surface_id_from_params_or_workspace_focused(snapshot, workspace_index, params)
}

fn surface_id_from_params_or_workspace_focused(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    params: &serde_json::Map<String, Value>,
) -> Option<String> {
    if params.contains_key("index") {
        return None;
    }

    let workspace = snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .get(workspace_index)?;
    let surfaces = surfaces_for_workspace(workspace);

    if let Some(surface_ref) = string_param(params, &["surface_ref", "ref"]) {
        if let Some(index) = one_based_ref_index(&surface_ref, "surface") {
            return surfaces
                .get(index)
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }

    if let Some(surface_id) = string_param(params, &["surface_id", "panel_id", "id"]) {
        if surfaces.iter().any(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == surface_id)
        }) {
            return Some(surface_id);
        }
        return None;
    }

    surfaces
        .iter()
        .find(|surface| {
            surface
                .get("focused")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| surfaces.first())
        .and_then(|surface| surface.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn surface_ref_for_panel(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> Option<String> {
    let workspace = snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .get(workspace_index)?;
    surfaces_for_workspace(workspace)
        .iter()
        .position(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == panel_id)
        })
        .map(surface_ref)
}

fn surface_is_terminal(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
    else {
        return false;
    };
    surfaces_for_workspace(workspace).iter().any(|surface| {
        surface
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == panel_id)
            && surface
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(|surface_type| surface_type == "terminal")
    })
}

fn surface_is_browser(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
    else {
        return false;
    };
    surfaces_for_workspace(workspace).iter().any(|surface| {
        surface
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == panel_id)
            && surface
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|surface_type| surface_type == "browser")
    })
}

fn terminal_key_sequence(key: &str) -> Option<&'static str> {
    let normalized = key.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "enter" | "return" => Some("\r"),
        "tab" => Some("\t"),
        "escape" | "esc" => Some("\x1b"),
        "backspace" | "bs" => Some("\x7f"),
        "delete" | "del" => Some("\x1b[3~"),
        "up" | "arrow-up" | "arrowup" => Some("\x1b[A"),
        "down" | "arrow-down" | "arrowdown" => Some("\x1b[B"),
        "right" | "arrow-right" | "arrowright" => Some("\x1b[C"),
        "left" | "arrow-left" | "arrowleft" => Some("\x1b[D"),
        "home" => Some("\x1b[H"),
        "end" => Some("\x1b[F"),
        "pageup" | "page-up" => Some("\x1b[5~"),
        "pagedown" | "page-down" => Some("\x1b[6~"),
        "ctrl-c" | "ctrl+c" | "sigint" => Some("\x03"),
        "ctrl-d" | "ctrl+d" | "eof" => Some("\x04"),
        "ctrl-l" | "ctrl+l" => Some("\x0c"),
        "ctrl-z" | "ctrl+z" => Some("\x1a"),
        _ => None,
    }
}

fn workspace_summary(workspace: &SessionWorkspaceSnapshot, index: usize, selected: bool) -> Value {
    json!({
        "id": workspace.workspace_id,
        "ref": workspace_ref(index),
        "title": workspace_display_name(workspace),
        "custom_title": workspace.custom_title,
        "has_custom_title": workspace.custom_title.as_ref().is_some_and(|title| !title.trim().is_empty()),
        "description": workspace.custom_description,
        "selected": selected,
        "pinned": workspace.is_pinned.unwrap_or(false),
        "listening_ports": workspace_listening_ports(workspace),
        "agent_listening_ports": workspace.agent_listening_ports.clone().unwrap_or_default(),
        "agent_pids": workspace.agent_pids.clone().unwrap_or_default(),
        "panel_ttys": workspace.panel_ttys.clone().unwrap_or_default(),
        "panel_shell_activity": workspace.panel_shell_activity.clone().unwrap_or_default(),
        "remote": workspace_remote_payload(workspace),
        "current_directory": workspace.current_directory,
        "initial_terminal_command": workspace.initial_terminal_command,
        "initial_terminal_input": workspace.initial_terminal_input,
        "initial_terminal_environment": workspace.initial_terminal_environment,
        "zoomed_panel_id": workspace.zoomed_panel_id,
        "restorable_agent_panels": restorable_agent_panel_summaries(workspace),
        "custom_color": workspace.custom_color,
        "group_id": workspace.group_id,
        "git_branch": workspace.git_branch,
        "panel_git_branches": workspace.panel_git_branches,
        "panel_pull_requests": workspace.panel_pull_requests,
        "sidebar_progress": workspace.sidebar_progress,
        "sidebar_status_entries": workspace.sidebar_status_entries,
        "sidebar_metadata_entries": workspace.sidebar_metadata_entries,
        "sidebar_metadata_blocks": workspace.sidebar_metadata_blocks,
        "sidebar_log_entries": workspace.sidebar_log_entries,
        "latest_conversation_message": Value::Null,
        "latest_submitted_message": Value::Null,
        "latest_submitted_at": Value::Null,
    })
}

fn canonical_workspace_summary(
    workspace: &SessionWorkspaceSnapshot,
    index: usize,
    selected: bool,
) -> Value {
    json!({
        "id": workspace.workspace_id,
        "ref": workspace_ref(index),
        "index": index,
        "title": workspace_display_name(workspace),
        "custom_title": workspace.custom_title,
        "has_custom_title": workspace.custom_title.as_ref().is_some_and(|title| !title.trim().is_empty()),
        "description": workspace.custom_description,
        "selected": selected,
        "pinned": workspace.is_pinned.unwrap_or(false),
        "listening_ports": workspace_listening_ports(workspace),
        "remote": workspace_remote_payload(workspace),
        "current_directory": workspace.current_directory,
        "custom_color": workspace.custom_color,
        "latest_conversation_message": Value::Null,
        "latest_submitted_message": Value::Null,
        "latest_submitted_at": Value::Null,
    })
}

fn workspace_remote_payload(workspace: &SessionWorkspaceSnapshot) -> Value {
    workspace.remote.as_ref().map_or_else(
        || {
            json!({
                "enabled": false,
                "state": "disconnected",
                "connected": false,
                "active_terminal_sessions": 0,
                "daemon": {
                    "state": "unavailable",
                    "capabilities": [],
                },
                "detected_ports": [],
                "forwarded_ports": [],
                "conflicted_ports": [],
                "detail": Value::Null,
                "transport": Value::Null,
                "destination": Value::Null,
                "port": Value::Null,
                "local_proxy_port": Value::Null,
                "persistent_daemon_slot": Value::Null,
                "proxy": {
                    "state": "unavailable",
                    "host": Value::Null,
                    "port": Value::Null,
                    "schemes": ["socks5", "http_connect"],
                    "url": Value::Null,
                    "error_code": Value::Null,
                },
            })
        },
        |remote| json!(remote),
    )
}

fn restorable_agent_panel_summaries(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    workspace
        .restorable_agent_snapshots
        .as_ref()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    json!({
                        "panel_id": entry.panel_id,
                        "kind": entry.snapshot.kind,
                        "session_id": entry.snapshot.session_id,
                        "working_directory": entry.snapshot.working_directory,
                        "resume_command": entry.snapshot.resume_command,
                        "fork_command": entry.snapshot.fork_command,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn workspace_group_summaries(
    workspaces: &[SessionWorkspaceSnapshot],
    groups: &Option<Vec<cmux_core::session::SessionWorkspaceGroupSnapshot>>,
) -> Vec<Value> {
    let Some(groups) = groups.as_ref() else {
        return Vec::new();
    };
    groups
        .iter()
        .map(|group| {
            let members: Vec<_> = workspaces
                .iter()
                .enumerate()
                .filter(|(_index, workspace)| {
                    workspace.group_id.as_deref() == Some(group.id.as_str())
                })
                .map(|(index, workspace)| {
                    json!({
                        "workspace_id": workspace.workspace_id,
                        "workspace_ref": workspace_ref(index),
                    })
                })
                .collect();
            json!({
                "id": group.id.clone(),
                "name": group.name.clone(),
                "collapsed": group.is_collapsed,
                "pinned": group.is_pinned.unwrap_or(false),
                "anchor_workspace_id": group.anchor_workspace_id.clone(),
                "anchor_member_index": group.anchor_member_index,
                "custom_color": group.custom_color.clone(),
                "icon_symbol": group.icon_symbol.clone(),
                "members": members,
            })
        })
        .collect()
}

fn workspace_display_name(workspace: &SessionWorkspaceSnapshot) -> String {
    workspace
        .custom_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            workspace
                .process_title
                .trim()
                .is_empty()
                .then_some("Workspace")
                .or(Some(workspace.process_title.as_str()))
        })
        .unwrap_or("Workspace")
        .to_string()
}

fn surfaces_for_workspace(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    let mut rows = Vec::new();
    if let Some(layout) = workspace.layout.as_ref() {
        collect_surfaces(layout, workspace, &mut rows);
    }
    rows
}

fn surface_kind_label(kind: &SessionSurfaceKindSnapshot) -> &'static str {
    match kind {
        SessionSurfaceKindSnapshot::Terminal => "terminal",
        SessionSurfaceKindSnapshot::Browser { .. } => "browser",
        SessionSurfaceKindSnapshot::AgentSession { .. } => "agent-session",
        SessionSurfaceKindSnapshot::Markdown { .. } => "markdown",
        SessionSurfaceKindSnapshot::File { .. } => "file",
        SessionSurfaceKindSnapshot::Diff { .. } => "diff",
        SessionSurfaceKindSnapshot::ProjectSidebar => "project-sidebar",
        SessionSurfaceKindSnapshot::RightSidebarTool => "right-sidebar-tool",
        SessionSurfaceKindSnapshot::RemoteTerminal { .. } => "remote-terminal",
    }
}

fn collect_surfaces(
    layout: &SessionWorkspaceLayoutSnapshot,
    workspace: &SessionWorkspaceSnapshot,
    rows: &mut Vec<Value>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => collect_pane_surfaces(pane, workspace, rows),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            collect_surfaces(&split.first, workspace, rows);
            collect_surfaces(&split.second, workspace, rows);
        }
    }
}

fn collect_pane_surfaces(
    pane: &SessionPaneLayoutSnapshot,
    workspace: &SessionWorkspaceSnapshot,
    rows: &mut Vec<Value>,
) {
    for (index_in_pane, panel_id) in pane.panel_ids.iter().enumerate() {
        let index = rows.len();
        let selected = pane
            .selected_panel_id
            .as_deref()
            .map(|selected| selected == panel_id)
            .unwrap_or(index_in_pane == 0);
        let focused = workspace.focused_panel_id.as_deref() == Some(panel_id.as_str());
        let record = workspace
            .surfaces
            .as_ref()
            .and_then(|records| records.iter().find(|record| record.surface_id == *panel_id));
        let surface_type = record
            .map(|record| surface_kind_label(&record.kind))
            .unwrap_or_else(|| pane.surface_kind.as_deref().unwrap_or("terminal"));
        let (browser_back_history, browser_forward_history) =
            match record.map(|record| &record.kind) {
                Some(SessionSurfaceKindSnapshot::Browser {
                    back_history,
                    forward_history,
                    ..
                }) => (back_history.as_deref(), forward_history.as_deref()),
                _ => (
                    pane.browser_back_history.as_deref(),
                    pane.browser_forward_history.as_deref(),
                ),
            };
        let browser_back_count = browser_back_history.map(<[String]>::len).unwrap_or(0);
        let browser_forward_count = browser_forward_history.map(<[String]>::len).unwrap_or(0);
        let browser_availability = cmux_core::session_ops::browser_navigation_availability(
            browser_back_history,
            browser_forward_history,
        );
        let terminal_startup = panel_terminal_startup(&workspace.panel_terminal_startups, panel_id);
        let restorable_agent =
            panel_restorable_agent(&workspace.restorable_agent_snapshots, panel_id);
        let authoritative_resume = record
            .and_then(|record| record.terminal_startup.as_ref())
            .and_then(|startup| startup.resume_binding.as_deref());
        let (initial_command, initial_input, initial_environment) =
            match record.and_then(|record| record.terminal_startup.as_ref()) {
                Some(startup) => (
                    startup.command.clone(),
                    startup.initial_input.clone(),
                    startup.environment.clone(),
                ),
                None => match terminal_startup {
                    Some(startup) => (
                        startup.initial_terminal_command.clone(),
                        startup.initial_terminal_input.clone(),
                        startup.initial_terminal_environment.clone(),
                    ),
                    None => (
                        workspace.initial_terminal_command.clone(),
                        workspace.initial_terminal_input.clone(),
                        workspace.initial_terminal_environment.clone(),
                    ),
                },
            };
        let authoritative_title = record.and_then(|record| record.metadata.custom_title.clone());
        let (markdown_file_path, file_path, diff_viewer_token, diff_viewer_request_path) =
            match record.map(|record| &record.kind) {
                Some(SessionSurfaceKindSnapshot::Markdown { path }) => {
                    (path.clone(), None, None, None)
                }
                Some(SessionSurfaceKindSnapshot::File { path }) => (None, path.clone(), None, None),
                Some(SessionSurfaceKindSnapshot::Diff {
                    token,
                    request_path,
                }) => (None, None, token.clone(), request_path.clone()),
                _ => (
                    pane.markdown_file_path.clone(),
                    pane.file_path.clone(),
                    pane.diff_viewer_token.clone(),
                    pane.diff_viewer_request_path.clone(),
                ),
            };
        let (
            browser_url,
            browser_proxy_url,
            browser_omnibar_visible,
            browser_focus_mode_active,
            browser_developer_tools_visible,
            browser_developer_tools_panel,
            browser_page_zoom,
        ) = match record.map(|record| &record.kind) {
            Some(SessionSurfaceKindSnapshot::Browser {
                url,
                proxy_url,
                omnibar_visible,
                focus_mode_active,
                developer_tools_visible,
                developer_tools_panel,
                page_zoom,
                ..
            }) => (
                url.clone(),
                proxy_url.clone(),
                omnibar_visible.unwrap_or(true),
                focus_mode_active.unwrap_or(false),
                developer_tools_visible.unwrap_or(false),
                developer_tools_panel.clone(),
                *page_zoom,
            ),
            _ => (
                pane.browser_url.clone(),
                pane.browser_proxy_url.clone(),
                pane.browser_omnibar_visible.unwrap_or(true),
                pane.browser_focus_mode_active.unwrap_or(false),
                pane.browser_developer_tools_visible.unwrap_or(false),
                pane.browser_developer_tools_panel.clone(),
                pane.browser_page_zoom,
            ),
        };
        rows.push(json!({
            "id": panel_id,
            "ref": surface_ref(index),
            "type": surface_type,
            "title": authoritative_title.clone().or_else(|| panel_title(&workspace.panel_titles, panel_id)).unwrap_or_else(|| surface_type.to_string()),
            "focused": focused,
            "pane_id": pane.pane_id,
            "pane_ref": pane.pane_id.as_ref().map(|_| format!("pane:{}", index + 1)),
            "selected_in_pane": selected,
            "custom_title": authoritative_title.or_else(|| panel_title(&workspace.panel_titles, panel_id)),
            "pinned": record.map(|record| record.metadata.pinned).unwrap_or_else(|| panel_pinned(&workspace.panel_pins, panel_id)),
            "unread": record.map(|record| record.metadata.unread).unwrap_or_else(|| panel_unread(&workspace.panel_unreads, panel_id)),
            "requested_working_directory": record.and_then(|record| record.terminal_startup.as_ref()).and_then(|startup| startup.working_directory.clone()).or_else(|| record.and_then(|record| record.metadata.reported_directory.clone())).or_else(|| workspace.current_directory.clone()),
            "initial_command": initial_command,
            "initial_input": initial_input,
            "initial_environment": initial_environment,
            "listening_ports": panel_listening_ports(workspace, panel_id),
            "tty": panel_tty(workspace, panel_id),
            "tty_name": panel_tty(workspace, panel_id),
            "shell_activity": panel_shell_activity(workspace, panel_id),
            "shell_activity_state": panel_shell_activity(workspace, panel_id),
            "tmux_start_command": record.and_then(|record| record.terminal_startup.as_ref()).and_then(|startup| startup.tmux_start_command.clone()),
            "resume_binding": authoritative_resume.or(restorable_agent).map(restorable_agent_binding_payload),
            "markdown_file_path": markdown_file_path,
            "file_path": file_path,
            "diff_viewer_token": diff_viewer_token,
            "diff_viewer_request_path": diff_viewer_request_path,
            "browser_url": browser_url,
            "browser_proxy_url": browser_proxy_url,
            "browser_can_go_back": browser_availability.can_go_back,
            "browser_can_go_forward": browser_availability.can_go_forward,
            "browser_back_history_count": browser_back_count,
            "browser_forward_history_count": browser_forward_count,
            "browser_omnibar_visible": browser_omnibar_visible,
            "browser_focus_mode_active": browser_focus_mode_active,
            "browser_developer_tools_visible": browser_developer_tools_visible,
            "browser_developer_tools_panel": browser_developer_tools_panel,
            "browser_page_zoom": browser_page_zoom,
        }));
    }
}

fn workspace_listening_ports(workspace: &SessionWorkspaceSnapshot) -> Vec<u16> {
    let mut ports: Vec<u16> = workspace
        .listening_ports
        .as_ref()
        .into_iter()
        .flat_map(|ports| ports.iter().copied())
        .chain(
            workspace
                .panel_listening_ports
                .as_ref()
                .into_iter()
                .flat_map(|entries| entries.iter())
                .flat_map(|entry| entry.ports.iter().copied()),
        )
        .chain(
            workspace
                .agent_listening_ports
                .as_ref()
                .into_iter()
                .flat_map(|ports| ports.iter().copied()),
        )
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

fn panel_listening_ports(workspace: &SessionWorkspaceSnapshot, panel_id: &str) -> Vec<u16> {
    workspace
        .panel_listening_ports
        .as_ref()
        .and_then(|entries| entries.iter().find(|entry| entry.panel_id == panel_id))
        .map(|entry| {
            let mut ports = entry.ports.clone();
            ports.sort_unstable();
            ports.dedup();
            ports
        })
        .unwrap_or_default()
}

fn panel_tty(workspace: &SessionWorkspaceSnapshot, panel_id: &str) -> Option<String> {
    workspace.panel_ttys.as_ref().and_then(|entries| {
        entries
            .iter()
            .find(|entry| entry.panel_id == panel_id)
            .map(|entry| entry.tty.clone())
    })
}

fn panel_shell_activity(workspace: &SessionWorkspaceSnapshot, panel_id: &str) -> Option<String> {
    workspace.panel_shell_activity.as_ref().and_then(|entries| {
        entries
            .iter()
            .find(|entry| entry.panel_id == panel_id)
            .map(|entry| match entry.state {
                SessionPanelShellActivityStateSnapshot::Unknown => "unknown",
                SessionPanelShellActivityStateSnapshot::PromptIdle => "promptIdle",
                SessionPanelShellActivityStateSnapshot::CommandRunning => "commandRunning",
            })
            .map(str::to_string)
    })
}

fn panel_title(
    panel_titles: &Option<Vec<cmux_core::session::SessionPanelTitleSnapshot>>,
    panel_id: &str,
) -> Option<String> {
    panel_titles.as_ref()?.iter().find_map(|entry| {
        (entry.panel_id == panel_id)
            .then(|| entry.custom_title.clone())
            .flatten()
    })
}

fn panel_pinned(
    panel_pins: &Option<Vec<cmux_core::session::SessionPanelPinSnapshot>>,
    panel_id: &str,
) -> bool {
    panel_pins
        .as_ref()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.panel_id == panel_id)
                .map(|entry| entry.is_pinned)
        })
        .unwrap_or(false)
}

fn panel_unread(
    panel_unreads: &Option<Vec<cmux_core::session::SessionPanelUnreadSnapshot>>,
    panel_id: &str,
) -> bool {
    panel_unreads
        .as_ref()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.panel_id == panel_id)
                .map(|entry| entry.is_unread)
        })
        .unwrap_or(false)
}

fn panel_terminal_startup<'a>(
    panel_terminal_startups: &'a Option<
        Vec<cmux_core::session::SessionPanelTerminalStartupSnapshot>,
    >,
    panel_id: &str,
) -> Option<&'a cmux_core::session::SessionPanelTerminalStartupSnapshot> {
    panel_terminal_startups
        .as_ref()?
        .iter()
        .find(|entry| entry.panel_id == panel_id)
}

fn panel_restorable_agent<'a>(
    restorable_agent_snapshots: &'a Option<
        Vec<cmux_core::session::SessionPanelRestorableAgentSnapshot>,
    >,
    panel_id: &str,
) -> Option<&'a cmux_core::session::SessionRestorableAgentSnapshot> {
    restorable_agent_snapshots
        .as_ref()?
        .iter()
        .find(|entry| entry.panel_id == panel_id)
        .map(|entry| &entry.snapshot)
}

fn restorable_agent_binding_payload(
    snapshot: &cmux_core::session::SessionRestorableAgentSnapshot,
) -> Value {
    json!({
        "kind": snapshot.kind,
        "session_id": snapshot.session_id,
        "working_directory": snapshot.working_directory,
        "launch_command": snapshot.launch_command,
        "resume_command": snapshot.resume_command,
        "fork_command": snapshot.fork_command,
    })
}

fn new_browser_surface_id(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    before_surface_ids: &[String],
) -> Option<String> {
    let workspace = snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .get(workspace_index)?;
    surfaces_for_workspace(workspace)
        .iter()
        .find(|surface| {
            surface
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|surface_type| surface_type == "browser")
                && surface
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !before_surface_ids.iter().any(|before| before == id))
        })
        .and_then(|surface| surface.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            surfaces_for_workspace(workspace)
                .iter()
                .find(|surface| {
                    surface
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|surface_type| surface_type == "browser")
                        && surface
                            .get("focused")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                })
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

fn browser_surface_payload(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> Option<Value> {
    let window = snapshot.windows.first()?;
    let workspace = window.tab_manager.workspaces.get(workspace_index)?;
    let surfaces = surfaces_for_workspace(workspace);
    let surface_index = surfaces.iter().position(|surface| {
        surface
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == panel_id)
    })?;
    let surface = surfaces.get(surface_index)?.clone();
    let url = surface
        .get("browser_url")
        .cloned()
        .filter(|value| !value.is_null())
        .unwrap_or_else(|| json!("about:blank"));
    Some(json!({
        "id": panel_id,
        "surface_id": panel_id,
        "panel_id": panel_id,
        "surface_ref": surface_ref(surface_index),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "url": url,
        "browser_url": surface.get("browser_url").cloned().unwrap_or(Value::Null),
        "surface": surface,
    }))
}

fn workspace_ref(index: usize) -> String {
    format!("workspace:{}", index + 1)
}

fn surface_ref(index: usize) -> String {
    format!("surface:{}", index + 1)
}

fn pane_ref(index: usize) -> String {
    format!("pane:{}", index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    static ENV_LOCK: Mutex<()> = Mutex::new(());
    use cmux_core::session::SessionPanelShellActivitySnapshot;
    use cmux_core::session::{
        AgentLaunchCommandSnapshot, AppSessionSnapshot, SessionGitBranchSnapshot,
        SessionPaneLayoutSnapshot, SessionPanelGitBranchSnapshot,
        SessionPanelListeningPortsSnapshot, SessionPanelPinSnapshot,
        SessionPanelPullRequestSnapshot, SessionPanelRestorableAgentSnapshot,
        SessionPanelTerminalStartupSnapshot, SessionPanelTitleSnapshot, SessionPanelTtySnapshot,
        SessionPanelUnreadSnapshot, SessionPullRequestStatusSnapshot,
        SessionRestorableAgentSnapshot, SessionTabManagerSnapshot, SessionWindowSnapshot,
        SessionWorkspaceAgentPidSnapshot, SessionWorkspaceGroupSnapshot,
        SessionWorkspaceRemoteDaemonSnapshot, SessionWorkspaceRemoteProxySnapshot,
        SessionWorkspaceRemoteSnapshot, SessionWorkspaceSidebarLogEntrySnapshot,
        SessionWorkspaceSidebarMetadataBlockSnapshot, SessionWorkspaceSidebarMetadataSnapshot,
        SessionWorkspaceSidebarProgressSnapshot, SessionWorkspaceSidebarStatusSnapshot,
        SESSION_SNAPSHOT_SCHEMA_VERSION,
    };

    fn test_snapshot() -> AppSessionSnapshot {
        AppSessionSnapshot {
            version: SESSION_SNAPSHOT_SCHEMA_VERSION,
            created_at: 0,
            windows: vec![SessionWindowSnapshot {
                window_id: Some("window-1".to_string()),
                selected_workspace_id: None,
                dock: None,
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index: Some(0),
                    workspaces: vec![SessionWorkspaceSnapshot {
                        workspace_id: Some("workspace-1".to_string()),
                        process_title: "shell".to_string(),
                        custom_title: Some("Phoenix".to_string()),
                        current_directory: Some("C:/repo".to_string()),
                        focused_panel_id: Some("surface-1".to_string()),
                        layout: Some(SessionWorkspaceLayoutSnapshot::Pane(
                            SessionPaneLayoutSnapshot {
                                pane_id: Some("pane-1".to_string()),
                                panel_ids: vec!["surface-1".to_string()],
                                selected_panel_id: Some("surface-1".to_string()),
                                surface_kind: None,
                                markdown_file_path: None,
                                file_path: None,
                                diff_viewer_token: None,
                                diff_viewer_request_path: None,
                                browser_url: None,
                                browser_proxy_url: None,
                                browser_back_history: None,
                                browser_forward_history: None,
                                browser_omnibar_visible: None,
                                browser_focus_mode_active: None,
                                browser_developer_tools_visible: None,
                                browser_developer_tools_panel: None,
                                browser_page_zoom: None,
                            },
                        )),
                        ..Default::default()
                    }],
                    workspace_groups: None,
                },
            }],
        }
    }

    #[test]
    fn resolved_same_title_rename_still_produces_one_rename_event_spec() {
        let snapshot = test_snapshot();

        let event = resolved_workspace_rename_event_spec(&snapshot, 0, 0).unwrap();

        assert_eq!(event.name, "workspace.renamed");
        assert_eq!(event.workspace_id.as_deref(), Some("workspace-1"));
        assert_eq!(event.payload["title"], json!("Phoenix"));
        assert_eq!(event.payload["previous_title"], json!("Phoenix"));
    }

    fn surface_move_snapshot() -> AppSessionSnapshot {
        let mut snapshot = test_snapshot();
        let source = &mut snapshot.windows[0].tab_manager.workspaces[0];
        let SessionWorkspaceLayoutSnapshot::Pane(source_pane) =
            source.layout.as_mut().expect("source layout")
        else {
            unreachable!();
        };
        source_pane.panel_ids.push("surface-2".to_string());

        let mut destination = source.clone();
        destination.workspace_id = Some("workspace-2".to_string());
        let SessionWorkspaceLayoutSnapshot::Pane(destination_pane) =
            destination.layout.as_mut().expect("destination layout")
        else {
            unreachable!();
        };
        destination_pane.pane_id = Some("pane-2".to_string());
        destination_pane.panel_ids = vec!["surface-3".to_string(), "surface-4".to_string()];
        destination_pane.selected_panel_id = Some("surface-3".to_string());
        snapshot.windows[0].tab_manager.workspaces.push(destination);
        snapshot
    }

    #[test]
    fn surface_move_resolver_matches_canonical_destination_precedence() {
        let snapshot = surface_move_snapshot();
        let params = serde_json::json!({
            "surface_id": "surface-1",
            "before_surface_id": "surface-4",
            "pane_id": "pane-1",
            "workspace_id": "workspace-1",
            "index": 99,
            "focus": true,
        });
        let resolved = resolve_surface_move(&snapshot, params.as_object().unwrap()).unwrap();
        assert_eq!(
            resolved,
            SurfaceMoveResolution {
                source_workspace_index: 0,
                panel_id: "surface-1".to_string(),
                target_workspace_index: 1,
                target_pane_id: "pane-2".to_string(),
                destination_index: Some(1),
                focus: true,
            }
        );

        let params = serde_json::json!({
            "surface_id": "surface-1",
            "pane_id": "pane-2",
            "workspace_id": "workspace-1",
            "index": 2,
        });
        let resolved = resolve_surface_move(&snapshot, params.as_object().unwrap()).unwrap();
        assert_eq!(resolved.target_workspace_index, 1);
        assert_eq!(resolved.target_pane_id, "pane-2");
        assert_eq!(resolved.destination_index, Some(2));

        let params = serde_json::json!({
            "surface_id": "surface-1",
            "workspace_id": "workspace-2",
        });
        let resolved = resolve_surface_move(&snapshot, params.as_object().unwrap()).unwrap();
        assert_eq!(resolved.target_workspace_index, 1);
        assert_eq!(resolved.target_pane_id, "pane-2");

        let conflict = serde_json::json!({
            "surface_id": "surface-1",
            "before_surface_id": "surface-3",
            "after_surface_id": "surface-4",
        });
        assert_eq!(
            resolve_surface_move(&snapshot, conflict.as_object().unwrap()),
            Err(SurfaceMoveResolveError::ConflictingAnchors)
        );
    }

    #[test]
    fn pane_join_source_resolves_explicit_surface_or_selected_pane_surface() {
        let snapshot = surface_move_snapshot();
        let direct = serde_json::json!({"surface_id": "surface-2"});
        assert_eq!(
            resolve_pane_join_source(&snapshot, direct.as_object().unwrap()),
            Ok("surface-2".to_string())
        );
        let by_pane = serde_json::json!({"pane_id": "pane-2"});
        assert_eq!(
            resolve_pane_join_source(&snapshot, by_pane.as_object().unwrap()),
            Ok("surface-3".to_string())
        );
        let by_ref = serde_json::json!({
            "workspace_ref": "workspace:2",
            "pane_ref": "pane:1",
        });
        assert_eq!(
            resolve_pane_join_source(&snapshot, by_ref.as_object().unwrap()),
            Ok("surface-3".to_string())
        );
        assert_eq!(
            resolve_pane_join_source(&snapshot, &serde_json::Map::new()),
            Err(PaneJoinSourceError::Missing)
        );
        let missing = serde_json::json!({"pane_id": "missing"});
        assert_eq!(
            resolve_pane_join_source(&snapshot, missing.as_object().unwrap()),
            Err(PaneJoinSourceError::SourcePaneUnresolved(
                "missing".to_string()
            ))
        );
    }

    #[test]
    fn resize_pane_resolves_id_ref_or_persisted_focus() {
        let mut snapshot = test_snapshot();
        let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
        workspace.focused_panel_id = Some("surface-1".to_string());

        let by_id = json!({"pane_id": "pane-1"});
        assert_eq!(
            resolve_resize_pane(workspace, by_id.as_object().unwrap()),
            Some((0, "pane-1".to_string()))
        );
        let by_ref = json!({"pane_ref": "pane:1"});
        assert_eq!(
            resolve_resize_pane(workspace, by_ref.as_object().unwrap()),
            Some((0, "pane-1".to_string()))
        );
        assert_eq!(
            resolve_resize_pane(workspace, &serde_json::Map::new()),
            Some((0, "pane-1".to_string()))
        );

        workspace.focused_panel_id = None;
        assert_eq!(
            resolve_resize_pane(workspace, &serde_json::Map::new()),
            None
        );
    }

    #[test]
    fn pane_list_geometry_walks_nested_splits_in_leaf_order() {
        let base = test_snapshot();
        let SessionWorkspaceLayoutSnapshot::Pane(base_pane) =
            base.windows[0].tab_manager.workspaces[0]
                .layout
                .as_ref()
                .unwrap()
        else {
            unreachable!()
        };
        let pane = |id: &str, surface: &str| {
            let mut pane = base_pane.clone();
            pane.pane_id = Some(id.to_string());
            pane.panel_ids = vec![surface.to_string()];
            pane.selected_panel_id = Some(surface.to_string());
            SessionWorkspaceLayoutSnapshot::Pane(pane)
        };
        let layout =
            SessionWorkspaceLayoutSnapshot::Split(cmux_core::session::SessionSplitLayoutSnapshot {
                split_id: Some("root".to_string()),
                orientation: SessionSplitOrientation::Horizontal,
                divider_position: 0.6,
                first: Box::new(SessionWorkspaceLayoutSnapshot::Split(
                    cmux_core::session::SessionSplitLayoutSnapshot {
                        split_id: Some("inner".to_string()),
                        orientation: SessionSplitOrientation::Vertical,
                        divider_position: 0.25,
                        first: Box::new(pane("pane-a", "surface-a")),
                        second: Box::new(pane("pane-b", "surface-b")),
                    },
                )),
                second: Box::new(pane("pane-c", "surface-c")),
            });
        let mut rows = Vec::new();
        pane_frames(
            &layout,
            PanePixelFrame {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 800.0,
            },
            &mut rows,
        );
        assert_eq!(
            rows.iter()
                .map(|(pane, _)| pane.pane_id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["pane-a", "pane-b", "pane-c"]
        );
        assert_eq!((rows[0].1.width, rows[0].1.height), (600.0, 200.0));
        assert_eq!((rows[1].1.x, rows[1].1.y), (0.0, 200.0));
        assert_eq!((rows[1].1.width, rows[1].1.height), (600.0, 600.0));
        assert_eq!((rows[2].1.x, rows[2].1.width), (600.0, 400.0));
    }

    #[test]
    fn pane_surfaces_target_resolves_global_id_scoped_ref_or_focus() {
        let mut snapshot = surface_move_snapshot();
        snapshot.windows[0].tab_manager.workspaces[0].focused_panel_id =
            Some("surface-1".to_string());
        let by_id = json!({"pane_id":"pane-2"});
        assert_eq!(
            resolve_pane_surfaces_target(&snapshot, by_id.as_object().unwrap(), 0),
            Some((1, 0, "pane-2".to_string()))
        );
        let by_ref = json!({"workspace_ref":"workspace:2", "pane_ref":"pane:1"});
        assert_eq!(
            resolve_pane_surfaces_target(&snapshot, by_ref.as_object().unwrap(), 0),
            Some((1, 0, "pane-2".to_string()))
        );
        assert_eq!(
            resolve_pane_surfaces_target(&snapshot, &serde_json::Map::new(), 0),
            Some((0, 0, "pane-1".to_string()))
        );
    }

    #[test]
    fn pane_focus_target_is_scoped_to_the_resolved_workspace() {
        let mut snapshot = surface_move_snapshot();
        snapshot.windows[0].tab_manager.selected_workspace_index = Some(1);
        let selected = json!({"pane_id":"pane-2"});
        assert_eq!(
            resolve_pane_focus_target(&snapshot, selected.as_object().unwrap(), 0),
            Ok((1, 0, "pane-2".to_string()))
        );

        let scoped = json!({"workspace_ref":"workspace:1", "pane_ref":"pane:1"});
        assert_eq!(
            resolve_pane_focus_target(&snapshot, scoped.as_object().unwrap(), 0),
            Ok((0, 0, "pane-1".to_string()))
        );

        let wrong_workspace = json!({"workspace_ref":"workspace:1", "pane_id":"pane-2"});
        assert_eq!(
            resolve_pane_focus_target(&snapshot, wrong_workspace.as_object().unwrap(), 0),
            Err(PaneFocusResolveError::PaneNotFound)
        );
    }

    #[test]
    fn workspace_window_move_resolves_refs_locally_and_ids_globally() {
        let mut snapshot = surface_move_snapshot();
        let destination = snapshot.windows[0]
            .tab_manager
            .workspaces
            .pop()
            .expect("destination workspace");
        snapshot.windows.push(SessionWindowSnapshot {
            window_id: Some("window-2".to_string()),
            selected_workspace_id: None,
            dock: None,
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![destination],
                workspace_groups: None,
            },
        });

        let by_ref = serde_json::json!({"workspace_ref": "workspace:1"});
        assert_eq!(
            workspace_id_for_window_move(&snapshot, by_ref.as_object().unwrap()).as_deref(),
            Some("workspace-1")
        );
        let by_id = serde_json::json!({"workspace_id": "workspace-2"});
        assert_eq!(
            workspace_id_for_window_move(&snapshot, by_id.as_object().unwrap()).as_deref(),
            Some("workspace-2")
        );
        assert_eq!(
            global_surface_location(&snapshot, "surface-3"),
            Some((1, 0))
        );
        assert_eq!(global_pane_location(&snapshot, "pane-2"), Some((1, 0, 0)));
        assert_eq!(
            split_off_workspace_index(&snapshot, by_ref.as_object().unwrap(), 0),
            Some(Some(0))
        );
    }

    #[test]
    fn custom_sidebar_action_reply_uses_native_bridge_envelope() {
        let ok_reply = custom_sidebar_action_reply(ControlCallResult::Ok(
            JsonValue::try_from(json!({ "accepted": true })).expect("json value"),
        ));
        assert_eq!(ok_reply["ok"], json!(true));
        assert_eq!(ok_reply["value"]["accepted"], json!(true));

        let err_reply = custom_sidebar_action_reply(ControlCallResult::Err {
            code: "invalid_params".to_string(),
            message: "bad action".to_string(),
            data: Some(JsonValue::try_from(json!({ "field": "method" })).expect("json value")),
        });
        assert_eq!(err_reply["ok"], json!(false));
        assert_eq!(err_reply["error"]["code"], json!("invalid_params"));
        assert_eq!(err_reply["error"]["userMessage"], json!("bad action"));
        assert_eq!(err_reply["error"]["data"]["field"], json!("method"));
    }

    #[test]
    fn custom_sidebar_action_policy_allows_safe_sidebar_methods() {
        for method in [
            "sidebar.list",
            "sidebar.select",
            "workspace.select",
            "workspace.set_status",
            "workspace.report_meta",
            "surface.focus",
            "extension.sidebar.snapshot",
        ] {
            assert!(
                custom_sidebar_action_policy_allows(method),
                "expected custom sidebar policy to allow {method}"
            );
        }
    }

    #[test]
    fn custom_sidebar_action_policy_denies_dangerous_methods_with_data() {
        for method in [
            "workspace.close",
            "surface.close",
            "browser.eval",
            "browser.addscript",
            "debug.terminals",
            "workspace.remote.configure",
        ] {
            assert!(
                !custom_sidebar_action_policy_allows(method),
                "expected custom sidebar policy to deny {method}"
            );
            let reply = custom_sidebar_action_reply(custom_sidebar_action_denied(method, None));
            assert_eq!(reply["ok"], json!(false));
            assert_eq!(
                reply["error"]["code"],
                json!("custom_sidebar_capability_denied")
            );
            assert_eq!(reply["error"]["data"]["method"], json!(method));
            assert_eq!(
                reply["error"]["data"]["policy"],
                json!(CUSTOM_SIDEBAR_ACTION_POLICY)
            );
            assert!(reply["error"]["data"]["allowed_methods"]
                .as_array()
                .is_some_and(|methods| methods.contains(&json!("workspace.select"))));
        }
    }

    #[test]
    fn custom_sidebar_action_schema_validates_required_method_params() {
        let empty = serde_json::Map::new();
        let reply = custom_sidebar_action_reply(
            validate_custom_sidebar_action_schema("workspace.select", &empty)
                .expect_err("workspace.select should require a selector"),
        );
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(
            reply["error"]["code"],
            json!("custom_sidebar_action_schema_invalid")
        );
        assert_eq!(reply["error"]["data"]["field"], json!("workspace"));
        assert_eq!(
            reply["error"]["data"]["accepted_keys"],
            json!(["workspace_id", "id", "workspace_ref", "ref"])
        );

        let params = json!({ "workspace_id": "workspace-1" })
            .as_object()
            .expect("object")
            .clone();
        assert!(validate_custom_sidebar_action_schema("workspace.select", &params).is_ok());

        let params = json!({ "key": "deploy", "value": "running", "priority": "10" })
            .as_object()
            .expect("object")
            .clone();
        assert!(validate_custom_sidebar_action_schema("workspace.set_status", &params).is_ok());

        let params = json!({ "key": "deploy", "value": "running", "priority": "high" })
            .as_object()
            .expect("object")
            .clone();
        let reply = custom_sidebar_action_reply(
            validate_custom_sidebar_action_schema("workspace.set_status", &params)
                .expect_err("priority should be an integer"),
        );
        assert_eq!(reply["error"]["data"]["field"], json!("priority"));
        assert_eq!(
            reply["error"]["data"]["expected"],
            json!("integer or integer string")
        );
    }

    #[test]
    fn custom_sidebar_action_schema_advertises_authoring_contract() {
        let catalog = custom_sidebar_action_schema_catalog();
        assert_eq!(
            catalog["version"],
            json!(CUSTOM_SIDEBAR_ACTION_SCHEMA_VERSION)
        );
        assert!(catalog["methods"].as_array().is_some_and(|methods| methods
            .iter()
            .any(|method| method["method"] == json!("workspace.set_status"))));
        assert_eq!(
            catalog["selector_keys"]["surface"],
            json!(["surface_id", "panel_id", "id", "surface_ref", "ref"])
        );
    }

    #[test]
    fn workspace_list_payload_matches_control_shape() {
        let payload = workspace_list_payload(&test_snapshot());
        assert_eq!(payload["window_id"], json!("window-1"));
        assert_eq!(payload["workspaces"][0]["id"], json!("workspace-1"));
        assert_eq!(payload["workspaces"][0]["ref"], json!("workspace:1"));
        assert_eq!(payload["workspaces"][0]["title"], json!("Phoenix"));
        assert_eq!(payload["workspaces"][0]["selected"], json!(true));
        assert_eq!(
            payload["workspaces"][0]["current_directory"],
            json!("C:/repo")
        );
        assert_eq!(
            payload["workspaces"][0]["initial_terminal_command"],
            Value::Null
        );
        assert_eq!(
            payload["workspaces"][0]["initial_terminal_input"],
            Value::Null
        );
        assert_eq!(
            payload["workspaces"][0]["initial_terminal_environment"],
            Value::Null
        );
        assert_eq!(payload["workspaces"][0]["zoomed_panel_id"], Value::Null);
        assert_eq!(
            payload["workspaces"][0]["restorable_agent_panels"],
            json!([])
        );
        assert_eq!(payload["workspaces"][0]["git_branch"], Value::Null);
        assert_eq!(payload["workspaces"][0]["panel_git_branches"], Value::Null);
        assert_eq!(payload["workspaces"][0]["panel_pull_requests"], Value::Null);
        assert_eq!(payload["workspaces"][0]["sidebar_progress"], Value::Null);
        assert_eq!(
            payload["workspaces"][0]["sidebar_status_entries"],
            Value::Null
        );
        assert_eq!(
            payload["workspaces"][0]["sidebar_metadata_entries"],
            Value::Null
        );
        assert_eq!(
            payload["workspaces"][0]["sidebar_metadata_blocks"],
            Value::Null
        );
        assert_eq!(payload["workspaces"][0]["sidebar_log_entries"], Value::Null);
        assert_eq!(payload["workspace_groups"], json!([]));
    }

    #[test]
    fn extension_sidebar_snapshot_projects_documented_authoring_data() {
        let mut snapshot = test_snapshot();
        let workspace = &mut snapshot.windows[0].tab_manager.workspaces[0];
        workspace.custom_description = Some("Ship parity".to_string());
        workspace.listening_ports = Some(vec![3000]);
        workspace.panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
            panel_id: "surface-1".to_string(),
            ports: vec![5173],
        }]);
        workspace.git_branch = Some(SessionGitBranchSnapshot {
            branch: "main".to_string(),
            is_dirty: false,
        });
        workspace.panel_git_branches = Some(vec![SessionPanelGitBranchSnapshot {
            panel_id: "surface-1".to_string(),
            branch: "feature/sidebar".to_string(),
            is_dirty: true,
        }]);
        workspace.panel_unreads = Some(vec![SessionPanelUnreadSnapshot {
            panel_id: "surface-1".to_string(),
            is_unread: true,
            unread_at: Some(10),
        }]);
        workspace.panel_pull_requests = Some(vec![SessionPanelPullRequestSnapshot {
            panel_id: "surface-1".to_string(),
            number: 42,
            label: "Review".to_string(),
            url: "https://github.com/example/repo/pull/42".to_string(),
            status: SessionPullRequestStatusSnapshot::Open,
            branch: Some("feature/sidebar".to_string()),
            is_stale: false,
        }]);
        workspace.sidebar_progress = Some(SessionWorkspaceSidebarProgressSnapshot {
            value: 0.5,
            label: Some("halfway".to_string()),
        });

        let payload = extension_sidebar_snapshot_payload(&snapshot);
        assert_eq!(
            payload["protocol"],
            json!("cmux-extension-sidebar-snapshot")
        );
        assert_eq!(payload["selected_workspace_id"], json!("workspace-1"));
        assert_eq!(payload["selectedId"], json!("workspace-1"));
        assert_eq!(payload["selectedTitle"], json!("Phoenix"));
        assert_eq!(payload["workspaceCount"], json!(1));
        assert_eq!(payload["unreadTotal"], json!(1));

        let workspace = &payload["workspaces"][0];
        assert_eq!(workspace["id"], json!("workspace-1"));
        assert_eq!(workspace["directory"], json!("C:/repo"));
        assert_eq!(workspace["root_path"], json!("C:/repo"));
        assert_eq!(workspace["ports"], json!([3000, 5173]));
        assert_eq!(workspace["portCount"], json!(2));
        assert_eq!(workspace["tabCount"], json!(1));
        assert_eq!(workspace["unread"], json!(1));
        assert_eq!(workspace["branch"], json!("feature/sidebar"));
        assert_eq!(workspace["dirty"], json!(true));
        assert_eq!(workspace["branch_summary"], json!("feature/sidebar*"));
        assert_eq!(workspace["pr"]["number"], json!(42));
        assert_eq!(workspace["pr"]["status"], json!("open"));
        assert_eq!(
            workspace["pull_request_urls"],
            json!(["https://github.com/example/repo/pull/42"])
        );
        assert_eq!(workspace["progress"]["label"], json!("halfway"));
        assert_eq!(workspace["tabs"][0]["id"], json!("surface-1"));
        assert_eq!(workspace["tabs"][0]["directory"], json!("C:/repo"));
        assert_eq!(workspace["tabs"][0]["ports"], json!([5173]));
        assert_eq!(workspace["tabs"][0]["branch"], json!("feature/sidebar"));
        assert_eq!(workspace["tabs"][0]["dirty"], json!(true));
        assert_eq!(
            workspace["panel_directories"]["surface-1"],
            json!("C:/repo")
        );
        let data = &payload["data"];
        assert_eq!(data["workspaceCount"], json!(1));
        assert_eq!(data["selectedId"], json!("workspace-1"));
        assert_eq!(data["selectedTitle"], json!("Phoenix"));
        assert_eq!(data["unreadTotal"], json!(1));
        assert_eq!(data["events"]["latest"], Value::Null);
        assert!(data["clock"]["time"].as_str().is_some());
        assert!(data["clock"]["epoch"].as_i64().is_some());
        let data_workspace = &data["workspaces"][0];
        assert_eq!(data_workspace["id"], json!("workspace-1"));
        assert_eq!(data_workspace["title"], json!("Phoenix"));
        assert_eq!(data_workspace["selected"], json!(true));
        assert_eq!(data_workspace["pinned"], json!(false));
        assert_eq!(data_workspace["index"], json!(0));
        assert_eq!(data_workspace["directory"], json!("C:/repo"));
        assert_eq!(data_workspace["ports"], json!([3000, 5173]));
        assert_eq!(data_workspace["portCount"], json!(2));
        assert_eq!(data_workspace["unread"], json!(1));
        assert_eq!(data_workspace["tabCount"], json!(1));
        assert_eq!(data_workspace["description"], json!("Ship parity"));
        assert_eq!(data_workspace["branch"], json!("feature/sidebar"));
        assert_eq!(data_workspace["dirty"], json!(true));
        assert_eq!(data_workspace["pr"]["number"], json!(42));
        assert_eq!(data_workspace["progress"]["label"], json!("halfway"));
        assert_eq!(data_workspace["latestMessage"], Value::Null);
        assert_eq!(data_workspace["latestPrompt"], Value::Null);
        assert_eq!(data_workspace["latestAt"], Value::Null);
        assert_eq!(data_workspace["tabs"][0]["id"], json!("surface-1"));
        assert_eq!(data_workspace["tabs"][0]["title"], json!("terminal"));
        assert_eq!(data_workspace["tabs"][0]["focused"], json!(true));
        assert_eq!(data_workspace["tabs"][0]["ports"], json!([5173]));
        assert_eq!(
            data_workspace["tabs"][0]["branch"],
            json!("feature/sidebar")
        );
        assert_eq!(data_workspace["tabs"][0]["dirty"], json!(true));
        assert_eq!(payload["events"]["latest"], Value::Null);
        assert_eq!(payload["events"]["recent"], json!([]));
    }

    #[test]
    fn extension_sidebar_snapshot_includes_event_context_for_eventbridge_bootstrap() {
        let events = extension_sidebar_events_context_from_retained(
            "boot-1".to_string(),
            4,
            vec![
                json!({
                    "type": "event",
                    "seq": 1,
                    "name": "session.changed",
                    "category": "session",
                }),
                json!({
                    "type": "event",
                    "seq": 2,
                    "name": "workspace.selected",
                    "category": "workspace",
                }),
                json!({
                    "type": "event",
                    "seq": 3,
                    "name": "surface.selected",
                    "category": "surface",
                }),
            ],
        );

        let payload =
            extension_sidebar_snapshot_payload_with_events(&test_snapshot(), events, json!({}));

        assert_eq!(payload["seq"], json!(3));
        assert_eq!(payload["latest_seq"], json!(3));
        assert_eq!(payload["events"]["protocol"], json!("cmux-events"));
        assert_eq!(payload["events"]["boot_id"], json!("boot-1"));
        assert_eq!(payload["events"]["oldest_seq"], json!(1));
        assert_eq!(payload["events"]["next_seq"], json!(4));
        assert_eq!(payload["events"]["retained_count"], json!(3));
        assert_eq!(
            payload["events"]["latest"]["name"],
            json!("surface.selected")
        );
        assert_eq!(
            payload["events"]["category_counts"],
            json!({
                "session": 1,
                "surface": 1,
                "workspace": 1,
            })
        );
        assert_eq!(
            payload["events"]["name_counts"]["workspace.selected"],
            json!(1)
        );
        assert_eq!(payload["events"]["recent"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn custom_sidebar_validation_prefers_swift_and_reports_invalid_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("status-board.json"), "{}").expect("write json");
        fs::write(dir.path().join("status-board.swift"), "Text(\"Status\")").expect("write swift");
        fs::write(dir.path().join("broken.json"), "{").expect("write broken");
        fs::write(
            dir.path().join("status-board.manifest.json"),
            r#"{"trusted":true,"capabilities":["workspace.select","browser.eval"]}"#,
        )
        .expect("write manifest");

        let payload = match validate_custom_sidebars_in_dir(dir.path(), None) {
            ControlCallResult::Ok(value) => Value::from(value),
            other => panic!("expected validation payload, got {other:?}"),
        };

        assert_eq!(payload["protocol"], json!("cmux-custom-sidebar-validation"));
        assert_eq!(payload["valid_count"], json!(1));
        assert_eq!(payload["invalid_count"], json!(1));
        assert_eq!(payload["ok"], json!(false));
        assert_eq!(payload["sidebars"][0]["name"], json!("broken"));
        assert_eq!(payload["sidebars"][0]["kind"], json!("json"));
        assert_eq!(payload["sidebars"][0]["valid"], json!(false));
        assert!(payload["sidebars"][0]["errors"][0]
            .as_str()
            .is_some_and(|error| error.contains("invalid JSON")));
        assert_eq!(payload["sidebars"][1]["name"], json!("status-board"));
        assert_eq!(payload["sidebars"][1]["kind"], json!("swift"));
        assert_eq!(payload["sidebars"][1]["valid"], json!(true));
        assert_eq!(payload["sidebars"][1]["manifest"]["trusted"], json!(true));
        assert_eq!(
            payload["sidebars"][1]["manifest"]["requested_methods"],
            json!(["browser.eval", "workspace.select"])
        );
        assert_eq!(
            payload["sidebars"][1]["manifest"]["allowed_requested_methods"],
            json!(["workspace.select"])
        );
        assert_eq!(
            payload["sidebars"][1]["manifest"]["denied_requested_methods"],
            json!(["browser.eval"])
        );
        assert!(payload["sidebars"][1]["shadowed_json_path"]
            .as_str()
            .is_some_and(|path| path.ends_with("status-board.json")));

        let named = match validate_custom_sidebars_in_dir(dir.path(), Some("status-board")) {
            ControlCallResult::Ok(value) => Value::from(value),
            other => panic!("expected named validation payload, got {other:?}"),
        };
        assert_eq!(named["name"], json!("status-board"));
        assert_eq!(named["sidebars"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn custom_sidebar_assets_are_minted_and_resolved_from_adjacent_asset_dir() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        let previous_dir = std::env::var_os("CMUX_SIDEBARS_DIR");
        let dir = tempfile::tempdir().expect("tempdir");
        unsafe {
            std::env::set_var("CMUX_SIDEBARS_DIR", dir.path());
        }

        let source_path = dir.path().join("ops.swift");
        let asset_dir = dir.path().join("ops.assets");
        fs::create_dir_all(asset_dir.join("icons")).expect("asset dir");
        fs::write(&source_path, "Image(\"logo\")").expect("write swift");
        fs::write(asset_dir.join("logo.png"), [137, 80, 78, 71]).expect("write png");
        fs::write(asset_dir.join("icons").join("badge.svg"), "<svg />").expect("write svg");
        fs::write(asset_dir.join("secret.txt"), "nope").expect("write text");

        let params = serde_json::Map::from_iter([(
            "source_path".to_string(),
            json!(source_path.to_string_lossy()),
        )]);
        let assets = custom_sidebar_asset_map_from_params(&params);
        assert!(assets["logo"]
            .as_str()
            .is_some_and(|url| url.starts_with("cmux-sidebar-asset://ops/logo.png?source=")));
        assert!(assets["logo.png"].as_str().is_some());
        assert!(assets["icons/badge"].as_str().is_some());
        assert!(assets.get("secret").is_none());

        let logo_url = assets["logo"].as_str().expect("logo url");
        let (resolved_path, mime) =
            resolve_custom_sidebar_asset_request(logo_url).expect("resolve logo");
        assert_eq!(
            resolved_path,
            fs::canonicalize(asset_dir.join("logo.png")).unwrap()
        );
        assert_eq!(mime, "image/png");
        assert!(resolve_custom_sidebar_asset_request(
            &logo_url.replace("logo.png", "../ops.swift")
        )
        .is_none());

        match previous_dir {
            Some(value) => unsafe {
                std::env::set_var("CMUX_SIDEBARS_DIR", value);
            },
            None => unsafe {
                std::env::remove_var("CMUX_SIDEBARS_DIR");
            },
        }
    }

    #[test]
    fn custom_sidebar_action_denial_can_include_manifest_context() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source_path = dir.path().join("ops.swift");
        fs::write(&source_path, "Text(\"Ops\")").expect("write swift");
        fs::write(
            dir.path().join("ops.manifest.json"),
            r#"{"trusted":false,"allowed_methods":["workspace.select","browser.eval"]}"#,
        )
        .expect("write manifest");

        let manifest =
            custom_sidebar_manifest_for_source(source_path.to_str()).expect("manifest summary");
        let reply = custom_sidebar_action_reply(custom_sidebar_action_denied(
            "browser.eval",
            Some(manifest),
        ));

        assert_eq!(reply["ok"], json!(false));
        assert_eq!(
            reply["error"]["code"],
            json!("custom_sidebar_capability_denied")
        );
        assert_eq!(
            reply["error"]["data"]["manifest"]["requested_methods"],
            json!(["browser.eval", "workspace.select"])
        );
        assert_eq!(
            reply["error"]["data"]["manifest"]["denied_requested_methods"],
            json!(["browser.eval"])
        );
    }

    #[test]
    fn custom_sidebar_reload_payload_targets_only_valid_sidebars() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("ops.json"), "{}").expect("write valid json");
        fs::write(dir.path().join("broken.json"), "{").expect("write broken json");

        let validation = match validate_custom_sidebars_in_dir(dir.path(), None) {
            ControlCallResult::Ok(value) => Value::from(value),
            other => panic!("expected validation payload, got {other:?}"),
        };
        let payload = custom_sidebar_reload_payload(None, &validation);

        assert_eq!(payload["protocol"], json!("cmux-custom-sidebar-reload"));
        assert_eq!(payload["event"], json!(CUSTOM_SIDEBAR_RELOAD_EVENT));
        assert_eq!(payload["all"], json!(true));
        assert_eq!(payload["name"], Value::Null);
        assert_eq!(payload["sidebars"].as_array().map(Vec::len), Some(1));
        assert_eq!(payload["sidebars"][0]["name"], json!("ops"));
        assert!(payload["paths"][0]
            .as_str()
            .is_some_and(|path| path.ends_with("ops.json")));

        let named_payload = custom_sidebar_reload_payload(Some("ops"), &validation);
        assert_eq!(named_payload["all"], json!(false));
        assert_eq!(named_payload["name"], json!("ops"));
    }

    #[test]
    fn custom_sidebar_select_payload_describes_selected_sidebar() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("ops.json");
        fs::write(&path, "{}").expect("write valid json");
        let candidate = custom_sidebar_candidate_for_name(dir.path(), "ops")
            .expect("discover")
            .expect("candidate");
        let validation = validate_custom_sidebar_candidate(&candidate);

        let payload = custom_sidebar_select_payload(&candidate, &validation);

        assert_eq!(payload["accepted"], json!(true));
        assert_eq!(payload["protocol"], json!("cmux-custom-sidebar-select"));
        assert_eq!(payload["event"], json!(CUSTOM_SIDEBAR_SELECT_EVENT));
        assert_eq!(payload["name"], json!("ops"));
        assert_eq!(payload["kind"], json!("json"));
        assert!(payload["path"]
            .as_str()
            .is_some_and(|path| path.ends_with("ops.json")));
        assert_eq!(payload["sidebar"]["valid"], json!(true));
    }

    #[test]
    fn workspace_list_payload_includes_sidebar_progress() {
        let mut snapshot = test_snapshot();
        snapshot.windows[0].tab_manager.workspaces[0].sidebar_progress =
            Some(SessionWorkspaceSidebarProgressSnapshot {
                value: 0.5,
                label: Some("Building".to_string()),
            });

        let payload = workspace_list_payload(&snapshot);

        assert_eq!(
            payload["workspaces"][0]["sidebar_progress"],
            json!({"value": 0.5, "label": "Building"})
        );
    }

    #[test]
    fn workspace_list_payload_includes_sidebar_status_and_log() {
        let mut snapshot = test_snapshot();
        snapshot.windows[0].tab_manager.workspaces[0].sidebar_status_entries =
            Some(vec![SessionWorkspaceSidebarStatusSnapshot {
                key: "build".to_string(),
                value: "green".to_string(),
                priority: Some(80),
                updated_at: 10,
            }]);
        snapshot.windows[0].tab_manager.workspaces[0].sidebar_log_entries =
            Some(vec![SessionWorkspaceSidebarLogEntrySnapshot {
                level: "info".to_string(),
                message: "ship it".to_string(),
                created_at: 11,
            }]);

        let payload = workspace_list_payload(&snapshot);

        assert_eq!(
            payload["workspaces"][0]["sidebar_status_entries"],
            json!([{"key": "build", "value": "green", "priority": 80, "updated_at": 10}])
        );
        assert_eq!(
            payload["workspaces"][0]["sidebar_log_entries"],
            json!([{"level": "info", "message": "ship it", "created_at": 11}])
        );
    }

    #[test]
    fn workspace_list_payload_includes_sidebar_metadata_entries_and_blocks() {
        let mut snapshot = test_snapshot();
        snapshot.windows[0].tab_manager.workspaces[0].sidebar_metadata_entries =
            Some(vec![SessionWorkspaceSidebarMetadataSnapshot {
                key: "task".to_string(),
                value: "review".to_string(),
                icon: Some("text:CTX".to_string()),
                color: Some("blue".to_string()),
                url: Some("https://example.test/pr".to_string()),
                priority: Some(50),
                format: Some("markdown".to_string()),
                updated_at: 12,
            }]);
        snapshot.windows[0].tab_manager.workspaces[0].sidebar_metadata_blocks =
            Some(vec![SessionWorkspaceSidebarMetadataBlockSnapshot {
                key: "notes".to_string(),
                markdown: "**Ready**".to_string(),
                priority: Some(10),
                updated_at: 13,
            }]);

        let payload = workspace_list_payload(&snapshot);

        assert_eq!(
            payload["workspaces"][0]["sidebar_metadata_entries"],
            json!([{
                "key": "task",
                "value": "review",
                "icon": "text:CTX",
                "color": "blue",
                "url": "https://example.test/pr",
                "priority": 50,
                "format": "markdown",
                "updated_at": 12
            }])
        );
        assert_eq!(
            payload["workspaces"][0]["sidebar_metadata_blocks"],
            json!([{
                "key": "notes",
                "markdown": "**Ready**",
                "priority": 10,
                "updated_at": 13
            }])
        );
    }

    #[test]
    fn workspace_list_payload_includes_panel_pull_requests() {
        let mut snapshot = test_snapshot();
        snapshot.windows[0].tab_manager.workspaces[0].panel_pull_requests =
            Some(vec![SessionPanelPullRequestSnapshot {
                panel_id: "surface-1".to_string(),
                number: 42,
                label: "MR".to_string(),
                url: "https://gitlab.example/project/-/merge_requests/42".to_string(),
                status: SessionPullRequestStatusSnapshot::Open,
                branch: Some("feature/api".to_string()),
                is_stale: false,
            }]);

        let payload = workspace_list_payload(&snapshot);
        assert_eq!(
            payload["workspaces"][0]["panel_pull_requests"],
            json!([{
                "panel_id": "surface-1",
                "number": 42,
                "label": "MR",
                "url": "https://gitlab.example/project/-/merge_requests/42",
                "status": "open",
                "branch": "feature/api",
                "is_stale": false,
            }])
        );
    }

    #[test]
    fn not_supported_errors_use_socket_contract_shape() {
        let result = not_supported("browser viewport override is not supported by WKWebView");
        match result {
            ControlCallResult::Err {
                code,
                message,
                data,
            } => {
                assert_eq!(code, "not_supported");
                assert!(message.contains("WKWebView"));
                assert_eq!(data, None);
            }
            other => panic!("expected not_supported error, got {other:?}"),
        }
    }

    #[test]
    fn event_filters_match_name_and_category() {
        let event = json!({
            "name": "session.changed",
            "category": "session",
        });
        assert!(event_matches_filters(&event, &[], &[]));
        assert!(event_matches_filters(
            &event,
            &["session.changed".to_string()],
            &["session".to_string()]
        ));
        assert!(!event_matches_filters(
            &event,
            &["workspace.selected".to_string()],
            &[]
        ));
        assert!(!event_matches_filters(
            &event,
            &[],
            &["notification".to_string()]
        ));
    }

    #[test]
    fn live_event_subscribers_receive_matching_frames_and_prune_closed_receivers() {
        let (workspace_sender, mut workspace_receiver) = cmux_ipc::stream_mpsc::unbounded_channel();
        let (surface_sender, mut surface_receiver) = cmux_ipc::stream_mpsc::unbounded_channel();
        let (closed_sender, closed_receiver) = cmux_ipc::stream_mpsc::unbounded_channel::<String>();
        drop(closed_receiver);
        let mut subscribers = vec![
            EventSubscriber {
                sender: workspace_sender,
                names: Vec::new(),
                categories: vec!["workspace".to_string()],
            },
            EventSubscriber {
                sender: surface_sender,
                names: Vec::new(),
                categories: vec!["surface".to_string()],
            },
            EventSubscriber {
                sender: closed_sender,
                names: Vec::new(),
                categories: vec!["workspace".to_string()],
            },
        ];
        let event = json!({
            "name": "workspace.selected",
            "category": "workspace",
        });

        fan_out_event_to_subscribers(&mut subscribers, &event, r#"{"seq":1}"#);

        assert_eq!(workspace_receiver.try_recv().unwrap(), r#"{"seq":1}"#);
        assert!(surface_receiver.try_recv().is_err());
        assert_eq!(subscribers.len(), 2);
    }

    fn event_summary(
        workspaces: Vec<WorkspaceEventSummary>,
        selected_index: usize,
    ) -> SessionEventSummary {
        SessionEventSummary {
            window_id: Some("window-1".to_string()),
            selected_workspace_id: workspaces
                .get(selected_index)
                .and_then(|workspace| workspace.id.clone()),
            selected_workspace_index: Some(selected_index),
            workspaces,
        }
    }

    fn event_workspace(
        id: &str,
        title: &str,
        index: usize,
        surfaces: &[&str],
        selected_surface_id: Option<&str>,
    ) -> WorkspaceEventSummary {
        WorkspaceEventSummary {
            key: id.to_string(),
            id: Some(id.to_string()),
            title: title.to_string(),
            index,
            panes: vec![event_pane("pane-1", 0, surfaces, selected_surface_id)],
            surface_ids: surfaces.iter().map(|surface| surface.to_string()).collect(),
            selected_surface_id: selected_surface_id.map(str::to_string),
            sidebar: WorkspaceSidebarEventSummary::default(),
        }
    }

    fn event_pane(
        id: &str,
        index: usize,
        surfaces: &[&str],
        selected_surface_id: Option<&str>,
    ) -> PaneEventSummary {
        PaneEventSummary {
            key: id.to_string(),
            id: Some(id.to_string()),
            index,
            surface_ids: surfaces.iter().map(|surface| surface.to_string()).collect(),
            selected_surface_id: selected_surface_id.map(str::to_string),
        }
    }

    fn sidebar_map(entries: &[(&str, Value)]) -> BTreeMap<String, Value> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    fn event_names(events: &[DerivedEventSpec]) -> Vec<&'static str> {
        events.iter().map(|event| event.name).collect()
    }

    #[test]
    fn derived_session_events_bootstrap_current_workspace_and_surface_state() {
        let current = event_summary(
            vec![event_workspace(
                "workspace-1",
                "Phoenix",
                0,
                &["surface-1"],
                Some("surface-1"),
            )],
            0,
        );

        let events = derived_session_event_specs(None, &current);

        assert_eq!(
            event_names(&events),
            vec![
                "session.changed",
                "workspace.created",
                "pane.created",
                "pane.focused",
                "surface.created",
                "workspace.selected",
                "surface.selected",
            ]
        );
        assert_eq!(events[1].payload["workspace_ref"], json!("workspace:1"));
        assert_eq!(events[2].payload["pane_ref"], json!("pane:1"));
        assert_eq!(events[4].payload["surface_ref"], json!("surface:1"));
    }

    #[test]
    fn derived_session_events_capture_workspace_and_surface_diffs() {
        let previous = event_summary(
            vec![
                event_workspace("workspace-a", "Alpha", 0, &["surface-a"], Some("surface-a")),
                event_workspace("workspace-b", "Beta", 1, &["surface-b"], Some("surface-b")),
            ],
            0,
        );
        let current = event_summary(
            vec![
                event_workspace(
                    "workspace-b",
                    "Beta Prime",
                    0,
                    &["surface-b", "surface-c"],
                    Some("surface-c"),
                ),
                event_workspace("workspace-a", "Alpha", 1, &[], None),
            ],
            0,
        );

        let events = derived_session_event_specs(Some(&previous), &current);
        let names = event_names(&events);

        assert!(names.contains(&"session.changed"));
        assert!(names.contains(&"workspace.selected"));
        assert!(names.contains(&"workspace.renamed"));
        assert!(names.contains(&"workspace.reordered"));
        assert!(names.contains(&"surface.created"));
        assert!(names.contains(&"surface.closed"));
        assert!(names.contains(&"surface.selected"));
        let selected = events
            .iter()
            .find(|event| event.name == "workspace.selected")
            .expect("workspace selected event");
        assert_eq!(
            selected.payload["previous_workspace_id"],
            json!("workspace-a")
        );
        let renamed = events
            .iter()
            .find(|event| event.name == "workspace.renamed")
            .expect("workspace renamed event");
        assert_eq!(renamed.payload["previous_title"], json!("Beta"));
        let reordered = events
            .iter()
            .find(|event| event.name == "workspace.reordered")
            .expect("workspace reordered event");
        assert_eq!(
            reordered.payload["workspace_ids"],
            json!(["workspace-b", "workspace-a"])
        );
    }

    #[test]
    fn derived_session_events_close_surfaces_when_workspace_closes() {
        let previous = event_summary(
            vec![event_workspace(
                "workspace-a",
                "Alpha",
                0,
                &["surface-a"],
                Some("surface-a"),
            )],
            0,
        );
        let current = event_summary(
            vec![event_workspace(
                "workspace-b",
                "Beta",
                0,
                &["surface-b"],
                Some("surface-b"),
            )],
            0,
        );

        let events = derived_session_event_specs(Some(&previous), &current);
        let names = event_names(&events);

        assert!(names.contains(&"workspace.created"));
        assert!(names.contains(&"workspace.closed"));
        assert!(names.contains(&"surface.created"));
        assert!(names.contains(&"surface.closed"));
        let closed_surface = events
            .iter()
            .find(|event| event.name == "surface.closed")
            .expect("surface closed event");
        assert_eq!(closed_surface.surface_id, Some("surface-a".to_string()));
    }

    #[test]
    fn derived_session_events_emit_one_move_without_close_create_duplicates() {
        let previous = event_summary(
            vec![
                event_workspace(
                    "workspace-a",
                    "Alpha",
                    0,
                    &["surface-a", "surface-b"],
                    Some("surface-a"),
                ),
                event_workspace("workspace-b", "Beta", 1, &["surface-c"], Some("surface-c")),
            ],
            0,
        );
        let current = event_summary(
            vec![
                event_workspace("workspace-a", "Alpha", 0, &["surface-b"], Some("surface-b")),
                event_workspace(
                    "workspace-b",
                    "Beta",
                    1,
                    &["surface-c", "surface-a"],
                    Some("surface-a"),
                ),
            ],
            1,
        );
        let events = derived_session_event_specs(Some(&previous), &current);
        assert!(!events.iter().any(|event| {
            event.name == "surface.moved" && event.surface_id.as_deref() == Some("surface-a")
        }));
        assert!(!events.iter().any(|event| {
            matches!(event.name, "surface.created" | "surface.closed")
                && event.surface_id.as_deref() == Some("surface-a")
        }));
    }

    #[test]
    fn derived_session_events_capture_pane_lifecycle_and_focus() {
        let mut previous_workspace = event_workspace(
            "workspace-a",
            "Alpha",
            0,
            &["surface-a", "surface-b"],
            Some("surface-a"),
        );
        previous_workspace.panes = vec![event_pane(
            "pane-a",
            0,
            &["surface-a", "surface-b"],
            Some("surface-a"),
        )];
        let previous = event_summary(vec![previous_workspace], 0);

        let mut current_workspace = event_workspace(
            "workspace-a",
            "Alpha",
            0,
            &["surface-a", "surface-b", "surface-c"],
            Some("surface-b"),
        );
        current_workspace.panes = vec![
            event_pane("pane-a", 0, &["surface-a", "surface-b"], Some("surface-b")),
            event_pane("pane-b", 1, &["surface-c"], Some("surface-c")),
        ];
        let current = event_summary(vec![current_workspace], 0);

        let events = derived_session_event_specs(Some(&previous), &current);
        let names = event_names(&events);

        assert!(names.contains(&"pane.focused"));
        assert!(names.contains(&"pane.created"));
        let focused = events
            .iter()
            .find(|event| {
                event.name == "pane.focused" && event.payload["pane_id"] == json!("pane-a")
            })
            .expect("pane focused");
        assert_eq!(focused.category, "pane");
        assert_eq!(focused.payload["previous_surface_id"], json!("surface-a"));
        assert_eq!(focused.payload["selected_surface_id"], json!("surface-b"));
        let created = events
            .iter()
            .find(|event| {
                event.name == "pane.created" && event.payload["pane_id"] == json!("pane-b")
            })
            .expect("pane created");
        assert_eq!(created.payload["pane_ref"], json!("pane:2"));
    }

    #[test]
    fn derived_session_events_close_panes_when_removed() {
        let mut previous_workspace = event_workspace(
            "workspace-a",
            "Alpha",
            0,
            &["surface-a", "surface-b"],
            Some("surface-a"),
        );
        previous_workspace.panes = vec![
            event_pane("pane-a", 0, &["surface-a"], Some("surface-a")),
            event_pane("pane-b", 1, &["surface-b"], Some("surface-b")),
        ];
        let previous = event_summary(vec![previous_workspace], 0);

        let mut current_workspace =
            event_workspace("workspace-a", "Alpha", 0, &["surface-a"], Some("surface-a"));
        current_workspace.panes = vec![event_pane("pane-a", 0, &["surface-a"], Some("surface-a"))];
        let current = event_summary(vec![current_workspace], 0);

        let events = derived_session_event_specs(Some(&previous), &current);

        let closed = events
            .iter()
            .find(|event| event.name == "pane.closed")
            .expect("pane closed");
        assert_eq!(closed.payload["pane_id"], json!("pane-b"));
        assert_eq!(closed.payload["pane_ref"], json!("pane:2"));
    }

    #[test]
    fn derived_session_events_capture_sidebar_metadata_updates() {
        let previous = event_summary(
            vec![event_workspace(
                "workspace-a",
                "Alpha",
                0,
                &["surface-a"],
                Some("surface-a"),
            )],
            0,
        );
        let mut workspace =
            event_workspace("workspace-a", "Alpha", 0, &["surface-a"], Some("surface-a"));
        workspace.sidebar.progress = Some(json!({"value": 0.5, "label": "Half"}));
        workspace.sidebar.status_entries =
            sidebar_map(&[("build", json!({"key": "build", "value": "green"}))]);
        workspace.sidebar.metadata_entries =
            sidebar_map(&[("task", json!({"key": "task", "value": "review"}))]);
        workspace.sidebar.metadata_blocks =
            sidebar_map(&[("notes", json!({"key": "notes", "markdown": "Ready"}))]);
        workspace.sidebar.log_entries = vec![json!({"level": "info", "message": "ship it"})];
        let current = event_summary(vec![workspace], 0);

        let events = derived_session_event_specs(Some(&previous), &current);
        let names = event_names(&events);

        assert!(names.contains(&"sidebar.progress.updated"));
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "sidebar.metadata.updated")
                .count(),
            3
        );
        assert!(names.contains(&"sidebar.log.appended"));
        let progress = events
            .iter()
            .find(|event| event.name == "sidebar.progress.updated")
            .expect("progress event");
        assert_eq!(progress.category, "sidebar");
        assert_eq!(progress.payload["kind"], json!("progress"));
        let status = events
            .iter()
            .find(|event| {
                event.name == "sidebar.metadata.updated" && event.payload["kind"] == json!("status")
            })
            .expect("status event");
        assert_eq!(status.payload["key"], json!("build"));
    }

    #[test]
    fn derived_session_events_capture_sidebar_clears() {
        let mut workspace =
            event_workspace("workspace-a", "Alpha", 0, &["surface-a"], Some("surface-a"));
        workspace.sidebar.progress = Some(json!({"value": 0.5}));
        workspace.sidebar.status_entries =
            sidebar_map(&[("build", json!({"key": "build", "value": "green"}))]);
        workspace.sidebar.metadata_entries =
            sidebar_map(&[("task", json!({"key": "task", "value": "review"}))]);
        workspace.sidebar.metadata_blocks =
            sidebar_map(&[("notes", json!({"key": "notes", "markdown": "Ready"}))]);
        workspace.sidebar.log_entries = vec![json!({"level": "info", "message": "ship it"})];
        let previous = event_summary(vec![workspace], 0);
        let current = event_summary(
            vec![event_workspace(
                "workspace-a",
                "Alpha",
                0,
                &["surface-a"],
                Some("surface-a"),
            )],
            0,
        );

        let events = derived_session_event_specs(Some(&previous), &current);
        let names = event_names(&events);

        assert!(names.contains(&"sidebar.progress.cleared"));
        assert_eq!(
            names
                .iter()
                .filter(|name| **name == "sidebar.metadata.cleared")
                .count(),
            3
        );
        assert!(names.contains(&"sidebar.log.cleared"));
        let cleared = events
            .iter()
            .find(|event| {
                event.name == "sidebar.metadata.cleared"
                    && event.payload["kind"] == json!("metadata_block")
            })
            .expect("metadata block cleared event");
        assert_eq!(cleared.payload["key"], json!("notes"));
    }

    #[test]
    fn event_log_append_writes_jsonl_and_rotates_one_archive() {
        let dir = tempfile::tempdir().expect("tempdir");
        append_event_line_to_dir(dir.path(), r#"{"seq":1}"#, 64).expect("append first");
        append_event_line_to_dir(dir.path(), r#"{"seq":2}"#, 64).expect("append second");
        assert_eq!(
            fs::read_to_string(dir.path().join(EVENT_LOG_FILE_NAME)).expect("current log"),
            "{\"seq\":1}\n{\"seq\":2}\n"
        );

        append_event_line_to_dir(dir.path(), r#"{"seq":3}"#, 24).expect("rotate append");
        assert_eq!(
            fs::read_to_string(dir.path().join(EVENT_LOG_ARCHIVE_FILE_NAME)).expect("archive log"),
            "{\"seq\":1}\n{\"seq\":2}\n"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(EVENT_LOG_FILE_NAME)).expect("rotated current log"),
            "{\"seq\":3}\n"
        );
    }

    #[test]
    fn control_socket_methods_advertise_browser_network_and_platform_gaps() {
        for method in [
            "system.capabilities",
            "config.reload",
            "window.list",
            "window.current",
            "notification.list",
            "notification.dismiss",
            "notification.mark_read",
            "notification.clear",
            "notification.open",
            "notification.jump_to_unread",
            "notification.create",
            "right_sidebar",
            "feed.push",
            "feed.list",
            "feed.permission.reply",
            "feed.question.reply",
            "feed.exit_plan.reply",
            "session.restore_previous",
            "events.stream",
            "extension.sidebar.snapshot",
            "sidebar.snapshot",
            "workspace.set_agent_pid",
            "workspace.clear_agent_pid",
            "workspace.move_to_window",
            "workspace.last",
            "surface.report_tty",
            "surface.report_shell_state",
            "surface.split_off",
            "surface.drag_to_split",
            "pane.swap",
            "pane.focus",
            "pane.break",
            "pane.join",
            "pane.last",
            "pane.list",
            "pane.surfaces",
            "pane.resize",
            "surface.move",
            "surface.clear_history",
            "surface.trigger_flash",
            "surface.refresh_all",
            "surface.read_text",
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
            "browser.open_split",
            "browser.navigate",
            "browser.reload",
            "browser.url.get",
            "browser.focus_webview",
            "browser.is_webview_focused",
            "browser.snapshot",
            "browser.eval",
            "browser.click",
            "browser.fill",
            "browser.get.text",
            "browser.is.visible",
            "browser.find.role",
            "browser.cookies.get",
            "browser.storage.get",
            "browser.tab.list",
            "browser.console.list",
            "browser.state.save",
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
        ] {
            assert!(
                CONTROL_SOCKET_METHODS.contains(&method),
                "missing advertised method {method}"
            );
        }
    }

    #[test]
    fn unported_browser_automation_methods_are_explicit_not_supported_contract() {
        assert!(!is_unported_browser_automation_method(
            "browser.network.requests"
        ));
        for method in [
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
            "browser.state.save",
            "browser.state.load",
            "browser.highlight",
            "browser.addinitscript",
            "browser.addscript",
            "browser.addstyle",
        ] {
            assert!(
                CONTROL_SOCKET_METHODS.contains(&method),
                "missing implemented browser automation method {method}"
            );
            assert!(
                !is_unported_browser_automation_method(method),
                "implemented browser automation method should not route to not_supported: {method}"
            );
        }
        assert!(!is_unported_browser_automation_method(
            "browser.viewport.set"
        ));
    }

    #[test]
    fn browser_surface_payload_returns_agent_browser_shape() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                pane_id: Some("pane-1".to_string()),
                panel_ids: vec!["surface-1".to_string()],
                selected_panel_id: Some("surface-1".to_string()),
                surface_kind: Some("browser".to_string()),
                markdown_file_path: None,
                file_path: None,
                diff_viewer_token: None,
                diff_viewer_request_path: None,
                browser_url: Some("https://example.com/path".to_string()),
                browser_proxy_url: None,
                browser_back_history: None,
                browser_forward_history: None,
                browser_omnibar_visible: None,
                browser_focus_mode_active: None,
                browser_developer_tools_visible: None,
                browser_developer_tools_panel: None,
                browser_page_zoom: None,
            },
        ));

        let payload = browser_surface_payload(&snapshot, 0, "surface-1")
            .expect("browser surface payload should exist");

        assert_eq!(payload["surface_id"], json!("surface-1"));
        assert_eq!(payload["panel_id"], json!("surface-1"));
        assert_eq!(payload["surface_ref"], json!("surface:1"));
        assert_eq!(payload["workspace_ref"], json!("workspace:1"));
        assert_eq!(payload["url"], json!("https://example.com/path"));
        assert_eq!(payload["surface"]["type"], json!("browser"));
    }

    #[test]
    fn new_browser_surface_id_prefers_new_browser_surface() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Split(
            cmux_core::session::SessionSplitLayoutSnapshot {
                split_id: None,
                orientation: SessionSplitOrientation::Horizontal,
                first: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                    SessionPaneLayoutSnapshot {
                        pane_id: Some("pane-1".to_string()),
                        panel_ids: vec!["surface-1".to_string()],
                        selected_panel_id: Some("surface-1".to_string()),
                        surface_kind: None,
                        markdown_file_path: None,
                        file_path: None,
                        diff_viewer_token: None,
                        diff_viewer_request_path: None,
                        browser_url: None,
                        browser_proxy_url: None,
                        browser_back_history: None,
                        browser_forward_history: None,
                        browser_omnibar_visible: None,
                        browser_focus_mode_active: None,
                        browser_developer_tools_visible: None,
                        browser_developer_tools_panel: None,
                        browser_page_zoom: None,
                    },
                )),
                second: Box::new(SessionWorkspaceLayoutSnapshot::Pane(
                    SessionPaneLayoutSnapshot {
                        pane_id: Some("pane-2".to_string()),
                        panel_ids: vec!["surface-2".to_string()],
                        selected_panel_id: Some("surface-2".to_string()),
                        surface_kind: Some("browser".to_string()),
                        markdown_file_path: None,
                        file_path: None,
                        diff_viewer_token: None,
                        diff_viewer_request_path: None,
                        browser_url: Some("about:blank".to_string()),
                        browser_proxy_url: None,
                        browser_back_history: None,
                        browser_forward_history: None,
                        browser_omnibar_visible: None,
                        browser_focus_mode_active: None,
                        browser_developer_tools_visible: None,
                        browser_developer_tools_panel: None,
                        browser_page_zoom: None,
                    },
                )),
                divider_position: 0.5,
            },
        ));

        assert_eq!(
            new_browser_surface_id(&snapshot, 0, &["surface-1".to_string()]).as_deref(),
            Some("surface-2")
        );
    }

    #[test]
    fn workspace_list_payload_includes_disconnected_remote_default() {
        let payload = workspace_list_payload(&test_snapshot());
        let remote = &payload["workspaces"][0]["remote"];
        assert_eq!(remote["enabled"], json!(false));
        assert_eq!(remote["state"], json!("disconnected"));
        assert_eq!(remote["connected"], json!(false));
        assert_eq!(remote["active_terminal_sessions"], json!(0));
        assert_eq!(remote["proxy"]["state"], json!("unavailable"));
        assert_eq!(remote["proxy"]["url"], Value::Null);
        assert_eq!(remote["daemon"]["state"], json!("unavailable"));
    }

    #[test]
    fn workspace_list_payload_includes_remote_proxy_endpoint() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.remote = Some(SessionWorkspaceRemoteSnapshot {
            enabled: true,
            state: "connected".to_string(),
            connected: true,
            transport: Some("ssh".to_string()),
            destination: Some("dev.example.com".to_string()),
            port: Some(22),
            local_proxy_port: Some(31337),
            persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
            has_ssh_options: true,
            detail: None,
            daemon: Some(SessionWorkspaceRemoteDaemonSnapshot {
                state: "ready".to_string(),
                capabilities: vec!["proxy.stream.push".to_string()],
            }),
            proxy: Some(SessionWorkspaceRemoteProxySnapshot {
                state: "ready".to_string(),
                host: Some("127.0.0.1".to_string()),
                port: Some(31337),
                schemes: vec!["socks5".to_string(), "http_connect".to_string()],
                url: Some("socks5://127.0.0.1:31337".to_string()),
                error_code: None,
            }),
            detected_ports: Vec::new(),
            forwarded_ports: Vec::new(),
            conflicted_ports: Vec::new(),
            active_terminal_sessions: Some(1),
        });

        let payload = workspace_list_payload(&snapshot);
        let remote = &payload["workspaces"][0]["remote"];
        assert_eq!(remote["enabled"], json!(true));
        assert_eq!(remote["state"], json!("connected"));
        assert_eq!(remote["destination"], json!("dev.example.com"));
        assert_eq!(remote["local_proxy_port"], json!(31337));
        assert_eq!(remote["proxy"]["url"], json!("socks5://127.0.0.1:31337"));
        assert_eq!(
            remote["daemon"]["capabilities"],
            json!(["proxy.stream.push"])
        );
    }

    #[test]
    fn workspace_list_payload_includes_workspace_runtime_metadata() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.initial_terminal_command = Some("npm run dev".to_string());
        workspace.initial_terminal_input = Some("ready".to_string());
        workspace.initial_terminal_environment = Some(BTreeMap::from_iter([(
            "NODE_ENV".to_string(),
            "development".to_string(),
        )]));
        workspace.zoomed_panel_id = Some("surface-1".to_string());
        workspace.restorable_agent_snapshots = Some(vec![SessionPanelRestorableAgentSnapshot {
            panel_id: "surface-1".to_string(),
            snapshot: SessionRestorableAgentSnapshot {
                kind: "codex".to_string(),
                session_id: "session-1".to_string(),
                working_directory: Some("C:/repo".to_string()),
                launch_command: None,
                resume_command: Some("codex resume session-1".to_string()),
                fork_command: Some("codex fork session-1".to_string()),
            },
        }]);
        workspace.listening_ports = Some(vec![5173]);
        workspace.agent_listening_ports = Some(vec![4173]);
        workspace.agent_pids = Some(vec![SessionWorkspaceAgentPidSnapshot {
            key: "codex.session-1".to_string(),
            pid: 1234,
            updated_at: 20,
        }]);
        workspace.panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
            panel_id: "surface-1".to_string(),
            ports: vec![3000, 5173],
        }]);
        workspace.panel_ttys = Some(vec![SessionPanelTtySnapshot {
            panel_id: "surface-1".to_string(),
            tty: "ttys004".to_string(),
            updated_at: 21,
        }]);
        workspace.panel_shell_activity = Some(vec![SessionPanelShellActivitySnapshot {
            panel_id: "surface-1".to_string(),
            state: SessionPanelShellActivityStateSnapshot::CommandRunning,
            updated_at: 22,
        }]);

        let payload = workspace_list_payload(&snapshot);
        let summary = &payload["workspaces"][0];
        assert_eq!(summary["initial_terminal_command"], json!("npm run dev"));
        assert_eq!(summary["initial_terminal_input"], json!("ready"));
        assert_eq!(
            summary["initial_terminal_environment"],
            json!({"NODE_ENV": "development"})
        );
        assert_eq!(summary["zoomed_panel_id"], json!("surface-1"));
        assert_eq!(
            summary["restorable_agent_panels"],
            json!([{
                "panel_id": "surface-1",
                "kind": "codex",
                "session_id": "session-1",
                "working_directory": "C:/repo",
                "resume_command": "codex resume session-1",
                "fork_command": "codex fork session-1",
            }])
        );
        assert_eq!(summary["listening_ports"], json!([3000, 4173, 5173]));
        assert_eq!(summary["agent_listening_ports"], json!([4173]));
        assert_eq!(
            summary["agent_pids"],
            json!([{"key": "codex.session-1", "pid": 1234, "updated_at": 20}])
        );
        assert_eq!(
            summary["panel_ttys"],
            json!([{"panel_id": "surface-1", "tty": "ttys004", "updated_at": 21}])
        );
        assert_eq!(
            summary["panel_shell_activity"],
            json!([{"panel_id": "surface-1", "state": "commandRunning", "updated_at": 22}])
        );
    }

    #[test]
    fn workspace_list_payload_includes_workspace_group_metadata() {
        let mut snapshot = test_snapshot();
        let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
        second.workspace_id = Some("workspace-2".to_string());
        second.group_id = Some("group-1".to_string());
        snapshot.windows[0].tab_manager.workspaces[0].group_id = Some("group-1".to_string());
        snapshot.windows[0].tab_manager.workspace_groups =
            Some(vec![SessionWorkspaceGroupSnapshot {
                id: "group-1".to_string(),
                name: "Backend".to_string(),
                is_collapsed: true,
                anchor_workspace_id: Some("workspace-1".to_string()),
                anchor_member_index: Some(0),
                is_pinned: Some(true),
                custom_color: Some("#123456".to_string()),
                icon_symbol: Some("folder".to_string()),
            }]);
        snapshot.windows[0].tab_manager.workspaces.push(second);

        let payload = workspace_list_payload(&snapshot);
        assert_eq!(payload["workspaces"][0]["group_id"], json!("group-1"));
        assert_eq!(payload["workspace_groups"][0]["id"], json!("group-1"));
        assert_eq!(payload["workspace_groups"][0]["name"], json!("Backend"));
        assert_eq!(payload["workspace_groups"][0]["collapsed"], json!(true));
        assert_eq!(payload["workspace_groups"][0]["pinned"], json!(true));
        assert_eq!(
            payload["workspace_groups"][0]["anchor_workspace_id"],
            json!("workspace-1")
        );
        assert_eq!(
            payload["workspace_groups"][0]["members"],
            json!([
                {"workspace_id": "workspace-1", "workspace_ref": "workspace:1"},
                {"workspace_id": "workspace-2", "workspace_ref": "workspace:2"},
            ])
        );
        assert!(payload["workspaces"][0].get("index").is_none());
        assert!(payload["workspace_groups"][0]["members"][0]
            .get("index")
            .is_none());
    }

    #[test]
    fn surface_list_payload_projects_pane_surfaces() {
        let result = surface_list(&test_snapshot());
        let ControlCallResult::Ok(value) = result else {
            panic!("surface list should succeed");
        };
        let payload: Value = value.into();
        assert_eq!(payload["workspace_id"], json!("workspace-1"));
        assert_eq!(payload["surfaces"][0]["id"], json!("surface-1"));
        assert_eq!(payload["surfaces"][0]["ref"], json!("surface:1"));
        assert!(payload["surfaces"][0].get("index").is_none());
        assert!(payload["surfaces"][0].get("index_in_pane").is_none());
        assert_eq!(payload["surfaces"][0]["type"], json!("terminal"));
        assert_eq!(payload["surfaces"][0]["pane_id"], json!("pane-1"));
        assert_eq!(payload["surfaces"][0]["custom_title"], Value::Null);
        assert_eq!(payload["surfaces"][0]["pinned"], json!(false));
        assert_eq!(payload["surfaces"][0]["unread"], json!(false));
        assert_eq!(
            payload["surfaces"][0]["requested_working_directory"],
            json!("C:/repo")
        );
        assert_eq!(payload["surfaces"][0]["initial_command"], Value::Null);
        assert_eq!(payload["surfaces"][0]["initial_input"], Value::Null);
        assert_eq!(payload["surfaces"][0]["initial_environment"], Value::Null);
        assert_eq!(payload["surfaces"][0]["listening_ports"], json!([]));
        assert_eq!(payload["surfaces"][0]["markdown_file_path"], Value::Null);
        assert_eq!(payload["surfaces"][0]["diff_viewer_token"], Value::Null);
        assert_eq!(
            payload["surfaces"][0]["diff_viewer_request_path"],
            Value::Null
        );
        assert_eq!(payload["surfaces"][0]["browser_url"], Value::Null);
        assert_eq!(payload["surfaces"][0]["browser_can_go_back"], json!(false));
        assert_eq!(
            payload["surfaces"][0]["browser_omnibar_visible"],
            json!(true)
        );
        assert_eq!(
            payload["surfaces"][0]["browser_developer_tools_visible"],
            json!(false)
        );
    }

    #[test]
    fn workspace_current_workspace_selector_routes_manager_and_returns_selection() {
        let mut snapshot = test_snapshot();
        let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
        second.workspace_id = Some("workspace-2".to_string());
        snapshot.windows[0].tab_manager.workspaces.push(second);
        snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);

        let ControlCallResult::Ok(value) = workspace_current_from_params(
            &snapshot,
            &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-2"))]),
        ) else {
            panic!("workspace current should succeed");
        };
        let payload: Value = value.into();
        assert_eq!(payload["workspace_id"], json!("workspace-1"));
        assert_eq!(payload["workspace_ref"], json!("workspace:1"));
        assert_eq!(payload["workspace"]["selected"], json!(true));
    }

    #[test]
    fn surface_list_can_be_scoped_to_background_workspace() {
        let mut snapshot = test_snapshot();
        let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
        second.workspace_id = Some("workspace-2".to_string());
        second.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                pane_id: Some("pane-2".to_string()),
                panel_ids: vec!["surface-2".to_string()],
                selected_panel_id: Some("surface-2".to_string()),
                surface_kind: Some("browser".to_string()),
                markdown_file_path: None,
                file_path: None,
                diff_viewer_token: None,
                diff_viewer_request_path: None,
                browser_url: Some("https://background.test".to_string()),
                browser_proxy_url: None,
                browser_back_history: None,
                browser_forward_history: None,
                browser_omnibar_visible: None,
                browser_focus_mode_active: None,
                browser_developer_tools_visible: None,
                browser_developer_tools_panel: None,
                browser_page_zoom: None,
            },
        ));
        snapshot.windows[0].tab_manager.workspaces.push(second);
        snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);

        let ControlCallResult::Ok(value) = surface_list_from_params(
            &snapshot,
            &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-2"))]),
        ) else {
            panic!("surface list should succeed");
        };
        let payload: Value = value.into();
        assert_eq!(payload["workspace_id"], json!("workspace-2"));
        assert_eq!(payload["workspace_ref"], json!("workspace:2"));
        assert_eq!(payload["surfaces"][0]["id"], json!("surface-2"));
        assert_eq!(
            payload["surfaces"][0]["browser_url"],
            json!("https://background.test")
        );
    }

    #[test]
    fn surface_list_payload_inherits_workspace_terminal_startup() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.initial_terminal_command = Some("cargo test".to_string());
        workspace.initial_terminal_input = Some("echo ready".to_string());
        workspace.initial_terminal_environment = Some(BTreeMap::from_iter([(
            "RUST_LOG".to_string(),
            "debug".to_string(),
        )]));

        let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
            panic!("surface list should succeed");
        };
        let payload: Value = value.into();
        let surface = &payload["surfaces"][0];
        assert_eq!(surface["requested_working_directory"], json!("C:/repo"));
        assert_eq!(surface["initial_command"], json!("cargo test"));
        assert_eq!(surface["initial_input"], json!("echo ready"));
        assert_eq!(surface["initial_environment"], json!({"RUST_LOG": "debug"}));
    }

    #[test]
    fn surface_list_payload_includes_surface_metadata() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.initial_terminal_command = Some("cargo test".to_string());
        workspace.initial_terminal_input = Some("workspace input".to_string());
        workspace.initial_terminal_environment = Some(BTreeMap::from_iter([(
            "WORKSPACE".to_string(),
            "1".to_string(),
        )]));
        workspace.panel_titles = Some(vec![SessionPanelTitleSnapshot {
            panel_id: "surface-1".to_string(),
            custom_title: Some("API logs".to_string()),
        }]);
        workspace.panel_pins = Some(vec![SessionPanelPinSnapshot {
            panel_id: "surface-1".to_string(),
            is_pinned: true,
        }]);
        workspace.panel_unreads = Some(vec![SessionPanelUnreadSnapshot {
            panel_id: "surface-1".to_string(),
            is_unread: true,
            unread_at: None,
        }]);
        workspace.panel_terminal_startups = Some(vec![SessionPanelTerminalStartupSnapshot {
            panel_id: "surface-1".to_string(),
            initial_terminal_command: Some("npm test".to_string()),
            initial_terminal_input: Some("hello".to_string()),
            initial_terminal_environment: Some(BTreeMap::from_iter([(
                "CI".to_string(),
                "1".to_string(),
            )])),
        }]);
        workspace.panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
            panel_id: "surface-1".to_string(),
            ports: vec![8080, 3000],
        }]);
        workspace.panel_ttys = Some(vec![SessionPanelTtySnapshot {
            panel_id: "surface-1".to_string(),
            tty: "/dev/pts/7".to_string(),
            updated_at: 22,
        }]);
        workspace.panel_shell_activity = Some(vec![SessionPanelShellActivitySnapshot {
            panel_id: "surface-1".to_string(),
            state: SessionPanelShellActivityStateSnapshot::PromptIdle,
            updated_at: 23,
        }]);

        let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
            panic!("surface list should succeed");
        };
        let payload: Value = value.into();
        let surface = &payload["surfaces"][0];
        assert_eq!(surface["title"], json!("API logs"));
        assert_eq!(surface["custom_title"], json!("API logs"));
        assert_eq!(surface["pinned"], json!(true));
        assert_eq!(surface["unread"], json!(true));
        assert_eq!(surface["requested_working_directory"], json!("C:/repo"));
        assert_eq!(surface["initial_command"], json!("npm test"));
        assert_eq!(surface["initial_input"], json!("hello"));
        assert_eq!(surface["initial_environment"], json!({"CI": "1"}));
        assert_eq!(surface["listening_ports"], json!([3000, 8080]));
        assert_eq!(surface["tty"], json!("/dev/pts/7"));
        assert_eq!(surface["tty_name"], json!("/dev/pts/7"));
        assert_eq!(surface["shell_activity"], json!("promptIdle"));
        assert_eq!(surface["shell_activity_state"], json!("promptIdle"));
    }

    #[test]
    fn debug_terminals_payload_includes_reported_tty() {
        let mut snapshot = test_snapshot();
        snapshot.windows[0].tab_manager.workspaces[0].panel_ttys =
            Some(vec![SessionPanelTtySnapshot {
                panel_id: "surface-1".to_string(),
                tty: "ttys004".to_string(),
                updated_at: 23,
            }]);

        let mut terminals = Vec::new();
        let workspace = &snapshot.windows[0].tab_manager.workspaces[0];
        for (surface_index, surface) in surfaces_for_workspace(workspace).into_iter().enumerate() {
            terminals.push(json!({
                "workspace_id": workspace.workspace_id,
                "workspace_ref": workspace_ref(0),
                "surface_id": surface.get("id").cloned().unwrap_or(Value::Null),
                "surface_ref": surface_ref(surface_index),
                "tty": surface.get("tty").cloned().unwrap_or(Value::Null),
            }));
        }

        assert_eq!(
            terminals[0],
            json!({
                "workspace_id": "workspace-1",
                "workspace_ref": "workspace:1",
                "surface_id": "surface-1",
                "surface_ref": "surface:1",
                "tty": "ttys004",
            })
        );
    }

    #[test]
    fn surface_list_payload_includes_restorable_agent_binding() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.restorable_agent_snapshots = Some(vec![SessionPanelRestorableAgentSnapshot {
            panel_id: "surface-1".to_string(),
            snapshot: SessionRestorableAgentSnapshot {
                kind: "codex".to_string(),
                session_id: "session-1".to_string(),
                working_directory: Some("C:/repo".to_string()),
                launch_command: Some(AgentLaunchCommandSnapshot {
                    launcher: None,
                    executable_path: Some("codex".to_string()),
                    arguments: vec!["resume".to_string(), "session-1".to_string()],
                    working_directory: Some("C:/repo".to_string()),
                    environment: Some(BTreeMap::from_iter([(
                        "CODEX_HOME".to_string(),
                        "C:/codex".to_string(),
                    )])),
                    source: Some("provider.start".to_string()),
                }),
                resume_command: Some("codex resume session-1".to_string()),
                fork_command: Some("codex fork session-1".to_string()),
            },
        }]);

        let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
            panic!("surface list should succeed");
        };
        let payload: Value = value.into();
        let binding = &payload["surfaces"][0]["resume_binding"];
        assert_eq!(binding["kind"], json!("codex"));
        assert_eq!(binding["session_id"], json!("session-1"));
        assert_eq!(binding["working_directory"], json!("C:/repo"));
        assert_eq!(binding["resume_command"], json!("codex resume session-1"));
        assert_eq!(binding["fork_command"], json!("codex fork session-1"));
        assert_eq!(binding["launch_command"]["executable_path"], json!("codex"));
        assert_eq!(
            binding["launch_command"]["environment"],
            json!({"CODEX_HOME": "C:/codex"})
        );
    }

    #[test]
    fn surface_list_payload_includes_markdown_file_and_diff_state() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                pane_id: Some("pane-1".to_string()),
                panel_ids: vec!["surface-1".to_string()],
                selected_panel_id: Some("surface-1".to_string()),
                surface_kind: Some("diff".to_string()),
                markdown_file_path: Some("C:/repo/README.md".to_string()),
                file_path: Some("C:/repo/notes.txt".to_string()),
                diff_viewer_token: Some("tok-abcdef0123456789".to_string()),
                diff_viewer_request_path: Some("/review/index.html".to_string()),
                browser_url: None,
                browser_proxy_url: None,
                browser_back_history: None,
                browser_forward_history: None,
                browser_omnibar_visible: None,
                browser_focus_mode_active: None,
                browser_developer_tools_visible: None,
                browser_developer_tools_panel: None,
                browser_page_zoom: None,
            },
        ));
        let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
            panic!("surface list should succeed");
        };
        let payload: Value = value.into();
        let surface = &payload["surfaces"][0];
        assert_eq!(surface["type"], json!("diff"));
        assert_eq!(surface["markdown_file_path"], json!("C:/repo/README.md"));
        assert_eq!(surface["file_path"], json!("C:/repo/notes.txt"));
        assert_eq!(surface["diff_viewer_token"], json!("tok-abcdef0123456789"));
        assert_eq!(
            surface["diff_viewer_request_path"],
            json!("/review/index.html")
        );
    }

    #[test]
    fn surface_list_payload_includes_browser_state() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                pane_id: Some("pane-1".to_string()),
                panel_ids: vec!["surface-1".to_string()],
                selected_panel_id: Some("surface-1".to_string()),
                surface_kind: Some("browser".to_string()),
                markdown_file_path: None,
                file_path: None,
                diff_viewer_token: None,
                diff_viewer_request_path: None,
                browser_url: Some("https://example.com".to_string()),
                browser_proxy_url: Some("socks5://127.0.0.1:31337".to_string()),
                browser_back_history: Some(vec!["https://previous.test".to_string()]),
                browser_forward_history: Some(vec!["https://forward.test".to_string()]),
                browser_omnibar_visible: Some(false),
                browser_focus_mode_active: Some(true),
                browser_developer_tools_visible: Some(true),
                browser_developer_tools_panel: Some("console".to_string()),
                browser_page_zoom: Some(1.25),
            },
        ));
        let ControlCallResult::Ok(value) = surface_list(&snapshot) else {
            panic!("surface list should succeed");
        };
        let payload: Value = value.into();
        let surface = &payload["surfaces"][0];
        assert_eq!(surface["type"], json!("browser"));
        assert_eq!(surface["browser_url"], json!("https://example.com"));
        assert_eq!(
            surface["browser_proxy_url"],
            json!("socks5://127.0.0.1:31337")
        );
        assert_eq!(surface["browser_can_go_back"], json!(true));
        assert_eq!(surface["browser_can_go_forward"], json!(true));
        assert_eq!(surface["browser_back_history_count"], json!(1));
        assert_eq!(surface["browser_forward_history_count"], json!(1));
        assert_eq!(surface["browser_omnibar_visible"], json!(false));
        assert_eq!(surface["browser_focus_mode_active"], json!(true));
        assert_eq!(surface["browser_developer_tools_visible"], json!(true));
        assert_eq!(surface["browser_developer_tools_panel"], json!("console"));
        assert_eq!(surface["browser_page_zoom"], json!(1.25));
    }

    #[test]
    fn ports_param_accepts_array_string_and_single_port() {
        assert_eq!(
            ports_param(&serde_json::Map::from_iter([(
                "ports".to_string(),
                json!([3000, "5173", 3000]),
            )])),
            Some(vec![3000, 5173])
        );
        assert_eq!(
            ports_param(&serde_json::Map::from_iter([(
                "listening_ports".to_string(),
                json!("8080, 9000 8080"),
            )])),
            Some(vec![8080, 9000])
        );
        assert_eq!(
            ports_param(&serde_json::Map::from_iter([(
                "port".to_string(),
                json!(1)
            )])),
            Some(vec![1])
        );
    }

    #[test]
    fn ports_param_rejects_out_of_range_or_non_integer_ports() {
        assert_eq!(
            ports_param(&serde_json::Map::from_iter([(
                "ports".to_string(),
                json!([0])
            )])),
            None
        );
        assert_eq!(
            ports_param(&serde_json::Map::from_iter([(
                "ports".to_string(),
                json!([65536]),
            )])),
            None
        );
        assert_eq!(
            ports_param(&serde_json::Map::from_iter([(
                "ports".to_string(),
                json!(["abc"]),
            )])),
            None
        );
    }

    #[test]
    fn optional_u16_param_accepts_numbers_strings_and_null_clear() {
        assert_eq!(
            optional_u16_param(
                &serde_json::Map::from_iter([("local_proxy_port".to_string(), json!(31337))]),
                "local_proxy_port",
            ),
            Some(Some(31337))
        );
        assert_eq!(
            optional_u16_param(
                &serde_json::Map::from_iter([("local_proxy_port".to_string(), json!("31338"))]),
                "local_proxy_port",
            ),
            Some(Some(31338))
        );
        assert_eq!(
            optional_u16_param(
                &serde_json::Map::from_iter([("local_proxy_port".to_string(), Value::Null)]),
                "local_proxy_port",
            ),
            Some(None)
        );
        assert_eq!(
            optional_u16_param(&serde_json::Map::new(), "local_proxy_port"),
            Some(None)
        );
    }

    #[test]
    fn optional_u16_param_rejects_invalid_ports() {
        for value in [json!(0), json!(65536), json!("abc"), json!([31337])] {
            assert_eq!(
                optional_u16_param(
                    &serde_json::Map::from_iter([("local_proxy_port".to_string(), value)]),
                    "local_proxy_port",
                ),
                None
            );
        }
    }

    #[test]
    fn string_vec_param_accepts_arrays_and_newline_strings() {
        assert_eq!(
            string_vec_param(
                &serde_json::Map::from_iter([(
                    "ssh_options".to_string(),
                    json!([
                        "StrictHostKeyChecking=no",
                        " ",
                        42,
                        "UserKnownHostsFile=/dev/null"
                    ]),
                )]),
                &["ssh_options"],
            ),
            Some(vec![
                "StrictHostKeyChecking=no".to_string(),
                "UserKnownHostsFile=/dev/null".to_string(),
            ])
        );
        assert_eq!(
            string_vec_param(
                &serde_json::Map::from_iter([(
                    "sshOptions".to_string(),
                    json!("ControlMaster=auto\n\nControlPersist=600"),
                )]),
                &["ssh_options", "sshOptions"],
            ),
            Some(vec![
                "ControlMaster=auto".to_string(),
                "ControlPersist=600".to_string(),
            ])
        );
    }

    #[test]
    fn surface_ports_kick_target_accepts_known_surface() {
        let target = surface_ports_kick_target(
            &test_snapshot(),
            &serde_json::Map::from_iter([("surface_id".to_string(), json!("surface-1"))]),
        )
        .expect("ports kick should target a known surface");
        assert_eq!(target, (0, "surface-1".to_string()));
    }

    #[test]
    fn surface_ports_kick_target_uses_workspace_scope_with_surface_index() {
        let mut snapshot = test_snapshot();
        let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
        second.workspace_id = Some("workspace-2".to_string());
        second.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                pane_id: Some("pane-2".to_string()),
                panel_ids: vec!["surface-2".to_string(), "surface-3".to_string()],
                selected_panel_id: Some("surface-3".to_string()),
                surface_kind: None,
                markdown_file_path: None,
                file_path: None,
                diff_viewer_token: None,
                diff_viewer_request_path: None,
                browser_url: None,
                browser_proxy_url: None,
                browser_back_history: None,
                browser_forward_history: None,
                browser_omnibar_visible: None,
                browser_focus_mode_active: None,
                browser_developer_tools_visible: None,
                browser_developer_tools_panel: None,
                browser_page_zoom: None,
            },
        ));
        snapshot.windows[0].tab_manager.workspaces.push(second);
        snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);

        let target = surface_ports_kick_target(
            &snapshot,
            &serde_json::Map::from_iter([
                ("workspace_id".to_string(), json!("workspace-2")),
                ("surface_ref".to_string(), json!("surface:1")),
            ]),
        )
        .expect("ports kick should target scoped surface ref");
        assert_eq!(target, (1, "surface-2".to_string()));
    }

    #[test]
    fn workspace_index_from_params_accepts_refs_or_workspace_id_not_index() {
        let snapshot = test_snapshot();
        assert_eq!(
            workspace_index_from_params(
                &snapshot,
                &serde_json::Map::from_iter([("index".to_string(), json!(0),)])
            ),
            None
        );
        assert_eq!(
            workspace_index_from_params(
                &snapshot,
                &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-1"),)])
            ),
            Some(0)
        );
        assert_eq!(
            workspace_index_from_params(
                &snapshot,
                &serde_json::Map::from_iter([("workspace_ref".to_string(), json!("workspace:1"),)])
            ),
            Some(0)
        );
        assert_eq!(
            workspace_index_from_params(
                &snapshot,
                &serde_json::Map::from_iter([("index".to_string(), json!(99),)])
            ),
            None
        );
    }

    #[test]
    fn workspace_indices_from_params_accepts_bulk_refs_and_ids_not_indices() {
        let mut snapshot = test_snapshot();
        let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
        second.workspace_id = Some("workspace-2".to_string());
        snapshot.windows[0].tab_manager.workspaces.push(second);

        assert_eq!(
            workspace_indices_from_params(
                &snapshot,
                &serde_json::Map::from_iter([("indices".to_string(), json!([0, "1"]),)])
            ),
            None
        );
        assert_eq!(
            workspace_indices_from_params(
                &snapshot,
                &serde_json::Map::from_iter([
                    ("workspace_refs".to_string(), json!(["workspace:2"])),
                    ("workspace_ids".to_string(), json!(["workspace-1"])),
                ])
            ),
            Some(vec![1, 0])
        );
        assert_eq!(
            workspace_indices_from_params(
                &snapshot,
                &serde_json::Map::from_iter([(
                    "workspace_refs".to_string(),
                    json!(["workspace:3"]),
                )])
            ),
            None
        );
    }

    #[test]
    fn workspace_reorder_destination_accepts_exactly_one_canonical_target() {
        let mut snapshot = test_snapshot();
        let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
        second.workspace_id = Some("workspace-2".to_string());
        let mut third = snapshot.windows[0].tab_manager.workspaces[0].clone();
        third.workspace_id = Some("workspace-3".to_string());
        snapshot.windows[0].tab_manager.workspaces.push(second);
        snapshot.windows[0].tab_manager.workspaces.push(third);

        assert_eq!(
            workspace_reorder_destination_index(
                &snapshot,
                &serde_json::Map::from_iter([(
                    "before_workspace_ref".to_string(),
                    json!("workspace:3"),
                )]),
                0,
            ),
            Some(1)
        );
        assert_eq!(
            workspace_reorder_destination_index(
                &snapshot,
                &serde_json::Map::from_iter([(
                    "after_workspace_ref".to_string(),
                    json!("workspace:3"),
                )]),
                0,
            ),
            Some(2)
        );
        assert_eq!(
            workspace_reorder_destination_index(
                &snapshot,
                &serde_json::Map::from_iter([(
                    "before_workspace_id".to_string(),
                    json!("workspace-1"),
                )]),
                2,
            ),
            Some(0)
        );
        assert_eq!(
            workspace_reorder_destination_index(
                &snapshot,
                &serde_json::Map::from_iter([(
                    "after_workspace_id".to_string(),
                    json!("workspace-1"),
                )]),
                2,
            ),
            Some(1)
        );
        assert_eq!(
            workspace_reorder_destination_index(
                &snapshot,
                &serde_json::Map::from_iter([("index".to_string(), json!(0))]),
                2,
            ),
            Some(0)
        );
        assert_eq!(
            workspace_reorder_destination_index(
                &snapshot,
                &serde_json::Map::from_iter([
                    ("index".to_string(), json!(0)),
                    ("before_workspace_ref".to_string(), json!("workspace:2"),),
                ]),
                2,
            ),
            None
        );
        assert_eq!(
            workspace_reorder_destination_index(
                &snapshot,
                &serde_json::Map::from_iter([("to_index".to_string(), json!(0))]),
                2,
            ),
            None
        );
    }

    #[test]
    fn workspace_reorder_window_scope_cannot_fall_through_to_first_window() {
        let snapshot = test_snapshot();
        assert!(workspace_reorder_window_matches(
            &snapshot,
            &serde_json::Map::new()
        ));
        assert!(workspace_reorder_window_matches(
            &snapshot,
            &serde_json::Map::from_iter([("window_ref".to_string(), json!("window:1"),)])
        ));
        assert!(!workspace_reorder_window_matches(
            &snapshot,
            &serde_json::Map::from_iter([("window_ref".to_string(), json!("window:2"),)])
        ));
    }

    #[test]
    fn workspace_reorder_many_order_resolves_refs_and_ids_in_request_order() {
        let first = "00000000-0000-0000-0000-000000000001";
        let second = "00000000-0000-0000-0000-000000000002";
        let mut snapshot = test_snapshot();
        snapshot.windows[0].tab_manager.workspaces[0].workspace_id = Some(first.to_string());
        let mut workspace = snapshot.windows[0].tab_manager.workspaces[0].clone();
        workspace.workspace_id = Some(second.to_string());
        snapshot.windows[0].tab_manager.workspaces.push(workspace);

        let order = workspace_reorder_many_order(
            &snapshot,
            &serde_json::Map::from_iter([(
                "workspace_ids".to_string(),
                json!(["workspace:2", first]),
            )]),
        )
        .unwrap();
        assert_eq!(
            order,
            [
                Uuid::parse_str(second).unwrap(),
                Uuid::parse_str(first).unwrap()
            ]
        );
        assert!(matches!(
            workspace_reorder_many_order(&snapshot, &serde_json::Map::new()),
            Err(WorkspaceReorderManyOrderError::Missing)
        ));
    }

    #[test]
    fn workspace_index_defaults_to_selected_for_current_commands() {
        let snapshot = test_snapshot();
        assert_eq!(
            workspace_index_from_params_or_selected(&snapshot, &serde_json::Map::new()),
            Some(0)
        );
    }

    #[test]
    fn workspace_index_from_params_requires_explicit_selector_for_close_commands() {
        let snapshot = test_snapshot();
        assert_eq!(
            canonical_workspace_target_index(&snapshot, 0, &serde_json::Map::new()),
            None
        );
        assert_eq!(
            canonical_workspace_target_index(
                &snapshot,
                0,
                &serde_json::Map::from_iter([("index".to_string(), json!(0),)])
            ),
            None
        );
        assert_eq!(
            canonical_workspace_target_index(
                &snapshot,
                0,
                &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-1"),)])
            ),
            Some(0)
        );
        assert_eq!(
            canonical_workspace_target_index(
                &snapshot,
                0,
                &serde_json::Map::from_iter([("workspace_ref".to_string(), json!("workspace:1"),)])
            ),
            None
        );
    }

    #[test]
    fn workspace_v2_list_has_canonical_shape_only() {
        let payload = workspace_list_payload_for_window(&test_snapshot(), 0);
        let object = payload.as_object().expect("workspace.list object");
        assert_eq!(
            object.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            BTreeSet::from(["window_id", "window_ref", "workspaces"])
        );
        let row = object["workspaces"][0]
            .as_object()
            .expect("workspace summary object");
        assert_eq!(
            row.keys().map(String::as_str).collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "id",
                "ref",
                "title",
                "custom_title",
                "has_custom_title",
                "description",
                "selected",
                "pinned",
                "listening_ports",
                "remote",
                "current_directory",
                "custom_color",
                "latest_conversation_message",
                "latest_submitted_message",
                "latest_submitted_at",
                "index",
            ])
        );
    }

    #[test]
    fn workspace_v2_current_routes_by_workspace_but_returns_owner_selection() {
        let mut snapshot = test_snapshot();
        snapshot.windows[0].window_id = Some("window-a".to_string());
        let mut background = snapshot.windows[0].clone();
        background.window_id = Some("window-b".to_string());
        background.tab_manager.workspaces[0].workspace_id = Some("workspace-b1".to_string());
        let mut requested = background.tab_manager.workspaces[0].clone();
        requested.workspace_id = Some("workspace-b2".to_string());
        background.tab_manager.workspaces.push(requested);
        background.tab_manager.selected_workspace_index = Some(0);
        snapshot.windows.push(background);

        let params =
            serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-b2"))]);
        let ControlCallResult::Ok(result) = workspace_current_from_params(&snapshot, &params)
        else {
            panic!("workspace.current should resolve the owning window");
        };
        let result: Value = result.into();
        assert_eq!(result["window_id"], json!("window-b"));
        assert_eq!(result["workspace_id"], json!("workspace-b1"));
        assert_eq!(result["workspace"]["selected"], json!(true));
    }

    #[test]
    fn workspace_v2_current_invalid_explicit_window_never_falls_back() {
        let params = serde_json::Map::from_iter([
            ("window_id".to_string(), json!("missing-window")),
            ("workspace_id".to_string(), json!("workspace-1")),
        ]);
        let result = workspace_current_from_params(&test_snapshot(), &params);
        assert!(matches!(
            result,
            ControlCallResult::Err { code, message, .. }
                if code == "unavailable" && message == "TabManager not available"
        ));
    }

    #[test]
    fn workspace_v2_current_stale_selection_preserves_identity_with_null_summary() {
        let mut snapshot = test_snapshot();
        snapshot.windows[0].selected_workspace_id = Some("workspace-1".to_string());
        snapshot.windows[0].tab_manager.selected_workspace_index = Some(99);

        let ControlCallResult::Ok(result) =
            workspace_current_from_params(&snapshot, &serde_json::Map::new())
        else {
            panic!("stale selected identity must still produce workspace.current success");
        };
        let result: Value = result.into();
        assert_eq!(result["workspace_id"], json!("workspace-1"));
        assert!(result["workspace_ref"].as_str().is_some());
        assert_eq!(result["workspace"], Value::Null);
    }

    #[test]
    fn workspace_v2_null_window_selector_falls_through_to_resolvable_workspace() {
        let params = serde_json::Map::from_iter([
            ("window_id".to_string(), Value::Null),
            ("workspace_id".to_string(), json!("workspace-1")),
        ]);
        assert_eq!(
            workspace_routed_window_index(&test_snapshot(), &params),
            Some(0)
        );
    }

    #[test]
    fn workspace_v2_routing_uses_canonical_selector_precedence() {
        let mut snapshot = test_snapshot();
        snapshot.windows[0].tab_manager.workspaces[0].group_id = Some("group-a".to_string());
        let mut background = snapshot.windows[0].clone();
        background.window_id = Some("window-b".to_string());
        background.tab_manager.workspaces[0].workspace_id = Some("workspace-b".to_string());
        background.tab_manager.workspaces[0].group_id = Some("group-b".to_string());
        if let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) =
            background.tab_manager.workspaces[0].layout.as_mut()
        {
            pane.pane_id = Some("pane-b".to_string());
            pane.panel_ids = vec!["surface-b".to_string()];
            pane.selected_panel_id = Some("surface-b".to_string());
        }
        snapshot.windows.push(background);

        assert_eq!(
            workspace_routed_window_index(
                &snapshot,
                &serde_json::Map::from_iter([
                    ("group_id".to_string(), json!("group-b")),
                    ("workspace_id".to_string(), json!("workspace-1")),
                ]),
            ),
            Some(1)
        );
        for key in [
            "workspace_id",
            "surface_id",
            "terminal_id",
            "tab_id",
            "pane_id",
        ] {
            let value = match key {
                "workspace_id" => "workspace-b",
                "pane_id" => "pane-b",
                _ => "surface-b",
            };
            assert_eq!(
                workspace_routed_window_index(
                    &snapshot,
                    &serde_json::Map::from_iter([(key.to_string(), json!(value))]),
                ),
                Some(1),
                "selector {key}"
            );
        }
    }

    #[test]
    fn workspace_v2_not_found_mints_workspace_ref_for_uuid_identity() {
        let workspace_id = "00000000-0000-0000-0000-000000000099";
        let mut registry = ControlHandleRegistry::default();
        let reference = registry.mint("workspace", workspace_id);
        let result = workspace_not_found_with_ref(workspace_id, &reference);
        let ControlCallResult::Err {
            data: Some(data), ..
        } = result
        else {
            panic!("not_found should include identity data");
        };
        let data: Value = data.into();
        assert_eq!(data["workspace_id"], json!(workspace_id));
        assert!(data["workspace_ref"].as_str().is_some());
    }

    #[test]
    fn workspace_handle_registry_is_stable_and_resolves_by_kind() {
        let mut registry = ControlHandleRegistry::default();
        let first = registry.mint("workspace", "workspace-a");
        let repeated = registry.mint("workspace", "workspace-a");
        let window = registry.mint("window", "window-a");

        assert_eq!(first, repeated);
        assert_eq!(
            registry.resolve("workspace", &first).as_deref(),
            Some("workspace-a")
        );
        assert_eq!(
            registry.resolve("window", &window).as_deref(),
            Some("window-a")
        );
        assert_eq!(registry.resolve("window", &first), None);
    }

    #[test]
    fn workspace_group_placement_uses_group_boundaries_and_reference() {
        let mut snapshot = test_snapshot();
        let first = &mut snapshot.windows[0].tab_manager.workspaces[0];
        first.group_id = Some("group-a".to_string());
        let mut second = first.clone();
        second.workspace_id = Some("workspace-2".to_string());
        let mut ungrouped = first.clone();
        ungrouped.workspace_id = Some("workspace-3".to_string());
        ungrouped.group_id = None;
        let tabs = &mut snapshot.windows[0].tab_manager;
        tabs.workspaces.extend([second, ungrouped]);

        assert_eq!(
            workspace_group_insert_index(tabs, "group-a", "top", None),
            Some(0)
        );
        assert_eq!(
            workspace_group_insert_index(tabs, "group-a", "end", None),
            Some(2)
        );
        assert_eq!(
            workspace_group_insert_index(tabs, "group-a", "afterCurrent", Some(0)),
            Some(1)
        );
    }

    #[test]
    fn workspace_select_focus_intent_targets_owning_window_only() {
        let mut snapshot = test_snapshot();
        let mut background = snapshot.windows[0].clone();
        background.window_id = Some("window-b".to_string());
        snapshot.windows.push(background);

        assert_eq!(
            workspace_select_focus_selector(&snapshot, 1),
            Some("window-b")
        );
        assert_eq!(workspace_select_focus_selector(&snapshot, 9), None);
    }

    #[test]
    fn workspace_events_observe_background_window_lifecycle_changes() {
        let previous = test_snapshot();
        let mut current = previous.clone();
        let mut background = current.windows[0].clone();
        background.window_id = Some("window-b".to_string());
        current.windows.push(background.clone());
        let mut previous_with_background = previous;
        previous_with_background.windows.push(background);
        current.windows[1].tab_manager.workspaces[0].custom_title = Some("Renamed".to_string());

        assert_ne!(
            session_event_summaries(&previous_with_background),
            session_event_summaries(&current),
            "background changes must reach lifecycle event derivation"
        );
    }

    #[test]
    fn string_map_param_accepts_string_environment_aliases() {
        let params = serde_json::Map::from_iter([(
            "env".to_string(),
            json!({
                "CMUX_FORK": "1",
                "EMPTY": "   ",
                "NUMBER": 7,
                "  TRIMMED_KEY  ": " value ",
            }),
        )]);
        let map = string_map_param(&params, &["environment", "env"]).expect("env map");
        assert_eq!(map.get("CMUX_FORK").map(String::as_str), Some("1"));
        assert_eq!(map.get("TRIMMED_KEY").map(String::as_str), Some("value"));
        assert!(!map.contains_key("EMPTY"));
        assert!(!map.contains_key("NUMBER"));
    }

    #[test]
    fn workspace_create_cwd_preserves_raw_fallback_but_trims_working_directory() {
        let inherited = Some("C:/inherited");
        assert_eq!(
            workspace_create_cwd_param(
                &serde_json::Map::from_iter([("cwd".to_string(), json!("  C:/raw  "))]),
                inherited,
            )
            .unwrap(),
            Some("  C:/raw  ".to_string())
        );
        assert_eq!(
            workspace_create_cwd_param(
                &serde_json::Map::from_iter([
                    ("working_directory".to_string(), json!("  C:/trimmed  ")),
                    ("cwd".to_string(), json!({"invalid": true})),
                ]),
                inherited,
            )
            .unwrap(),
            Some("C:/trimmed".to_string())
        );
        assert!(workspace_create_cwd_param(
            &serde_json::Map::from_iter([("cwd".to_string(), json!(42))]),
            inherited,
        )
        .is_err());
    }

    #[test]
    fn workspace_create_environment_sanitizers_preserve_values_and_differ() {
        let params = serde_json::Map::from_iter([
            (
                "initial_env".to_string(),
                json!({"  KEEP  ": "  value  ", "EMPTY": "", " ": "drop", "NUMBER": 7}),
            ),
            (
                "workspace_env".to_string(),
                json!({
                    "  KEEP  ": "  value  ",
                    "EMPTY": "",
                    "BAD=KEY": "drop",
                    "NUL\u{0000}KEY": "drop",
                    "NUL_VALUE": "bad\u{0000}value"
                }),
            ),
        ]);

        assert_eq!(
            workspace_create_initial_env(&params),
            BTreeMap::from([
                ("EMPTY".to_string(), "".to_string()),
                ("KEEP".to_string(), "  value  ".to_string()),
            ])
        );
        assert_eq!(
            workspace_create_workspace_env(&params),
            BTreeMap::from([("KEEP".to_string(), "  value  ".to_string())])
        );
    }

    #[test]
    fn raw_string_param_preserves_empty_strings_for_clearing_metadata() {
        let params = serde_json::Map::from_iter([("title".to_string(), json!(""))]);
        assert_eq!(raw_string_param(&params, &["title"]).as_deref(), Some(""));
        assert_eq!(string_param(&params, &["title"]), None);
    }

    #[test]
    fn surface_id_from_params_accepts_ref_id_or_focused_default_not_index() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                pane_id: Some("pane-1".to_string()),
                panel_ids: vec!["surface-1".to_string(), "surface-2".to_string()],
                selected_panel_id: Some("surface-2".to_string()),
                surface_kind: None,
                markdown_file_path: None,
                file_path: None,
                diff_viewer_token: None,
                diff_viewer_request_path: None,
                browser_url: None,
                browser_proxy_url: None,
                browser_back_history: None,
                browser_forward_history: None,
                browser_omnibar_visible: None,
                browser_focus_mode_active: None,
                browser_developer_tools_visible: None,
                browser_developer_tools_panel: None,
                browser_page_zoom: None,
            },
        ));
        workspace.focused_panel_id = Some("surface-2".to_string());

        assert_eq!(
            surface_id_from_params_or_focused(
                &snapshot,
                &serde_json::Map::from_iter([("index".to_string(), json!(0),)])
            ),
            None
        );
        assert_eq!(
            surface_id_from_params_or_focused(
                &snapshot,
                &serde_json::Map::from_iter([("surface_ref".to_string(), json!("surface:2"),)])
            ),
            Some("surface-2".to_string())
        );
        assert_eq!(
            surface_id_from_params_or_focused(
                &snapshot,
                &serde_json::Map::from_iter([("panel_id".to_string(), json!("surface-1"),)])
            ),
            Some("surface-1".to_string())
        );
        assert_eq!(
            surface_id_from_params_or_focused(&snapshot, &serde_json::Map::new()),
            Some("surface-2".to_string())
        );
    }

    #[test]
    fn surface_id_from_params_uses_ambient_workspace_scope_for_default_surface() {
        let mut snapshot = test_snapshot();
        let mut second = snapshot.windows[0].tab_manager.workspaces[0].clone();
        second.workspace_id = Some("workspace-2".to_string());
        second.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                pane_id: Some("pane-2".to_string()),
                panel_ids: vec!["surface-2".to_string(), "surface-3".to_string()],
                selected_panel_id: Some("surface-3".to_string()),
                surface_kind: None,
                markdown_file_path: None,
                file_path: None,
                diff_viewer_token: None,
                diff_viewer_request_path: None,
                browser_url: None,
                browser_proxy_url: None,
                browser_back_history: None,
                browser_forward_history: None,
                browser_omnibar_visible: None,
                browser_focus_mode_active: None,
                browser_developer_tools_visible: None,
                browser_developer_tools_panel: None,
                browser_page_zoom: None,
            },
        ));
        second.focused_panel_id = Some("surface-3".to_string());
        snapshot.windows[0].tab_manager.workspaces.push(second);
        snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);

        assert_eq!(
            surface_id_from_params_or_focused(&snapshot, &serde_json::Map::new()),
            Some("surface-1".to_string())
        );
        assert_eq!(
            surface_id_from_params_or_focused(
                &snapshot,
                &serde_json::Map::from_iter([("workspace_id".to_string(), json!("workspace-2"),)])
            ),
            Some("surface-3".to_string())
        );
        assert_eq!(
            surface_id_from_params_or_focused(
                &snapshot,
                &serde_json::Map::from_iter([
                    ("workspace_id".to_string(), json!("workspace-2")),
                    ("surface_ref".to_string(), json!("surface:1")),
                ])
            ),
            Some("surface-2".to_string())
        );
    }

    #[test]
    fn surface_ref_and_terminal_type_helpers_use_scoped_workspace() {
        let mut snapshot = test_snapshot();
        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        workspace.layout = Some(SessionWorkspaceLayoutSnapshot::Pane(
            SessionPaneLayoutSnapshot {
                pane_id: Some("pane-1".to_string()),
                panel_ids: vec!["surface-1".to_string()],
                selected_panel_id: Some("surface-1".to_string()),
                surface_kind: Some("browser".to_string()),
                markdown_file_path: None,
                file_path: None,
                diff_viewer_token: None,
                diff_viewer_request_path: None,
                browser_url: Some("https://example.com".to_string()),
                browser_proxy_url: None,
                browser_back_history: None,
                browser_forward_history: None,
                browser_omnibar_visible: None,
                browser_focus_mode_active: None,
                browser_developer_tools_visible: None,
                browser_developer_tools_panel: None,
                browser_page_zoom: None,
            },
        ));

        assert_eq!(
            surface_ref_for_panel(&snapshot, 0, "surface-1").as_deref(),
            Some("surface:1")
        );
        assert!(!surface_is_terminal(&snapshot, 0, "surface-1"));

        let workspace = snapshot.windows[0]
            .tab_manager
            .workspaces
            .first_mut()
            .unwrap();
        if let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = workspace.layout.as_mut() {
            pane.surface_kind = None;
        }
        assert!(surface_is_terminal(&snapshot, 0, "surface-1"));
    }

    #[test]
    fn terminal_key_sequence_maps_common_terminal_keys() {
        assert_eq!(terminal_key_sequence("enter"), Some("\r"));
        assert_eq!(terminal_key_sequence("ctrl+c"), Some("\x03"));
        assert_eq!(terminal_key_sequence("escape"), Some("\x1b"));
        assert_eq!(terminal_key_sequence("page-down"), Some("\x1b[6~"));
        assert_eq!(terminal_key_sequence("definitely-not-a-key"), None);
    }

    #[test]
    fn split_orientation_from_params_accepts_cmux_aliases() {
        assert_eq!(
            split_orientation_from_params(&serde_json::Map::new()),
            Some(SessionSplitOrientation::Horizontal)
        );
        assert_eq!(
            split_orientation_from_params(&serde_json::Map::from_iter([(
                "orientation".to_string(),
                json!("vertical"),
            )])),
            Some(SessionSplitOrientation::Vertical)
        );
        assert_eq!(
            split_orientation_from_params(&serde_json::Map::from_iter([(
                "direction".to_string(),
                json!("right"),
            )])),
            Some(SessionSplitOrientation::Horizontal)
        );
        assert_eq!(
            split_orientation_from_params(&serde_json::Map::from_iter([(
                "direction".to_string(),
                json!("u"),
            )])),
            Some(SessionSplitOrientation::Vertical)
        );
        assert_eq!(
            split_orientation_from_params(&serde_json::Map::from_iter([(
                "orientation".to_string(),
                json!("diagonal"),
            )])),
            None
        );
    }

    #[test]
    fn surface_kind_from_params_normalizes_terminal_and_known_surfaces() {
        assert_eq!(
            surface_kind_from_params(&serde_json::Map::from_iter([(
                "type".to_string(),
                json!("terminal"),
            )])),
            None
        );
        assert_eq!(
            surface_kind_from_params(&serde_json::Map::from_iter([(
                "kind".to_string(),
                json!("Browser"),
            )])),
            Some("browser".to_string())
        );
        assert_eq!(
            surface_kind_from_params(&serde_json::Map::from_iter([(
                "type".to_string(),
                json!("not-a-surface"),
            )])),
            Some("invalid".to_string())
        );
        assert_eq!(
            surface_kind_from_params(&serde_json::Map::new()),
            Some("invalid".to_string())
        );
    }

    #[test]
    fn f64_param_accepts_numbers_and_numeric_strings() {
        assert_eq!(
            f64_param(
                &serde_json::Map::from_iter([("zoom".to_string(), json!(1.25),)]),
                &["zoom"]
            ),
            Some(1.25)
        );
        assert_eq!(
            f64_param(
                &serde_json::Map::from_iter([("scale".to_string(), json!("1.5"),)]),
                &["zoom", "scale"]
            ),
            Some(1.5)
        );
        assert_eq!(
            f64_param(
                &serde_json::Map::from_iter([("zoom".to_string(), json!("nope"),)]),
                &["zoom"]
            ),
            None
        );
    }

    #[test]
    fn bool_param_accepts_booleans_and_common_strings() {
        assert_eq!(
            bool_param(
                &serde_json::Map::from_iter([("pinned".to_string(), json!(true),)]),
                &["pinned"]
            ),
            Some(true)
        );
        assert_eq!(
            bool_param(
                &serde_json::Map::from_iter([("unread".to_string(), json!("off"),)]),
                &["unread"]
            ),
            Some(false)
        );
        assert_eq!(
            bool_param(
                &serde_json::Map::from_iter([("pinned".to_string(), json!("maybe"),)]),
                &["pinned"]
            ),
            None
        );
    }

    #[test]
    fn remote_tmux_creation_uses_observed_window_arrival_and_typed_rollback() {
        let pane = RemoteTmuxTarget::for_create("split-window").unwrap();
        assert_eq!(pane.rollback_operation(), "kill-pane");
        assert!(pane.permits_immediate_arrival("runtime-pane-add"));

        let window = RemoteTmuxTarget::for_create("new-window").unwrap();
        assert_eq!(window.rollback_operation(), "kill-window");
        assert!(!window.permits_immediate_arrival("runtime-window-add"));
        assert!(!window.permits_immediate_arrival("runtime-pane-add"));
        assert!(immediate_remote_arrival(
            window,
            "runtime-window-add",
            "window",
            "workspace",
            "@12"
        )
        .is_none());
        assert!(
            immediate_remote_arrival(pane, "runtime-pane-add", "window", "workspace", "%34")
                .is_some()
        );

        let observed_window = StagedRemoteCreation {
            destination: "ssh://example.test".into(),
            target: window,
            token: "@12".into(),
            window_id: "window".into(),
            workspace_id: "workspace".into(),
            target_pane_id: Some("pane-existing".into()),
            source_surface_id: Some("surface-source".into()),
            source_pane_id: Some("pane-existing".into()),
            split_orientation: None,
            focus: false,
            observation: Some(RemoteTmuxObservation {
                window_token: "@12".into(),
                pane_token: "%34".into(),
            }),
            pane_observation: None,
            arrival: None,
        };
        let arrival = observed_remote_window_arrival(&observed_window, "%34")
            .expect("observed window pane should reconcile");
        assert_eq!(arrival.window_id, "window");
        assert_eq!(arrival.workspace_id, "workspace");
        assert_eq!(arrival.pane_id, "pane-existing");
        assert_eq!(arrival.remote_session_id, "%34");
        assert!(!arrival.creates_pane);
        assert_eq!(arrival.anchor_surface_id.as_deref(), Some("surface-source"));

        let immediate_pane = StagedRemoteCreation {
            destination: "ssh://example.test".into(),
            target: pane,
            token: "%34".into(),
            window_id: "window".into(),
            workspace_id: "workspace".into(),
            target_pane_id: None,
            source_surface_id: None,
            source_pane_id: None,
            split_orientation: None,
            focus: false,
            observation: None,
            pane_observation: Some("%34".into()),
            arrival: immediate_remote_arrival(
                pane,
                "runtime-pane-add",
                "window",
                "workspace",
                "%34",
            ),
        };
        assert!(observed_remote_window_arrival(&immediate_pane, "%34").is_none());
    }

    #[test]
    fn production_remote_new_window_builder_is_exact_for_focus_and_background() {
        let focused = RemoteTmuxCreateSpec {
            operation: "new-window",
            focus: true,
            source_target: Some("@7"),
            working_directory: Some("/srv/repo with spaces"),
        };
        assert_eq!(
            remote_tmux_create_argv(&focused).unwrap(),
            ["tmux new-window -a -t '@7' -c '/srv/repo with spaces' -P -F '#{window_id}\t#{pane_id}'"]
        );

        let background = RemoteTmuxCreateSpec {
            focus: false,
            ..focused
        };
        assert_eq!(
            remote_tmux_create_argv(&background).unwrap(),
            ["tmux new-window -d -a -t '@7' -c '/srv/repo with spaces' -P -F '#{window_id}\t#{pane_id}'"]
        );

        let fallback = RemoteTmuxCreateSpec {
            operation: "new-window",
            focus: false,
            source_target: None,
            working_directory: Some("/must/not/inherit"),
        };
        assert_eq!(
            remote_tmux_create_argv(&fallback).unwrap(),
            ["tmux new-window -d -a -t '{end}' -P -F '#{window_id}\t#{pane_id}'"]
        );

        let split = RemoteTmuxCreateSpec {
            operation: "split-window",
            focus: true,
            source_target: Some("@ignored"),
            working_directory: Some("/ignored"),
        };
        assert_eq!(
            remote_tmux_create_argv(&split).unwrap(),
            ["tmux split-window -P -F '#{pane_id}'"]
        );
        assert_eq!(
            remote_tmux_source_window_command("%7").unwrap(),
            ["tmux display-message -p -t '%7' '#{window_id}'"]
        );
        for invalid in ["@", "@x", "@7;echo", "@７"] {
            assert!(remote_tmux_create_argv(&RemoteTmuxCreateSpec {
                operation: "new-window",
                focus: false,
                source_target: Some(invalid),
                working_directory: None,
            })
            .is_err());
        }
    }

    #[test]
    fn production_remote_observation_is_authoritative_retried_and_compensated() {
        let observation = parse_remote_tmux_observation("@12\t%34\n").unwrap();
        assert_eq!(observation.window_token, "@12");
        assert_eq!(observation.pane_token, "%34");
        assert_eq!(
            parse_remote_tmux_observation("@12\t%34\r\n").unwrap(),
            observation
        );
        for invalid in [
            "@12\t%34\textra\n",
            "@12x\t%34\n",
            "@12\t%34\nextra\n",
            "@12\t%x\n",
        ] {
            assert!(parse_remote_tmux_observation(invalid).is_err());
        }
        assert_eq!(
            remote_observation_action(true, None, 0),
            RemoteObservationAction::Reconcile
        );
        assert_eq!(
            remote_observation_action(true, Some("transient persistence failure"), 0),
            RemoteObservationAction::RetainAndRetry
        );
        assert_eq!(
            remote_observation_action(false, None, 0),
            RemoteObservationAction::CompensateKillWindow
        );
        assert_eq!(
            remote_observation_action(true, Some("still failing"), REMOTE_OBSERVATION_MAX_RETRIES),
            RemoteObservationAction::CompensateKillWindow
        );
        assert!(should_focus_window_after_remote_arrival(true, true));
        assert!(!should_focus_window_after_remote_arrival(true, false));
        assert!(!should_focus_window_after_remote_arrival(false, true));
    }

    #[test]
    fn unchanged_effect_only_transition_skips_snapshot_commit_and_publication() {
        let previous = test_snapshot();
        assert!(!lifecycle_snapshot_changed(&previous, &previous));
        let mut candidate = previous.clone();
        candidate.created_at += 1;
        assert!(lifecycle_snapshot_changed(&candidate, &previous));
    }

    #[test]
    fn lifecycle_commit_failure_keeps_model_unpublished_and_compensates_resources() {
        #[derive(Default)]
        struct CommitFailure {
            prepared: Option<AppSessionSnapshot>,
            staged: usize,
            compensated: usize,
        }
        impl pane_surface_lifecycle::LifecycleEffectExecutor for CommitFailure {
            type Error = String;

            fn prepare_transition(
                &mut self,
                candidate: &AppSessionSnapshot,
            ) -> Result<(), Self::Error> {
                self.prepared = Some(candidate.clone());
                Ok(())
            }

            fn stage(
                &mut self,
                _effect: &pane_surface_lifecycle::LifecycleEffect,
            ) -> Result<(), Self::Error> {
                self.staged += 1;
                Ok(())
            }

            fn commit_staged(&mut self) -> Result<(), Self::Error> {
                Err("injected post-stage commit failure".into())
            }

            fn rollback_staged(&mut self) -> Result<(), Self::Error> {
                self.staged = 0;
                Ok(())
            }

            fn rollback_committed(&mut self) -> Result<(), Self::Error> {
                self.compensated += self.staged;
                self.staged = 0;
                Ok(())
            }
        }

        let before = test_snapshot();
        let transition = pane_surface_lifecycle::dispatch_lifecycle_request(
            &before,
            "pane.create",
            json!({"direction":"right","type":"terminal"})
                .as_object()
                .unwrap(),
            &pane_surface_lifecycle::LifecycleDispatchContext {
                viewport_size: Some((1_000.0, 800.0)),
                browser_enabled: true,
                dock_available: false,
                active_window_id: None,
            },
        );
        let candidate = transition.snapshot.clone();
        let effect_count = transition.effects.len();
        let mut published = before.clone();
        let mut executor = CommitFailure::default();
        assert!(pane_surface_lifecycle::commit_lifecycle_transition(
            &mut published,
            transition,
            &mut executor
        )
        .is_err());
        assert_eq!(published, before);
        assert_eq!(executor.prepared, Some(candidate));
        assert_eq!(executor.compensated, effect_count);
    }

    #[test]
    fn lifecycle_routing_uses_active_window_and_group_manager_selected_workspace() {
        let mut snapshot = test_snapshot();
        let mut second = snapshot.windows[0].clone();
        second.window_id = Some("window-2".into());
        let mut selected = second.tab_manager.workspaces[0].clone();
        selected.workspace_id = Some("workspace-selected".into());
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = selected.layout.as_mut().unwrap() else {
            unreachable!()
        };
        pane.pane_id = Some("pane-selected".into());
        pane.panel_ids = vec!["surface-selected".into()];
        pane.selected_panel_id = Some("surface-selected".into());
        selected.focused_panel_id = Some("surface-selected".into());
        let mut anchor = selected.clone();
        anchor.workspace_id = Some("workspace-anchor".into());
        anchor.focused_panel_id = Some("surface-anchor".into());
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = anchor.layout.as_mut().unwrap() else {
            unreachable!()
        };
        pane.pane_id = Some("pane-anchor".into());
        pane.panel_ids = vec!["surface-anchor".into()];
        pane.selected_panel_id = Some("surface-anchor".into());
        second.tab_manager.workspaces = vec![selected, anchor];
        second.tab_manager.selected_workspace_index = Some(0);
        second.tab_manager.workspace_groups = Some(vec![SessionWorkspaceGroupSnapshot {
            id: "group-2".into(),
            name: "Group".into(),
            anchor_workspace_id: Some("workspace-anchor".into()),
            ..Default::default()
        }]);
        snapshot.windows.push(second);
        let context = pane_surface_lifecycle::LifecycleDispatchContext {
            viewport_size: None,
            browser_enabled: true,
            dock_available: false,
            active_window_id: Some("window-2".into()),
        };

        for params in [
            json!({}),
            json!({"window_id":null}),
            json!({"group_id":"group-2"}),
        ] {
            let transition = pane_surface_lifecycle::dispatch_lifecycle_request(
                &snapshot,
                "surface.current",
                params.as_object().unwrap(),
                &context,
            );
            let ControlCallResult::Ok(value) = transition.result else {
                panic!("route failed")
            };
            let value = Value::from(value);
            assert_eq!(value["window_id"], "window-2");
            assert_eq!(value["workspace_id"], "workspace-selected");
        }
    }

    #[test]
    fn lifecycle_surface_create_preserves_all_heterogeneous_kinds() {
        let context = pane_surface_lifecycle::LifecycleDispatchContext {
            viewport_size: None,
            browser_enabled: true,
            dock_available: false,
            active_window_id: None,
        };
        for (token, expected) in [
            ("markdown", "markdown"),
            ("filePreview", "filePreview"),
            ("rightSidebarTool", "rightSidebarTool"),
            ("projectSidebar", "projectSidebar"),
            ("diff", "diff"),
        ] {
            let snapshot = test_snapshot();
            let params = json!({"pane_id":"pane-1","type":token});
            let transition = pane_surface_lifecycle::dispatch_lifecycle_request(
                &snapshot,
                "surface.create",
                params.as_object().unwrap(),
                &context,
            );
            let ControlCallResult::Ok(value) = &transition.result else {
                panic!("{token} failed")
            };
            assert_eq!(Value::from(value.clone())["type"], expected);
            assert!(transition.effects.iter().any(|effect| matches!(
                effect,
                pane_surface_lifecycle::LifecycleEffect::UiSurfaceAttach { kind, .. }
                    if kind == expected
            )));
        }

        let invalid = pane_surface_lifecycle::dispatch_lifecycle_request(
            &test_snapshot(),
            "surface.create",
            json!({"pane_id":"pane-1","type":"agentSession","renderer_kind":"canvas"})
                .as_object()
                .unwrap(),
            &context,
        );
        assert!(
            matches!(invalid.result, ControlCallResult::Err { code, .. } if code == "invalid_params")
        );
    }

    #[test]
    fn lifecycle_tab_refs_share_the_surface_handle_number() {
        let mut registry = ControlHandleRegistry::default();
        let surface_ref = registry.mint("surface", "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(surface_ref, "surface:1");
        let tab_ref = tab_ref_from_surface_ref(&surface_ref);
        assert_eq!(tab_ref, "tab:1");
        let normalized = surface_ref_from_tab_ref(&tab_ref).unwrap();
        assert_eq!(normalized, "surface:1");
        assert_eq!(
            registry.resolve("surface", &normalized).as_deref(),
            Some("550e8400-e29b-41d4-a716-446655440000")
        );
        assert_eq!(
            registry.resolve("surface", "550e8400-e29b-41d4-a716-446655440000"),
            None,
            "UUIDs bypass the ref registry and remain unchanged"
        );
    }

    #[test]
    fn shell_execute_codes_only_succeed_above_documented_error_range() {
        for code in [isize::MIN, 0, 2, 31, 32] {
            assert!(!shell_execute_succeeded(code), "code {code}");
        }
        for code in [33, 42, isize::MAX] {
            assert!(shell_execute_succeeded(code), "code {code}");
        }
    }

    #[test]
    fn lifecycle_result_decoration_covers_source_created_and_tab_id_families() {
        let mut value = json!({
            "window_id": "window-current",
            "source_window_id": "window-source",
            "workspace_id": "workspace-current",
            "source_workspace_id": "workspace-source",
            "created_workspace_id": "workspace-created",
            "pane_id": "pane-current",
            "surface_id": "surface-current",
            "created_surface_id": "surface-created",
            "tab_id": "surface-current",
            "created_tab_id": "surface-created",
            "nullable": { "created_surface_id": null },
            "rows": [{ "id": "surface-row" }]
        });
        let mut registry = ControlHandleRegistry::default();
        decorate_lifecycle_value_refs(&mut value, &mut |kind, id| registry.mint(kind, id));

        assert_eq!(value["window_ref"], "window:1");
        assert_eq!(value["source_window_ref"], "window:2");
        assert_eq!(value["workspace_ref"], "workspace:1");
        assert_eq!(value["source_workspace_ref"], "workspace:2");
        assert_eq!(value["created_workspace_ref"], "workspace:3");
        assert_eq!(value["pane_ref"], "pane:1");
        assert_eq!(value["surface_ref"], "surface:1");
        assert_eq!(value["created_surface_ref"], "surface:2");
        assert_eq!(value["tab_ref"], "tab:1");
        assert_eq!(value["created_tab_ref"], "tab:2");
        assert_eq!(value["nullable"]["created_surface_ref"], Value::Null);
        assert_eq!(value["rows"][0]["ref"], "surface:3");
    }

    #[path = "pane_surface_lifecycle_red.rs"]
    mod pane_surface_lifecycle_red;

    #[path = "surface_action_exhaustive_red.rs"]
    mod surface_action_exhaustive_red;

    #[path = "dock_api_adversarial_red.rs"]
    mod dock_api_adversarial_red;

    #[path = "dock_production_rollback_red.rs"]
    mod dock_production_rollback_red;

    #[path = "surface_action_adversarial_red.rs"]
    mod surface_action_adversarial_red;
}
