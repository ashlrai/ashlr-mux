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
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LockResult, Mutex, MutexGuard, PoisonError};

use cmux_config::{CmuxLayoutNode, CmuxSplitDirection, CmuxSurfaceType};
use cmux_core::session::{
    AgentLaunchCommandSnapshot, AppSessionSnapshot, SessionGitBranchSnapshot,
    SessionPanelGitBranchSnapshot, SessionPanelListeningPortsSnapshot,
    SessionPanelPullRequestSnapshot, SessionPanelRestorableAgentSnapshot,
    SessionPanelShellActivitySnapshot, SessionPanelShellActivityStateSnapshot,
    SessionPanelTerminalStartupSnapshot, SessionPanelTtySnapshot, SessionPullRequestStatusSnapshot,
    SessionRestorableAgentSnapshot, SessionSplitOrientation, SessionSurfaceKindSnapshot,
    SessionTabManagerSnapshot, SessionWindowSnapshot, SessionWorkspaceAgentPidSnapshot,
    SessionWorkspaceLayoutSnapshot, SessionWorkspaceRemoteDaemonSnapshot,
    SessionWorkspaceRemoteProxySnapshot, SessionWorkspaceRemoteSnapshot,
    SessionWorkspaceSidebarLogEntrySnapshot, SessionWorkspaceSidebarMetadataBlockSnapshot,
    SessionWorkspaceSidebarMetadataSnapshot, SessionWorkspaceSidebarProgressSnapshot,
    SessionWorkspaceSidebarStatusSnapshot, SessionWorkspaceSnapshot,
    SESSION_SNAPSHOT_SCHEMA_VERSION,
};
use cmux_core::session_ops::{self, CloseOutcome, SplitChild};
use cmux_workspaces::{WorkspaceBatchReorderError, WorkspaceReorderPlanItem};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use uuid::Uuid;

mod commands;
mod control_window_registration;
mod pane_topology;
mod workspace_ordering;
use commands::collapse_infallible_publication_error;
pub use commands::*;
#[cfg(test)]
use commands::{parse_session_navigation_uri, parse_ssh_uri, workspace_is_selected};
use control_window_registration::transact_register_window;
pub(crate) use control_window_registration::{
    register_prepared_window_for_control, unregister_window_for_control_suppressing_events,
};
pub(crate) use pane_topology::finalize_broken_pane_snapshot;
pub(crate) use workspace_ordering::{
    reorder_workspaces_in_window_for_control, reorder_workspaces_many_in_window_for_control,
    transact_value_if_changed_suppressing_derived_events, ReorderWorkspacesManyControlError,
};

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
    /// Every read/write guard automatically joins the same reentrant mutation
    /// gate used by complete control requests. Reentrancy lets a request hold
    /// the outer gate while helpers take snapshot guards on the same thread.
    snapshot: GatedSnapshot,
    next_panel: AtomicU64,
    closed_browser_tabs: Mutex<Vec<ClosedBrowserTabSnapshot>>,
    closed_workspaces: Mutex<Vec<ClosedWorkspaceSnapshot>>,
    remote_configs: Mutex<HashMap<String, WorkspaceRemoteControlConfig>>,
    workspace_focus_history: Mutex<HashMap<String, WorkspaceFocusHistory>>,
    remote_workspace_rename_controller: Arc<dyn RemoteWorkspaceRenameController>,
    deferred_remote_workspace_renames: Mutex<Vec<RemoteWorkspaceRenameRequest>>,
}

#[cfg(test)]
#[path = "session/change_gated_direct_writers_red.rs"]
mod change_gated_direct_writers_red;

#[cfg(test)]
#[path = "session/always_publish_direct_writers_red.rs"]
mod always_publish_direct_writers_red;

#[cfg(test)]
#[path = "session/terminal_structural_writers_red.rs"]
mod terminal_structural_writers_red;

#[cfg(test)]
#[path = "session/move_panel_workspace_red.rs"]
mod move_panel_workspace_red;

#[cfg(test)]
#[path = "session/pane_topology_red.rs"]
mod pane_topology_red;

#[cfg(test)]
#[path = "session/surface_move_reorder_red.rs"]
mod surface_move_reorder_red;

#[cfg(test)]
#[path = "session/focus_navigation_red.rs"]
mod focus_navigation_red;

#[cfg(test)]
#[path = "session/pane_layout_red.rs"]
mod pane_layout_red;

#[cfg(test)]
#[path = "session/workspace_selection_reorder_red.rs"]
mod workspace_selection_reorder_red;

#[cfg(test)]
#[path = "session/sidebar_metadata_red.rs"]
mod sidebar_metadata_red;

#[cfg(test)]
#[path = "session/public_control_metadata_red.rs"]
mod public_control_metadata_red;

#[cfg(test)]
#[path = "session/workspace_action_transactions_red.rs"]
mod workspace_action_transactions_red;

#[cfg(test)]
#[path = "session/runtime_model_facts_red.rs"]
mod runtime_model_facts_red;

#[cfg(test)]
#[path = "session/browser_view_state_red.rs"]
mod browser_view_state_red;

#[cfg(test)]
#[path = "session/pure_typed_writers_red.rs"]
mod pure_typed_writers_red;

#[cfg(test)]
#[path = "session/diff_durable_publication_red.rs"]
mod diff_durable_publication_red;

#[cfg(test)]
#[path = "session/restore_previous_launch_red.rs"]
mod restore_previous_launch_red;

#[cfg(test)]
#[path = "session/manual_restore_runtime_effects_red.rs"]
mod manual_restore_runtime_effects_red;

#[cfg(test)]
#[path = "session/restore_previous_additive_red.rs"]
mod restore_previous_additive_red;

#[cfg(test)]
#[path = "session/restore_identity_graph_red.rs"]
mod restore_identity_graph_red;

#[cfg(test)]
#[path = "session/remote_runtime_restore_red.rs"]
mod remote_runtime_restore_red;

#[cfg(test)]
#[path = "session/next_panel_allocation_fence_red.rs"]
mod next_panel_allocation_fence_red;

#[cfg(test)]
#[path = "session/browser_proxy_transactions_red.rs"]
mod browser_proxy_transactions_red;

#[cfg(test)]
#[path = "session/window_lifecycle_transactions_red.rs"]
mod window_lifecycle_transactions_red;

#[cfg(test)]
#[path = "session/activation_workspace_transactions_red.rs"]
mod activation_workspace_transactions_red;

#[cfg(test)]
#[path = "session/runtime_close_leases_red.rs"]
mod runtime_close_leases_red;

#[cfg(test)]
#[path = "session/browser_runtime_close_leases_red.rs"]
mod browser_runtime_close_leases_red;

#[cfg(test)]
#[path = "session/browser_built_child_cleanup_red.rs"]
mod browser_built_child_cleanup_red;

#[cfg(test)]
#[path = "session/browser_pending_cleanup_leases_red.rs"]
mod browser_pending_cleanup_leases_red;

#[cfg(test)]
#[path = "session/browser_navigation_callback_fence_red.rs"]
mod browser_navigation_callback_fence_red;

#[cfg(test)]
#[path = "session/browser_programmatic_navigation_handoff_red.rs"]
mod browser_programmatic_navigation_handoff_red;
mod control_snapshot;
mod persistence;
mod restore;
mod runtime_facts;
mod snapshot_mutations;
mod startup_restore;
#[cfg(test)]
#[path = "session/unit_tests.rs"]
mod tests;
mod workspace_selection;

use control_snapshot::notify_session_changed_with_event_policy;
pub(crate) use control_snapshot::{
    commit_lifecycle_snapshot_for_control, commit_lifecycle_snapshot_for_control_if_current,
    ensure_lifecycle_snapshot_current,
};
pub(crate) use persistence::default_workspace_directory;
use persistence::*;
use restore::*;
pub(crate) use restore::{restore_previous_launch_for_control, RestorePreviousLaunchOutcome};
use runtime_facts::*;
pub(crate) use runtime_facts::{record_started_agent_session, StartedAgentSessionSnapshot};
use snapshot_mutations::*;
pub(crate) use startup_restore::bootstrap_session_persistence;
use workspace_selection::select_workspace_in_window_candidate;

impl Default for SessionState {
    fn default() -> Self {
        let snapshot = initial_snapshot(FIRST_PANEL_ID);
        let workspace_focus_history = workspace_focus_history_for_snapshot(&snapshot);
        Self {
            snapshot: GatedSnapshot::new(snapshot),
            next_panel: AtomicU64::new(2),
            closed_browser_tabs: Mutex::new(Vec::new()),
            closed_workspaces: Mutex::new(Vec::new()),
            remote_configs: Mutex::new(HashMap::new()),
            workspace_focus_history: Mutex::new(workspace_focus_history),
            remote_workspace_rename_controller: Arc::new(SshRemoteWorkspaceRenameController),
            deferred_remote_workspace_renames: Mutex::new(Vec::new()),
        }
    }
}

impl SessionState {
    pub(crate) fn lock_control_mutation(
        &self,
    ) -> Result<parking_lot::ReentrantMutexGuard<'_, ()>, String> {
        Ok(self.snapshot.lock_gate())
    }

    fn defer_remote_workspace_rename(&self, request: RemoteWorkspaceRenameRequest) {
        self.deferred_remote_workspace_renames
            .lock()
            .expect("deferred remote workspace rename mutex poisoned")
            .push(request);
    }

    pub(crate) fn flush_deferred_remote_workspace_renames(&self) {
        let requests = std::mem::take(
            &mut *self
                .deferred_remote_workspace_renames
                .lock()
                .expect("deferred remote workspace rename mutex poisoned"),
        );
        for request in requests {
            let _ = dispatch_remote_workspace_rename(
                self.remote_workspace_rename_controller.as_ref(),
                &request,
            );
        }
    }

    #[cfg(test)]
    /// Exercises the exact snapshot-lock and mutation primitives used by the
    /// structural (`session_new_terminal_tab`), focus (`session_focus_panel`),
    /// and metadata (`session_set_process_title`) Tauri writer categories.
    pub(crate) fn exercise_ui_writer_for_test(
        &self,
        category: TestUiWriterCategory,
    ) -> Result<(), String> {
        let mut snapshot = self
            .snapshot
            .lock()
            .map_err(|_| "Session state is unavailable".to_string())?;
        match category {
            TestUiWriterCategory::Structural => {
                let _ = apply_new_terminal_tab(
                    &mut snapshot,
                    FIRST_PANEL_ID,
                    "surface-ui-writer",
                    None,
                    None,
                    None,
                );
            }
            TestUiWriterCategory::Focus => {
                let _ = apply_focus_panel(&mut snapshot, FIRST_PANEL_ID);
            }
            TestUiWriterCategory::Metadata => {
                let _ = apply_set_process_title(&mut snapshot, FIRST_PANEL_ID, "writer-title");
            }
        }
        Ok(())
    }

    pub(crate) fn snapshot_for_lifecycle(&self) -> Result<AppSessionSnapshot, String> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.clone())
            .map_err(|_| "Session state is unavailable".to_string())
    }

    pub(crate) fn transact_lifecycle<R>(
        &self,
        app: &AppHandle,
        mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<R, String>,
    ) -> Result<(R, AppSessionSnapshot), String> {
        let mut operations =
            ProductionSnapshotPublicationOperations::new(app, self, DerivedEventPolicy::Record);
        transact_lifecycle_snapshot(&self.snapshot, &mut operations, mutation)
    }

    pub(crate) fn transact_pane_topology<R, E>(
        &self,
        app: &AppHandle,
        mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<R, E>,
    ) -> Result<(R, AppSessionSnapshot), PaneTopologyControlError<E>> {
        let mut operations =
            ProductionSnapshotPublicationOperations::new(app, self, DerivedEventPolicy::Record);
        transact_pane_topology_snapshot(&self.snapshot, &mut operations, mutation)
    }

    pub(crate) fn transact_snapshot_if_changed(
        &self,
        app: &AppHandle,
        mutation: impl FnOnce(&mut AppSessionSnapshot) -> bool,
    ) -> Result<AppSessionSnapshot, String> {
        let mut operations =
            ProductionSnapshotPublicationOperations::new(app, self, DerivedEventPolicy::Record);
        transact_snapshot_if_changed(&self.snapshot, &mut operations, mutation)
    }

    pub(crate) fn transact_result_if_changed<E>(
        &self,
        app: &AppHandle,
        mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<bool, E>,
    ) -> Result<AppSessionSnapshot, PaneTopologyControlError<E>> {
        let mut operations =
            ProductionSnapshotPublicationOperations::new(app, self, DerivedEventPolicy::Record);
        transact_result_if_changed_snapshot(&self.snapshot, &mut operations, mutation)
    }

    pub(crate) fn transact_value_if_changed<R, E>(
        &self,
        app: &AppHandle,
        mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<(R, bool), E>,
    ) -> Result<(R, AppSessionSnapshot), PaneTopologyControlError<E>> {
        let mut operations =
            ProductionSnapshotPublicationOperations::new(app, self, DerivedEventPolicy::Record);
        transact_value_if_changed_snapshot(&self.snapshot, &mut operations, mutation)
    }

    pub(crate) fn transact_snapshot_always(
        &self,
        app: &AppHandle,
        mutation: impl FnOnce(&mut AppSessionSnapshot) -> bool,
    ) -> Result<AppSessionSnapshot, String> {
        let mut operations =
            ProductionSnapshotPublicationOperations::new(app, self, DerivedEventPolicy::Record);
        transact_snapshot_always(&self.snapshot, &mut operations, mutation)
    }
}

