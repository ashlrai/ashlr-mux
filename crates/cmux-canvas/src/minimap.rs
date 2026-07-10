//! Pure minimap geometry: navigation bounds, letterbox projection, and the
//! canvas↔minimap coordinate maps used to draw and navigate the overview.
//!
//! Ports the Foundation/CoreGraphics value core of the SwiftUI `CmuxCanvasUI`
//! minimap (the NSView drawing and mouse handling stay excluded):
//! - [`CanvasMinimapPaneSnapshot`] ← `CanvasMinimapPaneSnapshot.swift:5-8`
//! - [`CanvasMinimapProjection`] ← `CanvasMinimapProjection.swift:4-7`
//! - [`CanvasMinimapSnapshot`] ← `CanvasMinimapSnapshot.swift:6-93`
//!
//! DIVERGENCE (union): the Swift core computes navigation bounds over `CGRect`,
//! whose `union` special-cases empty/null rectangles; this port reuses
//! [`CanvasRect::union`] (plain min/max). The two agree for the positive-area
//! frames and viewports the minimap actually renders — `should_show` gates all
//! observable use of `navigation_bounds` behind a `> 1`-sized viewport, and
//! pane frames are never empty — so the special-casing is unobservable here.

use crate::geometry::{CanvasPoint, CanvasRect};
use crate::pane::CanvasPaneID;

/// One pane shown in the canvas minimap, in z-order.
///
/// Port of `CanvasMinimapPaneSnapshot.swift:5-8`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasMinimapPaneSnapshot {
    /// The pane identifier.
    pub id: CanvasPaneID,
    /// The pane frame in canvas coordinates.
    pub frame: CanvasRect,
}

impl CanvasMinimapPaneSnapshot {
    /// Creates a pane snapshot.
    pub fn new(id: CanvasPaneID, frame: CanvasRect) -> CanvasMinimapPaneSnapshot {
        CanvasMinimapPaneSnapshot { id, frame }
    }
}

/// Projection from canvas coordinates into minimap drawing coordinates.
///
/// Port of `CanvasMinimapProjection.swift:4-7`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasMinimapProjection {
    /// The uniform canvas→minimap scale factor.
    pub scale: f64,
    /// The minimap-space origin of the projected navigation bounds.
    pub origin: CanvasPoint,
}

impl CanvasMinimapProjection {
    /// Creates a projection.
    pub fn new(scale: f64, origin: CanvasPoint) -> CanvasMinimapProjection {
        CanvasMinimapProjection { scale, origin }
    }
}

/// Value snapshot used by the canvas minimap to draw and navigate without
/// reading live view state during rendering.
///
/// Port of `CanvasMinimapSnapshot.swift:6-93`.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasMinimapSnapshot {
    /// The panes shown in the minimap, in z-order.
    pub panes: Vec<CanvasMinimapPaneSnapshot>,
    /// The currently visible canvas viewport.
    pub visible_rect: CanvasRect,
    /// The focused pane, if any.
    pub focused_pane_id: Option<CanvasPaneID>,
    /// The union of all pane frames with the viewport, clamped to a minimum of
    /// 1×1. This is the region the minimap maps into its drawing rect.
    pub navigation_bounds: CanvasRect,
    /// The union of pane frames alone (`None` when there are no panes). Mirrors
    /// the Swift `private let contentBounds`.
    content_bounds: Option<CanvasRect>,
}

impl CanvasMinimapSnapshot {
    /// Builds a snapshot, deriving `navigation_bounds` and `content_bounds`.
    ///
    /// Port of `CanvasMinimapSnapshot.swift:13-36`.
    pub fn new(
        panes: Vec<CanvasMinimapPaneSnapshot>,
        visible_rect: CanvasRect,
        focused_pane_id: Option<CanvasPaneID>,
    ) -> CanvasMinimapSnapshot {
        // content = panes.map(\.frame).reduce(nil) { $0?.union($1) ?? $1 }
        let content_bounds = panes
            .iter()
            .map(|pane| pane.frame)
            .reduce(|partial, frame| partial.union(&frame));

        // var bounds = (content ?? visibleRect).union(visibleRect)
        let mut bounds = content_bounds.unwrap_or(visible_rect).union(&visible_rect);
        if bounds.width < 1.0 {
            bounds.x -= 0.5;
            bounds.width = 1.0;
        }
        if bounds.height < 1.0 {
            bounds.y -= 0.5;
            bounds.height = 1.0;
        }

        CanvasMinimapSnapshot {
            panes,
            visible_rect,
            focused_pane_id,
            navigation_bounds: bounds,
            content_bounds,
        }
    }

    /// Whether the minimap should be shown for this snapshot.
    ///
    /// Hidden when the viewport is degenerate (≤ 1 in either dimension), when
    /// there are no panes, or when a lone pane already fits inside the viewport
    /// (with a 24-point margin). Port of `CanvasMinimapSnapshot.swift:38-43`.
    pub fn should_show(&self) -> bool {
        if !(self.visible_rect.width > 1.0 && self.visible_rect.height > 1.0) {
            return false;
        }
        if self.panes.is_empty() {
            return false;
        }
        let content = match self.content_bounds {
            Some(content) => content,
            None => return false,
        };
        self.panes.len() > 1
            || !self
                .visible_rect
                .inset_by(-24.0, -24.0)
                .contains_rect(&content)
    }

