//! Pure mutation operations over the session **split-layout tree**
//! ([`SessionWorkspaceLayoutSnapshot`]).
//!
//! The layout is a binary tree: each node is a `Pane` (leaf, holding one or more
//! panel ids) or a `Split` (two children + orientation + one `divider_position`
//! ratio). These operations — split a pane, close a panel (collapsing an emptied
//! split into its sibling), move a divider — are the authoritative Rust side of
//! the same logic the web renderer mirrors in `apps/desktop/web/src/session/
//! splitLayout.ts`, and a port of the macOS `CmuxPanes` split model. They are
//! pure (no ConPTY, no I/O) so they unit-test headlessly; the Tauri command
//! layer binds them to real pseudo-consoles.

use std::collections::{HashMap, HashSet};

#[cfg(test)]
use cmux_browser_history::NavigationAvailability;

use crate::session::{
    AppSessionSnapshot, SessionPaneLayoutSnapshot, SessionPanelListeningPortsSnapshot,
    SessionPanelPinSnapshot, SessionPanelRestorableAgentSnapshot,
    SessionPanelShellActivitySnapshot, SessionPanelTerminalStartupSnapshot,
    SessionPanelTitleSnapshot, SessionPanelTtySnapshot, SessionPanelUnreadSnapshot,
    SessionSplitLayoutSnapshot, SessionSplitOrientation, SessionTabManagerSnapshot,
    SessionWorkspaceGroupSnapshot, SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot,
};
use crate::surface_lifecycle::{LifecycleError, SurfaceLifecycleModel};

mod browser;
mod canvas;
mod workspace_ordering;

pub use canvas::{
    apply_canvas_action, apply_canvas_action_with_gap, canvas_panes_from_layout,
    set_canvas_pane_frame, set_layout_mode, CanvasAction,
};

pub use browser::{
    browser_go_back, browser_go_forward, browser_navigation_availability, clear_browser_history,
    ensure_browser_page_zoom, navigate_browser, serializable_browser_history_url,
    set_browser_page_zoom, set_browser_url, show_browser_developer_tools,
    toggle_browser_developer_tools, toggle_browser_focus_mode, toggle_browser_omnibar_visible,
};
use workspace_ordering::normalize_workspace_groups_in_snapshot;
pub use workspace_ordering::{
    add_workspace_to_group_snapshot, create_workspace_group_snapshot,
    move_workspace_group_snapshot, move_workspace_to_top, move_workspace_to_top_for_notification,
    remove_workspace_from_group_snapshot, rename_workspace_group_snapshot, reorder_workspaces,
    reorder_workspaces_many, reorder_workspaces_with_mode, set_group_collapsed,
    set_workspace_group_anchor_snapshot, set_workspace_group_color_snapshot,
    set_workspace_group_icon_snapshot, set_workspace_group_pinned_snapshot,
    ungroup_workspace_group_snapshot, WorkspaceGroupMutationError, WorkspaceGroupPlacement,
};

/// Single production adapter for mutations of the authoritative persisted
/// surface model. The caller's `AppSessionSnapshot` changes only after the
/// model mutation and full reverse-index validation both succeed.
pub fn with_surface_lifecycle<R>(
    session: &mut crate::session::AppSessionSnapshot,
    mutate: impl FnOnce(&mut SurfaceLifecycleModel) -> Result<R, LifecycleError>,
) -> Result<R, LifecycleError> {
    let base = session.clone();
    let mut model = SurfaceLifecycleModel::from_app_session(&base)?;
    let result = mutate(&mut model)?;
    model.validate_indexes()?;
    *session = model.to_app_session(&base)?;
    Ok(result)
}

pub fn read_surface_lifecycle(
    session: &crate::session::AppSessionSnapshot,
) -> Result<SurfaceLifecycleModel, LifecycleError> {
    SurfaceLifecycleModel::from_app_session(session)
}
use cmux_workspaces::{
    assign_group, clamped_reorder_index, clamped_top_level_reorder_index,
    expand_workspace_group_for_selection_if_needed, insertion_index, is_workspace_group_anchor,
    move_workspace_group_members_after_anchors, normalize_workspace_group_contiguity,
    normalize_workspace_group_runs_preserving_order, sidebar_top_level_pinned_workspace_ids,
    sidebar_top_level_workspace_ids, sync_workspace_groups_order_to_anchor_order,
    top_level_workspace_ids, top_level_workspace_ids_preserving_order, NewWorkspacePlacement,
    WorkspaceBatchReorderError, WorkspaceGroup, WorkspaceOrderSnapshot, WorkspaceReorderPlanItem,
    WorkspaceReorderPlanner, WorkspaceRow,
};
use uuid::Uuid;

use serde::{Deserialize, Serialize};

type Layout = SessionWorkspaceLayoutSnapshot;

mod pane_layout;
pub use pane_layout::*;
use pane_layout::{attach_panel_metadata, detach_panel_metadata, pane_for_panel, panel_count};

// --- Tab-manager (workspace) operations -------------------------------------
//
// The layer above the split tree: a window's `SessionTabManagerSnapshot` holds
// an ordered list of workspaces and a selected index. These are the Rust port
// of the macOS `TabManager` workspace lifecycle (add / select / close), kept
// pure and headless-testable exactly like the pane ops above.

/// A fresh single-pane workspace titled `"Terminal"`, holding `panel_id`. The
/// canonical default new workspace (macOS `TabManager.addWorkspace`).
pub fn fresh_terminal_workspace(panel_id: &str) -> SessionWorkspaceSnapshot {
    SessionWorkspaceSnapshot {
        process_title: "Terminal".to_string(),
        layout: Some(single_pane(panel_id)),
        focused_panel_id: Some(panel_id.to_string()),
        ..Default::default()
    }
}