trait SnapshotPublicationOperations {
    fn persist(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String>;
    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot);
    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String>;
}

fn publish_snapshot_transaction(
    authority: &GatedSnapshot,
    expected: Option<&AppSessionSnapshot>,
    candidate: &AppSessionSnapshot,
    operations: &mut impl SnapshotPublicationOperations,
) -> Result<AppSessionSnapshot, String> {
    publish_snapshot_transaction_with_post_commit(
        authority,
        expected,
        candidate,
        operations,
        |_| {},
    )
}

fn publish_snapshot_transaction_with_post_commit(
    authority: &GatedSnapshot,
    expected: Option<&AppSessionSnapshot>,
    candidate: &AppSessionSnapshot,
    operations: &mut impl SnapshotPublicationOperations,
    post_commit: impl FnOnce(&AppSessionSnapshot),
) -> Result<AppSessionSnapshot, String> {
    let _publication_gate = authority.lock_gate();
    let mut guard = authority
        .lock()
        .map_err(|_| "Session state is unavailable".to_string())?;
    if let Some(expected) = expected {
        ensure_lifecycle_snapshot_current(&guard, expected)?;
    }
    operations.persist(candidate)?;
    *guard = candidate.clone();
    let committed = guard.clone();
    drop(guard);
    post_commit(&committed);
    operations.update_event_baseline(&committed);
    operations.emit(&committed)?;
    Ok(committed)
}

fn transact_lifecycle_snapshot<R>(
    authority: &GatedSnapshot,
    operations: &mut impl SnapshotPublicationOperations,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<R, String>,
) -> Result<(R, AppSessionSnapshot), String> {
    transact_pane_topology_snapshot(authority, operations, mutation).map_err(|error| match error {
        PaneTopologyControlError::Operation(error)
        | PaneTopologyControlError::Publication(error) => error,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PaneTopologyControlError<E> {
    Operation(E),
    Publication(String),
}

fn transact_pane_topology_snapshot<R, E>(
    authority: &GatedSnapshot,
    operations: &mut impl SnapshotPublicationOperations,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<R, E>,
) -> Result<(R, AppSessionSnapshot), PaneTopologyControlError<E>> {
    let _transaction_gate = authority.lock_gate();
    let current = authority
        .lock()
        .map_err(|_| {
            PaneTopologyControlError::Publication("Session state is unavailable".to_string())
        })?
        .clone();
    let mut candidate = current.clone();
    let result = mutation(&mut candidate).map_err(PaneTopologyControlError::Operation)?;
    let committed = publish_snapshot_transaction(authority, Some(&current), &candidate, operations)
        .map_err(PaneTopologyControlError::Publication)?;
    Ok((result, committed))
}

fn transact_snapshot_if_changed(
    authority: &GatedSnapshot,
    operations: &mut impl SnapshotPublicationOperations,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> bool,
) -> Result<AppSessionSnapshot, String> {
    match transact_result_if_changed_snapshot(authority, operations, |candidate| {
        Ok::<_, std::convert::Infallible>(mutation(candidate))
    }) {
        Ok(snapshot) => Ok(snapshot),
        Err(PaneTopologyControlError::Publication(error)) => Err(error),
        Err(PaneTopologyControlError::Operation(error)) => match error {},
    }
}

fn transact_result_if_changed_snapshot<E>(
    authority: &GatedSnapshot,
    operations: &mut impl SnapshotPublicationOperations,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<bool, E>,
) -> Result<AppSessionSnapshot, PaneTopologyControlError<E>> {
    transact_value_if_changed_snapshot(authority, operations, |candidate| {
        mutation(candidate).map(|changed| ((), changed))
    })
    .map(|((), snapshot)| snapshot)
}

fn transact_value_if_changed_snapshot<R, E>(
    authority: &GatedSnapshot,
    operations: &mut impl SnapshotPublicationOperations,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<(R, bool), E>,
) -> Result<(R, AppSessionSnapshot), PaneTopologyControlError<E>> {
    transact_value_if_changed_snapshot_with_post_commit(authority, operations, mutation, |_, _| {})
}

fn transact_value_if_changed_snapshot_with_post_commit<R, E>(
    authority: &GatedSnapshot,
    operations: &mut impl SnapshotPublicationOperations,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<(R, bool), E>,
    post_commit: impl FnOnce(&mut R, &AppSessionSnapshot),
) -> Result<(R, AppSessionSnapshot), PaneTopologyControlError<E>> {
    let _transaction_gate = authority.lock_gate();
    let current = authority
        .lock()
        .map_err(|_| {
            PaneTopologyControlError::Publication("Session state is unavailable".to_string())
        })?
        .clone();
    let mut candidate = current.clone();
    let (mut value, changed) =
        mutation(&mut candidate).map_err(PaneTopologyControlError::Operation)?;
    if !changed {
        return Ok((value, current));
    }
    let committed = publish_snapshot_transaction_with_post_commit(
        authority,
        Some(&current),
        &candidate,
        operations,
        |committed| post_commit(&mut value, committed),
    )
    .map_err(PaneTopologyControlError::Publication)?;
    Ok((value, committed))
}

fn transact_snapshot_always(
    authority: &GatedSnapshot,
    operations: &mut impl SnapshotPublicationOperations,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> bool,
) -> Result<AppSessionSnapshot, String> {
    match transact_pane_topology_snapshot(authority, operations, |candidate| {
        let _ = mutation(candidate);
        Ok::<_, std::convert::Infallible>(())
    }) {
        Ok(((), snapshot)) => Ok(snapshot),
        Err(PaneTopologyControlError::Publication(error)) => Err(error),
        Err(PaneTopologyControlError::Operation(error)) => match error {},
    }
}

/// A snapshot mutex whose guards always participate in the control mutation
/// gate. Keeping the gate inside this wrapper makes bypasses impossible for
/// existing and future `SessionState` writers that call `snapshot.lock()`.
struct GatedSnapshot {
    gate: parking_lot::ReentrantMutex<()>,
    value: Mutex<AppSessionSnapshot>,
}

impl GatedSnapshot {
    fn new(value: AppSessionSnapshot) -> Self {
        Self {
            gate: parking_lot::ReentrantMutex::new(()),
            value: Mutex::new(value),
        }
    }

    fn lock_gate(&self) -> parking_lot::ReentrantMutexGuard<'_, ()> {
        self.gate.lock()
    }

    fn lock(&self) -> LockResult<GatedSnapshotGuard<'_>> {
        let gate = self.gate.lock();
        match self.value.lock() {
            Ok(value) => Ok(GatedSnapshotGuard { value, _gate: gate }),
            Err(poisoned) => Err(PoisonError::new(GatedSnapshotGuard {
                value: poisoned.into_inner(),
                _gate: gate,
            })),
        }
    }
}

struct GatedSnapshotGuard<'a> {
    // Drop the snapshot guard before releasing the mutation gate.
    value: MutexGuard<'a, AppSessionSnapshot>,
    _gate: parking_lot::ReentrantMutexGuard<'a, ()>,
}

