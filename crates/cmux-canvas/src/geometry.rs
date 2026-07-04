//! Foundation-only value geometry for the canvas model.
//!
//! Ports (verbatim math, `f64` throughout):
//! - `CanvasPoint.swift`, `CanvasSize.swift`, `CanvasRect.swift`
//! - `CanvasMetrics.swift`
//! - `CanvasDirection.swift`
//! - `CanvasResizeEdges.swift`
//! - `CanvasGuide.swift`, `CanvasSnapResult.swift`
//! - `CanvasAlignmentCommand.swift`

use std::ops::{BitOr, RangeInclusive};

use serde::{Deserialize, Serialize};

/// A point in canvas coordinates (y-down). Port of `CanvasPoint.swift`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasPoint {
    /// Horizontal coordinate in canvas points.
    pub x: f64,
    /// Vertical coordinate in canvas points (grows downward).
    pub y: f64,
}

impl CanvasPoint {
    /// The origin point `(0, 0)`.
    pub const ZERO: CanvasPoint = CanvasPoint { x: 0.0, y: 0.0 };

    /// Creates a point.
    pub fn new(x: f64, y: f64) -> CanvasPoint {
        CanvasPoint { x, y }
    }

    /// Returns this point translated by the given deltas.
    pub fn offset_by(&self, dx: f64, dy: f64) -> CanvasPoint {
        CanvasPoint {
            x: self.x + dx,
            y: self.y + dy,
        }
    }
}

/// A width/height pair in canvas points. Port of `CanvasSize.swift`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasSize {
    /// Width in canvas points.
    pub width: f64,
    /// Height in canvas points.
    pub height: f64,
}

impl CanvasSize {
    /// The zero size.
    pub const ZERO: CanvasSize = CanvasSize {
        width: 0.0,
        height: 0.0,
    };

    /// Creates a size.
    pub fn new(width: f64, height: f64) -> CanvasSize {
        CanvasSize { width, height }
    }
}

/// An axis-aligned rectangle in canvas coordinates (y-down). Port of `CanvasRect.swift`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasRect {
    /// The minimum-x (left) edge.
    pub x: f64,
    /// The minimum-y (top) edge (y-down space).
    pub y: f64,
    /// Width in canvas points.
    pub width: f64,
    /// Height in canvas points.
    pub height: f64,
}