/// Insert a fresh single-pane workspace into `tabs` under the default
/// `AfterCurrent` placement and select it. Thin wrapper over
/// [`new_workspace_with_placement`] preserving the two-arg call site; the host
/// resolves `effectivePlacement` (settings reads stay host-side, mirroring the
/// `placement.rs` doc note) and calls the placement-aware variant directly.
///
/// For the common single-selected-tab, no-pins case `AfterCurrent` still yields
/// an append, so this matches the historical behaviour.
pub fn new_workspace(tabs: &mut SessionTabManagerSnapshot, panel_id: &str) {
    new_workspace_with_placement(tabs, panel_id, NewWorkspacePlacement::default());
}

/// Move `panel_id` out of its current workspace into a newly-created workspace,
/// selecting the new workspace. The command is only meaningful when the source
/// workspace has at least one other panel/surface left behind.
///
/// Pane-local state (surface kind, browser/markdown/diff bindings, zoom, etc.)
/// follows the moved panel. Panel-level title/pin/unread metadata is transferred
/// from the source workspace to the destination workspace.
pub fn move_panel_to_new_workspace(tabs: &mut SessionTabManagerSnapshot, panel_id: &str) -> bool {
    let Some(source_index) = tabs.workspaces.iter().position(|workspace| {
        workspace
            .layout
            .as_ref()
            .is_some_and(|layout| contains_panel(layout, panel_id))
    }) else {
        return false;
    };

    let Some(source_layout) = tabs.workspaces[source_index].layout.as_ref() else {
        return false;
    };
    if panel_count(source_layout) <= 1 {
        return false;
    }
    break_surface_to_new_workspace(tabs, source_index, panel_id, true).is_ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneBreakResult {
    pub workspace_index: usize,
    pub surface_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneBreakError {
    WorkspaceNotFound,
    SurfaceNotFound,
    DetachFailed,
}

/// Detach one surface into a fresh workspace in the same window. The source
/// workspace may become empty, matching canonical `pane.break`; panel metadata
/// follows the surface and the destination receives a new pane identity from
/// the stateful desktop layer. Invalid requests leave `tabs` byte-identical.
pub fn break_surface_to_new_workspace(
    tabs: &mut SessionTabManagerSnapshot,
    source_workspace_index: usize,
    panel_id: &str,
    focus: bool,
) -> Result<PaneBreakResult, PaneBreakError> {
    let source = tabs
        .workspaces
        .get(source_workspace_index)
        .ok_or(PaneBreakError::WorkspaceNotFound)?;
    let source_layout = source
        .layout
        .as_ref()
        .ok_or(PaneBreakError::SurfaceNotFound)?;
    let mut detached_pane =
        pane_for_panel(source_layout, panel_id).ok_or(PaneBreakError::SurfaceNotFound)?;
    detached_pane.pane_id = None;

    let total_count = tabs.workspaces.len() as i64;
    let pinned_count = tabs
        .workspaces
        .iter()
        .filter(|workspace| workspace.is_pinned == Some(true))
        .count() as i64;
    let selected_index = tabs.selected_workspace_index;
    let selected_is_pinned = selected_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| tabs.workspaces.get(index))
        .is_some_and(|workspace| workspace.is_pinned == Some(true));
    let insert_index = insertion_index(
        NewWorkspacePlacement::default(),
        selected_index,
        selected_is_pinned,
        pinned_count,
        total_count,
    )
    .clamp(0, total_count) as usize;

    let mut next = tabs.clone();
    let source = &mut next.workspaces[source_workspace_index];
    let metadata = detach_panel_metadata(source, panel_id);
    let process_title = metadata
        .title
        .as_ref()
        .and_then(|entry| entry.custom_title.clone())
        .unwrap_or_else(|| source.process_title.clone());
    let mut detached = SessionWorkspaceSnapshot {
        process_title,
        current_directory: source.current_directory.clone(),
        initial_terminal_command: source.initial_terminal_command.clone(),
        initial_terminal_input: source.initial_terminal_input.clone(),
        initial_terminal_environment: source.initial_terminal_environment.clone(),
        focused_panel_id: Some(panel_id.to_string()),
        layout: Some(Layout::Pane(detached_pane)),
        ..Default::default()
    };
    if matches!(
        close_panel(&mut source.layout, panel_id),
        CloseOutcome::NotFound
    ) {
        return Err(PaneBreakError::DetachFailed);
    }
    if source.zoomed_panel_id.as_deref() == Some(panel_id) {
        source.zoomed_panel_id = None;
    }
    if source.focused_panel_id.as_deref() == Some(panel_id) {
        source.focused_panel_id = source.layout.as_ref().and_then(first_panel_id);
    }
    attach_panel_metadata(&mut detached, metadata);

    next.workspaces.insert(insert_index, detached);
    if focus {
        next.selected_workspace_index = Some(insert_index as i64);
    } else if selected_index.is_some_and(|selected| selected >= insert_index as i64) {
        next.selected_workspace_index = selected_index.map(|selected| selected + 1);
    }
    normalize_workspace_groups_in_snapshot(&mut next);
    let destination_index = next
        .workspaces
        .iter()
        .position(|workspace| {
            workspace
                .layout
                .as_ref()
                .is_some_and(|layout| contains_panel(layout, panel_id))
        })
        .ok_or(PaneBreakError::DetachFailed)?;
    if focus {
        next.selected_workspace_index = Some(destination_index as i64);
    }
    *tabs = next;
    Ok(PaneBreakResult {
        workspace_index: destination_index,
        surface_id: panel_id.to_string(),
    })
}

/// Insert a fresh single-pane workspace into `tabs` at the position dictated by
/// `placement`, then select it. This is the Rust port of the macOS
/// `TabManager.addWorkspace` → `newTabInsertIndex(snapshot:placementOverride:)`
/// path (`Sources/TabManager.swift:1088`, `:1126-1132`, `:1156`).
///
/// The whole `newTabInsertIndex` switch (Top / End / AfterCurrent) is folded by
/// the already-ported [`insertion_index`] arithmetic
/// (`cmux-workspaces/src/placement.rs`), fed snapshot-derived inputs. Selection
/// is index-based here (canonical is id-based), so the current selection is read
/// directly from `selected_workspace_index` with no id lookup.
///
/// PARITY NUANCES (grounded in the A9 spec):
/// - `pinned_count` is a plain count of pinned workspaces; it equals the pinned
///   *boundary* only because pins form a contiguous prefix, which the canonical
///   sidebar guarantees and `insertion_index` (Top → `clampedPinnedCount`)
///   assumes.
/// - `selected_is_pinned` mirrors Swift `selectedTabWasPinned`
///   (`TabManager.swift:1340`, `selectedTabSnapshot?.isPinned ?? false`).
/// - AfterCurrent-with-no-selection: Swift `newTabInsertIndex` would return
///   `selectedTabWasPinned ? pinnedCount : count`, whereas `insertion_index`
///   returns End unconditionally. In the index-based session model a valid
///   selection always resolves, so this only differs for a `None`/stale
///   selection — then the port yields End.
/// - GROUP CONTIGUITY: canonical inserts by flat index THEN runs
///   `normalizeWorkspaceGroupContiguity` (`TabManager.swift:1136-1138`). The
///   port mirrors that through the snapshot↔workspace-row mirror: a fresh
///   ungrouped workspace inserted into a group run stays ungrouped, but is
///   moved out of the middle of the run so grouped members remain contiguous.
pub fn new_workspace_with_placement(
    tabs: &mut SessionTabManagerSnapshot,
    panel_id: &str,
    placement: NewWorkspacePlacement,
) {
    // Pre-insert shape, mirroring Swift's `liveTabs` reads.
    let total_count = tabs.workspaces.len() as i64;
    let pinned_count = tabs
        .workspaces
        .iter()
        .filter(|w| w.is_pinned == Some(true))
        .count() as i64;
    let selected_index = tabs.selected_workspace_index;
    let selected_is_pinned = selected_index
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| tabs.workspaces.get(i))
        .map(|w| w.is_pinned == Some(true))
        .unwrap_or(false);
    let inherited_directory = selected_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| tabs.workspaces.get(index))
        .and_then(|workspace| workspace.current_directory.clone());

    let idx = insertion_index(
        placement,
        selected_index,
        selected_is_pinned,
        pinned_count,
        total_count,
    );
    // `insertion_index` already clamps into `[0, total_count]`, but clamp again
    // defensively before the unsigned cast (Swift's `insert` also falls back to
    // an append when the index is out of range, `TabManager.swift:1126-1132`).
    let at = idx.clamp(0, total_count) as usize;
    let mut workspace = fresh_terminal_workspace(panel_id);
    workspace.current_directory = inherited_directory;
    tabs.workspaces.insert(at, workspace);
    // Canonical selects the newly created workspace (`TabManager.swift:1156`).
    tabs.selected_workspace_index = Some(at as i64);
    normalize_workspace_groups_in_snapshot(tabs);
}