impl Deref for GatedSnapshotGuard<'_> {
    type Target = AppSessionSnapshot;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl DerefMut for GatedSnapshotGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TestUiWriterCategory {
    Structural,
    Focus,
    Metadata,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct WorkspaceFocusHistory {
    entries: Vec<String>,
    index: Option<usize>,
}

impl WorkspaceFocusHistory {
    fn record(&mut self, workspace_id: &str) {
        if self
            .index
            .and_then(|index| self.entries.get(index))
            .is_some_and(|current| current == workspace_id)
        {
            return;
        }
        let keep = self.index.map_or(0, |index| index + 1);
        self.entries.truncate(keep);
        self.entries.push(workspace_id.to_string());
        if self.entries.len() > 50 {
            self.entries.remove(0);
        }
        self.index = Some(self.entries.len() - 1);
    }

    fn navigate_back(&mut self, valid_workspace_ids: &HashSet<&str>) -> Option<String> {
        let mut target = self.index?.checked_sub(1)?;
        loop {
            let workspace_id = self.entries.get(target)?.clone();
            if valid_workspace_ids.contains(workspace_id.as_str()) {
                self.index = Some(target);
                return Some(workspace_id);
            }
            self.entries.remove(target);
            if let Some(index) = self.index.as_mut() {
                *index = index.saturating_sub(1);
            }
            target = target.checked_sub(1)?;
        }
    }
}

fn workspace_focus_history_for_snapshot(
    snapshot: &AppSessionSnapshot,
) -> HashMap<String, WorkspaceFocusHistory> {
    snapshot
        .windows
        .iter()
        .filter_map(|window| {
            let index = usize::try_from(window.tab_manager.selected_workspace_index?).ok()?;
            let workspace_id = window
                .tab_manager
                .workspaces
                .get(index)?
                .workspace_id
                .as_deref()?;
            let mut history = WorkspaceFocusHistory::default();
            history.record(workspace_id);
            Some((
                window
                    .window_id
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
                history,
            ))
        })
        .collect()
}

fn record_workspace_focus_history(state: &SessionState, snapshot: &AppSessionSnapshot) {
    let mut histories = state
        .workspace_focus_history
        .lock()
        .expect("workspace focus history mutex poisoned");
    for window in &snapshot.windows {
        let Some(index) = window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
        else {
            continue;
        };
        let Some(workspace_id) = window
            .tab_manager
            .workspaces
            .get(index)
            .and_then(|workspace| workspace.workspace_id.as_deref())
        else {
            continue;
        };
        histories
            .entry(
                window
                    .window_id
                    .clone()
                    .unwrap_or_else(|| "main".to_string()),
            )
            .or_default()
            .record(workspace_id);
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct RemoteWorkspaceRenameRequest {
    workspace_id: String,
    destination: String,
    port: Option<u16>,
    identity_file: Option<String>,
    ssh_options: Vec<String>,
    session: Option<String>,
    title: String,
}

trait RemoteWorkspaceRenameController: Send + Sync {
    fn rename(&self, request: &RemoteWorkspaceRenameRequest) -> Result<(), String>;
}

struct SshRemoteWorkspaceRenameController;

impl RemoteWorkspaceRenameController for SshRemoteWorkspaceRenameController {
    fn rename(&self, request: &RemoteWorkspaceRenameRequest) -> Result<(), String> {
        let mut command = Command::new("ssh");
        command.args(["-T", "-o", "BatchMode=yes"]);
        if let Some(port) = request.port {
            command.args(["-p", &port.to_string()]);
        }
        if let Some(identity_file) = request.identity_file.as_deref() {
            command.args(["-i", identity_file]);
        }
        for option in &request.ssh_options {
            command.args(["-o", option]);
        }
        command.arg(&request.destination);
        command.args(["tmux", "rename-session"]);
        if let Some(session) = request.session.as_deref() {
            command.args(["-t", session]);
        }
        command
            .arg(&request.title)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("failed to launch remote tmux rename: {error}"))?;
        Ok(())
    }
}

fn dispatch_remote_workspace_rename(
    controller: &dyn RemoteWorkspaceRenameController,
    request: &RemoteWorkspaceRenameRequest,
) -> Result<(), String> {
    controller.rename(request)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceCloseTeardownPlan {
    workspace_id: String,
    panel_ids: Vec<String>,
    clear_notifications: bool,
    clear_metadata: bool,
    clear_focus_history: bool,
    stop_remote: bool,
}

fn workspace_close_teardown_plan(
    workspace_id: &str,
    workspace: &SessionWorkspaceSnapshot,
) -> WorkspaceCloseTeardownPlan {
    fn collect_panel_ids(layout: &SessionWorkspaceLayoutSnapshot, panel_ids: &mut Vec<String>) {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                panel_ids.extend(pane.panel_ids.iter().cloned());
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                collect_panel_ids(&split.first, panel_ids);
                collect_panel_ids(&split.second, panel_ids);
            }
        }
    }
    let mut panel_ids = Vec::new();
    if let Some(layout) = workspace.layout.as_ref() {
        collect_panel_ids(layout, &mut panel_ids);
    }
    if let Some(surfaces) = workspace.surfaces.as_ref() {
        for surface in surfaces {
            if !panel_ids.contains(&surface.surface_id) {
                panel_ids.push(surface.surface_id.clone());
            }
        }
    }
    WorkspaceCloseTeardownPlan {
        workspace_id: workspace_id.to_string(),
        panel_ids,
        clear_notifications: true,
        clear_metadata: true,
        clear_focus_history: true,
        stop_remote: workspace.remote.is_some(),
    }
}

fn apply_workspace_close_teardown(
    app: &AppHandle,
    state: &SessionState,
    teardown: &WorkspaceCloseTeardownPlan,
) {
    if let Some(terminals) = app.try_state::<crate::terminal::TerminalState>() {
        for panel_id in &teardown.panel_ids {
            for terminal_id in
                crate::terminal::terminal_ids_for_panel_for_control(terminals.inner(), panel_id)
            {
                let _ = crate::terminal::terminal_shutdown_id_preserving_authority_for_control(
                    terminals.inner(),
                    terminal_id,
                );
                let _ =
                    crate::terminal::terminal_remove_id_for_control(terminals.inner(), terminal_id);
            }
        }
    }
    if let Some(browsers) = app.try_state::<crate::browser::BrowserWebviewState>() {
        for panel_id in &teardown.panel_ids {
            if crate::browser::browser_has_webview_for_control(browsers.inner(), panel_id)
                .unwrap_or(false)
            {
                let _ = crate::browser::browser_close_webview_strict_for_control(
                    browsers.inner(),
                    panel_id,
                );
            }
        }
    }
    if teardown.clear_notifications {
        if let Some(notifications) =
            app.try_state::<crate::notifications::NotificationCommandState>()
        {
            if let Ok(effects) = crate::notifications::notification_clear_workspace_for_control(
                notifications.inner(),
                &teardown.workspace_id,
            ) {
                crate::control_socket::publish_notification_removal_effects(
                    app,
                    &effects,
                    "notification.cleared",
                    Some(&teardown.workspace_id),
                );
            }
        }
    }
    if teardown.clear_focus_history {
        let mut histories = state
            .workspace_focus_history
            .lock()
            .expect("workspace focus history mutex poisoned");
        for history in histories.values_mut() {
            history.entries.retain(|id| id != &teardown.workspace_id);
            history.index = history.entries.len().checked_sub(1);
        }
    }
    if teardown.clear_metadata {
        state
            .remote_configs
            .lock()
            .expect("remote config mutex poisoned")
            .remove(&teardown.workspace_id);
    }
    if teardown.stop_remote {
        if let Some(brokers) = app.try_state::<crate::remote_proxy::RemoteProxyBrokerState>() {
            brokers.stop_workspace_broker(&teardown.workspace_id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceRenameResolution {
    ResolvedChanged,
    ResolvedUnchanged,
    NotFound,
}

fn rename_workspace_resolution(
    tabs: &mut SessionTabManagerSnapshot,
    workspace_index: usize,
    title: &str,
) -> WorkspaceRenameResolution {
    if tabs.workspaces.get(workspace_index).is_none() {
        return WorkspaceRenameResolution::NotFound;
    }
    if session_ops::rename_workspace(tabs, workspace_index as i64, title) {
        WorkspaceRenameResolution::ResolvedChanged
    } else {
        WorkspaceRenameResolution::ResolvedUnchanged
    }
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
    let mut environment = active_workspace(snapshot)
        .and_then(|workspace| workspace.workspace_environment.clone())
        .unwrap_or_default();
    environment.extend(initial_terminal_environment.unwrap_or_default());
    let command = initial_terminal_command
        .and_then(normalize_nonempty)
        .map(str::to_string);
    let input = initial_terminal_input
        .and_then(normalize_nonempty)
        .map(str::to_string);
    let environment = (!environment.is_empty()).then_some(environment);
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

fn window_index_for_label(snapshot: &AppSessionSnapshot, label: &str) -> Option<usize> {
    snapshot
        .windows
        .iter()
        .position(|window| window.window_id.as_deref() == Some(label))
        .or_else(|| (label == "main" && !snapshot.windows.is_empty()).then_some(0))
}

pub(crate) fn snapshot_for_window(
    snapshot: &AppSessionSnapshot,
    label: &str,
) -> AppSessionSnapshot {
    let mut projected = snapshot.clone();
    if let Some(index) = window_index_for_label(&projected, label) {
        projected.windows.swap(0, index);
    }
    projected
}

fn emit_session_changed(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    for (label, window) in app.webview_windows() {
        let _ = window.emit(SESSION_CHANGED_EVENT, snapshot_for_window(snapshot, &label));
    }
}

fn notify_session_changed(app: &AppHandle, snapshot: &AppSessionSnapshot) {
    notify_session_changed_with_event_policy(app, snapshot, DerivedEventPolicy::Record);
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DerivedEventPolicy {
    Record,
    Suppress,
}

struct ProductionSnapshotPublicationOperations<'a> {
    app: &'a AppHandle,
    state: &'a SessionState,
    derived_events: DerivedEventPolicy,
    reseed_next_panel: bool,
    refresh_window_state: bool,
}

impl<'a> ProductionSnapshotPublicationOperations<'a> {
    fn new(
        app: &'a AppHandle,
        state: &'a SessionState,
        derived_events: DerivedEventPolicy,
    ) -> Self {
        Self {
            app,
            state,
            derived_events,
            reseed_next_panel: true,
            refresh_window_state: true,
        }
    }

    fn with_deferred_next_panel_reseed(app: &'a AppHandle, state: &'a SessionState) -> Self {
        Self {
            app,
            state,
            derived_events: DerivedEventPolicy::Record,
            reseed_next_panel: false,
            refresh_window_state: true,
        }
    }

    fn for_manual_restore(app: &'a AppHandle, state: &'a SessionState) -> Self {
        Self {
            app,
            state,
            derived_events: DerivedEventPolicy::Suppress,
            reseed_next_panel: false,
            refresh_window_state: true,
        }
    }
}

impl SnapshotPublicationOperations for ProductionSnapshotPublicationOperations<'_> {
    fn persist(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        persist_current_snapshot(self.app, candidate)
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        if self.reseed_next_panel {
            self.state
                .next_panel
                .fetch_max(next_panel_counter(candidate), Ordering::Relaxed);
        }
        record_workspace_focus_history(self.state, candidate);
        match self.derived_events {
            DerivedEventPolicy::Record => {
                crate::control_socket::record_session_changed_event(self.app, candidate)
            }
            DerivedEventPolicy::Suppress => {
                crate::control_socket::replace_session_event_baseline(self.app, candidate)
            }
        }
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        emit_session_changed(self.app, candidate);
        crate::window_title::refresh_window_titles(self.app, candidate);
        if self.refresh_window_state {
            crate::window::emit_window_states(self.app);
        }
        Ok(())
    }
}

fn mutate_optional_snapshot_for_control(
    app: &AppHandle,
    state: &SessionState,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> bool,
) -> Result<Option<AppSessionSnapshot>, String> {
    let (resolved, snapshot) = state
        .transact_value_if_changed(app, |snapshot| {
            let resolved = mutation(snapshot);
            Ok::<(bool, bool), std::convert::Infallible>((resolved, resolved))
        })
        .map_err(collapse_infallible_publication_error)?;
    Ok(resolved.then_some(snapshot))
}

pub(crate) fn open_markdown_file_in_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    file_path: &str,
) -> Result<Option<AppSessionSnapshot>, String> {
    mutate_optional_snapshot_for_control(app, state, |snapshot| {
        apply_open_markdown_file(snapshot, panel_id, file_path)
    })
}

pub(crate) fn open_file_in_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    file_path: &str,
) -> Result<Option<AppSessionSnapshot>, String> {
    mutate_optional_snapshot_for_control(app, state, |snapshot| {
        apply_open_file(snapshot, panel_id, file_path)
    })
}

pub(crate) fn open_custom_sidebar_in_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    file_path: &str,
) -> Result<Option<AppSessionSnapshot>, String> {
    mutate_optional_snapshot_for_control(app, state, |snapshot| {
        apply_open_custom_sidebar(snapshot, panel_id, file_path)
    })
}

pub(crate) fn open_diff_viewer_in_panel(
    app: &AppHandle,
    state: &SessionState,
    diff_state: &crate::diff::DiffState,
    panel_id: &str,
    token: &str,
    request_path: &str,
) -> Result<Option<AppSessionSnapshot>, String> {
    let Some(normalized_request_path) = normalize_diff_request_path(request_path) else {
        return Ok(None);
    };
    if !diff_state.has_registered_request(
        token.trim(),
        &normalized_request_path,
        std::time::SystemTime::now(),
    ) {
        return Ok(None);
    }
    let (resolved, snapshot) = state
        .transact_value_if_changed(app, |snapshot| {
            let resolved =
                apply_open_diff_viewer(snapshot, panel_id, token, &normalized_request_path);
            Ok::<_, std::convert::Infallible>((resolved, resolved))
        })
        .map_err(collapse_infallible_publication_error)?;
    Ok(resolved.then_some(snapshot))
}

struct BrowserProxyLease(crate::remote_proxy::WorkspacePanelBrokerLease);

impl BrowserProxyLease {
    fn proxy_url(&self) -> &str {
        self.0.proxy_url()
    }
}

trait BrowserProxyEffects {
    fn prepare(
        &mut self,
        snapshot: &AppSessionSnapshot,
        panel_id: &str,
    ) -> Option<BrowserProxyLease>;
    fn rollback(&mut self, lease: BrowserProxyLease);
    fn commit(&mut self, lease: BrowserProxyLease);
}

struct ProductionBrowserProxyEffects<'a> {
    app: &'a AppHandle,
}

impl BrowserProxyEffects for ProductionBrowserProxyEffects<'_> {
    fn prepare(
        &mut self,
        snapshot: &AppSessionSnapshot,
        panel_id: &str,
    ) -> Option<BrowserProxyLease> {
        let workspace_id = workspace_id_for_panel_in_snapshot(snapshot, panel_id)?;
        let observer = Arc::new(PanelBrowserProxyObserver {
            app: self.app.clone(),
            panel_id: panel_id.to_string(),
        });
        self.app
            .state::<crate::remote_proxy::RemoteProxyBrokerState>()
            .prepare_workspace_panel_broker(&workspace_id, panel_id, Some(observer))
            .ok()
            .map(BrowserProxyLease)
    }

    fn rollback(&mut self, lease: BrowserProxyLease) {
        self.app
            .state::<crate::remote_proxy::RemoteProxyBrokerState>()
            .rollback_workspace_panel_broker(lease.0);
    }

    fn commit(&mut self, lease: BrowserProxyLease) {
        self.app
            .state::<crate::remote_proxy::RemoteProxyBrokerState>()
            .commit_workspace_panel_broker(lease.0);
    }
}

fn bind_browser_proxy_lease(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    lease: Option<&BrowserProxyLease>,
) {
    let Some(lease) = lease else {
        return;
    };
    let Some(layout) = active_layout_slot(snapshot).and_then(Option::as_mut) else {
        return;
    };
    set_layout_browser_proxy_url_for_panel(layout, panel_id, Some(lease.proxy_url()));
}

fn publish_browser_candidate(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffects,
    current: &AppSessionSnapshot,
    mut candidate: AppSessionSnapshot,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    let lease = proxy.prepare(&candidate, panel_id);
    bind_browser_proxy_lease(&mut candidate, panel_id, lease.as_ref());
    match publish_snapshot_transaction(authority, Some(current), &candidate, publication) {
        Ok(committed) => {
            if let Some(lease) = lease {
                proxy.commit(lease);
            }
            Ok(committed)
        }
        Err(message) => {
            if let Some(lease) = lease {
                proxy.rollback(lease);
            }
            Err(message)
        }
    }
}

fn transaction_current_snapshot(authority: &GatedSnapshot) -> Result<AppSessionSnapshot, String> {
    authority
        .lock()
        .map(|snapshot| snapshot.clone())
        .map_err(|_| "Session state is unavailable".to_string())
}

fn transact_open_browser_url(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffects,
    panel_id: &str,
    url: Option<&str>,
) -> Result<Option<AppSessionSnapshot>, String> {
    let _transaction_guard = authority.lock_gate();
    let current = transaction_current_snapshot(authority)?;
    let mut candidate = current.clone();
    if !apply_open_browser_url(&mut candidate, panel_id, url) {
        return Ok(None);
    }
    publish_browser_candidate(authority, publication, proxy, &current, candidate, panel_id)
        .map(Some)
}

#[derive(Debug)]
pub(crate) enum BrowserPanelCreateError {
    NotFound(String),
    Publication(String),
}

impl std::fmt::Display for BrowserPanelCreateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(message) | Self::Publication(message) => formatter.write_str(message),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn transact_split_browser(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffects,
    panel_id: &str,
    orientation: SessionSplitOrientation,
    insert_first: bool,
    url: Option<&str>,
) -> Result<AppSessionSnapshot, BrowserPanelCreateError> {
    let _transaction_guard = authority.lock_gate();
    let current =
        transaction_current_snapshot(authority).map_err(BrowserPanelCreateError::Publication)?;
    let new_panel_id = Uuid::new_v4().to_string();
    let mut candidate = current.clone();
    if !apply_split(
        &mut candidate,
        panel_id,
        orientation,
        &new_panel_id,
        insert_first,
    ) {
        return Err(BrowserPanelCreateError::NotFound(format!(
            "no pane holds panel id {panel_id}"
        )));
    }
    apply_open_browser_url(&mut candidate, &new_panel_id, url);
    let committed = publish_browser_candidate(
        authority,
        publication,
        proxy,
        &current,
        candidate,
        &new_panel_id,
    )
    .map_err(BrowserPanelCreateError::Publication)?;
    next_panel.fetch_add(1, Ordering::Relaxed);
    Ok(committed)
}

