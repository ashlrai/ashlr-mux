//! Session-model bridge (Phase 2 workspace shell).
//!
//! Owns the authoritative **layout tree** — windows → workspaces → the split
//! tree of panes — as an [`AppSessionSnapshot`], and exposes commands to mutate
//! it (split / close / move-divider) plus a `cmux://session-changed` event that
//! carries the new snapshot to the webview.
//!
//! Deliberately structure-only: this layer does **not** spawn or kill
//! pseudo-consoles. Each pane's terminal lifecycle stays with the web
//! `<TerminalSurface>` (mount → `terminal_open`, unmount → `terminal_close`),
//! keyed by the pane's stable `panel_id`. That keeps the session layer pure and
//! headless-testable while the ConPTY plumbing lives in `terminal.rs`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use cmux_core::session::{
    AgentLaunchCommandSnapshot, AppSessionSnapshot, SessionGitBranchSnapshot,
    SessionPanelGitBranchSnapshot, SessionPanelListeningPortsSnapshot,
    SessionPanelPullRequestSnapshot, SessionPanelRestorableAgentSnapshot,
    SessionPanelShellActivitySnapshot, SessionPanelShellActivityStateSnapshot,
    SessionPanelTerminalStartupSnapshot, SessionPanelTtySnapshot, SessionPullRequestStatusSnapshot,
    SessionRestorableAgentSnapshot, SessionSplitOrientation, SessionTabManagerSnapshot,
    SessionWindowSnapshot, SessionWorkspaceAgentPidSnapshot, SessionWorkspaceLayoutSnapshot,
    SessionWorkspaceRemoteDaemonSnapshot, SessionWorkspaceRemoteProxySnapshot,
    SessionWorkspaceRemoteSnapshot, SessionWorkspaceSidebarLogEntrySnapshot,
    SessionWorkspaceSidebarMetadataBlockSnapshot, SessionWorkspaceSidebarMetadataSnapshot,
    SessionWorkspaceSidebarProgressSnapshot, SessionWorkspaceSidebarStatusSnapshot,
    SessionWorkspaceSnapshot, SESSION_SNAPSHOT_SCHEMA_VERSION,
};
use cmux_core::session_ops::{self, CloseOutcome, SplitChild};
use cmux_workspaces::{WorkspaceBatchReorderError, WorkspaceReorderPlanItem};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

/// Event carrying the full session snapshot after any structural change.
const SESSION_CHANGED_EVENT: &str = "cmux://session-changed";
const CURRENT_SESSION_SNAPSHOT_FILENAME: &str = "session-current.json";
const PREVIOUS_SESSION_SNAPSHOT_FILENAME: &str = "session-previous.json";
const DEFAULT_BROWSER_URL: &str = "about:blank";
const CLOSED_BROWSER_HISTORY_LIMIT: usize = 25;
const SIDEBAR_LOG_ENTRY_LIMIT: usize = 100;

/// The panel id of the very first pane. Later panes are `surface-N` (N from the
/// [`SessionState`] counter).
const FIRST_PANEL_ID: &str = "surface-1";

/// Managed Tauri state: the authoritative session snapshot + a monotonic panel
/// id counter so every new pane gets a unique, stable id.
pub struct SessionState {
    snapshot: Mutex<AppSessionSnapshot>,
    next_panel: AtomicU64,
    closed_browser_tabs: Mutex<Vec<ClosedBrowserTabSnapshot>>,
    remote_configs: Mutex<HashMap<String, WorkspaceRemoteControlConfig>>,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            snapshot: Mutex::new(initial_snapshot(FIRST_PANEL_ID)),
            next_panel: AtomicU64::new(2),
            closed_browser_tabs: Mutex::new(Vec::new()),
            remote_configs: Mutex::new(HashMap::new()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ClosedBrowserTabSnapshot {
    url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionNavigationTarget {
    workspace_id: String,
    panel_id: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct SessionNavigationHandleReply {
    pub handled: bool,
    pub changed: bool,
    pub workspace_id: String,
    pub panel_id: Option<String>,
    pub message: String,
}

#[derive(Debug, serde::Serialize)]
pub struct SessionSshUriHandleReply {
    pub handled: bool,
    pub changed: bool,
    pub panel_id: String,
    pub destination: String,
    pub no_focus: bool,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceRemoteControlConfig {
    pub transport: String,
    pub destination: String,
    pub port: Option<u16>,
    pub local_proxy_port: Option<u16>,
    pub persistent_daemon_slot: Option<String>,
    pub remote_daemon_path: Option<String>,
    pub remote_daemon_relay_port: Option<u16>,
    pub identity_file: Option<String>,
    pub ssh_options: Vec<String>,
    pub auto_connect: bool,
}

/// A single window / single workspace / single pane starting layout.
fn initial_snapshot(first_panel_id: &str) -> AppSessionSnapshot {
    let mut snapshot = AppSessionSnapshot {
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
        created_at: 0,
        windows: vec![SessionWindowSnapshot {
            window_id: Some("window-1".to_string()),
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![session_ops::fresh_terminal_workspace(first_panel_id)],
                workspace_groups: None,
            },
        }],
    };
    ensure_workspace_ids(&mut snapshot);
    ensure_pane_ids(&mut snapshot);
    snapshot
}

/// Mint a `workspace_id` for every workspace that lacks one. Canonical parity:
/// the Swift restore mints a fresh UUID exactly once per workspace missing an id
/// (`TabManager.swift:5960-5975`), and live `Workspace`s carry an identity from
/// init. This stateful session layer is the sole owner of id synthesis — the
/// pure `session_ops` builders stay deterministic and stateless projections
/// (`sidebar_render`, the web sidebar) never re-mint, they skip id-less rows.
fn ensure_workspace_ids(snapshot: &mut AppSessionSnapshot) {
    for window in &mut snapshot.windows {
        for workspace in &mut window.tab_manager.workspaces {
            if workspace.workspace_id.is_none() {
                workspace.workspace_id = Some(Uuid::new_v4().to_string());
            }
        }
    }
}

/// Mint a stable `pane_id` for every pane that lacks one. This mirrors the
/// workspace-id rule above: older snapshots decode without pane ids, and the
/// stateful desktop layer synthesizes them exactly once so downstream pure/UI
/// consumers can treat pane identity as stable.
fn ensure_pane_ids(snapshot: &mut AppSessionSnapshot) {
    fn ensure_layout_pane_ids(layout: &mut SessionWorkspaceLayoutSnapshot) {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                if pane.pane_id.is_none() {
                    pane.pane_id = Some(Uuid::new_v4().to_string());
                }
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                ensure_layout_pane_ids(&mut split.first);
                ensure_layout_pane_ids(&mut split.second);
            }
        }
    }

    for window in &mut snapshot.windows {
        for workspace in &mut window.tab_manager.workspaces {
            if let Some(layout) = workspace.layout.as_mut() {
                ensure_layout_pane_ids(layout);
            }
        }
    }
}

fn session_snapshot_paths(app: &AppHandle) -> Option<(PathBuf, PathBuf)> {
    let root = app.path().app_data_dir().ok()?.join("cmux");
    Some((
        root.join(CURRENT_SESSION_SNAPSHOT_FILENAME),
        root.join(PREVIOUS_SESSION_SNAPSHOT_FILENAME),
    ))
}

fn load_snapshot_file(path: &Path) -> Option<AppSessionSnapshot> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<AppSessionSnapshot>(&bytes).ok())
}

fn write_snapshot_file(path: &Path, snapshot: &AppSessionSnapshot) {
    let Ok(bytes) = serde_json::to_vec_pretty(snapshot) else {
        return;
    };
    let Some(parent) = path.parent() else {
        return;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp = path.with_extension("json.tmp");
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

fn persist_current_snapshot(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    if let Some((current, _previous)) = session_snapshot_paths(app) {
        write_snapshot_file(&current, snapshot);
    }
}

fn next_panel_counter(snapshot: &AppSessionSnapshot) -> u64 {
    fn visit_layout(layout: &SessionWorkspaceLayoutSnapshot, max_seen: &mut u64) {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                for panel_id in &pane.panel_ids {
                    let Some(raw) = panel_id.strip_prefix("surface-") else {
                        continue;
                    };
                    let Ok(value) = raw.parse::<u64>() else {
                        continue;
                    };
                    *max_seen = (*max_seen).max(value);
                }
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit_layout(&split.first, max_seen);
                visit_layout(&split.second, max_seen);
            }
        }
    }

    let mut max_seen = 0u64;
    for window in &snapshot.windows {
        for workspace in &window.tab_manager.workspaces {
            if let Some(layout) = workspace.layout.as_ref() {
                visit_layout(layout, &mut max_seen);
            }
        }
    }
    max_seen.saturating_add(1).max(1)
}

/// Rotate the last persisted live snapshot into the "previous launch" slot,
/// then persist the current in-memory session as the new live snapshot. Called
/// once during app setup before the web layer begins mutating the session.
pub fn bootstrap_session_persistence(app: &AppHandle, state: State<'_, SessionState>) {
    if let Some((current, previous)) = session_snapshot_paths(app) {
        if current.exists() {
            if let Some(parent) = previous.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::copy(&current, &previous);
        }
    }

    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        ensure_workspace_ids(&mut guard);
        ensure_pane_ids(&mut guard);
        guard.clone()
    };
    persist_current_snapshot(app, &snapshot);
}

/// The `Option<layout>` slot of the currently-selected workspace of the first
/// window, or `None` if there is no window/workspace.
fn active_workspace_slot(
    snapshot: &mut AppSessionSnapshot,
) -> Option<&mut SessionWorkspaceSnapshot> {
    let window = snapshot.windows.first_mut()?;
    // Copy the index before borrowing `workspaces` (both live on `tab_manager`).
    let raw_index = window.tab_manager.selected_workspace_index.unwrap_or(0);
    let index = if raw_index < 0 {
        0usize
    } else {
        raw_index as usize
    };
    let workspaces = &mut window.tab_manager.workspaces;
    let workspace = if index < workspaces.len() {
        &mut workspaces[index]
    } else {
        workspaces.first_mut()?
    };
    Some(workspace)
}

fn active_workspace(snapshot: &AppSessionSnapshot) -> Option<&SessionWorkspaceSnapshot> {
    let window = snapshot.windows.first()?;
    let raw_index = window.tab_manager.selected_workspace_index.unwrap_or(0);
    let index = if raw_index < 0 {
        0usize
    } else {
        raw_index as usize
    };
    let workspaces = &window.tab_manager.workspaces;
    if index < workspaces.len() {
        Some(&workspaces[index])
    } else {
        workspaces.first()
    }
}

/// The `Option<layout>` slot of the currently-selected workspace of the first
/// window, or `None` if there is no window/workspace.
fn active_layout_slot(
    snapshot: &mut AppSessionSnapshot,
) -> Option<&mut Option<SessionWorkspaceLayoutSnapshot>> {
    Some(&mut active_workspace_slot(snapshot)?.layout)
}

fn selected_panel_in_layout(layout: &SessionWorkspaceLayoutSnapshot) -> Option<&str> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => pane
            .selected_panel_id
            .as_deref()
            .or_else(|| pane.panel_ids.first().map(String::as_str)),
        SessionWorkspaceLayoutSnapshot::Split(split) => selected_panel_in_layout(&split.first)
            .or_else(|| selected_panel_in_layout(&split.second)),
    }
}

fn active_panel_id(snapshot: &AppSessionSnapshot) -> Option<String> {
    let workspace = active_workspace(snapshot)?;
    let layout = workspace.layout.as_ref()?;
    selected_panel_in_layout(layout).map(str::to_string)
}

/// Split the pane holding `target_panel_id`, adding `new_panel_id`. Returns
/// whether a matching pane was found. Pure — no ConPTY, no events.
fn apply_split(
    snapshot: &mut AppSessionSnapshot,
    target_panel_id: &str,
    orientation: SessionSplitOrientation,
    new_panel_id: &str,
    insert_first: bool,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed = session_ops::split_pane(
        root,
        target_panel_id,
        orientation,
        new_panel_id,
        insert_first,
    );
    if changed {
        ensure_pane_ids(snapshot);
    }
    changed
}

fn apply_split_with_terminal_startup(
    snapshot: &mut AppSessionSnapshot,
    target_panel_id: &str,
    orientation: SessionSplitOrientation,
    new_panel_id: &str,
    insert_first: bool,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> bool {
    if !apply_split(
        snapshot,
        target_panel_id,
        orientation,
        new_panel_id,
        insert_first,
    ) {
        return false;
    }
    set_panel_terminal_startup(
        snapshot,
        new_panel_id,
        initial_terminal_command,
        initial_terminal_input,
        initial_terminal_environment,
    );
    true
}

fn apply_new_terminal_tab(
    snapshot: &mut AppSessionSnapshot,
    anchor_panel_id: &str,
    new_panel_id: &str,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> bool {
    apply_new_terminal_tab_with_focus(
        snapshot,
        anchor_panel_id,
        new_panel_id,
        initial_terminal_command,
        initial_terminal_input,
        initial_terminal_environment,
        true,
    )
}

fn apply_new_terminal_tab_with_focus(
    snapshot: &mut AppSessionSnapshot,
    anchor_panel_id: &str,
    new_panel_id: &str,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
    focus_new_panel: bool,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    if !session_ops::add_panel_to_pane(root, anchor_panel_id, new_panel_id) {
        return false;
    }
    if !focus_new_panel {
        session_ops::select_panel(root, anchor_panel_id);
    }
    set_panel_terminal_startup(
        snapshot,
        new_panel_id,
        initial_terminal_command,
        initial_terminal_input,
        initial_terminal_environment,
    );
    true
}

fn set_panel_terminal_startup(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) {
    let command = initial_terminal_command
        .and_then(normalize_nonempty)
        .map(str::to_string);
    let input = initial_terminal_input
        .and_then(normalize_nonempty)
        .map(str::to_string);
    let environment = initial_terminal_environment.filter(|entries| !entries.is_empty());
    if command.is_none() && input.is_none() && environment.is_none() {
        return;
    }
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return;
    };
    let mut entries = workspace.panel_terminal_startups.take().unwrap_or_default();
    entries.retain(|entry| entry.panel_id != panel_id);
    entries.push(SessionPanelTerminalStartupSnapshot {
        panel_id: panel_id.to_string(),
        initial_terminal_command: command,
        initial_terminal_input: input,
        initial_terminal_environment: environment,
    });
    workspace.panel_terminal_startups = Some(entries);
}

fn ssh_terminal_command(request: &cmux_ssh::CmuxSSHURLRequest) -> String {
    let mut args = vec!["ssh".to_string()];
    if let Some(port) = request.port {
        args.push("-p".to_string());
        args.push(port.to_string());
    }
    for option in &request.ssh_options {
        args.push("-o".to_string());
        args.push(option.clone());
    }
    args.push(request.destination.clone());
    shell_join(&args)
}

fn shell_join(args: &[String]) -> String {
    args.iter()
        .map(|arg| shell_quote_argument(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote_argument(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"@%_+=:,./-".contains(&byte))
    {
        return arg.to_string();
    }
    if cfg!(windows) {
        format!("'{}'", arg.replace('\'', "''"))
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn apply_ssh_url_request(
    snapshot: &mut AppSessionSnapshot,
    anchor_panel_id: &str,
    new_panel_id: &str,
    request: &cmux_ssh::CmuxSSHURLRequest,
) -> bool {
    let command = ssh_terminal_command(request);
    apply_new_terminal_tab_with_focus(
        snapshot,
        anchor_panel_id,
        new_panel_id,
        Some(&command),
        None,
        None,
        !request.no_focus,
    )
}

fn sorted_unique_ports(ports: &[u16]) -> Vec<u16> {
    let mut ports: Vec<u16> = ports
        .iter()
        .copied()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    ports.sort_unstable();
    ports
}

fn recompute_workspace_listening_ports(workspace: &mut SessionWorkspaceSnapshot) {
    let ports: Vec<u16> = workspace
        .agent_listening_ports
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
        .collect();
    let ports = sorted_unique_ports(&ports);
    workspace.listening_ports = (!ports.is_empty()).then_some(ports);
}

fn set_workspace_agent_listening_ports(
    workspace: &mut SessionWorkspaceSnapshot,
    ports: &[u16],
) -> bool {
    let normalized_ports = sorted_unique_ports(ports);
    let before_agent_listening_ports = workspace.agent_listening_ports.clone();
    let before_listening_ports = workspace.listening_ports.clone();
    workspace.agent_listening_ports = (!normalized_ports.is_empty()).then_some(normalized_ports);
    recompute_workspace_listening_ports(workspace);
    workspace.agent_listening_ports != before_agent_listening_ports
        || workspace.listening_ports != before_listening_ports
}

fn set_workspace_agent_pid(
    workspace: &mut SessionWorkspaceSnapshot,
    key: &str,
    pid: u32,
    updated_at: i64,
) -> bool {
    let key = key.trim();
    if key.is_empty() || pid == 0 {
        return false;
    }
    let before = workspace.agent_pids.clone();
    let mut entries = workspace.agent_pids.take().unwrap_or_default();
    match entries.iter_mut().find(|entry| entry.key == key) {
        Some(entry) => {
            entry.pid = pid;
            entry.updated_at = updated_at;
        }
        None => entries.push(SessionWorkspaceAgentPidSnapshot {
            key: key.to_string(),
            pid,
            updated_at,
        }),
    }
    entries.sort_by(|left, right| left.key.cmp(&right.key));
    workspace.agent_pids = Some(entries);
    workspace.agent_pids != before
}

fn clear_workspace_agent_pid(workspace: &mut SessionWorkspaceSnapshot, key: &str) -> bool {
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let Some(entries) = workspace.agent_pids.as_mut() else {
        return false;
    };
    let before = entries.len();
    entries.retain(|entry| entry.key != key);
    if entries.is_empty() {
        workspace.agent_pids = None;
    }
    before != workspace.agent_pids.as_ref().map_or(0, Vec::len)
}

fn set_workspace_git_facts(
    workspace: &mut SessionWorkspaceSnapshot,
    git_branch: Option<SessionGitBranchSnapshot>,
    mut panel_git_branches: Vec<SessionPanelGitBranchSnapshot>,
    mut panel_pull_requests: Vec<SessionPanelPullRequestSnapshot>,
) -> bool {
    let before_git_branch = workspace.git_branch.clone();
    let before_panel_git_branches = workspace.panel_git_branches.clone();
    let before_panel_pull_requests = workspace.panel_pull_requests.clone();

    panel_git_branches.sort_by(|left, right| left.panel_id.cmp(&right.panel_id));
    panel_pull_requests.sort_by(|left, right| {
        left.panel_id
            .cmp(&right.panel_id)
            .then_with(|| left.number.cmp(&right.number))
            .then_with(|| left.url.cmp(&right.url))
    });

    workspace.git_branch = git_branch;
    workspace.panel_git_branches = (!panel_git_branches.is_empty()).then_some(panel_git_branches);
    workspace.panel_pull_requests =
        (!panel_pull_requests.is_empty()).then_some(panel_pull_requests);

    workspace.git_branch != before_git_branch
        || workspace.panel_git_branches != before_panel_git_branches
        || workspace.panel_pull_requests != before_panel_pull_requests
}

fn set_workspace_panel_pull_request(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    number: i64,
    label: &str,
    url: &str,
    status: SessionPullRequestStatusSnapshot,
    branch: Option<String>,
    is_stale: bool,
) -> bool {
    let panel_id = panel_id.trim();
    if panel_id.is_empty()
        || !workspace
            .layout
            .as_ref()
            .is_some_and(|layout| session_ops::contains_panel(layout, panel_id))
    {
        return false;
    }

    let before = workspace.panel_pull_requests.clone();
    let mut entries = workspace.panel_pull_requests.take().unwrap_or_default();
    entries.retain(|entry| entry.panel_id != panel_id);
    entries.push(SessionPanelPullRequestSnapshot {
        panel_id: panel_id.to_string(),
        number,
        label: label.to_string(),
        url: url.to_string(),
        status,
        branch,
        is_stale,
    });
    entries.sort_by(|left, right| {
        left.panel_id
            .cmp(&right.panel_id)
            .then_with(|| left.number.cmp(&right.number))
            .then_with(|| left.url.cmp(&right.url))
    });
    workspace.panel_pull_requests = Some(entries);
    workspace.panel_pull_requests != before
}

fn clear_workspace_panel_pull_request(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
) -> bool {
    let panel_id = panel_id.trim();
    if panel_id.is_empty() {
        return false;
    }
    let Some(entries) = workspace.panel_pull_requests.as_mut() else {
        return false;
    };
    let before = entries.len();
    entries.retain(|entry| entry.panel_id != panel_id);
    if entries.is_empty() {
        workspace.panel_pull_requests = None;
    }
    before != workspace.panel_pull_requests.as_ref().map_or(0, Vec::len)
}

fn set_workspace_panel_listening_ports(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    ports: &[u16],
) -> bool {
    let panel_id = panel_id.trim();
    if panel_id.is_empty()
        || !workspace
            .layout
            .as_ref()
            .is_some_and(|layout| session_ops::contains_panel(layout, panel_id))
    {
        return false;
    }

    let normalized_ports = sorted_unique_ports(ports);
    let before_panel_listening_ports = workspace.panel_listening_ports.clone();
    let before_listening_ports = workspace.listening_ports.clone();
    let mut entries = workspace.panel_listening_ports.take().unwrap_or_default();
    match entries.iter_mut().find(|entry| entry.panel_id == panel_id) {
        Some(_) if normalized_ports.is_empty() => {
            entries.retain(|entry| entry.panel_id != panel_id);
        }
        Some(entry) if entry.ports == normalized_ports => {}
        Some(entry) => {
            entry.ports = normalized_ports;
        }
        None if normalized_ports.is_empty() => {}
        None => entries.push(SessionPanelListeningPortsSnapshot {
            panel_id: panel_id.to_string(),
            ports: normalized_ports,
        }),
    }
    entries.sort_by(|left, right| left.panel_id.cmp(&right.panel_id));
    workspace.panel_listening_ports = (!entries.is_empty()).then_some(entries);
    recompute_workspace_listening_ports(workspace);
    workspace.panel_listening_ports != before_panel_listening_ports
        || workspace.listening_ports != before_listening_ports
}

fn prune_workspace_panel_listening_ports(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
) -> bool {
    let Some(entries) = workspace.panel_listening_ports.as_mut() else {
        return false;
    };
    let before = entries.len();
    entries.retain(|entry| entry.panel_id != panel_id);
    if entries.is_empty() {
        workspace.panel_listening_ports = None;
    }
    let changed = before != workspace.panel_listening_ports.as_ref().map_or(0, Vec::len);
    if changed {
        recompute_workspace_listening_ports(workspace);
    }
    changed
}

fn set_workspace_panel_tty(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    tty: &str,
    updated_at: i64,
) -> bool {
    let panel_id = panel_id.trim();
    let tty = tty.trim();
    if panel_id.is_empty()
        || tty.is_empty()
        || !workspace
            .layout
            .as_ref()
            .is_some_and(|layout| session_ops::contains_panel(layout, panel_id))
    {
        return false;
    }

    let before = workspace.panel_ttys.clone();
    let mut entries = workspace.panel_ttys.take().unwrap_or_default();
    match entries.iter_mut().find(|entry| entry.panel_id == panel_id) {
        Some(entry) => {
            entry.tty = tty.to_string();
            entry.updated_at = updated_at;
        }
        None => entries.push(SessionPanelTtySnapshot {
            panel_id: panel_id.to_string(),
            tty: tty.to_string(),
            updated_at,
        }),
    }
    entries.sort_by(|left, right| left.panel_id.cmp(&right.panel_id));
    workspace.panel_ttys = Some(entries);
    workspace.panel_ttys != before
}

fn prune_workspace_panel_tty(workspace: &mut SessionWorkspaceSnapshot, panel_id: &str) -> bool {
    let Some(entries) = workspace.panel_ttys.as_mut() else {
        return false;
    };
    let before = entries.len();
    entries.retain(|entry| entry.panel_id != panel_id);
    if entries.is_empty() {
        workspace.panel_ttys = None;
    }
    before != workspace.panel_ttys.as_ref().map_or(0, Vec::len)
}

fn set_workspace_panel_shell_activity(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    state: SessionPanelShellActivityStateSnapshot,
    updated_at: i64,
) -> bool {
    let panel_id = panel_id.trim();
    if panel_id.is_empty()
        || !workspace
            .layout
            .as_ref()
            .is_some_and(|layout| session_ops::contains_panel(layout, panel_id))
    {
        return false;
    }

    let before = workspace.panel_shell_activity.clone();
    let mut entries = workspace.panel_shell_activity.take().unwrap_or_default();
    match entries.iter_mut().find(|entry| entry.panel_id == panel_id) {
        Some(entry) => {
            entry.state = state;
            entry.updated_at = updated_at;
        }
        None => entries.push(SessionPanelShellActivitySnapshot {
            panel_id: panel_id.to_string(),
            state,
            updated_at,
        }),
    }
    entries.sort_by(|left, right| left.panel_id.cmp(&right.panel_id));
    workspace.panel_shell_activity = Some(entries);
    workspace.panel_shell_activity != before
}

fn prune_workspace_panel_shell_activity(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
) -> bool {
    let Some(entries) = workspace.panel_shell_activity.as_mut() else {
        return false;
    };
    let before = entries.len();
    entries.retain(|entry| entry.panel_id != panel_id);
    if entries.is_empty() {
        workspace.panel_shell_activity = None;
    }
    before != workspace.panel_shell_activity.as_ref().map_or(0, Vec::len)
}

/// Close `panel_id`, collapsing an emptied split into its sibling. Pure.
fn apply_close(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> CloseOutcome {
    let outcome = {
        let Some(slot) = active_layout_slot(snapshot) else {
            return CloseOutcome::NotFound;
        };
        session_ops::close_panel(slot, panel_id)
    };
    if matches!(outcome, CloseOutcome::Removed) {
        for window in &mut snapshot.windows {
            for workspace in &mut window.tab_manager.workspaces {
                prune_workspace_panel_listening_ports(workspace, panel_id);
                prune_workspace_panel_tty(workspace, panel_id);
                prune_workspace_panel_shell_activity(workspace, panel_id);
            }
        }
    };
    outcome
}

fn restorable_browser_url(url: Option<&str>) -> Option<String> {
    let trimmed = url?.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case(DEFAULT_BROWSER_URL) {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn closed_browser_tab(url: Option<&str>) -> Option<ClosedBrowserTabSnapshot> {
    restorable_browser_url(url).map(|url| ClosedBrowserTabSnapshot { url })
}

fn closed_browser_tab_for_panel(
    layout: &SessionWorkspaceLayoutSnapshot,
    panel_id: &str,
) -> Option<ClosedBrowserTabSnapshot> {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if pane.panel_ids.len() == 1
                && pane.panel_ids.iter().any(|id| id == panel_id)
                && pane.surface_kind.as_deref() == Some("browser")
            {
                closed_browser_tab(pane.browser_url.as_deref())
            } else {
                None
            }
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            closed_browser_tab_for_panel(&split.first, panel_id)
                .or_else(|| closed_browser_tab_for_panel(&split.second, panel_id))
        }
    }
}

fn closed_browser_tabs_in_layout(
    layout: &SessionWorkspaceLayoutSnapshot,
    tabs: &mut Vec<ClosedBrowserTabSnapshot>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if pane.surface_kind.as_deref() == Some("browser") {
                if let Some(tab) = closed_browser_tab(pane.browser_url.as_deref()) {
                    tabs.push(tab);
                }
            }
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            closed_browser_tabs_in_layout(&split.first, tabs);
            closed_browser_tabs_in_layout(&split.second, tabs);
        }
    }
}

fn closed_browser_tab_for_active_panel(
    snapshot: &AppSessionSnapshot,
    panel_id: &str,
) -> Option<ClosedBrowserTabSnapshot> {
    let layout = snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .get(
            snapshot
                .windows
                .first()?
                .tab_manager
                .selected_workspace_index
                .unwrap_or(0)
                .max(0) as usize,
        )?
        .layout
        .as_ref()?;
    closed_browser_tab_for_panel(layout, panel_id)
}

fn closed_browser_tabs_for_workspace(
    workspace: &SessionWorkspaceSnapshot,
) -> Vec<ClosedBrowserTabSnapshot> {
    let mut tabs = Vec::new();
    if let Some(layout) = workspace.layout.as_ref() {
        closed_browser_tabs_in_layout(layout, &mut tabs);
    }
    tabs
}

fn closed_browser_tabs_for_workspace_index(
    snapshot: &AppSessionSnapshot,
    index: i64,
) -> Vec<ClosedBrowserTabSnapshot> {
    if index < 0 {
        return Vec::new();
    }
    snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(index as usize))
        .map(closed_browser_tabs_for_workspace)
        .unwrap_or_default()
}

fn push_closed_browser_tabs(
    history: &mut Vec<ClosedBrowserTabSnapshot>,
    tabs: impl IntoIterator<Item = ClosedBrowserTabSnapshot>,
) {
    for tab in tabs {
        if history.last() == Some(&tab) {
            continue;
        }
        history.push(tab);
    }
    let overflow = history.len().saturating_sub(CLOSED_BROWSER_HISTORY_LIMIT);
    if overflow > 0 {
        history.drain(0..overflow);
    }
}

fn apply_reopen_closed_browser_tab(
    snapshot: &mut AppSessionSnapshot,
    tab: &ClosedBrowserTabSnapshot,
    new_panel_id: &str,
) -> bool {
    apply_new_workspace(snapshot, new_panel_id, None, None, None, None);
    apply_open_browser_url(snapshot, new_panel_id, Some(&tab.url))
}

/// Move the divider of the split at `path`. Returns whether the path resolved to
/// a split. Pure.
fn apply_set_divider(
    snapshot: &mut AppSessionSnapshot,
    path: &[SplitChild],
    position: f64,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::set_divider_at_path(root, path, position)
}

/// Equalize every split divider in the active workspace layout to its
/// orientation-aware span ratio. Returns whether any split was found (`false`
/// for absent/empty/single-pane layouts). Pure — mirrors `apply_set_divider`.
fn apply_equalize_dividers(snapshot: &mut AppSessionSnapshot) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::equalize_dividers(root)
}

/// Toggle split zoom for the pane holding `panel_id` in the active workspace.
fn apply_toggle_split_zoom(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::toggle_split_zoom(workspace, panel_id)
}

/// Set the active workspace's layout mode (`"canvas"` or split/default).
fn apply_set_layout_mode(snapshot: &mut AppSessionSnapshot, mode: Option<&str>) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_layout_mode(workspace, mode)
}

fn apply_set_canvas_pane_frame(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_canvas_pane_frame(workspace, panel_id, x, y, width, height)
}

fn apply_canvas_action(
    snapshot: &mut AppSessionSnapshot,
    action: &str,
    pane_gap: Option<i64>,
) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::apply_canvas_action_with_gap(workspace, action, pane_gap)
}