impl CanvasRect {
    /// The zero rect.
    pub const ZERO: CanvasRect = CanvasRect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    /// Creates a rectangle from origin and size components.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> CanvasRect {
        CanvasRect {
            x,
            y,
            width,
            height,
        }
    }

    /// Creates a rectangle from an origin point and a size.
    pub fn from_origin_size(origin: CanvasPoint, size: CanvasSize) -> CanvasRect {
        CanvasRect::new(origin.x, origin.y, size.width, size.height)
    }

    /// The left edge.
    pub fn min_x(&self) -> f64 {
        self.x
    }
    /// The right edge.
    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }
    /// The horizontal center.
    pub fn mid_x(&self) -> f64 {
        self.x + self.width / 2.0
    }
    /// The top edge (y-down space).
    pub fn min_y(&self) -> f64 {
        self.y
    }
    /// The bottom edge (y-down space).
    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }
    /// The vertical center.
    pub fn mid_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    /// The top-left corner.
    pub fn origin(&self) -> CanvasPoint {
        CanvasPoint::new(self.x, self.y)
    }

    /// Sets the top-left corner.
    pub fn set_origin(&mut self, origin: CanvasPoint) {
        self.x = origin.x;
        self.y = origin.y;
    }

    /// The rectangle size.
    pub fn size(&self) -> CanvasSize {
        CanvasSize::new(self.width, self.height)
    }

    /// Sets the rectangle size.
    pub fn set_size(&mut self, size: CanvasSize) {
        self.width = size.width;
        self.height = size.height;
    }

    /// The center point.
    pub fn center(&self) -> CanvasPoint {
        CanvasPoint::new(self.mid_x(), self.mid_y())
    }

    /// Returns this rect translated by the given deltas.
    pub fn offset_by(&self, dx: f64, dy: f64) -> CanvasRect {
        CanvasRect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// Returns this rect grown outward on every edge.
    pub fn expanded_by(&self, amount: f64) -> CanvasRect {
        CanvasRect::new(
            self.x - amount,
            self.y - amount,
            self.width + amount * 2.0,
            self.height + amount * 2.0,
        )
    }

    /// Whether this rect and `other` overlap with positive area.
    pub fn intersects(&self, other: &CanvasRect) -> bool {
        self.min_x() < other.max_x()
            && other.min_x() < self.max_x()
            && self.min_y() < other.max_y()
            && other.min_y() < self.max_y()
    }

    /// Whether the given point lies inside the rect (closed on min edges, open on max edges).
    pub fn contains(&self, point: CanvasPoint) -> bool {
        point.x >= self.min_x()
            && point.x < self.max_x()
            && point.y >= self.min_y()
            && point.y < self.max_y()
    }

    /// The smallest rect containing both this rect and `other`.
    pub fn union(&self, other: &CanvasRect) -> CanvasRect {
        let nx = self.min_x().min(other.min_x());
        let ny = self.min_y().min(other.min_y());
        CanvasRect::new(
            nx,
            ny,
            self.max_x().max(other.max_x()) - nx,
            self.max_y().max(other.max_y()) - ny,
        )
    }

    /// Returns this rect inset on every edge by the given amounts.
    ///
    /// Port of `CGRect.insetBy(dx:dy:)`: the origin moves inward by `(dx, dy)`
    /// and the size shrinks by `2·dx` / `2·dy`. Negative insets grow the rect
    /// outward (e.g. `inset_by(-24, -24)` expands by 24 on every edge).
    pub fn inset_by(&self, dx: f64, dy: f64) -> CanvasRect {
        CanvasRect::new(
            self.x + dx,
            self.y + dy,
            self.width - dx * 2.0,
            self.height - dy * 2.0,
        )
    }

    /// Whether `other` lies entirely within this rect (closed on every edge).
    ///
    /// Port of `CGRectContainsRect`. DIVERGENCE: CoreGraphics' empty/null
    /// special-casing is not modeled; callers here pass positive-area rects
    /// (the minimap only tests containment when the viewport has area), so the
    /// distinction is unobservable.
    pub fn contains_rect(&self, other: &CanvasRect) -> bool {
        other.min_x() >= self.min_x()
            && other.max_x() <= self.max_x()
            && other.min_y() >= self.min_y()
            && other.max_y() <= self.max_y()
    }

    /// The horizontal extent as a closed range.
    pub fn horizontal_range(&self) -> RangeInclusive<f64> {
        self.min_x()..=self.min_x().max(self.max_x())
    }

    /// The vertical extent as a closed range.
    pub fn vertical_range(&self) -> RangeInclusive<f64> {
        self.min_y()..=self.min_y().max(self.max_y())
    }
}

/// User-configurable layout metrics shared by every canvas operation. Port of `CanvasMetrics.swift`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CanvasMetrics {
    /// The canonical spacing between pane edges, in canvas points.
    pub gap: f64,
    /// Maximum distance at which a dragged or resized edge snaps to a target.
    pub snap_threshold: f64,
    /// The smallest size a pane may be resized to.
    pub min_pane_size: CanvasSize,
}

impl CanvasMetrics {
    /// The default gap in points used when the user has not configured one.
    pub const DEFAULT_GAP: f64 = 16.0;
    /// The default snap threshold in points.
    pub const DEFAULT_SNAP_THRESHOLD: f64 = 8.0;
    /// The default minimum pane size.
    pub const DEFAULT_MIN_PANE_SIZE: CanvasSize = CanvasSize {
        width: 200.0,
        height: 120.0,
    };

    /// Creates metrics.
    pub fn new(gap: f64, snap_threshold: f64, min_pane_size: CanvasSize) -> CanvasMetrics {
        CanvasMetrics {
            gap,
            snap_threshold,
            min_pane_size,
        }
    }
}

impl Default for CanvasMetrics {
    fn default() -> CanvasMetrics {
        CanvasMetrics {
            gap: CanvasMetrics::DEFAULT_GAP,
            snap_threshold: CanvasMetrics::DEFAULT_SNAP_THRESHOLD,
            min_pane_size: CanvasMetrics::DEFAULT_MIN_PANE_SIZE,
        }
    }
}

