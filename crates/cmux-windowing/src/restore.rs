//! Session-restore window-frame resolution (M5).
//!
//! Pure port of the macOS `AppDelegate.resolvedWindowFrame` family
//! (`Sources/AppDelegate.swift:3505-3805`): given a persisted window frame + the
//! display it was saved on, and the displays currently attached, decide where to
//! reopen the window. The decision ladder:
//!
//! 1. Reject a frame that is missing, non-finite, or smaller than the minimum
//!    window size (a corrupt/degenerate snapshot).
//! 2. With no displays attached, trust the saved frame as-is.
//! 3. Find the *target* display (by saved display id, else largest-overlap, else
//!    nearest-center of the saved reference rect).
//!    - If the display arrangement is byte-for-byte the saved one, preserve the
//!      exact frame.
//!    - Else, if the frame still intersects the target's visible area and its
//!      titlebar strip stays grabbable, preserve it; otherwise clamp it in.
//!    - If it doesn't intersect, proportionally remap it from the saved display
//!      to the target (or center it when there's no usable source reference).
//! 4. No matched target but some display's visible area intersects → clamp there.
//! 5. Otherwise remap/center onto the fallback display.
//!
//! All inputs are plain value types; the desktop shell converts persisted
//! session snapshots + live `NSScreen`/monitor state into these.

use crate::geometry::Rect;

/// Minimum restorable window width (`SessionPersistencePolicy.minimumWindowWidth`).
pub const MINIMUM_WINDOW_WIDTH: f64 = 300.0;
/// Minimum restorable window height (`SessionPersistencePolicy.minimumWindowHeight`).
pub const MINIMUM_WINDOW_HEIGHT: f64 = 200.0;

const MINIMUM_VISIBLE_TOP_STRIP_WIDTH: f64 = 120.0;
const TOP_STRIP_HEIGHT: f64 = 64.0;
const MINIMUM_VISIBLE_TOP_STRIP_HEIGHT: f64 = 24.0;
const RECT_EQUAL_TOLERANCE: f64 = 1.0;

/// A persisted rectangle (`SessionRectSnapshot`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectSnapshot {
    /// Origin x.
    pub x: f64,
    /// Origin y.
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

impl RectSnapshot {
    /// Convert to a [`Rect`].
    pub fn to_rect(self) -> Rect {
        Rect::new(self.x, self.y, self.width, self.height)
    }
}

/// The display a window was saved on (`SessionDisplaySnapshot`): identity plus
/// the full/visible frames as they were at save time. Any field may be absent.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DisplaySnapshot {
    /// CoreGraphics/monitor display id, when it was resolvable.
    pub display_id: Option<u32>,
    /// The saved display's full frame.
    pub frame: Option<RectSnapshot>,
    /// The saved display's visible frame.
    pub visible_frame: Option<RectSnapshot>,
}

impl DisplaySnapshot {
    /// The best available reference rectangle (visible frame preferred).
    fn reference_rect(&self) -> Option<Rect> {
        self.visible_frame.or(self.frame).map(RectSnapshot::to_rect)
    }
}

/// A currently-attached display (`SessionDisplayGeometry`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayGeometry {
    /// CoreGraphics/monitor display id, when resolvable.
    pub display_id: Option<u32>,
    /// The display's full frame in global screen coordinates.
    pub frame: Rect,
    /// The display's visible frame (excluding menu bar / taskbar).
    pub visible_frame: Rect,
}