/// Select the workspace at `index`, ignoring an out-of-range index. Mirrors
/// `TabManager.selectWorkspace`. Returns whether `index` resolved to a workspace.
pub fn select_workspace(tabs: &mut SessionTabManagerSnapshot, index: i64) -> bool {
    let count = tabs.workspaces.len();
    if count == 0 || index < 0 || (index as usize) >= count {
        return false;
    }
    tabs.selected_workspace_index = Some(index);
    true
}

/// Close the workspace at `index`. Mirrors canonical `TabManager.closeWorkspace`
/// (`guard tabs.count > 1`): closing the only workspace is a **no-op**. When the
/// removed workspace was at or before the selection, the selection is re-clamped
/// to keep pointing at the same surviving workspace (else the new last one). If
/// the closed workspace anchored a group, that group dissolves and its
/// surviving members become ungrouped, matching canonical `closeWorkspace`.
/// Returns whether a close happened.
fn anchored_group_ids_for_workspace_id(
    tabs: &SessionTabManagerSnapshot,
    workspace_id: &str,
) -> HashSet<String> {
    let mut group_ids = HashSet::new();
    for group in tabs.workspace_groups.as_deref().unwrap_or(&[]) {
        let members: Vec<&SessionWorkspaceSnapshot> = tabs
            .workspaces
            .iter()
            .filter(|ws| ws.group_id.as_deref() == Some(group.id.as_str()))
            .collect();
        if members.is_empty() {
            continue;
        }
        let anchor_workspace_id = group
            .anchor_workspace_id
            .as_ref()
            .filter(|anchor| {
                members
                    .iter()
                    .any(|member| member.workspace_id.as_ref() == Some(*anchor))
            })
            .cloned()
            .or_else(|| {
                group
                    .anchor_member_index
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| members.get(index))
                    .and_then(|member| member.workspace_id.clone())
            })
            .or_else(|| {
                members
                    .first()
                    .and_then(|member| member.workspace_id.clone())
            });
        if anchor_workspace_id.as_deref() == Some(workspace_id) {
            group_ids.insert(group.id.clone());
        }
    }
    group_ids
}

fn reconcile_workspace_groups_after_membership_change(tabs: &mut SessionTabManagerSnapshot) {
    let Some(groups) = tabs.workspace_groups.take() else {
        return;
    };

    let mut next_groups = Vec::new();
    for mut group in groups {
        let members: Vec<&SessionWorkspaceSnapshot> = tabs
            .workspaces
            .iter()
            .filter(|ws| ws.group_id.as_deref() == Some(group.id.as_str()))
            .collect();
        if members.is_empty() {
            continue;
        }

        let anchor_workspace_id = group
            .anchor_workspace_id
            .as_ref()
            .filter(|anchor| {
                members
                    .iter()
                    .any(|member| member.workspace_id.as_ref() == Some(*anchor))
            })
            .cloned()
            .or_else(|| {
                group
                    .anchor_member_index
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| members.get(index))
                    .and_then(|member| member.workspace_id.clone())
            })
            .or_else(|| {
                members
                    .first()
                    .and_then(|member| member.workspace_id.clone())
            });

        if let Some(anchor_workspace_id) = anchor_workspace_id {
            let anchor_member_index = members
                .iter()
                .position(|member| {
                    member.workspace_id.as_deref() == Some(anchor_workspace_id.as_str())
                })
                .map(|index| index as i64);
            group.anchor_workspace_id = Some(anchor_workspace_id);
            group.anchor_member_index = anchor_member_index;
        } else {
            group.anchor_workspace_id = None;
            group.anchor_member_index = None;
        }
        next_groups.push(group);
    }

    tabs.workspace_groups = if next_groups.is_empty() {
        None
    } else {
        Some(next_groups)
    };
}