/// Set the surface kind of the pane holding `panel_id` (`None` = terminal).
/// Returns whether a matching pane was found. Pure.
fn apply_set_surface_kind(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    kind: Option<String>,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::set_surface_kind(root, panel_id, kind)
}

/// Select the next/previous panel hosted by the pane that holds `panel_id`.
fn apply_select_adjacent_panel(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    next: bool,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::select_adjacent_panel(root, panel_id, next)
}

/// Select a workspace by id and select `panel_id` inside that workspace's pane.
fn apply_select_workspace_surface(
    snapshot: &mut AppSessionSnapshot,
    workspace_id: &str,
    panel_id: &str,
) -> bool {
    let Some(window) = snapshot.windows.first_mut() else {
        return false;
    };
    let tab_manager = &mut window.tab_manager;
    let Some(index) = tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
    else {
        return false;
    };
    let Some(layout) = tab_manager.workspaces[index].layout.as_mut() else {
        return false;
    };
    if !session_ops::contains_panel(layout, panel_id) {
        return false;
    }
    let selected_changed = session_ops::select_panel(layout, panel_id);
    let index_i64 = index as i64;
    let workspace_changed = tab_manager.selected_workspace_index != Some(index_i64);
    tab_manager.selected_workspace_index = Some(index_i64);
    selected_changed || workspace_changed
}

/// Open `file_path` in the markdown surface of the pane holding `panel_id`.
/// This binds the pane-local markdown file path and flips the pane into the
/// `"markdown"` surface in one mutation.
fn apply_open_markdown_file(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    file_path: &str,
) -> bool {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind = session_ops::set_surface_kind(root, panel_id, Some("markdown".to_string()));
    let changed_file =
        session_ops::set_markdown_file_path(root, panel_id, Some(trimmed.to_string()));
    changed_kind || changed_file
}

/// Open `file_path` in the plain-text file surface of the pane holding
/// `panel_id`. This binds the pane-local file path and flips the pane into the
/// `"file"` surface in one mutation.
fn apply_open_file(snapshot: &mut AppSessionSnapshot, panel_id: &str, file_path: &str) -> bool {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind = session_ops::set_surface_kind(root, panel_id, Some("file".to_string()));
    let changed_file = session_ops::set_file_path(root, panel_id, Some(trimmed.to_string()));
    changed_kind || changed_file
}

/// Open a custom sidebar source in the pane holding `panel_id`. This stores the
/// sidebar source path in the existing pane-local `file_path` slot and flips the
/// pane into the Windows/Tauri custom-sidebar renderer surface.
fn apply_open_custom_sidebar(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    file_path: &str,
) -> bool {
    let trimmed = file_path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind =
        session_ops::set_surface_kind(root, panel_id, Some("custom-sidebar".to_string()));
    let changed_file = session_ops::set_file_path(root, panel_id, Some(trimmed.to_string()));
    changed_kind || changed_file
}

fn normalize_diff_request_path(request_path: &str) -> Option<String> {
    let trimmed = request_path.trim();
    let normalized = if trimmed.is_empty() {
        "/index.html".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    cmux_diff::DiffSessionRegistry::is_valid_request_path(&normalized).then_some(normalized)
}

/// Open a registered diff-viewer session in the pane holding `panel_id`.
/// Binds the pane-local token + request path and flips the pane into the
/// `"diff"` surface in one mutation.
fn apply_open_diff_viewer(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    token: &str,
    request_path: &str,
) -> bool {
    let trimmed_token = token.trim();
    let Some(normalized_request_path) = normalize_diff_request_path(request_path) else {
        return false;
    };
    if !cmux_diff::DiffSessionRegistry::is_valid_token(trimmed_token) {
        return false;
    }
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind = session_ops::set_surface_kind(root, panel_id, Some("diff".to_string()));
    let changed_diff = session_ops::set_diff_viewer_session(
        root,
        panel_id,
        Some(trimmed_token.to_string()),
        Some(normalized_request_path),
    );
    changed_kind || changed_diff
}

fn normalize_browser_url(url: Option<&str>) -> String {
    let trimmed = url.unwrap_or(DEFAULT_BROWSER_URL).trim();
    if trimmed.is_empty() {
        DEFAULT_BROWSER_URL.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_browser_zoom(zoom: f64) -> f64 {
    if zoom.is_finite() {
        zoom.clamp(0.25, 3.0)
    } else {
        1.0
    }
}

fn browser_proxy_url_for_local_port(port: u16) -> String {
    crate::remote_proxy::loopback_socks5_proxy_url(port)
}

fn remote_browser_proxy_url(remote: Option<&SessionWorkspaceRemoteSnapshot>) -> Option<String> {
    let remote = remote?;
    remote
        .proxy
        .as_ref()
        .and_then(|proxy| proxy.url.clone())
        .or_else(|| {
            remote
                .local_proxy_port
                .map(browser_proxy_url_for_local_port)
        })
}

fn set_layout_browser_proxy_url(
    layout: &mut SessionWorkspaceLayoutSnapshot,
    proxy_url: Option<&str>,
) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            let next = proxy_url.map(str::to_string);
            if pane.browser_proxy_url == next {
                return false;
            }
            pane.browser_proxy_url = next;
            true
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let first_changed = set_layout_browser_proxy_url(&mut split.first, proxy_url);
            let second_changed = set_layout_browser_proxy_url(&mut split.second, proxy_url);
            first_changed || second_changed
        }
    }
}

fn set_layout_browser_proxy_url_for_panel(
    layout: &mut SessionWorkspaceLayoutSnapshot,
    panel_id: &str,
    proxy_url: Option<&str>,
) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if !pane.panel_ids.iter().any(|candidate| candidate == panel_id) {
                return false;
            }
            let next = proxy_url.map(str::to_string);
            if pane.browser_proxy_url == next {
                return false;
            }
            pane.browser_proxy_url = next;
            true
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let first_changed =
                set_layout_browser_proxy_url_for_panel(&mut split.first, panel_id, proxy_url);
            let second_changed =
                set_layout_browser_proxy_url_for_panel(&mut split.second, panel_id, proxy_url);
            first_changed || second_changed
        }
    }
}

fn set_browser_layout_proxy_url_for_browser_panes(
    layout: &mut SessionWorkspaceLayoutSnapshot,
    proxy_url: &str,
) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if pane.surface_kind.as_deref() != Some("browser") {
                return false;
            }
            let next = Some(proxy_url.to_string());
            if pane.browser_proxy_url == next {
                return false;
            }
            pane.browser_proxy_url = next;
            true
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let first_changed =
                set_browser_layout_proxy_url_for_browser_panes(&mut split.first, proxy_url);
            let second_changed =
                set_browser_layout_proxy_url_for_browser_panes(&mut split.second, proxy_url);
            first_changed || second_changed
        }
    }
}

fn apply_workspace_browser_proxy_url(
    workspace: &mut SessionWorkspaceSnapshot,
    proxy_url: Option<&str>,
) -> bool {
    let Some(layout) = workspace.layout.as_mut() else {
        return false;
    };
    match proxy_url {
        Some(proxy_url) => set_browser_layout_proxy_url_for_browser_panes(layout, proxy_url),
        None => set_layout_browser_proxy_url(layout, None),
    }
}

fn configured_remote_snapshot(
    config: WorkspaceRemoteControlConfig,
) -> SessionWorkspaceRemoteSnapshot {
    let proxy_url = config
        .local_proxy_port
        .map(browser_proxy_url_for_local_port);
    let proxy_configured = proxy_url.is_some();
    let state = if config.auto_connect {
        "connecting"
    } else {
        "disconnected"
    };
    SessionWorkspaceRemoteSnapshot {
        enabled: true,
        state: state.to_string(),
        connected: state == "connected",
        transport: Some(config.transport),
        destination: Some(config.destination),
        port: config.port,
        local_proxy_port: config.local_proxy_port,
        persistent_daemon_slot: config.persistent_daemon_slot,
        has_ssh_options: !config.ssh_options.is_empty() || config.identity_file.is_some(),
        detail: None,
        daemon: Some(SessionWorkspaceRemoteDaemonSnapshot {
            state: if config.auto_connect {
                "bootstrapping"
            } else {
                "configured"
            }
            .to_string(),
            capabilities: vec!["proxy.stream.push".to_string()],
        }),
        proxy: Some(SessionWorkspaceRemoteProxySnapshot {
            state: if config.auto_connect {
                "connecting"
            } else if proxy_configured {
                "ready"
            } else {
                "unavailable"
            }
            .to_string(),
            host: proxy_configured.then(|| "127.0.0.1".to_string()),
            port: config.local_proxy_port,
            schemes: vec!["socks5".to_string(), "http_connect".to_string()],
            url: proxy_url,
            error_code: (!proxy_configured).then(|| "proxy_unavailable".to_string()),
        }),
        detected_ports: Vec::new(),
        forwarded_ports: Vec::new(),
        conflicted_ports: Vec::new(),
        active_terminal_sessions: Some(1),
    }
}

/// Open `url` in the browser surface of the pane holding `panel_id`, switching
/// that pane to the browser surface and binding the pane-local browser state.
fn apply_open_browser_url(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    url: Option<&str>,
) -> bool {
    let normalized = normalize_browser_url(url);
    let proxy_url = active_workspace(snapshot).and_then(|workspace| {
        remote_browser_proxy_url(workspace.remote.as_ref()).filter(|url| !url.is_empty())
    });
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    let changed_kind = session_ops::set_surface_kind(root, panel_id, Some("browser".to_string()));
    let changed_url = session_ops::navigate_browser(root, panel_id, normalized);
    let changed_zoom = session_ops::ensure_browser_page_zoom(root, panel_id, 1.0);
    let changed_proxy = proxy_url
        .as_deref()
        .is_some_and(|url| set_layout_browser_proxy_url_for_panel(root, panel_id, Some(url)));
    changed_kind || changed_url || changed_zoom || changed_proxy
}

fn apply_browser_go_back(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::browser_go_back(root, panel_id)
}

fn apply_browser_go_forward(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::browser_go_forward(root, panel_id)
}

fn apply_clear_browser_history(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::clear_browser_history(root, panel_id)
}

fn apply_toggle_browser_omnibar(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::toggle_browser_omnibar_visible(root, panel_id)
}

fn apply_toggle_browser_focus_mode(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::toggle_browser_focus_mode(root, panel_id)
}

fn apply_toggle_browser_developer_tools(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::toggle_browser_developer_tools(root, panel_id)
}

fn apply_show_browser_developer_tools(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    panel: &str,
) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::show_browser_developer_tools(root, panel_id, panel.to_string())
}

fn apply_set_browser_zoom(snapshot: &mut AppSessionSnapshot, panel_id: &str, zoom: f64) -> bool {
    let Some(slot) = active_layout_slot(snapshot) else {
        return false;
    };
    let Some(root) = slot.as_mut() else {
        return false;
    };
    session_ops::set_browser_page_zoom(root, panel_id, Some(normalize_browser_zoom(zoom)))
}

/// Append a fresh single-pane workspace to the first window and select it.
/// Delegates the tab-manager mutation to [`session_ops::new_workspace`].
fn apply_new_workspace(
    snapshot: &mut AppSessionSnapshot,
    new_panel_id: &str,
    current_directory: Option<&str>,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) {
    if let Some(window) = snapshot.windows.first_mut() {
        session_ops::new_workspace(&mut window.tab_manager, new_panel_id);
        if let Some(selected_index) = window.tab_manager.selected_workspace_index {
            if let Some(workspace) = usize::try_from(selected_index)
                .ok()
                .and_then(|index| window.tab_manager.workspaces.get_mut(index))
            {
                workspace.current_directory = current_directory
                    .filter(|path| !path.is_empty())
                    .map(str::to_owned);
                workspace.initial_terminal_command = initial_terminal_command
                    .filter(|command| !command.trim().is_empty())
                    .map(str::to_owned);
                workspace.initial_terminal_input = initial_terminal_input
                    .filter(|input| !input.is_empty())
                    .map(str::to_owned);
                workspace.initial_terminal_environment =
                    initial_terminal_environment.filter(|environment| !environment.is_empty());
            }
        }
    }
    // The fresh workspace comes out of the pure builder id-less; identity is
    // minted here, in the stateful layer (see `ensure_workspace_ids`).
    ensure_workspace_ids(snapshot);
    ensure_pane_ids(snapshot);
}

/// Move an existing panel/surface into a newly-created workspace and select it.
/// The pure session op preserves pane-local surface state and panel metadata;
/// this stateful layer only mints the destination workspace/pane identity.
fn apply_move_panel_to_new_workspace(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> bool {
    let changed = match snapshot.windows.first_mut() {
        Some(window) => session_ops::move_panel_to_new_workspace(&mut window.tab_manager, panel_id),
        None => false,
    };
    if changed {
        ensure_workspace_ids(snapshot);
        ensure_pane_ids(snapshot);
    }
    changed
}

/// Select the workspace at `index` in the first window (out-of-range is a
/// no-op). Delegates to [`session_ops::select_workspace`].
fn apply_select_workspace(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::select_workspace(&mut window.tab_manager, index),
        None => false,
    }
}

fn apply_select_workspace_by_id(snapshot: &mut AppSessionSnapshot, workspace_id: &str) -> bool {
    let Some(window) = snapshot.windows.first_mut() else {
        return false;
    };
    let Some(index) = window
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
    else {
        return false;
    };
    let index = index as i64;
    let changed = window.tab_manager.selected_workspace_index != Some(index);
    window.tab_manager.selected_workspace_index = Some(index);
    changed
}

/// Close the workspace at `index` in the first window. Mirrors canonical
/// `TabManager.closeWorkspace`: closing the only workspace is a no-op.
/// Delegates to [`session_ops::close_workspace`].
fn apply_close_workspace(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::close_workspace(&mut window.tab_manager, index),
        None => false,
    }
}

/// Close multiple workspaces identified by their ORIGINAL indices in the first
/// window. Delegates to [`session_ops::close_workspaces`], which canonicalizes
/// the request into tab order, preserves the final surviving workspace, and
/// applies canonical group-anchor dissolution semantics on each close.
fn apply_close_workspaces(snapshot: &mut AppSessionSnapshot, indices: &[i64]) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::close_workspaces(&mut window.tab_manager, indices),
        None => false,
    }
}

/// Rename the workspace at `index` in the first window — canonical
/// `Workspace.setCustomTitle` trim/clear semantics (the title is trimmed;
/// empty/whitespace-only clears `custom_title`, restoring the process-title
/// fallback in the display chain). Pure — delegates to
/// [`session_ops::rename_workspace`]. Returns whether the title actually
/// changed.
fn apply_rename_workspace(snapshot: &mut AppSessionSnapshot, index: i64, title: &str) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::rename_workspace(&mut window.tab_manager, index, title),
        None => false,
    }
}

/// Set or clear the workspace description at `index` in the first window —
/// canonical `Workspace.setCustomDescription` semantics: blank/whitespace-only
/// clears, and line endings normalize to `\n`. Pure — delegates to
/// [`session_ops::set_workspace_description`]. Returns whether the description
/// actually changed.
fn apply_set_workspace_description(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    description: &str,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => {
            session_ops::set_workspace_description(&mut window.tab_manager, index, description)
        }
        None => false,
    }
}

/// Clear the custom workspace tab color at `index` in the first window. Pure —
/// delegates to [`session_ops::reset_workspace_color`]. Returns whether a
/// color was actually cleared.
fn apply_reset_workspace_color(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::reset_workspace_color(&mut window.tab_manager, index),
        None => false,
    }
}

fn apply_set_workspace_sidebar_progress(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    value: f64,
    label: Option<&str>,
) -> bool {
    let Some(workspace) = snapshot.windows.first_mut().and_then(|window| {
        window
            .tab_manager
            .workspaces
            .get_mut(usize::try_from(index).ok()?)
    }) else {
        return false;
    };
    let next = Some(SessionWorkspaceSidebarProgressSnapshot {
        value: value.clamp(0.0, 1.0),
        label: label
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
    });
    if workspace.sidebar_progress == next {
        return false;
    }
    workspace.sidebar_progress = next;
    true
}

fn apply_clear_workspace_sidebar_progress(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    let Some(workspace) = snapshot.windows.first_mut().and_then(|window| {
        window
            .tab_manager
            .workspaces
            .get_mut(usize::try_from(index).ok()?)
    }) else {
        return false;
    };
    if workspace.sidebar_progress.is_none() {
        return false;
    }
    workspace.sidebar_progress = None;
    true
}

fn workspace_mut_by_index(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
) -> Option<&mut SessionWorkspaceSnapshot> {
    snapshot.windows.first_mut().and_then(|window| {
        window
            .tab_manager
            .workspaces
            .get_mut(usize::try_from(index).ok()?)
    })
}

fn sorted_sidebar_status_entries(entries: &mut [SessionWorkspaceSidebarStatusSnapshot]) {
    entries.sort_by(|left, right| {
        right
            .priority
            .unwrap_or(0)
            .cmp(&left.priority.unwrap_or(0))
            .then_with(|| left.key.cmp(&right.key))
    });
}

fn sorted_sidebar_metadata_entries(entries: &mut [SessionWorkspaceSidebarMetadataSnapshot]) {
    entries.sort_by(|left, right| {
        right
            .priority
            .unwrap_or(0)
            .cmp(&left.priority.unwrap_or(0))
            .then_with(|| left.key.cmp(&right.key))
    });
}

fn sorted_sidebar_metadata_blocks(entries: &mut [SessionWorkspaceSidebarMetadataBlockSnapshot]) {
    entries.sort_by(|left, right| {
        right
            .priority
            .unwrap_or(0)
            .cmp(&left.priority.unwrap_or(0))
            .then_with(|| left.key.cmp(&right.key))
    });
}

fn apply_set_workspace_sidebar_status(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
    value: &str,
    priority: Option<i64>,
    updated_at: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let value = value.trim();
    let mut entries = workspace.sidebar_status_entries.clone().unwrap_or_default();
    if value.is_empty() {
        let original_len = entries.len();
        entries.retain(|entry| entry.key != key);
        if entries.len() == original_len {
            return false;
        }
    } else if let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) {
        let next_value = value.to_owned();
        if entry.value == next_value && entry.priority == priority {
            return false;
        }
        entry.value = next_value;
        entry.priority = priority;
        entry.updated_at = updated_at;
    } else {
        entries.push(SessionWorkspaceSidebarStatusSnapshot {
            key: key.to_owned(),
            value: value.to_owned(),
            priority,
            updated_at,
        });
    }
    sorted_sidebar_status_entries(&mut entries);
    workspace.sidebar_status_entries = (!entries.is_empty()).then_some(entries);
    true
}

