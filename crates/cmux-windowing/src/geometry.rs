//! Window-frame placement geometry (M5).
//!
//! Pure ports of the macOS window-placement math so the Windows shell makes the
//! same decisions about where a window may sit:
//!
//! * [`should_preserve_frame_during_constrain`] — the reachability test behind
//!   `CmuxMainWindow.shouldPreserveFrameDuringConstrain`
//!   (`Sources/App/CmuxMainWindow.swift`). On a display/sleep→wake the OS re-runs
//!   its constrain pass and creeps windows that are *already* on-screen; cmux
//!   refuses that re-constrain for any frame still reachable on some display,
//!   deferring to the OS only when the frame would otherwise be stranded.
//! * [`clamp_frame_within`] — the `CmuxMainWindow.clampedFrame` clamp: size the
//!   frame into `[default content size, visible frame]` and pin its origin so it
//!   stays fully within the display's visible area.
//!
//! Coordinates are `f64` (CGFloat) in the platform's global screen space; the
//! intersection math matches `CGRect.intersection` (a non-overlap yields zero
//! width/height, so it fails the reachability comparison).

/// Minimum overlap (points) in each dimension for a frame to count as
/// "reachable" — `CmuxMainWindow.shouldPreserveFrameDuringConstrain`'s default.
pub const DEFAULT_MINIMUM_VISIBLE_EXTENT: f64 = 60.0;

/// Default window content width (`CmuxMainWindow.defaultContentSize`).
pub const DEFAULT_CONTENT_WIDTH: f64 = 1_000.0;
/// Default window content height (`CmuxMainWindow.defaultContentSize`).
pub const DEFAULT_CONTENT_HEIGHT: f64 = 700.0;

/// An axis-aligned rectangle in global screen coordinates (origin + size),
/// mirroring `CGRect`/`NSRect`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Minimum-x (origin x).
    pub x: f64,
    /// Minimum-y (origin y).
    pub y: f64,
    /// Width (assumed non-negative, as screen frames are).
    pub width: f64,
    /// Height (assumed non-negative).
    pub height: f64,
}

impl Rect {
    /// The empty rectangle at the origin (a non-overlap result).
    pub const ZERO: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    /// Construct a rect from origin + size.
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Left edge.
    pub fn min_x(&self) -> f64 {
        self.x
    }
    /// Bottom edge (origin y).
    pub fn min_y(&self) -> f64 {
        self.y
    }
    /// Right edge.
    pub fn max_x(&self) -> f64 {
        self.x + self.width
    }
    /// Top edge.
    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }
    /// Horizontal midpoint.
    pub fn mid_x(&self) -> f64 {
        self.x + self.width / 2.0
    }
    /// Vertical midpoint.
    pub fn mid_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    /// Whether all four components are finite.
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.width.is_finite() && self.height.is_finite()
    }

    /// A copy with non-negative width/height (origin adjusted), matching
    /// `CGRect.standardized`.
    pub fn standardized(&self) -> Rect {
        let (x, width) = if self.width < 0.0 {
            (self.x + self.width, -self.width)
        } else {
            (self.x, self.width)
        };
        let (y, height) = if self.height < 0.0 {
            (self.y + self.height, -self.height)
        } else {
            (self.y, self.height)
        };
        Rect::new(x, y, width, height)
    }

    /// Whether the two rectangles overlap with positive area (matching
    /// `CGRect.intersects`; edge-only contact is not an intersection).
    pub fn intersects(&self, other: &Rect) -> bool {
        let intersection = self.intersection(other);
        intersection.width > 0.0 && intersection.height > 0.0
    }

    /// Area of the overlap with `other` (0 when disjoint).
    pub fn intersection_area(&self, other: &Rect) -> f64 {
        let intersection = self.intersection(other);
        intersection.width.max(0.0) * intersection.height.max(0.0)
    }

    /// The overlapping rectangle, or [`Rect::ZERO`] when the rectangles do not
    /// overlap — matching `CGRect.intersection` for the purposes here (a
    /// non-overlap has zero width/height). Edge-only contact (zero-area overlap)
    /// counts as no intersection.
    pub fn intersection(&self, other: &Rect) -> Rect {
        let left = self.min_x().max(other.min_x());
        let bottom = self.min_y().max(other.min_y());
        let right = self.max_x().min(other.max_x());
        let top = self.max_y().min(other.max_y());
        if right > left && top > bottom {
            Rect::new(left, bottom, right - left, top - bottom)
        } else {
            Rect::ZERO
        }
    }
}