fn detach_workspace_at_index(
    tabs: &mut SessionTabManagerSnapshot,
    removed: usize,
) -> SessionWorkspaceSnapshot {
    let anchored_group_ids = tabs.workspaces[removed]
        .workspace_id
        .as_deref()
        .map(|workspace_id| anchored_group_ids_for_workspace_id(tabs, workspace_id))
        .unwrap_or_default();
    let workspace = tabs.workspaces.remove(removed);

    if !anchored_group_ids.is_empty() {
        for workspace in &mut tabs.workspaces {
            if workspace
                .group_id
                .as_ref()
                .is_some_and(|group_id| anchored_group_ids.contains(group_id))
            {
                workspace.group_id = None;
            }
        }
    }
    reconcile_workspace_groups_after_membership_change(tabs);

    // Selection here is index-based (canonical is id-based). To keep the same
    // surviving workspace focused: removing a tab before the selected one shifts
    // it left; removing at/after clamps to the (new) last tab — canonical's
    // `min(index, count - 1)`.
    if tabs.workspaces.is_empty() {
        tabs.selected_workspace_index = None;
    } else {
        let selected = tabs.selected_workspace_index.unwrap_or(0).max(0) as usize;
        let next = if selected > removed {
            selected - 1
        } else {
            selected.min(tabs.workspaces.len() - 1)
        };
        tabs.selected_workspace_index = Some(next as i64);
    }
    workspace
}

fn close_workspace_at_index(tabs: &mut SessionTabManagerSnapshot, removed: usize) -> bool {
    detach_workspace_at_index(tabs, removed);
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveWorkspaceToWindowError {
    WorkspaceNotFound,
    WindowNotFound,
}

/// Transfer a workspace between window tab managers using canonical detach /
/// attach behavior. Detach dissolves a group anchored by the mover and clears
/// the mover's group membership. Empty source windows receive `bootstrap`.
/// Attach appends unpinned workspaces, places pinned workspaces at the end of
/// the pinned prefix, restores group contiguity, and selects only when `focus`
/// is requested. Validation is transactional.
pub fn move_workspace_to_window(
    snapshot: &mut AppSessionSnapshot,
    workspace_id: &str,
    target_window_id: &str,
    bootstrap: SessionWorkspaceSnapshot,
    focus: bool,
) -> Result<(), MoveWorkspaceToWindowError> {
    let source = snapshot
        .windows
        .iter()
        .enumerate()
        .find_map(|(window_index, window)| {
            window
                .tab_manager
                .workspaces
                .iter()
                .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
                .map(|workspace_index| (window_index, workspace_index))
        })
        .ok_or(MoveWorkspaceToWindowError::WorkspaceNotFound)?;
    let target_window_index = snapshot
        .windows
        .iter()
        .position(|window| window.window_id.as_deref() == Some(target_window_id))
        .ok_or(MoveWorkspaceToWindowError::WindowNotFound)?;

    let mut next = snapshot.clone();
    let mut moved = detach_workspace_at_index(&mut next.windows[source.0].tab_manager, source.1);
    moved.group_id = None;
    if next.windows[source.0].tab_manager.workspaces.is_empty() {
        next.windows[source.0]
            .tab_manager
            .workspaces
            .push(bootstrap);
        next.windows[source.0].tab_manager.selected_workspace_index = Some(0);
    }

    let target = &mut next.windows[target_window_index].tab_manager;
    let selected_workspace_id = target
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| target.workspaces.get(index))
        .and_then(|workspace| workspace.workspace_id.clone());
    let insert_index = if moved.is_pinned == Some(true) {
        target
            .workspaces
            .iter()
            .take_while(|workspace| workspace.is_pinned == Some(true))
            .count()
    } else {
        target.workspaces.len()
    };
    target.workspaces.insert(insert_index, moved);
    normalize_workspace_groups_in_snapshot(target);
    if focus {
        target.selected_workspace_index = target
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
            .map(|index| index as i64);
    } else if let Some(selected_workspace_id) = selected_workspace_id {
        target.selected_workspace_index = target
            .workspaces
            .iter()
            .position(|workspace| {
                workspace.workspace_id.as_deref() == Some(selected_workspace_id.as_str())
            })
            .map(|index| index as i64);
    }
    *snapshot = next;
    Ok(())
}

pub fn close_workspace(tabs: &mut SessionTabManagerSnapshot, index: i64) -> bool {
    let count = tabs.workspaces.len();
    if count <= 1 || index < 0 || (index as usize) >= count {
        return false;
    }
    if tabs.workspaces[index as usize].is_pinned == Some(true) {
        return false;
    }
    close_workspace_at_index(tabs, index as usize)
}

