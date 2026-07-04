//! Port of the per-workspace surface-list derivation model from
//! `Packages/macOS/CmuxWorkspaces/Sources/CmuxWorkspaces/SurfaceList/WorkspaceSurfaceListModel.swift`
//! (all 8 derivations) over the read seam described by
//! `SurfaceList/WorkspaceSurfaceTreeReading.swift`.
//!
//! Turns the live bonsplit split tree + panel registry into the ordered
//! panel-id lists the rest of the app navigates by (`orderedPanelIds`,
//! `focusedPanelId`, `representativePanelIdForWorkspaceManualUnread()`,
//! `effectiveSelectedPanelId(inPane:)`, the `surfaceIdsToLeft/Right/CloseOthers`
//! pane queries), and owns the reorder-detection that bumps the workspace's
//! pane-layout version.
//!
//! DIVERGENCE (value snapshot, not a live protocol). Swift reads the live
//! `BonsplitController` / `PaneTreeModel` through the synchronous
//! `WorkspaceSurfaceTreeReading` protocol so every derivation observes
//! authoritative current state in one MainActor turn. The pure port models the
//! seam as an immutable value snapshot ([`SurfaceTree`]) mirroring the Swift
//! test's `FakeTree` value model exactly (`WorkspaceSurfaceListModelTests.swift:10-54`).
//! This matches the crate's `WorkspaceRow` value-snapshot idiom and keeps the
//! module a pure leaf. The I/O shell is intentionally dropped: the
//! `@MainActor @Observable` wrapper, the weak `attach(tree:)`/detached-nil dance
//! (an empty snapshot reproduces the detached defaults — see
//! [`SurfaceTree::detached_defaults`] in tests), the live
//! `BonsplitController` + `PaneTreeModel` registry ownership (folded into the
//! snapshot), and the real `firstSidebarOrderedPanelId` directory/branch
//! derivation (consumed here as a precomputed `Option`).
//!
//! DIVERGENCE (pane-layout version stored on the snapshot). Swift's
//! `registerGeometryChange` mutates the `PaneTreeModel`-owned
//! `lastOrderedPanelIds` and calls `bumpPaneLayoutVersion()` (a wrapping `&+=`)
//! through the seam. The port folds those two pieces of bookkeeping onto the
//! snapshot itself: [`SurfaceTree::last_ordered_panel_ids`] and a wrapping
//! [`SurfaceTree::pane_layout_version`] `u32`, mutated in place by
//! [`SurfaceTree::register_geometry_change`].
//!
//! PARITY: the orphan/fallback tail of `ordered_panel_ids` is sorted by the
//! Swift `UUID.uuidString` key (UPPERCASE, hyphenated) — NOT the Rust `Uuid`
//! native byte order nor the lowercase `to_string()` — so the ordering matches
//! Swift exactly (Swift `WorkspaceSurfaceListModel.swift:56`).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One pane in the split tree: an ordered list of surface ids (bonsplit tab
/// order) plus the selected tab index.
///
/// Mirrors the Swift test `FakeTree.Pane` value model
/// (`WorkspaceSurfaceListModelTests.swift:11-15`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pane {
    /// The pane's stable identity (bonsplit `PaneID.id`).
    pub id: Uuid,
    /// The pane's surface ids in tab order (bonsplit `TabID.uuid`).
    pub surface_ids: Vec<Uuid>,
    /// The selected tab index, or `None` when the pane has no selection.
    pub selected_index: Option<usize>,
}

impl Pane {
    /// Creates a pane snapshot.
    pub fn new(id: Uuid, surface_ids: Vec<Uuid>, selected_index: Option<usize>) -> Self {
        Self {
            id,
            surface_ids,
            selected_index,
        }
    }

    /// The pane's selected surface id, or `None` when the pane has no selection
    /// or the index is out of bounds. Mirrors Swift `FakeTree`'s
    /// `selectedIndex`/`surfaceIds.indices.contains` guard
    /// (`WorkspaceSurfaceListModelTests.swift:41-45`).
    fn selected_surface_id(&self) -> Option<Uuid> {
        let index = self.selected_index?;
        self.surface_ids.get(index).copied()
    }
}

