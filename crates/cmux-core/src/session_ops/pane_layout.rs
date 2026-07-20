use super::*;

/// Divider ratios are clamped to [0.1, 0.9] — byte-for-byte the macOS bonsplit
/// bound and the web `splitLayout.ts` clamp. Keeps a pane from collapsing to
/// zero width/height.
pub const MIN_DIVIDER: f64 = 0.1;
pub const MAX_DIVIDER: f64 = 0.9;
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
fn is_default_powershell_bootstrap_title(title: &str) -> bool {
    title
        .replace('/', "\\")
        .to_ascii_lowercase()
        .ends_with(r"\windows\system32\windowspowershell\v1.0\powershell.exe")
}

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
        let runtime_title = workspace.surfaces.as_ref().and_then(|surfaces| {
            surfaces
                .iter()
                .find(|surface| surface.surface_id == panel_id)
                .and_then(|surface| surface.metadata.runtime_title.as_deref())
        });
        if runtime_title.is_none() && is_default_powershell_bootstrap_title(trimmed) {
            return false;
        }
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
    pane.selected_panel_id = Some(panel_id.to_string());
    true
}

/// Swap the selected surfaces of two panes while preserving both pane IDs.
/// This directly models canonical's placeholder-assisted two-move sequence:
/// selected tabs leave their panes, each enters the other pane at the end of
/// its pin tier, and both panes select their arriving surface.
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
    if let Some(surfaces) = workspace.surfaces.as_mut() {
        for surface in surfaces {
            if surface.surface_id == source_surface_id {
                surface.pane_id = target_pane_id.to_string();
            } else if surface.surface_id == target_surface_id {
                surface.pane_id = source_pane_id.to_string();
            }
        }
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

pub(super) fn panel_count(layout: &Layout) -> usize {
    match layout {
        Layout::Pane(pane) => pane.panel_ids.len(),
        Layout::Split(split) => panel_count(&split.first) + panel_count(&split.second),
    }
}

pub(super) fn pane_for_panel(layout: &Layout, panel_id: &str) -> Option<SessionPaneLayoutSnapshot> {
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

fn take_surface_record(
    surfaces: &mut Option<Vec<crate::session::SessionSurfaceSnapshot>>,
    panel_id: &str,
) -> Option<crate::session::SessionSurfaceSnapshot> {
    let entries = surfaces.as_mut()?;
    let index = entries
        .iter()
        .position(|entry| entry.surface_id == panel_id)?;
    let entry = entries.remove(index);
    if entries.is_empty() {
        *surfaces = None;
    }
    Some(entry)
}

#[derive(Default)]
pub(super) struct DetachedPanelMetadata {
    pub(super) title: Option<SessionPanelTitleSnapshot>,
    pin: Option<SessionPanelPinSnapshot>,
    unread: Option<SessionPanelUnreadSnapshot>,
    restorable_agent: Option<SessionPanelRestorableAgentSnapshot>,
    terminal_startup: Option<SessionPanelTerminalStartupSnapshot>,
    listening_ports: Option<SessionPanelListeningPortsSnapshot>,
    tty: Option<SessionPanelTtySnapshot>,
    shell_activity: Option<SessionPanelShellActivitySnapshot>,
    surface: Option<crate::session::SessionSurfaceSnapshot>,
}

pub(super) fn detach_panel_metadata(
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
        surface: take_surface_record(&mut workspace.surfaces, panel_id),
    };
    if metadata.listening_ports.is_some() {
        recompute_workspace_listening_ports(workspace);
    }
    metadata
}

pub(super) fn attach_panel_metadata(
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
    if let Some(entry) = metadata.surface {
        workspace.surfaces.get_or_insert_with(Vec::new).push(entry);
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