fn apply_set_workspace_sidebar_metadata(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
    value: &str,
    icon: Option<&str>,
    color: Option<&str>,
    url: Option<&str>,
    priority: Option<i64>,
    format: Option<&str>,
    updated_at: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let value = value.trim();
    let normalized_icon = icon.and_then(normalize_nonempty).map(str::to_string);
    let normalized_color = color.and_then(normalize_nonempty).map(str::to_string);
    let normalized_url = url.and_then(normalize_nonempty).map(str::to_string);
    let normalized_format = format
        .and_then(normalize_nonempty)
        .map(|value| value.to_ascii_lowercase());
    let mut entries = workspace
        .sidebar_metadata_entries
        .clone()
        .unwrap_or_default();
    if value.is_empty() {
        let original_len = entries.len();
        entries.retain(|entry| entry.key != key);
        if entries.len() == original_len {
            return false;
        }
    } else if let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) {
        let next_value = value.to_owned();
        if entry.value == next_value
            && entry.icon == normalized_icon
            && entry.color == normalized_color
            && entry.url == normalized_url
            && entry.priority == priority
            && entry.format == normalized_format
        {
            return false;
        }
        entry.value = next_value;
        entry.icon = normalized_icon;
        entry.color = normalized_color;
        entry.url = normalized_url;
        entry.priority = priority;
        entry.format = normalized_format;
        entry.updated_at = updated_at;
    } else {
        entries.push(SessionWorkspaceSidebarMetadataSnapshot {
            key: key.to_owned(),
            value: value.to_owned(),
            icon: normalized_icon,
            color: normalized_color,
            url: normalized_url,
            priority,
            format: normalized_format,
            updated_at,
        });
    }
    sorted_sidebar_metadata_entries(&mut entries);
    workspace.sidebar_metadata_entries = (!entries.is_empty()).then_some(entries);
    true
}

fn apply_clear_workspace_sidebar_metadata(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let mut entries = workspace
        .sidebar_metadata_entries
        .clone()
        .unwrap_or_default();
    let original_len = entries.len();
    entries.retain(|entry| entry.key != key);
    if entries.len() == original_len {
        return false;
    }
    workspace.sidebar_metadata_entries = (!entries.is_empty()).then_some(entries);
    true
}

fn apply_set_workspace_sidebar_metadata_block(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
    markdown: &str,
    priority: Option<i64>,
    updated_at: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let markdown = markdown.trim();
    let mut entries = workspace
        .sidebar_metadata_blocks
        .clone()
        .unwrap_or_default();
    if markdown.is_empty() {
        let original_len = entries.len();
        entries.retain(|entry| entry.key != key);
        if entries.len() == original_len {
            return false;
        }
    } else if let Some(entry) = entries.iter_mut().find(|entry| entry.key == key) {
        let next_markdown = markdown.to_owned();
        if entry.markdown == next_markdown && entry.priority == priority {
            return false;
        }
        entry.markdown = next_markdown;
        entry.priority = priority;
        entry.updated_at = updated_at;
    } else {
        entries.push(SessionWorkspaceSidebarMetadataBlockSnapshot {
            key: key.to_owned(),
            markdown: markdown.to_owned(),
            priority,
            updated_at,
        });
    }
    sorted_sidebar_metadata_blocks(&mut entries);
    workspace.sidebar_metadata_blocks = (!entries.is_empty()).then_some(entries);
    true
}

fn apply_clear_workspace_sidebar_metadata_block(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let mut entries = workspace
        .sidebar_metadata_blocks
        .clone()
        .unwrap_or_default();
    let original_len = entries.len();
    entries.retain(|entry| entry.key != key);
    if entries.len() == original_len {
        return false;
    }
    workspace.sidebar_metadata_blocks = (!entries.is_empty()).then_some(entries);
    true
}

fn apply_reset_workspace_sidebar_metadata(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let changed = workspace.sidebar_progress.is_some()
        || workspace.sidebar_status_entries.is_some()
        || workspace.sidebar_metadata_entries.is_some()
        || workspace.sidebar_metadata_blocks.is_some()
        || workspace.sidebar_log_entries.is_some();
    workspace.sidebar_progress = None;
    workspace.sidebar_status_entries = None;
    workspace.sidebar_metadata_entries = None;
    workspace.sidebar_metadata_blocks = None;
    workspace.sidebar_log_entries = None;
    changed
}

fn apply_clear_workspace_sidebar_status(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    key: &str,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let key = key.trim();
    if key.is_empty() {
        return false;
    }
    let mut entries = workspace.sidebar_status_entries.clone().unwrap_or_default();
    let original_len = entries.len();
    entries.retain(|entry| entry.key != key);
    if entries.len() == original_len {
        return false;
    }
    workspace.sidebar_status_entries = (!entries.is_empty()).then_some(entries);
    true
}

fn apply_append_workspace_sidebar_log(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    message: &str,
    level: &str,
    created_at: i64,
) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    let message = message.trim();
    if message.is_empty() {
        return false;
    }
    let level = level.trim();
    let level = if level.is_empty() { "info" } else { level };
    let mut entries = workspace.sidebar_log_entries.clone().unwrap_or_default();
    entries.push(SessionWorkspaceSidebarLogEntrySnapshot {
        level: level.to_owned(),
        message: message.to_owned(),
        created_at,
    });
    let overflow = entries.len().saturating_sub(SIDEBAR_LOG_ENTRY_LIMIT);
    if overflow > 0 {
        entries.drain(0..overflow);
    }
    workspace.sidebar_log_entries = Some(entries);
    true
}

fn apply_clear_workspace_sidebar_log(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    let Some(workspace) = workspace_mut_by_index(snapshot, index) else {
        return false;
    };
    if workspace.sidebar_log_entries.is_none() {
        return false;
    }
    workspace.sidebar_log_entries = None;
    true
}

/// Set or clear the custom title for `panel_id` in the active workspace. Pure —
/// delegates to [`session_ops::set_panel_title`]. Returns whether title
/// metadata actually changed.
fn apply_set_panel_title(snapshot: &mut AppSessionSnapshot, panel_id: &str, title: &str) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_panel_title(workspace, panel_id, title)
}

/// Set or clear the pinned state for `panel_id` in the active workspace. Pure —
/// delegates to [`session_ops::set_panel_pinned`]. Returns whether pin metadata
/// actually changed.
fn apply_set_panel_pinned(snapshot: &mut AppSessionSnapshot, panel_id: &str, pinned: bool) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_panel_pinned(workspace, panel_id, pinned)
}

/// Set or clear the unread state for `panel_id` in the active workspace,
/// stamping new unread markers for notification ordering. Returns whether
/// unread metadata actually changed.
fn apply_set_panel_unread(snapshot: &mut AppSessionSnapshot, panel_id: &str, unread: bool) -> bool {
    apply_set_panel_unread_at(snapshot, panel_id, unread, current_unix_timestamp_seconds())
}

fn apply_set_panel_unread_at(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    unread: bool,
    unread_at: i64,
) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_panel_unread_at(workspace, panel_id, unread, Some(unread_at))
}

fn apply_set_panel_listening_ports(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    ports: &[u16],
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_panel_listening_ports(workspace, panel_id, ports)
}

fn apply_set_panel_tty(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    tty: &str,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_panel_tty(workspace, panel_id, tty, current_unix_timestamp_seconds())
}

fn apply_set_panel_shell_activity(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    state: SessionPanelShellActivityStateSnapshot,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_panel_shell_activity(workspace, panel_id, state, current_unix_timestamp_seconds())
}

fn apply_set_workspace_agent_listening_ports(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    ports: &[u16],
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_agent_listening_ports(workspace, ports)
}

fn apply_set_workspace_agent_pid(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    key: &str,
    pid: u32,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_agent_pid(workspace, key, pid, current_unix_timestamp_seconds())
}

fn apply_clear_workspace_agent_pid(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    key: &str,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    clear_workspace_agent_pid(workspace, key)
}

fn apply_set_workspace_git_facts(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    git_branch: Option<SessionGitBranchSnapshot>,
    panel_git_branches: Vec<SessionPanelGitBranchSnapshot>,
    panel_pull_requests: Vec<SessionPanelPullRequestSnapshot>,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_git_facts(
        workspace,
        git_branch,
        panel_git_branches,
        panel_pull_requests,
    )
}

fn apply_set_workspace_panel_pull_request(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    number: i64,
    label: &str,
    url: &str,
    status: SessionPullRequestStatusSnapshot,
    branch: Option<String>,
    is_stale: bool,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_panel_pull_request(
        workspace, panel_id, number, label, url, status, branch, is_stale,
    )
}

fn apply_clear_workspace_panel_pull_request(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    clear_workspace_panel_pull_request(workspace, panel_id)
}

fn apply_restorable_agent_snapshot(
    snapshot: &mut AppSessionSnapshot,
    workspace_id: Option<&str>,
    panel_id: &str,
    restorable: SessionRestorableAgentSnapshot,
) -> bool {
    let normalized_panel_id = panel_id.trim();
    if normalized_panel_id.is_empty() {
        return false;
    }
    let normalized_workspace_id = workspace_id.and_then(normalize_nonempty);
    for window in &mut snapshot.windows {
        let workspace = window.tab_manager.workspaces.iter_mut().find(|workspace| {
            let panel_matches = workspace
                .layout
                .as_ref()
                .is_some_and(|layout| session_ops::contains_panel(layout, normalized_panel_id));
            match normalized_workspace_id {
                Some(id) => workspace.workspace_id.as_deref() == Some(id) && panel_matches,
                None => panel_matches,
            }
        });
        let Some(workspace) = workspace else {
            continue;
        };
        let mut entries = workspace
            .restorable_agent_snapshots
            .take()
            .unwrap_or_default();
        match entries
            .iter_mut()
            .find(|entry| entry.panel_id == normalized_panel_id)
        {
            Some(entry) if entry.snapshot == restorable => {
                workspace.restorable_agent_snapshots = Some(entries);
                return false;
            }
            Some(entry) => {
                entry.snapshot = restorable;
            }
            None => entries.push(SessionPanelRestorableAgentSnapshot {
                panel_id: normalized_panel_id.to_string(),
                snapshot: restorable,
            }),
        }
        workspace.restorable_agent_snapshots = Some(entries);
        return true;
    }
    false
}

pub(crate) struct StartedAgentSessionSnapshot {
    pub panel_id: String,
    pub workspace_id: Option<String>,
    pub provider_id: String,
    pub session_id: String,
    pub executable_path: String,
    pub arguments: Vec<String>,
    pub working_directory: Option<String>,
}

pub(crate) fn record_started_agent_session(
    app: &AppHandle,
    state: &SessionState,
    started: StartedAgentSessionSnapshot,
) -> bool {
    let restorable = restorable_snapshot_from_started(&started);
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_restorable_agent_snapshot(
            &mut guard,
            started.workspace_id.as_deref(),
            &started.panel_id,
            restorable,
        );
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    changed
}

fn restorable_snapshot_from_started(
    started: &StartedAgentSessionSnapshot,
) -> SessionRestorableAgentSnapshot {
    let launch_command = AgentLaunchCommandSnapshot {
        launcher: None,
        executable_path: normalize_nonempty(&started.executable_path).map(str::to_string),
        arguments: started.arguments.clone(),
        working_directory: started.working_directory.clone(),
        environment: None,
        source: Some("provider.start".to_string()),
    };
    SessionRestorableAgentSnapshot {
        kind: started.provider_id.clone(),
        session_id: started.session_id.clone(),
        working_directory: started.working_directory.clone(),
        launch_command: Some(launch_command),
        resume_command: agent_resume_command(
            &started.provider_id,
            &started.session_id,
            &started.executable_path,
        ),
        fork_command: agent_fork_command(
            &started.provider_id,
            &started.session_id,
            &started.executable_path,
        ),
    }
}

fn agent_resume_command(
    provider_id: &str,
    session_id: &str,
    executable_path: &str,
) -> Option<String> {
    let session = powershell_single_quoted(session_id.trim());
    let executable = powershell_executable(executable_path, provider_id);
    match provider_id {
        "claude" => Some(format!("{executable} --resume {session}")),
        "codex" => Some(format!("{executable} resume {session}")),
        "opencode" => Some(format!("{executable} --session {session}")),
        _ => None,
    }
}

fn agent_fork_command(
    provider_id: &str,
    session_id: &str,
    executable_path: &str,
) -> Option<String> {
    let session = powershell_single_quoted(session_id.trim());
    let executable = powershell_executable(executable_path, provider_id);
    match provider_id {
        "claude" => Some(format!("{executable} --resume {session} --fork-session")),
        "codex" => Some(format!("{executable} fork {session}")),
        "opencode" => Some(format!("{executable} --session {session} --fork")),
        _ => None,
    }
}

fn powershell_executable(executable_path: &str, fallback: &str) -> String {
    let executable = normalize_nonempty(executable_path).unwrap_or(fallback);
    if executable
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '\\' | ':'))
    {
        executable.to_string()
    } else {
        format!("& {}", powershell_single_quoted(executable))
    }
}

fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn normalize_nonempty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn current_unix_timestamp_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

/// Mark a workspace read/unread by updating its representative panel unread
/// metadata, stamping new unread markers for notification ordering.
fn apply_set_workspace_unread(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    preferred_panel_id: Option<&str>,
    unread: bool,
) -> bool {
    apply_set_workspace_unread_at(
        snapshot,
        index,
        preferred_panel_id,
        unread,
        current_unix_timestamp_seconds(),
    )
}

fn apply_set_workspace_unread_at(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    preferred_panel_id: Option<&str>,
    unread: bool,
    unread_at: i64,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::set_workspace_unread_at(
            &mut window.tab_manager,
            index,
            preferred_panel_id,
            unread,
            Some(unread_at),
        ),
        None => false,
    }
}

/// Pin/unpin the workspace at `index` in the first window — canonical
/// `WorkspaceReorderCoordinator.setPinned` + `reorderTabForPinnedState`
/// (already-at-value no-op; ungrouped tabs move to the pinned boundary;
/// grouped tabs flip the flag only). Pure — delegates to
/// [`session_ops::set_workspace_pinned`]. Returns whether the pin state
/// actually changed.
fn apply_set_workspace_pinned(snapshot: &mut AppSessionSnapshot, index: i64, pinned: bool) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::set_workspace_pinned(&mut window.tab_manager, index, pinned),
        None => false,
    }
}

/// Reorder the workspace at `index` toward `to_index` in the first window —
/// canonical `WorkspaceReorderCoordinator.reorderSidebarWorkspace`
/// (`WorkspaceReorderCoordinator.swift:243-257`) routing: a group-anchor mover
/// relocates its WHOLE group via the top-level path, every other mover takes
/// the plain clamped single move. Pure — delegates to
/// [`session_ops::reorder_workspaces`]. Returns whether the order actually
/// changed.
fn apply_reorder_workspaces(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    to_index: i64,
    uses_top_level_rows: bool,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::reorder_workspaces_with_mode(
            &mut window.tab_manager,
            index,
            to_index,
            uses_top_level_rows,
        ),
        None => false,
    }
}

/// Set the OSC/process title of the workspace owning `panel_id` (any workspace in
/// the first window, not only the active one). Pure — delegates to
/// [`session_ops::set_process_title`]. Returns whether a title actually changed.
fn apply_set_process_title(snapshot: &mut AppSessionSnapshot, panel_id: &str, title: &str) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::set_process_title(&mut window.tab_manager, panel_id, title),
        None => false,
    }
}

/// Set the collapsed flag of workspace group `group_id` in the first window.
/// Pure — delegates to [`session_ops::set_group_collapsed`]. Returns whether
/// the flag actually changed.
fn apply_set_group_collapsed(
    snapshot: &mut AppSessionSnapshot,
    group_id: &str,
    collapsed: bool,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => {
            session_ops::set_group_collapsed(&mut window.tab_manager, group_id, collapsed)
        }
        None => false,
    }
}

fn emit_session_changed(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    let _ = app.emit(SESSION_CHANGED_EVENT, snapshot);
}

fn notify_session_changed(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    persist_current_snapshot(app, snapshot);
    crate::control_socket::record_session_changed_event(app, snapshot);
    emit_session_changed(app, snapshot);
    crate::window_title::refresh_window_titles(app, snapshot);
    crate::window::emit_window_states(app);
}

pub(crate) fn open_markdown_file_in_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    file_path: &str,
) -> Option<AppSessionSnapshot> {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_open_markdown_file(&mut guard, panel_id, file_path) {
            return None;
        }
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    Some(snapshot)
}

pub(crate) fn open_file_in_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    file_path: &str,
) -> Option<AppSessionSnapshot> {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_open_file(&mut guard, panel_id, file_path) {
            return None;
        }
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    Some(snapshot)
}

pub(crate) fn open_custom_sidebar_in_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    file_path: &str,
) -> Option<AppSessionSnapshot> {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_open_custom_sidebar(&mut guard, panel_id, file_path) {
            return None;
        }
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    Some(snapshot)
}

pub(crate) fn open_diff_viewer_in_panel(
    app: &AppHandle,
    state: &SessionState,
    diff_state: &crate::diff::DiffState,
    panel_id: &str,
    token: &str,
    request_path: &str,
) -> Option<AppSessionSnapshot> {
    let normalized_request_path = normalize_diff_request_path(request_path)?;
    if !diff_state.has_registered_request(
        token.trim(),
        &normalized_request_path,
        std::time::SystemTime::now(),
    ) {
        return None;
    }
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_open_diff_viewer(&mut guard, panel_id, token, &normalized_request_path) {
            return None;
        }
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    Some(snapshot)
}

pub(crate) fn open_browser_url_in_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    url: Option<&str>,
) -> Option<AppSessionSnapshot> {
    let mut snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_open_browser_url(&mut guard, panel_id, url) {
            return None;
        }
        guard.clone()
    };
    if let Some(updated) = start_panel_browser_proxy_for_control(app, state, &snapshot, panel_id) {
        snapshot = updated;
    }
    notify_session_changed(app, &snapshot);
    Some(snapshot)
}

fn mutate_browser_for_control<F>(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    mutator: F,
) -> AppSessionSnapshot
where
    F: FnOnce(&mut AppSessionSnapshot, &str) -> bool,
{
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = mutator(&mut guard, panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn browser_go_back_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> AppSessionSnapshot {
    mutate_browser_for_control(app, state, panel_id, apply_browser_go_back)
}

pub(crate) fn browser_go_forward_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> AppSessionSnapshot {
    mutate_browser_for_control(app, state, panel_id, apply_browser_go_forward)
}

pub(crate) fn clear_browser_history_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> AppSessionSnapshot {
    mutate_browser_for_control(app, state, panel_id, apply_clear_browser_history)
}

pub(crate) fn toggle_browser_omnibar_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> AppSessionSnapshot {
    mutate_browser_for_control(app, state, panel_id, apply_toggle_browser_omnibar)
}

pub(crate) fn toggle_browser_focus_mode_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> AppSessionSnapshot {
    mutate_browser_for_control(app, state, panel_id, apply_toggle_browser_focus_mode)
}

pub(crate) fn toggle_browser_developer_tools_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> AppSessionSnapshot {
    mutate_browser_for_control(app, state, panel_id, apply_toggle_browser_developer_tools)
}

pub(crate) fn show_browser_developer_tools_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    panel: &str,
) -> AppSessionSnapshot {
    let normalized_panel = match panel {
        "console" | "react" => panel.to_string(),
        _ => "inspector".to_string(),
    };
    mutate_browser_for_control(app, state, panel_id, |snapshot, panel_id| {
        apply_show_browser_developer_tools(snapshot, panel_id, &normalized_panel)
    })
}

pub(crate) fn set_browser_zoom_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    zoom: f64,
) -> AppSessionSnapshot {
    mutate_browser_for_control(app, state, panel_id, |snapshot, panel_id| {
        apply_set_browser_zoom(snapshot, panel_id, zoom)
    })
}

/// Return the current session snapshot (structure of windows/workspaces/panes).
#[tauri::command]
pub fn session_snapshot(app: AppHandle, state: State<'_, SessionState>) -> AppSessionSnapshot {
    let snapshot = current_session_snapshot(&state);
    persist_current_snapshot(&app, &snapshot);
    crate::window_title::refresh_window_titles(&app, &snapshot);
    crate::window::emit_window_states(&app);
    snapshot
}

pub(crate) fn current_session_snapshot(state: &SessionState) -> AppSessionSnapshot {
    let mut guard = state
        .snapshot
        .lock()
        .expect("session snapshot mutex poisoned");
    ensure_workspace_ids(&mut guard);
    ensure_pane_ids(&mut guard);
    guard.clone()
}

pub(crate) fn configure_workspace_remote_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    config: WorkspaceRemoteControlConfig,
) -> Option<AppSessionSnapshot> {
    let start_config = config.clone();
    let (snapshot, workspace_id_to_stop) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let workspace = guard
            .windows
            .first_mut()?
            .tab_manager
            .workspaces
            .get_mut(workspace_index)?;
        let workspace_id_to_stop = workspace.workspace_id.clone();
        let remote = configured_remote_snapshot(config);
        let proxy_url = remote_browser_proxy_url(Some(&remote));
        workspace.remote = Some(remote);
        apply_workspace_browser_proxy_url(workspace, proxy_url.as_deref());
        (guard.clone(), workspace_id_to_stop)
    };
    if let Some(workspace_id) = workspace_id_to_stop {
        state
            .remote_configs
            .lock()
            .expect("remote config mutex poisoned")
            .insert(workspace_id.clone(), start_config.clone());
        let broker_state = app.state::<crate::remote_proxy::RemoteProxyBrokerState>();
        broker_state.stop_workspace_broker(&workspace_id);
        if start_config.auto_connect && start_config.transport == "ssh" {
            if let Some(local_proxy_port) = start_config.local_proxy_port {
                let proxy_url = browser_proxy_url_for_local_port(local_proxy_port);
                let observer = Arc::new(WorkspaceBrowserProxyObserver {
                    app: app.clone(),
                    workspace_id: workspace_id.clone(),
                    proxy_url,
                });
                let ssh_config = cmux_ssh::SshBatchConfiguration {
                    destination: start_config.destination.clone(),
                    port: start_config.port.map(i64::from),
                    identity_file: start_config.identity_file.clone(),
                    ssh_options: start_config.ssh_options.clone(),
                    persistent_daemon_slot: start_config.persistent_daemon_slot.clone(),
                };
                let start_result =
                    if let Some(remote_daemon_path) = start_config.remote_daemon_path.as_deref() {
                        broker_state.start_ssh_workspace_broker(
                            &workspace_id,
                            local_proxy_port,
                            &ssh_config,
                            remote_daemon_path,
                            Some(observer.clone()),
                        )
                    } else if let Some(remote_relay_port) = start_config.remote_daemon_relay_port {
                        schedule_relay_map_workspace_broker_retry(
                            app,
                            workspace_index,
                            workspace_id.clone(),
                            local_proxy_port,
                            ssh_config,
                            remote_relay_port,
                            observer.clone(),
                        );
                        Ok(String::new())
                    } else {
                        Ok(String::new())
                    };
                match start_result {
                    Ok(proxy_url) if !proxy_url.is_empty() => {
                        let snapshot = mark_workspace_remote_proxy_ready_for_control(
                            app,
                            state,
                            workspace_index,
                            local_proxy_port,
                            proxy_url,
                        )?;
                        let snapshot = start_existing_browser_panel_proxies_for_workspace_control(
                            app,
                            state,
                            snapshot,
                            &workspace_id,
                        );
                        notify_session_changed(app, &snapshot);
                        return Some(snapshot);
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let snapshot = mark_workspace_remote_proxy_unavailable_for_control(
                            app,
                            state,
                            workspace_index,
                            local_proxy_port,
                            error,
                        )?;
                        notify_session_changed(app, &snapshot);
                        return Some(snapshot);
                    }
                }
            }
        }
    }
    notify_session_changed(app, &snapshot);
    Some(snapshot)
}

pub(crate) fn reconnect_workspace_remote_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
) -> Result<Option<AppSessionSnapshot>, String> {
    let workspace_id = {
        let guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        guard
            .windows
            .first()
            .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
            .and_then(|workspace| workspace.workspace_id.clone())
    };
    let Some(workspace_id) = workspace_id else {
        return Ok(None);
    };
    let config = state
        .remote_configs
        .lock()
        .expect("remote config mutex poisoned")
        .get(&workspace_id)
        .cloned()
        .ok_or_else(|| "remote workspace is not configured".to_string())?;
    configure_workspace_remote_for_control(
        app,
        state,
        workspace_index,
        WorkspaceRemoteControlConfig {
            auto_connect: true,
            ..config
        },
    )
    .ok_or_else(|| "workspace not found".to_string())
    .map(Some)
}