/// A spatial direction on the canvas. Port of `CanvasDirection.swift`.
///
/// `Ord` follows declaration order (`left < right < up < down`) to match the
/// Swift `CaseIterable` ordering used for stable iteration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CanvasDirection {
    /// Toward smaller x.
    Left,
    /// Toward larger x.
    Right,
    /// Toward smaller y (the canvas space is y-down).
    Up,
    /// Toward larger y.
    Down,
}

impl CanvasDirection {
    /// All directions in declaration order (mirror of Swift `CaseIterable`).
    pub const ALL: [CanvasDirection; 4] = [
        CanvasDirection::Left,
        CanvasDirection::Right,
        CanvasDirection::Up,
        CanvasDirection::Down,
    ];
}

/// The set of pane edges being moved by a resize gesture. Port of `CanvasResizeEdges.swift`.
///
/// DIVERGENCE: Swift models this as `OptionSet`; here it is a hand-rolled
/// `u8`-backed flags struct (no `bitflags` dep) with the same bit values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CanvasResizeEdges(u8);

impl CanvasResizeEdges {
    /// The empty edge set.
    pub const NONE: CanvasResizeEdges = CanvasResizeEdges(0);
    /// The left edge is moving.
    pub const LEFT: CanvasResizeEdges = CanvasResizeEdges(1 << 0);
    /// The right edge is moving.
    pub const RIGHT: CanvasResizeEdges = CanvasResizeEdges(1 << 1);
    /// The top edge is moving (y-down space).
    pub const TOP: CanvasResizeEdges = CanvasResizeEdges(1 << 2);
    /// The bottom edge is moving.
    pub const BOTTOM: CanvasResizeEdges = CanvasResizeEdges(1 << 3);

    /// Creates an edge set from raw option bits.
    pub fn from_raw(raw_value: u8) -> CanvasResizeEdges {
        CanvasResizeEdges(raw_value)
    }

    /// The raw option bits.
    pub fn raw_value(&self) -> u8 {
        self.0
    }

    /// Whether every bit of `other` is present in this set.
    pub fn contains(&self, other: CanvasResizeEdges) -> bool {
        self.0 & other.0 == other.0
    }
}

impl BitOr for CanvasResizeEdges {
    type Output = CanvasResizeEdges;
    fn bitor(self, rhs: CanvasResizeEdges) -> CanvasResizeEdges {
        CanvasResizeEdges(self.0 | rhs.0)
    }
}

/// The orientation of a guide line. Port of `CanvasGuide.Axis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CanvasGuideAxis {
    /// A vertical line at a fixed x coordinate.
    Vertical,
    /// A horizontal line at a fixed y coordinate.
    Horizontal,
}

/// An alignment guide line produced while a snap is active. Port of `CanvasGuide.swift`.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasGuide {
    /// The guide orientation.
    pub axis: CanvasGuideAxis,
    /// The fixed coordinate: x for vertical guides, y for horizontal guides.
    pub position: f64,
    /// The extent of the line along its own axis, covering both snapped rects.
    pub span: RangeInclusive<f64>,
}

impl CanvasGuide {
    /// Creates a guide.
    pub fn new(axis: CanvasGuideAxis, position: f64, span: RangeInclusive<f64>) -> CanvasGuide {
        CanvasGuide {
            axis,
            position,
            span,
        }
    }
}

/// The outcome of snapping a dragged or resized frame. Port of `CanvasSnapResult.swift`.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasSnapResult {
    /// The frame after snapping. Equal to the proposed frame when nothing snapped.
    pub frame: CanvasRect,
    /// Guides to render while the snap is active. Empty when nothing snapped.
    pub guides: Vec<CanvasGuide>,
}

impl CanvasSnapResult {
    /// Creates a snap result.
    pub fn new(frame: CanvasRect, guides: Vec<CanvasGuide>) -> CanvasSnapResult {
        CanvasSnapResult { frame, guides }
    }
}