/// A value snapshot of the workspace-side surface tree the derivations read
/// through. Folds bonsplit's split tree, pane selection/tab order, the
/// surface→panel map, the panel registry, the sidebar-first fallback, and the
/// reorder bookkeeping into one immutable-except-for-bookkeeping value.
///
/// Mirrors the Swift test `FakeTree` (`WorkspaceSurfaceListModelTests.swift:10-54`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceTree {
    /// The panes in the split tree (membership order across panes = tab order).
    pub panes: Vec<Pane>,
    /// Pane ids in on-screen spatial order (legacy
    /// `bonsplitController.treeSnapshot().orderedPaneIds`).
    pub pane_spatial_order: Vec<Uuid>,
    /// The focused pane id, or `None` when no pane is focused.
    pub focused_pane_id: Option<Uuid>,
    /// Resolves the owning panel id for a surface id.
    pub surface_to_panel: HashMap<Uuid, Uuid>,
    /// The panel-existence registry (legacy `panels[panelId] != nil`).
    pub registry: HashSet<Uuid>,
    /// The first sidebar-ordered panel id, used only as the last-resort
    /// representative fallback (precomputed; see module divergence note).
    pub first_sidebar_ordered_panel_id: Option<Uuid>,
    /// The ordered panel ids captured at the last geometry notification, used
    /// to gate reorder bumps (legacy `Workspace.lastOrderedPanelIds`).
    pub last_ordered_panel_ids: Vec<Uuid>,
    /// The monotonic (wrapping) pane-layout version (legacy
    /// `Workspace.paneLayoutVersion`).
    pub pane_layout_version: u32,
}

impl SurfaceTree {
    // MARK: - Seam-derived reads (mirror the WorkspaceSurfaceTreeReading getters)

    fn pane(&self, pane_id: Uuid) -> Option<&Pane> {
        self.panes.iter().find(|pane| pane.id == pane_id)
    }

    /// Every surface across all panes in tab order (legacy
    /// `surfaceIdsInTabOrderAcrossAllPanes`, `FakeTree` line 28-30).
    fn surface_ids_in_tab_order_across_all_panes(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.panes.iter().flat_map(|pane| pane.surface_ids.iter().copied())
    }

    /// The focused pane's selected surface id (legacy
    /// `focusedPaneSelectedSurfaceId`, `FakeTree` line 32-36).
    fn focused_pane_selected_surface_id(&self) -> Option<Uuid> {
        let focused = self.focused_pane_id?;
        self.pane(focused)?.selected_surface_id()
    }

    /// The selected surface id in the pane (legacy `selectedSurfaceId(inPaneId:)`,
    /// `FakeTree` line 41-45).
    fn selected_surface_id(&self, pane_id: Uuid) -> Option<Uuid> {
        self.pane(pane_id)?.selected_surface_id()
    }

    /// The pane's surface ids in tab order, or `[]` when the pane is gone
    /// (legacy `surfaceIdsInTabOrder(inPaneId:)`, `FakeTree` line 47-49).
    fn surface_ids_in_tab_order(&self, pane_id: Uuid) -> &[Uuid] {
        self.pane(pane_id).map_or(&[], |pane| pane.surface_ids.as_slice())
    }

    fn panel_id(&self, surface_id: Uuid) -> Option<Uuid> {
        self.surface_to_panel.get(&surface_id).copied()
    }

    fn panel_exists(&self, panel_id: Uuid) -> bool {
        self.registry.contains(&panel_id)
    }

    // MARK: - Derivations

    /// Panel ids in bonsplit's spatial order: tab order across all panes,
    /// deduplicated (first-wins) and filtered to panels that still exist, with
    /// any registry panels missing from bonsplit appended in stable
    /// `uuidString` order so the list never drops a panel (legacy
    /// `Workspace.orderedPanelIds`, Swift `WorkspaceSurfaceListModel.swift:45-59`).
    pub fn ordered_panel_ids(&self) -> Vec<Uuid> {
        let mut result: Vec<Uuid> = Vec::new();
        let mut seen: HashSet<Uuid> = HashSet::new();
        for surface_id in self.surface_ids_in_tab_order_across_all_panes() {
            let Some(panel_id) = self.panel_id(surface_id) else {
                continue;
            };
            if !self.panel_exists(panel_id) {
                continue;
            }
            if !seen.insert(panel_id) {
                continue;
            }
            result.push(panel_id);
        }

        let mut orphans: Vec<Uuid> = self
            .registry
            .iter()
            .copied()
            .filter(|panel_id| !seen.contains(panel_id))
            .collect();
        // Swift `$0.uuidString < $1.uuidString`: UPPERCASE hyphenated string
        // order (see module PARITY note).
        orphans.sort_by_key(uuid_string);
        result.extend(orphans);
        result
    }

