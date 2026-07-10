//! Chooses where a new pane goes on the canvas. Port of `CanvasPlacer.swift`.

use crate::geometry::{CanvasDirection, CanvasMetrics, CanvasPoint, CanvasRect, CanvasSize};

/// Chooses the frame for a new pane without moving existing panes.
#[derive(Clone, Copy, Debug)]
pub struct CanvasPlacer {
    /// The metrics supplying the canonical gap.
    pub metrics: CanvasMetrics,
}

impl CanvasPlacer {
    /// Bound on the outward scan so placement stays O(existing panes).
    const SCAN_COLUMNS: usize = 16;
    const SCAN_ROWS: usize = 8;

    /// Creates a placer.
    pub fn new(metrics: CanvasMetrics) -> CanvasPlacer {
        CanvasPlacer { metrics }
    }

    /// Computes the frame for a new pane, at least `gap` away from every
    /// existing pane.
    pub fn frame_for_new_pane(
        &self,
        size: CanvasSize,
        anchor: Option<CanvasRect>,
        existing: &[CanvasRect],
        preferred_direction: Option<CanvasDirection>,
    ) -> CanvasRect {
        let gap = self.metrics.gap;
        if existing.is_empty() {
            let origin = anchor.map(|a| a.origin()).unwrap_or(CanvasPoint::ZERO);
            return CanvasRect::from_origin_size(origin, size);
        }
        let anchor = match anchor {
            Some(anchor) => anchor,
            None => return self.frame_right_of_content(size, existing),
        };

        let neighbors: [(CanvasDirection, CanvasRect); 4] = [
            (
                CanvasDirection::Right,
                CanvasRect::new(
                    anchor.max_x() + gap,
                    anchor.min_y(),
                    size.width,
                    size.height,
                ),
            ),
            (
                CanvasDirection::Down,
                CanvasRect::new(
                    anchor.min_x(),
                    anchor.max_y() + gap,
                    size.width,
                    size.height,
                ),
            ),
            (
                CanvasDirection::Left,
                CanvasRect::new(
                    anchor.min_x() - gap - size.width,
                    anchor.min_y(),
                    size.width,
                    size.height,
                ),
            ),
            (
                CanvasDirection::Up,
                CanvasRect::new(
                    anchor.min_x(),
                    anchor.min_y() - gap - size.height,
                    size.width,
                    size.height,
                ),
            ),
        ];

        let ordered_neighbors: Vec<CanvasRect> = match preferred_direction.and_then(|dir| {
            neighbors
                .iter()
                .find(|(d, _)| *d == dir)
                .map(|(_, f)| (dir, *f))
        }) {
            Some((dir, preferred)) => {
                let mut ordered = vec![preferred];
                ordered.extend(neighbors.iter().filter(|(d, _)| *d != dir).map(|(_, f)| *f));
                ordered
            }
            None => neighbors.iter().map(|(_, f)| *f).collect(),
        };
        for candidate in &ordered_neighbors {
            if self.is_free(candidate, existing) {
                return *candidate;
            }
        }

        // Scan a bounded grid rightward/downward from the anchor before falling back.
        for row in 0..Self::SCAN_ROWS {
            let y = anchor.min_y() + (row as f64) * (size.height + gap);
            for column in 0..Self::SCAN_COLUMNS {
                let x = anchor.max_x() + gap + (column as f64) * (size.width + gap);
                let candidate = CanvasRect::new(x, y, size.width, size.height);
                if self.is_free(&candidate, existing) {
                    return candidate;
                }
            }
        }
        self.frame_right_of_content(size, existing)
    }

    fn is_free(&self, candidate: &CanvasRect, existing: &[CanvasRect]) -> bool {
        // Inset slightly so a candidate exactly one gap away from a neighbor counts as free.
        let probe = candidate.expanded_by(self.metrics.gap - 0.5);
        !existing.iter().any(|e| probe.intersects(e))
    }