/// Resolve where to reopen a window, given its saved `frame_snapshot` + the
/// display it was on (`display_snapshot`), the currently attached
/// `available_displays`, and a `fallback_display` for the stranded case.
/// Returns `None` only when the saved frame is missing/degenerate/too small.
pub fn resolved_window_frame(
    frame_snapshot: Option<RectSnapshot>,
    display_snapshot: Option<&DisplaySnapshot>,
    available_displays: &[DisplayGeometry],
    fallback_display: Option<&DisplayGeometry>,
) -> Option<Rect> {
    let frame = frame_snapshot?.to_rect();
    if !frame.is_finite() {
        return None;
    }
    if frame.width < MINIMUM_WINDOW_WIDTH || frame.height < MINIMUM_WINDOW_HEIGHT {
        return None;
    }

    if available_displays.is_empty() {
        return Some(frame);
    }

    if let Some(target) = display_for(display_snapshot, available_displays) {
        if should_preserve_exact_frame(frame, display_snapshot, target) {
            return Some(frame);
        }
        return Some(resolved_window_frame_for_target(
            frame,
            display_snapshot,
            target,
        ));
    }

    if let Some(intersecting) = available_displays
        .iter()
        .find(|display| display.visible_frame.intersects(&frame))
    {
        return Some(clamp_frame(frame, intersecting.visible_frame));
    }

    let fallback_display = fallback_display?;
    if let Some(source) = display_snapshot.and_then(DisplaySnapshot::reference_rect) {
        Some(remapped_frame(
            frame,
            source,
            fallback_display.visible_frame,
        ))
    } else {
        Some(centered_frame(frame, fallback_display.visible_frame))
    }
}

/// Convenience: try the primary window's saved frame, then a fallback frame,
/// mirroring `AppDelegate.resolvedStartupPrimaryWindowFrame`.
pub fn resolved_startup_primary_window_frame(
    primary_frame: Option<RectSnapshot>,
    primary_display: Option<&DisplaySnapshot>,
    fallback_frame: Option<RectSnapshot>,
    fallback_display_snapshot: Option<&DisplaySnapshot>,
    available_displays: &[DisplayGeometry],
    fallback_display: Option<&DisplayGeometry>,
) -> Option<Rect> {
    resolved_window_frame(
        primary_frame,
        primary_display,
        available_displays,
        fallback_display,
    )
    .or_else(|| {
        resolved_window_frame(
            fallback_frame,
            fallback_display_snapshot,
            available_displays,
            fallback_display,
        )
    })
}

fn resolved_window_frame_for_target(
    frame: Rect,
    display_snapshot: Option<&DisplaySnapshot>,
    target: &DisplayGeometry,
) -> Rect {
    if target.visible_frame.intersects(&frame) {
        if should_preserve_accessible_frame(frame, target) {
            return frame;
        }
        return clamp_frame(frame, target.visible_frame);
    }

    if let Some(source) = display_snapshot.and_then(DisplaySnapshot::reference_rect) {
        remapped_frame(frame, source, target.visible_frame)
    } else {
        centered_frame(frame, target.visible_frame)
    }
}

/// Whether enough of the window's top strip stays on the target's visible area
/// that its titlebar remains grabbable (so the exact frame can be preserved).
fn should_preserve_accessible_frame(frame: Rect, target: &DisplayGeometry) -> bool {
    let frame = frame.standardized();
    if !frame.is_finite()
        || frame.width <= 0.0
        || frame.height <= 0.0
        || !frame.intersects(&target.frame)
    {
        return false;
    }

    let strip_height = TOP_STRIP_HEIGHT.min(frame.height);
    let top_strip = Rect::new(
        frame.min_x(),
        frame.max_y() - strip_height,
        frame.width,
        strip_height,
    );
    let visible_top_strip = top_strip.intersection(&target.visible_frame);
    if visible_top_strip.width <= 0.0 || visible_top_strip.height <= 0.0 {
        return false;
    }

    let required_width = MINIMUM_VISIBLE_TOP_STRIP_WIDTH.min(frame.width);
    let required_height = MINIMUM_VISIBLE_TOP_STRIP_HEIGHT.min(strip_height);
    visible_top_strip.width >= required_width && visible_top_strip.height >= required_height
}