    /// The focused pane's selected panel id, or `None` when no pane is focused
    /// or the selection resolves to no panel (legacy `Workspace.focusedPanelId`,
    /// Swift `WorkspaceSurfaceListModel.swift:64-67`).
    ///
    /// PARITY: deliberately does NOT registry-filter — only `ordered_panel_ids`
    /// and `representative_panel_id_for_workspace_manual_unread` do.
    pub fn focused_panel_id(&self) -> Option<Uuid> {
        let surface_id = self.focused_pane_selected_surface_id()?;
        self.panel_id(surface_id)
    }

    /// The panel that owns the workspace-level manual-unread indicator: the
    /// focused panel when it still exists, else the spatially-first selected
    /// panel across panes, else the first sidebar-ordered panel (legacy
    /// `Workspace.representativePanelIdForWorkspaceManualUnread()`, Swift
    /// `WorkspaceSurfaceListModel.swift:73-96`).
    pub fn representative_panel_id_for_workspace_manual_unread(&self) -> Option<Uuid> {
        if let Some(focused) = self.focused_panel_id() {
            if self.panel_exists(focused) {
                return Some(focused);
            }
        }

        // {paneId -> panelId} for panes whose selected surface maps to a live
        // panel, then walk pane_spatial_order for the deterministic first match.
        let mut selected_panels_by_pane_id: HashMap<Uuid, Uuid> = HashMap::new();
        for pane in &self.panes {
            let Some(surface_id) = pane.selected_surface_id() else {
                continue;
            };
            let Some(panel_id) = self.panel_id(surface_id) else {
                continue;
            };
            if !self.panel_exists(panel_id) {
                continue;
            }
            selected_panels_by_pane_id.insert(pane.id, panel_id);
        }

        for pane_id in &self.pane_spatial_order {
            if let Some(panel_id) = selected_panels_by_pane_id.get(pane_id) {
                return Some(*panel_id);
            }
        }

        self.first_sidebar_ordered_panel_id
    }

    /// The selected panel id in the pane, or `None` when the pane has no
    /// selection or the selection resolves to no panel (legacy
    /// `Workspace.effectiveSelectedPanelId(inPane:)`, Swift
    /// `WorkspaceSurfaceListModel.swift:101-104`).
    ///
    /// PARITY: deliberately does NOT registry-filter (see [`Self::focused_panel_id`]).
    pub fn effective_selected_panel_id(&self, pane_id: Uuid) -> Option<Uuid> {
        let surface_id = self.selected_surface_id(pane_id)?;
        self.panel_id(surface_id)
    }

    /// Surface ids in tab order strictly before the anchor surface in its pane,
    /// or `[]` when the anchor is not in the pane (legacy
    /// `Workspace.tabIdsToLeft(of:inPane:)`, Swift
    /// `WorkspaceSurfaceListModel.swift:109-114`).
    pub fn surface_ids_to_left(&self, anchor_surface_id: Uuid, pane_id: Uuid) -> Vec<Uuid> {
        let surface_ids = self.surface_ids_in_tab_order(pane_id);
        let Some(index) = surface_ids.iter().position(|id| *id == anchor_surface_id) else {
            return Vec::new();
        };
        surface_ids[..index].to_vec()
    }

    /// Surface ids in tab order strictly after the anchor surface in its pane,
    /// or `[]` when the anchor is absent or last (legacy
    /// `Workspace.tabIdsToRight(of:inPane:)`, Swift
    /// `WorkspaceSurfaceListModel.swift:119-125`).
    pub fn surface_ids_to_right(&self, anchor_surface_id: Uuid, pane_id: Uuid) -> Vec<Uuid> {
        let surface_ids = self.surface_ids_in_tab_order(pane_id);
        let Some(index) = surface_ids.iter().position(|id| *id == anchor_surface_id) else {
            return Vec::new();
        };
        if index + 1 >= surface_ids.len() {
            return Vec::new();
        }
        surface_ids[index + 1..].to_vec()
    }