/// Close multiple workspaces identified by their ORIGINAL indices in
/// `tabs.workspaces`. The request is canonicalized into current tab order,
/// deduplicated, then each close is applied against the live snapshot so
/// selection and group-anchor dissolution mirror repeated explicit closes.
///
/// Semantics:
/// - Empty, all-invalid, or single-workspace snapshots are no-ops.
/// - Indices are interpreted against the pre-close snapshot, sorted into the
///   window's current order (matching canonical `orderedClosableWorkspaces`).
/// - Duplicate indices close once.
/// - The last remaining workspace is preserved: once only one workspace
///   survives, later requested closes are skipped (the session model never
///   closes the final workspace/window).
/// - If a closed workspace anchored a group, that group dissolves and its
///   surviving members become ungrouped, matching canonical `closeWorkspace`.
///
/// Returns `true` iff at least one workspace actually closed.
pub fn close_workspaces(tabs: &mut SessionTabManagerSnapshot, indices: &[i64]) -> bool {
    if tabs.workspaces.len() <= 1 || indices.is_empty() {
        return false;
    }

    let mut ordered_indices: Vec<usize> = indices
        .iter()
        .filter_map(|&index| usize::try_from(index).ok())
        .filter(|&index| index < tabs.workspaces.len())
        .collect();
    ordered_indices.sort_unstable();
    ordered_indices.dedup();

    let ordered_workspace_ids: Vec<String> = ordered_indices
        .into_iter()
        .filter_map(|index| tabs.workspaces[index].workspace_id.clone())
        .collect();

    let mut changed = false;
    for workspace_id in ordered_workspace_ids {
        if tabs.workspaces.len() <= 1 {
            break;
        }
        let Some(index) = tabs
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
        else {
            continue;
        };
        changed |= close_workspace_at_index(tabs, index);
    }
    changed
}

/// Rename the workspace at `index` — the user-rename path of canonical
/// `Workspace.setCustomTitle(_:source:)` (`Workspace.swift:4390-4407`) reached
/// via `TabManager.setCustomTitle` (`TabManager.swift:1677-1698`). This op is
/// **user-source-only**: a manual rename always stamps
/// `custom_title_source = "user"` (Swift's default `source: .user`, raw value
/// pinned by `session_golden.rs`); the `.auto` OSC guard branch
/// (`Workspace.swift:4393-4396`) is a different feed and is not ported here.
///
/// The title is trimmed; an empty/whitespace-only result CLEARS both
/// `custom_title` and `custom_title_source` (`Workspace.swift:4397-4400`).
/// Canonical then restores `self.title = processTitle` — implicit here, since
/// the snapshot has no separate `title` field and the display title is derived
/// web-side (`custom_title || process_title`); `process_title` is never
/// touched. Out-of-range/negative `index` is a silent no-op (canonical
/// unknown-id guard, `TabManager.swift:1684`). Canonical side effects out of
/// port scope: `updateWindowTitle` when selected (the web derives the display
/// title from the snapshot) and remote-tmux `rename-session` propagation
/// (`TabManager.swift:1689-1696`, N/A on Windows).
///
/// Returns `true` iff `(custom_title, custom_title_source)` actually changed —
/// so re-stamping an `"auto"`-sourced title with the same text still reports a
/// change (source flips to `"user"`). Canonical `setCustomTitle` returns
/// "write landed" (always true for a resolved id); the changed-gate is this
/// port's emit policy, matching the `set_group_collapsed`/`set_process_title`
/// precedent — a deliberate documented divergence.
///
/// NOTE (minor divergence): Swift trims with `.whitespacesAndNewlines`; this
/// uses Rust `str::trim` (Unicode `White_Space`). The two differ only on exotic
/// separators a rename never carries in practice.
pub fn rename_workspace(tabs: &mut SessionTabManagerSnapshot, index: i64, title: &str) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() {
        return false;
    }
    let workspace = &mut tabs.workspaces[index as usize];
    let trimmed = title.trim();
    let (next_title, next_source) = if trimmed.is_empty() {
        (None, None)
    } else {
        (Some(trimmed.to_string()), Some("user".to_string()))
    };
    if workspace.custom_title == next_title && workspace.custom_title_source == next_source {
        return false;
    }
    workspace.custom_title = next_title;
    workspace.custom_title_source = next_source;
    true
}

fn normalized_workspace_description(description: &str) -> Option<String> {
    let normalized = description.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.trim().is_empty() {
        None
    } else {
        Some(normalized)
    }
}

/// Set or clear the workspace description at `index` — the port of canonical
/// `Workspace.setCustomDescription(_:)` (`Workspace.swift:4409-4428`) reached
/// through `TabManager.setCustomDescription`.
///
/// Line endings normalize to `\n`; blank/whitespace-only descriptions clear the
/// field; non-empty descriptions preserve their edge whitespace after line
/// normalization. Out-of-range/negative `index` is a silent no-op.
///
/// Returns `true` iff `custom_description` actually changed.
pub fn set_workspace_description(
    tabs: &mut SessionTabManagerSnapshot,
    index: i64,
    description: &str,
) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() {
        return false;
    }
    let workspace = &mut tabs.workspaces[index as usize];
    let next_description = normalized_workspace_description(description);
    if workspace.custom_description == next_description {
        return false;
    }
    workspace.custom_description = next_description;
    true
}

/// Clear the workspace color override at `index`. Canonical cmux stores the
/// workspace tab color as `customColor` / `custom_color`; reset removes that
/// override so the row falls back to group/default coloring.
///
/// Returns `true` iff a color was actually cleared.
pub fn reset_workspace_color(tabs: &mut SessionTabManagerSnapshot, index: i64) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() {
        return false;
    }
    let workspace = &mut tabs.workspaces[index as usize];
    if workspace.custom_color.is_none() {
        return false;
    }
    workspace.custom_color = None;
    true
}

/// Set or clear a normalized custom workspace color. Validation and named-palette
/// resolution belong to the caller; this is the authoritative persisted model
/// write shared by the socket and desktop lanes.
pub fn set_workspace_color(
    tabs: &mut SessionTabManagerSnapshot,
    index: i64,
    color: Option<&str>,
) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() {
        return false;
    }
    let workspace = &mut tabs.workspaces[index as usize];
    let next = color.map(str::to_owned);
    if workspace.custom_color == next {
        return false;
    }
    workspace.custom_color = next;
    true
}