/// Pick the display a saved snapshot maps to: exact id, else largest visible-area
/// overlap, else nearest center.
fn display_for<'a>(
    snapshot: Option<&DisplaySnapshot>,
    displays: &'a [DisplayGeometry],
) -> Option<&'a DisplayGeometry> {
    let snapshot = snapshot?;
    if let Some(display_id) = snapshot.display_id {
        if let Some(exact) = displays
            .iter()
            .find(|display| display.display_id == Some(display_id))
        {
            return Some(exact);
        }
    }

    let reference_rect = snapshot.reference_rect()?;

    let best_overlap = displays
        .iter()
        .map(|display| {
            (
                display,
                reference_rect.intersection_area(&display.visible_frame),
            )
        })
        .filter(|(_, area)| *area > 0.0)
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if let Some((display, _)) = best_overlap {
        return Some(display);
    }

    let reference_center = (reference_rect.mid_x(), reference_rect.mid_y());
    displays.iter().min_by(|lhs, rhs| {
        let lhs_distance = distance_squared(&lhs.visible_frame, reference_center);
        let rhs_distance = distance_squared(&rhs.visible_frame, reference_center);
        lhs_distance
            .partial_cmp(&rhs_distance)
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn remapped_frame(frame: Rect, source_rect: Rect, target_rect: Rect) -> Rect {
    let source = source_rect.standardized();
    let target = target_rect.standardized();
    if !source.is_finite()
        || !target.is_finite()
        || source.width <= 1.0
        || source.height <= 1.0
        || target.width <= 0.0
        || target.height <= 0.0
    {
        return centered_frame(frame, target_rect);
    }

    let relative_x = (frame.min_x() - source.min_x()) / source.width;
    let relative_y = (frame.min_y() - source.min_y()) / source.height;
    let relative_width = frame.width / source.width;
    let relative_height = frame.height / source.height;

    let remapped = Rect::new(
        target.min_x() + relative_x * target.width,
        target.min_y() + relative_y * target.height,
        target.width * relative_width,
        target.height * relative_height,
    );
    clamp_frame(remapped, target)
}

fn centered_frame(frame: Rect, visible_frame: Rect) -> Rect {
    let centered = Rect::new(
        visible_frame.mid_x() - frame.width / 2.0,
        visible_frame.mid_y() - frame.height / 2.0,
        frame.width,
        frame.height,
    );
    clamp_frame(centered, visible_frame)
}

/// Clamp `frame` into `visible_frame`, flooring size at the minimum window size
/// (bounded by the visible area) and pinning the origin inside. Port of
/// `AppDelegate.clampFrame`. Distinct from `geometry::clamp_frame_within`, which
/// uses the larger default-content-size floor for fresh windows.
fn clamp_frame(frame: Rect, visible_frame: Rect) -> Rect {
    if !visible_frame.is_finite() || visible_frame.width <= 0.0 || visible_frame.height <= 0.0 {
        return frame;
    }

    let max_width = visible_frame.width.max(1.0);
    let max_height = visible_frame.height.max(1.0);
    let width_floor = MINIMUM_WINDOW_WIDTH.min(max_width);
    let height_floor = MINIMUM_WINDOW_HEIGHT.min(max_height);

    let width = frame.width.max(width_floor).min(max_width);
    let height = frame.height.max(height_floor).min(max_height);
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

fn should_preserve_exact_frame(
    frame: Rect,
    display_snapshot: Option<&DisplaySnapshot>,
    target: &DisplayGeometry,
) -> bool {
    let Some(snapshot) = display_snapshot else {
        return false;
    };
    let (Some(snapshot_id), Some(target_id)) = (snapshot.display_id, target.display_id) else {
        return false;
    };
    if snapshot_id != target_id {
        return false;
    }

    let visible_matches = snapshot
        .visible_frame
        .is_some_and(|rect| rect_approximately_equal(rect.to_rect(), target.visible_frame));
    let frame_matches = snapshot
        .frame
        .is_some_and(|rect| rect_approximately_equal(rect.to_rect(), target.frame));
    if !visible_matches && !frame_matches {
        return false;
    }

    frame.is_finite()
}

fn rect_approximately_equal(lhs: Rect, rhs: Rect) -> bool {
    let lhs = lhs.standardized();
    let rhs = rhs.standardized();
    (lhs.x - rhs.x).abs() <= RECT_EQUAL_TOLERANCE
        && (lhs.y - rhs.y).abs() <= RECT_EQUAL_TOLERANCE
        && (lhs.width - rhs.width).abs() <= RECT_EQUAL_TOLERANCE
        && (lhs.height - rhs.height).abs() <= RECT_EQUAL_TOLERANCE
}

fn distance_squared(rect: &Rect, point: (f64, f64)) -> f64 {
    let dx = rect.mid_x() - point.0;
    let dy = rect.mid_y() - point.1;
    dx * dx + dy * dy
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(x: f64, y: f64, w: f64, h: f64) -> RectSnapshot {
        RectSnapshot {
            x,
            y,
            width: w,
            height: h,
        }
    }

    fn display(id: u32, frame: Rect, visible: Rect) -> DisplayGeometry {
        DisplayGeometry {
            display_id: Some(id),
            frame,
            visible_frame: visible,
        }
    }

    fn primary() -> DisplayGeometry {
        // 1920x1080 with a 32pt menu-bar inset at the top.
        display(
            1,
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
            Rect::new(0.0, 0.0, 1920.0, 1048.0),
        )
    }

    #[test]
    fn rejects_missing_frame() {
        assert_eq!(resolved_window_frame(None, None, &[primary()], None), None);
    }

    #[test]
    fn rejects_too_small_frame() {
        let tiny = Some(snap(0.0, 0.0, 100.0, 100.0));
        assert_eq!(resolved_window_frame(tiny, None, &[primary()], None), None);
    }

    #[test]
    fn rejects_non_finite_frame() {
        let bad = Some(snap(f64::NAN, 0.0, 800.0, 600.0));
        assert_eq!(resolved_window_frame(bad, None, &[primary()], None), None);
    }

    #[test]
    fn no_displays_trusts_saved_frame() {
        let frame = snap(100.0, 100.0, 800.0, 600.0);
        assert_eq!(
            resolved_window_frame(Some(frame), None, &[], None),
            Some(frame.to_rect())
        );
    }

    #[test]
    fn preserves_exact_frame_when_arrangement_unchanged() {
        let display = primary();
        let snapshot = DisplaySnapshot {
            display_id: Some(1),
            frame: Some(snap(0.0, 0.0, 1920.0, 1080.0)),
            visible_frame: Some(snap(0.0, 0.0, 1920.0, 1048.0)),
        };
        let frame = snap(200.0, 150.0, 1000.0, 700.0);
        assert_eq!(
            resolved_window_frame(Some(frame), Some(&snapshot), &[display], None),
            Some(frame.to_rect())
        );
    }

    #[test]
    fn clamps_when_titlebar_would_be_offscreen() {
        // Same display id but arrangement differs (no exact-preserve); the saved
        // frame sits with its top strip above the visible area → clamp in.
        let display = primary();
        let snapshot = DisplaySnapshot {
            display_id: Some(1),
            frame: Some(snap(0.0, 0.0, 1280.0, 800.0)), // differs from current
            visible_frame: Some(snap(0.0, 0.0, 1280.0, 770.0)),
        };
        // Window pushed up so its top is above the 1048 visible top.
        let frame = snap(100.0, 1000.0, 800.0, 600.0);
        let resolved = resolved_window_frame(Some(frame), Some(&snapshot), &[display], None)
            .expect("resolved");
        assert!(resolved.max_y() <= display.visible_frame.max_y() + 1.0);
        assert!(resolved.min_y() >= display.visible_frame.min_y());
    }

    #[test]
    fn preserves_frame_with_grabbable_titlebar() {
        // Same id, arrangement differs, but the frame is comfortably inside the
        // visible area → accessible titlebar → preserved unchanged.
        let display = primary();
        let snapshot = DisplaySnapshot {
            display_id: Some(1),
            frame: Some(snap(0.0, 0.0, 1280.0, 800.0)),
            visible_frame: Some(snap(0.0, 0.0, 1280.0, 770.0)),
        };
        let frame = snap(300.0, 200.0, 800.0, 600.0);
        assert_eq!(
            resolved_window_frame(Some(frame), Some(&snapshot), &[display], None),
            Some(frame.to_rect())
        );
    }

    #[test]
    fn picks_target_by_largest_overlap_when_id_absent() {
        let left = display(
            1,
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
            Rect::new(0.0, 0.0, 1000.0, 1000.0),
        );
        let right = display(
            2,
            Rect::new(1000.0, 0.0, 1000.0, 1000.0),
            Rect::new(1000.0, 0.0, 1000.0, 1000.0),
        );
        // Saved reference overlaps the right display far more.
        let snapshot = DisplaySnapshot {
            display_id: None,
            frame: None,
            visible_frame: Some(snap(1200.0, 100.0, 600.0, 600.0)),
        };
        let frame = snap(1200.0, 100.0, 600.0, 600.0);
        let resolved = resolved_window_frame(Some(frame), Some(&snapshot), &[left, right], None)
            .expect("resolved");
        // It should land on the right display.
        assert!(resolved.min_x() >= right.visible_frame.min_x());
    }

    #[test]
    fn remaps_proportionally_when_target_does_not_intersect() {
        // Saved on a 1000x1000 display at its center; current target is a
        // differently-sized display the frame does not intersect → proportional remap.
        let saved_display = snap(0.0, 0.0, 1000.0, 1000.0);
        let target = display(
            2,
            Rect::new(5000.0, 0.0, 2000.0, 2000.0),
            Rect::new(5000.0, 0.0, 2000.0, 2000.0),
        );
        let snapshot = DisplaySnapshot {
            display_id: Some(2), // matches target id → target chosen
            frame: Some(saved_display),
            visible_frame: Some(saved_display),
        };
        let frame = snap(250.0, 250.0, 500.0, 500.0); // centered quarter on the saved display
        let resolved =
            resolved_window_frame(Some(frame), Some(&snapshot), &[target], None).expect("resolved");
        // Proportionally that's a centered quarter of the 2000x2000 target.
        assert!(resolved.min_x() >= target.visible_frame.min_x());
        assert!(resolved.max_x() <= target.visible_frame.max_x() + 1.0);
        assert!((resolved.width - 1000.0).abs() < 1.0, "got {resolved:?}");
    }

    #[test]
    fn clamps_onto_any_intersecting_display_when_no_snapshot() {
        let display = primary();
        // No display snapshot → display_for returns None; frame intersects the
        // primary's visible area → clamp there.
        let frame = snap(-200.0, 100.0, 800.0, 600.0);
        let resolved =
            resolved_window_frame(Some(frame), None, &[display], None).expect("resolved");
        assert!(resolved.min_x() >= display.visible_frame.min_x());
    }

    #[test]
    fn falls_back_to_centered_when_stranded_without_reference() {
        let fallback = display(
            9,
            Rect::new(0.0, 0.0, 1920.0, 1080.0),
            Rect::new(0.0, 0.0, 1920.0, 1048.0),
        );
        // No available display intersects, no snapshot reference → center on fallback.
        let far = display(
            2,
            Rect::new(9000.0, 9000.0, 800.0, 600.0),
            Rect::new(9000.0, 9000.0, 800.0, 600.0),
        );
        let frame = snap(-5000.0, -5000.0, 800.0, 600.0);
        let resolved =
            resolved_window_frame(Some(frame), None, &[far], Some(&fallback)).expect("resolved");
        // Centered on the fallback's visible area.
        let expected_x = fallback.visible_frame.mid_x() - 400.0;
        assert!(
            (resolved.min_x() - expected_x).abs() < 1.0,
            "got {resolved:?}"
        );
    }

    #[test]
    fn startup_primary_falls_through_to_fallback_frame() {
        let display = primary();
        // Primary frame is too small (rejected) → use the fallback frame.
        let bad_primary = snap(0.0, 0.0, 50.0, 50.0);
        let good_fallback = snap(100.0, 100.0, 800.0, 600.0);
        let resolved = resolved_startup_primary_window_frame(
            Some(bad_primary),
            None,
            Some(good_fallback),
            None,
            &[display],
            None,
        )
        .expect("resolved");
        // Fallback frame is fully on-screen with a grabbable titlebar → preserved
        // (no snapshot id match, but it intersects and stays accessible).
        assert_eq!(resolved, good_fallback.to_rect());
    }
}
