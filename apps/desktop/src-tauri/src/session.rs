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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use cmux_core::session::{
    AppSessionSnapshot, SessionSplitOrientation, SessionTabManagerSnapshot, SessionWindowSnapshot,
    SessionWorkspaceLayoutSnapshot, SESSION_SNAPSHOT_SCHEMA_VERSION,
};
use cmux_core::session_ops::{self, CloseOutcome, SplitChild};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// Event carrying the full session snapshot after any structural change.
const SESSION_CHANGED_EVENT: &str = "cmux://session-changed";

/// The panel id of the very first pane. Later panes are `surface-N` (N from the
/// [`SessionState`] counter).
const FIRST_PANEL_ID: &str = "surface-1";

/// Managed Tauri state: the authoritative session snapshot + a monotonic panel
/// id counter so every new pane gets a unique, stable id.
pub struct SessionState {
    snapshot: Mutex<AppSessionSnapshot>,
    next_panel: AtomicU64,
}

impl Default for SessionState {
    fn default() -> Self {
        Self {
            snapshot: Mutex::new(initial_snapshot(FIRST_PANEL_ID)),
            next_panel: AtomicU64::new(2),
        }
    }
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

/// The `Option<layout>` slot of the currently-selected workspace of the first
/// window, or `None` if there is no window/workspace.
fn active_layout_slot(
    snapshot: &mut AppSessionSnapshot,
) -> Option<&mut Option<SessionWorkspaceLayoutSnapshot>> {
    let window = snapshot.windows.first_mut()?;
    // Copy the index before borrowing `workspaces` (both live on `tab_manager`).
    let raw_index = window.tab_manager.selected_workspace_index.unwrap_or(0);
    let index = if raw_index < 0 { 0usize } else { raw_index as usize };
    let workspaces = &mut window.tab_manager.workspaces;
    let workspace = if index < workspaces.len() {
        &mut workspaces[index]
    } else {
        workspaces.first_mut()?
    };
    Some(&mut workspace.layout)
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
    session_ops::split_pane(root, target_panel_id, orientation, new_panel_id, insert_first)
}

/// Close `panel_id`, collapsing an emptied split into its sibling. Pure.
fn apply_close(snapshot: &mut AppSessionSnapshot, panel_id: &str) -> CloseOutcome {
    let Some(slot) = active_layout_slot(snapshot) else {
        return CloseOutcome::NotFound;
    };
    session_ops::close_panel(slot, panel_id)
}

/// Move the divider of the split at `path`. Returns whether the path resolved to
/// a split. Pure.
fn apply_set_divider(snapshot: &mut AppSessionSnapshot, path: &[SplitChild], position: f64) -> bool {
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

/// Append a fresh single-pane workspace to the first window and select it.
/// Delegates the tab-manager mutation to [`session_ops::new_workspace`].
fn apply_new_workspace(snapshot: &mut AppSessionSnapshot, new_panel_id: &str) {
    if let Some(window) = snapshot.windows.first_mut() {
        session_ops::new_workspace(&mut window.tab_manager, new_panel_id);
    }
    // The fresh workspace comes out of the pure builder id-less; identity is
    // minted here, in the stateful layer (see `ensure_workspace_ids`).
    ensure_workspace_ids(snapshot);
}

/// Select the workspace at `index` in the first window (out-of-range is a
/// no-op). Delegates to [`session_ops::select_workspace`].
fn apply_select_workspace(snapshot: &mut AppSessionSnapshot, index: i64) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::select_workspace(&mut window.tab_manager, index),
        None => false,
    }
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

/// Pin/unpin the workspace at `index` in the first window — canonical
/// `WorkspaceReorderCoordinator.setPinned` + `reorderTabForPinnedState`
/// (already-at-value no-op; ungrouped tabs move to the pinned boundary;
/// grouped tabs flip the flag only). Pure — delegates to
/// [`session_ops::set_workspace_pinned`]. Returns whether the pin state
/// actually changed.
fn apply_set_workspace_pinned(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    pinned: bool,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => {
            session_ops::set_workspace_pinned(&mut window.tab_manager, index, pinned)
        }
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

/// Return the current session snapshot (structure of windows/workspaces/panes).
#[tauri::command]
pub fn session_snapshot(state: State<'_, SessionState>) -> AppSessionSnapshot {
    state
        .snapshot
        .lock()
        .expect("session snapshot mutex poisoned")
        .clone()
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
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        let changed = apply_set_process_title(&mut guard, &panel_id, &title);
        (changed, guard.clone())
    };
    if changed {
        emit_session_changed(&app, &snapshot);
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
) -> Result<AppSessionSnapshot, String> {
    let new_panel_id = format!("surface-{}", state.next_panel.fetch_add(1, Ordering::Relaxed));
    let snapshot = {
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        if !apply_split(
            &mut guard,
            &panel_id,
            orientation,
            &new_panel_id,
            insert_first.unwrap_or(false),
        ) {
            return Err(format!("no pane holds panel id {panel_id}"));
        }
        guard.clone()
    };
    emit_session_changed(&app, &snapshot);
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
    let snapshot = {
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        apply_close(&mut guard, &panel_id);
        guard.clone()
    };
    emit_session_changed(&app, &snapshot);
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
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        apply_set_divider(&mut guard, &path, position);
        guard.clone()
    };
    emit_session_changed(&app, &snapshot);
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
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        apply_equalize_dividers(&mut guard);
        guard.clone()
    };
    emit_session_changed(&app, &snapshot);
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
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        apply_set_surface_kind(&mut guard, &panel_id, kind);
        guard.clone()
    };
    emit_session_changed(&app, &snapshot);
    snapshot
}

