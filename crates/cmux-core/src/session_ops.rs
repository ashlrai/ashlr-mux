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
    AppSessionSnapshot, SessionCanvasPaneSnapshot, SessionPaneLayoutSnapshot,
    SessionPanelListeningPortsSnapshot, SessionPanelPinSnapshot,
    SessionPanelRestorableAgentSnapshot, SessionPanelShellActivitySnapshot,
    SessionPanelTerminalStartupSnapshot, SessionPanelTitleSnapshot, SessionPanelTtySnapshot,
    SessionPanelUnreadSnapshot, SessionSplitLayoutSnapshot, SessionSplitOrientation,
    SessionTabManagerSnapshot, SessionWorkspaceGroupSnapshot, SessionWorkspaceLayoutSnapshot,
    SessionWorkspaceSnapshot,
};
use crate::surface_lifecycle::{LifecycleError, SurfaceLifecycleModel};

mod browser;

pub use browser::{
    browser_go_back, browser_go_forward, browser_navigation_availability, clear_browser_history,
    ensure_browser_page_zoom, navigate_browser, serializable_browser_history_url,
    set_browser_page_zoom, set_browser_url, show_browser_developer_tools,
    toggle_browser_developer_tools, toggle_browser_focus_mode, toggle_browser_omnibar_visible,
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

/// Divider ratios are clamped to [0.1, 0.9] — byte-for-byte the macOS bonsplit
/// bound and the web `splitLayout.ts` clamp. Keeps a pane from collapsing to
/// zero width/height.
pub const MIN_DIVIDER: f64 = 0.1;
pub const MAX_DIVIDER: f64 = 0.9;
const CANVAS_SEED_WIDTH: i64 = 1200;
const CANVAS_SEED_HEIGHT: i64 = 800;

const CANVAS_DEFAULT_GAP: i64 = 16;

/// One step down the split tree, addressing a child of a split node. Serialized
/// as `"first"`/`"second"` to match the web-side `SplitPath`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitChild {
    First,
    Second,
}

/// What happened when closing a panel. The Tauri layer uses this to decide
/// whether the owning workspace should be dropped (`Emptied`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseOutcome {
    /// No pane in the tree held the panel id.
    NotFound,
    /// The panel was removed; the layout still has at least one pane.
    Removed,
    /// The last panel was removed; the layout is now empty (set to `None`).
    Emptied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasAction {
    Tidy,
    AlignLeft,
    AlignRight,
    AlignTop,
    AlignBottom,
    EqualizeWidths,
    EqualizeHeights,
    DistributeHorizontally,
    DistributeVertically,
}

impl CanvasAction {
    fn parse(action: &str) -> Option<Self> {
        let normalized = action
            .trim()
            .chars()
            .filter(|ch| !matches!(ch, '-' | '_' | '.' | ' '))
            .flat_map(char::to_lowercase)
            .collect::<String>();
        match normalized.as_str() {
            "tidy" | "canvastidy" => Some(Self::Tidy),
            "alignleft" | "canvasalignleft" => Some(Self::AlignLeft),
            "alignright" | "canvasalignright" => Some(Self::AlignRight),
            "aligntop" | "canvasaligntop" => Some(Self::AlignTop),
            "alignbottom" | "canvasalignbottom" => Some(Self::AlignBottom),
            "equalizewidths" | "canvasequalizewidths" => Some(Self::EqualizeWidths),
            "equalizeheights" | "canvasequalizeheights" => Some(Self::EqualizeHeights),
            "distributehorizontally" | "canvasdistributehorizontally" => {
                Some(Self::DistributeHorizontally)
            }
            "distributevertically" | "canvasdistributevertically" => {
                Some(Self::DistributeVertically)
            }
            _ => None,
        }
    }
}

/// Clamp a divider ratio into the legal range; NaN falls back to centered.
pub fn clamp_divider(position: f64) -> f64 {
    if position.is_nan() {
        return 0.5;
    }
    position.clamp(MIN_DIVIDER, MAX_DIVIDER)
}

/// A fresh single-pane layout holding one panel. New panes default to a terminal
/// surface (`surface_kind: None`); flip to an agent session with
/// [`set_surface_kind`].
pub fn single_pane(panel_id: impl Into<String>) -> Layout {
    let id = panel_id.into();
    Layout::Pane(SessionPaneLayoutSnapshot {
        pane_id: None,
        selected_panel_id: Some(id.clone()),
        panel_ids: vec![id],
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
    })
}

fn empty_pane() -> Layout {
    Layout::Pane(SessionPaneLayoutSnapshot {
        pane_id: None,
        panel_ids: Vec::new(),
        selected_panel_id: None,
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
    })
}

/// Set the `surface_kind` of the pane that holds `panel_id` (`None` clears it
/// back to a terminal). Returns `false` (a no-op) if no pane holds `panel_id`.
/// The kind rides on the pane node, so it survives splits (the pane keeps its
/// side of the new split) and divider moves.
pub fn set_surface_kind(node: &mut Layout, panel_id: &str, kind: Option<String>) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                p.surface_kind = kind;
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            set_surface_kind(&mut s.first, panel_id, kind.clone())
                || set_surface_kind(&mut s.second, panel_id, kind)
        }
    }
}

/// Select the next/previous panel id within the pane that contains `panel_id`.
/// Returns true only when the selected panel actually changes.
pub fn select_adjacent_panel(node: &mut Layout, panel_id: &str, next: bool) -> bool {
    match node {
        Layout::Pane(p) => {
            if !p.panel_ids.iter().any(|id| id == panel_id) {
                return false;
            }
            let count = p.panel_ids.len();
            if count <= 1 {
                return false;
            }
            let current_index = p
                .selected_panel_id
                .as_deref()
                .and_then(|selected| p.panel_ids.iter().position(|id| id == selected))
                .or_else(|| p.panel_ids.iter().position(|id| id == panel_id))
                .unwrap_or(0);
            let next_index = if next {
                (current_index + 1) % count
            } else {
                (current_index + count - 1) % count
            };
            let selected = p.panel_ids[next_index].clone();
            if p.selected_panel_id.as_deref() == Some(selected.as_str()) {
                return false;
            }
            p.selected_panel_id = Some(selected);
            true
        }
        Layout::Split(s) => {
            select_adjacent_panel(&mut s.first, panel_id, next)
                || select_adjacent_panel(&mut s.second, panel_id, next)
        }
    }
}

/// Select `panel_id` within the pane that contains it. Returns true only when
/// the pane's selected panel actually changes.
pub fn select_panel(node: &mut Layout, panel_id: &str) -> bool {
    match node {
        Layout::Pane(pane) => {
            if !pane.panel_ids.iter().any(|id| id == panel_id) {
                return false;
            }
            if pane.selected_panel_id.as_deref() == Some(panel_id) {
                return false;
            }
            pane.selected_panel_id = Some(panel_id.to_string());
            true
        }
        Layout::Split(split) => {
            select_panel(&mut split.first, panel_id) || select_panel(&mut split.second, panel_id)
        }
    }
}

/// Add `new_panel_id` as a sibling tab in the pane containing
/// `anchor_panel_id`, immediately after the anchor, and select it.
pub fn add_panel_to_pane(node: &mut Layout, anchor_panel_id: &str, new_panel_id: &str) -> bool {
    match node {
        Layout::Pane(pane) => {
            let Some(anchor_index) = pane
                .panel_ids
                .iter()
                .position(|panel_id| panel_id == anchor_panel_id)
            else {
                return false;
            };
            if pane
                .panel_ids
                .iter()
                .any(|panel_id| panel_id == new_panel_id)
            {
                return false;
            }
            pane.panel_ids
                .insert(anchor_index + 1, new_panel_id.to_string());
            pane.selected_panel_id = Some(new_panel_id.to_string());
            true
        }
        Layout::Split(split) => {
            add_panel_to_pane(&mut split.first, anchor_panel_id, new_panel_id)
                || add_panel_to_pane(&mut split.second, anchor_panel_id, new_panel_id)
        }
    }
}

/// Bind or clear the markdown file path of the pane that holds `panel_id`.
/// Returns `false` if no pane holds the id. The file path rides on the pane
/// node itself, so it survives splits and divider moves with the same
/// persistence semantics as `surface_kind`.
pub fn set_markdown_file_path(
    node: &mut Layout,
    panel_id: &str,
    file_path: Option<String>,
) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                p.markdown_file_path = file_path;
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            set_markdown_file_path(&mut s.first, panel_id, file_path.clone())
                || set_markdown_file_path(&mut s.second, panel_id, file_path)
        }
    }
}

/// Bind or clear the plain-text file path of the pane that holds `panel_id`.
/// Returns `false` if no pane holds the id. The file path rides on the pane
/// node itself, so it survives splits and divider moves with the same
/// persistence semantics as `surface_kind`.
pub fn set_file_path(node: &mut Layout, panel_id: &str, file_path: Option<String>) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                p.file_path = file_path;
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            set_file_path(&mut s.first, panel_id, file_path.clone())
                || set_file_path(&mut s.second, panel_id, file_path)
        }
    }
}

/// Bind or clear the diff-viewer session of the pane that holds `panel_id`.
/// Returns `false` if no pane holds the id. The token + request path ride on
/// the pane node, so they survive splits and divider moves with the same
/// persistence semantics as `surface_kind`.
pub fn set_diff_viewer_session(
    node: &mut Layout,
    panel_id: &str,
    token: Option<String>,
    request_path: Option<String>,
) -> bool {
    match node {
        Layout::Pane(p) => {
            if p.panel_ids.iter().any(|id| id == panel_id) {
                p.diff_viewer_token = token;
                p.diff_viewer_request_path = request_path;
                true
            } else {
                false
            }
        }
        Layout::Split(s) => {
            set_diff_viewer_session(&mut s.first, panel_id, token.clone(), request_path.clone())
                || set_diff_viewer_session(&mut s.second, panel_id, token, request_path)
        }
    }
}

/// Number of leaf panes in a subtree.
pub fn count_leaves(layout: &Layout) -> usize {
    match layout {
        Layout::Pane(_) => 1,
        Layout::Split(s) => count_leaves(&s.first) + count_leaves(&s.second),
    }
}

/// Whether any pane in the subtree holds `panel_id`.
pub fn contains_panel(layout: &Layout, panel_id: &str) -> bool {
    match layout {
        Layout::Pane(p) => p.panel_ids.iter().any(|id| id == panel_id),
        Layout::Split(s) => {
            contains_panel(&s.first, panel_id) || contains_panel(&s.second, panel_id)
        }
    }
}

/// Toggle split zoom for `panel_id` within a workspace. A split zoom only makes
/// sense when the workspace has at least two leaf panes; missing/single-pane
/// layouts are no-ops.
pub fn toggle_split_zoom(workspace: &mut SessionWorkspaceSnapshot, panel_id: &str) -> bool {
    let Some(layout) = workspace.layout.as_ref() else {
        return false;
    };
    if count_leaves(layout) <= 1 || !contains_panel(layout, panel_id) {
        return false;
    }
    if workspace.zoomed_panel_id.as_deref() == Some(panel_id) {
        workspace.zoomed_panel_id = None;
    } else {
        workspace.zoomed_panel_id = Some(panel_id.to_string());
    }
    true
}

fn selected_panel_id(pane: &SessionPaneLayoutSnapshot) -> Option<&str> {
    pane.selected_panel_id
        .as_deref()
        .filter(|selected| pane.panel_ids.iter().any(|id| id == selected))
        .or_else(|| pane.panel_ids.first().map(String::as_str))
}

fn canvas_panes_from_layout_rect(
    layout: &Layout,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
    out: &mut Vec<SessionCanvasPaneSnapshot>,
) {
    match layout {
        Layout::Pane(pane) => {
            let Some(panel_id) = selected_panel_id(pane) else {
                return;
            };
            out.push(SessionCanvasPaneSnapshot {
                panel_id: panel_id.to_string(),
                x,
                y,
                width,
                height,
                panel_ids: (pane.panel_ids.len() > 1).then(|| pane.panel_ids.clone()),
                selected_panel_id: pane.selected_panel_id.clone(),
            });
        }
        Layout::Split(split) => {
            let divider = clamp_divider(split.divider_position);
            match split.orientation {
                SessionSplitOrientation::Horizontal => {
                    let first_width = ((width as f64) * divider).round() as i64;
                    let first_width = first_width.clamp(1, width.saturating_sub(1).max(1));
                    let second_width = width.saturating_sub(first_width);
                    canvas_panes_from_layout_rect(&split.first, x, y, first_width, height, out);
                    canvas_panes_from_layout_rect(
                        &split.second,
                        x.saturating_add(first_width),
                        y,
                        second_width,
                        height,
                        out,
                    );
                }
                SessionSplitOrientation::Vertical => {
                    let first_height = ((height as f64) * divider).round() as i64;
                    let first_height = first_height.clamp(1, height.saturating_sub(1).max(1));
                    let second_height = height.saturating_sub(first_height);
                    canvas_panes_from_layout_rect(&split.first, x, y, width, first_height, out);
                    canvas_panes_from_layout_rect(
                        &split.second,
                        x,
                        y.saturating_add(first_height),
                        width,
                        second_height,
                        out,
                    );
                }
            }
        }
    }
}

/// Seed the workspace's canvas pane list from its current split tree. The seed
/// preserves split ratios inside a deterministic 1200x800 canvas so the first
/// canvas render has stable geometry even before the richer canvas engine
/// starts mutating frames.
pub fn canvas_panes_from_layout(layout: &Layout) -> Vec<SessionCanvasPaneSnapshot> {
    let mut panes = Vec::new();
    canvas_panes_from_layout_rect(
        layout,
        0,
        0,
        CANVAS_SEED_WIDTH,
        CANVAS_SEED_HEIGHT,
        &mut panes,
    );
    panes
}

fn normalized_layout_mode(mode: Option<&str>) -> Option<String> {
    match mode.map(str::trim).filter(|mode| !mode.is_empty()) {
        Some(mode) if mode.eq_ignore_ascii_case("canvas") => Some("canvas".to_string()),
        Some(mode) if mode.eq_ignore_ascii_case("split") => None,
        Some(mode) if mode.eq_ignore_ascii_case("default") => None,
        Some(_) | None => None,
    }
}

/// Set the workspace layout mode. `None`, `"split"`, and `"default"` all mean
/// the canonical split layout; `"canvas"` flips the workspace into canvas mode
/// and lazily seeds `canvas_panes` from the current split tree when needed.
pub fn set_layout_mode(workspace: &mut SessionWorkspaceSnapshot, mode: Option<&str>) -> bool {
    let next = normalized_layout_mode(mode);
    let mut changed = workspace.layout_mode != next;
    workspace.layout_mode = next.clone();

    if next.as_deref() == Some("canvas") {
        let needs_seed = workspace
            .canvas_panes
            .as_ref()
            .is_none_or(|panes| panes.is_empty());
        if needs_seed {
            if let Some(layout) = workspace.layout.as_ref() {
                let panes = canvas_panes_from_layout(layout);
                if !panes.is_empty() {
                    workspace.canvas_panes = Some(panes);
                    changed = true;
                }
            }
        }
    }

    changed
}

fn canvas_pane_matches(pane: &SessionCanvasPaneSnapshot, panel_id: &str) -> bool {
    pane.panel_id == panel_id
        || pane.selected_panel_id.as_deref() == Some(panel_id)
        || pane
            .panel_ids
            .as_ref()
            .is_some_and(|ids| ids.iter().any(|id| id == panel_id))
}

/// Persist a canvas pane frame for `panel_id`, seeding from the split layout
/// first when the workspace has not entered canvas mode before.
pub fn set_canvas_pane_frame(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
) -> bool {
    if panel_id.trim().is_empty() {
        return false;
    }
    if workspace
        .canvas_panes
        .as_ref()
        .is_none_or(|panes| panes.is_empty())
    {
        if let Some(layout) = workspace.layout.as_ref() {
            let panes = canvas_panes_from_layout(layout);
            if !panes.is_empty() {
                workspace.canvas_panes = Some(panes);
            }
        }
    }

    let next = SessionCanvasPaneSnapshot {
        panel_id: panel_id.to_string(),
        x,
        y,
        width: width.max(1),
        height: height.max(1),
        panel_ids: None,
        selected_panel_id: Some(panel_id.to_string()),
    };
    let panes = workspace.canvas_panes.get_or_insert_with(Vec::new);
    if let Some(existing) = panes
        .iter_mut()
        .find(|pane| canvas_pane_matches(pane, panel_id))
    {
        if existing.x == next.x
            && existing.y == next.y
            && existing.width == next.width
            && existing.height == next.height
        {
            return false;
        }
        existing.x = next.x;
        existing.y = next.y;
        existing.width = next.width;
        existing.height = next.height;
        if existing.selected_panel_id.is_none() {
            existing.selected_panel_id = Some(panel_id.to_string());
        }
        true
    } else {
        panes.push(next);
        true
    }
}

/// Apply a canvas geometry command to the active workspace's persisted pane
/// frames. Until the web canvas grows multi-pane selection state, commands
/// operate on every canvas pane in z-order.
pub fn apply_canvas_action(workspace: &mut SessionWorkspaceSnapshot, action: &str) -> bool {
    apply_canvas_action_with_gap(workspace, action, None)
}

/// Apply a canvas geometry command with an optional configured pane gap.
/// Invalid gaps fall back to the shared default.
pub fn apply_canvas_action_with_gap(
    workspace: &mut SessionWorkspaceSnapshot,
    action: &str,
    pane_gap: Option<i64>,
) -> bool {
    if workspace.layout_mode.as_deref() != Some("canvas") {
        return false;
    }
    let Some(action) = CanvasAction::parse(action) else {
        return false;
    };
    if workspace
        .canvas_panes
        .as_ref()
        .is_none_or(|panes| panes.is_empty())
    {
        if let Some(layout) = workspace.layout.as_ref() {
            let panes = canvas_panes_from_layout(layout);
            if !panes.is_empty() {
                workspace.canvas_panes = Some(panes);
            }
        }
    }
    let Some(panes) = workspace.canvas_panes.as_mut() else {
        return false;
    };
    let indexes = canvas_valid_pane_indexes(panes);
    if indexes.len() < 2 {
        return false;
    }

    let updates = canvas_action_frames(panes, &indexes, action, canvas_gap(pane_gap));
    if updates.is_empty() {
        return false;
    }
    let mut changed = false;
    for (index, x, y, width, height) in updates {
        let pane = &mut panes[index];
        let width = width.max(1);
        let height = height.max(1);
        if (pane.x, pane.y, pane.width, pane.height) != (x, y, width, height) {
            pane.x = x;
            pane.y = y;
            pane.width = width;
            pane.height = height;
            changed = true;
        }
    }
    changed
}

fn canvas_gap(pane_gap: Option<i64>) -> i64 {
    pane_gap
        .filter(|gap| *gap >= 0)
        .unwrap_or(CANVAS_DEFAULT_GAP)
}

fn canvas_valid_pane_indexes(panes: &[SessionCanvasPaneSnapshot]) -> Vec<usize> {
    panes
        .iter()
        .enumerate()
        .filter_map(|(index, pane)| (pane.width > 0 && pane.height > 0).then_some(index))
        .collect()
}

fn canvas_action_frames(
    panes: &[SessionCanvasPaneSnapshot],
    indexes: &[usize],
    action: CanvasAction,
    gap: i64,
) -> Vec<(usize, i64, i64, i64, i64)> {
    match action {
        CanvasAction::AlignLeft => {
            let target = indexes
                .iter()
                .map(|index| panes[*index].x)
                .min()
                .unwrap_or(0);
            indexes
                .iter()
                .map(|index| {
                    let pane = &panes[*index];
                    (*index, target, pane.y, pane.width, pane.height)
                })
                .collect()
        }
        CanvasAction::AlignRight => {
            let target = indexes
                .iter()
                .map(|index| panes[*index].x + panes[*index].width)
                .max()
                .unwrap_or(0);
            indexes
                .iter()
                .map(|index| {
                    let pane = &panes[*index];
                    (*index, target - pane.width, pane.y, pane.width, pane.height)
                })
                .collect()
        }
        CanvasAction::AlignTop => {
            let target = indexes
                .iter()
                .map(|index| panes[*index].y)
                .min()
                .unwrap_or(0);
            indexes
                .iter()
                .map(|index| {
                    let pane = &panes[*index];
                    (*index, pane.x, target, pane.width, pane.height)
                })
                .collect()
        }
        CanvasAction::AlignBottom => {
            let target = indexes
                .iter()
                .map(|index| panes[*index].y + panes[*index].height)
                .max()
                .unwrap_or(0);
            indexes
                .iter()
                .map(|index| {
                    let pane = &panes[*index];
                    (
                        *index,
                        pane.x,
                        target - pane.height,
                        pane.width,
                        pane.height,
                    )
                })
                .collect()
        }
        CanvasAction::EqualizeWidths => {
            let target = indexes
                .iter()
                .map(|index| panes[*index].width)
                .max()
                .unwrap_or(1);
            indexes
                .iter()
                .map(|index| {
                    let pane = &panes[*index];
                    (*index, pane.x, pane.y, target, pane.height)
                })
                .collect()
        }
        CanvasAction::EqualizeHeights => {
            let target = indexes
                .iter()
                .map(|index| panes[*index].height)
                .max()
                .unwrap_or(1);
            indexes
                .iter()
                .map(|index| {
                    let pane = &panes[*index];
                    (*index, pane.x, pane.y, pane.width, target)
                })
                .collect()
        }
        CanvasAction::DistributeHorizontally => {
            distributed_canvas_frames(panes, indexes, true, gap)
        }
        CanvasAction::DistributeVertically => distributed_canvas_frames(panes, indexes, false, gap),
        CanvasAction::Tidy => tidy_canvas_frames(panes, indexes, gap),
    }
}

fn distributed_canvas_frames(
    panes: &[SessionCanvasPaneSnapshot],
    indexes: &[usize],
    horizontally: bool,
    gap: i64,
) -> Vec<(usize, i64, i64, i64, i64)> {
    let mut sorted = indexes.to_vec();
    sorted.sort_by(|lhs, rhs| {
        let left = if horizontally {
            panes[*lhs].x
        } else {
            panes[*lhs].y
        };
        let right = if horizontally {
            panes[*rhs].x
        } else {
            panes[*rhs].y
        };
        left.cmp(&right)
            .then_with(|| panes[*lhs].panel_id.cmp(&panes[*rhs].panel_id))
    });
    let Some(first) = sorted.first().copied() else {
        return Vec::new();
    };
    let mut cursor = if horizontally {
        panes[first].x + panes[first].width
    } else {
        panes[first].y + panes[first].height
    };
    let mut updates = Vec::new();
    for index in sorted.into_iter().skip(1) {
        let pane = &panes[index];
        if horizontally {
            let x = cursor + gap;
            updates.push((index, x, pane.y, pane.width, pane.height));
            cursor = x + pane.width;
        } else {
            let y = cursor + gap;
            updates.push((index, pane.x, y, pane.width, pane.height));
            cursor = y + pane.height;
        }
    }
    updates
}

fn tidy_canvas_frames(
    panes: &[SessionCanvasPaneSnapshot],
    indexes: &[usize],
    gap: i64,
) -> Vec<(usize, i64, i64, i64, i64)> {
    let origin_x = indexes
        .iter()
        .map(|index| panes[*index].x)
        .min()
        .unwrap_or(0);
    let origin_y = indexes
        .iter()
        .map(|index| panes[*index].y)
        .min()
        .unwrap_or(0);
    let mut sorted = indexes.to_vec();
    sorted.sort_by(|lhs, rhs| {
        let left_mid_y = panes[*lhs].y + panes[*lhs].height / 2;
        let right_mid_y = panes[*rhs].y + panes[*rhs].height / 2;
        let left_mid_x = panes[*lhs].x + panes[*lhs].width / 2;
        let right_mid_x = panes[*rhs].x + panes[*rhs].width / 2;
        left_mid_y
            .cmp(&right_mid_y)
            .then_with(|| left_mid_x.cmp(&right_mid_x))
            .then_with(|| panes[*lhs].panel_id.cmp(&panes[*rhs].panel_id))
    });

    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut row_bottom = i64::MIN;
    for index in sorted {
        let pane = &panes[index];
        let mid_y = pane.y + pane.height / 2;
        if rows.is_empty() || mid_y >= row_bottom {
            row_bottom = pane.y + pane.height;
            rows.push(vec![index]);
        } else {
            row_bottom = row_bottom.max(pane.y + pane.height);
            rows.last_mut().unwrap().push(index);
        }
    }

    let mut y = origin_y;
    let mut updates = Vec::new();
    for row in rows {
        let row_height = row
            .iter()
            .map(|index| panes[*index].height)
            .max()
            .unwrap_or(0);
        let mut x = origin_x;
        for index in row {
            let pane = &panes[index];
            updates.push((index, x, y, pane.width, pane.height));
            x += pane.width + gap;
        }
        y += row_height + gap;
    }
    updates
}

/// Set the OSC/runtime title of the exact surface whose layout owns `panel_id`.
///
/// This is the workspace-title feed for a terminal pane's top label
/// (`cmux-terminal::top_label` `panelTitles`, seeded `"Terminal"` at panel
/// creation): the incoming OSC title is trimmed and an empty title is dropped
/// (a blank title never clobbers a real one), last-write-wins. The exact surface
/// always retains the runtime title. `process_title` follows it only when this
/// is the workspace's sole panel and no custom workspace title is set.
///
/// Returns `true` iff either the exact surface title or eligible workspace
/// process title changed.
///
/// NOTE (minor divergence): Swift trims with `.whitespacesAndNewlines`; this
/// uses Rust `str::trim` (Unicode `White_Space`). The two differ only on exotic
/// separators an OSC title never carries in practice.
pub fn set_process_title(
    tabs: &mut SessionTabManagerSnapshot,
    panel_id: &str,
    title: &str,
) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return false;
    }
    for workspace in &mut tabs.workspaces {
        let Some(layout) = workspace
            .layout
            .as_ref()
            .filter(|layout| contains_panel(layout, panel_id))
        else {
            continue;
        };
        let mut changed = false;
        if let Some(surface) = workspace
            .surfaces
            .as_mut()
            .and_then(|surfaces| surfaces.iter_mut().find(|row| row.surface_id == panel_id))
        {
            if surface.metadata.runtime_title.as_deref() != Some(trimmed) {
                surface.metadata.runtime_title = Some(trimmed.to_string());
                changed = true;
            }
        }
        if panel_count(layout) == 1
            && workspace.custom_title.is_none()
            && workspace.process_title != trimmed
        {
            workspace.process_title = trimmed.to_string();
            changed = true;
        }
        return changed;
    }
    false
}

/// The equalized divider ratio for a split = the first subtree's share of leaf
/// panes (macOS `equalizeDividerPlan`: `firstSpanCount / totalSpanCount`).
pub fn equalize_divider(split: &SessionSplitLayoutSnapshot) -> f64 {
    let first = count_leaves(&split.first);
    let total = first + count_leaves(&split.second);
    if total == 0 {
        0.5
    } else {
        clamp_divider(first as f64 / total as f64)
    }
}

/// Orientation-aware span count, mirroring macOS
/// `ExternalTreeNode.spanCount(along:)`
/// (`Packages/macOS/CmuxPanes/Sources/CmuxPanes/Geometry/ExternalTreeNode+SplitGeometry.swift:69-81`).
///
/// A pane spans `1`. A nested split contributes its *recursive* span only when
/// its orientation matches `axis`; a differently-oriented subtree counts as a
/// single unit (span `1`). This is what makes equalize weight by same-axis panes
/// rather than by all leaves — e.g. in `H( V(a,b), c )` the `V(a,b)` subtree
/// counts as span `1` along the horizontal axis, so the root divides 0.5/0.5.
fn span_count(node: &Layout, axis: &SessionSplitOrientation) -> usize {
    match node {
        Layout::Pane(_) => 1,
        Layout::Split(s) => {
            if &s.orientation == axis {
                span_count(&s.first, axis) + span_count(&s.second, axis)
            } else {
                1
            }
        }
    }
}

