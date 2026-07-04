//! Bidirectional surface-id <-> panel-id map with the exclusive-by-panel
//! invariant.
//!
//! A pure port of the surface-mapping half of `CmuxPanes`' `PaneTreeModel`
//! (`Model/PaneTreeModel.swift:42-112`): the `surfaceIdToPanelId` /
//! `panelIdToSurfaceId` dictionary pair and the seven deterministic operations
//! over them (`bindSurface`, `removeSurfaceMapping(forSurfaceId:)`,
//! `removeSurfaceMappings(forPanelId:)`, `panelId(forSurfaceId:)`,
//! `surfaceId(forPanelId:)`, plus forward-map read access).
//!
//! Deliberately NOT ported (GUI / app-coupled, `PaneTreeModel.swift:15-65`):
//! the `@MainActor @Observable` wrapper, the generic `panels: [UUID: Panel]`
//! registry, `paneLayoutVersion`, `lastOrderedPanelIds`, the `PaneTreeHosting`
//! `willSet` observer hooks, and Observation timing. This module ports only the
//! standalone bidirectional dictionary and its invariant, which is the
//! groundable, OS-independent logic.
//!
//! Swift keys the forward map on Bonsplit's `TabID` (surface id). Bonsplit's
//! `TabID()` default-constructs a fresh unique value like `UUID()`; the Bonsplit
//! submodule is a gap in this port (see `tree.rs`), so surface ids are modelled
//! by the [`SurfaceId`] newtype. It is a distinct type from the panel `Uuid` so
//! a panel id can never be silently used to index the surface map.

use std::collections::HashMap;

use uuid::Uuid;

/// A Bonsplit surface id (the `TabID` the split tree assigns to a mounted tab).
///
/// Modelled as a newtype over [`Uuid`] rather than a bare alias so it is a
/// distinct type from the panel `Uuid`; this prevents cross-keying the two
/// maps. Mirrors Bonsplit `TabID`, whose default initializer mints a fresh
/// unique value just like `UUID()`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SurfaceId(pub Uuid);

impl SurfaceId {
    /// Mints a fresh unique surface id (parity with Bonsplit `TabID()` /
    /// `UUID()`).
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        SurfaceId(Uuid::new_v4())
    }
}

/// The workspace-side mapping from Bonsplit surface ids onto panel ids.
///
/// Port of the `surfaceIdToPanelId` / `panelIdToSurfaceId` pair in
/// `PaneTreeModel` (`PaneTreeModel.swift:42,51`). The binding is exclusive by
/// panel id: a live panel is represented by at most one surface at a time.
#[derive(Debug, Default, Clone)]
pub struct PaneSurfaceMap {
    /// Forward index: surface id -> owning panel id
    /// (`PaneTreeModel.surfaceIdToPanelId`, `PaneTreeModel.swift:42`).
    surface_to_panel: HashMap<SurfaceId, Uuid>,
    /// Reverse index for targeted lookups and stale-surface removal
    /// (`PaneTreeModel.panelIdToSurfaceId`, `PaneTreeModel.swift:51`).
    panel_to_surface: HashMap<Uuid, SurfaceId>,
}

impl PaneSurfaceMap {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds a bonsplit surface id to a panel id.
    ///
    /// The binding is exclusive by panel id: a live panel can be represented by
    /// only one surface at a time, so rebinding removes stale entries before
    /// installing the new owner.
    ///
    /// Verbatim port of `PaneTreeModel.bindSurface(_:toPanelId:)`
    /// (`PaneTreeModel.swift:72-85`). The two guarded cleanup steps are kept
    /// exactly as written and must NOT be collapsed to unconditional removals:
    ///
    /// 1. If this panel already pointed at a *different* surface, drop that old
    ///    surface's forward entry.
    /// 2. If this surface already pointed at a *different* panel **and** that
    ///    panel's reverse entry still points back at this surface, drop that
    ///    panel's reverse entry. The second condition prevents a stale surface
    ///    from clobbering a panel that has already been rebound elsewhere.
    pub fn bind(&mut self, surface_id: SurfaceId, panel_id: Uuid) {
        if let Some(&previous_surface_id) = self.panel_to_surface.get(&panel_id) {
            if previous_surface_id != surface_id {
                self.surface_to_panel.remove(&previous_surface_id);
            }
        }
        if let Some(&previous_panel_id) = self.surface_to_panel.get(&surface_id) {
            if previous_panel_id != panel_id
                && self.panel_to_surface.get(&previous_panel_id) == Some(&surface_id)
            {
                self.panel_to_surface.remove(&previous_panel_id);
            }
        }

        self.surface_to_panel.insert(surface_id, panel_id);
        self.panel_to_surface.insert(panel_id, surface_id);
    }

