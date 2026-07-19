//! Pure canvas layout and persisted geometry mutations.

use super::clamp_divider;
use crate::session::{
    SessionCanvasPaneSnapshot, SessionPaneLayoutSnapshot, SessionSplitOrientation,
    SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot,
};

type Layout = SessionWorkspaceLayoutSnapshot;

const CANVAS_SEED_WIDTH: i64 = 1200;
const CANVAS_SEED_HEIGHT: i64 = 800;
const CANVAS_DEFAULT_GAP: i64 = 16;

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