/// Whether `proposed_frame` is reachable enough across `visible_frames` that the
/// OS's constraining pass should be skipped, using the default
/// [`DEFAULT_MINIMUM_VISIBLE_EXTENT`].
pub fn should_preserve_frame_during_constrain(
    proposed_frame: Rect,
    visible_frames: &[Rect],
) -> bool {
    should_preserve_frame_during_constrain_with_extent(
        proposed_frame,
        visible_frames,
        DEFAULT_MINIMUM_VISIBLE_EXTENT,
    )
}

/// As [`should_preserve_frame_during_constrain`] but with an explicit minimum
/// visible extent. The frame qualifies when it overlaps some visible frame by at
/// least `minimum_visible_extent` points in *both* dimensions (or its full
/// extent when the frame is smaller than that) — i.e. a usable, grabbable slice
/// is on-screen.
pub fn should_preserve_frame_during_constrain_with_extent(
    proposed_frame: Rect,
    visible_frames: &[Rect],
    minimum_visible_extent: f64,
) -> bool {
    let required_width = proposed_frame.width.min(minimum_visible_extent);
    let required_height = proposed_frame.height.min(minimum_visible_extent);
    visible_frames.iter().any(|visible_frame| {
        let intersection = proposed_frame.intersection(visible_frame);
        intersection.width >= required_width && intersection.height >= required_height
    })
}

