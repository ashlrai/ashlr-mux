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

use crate::session::{
    SessionPaneLayoutSnapshot, SessionSplitLayoutSnapshot, SessionSplitOrientation,
    SessionTabManagerSnapshot, SessionWorkspaceLayoutSnapshot, SessionWorkspaceSnapshot,
};
use cmux_workspaces::{insertion_index, NewWorkspacePlacement};

use serde::{Deserialize, Serialize};

type Layout = SessionWorkspaceLayoutSnapshot;

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
        selected_panel_id: Some(id.clone()),
        panel_ids: vec![id],
        surface_kind: None,
    })
}

fn empty_pane() -> Layout {
    Layout::Pane(SessionPaneLayoutSnapshot {
        panel_ids: Vec::new(),
        selected_panel_id: None,
        surface_kind: None,
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
        Layout::Split(s) => contains_panel(&s.first, panel_id) || contains_panel(&s.second, panel_id),
    }
}

/// Set the OSC/process title of the workspace whose layout owns `panel_id`.
///
/// This is the workspace-title feed for a terminal pane's top label
/// (`cmux-terminal::top_label` `panelTitles`, seeded `"Terminal"` at panel
/// creation): the incoming OSC title is trimmed and an empty title is dropped
/// (a blank title never clobbers a real one), last-write-wins. `process_title`
/// is what `workspaceDisplayName` shows once a workspace has no `custom_title`,
/// so this is what replaces the "Terminal" fallback with the running program /
/// directory the shell reports.
///
/// Returns `true` iff a workspace's `process_title` actually changed.
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
        let owns = workspace
            .layout
            .as_ref()
            .is_some_and(|layout| contains_panel(layout, panel_id));
        if owns {
            if workspace.process_title == trimmed {
                return false;
            }
            workspace.process_title = trimmed.to_string();
            return true;
        }
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
            orientation: orientation.clone(),
            divider_position: 0.5,
            first: Box::new(first),
            second: Box::new(second),
        });
        return true;
    }
    match node {
        Layout::Split(s) => {
            split_pane_impl(&mut s.first, target_panel_id, orientation, new_panel_id, insert_first)
                || split_pane_impl(&mut s.second, target_panel_id, orientation, new_panel_id, insert_first)
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
/// - GROUP CONTIGUITY GAP: canonical inserts by flat index THEN runs
///   `normalizeWorkspaceGroupContiguity` (`TabManager.swift:1136-1138`). The
///   Rust port of that pass consumes `WorkspaceRow`/`WorkspaceGroup`, not the
///   session snapshot types, so it is out of reach (and out of Lane scope) here.
///   A9 therefore places by flat index only; the new workspace inherits no
///   `group_id` and a contiguity fix-up is a documented future bridge.
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
    tabs.workspaces.insert(at, fresh_terminal_workspace(panel_id));
    // Canonical selects the newly created workspace (`TabManager.swift:1156`).
    tabs.selected_workspace_index = Some(at as i64);
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
/// to keep pointing at the same surviving workspace (else the new last one).
/// Returns whether a close happened.
pub fn close_workspace(tabs: &mut SessionTabManagerSnapshot, index: i64) -> bool {
    let count = tabs.workspaces.len();
    if count <= 1 || index < 0 || (index as usize) >= count {
        return false;
    }
    let removed = index as usize;
    tabs.workspaces.remove(removed);

    // Selection here is index-based (canonical is id-based). To keep the same
    // surviving workspace focused: removing a tab before the selected one shifts
    // it left; removing at/after clamps to the (new) last tab — canonical's
    // `min(index, count - 1)`.
    let selected = tabs.selected_workspace_index.unwrap_or(0).max(0) as usize;
    let next = if selected > removed {
        selected - 1
    } else {
        selected.min(tabs.workspaces.len() - 1)
    };
    tabs.selected_workspace_index = Some(next as i64);
    true
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
/// - GROUPED workspace (`group_id` present): flag-only — pinning never ejects
///   a tab from its group and never moves it globally. Canonical additionally
///   runs `normalizeWorkspaceGroupContiguity` (Coordinator:531-533); that
///   contiguity pass consumes live model types, not the snapshot, so it is a
///   documented gap here — same precedent as `new_workspace_with_placement`.
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
        // Flag-only for grouped tabs (documented contiguity gap, see above).
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

#[cfg(test)]
mod tests {
    use super::*;

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
            orientation: SessionSplitOrientation::Horizontal,
            divider_position: 0.5,
            first: Box::new(split(SessionSplitOrientation::Vertical, 0.5, pane("a"), pane("b"))),
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
            split(SessionSplitOrientation::Horizontal, 0.15, pane("b"), pane("c")),
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
        assert!(split_pane(&mut tree, "a", SessionSplitOrientation::Vertical, "b", true));
        if let Layout::Split(s) = &tree {
            assert_eq!(panel_ids(&s.first), vec!["b"]);
            assert_eq!(panel_ids(&s.second), vec!["a"]);
        } else {
            panic!("expected a split");
        }
    }

    #[test]
    fn split_pane_targets_a_nested_pane() {
        let mut tree = split(SessionSplitOrientation::Horizontal, 0.5, pane("a"), pane("b"));
        assert!(split_pane(&mut tree, "b", SessionSplitOrientation::Vertical, "c", false));
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
        assert!(!split_pane(&mut tree, "zzz", SessionSplitOrientation::Horizontal, "b", false));
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
        let expected = split(SessionSplitOrientation::Horizontal, 0.5, pane("a"), pane("c"));
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
            panel_ids: vec!["a".into(), "b".into()],
            selected_panel_id: Some("a".into()),
            surface_kind: None,
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
        assert!(split_pane(&mut tree, "a", SessionSplitOrientation::Horizontal, "b", false));
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
    fn split_child_serializes_lowercase_matching_the_web_path() {
        assert_eq!(serde_json::to_string(&SplitChild::First).unwrap(), "\"first\"");
        assert_eq!(serde_json::to_string(&SplitChild::Second).unwrap(), "\"second\"");
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
        assert_eq!(panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()), ["new"]);
        assert_eq!(tabs.selected_workspace_index, Some(2));
    }

    // Case C: End placement appends.
    #[test]
    fn new_workspace_end_appends() {
        let mut tabs = tabs_with(4, 0, 1);
        new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::End);
        assert_eq!(tabs.workspaces.len(), 5);
        assert_eq!(panel_ids(tabs.workspaces[4].layout.as_ref().unwrap()), ["new"]);
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
        assert_eq!(panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()), ["new"]);
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
        assert_eq!(panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()), ["new"]);
        assert_eq!(tabs.selected_workspace_index, Some(2));
    }

    // Case F: into-selected-group — flat placement index lands the new ws adjacent
    // to the selected group member. Documents the contiguity gap: the new ws
    // inherits no group_id and no snapshot-level contiguity pass runs here.
    #[test]
    fn new_workspace_into_selected_group_places_by_flat_index_only() {
        let mut tabs = tabs_with(4, 0, 1);
        // Mark the selected ws (index 1) and its neighbour (index 2) as a group.
        tabs.workspaces[1].group_id = Some("g".to_string());
        tabs.workspaces[2].group_id = Some("g".to_string());
        tabs.workspace_groups = Some(vec![crate::session::SessionWorkspaceGroupSnapshot {
            id: "g".to_string(),
            name: "G".to_string(),
            ..Default::default()
        }]);
        new_workspace_with_placement(&mut tabs, "new", NewWorkspacePlacement::AfterCurrent);
        // Flat index parity: lands at index 2, adjacent to the selected member.
        assert_eq!(panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()), ["new"]);
        assert_eq!(tabs.selected_workspace_index, Some(2));
        // Documented gap: the new ws stays ungrouped (no contiguity fix-up here).
        assert_eq!(tabs.workspaces[2].group_id, None);
    }

    // The default two-arg wrapper resolves to AfterCurrent.
    #[test]
    fn new_workspace_defaults_to_after_current() {
        let mut tabs = tabs_with(4, 0, 1);
        new_workspace(&mut tabs, "new");
        assert_eq!(panel_ids(tabs.workspaces[2].layout.as_ref().unwrap()), ["new"]);
        assert_eq!(tabs.selected_workspace_index, Some(2));
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
        assert_eq!(tabs.workspaces[0].custom_title_source.as_deref(), Some("user"));
    }

    #[test]
    fn rename_workspace_out_of_range_and_negative_index_are_no_ops() {
        let mut tabs = tabs_with(2, 0, 0);
        let before = tabs.clone();
        assert!(!rename_workspace(&mut tabs, 2, "nope"));
        assert!(!rename_workspace(&mut tabs, -1, "nope"));
        assert_eq!(tabs, before);
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
    fn grouped_workspace_pin_flips_flag_without_reorder() {
        let mut tabs = tabs_with(3, 0, 0);
        tabs.workspaces[1].group_id = Some("g".to_string());
        tabs.workspace_groups = Some(vec![group("g", false)]);
        assert!(set_workspace_pinned(&mut tabs, 1, true));
        // Flag-only change (documented contiguity gap): no global move, group
        // membership preserved.
        assert_eq!(order_of(&tabs), ["surface-0", "surface-1", "surface-2"]);
        assert_eq!(tabs.workspaces[1].is_pinned, Some(true));
        assert_eq!(tabs.workspaces[1].group_id.as_deref(), Some("g"));
        assert_eq!(tabs.selected_workspace_index, Some(0));
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
