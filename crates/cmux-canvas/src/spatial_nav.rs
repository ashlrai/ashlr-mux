//! Directional focus movement. Port of `CanvasSpatialNavigator.swift`.

use crate::geometry::{CanvasDirection, CanvasRect};
use crate::layout::CanvasLayout;
use crate::pane::CanvasPaneID;

/// Answers "which pane is to the left/right/above/below the focused pane".
#[derive(Clone, Copy, Debug, Default)]
pub struct CanvasSpatialNavigator;

impl CanvasSpatialNavigator {
    /// Orthogonal misalignment weight for candidates overlapping the focused
    /// pane's orthogonal extent.
    const OVERLAPPING_WEIGHT: f64 = 0.25;
    /// Orthogonal misalignment weight for non-overlapping candidates.
    const NON_OVERLAPPING_WEIGHT: f64 = 3.0;

    /// Creates a navigator.
    pub fn new() -> CanvasSpatialNavigator {
        CanvasSpatialNavigator
    }

    /// Finds the nearest pane in a direction, or `None` when none lies that way
    /// (or `from` is not on the canvas).
    pub fn pane(
        &self,
        direction: CanvasDirection,
        from: CanvasPaneID,
        layout: &CanvasLayout,
    ) -> Option<CanvasPaneID> {
        let origin = layout.frame(from)?;

        let mut best: Option<(CanvasPaneID, f64)> = None;
        for pane in layout.panes() {
            if pane.id == from {
                continue;
            }
            let score = match Self::score(&pane.frame, &origin, direction) {
                Some(score) => score,
                None => continue,
            };
            match best {
                Some((best_id, best_score)) => {
                    if score < best_score || (score == best_score && pane.id < best_id) {
                        best = Some((pane.id, score));
                    }
                }
                None => best = Some((pane.id, score)),
            }
        }
        best.map(|(id, _)| id)
    }

    fn score(candidate: &CanvasRect, origin: &CanvasRect, direction: CanvasDirection) -> Option<f64> {
        let axis_distance: f64;
        let orthogonal_distance: f64;
        let overlaps: bool;

        match direction {
            CanvasDirection::Left | CanvasDirection::Right => {
                axis_distance = if direction == CanvasDirection::Right {
                    candidate.mid_x() - origin.mid_x()
                } else {
                    origin.mid_x() - candidate.mid_x()
                };
                orthogonal_distance = (candidate.mid_y() - origin.mid_y()).abs();
                overlaps = candidate.min_y() < origin.max_y() && origin.min_y() < candidate.max_y();
            }
            CanvasDirection::Up | CanvasDirection::Down => {
                axis_distance = if direction == CanvasDirection::Down {
                    candidate.mid_y() - origin.mid_y()
                } else {
                    origin.mid_y() - candidate.mid_y()
                };
                orthogonal_distance = (candidate.mid_x() - origin.mid_x()).abs();
                overlaps = candidate.min_x() < origin.max_x() && origin.min_x() < candidate.max_x();
            }
        }

        if axis_distance > 0.5 {
            let weight = if overlaps {
                Self::OVERLAPPING_WEIGHT
            } else {
                Self::NON_OVERLAPPING_WEIGHT
            };
            Some(axis_distance + weight * orthogonal_distance)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    // Port of CanvasSpatialNavigatorTests.swift (6 @Test cases).
    use super::*;
    use crate::pane::{CanvasPane, Uuid};

    fn navigator() -> CanvasSpatialNavigator {
        CanvasSpatialNavigator::new()
    }
    fn id(value: u8) -> CanvasPaneID {
        let mut bytes = [0u8; 16];
        bytes[0] = value;
        CanvasPaneID::new(Uuid::from_bytes(bytes))
    }
    fn layout(frames: &[(u8, CanvasRect)]) -> CanvasLayout {
        let mut layout = CanvasLayout::default();
        for (value, frame) in frames {
            layout.add(CanvasPane::new(id(*value), *frame));
        }
        layout
    }

    #[test]
    fn finds_nearest_in_each_direction() {
        let layout = layout(&[
            (0, CanvasRect::new(300.0, 300.0, 100.0, 100.0)),
            (1, CanvasRect::new(100.0, 300.0, 100.0, 100.0)),
            (2, CanvasRect::new(500.0, 300.0, 100.0, 100.0)),
            (3, CanvasRect::new(300.0, 100.0, 100.0, 100.0)),
            (4, CanvasRect::new(300.0, 500.0, 100.0, 100.0)),
        ]);
        assert_eq!(navigator().pane(CanvasDirection::Left, id(0), &layout), Some(id(1)));
        assert_eq!(navigator().pane(CanvasDirection::Right, id(0), &layout), Some(id(2)));
        assert_eq!(navigator().pane(CanvasDirection::Up, id(0), &layout), Some(id(3)));
        assert_eq!(navigator().pane(CanvasDirection::Down, id(0), &layout), Some(id(4)));
    }

    #[test]
    fn prefers_overlapping_band_over_closer_misaligned_pane() {
        let layout = layout(&[
            (0, CanvasRect::new(0.0, 0.0, 100.0, 100.0)),
            (1, CanvasRect::new(150.0, 400.0, 100.0, 100.0)),
            (2, CanvasRect::new(300.0, 20.0, 100.0, 100.0)),
        ]);
        assert_eq!(navigator().pane(CanvasDirection::Right, id(0), &layout), Some(id(2)));
    }

    #[test]
    fn returns_nil_when_no_pane_in_direction() {
        let layout = layout(&[
            (0, CanvasRect::new(0.0, 0.0, 100.0, 100.0)),
            (1, CanvasRect::new(200.0, 0.0, 100.0, 100.0)),
        ]);
        assert_eq!(navigator().pane(CanvasDirection::Left, id(0), &layout), None);
        assert_eq!(navigator().pane(CanvasDirection::Right, id(1), &layout), None);
    }

    #[test]
    fn returns_nil_for_unknown_origin() {
        let layout = layout(&[(0, CanvasRect::new(0.0, 0.0, 100.0, 100.0))]);
        assert_eq!(navigator().pane(CanvasDirection::Left, id(9), &layout), None);
    }

    #[test]
    fn tie_breaks_deterministically_by_id() {
        let layout = layout(&[
            (0, CanvasRect::new(0.0, 0.0, 100.0, 100.0)),
            (5, CanvasRect::new(200.0, 110.0, 100.0, 100.0)),
            (3, CanvasRect::new(200.0, -110.0, 100.0, 100.0)),
        ]);
        assert_eq!(navigator().pane(CanvasDirection::Right, id(0), &layout), Some(id(3)));
    }

    #[test]
    fn side_by_side_panes_navigate_despite_center_proximity() {
        let layout = layout(&[
            (0, CanvasRect::new(0.0, 0.0, 600.0, 100.0)),
            (1, CanvasRect::new(616.0, 0.0, 100.0, 100.0)),
        ]);
        assert_eq!(navigator().pane(CanvasDirection::Right, id(0), &layout), Some(id(1)));
        assert_eq!(navigator().pane(CanvasDirection::Left, id(1), &layout), Some(id(0)));
    }
}