fn normalized_panel_title(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Set or clear a panel/tab custom title within `workspace`. The target panel
/// must exist in the workspace layout; blank/whitespace-only titles clear the
/// custom title and remove its metadata entry.
///
/// Returns `true` iff the stored metadata actually changed.
pub fn set_panel_title(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    title: &str,
) -> bool {
    let Some(layout) = workspace.layout.as_ref() else {
        return false;
    };
    if !contains_panel(layout, panel_id) {
        return false;
    }

    let next_title = normalized_panel_title(title);
    let mut titles = workspace.panel_titles.take().unwrap_or_default();
    let existing = titles
        .iter()
        .find(|entry| entry.panel_id == panel_id)
        .and_then(|entry| entry.custom_title.clone());
    if existing == next_title {
        workspace.panel_titles = (!titles.is_empty()).then_some(titles);
        return false;
    }

    titles.retain(|entry| entry.panel_id != panel_id);
    if let Some(custom_title) = next_title {
        titles.push(SessionPanelTitleSnapshot {
            panel_id: panel_id.to_string(),
            custom_title: Some(custom_title),
        });
    }
    workspace.panel_titles = (!titles.is_empty()).then_some(titles);
    true
}

/// Set or clear a panel/tab pin within `workspace`. The target panel must
/// exist in the workspace layout. Unpinned panels are represented by absence,
/// so clearing the last pin removes `panel_pins` entirely.
///
/// Returns `true` iff the stored metadata actually changed.
pub fn set_panel_pinned(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    is_pinned: bool,
) -> bool {
    let Some(layout) = workspace.layout.as_ref() else {
        return false;
    };
    if !contains_panel(layout, panel_id) {
        return false;
    }

    let mut pins = workspace.panel_pins.take().unwrap_or_default();
    let existing = pins
        .iter()
        .find(|entry| entry.panel_id == panel_id)
        .is_some_and(|entry| entry.is_pinned);
    if existing == is_pinned {
        workspace.panel_pins = (!pins.is_empty()).then_some(pins);
        return false;
    }

    pins.retain(|entry| entry.panel_id != panel_id);
    if is_pinned {
        pins.push(SessionPanelPinSnapshot {
            panel_id: panel_id.to_string(),
            is_pinned: true,
        });
    }
    workspace.panel_pins = (!pins.is_empty()).then_some(pins);
    true
}

/// Reorder a surface within its existing pane using bonsplit's insertion-offset
/// contract. The destination is clamped to `0...count`; forward moves subtract
/// one after removal; self/self+1 drops are order no-ops; and the moved tab is
/// clamped to its pinned or unpinned tier. `focus=false` preserves selection,
/// while `focus=true` selects the moved surface. Returns `None` when the surface
/// is absent, otherwise whether persisted order or selection changed.
pub fn reorder_surface(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    destination_index: i64,
    focus: bool,
) -> Option<bool> {
    let pinned: HashSet<&str> = workspace
        .panel_pins
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.is_pinned)
        .map(|entry| entry.panel_id.as_str())
        .collect();
    let layout = workspace.layout.as_mut()?;

    fn reorder_in_layout(
        layout: &mut Layout,
        panel_id: &str,
        destination_index: i64,
        focus: bool,
        pinned: &HashSet<&str>,
    ) -> Option<bool> {
        match layout {
            Layout::Pane(pane) => {
                let source_index = pane.panel_ids.iter().position(|id| id == panel_id)?;
                let destination_index =
                    destination_index.clamp(0, pane.panel_ids.len() as i64) as usize;
                let previous_selection = pane.selected_panel_id.clone();
                let mut order_changed = false;
                if destination_index != source_index && destination_index != source_index + 1 {
                    let moved = pane.panel_ids.remove(source_index);
                    let requested_index = if destination_index > source_index {
                        destination_index - 1
                    } else {
                        destination_index
                    };
                    let pinned_count = pane
                        .panel_ids
                        .iter()
                        .filter(|id| pinned.contains(id.as_str()))
                        .count();
                    let tier_index = if pinned.contains(panel_id) {
                        requested_index.min(pinned_count)
                    } else {
                        requested_index.max(pinned_count)
                    };
                    let safe_index = tier_index.min(pane.panel_ids.len());
                    pane.panel_ids.insert(safe_index, moved);
                    order_changed = pane
                        .panel_ids
                        .get(source_index)
                        .is_none_or(|id| id != panel_id);
                }
                if focus {
                    pane.selected_panel_id = Some(panel_id.to_string());
                }
                Some(order_changed || pane.selected_panel_id != previous_selection)
            }
            Layout::Split(split) => {
                reorder_in_layout(&mut split.first, panel_id, destination_index, focus, pinned)
                    .or_else(|| {
                        reorder_in_layout(
                            &mut split.second,
                            panel_id,
                            destination_index,
                            focus,
                            pinned,
                        )
                    })
            }
        }
    }

    reorder_in_layout(layout, panel_id, destination_index, focus, &pinned)
}

fn pane_containing_panel<'a>(
    layout: &'a Layout,
    panel_id: &str,
) -> Option<&'a SessionPaneLayoutSnapshot> {
    match layout {
        Layout::Pane(pane) => pane
            .panel_ids
            .iter()
            .any(|id| id == panel_id)
            .then_some(pane),
        Layout::Split(split) => pane_containing_panel(&split.first, panel_id)
            .or_else(|| pane_containing_panel(&split.second, panel_id)),
    }
}

fn pane_by_id_mut<'a>(
    layout: &'a mut Layout,
    pane_id: &str,
) -> Option<&'a mut SessionPaneLayoutSnapshot> {
    match layout {
        Layout::Pane(pane) => (pane.pane_id.as_deref() == Some(pane_id)).then_some(pane),
        Layout::Split(split) => pane_by_id_mut(&mut split.first, pane_id)
            .or_else(|| pane_by_id_mut(&mut split.second, pane_id)),
    }
}

fn pane_by_id<'a>(layout: &'a Layout, pane_id: &str) -> Option<&'a SessionPaneLayoutSnapshot> {
    match layout {
        Layout::Pane(pane) => (pane.pane_id.as_deref() == Some(pane_id)).then_some(pane),
        Layout::Split(split) => {
            pane_by_id(&split.first, pane_id).or_else(|| pane_by_id(&split.second, pane_id))
        }
    }
}