fn transact_new_browser_workspace(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffects,
    url: Option<&str>,
) -> Result<AppSessionSnapshot, String> {
    let _transaction_guard = authority.lock_gate();
    let current = transaction_current_snapshot(authority)?;
    let new_panel_id = Uuid::new_v4().to_string();
    let mut candidate = current.clone();
    apply_new_workspace(&mut candidate, &new_panel_id, None, None, None, None);
    apply_open_browser_url(&mut candidate, &new_panel_id, url);
    let committed = publish_browser_candidate(
        authority,
        publication,
        proxy,
        &current,
        candidate,
        &new_panel_id,
    )?;
    next_panel.fetch_add(1, Ordering::Relaxed);
    Ok(committed)
}

fn transact_reopen_closed_browser_tab(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    history: &Mutex<Vec<ClosedBrowserTabSnapshot>>,
    publication: &mut impl SnapshotPublicationOperations,
    proxy: &mut impl BrowserProxyEffects,
) -> Result<AppSessionSnapshot, String> {
    let _transaction_guard = authority.lock_gate();
    let mut history = history
        .lock()
        .expect("closed browser history mutex poisoned");
    let current = transaction_current_snapshot(authority)?;
    let Some(tab) = history.last().cloned() else {
        return Ok(current);
    };
    let new_panel_id = Uuid::new_v4().to_string();
    let mut candidate = current.clone();
    apply_reopen_closed_browser_tab(&mut candidate, &tab, &new_panel_id);
    let committed = publish_browser_candidate(
        authority,
        publication,
        proxy,
        &current,
        candidate,
        &new_panel_id,
    )?;
    history.pop();
    next_panel.fetch_add(1, Ordering::Relaxed);
    Ok(committed)
}

pub(crate) fn open_browser_url_in_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    url: Option<&str>,
) -> Result<Option<AppSessionSnapshot>, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Record);
    let mut proxy = ProductionBrowserProxyEffects { app };
    transact_open_browser_url(&state.snapshot, &mut publication, &mut proxy, panel_id, url)
}

fn mutate_browser_for_control<F>(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    mutator: F,
) -> Result<AppSessionSnapshot, String>
where
    F: FnOnce(&mut AppSessionSnapshot, &str) -> bool,
{
    state.transact_snapshot_if_changed(app, |snapshot| mutator(snapshot, panel_id))
}

pub(crate) fn browser_go_back_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    mutate_browser_for_control(app, state, panel_id, apply_browser_go_back)
}

pub(crate) fn browser_go_forward_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    mutate_browser_for_control(app, state, panel_id, apply_browser_go_forward)
}

pub(crate) fn clear_browser_history_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    mutate_browser_for_control(app, state, panel_id, apply_clear_browser_history)
}

pub(crate) fn toggle_browser_omnibar_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    mutate_browser_for_control(app, state, panel_id, apply_toggle_browser_omnibar)
}

pub(crate) fn toggle_browser_focus_mode_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    mutate_browser_for_control(app, state, panel_id, apply_toggle_browser_focus_mode)
}

pub(crate) fn toggle_browser_developer_tools_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    mutate_browser_for_control(app, state, panel_id, apply_toggle_browser_developer_tools)
}

pub(crate) fn show_browser_developer_tools_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    panel: &str,
) -> Result<AppSessionSnapshot, String> {
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
) -> Result<AppSessionSnapshot, String> {
    mutate_browser_for_control(app, state, panel_id, |snapshot, panel_id| {
        apply_set_browser_zoom(snapshot, panel_id, zoom)
    })
}

/// Return the current session snapshot (structure of windows/workspaces/panes).
#[tauri::command]
pub fn session_snapshot(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, SessionState>,
) -> AppSessionSnapshot {
    let snapshot = current_session_snapshot(&state);
    crate::window_title::refresh_window_titles(&app, &snapshot);
    crate::window::emit_window_states(&app);
    snapshot_for_window(&snapshot, window.label())
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

#[cfg(windows)]
fn replace_file_atomically(staged: &Path, destination: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let staged: Vec<u16> = staged.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(staged.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|error| error.to_string())
}

#[cfg(not(windows))]
fn replace_file_atomically(staged: &Path, destination: &Path) -> Result<(), String> {
    std::fs::rename(staged, destination).map_err(|error| error.to_string())
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum RegisterWindowOutcome {
    Unchanged(AppSessionSnapshot),
    Registered(AppSessionSnapshot),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RemovedWindowLease {
    removed: SessionWindowSnapshot,
    original_index: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UnregisterWindowOutcome {
    Unchanged(AppSessionSnapshot),
    Removed {
        snapshot: AppSessionSnapshot,
        lease: RemovedWindowLease,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MoveWorkspaceToWindowControlError {
    NotFound,
    Publication(String),
}

pub(crate) fn fresh_control_window_workspace(panel_id: &str) -> SessionWorkspaceSnapshot {
    let mut workspace = session_ops::fresh_terminal_workspace(panel_id);
    workspace.workspace_id = Some(Uuid::new_v4().to_string());
    workspace.current_directory = default_workspace_directory();
    if let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = workspace.layout.as_mut() {
        pane.pane_id = Some(Uuid::new_v4().to_string());
    }
    seed_initial_surface_record(&mut workspace);
    workspace
}

fn auxiliary_window_snapshot(window_id: &str, panel_id: &str) -> SessionWindowSnapshot {
    let workspace = fresh_control_window_workspace(panel_id);
    SessionWindowSnapshot {
        window_id: Some(window_id.to_string()),
        selected_workspace_id: workspace.workspace_id.clone(),
        dock: None,
        tab_manager: SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![workspace],
            workspace_groups: None,
        },
    }
}

fn transact_unregister_window(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    window_id: &str,
) -> Result<UnregisterWindowOutcome, String> {
    let _transaction_guard = authority.lock_gate();
    let before = transaction_current_snapshot(authority)?;
    let Some(index) = before
        .windows
        .iter()
        .position(|window| window.window_id.as_deref() == Some(window_id))
        .filter(|index| *index != 0)
    else {
        return Ok(UnregisterWindowOutcome::Unchanged(before));
    };
    let mut candidate = before.clone();
    let removed = candidate.windows.remove(index);
    let committed =
        publish_snapshot_transaction(authority, Some(&before), &candidate, publication)?;
    Ok(UnregisterWindowOutcome::Removed {
        snapshot: committed,
        lease: RemovedWindowLease {
            removed,
            original_index: index,
        },
    })
}

fn transact_restore_removed_window(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    lease: &RemovedWindowLease,
) -> Result<AppSessionSnapshot, String> {
    let _transaction_guard = authority.lock_gate();
    let current = transaction_current_snapshot(authority)?;
    let removed_window_id = lease.removed.window_id.as_deref();
    if current
        .windows
        .iter()
        .any(|window| window.window_id.as_deref() == removed_window_id)
    {
        return Ok(current);
    }
    let mut candidate = current.clone();
    let index = lease.original_index.min(candidate.windows.len());
    candidate.windows.insert(index, lease.removed.clone());
    publish_snapshot_transaction(authority, Some(&current), &candidate, publication)
}

fn transact_move_workspace_and_register_window(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    workspace_id: &str,
    target_window_id: &str,
    focus: bool,
) -> Result<AppSessionSnapshot, MoveWorkspaceToWindowControlError> {
    let _transaction_guard = authority.lock_gate();
    let before = transaction_current_snapshot(authority)
        .map_err(MoveWorkspaceToWindowControlError::Publication)?;
    if !before.windows.iter().any(|window| {
        window
            .tab_manager
            .workspaces
            .iter()
            .any(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
    }) {
        return Err(MoveWorkspaceToWindowControlError::NotFound);
    }
    let mut candidate = before.clone();
    let mut used = 0;
    if !candidate
        .windows
        .iter()
        .any(|window| window.window_id.as_deref() == Some(target_window_id))
    {
        let panel_id = Uuid::new_v4().to_string();
        used += 1;
        candidate
            .windows
            .push(auxiliary_window_snapshot(target_window_id, &panel_id));
    }
    let bootstrap_panel_id = Uuid::new_v4().to_string();
    used += 1;
    session_ops::move_workspace_to_window(
        &mut candidate,
        workspace_id,
        target_window_id,
        session_ops::fresh_terminal_workspace(&bootstrap_panel_id),
        focus,
    )
    .map_err(|_| MoveWorkspaceToWindowControlError::NotFound)?;
    ensure_workspace_ids(&mut candidate);
    ensure_pane_ids(&mut candidate);
    let committed = publish_snapshot_transaction(authority, Some(&before), &candidate, publication)
        .map_err(MoveWorkspaceToWindowControlError::Publication)?;
    next_panel.fetch_add(used, Ordering::Relaxed);
    Ok(committed)
}

pub(crate) fn register_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_id: &str,
) -> Result<RegisterWindowOutcome, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::with_deferred_next_panel_reseed(app, state);
    transact_register_window(
        &state.snapshot,
        &state.next_panel,
        &mut publication,
        window_id,
    )
}

pub(crate) fn unregister_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_id: &str,
) -> Result<UnregisterWindowOutcome, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Record);
    transact_unregister_window(&state.snapshot, &mut publication, window_id)
}

pub(crate) fn restore_removed_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    lease: &RemovedWindowLease,
) -> Result<AppSessionSnapshot, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Record);
    transact_restore_removed_window(&state.snapshot, &mut publication, lease)
}

pub(crate) fn move_workspace_to_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_id: &str,
    target_window_id: &str,
    focus: bool,
) -> Result<AppSessionSnapshot, MoveWorkspaceToWindowControlError> {
    let mut publication =
        ProductionSnapshotPublicationOperations::with_deferred_next_panel_reseed(app, state);
    publication.derived_events = DerivedEventPolicy::Suppress;
    transact_move_workspace_and_register_window(
        &state.snapshot,
        &state.next_panel,
        &mut publication,
        workspace_id,
        target_window_id,
        focus,
    )
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
    let _ = persist_current_snapshot(app, &snapshot);
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
    let _ = persist_current_snapshot(app, &snapshot);
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
    let _proxy_guard = state.snapshot.lock_gate();
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
    let _proxy_guard = state.snapshot.lock_gate();
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
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_always(app, |snapshot| apply_select_workspace(snapshot, index))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceSelectControlError {
    WindowNotFound,
}

pub(crate) fn select_workspace_in_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    workspace_index: usize,
    event_policy: DerivedEventPolicy,
) -> Result<AppSessionSnapshot, PaneTopologyControlError<WorkspaceSelectControlError>> {
    let mut publication = ProductionSnapshotPublicationOperations::new(app, state, event_policy);
    let ((), snapshot) =
        transact_value_if_changed_snapshot(&state.snapshot, &mut publication, |snapshot| {
            select_workspace_in_window_candidate(snapshot, window_index, workspace_index)
        })?;
    Ok(snapshot)
}

pub(crate) fn equalize_dividers_for_control(
    app: &AppHandle,
    state: &SessionState,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_always(app, apply_equalize_dividers)
}

#[derive(Debug)]
struct DeferredPanelIds {
    used: u64,
}

impl DeferredPanelIds {
    fn new(_counter: &AtomicU64) -> Self {
        Self { used: 0 }
    }

    fn next(&mut self) -> String {
        // D2: canonical surface/panel ids are UUIDs (live capture); the
        // legacy counter remains only as a transaction fence.
        self.used += 1;
        Uuid::new_v4().to_string()
    }

    fn commit(self, counter: &AtomicU64) {
        counter.fetch_add(self.used, Ordering::Relaxed);
    }
}

fn transact_new_workspace(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    current_directory: Option<&str>,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, String> {
    let _transaction_guard = authority.lock_gate();
    let current = transaction_current_snapshot(authority)?;
    let mut ids = DeferredPanelIds::new(next_panel);
    let new_panel_id = ids.next();
    let mut candidate = current.clone();
    apply_new_workspace(
        &mut candidate,
        &new_panel_id,
        current_directory,
        initial_terminal_command,
        initial_terminal_input,
        initial_terminal_environment,
    );
    let committed =
        publish_snapshot_transaction(authority, Some(&current), &candidate, publication)?;
    ids.commit(next_panel);
    Ok(committed)
}

pub(crate) fn new_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    current_directory: Option<&str>,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::with_deferred_next_panel_reseed(app, state);
    transact_new_workspace(
        &state.snapshot,
        &state.next_panel,
        &mut publication,
        current_directory,
        initial_terminal_command,
        initial_terminal_input,
        initial_terminal_environment,
    )
}