    /// The letterbox-fit projection of `navigation_bounds` into `drawing_rect`.
    ///
    /// Chooses the largest uniform scale that fits the navigation bounds inside
    /// the drawing rect, then centers the used area. A degenerate drawing rect
    /// yields an identity scale at the drawing origin. Port of
    /// `CanvasMinimapSnapshot.swift:45-64`.
    pub fn projection(&self, drawing_rect: CanvasRect) -> CanvasMinimapProjection {
        if !(drawing_rect.width > 0.0 && drawing_rect.height > 0.0) {
            return CanvasMinimapProjection::new(1.0, drawing_rect.origin());
        }
        let scale = (drawing_rect.width / self.navigation_bounds.width)
            .min(drawing_rect.height / self.navigation_bounds.height);
        let used_width = self.navigation_bounds.width * scale;
        let used_height = self.navigation_bounds.height * scale;
        CanvasMinimapProjection::new(
            scale,
            CanvasPoint::new(
                drawing_rect.min_x() + (drawing_rect.width - used_width) / 2.0,
                drawing_rect.min_y() + (drawing_rect.height - used_height) / 2.0,
            ),
        )
    }

    /// Maps a canvas-space rect into minimap drawing coordinates.
    ///
    /// Port of `CanvasMinimapSnapshot.swift:66-74`.
    pub fn minimap_rect(&self, canvas_rect: CanvasRect, drawing_rect: CanvasRect) -> CanvasRect {
        let projection = self.projection(drawing_rect);
        CanvasRect::new(
            projection.origin.x
                + (canvas_rect.min_x() - self.navigation_bounds.min_x()) * projection.scale,
            projection.origin.y
                + (canvas_rect.min_y() - self.navigation_bounds.min_y()) * projection.scale,
            canvas_rect.width * projection.scale,
            canvas_rect.height * projection.scale,
        )
    }

    /// The projected navigation bounds, excluding the centered letterbox
    /// padding. Port of `CanvasMinimapSnapshot.swift:76-84`.
    pub fn projected_navigation_bounds(&self, drawing_rect: CanvasRect) -> CanvasRect {
        let projection = self.projection(drawing_rect);
        CanvasRect::new(
            projection.origin.x,
            projection.origin.y,
            self.navigation_bounds.width * projection.scale,
            self.navigation_bounds.height * projection.scale,
        )
    }

    /// Inverse-maps a minimap-space point back into canvas coordinates.
    ///
    /// Port of `CanvasMinimapSnapshot.swift:86-92`.
    pub fn canvas_point(
        &self,
        minimap_point: CanvasPoint,
        drawing_rect: CanvasRect,
    ) -> CanvasPoint {
        let projection = self.projection(drawing_rect);
        CanvasPoint::new(
            self.navigation_bounds.min_x()
                + (minimap_point.x - projection.origin.x) / projection.scale,
            self.navigation_bounds.min_y()
                + (minimap_point.y - projection.origin.y) / projection.scale,
        )
    }
}

#[cfg(test)]
mod tests {
    // Port of CanvasMinimapSnapshotTests.swift (6 @Test cases), plus
    // hand-computed edge cases for the 1×1 clamp and degenerate drawing rect.
    use super::*;
    use crate::pane::Uuid;

    fn pane_id(byte: u8) -> CanvasPaneID {
        CanvasPaneID::new(Uuid::from_bytes([byte; 16]))
    }

    fn pane(byte: u8, frame: CanvasRect) -> CanvasMinimapPaneSnapshot {
        CanvasMinimapPaneSnapshot::new(pane_id(byte), frame)
    }

    #[test]
    fn navigation_bounds_include_content_and_visible_viewport() {
        let p = pane(1, CanvasRect::new(0.0, 0.0, 300.0, 200.0));
        let snapshot = CanvasMinimapSnapshot::new(
            vec![p],
            CanvasRect::new(900.0, 500.0, 400.0, 300.0),
            Some(p.id),
        );

        assert_eq!(
            snapshot.navigation_bounds,
            CanvasRect::new(0.0, 0.0, 1300.0, 800.0)
        );
        assert!(snapshot.should_show());
    }

    #[test]
    fn projection_centers_letterboxed_content() {
        let p = pane(1, CanvasRect::new(0.0, 0.0, 200.0, 100.0));
        let snapshot =
            CanvasMinimapSnapshot::new(vec![p], CanvasRect::new(0.0, 0.0, 200.0, 100.0), None);

        let projection = snapshot.projection(CanvasRect::new(0.0, 0.0, 100.0, 100.0));

        assert_eq!(projection.scale, 0.5);
        assert_eq!(projection.origin, CanvasPoint::new(0.0, 25.0));
    }