/// Clamp `frame` so it fits within `visible_frame`: its size is bounded to
/// `[default content size, visible frame size]` and its origin is pinned so the
/// whole window stays inside the visible area. Port of
/// `CmuxMainWindow.clampedFrame`. A degenerate (non-positive) visible frame
/// returns `frame` unchanged.
pub fn clamp_frame_within(frame: Rect, visible_frame: Rect) -> Rect {
    if visible_frame.width <= 0.0 || visible_frame.height <= 0.0 {
        return frame;
    }

    let width = frame
        .width
        .max(DEFAULT_CONTENT_WIDTH)
        .min(visible_frame.width);
    let height = frame
        .height
        .max(DEFAULT_CONTENT_HEIGHT)
        .min(visible_frame.height);
    let x = frame
        .min_x()
        .max(visible_frame.min_x())
        .min(visible_frame.max_x() - width);
    let y = frame
        .min_y()
        .max(visible_frame.min_y())
        .min(visible_frame.max_y() - height);
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect::new(x, y, w, h)
    }

    #[test]
    fn intersection_of_overlapping_rects() {
        let a = r(0.0, 0.0, 100.0, 100.0);
        let b = r(50.0, 50.0, 100.0, 100.0);
        assert_eq!(a.intersection(&b), r(50.0, 50.0, 50.0, 50.0));
    }

    #[test]
    fn intersection_of_disjoint_rects_is_zero() {
        let a = r(0.0, 0.0, 10.0, 10.0);
        let b = r(100.0, 100.0, 10.0, 10.0);
        assert_eq!(a.intersection(&b), Rect::ZERO);
    }

    #[test]
    fn edge_contact_is_not_an_intersection() {
        // Touching at x == 10 (zero-area overlap) → not reachable.
        let a = r(0.0, 0.0, 10.0, 10.0);
        let b = r(10.0, 0.0, 10.0, 10.0);
        assert_eq!(a.intersection(&b), Rect::ZERO);
    }

    #[test]
    fn fully_on_screen_frame_is_preserved() {
        let screen = r(0.0, 0.0, 1920.0, 1080.0);
        let window = r(100.0, 100.0, 800.0, 600.0);
        assert!(should_preserve_frame_during_constrain(window, &[screen]));
    }

    #[test]
    fn slightly_overlapping_frame_is_preserved_when_grabbable() {
        // Only the bottom-right 80×80 of the window pokes onto the screen — more
        // than the 60pt grabbable slice in both dims → preserved.
        let screen = r(0.0, 0.0, 1000.0, 1000.0);
        let window = r(-720.0, -520.0, 800.0, 600.0); // overlaps [0,80) × [0,80)
        let overlap = window.intersection(&screen);
        assert_eq!((overlap.width, overlap.height), (80.0, 80.0));
        assert!(should_preserve_frame_during_constrain(window, &[screen]));
    }

    #[test]
    fn barely_overlapping_frame_is_not_preserved() {
        // Only a 40×40 sliver overlaps — below the 60pt threshold → not reachable.
        let screen = r(0.0, 0.0, 1000.0, 1000.0);
        let window = r(-760.0, -560.0, 800.0, 600.0); // overlaps [0,40) × [0,40)
        assert!(!should_preserve_frame_during_constrain(window, &[screen]));
    }

    #[test]
    fn small_window_uses_its_full_extent_as_threshold() {
        // A 30×30 window needs only 30pt of overlap (its full extent), not 60.
        let screen = r(0.0, 0.0, 1000.0, 1000.0);
        let window = r(-10.0, -10.0, 30.0, 30.0); // overlaps [0,20) × [0,20)
        // 20 < 30 → still not reachable.
        assert!(!should_preserve_frame_during_constrain(window, &[screen]));
        let window2 = r(-2.0, -2.0, 30.0, 30.0); // overlaps [0,28) × [0,28) ≥ 28? need ≥30
        assert!(!should_preserve_frame_during_constrain(window2, &[screen]));
        let window3 = r(0.0, 0.0, 30.0, 30.0); // fully on → 30 ≥ 30
        assert!(should_preserve_frame_during_constrain(window3, &[screen]));
    }

    #[test]
    fn offscreen_frame_after_display_loss_is_not_preserved() {
        // Window stranded far off every visible frame → defer to OS constrain.
        let screen = r(0.0, 0.0, 1920.0, 1080.0);
        let window = r(5000.0, 5000.0, 800.0, 600.0);
        assert!(!should_preserve_frame_during_constrain(window, &[screen]));
    }

    #[test]
    fn reachable_on_any_of_multiple_displays() {
        let primary = r(0.0, 0.0, 1920.0, 1080.0);
        let secondary = r(1920.0, 0.0, 2560.0, 1440.0);
        let window = r(2200.0, 200.0, 800.0, 600.0); // fully on the secondary
        assert!(should_preserve_frame_during_constrain(window, &[primary, secondary]));
    }

    #[test]
    fn clamp_pulls_offscreen_window_back_into_visible_area() {
        let visible = r(0.0, 0.0, 1920.0, 1080.0);
        let frame = r(3000.0, 2000.0, 800.0, 600.0);
        let clamped = clamp_frame_within(frame, visible);
        // Sized at least default content size, fully inside the visible frame.
        assert!(clamped.width >= DEFAULT_CONTENT_WIDTH);
        assert!(clamped.height >= DEFAULT_CONTENT_HEIGHT);
        assert!(clamped.min_x() >= visible.min_x());
        assert!(clamped.min_y() >= visible.min_y());
        assert!(clamped.max_x() <= visible.max_x() + f64::EPSILON);
        assert!(clamped.max_y() <= visible.max_y() + f64::EPSILON);
    }

    #[test]
    fn clamp_enforces_minimum_content_size() {
        let visible = r(0.0, 0.0, 1920.0, 1080.0);
        let tiny = r(10.0, 10.0, 200.0, 150.0);
        let clamped = clamp_frame_within(tiny, visible);
        assert_eq!(clamped.width, DEFAULT_CONTENT_WIDTH);
        assert_eq!(clamped.height, DEFAULT_CONTENT_HEIGHT);
    }

    #[test]
    fn clamp_bounds_size_to_visible_frame_when_smaller_than_default() {
        // A visible area smaller than the default content size clamps down to it.
        let visible = r(0.0, 0.0, 800.0, 500.0);
        let frame = r(0.0, 0.0, 1200.0, 900.0);
        let clamped = clamp_frame_within(frame, visible);
        assert_eq!(clamped.width, 800.0);
        assert_eq!(clamped.height, 500.0);
    }

    #[test]
    fn clamp_leaves_a_well_placed_frame_essentially_unchanged() {
        let visible = r(0.0, 0.0, 1920.0, 1080.0);
        let frame = r(100.0, 100.0, 1000.0, 700.0);
        assert_eq!(clamp_frame_within(frame, visible), frame);
    }

    #[test]
    fn clamp_returns_frame_unchanged_for_degenerate_visible() {
        let frame = r(100.0, 100.0, 800.0, 600.0);
        assert_eq!(clamp_frame_within(frame, Rect::ZERO), frame);
    }

    #[test]
    fn clamp_respects_nonzero_visible_origin() {
        // Secondary display offset to the right; clamp must keep the window on it.
        let visible = r(1920.0, 0.0, 1280.0, 1024.0);
        let frame = r(0.0, 0.0, 1000.0, 700.0);
        let clamped = clamp_frame_within(frame, visible);
        assert!(clamped.min_x() >= visible.min_x());
        assert!(clamped.max_x() <= visible.max_x() + f64::EPSILON);
    }
}