#[allow(clippy::too_many_arguments)]
fn transact_new_workspace_in_window(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    window_index: usize,
    current_directory: Option<&str>,
    initial_terminal_command: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
    title: Option<&str>,
    description: Option<&str>,
    workspace_environment: Option<BTreeMap<String, String>>,
    group_id: Option<&str>,
    layout: Option<CmuxLayoutNode>,
    group_insert_index: Option<usize>,
    focus: bool,
) -> Result<Option<(AppSessionSnapshot, usize)>, String> {
    let _transaction_guard = authority.lock_gate();
    let current = transaction_current_snapshot(authority)?;
    if current.windows.get(window_index).is_none() {
        return Ok(None);
    }
    let mut ids = DeferredPanelIds::new(next_panel);
    let new_panel_id = ids.next();
    let mut candidate = current.clone();
    let created_index = {
        let tabs = &mut candidate
            .windows
            .get_mut(window_index)
            .expect("validated window")
            .tab_manager;
        let previous_selected_id = tabs
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| tabs.workspaces.get(index))
            .and_then(|workspace| workspace.workspace_id.clone());
        let previous_selected_index = tabs.selected_workspace_index;
        session_ops::new_workspace(tabs, &new_panel_id);
        let mut created_index = usize::try_from(
            tabs.selected_workspace_index
                .expect("new workspace is selected"),
        )
        .expect("selected workspace index is non-negative");
        if let Some(insert_index) = group_insert_index {
            let workspace = tabs.workspaces.remove(created_index);
            let insert_index = insert_index.min(tabs.workspaces.len());
            tabs.workspaces.insert(insert_index, workspace);
            created_index = insert_index;
            tabs.selected_workspace_index = Some(created_index as i64);
        }
        let workspace = tabs
            .workspaces
            .get_mut(created_index)
            .expect("selected workspace exists");
        if let Some(current_directory) = current_directory {
            workspace.current_directory = Some(current_directory.to_owned());
        }
        workspace.initial_terminal_command = initial_terminal_command.map(str::to_owned);
        let initial_terminal_environment = initial_terminal_environment.unwrap_or_default();
        let mut effective_environment = workspace_environment.clone().unwrap_or_default();
        effective_environment.extend(initial_terminal_environment.clone());
        workspace.initial_terminal_environment =
            (!effective_environment.is_empty()).then_some(effective_environment);
        workspace.custom_title = title.map(str::to_owned);
        workspace.custom_title_source = title.map(|_| "user".to_string());
        workspace.custom_description = description.map(str::to_owned);
        workspace.group_id = group_id.map(str::to_owned);
        workspace.workspace_environment =
            workspace_environment.filter(|environment| !environment.is_empty());
        if let Some(layout) = layout {
            let (layout, focused_panel_id, mut terminal_startups) =
                match session_layout_from_cmux(layout, &mut ids) {
                    Some(layout) => layout,
                    None => return Ok(None),
                };
            for startup in &mut terminal_startups {
                let mut environment = workspace.workspace_environment.clone().unwrap_or_default();
                environment.extend(initial_terminal_environment.clone());
                environment.extend(
                    startup
                        .initial_terminal_environment
                        .take()
                        .unwrap_or_default(),
                );
                startup.initial_terminal_environment =
                    (!environment.is_empty()).then_some(environment);
            }
            workspace.layout = Some(layout);
            workspace.focused_panel_id = focused_panel_id;
            workspace.panel_terminal_startups =
                (!terminal_startups.is_empty()).then_some(terminal_startups);
            workspace.initial_terminal_command = None;
            workspace.initial_terminal_environment = None;
        }
        // The initial surface must carry its requested directory; layout-less
        // creations are seeded after stable ids are minted below.
        if !focus {
            tabs.selected_workspace_index = previous_selected_id
                .as_deref()
                .and_then(|workspace_id| {
                    tabs.workspaces.iter().position(|workspace| {
                        workspace.workspace_id.as_deref() == Some(workspace_id)
                    })
                })
                .map(|index| index as i64)
                .or(previous_selected_index);
        }
        created_index
    };
    ensure_workspace_ids(&mut candidate);
    ensure_pane_ids(&mut candidate);
    if let Some(workspace) = candidate
        .windows
        .get_mut(window_index)
        .and_then(|window| window.tab_manager.workspaces.get_mut(created_index))
    {
        seed_initial_surface_record(workspace);
    }
    let committed =
        publish_snapshot_transaction(authority, Some(&current), &candidate, publication)?;
    ids.commit(next_panel);
    Ok(Some((committed, created_index)))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn new_workspace_in_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    current_directory: Option<&str>,
    initial_terminal_command: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
    title: Option<&str>,
    description: Option<&str>,
    workspace_environment: Option<BTreeMap<String, String>>,
    group_id: Option<&str>,
    layout: Option<CmuxLayoutNode>,
    group_insert_index: Option<usize>,
    focus: bool,
    event_policy: DerivedEventPolicy,
) -> Result<Option<(AppSessionSnapshot, usize)>, String> {
    let mut publication = ProductionSnapshotPublicationOperations::new(app, state, event_policy);
    publication.reseed_next_panel = false;
    transact_new_workspace_in_window(
        &state.snapshot,
        &state.next_panel,
        &mut publication,
        window_index,
        current_directory,
        initial_terminal_command,
        initial_terminal_environment,
        title,
        description,
        workspace_environment,
        group_id,
        layout,
        group_insert_index,
        focus,
    )
}

#[derive(Debug, Clone, PartialEq)]
struct ClosedWorkspaceSnapshot {
    window_id: Option<String>,
    workspace: SessionWorkspaceSnapshot,
    original_index: usize,
}

fn closed_workspace_snapshot(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) -> Option<ClosedWorkspaceSnapshot> {
    let window = snapshot.windows.get(window_index)?;
    Some(ClosedWorkspaceSnapshot {
        window_id: window.window_id.clone(),
        workspace: window.tab_manager.workspaces.get(workspace_index)?.clone(),
        original_index: workspace_index,
    })
}

fn apply_reopen_closed_workspace(
    snapshot: &mut AppSessionSnapshot,
    closed: ClosedWorkspaceSnapshot,
) -> bool {
    let window_index = closed
        .window_id
        .as_deref()
        .and_then(|window_id| {
            snapshot
                .windows
                .iter()
                .position(|window| window.window_id.as_deref() == Some(window_id))
        })
        .or_else(|| (!snapshot.windows.is_empty()).then_some(0));
    let Some(window_index) = window_index else {
        return false;
    };
    let window = &mut snapshot.windows[window_index];
    if window.tab_manager.workspaces.iter().any(|workspace| {
        workspace.workspace_id.is_some() && workspace.workspace_id == closed.workspace.workspace_id
    }) {
        return false;
    }
    let index = closed
        .original_index
        .min(window.tab_manager.workspaces.len());
    let workspace_id = closed.workspace.workspace_id.clone();
    window
        .tab_manager
        .workspaces
        .insert(index, closed.workspace);
    window.tab_manager.selected_workspace_index = Some(index as i64);
    window.selected_workspace_id = workspace_id;
    ensure_workspace_ids(snapshot);
    ensure_pane_ids(snapshot);
    true
}

fn transact_reopen_closed_workspace(
    authority: &GatedSnapshot,
    history: &Mutex<Vec<ClosedWorkspaceSnapshot>>,
    publication: &mut impl SnapshotPublicationOperations,
) -> Result<Option<AppSessionSnapshot>, String> {
    let _transaction_guard = authority.lock_gate();
    let mut history = history
        .lock()
        .expect("closed workspace history mutex poisoned");
    let current = transaction_current_snapshot(authority)?;
    let Some(closed) = history.last().cloned() else {
        return Ok(None);
    };
    let mut candidate = current.clone();
    if !apply_reopen_closed_workspace(&mut candidate, closed) {
        return Ok(None);
    }
    let committed =
        publish_snapshot_transaction(authority, Some(&current), &candidate, publication)?;
    history.pop();
    Ok(Some(committed))
}

pub(crate) fn reopen_closed_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
) -> Result<Option<AppSessionSnapshot>, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Record);
    transact_reopen_closed_workspace(&state.snapshot, &state.closed_workspaces, &mut publication)
}

fn remote_workspace_rename_intent(
    workspace: &SessionWorkspaceSnapshot,
    title: &str,
) -> Option<(String, String)> {
    if !workspace.remote.as_ref().is_some_and(|remote| {
        remote.enabled && remote.connected && remote.transport.as_deref() == Some("tmux")
    }) {
        return None;
    }
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    workspace
        .workspace_id
        .clone()
        .map(|workspace_id| (workspace_id, title.to_string()))
}

fn remote_workspace_rename_request(
    state: &SessionState,
    workspace: &SessionWorkspaceSnapshot,
    title: &str,
) -> Option<RemoteWorkspaceRenameRequest> {
    let (workspace_id, title) = remote_workspace_rename_intent(workspace, title)?;
    let config = state
        .remote_configs
        .lock()
        .expect("remote config mutex poisoned")
        .get(&workspace_id)
        .cloned()?;
    (config.transport == "ssh").then_some(RemoteWorkspaceRenameRequest {
        workspace_id,
        destination: config.destination,
        port: config.port,
        identity_file: config.identity_file,
        ssh_options: config.ssh_options,
        session: config.persistent_daemon_slot,
        title,
    })
}

fn session_layout_from_cmux(
    node: CmuxLayoutNode,
    ids: &mut DeferredPanelIds,
) -> Option<(
    SessionWorkspaceLayoutSnapshot,
    Option<String>,
    Vec<SessionPanelTerminalStartupSnapshot>,
)> {
    fn build(
        node: CmuxLayoutNode,
        ids: &mut DeferredPanelIds,
        focused: &mut Option<String>,
        startups: &mut Vec<SessionPanelTerminalStartupSnapshot>,
    ) -> Option<SessionWorkspaceLayoutSnapshot> {
        match node {
            CmuxLayoutNode::Pane(pane) => {
                if pane.surfaces.is_empty() {
                    return None;
                }
                let mut panel_ids = Vec::with_capacity(pane.surfaces.len());
                let mut selected_panel_id = None;
                let mut selected_kind = None;
                let mut selected_url = None;
                for surface in pane.surfaces {
                    let panel_id = ids.next();
                    let is_terminal = surface.surface_type == CmuxSurfaceType::Terminal;
                    if selected_panel_id.is_none() || surface.focus == Some(true) {
                        selected_panel_id = Some(panel_id.clone());
                        selected_kind = Some(surface.surface_type);
                        selected_url = surface.url.clone();
                    }
                    if surface.focus == Some(true) {
                        *focused = Some(panel_id.clone());
                    }
                    if is_terminal {
                        startups.push(SessionPanelTerminalStartupSnapshot {
                            panel_id: panel_id.clone(),
                            initial_terminal_command: surface.command,
                            initial_terminal_input: None,
                            initial_terminal_environment: surface.env,
                        });
                    }
                    panel_ids.push(panel_id);
                }
                let selected_panel_id = selected_panel_id.or_else(|| panel_ids.first().cloned());
                if focused.is_none() {
                    *focused = selected_panel_id.clone();
                }
                let first_panel = panel_ids.first()?.clone();
                let Some(SessionWorkspaceLayoutSnapshot::Pane(mut pane)) =
                    session_ops::fresh_terminal_workspace(&first_panel).layout
                else {
                    return None;
                };
                pane.panel_ids = panel_ids;
                pane.selected_panel_id = selected_panel_id;
                pane.surface_kind = selected_kind.and_then(|kind| match kind {
                    CmuxSurfaceType::Terminal => None,
                    CmuxSurfaceType::Browser => Some("browser".to_string()),
                    CmuxSurfaceType::Project => Some("project".to_string()),
                });
                pane.browser_url = selected_url;
                Some(SessionWorkspaceLayoutSnapshot::Pane(pane))
            }
            CmuxLayoutNode::Split(split) => {
                let mut children = split.children.into_iter();
                let first = build(children.next()?, ids, focused, startups)?;
                let second = build(children.next()?, ids, focused, startups)?;
                if children.next().is_some() {
                    return None;
                }
                Some(SessionWorkspaceLayoutSnapshot::Split(
                    cmux_core::session::SessionSplitLayoutSnapshot {
                        split_id: None,
                        orientation: match split.direction {
                            CmuxSplitDirection::Horizontal => SessionSplitOrientation::Horizontal,
                            CmuxSplitDirection::Vertical => SessionSplitOrientation::Vertical,
                        },
                        divider_position: split.split.unwrap_or(0.5).clamp(0.0, 1.0),
                        first: Box::new(first),
                        second: Box::new(second),
                    },
                ))
            }
        }
    }

    let mut focused = None;
    let mut startups = Vec::new();
    let layout = build(node, ids, &mut focused, &mut startups)?;
    Some((layout, focused, startups))
}

pub(crate) fn new_browser_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    url: Option<&str>,
) -> Result<AppSessionSnapshot, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::with_deferred_next_panel_reseed(app, state);
    let mut proxy = ProductionBrowserProxyEffects { app };
    transact_new_browser_workspace(
        &state.snapshot,
        &state.next_panel,
        &mut publication,
        &mut proxy,
        url,
    )
}