fn schedule_relay_map_workspace_broker_retry(
    app: &AppHandle,
    workspace_index: usize,
    workspace_id: String,
    local_proxy_port: u16,
    ssh_config: cmux_ssh::SshBatchConfiguration,
    remote_relay_port: u16,
    observer: Arc<WorkspaceBrowserProxyObserver>,
) {
    let app = app.clone();
    std::thread::Builder::new()
        .name(format!("cmux-remote-relay-map-{remote_relay_port}"))
        .spawn(move || {
            let session_state = app.state::<SessionState>();
            let mut last_error = String::new();
            for attempt in 1..=30 {
                if !workspace_id_exists(&session_state, &workspace_id) {
                    return;
                }
                let broker_state = app.state::<crate::remote_proxy::RemoteProxyBrokerState>();
                match broker_state.start_ssh_workspace_broker_from_relay_map(
                    &workspace_id,
                    local_proxy_port,
                    &ssh_config,
                    remote_relay_port,
                    Some(observer.clone()),
                ) {
                    Ok(proxy_url) => {
                        if !proxy_url.is_empty() {
                            if let Some(snapshot) = mark_workspace_remote_proxy_ready_for_control(
                                &app,
                                &session_state,
                                workspace_index,
                                local_proxy_port,
                                proxy_url,
                            ) {
                                let snapshot =
                                    start_existing_browser_panel_proxies_for_workspace_control(
                                        &app,
                                        &session_state,
                                        snapshot,
                                        &workspace_id,
                                    );
                                notify_session_changed(&app, &snapshot);
                            }
                        }
                        return;
                    }
                    Err(error) => {
                        last_error = format!("bootstrap failed after retry {attempt}: {error}");
                        std::thread::sleep(std::time::Duration::from_millis(750));
                    }
                }
            }
            if workspace_id_exists(&session_state, &workspace_id) {
                if let Some(snapshot) = mark_workspace_remote_proxy_unavailable_for_control(
                    &app,
                    &session_state,
                    workspace_index,
                    local_proxy_port,
                    last_error,
                ) {
                    notify_session_changed(&app, &snapshot);
                }
            }
        })
        .ok();
}

fn workspace_id_exists(state: &SessionState, workspace_id: &str) -> bool {
    let guard = state
        .snapshot
        .lock()
        .expect("session snapshot mutex poisoned");
    guard.windows.iter().any(|window| {
        window.tab_manager.workspaces.iter().any(|workspace| {
            workspace
                .workspace_id
                .as_deref()
                .is_some_and(|id| id == workspace_id)
        })
    })
}

fn mark_workspace_remote_proxy_ready_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    local_proxy_port: u16,
    proxy_url: String,
) -> Option<AppSessionSnapshot> {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let workspace = guard
            .windows
            .first_mut()?
            .tab_manager
            .workspaces
            .get_mut(workspace_index)?;
        if let Some(remote) = workspace.remote.as_mut() {
            remote.state = "connected".to_string();
            remote.connected = true;
            remote.detail = None;
            remote.conflicted_ports.clear();
            remote.local_proxy_port = Some(local_proxy_port);
            if let Some(daemon) = remote.daemon.as_mut() {
                daemon.state = "ready".to_string();
            }
            if let Some(proxy) = remote.proxy.as_mut() {
                proxy.state = "ready".to_string();
                proxy.host = Some("127.0.0.1".to_string());
                proxy.port = Some(local_proxy_port);
                proxy.url = Some(proxy_url.clone());
                proxy.error_code = None;
            }
        }
        apply_workspace_browser_proxy_url(workspace, Some(&proxy_url));
        guard.clone()
    };
    persist_current_snapshot(app, &snapshot);
    Some(snapshot)
}

fn mark_workspace_remote_proxy_unavailable_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    local_proxy_port: u16,
    detail: String,
) -> Option<AppSessionSnapshot> {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let workspace = guard
            .windows
            .first_mut()?
            .tab_manager
            .workspaces
            .get_mut(workspace_index)?;
        if let Some(remote) = workspace.remote.as_mut() {
            remote.state = "error".to_string();
            remote.connected = false;
            remote.detail = Some(detail);
            remote.conflicted_ports = vec![local_proxy_port];
            if let Some(daemon) = remote.daemon.as_mut() {
                daemon.state = "error".to_string();
            }
            if let Some(proxy) = remote.proxy.as_mut() {
                proxy.state = "unavailable".to_string();
                proxy.error_code = Some("proxy_unavailable".to_string());
                proxy.url = None;
            }
        }
        apply_workspace_browser_proxy_url(workspace, None);
        guard.clone()
    };
    persist_current_snapshot(app, &snapshot);
    Some(snapshot)
}

struct WorkspaceBrowserProxyObserver {
    app: AppHandle,
    workspace_id: String,
    proxy_url: String,
}

impl crate::remote_proxy::ProxyTrafficObserver for WorkspaceBrowserProxyObserver {
    fn observe(&self, observation: crate::remote_proxy::ProxyTrafficObservation) {
        let session_state = self.app.state::<SessionState>();
        let snapshot = current_session_snapshot(&session_state);
        let browser_state = self.app.state::<crate::browser::BrowserWebviewState>();
        let _ = record_workspace_proxy_observation_for_browser_panel(
            &snapshot,
            browser_state.inner(),
            &self.workspace_id,
            &self.proxy_url,
            &observation,
        );
    }
}

struct PanelBrowserProxyObserver {
    app: AppHandle,
    panel_id: String,
}

impl crate::remote_proxy::ProxyTrafficObserver for PanelBrowserProxyObserver {
    fn observe(&self, observation: crate::remote_proxy::ProxyTrafficObservation) {
        let browser_state = self.app.state::<crate::browser::BrowserWebviewState>();
        let _ = record_proxy_observation_for_browser_panel(
            browser_state.inner(),
            &self.panel_id,
            &observation,
            "panel",
        );
    }
}

fn record_workspace_proxy_observation_for_browser_panel(
    snapshot: &AppSessionSnapshot,
    browser_state: &crate::browser::BrowserWebviewState,
    workspace_id: &str,
    proxy_url: &str,
    observation: &crate::remote_proxy::ProxyTrafficObservation,
) -> Result<Option<String>, String> {
    let matching_panels = browser_panels_for_workspace_proxy_url(snapshot, workspace_id, proxy_url);
    let [panel_id] = matching_panels.as_slice() else {
        return Ok(None);
    };
    record_proxy_observation_for_browser_panel(browser_state, panel_id, observation, "workspace")?;
    Ok(Some(panel_id.clone()))
}

fn record_proxy_observation_for_browser_panel(
    browser_state: &crate::browser::BrowserWebviewState,
    panel_id: &str,
    observation: &crate::remote_proxy::ProxyTrafficObservation,
    proxy_attribution: &str,
) -> Result<(), String> {
    let tunnel_protocol = proxy_tunnel_protocol_label(observation.protocol.clone());
    match crate::browser::record_proxy_http_exchange_observation_with_attribution(
        browser_state,
        panel_id,
        tunnel_protocol,
        &observation.target.host,
        observation.target.port,
        &observation.upstream_prefix,
        &observation.downstream_prefix,
        observation.started_at_ms,
        observation.completed_at_ms,
        Some(proxy_attribution),
    ) {
        Ok(()) => Ok(()),
        Err(_) => crate::browser::record_proxy_tunnel_observation_with_attribution(
            browser_state,
            panel_id,
            tunnel_protocol,
            &observation.target.host,
            observation.target.port,
            &observation.upstream_prefix,
            observation.upstream_truncated,
            &observation.downstream_prefix,
            observation.downstream_truncated,
            observation.started_at_ms,
            observation.completed_at_ms,
            Some(proxy_attribution),
        ),
    }
}

fn proxy_tunnel_protocol_label(
    protocol: crate::remote_proxy::ProxyHandshakeProtocol,
) -> &'static str {
    match protocol {
        crate::remote_proxy::ProxyHandshakeProtocol::Socks5 => "socks5",
        crate::remote_proxy::ProxyHandshakeProtocol::HttpConnect => "http-connect",
        crate::remote_proxy::ProxyHandshakeProtocol::HttpForward => "http-forward",
    }
}

fn start_panel_browser_proxy_for_control(
    app: &AppHandle,
    state: &SessionState,
    snapshot: &AppSessionSnapshot,
    panel_id: &str,
) -> Option<AppSessionSnapshot> {
    let workspace_id = workspace_id_for_panel_in_snapshot(snapshot, panel_id)?;
    let observer = Arc::new(PanelBrowserProxyObserver {
        app: app.clone(),
        panel_id: panel_id.to_string(),
    });
    let proxy_url = app
        .state::<crate::remote_proxy::RemoteProxyBrokerState>()
        .start_workspace_panel_broker(&workspace_id, panel_id, Some(observer))
        .ok()?;
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let workspace = guard
            .windows
            .iter_mut()
            .flat_map(|window| window.tab_manager.workspaces.iter_mut())
            .find(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))?;
        let layout = workspace.layout.as_mut()?;
        set_layout_browser_proxy_url_for_panel(layout, panel_id, Some(&proxy_url));
        guard.clone()
    };
    Some(snapshot)
}

pub(crate) fn start_direct_browser_proxy_for_control(
    app: &AppHandle,
    state: &SessionState,
    snapshot: &AppSessionSnapshot,
    panel_id: &str,
    target_override: Option<crate::remote_proxy::ProxyTarget>,
) -> Result<(AppSessionSnapshot, String), String> {
    let workspace_id = workspace_id_for_panel_in_snapshot(snapshot, panel_id)
        .ok_or_else(|| "browser panel is not attached to a workspace".to_string())?;
    let observer = Arc::new(PanelBrowserProxyObserver {
        app: app.clone(),
        panel_id: panel_id.to_string(),
    });
    let proxy_url = app
        .state::<crate::remote_proxy::RemoteProxyBrokerState>()
        .start_direct_panel_broker(panel_id, target_override, Some(observer))?;
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let workspace = guard
            .windows
            .iter_mut()
            .flat_map(|window| window.tab_manager.workspaces.iter_mut())
            .find(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
            .ok_or_else(|| "browser panel workspace is no longer available".to_string())?;
        let layout = workspace
            .layout
            .as_mut()
            .ok_or_else(|| "browser panel workspace has no layout".to_string())?;
        set_layout_browser_proxy_url_for_panel(layout, panel_id, Some(&proxy_url));
        guard.clone()
    };
    Ok((snapshot, proxy_url))
}

fn start_existing_browser_panel_proxies_for_workspace_control(
    app: &AppHandle,
    state: &SessionState,
    snapshot: AppSessionSnapshot,
    workspace_id: &str,
) -> AppSessionSnapshot {
    let panel_ids = browser_panels_for_workspace(&snapshot, workspace_id);
    let mut latest = snapshot;
    for panel_id in panel_ids {
        if let Some(updated) = start_panel_browser_proxy_for_control(app, state, &latest, &panel_id)
        {
            latest = updated;
        }
    }
    latest
}

fn workspace_id_for_panel_in_snapshot(
    snapshot: &AppSessionSnapshot,
    panel_id: &str,
) -> Option<String> {
    fn layout_contains_panel(layout: &SessionWorkspaceLayoutSnapshot, panel_id: &str) -> bool {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                pane.panel_ids.iter().any(|candidate| candidate == panel_id)
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                layout_contains_panel(&split.first, panel_id)
                    || layout_contains_panel(&split.second, panel_id)
            }
        }
    }

    snapshot
        .windows
        .iter()
        .flat_map(|window| window.tab_manager.workspaces.iter())
        .find(|workspace| {
            workspace
                .layout
                .as_ref()
                .is_some_and(|layout| layout_contains_panel(layout, panel_id))
        })
        .and_then(|workspace| workspace.workspace_id.clone())
}

fn browser_panels_for_workspace_proxy_url(
    snapshot: &AppSessionSnapshot,
    workspace_id: &str,
    proxy_url: &str,
) -> Vec<String> {
    let Some(workspace) = snapshot
        .windows
        .iter()
        .flat_map(|window| window.tab_manager.workspaces.iter())
        .find(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
    else {
        return Vec::new();
    };
    let Some(layout) = workspace.layout.as_ref() else {
        return Vec::new();
    };
    let mut panels = Vec::new();
    collect_browser_panels_for_proxy_url(layout, proxy_url, &mut panels);
    panels
}

fn browser_panels_for_workspace(snapshot: &AppSessionSnapshot, workspace_id: &str) -> Vec<String> {
    let Some(workspace) = snapshot
        .windows
        .iter()
        .flat_map(|window| window.tab_manager.workspaces.iter())
        .find(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
    else {
        return Vec::new();
    };
    let Some(layout) = workspace.layout.as_ref() else {
        return Vec::new();
    };
    let mut panels = Vec::new();
    collect_browser_panels(layout, &mut panels);
    panels
}

fn collect_browser_panels(layout: &SessionWorkspaceLayoutSnapshot, panels: &mut Vec<String>) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if pane.surface_kind.as_deref() != Some("browser") {
                return;
            }
            if let Some(panel_id) = pane
                .selected_panel_id
                .as_ref()
                .or_else(|| pane.panel_ids.first())
            {
                panels.push(panel_id.clone());
            }
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            collect_browser_panels(&split.first, panels);
            collect_browser_panels(&split.second, panels);
        }
    }
}

fn collect_browser_panels_for_proxy_url(
    layout: &SessionWorkspaceLayoutSnapshot,
    proxy_url: &str,
    panels: &mut Vec<String>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if pane.surface_kind.as_deref() != Some("browser") {
                return;
            }
            if pane.browser_proxy_url.as_deref() != Some(proxy_url) {
                return;
            }
            if let Some(panel_id) = pane
                .selected_panel_id
                .as_ref()
                .or_else(|| pane.panel_ids.first())
            {
                panels.push(panel_id.clone());
            }
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            collect_browser_panels_for_proxy_url(&split.first, proxy_url, panels);
            collect_browser_panels_for_proxy_url(&split.second, proxy_url, panels);
        }
    }
}

pub(crate) fn clear_workspace_remote_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
) -> Option<AppSessionSnapshot> {
    let (snapshot, workspace_id_to_stop) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let workspace = guard
            .windows
            .first_mut()?
            .tab_manager
            .workspaces
            .get_mut(workspace_index)?;
        let workspace_id_to_stop = workspace.workspace_id.clone();
        workspace.remote = Some(SessionWorkspaceRemoteSnapshot {
            enabled: false,
            state: "disconnected".to_string(),
            connected: false,
            transport: None,
            destination: None,
            port: None,
            local_proxy_port: None,
            persistent_daemon_slot: None,
            has_ssh_options: false,
            detail: None,
            daemon: Some(SessionWorkspaceRemoteDaemonSnapshot {
                state: "unavailable".to_string(),
                capabilities: Vec::new(),
            }),
            proxy: Some(SessionWorkspaceRemoteProxySnapshot {
                state: "unavailable".to_string(),
                host: None,
                port: None,
                schemes: vec!["socks5".to_string(), "http_connect".to_string()],
                url: None,
                error_code: None,
            }),
            detected_ports: Vec::new(),
            forwarded_ports: Vec::new(),
            conflicted_ports: Vec::new(),
            active_terminal_sessions: Some(0),
        });
        apply_workspace_browser_proxy_url(workspace, None);
        (guard.clone(), workspace_id_to_stop)
    };
    if let Some(workspace_id) = workspace_id_to_stop {
        state
            .remote_configs
            .lock()
            .expect("remote config mutex poisoned")
            .remove(&workspace_id);
        app.state::<crate::remote_proxy::RemoteProxyBrokerState>()
            .stop_workspace_broker(&workspace_id);
    }
    notify_session_changed(app, &snapshot);
    Some(snapshot)
}

pub(crate) fn select_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> AppSessionSnapshot {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_select_workspace(&mut guard, index);
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    snapshot
}

pub(crate) fn equalize_dividers_for_control(
    app: &AppHandle,
    state: &SessionState,
) -> AppSessionSnapshot {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_equalize_dividers(&mut guard);
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    snapshot
}

pub(crate) fn new_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    current_directory: Option<&str>,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> AppSessionSnapshot {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_new_workspace(
            &mut guard,
            &new_panel_id,
            current_directory,
            initial_terminal_command,
            initial_terminal_input,
            initial_terminal_environment,
        );
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    snapshot
}

pub(crate) fn new_browser_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    url: Option<&str>,
) -> AppSessionSnapshot {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_new_workspace(&mut guard, &new_panel_id, None, None, None, None);
        apply_open_browser_url(&mut guard, &new_panel_id, url);
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    snapshot
}

pub(crate) fn reopen_closed_browser_tab_for_control(
    app: &AppHandle,
    state: &SessionState,
) -> AppSessionSnapshot {
    let Some(tab) = ({
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        history.pop()
    }) else {
        return current_session_snapshot(state);
    };
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_reopen_closed_browser_tab(&mut guard, &tab, &new_panel_id);
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    snapshot
}

pub(crate) fn close_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> AppSessionSnapshot {
    let (closed_browser_tabs, changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let closed_browser_tabs = closed_browser_tabs_for_workspace_index(&guard, index);
        let changed = apply_close_workspace(&mut guard, index);
        (closed_browser_tabs, changed, guard.clone())
    };
    if changed && !closed_browser_tabs.is_empty() {
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        push_closed_browser_tabs(&mut history, closed_browser_tabs);
    }
    notify_session_changed(app, &snapshot);
    snapshot
}

pub(crate) fn close_workspaces_for_control(
    app: &AppHandle,
    state: &SessionState,
    indices: &[i64],
) -> AppSessionSnapshot {
    let (closed_browser_tabs, changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let candidates: Vec<_> = indices
            .iter()
            .filter_map(|index| {
                if *index < 0 {
                    return None;
                }
                let workspace = guard
                    .windows
                    .first()
                    .and_then(|window| window.tab_manager.workspaces.get(*index as usize))?;
                let tabs = closed_browser_tabs_for_workspace(workspace);
                (!tabs.is_empty()).then(|| (workspace.workspace_id.clone(), tabs))
            })
            .collect();
        let changed = apply_close_workspaces(&mut guard, indices);
        let remaining_ids: HashSet<String> = guard
            .windows
            .first()
            .into_iter()
            .flat_map(|window| window.tab_manager.workspaces.iter())
            .filter_map(|workspace| workspace.workspace_id.clone())
            .collect();
        let closed_browser_tabs = if changed {
            candidates
                .into_iter()
                .filter(|(workspace_id, _tabs)| {
                    workspace_id
                        .as_ref()
                        .is_none_or(|id| !remaining_ids.contains(id))
                })
                .flat_map(|(_workspace_id, tabs)| tabs)
                .collect()
        } else {
            Vec::new()
        };
        (closed_browser_tabs, changed, guard.clone())
    };
    if !closed_browser_tabs.is_empty() {
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        push_closed_browser_tabs(&mut history, closed_browser_tabs);
    }
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn rename_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    title: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_rename_workspace(&mut guard, index, title);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn split_panel_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    orientation: SessionSplitOrientation,
    insert_first: bool,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, String> {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_split_with_terminal_startup(
            &mut guard,
            panel_id,
            orientation,
            &new_panel_id,
            insert_first,
            initial_terminal_command,
            initial_terminal_input,
            initial_terminal_environment,
        ) {
            return Err(format!("no pane holds panel id {panel_id}"));
        }
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    Ok(snapshot)
}

pub(crate) fn new_terminal_tab_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, String> {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_new_terminal_tab(
            &mut guard,
            panel_id,
            &new_panel_id,
            initial_terminal_command,
            initial_terminal_input,
            initial_terminal_environment,
        ) {
            return Err(format!("no pane holds panel id {panel_id}"));
        }
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    Ok(snapshot)
}

pub(crate) fn split_browser_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    orientation: SessionSplitOrientation,
    insert_first: bool,
    url: Option<&str>,
) -> Result<AppSessionSnapshot, String> {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let mut snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_split(
            &mut guard,
            panel_id,
            orientation,
            &new_panel_id,
            insert_first,
        ) {
            return Err(format!("no pane holds panel id {panel_id}"));
        }
        apply_open_browser_url(&mut guard, &new_panel_id, url);
        guard.clone()
    };
    if let Some(updated) =
        start_panel_browser_proxy_for_control(app, state, &snapshot, &new_panel_id)
    {
        snapshot = updated;
    }
    notify_session_changed(app, &snapshot);
    Ok(snapshot)
}

pub(crate) fn close_panel_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> AppSessionSnapshot {
    let (closed_browser_tab, outcome, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let closed_browser_tab = closed_browser_tab_for_active_panel(&guard, panel_id);
        let outcome = apply_close(&mut guard, panel_id);
        (closed_browser_tab, outcome, guard.clone())
    };
    if outcome != CloseOutcome::NotFound {
        app.state::<crate::remote_proxy::RemoteProxyBrokerState>()
            .stop_panel_broker(panel_id);
        if let Some(tab) = closed_browser_tab {
            let mut history = state
                .closed_browser_tabs
                .lock()
                .expect("closed browser history mutex poisoned");
            push_closed_browser_tabs(&mut history, [tab]);
        }
    }
    notify_session_changed(app, &snapshot);
    snapshot
}

pub(crate) fn set_surface_kind_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    kind: Option<String>,
) -> AppSessionSnapshot {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_set_surface_kind(&mut guard, panel_id, kind);
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    snapshot
}