/// Equalize **every** split divider in the subtree to its orientation-aware span
/// ratio (`firstSpanCount / totalSpanCount`), the Rust port of macOS
/// `equalizeDividerPlan`
/// (`ExternalTreeNode+SplitGeometry.swift:14-81`). Returns whether the subtree
/// contained at least one split — mirroring canonical `foundSplit` (a lone pane
/// yields `false`, i.e. a no-op).
///
/// Unlike per-split [`equalize_divider`] (which weights by *all* leaves via
/// [`count_leaves`]), this uses [`span_count`], so differently-oriented subtrees
/// count as one span. The two diverge on mixed-orientation trees; this matches
/// canonical macOS.
///
/// Canonical walks post-order only because its controller applies side effects
/// per node; here the mutation just sets each `divider_position`, which never
/// changes span counts, so recursion order is irrelevant.
pub fn equalize_dividers(node: &mut Layout) -> bool {
    let Layout::Split(s) = node else {
        return false;
    };
    let first_span = span_count(&s.first, &s.orientation);
    let total_span = first_span + span_count(&s.second, &s.orientation);
    s.divider_position = clamp_divider(first_span as f64 / total_span as f64);
    equalize_dividers(&mut s.first);
    equalize_dividers(&mut s.second);
    true
}

/// Set the `divider_position` of the split reached by `path` (empty path = the
/// root split). Returns `false` (a no-op) if the path runs off a leaf.
pub fn set_divider_at_path(node: &mut Layout, path: &[SplitChild], position: f64) -> bool {
    let Layout::Split(split) = node else {
        return false;
    };
    match path.split_first() {
        None => {
            split.divider_position = clamp_divider(position);
            true
        }
        Some((head, rest)) => {
            let child = match head {
                SplitChild::First => split.first.as_mut(),
                SplitChild::Second => split.second.as_mut(),
            };
            set_divider_at_path(child, rest, position)
        }
    }
}

fn pane_contains(node: &Layout, target: &str) -> bool {
    matches!(node, Layout::Pane(p) if p.panel_ids.iter().any(|id| id == target))
}

/// Split the pane that holds `target_panel_id` into two, adding a new pane for
/// `new_panel_id`. The new pane goes to the `first` side when `insert_first`,
/// else `second`; the existing pane takes the other side. The new split is
/// centered (`divider_position = 0.5`). Returns `false` if no pane holds
/// `target_panel_id`.
pub fn split_pane(
    node: &mut Layout,
    target_panel_id: &str,
    orientation: SessionSplitOrientation,
    new_panel_id: impl Into<String>,
    insert_first: bool,
) -> bool {
    split_pane_impl(
        node,
        target_panel_id,
        &orientation,
        &new_panel_id.into(),
        insert_first,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitOffSurfaceError {
    SurfaceNotFound,
    WouldEmptySourcePane,
}

/// Move an existing surface tab into a new split adjacent to its source pane.
/// The source must retain at least one tab. Pane-local presentation state is
/// copied to the new leaf because the Windows snapshot currently stores that
/// state on panes; the stateful layer assigns the new pane identity.
pub fn split_off_surface(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
    orientation: SessionSplitOrientation,
    insert_first: bool,
) -> Result<(), SplitOffSurfaceError> {
    fn split_off(
        layout: &mut Layout,
        panel_id: &str,
        orientation: &SessionSplitOrientation,
        insert_first: bool,
    ) -> Result<(), SplitOffSurfaceError> {
        match layout {
            Layout::Pane(pane) => {
                let Some(index) = pane.panel_ids.iter().position(|id| id == panel_id) else {
                    return Err(SplitOffSurfaceError::SurfaceNotFound);
                };
                if pane.panel_ids.len() <= 1 {
                    return Err(SplitOffSurfaceError::WouldEmptySourcePane);
                }
                let mut source = pane.clone();
                source.panel_ids.remove(index);
                if source.selected_panel_id.as_deref() == Some(panel_id) {
                    source.selected_panel_id = source.panel_ids.first().cloned();
                }
                let mut moved = pane.clone();
                moved.pane_id = None;
                moved.panel_ids = vec![panel_id.to_string()];
                moved.selected_panel_id = Some(panel_id.to_string());
                let (first, second) = if insert_first {
                    (Layout::Pane(moved), Layout::Pane(source))
                } else {
                    (Layout::Pane(source), Layout::Pane(moved))
                };
                *layout = Layout::Split(SessionSplitLayoutSnapshot {
                    split_id: None,
                    orientation: orientation.clone(),
                    divider_position: 0.5,
                    first: Box::new(first),
                    second: Box::new(second),
                });
                Ok(())
            }
            Layout::Split(split) => {
                match split_off(&mut split.first, panel_id, orientation, insert_first) {
                    Err(SplitOffSurfaceError::SurfaceNotFound) => {
                        split_off(&mut split.second, panel_id, orientation, insert_first)
                    }
                    result => result,
                }
            }
        }
    }

    split_off(
        workspace
            .layout
            .as_mut()
            .ok_or(SplitOffSurfaceError::SurfaceNotFound)?,
        panel_id,
        &orientation,
        insert_first,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneSwapResult {
    pub source_surface_id: String,
    pub target_surface_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneSwapError {
    SamePane,
    SourcePaneNotFound,
    TargetPaneNotFound,
    BothPanesNeedSurface,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneLastResult {
    pub pane_id: String,
    pub surface_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneLastError {
    NoFocusedPane,
    NoAlternatePane,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocusError {
    PaneNotFound,
}

/// Resolve the stable pane identity and its selected surface without changing
/// pane-local tab selection.
pub fn focus_pane_target(
    workspace: &SessionWorkspaceSnapshot,
    pane_id: &str,
) -> Result<PaneLastResult, PaneFocusError> {
    let pane = workspace
        .layout
        .as_ref()
        .and_then(|layout| pane_by_id(layout, pane_id))
        .ok_or(PaneFocusError::PaneNotFound)?;
    Ok(PaneLastResult {
        pane_id: pane_id.to_string(),
        surface_id: pane
            .selected_panel_id
            .clone()
            .filter(|id| pane.panel_ids.contains(id))
            .or_else(|| pane.panel_ids.first().cloned()),
    })
}

/// Resolve canonical `pane.last`: validate the focused pane, then choose the
/// first pane in layout order whose identity differs from it.
pub fn focus_alternate_pane(
    workspace: &SessionWorkspaceSnapshot,
    focused_pane_id: Option<&str>,
) -> Result<PaneLastResult, PaneLastError> {
    fn first_other<'a>(
        layout: &'a Layout,
        focused_pane_id: &str,
    ) -> Option<&'a SessionPaneLayoutSnapshot> {
        match layout {
            Layout::Pane(pane) => pane
                .pane_id
                .as_deref()
                .is_some_and(|id| id != focused_pane_id)
                .then_some(pane),
            Layout::Split(split) => first_other(&split.first, focused_pane_id)
                .or_else(|| first_other(&split.second, focused_pane_id)),
        }
    }
    let focused_pane_id = focused_pane_id.ok_or(PaneLastError::NoFocusedPane)?;
    let layout = workspace
        .layout
        .as_ref()
        .ok_or(PaneLastError::NoFocusedPane)?;
    pane_by_id(layout, focused_pane_id).ok_or(PaneLastError::NoFocusedPane)?;
    let target = first_other(layout, focused_pane_id).ok_or(PaneLastError::NoAlternatePane)?;
    let pane_id = target
        .pane_id
        .clone()
        .ok_or(PaneLastError::NoAlternatePane)?;
    Ok(PaneLastResult {
        pane_id,
        surface_id: target
            .selected_panel_id
            .clone()
            .filter(|id| target.panel_ids.contains(id))
            .or_else(|| target.panel_ids.first().cloned()),
    })
}

pub fn pane_id_containing_surface<'a>(
    workspace: &'a SessionWorkspaceSnapshot,
    panel_id: &str,
) -> Option<&'a str> {
    pane_containing_panel(workspace.layout.as_ref()?, panel_id)?
        .pane_id
        .as_deref()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneResizeDirection {
    Left,
    Right,
    Up,
    Down,
}

impl PaneResizeDirection {
    fn orientation(self) -> SessionSplitOrientation {
        match self {
            Self::Left | Self::Right => SessionSplitOrientation::Horizontal,
            Self::Up | Self::Down => SessionSplitOrientation::Vertical,
        }
    }

    fn requires_first_child(self) -> bool {
        matches!(self, Self::Right | Self::Down)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PaneResizeResult {
    pub split_id: String,
    pub old_divider_position: f64,
    pub new_divider_position: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneResizeError {
    PaneNotFoundInTree,
    NoOrientationSplitAncestor,
    NoAdjacentBorder,
    MissingSplitIdentity,
}

struct PaneResizeCandidate {
    path: Vec<SplitChild>,
    split_id: Option<String>,
    orientation: SessionSplitOrientation,
    pane_in_first_child: bool,
    divider_position: f64,
    axis_pixels: f64,
}

fn pane_resize_candidates(
    layout: &Layout,
    pane_id: &str,
    width: f64,
    height: f64,
    path: &mut Vec<SplitChild>,
    candidates: &mut Vec<PaneResizeCandidate>,
) -> bool {
    match layout {
        Layout::Pane(pane) => pane.pane_id.as_deref() == Some(pane_id),
        Layout::Split(split) => {
            let divider = clamp_divider(split.divider_position);
            let (first_width, first_height, second_width, second_height) = match split.orientation {
                SessionSplitOrientation::Horizontal => {
                    let first_width = width * divider;
                    (first_width, height, width - first_width, height)
                }
                SessionSplitOrientation::Vertical => {
                    let first_height = height * divider;
                    (width, first_height, width, height - first_height)
                }
            };
            path.push(SplitChild::First);
            let first_contains = pane_resize_candidates(
                &split.first,
                pane_id,
                first_width,
                first_height,
                path,
                candidates,
            );
            path.pop();
            path.push(SplitChild::Second);
            let second_contains = pane_resize_candidates(
                &split.second,
                pane_id,
                second_width,
                second_height,
                path,
                candidates,
            );
            path.pop();
            if first_contains || second_contains {
                candidates.push(PaneResizeCandidate {
                    path: path.clone(),
                    split_id: split.split_id.clone(),
                    orientation: split.orientation.clone(),
                    pane_in_first_child: first_contains,
                    divider_position: split.divider_position,
                    axis_pixels: match split.orientation {
                        SessionSplitOrientation::Horizontal => width.max(1.0),
                        SessionSplitOrientation::Vertical => height.max(1.0),
                    },
                });
            }
            first_contains || second_contains
        }
    }
}

fn apply_pane_resize(
    workspace: &mut SessionWorkspaceSnapshot,
    candidate: &PaneResizeCandidate,
    new_position: f64,
) -> Result<PaneResizeResult, PaneResizeError> {
    let split_id = candidate
        .split_id
        .clone()
        .ok_or(PaneResizeError::MissingSplitIdentity)?;
    let mut next = workspace
        .layout
        .clone()
        .ok_or(PaneResizeError::PaneNotFoundInTree)?;
    if !set_divider_at_path(&mut next, &candidate.path, new_position) {
        return Err(PaneResizeError::PaneNotFoundInTree);
    }
    let new_divider_position = clamp_divider(new_position);
    workspace.layout = Some(next);
    Ok(PaneResizeResult {
        split_id,
        old_divider_position: candidate.divider_position,
        new_divider_position,
    })
}

pub fn resize_pane_relative(
    workspace: &mut SessionWorkspaceSnapshot,
    pane_id: &str,
    direction: PaneResizeDirection,
    amount: u64,
    width: f64,
    height: f64,
) -> Result<PaneResizeResult, PaneResizeError> {
    let mut candidates = Vec::new();
    let contains_target = workspace.layout.as_ref().is_some_and(|layout| {
        pane_resize_candidates(
            layout,
            pane_id,
            width.max(1.0),
            height.max(1.0),
            &mut Vec::new(),
            &mut candidates,
        )
    });
    if !contains_target {
        return Err(PaneResizeError::PaneNotFoundInTree);
    }
    let orientation = direction.orientation();
    if !candidates
        .iter()
        .any(|candidate| candidate.orientation == orientation)
    {
        return Err(PaneResizeError::NoOrientationSplitAncestor);
    }
    let candidate = candidates
        .iter()
        .find(|candidate| {
            candidate.orientation == orientation
                && candidate.pane_in_first_child == direction.requires_first_child()
        })
        .ok_or(PaneResizeError::NoAdjacentBorder)?;
    let sign = if direction.requires_first_child() {
        1.0
    } else {
        -1.0
    };
    let requested = candidate.divider_position + sign * amount as f64 / candidate.axis_pixels;
    apply_pane_resize(workspace, candidate, requested)
}

pub fn resize_pane_absolute(
    workspace: &mut SessionWorkspaceSnapshot,
    pane_id: &str,
    axis: SessionSplitOrientation,
    target_pixels: f64,
    width: f64,
    height: f64,
) -> Result<PaneResizeResult, PaneResizeError> {
    let mut candidates = Vec::new();
    let contains_target = workspace.layout.as_ref().is_some_and(|layout| {
        pane_resize_candidates(
            layout,
            pane_id,
            width.max(1.0),
            height.max(1.0),
            &mut Vec::new(),
            &mut candidates,
        )
    });
    if !contains_target {
        return Err(PaneResizeError::PaneNotFoundInTree);
    }
    let candidate = candidates
        .iter()
        .find(|candidate| candidate.orientation == axis)
        .ok_or(PaneResizeError::NoOrientationSplitAncestor)?;
    let fraction = target_pixels / candidate.axis_pixels;
    let requested = if candidate.pane_in_first_child {
        fraction
    } else {
        1.0 - fraction
    };
    apply_pane_resize(workspace, candidate, requested)
}

fn remove_surface_from_pane(layout: &mut Layout, pane_id: &str, panel_id: &str) -> bool {
    let Some(pane) = pane_by_id_mut(layout, pane_id) else {
        return false;
    };
    let Some(index) = pane.panel_ids.iter().position(|id| id == panel_id) else {
        return false;
    };
    pane.panel_ids.remove(index);
    if pane.selected_panel_id.as_deref() == Some(panel_id) {
        pane.selected_panel_id = pane
            .panel_ids
            .get(index)
            .or_else(|| pane.panel_ids.last())
            .cloned();
    }
    true
}

fn append_surface_to_pane(
    layout: &mut Layout,
    pane_id: &str,
    panel_id: &str,
    pinned: &HashSet<&str>,
) -> bool {
    let Some(pane) = pane_by_id_mut(layout, pane_id) else {
        return false;
    };
    let pinned_count = pane
        .panel_ids
        .iter()
        .filter(|id| pinned.contains(id.as_str()))
        .count();
    let index = if pinned.contains(panel_id) {
        pinned_count
    } else {
        pane.panel_ids.len()
    };
    pane.panel_ids.insert(index, panel_id.to_string());
    if pane.selected_panel_id.is_none() {
        pane.selected_panel_id = Some(panel_id.to_string());
    }
    true
}

/// Swap the selected surfaces of two panes while preserving both pane IDs.
/// This directly models canonical's placeholder-assisted two-move sequence:
/// selected tabs leave their panes, each enters the other pane at the end of
/// its pin tier, and a singleton pane selects the arriving surface.
pub fn swap_selected_pane_surfaces(
    workspace: &mut SessionWorkspaceSnapshot,
    source_pane_id: &str,
    target_pane_id: &str,
) -> Result<PaneSwapResult, PaneSwapError> {
    if source_pane_id == target_pane_id {
        return Err(PaneSwapError::SamePane);
    }
    let layout = workspace
        .layout
        .as_ref()
        .ok_or(PaneSwapError::SourcePaneNotFound)?;
    let source = pane_by_id(layout, source_pane_id).ok_or(PaneSwapError::SourcePaneNotFound)?;
    let target = pane_by_id(layout, target_pane_id).ok_or(PaneSwapError::TargetPaneNotFound)?;
    let source_surface_id = source
        .selected_panel_id
        .clone()
        .filter(|id| source.panel_ids.contains(id))
        .ok_or(PaneSwapError::BothPanesNeedSurface)?;
    let target_surface_id = target
        .selected_panel_id
        .clone()
        .filter(|id| target.panel_ids.contains(id))
        .ok_or(PaneSwapError::BothPanesNeedSurface)?;
    let pinned: HashSet<&str> = workspace
        .panel_pins
        .as_deref()
        .unwrap_or_default()
        .iter()
        .filter(|entry| entry.is_pinned)
        .map(|entry| entry.panel_id.as_str())
        .collect();
    let mut next = workspace.layout.clone().expect("layout validated above");
    if !remove_surface_from_pane(&mut next, source_pane_id, &source_surface_id)
        || !remove_surface_from_pane(&mut next, target_pane_id, &target_surface_id)
        || !append_surface_to_pane(&mut next, target_pane_id, &source_surface_id, &pinned)
        || !append_surface_to_pane(&mut next, source_pane_id, &target_surface_id, &pinned)
    {
        return Err(PaneSwapError::BothPanesNeedSurface);
    }
    workspace.layout = Some(next);
    Ok(PaneSwapResult {
        source_surface_id,
        target_surface_id,
    })
}

fn split_pane_impl(
    node: &mut Layout,
    target_panel_id: &str,
    orientation: &SessionSplitOrientation,
    new_panel_id: &str,
    insert_first: bool,
) -> bool {
    if pane_contains(node, target_panel_id) {
        let existing = std::mem::replace(node, empty_pane());
        let new_pane = single_pane(new_panel_id);
        let (first, second) = if insert_first {
            (new_pane, existing)
        } else {
            (existing, new_pane)
        };
        *node = Layout::Split(SessionSplitLayoutSnapshot {
            split_id: None,
            orientation: orientation.clone(),
            divider_position: 0.5,
            first: Box::new(first),
            second: Box::new(second),
        });
        return true;
    }
    match node {
        Layout::Split(s) => {
            split_pane_impl(
                &mut s.first,
                target_panel_id,
                orientation,
                new_panel_id,
                insert_first,
            ) || split_pane_impl(
                &mut s.second,
                target_panel_id,
                orientation,
                new_panel_id,
                insert_first,
            )
        }
        Layout::Pane(_) => false,
    }
}

/// Removes `panel_id` from whichever pane holds it, collapsing an emptied split
/// into its surviving sibling. Operates on the workspace's `Option<layout>` so
/// emptying the last pane clears the layout to `None`.
pub fn close_panel(layout: &mut Option<Layout>, panel_id: &str) -> CloseOutcome {
    let Some(root) = layout.as_mut() else {
        return CloseOutcome::NotFound;
    };
    match remove_from_node(root, panel_id) {
        NodeEdit::NotFound => CloseOutcome::NotFound,
        NodeEdit::RemovedFromPane => CloseOutcome::Removed,
        NodeEdit::RemovePane => {
            // The root pane itself emptied (no parent split to collapse into).
            *layout = None;
            CloseOutcome::Emptied
        }
    }
}

enum NodeEdit {
    NotFound,
    RemovedFromPane,
    RemovePane,
}

fn take_layout(boxed: &mut Box<Layout>) -> Layout {
    std::mem::replace(boxed.as_mut(), empty_pane())
}

fn remove_from_node(node: &mut Layout, panel_id: &str) -> NodeEdit {
    let collapse_to: Option<Layout>;
    match node {
        Layout::Pane(p) => {
            let Some(index) = p.panel_ids.iter().position(|id| id == panel_id) else {
                return NodeEdit::NotFound;
            };
            p.panel_ids.remove(index);
            if p.selected_panel_id.as_deref() == Some(panel_id) {
                p.selected_panel_id = p.panel_ids.first().cloned();
            }
            return if p.panel_ids.is_empty() {
                NodeEdit::RemovePane
            } else {
                NodeEdit::RemovedFromPane
            };
        }
        Layout::Split(s) => match remove_from_node(&mut s.first, panel_id) {
            NodeEdit::RemovedFromPane => return NodeEdit::RemovedFromPane,
            NodeEdit::RemovePane => {
                // First child emptied → collapse this split into the second.
                collapse_to = Some(take_layout(&mut s.second));
            }
            NodeEdit::NotFound => match remove_from_node(&mut s.second, panel_id) {
                NodeEdit::RemovedFromPane => return NodeEdit::RemovedFromPane,
                NodeEdit::RemovePane => {
                    collapse_to = Some(take_layout(&mut s.first));
                }
                NodeEdit::NotFound => return NodeEdit::NotFound,
            },
        },
    }
    // The `match node` borrow has ended; perform the collapse the child asked
    // for by replacing this split node with its surviving subtree.
    if let Some(survivor) = collapse_to {
        *node = survivor;
    }
    NodeEdit::RemovedFromPane
}

fn panel_count(layout: &Layout) -> usize {
    match layout {
        Layout::Pane(pane) => pane.panel_ids.len(),
        Layout::Split(split) => panel_count(&split.first) + panel_count(&split.second),
    }
}

fn pane_for_panel(layout: &Layout, panel_id: &str) -> Option<SessionPaneLayoutSnapshot> {
    match layout {
        Layout::Pane(pane) => {
            if pane.panel_ids.iter().any(|id| id == panel_id) {
                let mut pane = pane.clone();
                pane.panel_ids = vec![panel_id.to_string()];
                pane.selected_panel_id = Some(panel_id.to_string());
                Some(pane)
            } else {
                None
            }
        }
        Layout::Split(split) => pane_for_panel(&split.first, panel_id)
            .or_else(|| pane_for_panel(&split.second, panel_id)),
    }
}

fn take_panel_title(
    titles: &mut Option<Vec<SessionPanelTitleSnapshot>>,
    panel_id: &str,
) -> Option<SessionPanelTitleSnapshot> {
    let entries = titles.as_mut()?;
    let index = entries
        .iter()
        .position(|entry| entry.panel_id == panel_id)?;
    let entry = entries.remove(index);
    if entries.is_empty() {
        *titles = None;
    }
    Some(entry)
}

fn take_panel_pin(
    pins: &mut Option<Vec<SessionPanelPinSnapshot>>,
    panel_id: &str,
) -> Option<SessionPanelPinSnapshot> {
    let entries = pins.as_mut()?;
    let index = entries
        .iter()
        .position(|entry| entry.panel_id == panel_id)?;
    let entry = entries.remove(index);
    if entries.is_empty() {
        *pins = None;
    }
    Some(entry)
}

fn take_panel_unread(
    unreads: &mut Option<Vec<SessionPanelUnreadSnapshot>>,
    panel_id: &str,
) -> Option<SessionPanelUnreadSnapshot> {
    let entries = unreads.as_mut()?;
    let index = entries
        .iter()
        .position(|entry| entry.panel_id == panel_id)?;
    let entry = entries.remove(index);
    if entries.is_empty() {
        *unreads = None;
    }
    Some(entry)
}

fn take_panel_restorable_agent(
    agents: &mut Option<Vec<SessionPanelRestorableAgentSnapshot>>,
    panel_id: &str,
) -> Option<SessionPanelRestorableAgentSnapshot> {
    let entries = agents.as_mut()?;
    let index = entries
        .iter()
        .position(|entry| entry.panel_id == panel_id)?;
    let entry = entries.remove(index);
    if entries.is_empty() {
        *agents = None;
    }
    Some(entry)
}

fn take_panel_terminal_startup(
    startups: &mut Option<Vec<SessionPanelTerminalStartupSnapshot>>,
    panel_id: &str,
) -> Option<SessionPanelTerminalStartupSnapshot> {
    let entries = startups.as_mut()?;
    let index = entries
        .iter()
        .position(|entry| entry.panel_id == panel_id)?;
    let entry = entries.remove(index);
    if entries.is_empty() {
        *startups = None;
    }
    Some(entry)
}

fn take_panel_listening_ports(
    ports: &mut Option<Vec<SessionPanelListeningPortsSnapshot>>,
    panel_id: &str,
) -> Option<SessionPanelListeningPortsSnapshot> {
    let entries = ports.as_mut()?;
    let index = entries
        .iter()
        .position(|entry| entry.panel_id == panel_id)?;
    let entry = entries.remove(index);
    if entries.is_empty() {
        *ports = None;
    }
    Some(entry)
}

fn take_panel_tty(
    ttys: &mut Option<Vec<SessionPanelTtySnapshot>>,
    panel_id: &str,
) -> Option<SessionPanelTtySnapshot> {
    let entries = ttys.as_mut()?;
    let index = entries
        .iter()
        .position(|entry| entry.panel_id == panel_id)?;
    let entry = entries.remove(index);
    if entries.is_empty() {
        *ttys = None;
    }
    Some(entry)
}

fn take_panel_shell_activity(
    shell_activity: &mut Option<Vec<SessionPanelShellActivitySnapshot>>,
    panel_id: &str,
) -> Option<SessionPanelShellActivitySnapshot> {
    let entries = shell_activity.as_mut()?;
    let index = entries
        .iter()
        .position(|entry| entry.panel_id == panel_id)?;
    let entry = entries.remove(index);
    if entries.is_empty() {
        *shell_activity = None;
    }
    Some(entry)
}

#[derive(Default)]
struct DetachedPanelMetadata {
    title: Option<SessionPanelTitleSnapshot>,
    pin: Option<SessionPanelPinSnapshot>,
    unread: Option<SessionPanelUnreadSnapshot>,
    restorable_agent: Option<SessionPanelRestorableAgentSnapshot>,
    terminal_startup: Option<SessionPanelTerminalStartupSnapshot>,
    listening_ports: Option<SessionPanelListeningPortsSnapshot>,
    tty: Option<SessionPanelTtySnapshot>,
    shell_activity: Option<SessionPanelShellActivitySnapshot>,
}

fn detach_panel_metadata(
    workspace: &mut SessionWorkspaceSnapshot,
    panel_id: &str,
) -> DetachedPanelMetadata {
    let metadata = DetachedPanelMetadata {
        title: take_panel_title(&mut workspace.panel_titles, panel_id),
        pin: take_panel_pin(&mut workspace.panel_pins, panel_id),
        unread: take_panel_unread(&mut workspace.panel_unreads, panel_id),
        restorable_agent: take_panel_restorable_agent(
            &mut workspace.restorable_agent_snapshots,
            panel_id,
        ),
        terminal_startup: take_panel_terminal_startup(
            &mut workspace.panel_terminal_startups,
            panel_id,
        ),
        listening_ports: take_panel_listening_ports(&mut workspace.panel_listening_ports, panel_id),
        tty: take_panel_tty(&mut workspace.panel_ttys, panel_id),
        shell_activity: take_panel_shell_activity(&mut workspace.panel_shell_activity, panel_id),
    };
    if metadata.listening_ports.is_some() {
        recompute_workspace_listening_ports(workspace);
    }
    metadata
}

fn attach_panel_metadata(
    workspace: &mut SessionWorkspaceSnapshot,
    metadata: DetachedPanelMetadata,
) {
    if let Some(entry) = metadata.title {
        workspace
            .panel_titles
            .get_or_insert_with(Vec::new)
            .push(entry);
    }
    if let Some(entry) = metadata.pin {
        workspace
            .panel_pins
            .get_or_insert_with(Vec::new)
            .push(entry);
    }
    if let Some(entry) = metadata.unread {
        workspace
            .panel_unreads
            .get_or_insert_with(Vec::new)
            .push(entry);
    }
    if let Some(entry) = metadata.restorable_agent {
        workspace
            .restorable_agent_snapshots
            .get_or_insert_with(Vec::new)
            .push(entry);
    }
    if let Some(entry) = metadata.terminal_startup {
        workspace
            .panel_terminal_startups
            .get_or_insert_with(Vec::new)
            .push(entry);
    }
    if let Some(entry) = metadata.listening_ports {
        workspace
            .panel_listening_ports
            .get_or_insert_with(Vec::new)
            .push(entry);
        recompute_workspace_listening_ports(workspace);
    }
    if let Some(entry) = metadata.tty {
        workspace
            .panel_ttys
            .get_or_insert_with(Vec::new)
            .push(entry);
    }
    if let Some(entry) = metadata.shell_activity {
        workspace
            .panel_shell_activity
            .get_or_insert_with(Vec::new)
            .push(entry);
    }
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
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    if ports.is_empty() {
        workspace.listening_ports = None;
        return;
    }
    let mut ports = ports;
    ports.sort_unstable();
    workspace.listening_ports = Some(ports);
}

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

/// Placement of an existing or freshly-created workspace inside a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceGroupPlacement {
    /// Immediately after a supplied reference member, or after the anchor
    /// when no reference is supplied.
    AfterCurrent,
    /// Immediately after the anchor.
    Top,
    /// After the group's last member.
    End,
}

/// Pure-model failures that command adapters can translate into their own
/// transport error vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceGroupMutationError {
    GroupNotFound,
    GroupAlreadyExists,
    AnchorWorkspaceNotFound,
    WorkspaceNotFound,
    WorkspaceNotGrouped,
    WorkspaceNotGroupMember,
    WorkspaceIsOtherGroupAnchor,
    InvalidReferenceWorkspace,
}

fn workspace_snapshot_index(tabs: &SessionTabManagerSnapshot, workspace_id: Uuid) -> Option<usize> {
    tabs.workspaces.iter().position(|workspace| {
        workspace
            .workspace_id
            .as_deref()
            .and_then(|raw| Uuid::parse_str(raw).ok())
            == Some(workspace_id)
    })
}

fn workspace_group_snapshot_index(
    tabs: &SessionTabManagerSnapshot,
    group_id: Uuid,
) -> Option<usize> {
    tabs.workspace_groups
        .as_deref()?
        .iter()
        .position(|group| Uuid::parse_str(&group.id).ok() == Some(group_id))
}

fn selected_workspace_uuid(tabs: &SessionTabManagerSnapshot) -> Option<Uuid> {
    let index = usize::try_from(tabs.selected_workspace_index?).ok()?;
    tabs.workspaces
        .get(index)?
        .workspace_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok())
}

fn restore_selected_workspace_uuid(
    tabs: &mut SessionTabManagerSnapshot,
    selected_workspace_id: Option<Uuid>,
) {
    let Some(selected_workspace_id) = selected_workspace_id else {
        return;
    };
    if let Some(index) = workspace_snapshot_index(tabs, selected_workspace_id) {
        tabs.selected_workspace_index = Some(index as i64);
    }
}

fn resolved_workspace_group_name(tabs: &SessionTabManagerSnapshot, requested: &str) -> String {
    let trimmed = requested.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    let groups = tabs.workspace_groups.as_deref().unwrap_or(&[]);
    let used: HashSet<&str> = groups.iter().map(|group| group.name.as_str()).collect();
    let mut number = groups.len() + 1;
    loop {
        let candidate = format!("Group {number}");
        if !used.contains(candidate.as_str()) {
            return candidate;
        }
        number += 1;
    }
}

/// Form a new group around an already-created fresh anchor workspace.
///
/// This is the pure snapshot half of canonical `createWorkspaceGroup`: the
/// host creates the anchor/runtime first, then calls this operation with its
/// identity. Existing pinned workspaces and anchors of other groups are
/// ineligible and are silently skipped, matching the coordinator. Eligible
/// children are adopted in tab order, and the new contiguous anchor-first run
/// occupies the first eligible child's former top-level slot. An empty eligible
/// set leaves an anchor-only group in the normal unpinned tier. Selection is
/// preserved by workspace identity.
pub fn create_workspace_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    name: &str,
    anchor_workspace_id: Uuid,
    child_workspace_ids: &[Uuid],
) -> Result<SessionWorkspaceGroupSnapshot, WorkspaceGroupMutationError> {
    if workspace_group_snapshot_index(tabs, group_id).is_some() {
        return Err(WorkspaceGroupMutationError::GroupAlreadyExists);
    }
    let anchor_index = workspace_snapshot_index(tabs, anchor_workspace_id)
        .ok_or(WorkspaceGroupMutationError::AnchorWorkspaceNotFound)?;
    let selected_workspace_id = selected_workspace_uuid(tabs);

    let original_tab_order: Vec<Uuid> = tabs
        .workspaces
        .iter()
        .filter_map(|workspace| {
            workspace
                .workspace_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
        })
        .filter(|id| *id != anchor_workspace_id)
        .collect();
    let existing_anchor_ids: HashSet<Uuid> = tabs
        .workspace_groups
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .filter_map(|group| {
            group
                .anchor_workspace_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
        })
        .collect();
    let requested: HashSet<Uuid> = child_workspace_ids.iter().copied().collect();
    let eligible_children: Vec<Uuid> = original_tab_order
        .iter()
        .copied()
        .filter(|id| requested.contains(id))
        .filter(|id| !existing_anchor_ids.contains(id))
        .filter(|id| {
            workspace_snapshot_index(tabs, *id)
                .is_some_and(|index| tabs.workspaces[index].is_pinned != Some(true))
        })
        .collect();

    let group_id_string = group_id.to_string();
    tabs.workspaces[anchor_index].group_id = Some(group_id_string.clone());
    for child_id in &eligible_children {
        if let Some(index) = workspace_snapshot_index(tabs, *child_id) {
            tabs.workspaces[index].group_id = Some(group_id_string.clone());
        }
    }
    let created = SessionWorkspaceGroupSnapshot {
        id: group_id_string,
        name: resolved_workspace_group_name(tabs, name),
        is_collapsed: false,
        anchor_workspace_id: Some(anchor_workspace_id.to_string()),
        anchor_member_index: None,
        is_pinned: None,
        custom_color: None,
        icon_symbol: None,
    };
    tabs.workspace_groups
        .get_or_insert_with(Vec::new)
        .push(created.clone());

    let (rows, groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let (final_rows, final_groups) = if let Some(first_child) = eligible_children.first() {
        let child_set: HashSet<Uuid> = eligible_children.iter().copied().collect();
        let mut desired = Vec::with_capacity(rows.len());
        for id in &original_tab_order {
            if id == first_child {
                desired.push(anchor_workspace_id);
                desired.extend(eligible_children.iter().copied());
            }
            if !child_set.contains(id) {
                desired.push(*id);
            }
        }
        let preferred = top_level_workspace_ids_preserving_order(&rows, &groups, &desired);
        normalize_workspace_group_contiguity(&rows, &groups, Some(&preferred))
    } else {
        normalize_workspace_group_contiguity(&rows, &groups, None)
    };
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    if final_row_ids != original_row_ids {
        write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    } else if let Some(groups) = tabs.workspace_groups.as_mut() {
        let order: HashMap<Uuid, usize> = final_group_ids
            .iter()
            .enumerate()
            .map(|(index, id)| (*id, index))
            .collect();
        groups.sort_by_key(|group| {
            Uuid::parse_str(&group.id)
                .ok()
                .and_then(|id| order.get(&id).copied())
                .unwrap_or(usize::MAX)
        });
    }
    restore_selected_workspace_uuid(tabs, selected_workspace_id);
    Ok(created)
}

/// Dissolve a group in place, preserving all member workspaces and their exact
/// row positions. Returns member ids in row order for host event emission.
pub fn ungroup_workspace_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
) -> Result<Vec<Uuid>, WorkspaceGroupMutationError> {
    let group_index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let member_ids: Vec<Uuid> = tabs
        .workspaces
        .iter()
        .filter(|workspace| {
            workspace
                .group_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
                == Some(group_id)
        })
        .filter_map(|workspace| {
            workspace
                .workspace_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
        })
        .collect();
    for workspace in &mut tabs.workspaces {
        if workspace
            .group_id
            .as_deref()
            .and_then(|raw| Uuid::parse_str(raw).ok())
            == Some(group_id)
        {
            workspace.group_id = None;
        }
    }
    tabs.workspace_groups
        .as_mut()
        .expect("group index implies group storage")
        .remove(group_index);
    Ok(member_ids)
}

/// Rename a group after canonical whitespace trimming. A blank name is a
/// successful no-op; `Ok(false)` also represents an already-equal value.
pub fn rename_workspace_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    name: &str,
) -> Result<bool, WorkspaceGroupMutationError> {
    let index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let trimmed = name.trim();
    if trimmed.is_empty() || tabs.workspace_groups.as_ref().unwrap()[index].name == trimmed {
        return Ok(false);
    }
    tabs.workspace_groups.as_mut().unwrap()[index].name = trimmed.to_string();
    Ok(true)
}

/// Set a group's pin state and restore canonical group contiguity/pin tiers.
pub fn set_workspace_group_pinned_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    pinned: bool,
) -> Result<bool, WorkspaceGroupMutationError> {
    let index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    if (tabs.workspace_groups.as_ref().unwrap()[index].is_pinned == Some(true)) == pinned {
        return Ok(false);
    }
    tabs.workspace_groups.as_mut().unwrap()[index].is_pinned = pinned.then_some(true);
    normalize_workspace_groups_in_snapshot(tabs);
    Ok(true)
}

/// Set or clear a group's exact custom color value.
pub fn set_workspace_group_color_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    color: Option<String>,
) -> Result<bool, WorkspaceGroupMutationError> {
    let index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let group = &mut tabs.workspace_groups.as_mut().unwrap()[index];
    if group.custom_color == color {
        return Ok(false);
    }
    group.custom_color = color;
    Ok(true)
}

/// Store the host-normalized group icon symbol (or clear it).
pub fn set_workspace_group_icon_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    icon_symbol: Option<String>,
) -> Result<bool, WorkspaceGroupMutationError> {
    let index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let group = &mut tabs.workspace_groups.as_mut().unwrap()[index];
    if group.icon_symbol == icon_symbol {
        return Ok(false);
    }
    group.icon_symbol = icon_symbol;
    Ok(true)
}

