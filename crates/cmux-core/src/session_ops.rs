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
///
/// Mints a fresh `workspace_id`, mirroring the canonical Swift `Workspace`
/// initializer (`id = UUID()` at creation). Restored snapshots keep their
/// persisted ids; only genuinely new workspaces mint. Without an id the
/// sidebar projection (`cmux_workspaces::render_items`) skips the row.
pub fn fresh_terminal_workspace(panel_id: &str) -> SessionWorkspaceSnapshot {
    SessionWorkspaceSnapshot {
        workspace_id: Some(uuid::Uuid::new_v4().to_string()),
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

    // Canonical `Workspace.init` mints `id = UUID()`; without an id the sidebar
    // projection (`cmux_workspaces::render_items`) would skip the row entirely.
    #[test]
    fn fresh_terminal_workspace_mints_a_unique_workspace_id() {
        let a = fresh_terminal_workspace("surface-1");
        let b = fresh_terminal_workspace("surface-2");
        let id_a = a.workspace_id.as_deref().expect("fresh workspace must carry an id");
        let id_b = b.workspace_id.as_deref().expect("fresh workspace must carry an id");
        assert!(uuid::Uuid::parse_str(id_a).is_ok(), "id must be a UUID: {id_a}");
        assert_ne!(id_a, id_b, "each fresh workspace mints its own id");
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
}