pub(crate) fn select_adjacent_panel_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    next: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_select_adjacent_panel(&mut guard, panel_id, next);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn toggle_split_zoom_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_toggle_split_zoom(&mut guard, panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn move_panel_to_new_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_move_panel_to_new_workspace(&mut guard, panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_description_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    description: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_description(&mut guard, index, description);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn reset_workspace_color_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_reset_workspace_color(&mut guard, index);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_sidebar_progress_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    value: f64,
    label: Option<&str>,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_sidebar_progress(&mut guard, index, value, label);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn clear_workspace_sidebar_progress_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_clear_workspace_sidebar_progress(&mut guard, index);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_sidebar_status_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
    value: &str,
    priority: Option<i64>,
) -> AppSessionSnapshot {
    let now = current_unix_timestamp_seconds();
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed_status =
            apply_set_workspace_sidebar_status(&mut guard, index, key, value, priority, now);
        let changed_metadata = apply_set_workspace_sidebar_metadata(
            &mut guard, index, key, value, None, None, None, priority, None, now,
        );
        let changed = changed_status || changed_metadata;
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn clear_workspace_sidebar_status_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed_status = apply_clear_workspace_sidebar_status(&mut guard, index, key);
        let changed_metadata = apply_clear_workspace_sidebar_metadata(&mut guard, index, key);
        let changed = changed_status || changed_metadata;
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_sidebar_metadata_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
    value: &str,
    icon: Option<&str>,
    color: Option<&str>,
    url: Option<&str>,
    priority: Option<i64>,
    format: Option<&str>,
) -> AppSessionSnapshot {
    let now = current_unix_timestamp_seconds();
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed_status =
            apply_set_workspace_sidebar_status(&mut guard, index, key, value, priority, now);
        let changed_metadata = apply_set_workspace_sidebar_metadata(
            &mut guard, index, key, value, icon, color, url, priority, format, now,
        );
        let changed = changed_status || changed_metadata;
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn clear_workspace_sidebar_metadata_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed_status = apply_clear_workspace_sidebar_status(&mut guard, index, key);
        let changed_metadata = apply_clear_workspace_sidebar_metadata(&mut guard, index, key);
        let changed = changed_status || changed_metadata;
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_sidebar_metadata_block_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
    markdown: &str,
    priority: Option<i64>,
) -> AppSessionSnapshot {
    let now = current_unix_timestamp_seconds();
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_sidebar_metadata_block(
            &mut guard, index, key, markdown, priority, now,
        );
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn clear_workspace_sidebar_metadata_block_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_clear_workspace_sidebar_metadata_block(&mut guard, index, key);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn reset_workspace_sidebar_metadata_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_reset_workspace_sidebar_metadata(&mut guard, index);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn append_workspace_sidebar_log_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    message: &str,
    level: &str,
) -> AppSessionSnapshot {
    let now = current_unix_timestamp_seconds();
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_append_workspace_sidebar_log(&mut guard, index, message, level, now);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn clear_workspace_sidebar_log_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_clear_workspace_sidebar_log(&mut guard, index);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_unread_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    preferred_panel_id: Option<&str>,
    unread: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_unread(&mut guard, index, preferred_panel_id, unread);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_pinned_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    pinned: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_pinned(&mut guard, index, pinned);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn reorder_workspaces_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    to_index: i64,
    uses_top_level_rows: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_reorder_workspaces(&mut guard, index, to_index, uses_top_level_rows);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn reorder_surface_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
    destination_index: i64,
    focus: bool,
) -> Option<AppSessionSnapshot> {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let workspace = guard
            .windows
            .first_mut()?
            .tab_manager
            .workspaces
            .get_mut(workspace_index)?;
        let changed = session_ops::reorder_surface(workspace, panel_id, destination_index, focus)?;
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    Some(snapshot)
}

pub(crate) fn move_surface_for_control(
    app: &AppHandle,
    state: &SessionState,
    source_workspace_index: usize,
    panel_id: &str,
    target_workspace_index: usize,
    target_pane_id: &str,
    destination_index: Option<i64>,
    focus: bool,
) -> Option<AppSessionSnapshot> {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let tabs = &mut guard.windows.first_mut()?.tab_manager;
        let changed = session_ops::move_surface(
            tabs,
            source_workspace_index,
            panel_id,
            target_workspace_index,
            target_pane_id,
            destination_index,
            focus,
        )?;
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    Some(snapshot)
}

pub(crate) fn reorder_workspaces_many_for_control(
    app: &AppHandle,
    state: &SessionState,
    ordered_workspace_ids: &[Uuid],
    dry_run: bool,
) -> Result<(Vec<WorkspaceReorderPlanItem>, AppSessionSnapshot), ReorderWorkspacesManyControlError>
{
    let (plan, snapshot) = if dry_run {
        let guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let mut snapshot = guard.clone();
        let window = snapshot
            .windows
            .first_mut()
            .ok_or(ReorderWorkspacesManyControlError::Unavailable)?;
        let plan = session_ops::reorder_workspaces_many(
            &mut window.tab_manager,
            ordered_workspace_ids,
            false,
        )?;
        (plan, snapshot)
    } else {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let window = guard
            .windows
            .first_mut()
            .ok_or(ReorderWorkspacesManyControlError::Unavailable)?;
        let plan = session_ops::reorder_workspaces_many(
            &mut window.tab_manager,
            ordered_workspace_ids,
            false,
        )?;
        (plan, guard.clone())
    };
    if !dry_run && plan.iter().any(|item| item.from_index != item.to_index) {
        notify_session_changed(app, &snapshot);
    }
    Ok((plan, snapshot))
}

pub(crate) enum ReorderWorkspacesManyControlError {
    Unavailable,
    Batch(WorkspaceBatchReorderError),
}

impl From<WorkspaceBatchReorderError> for ReorderWorkspacesManyControlError {
    fn from(error: WorkspaceBatchReorderError) -> Self {
        Self::Batch(error)
    }
}

pub(crate) fn set_group_collapsed_for_control(
    app: &AppHandle,
    state: &SessionState,
    group_id: &str,
    collapsed: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_group_collapsed(&mut guard, group_id, collapsed);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_panel_title_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    title: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_panel_title(&mut guard, panel_id, title);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_process_title_for_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    title: &str,
) -> AppSessionSnapshot {
    let panel_id = panel_id.trim();
    if panel_id.is_empty() {
        return current_session_snapshot(state);
    }
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_process_title(&mut guard, panel_id, title);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_panel_pinned_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    pinned: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_panel_pinned(&mut guard, panel_id, pinned);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_panel_unread_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    unread: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_panel_unread(&mut guard, panel_id, unread);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_panel_listening_ports_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
    ports: &[u16],
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_panel_listening_ports(&mut guard, workspace_index, panel_id, ports);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_panel_tty_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
    tty: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_panel_tty(&mut guard, workspace_index, panel_id, tty);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_panel_shell_activity_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
    shell_activity: SessionPanelShellActivityStateSnapshot,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed =
            apply_set_panel_shell_activity(&mut guard, workspace_index, panel_id, shell_activity);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_panel_listening_ports_for_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    ports: &[u16],
) -> AppSessionSnapshot {
    let panel_id = panel_id.trim();
    if panel_id.is_empty() {
        return current_session_snapshot(state);
    }
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let workspace_index = guard.windows.first().and_then(|window| {
            window.tab_manager.workspaces.iter().position(|workspace| {
                workspace
                    .layout
                    .as_ref()
                    .is_some_and(|layout| session_ops::contains_panel(layout, panel_id))
            })
        });
        let changed = workspace_index
            .map(|index| apply_set_panel_listening_ports(&mut guard, index, panel_id, ports))
            .unwrap_or(false);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_agent_listening_ports_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    ports: &[u16],
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_agent_listening_ports(&mut guard, workspace_index, ports);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_agent_pid_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    key: &str,
    pid: u32,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_agent_pid(&mut guard, workspace_index, key, pid);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn clear_workspace_agent_pid_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    key: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_clear_workspace_agent_pid(&mut guard, workspace_index, key);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn set_workspace_git_facts_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    git_branch: Option<SessionGitBranchSnapshot>,
    panel_git_branches: Vec<SessionPanelGitBranchSnapshot>,
    panel_pull_requests: Vec<SessionPanelPullRequestSnapshot>,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_git_facts(
            &mut guard,
            workspace_index,
            git_branch,
            panel_git_branches,
            panel_pull_requests,
        );
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn set_workspace_panel_pull_request_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
    number: i64,
    label: &str,
    url: &str,
    status: SessionPullRequestStatusSnapshot,
    branch: Option<String>,
    is_stale: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_panel_pull_request(
            &mut guard,
            workspace_index,
            panel_id,
            number,
            label,
            url,
            status,
            branch,
            is_stale,
        );
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn clear_workspace_panel_pull_request_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed =
            apply_clear_workspace_panel_pull_request(&mut guard, workspace_index, panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

pub(crate) fn restore_previous_launch_for_control(
    app: &AppHandle,
    state: &SessionState,
) -> AppSessionSnapshot {
    let Some(mut restored) =
        session_snapshot_paths(app).and_then(|(_current, previous)| load_snapshot_file(&previous))
    else {
        return current_session_snapshot(state);
    };

    ensure_workspace_ids(&mut restored);
    ensure_pane_ids(&mut restored);
    let next_panel = next_panel_counter(&restored);

    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        *guard = restored;
        state.next_panel.store(next_panel, Ordering::Relaxed);
        guard.clone()
    };
    notify_session_changed(app, &snapshot);
    snapshot
}

/// Restore the previous launch's persisted session snapshot if one exists.
/// Corrupt/missing files are a no-op: the current in-memory snapshot is
/// returned unchanged. A successful restore also reseeds the next `surface-N`
/// counter so future splits/workspaces keep unique panel ids.
#[tauri::command]
pub fn session_restore_previous_launch(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> AppSessionSnapshot {
    restore_previous_launch_for_control(&app, &state)
}

/// Set the OSC/process title of the workspace owning `panel_id`, fed by a
/// terminal surface's title changes (xterm `onTitleChange`). Emits
/// `cmux://session-changed` and returns the snapshot ONLY when the title
/// actually changed, so the high-frequency title stream never floods the event
/// bus with no-op churn. `panelId` (camelCase) maps to the `panel_id` param.
#[tauri::command]
pub fn session_set_process_title(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    title: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_process_title(&mut guard, &panel_id, &title);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Split the pane holding `panel_id` in `orientation`, allocating a fresh panel
/// id for the new pane. Emits `cmux://session-changed` and returns the snapshot.
#[tauri::command]
pub fn session_split(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    orientation: SessionSplitOrientation,
    insert_first: Option<bool>,
    initial_terminal_command: Option<String>,
    initial_terminal_input: Option<String>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, String> {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_split_with_terminal_startup(
            &mut guard,
            &panel_id,
            orientation,
            &new_panel_id,
            insert_first.unwrap_or(false),
            initial_terminal_command.as_deref(),
            initial_terminal_input.as_deref(),
            initial_terminal_environment,
        ) {
            return Err(format!("no pane holds panel id {panel_id}"));
        }
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    Ok(snapshot)
}

/// Add a new terminal tab beside `panel_id` in the same pane.
#[tauri::command]
pub fn session_new_terminal_tab(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    initial_terminal_command: Option<String>,
    initial_terminal_input: Option<String>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, String> {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_new_terminal_tab(
            &mut guard,
            &panel_id,
            &new_panel_id,
            initial_terminal_command.as_deref(),
            initial_terminal_input.as_deref(),
            initial_terminal_environment,
        ) {
            return Err(format!("no pane holds panel id {panel_id}"));
        }
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    Ok(snapshot)
}

/// Split the pane holding `panel_id`, making the new pane a browser surface.
#[tauri::command]
pub fn session_split_browser(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    orientation: SessionSplitOrientation,
    insert_first: Option<bool>,
    url: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let mut snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_split(
            &mut guard,
            &panel_id,
            orientation,
            &new_panel_id,
            insert_first.unwrap_or(false),
        ) {
            return Err(format!("no pane holds panel id {panel_id}"));
        }
        apply_open_browser_url(&mut guard, &new_panel_id, url.as_deref());
        guard.clone()
    };
    if let Some(updated) =
        start_panel_browser_proxy_for_control(&app, &state, &snapshot, &new_panel_id)
    {
        snapshot = updated;
    }
    notify_session_changed(&app, &snapshot);
    Ok(snapshot)
}

/// Close the pane/panel `panel_id`. Emits `cmux://session-changed` and returns
/// the snapshot. A no-op (still Ok) if the panel is already gone.
#[tauri::command]
pub fn session_close(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (closed_browser_tab, outcome, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let closed_browser_tab = closed_browser_tab_for_active_panel(&guard, &panel_id);
        let outcome = apply_close(&mut guard, &panel_id);
        (closed_browser_tab, outcome, guard.clone())
    };
    if outcome != CloseOutcome::NotFound {
        app.state::<crate::remote_proxy::RemoteProxyBrokerState>()
            .stop_panel_broker(&panel_id);
        if let Some(tab) = closed_browser_tab {
            let mut history = state
                .closed_browser_tabs
                .lock()
                .expect("closed browser history mutex poisoned");
            push_closed_browser_tabs(&mut history, [tab]);
        }
    }
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Set the divider ratio of the split at `path` (array of `"first"`/`"second"`).
/// Emits `cmux://session-changed` and returns the snapshot.
#[tauri::command]
pub fn session_set_divider(
    app: AppHandle,
    state: State<'_, SessionState>,
    path: Vec<SplitChild>,
    position: f64,
) -> AppSessionSnapshot {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_set_divider(&mut guard, &path, position);
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Equalize every split divider in the active workspace layout so panes share
/// space evenly by span. Emits `cmux://session-changed` and returns the
/// snapshot. A no-op (still returns the snapshot) on absent/single-pane layouts.
#[tauri::command]
pub fn session_equalize_dividers(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> AppSessionSnapshot {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_equalize_dividers(&mut guard);
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Toggle split zoom for the active pane. When zoomed, the web workspace renders
/// only that pane at full size while preserving the underlying split tree.
#[tauri::command]
pub fn session_toggle_split_zoom(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_toggle_split_zoom(&mut guard, &panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Set the active workspace layout mode. `"canvas"` enables freeform canvas
/// mode and seeds `canvas_panes`; `null`, `"split"`, or `"default"` returns to
/// the canonical split layout while preserving the canvas seed for later.
#[tauri::command]
pub fn session_set_layout_mode(
    app: AppHandle,
    state: State<'_, SessionState>,
    mode: Option<String>,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_layout_mode(&mut guard, mode.as_deref());
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

#[tauri::command]
pub fn session_set_canvas_pane_frame(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_canvas_pane_frame(&mut guard, &panel_id, x, y, width, height);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

#[tauri::command]
pub fn session_apply_canvas_action(
    app: AppHandle,
    state: State<'_, SessionState>,
    action: String,
    pane_gap: Option<i64>,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_canvas_action(&mut guard, &action, pane_gap);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Set (or clear) the surface kind of the pane holding `panelId`: `"agent"` for
/// a canonical agent session, or `null`/absent to revert to a terminal. Emits
/// `cmux://session-changed` and returns the snapshot. A no-op (still returns the
/// snapshot) if no pane holds the id.
#[tauri::command]
pub fn session_set_surface_kind(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    kind: Option<String>,
) -> AppSessionSnapshot {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_set_surface_kind(&mut guard, &panel_id, kind);
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Select the next/previous tab inside the pane holding `panelId`.
#[tauri::command]
pub fn session_select_adjacent_panel(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    next: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_select_adjacent_panel(&mut guard, &panel_id, next);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Select a workspace by id and focus a panel/tab inside it.
#[tauri::command]
pub fn session_select_workspace_surface(
    app: AppHandle,
    state: State<'_, SessionState>,
    workspace_id: String,
    panel_id: String,
) -> AppSessionSnapshot {
    let (_, snapshot) = select_workspace_surface(&app, &state, &workspace_id, &panel_id);
    snapshot
}

pub(crate) fn select_workspace_surface(
    app: &AppHandle,
    state: &State<'_, SessionState>,
    workspace_id: &str,
    panel_id: &str,
) -> (bool, AppSessionSnapshot) {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_select_workspace_surface(&mut guard, workspace_id, panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    (changed, snapshot)
}

pub(crate) fn workspace_surface_is_selected(
    snapshot: &AppSessionSnapshot,
    workspace_id: &str,
    panel_id: &str,
) -> bool {
    let Some(window) = snapshot.windows.first() else {
        return false;
    };
    let Some(index) = window.tab_manager.selected_workspace_index else {
        return false;
    };
    let Ok(index) = usize::try_from(index) else {
        return false;
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return false;
    };
    if workspace.workspace_id.as_deref() != Some(workspace_id) {
        return false;
    }
    workspace
        .layout
        .as_ref()
        .is_some_and(|layout| layout_selects_panel(layout, panel_id))
}

fn layout_selects_panel(layout: &SessionWorkspaceLayoutSnapshot, panel_id: &str) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            pane.selected_panel_id.as_deref() == Some(panel_id)
                && pane.panel_ids.iter().any(|id| id == panel_id)
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            layout_selects_panel(&split.first, panel_id)
                || layout_selects_panel(&split.second, panel_id)
        }
    }
}

pub(crate) fn workspace_id_for_panel(state: &SessionState, panel_id: &str) -> Option<String> {
    let panel_id = panel_id.trim();
    if panel_id.is_empty() {
        return None;
    }
    let guard = state
        .snapshot
        .lock()
        .expect("session snapshot mutex poisoned");
    guard.windows.iter().find_map(|window| {
        window.tab_manager.workspaces.iter().find_map(|workspace| {
            let layout = workspace.layout.as_ref()?;
            if session_ops::contains_panel(layout, panel_id) {
                workspace.workspace_id.clone()
            } else {
                None
            }
        })
    })
}

fn workspace_is_selected(snapshot: &AppSessionSnapshot, workspace_id: &str) -> bool {
    let Some(window) = snapshot.windows.first() else {
        return false;
    };
    let Some(index) = window.tab_manager.selected_workspace_index else {
        return false;
    };
    let Ok(index) = usize::try_from(index) else {
        return false;
    };
    window
        .tab_manager
        .workspaces
        .get(index)
        .is_some_and(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
}

fn select_workspace_by_id(
    app: &AppHandle,
    state: &State<'_, SessionState>,
    workspace_id: &str,
) -> (bool, AppSessionSnapshot) {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_select_workspace_by_id(&mut guard, workspace_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(app, &snapshot);
    }
    (changed, snapshot)
}

fn open_ssh_url_request(
    app: &AppHandle,
    state: &State<'_, SessionState>,
    request: &cmux_ssh::CmuxSSHURLRequest,
) -> Result<(bool, AppSessionSnapshot, String), String> {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let anchor_panel_id = active_panel_id(&guard)
            .ok_or_else(|| "no active terminal pane is available for SSH URL".to_string())?;
        if !apply_ssh_url_request(&mut guard, &anchor_panel_id, &new_panel_id, request) {
            return Err(format!("no pane holds panel id {anchor_panel_id}"));
        }
        (true, guard.clone())
    };
    notify_session_changed(app, &snapshot);
    Ok((changed, snapshot, new_panel_id))
}

fn parse_ssh_uri(uri: &str) -> Result<cmux_ssh::CmuxSSHURLRequest, String> {
    cmux_ssh::CmuxSSHURLRequest::parse(uri, &cmux_ssh::ssh_url::SUPPORTED_SCHEMES)
        .map_err(|error| format!("invalid SSH URI: {error}"))?
        .ok_or_else(|| "URI is not a supported cmux SSH route".to_string())
}

fn parse_session_navigation_uri(uri: &str) -> Result<SessionNavigationTarget, String> {
    let (scheme, rest) = uri
        .split_once("://")
        .ok_or_else(|| "navigation URI must use <scheme>://workspace/...".to_owned())?;
    let supported_schemes = crate::auth_environment::active_navigation_schemes();
    if !supported_schemes
        .iter()
        .any(|candidate| candidate == scheme)
    {
        return Err(format!(
            "navigation URI used scheme {scheme:?}, expected one of {:?}",
            supported_schemes
        ));
    }

    let path = rest
        .split_once('?')
        .map_or(rest, |(path, _query)| path)
        .trim_matches('/');
    let segments = path.split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["workspace", workspace_id] if !workspace_id.is_empty() => Ok(SessionNavigationTarget {
            workspace_id: percent_decode_path_segment(workspace_id)?,
            panel_id: None,
        }),
        ["workspace", workspace_id, "pane" | "surface", panel_id]
            if !workspace_id.is_empty() && !panel_id.is_empty() =>
        {
            Ok(SessionNavigationTarget {
                workspace_id: percent_decode_path_segment(workspace_id)?,
                panel_id: Some(percent_decode_path_segment(panel_id)?),
            })
        }
        _ => Err(
            "navigation URI must be <scheme>://workspace/{id}, /pane/{id}, or /surface/{id}"
                .to_owned(),
        ),
    }
}

fn percent_decode_path_segment(value: &str) -> Result<String, String> {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(format!(
                "navigation URI percent encoding is incomplete near {:?}",
                &value[index..]
            ));
        }
        let Some(high) = path_hex_value(bytes[index + 1]) else {
            return Err(format!(
                "navigation URI percent encoding is invalid near {:?}",
                &value[index..index + 3]
            ));
        };
        let Some(low) = path_hex_value(bytes[index + 2]) else {
            return Err(format!(
                "navigation URI percent encoding is invalid near {:?}",
                &value[index..index + 3]
            ));
        };
        decoded.push((high << 4) | low);
        index += 3;
    }
    String::from_utf8(decoded)
        .map_err(|_| "navigation URI percent encoding is not valid UTF-8".to_owned())
}

fn path_hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Open a markdown file in the pane holding `panelId`, switching that pane to
/// the markdown surface and binding the file path into the pane-local session
/// snapshot. Returns an error when no pane holds the id or the path is blank.
#[tauri::command]
pub fn session_open_markdown_file(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    file_path: String,
) -> Result<AppSessionSnapshot, String> {
    open_markdown_file_in_panel(&app, &state, &panel_id, &file_path)
        .ok_or_else(|| format!("unable to open markdown file in pane {panel_id}"))
}

/// Open a plain-text file in the pane holding `panelId`, switching that pane to
/// the file editor surface and binding the file path into the pane-local
/// session snapshot.
#[tauri::command]
pub fn session_open_file(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    file_path: String,
) -> Result<AppSessionSnapshot, String> {
    open_file_in_panel(&app, &state, &panel_id, &file_path)
        .ok_or_else(|| format!("unable to open file in pane {panel_id}"))
}

/// Open a registered diff-viewer session in the pane holding `panelId`,
/// switching that pane to the diff surface and binding the token + request path
/// into the pane-local session snapshot. Returns an error when the pane id is
/// unknown or the diff request is not currently restorable from the native
/// registry/manifest.
#[tauri::command]
pub fn session_open_diff_viewer(
    app: AppHandle,
    state: State<'_, SessionState>,
    diff_state: State<'_, crate::diff::DiffState>,
    panel_id: String,
    token: String,
    request_path: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    open_diff_viewer_in_panel(
        &app,
        &state,
        &diff_state,
        &panel_id,
        &token,
        request_path.as_deref().unwrap_or("/index.html"),
    )
    .ok_or_else(|| format!("unable to open diff viewer in pane {panel_id}"))
}

/// Open a URL in the pane holding `panelId`, switching that pane to the browser
/// surface and persisting the pane-local browser state.
#[tauri::command]
pub fn session_open_browser_url(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    url: Option<String>,
) -> Result<AppSessionSnapshot, String> {
    open_browser_url_in_panel(&app, &state, &panel_id, url.as_deref())
        .ok_or_else(|| format!("unable to open browser in pane {panel_id}"))
}

/// Navigate the pane-local browser back if it has persisted history.
#[tauri::command]
pub fn session_browser_go_back(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_browser_go_back(&mut guard, &panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Navigate the pane-local browser forward if it has persisted history.
#[tauri::command]
pub fn session_browser_go_forward(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_browser_go_forward(&mut guard, &panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Clear the pane-local browser history while preserving the current page.
#[tauri::command]
pub fn session_clear_browser_history(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_clear_browser_history(&mut guard, &panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Toggle the pane-local browser omnibar/toolbar visibility.
#[tauri::command]
pub fn session_toggle_browser_omnibar(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_toggle_browser_omnibar(&mut guard, &panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Toggle browser focus mode for the pane holding `panelId`.
#[tauri::command]
pub fn session_toggle_browser_focus_mode(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_toggle_browser_focus_mode(&mut guard, &panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Toggle the browser developer-tools drawer for the pane holding `panelId`.
#[tauri::command]
pub fn session_toggle_browser_developer_tools(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_toggle_browser_developer_tools(&mut guard, &panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Show the browser developer-tools drawer on a specific panel/lane.
#[tauri::command]
pub fn session_show_browser_developer_tools(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    panel: String,
) -> AppSessionSnapshot {
    let normalized_panel = match panel.as_str() {
        "console" | "react" => panel,
        _ => "inspector".to_string(),
    };
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_show_browser_developer_tools(&mut guard, &panel_id, &normalized_panel);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Persist the browser zoom factor for the pane holding `panelId`.
#[tauri::command]
pub fn session_set_browser_zoom(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    zoom: f64,
) -> Result<AppSessionSnapshot, String> {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        if !apply_set_browser_zoom(&mut guard, &panel_id, zoom) {
            return Err(format!("unable to set browser zoom for pane {panel_id}"));
        }
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    Ok(snapshot)
}

/// Create a new workspace (fresh single-pane terminal) and select it. Emits
/// `cmux://session-changed` and returns the snapshot.
#[tauri::command]
pub fn session_new_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    current_directory: Option<String>,
    initial_terminal_command: Option<String>,
    initial_terminal_input: Option<String>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> AppSessionSnapshot {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_new_workspace(
            &mut guard,
            &new_panel_id,
            current_directory.as_deref(),
            initial_terminal_command.as_deref(),
            initial_terminal_input.as_deref(),
            initial_terminal_environment,
        );
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Create a new browser workspace, select it, and bind its initial URL.
#[tauri::command]
pub fn session_new_browser_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    url: Option<String>,
) -> AppSessionSnapshot {
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_new_workspace(&mut guard, &new_panel_id, None, None, None, None);
        apply_open_browser_url(&mut guard, &new_panel_id, url.as_deref());
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Reopen the most recent browser pane/workspace that disappeared, restoring
/// its URL into a fresh browser workspace. This is the first host-wired slice of
/// the canonical recently-closed history; full item-menu persistence remains a
/// separate layer.
#[tauri::command]
pub fn session_reopen_closed_browser_tab(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> AppSessionSnapshot {
    let Some(tab) = ({
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        history.pop()
    }) else {
        return session_snapshot(app, state);
    };
    let new_panel_id = format!(
        "surface-{}",
        state.next_panel.fetch_add(1, Ordering::Relaxed)
    );
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_reopen_closed_browser_tab(&mut guard, &tab, &new_panel_id);
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Move `panelId` from the active workspace into a newly-created workspace.
#[tauri::command]
pub fn session_move_panel_to_new_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_move_panel_to_new_workspace(&mut guard, &panel_id);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

#[tauri::command]
pub fn session_handle_navigation_uri(
    app: AppHandle,
    state: State<'_, SessionState>,
    uri: String,
) -> Result<SessionNavigationHandleReply, String> {
    let target = parse_session_navigation_uri(&uri)?;
    let (changed, snapshot) = if let Some(panel_id) = target.panel_id.as_deref() {
        select_workspace_surface(&app, &state, &target.workspace_id, panel_id)
    } else {
        select_workspace_by_id(&app, &state, &target.workspace_id)
    };
    let handled = match target.panel_id.as_deref() {
        Some(panel_id) => {
            changed || workspace_surface_is_selected(&snapshot, &target.workspace_id, panel_id)
        }
        None => changed || workspace_is_selected(&snapshot, &target.workspace_id),
    };
    let message = if handled {
        "Navigation URI routed to the current session.".to_owned()
    } else {
        "Navigation URI target was not found in the current session.".to_owned()
    };

    Ok(SessionNavigationHandleReply {
        handled,
        changed,
        workspace_id: target.workspace_id,
        panel_id: target.panel_id,
        message,
    })
}

#[tauri::command]
pub fn session_handle_ssh_uri(
    app: AppHandle,
    state: State<'_, SessionState>,
    uri: String,
) -> Result<SessionSshUriHandleReply, String> {
    let request = parse_ssh_uri(&uri)?;
    let (changed, _snapshot, panel_id) = open_ssh_url_request(&app, &state, &request)?;
    Ok(SessionSshUriHandleReply {
        handled: true,
        changed,
        panel_id,
        destination: request.display_target(),
        no_focus: request.no_focus,
        message: "SSH URI opened in a terminal tab.".to_string(),
    })
}

/// Select the workspace at `index`. Emits `cmux://session-changed` and returns
/// the snapshot. A no-op (still returns the snapshot) if the index is invalid.
#[tauri::command]
pub fn session_select_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
) -> AppSessionSnapshot {
    let snapshot = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        apply_select_workspace(&mut guard, index);
        guard.clone()
    };
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Close the workspace at `index`. Closing the sole remaining workspace is a
/// no-op (canonical parity). Emits `cmux://session-changed` and returns the
/// snapshot.
#[tauri::command]
pub fn session_close_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
) -> AppSessionSnapshot {
    let (closed_browser_tabs, changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let closed_browser_tabs = closed_browser_tabs_for_workspace_index(&guard, index);
        let changed = apply_close_workspace(&mut guard, index);
        (closed_browser_tabs, changed, guard.clone())
    };
    if changed && !closed_browser_tabs.is_empty() {
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        push_closed_browser_tabs(&mut history, closed_browser_tabs);
    }
    notify_session_changed(&app, &snapshot);
    snapshot
}

/// Close multiple workspaces addressed by their ORIGINAL indices in the first
/// window. Canonicalized into current tab order Rust-side, so callers can pass
/// ranges like "close everything above/below" without racing index shifts.
/// Emits `cmux://session-changed` only when at least one workspace actually
/// closed, and returns the snapshot either way.
#[tauri::command]
pub fn session_close_workspaces(
    app: AppHandle,
    state: State<'_, SessionState>,
    indices: Vec<i64>,
) -> AppSessionSnapshot {
    let (closed_browser_tabs, changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let candidates: Vec<_> = indices
            .iter()
            .filter_map(|index| {
                if *index < 0 {
                    return None;
                }
                let workspace = guard
                    .windows
                    .first()
                    .and_then(|window| window.tab_manager.workspaces.get(*index as usize))?;
                let tabs = closed_browser_tabs_for_workspace(workspace);
                (!tabs.is_empty()).then(|| (workspace.workspace_id.clone(), tabs))
            })
            .collect();
        let changed = apply_close_workspaces(&mut guard, &indices);
        let remaining_ids: HashSet<String> = guard
            .windows
            .first()
            .into_iter()
            .flat_map(|window| window.tab_manager.workspaces.iter())
            .filter_map(|workspace| workspace.workspace_id.clone())
            .collect();
        let closed_browser_tabs = if changed {
            candidates
                .into_iter()
                .filter(|(workspace_id, _tabs)| {
                    workspace_id
                        .as_ref()
                        .is_none_or(|id| !remaining_ids.contains(id))
                })
                .flat_map(|(_workspace_id, tabs)| tabs)
                .collect()
        } else {
            Vec::new()
        };
        (closed_browser_tabs, changed, guard.clone())
    };
    if !closed_browser_tabs.is_empty() {
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        push_closed_browser_tabs(&mut history, closed_browser_tabs);
    }
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Set (collapse/expand) a workspace group's collapsed flag. Canonical parity:
/// the pure-data `setWorkspaceGroupCollapsed` used by socket/CLI paths —
/// never moves selection. Emits `cmux://session-changed` only when the flag
/// actually changed (unknown id / already-at-value are no-ops) and returns
/// the snapshot. `groupId` (camelCase) maps to the `group_id` param.
#[tauri::command]
pub fn session_set_group_collapsed(
    app: AppHandle,
    state: State<'_, SessionState>,
    group_id: String,
    collapsed: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_group_collapsed(&mut guard, &group_id, collapsed);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Rename the workspace at `index`. Canonical `Workspace.setCustomTitle`
/// (user-source) semantics: the raw title is trimmed here — a single mutation
/// path, the web sends the input verbatim — and an empty/whitespace-only title
/// clears `custom_title` (restores the process-title fallback); non-empty
/// stamps `custom_title_source = "user"`. Emits `cmux://session-changed` only
/// when the title actually changed (unknown index / identical title are
/// no-ops) and returns the snapshot.
#[tauri::command]
pub fn session_rename_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    title: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_rename_workspace(&mut guard, index, &title);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Set or clear the workspace description at `index`. Blank/whitespace-only
/// descriptions clear `custom_description`; non-empty descriptions preserve
/// their edge whitespace after canonical line-ending normalization. Emits
/// `cmux://session-changed` only when the description actually changed
/// (unknown index / identical description are no-ops) and returns the
/// snapshot.
#[tauri::command]
pub fn session_set_workspace_description(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    description: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_description(&mut guard, index, &description);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Clear the custom workspace tab color at `index`. Emits
/// `cmux://session-changed` only when a color was actually cleared.
#[tauri::command]
pub fn session_reset_workspace_color(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_reset_workspace_color(&mut guard, index);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Set or clear a panel/tab custom title in the active workspace. Empty or
/// whitespace-only `title` clears the custom name.
#[tauri::command]
pub fn session_set_panel_title(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    title: String,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_panel_title(&mut guard, &panel_id, &title);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Pin or unpin a panel/tab in the active workspace.
#[tauri::command]
pub fn session_set_panel_pinned(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    pinned: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_panel_pinned(&mut guard, &panel_id, pinned);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Mark a panel/tab read or unread in the active workspace.
#[tauri::command]
pub fn session_set_panel_unread(
    app: AppHandle,
    state: State<'_, SessionState>,
    panel_id: String,
    unread: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_panel_unread(&mut guard, &panel_id, unread);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Mark a workspace read or unread.
#[tauri::command]
pub fn session_set_workspace_unread(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    unread: bool,
    preferred_panel_id: Option<String>,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed =
            apply_set_workspace_unread(&mut guard, index, preferred_panel_id.as_deref(), unread);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Pin/unpin the workspace at `index`. Canonical
/// `WorkspaceReorderCoordinator.setPinned` semantics
/// (`WorkspaceReorderCoordinator.swift:467-472` + the pinned-ahead
/// normalization `reorderTabForPinnedState`, `:529-539`): pin floats the tab
/// to the end of the pinned prefix, unpin drops it to the front of the
/// unpinned segment; grouped tabs flip the flag only. Pin persists as
/// `Some(true)`, unpin as `None` (never `Some(false)`) — the port's golden
/// byte-stability decision (see `cmux-core/src/session.rs`). Emits
/// `cmux://session-changed` only when the pin state actually changed
/// (already-at-value / unknown index are no-ops) and returns the snapshot.
#[tauri::command]
pub fn session_set_workspace_pinned(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    pinned: bool,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_pinned(&mut guard, index, pinned);
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

/// Reorder the workspace at `index` toward `toIndex`. Canonical
/// `WorkspaceReorderCoordinator.reorderSidebarWorkspace`
/// (`WorkspaceReorderCoordinator.swift:243-257`) routing: a group-anchor mover
/// relocates its WHOLE group (all member rows, contiguous, anchor-first) via
/// the top-level path, any other mover takes the plain path with the
/// grouped-member section clamp and the global pin-tier clamp.
///
/// INDEX-SPACE CONTRACT: `index` identifies the mover as a position in
/// `tabs.workspaces` (matching select/close/rename/pin); `toIndex` is
/// interpreted in the row space canonical uses for that mover — a
/// `tabs.workspaces` index for non-anchors, a TOP-LEVEL row index for group
/// anchors (canonical UI feeds indices from the matching space via
/// `sidebarReorderWorkspaceIds`, Coordinator:171-183; the web drag lane does
/// the same). `usesTopLevelRows` (camelCase → `uses_top_level_rows`) enables
/// the canonical grouped-child promotion path too: a sidebar drag can lift a
/// grouped non-anchor into top-level row space before reordering it. Emits
/// `cmux://session-changed` only when the order actually changed
/// (out-of-range / clamped-back-to-place / normalization-reverted moves are
/// no-ops) and returns the snapshot. `toIndex` (camelCase) maps to the
/// `to_index` param.
#[tauri::command]
pub fn session_reorder_workspaces(
    app: AppHandle,
    state: State<'_, SessionState>,
    index: i64,
    to_index: i64,
    uses_top_level_rows: Option<bool>,
) -> AppSessionSnapshot {
    let (changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let changed = apply_reorder_workspaces(
            &mut guard,
            index,
            to_index,
            uses_top_level_rows.unwrap_or(false),
        );
        (changed, guard.clone())
    };
    if changed {
        notify_session_changed(&app, &snapshot);
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_core::session_ops::count_leaves;

    fn active_layout(snapshot: &AppSessionSnapshot) -> &SessionWorkspaceLayoutSnapshot {
        snapshot.windows[0].tab_manager.workspaces[0]
            .layout
            .as_ref()
            .expect("layout present")
    }

    fn first_panel_id(workspace: &cmux_core::session::SessionWorkspaceSnapshot) -> Option<&str> {
        match workspace.layout.as_ref()? {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                pane.panel_ids.first().map(String::as_str)
            }
            SessionWorkspaceLayoutSnapshot::Split(_) => None,
        }
    }

    fn pane_ids_in_layout(layout: &SessionWorkspaceLayoutSnapshot) -> Vec<&str> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                pane.pane_id.as_deref().into_iter().collect()
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                let mut ids = pane_ids_in_layout(&split.first);
                ids.extend(pane_ids_in_layout(&split.second));
                ids
            }
        }
    }

    #[test]
    fn initial_snapshot_is_one_window_workspace_pane() {
        let snapshot = initial_snapshot(FIRST_PANEL_ID);
        assert_eq!(snapshot.version, SESSION_SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(snapshot.windows.len(), 1);
        let tabs = &snapshot.windows[0].tab_manager;
        assert_eq!(tabs.selected_workspace_index, Some(0));
        assert_eq!(tabs.workspaces.len(), 1);
        assert_eq!(count_leaves(active_layout(&snapshot)), 1);
    }

    #[test]
    fn initial_snapshot_mints_a_workspace_id() {
        let snapshot = initial_snapshot(FIRST_PANEL_ID);
        let id = snapshot.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .as_deref()
            .expect("workspace_id minted");
        assert!(Uuid::parse_str(id).is_ok(), "not a uuid: {id}");
    }

    #[test]
    fn initial_snapshot_mints_a_pane_id() {
        let snapshot = initial_snapshot(FIRST_PANEL_ID);
        let pane_ids = pane_ids_in_layout(active_layout(&snapshot));
        assert_eq!(pane_ids.len(), 1);
        assert!(
            Uuid::parse_str(pane_ids[0]).is_ok(),
            "not a uuid: {}",
            pane_ids[0]
        );
    }

    #[test]
    fn snapshot_file_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        let snapshot = initial_snapshot(FIRST_PANEL_ID);
        write_snapshot_file(&path, &snapshot);
        let loaded = load_snapshot_file(&path).expect("snapshot reloads");
        assert_eq!(loaded, snapshot);
    }

    #[test]
    fn next_panel_counter_tracks_the_highest_surface_suffix() {
        let mut snapshot = initial_snapshot(FIRST_PANEL_ID);
        apply_new_workspace(&mut snapshot, "surface-9", None, None, None, None);
        assert_eq!(next_panel_counter(&snapshot), 10);
    }

    #[test]
    fn apply_close_workspaces_closes_requested_indices_in_original_order() {
        let mut snapshot = initial_snapshot(FIRST_PANEL_ID);
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        apply_new_workspace(&mut snapshot, "surface-3", None, None, None, None);
        apply_new_workspace(&mut snapshot, "surface-4", None, None, None, None);

        assert!(apply_close_workspaces(&mut snapshot, &[3, 1]));
        let workspaces = &snapshot.windows[0].tab_manager.workspaces;
        assert_eq!(workspaces.len(), 2);
        assert_eq!(first_panel_id(&workspaces[0]), Some("surface-1"));
        assert_eq!(first_panel_id(&workspaces[1]), Some("surface-3"));
    }

    #[test]
    fn new_workspace_mints_an_id_and_keeps_existing_ids() {
        let mut snapshot = initial_snapshot("surface-1");
        let first_id = snapshot.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .clone();
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        let tabs = &snapshot.windows[0].tab_manager;
        assert_eq!(tabs.workspaces.len(), 2);
        // Every workspace has a valid uuid id, the pre-existing one unchanged,
        // and the two ids are distinct.
        let ids: Vec<&str> = tabs
            .workspaces
            .iter()
            .map(|ws| ws.workspace_id.as_deref().expect("id minted"))
            .collect();
        assert!(ids.iter().all(|id| Uuid::parse_str(id).is_ok()));
        assert_eq!(tabs.workspaces[0].workspace_id, first_id);
        assert_ne!(ids[0], ids[1]);
    }

    #[test]
    fn apply_split_grows_the_active_layout() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));
        assert_eq!(count_leaves(active_layout(&snapshot)), 2);
    }

    #[test]
    fn apply_new_terminal_tab_adds_selected_panel_with_startup_input() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_new_terminal_tab(
            &mut snapshot,
            "surface-1",
            "surface-2",
            None,
            Some("codex fork session-1\r\n"),
            None,
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected pane");
        };
        assert_eq!(pane.panel_ids, ["surface-1", "surface-2"]);
        assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-2"));
        let startups = tab_manager(&snapshot).workspaces[0]
            .panel_terminal_startups
            .as_ref()
            .expect("panel startup");
        assert_eq!(startups.len(), 1);
        assert_eq!(startups[0].panel_id, "surface-2");
        assert_eq!(
            startups[0].initial_terminal_input.as_deref(),
            Some("codex fork session-1")
        );
    }

    #[test]
    fn ssh_terminal_command_uses_parser_approved_open_ssh_arguments() {
        let request = parse_ssh_uri(
            "cmux://ssh?host=dev.example.com&user=alice&port=2222&connect-timeout=10&host-key-policy=accept-new",
        )
        .unwrap();

        assert_eq!(
            ssh_terminal_command(&request),
            "ssh -p 2222 -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new alice@dev.example.com"
        );
    }

    #[test]
    fn apply_ssh_url_request_adds_terminal_tab_with_startup_command() {
        let request =
            parse_ssh_uri("cmux://ssh?host=dev.example.com&user=alice&port=2222").unwrap();
        let mut snapshot = initial_snapshot("surface-1");

        assert!(apply_ssh_url_request(
            &mut snapshot,
            "surface-1",
            "surface-2",
            &request,
        ));

        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected pane");
        };
        assert_eq!(pane.panel_ids, ["surface-1", "surface-2"]);
        assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-2"));
        let startups = tab_manager(&snapshot).workspaces[0]
            .panel_terminal_startups
            .as_ref()
            .expect("panel startup");
        assert_eq!(startups[0].panel_id, "surface-2");
        assert_eq!(
            startups[0].initial_terminal_command.as_deref(),
            Some("ssh -p 2222 alice@dev.example.com")
        );
    }

    #[test]
    fn apply_ssh_url_request_honors_no_focus() {
        let request = parse_ssh_uri("cmux://ssh?host=dev.example.com&no-focus").unwrap();
        let mut snapshot = initial_snapshot("surface-1");

        assert!(apply_ssh_url_request(
            &mut snapshot,
            "surface-1",
            "surface-2",
            &request,
        ));

        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected pane");
        };
        assert_eq!(pane.panel_ids, ["surface-1", "surface-2"]);
        assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-1"));
    }

    #[test]
    fn apply_split_preserves_existing_pane_id_and_mints_a_new_one() {
        let mut snapshot = initial_snapshot("surface-1");
        let before = pane_ids_in_layout(active_layout(&snapshot))
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(before.len(), 1);

        assert!(apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));

        let after = pane_ids_in_layout(active_layout(&snapshot));
        assert_eq!(after.len(), 2);
        assert!(after.iter().any(|pane_id| *pane_id == before[0].as_str()));
        assert_eq!(
            after
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            2
        );
        for pane_id in after {
            assert!(Uuid::parse_str(pane_id).is_ok(), "not a uuid: {pane_id}");
        }
    }

    #[test]
    fn apply_split_unknown_panel_is_rejected() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(!apply_split(
            &mut snapshot,
            "nope",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));
        assert_eq!(count_leaves(active_layout(&snapshot)), 1);
    }

    #[test]
    fn apply_close_collapses_back_to_a_single_pane() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );
        assert_eq!(
            apply_close(&mut snapshot, "surface-2"),
            CloseOutcome::Removed
        );
        assert_eq!(count_leaves(active_layout(&snapshot)), 1);
    }

    #[test]
    fn apply_close_emptying_the_last_pane_clears_layout() {
        let mut snapshot = initial_snapshot("surface-1");
        assert_eq!(
            apply_close(&mut snapshot, "surface-1"),
            CloseOutcome::Emptied
        );
        assert!(snapshot.windows[0].tab_manager.workspaces[0]
            .layout
            .is_none());
    }

    #[test]
    fn closed_browser_tab_for_active_panel_captures_disappearing_browser_pane() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-2",
            Some("https://example.com")
        ));

        let tab = closed_browser_tab_for_active_panel(&snapshot, "surface-2")
            .expect("closed browser tab captured");
        assert_eq!(tab.url, "https://example.com");
        assert_eq!(
            closed_browser_tab_for_active_panel(&snapshot, "surface-1"),
            None
        );
    }

    #[test]
    fn closed_browser_tab_for_active_panel_skips_about_blank() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_open_browser_url(&mut snapshot, "surface-1", None);

        assert_eq!(
            closed_browser_tab_for_active_panel(&snapshot, "surface-1"),
            None
        );
    }

    #[test]
    fn apply_reopen_closed_browser_tab_creates_selected_browser_workspace() {
        let mut snapshot = initial_snapshot("surface-1");
        let tab = ClosedBrowserTabSnapshot {
            url: "https://example.com/docs".to_string(),
        };

        assert!(apply_reopen_closed_browser_tab(
            &mut snapshot,
            &tab,
            "surface-2"
        ));

        let tabs = &snapshot.windows[0].tab_manager;
        assert_eq!(tabs.workspaces.len(), 2);
        assert_eq!(tabs.selected_workspace_index, Some(1));
        let restored = tabs.workspaces[1].layout.as_ref().expect("layout present");
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = restored else {
            panic!("expected single browser pane");
        };
        assert_eq!(pane.panel_ids, ["surface-2"]);
        assert_eq!(pane.surface_kind.as_deref(), Some("browser"));
        assert_eq!(
            pane.browser_url.as_deref(),
            Some("https://example.com/docs")
        );
    }

    #[test]
    fn apply_set_divider_updates_the_active_split() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );
        assert!(apply_set_divider(&mut snapshot, &[], 0.25));
        if let SessionWorkspaceLayoutSnapshot::Split(s) = active_layout(&snapshot) {
            assert_eq!(s.divider_position, 0.25);
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn apply_equalize_dividers_evens_out_the_active_layout() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );
        // Skew the divider, then equalize a 2-pane same-axis split back to 0.5.
        apply_set_divider(&mut snapshot, &[], 0.85);
        assert!(apply_equalize_dividers(&mut snapshot));
        if let SessionWorkspaceLayoutSnapshot::Split(s) = active_layout(&snapshot) {
            assert_eq!(s.divider_position, 0.5);
        } else {
            panic!("expected a split");
        }
        // Equalize preserves the leaf count (never adds/removes panes).
        assert_eq!(count_leaves(active_layout(&snapshot)), 2);
    }

    #[test]
    fn apply_equalize_dividers_on_a_single_pane_is_a_noop() {
        // Fresh single-pane layout → no split found → false, snapshot unchanged.
        let mut snapshot = initial_snapshot("surface-1");
        let before = snapshot.clone();
        assert!(!apply_equalize_dividers(&mut snapshot));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_equalize_dividers_on_absent_layout_is_a_noop() {
        // Emptied layout slot (None) → no-op.
        let mut snapshot = initial_snapshot("surface-1");
        assert_eq!(
            apply_close(&mut snapshot, "surface-1"),
            CloseOutcome::Emptied
        );
        assert!(!apply_equalize_dividers(&mut snapshot));
    }

    #[test]
    fn apply_toggle_split_zoom_sets_and_clears_workspace_zoom_target() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));
        assert!(apply_toggle_split_zoom(&mut snapshot, "surface-2"));
        assert_eq!(
            snapshot.windows[0].tab_manager.workspaces[0]
                .zoomed_panel_id
                .as_deref(),
            Some("surface-2")
        );
        assert!(apply_toggle_split_zoom(&mut snapshot, "surface-2"));
        assert_eq!(
            snapshot.windows[0].tab_manager.workspaces[0].zoomed_panel_id,
            None
        );
    }

    #[test]
    fn apply_set_layout_mode_enables_canvas_and_seeds_panes() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));

        assert!(apply_set_layout_mode(&mut snapshot, Some("canvas")));
        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(workspace.layout_mode.as_deref(), Some("canvas"));
        assert_eq!(workspace.canvas_panes.as_ref().map(Vec::len), Some(2));

        let seeded = workspace.canvas_panes.clone();
        assert!(!apply_set_layout_mode(&mut snapshot, Some("canvas")));
        assert_eq!(tab_manager(&snapshot).workspaces[0].canvas_panes, seeded);

        assert!(apply_set_layout_mode(&mut snapshot, None));
        assert_eq!(tab_manager(&snapshot).workspaces[0].layout_mode, None);
        assert_eq!(tab_manager(&snapshot).workspaces[0].canvas_panes, seeded);
    }

    #[test]
    fn apply_set_canvas_pane_frame_updates_active_workspace_canvas_panes() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_layout_mode(&mut snapshot, Some("canvas")));

        assert!(apply_set_canvas_pane_frame(
            &mut snapshot,
            "surface-1",
            24,
            32,
            640,
            360
        ));

        let pane = &tab_manager(&snapshot).workspaces[0]
            .canvas_panes
            .as_ref()
            .unwrap()[0];
        assert_eq!(
            (pane.x, pane.y, pane.width, pane.height),
            (24, 32, 640, 360)
        );
        assert!(!apply_set_canvas_pane_frame(
            &mut snapshot,
            "surface-1",
            24,
            32,
            640,
            360
        ));
    }

    #[test]
    fn apply_canvas_action_updates_active_workspace_canvas_panes() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));
        assert!(apply_set_layout_mode(&mut snapshot, Some("canvas")));

        assert!(apply_canvas_action(
            &mut snapshot,
            "distributeVertically",
            None
        ));
        let panes = tab_manager(&snapshot).workspaces[0]
            .canvas_panes
            .as_ref()
            .unwrap();
        assert_eq!(panes[0].y, 0);
        assert_eq!(panes[1].y, 816);
        assert!(!apply_canvas_action(&mut snapshot, "doesNotExist", None));
    }

    #[test]
    fn apply_set_surface_kind_marks_the_target_pane() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_surface_kind(
            &mut snapshot,
            "surface-1",
            Some("agent".to_string())
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.surface_kind.as_deref(), Some("agent"));
    }

    #[test]
    fn apply_select_adjacent_panel_updates_the_active_pane_selection() {
        let mut snapshot = initial_snapshot("surface-1");
        let layout = active_layout_slot(&mut snapshot)
            .expect("layout slot")
            .as_mut()
            .expect("layout");
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = layout else {
            panic!("expected a pane");
        };
        pane.panel_ids = vec![
            "surface-1".to_string(),
            "surface-2".to_string(),
            "surface-3".to_string(),
        ];
        pane.selected_panel_id = Some("surface-1".to_string());

        assert!(apply_select_adjacent_panel(
            &mut snapshot,
            "surface-1",
            true
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-2"));

        assert!(apply_select_adjacent_panel(
            &mut snapshot,
            "surface-1",
            false
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-1"));
    }

    #[test]
    fn apply_select_workspace_surface_selects_workspace_and_panel() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);
        let workspace_id = snapshot.windows[0].tab_manager.workspaces[1]
            .workspace_id
            .clone()
            .expect("workspace id");
        let layout = snapshot.windows[0].tab_manager.workspaces[1]
            .layout
            .as_mut()
            .expect("layout");
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = layout else {
            panic!("expected a pane");
        };
        pane.panel_ids = vec!["surface-2".to_string(), "surface-3".to_string()];
        pane.selected_panel_id = Some("surface-2".to_string());

        assert!(apply_select_workspace_surface(
            &mut snapshot,
            &workspace_id,
            "surface-3",
        ));
        assert_eq!(
            snapshot.windows[0].tab_manager.selected_workspace_index,
            Some(1)
        );
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = snapshot.windows[0].tab_manager.workspaces
            [1]
        .layout
        .as_ref()
        .expect("layout") else {
            panic!("expected a pane");
        };
        assert_eq!(pane.selected_panel_id.as_deref(), Some("surface-3"));

        snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);
        assert!(!apply_select_workspace_surface(
            &mut snapshot,
            &workspace_id,
            "missing",
        ));
        assert_eq!(
            snapshot.windows[0].tab_manager.selected_workspace_index,
            Some(0)
        );
    }

    #[test]
    fn apply_select_workspace_by_id_selects_existing_workspace() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        snapshot.windows[0].tab_manager.selected_workspace_index = Some(0);
        let workspace_id = snapshot.windows[0].tab_manager.workspaces[1]
            .workspace_id
            .clone()
            .expect("workspace id");

        assert!(apply_select_workspace_by_id(&mut snapshot, &workspace_id));
        assert_eq!(
            snapshot.windows[0].tab_manager.selected_workspace_index,
            Some(1)
        );
        assert!(workspace_is_selected(&snapshot, &workspace_id));
        assert!(!apply_select_workspace_by_id(&mut snapshot, &workspace_id));
        assert!(!apply_select_workspace_by_id(&mut snapshot, "missing"));
    }

    #[test]
    fn parse_session_navigation_uri_accepts_workspace_pane_and_surface_links() {
        assert_eq!(
            parse_session_navigation_uri("cmux://workspace/workspace-1").unwrap(),
            SessionNavigationTarget {
                workspace_id: "workspace-1".to_string(),
                panel_id: None,
            }
        );
        assert_eq!(
            parse_session_navigation_uri("cmux://workspace/workspace%201/surface/surface%2F1")
                .unwrap(),
            SessionNavigationTarget {
                workspace_id: "workspace 1".to_string(),
                panel_id: Some("surface/1".to_string()),
            }
        );
        assert_eq!(
            parse_session_navigation_uri("cmux://workspace/workspace-1/pane/pane-1").unwrap(),
            SessionNavigationTarget {
                workspace_id: "workspace-1".to_string(),
                panel_id: Some("pane-1".to_string()),
            }
        );
        assert_eq!(
            parse_session_navigation_uri("cmux-dev://workspace/workspace-1").unwrap(),
            SessionNavigationTarget {
                workspace_id: "workspace-1".to_string(),
                panel_id: None,
            }
        );
        assert_eq!(
            parse_session_navigation_uri("cmux-nightly://workspace/workspace-1").unwrap(),
            SessionNavigationTarget {
                workspace_id: "workspace-1".to_string(),
                panel_id: None,
            }
        );
    }

    #[test]
    fn parse_session_navigation_uri_rejects_bad_scheme_route_or_encoding() {
        assert!(parse_session_navigation_uri("other://workspace/workspace-1").is_err());
        assert!(parse_session_navigation_uri("cmux://notification?id=1").is_err());
        assert!(parse_session_navigation_uri("cmux://workspace/workspace-1/surface/%ZZ").is_err());
    }

    #[test]
    fn apply_open_markdown_file_binds_the_path_and_switches_the_surface() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_open_markdown_file(
            &mut snapshot,
            "surface-1",
            "C:/docs/readme.md"
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.surface_kind.as_deref(), Some("markdown"));
        assert_eq!(
            pane.markdown_file_path.as_deref(),
            Some("C:/docs/readme.md")
        );
    }

    #[test]
    fn apply_open_file_binds_the_path_and_switches_the_surface() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_open_file(
            &mut snapshot,
            "surface-1",
            "C:/docs/notes.txt"
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.surface_kind.as_deref(), Some("file"));
        assert_eq!(pane.file_path.as_deref(), Some("C:/docs/notes.txt"));
    }

    #[test]
    fn apply_open_diff_viewer_binds_the_token_path_and_switches_the_surface() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_open_diff_viewer(
            &mut snapshot,
            "surface-1",
            "tok-abcdef0123456789",
            "/review/index.html"
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.surface_kind.as_deref(), Some("diff"));
        assert_eq!(
            pane.diff_viewer_token.as_deref(),
            Some("tok-abcdef0123456789")
        );
        assert_eq!(
            pane.diff_viewer_request_path.as_deref(),
            Some("/review/index.html")
        );
    }

    #[test]
    fn apply_open_browser_url_binds_the_url_and_switches_the_surface() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://example.com")
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.surface_kind.as_deref(), Some("browser"));
        assert_eq!(pane.browser_url.as_deref(), Some("https://example.com"));
        assert_eq!(pane.browser_back_history, None);
        assert_eq!(pane.browser_forward_history, None);
        assert_eq!(pane.browser_omnibar_visible, None);
        assert_eq!(pane.browser_focus_mode_active, None);
        assert_eq!(pane.browser_developer_tools_visible, None);
        assert_eq!(pane.browser_developer_tools_panel, None);
        assert_eq!(pane.browser_page_zoom, Some(1.0));
    }

    #[test]
    fn apply_open_browser_url_inherits_remote_workspace_proxy_url() {
        let mut snapshot = initial_snapshot("surface-1");
        snapshot.windows[0].tab_manager.workspaces[0].remote =
            Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
                transport: "ssh".to_string(),
                destination: "dev.example.com".to_string(),
                port: Some(22),
                local_proxy_port: Some(31337),
                persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
                remote_daemon_path: None,
                remote_daemon_relay_port: None,
                identity_file: None,
                ssh_options: Vec::new(),
                auto_connect: true,
            }));

        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://example.com")
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(
            pane.browser_proxy_url.as_deref(),
            Some("socks5://127.0.0.1:31337")
        );
    }

    #[test]
    fn browser_panels_for_workspace_proxy_url_finds_matching_browser_panel() {
        let mut snapshot = initial_snapshot("surface-1");
        let workspace_id = snapshot.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .clone()
            .expect("workspace id");
        snapshot.windows[0].tab_manager.workspaces[0].remote =
            Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
                transport: "ssh".to_string(),
                destination: "dev.example.com".to_string(),
                port: Some(22),
                local_proxy_port: Some(31337),
                persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
                remote_daemon_path: None,
                remote_daemon_relay_port: None,
                identity_file: None,
                ssh_options: Vec::new(),
                auto_connect: true,
            }));
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://example.com")
        ));

        assert_eq!(
            browser_panels_for_workspace_proxy_url(
                &snapshot,
                &workspace_id,
                "socks5://127.0.0.1:31337"
            ),
            vec!["surface-1".to_string()]
        );
        assert!(browser_panels_for_workspace_proxy_url(
            &snapshot,
            &workspace_id,
            "socks5://127.0.0.1:31338"
        )
        .is_empty());
    }

    #[test]
    fn browser_panels_for_workspace_collects_existing_browser_panes() {
        let mut snapshot = initial_snapshot("surface-1");
        let workspace_id = snapshot.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .clone()
            .expect("workspace id");
        assert!(apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));
        assert!(apply_split(
            &mut snapshot,
            "surface-2",
            SessionSplitOrientation::Vertical,
            "surface-3",
            false,
        ));
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://one.example")
        ));
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-3",
            Some("https://three.example")
        ));

        assert_eq!(
            browser_panels_for_workspace(&snapshot, &workspace_id),
            vec!["surface-1".to_string(), "surface-3".to_string()]
        );
    }

    fn proxy_http_observation(
        protocol: crate::remote_proxy::ProxyHandshakeProtocol,
        path: &str,
        response_body: &str,
    ) -> crate::remote_proxy::ProxyTrafficObservation {
        crate::remote_proxy::ProxyTrafficObservation {
            protocol,
            target: crate::remote_proxy::ProxyTarget {
                host: "example.com".to_string(),
                port: 80,
            },
            upstream_prefix: format!(
                "GET {path} HTTP/1.1\r\nHost: example.com\r\nX-Test: observer\r\n\r\n"
            )
            .into_bytes(),
            upstream_truncated: false,
            downstream_prefix: format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n{response_body}"
            )
            .into_bytes(),
            downstream_truncated: false,
            started_at_ms: 10,
            completed_at_ms: 42,
        }
    }

    fn proxy_opaque_tunnel_observation(
        protocol: crate::remote_proxy::ProxyHandshakeProtocol,
    ) -> crate::remote_proxy::ProxyTrafficObservation {
        crate::remote_proxy::ProxyTrafficObservation {
            protocol,
            target: crate::remote_proxy::ProxyTarget {
                host: "secure.example".to_string(),
                port: 443,
            },
            upstream_prefix: vec![0x16, 0x03, 0x01, 0x00, 0x2a],
            upstream_truncated: false,
            downstream_prefix: vec![0x16, 0x03, 0x03, 0x00, 0x31],
            downstream_truncated: true,
            started_at_ms: 100,
            completed_at_ms: 155,
        }
    }

    #[test]
    fn panel_proxy_observer_bridge_records_panel_attributed_network_metadata() {
        let browser_state = crate::browser::BrowserWebviewState::default();
        let observation = proxy_http_observation(
            crate::remote_proxy::ProxyHandshakeProtocol::Socks5,
            "/panel",
            "panel ok",
        );

        record_proxy_observation_for_browser_panel(
            &browser_state,
            "surface-1",
            &observation,
            "panel",
        )
        .unwrap();

        let reply = crate::browser::browser_network_requests_for_control(
            &browser_state,
            "surface-1",
            crate::browser::BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert_eq!(reply.requests.len(), 1);
        let record = &reply.requests[0];
        assert_eq!(record.source, "proxy-stream-http");
        assert_eq!(record.transport, "socks5");
        assert_eq!(record.proxy_attribution.as_deref(), Some("panel"));
        assert_eq!(record.url, "http://example.com/panel");
        assert_eq!(record.method, "GET");
        assert_eq!(
            record.request_headers.get("x-test").map(String::as_str),
            Some("observer")
        );
        assert_eq!(record.response_status, Some(200));
        assert_eq!(record.response_body.as_deref(), Some("panel ok"));
        assert_eq!(record.response_body_preview_kind, "text");
        assert_eq!(record.duration_ms, Some(32));
        assert_eq!(reply.observer.proxy_attribution_mode, "panel");
    }

    #[test]
    fn panel_proxy_observer_bridge_records_opaque_tunnel_metadata() {
        let browser_state = crate::browser::BrowserWebviewState::default();
        let observation =
            proxy_opaque_tunnel_observation(crate::remote_proxy::ProxyHandshakeProtocol::Socks5);

        record_proxy_observation_for_browser_panel(
            &browser_state,
            "surface-1",
            &observation,
            "panel",
        )
        .unwrap();

        let reply = crate::browser::browser_network_requests_for_control(
            &browser_state,
            "surface-1",
            crate::browser::BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert_eq!(reply.requests.len(), 1);
        let record = &reply.requests[0];
        assert_eq!(record.source, "proxy-stream-tunnel");
        assert_eq!(record.transport, "socks5");
        assert_eq!(record.proxy_attribution.as_deref(), Some("panel"));
        assert_eq!(record.url, "https://secure.example/");
        assert_eq!(record.method, "CONNECT");
        assert_eq!(record.response_status, Some(200));
        assert_eq!(record.request_body_preview_kind, "binary");
        assert_eq!(record.response_body_preview_kind, "binary");
        assert!(record.response_body_truncated);
        assert_eq!(record.duration_ms, Some(55));
        assert_eq!(reply.observer.proxy_attribution_mode, "panel");
    }

    #[test]
    fn workspace_proxy_observer_bridge_records_only_unambiguous_browser_panel() {
        let mut snapshot = initial_snapshot("surface-1");
        let workspace_id = snapshot.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .clone()
            .expect("workspace id");
        snapshot.windows[0].tab_manager.workspaces[0].remote =
            Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
                transport: "ssh".to_string(),
                destination: "dev.example.com".to_string(),
                port: Some(22),
                local_proxy_port: Some(31337),
                persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
                remote_daemon_path: None,
                remote_daemon_relay_port: None,
                identity_file: None,
                ssh_options: Vec::new(),
                auto_connect: true,
            }));
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://example.com")
        ));

        let browser_state = crate::browser::BrowserWebviewState::default();
        let observation = proxy_http_observation(
            crate::remote_proxy::ProxyHandshakeProtocol::HttpConnect,
            "/workspace",
            "workspace ok",
        );
        let recorded_panel = record_workspace_proxy_observation_for_browser_panel(
            &snapshot,
            &browser_state,
            &workspace_id,
            "socks5://127.0.0.1:31337",
            &observation,
        )
        .unwrap();

        assert_eq!(recorded_panel.as_deref(), Some("surface-1"));
        let reply = crate::browser::browser_network_requests_for_control(
            &browser_state,
            "surface-1",
            crate::browser::BrowserNetworkRequestsQuery::default(),
        )
        .unwrap();
        assert_eq!(reply.requests.len(), 1);
        let record = &reply.requests[0];
        assert_eq!(record.transport, "http-connect");
        assert_eq!(record.proxy_attribution.as_deref(), Some("workspace"));
        assert_eq!(record.url, "http://example.com/workspace");
        assert_eq!(record.response_body.as_deref(), Some("workspace ok"));
        assert_eq!(reply.observer.proxy_attribution_mode, "workspace");
    }

    #[test]
    fn workspace_proxy_observer_bridge_skips_ambiguous_shared_proxy_panels() {
        let mut snapshot = initial_snapshot("surface-1");
        let workspace_id = snapshot.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .clone()
            .expect("workspace id");
        snapshot.windows[0].tab_manager.workspaces[0].remote =
            Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
                transport: "ssh".to_string(),
                destination: "dev.example.com".to_string(),
                port: Some(22),
                local_proxy_port: Some(31337),
                persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
                remote_daemon_path: None,
                remote_daemon_relay_port: None,
                identity_file: None,
                ssh_options: Vec::new(),
                auto_connect: true,
            }));
        assert!(apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://one.example")
        ));
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-2",
            Some("https://two.example")
        ));

        let browser_state = crate::browser::BrowserWebviewState::default();
        let observation = proxy_http_observation(
            crate::remote_proxy::ProxyHandshakeProtocol::Socks5,
            "/ambiguous",
            "ambiguous",
        );
        let recorded_panel = record_workspace_proxy_observation_for_browser_panel(
            &snapshot,
            &browser_state,
            &workspace_id,
            "socks5://127.0.0.1:31337",
            &observation,
        )
        .unwrap();

        assert_eq!(recorded_panel, None);
        for panel_id in ["surface-1", "surface-2"] {
            let reply = crate::browser::browser_network_requests_for_control(
                &browser_state,
                panel_id,
                crate::browser::BrowserNetworkRequestsQuery::default(),
            )
            .unwrap();
            assert!(reply.requests.is_empty());
        }
    }

    #[test]
    fn apply_open_browser_url_sets_remote_proxy_only_on_target_pane() {
        fn proxy_for_panel<'a>(
            layout: &'a SessionWorkspaceLayoutSnapshot,
            panel_id: &str,
        ) -> Option<&'a str> {
            match layout {
                SessionWorkspaceLayoutSnapshot::Pane(pane) => pane
                    .panel_ids
                    .iter()
                    .any(|candidate| candidate == panel_id)
                    .then(|| pane.browser_proxy_url.as_deref())
                    .flatten(),
                SessionWorkspaceLayoutSnapshot::Split(split) => {
                    proxy_for_panel(&split.first, panel_id)
                        .or_else(|| proxy_for_panel(&split.second, panel_id))
                }
            }
        }

        let mut snapshot = initial_snapshot("surface-1");
        snapshot.windows[0].tab_manager.workspaces[0].remote =
            Some(configured_remote_snapshot(WorkspaceRemoteControlConfig {
                transport: "ssh".to_string(),
                destination: "dev.example.com".to_string(),
                port: Some(22),
                local_proxy_port: Some(31337),
                persistent_daemon_slot: Some("ssh-workspace-1".to_string()),
                remote_daemon_path: None,
                remote_daemon_relay_port: None,
                identity_file: None,
                ssh_options: Vec::new(),
                auto_connect: true,
            }));
        assert!(apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));

        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-2",
            Some("https://example.com")
        ));

        let layout = active_layout(&snapshot);
        assert_eq!(proxy_for_panel(layout, "surface-1"), None);
        assert_eq!(
            proxy_for_panel(layout, "surface-2"),
            Some("socks5://127.0.0.1:31337")
        );
    }

    #[test]
    fn apply_open_browser_url_tracks_history_without_resetting_zoom() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://one.example")
        ));
        assert!(apply_set_browser_zoom(&mut snapshot, "surface-1", 1.5));
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://two.example")
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_url.as_deref(), Some("https://two.example"));
        assert_eq!(
            pane.browser_back_history.as_deref(),
            Some(["https://one.example/".to_string()].as_slice())
        );
        assert_eq!(pane.browser_forward_history, None);
        assert_eq!(pane.browser_page_zoom, Some(1.5));
    }

    #[test]
    fn apply_browser_back_and_forward_use_persisted_history() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://one.example")
        ));
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://two.example")
        ));

        assert!(apply_browser_go_back(&mut snapshot, "surface-1"));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_url.as_deref(), Some("https://one.example/"));
        assert_eq!(pane.browser_back_history, None);
        assert_eq!(
            pane.browser_forward_history.as_deref(),
            Some(["https://two.example/".to_string()].as_slice())
        );

        assert!(apply_browser_go_forward(&mut snapshot, "surface-1"));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_url.as_deref(), Some("https://two.example/"));
        assert_eq!(
            pane.browser_back_history.as_deref(),
            Some(["https://one.example/".to_string()].as_slice())
        );
        assert_eq!(pane.browser_forward_history, None);
    }

    #[test]
    fn apply_clear_browser_history_preserves_current_url() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://one.example")
        ));
        assert!(apply_open_browser_url(
            &mut snapshot,
            "surface-1",
            Some("https://two.example")
        ));
        assert!(apply_browser_go_back(&mut snapshot, "surface-1"));

        assert!(apply_clear_browser_history(&mut snapshot, "surface-1"));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_url.as_deref(), Some("https://one.example/"));
        assert_eq!(pane.browser_back_history, None);
        assert_eq!(pane.browser_forward_history, None);
        assert!(!apply_clear_browser_history(&mut snapshot, "surface-1"));
    }

    #[test]
    fn apply_toggle_browser_omnibar_flips_visible_default() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_toggle_browser_omnibar(&mut snapshot, "surface-1"));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_omnibar_visible, Some(false));

        assert!(apply_toggle_browser_omnibar(&mut snapshot, "surface-1"));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_omnibar_visible, Some(true));
    }

    #[test]
    fn apply_toggle_browser_focus_mode_flips_inactive_default() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_toggle_browser_focus_mode(&mut snapshot, "surface-1"));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_focus_mode_active, Some(true));

        assert!(apply_toggle_browser_focus_mode(&mut snapshot, "surface-1"));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_focus_mode_active, Some(false));
    }

    #[test]
    fn apply_browser_developer_tools_visibility_and_panel_persist() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_toggle_browser_developer_tools(
            &mut snapshot,
            "surface-1"
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_developer_tools_visible, Some(true));
        assert_eq!(
            pane.browser_developer_tools_panel.as_deref(),
            Some("inspector")
        );

        assert!(apply_show_browser_developer_tools(
            &mut snapshot,
            "surface-1",
            "console"
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_developer_tools_visible, Some(true));
        assert_eq!(
            pane.browser_developer_tools_panel.as_deref(),
            Some("console")
        );

        assert!(apply_toggle_browser_developer_tools(
            &mut snapshot,
            "surface-1"
        ));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_developer_tools_visible, Some(false));
        assert_eq!(
            pane.browser_developer_tools_panel.as_deref(),
            Some("console")
        );
    }

    #[test]
    fn apply_set_browser_zoom_clamps_to_supported_range() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_browser_zoom(&mut snapshot, "surface-1", 12.0));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_page_zoom, Some(3.0));
        assert!(apply_set_browser_zoom(&mut snapshot, "surface-1", f64::NAN));
        let SessionWorkspaceLayoutSnapshot::Pane(pane) = active_layout(&snapshot) else {
            panic!("expected a pane");
        };
        assert_eq!(pane.browser_page_zoom, Some(1.0));
    }

    fn tab_manager(snapshot: &AppSessionSnapshot) -> &SessionTabManagerSnapshot {
        &snapshot.windows[0].tab_manager
    }

    // The tab-manager workspace logic is unit-tested in `cmux_core::session_ops`;
    // these verify the desktop `apply_*` fns delegate to it against the first
    // window of a real `AppSessionSnapshot`.

    #[test]
    fn apply_new_workspace_appends_and_selects_it() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        let tabs = tab_manager(&snapshot);
        assert_eq!(tabs.workspaces.len(), 2);
        assert_eq!(tabs.selected_workspace_index, Some(1));
        assert_eq!(count_leaves(active_layout(&snapshot)), 1);
    }

    #[test]
    fn apply_new_workspace_mints_a_pane_id_for_the_new_workspace() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        let layout = tab_manager(&snapshot).workspaces[1]
            .layout
            .as_ref()
            .expect("layout present");
        let pane_ids = pane_ids_in_layout(layout);
        assert_eq!(pane_ids.len(), 1);
        assert!(
            Uuid::parse_str(pane_ids[0]).is_ok(),
            "not a uuid: {}",
            pane_ids[0]
        );
    }

    #[test]
    fn apply_new_workspace_carries_the_requested_directory() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(
            &mut snapshot,
            "surface-2",
            Some("C:/repo"),
            None,
            None,
            None,
        );
        let workspace = &tab_manager(&snapshot).workspaces[1];
        assert_eq!(workspace.current_directory.as_deref(), Some("C:/repo"));
    }

    #[test]
    fn apply_new_workspace_carries_initial_terminal_startup_metadata() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(
            &mut snapshot,
            "surface-2",
            Some("C:/repo"),
            Some("ssh example.com"),
            Some("echo ready\r"),
            Some(BTreeMap::from([("CMUX_FORK".to_string(), "1".to_string())])),
        );
        let workspace = &tab_manager(&snapshot).workspaces[1];
        assert_eq!(workspace.current_directory.as_deref(), Some("C:/repo"));
        assert_eq!(
            workspace.initial_terminal_command.as_deref(),
            Some("ssh example.com")
        );
        assert_eq!(
            workspace.initial_terminal_input.as_deref(),
            Some("echo ready\r")
        );
        assert_eq!(
            workspace
                .initial_terminal_environment
                .as_ref()
                .and_then(|environment| environment.get("CMUX_FORK"))
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn apply_move_panel_to_new_workspace_mints_destination_ids() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));

        assert!(apply_move_panel_to_new_workspace(
            &mut snapshot,
            "surface-2"
        ));

        let tabs = tab_manager(&snapshot);
        assert_eq!(tabs.workspaces.len(), 2);
        assert_eq!(tabs.selected_workspace_index, Some(1));
        assert!(Uuid::parse_str(tabs.workspaces[1].workspace_id.as_deref().unwrap()).is_ok());
        let pane_ids = pane_ids_in_layout(tabs.workspaces[1].layout.as_ref().unwrap());
        assert_eq!(pane_ids.len(), 1);
        assert!(Uuid::parse_str(pane_ids[0]).is_ok());
    }

    #[test]
    fn apply_close_workspace_removes_and_reclamps_selection() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        apply_new_workspace(&mut snapshot, "surface-3", None, None, None, None); // 3 workspaces, sel=2
                                                                                 // Close the first: selection (2) shifts left to 1.
        assert!(apply_close_workspace(&mut snapshot, 0));
        let tabs = tab_manager(&snapshot);
        assert_eq!(tabs.workspaces.len(), 2);
        assert_eq!(tabs.selected_workspace_index, Some(1));
    }

    #[test]
    fn apply_close_only_workspace_is_a_noop() {
        // Canonical `guard tabs.count > 1`: closing the sole workspace does
        // nothing (no replace-with-fresh).
        let mut snapshot = initial_snapshot("surface-1");
        assert!(!apply_close_workspace(&mut snapshot, 0));
        let tabs = tab_manager(&snapshot);
        assert_eq!(tabs.workspaces.len(), 1);
        assert_eq!(tabs.selected_workspace_index, Some(0));
    }

    #[test]
    fn apply_rename_workspace_sets_custom_title_and_user_source() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_rename_workspace(&mut snapshot, 0, "X"));
        let ws = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(ws.custom_title.as_deref(), Some("X"));
        assert_eq!(ws.custom_title_source.as_deref(), Some("user"));
        // Identical title again → false (drives the emit gate).
        assert!(!apply_rename_workspace(&mut snapshot, 0, "X"));
    }

    #[test]
    fn apply_rename_workspace_empty_title_clears() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_rename_workspace(&mut snapshot, 0, "X"));
        assert!(apply_rename_workspace(&mut snapshot, 0, ""));
        let ws = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(ws.custom_title, None);
        assert_eq!(ws.custom_title_source, None);
    }

    #[test]
    fn apply_rename_workspace_out_of_range_index_is_a_noop() {
        let mut snapshot = initial_snapshot("surface-1");
        let before = snapshot.clone();
        assert!(!apply_rename_workspace(&mut snapshot, 5, "nope"));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_set_workspace_description_normalizes_and_clears() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_workspace_description(
            &mut snapshot,
            0,
            "alpha\r\nbeta\rgamma"
        ));
        assert_eq!(
            tab_manager(&snapshot).workspaces[0]
                .custom_description
                .as_deref(),
            Some("alpha\nbeta\ngamma")
        );
        assert!(apply_set_workspace_description(&mut snapshot, 0, " \n\t "));
        assert_eq!(
            tab_manager(&snapshot).workspaces[0].custom_description,
            None
        );
    }

    #[test]
    fn apply_set_workspace_description_out_of_range_index_is_a_noop() {
        let mut snapshot = initial_snapshot("surface-1");
        let before = snapshot.clone();
        assert!(!apply_set_workspace_description(&mut snapshot, 5, "nope"));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_reset_workspace_color_clears_custom_color() {
        let mut snapshot = initial_snapshot("surface-1");
        snapshot.windows[0].tab_manager.workspaces[0].custom_color = Some("#C0392B".to_string());
        assert!(apply_reset_workspace_color(&mut snapshot, 0));
        assert_eq!(tab_manager(&snapshot).workspaces[0].custom_color, None);
        assert!(!apply_reset_workspace_color(&mut snapshot, 0));
    }

    #[test]
    fn apply_reset_workspace_color_out_of_range_index_is_a_noop() {
        let mut snapshot = initial_snapshot("surface-1");
        snapshot.windows[0].tab_manager.workspaces[0].custom_color = Some("#C0392B".to_string());
        let before = snapshot.clone();
        assert!(!apply_reset_workspace_color(&mut snapshot, 5));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_set_panel_title_sets_and_clears_active_workspace_panel_title() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_panel_title(
            &mut snapshot,
            "surface-1",
            " api logs "
        ));
        let titles = tab_manager(&snapshot).workspaces[0]
            .panel_titles
            .as_ref()
            .expect("title metadata");
        assert_eq!(titles[0].panel_id, "surface-1");
        assert_eq!(titles[0].custom_title.as_deref(), Some("api logs"));

        assert!(!apply_set_panel_title(
            &mut snapshot,
            "surface-1",
            "api logs"
        ));
        assert!(apply_set_panel_title(&mut snapshot, "surface-1", ""));
        assert_eq!(tab_manager(&snapshot).workspaces[0].panel_titles, None);
    }

    #[test]
    fn apply_set_panel_title_missing_panel_is_a_noop() {
        let mut snapshot = initial_snapshot("surface-1");
        let before = snapshot.clone();
        assert!(!apply_set_panel_title(&mut snapshot, "missing", "api logs"));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_set_panel_pinned_sets_and_clears_active_workspace_panel_pin() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_panel_pinned(&mut snapshot, "surface-1", true));
        let pins = tab_manager(&snapshot).workspaces[0]
            .panel_pins
            .as_ref()
            .expect("pin metadata");
        assert_eq!(pins[0].panel_id, "surface-1");
        assert!(pins[0].is_pinned);

        assert!(!apply_set_panel_pinned(&mut snapshot, "surface-1", true));
        assert!(apply_set_panel_pinned(&mut snapshot, "surface-1", false));
        assert_eq!(tab_manager(&snapshot).workspaces[0].panel_pins, None);
    }

    #[test]
    fn apply_set_panel_pinned_missing_panel_is_a_noop() {
        let mut snapshot = initial_snapshot("surface-1");
        let before = snapshot.clone();
        assert!(!apply_set_panel_pinned(&mut snapshot, "missing", true));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_set_panel_unread_sets_and_clears_active_workspace_panel_unread() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_panel_unread_at(
            &mut snapshot,
            "surface-1",
            true,
            123
        ));
        let unreads = tab_manager(&snapshot).workspaces[0]
            .panel_unreads
            .as_ref()
            .expect("unread metadata");
        assert_eq!(unreads[0].panel_id, "surface-1");
        assert!(unreads[0].is_unread);
        assert_eq!(unreads[0].unread_at, Some(123));

        assert!(!apply_set_panel_unread(&mut snapshot, "surface-1", true));
        assert!(apply_set_panel_unread(&mut snapshot, "surface-1", false));
        assert_eq!(tab_manager(&snapshot).workspaces[0].panel_unreads, None);
    }

    #[test]
    fn apply_set_panel_unread_missing_panel_is_a_noop() {
        let mut snapshot = initial_snapshot("surface-1");
        let before = snapshot.clone();
        assert!(!apply_set_panel_unread(&mut snapshot, "missing", true));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_set_panel_listening_ports_updates_panel_and_workspace_aggregate() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );

        assert!(apply_set_panel_listening_ports(
            &mut snapshot,
            0,
            "surface-2",
            &[5173, 3000, 5173],
        ));
        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(workspace.listening_ports, Some(vec![3000, 5173]));
        assert_eq!(
            workspace
                .panel_listening_ports
                .as_ref()
                .unwrap()
                .iter()
                .find(|entry| entry.panel_id == "surface-2")
                .map(|entry| entry.ports.clone()),
            Some(vec![3000, 5173])
        );
    }

    #[test]
    fn apply_set_panel_listening_ports_clears_and_recomputes_aggregate() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );
        assert!(apply_set_panel_listening_ports(
            &mut snapshot,
            0,
            "surface-1",
            &[3000],
        ));
        assert!(apply_set_panel_listening_ports(
            &mut snapshot,
            0,
            "surface-2",
            &[5173],
        ));
        assert!(apply_set_panel_listening_ports(
            &mut snapshot,
            0,
            "surface-1",
            &[],
        ));

        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(workspace.listening_ports, Some(vec![5173]));
        assert_eq!(
            workspace
                .panel_listening_ports
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| entry.panel_id.as_str())
                .collect::<Vec<_>>(),
            vec!["surface-2"]
        );
    }

    #[test]
    fn apply_set_panel_tty_upserts_and_sorts_by_panel() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );

        assert!(apply_set_panel_tty(
            &mut snapshot,
            0,
            "surface-2",
            "ttys002"
        ));
        assert!(apply_set_panel_tty(
            &mut snapshot,
            0,
            "surface-1",
            "ttys001"
        ));
        assert!(apply_set_panel_tty(
            &mut snapshot,
            0,
            "surface-2",
            "/dev/pts/7"
        ));
        assert!(!apply_set_panel_tty(&mut snapshot, 0, "missing", "ttys009"));
        assert!(!apply_set_panel_tty(&mut snapshot, 0, "surface-1", " "));

        let ttys = tab_manager(&snapshot).workspaces[0]
            .panel_ttys
            .as_ref()
            .expect("panel ttys set");
        assert_eq!(
            ttys.iter()
                .map(|entry| (entry.panel_id.as_str(), entry.tty.as_str()))
                .collect::<Vec<_>>(),
            vec![("surface-1", "ttys001"), ("surface-2", "/dev/pts/7")]
        );
    }

    #[test]
    fn apply_set_panel_shell_activity_upserts_and_sorts_by_panel() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );

        assert!(apply_set_panel_shell_activity(
            &mut snapshot,
            0,
            "surface-2",
            SessionPanelShellActivityStateSnapshot::CommandRunning,
        ));
        assert!(apply_set_panel_shell_activity(
            &mut snapshot,
            0,
            "surface-1",
            SessionPanelShellActivityStateSnapshot::PromptIdle,
        ));
        assert!(apply_set_panel_shell_activity(
            &mut snapshot,
            0,
            "surface-2",
            SessionPanelShellActivityStateSnapshot::Unknown,
        ));
        assert!(!apply_set_panel_shell_activity(
            &mut snapshot,
            0,
            "missing",
            SessionPanelShellActivityStateSnapshot::PromptIdle,
        ));

        let activity = tab_manager(&snapshot).workspaces[0]
            .panel_shell_activity
            .as_ref()
            .expect("panel shell activity set");
        assert_eq!(
            activity
                .iter()
                .map(|entry| (entry.panel_id.as_str(), entry.state.clone()))
                .collect::<Vec<_>>(),
            vec![
                (
                    "surface-1",
                    SessionPanelShellActivityStateSnapshot::PromptIdle
                ),
                ("surface-2", SessionPanelShellActivityStateSnapshot::Unknown),
            ]
        );
    }

    #[test]
    fn apply_set_workspace_agent_listening_ports_unions_with_panel_ports() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_panel_listening_ports(
            &mut snapshot,
            0,
            "surface-1",
            &[5173, 3000],
        ));
        assert!(apply_set_workspace_agent_listening_ports(
            &mut snapshot,
            0,
            &[7000, 3000, 7000],
        ));

        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(workspace.agent_listening_ports, Some(vec![3000, 7000]));
        assert_eq!(workspace.listening_ports, Some(vec![3000, 5173, 7000]));

        assert!(apply_set_workspace_agent_listening_ports(
            &mut snapshot,
            0,
            &[],
        ));
        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(workspace.agent_listening_ports, None);
        assert_eq!(workspace.listening_ports, Some(vec![3000, 5173]));
    }

    #[test]
    fn apply_set_and_clear_workspace_agent_pid_updates_ownership_facts() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_workspace_agent_pid(
            &mut snapshot,
            0,
            "codex.session-2",
            2222,
        ));
        assert!(apply_set_workspace_agent_pid(
            &mut snapshot,
            0,
            "codex.session-1",
            1111,
        ));
        assert!(apply_set_workspace_agent_pid(
            &mut snapshot,
            0,
            "codex.session-2",
            3333,
        ));

        let pids = tab_manager(&snapshot).workspaces[0]
            .agent_pids
            .as_ref()
            .expect("agent pids set");
        assert_eq!(
            pids.iter()
                .map(|entry| entry.key.as_str())
                .collect::<Vec<_>>(),
            vec!["codex.session-1", "codex.session-2"]
        );
        assert_eq!(pids[1].pid, 3333);

        assert!(apply_clear_workspace_agent_pid(
            &mut snapshot,
            0,
            "codex.session-2",
        ));
        let pids = tab_manager(&snapshot).workspaces[0]
            .agent_pids
            .as_ref()
            .expect("one pid remains");
        assert_eq!(pids.len(), 1);
        assert_eq!(pids[0].key, "codex.session-1");

        assert!(apply_clear_workspace_agent_pid(
            &mut snapshot,
            0,
            "codex.session-1",
        ));
        assert_eq!(tab_manager(&snapshot).workspaces[0].agent_pids, None);
    }

    #[test]
    fn apply_set_workspace_git_facts_sets_and_clears_badge_metadata() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );

        assert!(apply_set_workspace_git_facts(
            &mut snapshot,
            0,
            Some(SessionGitBranchSnapshot {
                branch: "feature/api".to_string(),
                is_dirty: true,
            }),
            vec![
                SessionPanelGitBranchSnapshot {
                    panel_id: "surface-2".to_string(),
                    branch: "feature/api".to_string(),
                    is_dirty: true,
                },
                SessionPanelGitBranchSnapshot {
                    panel_id: "surface-1".to_string(),
                    branch: "feature/api".to_string(),
                    is_dirty: true,
                },
            ],
            vec![SessionPanelPullRequestSnapshot {
                panel_id: "surface-2".to_string(),
                number: 42,
                label: "manaflow-ai/cmux".to_string(),
                url: "https://github.com/manaflow-ai/cmux/pull/42".to_string(),
                status: cmux_core::session::SessionPullRequestStatusSnapshot::Open,
                branch: Some("feature/api".to_string()),
                is_stale: false,
            }],
        ));

        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(
            workspace.git_branch,
            Some(SessionGitBranchSnapshot {
                branch: "feature/api".to_string(),
                is_dirty: true,
            })
        );
        assert_eq!(
            workspace
                .panel_git_branches
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| entry.panel_id.as_str())
                .collect::<Vec<_>>(),
            vec!["surface-1", "surface-2"]
        );
        assert_eq!(
            workspace.panel_pull_requests.as_ref().unwrap()[0].url,
            "https://github.com/manaflow-ai/cmux/pull/42"
        );

        assert!(apply_set_workspace_git_facts(
            &mut snapshot,
            0,
            None,
            Vec::new(),
            Vec::new(),
        ));
        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(workspace.git_branch, None);
        assert_eq!(workspace.panel_git_branches, None);
        assert_eq!(workspace.panel_pull_requests, None);
    }

    #[test]
    fn apply_workspace_panel_pull_request_upserts_and_clears_one_panel() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );

        assert!(apply_set_workspace_panel_pull_request(
            &mut snapshot,
            0,
            "surface-2",
            42,
            "MR",
            "https://gitlab.example/project/-/merge_requests/42",
            SessionPullRequestStatusSnapshot::Open,
            Some("feature/api".to_string()),
            false,
        ));
        assert!(apply_set_workspace_panel_pull_request(
            &mut snapshot,
            0,
            "surface-1",
            7,
            "PR",
            "https://github.com/manaflow-ai/cmux/pull/7",
            SessionPullRequestStatusSnapshot::Merged,
            None,
            false,
        ));
        assert!(apply_set_workspace_panel_pull_request(
            &mut snapshot,
            0,
            "surface-2",
            43,
            "Review",
            "https://example.test/reviews/43",
            SessionPullRequestStatusSnapshot::Closed,
            None,
            true,
        ));

        let requests = tab_manager(&snapshot).workspaces[0]
            .panel_pull_requests
            .as_ref()
            .expect("pull requests set");
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].panel_id, "surface-1");
        assert_eq!(requests[1].panel_id, "surface-2");
        assert_eq!(requests[1].number, 43);
        assert_eq!(requests[1].label, "Review");
        assert_eq!(requests[1].status, SessionPullRequestStatusSnapshot::Closed);
        assert!(requests[1].is_stale);

        assert!(apply_clear_workspace_panel_pull_request(
            &mut snapshot,
            0,
            "surface-2"
        ));
        let requests = tab_manager(&snapshot).workspaces[0]
            .panel_pull_requests
            .as_ref()
            .expect("one pull request remains");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].panel_id, "surface-1");

        assert!(apply_clear_workspace_panel_pull_request(
            &mut snapshot,
            0,
            "surface-1"
        ));
        assert_eq!(
            tab_manager(&snapshot).workspaces[0].panel_pull_requests,
            None
        );
    }

    #[test]
    fn apply_close_prunes_panel_listening_ports() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );
        assert!(apply_set_panel_listening_ports(
            &mut snapshot,
            0,
            "surface-1",
            &[3000],
        ));
        assert!(apply_set_panel_listening_ports(
            &mut snapshot,
            0,
            "surface-2",
            &[5173],
        ));

        assert_eq!(
            apply_close(&mut snapshot, "surface-2"),
            CloseOutcome::Removed
        );
        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(workspace.listening_ports, Some(vec![3000]));
        assert_eq!(
            workspace
                .panel_listening_ports
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| entry.panel_id.as_str())
                .collect::<Vec<_>>(),
            vec!["surface-1"]
        );
    }

    #[test]
    fn apply_close_prunes_panel_ttys() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );
        assert!(apply_set_panel_tty(
            &mut snapshot,
            0,
            "surface-1",
            "ttys001"
        ));
        assert!(apply_set_panel_tty(
            &mut snapshot,
            0,
            "surface-2",
            "ttys002"
        ));

        assert_eq!(
            apply_close(&mut snapshot, "surface-2"),
            CloseOutcome::Removed
        );
        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(
            workspace
                .panel_ttys
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| (entry.panel_id.as_str(), entry.tty.as_str()))
                .collect::<Vec<_>>(),
            vec![("surface-1", "ttys001")]
        );
    }

    #[test]
    fn apply_close_prunes_panel_shell_activity() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        );
        assert!(apply_set_panel_shell_activity(
            &mut snapshot,
            0,
            "surface-1",
            SessionPanelShellActivityStateSnapshot::PromptIdle,
        ));
        assert!(apply_set_panel_shell_activity(
            &mut snapshot,
            0,
            "surface-2",
            SessionPanelShellActivityStateSnapshot::CommandRunning,
        ));

        assert_eq!(
            apply_close(&mut snapshot, "surface-2"),
            CloseOutcome::Removed
        );
        let workspace = &tab_manager(&snapshot).workspaces[0];
        assert_eq!(
            workspace
                .panel_shell_activity
                .as_ref()
                .unwrap()
                .iter()
                .map(|entry| (entry.panel_id.as_str(), entry.state.clone()))
                .collect::<Vec<_>>(),
            vec![(
                "surface-1",
                SessionPanelShellActivityStateSnapshot::PromptIdle
            )]
        );
    }

    #[test]
    fn restorable_agent_snapshot_upserts_for_the_scoped_panel() {
        let mut snapshot = initial_snapshot("surface-1");
        let workspace_id = tab_manager(&snapshot).workspaces[0]
            .workspace_id
            .clone()
            .expect("workspace id");
        let started = StartedAgentSessionSnapshot {
            panel_id: "surface-1".to_string(),
            workspace_id: Some(workspace_id.clone()),
            provider_id: "codex".to_string(),
            session_id: "codex-session-1".to_string(),
            executable_path: "C:\\Program Files\\Codex\\codex.exe".to_string(),
            arguments: vec!["app-server".to_string()],
            working_directory: Some("C:\\repo".to_string()),
        };
        let restorable = restorable_snapshot_from_started(&started);
        assert!(apply_restorable_agent_snapshot(
            &mut snapshot,
            started.workspace_id.as_deref(),
            &started.panel_id,
            restorable.clone(),
        ));
        let entries = tab_manager(&snapshot).workspaces[0]
            .restorable_agent_snapshots
            .as_ref()
            .expect("agent snapshot");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].panel_id, "surface-1");
        assert_eq!(entries[0].snapshot.kind, "codex");
        assert_eq!(entries[0].snapshot.session_id, "codex-session-1");
        assert_eq!(
            entries[0].snapshot.fork_command.as_deref(),
            Some("& 'C:\\Program Files\\Codex\\codex.exe' fork 'codex-session-1'")
        );

        assert!(!apply_restorable_agent_snapshot(
            &mut snapshot,
            Some(&workspace_id),
            "surface-1",
            restorable,
        ));
    }

    #[test]
    fn restorable_agent_snapshot_rejects_wrong_workspace_scope() {
        let mut snapshot = initial_snapshot("surface-1");
        let started = StartedAgentSessionSnapshot {
            panel_id: "surface-1".to_string(),
            workspace_id: Some("workspace-missing".to_string()),
            provider_id: "claude".to_string(),
            session_id: "claude-session-1".to_string(),
            executable_path: "claude".to_string(),
            arguments: Vec::new(),
            working_directory: None,
        };
        assert!(!apply_restorable_agent_snapshot(
            &mut snapshot,
            started.workspace_id.as_deref(),
            &started.panel_id,
            restorable_snapshot_from_started(&started),
        ));
        assert_eq!(
            tab_manager(&snapshot).workspaces[0].restorable_agent_snapshots,
            None
        );
    }

    #[test]
    fn apply_set_workspace_unread_sets_and_clears_selected_workspace() {
        let mut snapshot = initial_snapshot("surface-1");

        assert!(apply_set_workspace_unread_at(
            &mut snapshot,
            0,
            Some("surface-1"),
            true,
            456
        ));
        let unreads = tab_manager(&snapshot).workspaces[0]
            .panel_unreads
            .as_ref()
            .expect("unread metadata");
        assert_eq!(unreads[0].panel_id, "surface-1");
        assert!(unreads[0].is_unread);
        assert_eq!(unreads[0].unread_at, Some(456));

        assert!(!apply_set_workspace_unread(
            &mut snapshot,
            0,
            Some("surface-1"),
            true
        ));
        assert!(apply_set_workspace_unread(&mut snapshot, 0, None, false));
        assert_eq!(tab_manager(&snapshot).workspaces[0].panel_unreads, None);
    }

    #[test]
    fn apply_set_workspace_unread_missing_workspace_is_a_noop() {
        let mut snapshot = initial_snapshot("surface-1");
        let before = snapshot.clone();
        assert!(!apply_set_workspace_unread(
            &mut snapshot,
            2,
            Some("surface-1"),
            true
        ));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_set_workspace_pinned_reorders_and_selection_follows() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None); // 2 workspaces, sel=1
        assert!(apply_set_workspace_pinned(&mut snapshot, 1, true));
        let tabs = tab_manager(&snapshot);
        // The pinned workspace floats to the top; selection follows it.
        assert_eq!(tabs.workspaces[0].is_pinned, Some(true));
        assert_eq!(tabs.workspaces[1].is_pinned, None);
        assert_eq!(tabs.selected_workspace_index, Some(0));
        assert_eq!(count_leaves(active_layout(&snapshot)), 1);
    }

    #[test]
    fn apply_set_workspace_pinned_already_at_value_is_a_noop() {
        let mut snapshot = initial_snapshot("surface-1");
        assert!(apply_set_workspace_pinned(&mut snapshot, 0, true));
        let before = snapshot.clone();
        // Already pinned → false (drives the emit gate), snapshot unchanged.
        assert!(!apply_set_workspace_pinned(&mut snapshot, 0, true));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_reorder_workspaces_moves_and_selection_follows() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        apply_new_workspace(&mut snapshot, "surface-3", None, None, None, None); // 3 workspaces, sel=2
        assert!(apply_reorder_workspaces(&mut snapshot, 2, 0, false));
        let tabs = tab_manager(&snapshot);
        // The mover lands at index 0 and the index-based selection follows it.
        if let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = &tabs.workspaces[0].layout {
            assert_eq!(pane.panel_ids, ["surface-3"]);
        } else {
            panic!("expected a pane");
        }
        assert_eq!(tabs.selected_workspace_index, Some(0));
    }

    #[test]
    fn apply_reorder_workspaces_no_op_is_gated() {
        // Out-of-range mover / clamp-back-to-place both report false (drives
        // the emit gate) and leave the snapshot untouched.
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2", None, None, None, None);
        let before = snapshot.clone();
        assert!(!apply_reorder_workspaces(&mut snapshot, 5, 0, false));
        assert!(!apply_reorder_workspaces(&mut snapshot, 1, 999, false)); // clamps to 1
        assert_eq!(snapshot, before);
    }

    #[test]
    fn apply_set_group_collapsed_flips_the_flag() {
        let mut snapshot = initial_snapshot("surface-1");
        snapshot.windows[0].tab_manager.workspace_groups =
            Some(vec![cmux_core::session::SessionWorkspaceGroupSnapshot {
                id: "g".to_string(),
                name: "G".to_string(),
                ..Default::default()
            }]);
        assert!(apply_set_group_collapsed(&mut snapshot, "g", true));
        let groups = tab_manager(&snapshot).workspace_groups.as_ref().unwrap();
        assert!(groups[0].is_collapsed);
        // Already at the requested value → false (drives the emit gate).
        assert!(!apply_set_group_collapsed(&mut snapshot, "g", true));
    }

    #[test]
    fn apply_set_group_collapsed_unknown_group_is_a_noop() {
        let mut snapshot = initial_snapshot("surface-1");
        let before = snapshot.clone();
        assert!(!apply_set_group_collapsed(&mut snapshot, "g", true));
        assert_eq!(snapshot, before);
    }

    #[test]
    fn snapshot_serializes_with_the_session_changed_shape() {
        // The web bridge parses this exact JSON; assert the round-trip holds and
        // the layout union uses the `{type, pane|split}` wire shape.
        let mut snapshot = initial_snapshot("surface-1");
        apply_split(
            &mut snapshot,
            "surface-1",
            SessionSplitOrientation::Vertical,
            "surface-2",
            false,
        );
        let json = serde_json::to_string(&snapshot).expect("serialize");
        assert!(json.contains("\"type\":\"split\""));
        assert!(json.contains("\"orientation\":\"vertical\""));
        let round: AppSessionSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(round, snapshot);
    }
}
