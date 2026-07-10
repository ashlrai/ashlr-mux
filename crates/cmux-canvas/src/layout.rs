//! The z-ordered pane list with panel/tab hosting. Port of `CanvasLayout.swift`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::geometry::CanvasRect;
use crate::pane::{CanvasPane, CanvasPaneID, CanvasPanelID};

/// The complete geometric state of one workspace's canvas.
///
/// Array order is z-order, back to front. Every mutation is synchronous,
/// deterministic, and `serde` round-trips exactly.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct CanvasLayout {
    /// Panes in z-order, back to front (`private(set)` in Swift).
    panes: Vec<CanvasPane>,
}

impl CanvasLayout {
    /// Creates a layout with the given panes in z-order, back to front.
    pub fn new(panes: Vec<CanvasPane>) -> CanvasLayout {
        CanvasLayout { panes }
    }

    /// Panes in z-order, back to front.
    pub fn panes(&self) -> &[CanvasPane] {
        &self.panes
    }

    /// All pane identifiers in z-order, back to front.
    pub fn pane_ids(&self) -> Vec<CanvasPaneID> {
        self.panes.iter().map(|p| p.id).collect()
    }

    /// Whether the layout has no panes.
    pub fn is_empty(&self) -> bool {
        self.panes.is_empty()
    }

    /// Returns the frame of the given pane, if present.
    pub fn frame(&self, id: CanvasPaneID) -> Option<CanvasRect> {
        self.panes.iter().find(|p| p.id == id).map(|p| p.frame)
    }

    /// Whether the layout contains the given pane.
    pub fn contains(&self, id: CanvasPaneID) -> bool {
        self.panes.iter().any(|p| p.id == id)
    }

    /// The frames of every pane except the given one, in z-order.
    pub fn frames_excluding(&self, excluded: CanvasPaneID) -> Vec<CanvasRect> {
        self.panes
            .iter()
            .filter(|p| p.id != excluded)
            .map(|p| p.frame)
            .collect()
    }

    /// The smallest rect containing every pane, or `None` for an empty canvas.
    pub fn content_bounds(&self) -> Option<CanvasRect> {
        let first = self.panes.first()?;
        Some(
            self.panes
                .iter()
                .skip(1)
                .fold(first.frame, |acc, pane| acc.union(&pane.frame)),
        )
    }

    /// The top-most pane whose frame contains the given point, if any.
    pub fn top_pane(&self, point: crate::geometry::CanvasPoint) -> Option<CanvasPaneID> {
        self.panes
            .iter()
            .rev()
            .find(|p| p.frame.contains(point))
            .map(|p| p.id)
    }

    /// Adds a pane in front of all existing panes. Re-adding an existing id
    /// replaces its frame and brings it to the front.
    pub fn add(&mut self, pane: CanvasPane) {
        self.panes.retain(|p| p.id != pane.id);
        self.panes.push(pane);
    }

    /// Removes a pane. Removing an absent pane is a no-op.
    pub fn remove(&mut self, id: CanvasPaneID) {
        self.panes.retain(|p| p.id != id);
    }

    /// Replaces the frame of an existing pane. Updating an absent pane is a no-op.
    pub fn set_frame(&mut self, frame: CanvasRect, id: CanvasPaneID) {
        if let Some(index) = self.panes.iter().position(|p| p.id == id) {
            self.panes[index].frame = frame;
        }
    }

    /// Applies a batch of frame updates in one mutation. Unknown ids are ignored.
    pub fn set_frames(&mut self, frames: &HashMap<CanvasPaneID, CanvasRect>) {
        for index in 0..self.panes.len() {
            if let Some(frame) = frames.get(&self.panes[index].id) {
                self.panes[index].frame = *frame;
            }
        }
    }

    /// Moves a pane to the front of the z-order. Raising an absent pane is a no-op.
    pub fn bring_to_front(&mut self, id: CanvasPaneID) {
        let index = match self.panes.iter().position(|p| p.id == id) {
            Some(index) => index,
            None => return,
        };
        if index == self.panes.len() - 1 {
            return;
        }
        let pane = self.panes.remove(index);
        self.panes.push(pane);
    }

    // MARK: - Panels (tabs)

    /// All hosted panel identifiers, pane by pane in z-order, tabs left to right.
    pub fn all_panel_ids(&self) -> Vec<CanvasPanelID> {
        self.panes
            .iter()
            .flat_map(|p| p.panel_ids().iter().copied())
            .collect()
    }