pub(crate) fn reopen_closed_browser_tab_for_control(
    app: &AppHandle,
    state: &SessionState,
) -> Result<AppSessionSnapshot, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::with_deferred_next_panel_reseed(app, state);
    let mut proxy = ProductionBrowserProxyEffects { app };
    transact_reopen_closed_browser_tab(
        &state.snapshot,
        &state.next_panel,
        &state.closed_browser_tabs,
        &mut publication,
        &mut proxy,
    )
}

pub(crate) fn close_workspace_in_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    workspace_index: usize,
    event_policy: DerivedEventPolicy,
) -> Option<(AppSessionSnapshot, bool)> {
    let (closed_browser_tabs, closed_workspace, teardown, changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let closed_browser_tabs = guard
            .windows
            .get(window_index)?
            .tab_manager
            .workspaces
            .get(workspace_index)
            .map(closed_browser_tabs_for_workspace)
            .unwrap_or_default();
        let closed_workspace = closed_workspace_snapshot(&guard, window_index, workspace_index);
        let teardown = guard.windows[window_index]
            .tab_manager
            .workspaces
            .get(workspace_index)
            .and_then(|workspace| {
                workspace
                    .workspace_id
                    .as_deref()
                    .map(|workspace_id| workspace_close_teardown_plan(workspace_id, workspace))
            });
        let window = guard.windows.get_mut(window_index)?;
        let changed = session_ops::close_workspace(&mut window.tab_manager, workspace_index as i64);
        if changed {
            sync_window_selected_workspace_id(window);
        }
        (
            closed_browser_tabs,
            closed_workspace,
            teardown,
            changed,
            guard.clone(),
        )
    };
    if changed && !closed_browser_tabs.is_empty() {
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        push_closed_browser_tabs(&mut history, closed_browser_tabs);
    }
    if let (true, Some(closed_workspace)) = (changed, closed_workspace) {
        state
            .closed_workspaces
            .lock()
            .expect("closed workspace history mutex poisoned")
            .push(closed_workspace);
    }
    if let (true, Some(teardown)) = (changed, teardown) {
        apply_workspace_close_teardown(app, state, &teardown);
    }
    if changed {
        notify_session_changed_with_event_policy(app, &snapshot, event_policy);
    }
    Some((snapshot, changed))
}

pub(crate) fn close_workspaces_for_control(
    app: &AppHandle,
    state: &SessionState,
    indices: &[i64],
) -> AppSessionSnapshot {
    let (closed_browser_tabs, closed_workspaces, teardowns, changed, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let mut seen_indices = HashSet::new();
        let candidates: Vec<_> = indices
            .iter()
            .filter(|index| seen_indices.insert(**index))
            .filter_map(|index| {
                if *index < 0 {
                    return None;
                }
                let workspace = guard
                    .windows
                    .first()
                    .and_then(|window| window.tab_manager.workspaces.get(*index as usize))?;
                let workspace_id = workspace.workspace_id.clone();
                Some((
                    workspace_id.clone(),
                    closed_browser_tabs_for_workspace(workspace),
                    closed_workspace_snapshot(&guard, 0, *index as usize),
                    workspace_id
                        .as_deref()
                        .map(|workspace_id| workspace_close_teardown_plan(workspace_id, workspace)),
                ))
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
        let closed: Vec<_> = if changed {
            candidates
                .into_iter()
                .filter(|(workspace_id, _, _, _)| {
                    workspace_id
                        .as_ref()
                        .is_none_or(|id| !remaining_ids.contains(id))
                })
                .collect()
        } else {
            Vec::new()
        };
        let closed_browser_tabs: Vec<ClosedBrowserTabSnapshot> = closed
            .iter()
            .flat_map(|(_, tabs, _, _)| tabs.clone())
            .collect();
        let closed_workspaces: Vec<ClosedWorkspaceSnapshot> = closed
            .iter()
            .filter_map(|(_, _, workspace, _)| workspace.clone())
            .collect();
        let teardowns: Vec<WorkspaceCloseTeardownPlan> = closed
            .into_iter()
            .filter_map(|(_, _, _, teardown)| teardown)
            .collect();
        (
            closed_browser_tabs,
            closed_workspaces,
            teardowns,
            changed,
            guard.clone(),
        )
    };
    if !closed_browser_tabs.is_empty() {
        let mut history = state
            .closed_browser_tabs
            .lock()
            .expect("closed browser history mutex poisoned");
        push_closed_browser_tabs(&mut history, closed_browser_tabs);
    }
    if !closed_workspaces.is_empty() {
        state
            .closed_workspaces
            .lock()
            .expect("closed workspace history mutex poisoned")
            .extend(closed_workspaces);
    }
    for teardown in teardowns {
        apply_workspace_close_teardown(app, state, &teardown);
    }
    if changed {
        notify_session_changed(app, &snapshot);
    }
    snapshot
}

#[derive(Debug)]
struct WorkspaceGroupDeleteArtifacts {
    closed_browser_tabs: Vec<ClosedBrowserTabSnapshot>,
    closed_workspaces: Vec<ClosedWorkspaceSnapshot>,
    teardowns: Vec<WorkspaceCloseTeardownPlan>,
    closed_count: usize,
}

fn apply_delete_workspace_group_candidate(
    candidate: &mut AppSessionSnapshot,
    window_index: usize,
    group_id: &str,
) -> (Option<WorkspaceGroupDeleteArtifacts>, bool) {
    let Some(window) = candidate.windows.get(window_index) else {
        return (None, false);
    };
    let group = window
        .tab_manager
        .workspace_groups
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|group| group.id == group_id);
    let Some(group) = group else {
        return (None, false);
    };
    let anchor_workspace_id = group.anchor_workspace_id.clone();
    let tab_order_member_indices = window
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| {
            (workspace.group_id.as_deref() == Some(group_id)).then_some(index)
        })
        .collect::<Vec<_>>();
    let mut member_indices = tab_order_member_indices
        .iter()
        .copied()
        .filter(|index| {
            window.tab_manager.workspaces[*index]
                .workspace_id
                .as_deref()
                != anchor_workspace_id.as_deref()
        })
        .collect::<Vec<_>>();
    member_indices.extend(tab_order_member_indices.iter().copied().filter(|index| {
        window.tab_manager.workspaces[*index]
            .workspace_id
            .as_deref()
            == anchor_workspace_id.as_deref()
    }));
    let closed_browser_tabs = member_indices
        .iter()
        .flat_map(|index| {
            closed_browser_tabs_for_workspace(
                &candidate.windows[window_index].tab_manager.workspaces[*index],
            )
        })
        .collect::<Vec<_>>();
    let closed_workspaces = member_indices
        .iter()
        .filter_map(|index| closed_workspace_snapshot(candidate, window_index, *index))
        .collect::<Vec<_>>();
    let teardowns = member_indices
        .iter()
        .filter_map(|index| {
            let workspace = &candidate.windows[window_index].tab_manager.workspaces[*index];
            workspace
                .workspace_id
                .as_deref()
                .map(|workspace_id| workspace_close_teardown_plan(workspace_id, workspace))
        })
        .collect::<Vec<_>>();

    if !member_indices.is_empty()
        && member_indices.len() == candidate.windows[window_index].tab_manager.workspaces.len()
    {
        let panel_id = Uuid::new_v4().to_string();
        let inherited_directory = tab_order_member_indices
            .iter()
            .find(|index| {
                candidate.windows[window_index].tab_manager.workspaces[**index]
                    .workspace_id
                    .as_deref()
                    == anchor_workspace_id.as_deref()
            })
            .or_else(|| tab_order_member_indices.first())
            .and_then(|index| {
                candidate.windows[window_index]
                    .tab_manager
                    .workspaces
                    .get(*index)
            })
            .and_then(|workspace| workspace.current_directory.clone());
        session_ops::new_workspace(&mut candidate.windows[window_index].tab_manager, &panel_id);
        let replacement = candidate.windows[window_index]
            .tab_manager
            .workspaces
            .last_mut()
            .expect("new workspace exists");
        replacement.current_directory = inherited_directory;
        replacement.workspace_id = Some(Uuid::new_v4().to_string());
        if let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = replacement.layout.as_mut() {
            pane.pane_id = Some(Uuid::new_v4().to_string());
        }
        seed_initial_surface_record(replacement);
    }

    let original_member_ids = member_indices
        .iter()
        .filter_map(|index| {
            candidate.windows[window_index].tab_manager.workspaces[*index]
                .workspace_id
                .clone()
        })
        .collect::<HashSet<_>>();
    let live_indices = candidate.windows[window_index]
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| {
            workspace
                .workspace_id
                .as_ref()
                .is_some_and(|id| original_member_ids.contains(id))
                .then_some(index as i64)
        })
        .collect::<Vec<_>>();
    let mut changed = session_ops::close_workspaces(
        &mut candidate.windows[window_index].tab_manager,
        &live_indices,
    );
    let tabs = &mut candidate.windows[window_index].tab_manager;
    for workspace in &mut tabs.workspaces {
        if workspace.group_id.as_deref() == Some(group_id) {
            workspace.group_id = None;
            changed = true;
        }
    }
    if let Some(groups) = tabs.workspace_groups.as_mut() {
        let before = groups.len();
        groups.retain(|group| group.id != group_id);
        changed |= groups.len() != before;
    }
    if changed {
        sync_window_selected_workspace_id(&mut candidate.windows[window_index]);
    }
    (
        Some(WorkspaceGroupDeleteArtifacts {
            closed_browser_tabs,
            closed_workspaces,
            teardowns,
            closed_count: member_indices.len(),
        }),
        changed,
    )
}

/// Atomically remove every workspace owned by `group_id` in one window. When
/// the group owns the window's final workspaces, seed one fresh terminal
/// workspace in the same durable candidate before closing the group. Runtime
/// teardown and close-history effects run only after persistence commits.
pub(crate) fn delete_workspace_group_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    group_id: &str,
) -> Result<Option<(AppSessionSnapshot, usize)>, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Suppress);
    let result = transact_value_if_changed_snapshot_with_post_commit(
        &state.snapshot,
        &mut publication,
        |candidate| {
            Ok::<_, std::convert::Infallible>(apply_delete_workspace_group_candidate(
                candidate,
                window_index,
                group_id,
            ))
        },
        |artifacts, _committed| {
            let Some(artifacts) = artifacts.as_mut() else {
                return;
            };
            if !artifacts.closed_browser_tabs.is_empty() {
                push_closed_browser_tabs(
                    &mut state
                        .closed_browser_tabs
                        .lock()
                        .expect("closed browser history mutex poisoned"),
                    std::mem::take(&mut artifacts.closed_browser_tabs),
                );
            }
            if !artifacts.closed_workspaces.is_empty() {
                state
                    .closed_workspaces
                    .lock()
                    .expect("closed workspace history mutex poisoned")
                    .append(&mut artifacts.closed_workspaces);
            }
            for teardown in std::mem::take(&mut artifacts.teardowns) {
                apply_workspace_close_teardown(app, state, &teardown);
            }
        },
    )
    .map_err(collapse_infallible_publication_error)?;
    let (artifacts, snapshot) = result;
    Ok(artifacts.map(|artifacts| (snapshot, artifacts.closed_count)))
}

#[derive(Debug, Default)]
struct WorkspaceActionClosedArtifacts {
    browser_tabs: Vec<ClosedBrowserTabSnapshot>,
    workspaces: Vec<ClosedWorkspaceSnapshot>,
    teardowns: Vec<WorkspaceCloseTeardownPlan>,
}

fn transact_workspace_action_mutation(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    mutation: &crate::workspace_action::WorkspaceActionMutation,
    post_commit: impl FnOnce(&mut WorkspaceActionClosedArtifacts, &AppSessionSnapshot),
) -> Result<(WorkspaceActionClosedArtifacts, AppSessionSnapshot), String> {
    use crate::workspace_action::WorkspaceActionMutation;

    if let WorkspaceActionMutation::Close {
        window_index,
        workspace_indices,
    } = mutation
    {
        return match transact_value_if_changed_snapshot_with_post_commit(
            authority,
            publication,
            |candidate| {
                let candidates = workspace_indices
                    .iter()
                    .filter_map(|index| {
                        let workspace = candidate
                            .windows
                            .get(*window_index)?
                            .tab_manager
                            .workspaces
                            .get(*index)?;
                        let workspace_id = workspace.workspace_id.clone();
                        Some((
                            workspace_id.clone(),
                            closed_browser_tabs_for_workspace(workspace),
                            closed_workspace_snapshot(candidate, *window_index, *index),
                            workspace_id.as_deref().map(|workspace_id| {
                                workspace_close_teardown_plan(workspace_id, workspace)
                            }),
                        ))
                    })
                    .collect::<Vec<_>>();
                let changed =
                    crate::workspace_action::apply_workspace_action_mutation(candidate, mutation);
                let remaining_ids = candidate
                    .windows
                    .get(*window_index)
                    .into_iter()
                    .flat_map(|window| window.tab_manager.workspaces.iter())
                    .filter_map(|workspace| workspace.workspace_id.clone())
                    .collect::<HashSet<_>>();
                let closed = candidates
                    .into_iter()
                    .filter(|(workspace_id, _, _, _)| {
                        workspace_id
                            .as_ref()
                            .is_none_or(|id| !remaining_ids.contains(id))
                    })
                    .collect::<Vec<_>>();
                let artifacts = WorkspaceActionClosedArtifacts {
                    browser_tabs: closed
                        .iter()
                        .flat_map(|(_, tabs, _, _)| tabs.clone())
                        .collect(),
                    workspaces: closed
                        .iter()
                        .filter_map(|(_, _, workspace, _)| workspace.clone())
                        .collect(),
                    teardowns: closed
                        .into_iter()
                        .filter_map(|(_, _, _, teardown)| teardown)
                        .collect(),
                };
                Ok::<_, std::convert::Infallible>((artifacts, changed))
            },
            post_commit,
        ) {
            Ok(value) => Ok(value),
            Err(PaneTopologyControlError::Publication(message)) => Err(message),
            Err(PaneTopologyControlError::Operation(error)) => match error {},
        };
    }

    transact_snapshot_if_changed(authority, publication, |candidate| {
        crate::workspace_action::apply_workspace_action_mutation(candidate, mutation)
    })
    .map(|snapshot| (WorkspaceActionClosedArtifacts::default(), snapshot))
}