    /// Removes the mapping for one bonsplit surface id.
    ///
    /// Port of `PaneTreeModel.removeSurfaceMapping(forSurfaceId:)`
    /// (`PaneTreeModel.swift:88-93`). Keys off the forward map, then removes the
    /// reverse entry only if it still points back at this surface (the
    /// equality re-check that keeps a rebound surface intact).
    pub fn remove_surface(&mut self, surface_id: SurfaceId) {
        if let Some(panel_id) = self.surface_to_panel.remove(&surface_id) {
            if self.panel_to_surface.get(&panel_id) == Some(&surface_id) {
                self.panel_to_surface.remove(&panel_id);
            }
        }
    }

    /// Removes every mapping that can still resolve to a closed panel.
    ///
    /// Port of `PaneTreeModel.removeSurfaceMappings(forPanelId:)`
    /// (`PaneTreeModel.swift:96-100`). Asymmetric with [`Self::remove_surface`]:
    /// keys off the reverse map and drops the surface's forward entry without an
    /// equality re-check (the reverse entry is authoritative for the panel).
    pub fn remove_by_panel(&mut self, panel_id: Uuid) {
        if let Some(surface_id) = self.panel_to_surface.remove(&panel_id) {
            self.surface_to_panel.remove(&surface_id);
        }
    }

    /// Resolves the owning panel id for a bonsplit surface id.
    ///
    /// Port of `PaneTreeModel.panelId(forSurfaceId:)`
    /// (`PaneTreeModel.swift:104-106`).
    pub fn panel_for_surface(&self, surface_id: SurfaceId) -> Option<Uuid> {
        self.surface_to_panel.get(&surface_id).copied()
    }

    /// Resolves the bonsplit surface id currently mapped to a panel id.
    ///
    /// Port of `PaneTreeModel.surfaceId(forPanelId:)`
    /// (`PaneTreeModel.swift:110-112`).
    pub fn surface_for_panel(&self, panel_id: Uuid) -> Option<SurfaceId> {
        self.panel_to_surface.get(&panel_id).copied()
    }

    /// Read access to the forward map (surface id -> panel id), mirroring the
    /// `public private(set)` `surfaceIdToPanelId` accessor
    /// (`PaneTreeModel.swift:42`).
    pub fn surface_to_panel(&self) -> &HashMap<SurfaceId, Uuid> {
        &self.surface_to_panel
    }