    /// The pane hosting the given panel, if any.
    pub fn pane_containing(&self, panel_id: CanvasPanelID) -> Option<CanvasPaneID> {
        self.panes
            .iter()
            .find(|p| p.contains(panel_id))
            .map(|p| p.id)
    }

    /// The ordered tabs of a pane, or `None` for an absent pane.
    pub fn panel_ids_in(&self, id: CanvasPaneID) -> Option<Vec<CanvasPanelID>> {
        self.panes
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.panel_ids().to_vec())
    }

    /// The selected tab of a pane, or `None` for an absent pane.
    pub fn selected_panel_id_in(&self, id: CanvasPaneID) -> Option<CanvasPanelID> {
        self.panes
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.selected_panel_id())
    }

    /// Selects a tab inside the pane hosting it. No-op when no pane hosts it.
    pub fn select_panel(&mut self, panel_id: CanvasPanelID) {
        if let Some(index) = self.panes.iter().position(|p| p.contains(panel_id)) {
            self.panes[index].select(panel_id);
        }
    }

    /// Inserts a panel into an existing pane (a join). The panel is first
    /// removed from any pane currently hosting it, so this is also the move
    /// primitive. Joining into an absent pane is a no-op.
    pub fn add_panel(
        &mut self,
        panel_id: CanvasPanelID,
        to_pane: CanvasPaneID,
        index: Option<i64>,
        select: bool,
    ) {
        if !self.panes.iter().any(|p| p.id == to_pane) {
            return;
        }
        if self.pane_containing(panel_id) != Some(to_pane) {
            self.remove_panel(panel_id);
        }
        let destination = match self.panes.iter().position(|p| p.id == to_pane) {
            Some(destination) => destination,
            None => return,
        };
        self.panes[destination].insert(panel_id, index, select);
    }

    /// Removes a panel from the pane hosting it. A pane that loses its last
    /// panel is removed from the canvas. Returns the pane that hosted it, or
    /// `None` when no pane did.
    pub fn remove_panel(&mut self, panel_id: CanvasPanelID) -> Option<CanvasPaneID> {
        let index = self.panes.iter().position(|p| p.contains(panel_id))?;
        let pane_id = self.panes[index].id;
        if !self.panes[index].remove_panel(panel_id) {
            self.panes.remove(index);
        }
        Some(pane_id)
    }

    /// Breaks a panel out of its pane into a new single-tab pane with the given
    /// identifier and frame, in front of all existing panes. No-op when no pane
    /// hosts the panel, when it is already alone in its pane, or when the new
    /// pane id already exists.
    pub fn break_out_panel(
        &mut self,
        panel_id: CanvasPanelID,
        new_pane_id: CanvasPaneID,
        frame: CanvasRect,
    ) -> bool {
        let index = match self.panes.iter().position(|p| p.contains(panel_id)) {
            Some(index) => index,
            None => return false,
        };
        if self.panes[index].panel_ids().len() <= 1 || self.contains(new_pane_id) {
            return false;
        }
        let _ = self.panes[index].remove_panel(panel_id);
        self.panes.push(CanvasPane::with_panels(
            new_pane_id,
            frame,
            vec![panel_id],
            panel_id,
        ));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::CanvasPoint;
    use crate::pane::Uuid;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn id(value: u8) -> CanvasPaneID {
        let mut bytes = [0u8; 16];
        bytes[0] = value;
        CanvasPaneID::new(Uuid::from_bytes(bytes))
    }

    // Deterministic stand-in for `UUID()` in the tab tests: only distinctness
    // matters for the assertions, so a monotonic counter preserves parity while
    // staying reproducible (no randomness).
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    fn fresh_uuid() -> Uuid {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut bytes = [0u8; 16];
        bytes[8..16].copy_from_slice(&n.to_be_bytes());
        Uuid::from_bytes(bytes)
    }

    fn panel_id() -> CanvasPanelID {
        CanvasPanelID::new(fresh_uuid())
    }

    fn single_tab_pane(x: f64) -> CanvasPane {
        CanvasPane::new(
            CanvasPaneID::new(fresh_uuid()),
            CanvasRect::new(x, 0.0, 300.0, 200.0),
        )
    }

    // ---- Port of CanvasLayoutTests.swift (7 @Test cases) ----

    #[test]
    fn add_remove_and_lookup() {
        let mut layout = CanvasLayout::default();
        assert!(layout.is_empty());
        let a = id(1);
        layout.add(CanvasPane::new(a, CanvasRect::new(0.0, 0.0, 100.0, 100.0)));
        assert!(layout.contains(a));
        assert_eq!(
            layout.frame(a),
            Some(CanvasRect::new(0.0, 0.0, 100.0, 100.0))
        );
        layout.remove(a);
        assert!(!layout.contains(a));
        assert_eq!(layout.frame(a), None);
    }

    #[test]
    fn adding_existing_pane_replaces_and_raises() {
        let mut layout = CanvasLayout::default();
        let a = id(1);
        let b = id(2);
        layout.add(CanvasPane::new(a, CanvasRect::new(0.0, 0.0, 10.0, 10.0)));
        layout.add(CanvasPane::new(b, CanvasRect::new(20.0, 0.0, 10.0, 10.0)));
        layout.add(CanvasPane::new(a, CanvasRect::new(40.0, 0.0, 10.0, 10.0)));
        assert_eq!(layout.panes().len(), 2);
        assert_eq!(layout.pane_ids(), vec![b, a]);
        assert_eq!(
            layout.frame(a),
            Some(CanvasRect::new(40.0, 0.0, 10.0, 10.0))
        );
    }

    #[test]
    fn z_order_and_bring_to_front() {
        let mut layout = CanvasLayout::default();
        let a = id(1);
        let b = id(2);
        let c = id(3);
        for (pane_id, x) in [(a, 0.0), (b, 10.0), (c, 20.0)] {
            layout.add(CanvasPane::new(
                pane_id,
                CanvasRect::new(x, 0.0, 10.0, 10.0),
            ));
        }
        layout.bring_to_front(a);
        assert_eq!(layout.pane_ids(), vec![b, c, a]);
        // Raising the front pane keeps order.
        layout.bring_to_front(a);
        assert_eq!(layout.pane_ids(), vec![b, c, a]);
    }

    #[test]
    fn top_pane_hit_tests_front_first() {
        let mut layout = CanvasLayout::default();
        let back = id(1);
        let front = id(2);
        layout.add(CanvasPane::new(
            back,
            CanvasRect::new(0.0, 0.0, 100.0, 100.0),
        ));
        layout.add(CanvasPane::new(
            front,
            CanvasRect::new(50.0, 50.0, 100.0, 100.0),
        ));
        assert_eq!(layout.top_pane(CanvasPoint::new(75.0, 75.0)), Some(front));
        assert_eq!(layout.top_pane(CanvasPoint::new(10.0, 10.0)), Some(back));
        assert_eq!(layout.top_pane(CanvasPoint::new(500.0, 500.0)), None);
    }

    #[test]
    fn content_bounds_unions_all_panes() {
        let mut layout = CanvasLayout::default();
        assert_eq!(layout.content_bounds(), None);
        layout.add(CanvasPane::new(
            id(1),
            CanvasRect::new(-10.0, 0.0, 20.0, 20.0),
        ));
        layout.add(CanvasPane::new(
            id(2),
            CanvasRect::new(100.0, -50.0, 30.0, 30.0),
        ));
        assert_eq!(
            layout.content_bounds(),
            Some(CanvasRect::new(-10.0, -50.0, 140.0, 70.0))
        );
    }

    #[test]
    fn set_frames_applies_batch_and_ignores_unknown_ids() {
        let mut layout = CanvasLayout::default();
        let a = id(1);
        layout.add(CanvasPane::new(a, CanvasRect::new(0.0, 0.0, 10.0, 10.0)));
        let mut frames = HashMap::new();
        frames.insert(a, CanvasRect::new(5.0, 5.0, 10.0, 10.0));
        frames.insert(id(9), CanvasRect::new(99.0, 99.0, 1.0, 1.0));
        layout.set_frames(&frames);
        assert_eq!(layout.frame(a), Some(CanvasRect::new(5.0, 5.0, 10.0, 10.0)));
        assert_eq!(layout.panes().len(), 1);
    }

    #[test]
    fn codable_round_trip_preserves_order_and_frames() {
        let mut layout = CanvasLayout::default();
        layout.add(CanvasPane::new(
            id(3),
            CanvasRect::new(1.5, -2.25, 320.0, 240.0),
        ));
        layout.add(CanvasPane::new(
            id(1),
            CanvasRect::new(400.0, 0.0, 100.0, 100.0),
        ));
        let data = serde_json::to_string(&layout).unwrap();
        let decoded: CanvasLayout = serde_json::from_str(&data).unwrap();
        assert_eq!(decoded, layout);
        assert_eq!(decoded.pane_ids(), layout.pane_ids());
    }

    // Serde golden round-trip: exact JSON shape mirrors Swift's synthesized
    // `Codable` (keyed containers, `rawValue` UUID string, camelCase tab keys).
    #[test]
    fn serde_golden_layout() {
        let mut layout = CanvasLayout::default();
        layout.add(CanvasPane::new(
            id(3),
            CanvasRect::new(1.5, -2.25, 320.0, 240.0),
        ));
        let json = serde_json::to_string(&layout).unwrap();
        let golden = concat!(
            "{\"panes\":[{",
            "\"id\":{\"rawValue\":\"03000000-0000-0000-0000-000000000000\"},",
            "\"frame\":{\"x\":1.5,\"y\":-2.25,\"width\":320.0,\"height\":240.0},",
            "\"panelIds\":[{\"rawValue\":\"03000000-0000-0000-0000-000000000000\"}],",
            "\"selectedPanelId\":{\"rawValue\":\"03000000-0000-0000-0000-000000000000\"}",
            "}]}"
        );
        assert_eq!(json, golden);
        let decoded: CanvasLayout = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, layout);
    }

    // ---- Port of CanvasLayoutTabTests.swift (9 @Test cases) ----

    #[test]
    fn single_tab_pane_hosts_its_founding_panel() {
        let pane = single_tab_pane(0.0);
        assert_eq!(pane.panel_ids(), &[CanvasPanelID::new(pane.id.raw_value)]);
        assert_eq!(pane.selected_panel_id().raw_value, pane.id.raw_value);
    }

    #[test]
    fn add_panel_joins_and_selects() {
        let mut layout = CanvasLayout::new(vec![single_tab_pane(0.0), single_tab_pane(400.0)]);
        let destination = layout.pane_ids()[0];
        let joining = panel_id();
        layout.add(CanvasPane::new(
            CanvasPaneID::new(joining.raw_value),
            CanvasRect::new(800.0, 0.0, 300.0, 200.0),
        ));

        layout.add_panel(joining, destination, None, true);

        // The joining panel's old single-tab pane disappeared with it.
        assert_eq!(layout.panes().len(), 2);
        assert_eq!(layout.pane_containing(joining), Some(destination));
        assert_eq!(layout.selected_panel_id_in(destination), Some(joining));
        assert_eq!(layout.panel_ids_in(destination).map(|p| p.len()), Some(2));
    }

    #[test]
    fn add_panel_at_index_clamps_and_orders() {
        let mut layout = CanvasLayout::new(vec![single_tab_pane(0.0)]);
        let destination = layout.pane_ids()[0];
        let a = panel_id();
        let b = panel_id();
        layout.add(CanvasPane::new(
            CanvasPaneID::new(a.raw_value),
            CanvasRect::new(400.0, 0.0, 300.0, 200.0),
        ));
        layout.add(CanvasPane::new(
            CanvasPaneID::new(b.raw_value),
            CanvasRect::new(800.0, 0.0, 300.0, 200.0),
        ));

        layout.add_panel(a, destination, Some(0), false);
        layout.add_panel(b, destination, Some(99), false);

        let founding = CanvasPanelID::new(destination.raw_value);
        assert_eq!(layout.panel_ids_in(destination), Some(vec![a, founding, b]));
        // select: false keeps the original selection.
        assert_eq!(layout.selected_panel_id_in(destination), Some(founding));
    }

    #[test]
    fn remove_panel_moves_selection_to_neighbor() {
        let mut layout = CanvasLayout::new(vec![single_tab_pane(0.0)]);
        let destination = layout.pane_ids()[0];
        let a = panel_id();
        layout.add(CanvasPane::new(
            CanvasPaneID::new(a.raw_value),
            CanvasRect::new(400.0, 0.0, 300.0, 200.0),
        ));
        layout.add_panel(a, destination, None, true);

        let hosting_pane = layout.remove_panel(a);

        assert_eq!(hosting_pane, Some(destination));
        assert_eq!(layout.panes().len(), 1);
        assert_eq!(
            layout.selected_panel_id_in(destination),
            Some(CanvasPanelID::new(destination.raw_value))
        );
    }

    #[test]
    fn removing_last_panel_removes_pane() {
        let mut layout = CanvasLayout::new(vec![single_tab_pane(0.0)]);
        let pane = layout.pane_ids()[0];
        layout.remove_panel(CanvasPanelID::new(pane.raw_value));
        assert!(layout.is_empty());
    }

    #[test]
    fn break_out_panel_creates_frontmost_single_tab_pane() {
        let mut layout = CanvasLayout::new(vec![single_tab_pane(0.0), single_tab_pane(400.0)]);
        let destination = layout.pane_ids()[0];
        let joining = panel_id();
        layout.add(CanvasPane::new(
            CanvasPaneID::new(joining.raw_value),
            CanvasRect::new(800.0, 0.0, 300.0, 200.0),
        ));
        layout.add_panel(joining, destination, None, true);

        let new_pane_id = CanvasPaneID::new(fresh_uuid());
        let frame = CanvasRect::new(1200.0, 0.0, 300.0, 200.0);
        let did_break = layout.break_out_panel(joining, new_pane_id, frame);
        assert!(did_break);

        assert_eq!(layout.pane_containing(joining), Some(new_pane_id));
        assert_eq!(layout.pane_ids().last().copied(), Some(new_pane_id));
        assert_eq!(layout.frame(new_pane_id), Some(frame));
        // Breaking the now-single founding panel out of its pane is a no-op.
        let founding = CanvasPanelID::new(destination.raw_value);
        let did_break_lone =
            layout.break_out_panel(founding, CanvasPaneID::new(fresh_uuid()), frame);
        assert!(!did_break_lone);
    }

    #[test]
    fn break_out_founding_panel_of_multi_tab_pane() {
        let mut layout = CanvasLayout::new(vec![single_tab_pane(0.0)]);
        let destination = layout.pane_ids()[0];
        let founding = CanvasPanelID::new(destination.raw_value);
        let joined = panel_id();
        layout.add(CanvasPane::new(
            CanvasPaneID::new(joined.raw_value),
            CanvasRect::new(400.0, 0.0, 300.0, 200.0),
        ));
        layout.add_panel(joined, destination, None, false);
        assert_eq!(
            layout.panel_ids_in(destination),
            Some(vec![founding, joined])
        );

        let new_pane_id = CanvasPaneID::new(fresh_uuid());
        let frame = CanvasRect::new(1200.0, 0.0, 300.0, 200.0);
        let did_break = layout.break_out_panel(founding, new_pane_id, frame);
        assert!(did_break);

        assert_eq!(layout.pane_containing(founding), Some(new_pane_id));
        assert_eq!(layout.panel_ids_in(new_pane_id), Some(vec![founding]));
        assert_eq!(layout.pane_containing(joined), Some(destination));
        assert_eq!(layout.panel_ids_in(destination), Some(vec![joined]));
        assert_eq!(layout.pane_ids().last().copied(), Some(new_pane_id));
        assert_eq!(layout.frame(new_pane_id), Some(frame));
    }

    #[test]
    fn select_panel_only_affects_hosting_pane() {
        let mut layout = CanvasLayout::new(vec![single_tab_pane(0.0)]);
        let destination = layout.pane_ids()[0];
        let a = panel_id();
        layout.add(CanvasPane::new(
            CanvasPaneID::new(a.raw_value),
            CanvasRect::new(400.0, 0.0, 300.0, 200.0),
        ));
        layout.add_panel(a, destination, None, false);

        layout.select_panel(a);
        assert_eq!(layout.selected_panel_id_in(destination), Some(a));

        layout.select_panel(panel_id());
        assert_eq!(layout.selected_panel_id_in(destination), Some(a));
    }

    #[test]
    fn codable_round_trips_tabs() {
        let mut layout = CanvasLayout::new(vec![single_tab_pane(0.0)]);
        let destination = layout.pane_ids()[0];
        let a = panel_id();
        layout.add(CanvasPane::new(
            CanvasPaneID::new(a.raw_value),
            CanvasRect::new(400.0, 0.0, 300.0, 200.0),
        ));
        layout.add_panel(a, destination, None, true);

        let data = serde_json::to_string(&layout).unwrap();
        let decoded: CanvasLayout = serde_json::from_str(&data).unwrap();
        assert_eq!(decoded, layout);
    }
}