#[cfg(test)]
fn transact_workspace_action_and_propagate_remote(
    authority: &GatedSnapshot,
    publication: &mut impl SnapshotPublicationOperations,
    mutation: &crate::workspace_action::WorkspaceActionMutation,
    remote_controller: &dyn RemoteWorkspaceRenameController,
    remote_request: Option<&RemoteWorkspaceRenameRequest>,
    post_commit: impl FnOnce(&mut WorkspaceActionClosedArtifacts, &AppSessionSnapshot),
) -> Result<(WorkspaceActionClosedArtifacts, AppSessionSnapshot), String> {
    let result = transact_workspace_action_mutation(authority, publication, mutation, post_commit)?;
    if let Some(request) = remote_request {
        let _ = dispatch_remote_workspace_rename(remote_controller, request);
    }
    Ok(result)
}

/// Apply one planned `workspace.action` mutation through the durable snapshot
/// transaction seam. Socket completion is recorded separately by the control
/// layer; canonical derived close/reorder lifecycle events and normal Tauri
/// session/window publications remain enabled. Close histories and runtime
/// teardowns run after persistence and authority commit but before publication.
pub(crate) fn apply_workspace_action_for_control_with_post_commit(
    app: &AppHandle,
    state: &SessionState,
    mutation: &crate::workspace_action::WorkspaceActionMutation,
    notification_post_commit: impl FnOnce(),
) -> Result<AppSessionSnapshot, String> {
    use crate::workspace_action::WorkspaceActionMutation;

    let remote_request = if let WorkspaceActionMutation::Rename {
        window_index,
        workspace_index,
        title,
    } = mutation
    {
        let remote_request = {
            let guard = state
                .snapshot
                .lock()
                .map_err(|_| "Session state is unavailable".to_string())?;
            guard
                .windows
                .get(*window_index)
                .and_then(|window| window.tab_manager.workspaces.get(*workspace_index))
                .and_then(|workspace| remote_workspace_rename_request(state, workspace, title))
        };
        remote_request
    } else {
        None
    };

    let mut notification_post_commit = Some(notification_post_commit);
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Record);
    let (_, snapshot) = transact_workspace_action_mutation(
        &state.snapshot,
        &mut publication,
        mutation,
        |artifacts, _| {
            let browser_tabs = std::mem::take(&mut artifacts.browser_tabs);
            if !browser_tabs.is_empty() {
                push_closed_browser_tabs(
                    &mut state
                        .closed_browser_tabs
                        .lock()
                        .expect("closed browser history mutex poisoned"),
                    browser_tabs,
                );
            }
            let workspaces = std::mem::take(&mut artifacts.workspaces);
            if !workspaces.is_empty() {
                state
                    .closed_workspaces
                    .lock()
                    .expect("closed workspace history mutex poisoned")
                    .extend(workspaces);
            }
            for teardown in std::mem::take(&mut artifacts.teardowns) {
                apply_workspace_close_teardown(app, state, &teardown);
            }
            if let Some(post_commit) = notification_post_commit.take() {
                post_commit();
            }
        },
    )?;
    // A no-op session projection may still correspond to unread notification
    // rows that need clearing. There was no publication in that case, so apply
    // the already-infallible store mutation now.
    if let Some(post_commit) = notification_post_commit.take() {
        post_commit();
    }
    if let Some(request) = remote_request {
        state.defer_remote_workspace_rename(request);
    }
    Ok(snapshot)
}

pub(crate) fn apply_workspace_action_for_control(
    app: &AppHandle,
    state: &SessionState,
    mutation: &crate::workspace_action::WorkspaceActionMutation,
) -> Result<AppSessionSnapshot, String> {
    apply_workspace_action_for_control_with_post_commit(app, state, mutation, || {})
}

pub(crate) fn rename_workspace_in_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    workspace_index: usize,
    title: &str,
    event_policy: DerivedEventPolicy,
) -> Result<Option<AppSessionSnapshot>, String> {
    let remote_request = {
        let guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let Some(workspace) = guard
            .windows
            .get(window_index)
            .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
        else {
            return Ok(None);
        };
        remote_workspace_rename_request(state, workspace, title)
    };
    let (resolution, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let Some(window) = guard.windows.get_mut(window_index) else {
            return Ok(None);
        };
        let tabs = &mut window.tab_manager;
        let resolution = rename_workspace_resolution(tabs, workspace_index, title);
        (resolution, guard.clone())
    };
    if resolution == WorkspaceRenameResolution::NotFound {
        return Ok(None);
    }
    if resolution == WorkspaceRenameResolution::ResolvedChanged {
        notify_session_changed_with_event_policy(app, &snapshot, event_policy);
        if let Some(request) = remote_request {
            state.defer_remote_workspace_rename(request);
        }
    }
    Ok(Some(snapshot))
}

#[derive(Debug)]
pub(crate) enum TerminalPanelCreateError {
    NotFound(String),
    Publication(String),
}

impl std::fmt::Display for TerminalPanelCreateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(message) | Self::Publication(message) => formatter.write_str(message),
        }
    }
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
) -> Result<AppSessionSnapshot, TerminalPanelCreateError> {
    let control_mutation = state.lock_control_mutation();
    let _control_guard = control_mutation.map_err(TerminalPanelCreateError::Publication)?;
    let mut validation = state
        .snapshot_for_lifecycle()
        .map_err(TerminalPanelCreateError::Publication)?;
    if !apply_split_with_terminal_startup(
        &mut validation,
        panel_id,
        orientation.clone(),
        "surface-validation",
        insert_first,
        initial_terminal_command,
        initial_terminal_input,
        initial_terminal_environment.clone(),
    ) {
        return Err(TerminalPanelCreateError::NotFound(format!(
            "no pane holds panel id {panel_id}"
        )));
    }
    // D2: canonical surface ids are UUIDs (live capture); the counter stays
    // only as a legacy fence.
    state.next_panel.fetch_add(1, Ordering::Relaxed);
    let new_panel_id = Uuid::new_v4().to_string();
    let transaction = state.transact_lifecycle(app, |snapshot| {
        if !apply_split_with_terminal_startup(
            snapshot,
            panel_id,
            orientation,
            &new_panel_id,
            insert_first,
            initial_terminal_command,
            initial_terminal_input,
            initial_terminal_environment,
        ) {
            return Err(format!(
                "validated pane disappeared for panel id {panel_id}"
            ));
        }
        Ok(())
    });
    transaction
        .map(|(_, snapshot)| snapshot)
        .map_err(TerminalPanelCreateError::Publication)
}

pub(crate) fn split_off_surface_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    workspace_index: usize,
    panel_id: &str,
    orientation: SessionSplitOrientation,
    insert_first: bool,
    focus: bool,
) -> Result<AppSessionSnapshot, PaneTopologyControlError<session_ops::SplitOffSurfaceError>> {
    let transaction = state.transact_pane_topology(app, |snapshot| {
        let workspace = snapshot
            .windows
            .get_mut(window_index)
            .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
            .ok_or(session_ops::SplitOffSurfaceError::SurfaceNotFound)?;
        session_ops::split_off_surface(workspace, panel_id, orientation, insert_first)?;
        if focus {
            snapshot.windows[window_index]
                .tab_manager
                .selected_workspace_index = Some(workspace_index as i64);
            sync_window_selected_workspace_id(&mut snapshot.windows[window_index]);
        }
        ensure_pane_ids(snapshot);
        Ok(())
    });
    transaction.map(|(_, snapshot)| snapshot)
}

#[derive(Debug)]
pub(crate) enum PaneLastControlError {
    WorkspaceNotFound,
    Pane(session_ops::PaneLastError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneFocusControlError {
    WorkspaceNotFound,
    PaneNotFound,
}

fn apply_focus_pane(
    snapshot: &mut AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    pane_id: &str,
) -> Result<(), PaneFocusControlError> {
    let workspace = snapshot
        .windows
        .get_mut(window_index)
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
        .ok_or(PaneFocusControlError::WorkspaceNotFound)?;
    let target = session_ops::focus_pane_target(workspace, pane_id)
        .map_err(|_| PaneFocusControlError::PaneNotFound)?;
    workspace.focused_panel_id = target.surface_id;
    snapshot.windows[window_index]
        .tab_manager
        .selected_workspace_index = Some(workspace_index as i64);
    sync_window_selected_workspace_id(&mut snapshot.windows[window_index]);
    Ok(())
}

pub(crate) fn focus_pane_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    workspace_index: usize,
    pane_id: &str,
) -> Result<AppSessionSnapshot, PaneTopologyControlError<PaneFocusControlError>> {
    let ((), snapshot) = state.transact_value_if_changed(app, |snapshot| {
        let before = snapshot.clone();
        apply_focus_pane(snapshot, window_index, workspace_index, pane_id)?;
        Ok(((), *snapshot != before))
    })?;
    Ok(snapshot)
}

pub(crate) fn focus_last_pane_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    workspace_index: usize,
) -> Result<
    (session_ops::PaneLastResult, AppSessionSnapshot),
    PaneTopologyControlError<PaneLastControlError>,
> {
    state.transact_value_if_changed(app, |snapshot| {
        let before = snapshot.clone();
        let workspace = snapshot
            .windows
            .get_mut(window_index)
            .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
            .ok_or(PaneLastControlError::WorkspaceNotFound)?;
        let focused_pane_id = workspace.focused_panel_id.as_deref().and_then(|panel_id| {
            session_ops::pane_id_containing_surface(workspace, panel_id).map(str::to_string)
        });
        let focused = session_ops::focus_alternate_pane(workspace, focused_pane_id.as_deref())
            .map_err(PaneLastControlError::Pane)?;
        workspace.focused_panel_id = focused.surface_id.clone();
        snapshot.windows[window_index]
            .tab_manager
            .selected_workspace_index = Some(workspace_index as i64);
        sync_window_selected_workspace_id(&mut snapshot.windows[window_index]);
        Ok((focused, *snapshot != before))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkspaceLastControlError {
    TabManagerUnavailable,
    NoPreviousWorkspace,
}

pub(crate) fn select_last_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    event_policy: DerivedEventPolicy,
) -> Result<(String, AppSessionSnapshot), WorkspaceLastControlError> {
    let (workspace_id, snapshot) = {
        let mut guard = state
            .snapshot
            .lock()
            .expect("session snapshot mutex poisoned");
        let window = guard
            .windows
            .get_mut(window_index)
            .ok_or(WorkspaceLastControlError::TabManagerUnavailable)?;
        let window_key = window
            .window_id
            .clone()
            .unwrap_or_else(|| "main".to_string());
        let valid_ids = window
            .tab_manager
            .workspaces
            .iter()
            .filter_map(|workspace| workspace.workspace_id.as_deref())
            .collect::<HashSet<_>>();
        let workspace_id = state
            .workspace_focus_history
            .lock()
            .expect("workspace focus history mutex poisoned")
            .get_mut(&window_key)
            .and_then(|history| history.navigate_back(&valid_ids))
            .ok_or(WorkspaceLastControlError::NoPreviousWorkspace)?;
        let index = window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
            .ok_or(WorkspaceLastControlError::NoPreviousWorkspace)?;
        window.tab_manager.selected_workspace_index = Some(index as i64);
        sync_window_selected_workspace_id(window);
        (workspace_id, guard.clone())
    };
    notify_session_changed_with_event_policy(app, &snapshot, event_policy);
    Ok((workspace_id, snapshot))
}

#[derive(Debug, Clone)]
pub(crate) enum PaneResizeControlIntent {
    Relative {
        direction: session_ops::PaneResizeDirection,
        amount: u64,
    },
    Absolute {
        axis: SessionSplitOrientation,
        target_pixels: f64,
    },
}

#[derive(Debug)]
pub(crate) enum PaneResizeControlError {
    WorkspaceNotFound,
    Pane(session_ops::PaneResizeError),
}

pub(crate) fn resize_pane_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    workspace_index: usize,
    pane_id: &str,
    intent: PaneResizeControlIntent,
    width: f64,
    height: f64,
) -> Result<
    (session_ops::PaneResizeResult, AppSessionSnapshot),
    PaneTopologyControlError<PaneResizeControlError>,
> {
    state.transact_pane_topology(app, |snapshot| {
        let workspace = snapshot
            .windows
            .get_mut(window_index)
            .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
            .ok_or(PaneResizeControlError::WorkspaceNotFound)?;
        let resized = match intent {
            PaneResizeControlIntent::Relative { direction, amount } => {
                session_ops::resize_pane_relative(
                    workspace, pane_id, direction, amount, width, height,
                )
            }
            PaneResizeControlIntent::Absolute {
                axis,
                target_pixels,
            } => session_ops::resize_pane_absolute(
                workspace,
                pane_id,
                axis,
                target_pixels,
                width,
                height,
            ),
        }
        .map_err(PaneResizeControlError::Pane)?;
        Ok(resized)
    })
}

pub(crate) fn new_terminal_tab_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    initial_terminal_command: Option<&str>,
    initial_terminal_input: Option<&str>,
    initial_terminal_environment: Option<BTreeMap<String, String>>,
) -> Result<AppSessionSnapshot, TerminalPanelCreateError> {
    let control_mutation = state.lock_control_mutation();
    let _control_guard = control_mutation.map_err(TerminalPanelCreateError::Publication)?;
    let mut validation = state
        .snapshot_for_lifecycle()
        .map_err(TerminalPanelCreateError::Publication)?;
    if !apply_new_terminal_tab(
        &mut validation,
        panel_id,
        "surface-validation",
        initial_terminal_command,
        initial_terminal_input,
        initial_terminal_environment.clone(),
    ) {
        return Err(TerminalPanelCreateError::NotFound(format!(
            "no pane holds panel id {panel_id}"
        )));
    }
    // D2: canonical surface ids are UUIDs (live capture); the counter stays
    // only as a legacy fence.
    state.next_panel.fetch_add(1, Ordering::Relaxed);
    let new_panel_id = Uuid::new_v4().to_string();
    let transaction = state.transact_lifecycle(app, |snapshot| {
        if !apply_new_terminal_tab(
            snapshot,
            panel_id,
            &new_panel_id,
            initial_terminal_command,
            initial_terminal_input,
            initial_terminal_environment,
        ) {
            return Err(format!(
                "validated pane disappeared for panel id {panel_id}"
            ));
        }
        Ok(())
    });
    transaction
        .map(|(_, snapshot)| snapshot)
        .map_err(TerminalPanelCreateError::Publication)
}

