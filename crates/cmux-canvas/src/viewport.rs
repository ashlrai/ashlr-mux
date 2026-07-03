//! Scroll-to-reveal targets and overview magnification. Port of `CanvasViewportMath.swift`.

use std::ops::RangeInclusive;

use crate::geometry::{CanvasPoint, CanvasRect, CanvasSize};

/// Pure viewport math: scroll-to-reveal targets and overview magnification.
#[derive(Clone, Copy, Debug, Default)]
pub struct CanvasViewportMath;

impl CanvasViewportMath {
    /// Creates the math helper.
    pub fn new() -> CanvasViewportMath {
        CanvasViewportMath
    }

    /// Computes the minimal scroll origin that brings a rect into view.
    ///
    /// A target already visible (with margin) returns the current origin
    /// unchanged. A target larger than the viewport aligns its top-left corner
    /// (plus margin).
    pub fn origin_to_reveal(
        &self,
        target: CanvasRect,
        viewport_origin: CanvasPoint,
        viewport_size: CanvasSize,
        margin: f64,
    ) -> CanvasPoint {
        CanvasPoint::new(
            Self::axis_origin_to_reveal(
                target.min_x() - margin,
                target.max_x() + margin,
                viewport_origin.x,
                viewport_size.width,
            ),
            Self::axis_origin_to_reveal(
                target.min_y() - margin,
                target.max_y() + margin,
                viewport_origin.y,
                viewport_size.height,
            ),
        )
    }

    /// Computes the magnification that fits a content rect inside a viewport.
    ///
    /// The result is clamped into `range`. Degenerate content returns the
    /// range's upper bound clamped to `1`.
    pub fn magnification_to_fit(
        &self,
        content: CanvasRect,
        viewport_size: CanvasSize,
        padding: f64,
        range: RangeInclusive<f64>,
    ) -> f64 {
        let padded_width = content.width + padding * 2.0;
        let padded_height = content.height + padding * 2.0;
        if !(padded_width > 0.0
            && padded_height > 0.0
            && viewport_size.width > 0.0
            && viewport_size.height > 0.0)
        {
            return 1.0_f64.min(*range.end());
        }
        let fit = (viewport_size.width / padded_width).min(viewport_size.height / padded_height);
        fit.max(*range.start()).min(*range.end())
    }

    fn axis_origin_to_reveal(
        target_min: f64,
        target_max: f64,
        viewport_min: f64,
        viewport_length: f64,
    ) -> f64 {
        if target_max - target_min >= viewport_length {
            return target_min;
        }
        if target_min < viewport_min {
            return target_min;
        }
        if target_max > viewport_min + viewport_length {
            return target_max - viewport_length;
        }
        viewport_min
    }
}

#[cfg(test)]
mod tests {
    // Port of CanvasViewportMathTests.swift (7 @Test cases).
    use super::*;

    fn math() -> CanvasViewportMath {
        CanvasViewportMath::new()
    }
    fn viewport_size() -> CanvasSize {
        CanvasSize::new(800.0, 600.0)
    }

    #[test]
    fn visible_target_keeps_origin() {
        let origin = math().origin_to_reveal(
            CanvasRect::new(100.0, 100.0, 200.0, 200.0),
            CanvasPoint::new(0.0, 0.0),
            viewport_size(),
            24.0,
        );
        assert_eq!(origin, CanvasPoint::new(0.0, 0.0));
    }

    #[test]
    fn target_beyond_right_bottom_scrolls_minimally() {
        let origin = math().origin_to_reveal(
            CanvasRect::new(900.0, 700.0, 200.0, 100.0),
            CanvasPoint::new(0.0, 0.0),
            viewport_size(),
            24.0,
        );
        assert_eq!(origin, CanvasPoint::new(1124.0 - 800.0, 824.0 - 600.0));
    }

    #[test]
    fn target_beyond_left_top_scrolls_to_its_origin() {
        let origin = math().origin_to_reveal(
            CanvasRect::new(-500.0, -300.0, 100.0, 100.0),
            CanvasPoint::new(0.0, 0.0),
            viewport_size(),
            24.0,
        );
        assert_eq!(origin, CanvasPoint::new(-524.0, -324.0));
    }

    #[test]
    fn oversized_target_aligns_top_left() {
        let origin = math().origin_to_reveal(
            CanvasRect::new(100.0, 50.0, 2000.0, 3000.0),
            CanvasPoint::new(0.0, 0.0),
            viewport_size(),
            24.0,
        );
        assert_eq!(origin, CanvasPoint::new(76.0, 26.0));
    }

    #[test]
    fn fit_magnification_clamps_to_range() {
        let content = CanvasRect::new(0.0, 0.0, 8000.0, 600.0);
        let fit = math().magnification_to_fit(content, viewport_size(), 40.0, 0.25..=1.0);
        assert_eq!(fit, 0.25);

        let small = CanvasRect::new(0.0, 0.0, 100.0, 100.0);
        let fit_small = math().magnification_to_fit(small, viewport_size(), 40.0, 0.25..=1.0);
        assert_eq!(fit_small, 1.0);
    }

    #[test]
    fn fit_magnification_exact_fit() {
        let content = CanvasRect::new(0.0, 0.0, 1520.0, 600.0);
        let fit = math().magnification_to_fit(content, viewport_size(), 40.0, 0.1..=1.0);
        assert_eq!(fit, 0.5);
    }

    #[test]
    fn degenerate_content_returns_safe_magnification() {
        let fit = math().magnification_to_fit(
            CanvasRect::new(0.0, 0.0, 0.0, 0.0),
            viewport_size(),
            0.0,
            0.25..=4.0,
        );
        assert_eq!(fit, 1.0);
    }
}