/// Create a new workspace (fresh single-pane terminal) and select it. Emits
/// `cmux://session-changed` and returns the snapshot.
#[tauri::command]
pub fn session_new_workspace(
    app: AppHandle,
    state: State<'_, SessionState>,
) -> AppSessionSnapshot {
    let new_panel_id = format!("surface-{}", state.next_panel.fetch_add(1, Ordering::Relaxed));
    let snapshot = {
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        apply_new_workspace(&mut guard, &new_panel_id);
        guard.clone()
    };
    emit_session_changed(&app, &snapshot);
    snapshot
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
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        apply_select_workspace(&mut guard, index);
        guard.clone()
    };
    emit_session_changed(&app, &snapshot);
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
    let snapshot = {
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        apply_close_workspace(&mut guard, index);
        guard.clone()
    };
    emit_session_changed(&app, &snapshot);
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
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        let changed = apply_set_group_collapsed(&mut guard, &group_id, collapsed);
        (changed, guard.clone())
    };
    if changed {
        emit_session_changed(&app, &snapshot);
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
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        let changed = apply_rename_workspace(&mut guard, index, &title);
        (changed, guard.clone())
    };
    if changed {
        emit_session_changed(&app, &snapshot);
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
        let mut guard = state.snapshot.lock().expect("session snapshot mutex poisoned");
        let changed = apply_set_workspace_pinned(&mut guard, index, pinned);
        (changed, guard.clone())
    };
    if changed {
        emit_session_changed(&app, &snapshot);
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
    fn new_workspace_mints_an_id_and_keeps_existing_ids() {
        let mut snapshot = initial_snapshot("surface-1");
        let first_id = snapshot.windows[0].tab_manager.workspaces[0]
            .workspace_id
            .clone();
        apply_new_workspace(&mut snapshot, "surface-2");
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
        assert_eq!(apply_close(&mut snapshot, "surface-2"), CloseOutcome::Removed);
        assert_eq!(count_leaves(active_layout(&snapshot)), 1);
    }

    #[test]
    fn apply_close_emptying_the_last_pane_clears_layout() {
        let mut snapshot = initial_snapshot("surface-1");
        assert_eq!(apply_close(&mut snapshot, "surface-1"), CloseOutcome::Emptied);
        assert!(snapshot.windows[0].tab_manager.workspaces[0].layout.is_none());
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
        assert_eq!(apply_close(&mut snapshot, "surface-1"), CloseOutcome::Emptied);
        assert!(!apply_equalize_dividers(&mut snapshot));
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
        apply_new_workspace(&mut snapshot, "surface-2");
        let tabs = tab_manager(&snapshot);
        assert_eq!(tabs.workspaces.len(), 2);
        assert_eq!(tabs.selected_workspace_index, Some(1));
        assert_eq!(count_leaves(active_layout(&snapshot)), 1);
    }

    #[test]
    fn apply_close_workspace_removes_and_reclamps_selection() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2");
        apply_new_workspace(&mut snapshot, "surface-3"); // 3 workspaces, sel=2
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
    fn apply_set_workspace_pinned_reorders_and_selection_follows() {
        let mut snapshot = initial_snapshot("surface-1");
        apply_new_workspace(&mut snapshot, "surface-2"); // 2 workspaces, sel=1
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