fn insert_surface_into_pane(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    pane_id: &str,
    destination_index: Option<i64>,
    moved_is_pinned: bool,
    focus: bool,
) -> bool {
    let pinned: HashSet<&str> = workspace
        .panel_pins
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.is_pinned)
        .map(|entry| entry.panel_id.as_str())
        .collect();
    let Some(layout) = workspace.layout.as_mut() else {
        return false;
    };
    let Some(pane) = pane_by_id_mut(layout, pane_id) else {
        return false;
    };
    let requested_index = destination_index
        .unwrap_or(pane.panel_ids.len() as i64)
        .clamp(0, pane.panel_ids.len() as i64) as usize;
    let pinned_count = pane
        .panel_ids
        .iter()
        .filter(|id| pinned.contains(id.as_str()))
        .count();
    let tier_index = if moved_is_pinned {
        requested_index.min(pinned_count)
    } else {
        requested_index.max(pinned_count)
    };
    pane.panel_ids.insert(tier_index, panel_id.to_string());
    if focus {
        pane.selected_panel_id = Some(panel_id.to_string());
    }
    true
}

/// Move a surface to a pane in the same or another workspace. Destination
/// indexes are bonsplit insertion offsets: omitted means append, bounds clamp,
/// and pinned/unpinned surfaces remain within their tier. The mutation is
/// transactional; an invalid source, workspace, or pane leaves `tabs`
/// unchanged. Returns `None` for an invalid request, otherwise whether state
/// changed.
pub fn move_surface(
    tabs: &mut SessionTabManagerSnapshot,
    source_workspace_index: usize,
    panel_id: &str,
    target_workspace_index: usize,
    target_pane_id: &str,
    destination_index: Option<i64>,
    focus: bool,
) -> Option<bool> {
    let source_workspace = tabs.workspaces.get(source_workspace_index)?;
    let source_pane = pane_containing_panel(source_workspace.layout.as_ref()?, panel_id)?;
    let target_workspace = tabs.workspaces.get(target_workspace_index)?;
    pane_by_id(target_workspace.layout.as_ref()?, target_pane_id)?;

    if source_workspace_index == target_workspace_index
        && source_pane.pane_id.as_deref() == Some(target_pane_id)
    {
        let destination_index = destination_index.unwrap_or(source_pane.panel_ids.len() as i64);
        return reorder_surface(
            tabs.workspaces.get_mut(source_workspace_index)?,
            panel_id,
            destination_index,
            focus,
        );
    }

    let moved_is_pinned = source_workspace
        .panel_pins
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|entry| entry.panel_id == panel_id && entry.is_pinned);
    let mut next = tabs.clone();
    let metadata = if source_workspace_index == target_workspace_index {
        None
    } else {
        Some(detach_panel_metadata(
            next.workspaces.get_mut(source_workspace_index)?,
            panel_id,
        ))
    };
    let source = next.workspaces.get_mut(source_workspace_index)?;
    if matches!(
        close_panel(&mut source.layout, panel_id),
        CloseOutcome::NotFound
    ) {
        return None;
    }
    if source.zoomed_panel_id.as_deref() == Some(panel_id) {
        source.zoomed_panel_id = None;
    }
    let target = next.workspaces.get_mut(target_workspace_index)?;
    if !insert_surface_into_pane(
        target,
        panel_id,
        target_pane_id,
        destination_index,
        moved_is_pinned,
        focus,
    ) {
        return None;
    }
    if focus {
        target.focused_panel_id = Some(panel_id.to_string());
    }
    if let Some(metadata) = metadata {
        attach_panel_metadata(target, metadata);
    }
    if focus {
        next.selected_workspace_index = Some(target_workspace_index as i64);
    }
    *tabs = next;
    Some(true)
}

/// Set or clear a panel/tab manual unread indicator within `workspace`. The
/// target panel must exist in the workspace layout. Read panels are represented
/// by absence, so clearing the last unread entry removes `panel_unreads`.
///
/// Returns `true` iff the stored metadata actually changed.
pub fn set_panel_unread(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    is_unread: bool,
) -> bool {
    set_panel_unread_at(workspace, panel_id, is_unread, None)
}

/// Timestamp-aware variant of [`set_panel_unread`]. New unread entries carry
/// `unread_at` when the caller has a clock; read/legacy callers can pass `None`.
pub fn set_panel_unread_at(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    is_unread: bool,
    unread_at: Option<i64>,
) -> bool {
    let Some(layout) = workspace.layout.as_ref() else {
        return false;
    };
    if !contains_panel(layout, panel_id) {
        return false;
    }

    let mut unreads = workspace.panel_unreads.take().unwrap_or_default();
    let existing = unreads
        .iter()
        .find(|entry| entry.panel_id == panel_id)
        .is_some_and(|entry| entry.is_unread);
    if existing == is_unread {
        workspace.panel_unreads = (!unreads.is_empty()).then_some(unreads);
        return false;
    }

    unreads.retain(|entry| entry.panel_id != panel_id);
    if is_unread {
        unreads.push(SessionPanelUnreadSnapshot {
            panel_id: panel_id.to_string(),
            is_unread: true,
            unread_at,
        });
    }
    workspace.panel_unreads = (!unreads.is_empty()).then_some(unreads);
    true
}

fn first_panel_id(layout: &Layout) -> Option<String> {
    match layout {
        Layout::Pane(pane) => pane
            .selected_panel_id
            .clone()
            .or_else(|| pane.panel_ids.first().cloned()),
        Layout::Split(split) => {
            first_panel_id(&split.first).or_else(|| first_panel_id(&split.second))
        }
    }
}

fn workspace_has_unread_panel(workspace: &SessionWorkspaceSnapshot) -> bool {
    workspace
        .panel_unreads
        .as_ref()
        .is_some_and(|unreads| unreads.iter().any(|entry| entry.is_unread))
}