/// An explicit alignment or distribution command. Port of `CanvasAlignmentCommand.swift`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CanvasAlignmentCommand {
    /// Align every pane's left edge to the leftmost selected edge.
    AlignLeft,
    /// Align every pane's right edge to the rightmost selected edge.
    AlignRight,
    /// Align every pane's top edge to the topmost selected edge.
    AlignTop,
    /// Align every pane's bottom edge to the bottommost selected edge.
    AlignBottom,
    /// Give every pane the reference pane's width, keeping left edges fixed.
    EqualizeWidths,
    /// Give every pane the reference pane's height, keeping top edges fixed.
    EqualizeHeights,
    /// Pack panes left-to-right at the canonical gap, keeping vertical positions.
    DistributeHorizontally,
    /// Pack panes top-to-bottom at the canonical gap, keeping horizontal positions.
    DistributeVertically,
    /// Re-pack panes into clean rows at the canonical gap.
    Tidy,
}

impl CanvasAlignmentCommand {
    /// All commands in declaration order (mirror of Swift `CaseIterable`).
    pub const ALL: [CanvasAlignmentCommand; 9] = [
        CanvasAlignmentCommand::AlignLeft,
        CanvasAlignmentCommand::AlignRight,
        CanvasAlignmentCommand::AlignTop,
        CanvasAlignmentCommand::AlignBottom,
        CanvasAlignmentCommand::EqualizeWidths,
        CanvasAlignmentCommand::EqualizeHeights,
        CanvasAlignmentCommand::DistributeHorizontally,
        CanvasAlignmentCommand::DistributeVertically,
        CanvasAlignmentCommand::Tidy,
    ];
}

#[cfg(test)]
mod tests {
    // Port of CanvasRectTests.swift (5 @Test cases).
    use super::*;

    #[test]
    fn edges_and_center() {
        let rect = CanvasRect::new(10.0, 20.0, 100.0, 50.0);
        assert_eq!(rect.min_x(), 10.0);
        assert_eq!(rect.max_x(), 110.0);
        assert_eq!(rect.mid_x(), 60.0);
        assert_eq!(rect.min_y(), 20.0);
        assert_eq!(rect.max_y(), 70.0);
        assert_eq!(rect.mid_y(), 45.0);
        assert_eq!(rect.center(), CanvasPoint::new(60.0, 45.0));
    }

    #[test]
    fn offset_and_expand() {
        let rect = CanvasRect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            rect.offset_by(5.0, -5.0),
            CanvasRect::new(5.0, -5.0, 10.0, 10.0)
        );
        assert_eq!(rect.expanded_by(2.0), CanvasRect::new(-2.0, -2.0, 14.0, 14.0));
        assert_eq!(rect.expanded_by(-2.0), CanvasRect::new(2.0, 2.0, 6.0, 6.0));
    }

    #[test]
    fn intersection_requires_positive_area() {
        let a = CanvasRect::new(0.0, 0.0, 10.0, 10.0);
        assert!(a.intersects(&CanvasRect::new(5.0, 5.0, 10.0, 10.0)));
        // Edge-touching rects do not intersect.
        assert!(!a.intersects(&CanvasRect::new(10.0, 0.0, 10.0, 10.0)));
        assert!(!a.intersects(&CanvasRect::new(0.0, 10.0, 10.0, 10.0)));
        assert!(!a.intersects(&CanvasRect::new(20.0, 20.0, 5.0, 5.0)));
    }

    #[test]
    fn contains_is_closed_open_per_axis() {
        let rect = CanvasRect::new(0.0, 0.0, 10.0, 10.0);
        assert!(rect.contains(CanvasPoint::new(0.0, 0.0)));
        assert!(rect.contains(CanvasPoint::new(9.99, 9.99)));
        assert!(!rect.contains(CanvasPoint::new(10.0, 5.0)));
        assert!(!rect.contains(CanvasPoint::new(5.0, 10.0)));
        assert!(!rect.contains(CanvasPoint::new(-0.01, 5.0)));
    }

    #[test]
    fn union_covers_both_rects() {
        let a = CanvasRect::new(0.0, 0.0, 10.0, 10.0);
        let b = CanvasRect::new(30.0, -5.0, 5.0, 5.0);
        assert_eq!(a.union(&b), CanvasRect::new(0.0, -5.0, 35.0, 15.0));
    }
}