pub(crate) fn split_browser_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    orientation: SessionSplitOrientation,
    insert_first: bool,
    url: Option<&str>,
) -> Result<AppSessionSnapshot, BrowserPanelCreateError> {
    let mut publication =
        ProductionSnapshotPublicationOperations::with_deferred_next_panel_reseed(app, state);
    let mut proxy = ProductionBrowserProxyEffects { app };
    transact_split_browser(
        &state.snapshot,
        &state.next_panel,
        &mut publication,
        &mut proxy,
        panel_id,
        orientation,
        insert_first,
        url,
    )
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
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_always(app, |snapshot| {
        apply_set_surface_kind(snapshot, panel_id, kind)
    })
}

pub(crate) fn select_adjacent_panel_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    next: bool,
) -> Result<AppSessionSnapshot, PaneTopologyControlError<std::convert::Infallible>> {
    let ((), snapshot) = state.transact_value_if_changed(app, |snapshot| {
        let changed = apply_select_adjacent_panel(snapshot, panel_id, next);
        Ok(((), changed))
    })?;
    Ok(snapshot)
}

pub(crate) fn toggle_split_zoom_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| apply_toggle_split_zoom(snapshot, panel_id))
}

pub(crate) fn move_panel_to_new_workspace_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_move_panel_to_new_workspace(snapshot, panel_id)
    })
}

pub(crate) fn set_workspace_description_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    description: &str,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_workspace_description(snapshot, index, description)
    })
}

pub(crate) fn reset_workspace_color_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| apply_reset_workspace_color(snapshot, index))
}

pub(crate) fn set_workspace_sidebar_progress_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    value: f64,
    label: Option<&str>,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_workspace_sidebar_progress(snapshot, index, value, label)
    })
}

pub(crate) fn clear_workspace_sidebar_progress_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_clear_workspace_sidebar_progress(snapshot, index)
    })
}

pub(crate) fn set_workspace_sidebar_status_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
    value: &str,
    priority: Option<i64>,
) -> Result<AppSessionSnapshot, String> {
    let now = current_unix_timestamp_seconds();
    state.transact_snapshot_if_changed(app, |snapshot| {
        let changed_status =
            apply_set_workspace_sidebar_status(snapshot, index, key, value, priority, now);
        let changed_metadata = apply_set_workspace_sidebar_metadata(
            snapshot, index, key, value, None, None, None, priority, None, now,
        );
        changed_status || changed_metadata
    })
}

pub(crate) fn clear_workspace_sidebar_status_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        let changed_status = apply_clear_workspace_sidebar_status(snapshot, index, key);
        let changed_metadata = apply_clear_workspace_sidebar_metadata(snapshot, index, key);
        changed_status || changed_metadata
    })
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
) -> Result<AppSessionSnapshot, String> {
    let now = current_unix_timestamp_seconds();
    state.transact_snapshot_if_changed(app, |snapshot| {
        let changed_status =
            apply_set_workspace_sidebar_status(snapshot, index, key, value, priority, now);
        let changed_metadata = apply_set_workspace_sidebar_metadata(
            snapshot, index, key, value, icon, color, url, priority, format, now,
        );
        changed_status || changed_metadata
    })
}

pub(crate) fn clear_workspace_sidebar_metadata_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        let changed_status = apply_clear_workspace_sidebar_status(snapshot, index, key);
        let changed_metadata = apply_clear_workspace_sidebar_metadata(snapshot, index, key);
        changed_status || changed_metadata
    })
}

pub(crate) fn set_workspace_sidebar_metadata_block_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
    markdown: &str,
    priority: Option<i64>,
) -> Result<AppSessionSnapshot, String> {
    let now = current_unix_timestamp_seconds();
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_workspace_sidebar_metadata_block(snapshot, index, key, markdown, priority, now)
    })
}

pub(crate) fn clear_workspace_sidebar_metadata_block_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    key: &str,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_clear_workspace_sidebar_metadata_block(snapshot, index, key)
    })
}

pub(crate) fn reset_workspace_sidebar_metadata_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_reset_workspace_sidebar_metadata(snapshot, index)
    })
}

pub(crate) fn append_workspace_sidebar_log_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    message: &str,
    level: &str,
) -> Result<AppSessionSnapshot, String> {
    let now = current_unix_timestamp_seconds();
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_append_workspace_sidebar_log(snapshot, index, message, level, now)
    })
}

pub(crate) fn clear_workspace_sidebar_log_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_clear_workspace_sidebar_log(snapshot, index)
    })
}

pub(crate) fn set_workspace_unread_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    preferred_panel_id: Option<&str>,
    unread: bool,
) -> Result<AppSessionSnapshot, String> {
    let unread_at = current_unix_timestamp_seconds();
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_workspace_unread_at(snapshot, index, preferred_panel_id, unread, unread_at)
    })
}

pub(crate) fn set_workspace_pinned_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    pinned: bool,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_workspace_pinned(snapshot, index, pinned)
    })
}

pub(crate) fn reorder_workspaces_for_control(
    app: &AppHandle,
    state: &SessionState,
    index: i64,
    to_index: i64,
    uses_top_level_rows: bool,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_reorder_workspaces(snapshot, index, to_index, uses_top_level_rows)
    })
}

pub(crate) fn reorder_surface_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
    destination_index: i64,
    focus: bool,
) -> Result<AppSessionSnapshot, PaneTopologyControlError<SurfacePositionControlError>> {
    state.transact_result_if_changed(app, |snapshot| {
        let workspace = snapshot
            .windows
            .first_mut()
            .ok_or(SurfacePositionControlError::InvalidRequest)?
            .tab_manager
            .workspaces
            .get_mut(workspace_index)
            .ok_or(SurfacePositionControlError::InvalidRequest)?;
        session_ops::reorder_surface(workspace, panel_id, destination_index, focus)
            .ok_or(SurfacePositionControlError::InvalidRequest)
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfacePositionControlError {
    InvalidRequest,
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
) -> Result<AppSessionSnapshot, PaneTopologyControlError<SurfacePositionControlError>> {
    state.transact_result_if_changed(app, |snapshot| {
        let tabs = &mut snapshot
            .windows
            .first_mut()
            .ok_or(SurfacePositionControlError::InvalidRequest)?
            .tab_manager;
        session_ops::move_surface(
            tabs,
            source_workspace_index,
            panel_id,
            target_workspace_index,
            target_pane_id,
            destination_index,
            focus,
        )
        .ok_or(SurfacePositionControlError::InvalidRequest)
    })
}

pub(crate) fn set_group_collapsed_for_control(
    app: &AppHandle,
    state: &SessionState,
    group_id: &str,
    collapsed: bool,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_group_collapsed(snapshot, group_id, collapsed)
    })
}

pub(crate) fn set_panel_title_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    title: &str,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_panel_title(snapshot, panel_id, title)
    })
}

pub(crate) fn set_process_title_for_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    title: &str,
) -> Result<AppSessionSnapshot, String> {
    let panel_id = panel_id.trim();
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_process_title(snapshot, panel_id, title)
    })
}

pub(crate) fn set_panel_pinned_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    pinned: bool,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_panel_pinned(snapshot, panel_id, pinned)
    })
}

pub(crate) fn set_panel_unread_for_control(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    unread: bool,
) -> Result<AppSessionSnapshot, String> {
    let unread_at = current_unix_timestamp_seconds();
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_panel_unread_at(snapshot, panel_id, unread, unread_at)
    })
}

pub(crate) fn set_panel_listening_ports_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
    ports: &[u16],
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_panel_listening_ports(snapshot, workspace_index, panel_id, ports)
    })
}

pub(crate) fn set_panel_tty_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
    tty: &str,
) -> Result<AppSessionSnapshot, String> {
    let updated_at = current_unix_timestamp_seconds();
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_panel_tty_at(snapshot, workspace_index, panel_id, tty, updated_at)
    })
}

pub(crate) fn set_panel_shell_activity_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
    shell_activity: SessionPanelShellActivityStateSnapshot,
) -> Result<AppSessionSnapshot, String> {
    let updated_at = current_unix_timestamp_seconds();
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_panel_shell_activity_at(
            snapshot,
            workspace_index,
            panel_id,
            shell_activity,
            updated_at,
        )
    })
}

pub(crate) fn set_panel_listening_ports_for_panel(
    app: &AppHandle,
    state: &SessionState,
    panel_id: &str,
    ports: &[u16],
) -> Result<AppSessionSnapshot, String> {
    let panel_id = panel_id.trim();
    state.transact_snapshot_if_changed(app, |snapshot| {
        let workspace_index = snapshot.windows.first().and_then(|window| {
            window.tab_manager.workspaces.iter().position(|workspace| {
                workspace
                    .layout
                    .as_ref()
                    .is_some_and(|layout| session_ops::contains_panel(layout, panel_id))
            })
        });
        workspace_index
            .map(|index| apply_set_panel_listening_ports(snapshot, index, panel_id, ports))
            .unwrap_or(false)
    })
}

pub(crate) fn set_workspace_agent_listening_ports_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    ports: &[u16],
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_workspace_agent_listening_ports(snapshot, workspace_index, ports)
    })
}

pub(crate) fn set_workspace_agent_pid_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    key: &str,
    pid: u32,
) -> Result<AppSessionSnapshot, String> {
    let updated_at = current_unix_timestamp_seconds();
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_workspace_agent_pid_at(snapshot, workspace_index, key, pid, updated_at)
    })
}

pub(crate) fn clear_workspace_agent_pid_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    key: &str,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_clear_workspace_agent_pid(snapshot, workspace_index, key)
    })
}

pub(crate) fn set_workspace_git_facts_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    git_branch: Option<SessionGitBranchSnapshot>,
    panel_git_branches: Vec<SessionPanelGitBranchSnapshot>,
    panel_pull_requests: Vec<SessionPanelPullRequestSnapshot>,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_workspace_git_facts(
            snapshot,
            workspace_index,
            git_branch,
            panel_git_branches,
            panel_pull_requests,
        )
    })
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
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_set_workspace_panel_pull_request(
            snapshot,
            workspace_index,
            panel_id,
            number,
            label,
            url,
            status,
            branch,
            is_stale,
        )
    })
}

pub(crate) fn clear_workspace_panel_pull_request_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_index: usize,
    panel_id: &str,
) -> Result<AppSessionSnapshot, String> {
    state.transact_snapshot_if_changed(app, |snapshot| {
        apply_clear_workspace_panel_pull_request(snapshot, workspace_index, panel_id)
    })
}