fn place_workspace_within_group(
    tabs: &mut SessionTabManagerSnapshot,
    workspace_id: Uuid,
    group_id: Uuid,
    placement: WorkspaceGroupPlacement,
    reference_workspace_id: Option<Uuid>,
) {
    let Some(current_index) = workspace_snapshot_index(tabs, workspace_id) else {
        return;
    };
    let Some(group_index) = workspace_group_snapshot_index(tabs, group_id) else {
        return;
    };
    let anchor_id = tabs.workspace_groups.as_ref().unwrap()[group_index]
        .anchor_workspace_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok());
    let member_indices: Vec<usize> = tabs
        .workspaces
        .iter()
        .enumerate()
        .filter(|(index, workspace)| {
            *index != current_index
                && workspace
                    .group_id
                    .as_deref()
                    .and_then(|raw| Uuid::parse_str(raw).ok())
                    == Some(group_id)
        })
        .map(|(index, _)| index)
        .collect();
    let target_index = match placement {
        WorkspaceGroupPlacement::AfterCurrent => reference_workspace_id
            .and_then(|reference| workspace_snapshot_index(tabs, reference))
            .map(|index| index + 1)
            .or_else(|| anchor_id.and_then(|anchor| workspace_snapshot_index(tabs, anchor)))
            .map(|index| index + 1)
            .or_else(|| member_indices.first().copied()),
        WorkspaceGroupPlacement::Top => anchor_id
            .and_then(|anchor| workspace_snapshot_index(tabs, anchor))
            .map(|index| index + 1)
            .or_else(|| member_indices.first().copied()),
        WorkspaceGroupPlacement::End => member_indices
            .last()
            .copied()
            .map(|index| index + 1)
            .or_else(|| {
                anchor_id
                    .and_then(|anchor| workspace_snapshot_index(tabs, anchor))
                    .map(|index| index + 1)
            }),
    };
    let Some(target_index) = target_index else {
        return;
    };
    if current_index == target_index {
        return;
    }
    let selected_workspace_id = selected_workspace_uuid(tabs);
    let workspace = tabs.workspaces.remove(current_index);
    let insert_at = if current_index < target_index {
        target_index - 1
    } else {
        target_index
    }
    .min(tabs.workspaces.len());
    tabs.workspaces.insert(insert_at, workspace);
    restore_selected_workspace_uuid(tabs, selected_workspace_id);
}

/// Add an existing workspace as a non-anchor group member. The move preserves
/// the group's former top-level slot, rejects anchors of other groups, and
/// optionally places the member at a canonical in-group position.
pub fn add_workspace_to_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    workspace_id: Uuid,
    placement: Option<WorkspaceGroupPlacement>,
    reference_workspace_id: Option<Uuid>,
) -> Result<bool, WorkspaceGroupMutationError> {
    if workspace_group_snapshot_index(tabs, group_id).is_none() {
        return Err(WorkspaceGroupMutationError::GroupNotFound);
    }
    let workspace_index = workspace_snapshot_index(tabs, workspace_id)
        .ok_or(WorkspaceGroupMutationError::WorkspaceNotFound)?;
    if let Some(reference_id) = reference_workspace_id {
        let valid_reference = workspace_snapshot_index(tabs, reference_id).is_some_and(|index| {
            tabs.workspaces[index]
                .group_id
                .as_deref()
                .and_then(|raw| Uuid::parse_str(raw).ok())
                == Some(group_id)
        });
        if !valid_reference {
            return Err(WorkspaceGroupMutationError::InvalidReferenceWorkspace);
        }
    }
    let current_group = tabs.workspaces[workspace_index]
        .group_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok());
    if current_group == Some(group_id) {
        return Ok(false);
    }
    let is_other_anchor = tabs
        .workspace_groups
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .any(|group| {
            Uuid::parse_str(&group.id).ok() != Some(group_id)
                && group
                    .anchor_workspace_id
                    .as_deref()
                    .and_then(|raw| Uuid::parse_str(raw).ok())
                    == Some(workspace_id)
        });
    if is_other_anchor {
        return Err(WorkspaceGroupMutationError::WorkspaceIsOtherGroupAnchor);
    }

    let (rows, groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let original_top_level_ids = sidebar_top_level_workspace_ids(&rows, &groups, None);
    tabs.workspaces[workspace_index].group_id = Some(group_id.to_string());
    let (assigned_rows, assigned_groups) = workspace_mirror(tabs);
    let expanded_groups = expand_workspace_group_for_selection_if_needed(
        &assigned_rows,
        &assigned_groups,
        selected_workspace_uuid(tabs),
    );
    if expanded_groups != assigned_groups {
        let expanded_by_id: HashMap<Uuid, bool> = expanded_groups
            .iter()
            .map(|group| (group.id, group.is_collapsed))
            .collect();
        for group in tabs.workspace_groups.as_mut().unwrap() {
            if let Some(collapsed) = Uuid::parse_str(&group.id)
                .ok()
                .and_then(|id| expanded_by_id.get(&id).copied())
            {
                group.is_collapsed = collapsed;
            }
        }
    }
    let preferred: Vec<Uuid> = original_top_level_ids
        .into_iter()
        .filter(|id| *id != workspace_id)
        .collect();
    let (final_rows, final_groups) =
        normalize_workspace_group_contiguity(&assigned_rows, &assigned_groups, Some(&preferred));
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    if final_row_ids != original_row_ids {
        write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    }
    if let Some(placement) = placement {
        place_workspace_within_group(
            tabs,
            workspace_id,
            group_id,
            placement,
            reference_workspace_id,
        );
    }
    Ok(true)
}

/// Remove a workspace from its group. Removing the anchor dissolves the whole
/// group in place; removing a child restores normal global group/pin ordering.
pub fn remove_workspace_from_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    workspace_id: Uuid,
) -> Result<bool, WorkspaceGroupMutationError> {
    let workspace_index = workspace_snapshot_index(tabs, workspace_id)
        .ok_or(WorkspaceGroupMutationError::WorkspaceNotFound)?;
    let group_id = tabs.workspaces[workspace_index]
        .group_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok())
        .ok_or(WorkspaceGroupMutationError::WorkspaceNotGrouped)?;
    let group_index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let anchor_id = tabs.workspace_groups.as_ref().unwrap()[group_index]
        .anchor_workspace_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok());
    if anchor_id == Some(workspace_id) {
        ungroup_workspace_group_snapshot(tabs, group_id)?;
        return Ok(true);
    }
    tabs.workspaces[workspace_index].group_id = None;
    normalize_workspace_groups_in_snapshot(tabs);
    Ok(true)
}

/// Make an existing member the group's anchor and hoist it to the front of the
/// contiguous member run. Legacy index-based anchor state is cleared so the
/// explicit stable identity remains authoritative.
pub fn set_workspace_group_anchor_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    workspace_id: Uuid,
) -> Result<bool, WorkspaceGroupMutationError> {
    let group_index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let workspace_index = workspace_snapshot_index(tabs, workspace_id)
        .ok_or(WorkspaceGroupMutationError::WorkspaceNotFound)?;
    let is_member = tabs.workspaces[workspace_index]
        .group_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok())
        == Some(group_id);
    if !is_member {
        return Err(WorkspaceGroupMutationError::WorkspaceNotGroupMember);
    }
    let group = &mut tabs.workspace_groups.as_mut().unwrap()[group_index];
    if group
        .anchor_workspace_id
        .as_deref()
        .and_then(|raw| Uuid::parse_str(raw).ok())
        == Some(workspace_id)
        && group.anchor_member_index.is_none()
    {
        return Ok(false);
    }
    group.anchor_workspace_id = Some(workspace_id.to_string());
    group.anchor_member_index = None;
    normalize_workspace_groups_in_snapshot(tabs);
    Ok(true)
}

/// Move a group to a final group-array index, clamped to the source group's pin
/// tier, then project the new group-slot order back into the workspace rows
/// without moving ungrouped top-level slots.
pub fn move_workspace_group_snapshot(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: Uuid,
    target_index: i64,
) -> Result<bool, WorkspaceGroupMutationError> {
    let current_index = workspace_group_snapshot_index(tabs, group_id)
        .ok_or(WorkspaceGroupMutationError::GroupNotFound)?;
    let groups = tabs.workspace_groups.as_ref().unwrap();
    let group_count = groups.len();
    let pinned = groups[current_index].is_pinned == Some(true);
    let same_tier: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter(|(_, group)| (group.is_pinned == Some(true)) == pinned)
        .map(|(index, _)| index)
        .collect();
    let first = *same_tier
        .first()
        .expect("source group is in its own pin tier");
    let last = *same_tier
        .last()
        .expect("source group is in its own pin tier");
    let clamped = target_index.max(first as i64).min(last as i64) as usize;
    if current_index == clamped {
        return Ok(false);
    }
    let moved = tabs
        .workspace_groups
        .as_mut()
        .unwrap()
        .remove(current_index);
    tabs.workspace_groups
        .as_mut()
        .unwrap()
        .insert(clamped.min(group_count - 1), moved);

    let (rows, model_groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let top_level_ids = sidebar_top_level_workspace_ids(&rows, &model_groups, None);
    let pinned_ids = sidebar_top_level_pinned_workspace_ids(&rows, &model_groups);
    let tiered_top_level_ids: Vec<Uuid> = top_level_ids
        .iter()
        .copied()
        .filter(|id| pinned_ids.contains(id))
        .chain(
            top_level_ids
                .iter()
                .copied()
                .filter(|id| !pinned_ids.contains(id)),
        )
        .collect();
    let pinned_anchors: Vec<Uuid> = model_groups
        .iter()
        .filter(|group| group.is_pinned)
        .map(|group| group.anchor_workspace_id)
        .collect();
    let unpinned_anchors: Vec<Uuid> = model_groups
        .iter()
        .filter(|group| !group.is_pinned)
        .map(|group| group.anchor_workspace_id)
        .collect();
    let groups_by_anchor: HashMap<Uuid, &WorkspaceGroup> = model_groups
        .iter()
        .map(|group| (group.anchor_workspace_id, group))
        .collect();
    let mut pinned_index = 0;
    let mut unpinned_index = 0;
    let desired: Vec<Uuid> = tiered_top_level_ids
        .into_iter()
        .map(|id| match groups_by_anchor.get(&id) {
            Some(group) if group.is_pinned => {
                let replacement = pinned_anchors[pinned_index];
                pinned_index += 1;
                replacement
            }
            Some(_) => {
                let replacement = unpinned_anchors[unpinned_index];
                unpinned_index += 1;
                replacement
            }
            None => id,
        })
        .collect();
    let final_rows =
        normalize_workspace_group_runs_preserving_order(&rows, &model_groups, &desired);
    let final_groups = sync_workspace_groups_order_to_anchor_order(&final_rows, &model_groups);
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    Ok(true)
}

/// Set the collapsed flag of workspace group `group_id`. Mirrors canonical
/// `WorkspaceGroupCoordinator.setWorkspaceGroupCollapsed`
/// (`WorkspaceGroupCoordinator.swift:408-412`): the **pure data** variant —
/// unknown group id and already-at-value are both no-ops, and selection is
/// never touched (the anchor-selecting behavior belongs to the UI-only
/// `toggleWorkspaceGroupCollapsed`, not ported here). Returns whether the
/// flag actually changed.
pub fn set_group_collapsed(
    tabs: &mut SessionTabManagerSnapshot,
    group_id: &str,
    collapsed: bool,
) -> bool {
    let Some(groups) = tabs.workspace_groups.as_mut() else {
        return false;
    };
    let Some(group) = groups.iter_mut().find(|g| g.id == group_id) else {
        return false;
    };
    if group.is_collapsed == collapsed {
        return false;
    }
    group.is_collapsed = collapsed;
    true
}

/// Mirror a snapshot's workspaces/groups into the `cmux-workspaces` value types
/// the ported clamp/normalize math consumes. Returns rows PARALLEL to
/// `tabs.workspaces` positions plus the mapped groups in stored order.
///
/// DIVERGENCE from `sidebar_render`'s projection: rows are NEVER skipped — the
/// reorder clamps are positional, so skipping a row would shift indices. A
/// workspace with an absent/unparseable `workspace_id` gets a freshly MINTED v4
/// id (collision with stored ids is negligible and the version bits differ; a
/// duplicated stored id is de-duplicated the same way), purely to give the row
/// a stable handle for the permutation write-back — the snapshot itself is
/// never rewritten with minted ids. A row `group_id` that parses but references
/// no known group is LEFT dangling: the crate fns already fall back correctly
/// (`isGlobalPinnedRow`'s nil-group arm, `WorkspacesModel+Ordering.swift`
/// :201-207) and the normalize pass clears it in the MIRROR only.
///
/// Groups map with the same rules as `sidebar_render.rs`: unparseable id and
/// member-less groups are skipped, a duplicate group id keeps the first
/// occurrence, and the anchor resolves via the oracle's 3-tier fallback
/// (`TabManager.swift:6018-6027`: `anchor_member_index` into the members in
/// row order → stored `anchor_workspace_id` when still a member → first
/// member). `name`/`custom_color`/`icon_symbol` pass through (inert for
/// ordering).
fn workspace_mirror(tabs: &SessionTabManagerSnapshot) -> (Vec<WorkspaceRow>, Vec<WorkspaceGroup>) {
    let mut used_ids: HashSet<Uuid> = HashSet::new();
    let rows: Vec<WorkspaceRow> = tabs
        .workspaces
        .iter()
        .map(|w| {
            let id = w
                .workspace_id
                .as_deref()
                .and_then(|s| Uuid::parse_str(s).ok())
                .filter(|id| !used_ids.contains(id))
                .unwrap_or_else(Uuid::new_v4);
            used_ids.insert(id);
            let group_id = w.group_id.as_deref().and_then(|s| Uuid::parse_str(s).ok());
            WorkspaceRow::new(id, group_id, w.is_pinned == Some(true))
        })
        .collect();

    // Members-by-group over the MIRROR rows, in row order — the oracle's
    // `workspaceIdsByGroupId` (TabManager.swift:6000-6008).
    let mut members_by_group_id: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for row in &rows {
        if let Some(gid) = row.group_id {
            members_by_group_id.entry(gid).or_default().push(row.id);
        }
    }

    let mut seen: HashSet<Uuid> = HashSet::new();
    let mut groups: Vec<WorkspaceGroup> = Vec::new();
    for group in tabs.workspace_groups.as_deref().unwrap_or(&[]) {
        let Ok(id) = Uuid::parse_str(&group.id) else {
            continue;
        };
        let Some(members) = members_by_group_id.get(&id) else {
            continue;
        };
        if !seen.insert(id) {
            continue;
        }
        let stored_anchor = group
            .anchor_workspace_id
            .as_deref()
            .and_then(|s| Uuid::parse_str(s).ok());
        let anchor_workspace_id = group
            .anchor_member_index
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| members.get(i).copied())
            .or_else(|| stored_anchor.filter(|a| members.contains(a)))
            .unwrap_or(members[0]);
        groups.push(WorkspaceGroup::new(
            id,
            group.name.clone(),
            group.is_collapsed,
            group.is_pinned.unwrap_or(false),
            anchor_workspace_id,
            group.custom_color.clone(),
            group.icon_symbol.clone(),
        ));
    }
    (rows, groups)
}

/// Permute `tabs.workspaces` into `final_row_ids` order (every final id maps to
/// exactly one original position via the parallel `original_row_ids`), permute
/// `tabs.workspace_groups` by the final mirror group order (snapshot groups
/// absent from the mirror — unparseable/member-less/duplicate ids — sort last,
/// stable, mirroring the crate sync's missing-anchor-last rule), and remap the
/// index-based `selected_workspace_index` through the old→new permutation so it
/// keeps following the same workspace (canonical selection is id-based and
/// untouched — the `set_workspace_pinned` precedent). `None`/out-of-range
/// selection stays as-is. Serialized objects are MOVED, never rewritten: the
/// mirror's dangling-`group_id` clears are NOT written back (snapshot strings
/// stay, the `set_workspace_pinned` posture).
fn write_back_reordered(
    tabs: &mut SessionTabManagerSnapshot,
    original_row_ids: &[Uuid],
    final_row_ids: &[Uuid],
    final_group_ids: &[Uuid],
) {
    let back_map: HashMap<Uuid, usize> = original_row_ids
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();
    // Every row has exactly one mirror id (minted ids are unique), so this is a
    // full permutation of the original positions.
    let perm: Vec<usize> = final_row_ids.iter().map(|id| back_map[id]).collect();
    debug_assert_eq!(perm.len(), tabs.workspaces.len());
    let mut slots: Vec<Option<SessionWorkspaceSnapshot>> = std::mem::take(&mut tabs.workspaces)
        .into_iter()
        .map(Some)
        .collect();
    tabs.workspaces = perm
        .iter()
        .map(|&i| slots[i].take().expect("row permutation is a bijection"))
        .collect();

    if let Some(groups) = tabs.workspace_groups.as_mut() {
        let order: HashMap<Uuid, usize> = final_group_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i))
            .collect();
        groups.sort_by_key(|g| {
            Uuid::parse_str(&g.id)
                .ok()
                .and_then(|id| order.get(&id).copied())
                .unwrap_or(usize::MAX)
        });
    }

    if let Some(sel) = tabs.selected_workspace_index {
        if sel >= 0 && (sel as usize) < perm.len() {
            if let Some(next) = perm.iter().position(|&old| old == sel as usize) {
                tabs.selected_workspace_index = Some(next as i64);
            }
        }
    }
}

