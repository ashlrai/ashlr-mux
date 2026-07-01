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
    SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot, SESSION_SNAPSHOT_SCHEMA_VERSION,
};
use cmux_core::session_ops::{self, CloseOutcome, SplitChild};
use tauri::{AppHandle, Emitter, State};

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
    let workspace = SessionWorkspaceSnapshot {
        process_title: "Terminal".to_string(),
        layout: Some(session_ops::single_pane(first_panel_id)),
        ..Default::default()
    };
    AppSessionSnapshot {
        version: SESSION_SNAPSHOT_SCHEMA_VERSION,
        created_at: 0,
        windows: vec![SessionWindowSnapshot {
            window_id: Some("window-1".to_string()),
            tab_manager: SessionTabManagerSnapshot {
                selected_workspace_index: Some(0),
                workspaces: vec![workspace],
                workspace_groups: None,
            },
        }],
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