    /// Every surface id in the pane except the anchor, in tab order (legacy
    /// `Workspace.tabIdsToCloseOthers(of:inPane:)`, Swift
    /// `WorkspaceSurfaceListModel.swift:129-132`).
    pub fn surface_ids_to_close_others(&self, anchor_surface_id: Uuid, pane_id: Uuid) -> Vec<Uuid> {
        self.surface_ids_in_tab_order(pane_id)
            .iter()
            .copied()
            .filter(|id| *id != anchor_surface_id)
            .collect()
    }

    /// Reconciles the reorder-detection bookkeeping after a geometry change:
    /// bumps `pane_layout_version` (wrapping) only when the ordered panel-id
    /// sequence actually changed, updating `last_ordered_panel_ids`. Returns
    /// whether the version was bumped (legacy gate in
    /// `Workspace.splitTabBar(_:didChangeGeometry:)`, Swift
    /// `WorkspaceSurfaceListModel.swift:148-155`).
    ///
    /// PARITY: the comparison is against the full ordered list INCLUDING the
    /// orphan tail, so a registry-only change that alters `ordered_panel_ids`
    /// still bumps — matching Swift.
    pub fn register_geometry_change(&mut self) -> bool {
        let current_order = self.ordered_panel_ids();
        if current_order == self.last_ordered_panel_ids {
            return false;
        }
        self.last_ordered_panel_ids = current_order;
        self.pane_layout_version = self.pane_layout_version.wrapping_add(1);
        true
    }
}

/// The Swift `UUID.uuidString` key: UPPERCASE, hyphenated. The Rust `Uuid`
/// `Display`/`to_string()` is lowercase, so we uppercase to match Swift's
/// `uuidString < uuidString` orphan sort exactly.
fn uuid_string(id: &Uuid) -> String {
    id.to_string().to_uppercase()
}