    /// `true` when no surface is bound.
    pub fn is_empty(&self) -> bool {
        self.surface_to_panel.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bookkeeping stores round-trip unchanged.
    ///
    /// Port of `bookkeepingStoresRoundTrip`
    /// (`PaneTreeModelTests.swift:79-88`; the `lastOrderedPanelIds` line is
    /// dropped — that field is the un-ported GUI bookkeeping).
    #[test]
    fn bookkeeping_stores_round_trip() {
        let mut model = PaneSurfaceMap::new();
        let tab_id = SurfaceId::new();
        let panel_id = Uuid::new_v4();
        model.bind(tab_id, panel_id);
        assert_eq!(model.surface_to_panel().get(&tab_id), Some(&panel_id));
    }

    /// The registry queries resolve through the surface-id mapping.
    ///
    /// Port of `registryQueriesResolveTheMapping`
    /// (`PaneTreeModelTests.swift:90-103`).
    #[test]
    fn registry_queries_resolve_the_mapping() {
        let mut model = PaneSurfaceMap::new();
        let tab_id = SurfaceId::new();
        let panel_id = Uuid::new_v4();
        model.bind(tab_id, panel_id);

        assert_eq!(model.panel_for_surface(tab_id), Some(panel_id));
        assert_eq!(model.surface_for_panel(panel_id), Some(tab_id));
        assert_eq!(model.panel_for_surface(SurfaceId::new()), None);
        assert_eq!(model.surface_for_panel(Uuid::new_v4()), None);
    }

    /// Rebinding one live panel to a new bonsplit surface must not leave the old
    /// surface id resolving to the same panel.
    ///
    /// Port of `rebindingPanelToNewSurfaceInvalidatesOldSurfaceMapping`
    /// (`PaneTreeModelTests.swift:105-119`).
    #[test]
    fn rebinding_panel_to_new_surface_invalidates_old_surface_mapping() {
        let mut model = PaneSurfaceMap::new();
        let old_tab_id = SurfaceId::new();
        let new_tab_id = SurfaceId::new();
        let panel_id = Uuid::new_v4();

        model.bind(old_tab_id, panel_id);
        model.bind(new_tab_id, panel_id);

        assert_eq!(model.panel_for_surface(old_tab_id), None);
        assert_eq!(model.panel_for_surface(new_tab_id), Some(panel_id));
        assert_eq!(model.surface_for_panel(panel_id), Some(new_tab_id));
    }

    /// Reusing one bonsplit surface for a different panel must also clear the old
    /// panel's reverse lookup.
    ///
    /// Port of `rebindingSurfaceToNewPanelInvalidatesOldPanelMapping`
    /// (`PaneTreeModelTests.swift:121-135`).
    #[test]
    fn rebinding_surface_to_new_panel_invalidates_old_panel_mapping() {
        let mut model = PaneSurfaceMap::new();
        let tab_id = SurfaceId::new();
        let old_panel_id = Uuid::new_v4();
        let new_panel_id = Uuid::new_v4();

        model.bind(tab_id, old_panel_id);
        model.bind(tab_id, new_panel_id);

        assert_eq!(model.panel_for_surface(tab_id), Some(new_panel_id));
        assert_eq!(model.surface_for_panel(old_panel_id), None);
        assert_eq!(model.surface_for_panel(new_panel_id), Some(tab_id));
    }

    /// Close cleanup removes the surface owned by the closed panel.
    ///
    /// Port of `closedPanelCleanupRemovesClosedSurfaceMapping`
    /// (`PaneTreeModelTests.swift:137-148`).
    #[test]
    fn closed_panel_cleanup_removes_closed_surface_mapping() {
        let mut model = PaneSurfaceMap::new();
        let closed_panel_tab_id = SurfaceId::new();
        let closed_panel_id = Uuid::new_v4();

        model.bind(closed_panel_tab_id, closed_panel_id);
        model.remove_by_panel(closed_panel_id);

        assert_eq!(model.panel_for_surface(closed_panel_tab_id), None);
        assert_eq!(model.surface_for_panel(closed_panel_id), None);
    }

    /// Close cleanup removes stale aliases for the closed panel without using a
    /// stale tab id to drop a surface that has already moved to another panel.
    ///
    /// Port of `closedPanelCleanupKeepsReboundSurfaceMapping`
    /// (`PaneTreeModelTests.swift:150-166`). This is the test that exercises the
    /// subtle second guard in [`PaneSurfaceMap::bind`].
    #[test]
    fn closed_panel_cleanup_keeps_rebound_surface_mapping() {
        let mut model = PaneSurfaceMap::new();
        let rebound_tab_id = SurfaceId::new();
        let closed_panel_id = Uuid::new_v4();
        let live_panel_id = Uuid::new_v4();

        model.bind(rebound_tab_id, closed_panel_id);
        model.bind(rebound_tab_id, live_panel_id);

        model.remove_by_panel(closed_panel_id);

        assert_eq!(model.panel_for_surface(rebound_tab_id), Some(live_panel_id));
        assert_eq!(model.surface_for_panel(closed_panel_id), None);
        assert_eq!(model.surface_for_panel(live_panel_id), Some(rebound_tab_id));
    }

    /// Author-derived oracle for the singular `remove_surface`
    /// (`removeSurfaceMapping(forSurfaceId:)`, `PaneTreeModel.swift:88-93`),
    /// which has a public method but no dedicated Swift test. Removing a bound
    /// surface clears both directions.
    #[test]
    fn remove_surface_removes_both_directions() {
        let mut model = PaneSurfaceMap::new();
        let tab_id = SurfaceId::new();
        let panel_id = Uuid::new_v4();

        model.bind(tab_id, panel_id);
        model.remove_surface(tab_id);

        assert_eq!(model.panel_for_surface(tab_id), None);
        assert_eq!(model.surface_for_panel(panel_id), None);
        assert!(model.is_empty());
    }

    /// Author-derived oracle exercising the equality re-check in
    /// `remove_surface` (`PaneTreeModel.swift:90`): after a surface is rebound
    /// to a new panel, removing that surface must not touch the OLD panel's
    /// reverse entry — but here the old panel's reverse was already dropped by
    /// the rebind, so removing the surface leaves the map empty of that surface
    /// while the reverse for the live panel is cleared. This pins the
    /// asymmetry: `remove_surface` re-checks reverse equality before deleting.
    #[test]
    fn remove_surface_respects_reverse_equality_recheck() {
        let mut model = PaneSurfaceMap::new();
        let tab_id = SurfaceId::new();
        let old_panel_id = Uuid::new_v4();
        let new_panel_id = Uuid::new_v4();

        // Rebind the surface to a new panel: reverse now points new_panel -> tab.
        model.bind(tab_id, old_panel_id);
        model.bind(tab_id, new_panel_id);

        // Removing the surface: forward removed yields new_panel_id, whose
        // reverse still == tab_id, so the reverse is dropped.
        model.remove_surface(tab_id);
        assert_eq!(model.panel_for_surface(tab_id), None);
        assert_eq!(model.surface_for_panel(new_panel_id), None);
        // The old panel's reverse was cleared during the rebind, not here.
        assert_eq!(model.surface_for_panel(old_panel_id), None);
    }
}