    #[test]
    fn projected_navigation_bounds_exclude_letterbox_padding() {
        let p = pane(1, CanvasRect::new(0.0, 0.0, 200.0, 100.0));
        let snapshot =
            CanvasMinimapSnapshot::new(vec![p], CanvasRect::new(0.0, 0.0, 200.0, 100.0), None);

        let projected =
            snapshot.projected_navigation_bounds(CanvasRect::new(0.0, 0.0, 100.0, 100.0));

        assert_eq!(projected, CanvasRect::new(0.0, 25.0, 100.0, 50.0));
    }

    #[test]
    fn point_mapping_round_trips_through_projection() {
        let p = pane(1, CanvasRect::new(-200.0, 100.0, 400.0, 300.0));
        let snapshot =
            CanvasMinimapSnapshot::new(vec![p], CanvasRect::new(-100.0, 200.0, 100.0, 100.0), None);
        let drawing_rect = CanvasRect::new(10.0, 10.0, 160.0, 80.0);
        let canvas_point = CanvasPoint::new(20.0, 250.0);
        let minimap_rect = snapshot.minimap_rect(
            CanvasRect::from_origin_size(canvas_point, crate::geometry::CanvasSize::new(1.0, 1.0)),
            drawing_rect,
        );

        let mapped = snapshot.canvas_point(minimap_rect.origin(), drawing_rect);

        assert!((mapped.x - canvas_point.x).abs() < 0.0001);
        assert!((mapped.y - canvas_point.y).abs() < 0.0001);
    }

    #[test]
    fn single_pane_that_fits_viewport_stays_hidden() {
        let p = pane(1, CanvasRect::new(40.0, 40.0, 120.0, 90.0));
        let snapshot =
            CanvasMinimapSnapshot::new(vec![p], CanvasRect::new(0.0, 0.0, 400.0, 300.0), None);

        assert!(!snapshot.should_show());
    }

    #[test]
    fn degenerate_viewport_stays_hidden() {
        let p = pane(1, CanvasRect::new(0.0, 0.0, 120.0, 90.0));
        let snapshot = CanvasMinimapSnapshot::new(vec![p, p], CanvasRect::ZERO, None);

        assert!(!snapshot.should_show());
    }

    // --- Hand-computed edge cases (no dedicated Swift oracle) ---

    #[test]
    fn navigation_bounds_clamp_to_minimum_one_by_one() {
        // Sub-1 content in both dimensions: origin shifts by -0.5, size → 1.
        // Swift `CanvasMinimapSnapshot.swift:27-34`.
        let p = pane(1, CanvasRect::new(10.0, 10.0, 0.5, 0.5));
        let snapshot =
            CanvasMinimapSnapshot::new(vec![p], CanvasRect::new(10.0, 10.0, 0.5, 0.5), None);

        assert_eq!(
            snapshot.navigation_bounds,
            CanvasRect::new(9.5, 9.5, 1.0, 1.0)
        );
    }

    #[test]
    fn navigation_bounds_clamp_single_axis() {
        // Width < 1 clamps x/width; height ≥ 1 is left untouched.
        let p = pane(1, CanvasRect::new(10.0, 10.0, 0.5, 2.0));
        let snapshot =
            CanvasMinimapSnapshot::new(vec![p], CanvasRect::new(10.0, 10.0, 0.5, 2.0), None);

        assert_eq!(
            snapshot.navigation_bounds,
            CanvasRect::new(9.5, 10.0, 1.0, 2.0)
        );
    }

    #[test]
    fn degenerate_drawing_rect_yields_identity_projection() {
        // Swift `CanvasMinimapSnapshot.swift:46-48`.
        let p = pane(1, CanvasRect::new(0.0, 0.0, 200.0, 100.0));
        let snapshot =
            CanvasMinimapSnapshot::new(vec![p], CanvasRect::new(0.0, 0.0, 200.0, 100.0), None);

        let projection = snapshot.projection(CanvasRect::new(7.0, 9.0, 0.0, 50.0));

        assert_eq!(projection.scale, 1.0);
        assert_eq!(projection.origin, CanvasPoint::new(7.0, 9.0));
    }

    #[test]
    fn empty_panes_never_show() {
        let snapshot =
            CanvasMinimapSnapshot::new(vec![], CanvasRect::new(0.0, 0.0, 400.0, 300.0), None);

        // content_bounds is None → navigation_bounds falls back to the viewport.
        assert_eq!(
            snapshot.navigation_bounds,
            CanvasRect::new(0.0, 0.0, 400.0, 300.0)
        );
        assert!(!snapshot.should_show());
    }

    #[test]
    fn multiple_panes_always_show_even_when_fitting() {
        // panes.count > 1 short-circuits the containment test.
        let a = pane(1, CanvasRect::new(40.0, 40.0, 60.0, 60.0));
        let b = pane(2, CanvasRect::new(120.0, 40.0, 60.0, 60.0));
        let snapshot =
            CanvasMinimapSnapshot::new(vec![a, b], CanvasRect::new(0.0, 0.0, 400.0, 300.0), None);

        assert!(snapshot.should_show());
    }
}