/// Mark a workspace read/unread using the representative panel unread marker.
/// In the full notification model, manual workspace unread is owned by the
/// notification store and mirrored onto a representative panel. The desktop
/// snapshot currently owns that mirror, so this op updates the panel metadata
/// directly while preserving the same user-visible read/unread affordance.
pub fn set_workspace_unread(
    tabs: &mut SessionTabManagerSnapshot,
    index: i64,
    preferred_panel_id: Option<&str>,
    is_unread: bool,
) -> bool {
    set_workspace_unread_at(tabs, index, preferred_panel_id, is_unread, None)
}

/// Timestamp-aware variant of [`set_workspace_unread`]. The timestamp is stored
/// on the representative panel unread entry created for the workspace.
pub fn set_workspace_unread_at(
    tabs: &mut SessionTabManagerSnapshot,
    index: i64,
    preferred_panel_id: Option<&str>,
    is_unread: bool,
    unread_at: Option<i64>,
) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() {
        return false;
    }
    let workspace = &mut tabs.workspaces[index as usize];
    if !is_unread {
        if workspace.panel_unreads.is_none() {
            return false;
        }
        workspace.panel_unreads = None;
        return true;
    }
    if workspace_has_unread_panel(workspace) {
        return false;
    }

    let Some(layout) = workspace.layout.as_ref() else {
        return false;
    };
    let panel_id = preferred_panel_id
        .filter(|panel_id| contains_panel(layout, panel_id))
        .map(str::to_owned)
        .or_else(|| first_panel_id(layout));
    let Some(panel_id) = panel_id else {
        return false;
    };
    set_panel_unread_at(workspace, &panel_id, true, unread_at)
}

/// Pin/unpin the workspace at `index` — the port of canonical
/// `WorkspaceReorderCoordinator.setPinned`
/// (`WorkspaceReorderCoordinator.swift:467-472`) plus its pinned-ahead
/// normalization `reorderTabForPinnedState` (`:529-539`), reached via
/// `TabManager.setPinned` (`TabManager.swift:1754-1759`).
///
/// Semantics:
/// - Already-at-value is a no-op (`guard tab.isPinned != pinned`,
///   Coordinator:468). Out-of-range/negative `index` is a silent no-op
///   (rename precedent).
/// - GROUPED workspace (`group_id` present): pinning never ejects a tab from its
///   group. Canonical then runs `normalizeWorkspaceGroupContiguity`
///   (Coordinator:531-533); the port mirrors that through the snapshot mirror,
///   so any pre-existing broken group run is repaired after the flag change.
/// - UNGROUPED: remove the tab, count the leading globally-pinned rows of the
///   REMAINDER (`leadingGlobalPinnedRowCount` + `isGlobalPinnedRow`,
///   `WorkspacesModel+Ordering.swift:190-207`: grouped rows count by their
///   GROUP's pin, ungrouped by their own flag), and re-insert at that
///   boundary (Coordinator:535-538). Because the flag is flipped before the
///   move, the single rule yields both directions: pin → END of the pinned
///   prefix, unpin → FRONT of the unpinned segment; all other rows keep
///   their relative order.
/// - SELECTION: canonical selection is id-based and untouched by `setPinned`;
///   the port's `selected_workspace_index` is index-based, so it is remapped
///   to keep following the same workspace across the move.
///
/// PERSISTENCE DECISION: canonical `SessionWorkspaceSnapshot.isPinned` is a
/// non-optional `Bool` (`SessionPersistence.swift:1833`), but the port models
/// it as omit-when-`None` `Option<bool>` for golden byte-stability (see
/// `session.rs`). Pin writes `Some(true)`; unpin writes `None`, never
/// `Some(false)`. A canonical-written `Some(false)` reads as unpinned, so
/// unpinning it is caught by the already-at-value guard and leaves it as-is.
///
/// Returns `true` iff the pin state actually changed (drives the emit gate).
pub fn set_workspace_pinned(
    tabs: &mut SessionTabManagerSnapshot,
    index: i64,
    pinned: bool,
) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() {
        return false;
    }
    let from = index as usize;
    let was = tabs.workspaces[from].is_pinned == Some(true);
    if was == pinned {
        return false;
    }
    // Some(true)/None per the persistence decision above. An unpin of a
    // decoded `Some(false)` never reaches here (already-unpinned no-op).
    tabs.workspaces[from].is_pinned = pinned.then_some(true);

    if tabs.workspaces[from].group_id.is_some() {
        // Group membership is preserved; canonical still repairs any broken
        // group runs after changing the flag.
        normalize_workspace_groups_in_snapshot(tabs);
        return true;
    }

    // The boundary move (Coordinator:535-538): remove first so the leading
    // count runs over the remaining rows, then insert at the boundary.
    let moved = tabs.workspaces.remove(from);
    let to = {
        let groups = tabs.workspace_groups.as_deref().unwrap_or(&[]);
        let boundary = tabs
            .workspaces
            .iter()
            .take_while(|w| match w.group_id.as_deref() {
                // Grouped rows count by their GROUP's pin (isGlobalPinnedRow,
                // Ordering.swift:201-207); a dangling group id falls back to
                // the row's own flag, exactly like the oracle's nil-group arm.
                Some(gid) => groups
                    .iter()
                    .find(|g| g.id == gid)
                    .map_or(w.is_pinned == Some(true), |g| g.is_pinned == Some(true)),
                None => w.is_pinned == Some(true),
            })
            .count();
        boundary.min(tabs.workspaces.len())
    };
    tabs.workspaces.insert(to, moved);

    // Index-based selection follows the same workspace (canonical id-based
    // selection is inherently untouched). Invalid/None selection stays as-is.
    if let Some(sel) = tabs.selected_workspace_index {
        if sel >= 0 && (sel as usize) < tabs.workspaces.len() {
            let sel = sel as usize;
            let next = if sel == from {
                to
            } else {
                // Simulate the remove (positions after `from` shift left) then
                // the insert (positions at/after `to` shift right).
                let s = if sel > from { sel - 1 } else { sel };
                if s >= to {
                    s + 1
                } else {
                    s
                }
            };
            tabs.selected_workspace_index = Some(next as i64);
        }
    }
    true
}

#[cfg(test)]
#[path = "session_ops/tests.rs"]
mod tests;