#[cfg(test)]
// The tests build a `SurfaceTree` then assign fields, mirroring the Swift test's
// `FakeTree` create-then-populate idiom (`WorkspaceSurfaceListModelTests.swift`)
// one-for-one; a struct-literal rewrite would obscure that parity.
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    /// Empty snapshot: reproduces the Swift `detachedModelReturnsEmptyDefaults`
    /// case, where a detached (nil-tree) model returns `[]`/`nil`/`false`.
    fn detached_defaults() -> SurfaceTree {
        SurfaceTree::default()
    }

    fn uuid(s: &str) -> Uuid {
        Uuid::parse_str(s).unwrap()
    }

    // Swift `orderedPanelIdsFollowsTabOrderThenAppendsOrphansSorted`.
    #[test]
    fn ordered_panel_ids_follows_tab_order_then_appends_orphans_sorted() {
        let s1 = Uuid::new_v4();
        let s2 = Uuid::new_v4();
        let p1 = Uuid::new_v4();
        let p2 = Uuid::new_v4();
        // Two orphan panels with deterministic uuidString order.
        let orphan_a = uuid("00000000-0000-0000-0000-0000000000AA");
        let orphan_b = uuid("00000000-0000-0000-0000-0000000000BB");

        let mut tree = SurfaceTree::default();
        tree.panes = vec![Pane::new(Uuid::new_v4(), vec![s1, s2], Some(0))];
        tree.surface_to_panel = HashMap::from([(s1, p1), (s2, p2)]);
        tree.registry = HashSet::from([p1, p2, orphan_a, orphan_b]);

        assert_eq!(tree.ordered_panel_ids(), vec![p1, p2, orphan_a, orphan_b]);
    }

    // Extra parity guard for the orphan sort key: lowercase Rust `to_string()`
    // would still sort AA before BB, so pin a case where native byte-order vs
    // uppercase-string order could diverge is unnecessary (hyphenated hex is
    // identical under both) — instead pin that a hex-letter vs digit boundary
    // sorts by string, matching Swift `uuidString` (digits '0'-'9' < letters
    // 'A'-'F' in ASCII, same as Swift's uppercase compare).
    #[test]
    fn orphan_sort_uses_uppercase_uuid_string_order() {
        // '0' (0x30) < '9' (0x39) < 'A' (0x41): a digit-suffixed uuid sorts
        // before a letter-suffixed one under the uppercase-string key.
        let orphan_digit = uuid("00000000-0000-0000-0000-000000000009");
        let orphan_letter = uuid("00000000-0000-0000-0000-0000000000FA");
        let mut tree = SurfaceTree::default();
        tree.registry = HashSet::from([orphan_letter, orphan_digit]);
        assert_eq!(tree.ordered_panel_ids(), vec![orphan_digit, orphan_letter]);
    }

    // Swift `orderedPanelIdsDropsSurfacesWithoutLivePanels`.
    #[test]
    fn ordered_panel_ids_drops_surfaces_without_live_panels() {
        let s1 = Uuid::new_v4();
        let s2 = Uuid::new_v4();
        let p1 = Uuid::new_v4();
        let p2 = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.panes = vec![Pane::new(Uuid::new_v4(), vec![s1, s2], Some(0))];
        tree.surface_to_panel = HashMap::from([(s1, p1), (s2, p2)]);
        tree.registry = HashSet::from([p1]); // p2 not in registry

        assert_eq!(tree.ordered_panel_ids(), vec![p1]);
    }

    // Swift `orderedPanelIdsDeduplicatesRepeatedSurfaces`.
    #[test]
    fn ordered_panel_ids_deduplicates_repeated_surfaces() {
        let s1 = Uuid::new_v4();
        let s2 = Uuid::new_v4();
        let p1 = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.panes = vec![Pane::new(Uuid::new_v4(), vec![s1, s2], Some(0))];
        tree.surface_to_panel = HashMap::from([(s1, p1), (s2, p1)]);
        tree.registry = HashSet::from([p1]);

        assert_eq!(tree.ordered_panel_ids(), vec![p1]);
    }

    // Swift `focusedPanelIdResolvesThroughFocusedPaneSelection`.
    #[test]
    fn focused_panel_id_resolves_through_focused_pane_selection() {
        let pane = Uuid::new_v4();
        let s1 = Uuid::new_v4();
        let p1 = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.panes = vec![Pane::new(pane, vec![s1], Some(0))];
        tree.focused_pane_id = Some(pane);
        tree.surface_to_panel = HashMap::from([(s1, p1)]);
        tree.registry = HashSet::from([p1]);

        assert_eq!(tree.focused_panel_id(), Some(p1));
    }

    // Swift `focusedPanelIdNilWhenNoPaneFocused`.
    #[test]
    fn focused_panel_id_nil_when_no_pane_focused() {
        let s1 = Uuid::new_v4();
        let p1 = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.panes = vec![Pane::new(Uuid::new_v4(), vec![s1], Some(0))];
        tree.surface_to_panel = HashMap::from([(s1, p1)]);
        tree.registry = HashSet::from([p1]);

        assert_eq!(tree.focused_panel_id(), None);
    }

    // Swift `representativePrefersFocusedWhenItExists`.
    #[test]
    fn representative_prefers_focused_when_it_exists() {
        let pane = Uuid::new_v4();
        let s1 = Uuid::new_v4();
        let p1 = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.panes = vec![Pane::new(pane, vec![s1], Some(0))];
        tree.focused_pane_id = Some(pane);
        tree.surface_to_panel = HashMap::from([(s1, p1)]);
        tree.registry = HashSet::from([p1]);

        assert_eq!(
            tree.representative_panel_id_for_workspace_manual_unread(),
            Some(p1)
        );
    }

    // Swift `representativeFallsBackToSpatiallyFirstSelectedWhenNoFocus`.
    #[test]
    fn representative_falls_back_to_spatially_first_selected_when_no_focus() {
        let pane_a = Uuid::new_v4();
        let pane_b = Uuid::new_v4();
        let s_a = Uuid::new_v4();
        let s_b = Uuid::new_v4();
        let p_a = Uuid::new_v4();
        let p_b = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.panes = vec![
            Pane::new(pane_a, vec![s_a], Some(0)),
            Pane::new(pane_b, vec![s_b], Some(0)),
        ];
        // Spatial order puts B before A.
        tree.pane_spatial_order = vec![pane_b, pane_a];
        tree.surface_to_panel = HashMap::from([(s_a, p_a), (s_b, p_b)]);
        tree.registry = HashSet::from([p_a, p_b]);

        assert_eq!(
            tree.representative_panel_id_for_workspace_manual_unread(),
            Some(p_b)
        );
    }

    // Swift `representativeFallsBackToSidebarFirstWhenNoSelection`.
    #[test]
    fn representative_falls_back_to_sidebar_first_when_no_selection() {
        let sidebar_first = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.first_sidebar_ordered_panel_id = Some(sidebar_first);

        assert_eq!(
            tree.representative_panel_id_for_workspace_manual_unread(),
            Some(sidebar_first)
        );
    }

    // Swift `effectiveSelectedPanelIdResolvesPerPane`.
    #[test]
    fn effective_selected_panel_id_resolves_per_pane() {
        let pane = Uuid::new_v4();
        let s1 = Uuid::new_v4();
        let p1 = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.panes = vec![Pane::new(pane, vec![s1], Some(0))];
        tree.surface_to_panel = HashMap::from([(s1, p1)]);
        tree.registry = HashSet::from([p1]);

        assert_eq!(tree.effective_selected_panel_id(pane), Some(p1));
        assert_eq!(tree.effective_selected_panel_id(Uuid::new_v4()), None);
    }

    // Swift `surfaceIdsLeftRightAndCloseOthers`.
    #[test]
    fn surface_ids_left_right_and_close_others() {
        let pane = Uuid::new_v4();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.panes = vec![Pane::new(pane, vec![a, b, c], Some(0))];

        assert_eq!(tree.surface_ids_to_left(b, pane), vec![a]);
        assert_eq!(tree.surface_ids_to_right(b, pane), vec![c]);
        assert_eq!(tree.surface_ids_to_close_others(b, pane), vec![a, c]);
        // Anchor absent.
        assert_eq!(tree.surface_ids_to_left(Uuid::new_v4(), pane), Vec::<Uuid>::new());
        // Last tab has nothing to the right.
        assert_eq!(tree.surface_ids_to_right(c, pane), Vec::<Uuid>::new());
    }

    // Swift `registerGeometryChangeBumpsOnlyOnReorder`.
    #[test]
    fn register_geometry_change_bumps_only_on_reorder() {
        let s1 = Uuid::new_v4();
        let s2 = Uuid::new_v4();
        let p1 = Uuid::new_v4();
        let p2 = Uuid::new_v4();
        let pane = Uuid::new_v4();
        let mut tree = SurfaceTree::default();
        tree.panes = vec![Pane::new(pane, vec![s1, s2], Some(0))];
        tree.surface_to_panel = HashMap::from([(s1, p1), (s2, p2)]);
        tree.registry = HashSet::from([p1, p2]);

        // First call: order changed from empty -> bump.
        assert!(tree.register_geometry_change());
        assert_eq!(tree.pane_layout_version, 1);
        assert_eq!(tree.last_ordered_panel_ids, vec![p1, p2]);

        // No change -> no bump.
        assert!(!tree.register_geometry_change());
        assert_eq!(tree.pane_layout_version, 1);

        // Reorder -> bump.
        tree.panes = vec![Pane::new(pane, vec![s2, s1], Some(0))];
        assert!(tree.register_geometry_change());
        assert_eq!(tree.pane_layout_version, 2);
        assert_eq!(tree.last_ordered_panel_ids, vec![p2, p1]);
    }

    // Swift `detachedModelReturnsEmptyDefaults`.
    #[test]
    fn detached_model_returns_empty_defaults() {
        let mut tree = detached_defaults();
        assert_eq!(tree.ordered_panel_ids(), Vec::<Uuid>::new());
        assert_eq!(tree.focused_panel_id(), None);
        assert_eq!(tree.representative_panel_id_for_workspace_manual_unread(), None);
        assert!(!tree.register_geometry_change());
    }
}
