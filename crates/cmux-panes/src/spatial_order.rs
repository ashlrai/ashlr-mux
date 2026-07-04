//! On-screen spatial ordering over the split tree.
//!
//! A verbatim port of `CmuxPanes`'
//! `ExternalTreeNode+SpatialOrder.swift:4-45` (formerly
//! `SidebarBranchOrdering.orderedPaneIds(tree:)` /
//! `.orderedPanelIds(tree:paneTabs:fallbackPanelIds:)`). Both operations are
//! pure depth-first walks: panes are visited first-child-before-second, which
//! is on-screen order for both horizontal and vertical Bonsplit splits.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::tree::ExternalTreeNode;

impl ExternalTreeNode {
    /// Pane ids in on-screen spatial order: depth-first over the split tree,
    /// first/top child before second/bottom child.
    ///
    /// Port of `ExternalTreeNode.orderedPaneIds`
    /// (`ExternalTreeNode+SpatialOrder.swift:8-16`). Bonsplit's split order
    /// matches visual order for both horizontal and vertical splits, so the walk
    /// is a plain `first ++ second` concatenation.
    pub fn ordered_pane_ids(&self) -> Vec<String> {
        match self {
            ExternalTreeNode::Pane(pane) => vec![pane.id.clone()],
            ExternalTreeNode::Split(split) => {
                let mut ids = split.first.ordered_pane_ids();
                ids.extend(split.second.ordered_pane_ids());
                ids
            }
        }
    }

    /// Panel ids in on-screen spatial order: panes in [`Self::ordered_pane_ids`]
    /// order, tabs within each pane in tab order, then any panels missing from
    /// the tree in the caller-provided stable `fallback_panel_ids` order.
    /// Every id appears at most once (first occurrence wins).
    ///
    /// Port of
    /// `ExternalTreeNode.orderedPanelIds(paneTabs:fallbackPanelIds:)`
    /// (`ExternalTreeNode+SpatialOrder.swift:22-44`). `seen.insert(_).inserted`
    /// maps to [`HashSet::insert`] returning `true` on first insertion; a pane
    /// with no `pane_tabs` entry contributes nothing (Swift's `?? []`).
    pub fn ordered_panel_ids(
        &self,
        pane_tabs: &HashMap<String, Vec<Uuid>>,
        fallback_panel_ids: &[Uuid],
    ) -> Vec<Uuid> {
        let mut ordered: Vec<Uuid> = Vec::new();
        let mut seen: HashSet<Uuid> = HashSet::new();

        for pane_id in self.ordered_pane_ids() {
            if let Some(panel_ids) = pane_tabs.get(&pane_id) {
                for &panel_id in panel_ids {
                    if seen.insert(panel_id) {
                        ordered.push(panel_id);
                    }
                }
            }
        }

        for &panel_id in fallback_panel_ids {
            if seen.insert(panel_id) {
                ordered.push(panel_id);
            }
        }

        ordered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::{ExternalPaneNode, ExternalSplitNode, PixelRect};

    /// Mirrors the Swift test's private `pane(_:)` helper
    /// (`SpatialOrderTests.swift:7-9`): a leaf with a unit frame and no tabs.
    fn pane(id: &str) -> ExternalTreeNode {
        ExternalTreeNode::Pane(ExternalPaneNode::new(
            id,
            PixelRect::new(0.0, 0.0, 100.0, 100.0),
            Vec::new(),
            None,
        ))
    }

    fn split(
        id: &str,
        orientation: &str,
        divider_position: f64,
        first: ExternalTreeNode,
        second: ExternalTreeNode,
    ) -> ExternalTreeNode {
        ExternalTreeNode::Split(ExternalSplitNode::new(
            id,
            orientation,
            divider_position,
            first,
            second,
        ))
    }

    /// Depth-first, first/top before second/bottom: the on-screen order.
    /// Ported from `SpatialOrderTests.orderedPaneIdsWalksDepthFirst`
    /// (`SpatialOrderTests.swift:12-22`).
    #[test]
    fn ordered_pane_ids_walks_depth_first() {
        let tree = split(
            "s1",
            "horizontal",
            0.5,
            pane("a"),
            split("s2", "vertical", 0.5, pane("b"), pane("c")),
        );
        assert_eq!(tree.ordered_pane_ids(), vec!["a", "b", "c"]);
    }

    /// Pane order then tab order, deduplicated, then stable fallback order.
    /// Ported from `SpatialOrderTests.orderedPanelIdsUsesPaneTabsThenFallback`
    /// (`SpatialOrderTests.swift:25-36`).
    #[test]
    fn ordered_panel_ids_uses_pane_tabs_then_fallback() {
        let (p1, p2, p3, orphan) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        let tree = split("s1", "horizontal", 0.5, pane("a"), pane("b"));

        let pane_tabs = HashMap::from([
            ("a".to_string(), vec![p1, p2]),
            ("b".to_string(), vec![p3, p1]),
        ]);
        let result = tree.ordered_panel_ids(&pane_tabs, &[orphan, p2]);

        // "a" → p1,p2 ; "b" → p3,(p1 dup dropped) ; fallback → orphan,(p2 dup dropped).
        assert_eq!(result, vec![p1, p2, p3, orphan]);
    }

    // --- Edge cases the port notes flag (parity-risk inputs) -----------------

    /// A lone pane is its own order (the `.pane` base case).
    #[test]
    fn ordered_pane_ids_of_single_pane() {
        assert_eq!(pane("solo").ordered_pane_ids(), vec!["solo"]);
    }

    /// A pane absent from `pane_tabs` contributes nothing (Swift `paneTabs[id]
    /// ?? []`); its panels come only from the fallback list, in fallback order.
    #[test]
    fn ordered_panel_ids_skips_panes_missing_from_map() {
        let (p1, p2) = (Uuid::new_v4(), Uuid::new_v4());
        let tree = split("s1", "horizontal", 0.5, pane("a"), pane("b"));
        // Only "b" has an entry; "a" falls through to `?? []`.
        let pane_tabs = HashMap::from([("b".to_string(), vec![p1])]);
        let result = tree.ordered_panel_ids(&pane_tabs, &[p2]);
        assert_eq!(result, vec![p1, p2]);
    }

    /// A single pane with no tabs and an empty fallback yields nothing.
    #[test]
    fn ordered_panel_ids_empty_when_no_tabs_and_no_fallback() {
        let result = pane("a").ordered_panel_ids(&HashMap::new(), &[]);
        assert!(result.is_empty());
    }

    /// The fallback list itself is deduplicated against what the tree already
    /// contributed and against its own earlier entries (single `seen` set).
    #[test]
    fn ordered_panel_ids_dedups_fallback_against_tree_and_self() {
        let (p1, p2) = (Uuid::new_v4(), Uuid::new_v4());
        let tree = pane("a");
        let pane_tabs = HashMap::from([("a".to_string(), vec![p1])]);
        // Fallback repeats p1 (already seen) and p2 twice.
        let result = tree.ordered_panel_ids(&pane_tabs, &[p1, p2, p2]);
        assert_eq!(result, vec![p1, p2]);
    }

    /// Deeper nesting stays strictly depth-first, first-before-second at every
    /// level regardless of orientation.
    #[test]
    fn ordered_pane_ids_nested_both_orientations() {
        let tree = split(
            "root",
            "vertical",
            0.5,
            split("l", "horizontal", 0.5, pane("a"), pane("b")),
            split("r", "horizontal", 0.5, pane("c"), pane("d")),
        );
        assert_eq!(tree.ordered_pane_ids(), vec!["a", "b", "c", "d"]);
    }
}