/// Run canonical workspace-group contiguity over the session snapshot mirror and
/// write any resulting row/group ordering back to the serialized snapshot.
fn normalize_workspace_groups_in_snapshot(tabs: &mut SessionTabManagerSnapshot) -> bool {
    let (rows, groups) = workspace_mirror(tabs);
    if groups.is_empty() {
        return false;
    }

    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let original_group_ids: Vec<Uuid> = groups.iter().map(|group| group.id).collect();
    let (final_rows, final_groups) = normalize_workspace_group_contiguity(&rows, &groups, None);
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();

    if final_row_ids == original_row_ids && final_group_ids == original_group_ids {
        return false;
    }

    write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    true
}

/// Reorder the workspace at `index` toward `to_index` — the port of canonical
/// `WorkspaceReorderCoordinator.reorderSidebarWorkspace`
/// (`WorkspaceReorderCoordinator.swift:243-257`): a mover that anchors a group
/// (or `usesTopLevelRows`) routes to `reorderTopLevelWorkspaceItem` (:260-296),
/// everything else to plain `reorderWorkspace(tabId:toIndex:)` (:109-132). This
/// routing is LOAD-BEARING: a group anchor moved through the plain path snaps
/// back (normalize re-emits the group at its FIRST member's slot), so "move a
/// group" only works via the top-level path. The plural name reflects that an
/// anchor mover relocates its WHOLE group — every member row moves with it,
/// contiguously and anchor-first.
///
/// INDEX-SPACE CONTRACT: `index` identifies the MOVER as a position in
/// `tabs.workspaces` (the command-layer convention, matching select/close/
/// rename/pin). `to_index` is interpreted in the row space canonical uses for
/// that mover: a `tabs.workspaces` index for non-anchors, a TOP-LEVEL row index
/// for group anchors (canonical UI feeds indices from the matching space via
/// `sidebarReorderWorkspaceIds`, Coordinator:171-183; the web drag lane does
/// the same). The sidebar-drag-only `uses_top_level_rows` mode additionally
/// promotes a grouped child into that top-level row space, matching canonical
/// `reorderSidebarWorkspace(... usesTopLevelRows: true)`.
///
/// PLAIN path (Coordinator:109-132 + `workspaceReorderPlan` :142-151):
/// - Unknown id → plan nil → no-op (:143, :110); `tabs.count <= 1` → no
///   mutation and NO group inference (:116-118 — the canonical comment: no-op
///   reorders must not run inference, else socket `move_down` on the last
///   ungrouped row absorbs it into the group above).
/// - Clamp via `clampedReorderIndex` (`WorkspacesModel+Ordering.swift`
///   :143-156): `[0, count-1]`, then the in-section clamp for grouped
///   non-anchor members (`clampedGroupedMemberReorderIndex` :160-187 — section
///   `[firstIndex+1 .. lastIndex]`, pinned members in the
///   `[firstIndex+1 .. firstIndex+pinnedMemberCount]` sub-tier, unpinned in
///   `[firstIndex+1+pinnedMemberCount .. lastIndex]`), else the global
///   pin-tier clamp (pinned mover → `min(clamped, pinnedCount-1)`, unpinned →
///   `max(clamped, pinnedCount)` with `pinnedCount =
///   leadingGlobalPinnedRowCount` :190-197 counting rows by `isGlobalPinnedRow`
///   :201-207: grouped rows count by their GROUP's pin, a dangling group id
///   falls back to the row's own flag).
/// - `from == clamped` → no mutation, and crucially no normalization
///   (:116-118). Otherwise remove/insert (:120-121), then the non-drag tail
///   (:124-129): when groups exist, `normalizeWorkspaceGroupContiguity`
///   (`WorkspacesModel+GroupInvariants.swift:70-87`). The canonical
///   pre-sync-if-anchor step never changes normalize's ROW output (top-level
///   order derives from rows, never the groups array) and the crate's
///   `normalize_workspace_group_contiguity` already ends with the group-order
///   sync, so calling it alone is exact. The `isDragOperation=true`
///   group-membership inference (`applyDragInferredGroupMembership` :346-397)
///   is UI-drag semantics deferred to the sidebar drag lane; this is the
///   `isDragOperation=false` path.
///
/// TOP-LEVEL path (Coordinator:260-296, `promotesGroupedWorkspace=false`):
/// - `topLevelIds = sidebarTopLevelWorkspaceIds` (Ordering.swift:37-61, no
///   promotion); the mover absent from it → no-op (:268).
/// - Clamp via `clampedTopLevelReorderIndex` (Ordering.swift:109-125, pin tier
///   over `sidebarTopLevelPinnedWorkspaceIds` :97-106 — pinned groups by GROUP
///   pin, ungrouped rows by their own flag). `from == clamped` → no-op (:274 —
///   canonical returns `false` here, unlike the plain path's no-op-true; both
///   map to `changed = false` in the port).
/// - remove/insert in the top-level ids, then
///   `normalizeWorkspaceGroupRunsPreservingOrder(desired)` +
///   `syncWorkspaceGroupsOrderToAnchorOrder` (:276-286). NO pinned/unpinned
///   re-partition happens here — the clamp already enforced tiers, and :285
///   uses the desired order directly (a deliberate canonical divergence from
///   `normalizeWorkspaceGroupContiguity`'s desired computation).
///
/// DOCUMENTED DIVERGENCES (the port's changed-bool emit policy, same as
/// rename/pin): canonical's plain path returns `true` for its `count <= 1` and
/// `from == clamped` no-ops while the top-level path returns `false` for the
/// same — the port returns `true` iff the snapshot actually changed, which
/// drives the emit gate. Canonical also emits unconditionally post-mutation
/// (:130); the port compares the final row order against the original and only
/// writes back on a real change (normalization can revert the raw move, e.g. an
/// ungrouped row nudged into the middle of a group's section snaps back out).
///
/// Batch multi-id reorder (`reorderWorkspaces(orderedWorkspaceIds:)`,
/// Coordinator:414-444) is exposed separately by [`reorder_workspaces_many`];
/// canonical drag is single-row, so it is intentionally not part of this op.
pub fn reorder_workspaces(tabs: &mut SessionTabManagerSnapshot, index: i64, to_index: i64) -> bool {
    reorder_workspaces_with_mode(tabs, index, to_index, false)
}

/// Move one workspace to the top of its current pin tier, matching canonical
/// `WorkspaceReorderCoordinator.moveTabToTop`. Group members hoist their whole
/// top-level group row, and index-based selection is remapped to keep following
/// the same workspace identity.
pub fn move_workspace_to_top(tabs: &mut SessionTabManagerSnapshot, index: i64) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() || tabs.workspaces.len() <= 1 {
        return false;
    }
    let (rows, groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let selected_id = rows[index as usize].id;

    let (final_rows, final_groups) = if groups.is_empty() {
        let mut reordered = rows;
        let selected = reordered.remove(index as usize);
        let destination = if selected.is_pinned {
            0
        } else {
            reordered.iter().take_while(|row| row.is_pinned).count()
        };
        reordered.insert(destination, selected);
        (reordered, groups)
    } else {
        let hoisted = move_workspace_group_members_after_anchors(&rows, &groups, &[selected_id]);
        let top_level = sidebar_top_level_workspace_ids(&hoisted, &groups, None);
        let Some(selected_row) = hoisted.iter().find(|row| row.id == selected_id) else {
            return false;
        };
        let selected_top_level =
            top_level_workspace_ids(std::slice::from_ref(selected_row), &groups);
        let selected_set: HashSet<Uuid> = selected_top_level.iter().copied().collect();
        let pinned_set: HashSet<Uuid> = sidebar_top_level_pinned_workspace_ids(&hoisted, &groups)
            .into_iter()
            .collect();
        let mut desired = Vec::with_capacity(top_level.len());
        desired.extend(
            selected_top_level
                .iter()
                .filter(|id| pinned_set.contains(id))
                .copied(),
        );
        desired.extend(
            top_level
                .iter()
                .filter(|id| pinned_set.contains(id) && !selected_set.contains(id))
                .copied(),
        );
        desired.extend(
            selected_top_level
                .iter()
                .filter(|id| !pinned_set.contains(id))
                .copied(),
        );
        desired.extend(
            top_level
                .iter()
                .filter(|id| !pinned_set.contains(id) && !selected_set.contains(id))
                .copied(),
        );
        let final_rows =
            normalize_workspace_group_runs_preserving_order(&hoisted, &groups, &desired);
        let final_groups = sync_workspace_groups_order_to_anchor_order(&final_rows, &groups);
        (final_rows, final_groups)
    };

    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    if final_row_ids == original_row_ids {
        return false;
    }
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    true
}

/// Atomically reorder a requested leading subset within pinned and unpinned
/// tiers, returning canonical pre-application plan indexes. A dry run validates
/// and plans without touching the snapshot. Applying rebuilds the full row
/// order, then restores group contiguity and anchor ordering exactly like the
/// canonical batch coordinator.
pub fn reorder_workspaces_many(
    tabs: &mut SessionTabManagerSnapshot,
    ordered_workspace_ids: &[Uuid],
    dry_run: bool,
) -> Result<Vec<WorkspaceReorderPlanItem>, WorkspaceBatchReorderError> {
    let (rows, groups) = workspace_mirror(tabs);
    let current: Vec<WorkspaceOrderSnapshot> = rows
        .iter()
        .map(|row| WorkspaceOrderSnapshot::new(row.id, row.is_pinned))
        .collect();
    let planner = WorkspaceReorderPlanner::new();
    let plan = planner.batch_reorder_plan(ordered_workspace_ids, &current)?;
    if dry_run || !plan.iter().any(|item| item.from_index != item.to_index) {
        return Ok(plan);
    }

    let original_ids: Vec<Uuid> = rows.iter().map(|row| row.id).collect();
    let rows_by_id: HashMap<Uuid, WorkspaceRow> = rows.iter().map(|row| (row.id, *row)).collect();
    let final_ids = planner.batch_reorder_final_ids(ordered_workspace_ids, &current);
    let reordered_rows: Vec<WorkspaceRow> = final_ids
        .iter()
        .filter_map(|id| rows_by_id.get(id).copied())
        .collect();
    let (final_rows, final_groups) = if groups.is_empty() {
        (reordered_rows, groups)
    } else {
        let synced_groups = sync_workspace_groups_order_to_anchor_order(&reordered_rows, &groups);
        normalize_workspace_group_contiguity(&reordered_rows, &synced_groups, None)
    };
    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|row| row.id).collect();
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|group| group.id).collect();
    write_back_reordered(tabs, &original_ids, &final_row_ids, &final_group_ids);
    Ok(plan)
}