    fn frame_right_of_content(&self, size: CanvasSize, existing: &[CanvasRect]) -> CanvasRect {
        let bounds = existing
            .iter()
            .skip(1)
            .fold(existing[0], |acc, r| acc.union(r));
        CanvasRect::new(
            bounds.max_x() + self.metrics.gap,
            bounds.min_y(),
            size.width,
            size.height,
        )
    }
}

#[cfg(test)]
mod tests {
    // Port of CanvasPlacerTests.swift (6 @Test cases).
    use super::*;

    fn metrics() -> CanvasMetrics {
        CanvasMetrics::new(16.0, 8.0, CanvasMetrics::DEFAULT_MIN_PANE_SIZE)
    }
    fn placer() -> CanvasPlacer {
        CanvasPlacer::new(metrics())
    }
    fn size() -> CanvasSize {
        CanvasSize::new(300.0, 200.0)
    }

    #[test]
    fn empty_canvas_places_at_origin_or_anchor() {
        assert_eq!(
            placer().frame_for_new_pane(size(), None, &[], None),
            CanvasRect::new(0.0, 0.0, 300.0, 200.0)
        );
        let anchor = CanvasRect::new(50.0, 60.0, 10.0, 10.0);
        assert_eq!(
            placer()
                .frame_for_new_pane(size(), Some(anchor), &[], None)
                .origin(),
            CanvasPoint::new(50.0, 60.0)
        );
    }

    #[test]
    fn places_right_of_anchor_at_gap() {
        let anchor = CanvasRect::new(0.0, 0.0, 300.0, 200.0);
        let frame = placer().frame_for_new_pane(size(), Some(anchor), &[anchor], None);
        assert_eq!(frame, CanvasRect::new(316.0, 0.0, 300.0, 200.0));
    }

    #[test]
    fn falls_through_right_below_left_above() {
        let anchor = CanvasRect::new(1000.0, 1000.0, 300.0, 200.0);
        let right = CanvasRect::new(1316.0, 1000.0, 300.0, 200.0);
        let below = CanvasRect::new(1000.0, 1216.0, 300.0, 200.0);
        let frame =
            placer().frame_for_new_pane(size(), Some(anchor), &[anchor, right, below], None);
        assert_eq!(frame, CanvasRect::new(684.0, 1000.0, 300.0, 200.0));
    }

    #[test]
    fn new_pane_keeps_gap_distance_from_all_panes() {
        let anchor = CanvasRect::new(0.0, 0.0, 300.0, 200.0);
        let blocker = CanvasRect::new(320.0, 10.0, 300.0, 200.0);
        let frame = placer().frame_for_new_pane(size(), Some(anchor), &[anchor, blocker], None);
        for existing in [anchor, blocker] {
            assert!(!frame.expanded_by(metrics().gap - 0.5).intersects(&existing));
        }
    }

    #[test]
    fn no_anchor_places_right_of_content() {
        let existing = [
            CanvasRect::new(0.0, 40.0, 300.0, 200.0),
            CanvasRect::new(350.0, 0.0, 300.0, 200.0),
        ];
        let frame = placer().frame_for_new_pane(size(), None, &existing, None);
        assert_eq!(frame, CanvasRect::new(666.0, 0.0, 300.0, 200.0));
    }

    #[test]
    fn crowded_neighborhood_falls_back_outside_content() {
        // Surround the anchor completely so every nearby slot is taken.
        let anchor = CanvasRect::new(0.0, 0.0, 100.0, 100.0);
        let mut existing = vec![anchor];
        let mut dx = -1300.0;
        while dx <= 6000.0 {
            let mut dy = -1300.0;
            while dy <= 2600.0 {
                if !(dx == 0.0 && dy == 0.0) {
                    existing.push(CanvasRect::new(dx, dy, 100.0, 100.0));
                }
                dy += 100.0;
            }
            dx += 100.0;
        }
        let frame = placer().frame_for_new_pane(size(), Some(anchor), &existing, None);
        for rect in &existing {
            assert!(!frame.expanded_by(metrics().gap - 0.5).intersects(rect));
        }
    }
}