/// Sidebar-reorder variant of [`reorder_workspaces`]. When
/// `uses_top_level_rows` is `true`, the mover is planned in top-level row space
/// even if it is a non-anchor grouped child, mirroring canonical
/// `reorderSidebarWorkspace(... usesTopLevelRows: true)` for "drag this member
/// out of its group / into top-level space". In that promotion case, the moved
/// workspace's serialized `group_id` is cleared on write-back too.
pub fn reorder_workspaces_with_mode(
    tabs: &mut SessionTabManagerSnapshot,
    index: i64,
    to_index: i64,
    uses_top_level_rows: bool,
) -> bool {
    if index < 0 || index as usize >= tabs.workspaces.len() {
        return false;
    }
    // Canonical's plain path treats count <= 1 as a successful no-op WITHOUT
    // mutating or running group inference (Coordinator:116-118); the port's
    // changed-gate maps that to `false`.
    if tabs.workspaces.len() <= 1 {
        return false;
    }
    let from = index as usize;
    let (rows, groups) = workspace_mirror(tabs);
    let original_row_ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mover_id = rows[from].id;
    let mover_is_anchor = is_workspace_group_anchor(&groups, mover_id);
    let promotes_grouped_workspace =
        uses_top_level_rows && !mover_is_anchor && rows[from].group_id.is_some();

    let (final_rows, final_groups) = if mover_is_anchor || uses_top_level_rows {
        // TOP-LEVEL (group-row) move, Coordinator:260-296.
        let top = sidebar_top_level_workspace_ids(
            &rows,
            &groups,
            promotes_grouped_workspace.then_some(mover_id),
        );
        let Some(top_from) = top.iter().position(|id| *id == mover_id) else {
            // Coordinator:268 — mover absent from the top-level rows.
            return false;
        };
        let clamped = clamped_top_level_reorder_index(&rows, &groups, mover_id, to_index, &top);
        if clamped as usize == top_from {
            // Coordinator:274 — canonical returns false for this no-op too.
            return false;
        }
        let mut desired = top;
        desired.remove(top_from);
        desired.insert(clamped as usize, mover_id);
        let base_rows = if promotes_grouped_workspace {
            // Canonical `reorderTopLevelWorkspaceItem(... promotesGroupedWorkspace:
            // true)` clears the mover's group membership before materializing the
            // desired top-level order, so the dragged child becomes its own
            // top-level row instead of snapping back into the old group run.
            assign_group(&rows, mover_id, None)
        } else {
            rows.clone()
        };
        let new_rows =
            normalize_workspace_group_runs_preserving_order(&base_rows, &groups, &desired);
        let new_groups = sync_workspace_groups_order_to_anchor_order(&new_rows, &groups);
        (new_rows, new_groups)
    } else {
        // PLAIN single move, Coordinator:109-132.
        let clamped = clamped_reorder_index(&rows, &groups, &rows[from], to_index);
        if clamped as usize == from {
            // Must NOT normalize on a no-op reorder (Coordinator:111-118).
            return false;
        }
        let mut new_rows = rows;
        let moved = new_rows.remove(from);
        new_rows.insert(clamped as usize, moved);
        if groups.is_empty() {
            // Canonical guard (:124): the non-drag tail only runs with groups.
            (new_rows, groups)
        } else {
            normalize_workspace_group_contiguity(&new_rows, &groups, None)
        }
    };

    let final_row_ids: Vec<Uuid> = final_rows.iter().map(|r| r.id).collect();
    if final_row_ids == original_row_ids {
        // Normalization restored the original order — nothing changed.
        return false;
    }
    let final_group_ids: Vec<Uuid> = final_groups.iter().map(|g| g.id).collect();
    write_back_reordered(tabs, &original_row_ids, &final_row_ids, &final_group_ids);
    if promotes_grouped_workspace {
        if let Some(new_index) = final_row_ids.iter().position(|id| *id == mover_id) {
            tabs.workspaces[new_index].group_id = None;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::SessionPanelShellActivityStateSnapshot;

    fn pane(id: &str) -> Layout {
        single_pane(id)
    }

    fn split(
        orientation: SessionSplitOrientation,
        divider: f64,
        first: Layout,
        second: Layout,
    ) -> Layout {
        Layout::Split(SessionSplitLayoutSnapshot {
            split_id: None,
            orientation,
            divider_position: divider,
            first: Box::new(first),
            second: Box::new(second),
        })
    }

    fn panel_ids(layout: &Layout) -> Vec<String> {
        match layout {
            Layout::Pane(p) => p.panel_ids.clone(),
            Layout::Split(_) => panic!("expected a pane"),
        }
    }

    #[test]
    fn clamp_divider_bounds_and_nan() {
        assert_eq!(clamp_divider(0.5), 0.5);
        assert_eq!(clamp_divider(-1.0), MIN_DIVIDER);
        assert_eq!(clamp_divider(2.0), MAX_DIVIDER);
        assert_eq!(clamp_divider(f64::NAN), 0.5);
    }

    #[test]
    fn count_leaves_walks_the_tree() {
        assert_eq!(count_leaves(&pane("a")), 1);
        let tree = split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            split(SessionSplitOrientation::Vertical, 0.5, pane("b"), pane("c")),
        );
        assert_eq!(count_leaves(&tree), 3);
    }

    #[test]
    fn equalize_weights_by_leaf_count() {
        let two_vs_one = SessionSplitLayoutSnapshot {
            split_id: None,
            orientation: SessionSplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(split(
                SessionSplitOrientation::Vertical,
                0.5,
                pane("a"),
                pane("b"),
            )),
            second: Box::new(pane("c")),
        };
        assert!((equalize_divider(&two_vs_one) - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn equalize_dividers_resets_a_mixed_orientation_tree_to_span_ratios() {
        // H( V(a,b), c ) with skewed dividers; equalize uses orientation-aware
        // span counts, so the horizontal root sees span 1 (the vertical subtree)
        // vs 1 (pane c) → 0.5, and the inner vertical split → 0.5. This diverges
        // from leaf-count weighting (which would give the root 2/3).
        let mut tree = split(
            SessionSplitOrientation::Horizontal,
            0.8,
            split(SessionSplitOrientation::Vertical, 0.2, pane("a"), pane("b")),
            pane("c"),
        );
        assert!(equalize_dividers(&mut tree));
        if let Layout::Split(root) = &tree {
            assert_eq!(root.divider_position, 0.5); // span-weighted, NOT 2/3
            if let Layout::Split(inner) = root.first.as_ref() {
                assert_eq!(inner.divider_position, 0.5);
            } else {
                panic!("expected nested vertical split");
            }
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn equalize_dividers_on_a_single_pane_is_a_noop() {
        // Canonical `foundSplit == false` for a lone pane: returns false and
        // leaves the pane byte-identical.
        let mut tree = pane("a");
        assert!(!equalize_dividers(&mut tree));
        assert_eq!(tree, pane("a"));
    }

    #[test]
    fn equalize_dividers_preserves_leaf_count() {
        // Equalize never adds or removes panes; only divider positions change.
        let mut tree = split(
            SessionSplitOrientation::Horizontal,
            0.75,
            pane("a"),
            split(
                SessionSplitOrientation::Horizontal,
                0.15,
                pane("b"),
                pane("c"),
            ),
        );
        let before = count_leaves(&tree);
        assert!(equalize_dividers(&mut tree));
        assert_eq!(count_leaves(&tree), before);
        // Same-axis nesting: root sees span 1 (a) vs 2 (b,c) → 1/3; inner → 0.5.
        if let Layout::Split(root) = &tree {
            assert!((root.divider_position - 1.0 / 3.0).abs() < 1e-9);
            if let Layout::Split(inner) = root.second.as_ref() {
                assert_eq!(inner.divider_position, 0.5);
            } else {
                panic!("expected nested split");
            }
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn toggle_split_zoom_sets_and_clears_the_workspace_zoom_target() {
        let mut workspace = fresh_terminal_workspace("a");
        workspace.layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            pane("b"),
        ));
        assert!(toggle_split_zoom(&mut workspace, "b"));
        assert_eq!(workspace.zoomed_panel_id.as_deref(), Some("b"));
        assert!(toggle_split_zoom(&mut workspace, "b"));
        assert_eq!(workspace.zoomed_panel_id, None);
    }

    #[test]
    fn toggle_split_zoom_is_a_noop_without_a_split_or_matching_panel() {
        let mut workspace = fresh_terminal_workspace("a");
        assert!(!toggle_split_zoom(&mut workspace, "a"));
        assert_eq!(workspace.zoomed_panel_id, None);

        workspace.layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            pane("b"),
        ));
        assert!(!toggle_split_zoom(&mut workspace, "missing"));
        assert_eq!(workspace.zoomed_panel_id, None);
    }

    #[test]
    fn canvas_panes_from_layout_preserves_split_ratios() {
        let tree = split(
            SessionSplitOrientation::Horizontal,
            0.25,
            pane("a"),
            split(
                SessionSplitOrientation::Vertical,
                0.75,
                pane("b"),
                pane("c"),
            ),
        );

        let panes = canvas_panes_from_layout(&tree);

        assert_eq!(panes.len(), 3);
        assert_eq!(panes[0].panel_id, "a");
        assert_eq!(
            (panes[0].x, panes[0].y, panes[0].width, panes[0].height),
            (0, 0, 300, 800)
        );
        assert_eq!(panes[1].panel_id, "b");
        assert_eq!(
            (panes[1].x, panes[1].y, panes[1].width, panes[1].height),
            (300, 0, 900, 600)
        );
        assert_eq!(panes[2].panel_id, "c");
        assert_eq!(
            (panes[2].x, panes[2].y, panes[2].width, panes[2].height),
            (300, 600, 900, 200)
        );
    }

    #[test]
    fn set_layout_mode_toggles_canvas_and_seeds_once() {
        let mut workspace = fresh_terminal_workspace("a");
        workspace.layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            pane("b"),
        ));

        assert!(set_layout_mode(&mut workspace, Some("canvas")));
        assert_eq!(workspace.layout_mode.as_deref(), Some("canvas"));
        assert_eq!(workspace.canvas_panes.as_ref().map(Vec::len), Some(2));

        let seeded = workspace.canvas_panes.clone();
        assert!(!set_layout_mode(&mut workspace, Some("canvas")));
        assert_eq!(workspace.canvas_panes, seeded);

        assert!(set_layout_mode(&mut workspace, Some("split")));
        assert_eq!(workspace.layout_mode, None);
        assert_eq!(workspace.canvas_panes, seeded);
    }

    #[test]
    fn set_layout_mode_unknown_values_fall_back_to_split() {
        let mut workspace = fresh_terminal_workspace("a");
        assert!(set_layout_mode(&mut workspace, Some("canvas")));
        assert!(set_layout_mode(&mut workspace, Some("mystery")));
        assert_eq!(workspace.layout_mode, None);
    }

    #[test]
    fn set_canvas_pane_frame_updates_existing_seeded_pane() {
        let mut workspace = fresh_terminal_workspace("a");
        assert!(set_layout_mode(&mut workspace, Some("canvas")));

        assert!(set_canvas_pane_frame(&mut workspace, "a", 40, 50, 640, 360));
        let pane = &workspace.canvas_panes.as_ref().unwrap()[0];
        assert_eq!(
            (pane.x, pane.y, pane.width, pane.height),
            (40, 50, 640, 360)
        );
        assert!(!set_canvas_pane_frame(
            &mut workspace,
            "a",
            40,
            50,
            640,
            360
        ));
    }

    #[test]
    fn set_canvas_pane_frame_seeds_from_layout_before_updating() {
        let mut workspace = fresh_terminal_workspace("a");
        workspace.layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            pane("b"),
        ));

        assert!(set_canvas_pane_frame(
            &mut workspace,
            "b",
            700,
            20,
            300,
            240
        ));

        let panes = workspace.canvas_panes.as_ref().unwrap();
        assert_eq!(panes.len(), 2);
        let updated = panes.iter().find(|pane| pane.panel_id == "b").unwrap();
        assert_eq!(
            (updated.x, updated.y, updated.width, updated.height),
            (700, 20, 300, 240)
        );
    }

    #[test]
    fn set_canvas_pane_frame_adds_missing_panel_and_rejects_blank_ids() {
        let mut workspace = fresh_terminal_workspace("a");
        assert!(!set_canvas_pane_frame(&mut workspace, " ", 0, 0, 1, 1));
        assert!(set_canvas_pane_frame(&mut workspace, "new", 1, 2, -3, 0));
        let pane = workspace.canvas_panes.as_ref().unwrap().last().unwrap();
        assert_eq!(pane.panel_id, "new");
        assert_eq!((pane.width, pane.height), (1, 1));
    }

    #[test]
    fn apply_canvas_action_rejects_split_mode_and_unknown_actions() {
        let mut workspace = fresh_terminal_workspace("a");
        workspace.canvas_panes = Some(vec![
            SessionCanvasPaneSnapshot {
                panel_id: "a".to_string(),
                x: 20,
                y: 30,
                width: 100,
                height: 80,
                panel_ids: None,
                selected_panel_id: Some("a".to_string()),
            },
            SessionCanvasPaneSnapshot {
                panel_id: "b".to_string(),
                x: 80,
                y: 90,
                width: 200,
                height: 120,
                panel_ids: None,
                selected_panel_id: Some("b".to_string()),
            },
        ]);
        assert!(!apply_canvas_action(&mut workspace, "alignLeft"));
        workspace.layout_mode = Some("canvas".to_string());
        assert!(!apply_canvas_action(&mut workspace, "mystery"));
    }

    #[test]
    fn apply_canvas_action_aligns_and_equalizes_persisted_frames() {
        let mut workspace = fresh_terminal_workspace("a");
        workspace.layout_mode = Some("canvas".to_string());
        workspace.canvas_panes = Some(vec![
            SessionCanvasPaneSnapshot {
                panel_id: "a".to_string(),
                x: 20,
                y: 30,
                width: 100,
                height: 80,
                panel_ids: None,
                selected_panel_id: Some("a".to_string()),
            },
            SessionCanvasPaneSnapshot {
                panel_id: "b".to_string(),
                x: 80,
                y: 90,
                width: 200,
                height: 120,
                panel_ids: None,
                selected_panel_id: Some("b".to_string()),
            },
        ]);

        assert!(apply_canvas_action(&mut workspace, "canvas.alignLeft"));
        let panes = workspace.canvas_panes.as_ref().unwrap();
        assert_eq!(
            panes.iter().map(|pane| pane.x).collect::<Vec<_>>(),
            vec![20, 20]
        );

        assert!(apply_canvas_action(&mut workspace, "equalizeHeights"));
        let panes = workspace.canvas_panes.as_ref().unwrap();
        assert_eq!(
            panes.iter().map(|pane| pane.height).collect::<Vec<_>>(),
            vec![120, 120]
        );
    }

    #[test]
    fn apply_canvas_action_distributes_and_tidies_frames() {
        let mut workspace = fresh_terminal_workspace("a");
        workspace.layout_mode = Some("canvas".to_string());
        workspace.canvas_panes = Some(vec![
            SessionCanvasPaneSnapshot {
                panel_id: "c".to_string(),
                x: 320,
                y: 210,
                width: 50,
                height: 50,
                panel_ids: None,
                selected_panel_id: Some("c".to_string()),
            },
            SessionCanvasPaneSnapshot {
                panel_id: "a".to_string(),
                x: 10,
                y: 20,
                width: 100,
                height: 80,
                panel_ids: None,
                selected_panel_id: Some("a".to_string()),
            },
            SessionCanvasPaneSnapshot {
                panel_id: "b".to_string(),
                x: 240,
                y: 50,
                width: 75,
                height: 60,
                panel_ids: None,
                selected_panel_id: Some("b".to_string()),
            },
        ]);

        assert!(apply_canvas_action(
            &mut workspace,
            "distributeHorizontally"
        ));
        let panes = workspace.canvas_panes.as_ref().unwrap();
        assert_eq!(panes[1].x, 10);
        assert_eq!(panes[2].x, 126);
        assert_eq!(panes[0].x, 217);

        assert!(apply_canvas_action(&mut workspace, "tidy"));
        let panes = workspace.canvas_panes.as_ref().unwrap();
        assert_eq!(
            panes
                .iter()
                .map(|pane| (pane.panel_id.as_str(), pane.x, pane.y))
                .collect::<Vec<_>>(),
            vec![("c", 10, 116), ("a", 10, 20), ("b", 126, 20)]
        );
    }

    #[test]
    fn apply_canvas_action_with_gap_uses_configured_spacing() {
        let mut workspace = fresh_terminal_workspace("a");
        workspace.layout_mode = Some("canvas".to_string());
        workspace.canvas_panes = Some(vec![
            SessionCanvasPaneSnapshot {
                panel_id: "a".to_string(),
                x: 10,
                y: 20,
                width: 100,
                height: 80,
                panel_ids: None,
                selected_panel_id: Some("a".to_string()),
            },
            SessionCanvasPaneSnapshot {
                panel_id: "b".to_string(),
                x: 240,
                y: 50,
                width: 75,
                height: 60,
                panel_ids: None,
                selected_panel_id: Some("b".to_string()),
            },
            SessionCanvasPaneSnapshot {
                panel_id: "c".to_string(),
                x: 320,
                y: 210,
                width: 50,
                height: 50,
                panel_ids: None,
                selected_panel_id: Some("c".to_string()),
            },
        ]);

        assert!(apply_canvas_action_with_gap(
            &mut workspace,
            "distributeHorizontally",
            Some(24)
        ));
        let panes = workspace.canvas_panes.as_ref().unwrap();
        assert_eq!(panes[0].x, 10);
        assert_eq!(panes[1].x, 134);
        assert_eq!(panes[2].x, 233);

        assert!(apply_canvas_action_with_gap(
            &mut workspace,
            "tidy",
            Some(24)
        ));
        let panes = workspace.canvas_panes.as_ref().unwrap();
        assert_eq!(
            panes
                .iter()
                .map(|pane| (pane.panel_id.as_str(), pane.x, pane.y))
                .collect::<Vec<_>>(),
            vec![("a", 10, 20), ("b", 134, 20), ("c", 10, 124)]
        );
    }

    #[test]
    fn apply_canvas_action_seeds_from_layout_when_needed() {
        let mut workspace = fresh_terminal_workspace("a");
        workspace.layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            pane("b"),
        ));
        workspace.layout_mode = Some("canvas".to_string());

        assert!(apply_canvas_action(&mut workspace, "distributeVertically"));
        let panes = workspace.canvas_panes.as_ref().unwrap();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].y, 0);
        assert_eq!(panes[1].y, 816);
    }

    #[test]
    fn split_pane_replaces_target_with_a_centered_split() {
        let mut tree = pane("a");
        assert!(split_pane(
            &mut tree,
            "a",
            SessionSplitOrientation::Horizontal,
            "b",
            false,
        ));
        match &tree {
            Layout::Split(s) => {
                assert_eq!(s.divider_position, 0.5);
                assert_eq!(s.orientation, SessionSplitOrientation::Horizontal);
                assert_eq!(panel_ids(&s.first), vec!["a"]); // existing stays first
                assert_eq!(panel_ids(&s.second), vec!["b"]); // new pane second
            }
            Layout::Pane(_) => panic!("expected a split"),
        }
    }

    #[test]
    fn split_pane_insert_first_puts_the_new_pane_first() {
        let mut tree = pane("a");
        assert!(split_pane(
            &mut tree,
            "a",
            SessionSplitOrientation::Vertical,
            "b",
            true
        ));
        if let Layout::Split(s) = &tree {
            assert_eq!(panel_ids(&s.first), vec!["b"]);
            assert_eq!(panel_ids(&s.second), vec!["a"]);
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn split_pane_targets_a_nested_pane() {
        let mut tree = split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            pane("b"),
        );
        assert!(split_pane(
            &mut tree,
            "b",
            SessionSplitOrientation::Vertical,
            "c",
            false
        ));
        // The right child became a vertical split of b|c; the root is untouched.
        if let Layout::Split(root) = &tree {
            assert_eq!(count_leaves(&root.second), 2);
            assert_eq!(count_leaves(&root.first), 1);
        } else {
            panic!("expected a split");
        }
        assert_eq!(count_leaves(&tree), 3);
    }

    #[test]
    fn split_pane_returns_false_for_unknown_target() {
        let mut tree = pane("a");
        assert!(!split_pane(
            &mut tree,
            "zzz",
            SessionSplitOrientation::Horizontal,
            "b",
            false
        ));
        assert_eq!(tree, pane("a"));
    }

    #[test]
    fn close_panel_collapses_a_split_into_its_sibling() {
        let mut layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            pane("b"),
        ));
        assert_eq!(close_panel(&mut layout, "a"), CloseOutcome::Removed);
        // The split collapsed to the surviving pane `b`.
        assert_eq!(layout, Some(pane("b")));
    }

    #[test]
    fn close_panel_collapses_deeply_and_preserves_the_far_sibling() {
        // split( a, split( b, c ) ); closing b collapses the inner split to c.
        let mut layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            split(SessionSplitOrientation::Vertical, 0.5, pane("b"), pane("c")),
        ));
        assert_eq!(close_panel(&mut layout, "b"), CloseOutcome::Removed);
        let expected = split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            pane("c"),
        );
        assert_eq!(layout, Some(expected));
    }

    #[test]
    fn close_panel_emptying_the_root_pane_clears_the_layout() {
        let mut layout = Some(pane("a"));
        assert_eq!(close_panel(&mut layout, "a"), CloseOutcome::Emptied);
        assert_eq!(layout, None);
    }

    #[test]
    fn close_panel_removes_one_of_several_tabs_without_collapsing() {
        let mut layout = Some(Layout::Pane(SessionPaneLayoutSnapshot {
            pane_id: None,
            panel_ids: vec!["a".into(), "b".into()],
            selected_panel_id: Some("a".into()),
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
        }));
        assert_eq!(close_panel(&mut layout, "a"), CloseOutcome::Removed);
        // Pane survives with `b`, and selection moved off the closed panel.
        if let Some(Layout::Pane(p)) = &layout {
            assert_eq!(p.panel_ids, vec!["b".to_string()]);
            assert_eq!(p.selected_panel_id.as_deref(), Some("b"));
        } else {
            panic!("expected a surviving pane");
        }
    }

    #[test]
    fn select_adjacent_panel_wraps_within_a_multi_panel_pane() {
        let mut tree = Layout::Pane(SessionPaneLayoutSnapshot {
            pane_id: None,
            panel_ids: vec!["a".into(), "b".into(), "c".into()],
            selected_panel_id: Some("c".into()),
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
        });
        assert!(select_adjacent_panel(&mut tree, "b", true));
        if let Layout::Pane(pane) = &tree {
            assert_eq!(pane.selected_panel_id.as_deref(), Some("a"));
        } else {
            panic!("expected pane");
        }
    }

    #[test]
    fn select_adjacent_panel_previous_wraps_backward() {
        let mut tree = Layout::Pane(SessionPaneLayoutSnapshot {
            pane_id: None,
            panel_ids: vec!["a".into(), "b".into(), "c".into()],
            selected_panel_id: Some("a".into()),
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
        });
        assert!(select_adjacent_panel(&mut tree, "a", false));
        if let Layout::Pane(pane) = &tree {
            assert_eq!(pane.selected_panel_id.as_deref(), Some("c"));
        } else {
            panic!("expected pane");
        }
    }

    #[test]
    fn select_adjacent_panel_is_noop_for_single_panel_or_unknown_panel() {
        let mut tree = pane("a");
        assert!(!select_adjacent_panel(&mut tree, "a", true));
        assert_eq!(tree, pane("a"));
        assert!(!select_adjacent_panel(&mut tree, "missing", true));
        assert_eq!(tree, pane("a"));
    }

    #[test]
    fn select_panel_sets_the_requested_panel_in_its_pane() {
        let mut tree = Layout::Pane(SessionPaneLayoutSnapshot {
            pane_id: None,
            panel_ids: vec!["a".into(), "b".into(), "c".into()],
            selected_panel_id: Some("a".into()),
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
        });

        assert!(select_panel(&mut tree, "c"));
        if let Layout::Pane(pane) = &tree {
            assert_eq!(pane.selected_panel_id.as_deref(), Some("c"));
        } else {
            panic!("expected pane");
        }
        assert!(!select_panel(&mut tree, "c"));
        assert!(!select_panel(&mut tree, "missing"));
    }

    #[test]
    fn add_panel_to_pane_inserts_after_anchor_and_selects_new_panel() {
        let mut tree = Layout::Pane(SessionPaneLayoutSnapshot {
            pane_id: Some("pane-1".into()),
            panel_ids: vec!["a".into(), "c".into()],
            selected_panel_id: Some("a".into()),
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
        });
        assert!(add_panel_to_pane(&mut tree, "a", "b"));
        if let Layout::Pane(pane) = tree {
            assert_eq!(pane.panel_ids, ["a", "b", "c"]);
            assert_eq!(pane.selected_panel_id.as_deref(), Some("b"));
        } else {
            panic!("expected pane");
        }
    }

    #[test]
    fn close_panel_unknown_id_is_not_found_and_leaves_the_tree() {
        let mut layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            pane("b"),
        ));
        let before = layout.clone();
        assert_eq!(close_panel(&mut layout, "zzz"), CloseOutcome::NotFound);
        assert_eq!(layout, before);
    }

    #[test]
    fn set_divider_at_path_updates_root_and_nested() {
        let mut tree = split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("a"),
            split(SessionSplitOrientation::Vertical, 0.5, pane("b"), pane("c")),
        );
        assert!(set_divider_at_path(&mut tree, &[], 0.3));
        assert!(set_divider_at_path(&mut tree, &[SplitChild::Second], 5.0));
        if let Layout::Split(root) = &tree {
            assert_eq!(root.divider_position, 0.3);
            if let Layout::Split(inner) = root.second.as_ref() {
                assert_eq!(inner.divider_position, MAX_DIVIDER); // clamped
            } else {
                panic!("expected nested split");
            }
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn set_divider_at_path_off_a_leaf_is_a_noop() {
        let mut tree = pane("a");
        assert!(!set_divider_at_path(&mut tree, &[], 0.3));
        assert_eq!(tree, pane("a"));
    }

    #[test]
    fn set_surface_kind_marks_the_pane_and_survives_a_split() {
        let mut tree = pane("a");
        // Unknown panel → no-op.
        assert!(!set_surface_kind(&mut tree, "zzz", Some("agent".into())));
        // Mark pane `a` as an agent surface.
        assert!(set_surface_kind(&mut tree, "a", Some("agent".into())));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.surface_kind.as_deref(), Some("agent"));
        } else {
            panic!("expected a pane");
        }
        // Splitting keeps `a`'s agent kind on its side; the new pane defaults off.
        assert!(split_pane(
            &mut tree,
            "a",
            SessionSplitOrientation::Horizontal,
            "b",
            false
        ));
        if let Layout::Split(s) = &tree {
            if let Layout::Pane(first) = s.first.as_ref() {
                assert_eq!(first.surface_kind.as_deref(), Some("agent"));
            } else {
                panic!("expected pane a first");
            }
            if let Layout::Pane(second) = s.second.as_ref() {
                assert_eq!(second.surface_kind, None);
            } else {
                panic!("expected pane b second");
            }
        } else {
            panic!("expected a split");
        }
        // Clearing it back to a terminal.
        assert!(set_surface_kind(&mut tree, "a", None));
        if let Layout::Split(s) = &tree {
            if let Layout::Pane(first) = s.first.as_ref() {
                assert_eq!(first.surface_kind, None);
            }
        }
    }

    #[test]
    fn set_markdown_file_path_marks_the_pane_and_survives_a_split() {
        let mut tree = pane("a");
        assert!(set_markdown_file_path(
            &mut tree,
            "a",
            Some("C:/docs/readme.md".into())
        ));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.markdown_file_path.as_deref(), Some("C:/docs/readme.md"));
        } else {
            panic!("expected a pane");
        }
        assert!(split_pane(
            &mut tree,
            "a",
            SessionSplitOrientation::Horizontal,
            "b",
            false
        ));
        if let Layout::Split(s) = &tree {
            if let Layout::Pane(first) = s.first.as_ref() {
                assert_eq!(
                    first.markdown_file_path.as_deref(),
                    Some("C:/docs/readme.md")
                );
            } else {
                panic!("expected the existing pane to survive as first");
            }
            if let Layout::Pane(second) = s.second.as_ref() {
                assert_eq!(second.markdown_file_path, None);
            } else {
                panic!("expected the new pane as second");
            }
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn set_file_path_marks_the_pane_and_survives_a_split() {
        let mut tree = pane("a");
        assert!(set_file_path(
            &mut tree,
            "a",
            Some("C:/docs/notes.txt".into())
        ));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.file_path.as_deref(), Some("C:/docs/notes.txt"));
        } else {
            panic!("expected a pane");
        }
        assert!(split_pane(
            &mut tree,
            "a",
            SessionSplitOrientation::Horizontal,
            "b",
            false
        ));
        if let Layout::Split(s) = &tree {
            if let Layout::Pane(first) = s.first.as_ref() {
                assert_eq!(first.file_path.as_deref(), Some("C:/docs/notes.txt"));
            } else {
                panic!("expected the existing pane to survive as first");
            }
            if let Layout::Pane(second) = s.second.as_ref() {
                assert_eq!(second.file_path, None);
            } else {
                panic!("expected the new pane as second");
            }
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn set_diff_viewer_session_marks_the_pane_and_survives_a_split() {
        let mut tree = pane("a");
        assert!(set_diff_viewer_session(
            &mut tree,
            "a",
            Some("tok-abcdef0123456789".into()),
            Some("/index.html".into())
        ));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.diff_viewer_token.as_deref(), Some("tok-abcdef0123456789"));
            assert_eq!(p.diff_viewer_request_path.as_deref(), Some("/index.html"));
        } else {
            panic!("expected a pane");
        }
        assert!(split_pane(
            &mut tree,
            "a",
            SessionSplitOrientation::Horizontal,
            "b",
            false
        ));
        if let Layout::Split(s) = &tree {
            if let Layout::Pane(first) = s.first.as_ref() {
                assert_eq!(
                    first.diff_viewer_token.as_deref(),
                    Some("tok-abcdef0123456789")
                );
                assert_eq!(
                    first.diff_viewer_request_path.as_deref(),
                    Some("/index.html")
                );
            } else {
                panic!("expected the existing pane to survive as first");
            }
            if let Layout::Pane(second) = s.second.as_ref() {
                assert_eq!(second.diff_viewer_token, None);
                assert_eq!(second.diff_viewer_request_path, None);
            } else {
                panic!("expected the new pane as second");
            }
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn set_browser_state_marks_the_pane_and_survives_a_split() {
        let mut tree = pane("a");
        assert!(set_browser_url(
            &mut tree,
            "a",
            Some("https://example.com".into())
        ));
        assert!(set_browser_page_zoom(&mut tree, "a", Some(1.25)));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_url.as_deref(), Some("https://example.com"));
            assert_eq!(p.browser_page_zoom, Some(1.25));
        } else {
            panic!("expected a pane");
        }
        assert!(split_pane(
            &mut tree,
            "a",
            SessionSplitOrientation::Horizontal,
            "b",
            false
        ));
        if let Layout::Split(s) = &tree {
            if let Layout::Pane(first) = s.first.as_ref() {
                assert_eq!(first.browser_url.as_deref(), Some("https://example.com"));
                assert_eq!(first.browser_page_zoom, Some(1.25));
            } else {
                panic!("expected the existing pane to survive as first");
            }
            if let Layout::Pane(second) = s.second.as_ref() {
                assert_eq!(second.browser_url, None);
                assert_eq!(second.browser_back_history, None);
                assert_eq!(second.browser_forward_history, None);
                assert_eq!(second.browser_omnibar_visible, None);
                assert_eq!(second.browser_focus_mode_active, None);
                assert_eq!(second.browser_developer_tools_visible, None);
                assert_eq!(second.browser_developer_tools_panel, None);
                assert_eq!(second.browser_page_zoom, None);
            } else {
                panic!("expected the new pane as second");
            }
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn browser_history_can_be_cleared_without_losing_the_current_url() {
        let mut tree = pane("a");
        assert!(navigate_browser(
            &mut tree,
            "a",
            "https://one.example".into()
        ));
        assert!(navigate_browser(
            &mut tree,
            "a",
            "https://two.example".into()
        ));
        assert!(browser_go_back(&mut tree, "a"));

        assert!(clear_browser_history(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_url.as_deref(), Some("https://one.example/"));
            assert_eq!(p.browser_back_history, None);
            assert_eq!(p.browser_forward_history, None);
        } else {
            panic!("expected a pane");
        }
        assert!(!clear_browser_history(&mut tree, "a"));
    }

    #[test]
    fn browser_omnibar_visibility_toggles_from_visible_default() {
        let mut tree = pane("a");
        assert!(toggle_browser_omnibar_visible(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_omnibar_visible, Some(false));
        } else {
            panic!("expected a pane");
        }
        assert!(toggle_browser_omnibar_visible(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_omnibar_visible, Some(true));
        } else {
            panic!("expected a pane");
        }
    }

    #[test]
    fn browser_focus_mode_toggles_from_inactive_default() {
        let mut tree = pane("a");
        assert!(toggle_browser_focus_mode(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_focus_mode_active, Some(true));
        } else {
            panic!("expected a pane");
        }
        assert!(toggle_browser_focus_mode(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_focus_mode_active, Some(false));
        } else {
            panic!("expected a pane");
        }
    }

    #[test]
    fn browser_developer_tools_toggle_and_panel_selection_persist() {
        let mut tree = pane("a");
        assert!(toggle_browser_developer_tools(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_developer_tools_visible, Some(true));
            assert_eq!(
                p.browser_developer_tools_panel.as_deref(),
                Some("inspector")
            );
        } else {
            panic!("expected a pane");
        }

        assert!(show_browser_developer_tools(&mut tree, "a", "console"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_developer_tools_visible, Some(true));
            assert_eq!(p.browser_developer_tools_panel.as_deref(), Some("console"));
        } else {
            panic!("expected a pane");
        }

        assert!(toggle_browser_developer_tools(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_developer_tools_visible, Some(false));
            assert_eq!(p.browser_developer_tools_panel.as_deref(), Some("console"));
        } else {
            panic!("expected a pane");
        }
    }

    #[test]
    fn browser_navigation_history_round_trips_back_and_forward() {
        let mut tree = pane("a");
        assert!(navigate_browser(
            &mut tree,
            "a",
            "https://one.example".into()
        ));
        assert!(navigate_browser(
            &mut tree,
            "a",
            "https://two.example".into()
        ));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_url.as_deref(), Some("https://two.example"));
            assert_eq!(
                p.browser_back_history.as_deref(),
                Some(["https://one.example/".to_string()].as_slice())
            );
            assert_eq!(p.browser_forward_history, None);
        } else {
            panic!("expected a pane");
        }

        assert!(browser_go_back(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_url.as_deref(), Some("https://one.example/"));
            assert_eq!(p.browser_back_history, None);
            assert_eq!(
                p.browser_forward_history.as_deref(),
                Some(["https://two.example/".to_string()].as_slice())
            );
        } else {
            panic!("expected a pane");
        }

        assert!(browser_go_forward(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_url.as_deref(), Some("https://two.example/"));
            assert_eq!(
                p.browser_back_history.as_deref(),
                Some(["https://one.example/".to_string()].as_slice())
            );
            assert_eq!(p.browser_forward_history, None);
        } else {
            panic!("expected a pane");
        }
    }

    #[test]
    fn browser_navigation_history_uses_session_history_sanitizer() {
        let mut tree = pane("a");
        assert!(navigate_browser(
            &mut tree,
            "a",
            "cmux-diff-viewer://tok-abcdef0123456789/index.html".into()
        ));
        assert!(navigate_browser(
            &mut tree,
            "a",
            "https://two.example/path".into()
        ));
        assert!(navigate_browser(&mut tree, "a", "about:blank".into()));
        if let Layout::Pane(p) = &mut tree {
            assert_eq!(
                p.browser_back_history.as_deref(),
                Some(["https://two.example/path".to_string()].as_slice())
            );
            p.browser_back_history = Some(vec![
                "about:blank".to_string(),
                "http://cmux-diff-viewer.localhost/tok-abcdef0123456789/index.html".to_string(),
                "https://valid.example".to_string(),
            ]);
            p.browser_forward_history = Some(vec![
                "cmux-remote-image://img?url=https://x.test/a.png".to_string(),
            ]);
        } else {
            panic!("expected a pane");
        }

        let Layout::Pane(p) = &tree else {
            panic!("expected a pane");
        };
        assert_eq!(
            browser_navigation_availability(
                p.browser_back_history.as_deref(),
                p.browser_forward_history.as_deref(),
            ),
            NavigationAvailability::new(true, false)
        );

        assert!(browser_go_back(&mut tree, "a"));
        if let Layout::Pane(p) = &tree {
            assert_eq!(p.browser_url.as_deref(), Some("https://valid.example/"));
            assert_eq!(p.browser_back_history, None);
        } else {
            panic!("expected a pane");
        }
    }

    #[test]
    fn split_child_serializes_lowercase_matching_the_web_path() {
        assert_eq!(
            serde_json::to_string(&SplitChild::First).unwrap(),
            "\"first\""
        );
        assert_eq!(
            serde_json::to_string(&SplitChild::Second).unwrap(),
            "\"second\""
        );
        let path: Vec<SplitChild> = serde_json::from_str("[\"first\",\"second\"]").unwrap();
        assert_eq!(path, vec![SplitChild::First, SplitChild::Second]);
    }

    // --- Tab-manager (workspace) ops ---

    fn one_workspace_tabs(panel_id: &str) -> SessionTabManagerSnapshot {
        SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![fresh_terminal_workspace(panel_id)],
            workspace_groups: None,
        }
    }

    /// `n` workspaces (`surface-0`..`surface-{n-1}`), the first `pinned` of them
    /// pinned (contiguous prefix, as the sidebar guarantees), selected at
    /// `selected`.
    fn tabs_with(n: usize, pinned: usize, selected: i64) -> SessionTabManagerSnapshot {
        let workspaces = (0..n)
            .map(|i| SessionWorkspaceSnapshot {
                is_pinned: (i < pinned).then_some(true),
                ..fresh_terminal_workspace(&format!("surface-{i}"))
            })
            .collect();
        SessionTabManagerSnapshot {
            selected_workspace_index: Some(selected),
            workspaces,
            workspace_groups: None,
        }
    }

    #[test]
    fn set_process_title_updates_the_owning_workspace() {
        let mut tabs = one_workspace_tabs("surface-1");
        assert!(set_process_title(&mut tabs, "surface-1", "pwsh — ~/proj"));
        assert_eq!(tabs.workspaces[0].process_title, "pwsh — ~/proj");
    }

    #[test]
    fn set_process_title_trims_and_drops_an_empty_title() {
        let mut tabs = one_workspace_tabs("surface-1");
        // Whitespace-only never clobbers the existing title.
        assert!(!set_process_title(&mut tabs, "surface-1", "   \t "));
        assert_eq!(tabs.workspaces[0].process_title, "Terminal");
        // A padded real title is trimmed on both ends.
        assert!(set_process_title(&mut tabs, "surface-1", "  vim  "));
        assert_eq!(tabs.workspaces[0].process_title, "vim");
    }

    #[test]
    fn set_process_title_is_a_no_op_for_an_unknown_panel() {
        let mut tabs = one_workspace_tabs("surface-1");
        assert!(!set_process_title(&mut tabs, "surface-999", "nope"));
        assert_eq!(tabs.workspaces[0].process_title, "Terminal");
    }

    #[test]
    fn set_process_title_reports_no_change_when_identical() {
        let mut tabs = one_workspace_tabs("surface-1");
        assert!(set_process_title(&mut tabs, "surface-1", "npm run dev"));
        // Same title again → false (nothing changed).
        assert!(!set_process_title(&mut tabs, "surface-1", "npm run dev"));
    }

    #[test]
    fn set_process_title_targets_only_the_workspace_owning_the_panel() {
        // surface-0..surface-2 each in their own workspace.
        let mut tabs = tabs_with(3, 0, 0);
        assert!(set_process_title(&mut tabs, "surface-2", "cargo test"));
        assert_eq!(tabs.workspaces[0].process_title, "Terminal");
        assert_eq!(tabs.workspaces[1].process_title, "Terminal");
        assert_eq!(tabs.workspaces[2].process_title, "cargo test");
    }

    fn install_surface_records(tabs: &mut SessionTabManagerSnapshot, surface_ids: &[&str]) {
        tabs.workspaces[0].surfaces = Some(
            surface_ids
                .iter()
                .map(|surface_id| {
                    serde_json::from_value(serde_json::json!({
                        "surface_id": surface_id,
                        "pane_id": "surface-1",
                        "generation": 1,
                        "kind": {"type": "terminal"},
                        "metadata": {}
                    }))
                    .expect("surface record")
                })
                .collect(),
        );
    }

    fn runtime_title(tabs: &SessionTabManagerSnapshot, surface_id: &str) -> Option<String> {
        serde_json::to_value(&tabs.workspaces[0].surfaces)
            .expect("serialize surface records")
            .as_array()
            .and_then(|rows| {
                rows.iter()
                    .find(|row| row["surface_id"] == surface_id)
                    .and_then(|row| row["metadata"]["runtime_title"].as_str())
            })
            .map(str::to_owned)
    }

    #[test]
    fn set_process_title_keeps_exact_surface_authority_and_ignores_blank() {
        let mut tabs = one_workspace_tabs("surface-1");
        install_surface_records(&mut tabs, &["surface-1"]);

        assert!(set_process_title(
            &mut tabs,
            "surface-1",
            "  pwsh — C:/repo  "
        ));
        assert_eq!(tabs.workspaces[0].process_title, "pwsh — C:/repo");
        assert_eq!(
            runtime_title(&tabs, "surface-1").as_deref(),
            Some("pwsh — C:/repo")
        );

        assert!(!set_process_title(&mut tabs, "surface-1", " \r\n\t "));
        assert_eq!(
            runtime_title(&tabs, "surface-1").as_deref(),
            Some("pwsh — C:/repo")
        );
        assert!(!set_process_title(&mut tabs, "unknown", "ignored"));
    }

    #[test]
    fn set_process_title_does_not_replace_multi_panel_or_custom_workspace_title() {
        let mut multi = one_workspace_tabs("surface-1");
        assert!(split_pane(
            multi.workspaces[0].layout.as_mut().unwrap(),
            "surface-1",
            SessionSplitOrientation::Horizontal,
            "surface-2",
            false,
        ));
        install_surface_records(&mut multi, &["surface-1", "surface-2"]);
        assert!(set_process_title(&mut multi, "surface-2", "cargo test"));
        assert_eq!(multi.workspaces[0].process_title, "Terminal");
        assert_eq!(
            runtime_title(&multi, "surface-2").as_deref(),
            Some("cargo test")
        );

        let mut custom = one_workspace_tabs("surface-1");
        custom.workspaces[0].custom_title = Some("Pinned name".into());
        install_surface_records(&mut custom, &["surface-1"]);
        assert!(set_process_title(&mut custom, "surface-1", "npm run dev"));
        assert_eq!(custom.workspaces[0].process_title, "Terminal");
        assert_eq!(
            runtime_title(&custom, "surface-1").as_deref(),
            Some("npm run dev")
        );
    }

    // Case A: append-when-no-groups / no-pins (AfterCurrent, single tab).
    #[test]
    fn new_workspace_appends_and_selects_it() {
        let mut tabs = one_workspace_tabs("surface-1");
        new_workspace(&mut tabs, "surface-2");
        assert_eq!(tabs.workspaces.len(), 2);
        assert_eq!(tabs.selected_workspace_index, Some(1));
        assert!(matches!(tabs.workspaces[1].layout, Some(Layout::Pane(_))));
        assert_eq!(tabs.workspaces[1].process_title, "Terminal");
    }

    // Case B: insert-after-selected (AfterCurrent, middle selection).
    #[test]
    fn new_workspace_after_current_inserts_after_selected() {
        let mut tabs = tabs_with(4, 0, 1);
        new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::AfterCurrent);
        assert_eq!(tabs.workspaces.len(), 5);
        // Lands between old index-1 and old index-2.
        assert_eq!(
            panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()),
            ["new"]
        );
        assert_eq!(tabs.selected_workspace_index, Some(2));
    }

    // Case C: End placement appends.
    #[test]
    fn new_workspace_end_appends() {
        let mut tabs = tabs_with(4, 0, 1);
        new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::End);
        assert_eq!(tabs.workspaces.len(), 5);
        assert_eq!(
            panel_ids(tabs.workspaces[4].layout.as_ref().unwrap()),
            ["new"]
        );
        assert_eq!(tabs.selected_workspace_index, Some(4));
    }

    // Case D: Top placement lands just after the pinned prefix.
    #[test]
    fn new_workspace_top_inserts_after_pinned_prefix() {
        // 5 ws, first 2 pinned, selected = 3 (unpinned).
        let mut tabs = tabs_with(5, 2, 3);
        new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::Top);
        assert_eq!(tabs.workspaces.len(), 6);
        // At index 2: just after the pinned prefix, ahead of the unpinned tabs.
        assert_eq!(
            panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()),
            ["new"]
        );
        assert_eq!(tabs.selected_workspace_index, Some(2));
        // Not at the end, and not inside the pinned prefix.
        assert!(tabs.workspaces[0].is_pinned == Some(true));
        assert!(tabs.workspaces[1].is_pinned == Some(true));
    }

    // Case E: pinned selection under AfterCurrent inserts at the pinned boundary,
    // not after itself (mirrors placement.rs pinned-selection test).
    #[test]
    fn new_workspace_after_current_pinned_selection_inserts_at_boundary() {
        // 5 ws, first 2 pinned, selected = 0 (pinned).
        let mut tabs = tabs_with(5, 2, 0);
        new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::AfterCurrent);
        assert_eq!(tabs.workspaces.len(), 6);
        // Inserts at the pinned boundary (2), not after itself (index 1).
        assert_eq!(
            panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()),
            ["new"]
        );
        assert_eq!(tabs.selected_workspace_index, Some(2));
    }

    // Case F: into-selected-group — flat placement first lands the new ws
    // adjacent to the selected group member, then canonical contiguity moves the
    // ungrouped workspace out of the middle of the group run.
    #[test]
    fn new_workspace_into_selected_group_normalizes_contiguity() {
        let group_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let mut tabs = tabs_with(4, 0, 1);
        // Mark the selected ws (index 1) and its neighbour (index 2) as a group.
        tabs.workspaces[1].group_id = Some(group_id.to_string());
        tabs.workspaces[2].group_id = Some(group_id.to_string());
        tabs.workspace_groups = Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
            id: group_id.to_string(),
            name: "G".to_string(),
            ..Default::default()
        }]);
        new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::AfterCurrent);
        // The fresh workspace is ungrouped. Normalization moves it after the
        // contiguous group run and remaps selection to keep following it.
        assert_eq!(
            panel_ids(tabs.workspaces[3].layout.as_ref().unwrap()),
            ["new"]
        );
        assert_eq!(tabs.selected_workspace_index, Some(3));
        assert_eq!(tabs.workspaces[3].group_id, None);
        assert_eq!(tabs.workspaces[1].group_id.as_deref(), Some(group_id));
        assert_eq!(tabs.workspaces[2].group_id.as_deref(), Some(group_id));
    }

    // The default two-arg wrapper resolves to AfterCurrent.
    #[test]
    fn new_workspace_defaults_to_after_current() {
        let mut tabs = tabs_with(4, 0, 1);
        new_workspace(&mut tabs, "new");
        assert_eq!(
            panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()),
            ["new"]
        );
        assert_eq!(tabs.selected_workspace_index, Some(2));
    }

    #[test]
    fn move_panel_to_new_workspace_extracts_panel_and_selects_destination() {
        let mut tabs = one_workspace_tabs("surface-1");
        tabs.workspaces[0].layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            pane("surface-1"),
            pane("surface-2"),
        ));
        tabs.workspaces[0].current_directory = Some("C:/repo".to_string());
        assert!(set_panel_title(&mut tabs.workspaces[0], "surface-2", "api"));
        assert!(set_panel_pinned(&mut tabs.workspaces[0], "surface-2", true));
        assert!(set_panel_unread(&mut tabs.workspaces[0], "surface-2", true));
        tabs.workspaces[0].agent_listening_ports = Some(vec![9000]);
        tabs.workspaces[0].listening_ports = Some(vec![3000, 5173, 9000]);
        tabs.workspaces[0].panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
            panel_id: "surface-2".to_string(),
            ports: vec![3000, 5173],
        }]);
        tabs.workspaces[0].panel_shell_activity = Some(vec![SessionPanelShellActivitySnapshot {
            panel_id: "surface-2".to_string(),
            state: SessionPanelShellActivityStateSnapshot::CommandRunning,
            updated_at: 12,
        }]);

        assert!(move_panel_to_new_workspace(&mut tabs, "surface-2"));

        assert_eq!(tabs.workspaces.len(), 2);
        assert_eq!(tabs.selected_workspace_index, Some(1));
        assert_eq!(tabs.workspaces[1].process_title, "api");
        assert_eq!(
            tabs.workspaces[1].current_directory.as_deref(),
            Some("C:/repo")
        );
        assert!(contains_panel(
            tabs.workspaces[0].layout.as_ref().unwrap(),
            "surface-1"
        ));
        assert!(!contains_panel(
            tabs.workspaces[0].layout.as_ref().unwrap(),
            "surface-2"
        ));
        assert!(contains_panel(
            tabs.workspaces[1].layout.as_ref().unwrap(),
            "surface-2"
        ));
        assert_eq!(
            tabs.workspaces[1].panel_titles.as_ref().unwrap()[0]
                .custom_title
                .as_deref(),
            Some("api")
        );
        assert_eq!(tabs.workspaces[0].panel_titles, None);
        assert!(tabs.workspaces[1].panel_pins.as_ref().unwrap()[0].is_pinned);
        assert!(tabs.workspaces[1].panel_unreads.as_ref().unwrap()[0].is_unread);
        assert_eq!(tabs.workspaces[0].listening_ports, Some(vec![9000]));
        assert_eq!(tabs.workspaces[0].agent_listening_ports, Some(vec![9000]));
        assert_eq!(tabs.workspaces[0].panel_listening_ports, None);
        assert_eq!(tabs.workspaces[1].listening_ports, Some(vec![3000, 5173]));
        assert_eq!(tabs.workspaces[1].agent_listening_ports, None);
        assert_eq!(
            tabs.workspaces[1].panel_listening_ports.as_ref().unwrap()[0].ports,
            vec![3000, 5173]
        );
        assert_eq!(tabs.workspaces[0].panel_shell_activity, None);
        assert_eq!(
            tabs.workspaces[1].panel_shell_activity.as_ref().unwrap()[0].state,
            SessionPanelShellActivityStateSnapshot::CommandRunning
        );
    }

    #[test]
    fn move_panel_to_new_workspace_rejects_the_only_panel() {
        let mut tabs = one_workspace_tabs("surface-1");
        let before = tabs.clone();
        assert!(!move_panel_to_new_workspace(&mut tabs, "surface-1"));
        assert_eq!(tabs, before);
    }

    #[test]
    fn select_workspace_ignores_out_of_range() {
        let mut tabs = one_workspace_tabs("surface-1");
        new_workspace(&mut tabs, "surface-2"); // 2 workspaces, selected = 1
        assert!(select_workspace(&mut tabs, 0));
        assert_eq!(tabs.selected_workspace_index, Some(0));
        assert!(!select_workspace(&mut tabs, 9));
        assert!(!select_workspace(&mut tabs, -1));
        assert_eq!(tabs.selected_workspace_index, Some(0));
    }

    #[test]
    fn close_workspace_before_selection_shifts_it_left() {
        let mut tabs = one_workspace_tabs("surface-1");
        new_workspace(&mut tabs, "surface-2");
        new_workspace(&mut tabs, "surface-3"); // 3 workspaces, selected = 2
        assert!(close_workspace(&mut tabs, 0));
        assert_eq!(tabs.workspaces.len(), 2);
        assert_eq!(tabs.selected_workspace_index, Some(1));
    }

    #[test]
    fn close_selected_last_workspace_clamps_selection() {
        let mut tabs = one_workspace_tabs("surface-1");
        new_workspace(&mut tabs, "surface-2"); // 2 workspaces, selected = 1 (the last)
        assert!(close_workspace(&mut tabs, 1));
        assert_eq!(tabs.workspaces.len(), 1);
        assert_eq!(tabs.selected_workspace_index, Some(0));
    }

    #[test]
    fn close_only_workspace_is_a_noop() {
        // Canonical `guard tabs.count > 1`: the sole workspace cannot be closed.
        let mut tabs = one_workspace_tabs("surface-1");
        assert!(!close_workspace(&mut tabs, 0));
        assert_eq!(tabs.workspaces.len(), 1);
        assert_eq!(tabs.selected_workspace_index, Some(0));
    }

    #[test]
    fn close_workspace_out_of_range_is_rejected() {
        let mut tabs = one_workspace_tabs("surface-1");
        new_workspace(&mut tabs, "surface-2");
        assert!(!close_workspace(&mut tabs, 5));
        assert_eq!(tabs.workspaces.len(), 2);
    }

    #[test]
    fn close_workspace_rejects_pinned_workspace_without_mutation() {
        let mut tabs = one_workspace_tabs("surface-1");
        new_workspace(&mut tabs, "surface-2");
        tabs.workspaces[0].is_pinned = Some(true);
        let before = tabs.clone();

        assert!(!close_workspace(&mut tabs, 0));
        assert_eq!(tabs, before);
    }

    #[test]
    fn new_workspace_inherits_selected_workspace_directory() {
        let mut tabs = one_workspace_tabs("surface-1");
        tabs.workspaces[0].current_directory = Some("C:/inherited".to_string());

        new_workspace(&mut tabs, "surface-2");

        assert_eq!(
            tabs.workspaces[1].current_directory.as_deref(),
            Some("C:/inherited")
        );
    }

    #[test]
    fn close_anchor_workspace_dissolves_its_group() {
        let mut tabs = SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-anchor".to_string()),
                    group_id: Some("g".to_string()),
                    ..fresh_terminal_workspace("surface-1")
                },
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-member".to_string()),
                    group_id: Some("g".to_string()),
                    ..fresh_terminal_workspace("surface-2")
                },
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-solo".to_string()),
                    ..fresh_terminal_workspace("surface-3")
                },
            ],
            workspace_groups: Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
                id: "g".to_string(),
                name: "Group".to_string(),
                anchor_workspace_id: Some("ws-anchor".to_string()),
                anchor_member_index: Some(0),
                ..Default::default()
            }]),
        };

        assert!(close_workspace(&mut tabs, 0));
        assert_eq!(tabs.workspaces.len(), 2);
        assert_eq!(
            tabs.workspaces[0].workspace_id.as_deref(),
            Some("ws-member")
        );
        assert_eq!(tabs.workspaces[0].group_id, None);
        assert_eq!(tabs.workspace_groups, None);
        assert_eq!(tabs.selected_workspace_index, Some(0));
    }

    #[test]
    fn close_group_member_preserves_group_and_reanchors_member_index() {
        let mut tabs = SessionTabManagerSnapshot {
            selected_workspace_index: Some(2),
            workspaces: vec![
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-anchor".to_string()),
                    group_id: Some("g".to_string()),
                    ..fresh_terminal_workspace("surface-1")
                },
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-member".to_string()),
                    group_id: Some("g".to_string()),
                    ..fresh_terminal_workspace("surface-2")
                },
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-solo".to_string()),
                    ..fresh_terminal_workspace("surface-3")
                },
            ],
            workspace_groups: Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
                id: "g".to_string(),
                name: "Group".to_string(),
                anchor_workspace_id: Some("ws-anchor".to_string()),
                anchor_member_index: Some(1),
                ..Default::default()
            }]),
        };

        assert!(close_workspace(&mut tabs, 1));
        let group = tabs.workspace_groups.as_ref().expect("group survives");
        assert_eq!(group.len(), 1);
        assert_eq!(group[0].anchor_workspace_id.as_deref(), Some("ws-anchor"));
        assert_eq!(group[0].anchor_member_index, Some(0));
        assert_eq!(tabs.selected_workspace_index, Some(1));
    }

    #[test]
    fn close_workspaces_targets_original_indices_in_tab_order() {
        let mut tabs = SessionTabManagerSnapshot {
            selected_workspace_index: Some(2),
            workspaces: vec![
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-0".to_string()),
                    ..fresh_terminal_workspace("surface-0")
                },
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-1".to_string()),
                    ..fresh_terminal_workspace("surface-1")
                },
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-2".to_string()),
                    ..fresh_terminal_workspace("surface-2")
                },
                SessionWorkspaceSnapshot {
                    workspace_id: Some("ws-3".to_string()),
                    ..fresh_terminal_workspace("surface-3")
                },
            ],
            workspace_groups: None,
        };

        assert!(close_workspaces(&mut tabs, &[3, 1, 1, 99, -1]));
        assert_eq!(
            tabs.workspaces
                .iter()
                .map(|workspace| workspace.workspace_id.as_deref().unwrap_or(""))
                .collect::<Vec<_>>(),
            ["ws-0", "ws-2"]
        );
        assert_eq!(tabs.selected_workspace_index, Some(1));
    }

    // --- Workspace rename (custom title) ---

    #[test]
    fn rename_workspace_sets_custom_title_and_user_source() {
        let mut tabs = one_workspace_tabs("surface-1");
        assert!(rename_workspace(&mut tabs, 0, "Fix auth"));
        let ws = &tabs.workspaces[0];
        assert_eq!(ws.custom_title.as_deref(), Some("Fix auth"));
        assert_eq!(ws.custom_title_source.as_deref(), Some("user"));
        // The process-title fallback is never touched by a rename.
        assert_eq!(ws.process_title, "Terminal");
    }

    #[test]
    fn rename_workspace_trims_padding() {
        let mut tabs = one_workspace_tabs("surface-1");
        assert!(rename_workspace(&mut tabs, 0, "  Fix auth  "));
        assert_eq!(tabs.workspaces[0].custom_title.as_deref(), Some("Fix auth"));
    }

    #[test]
    fn rename_workspace_empty_title_clears_both_fields() {
        let mut tabs = one_workspace_tabs("surface-1");
        tabs.workspaces[0].custom_title = Some("Named".to_string());
        tabs.workspaces[0].custom_title_source = Some("user".to_string());
        assert!(rename_workspace(&mut tabs, 0, ""));
        assert_eq!(tabs.workspaces[0].custom_title, None);
        assert_eq!(tabs.workspaces[0].custom_title_source, None);
    }

    #[test]
    fn rename_workspace_whitespace_only_clears_when_previously_set() {
        let mut tabs = one_workspace_tabs("surface-1");
        tabs.workspaces[0].custom_title = Some("Named".to_string());
        tabs.workspaces[0].custom_title_source = Some("user".to_string());
        assert!(rename_workspace(&mut tabs, 0, "   \t "));
        assert_eq!(tabs.workspaces[0].custom_title, None);
        assert_eq!(tabs.workspaces[0].custom_title_source, None);
    }

    #[test]
    fn rename_workspace_clearing_an_already_clear_title_is_no_change() {
        let mut tabs = one_workspace_tabs("surface-1");
        assert!(!rename_workspace(&mut tabs, 0, ""));
        assert_eq!(tabs.workspaces[0].custom_title, None);
    }

    #[test]
    fn rename_workspace_identical_title_is_no_change() {
        let mut tabs = one_workspace_tabs("surface-1");
        assert!(rename_workspace(&mut tabs, 0, "Fix auth"));
        assert!(!rename_workspace(&mut tabs, 0, "Fix auth"));
    }

    #[test]
    fn rename_workspace_same_title_flips_auto_source_to_user() {
        // An OSC/auto-stamped title renamed to the very same text still counts
        // as a change: the source flips "auto" → "user".
        let mut tabs = one_workspace_tabs("surface-1");
        tabs.workspaces[0].custom_title = Some("Fix auth".to_string());
        tabs.workspaces[0].custom_title_source = Some("auto".to_string());
        assert!(rename_workspace(&mut tabs, 0, "Fix auth"));
        assert_eq!(tabs.workspaces[0].custom_title.as_deref(), Some("Fix auth"));
        assert_eq!(
            tabs.workspaces[0].custom_title_source.as_deref(),
            Some("user")
        );
    }

    #[test]
    fn rename_workspace_out_of_range_and_negative_index_are_no_ops() {
        let mut tabs = tabs_with(2, 0, 0);
        let before = tabs.clone();
        assert!(!rename_workspace(&mut tabs, 2, "nope"));
        assert!(!rename_workspace(&mut tabs, -1, "nope"));
        assert_eq!(tabs, before);
    }

    #[test]
    fn set_workspace_description_normalizes_line_endings() {
        let mut tabs = tabs_with(1, 0, 0);
        assert!(set_workspace_description(
            &mut tabs,
            0,
            "alpha\r\nbeta\rgamma"
        ));
        assert_eq!(
            tabs.workspaces[0].custom_description.as_deref(),
            Some("alpha\nbeta\ngamma")
        );
    }

    #[test]
    fn set_workspace_description_preserves_nonempty_edge_whitespace() {
        let mut tabs = tabs_with(1, 0, 0);
        assert!(set_workspace_description(&mut tabs, 0, "  notes  "));
        assert_eq!(
            tabs.workspaces[0].custom_description.as_deref(),
            Some("  notes  ")
        );
    }

    #[test]
    fn set_workspace_description_empty_or_whitespace_clears() {
        let mut tabs = tabs_with(1, 0, 0);
        tabs.workspaces[0].custom_description = Some("Named".to_string());
        assert!(set_workspace_description(&mut tabs, 0, " \r\n\t "));
        assert_eq!(tabs.workspaces[0].custom_description, None);
    }

    #[test]
    fn set_workspace_description_identical_value_is_no_change() {
        let mut tabs = tabs_with(1, 0, 0);
        assert!(set_workspace_description(&mut tabs, 0, "alpha\nbeta"));
        assert!(!set_workspace_description(&mut tabs, 0, "alpha\nbeta"));
    }

    #[test]
    fn set_workspace_description_out_of_range_and_negative_index_are_no_ops() {
        let mut tabs = tabs_with(1, 0, 0);
        let before = tabs.clone();
        assert!(!set_workspace_description(&mut tabs, 2, "nope"));
        assert!(!set_workspace_description(&mut tabs, -1, "nope"));
        assert_eq!(tabs, before);
    }

    #[test]
    fn reset_workspace_color_clears_custom_color() {
        let mut tabs = tabs_with(1, 0, 0);
        tabs.workspaces[0].custom_color = Some("#C0392B".to_string());
        assert!(reset_workspace_color(&mut tabs, 0));
        assert_eq!(tabs.workspaces[0].custom_color, None);
    }

    #[test]
    fn reset_workspace_color_no_ops_when_clear_or_out_of_range() {
        let mut tabs = tabs_with(1, 0, 0);
        let before = tabs.clone();
        assert!(!reset_workspace_color(&mut tabs, 0));
        assert!(!reset_workspace_color(&mut tabs, 2));
        assert!(!reset_workspace_color(&mut tabs, -1));
        assert_eq!(tabs, before);
    }

    #[test]
    fn set_panel_title_sets_trims_and_clears_custom_title() {
        let mut workspace = fresh_terminal_workspace("surface-1");
        assert!(set_panel_title(&mut workspace, "surface-1", "  api logs  "));
        let titles = workspace.panel_titles.as_ref().expect("title metadata");
        assert_eq!(titles.len(), 1);
        assert_eq!(titles[0].panel_id, "surface-1");
        assert_eq!(titles[0].custom_title.as_deref(), Some("api logs"));

        assert!(!set_panel_title(&mut workspace, "surface-1", "api logs"));
        assert!(set_panel_title(&mut workspace, "surface-1", ""));
        assert_eq!(workspace.panel_titles, None);
    }

    #[test]
    fn set_panel_title_rejects_missing_layout_or_panel() {
        let mut workspace = fresh_terminal_workspace("surface-1");
        let before = workspace.clone();
        assert!(!set_panel_title(&mut workspace, "missing", "api logs"));
        assert_eq!(workspace, before);

        workspace.layout = None;
        assert!(!set_panel_title(&mut workspace, "surface-1", "api logs"));
        assert_eq!(workspace.panel_titles, None);
    }

    #[test]
    fn set_panel_pinned_sets_and_clears_panel_pin() {
        let mut workspace = fresh_terminal_workspace("surface-1");

        assert!(set_panel_pinned(&mut workspace, "surface-1", true));
        let pins = workspace.panel_pins.as_ref().expect("pin metadata");
        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0].panel_id, "surface-1");
        assert!(pins[0].is_pinned);

        assert!(!set_panel_pinned(&mut workspace, "surface-1", true));
        assert!(set_panel_pinned(&mut workspace, "surface-1", false));
        assert_eq!(workspace.panel_pins, None);
        assert!(!set_panel_pinned(&mut workspace, "surface-1", false));
    }

    #[test]
    fn set_panel_pinned_rejects_missing_layout_or_panel() {
        let mut workspace = fresh_terminal_workspace("surface-1");
        let before = workspace.clone();
        assert!(!set_panel_pinned(&mut workspace, "missing", true));
        assert_eq!(workspace, before);

        workspace.layout = None;
        assert!(!set_panel_pinned(&mut workspace, "surface-1", true));
        assert_eq!(workspace.panel_pins, None);
    }

    #[test]
    fn reorder_surface_matches_bonsplit_offsets_pin_tiers_and_focus_policy() {
        let mut workspace = fresh_terminal_workspace("a");
        let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
            panic!("expected pane");
        };
        pane.panel_ids = vec!["a".into(), "b".into(), "c".into()];
        pane.selected_panel_id = Some("b".into());
        workspace.panel_pins = Some(vec![SessionPanelPinSnapshot {
            panel_id: "a".into(),
            is_pinned: true,
        }]);

        assert_eq!(reorder_surface(&mut workspace, "c", 0, false), Some(true));
        assert_eq!(
            panel_ids(workspace.layout.as_ref().unwrap()),
            ["a", "c", "b"]
        );
        let Layout::Pane(pane) = workspace.layout.as_ref().unwrap() else {
            panic!("expected pane");
        };
        assert_eq!(pane.selected_panel_id.as_deref(), Some("b"));

        assert_eq!(reorder_surface(&mut workspace, "c", 3, true), Some(true));
        assert_eq!(
            panel_ids(workspace.layout.as_ref().unwrap()),
            ["a", "b", "c"]
        );
        let Layout::Pane(pane) = workspace.layout.as_ref().unwrap() else {
            panic!("expected pane");
        };
        assert_eq!(pane.selected_panel_id.as_deref(), Some("c"));
        assert_eq!(reorder_surface(&mut workspace, "missing", 0, false), None);
    }

    #[test]
    fn move_surface_cross_pane_collapses_source_and_honors_pin_tier() {
        let mut tabs = one_workspace_tabs("a");
        let mut source = pane("a");
        let Layout::Pane(source_pane) = &mut source else {
            unreachable!();
        };
        source_pane.pane_id = Some("pane-source".into());
        let mut target = pane("b");
        let Layout::Pane(target_pane) = &mut target else {
            unreachable!();
        };
        target_pane.pane_id = Some("pane-target".into());
        target_pane.panel_ids.push("c".into());
        target_pane.selected_panel_id = Some("b".into());
        tabs.workspaces[0].layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            source,
            target,
        ));
        tabs.workspaces[0].panel_pins = Some(vec![
            SessionPanelPinSnapshot {
                panel_id: "a".into(),
                is_pinned: true,
            },
            SessionPanelPinSnapshot {
                panel_id: "b".into(),
                is_pinned: true,
            },
        ]);

        assert_eq!(
            move_surface(&mut tabs, 0, "a", 0, "pane-target", Some(99), false),
            Some(true)
        );
        let Layout::Pane(target) = tabs.workspaces[0].layout.as_ref().unwrap() else {
            panic!("emptied source split should collapse to the target pane");
        };
        assert_eq!(target.pane_id.as_deref(), Some("pane-target"));
        assert_eq!(target.panel_ids, ["b", "a", "c"]);
        assert_eq!(target.selected_panel_id.as_deref(), Some("b"));
    }

    #[test]
    fn move_surface_cross_workspace_transfers_metadata_and_focuses_destination() {
        let mut tabs = one_workspace_tabs("a");
        let Layout::Pane(source) = tabs.workspaces[0].layout.as_mut().unwrap() else {
            unreachable!();
        };
        source.pane_id = Some("pane-source".into());
        source.panel_ids.push("b".into());
        assert!(set_panel_title(&mut tabs.workspaces[0], "b", "build"));
        assert!(set_panel_pinned(&mut tabs.workspaces[0], "b", true));
        assert!(set_panel_unread(&mut tabs.workspaces[0], "b", true));
        tabs.workspaces[0].listening_ports = Some(vec![3000]);
        tabs.workspaces[0].panel_listening_ports = Some(vec![SessionPanelListeningPortsSnapshot {
            panel_id: "b".into(),
            ports: vec![3000],
        }]);
        let mut destination = fresh_terminal_workspace("c");
        destination.workspace_id = Some("workspace-destination".into());
        let Layout::Pane(target) = destination.layout.as_mut().unwrap() else {
            unreachable!();
        };
        target.pane_id = Some("pane-target".into());
        tabs.workspaces.push(destination);

        assert_eq!(
            move_surface(&mut tabs, 0, "b", 1, "pane-target", None, true),
            Some(true)
        );
        assert_eq!(tabs.selected_workspace_index, Some(1));
        assert_eq!(tabs.workspaces[0].panel_titles, None);
        assert_eq!(tabs.workspaces[0].panel_pins, None);
        assert_eq!(tabs.workspaces[0].panel_unreads, None);
        assert_eq!(tabs.workspaces[0].listening_ports, None);
        assert_eq!(tabs.workspaces[0].panel_listening_ports, None);
        let target = match tabs.workspaces[1].layout.as_ref().unwrap() {
            Layout::Pane(target) => target,
            Layout::Split(_) => panic!("expected destination pane"),
        };
        assert_eq!(target.panel_ids, ["b", "c"]);
        assert_eq!(target.selected_panel_id.as_deref(), Some("b"));
        assert_eq!(
            tabs.workspaces[1].panel_titles.as_ref().unwrap()[0]
                .custom_title
                .as_deref(),
            Some("build")
        );
        assert!(tabs.workspaces[1].panel_pins.as_ref().unwrap()[0].is_pinned);
        assert!(tabs.workspaces[1].panel_unreads.as_ref().unwrap()[0].is_unread);
        assert_eq!(tabs.workspaces[1].listening_ports, Some(vec![3000]));
        assert_eq!(
            tabs.workspaces[1].panel_listening_ports.as_ref().unwrap()[0].ports,
            [3000]
        );
    }

    #[test]
    fn move_surface_invalid_destination_is_atomic() {
        let mut tabs = one_workspace_tabs("a");
        let before = tabs.clone();
        assert_eq!(
            move_surface(&mut tabs, 0, "a", 0, "missing-pane", Some(0), false),
            None
        );
        assert_eq!(tabs, before);
    }

    fn workspace_with_id(id: &str, panel_id: &str) -> SessionWorkspaceSnapshot {
        let mut workspace = fresh_terminal_workspace(panel_id);
        workspace.workspace_id = Some(id.to_string());
        workspace
    }

    #[test]
    fn move_workspace_to_window_detaches_group_and_preserves_unfocused_selection() {
        let group_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let mut source_tabs = SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![
                workspace_with_id("workspace-a", "a"),
                workspace_with_id("workspace-b", "b"),
            ],
            workspace_groups: Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
                id: group_id.to_string(),
                name: "group".to_string(),
                anchor_workspace_id: Some("workspace-a".to_string()),
                ..Default::default()
            }]),
        };
        source_tabs.workspaces[0].group_id = Some(group_id.to_string());
        source_tabs.workspaces[1].group_id = Some(group_id.to_string());
        source_tabs.workspaces[0].is_pinned = Some(true);
        let destination_tabs = SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: vec![workspace_with_id("workspace-c", "c")],
            workspace_groups: None,
        };
        let mut snapshot = crate::session::AppSessionSnapshot {
            windows: vec![
                crate::session::SessionWindowSnapshot {
                    window_id: Some("window-a".to_string()),
                    selected_workspace_id: None,
                    dock: None,
                    tab_manager: source_tabs,
                },
                crate::session::SessionWindowSnapshot {
                    window_id: Some("window-b".to_string()),
                    selected_workspace_id: None,
                    dock: None,
                    tab_manager: destination_tabs,
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            move_workspace_to_window(
                &mut snapshot,
                "workspace-a",
                "window-b",
                workspace_with_id("bootstrap", "bootstrap-panel"),
                false,
            ),
            Ok(())
        );
        assert_eq!(
            snapshot.windows[0].tab_manager.workspaces[0]
                .workspace_id
                .as_deref(),
            Some("workspace-b")
        );
        assert_eq!(snapshot.windows[0].tab_manager.workspaces[0].group_id, None);
        assert_eq!(snapshot.windows[0].tab_manager.workspace_groups, None);
        assert_eq!(
            snapshot.windows[0].tab_manager.selected_workspace_index,
            Some(0)
        );
        assert_eq!(
            snapshot.windows[1].tab_manager.selected_workspace_index,
            Some(1)
        );
        assert_eq!(
            snapshot.windows[1]
                .tab_manager
                .workspaces
                .iter()
                .map(|workspace| workspace.workspace_id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["workspace-a", "workspace-c"]
        );
        assert_eq!(snapshot.windows[1].tab_manager.workspaces[0].group_id, None);
    }

    #[test]
    fn move_workspace_to_window_bootstraps_empty_source_and_focuses_destination() {
        let mut snapshot = crate::session::AppSessionSnapshot {
            windows: vec![
                crate::session::SessionWindowSnapshot {
                    window_id: Some("window-a".to_string()),
                    selected_workspace_id: None,
                    dock: None,
                    tab_manager: SessionTabManagerSnapshot {
                        selected_workspace_index: Some(0),
                        workspaces: vec![workspace_with_id("workspace-a", "a")],
                        workspace_groups: None,
                    },
                },
                crate::session::SessionWindowSnapshot {
                    window_id: Some("window-b".to_string()),
                    selected_workspace_id: None,
                    dock: None,
                    tab_manager: SessionTabManagerSnapshot {
                        selected_workspace_index: Some(0),
                        workspaces: vec![workspace_with_id("workspace-b", "b")],
                        workspace_groups: None,
                    },
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            move_workspace_to_window(
                &mut snapshot,
                "workspace-a",
                "window-b",
                workspace_with_id("bootstrap", "bootstrap-panel"),
                true,
            ),
            Ok(())
        );
        assert_eq!(
            snapshot.windows[0].tab_manager.workspaces[0]
                .workspace_id
                .as_deref(),
            Some("bootstrap")
        );
        assert_eq!(
            snapshot.windows[0].tab_manager.selected_workspace_index,
            Some(0)
        );
        assert_eq!(
            snapshot.windows[1].tab_manager.selected_workspace_index,
            Some(1)
        );
        assert_eq!(
            snapshot.windows[1].tab_manager.workspaces[1]
                .workspace_id
                .as_deref(),
            Some("workspace-a")
        );
    }

    #[test]
    fn move_workspace_to_same_window_uses_detach_attach_semantics() {
        let mut snapshot = crate::session::AppSessionSnapshot {
            windows: vec![crate::session::SessionWindowSnapshot {
                window_id: Some("window-a".to_string()),
                selected_workspace_id: None,
                dock: None,
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index: Some(0),
                    workspaces: vec![workspace_with_id("workspace-a", "a")],
                    workspace_groups: None,
                },
            }],
            ..Default::default()
        };

        assert_eq!(
            move_workspace_to_window(
                &mut snapshot,
                "workspace-a",
                "window-a",
                workspace_with_id("bootstrap", "bootstrap-panel"),
                false,
            ),
            Ok(())
        );
        assert_eq!(
            snapshot.windows[0]
                .tab_manager
                .workspaces
                .iter()
                .map(|workspace| workspace.workspace_id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["bootstrap", "workspace-a"]
        );
        assert_eq!(
            snapshot.windows[0].tab_manager.selected_workspace_index,
            Some(0)
        );
    }

    #[test]
    fn move_workspace_to_window_rejects_missing_targets_atomically() {
        let mut snapshot = crate::session::AppSessionSnapshot {
            windows: vec![crate::session::SessionWindowSnapshot {
                window_id: Some("window-a".to_string()),
                selected_workspace_id: None,
                dock: None,
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index: Some(0),
                    workspaces: vec![workspace_with_id("workspace-a", "a")],
                    workspace_groups: None,
                },
            }],
            ..Default::default()
        };
        let before = snapshot.clone();
        assert_eq!(
            move_workspace_to_window(
                &mut snapshot,
                "workspace-a",
                "missing-window",
                workspace_with_id("bootstrap", "bootstrap-panel"),
                false,
            ),
            Err(MoveWorkspaceToWindowError::WindowNotFound)
        );
        assert_eq!(snapshot, before);
    }

    #[test]
    fn split_off_surface_moves_tab_into_adjacent_pane() {
        let mut workspace = fresh_terminal_workspace("a");
        let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
            unreachable!();
        };
        pane.pane_id = Some("pane-source".to_string());
        pane.panel_ids = vec!["a".into(), "b".into(), "c".into()];
        pane.selected_panel_id = Some("a".into());

        assert_eq!(
            split_off_surface(
                &mut workspace,
                "b",
                SessionSplitOrientation::Horizontal,
                false,
            ),
            Ok(())
        );
        let Layout::Split(split) = workspace.layout.as_ref().unwrap() else {
            panic!("expected split");
        };
        assert_eq!(split.orientation, SessionSplitOrientation::Horizontal);
        let Layout::Pane(source) = split.first.as_ref() else {
            panic!("expected source pane first");
        };
        let Layout::Pane(moved) = split.second.as_ref() else {
            panic!("expected moved pane second");
        };
        assert_eq!(source.pane_id.as_deref(), Some("pane-source"));
        assert_eq!(source.panel_ids, ["a", "c"]);
        assert_eq!(source.selected_panel_id.as_deref(), Some("a"));
        assert_eq!(moved.pane_id, None);
        assert_eq!(moved.panel_ids, ["b"]);
        assert_eq!(moved.selected_panel_id.as_deref(), Some("b"));
    }

    #[test]
    fn split_off_surface_rejects_single_tab_pane_atomically() {
        let mut workspace = fresh_terminal_workspace("a");
        let before = workspace.clone();
        assert_eq!(
            split_off_surface(&mut workspace, "a", SessionSplitOrientation::Vertical, true,),
            Err(SplitOffSurfaceError::WouldEmptySourcePane)
        );
        assert_eq!(workspace, before);
    }

    #[test]
    fn swap_selected_pane_surfaces_matches_canonical_move_order() {
        let mut source = pane("a");
        let Layout::Pane(source_pane) = &mut source else {
            unreachable!();
        };
        source_pane.pane_id = Some("pane-source".into());
        source_pane.panel_ids.push("b".into());
        source_pane.selected_panel_id = Some("b".into());
        let mut target = pane("c");
        let Layout::Pane(target_pane) = &mut target else {
            unreachable!();
        };
        target_pane.pane_id = Some("pane-target".into());
        target_pane.panel_ids.push("d".into());
        target_pane.selected_panel_id = Some("c".into());
        let mut workspace = fresh_terminal_workspace("unused");
        workspace.layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            source,
            target,
        ));
        workspace.panel_pins = Some(vec![
            SessionPanelPinSnapshot {
                panel_id: "b".into(),
                is_pinned: true,
            },
            SessionPanelPinSnapshot {
                panel_id: "d".into(),
                is_pinned: true,
            },
        ]);

        assert_eq!(
            swap_selected_pane_surfaces(&mut workspace, "pane-source", "pane-target"),
            Ok(PaneSwapResult {
                source_surface_id: "b".into(),
                target_surface_id: "c".into(),
            })
        );
        let Layout::Split(split) = workspace.layout.as_ref().unwrap() else {
            panic!("expected split");
        };
        let Layout::Pane(source) = split.first.as_ref() else {
            panic!("expected source pane");
        };
        let Layout::Pane(target) = split.second.as_ref() else {
            panic!("expected target pane");
        };
        assert_eq!(source.pane_id.as_deref(), Some("pane-source"));
        assert_eq!(target.pane_id.as_deref(), Some("pane-target"));
        assert_eq!(source.panel_ids, ["a", "c"]);
        assert_eq!(target.panel_ids, ["d", "b"]);
        assert_eq!(source.selected_panel_id.as_deref(), Some("a"));
        assert_eq!(target.selected_panel_id.as_deref(), Some("d"));
    }

    #[test]
    fn swap_selected_singleton_panes_preserves_identities_and_selection() {
        let mut source = pane("a");
        let Layout::Pane(source_pane) = &mut source else {
            unreachable!();
        };
        source_pane.pane_id = Some("pane-source".into());
        let mut target = pane("b");
        let Layout::Pane(target_pane) = &mut target else {
            unreachable!();
        };
        target_pane.pane_id = Some("pane-target".into());
        let mut workspace = fresh_terminal_workspace("unused");
        workspace.layout = Some(split(
            SessionSplitOrientation::Vertical,
            0.5,
            source,
            target,
        ));

        assert!(swap_selected_pane_surfaces(&mut workspace, "pane-source", "pane-target").is_ok());
        let Layout::Split(split) = workspace.layout.as_ref().unwrap() else {
            panic!("expected split");
        };
        let Layout::Pane(source) = split.first.as_ref() else {
            panic!("expected source pane");
        };
        let Layout::Pane(target) = split.second.as_ref() else {
            panic!("expected target pane");
        };
        assert_eq!(source.panel_ids, ["b"]);
        assert_eq!(target.panel_ids, ["a"]);
        assert_eq!(source.selected_panel_id.as_deref(), Some("b"));
        assert_eq!(target.selected_panel_id.as_deref(), Some("a"));
    }

    #[test]
    fn swap_selected_pane_surfaces_rejects_invalid_target_atomically() {
        let mut workspace = fresh_terminal_workspace("a");
        let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
            unreachable!();
        };
        pane.pane_id = Some("pane-source".into());
        let before = workspace.clone();

        assert_eq!(
            swap_selected_pane_surfaces(&mut workspace, "pane-source", "pane-missing"),
            Err(PaneSwapError::TargetPaneNotFound)
        );
        assert_eq!(workspace, before);
    }

    #[test]
    fn break_surface_to_new_workspace_transfers_state_without_forcing_focus() {
        let mut tabs = one_workspace_tabs("a");
        let Layout::Pane(pane) = tabs.workspaces[0].layout.as_mut().unwrap() else {
            unreachable!();
        };
        pane.pane_id = Some("pane-source".into());
        pane.panel_ids.push("b".into());
        pane.selected_panel_id = Some("b".into());
        assert!(set_panel_title(&mut tabs.workspaces[0], "b", "Build"));

        let result = break_surface_to_new_workspace(&mut tabs, 0, "b", false).unwrap();

        assert_eq!(result.surface_id, "b");
        assert_eq!(result.workspace_index, 1);
        assert_eq!(tabs.selected_workspace_index, Some(0));
        assert_eq!(
            panel_ids(tabs.workspaces[0].layout.as_ref().unwrap()),
            ["a"]
        );
        let destination = &tabs.workspaces[1];
        assert_eq!(panel_ids(destination.layout.as_ref().unwrap()), ["b"]);
        let Layout::Pane(destination_pane) = destination.layout.as_ref().unwrap() else {
            unreachable!();
        };
        assert_eq!(destination_pane.pane_id, None);
        assert_eq!(destination.process_title, "Build");
        assert_eq!(destination.panel_titles.as_ref().unwrap()[0].panel_id, "b");
    }

    #[test]
    fn break_surface_to_new_workspace_is_atomic_for_missing_surface() {
        let mut tabs = one_workspace_tabs("a");
        let before = tabs.clone();
        assert_eq!(
            break_surface_to_new_workspace(&mut tabs, 0, "missing", true),
            Err(PaneBreakError::SurfaceNotFound)
        );
        assert_eq!(tabs, before);
    }

    #[test]
    fn break_only_surface_leaves_empty_source_and_focuses_new_workspace() {
        let mut tabs = one_workspace_tabs("a");

        let result = break_surface_to_new_workspace(&mut tabs, 0, "a", true).unwrap();

        assert_eq!(result.workspace_index, 1);
        assert_eq!(tabs.workspaces[0].layout, None);
        assert_eq!(
            panel_ids(tabs.workspaces[1].layout.as_ref().unwrap()),
            ["a"]
        );
        assert_eq!(tabs.selected_workspace_index, Some(1));
    }

    #[test]
    fn focus_alternate_pane_selects_first_pane_different_from_focus() {
        let mut first = pane("a");
        let Layout::Pane(first_pane) = &mut first else {
            unreachable!();
        };
        first_pane.pane_id = Some("pane-a".into());
        let mut second = pane("b");
        let Layout::Pane(second_pane) = &mut second else {
            unreachable!();
        };
        second_pane.pane_id = Some("pane-b".into());
        let mut workspace = fresh_terminal_workspace("unused");
        workspace.layout = Some(split(
            SessionSplitOrientation::Horizontal,
            0.5,
            first,
            second,
        ));

        assert_eq!(
            focus_alternate_pane(&workspace, Some("pane-a")),
            Ok(PaneLastResult {
                pane_id: "pane-b".into(),
                surface_id: Some("b".into()),
            })
        );
        assert_eq!(
            focus_alternate_pane(&workspace, Some("pane-b")),
            Ok(PaneLastResult {
                pane_id: "pane-a".into(),
                surface_id: Some("a".into()),
            })
        );
        assert_eq!(
            focus_alternate_pane(&workspace, None),
            Err(PaneLastError::NoFocusedPane)
        );
    }

    #[test]
    fn focus_pane_target_preserves_selected_surface_and_rejects_missing_pane() {
        let mut workspace = fresh_terminal_workspace("a");
        let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
            unreachable!();
        };
        pane.pane_id = Some("pane-a".into());
        pane.panel_ids.push("b".into());
        pane.selected_panel_id = Some("b".into());
        assert_eq!(
            focus_pane_target(&workspace, "pane-a"),
            Ok(PaneLastResult {
                pane_id: "pane-a".into(),
                surface_id: Some("b".into()),
            })
        );
        assert_eq!(
            focus_pane_target(&workspace, "missing"),
            Err(PaneFocusError::PaneNotFound)
        );
    }

    #[test]
    fn focus_alternate_pane_rejects_single_pane() {
        let mut workspace = fresh_terminal_workspace("a");
        let Layout::Pane(pane) = workspace.layout.as_mut().unwrap() else {
            unreachable!();
        };
        pane.pane_id = Some("pane-a".into());
        assert_eq!(
            focus_alternate_pane(&workspace, Some("pane-a")),
            Err(PaneLastError::NoAlternatePane)
        );
    }

    fn resize_workspace() -> SessionWorkspaceSnapshot {
        let mut a = pane("a");
        let Layout::Pane(a_pane) = &mut a else {
            unreachable!()
        };
        a_pane.pane_id = Some("pane-a".into());
        let mut b = pane("b");
        let Layout::Pane(b_pane) = &mut b else {
            unreachable!()
        };
        b_pane.pane_id = Some("pane-b".into());
        let mut c = pane("c");
        let Layout::Pane(c_pane) = &mut c else {
            unreachable!()
        };
        c_pane.pane_id = Some("pane-c".into());
        let mut inner = split(SessionSplitOrientation::Horizontal, 0.5, a, b);
        let Layout::Split(inner_split) = &mut inner else {
            unreachable!()
        };
        inner_split.split_id = Some("split-inner".into());
        let mut root = split(SessionSplitOrientation::Horizontal, 0.6, inner, c);
        let Layout::Split(root_split) = &mut root else {
            unreachable!()
        };
        root_split.split_id = Some("split-root".into());
        SessionWorkspaceSnapshot {
            layout: Some(root),
            ..fresh_terminal_workspace("unused")
        }
    }

    #[test]
    fn resize_pane_relative_uses_nearest_matching_adjacent_ancestor() {
        let mut workspace = resize_workspace();
        assert_eq!(
            resize_pane_relative(
                &mut workspace,
                "pane-a",
                PaneResizeDirection::Right,
                60,
                1000.0,
                800.0,
            ),
            Ok(PaneResizeResult {
                split_id: "split-inner".into(),
                old_divider_position: 0.5,
                new_divider_position: 0.6,
            })
        );
        let Layout::Split(root) = workspace.layout.as_ref().unwrap() else {
            unreachable!()
        };
        assert_eq!(root.divider_position, 0.6);
        let Layout::Split(inner) = root.first.as_ref() else {
            unreachable!()
        };
        assert!((inner.divider_position - 0.6).abs() < 1e-9);
    }

    #[test]
    fn resize_pane_relative_walks_outward_for_requested_border() {
        let mut workspace = resize_workspace();
        let result = resize_pane_relative(
            &mut workspace,
            "pane-b",
            PaneResizeDirection::Right,
            100,
            1000.0,
            800.0,
        )
        .unwrap();
        assert_eq!(result.split_id, "split-root");
        assert!((result.new_divider_position - 0.7).abs() < 1e-9);

        let before = workspace.clone();
        assert_eq!(
            resize_pane_relative(
                &mut workspace,
                "pane-c",
                PaneResizeDirection::Right,
                10,
                1000.0,
                800.0,
            ),
            Err(PaneResizeError::NoAdjacentBorder)
        );
        assert_eq!(workspace, before);
    }

    #[test]
    fn resize_pane_absolute_uses_target_child_fraction_and_clamps() {
        let mut workspace = resize_workspace();
        let result = resize_pane_absolute(
            &mut workspace,
            "pane-b",
            SessionSplitOrientation::Horizontal,
            120.0,
            1000.0,
            800.0,
        )
        .unwrap();
        assert_eq!(result.split_id, "split-inner");
        assert!((result.new_divider_position - 0.8).abs() < 1e-9);

        let clamped = resize_pane_absolute(
            &mut workspace,
            "pane-a",
            SessionSplitOrientation::Horizontal,
            1.0,
            1000.0,
            800.0,
        )
        .unwrap();
        assert_eq!(clamped.new_divider_position, MIN_DIVIDER);
    }

    #[test]
    fn resize_pane_rejects_missing_axis_or_identity_without_mutating() {
        let mut workspace = resize_workspace();
        let before = workspace.clone();
        assert_eq!(
            resize_pane_relative(
                &mut workspace,
                "pane-a",
                PaneResizeDirection::Down,
                10,
                1000.0,
                800.0,
            ),
            Err(PaneResizeError::NoOrientationSplitAncestor)
        );
        assert_eq!(workspace, before);

        let Layout::Split(root) = workspace.layout.as_mut().unwrap() else {
            unreachable!()
        };
        let Layout::Split(inner) = root.first.as_mut() else {
            unreachable!()
        };
        inner.split_id = None;
        let before = workspace.clone();
        assert_eq!(
            resize_pane_relative(
                &mut workspace,
                "pane-a",
                PaneResizeDirection::Right,
                10,
                1000.0,
                800.0,
            ),
            Err(PaneResizeError::MissingSplitIdentity)
        );
        assert_eq!(workspace, before);
    }

    #[test]
    fn set_panel_unread_sets_and_clears_panel_unread() {
        let mut workspace = fresh_terminal_workspace("surface-1");

        assert!(set_panel_unread_at(
            &mut workspace,
            "surface-1",
            true,
            Some(123)
        ));
        let unreads = workspace.panel_unreads.as_ref().expect("unread metadata");
        assert_eq!(unreads.len(), 1);
        assert_eq!(unreads[0].panel_id, "surface-1");
        assert!(unreads[0].is_unread);
        assert_eq!(unreads[0].unread_at, Some(123));

        assert!(!set_panel_unread(&mut workspace, "surface-1", true));
        assert!(set_panel_unread(&mut workspace, "surface-1", false));
        assert_eq!(workspace.panel_unreads, None);
        assert!(!set_panel_unread(&mut workspace, "surface-1", false));
    }

    #[test]
    fn set_panel_unread_rejects_missing_layout_or_panel() {
        let mut workspace = fresh_terminal_workspace("surface-1");
        let before = workspace.clone();
        assert!(!set_panel_unread(&mut workspace, "missing", true));
        assert_eq!(workspace, before);

        workspace.layout = None;
        assert!(!set_panel_unread(&mut workspace, "surface-1", true));
        assert_eq!(workspace.panel_unreads, None);
    }

    #[test]
    fn set_workspace_unread_sets_preferred_or_first_panel_and_clears_all() {
        let mut tabs = tabs_with(2, 0, 0);
        tabs.workspaces[0].layout = Some(SessionWorkspaceLayoutSnapshot::Split(
            SessionSplitLayoutSnapshot {
                split_id: None,
                orientation: SessionSplitOrientation::Horizontal,
                divider_position: 0.5,
                first: Box::new(single_pane("surface-1")),
                second: Box::new(single_pane("surface-2")),
            },
        ));

        assert!(set_workspace_unread_at(
            &mut tabs,
            0,
            Some("surface-2"),
            true,
            Some(456)
        ));
        let unreads = tabs.workspaces[0]
            .panel_unreads
            .as_ref()
            .expect("unread metadata");
        assert_eq!(unreads.len(), 1);
        assert_eq!(unreads[0].panel_id, "surface-2");
        assert!(unreads[0].is_unread);
        assert_eq!(unreads[0].unread_at, Some(456));

        assert!(!set_workspace_unread(&mut tabs, 0, Some("surface-1"), true));
        assert!(set_workspace_unread(&mut tabs, 0, None, false));
        assert_eq!(tabs.workspaces[0].panel_unreads, None);

        assert!(set_workspace_unread(&mut tabs, 0, Some("missing"), true));
        assert_eq!(
            tabs.workspaces[0].panel_unreads.as_ref().unwrap()[0].panel_id,
            "surface-1"
        );
    }

    #[test]
    fn set_workspace_unread_rejects_invalid_workspace_or_empty_layout() {
        let mut tabs = tabs_with(1, 0, 0);
        let before = tabs.clone();
        assert!(!set_workspace_unread(&mut tabs, -1, None, true));
        assert!(!set_workspace_unread(&mut tabs, 2, None, true));
        assert_eq!(tabs, before);

        tabs.workspaces[0].layout = None;
        assert!(!set_workspace_unread(&mut tabs, 0, None, true));
        assert_eq!(tabs.workspaces[0].panel_unreads, None);
    }

    // --- Workspace pin/unpin ---

    /// The `panel_ids` of each workspace's layout — a stable identity for order
    /// assertions (all `tabs_with` workspaces share the "Terminal" title).
    fn order_of(tabs: &SessionTabManagerSnapshot) -> Vec<String> {
        tabs.workspaces
            .iter()
            .map(|w| panel_ids(w.layout.as_ref().unwrap())[0].clone())
            .collect()
    }

    #[test]
    fn pin_moves_workspace_to_end_of_pinned_prefix() {
        let mut tabs = tabs_with(4, 2, 0);
        assert!(set_workspace_pinned(&mut tabs, 3, true));
        // The newly pinned tab lands at the END of the pinned prefix (index 2);
        // the unpinned remainder keeps its relative order.
        assert_eq!(
            order_of(&tabs),
            ["surface-0", "surface-1", "surface-3", "surface-2"]
        );
        assert_eq!(tabs.workspaces[2].is_pinned, Some(true));
    }

    #[test]
    fn pin_first_of_all_unpinned_stays_at_index_0() {
        let mut tabs = tabs_with(3, 0, 0);
        assert!(set_workspace_pinned(&mut tabs, 0, true));
        assert_eq!(order_of(&tabs), ["surface-0", "surface-1", "surface-2"]);
        assert_eq!(tabs.workspaces[0].is_pinned, Some(true));
    }

    #[test]
    fn pin_middle_moves_to_front() {
        let mut tabs = tabs_with(3, 0, 0);
        assert!(set_workspace_pinned(&mut tabs, 1, true));
        assert_eq!(order_of(&tabs), ["surface-1", "surface-0", "surface-2"]);
        assert_eq!(tabs.workspaces[0].is_pinned, Some(true));
    }

    #[test]
    fn unpin_inserts_at_front_of_unpinned_segment() {
        let mut tabs = tabs_with(3, 2, 0);
        assert!(set_workspace_pinned(&mut tabs, 0, false));
        // The unpinned tab lands at the FRONT of the unpinned segment.
        assert_eq!(order_of(&tabs), ["surface-1", "surface-0", "surface-2"]);
        // Unpin stores None (omit-key), never Some(false) — golden
        // byte-stability: the serialized object must not carry the key at all.
        assert_eq!(tabs.workspaces[1].is_pinned, None);
        let object = serde_json::to_value(&tabs.workspaces[1]).unwrap();
        assert!(!object.as_object().unwrap().contains_key("is_pinned"));
    }

    #[test]
    fn pin_already_pinned_is_no_change() {
        let mut tabs = tabs_with(3, 2, 0);
        let before = tabs.clone();
        assert!(!set_workspace_pinned(&mut tabs, 0, true));
        assert_eq!(tabs, before);
    }

    #[test]
    fn unpin_never_pinned_is_no_change() {
        let mut tabs = tabs_with(3, 2, 0);
        let before = tabs.clone();
        assert!(!set_workspace_pinned(&mut tabs, 2, false));
        assert_eq!(tabs, before);
    }

    #[test]
    fn set_workspace_pinned_out_of_range_and_negative_index_are_no_ops() {
        let mut tabs = tabs_with(2, 0, 0);
        let before = tabs.clone();
        assert!(!set_workspace_pinned(&mut tabs, 2, true));
        assert!(!set_workspace_pinned(&mut tabs, -1, true));
        assert_eq!(tabs, before);
    }

    #[test]
    fn selection_follows_the_pinned_workspace() {
        let mut tabs = tabs_with(3, 0, 2);
        assert!(set_workspace_pinned(&mut tabs, 2, true));
        // ws2 moved to index 0; the selection follows it there.
        assert_eq!(order_of(&tabs), ["surface-2", "surface-0", "surface-1"]);
        assert_eq!(tabs.selected_workspace_index, Some(0));
    }

    #[test]
    fn selection_stays_on_unmoved_workspace() {
        let mut tabs = tabs_with(3, 0, 1);
        assert!(set_workspace_pinned(&mut tabs, 2, true));
        // ws2 moved to index 0, shifting ws1 to index 2 — selection follows.
        assert_eq!(order_of(&tabs), ["surface-2", "surface-0", "surface-1"]);
        assert_eq!(tabs.selected_workspace_index, Some(2));
    }

    #[test]
    fn unpin_selection_follows() {
        // Selected tab is the one being unpinned: follows it to index 1.
        let mut tabs = tabs_with(3, 2, 0);
        assert!(set_workspace_pinned(&mut tabs, 0, false));
        assert_eq!(tabs.selected_workspace_index, Some(1));

        // Selected tab is the OTHER pinned tab (p1): unpinning index 0 moves
        // p1 up to index 0 — selection follows.
        let mut tabs = tabs_with(3, 2, 1);
        assert!(set_workspace_pinned(&mut tabs, 0, false));
        assert_eq!(tabs.selected_workspace_index, Some(0));
    }

    #[test]
    fn grouped_workspace_pin_preserves_membership_and_normalizes_contiguity() {
        let group_id = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let mut tabs = tabs_with(4, 0, 1);
        tabs.workspaces[0].group_id = Some(group_id.to_string());
        tabs.workspaces[2].group_id = Some(group_id.to_string());
        tabs.workspace_groups = Some(vec![group(group_id, false)]);
        assert!(set_workspace_pinned(&mut tabs, 2, true));
        // Group membership is preserved, but the canonical normalization tail
        // repairs the broken group run and selection follows the shifted row.
        assert_eq!(
            order_of(&tabs),
            ["surface-0", "surface-2", "surface-1", "surface-3"]
        );
        assert_eq!(tabs.workspaces[1].is_pinned, Some(true));
        assert_eq!(tabs.workspaces[1].group_id.as_deref(), Some(group_id));
        assert_eq!(tabs.selected_workspace_index, Some(2));
    }

    #[test]
    fn boundary_counts_grouped_rows_by_group_pin() {
        // Leading grouped members of a PINNED group (members' own is_pinned is
        // None) followed by unpinned rows; pinning a trailing ungrouped ws must
        // insert AFTER the grouped pinned run (isGlobalPinnedRow parity:
        // grouped rows count by their group's pin, Ordering.swift:201-207).
        let mut tabs = tabs_with(4, 0, 0);
        tabs.workspaces[0].group_id = Some("g".to_string());
        tabs.workspaces[1].group_id = Some("g".to_string());
        tabs.workspace_groups = Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
            is_pinned: Some(true),
            ..group("g", false)
        }]);
        assert!(set_workspace_pinned(&mut tabs, 3, true));
        assert_eq!(
            order_of(&tabs),
            ["surface-0", "surface-1", "surface-3", "surface-2"]
        );
        assert_eq!(tabs.workspaces[2].is_pinned, Some(true));
    }

    #[test]
    fn boundary_dangling_group_id_falls_back_to_own_pin() {
        // A leading row whose group_id resolves to NO group still counts by
        // its own is_pinned (the oracle's isGlobalPinnedRow nil-group arm,
        // Ordering.swift:201-207) — it must not be treated as unpinned.
        let mut tabs = tabs_with(3, 0, 0);
        tabs.workspaces[0].is_pinned = Some(true);
        tabs.workspaces[0].group_id = Some("gone".to_string());
        tabs.workspace_groups = None;
        assert!(set_workspace_pinned(&mut tabs, 2, true));
        // surface-2 lands AFTER the dangling-group pinned row, not before it.
        assert_eq!(order_of(&tabs), ["surface-0", "surface-2", "surface-1"]);
    }

    // --- Workspace reorder ---

    const R_W1: &str = "00000000-0000-0000-0000-000000000001";
    const R_W2: &str = "00000000-0000-0000-0000-000000000002";
    const R_W3: &str = "00000000-0000-0000-0000-000000000003";
    const R_W4: &str = "00000000-0000-0000-0000-000000000004";
    const R_G1: &str = "11111111-0000-0000-0000-000000000001";
    const R_G2: &str = "11111111-0000-0000-0000-000000000002";
    /// Parseable UUID that never appears in `workspace_groups` (dangling).
    const R_G_DANGLING: &str = "dddddddd-0000-0000-0000-000000000001";

    /// Workspaces with fixed UUID ids; panel id mirrors the position (for
    /// `order_of`) and each spec is `(workspace_id, group_id, pinned)`.
    fn reorder_tabs(
        specs: &[(&str, Option<&str>, bool)],
        selected: Option<i64>,
    ) -> SessionTabManagerSnapshot {
        let workspaces = specs
            .iter()
            .enumerate()
            .map(|(i, (id, gid, pinned))| SessionWorkspaceSnapshot {
                workspace_id: Some(id.to_string()),
                group_id: gid.map(str::to_string),
                is_pinned: pinned.then_some(true),
                ..fresh_terminal_workspace(&format!("surface-{i}"))
            })
            .collect();
        SessionTabManagerSnapshot {
            selected_workspace_index: selected,
            workspaces,
            workspace_groups: None,
        }
    }

    fn ws_id_order(tabs: &SessionTabManagerSnapshot) -> Vec<&str> {
        tabs.workspaces
            .iter()
            .map(|w| w.workspace_id.as_deref().unwrap_or(""))
            .collect()
    }

    fn uuid(raw: &str) -> Uuid {
        Uuid::parse_str(raw).unwrap()
    }

    #[test]
    fn create_workspace_group_adopts_eligible_children_at_first_child_slot() {
        let mut tabs = reorder_tabs(
            &[
                (R_W1, None, false),
                (R_W2, None, true),
                (R_W3, None, false),
                (R_W4, None, false), // fresh anchor, appended by the host
            ],
            Some(2),
        );

        let created = create_workspace_group_snapshot(
            &mut tabs,
            uuid(R_G1),
            "  Team  ",
            uuid(R_W4),
            &[uuid(R_W3), uuid(R_W2), uuid(R_W1)],
        )
        .unwrap();

        // Pinned W2 is ineligible. Eligible children retain tab order rather
        // than request order, and the run replaces the first child's slot.
        assert_eq!(ws_id_order(&tabs), [R_W2, R_W4, R_W1, R_W3]);
        assert_eq!(tabs.selected_workspace_index, Some(3));
        assert_eq!(created.name, "Team");
        assert_eq!(created.anchor_workspace_id.as_deref(), Some(R_W4));
        assert_eq!(
            tabs.workspaces
                .iter()
                .map(|workspace| workspace.group_id.as_deref())
                .collect::<Vec<_>>(),
            [None, Some(R_G1), Some(R_G1), Some(R_G1)]
        );
    }

    #[test]
    fn add_workspace_rejects_foreign_anchor_and_expands_for_selected_member() {
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G1), false),
                (R_W2, Some(R_G1), false),
                (R_W3, Some(R_G2), false),
                (R_W4, None, false),
            ],
            Some(3),
        );
        tabs.workspace_groups = Some(vec![
            SessionWorkspaceGroupSnapshot {
                is_collapsed: true,
                ..reorder_group(R_G1, R_W1, false)
            },
            reorder_group(R_G2, R_W3, false),
        ]);
        let before = tabs.clone();
        assert_eq!(
            add_workspace_to_group_snapshot(&mut tabs, uuid(R_G1), uuid(R_W3), None, None),
            Err(WorkspaceGroupMutationError::WorkspaceIsOtherGroupAnchor)
        );
        assert_eq!(tabs, before);

        assert_eq!(
            add_workspace_to_group_snapshot(
                &mut tabs,
                uuid(R_G1),
                uuid(R_W4),
                Some(WorkspaceGroupPlacement::Top),
                None,
            ),
            Ok(true)
        );
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W4, R_W2, R_W3]);
        assert_eq!(tabs.selected_workspace_index, Some(1));
        assert!(!tabs.workspace_groups.as_ref().unwrap()[0].is_collapsed);
    }

    #[test]
    fn remove_member_normalizes_but_remove_anchor_flattens_in_place() {
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G1), false),
                (R_W2, Some(R_G1), false),
                (R_W3, None, false),
            ],
            Some(1),
        );
        tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
        assert_eq!(
            remove_workspace_from_group_snapshot(&mut tabs, uuid(R_W2)),
            Ok(true)
        );
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W2, R_W3]);
        assert_eq!(tabs.workspaces[1].group_id, None);
        assert_eq!(tabs.selected_workspace_index, Some(1));

        tabs.workspaces[1].group_id = Some(R_G1.to_string());
        assert_eq!(
            remove_workspace_from_group_snapshot(&mut tabs, uuid(R_W1)),
            Ok(true)
        );
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W2, R_W3]);
        assert!(tabs
            .workspaces
            .iter()
            .all(|workspace| workspace.group_id.is_none()));
        assert!(tabs.workspace_groups.as_ref().unwrap().is_empty());
    }

    #[test]
    fn group_anchor_pin_metadata_and_slot_move_preserve_selection_identity() {
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G1), false),
                (R_W2, Some(R_G1), false),
                (R_W3, None, false),
                (R_W4, Some(R_G2), false),
            ],
            Some(1),
        );
        tabs.workspace_groups = Some(vec![
            reorder_group(R_G1, R_W1, false),
            reorder_group(R_G2, R_W4, false),
        ]);
        assert_eq!(
            set_workspace_group_anchor_snapshot(&mut tabs, uuid(R_G1), uuid(R_W2)),
            Ok(true)
        );
        assert_eq!(ws_id_order(&tabs), [R_W2, R_W1, R_W3, R_W4]);
        assert_eq!(tabs.selected_workspace_index, Some(0));
        assert_eq!(
            rename_workspace_group_snapshot(&mut tabs, uuid(R_G1), "  Renamed  "),
            Ok(true)
        );
        assert_eq!(
            set_workspace_group_color_snapshot(&mut tabs, uuid(R_G1), Some("#123456".into())),
            Ok(true)
        );
        assert_eq!(
            set_workspace_group_icon_snapshot(&mut tabs, uuid(R_G1), Some("folder.fill".into())),
            Ok(true)
        );

        assert_eq!(
            move_workspace_group_snapshot(&mut tabs, uuid(R_G2), 0),
            Ok(true)
        );
        // The ungrouped W3 retains the only ungrouped top-level slot while G2
        // and G1 exchange the two group slots.
        assert_eq!(ws_id_order(&tabs), [R_W4, R_W3, R_W2, R_W1]);
        assert_eq!(tabs.selected_workspace_index, Some(2));
        assert_eq!(
            tabs.workspace_groups
                .as_ref()
                .unwrap()
                .iter()
                .map(|group| group.id.as_str())
                .collect::<Vec<_>>(),
            [R_G2, R_G1]
        );
    }

    #[test]
    fn reorder_workspaces_many_plans_dry_run_and_applies_atomically() {
        let ordered = [
            Uuid::parse_str(R_W3).unwrap(),
            Uuid::parse_str(R_W2).unwrap(),
        ];
        let mut tabs = reorder_tabs(
            &[(R_W1, None, true), (R_W2, None, false), (R_W3, None, false)],
            Some(1),
        );
        let before = tabs.clone();

        let dry_plan = reorder_workspaces_many(&mut tabs, &ordered, true).unwrap();
        assert_eq!(
            dry_plan,
            vec![
                WorkspaceReorderPlanItem::new(ordered[0], 2, 1),
                WorkspaceReorderPlanItem::new(ordered[1], 1, 2),
            ]
        );
        assert_eq!(tabs, before);

        let applied_plan = reorder_workspaces_many(&mut tabs, &ordered, false).unwrap();
        assert_eq!(applied_plan, dry_plan);
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2]);
        assert_eq!(tabs.selected_workspace_index, Some(2));

        let before_duplicate = tabs.clone();
        assert_eq!(
            reorder_workspaces_many(&mut tabs, &[ordered[0], ordered[0]], false),
            Err(WorkspaceBatchReorderError::DuplicateWorkspace(ordered[0]))
        );
        assert_eq!(tabs, before_duplicate);
    }

    #[test]
    fn reorder_workspaces_many_restores_group_contiguity_and_anchor_order() {
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G1), false),
                (R_W2, Some(R_G1), false),
                (R_W3, None, false),
            ],
            Some(0),
        );
        tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
        let ordered = [
            Uuid::parse_str(R_W3).unwrap(),
            Uuid::parse_str(R_W2).unwrap(),
        ];

        reorder_workspaces_many(&mut tabs, &ordered, false).unwrap();
        assert_eq!(ws_id_order(&tabs), [R_W3, R_W1, R_W2]);
        assert_eq!(tabs.selected_workspace_index, Some(1));
    }

    fn reorder_group(
        id: &str,
        anchor: &str,
        pinned: bool,
    ) -> crate::session::SessionWorkspaceGroupSnapshot {
        crate::session::SessionWorkspaceGroupSnapshot {
            id: id.to_string(),
            name: "G".to_string(),
            anchor_workspace_id: Some(anchor.to_string()),
            // Some(true)/None convention (byte-stability, matching workspaces).
            is_pinned: pinned.then_some(true),
            ..Default::default()
        }
    }

    #[test]
    fn reorder_unpinned_mover_clamps_below_pinned_prefix() {
        // Cross-tier attempt clamps to pinnedCount, never crosses.
        let mut tabs = reorder_tabs(
            &[
                (R_W1, None, true),
                (R_W2, None, true),
                (R_W3, None, false),
                (R_W4, None, false),
            ],
            Some(0),
        );
        assert!(reorder_workspaces(&mut tabs, 3, 0));
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W2, R_W4, R_W3]);
    }

    #[test]
    fn reorder_pinned_mover_clamps_into_pinned_tier() {
        // Pinned mover dragged past the boundary clamps to pinnedCount-1.
        let mut tabs = reorder_tabs(
            &[
                (R_W1, None, true),
                (R_W2, None, true),
                (R_W3, None, false),
                (R_W4, None, false),
            ],
            Some(0),
        );
        assert!(reorder_workspaces(&mut tabs, 0, 3));
        assert_eq!(ws_id_order(&tabs), [R_W2, R_W1, R_W3, R_W4]);
    }

    #[test]
    fn reorder_boundary_counts_grouped_rows_by_group_pin() {
        // Leading grouped members of a PINNED group (members' own is_pinned is
        // None) count as pinned rows (isGlobalPinnedRow parity, regression
        // sibling of `boundary_counts_grouped_rows_by_group_pin`).
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G1), false),
                (R_W2, Some(R_G1), false),
                (R_W3, None, false),
                (R_W4, None, false),
            ],
            Some(0),
        );
        tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, true)]);
        assert!(reorder_workspaces(&mut tabs, 3, 0));
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W2, R_W4, R_W3]);
    }

    #[test]
    fn reorder_boundary_dangling_group_id_falls_back_to_own_flag() {
        // A leading row whose group_id resolves to NO group counts by its own
        // pin flag (Ordering.swift:201-207 nil-group arm), and its dangling
        // group_id string survives the reorder untouched.
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G_DANGLING), true),
                (R_W2, None, false),
                (R_W3, None, false),
            ],
            Some(0),
        );
        assert!(reorder_workspaces(&mut tabs, 2, 0));
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2]);
        assert_eq!(tabs.workspaces[0].group_id.as_deref(), Some(R_G_DANGLING));
    }

    #[test]
    fn reorder_grouped_member_confined_to_section() {
        // Unpinned member section clamp: [firstIndex+1 .. lastIndex].
        let specs: &[(&str, Option<&str>, bool)] = &[
            (R_W1, Some(R_G1), false),
            (R_W2, Some(R_G1), false),
            (R_W3, Some(R_G1), false),
            (R_W4, None, false),
        ];
        // Toward 0: clamps to firstIndex+1 == from → no-op, byte-identical
        // (must not normalize, Coordinator:111-118).
        let mut tabs = reorder_tabs(specs, Some(0));
        tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
        let before = serde_json::to_string(&tabs).unwrap();
        assert!(!reorder_workspaces(&mut tabs, 1, 0));
        assert_eq!(serde_json::to_string(&tabs).unwrap(), before);
        // Toward 999: clamps to lastIndex (2), stays inside the section.
        assert!(reorder_workspaces(&mut tabs, 1, 999));
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2, R_W4]);
    }

    #[test]
    fn reorder_pinned_member_clamps_into_pinned_subtier() {
        // Pinned member sub-tier: [firstIndex+1 .. firstIndex+pinnedMemberCount].
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G1), false), // anchor
                (R_W2, Some(R_G1), true),  // pinned member
                (R_W3, Some(R_G1), true),  // pinned member
                (R_W4, Some(R_G1), false), // unpinned member
            ],
            Some(0),
        );
        tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
        // Pinned member dragged to 999 clamps to firstIndex+pinnedMemberCount (2).
        assert!(reorder_workspaces(&mut tabs, 1, 999));
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2, R_W4]);
    }

    #[test]
    fn reorder_anchor_moves_whole_group_and_syncs_group_order() {
        // Router: an anchor mover takes the TOP-LEVEL path (`to_index` is a
        // top-level row index) and relocates ALL members contiguously,
        // anchor-first, with relative member order preserved; the groups array
        // syncs to the new anchor order.
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G1), false), // anchor of g1
                (R_W2, Some(R_G1), false),
                (R_W3, Some(R_G2), false), // anchor of g2
                (R_W4, Some(R_G2), false),
            ],
            Some(0),
        );
        tabs.workspace_groups = Some(vec![
            reorder_group(R_G1, R_W1, false),
            reorder_group(R_G2, R_W3, false),
        ]);
        // Mover = tabs index 2 (anchor R_W3); target = top-level index 0.
        assert!(reorder_workspaces(&mut tabs, 2, 0));
        assert_eq!(ws_id_order(&tabs), [R_W3, R_W4, R_W1, R_W2]);
        let groups = tabs.workspace_groups.as_ref().unwrap();
        assert_eq!(
            groups.iter().map(|g| g.id.as_str()).collect::<Vec<_>>(),
            [R_G2, R_G1]
        );
    }

    #[test]
    fn reorder_grouped_child_with_top_level_rows_promotes_it_out_of_the_group() {
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G1), false), // anchor
                (R_W2, Some(R_G1), false), // grouped child
                (R_W3, None, false),       // ungrouped
            ],
            Some(1),
        );
        tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);

        // Top-level drag lane: the grouped child is promoted to top-level row
        // space, then moved after the ungrouped row.
        assert!(reorder_workspaces_with_mode(&mut tabs, 1, 2, true));
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W3, R_W2]);
        assert_eq!(tabs.workspaces[2].group_id, None);
        assert_eq!(tabs.selected_workspace_index, Some(2));
        // The surviving group keeps its anchor and order.
        let groups = tabs.workspace_groups.as_ref().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].anchor_workspace_id.as_deref(), Some(R_W1));
    }

    #[test]
    fn reorder_selection_follows_mover_and_displaced_rows() {
        // Selection on the mover follows it to its landing index.
        let mut tabs = reorder_tabs(
            &[
                (R_W1, None, false),
                (R_W2, None, false),
                (R_W3, None, false),
            ],
            Some(2),
        );
        assert!(reorder_workspaces(&mut tabs, 2, 0));
        assert_eq!(ws_id_order(&tabs), [R_W3, R_W1, R_W2]);
        assert_eq!(tabs.selected_workspace_index, Some(0));

        // Selection on a displaced neighbor keeps pointing at the same
        // workspace after it shifts.
        let mut tabs = reorder_tabs(
            &[
                (R_W1, None, false),
                (R_W2, None, false),
                (R_W3, None, false),
            ],
            Some(0),
        );
        assert!(reorder_workspaces(&mut tabs, 2, 0));
        assert_eq!(tabs.selected_workspace_index, Some(1));

        // None / out-of-range selection stays untouched.
        let mut tabs = reorder_tabs(&[(R_W1, None, false), (R_W2, None, false)], None);
        assert!(reorder_workspaces(&mut tabs, 1, 0));
        assert_eq!(tabs.selected_workspace_index, None);
        let mut tabs = reorder_tabs(&[(R_W1, None, false), (R_W2, None, false)], Some(99));
        assert!(reorder_workspaces(&mut tabs, 1, 0));
        assert_eq!(tabs.selected_workspace_index, Some(99));
    }

    #[test]
    fn reorder_no_op_cases_return_false_and_snapshot_is_byte_identical() {
        let specs: &[(&str, Option<&str>, bool)] =
            &[(R_W1, None, true), (R_W2, None, false), (R_W3, None, false)];
        let mut tabs = reorder_tabs(specs, Some(1));
        let before = serde_json::to_string(&tabs).unwrap();
        // Same index (clamps to itself).
        assert!(!reorder_workspaces(&mut tabs, 2, 2));
        // Out-of-range / negative mover index.
        assert!(!reorder_workspaces(&mut tabs, 3, 0));
        assert!(!reorder_workspaces(&mut tabs, -1, 0));
        // Unpinned mover at the boundary asked past it clamps back to `from`
        // (pinnedCount = 1, mover already at index 1).
        assert!(!reorder_workspaces(&mut tabs, 1, 0));
        assert_eq!(serde_json::to_string(&tabs).unwrap(), before);

        // Single workspace.
        let mut solo = reorder_tabs(&[(R_W1, None, false)], Some(0));
        let before = serde_json::to_string(&solo).unwrap();
        assert!(!reorder_workspaces(&mut solo, 0, 0));
        assert_eq!(serde_json::to_string(&solo).unwrap(), before);
    }

    #[test]
    fn reorder_reverted_by_normalization_returns_false() {
        // An ungrouped row nudged into the middle of a group's section snaps
        // back out via normalization — the changed-gate reports false and the
        // snapshot stays byte-identical.
        let mut tabs = reorder_tabs(
            &[
                (R_W1, Some(R_G1), false), // anchor
                (R_W2, Some(R_G1), false), // member
                (R_W3, None, false),       // ungrouped
            ],
            Some(0),
        );
        tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W1, false)]);
        let before = serde_json::to_string(&tabs).unwrap();
        assert!(!reorder_workspaces(&mut tabs, 2, 1));
        assert_eq!(serde_json::to_string(&tabs).unwrap(), before);
    }

    #[test]
    fn reorder_id_less_rows_move_positionally() {
        // workspace_id None rows get MINTED mirror ids (positional identity)
        // and still reorder; their serialized objects stay untouched (no id is
        // written back).
        let mut tabs = SessionTabManagerSnapshot {
            selected_workspace_index: Some(0),
            workspaces: (0..3)
                .map(|i| fresh_terminal_workspace(&format!("surface-{i}")))
                .collect(),
            workspace_groups: None,
        };
        let before: Vec<String> = tabs
            .workspaces
            .iter()
            .map(|w| serde_json::to_string(w).unwrap())
            .collect();
        assert!(reorder_workspaces(&mut tabs, 2, 0));
        assert_eq!(order_of(&tabs), ["surface-2", "surface-0", "surface-1"]);
        let after: Vec<String> = tabs
            .workspaces
            .iter()
            .map(|w| serde_json::to_string(w).unwrap())
            .collect();
        assert_eq!(
            after,
            [before[2].clone(), before[0].clone(), before[1].clone()]
        );
        assert!(tabs.workspaces.iter().all(|w| w.workspace_id.is_none()));
    }

    #[test]
    fn reorder_preserves_each_object_byte_for_byte() {
        // Only ARRAY ORDER may change: every workspace/group object's own
        // serialization must equal its pre-move serialization (is_pinned
        // Some(true)/None convention and dangling group_id strings untouched).
        let mut tabs = reorder_tabs(
            &[
                (R_W1, None, true),
                (R_W2, Some(R_G1), false), // anchor
                (R_W3, Some(R_G1), false), // member
                (R_W4, Some(R_G_DANGLING), false),
            ],
            Some(0),
        );
        tabs.workspace_groups = Some(vec![reorder_group(R_G1, R_W2, false)]);
        let ws_before: std::collections::HashMap<String, String> = tabs
            .workspaces
            .iter()
            .map(|w| {
                (
                    w.workspace_id.clone().unwrap(),
                    serde_json::to_string(w).unwrap(),
                )
            })
            .collect();
        let group_before =
            serde_json::to_string(&tabs.workspace_groups.as_ref().unwrap()[0]).unwrap();
        // Move the dangling-group row (index 3) up; clamps to the unpinned
        // boundary (1).
        assert!(reorder_workspaces(&mut tabs, 3, 1));
        assert_eq!(ws_id_order(&tabs), [R_W1, R_W4, R_W2, R_W3]);
        for w in &tabs.workspaces {
            assert_eq!(
                serde_json::to_string(w).unwrap(),
                ws_before[w.workspace_id.as_deref().unwrap()]
            );
        }
        assert_eq!(
            serde_json::to_string(&tabs.workspace_groups.as_ref().unwrap()[0]).unwrap(),
            group_before
        );
    }

    // --- Workspace-group collapse ---

    fn group(id: &str, is_collapsed: bool) -> crate::session::SessionWorkspaceGroupSnapshot {
        crate::session::SessionWorkspaceGroupSnapshot {
            id: id.to_string(),
            name: id.to_uppercase(),
            is_collapsed,
            ..Default::default()
        }
    }

    fn tabs_with_group(is_collapsed: bool) -> SessionTabManagerSnapshot {
        let mut tabs = one_workspace_tabs("surface-1");
        tabs.workspace_groups = Some(vec![group("g", is_collapsed)]);
        tabs
    }

    #[test]
    fn set_group_collapsed_collapses_a_group() {
        let mut tabs = tabs_with_group(false);
        assert!(set_group_collapsed(&mut tabs, "g", true));
        assert!(tabs.workspace_groups.as_ref().unwrap()[0].is_collapsed);
        assert_eq!(tabs.selected_workspace_index, Some(0));
    }

    #[test]
    fn set_group_collapsed_expands_a_group() {
        let mut tabs = tabs_with_group(true);
        assert!(set_group_collapsed(&mut tabs, "g", false));
        assert!(!tabs.workspace_groups.as_ref().unwrap()[0].is_collapsed);
    }

    #[test]
    fn set_group_collapsed_same_value_is_a_no_op() {
        let mut tabs = tabs_with_group(false);
        let before = tabs.clone();
        assert!(!set_group_collapsed(&mut tabs, "g", false));
        assert_eq!(tabs, before);
    }

    #[test]
    fn set_group_collapsed_unknown_group_is_a_no_op() {
        let mut tabs = tabs_with_group(false);
        let before = tabs.clone();
        assert!(!set_group_collapsed(&mut tabs, "nope", true));
        assert_eq!(tabs, before);
    }

    #[test]
    fn set_group_collapsed_with_no_groups_is_a_no_op() {
        // `None` groups must stay `None` — never materialize `Some(vec![])`.
        let mut tabs = one_workspace_tabs("surface-1");
        assert!(!set_group_collapsed(&mut tabs, "g", true));
        assert_eq!(tabs.workspace_groups, None);
    }

    #[test]
    fn set_group_collapsed_targets_only_the_named_group() {
        let mut tabs = one_workspace_tabs("surface-1");
        tabs.workspace_groups = Some(vec![group("g1", false), group("g2", false)]);
        assert!(set_group_collapsed(&mut tabs, "g2", true));
        let groups = tabs.workspace_groups.as_ref().unwrap();
        assert!(!groups[0].is_collapsed);
        assert!(groups[1].is_collapsed);
    }

    // Pins the canonical pure-data contract (WorkspaceGroupCoordinator.swift:405-407)
    // against drift toward the UI toggle's anchor-select semantics: collapsing a
    // group whose selected member is a NON-anchor must not move selection.
    #[test]
    fn set_group_collapsed_never_moves_selection() {
        let mut tabs = tabs_with(2, 0, 1);
        tabs.workspaces[0].workspace_id = Some("ws-anchor".to_string());
        tabs.workspaces[0].group_id = Some("g".to_string());
        tabs.workspaces[1].workspace_id = Some("ws-member".to_string());
        tabs.workspaces[1].group_id = Some("g".to_string());
        tabs.workspace_groups = Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
            anchor_workspace_id: Some("ws-anchor".to_string()),
            ..group("g", false)
        }]);
        assert!(set_group_collapsed(&mut tabs, "g", true));
        // Selection stays on the (now hidden-in-UI) non-anchor member.
        assert_eq!(tabs.selected_workspace_index, Some(1));
    }
}
