//! Snaps dragged and resized pane frames to neighbor edges and the canonical
//! gap. Port of `CanvasSnapEngine.swift`.

use std::cmp::Ordering;

use crate::geometry::{
    CanvasGuide, CanvasGuideAxis, CanvasMetrics, CanvasRect, CanvasResizeEdges, CanvasSnapResult,
};

/// One snap candidate on a single axis.
struct Candidate {
    /// Distance the proposed frame must move along the axis to snap.
    delta: f64,
    /// Where the rendered guide line sits after snapping.
    guide_position: f64,
    /// Lower numbers win ties at equal distance.
    priority: i32,
    /// The neighbor that produced the candidate, for guide span computation.
    neighbor: CanvasRect,
}

/// Snaps dragged and resized pane frames. Port of `CanvasSnapEngine`.
#[derive(Clone, Copy, Debug)]
pub struct CanvasSnapEngine {
    /// The metrics supplying the gap and snap threshold.
    pub metrics: CanvasMetrics,
}

impl CanvasSnapEngine {
    /// Creates a snap engine.
    pub fn new(metrics: CanvasMetrics) -> CanvasSnapEngine {
        CanvasSnapEngine { metrics }
    }

    /// Snaps a frame being moved (both edges of each axis translate together).
    pub fn snap_for_move(
        &self,
        proposed: CanvasRect,
        neighbors: &[CanvasRect],
    ) -> CanvasSnapResult {
        let mut frame = proposed;
        let mut guides: Vec<CanvasGuide> = Vec::new();

        if let Some(best) = self.best_candidate(self.move_candidates_x(&proposed, neighbors)) {
            frame.x += best.delta;
            guides.push(Self::vertical_guide(
                best.guide_position,
                &frame,
                &best.neighbor,
            ));
        }
        if let Some(best) = self.best_candidate(self.move_candidates_y(&proposed, neighbors)) {
            frame.y += best.delta;
            guides.push(Self::horizontal_guide(
                best.guide_position,
                &frame,
                &best.neighbor,
            ));
        }
        CanvasSnapResult::new(frame, guides)
    }

    /// Snaps and clamps a frame being resized. Only the edges named in `edges`
    /// move; opposite edges stay fixed. After snapping, the frame is clamped to
    /// `min_pane_size` by moving the dragged edge back; a snap undone by
    /// clamping drops its guide.
    pub fn snap_for_resize(
        &self,
        proposed: CanvasRect,
        edges: CanvasResizeEdges,
        neighbors: &[CanvasRect],
    ) -> CanvasSnapResult {
        let mut frame = proposed;
        let mut guides: Vec<CanvasGuide> = Vec::new();
        let gap = self.metrics.gap;

        if edges.contains(CanvasResizeEdges::LEFT) {
            let align: Vec<(f64, CanvasRect)> = neighbors.iter().map(|n| (n.min_x(), *n)).collect();
            let gaps: Vec<(f64, CanvasRect)> =
                neighbors.iter().map(|n| (n.max_x() + gap, *n)).collect();
            if let Some(best) =
                self.best_candidate(Self::edge_candidates(proposed.min_x(), &align, &gaps))
            {
                frame.x = proposed.min_x() + best.delta;
                frame.width = proposed.max_x() - frame.x;
                guides.push(Self::vertical_guide(
                    best.guide_position,
                    &frame,
                    &best.neighbor,
                ));
            }
            if frame.width < self.metrics.min_pane_size.width {
                frame.x = frame.max_x() - self.metrics.min_pane_size.width;
                frame.width = self.metrics.min_pane_size.width;
                guides.retain(|g| g.axis != CanvasGuideAxis::Vertical);
            }
        } else if edges.contains(CanvasResizeEdges::RIGHT) {
            let align: Vec<(f64, CanvasRect)> = neighbors.iter().map(|n| (n.max_x(), *n)).collect();
            let gaps: Vec<(f64, CanvasRect)> =
                neighbors.iter().map(|n| (n.min_x() - gap, *n)).collect();
            if let Some(best) =
                self.best_candidate(Self::edge_candidates(proposed.max_x(), &align, &gaps))
            {
                frame.width = proposed.max_x() + best.delta - frame.x;
                guides.push(Self::vertical_guide(
                    best.guide_position,
                    &frame,
                    &best.neighbor,
                ));
            }
            if frame.width < self.metrics.min_pane_size.width {
                frame.width = self.metrics.min_pane_size.width;
                guides.retain(|g| g.axis != CanvasGuideAxis::Vertical);
            }
        }

        if edges.contains(CanvasResizeEdges::TOP) {
            let align: Vec<(f64, CanvasRect)> = neighbors.iter().map(|n| (n.min_y(), *n)).collect();
            let gaps: Vec<(f64, CanvasRect)> =
                neighbors.iter().map(|n| (n.max_y() + gap, *n)).collect();
            if let Some(best) =
                self.best_candidate(Self::edge_candidates(proposed.min_y(), &align, &gaps))
            {
                frame.y = proposed.min_y() + best.delta;
                frame.height = proposed.max_y() - frame.y;
                guides.push(Self::horizontal_guide(
                    best.guide_position,
                    &frame,
                    &best.neighbor,
                ));
            }
            if frame.height < self.metrics.min_pane_size.height {
                frame.y = frame.max_y() - self.metrics.min_pane_size.height;
                frame.height = self.metrics.min_pane_size.height;
                guides.retain(|g| g.axis != CanvasGuideAxis::Horizontal);
            }
        } else if edges.contains(CanvasResizeEdges::BOTTOM) {
            let align: Vec<(f64, CanvasRect)> = neighbors.iter().map(|n| (n.max_y(), *n)).collect();
            let gaps: Vec<(f64, CanvasRect)> =
                neighbors.iter().map(|n| (n.min_y() - gap, *n)).collect();
            if let Some(best) =
                self.best_candidate(Self::edge_candidates(proposed.max_y(), &align, &gaps))
            {
                frame.height = proposed.max_y() + best.delta - frame.y;
                guides.push(Self::horizontal_guide(
                    best.guide_position,
                    &frame,
                    &best.neighbor,
                ));
            }
            if frame.height < self.metrics.min_pane_size.height {
                frame.height = self.metrics.min_pane_size.height;
                guides.retain(|g| g.axis != CanvasGuideAxis::Horizontal);
            }
        }

        CanvasSnapResult::new(frame, guides)
    }

    fn best_candidate(&self, candidates: Vec<Candidate>) -> Option<Candidate> {
        let threshold = self.metrics.snap_threshold;
        candidates
            .into_iter()
            .filter(|c| c.delta.abs() <= threshold)
            .min_by(|lhs, rhs| {
                if lhs.delta.abs() != rhs.delta.abs() {
                    lhs.delta
                        .abs()
                        .partial_cmp(&rhs.delta.abs())
                        .unwrap_or(Ordering::Equal)
                } else {
                    lhs.priority.cmp(&rhs.priority)
                }
            })
    }

    fn move_candidates_x(&self, rect: &CanvasRect, neighbors: &[CanvasRect]) -> Vec<Candidate> {
        let gap = self.metrics.gap;
        let mut candidates: Vec<Candidate> = Vec::with_capacity(neighbors.len() * 5);
        for neighbor in neighbors {
            candidates.push(Candidate {
                delta: neighbor.min_x() - rect.min_x(),
                guide_position: neighbor.min_x(),
                priority: 0,
                neighbor: *neighbor,
            });
            candidates.push(Candidate {
                delta: neighbor.max_x() - rect.max_x(),
                guide_position: neighbor.max_x(),
                priority: 0,
                neighbor: *neighbor,
            });
            candidates.push(Candidate {
                delta: neighbor.max_x() + gap - rect.min_x(),
                guide_position: neighbor.max_x() + gap,
                priority: 1,
                neighbor: *neighbor,
            });
            candidates.push(Candidate {
                delta: neighbor.min_x() - gap - rect.max_x(),
                guide_position: neighbor.min_x() - gap,
                priority: 1,
                neighbor: *neighbor,
            });
            candidates.push(Candidate {
                delta: neighbor.mid_x() - rect.mid_x(),
                guide_position: neighbor.mid_x(),
                priority: 2,
                neighbor: *neighbor,
            });
        }
        candidates
    }

    fn move_candidates_y(&self, rect: &CanvasRect, neighbors: &[CanvasRect]) -> Vec<Candidate> {
        let gap = self.metrics.gap;
        let mut candidates: Vec<Candidate> = Vec::with_capacity(neighbors.len() * 5);
        for neighbor in neighbors {
            candidates.push(Candidate {
                delta: neighbor.min_y() - rect.min_y(),
                guide_position: neighbor.min_y(),
                priority: 0,
                neighbor: *neighbor,
            });
            candidates.push(Candidate {
                delta: neighbor.max_y() - rect.max_y(),
                guide_position: neighbor.max_y(),
                priority: 0,
                neighbor: *neighbor,
            });
            candidates.push(Candidate {
                delta: neighbor.max_y() + gap - rect.min_y(),
                guide_position: neighbor.max_y() + gap,
                priority: 1,
                neighbor: *neighbor,
            });
            candidates.push(Candidate {
                delta: neighbor.min_y() - gap - rect.max_y(),
                guide_position: neighbor.min_y() - gap,
                priority: 1,
                neighbor: *neighbor,
            });
            candidates.push(Candidate {
                delta: neighbor.mid_y() - rect.mid_y(),
                guide_position: neighbor.mid_y(),
                priority: 2,
                neighbor: *neighbor,
            });
        }
        candidates
    }

    fn edge_candidates(
        edge: f64,
        align_targets: &[(f64, CanvasRect)],
        gap_targets: &[(f64, CanvasRect)],
    ) -> Vec<Candidate> {
        let mut candidates: Vec<Candidate> =
            Vec::with_capacity(align_targets.len() + gap_targets.len());
        for (target, neighbor) in align_targets {
            candidates.push(Candidate {
                delta: target - edge,
                guide_position: *target,
                priority: 0,
                neighbor: *neighbor,
            });
        }
        for (target, neighbor) in gap_targets {
            candidates.push(Candidate {
                delta: target - edge,
                guide_position: *target,
                priority: 1,
                neighbor: *neighbor,
            });
        }
        candidates
    }

    fn vertical_guide(position: f64, snapped: &CanvasRect, neighbor: &CanvasRect) -> CanvasGuide {
        let lower = snapped.min_y().min(neighbor.min_y());
        let upper = snapped.max_y().max(neighbor.max_y());
        CanvasGuide::new(
            CanvasGuideAxis::Vertical,
            position,
            lower..=lower.max(upper),
        )
    }

    fn horizontal_guide(position: f64, snapped: &CanvasRect, neighbor: &CanvasRect) -> CanvasGuide {
        let lower = snapped.min_x().min(neighbor.min_x());
        let upper = snapped.max_x().max(neighbor.max_x());
        CanvasGuide::new(
            CanvasGuideAxis::Horizontal,
            position,
            lower..=lower.max(upper),
        )
    }
}

#[cfg(test)]
mod tests {
    // Port of CanvasSnapEngineTests.swift (15 @Test cases).
    use super::*;
    use crate::geometry::CanvasSize;

    fn metrics() -> CanvasMetrics {
        CanvasMetrics::new(16.0, 8.0, CanvasSize::new(200.0, 120.0))
    }
    fn engine() -> CanvasSnapEngine {
        CanvasSnapEngine::new(metrics())
    }
    fn neighbor() -> CanvasRect {
        CanvasRect::new(0.0, 0.0, 300.0, 200.0)
    }

    // MARK: Move

    #[test]
    fn move_snaps_left_edge_to_neighbor_left() {
        let proposed = CanvasRect::new(5.0, 400.0, 300.0, 200.0);
        let result = engine().snap_for_move(proposed, &[neighbor()]);
        assert_eq!(result.frame.x, 0.0);
        assert_eq!(result.frame.y, 400.0);
        assert_eq!(result.guides.len(), 1);
        assert_eq!(result.guides[0].axis, CanvasGuideAxis::Vertical);
        assert_eq!(result.guides[0].position, 0.0);
        assert_eq!(result.guides[0].span, 0.0..=600.0);
    }

    #[test]
    fn move_snaps_to_gap_adjacency() {
        let proposed = CanvasRect::new(312.0, 500.0, 300.0, 200.0);
        let result = engine().snap_for_move(proposed, &[neighbor()]);
        assert_eq!(result.frame.x, 316.0);
        assert_eq!(result.frame.y, 500.0);
        assert_eq!(result.guides.len(), 1);
        assert_eq!(result.guides[0].position, 316.0);
    }

    #[test]
    fn move_snaps_centers_on_both_axes() {
        let proposed = CanvasRect::new(3.0, 102.0, 300.0, 200.0);
        let result = engine().snap_for_move(proposed, &[neighbor()]);
        assert_eq!(result.frame.x, 0.0);
        assert_eq!(result.frame.y, 102.0);
        assert_eq!(result.guides.len(), 1);
    }

    #[test]
    fn move_beyond_threshold_does_not_snap() {
        let proposed = CanvasRect::new(9.0, 400.0, 300.0, 200.0);
        let result = engine().snap_for_move(proposed, &[neighbor()]);
        assert_eq!(result.frame, proposed);
        assert!(result.guides.is_empty());
    }

    #[test]
    fn move_at_exact_threshold_snaps() {
        let proposed = CanvasRect::new(8.0, 400.0, 300.0, 200.0);
        let result = engine().snap_for_move(proposed, &[neighbor()]);
        assert_eq!(result.frame.x, 0.0);
    }

    #[test]
    fn move_prefers_edge_alignment_over_center_on_tie() {
        let other = CanvasRect::new(100.0, 0.0, 100.0, 100.0);
        let proposed = CanvasRect::new(98.0, 300.0, 100.0, 100.0);
        let result = engine().snap_for_move(proposed, &[other]);
        assert_eq!(result.frame.x, 100.0);
        assert_eq!(result.guides[0].position, 100.0);
    }

    #[test]
    fn move_with_no_neighbors_returns_proposed() {
        let proposed = CanvasRect::new(42.0, 42.0, 300.0, 200.0);
        let result = engine().snap_for_move(proposed, &[]);
        assert_eq!(result.frame, proposed);
        assert!(result.guides.is_empty());
    }

    #[test]
    fn move_snaps_both_axes_independently() {
        let proposed = CanvasRect::new(314.0, 3.0, 300.0, 200.0);
        let result = engine().snap_for_move(proposed, &[neighbor()]);
        assert_eq!(result.frame.x, 316.0);
        assert_eq!(result.frame.y, 0.0);
        assert_eq!(result.guides.len(), 2);
        assert!(result
            .guides
            .iter()
            .any(|g| g.axis == CanvasGuideAxis::Vertical));
        assert!(result
            .guides
            .iter()
            .any(|g| g.axis == CanvasGuideAxis::Horizontal));
    }

    // MARK: Resize

    #[test]
    fn resize_right_edge_snaps_to_neighbor_right() {
        let proposed = CanvasRect::new(0.0, 400.0, 295.0, 200.0);
        let result = engine().snap_for_resize(proposed, CanvasResizeEdges::RIGHT, &[neighbor()]);
        assert_eq!(result.frame, CanvasRect::new(0.0, 400.0, 300.0, 200.0));
        assert_eq!(result.guides.len(), 1);
        assert_eq!(result.guides[0].position, 300.0);
    }

    #[test]
    fn resize_left_edge_snaps_to_gap_beside_neighbor() {
        let proposed = CanvasRect::new(320.0, 0.0, 300.0, 200.0);
        let result = engine().snap_for_resize(proposed, CanvasResizeEdges::LEFT, &[neighbor()]);
        assert_eq!(result.frame.min_x(), 316.0);
        assert_eq!(result.frame.max_x(), 620.0);
    }

    #[test]
    fn resize_clamps_to_minimum_size() {
        let proposed = CanvasRect::new(0.0, 0.0, 150.0, 80.0);
        let result = engine().snap_for_resize(
            proposed,
            CanvasResizeEdges::RIGHT | CanvasResizeEdges::BOTTOM,
            &[],
        );
        assert_eq!(result.frame.width, 200.0);
        assert_eq!(result.frame.height, 120.0);
        assert_eq!(result.frame.origin(), proposed.origin());
    }

    #[test]
    fn resize_left_clamp_keeps_right_edge_fixed() {
        let proposed = CanvasRect::new(450.0, 0.0, 150.0, 200.0);
        let result = engine().snap_for_resize(proposed, CanvasResizeEdges::LEFT, &[]);
        assert_eq!(result.frame.max_x(), 600.0);
        assert_eq!(result.frame.width, 200.0);
        assert_eq!(result.frame.min_x(), 400.0);
    }

    #[test]
    fn resize_clamp_drops_undone_snap_guides() {
        let neighbor = CanvasRect::new(395.0, 0.0, 100.0, 100.0);
        let proposed = CanvasRect::new(400.0, 0.0, 150.0, 200.0);
        let result = engine().snap_for_resize(proposed, CanvasResizeEdges::LEFT, &[neighbor]);
        assert_eq!(result.frame.width, 200.0);
        assert_eq!(result.frame.max_x(), 550.0);
        assert!(result.guides.is_empty());
    }

    #[test]
    fn resize_top_and_corner_combination() {
        let proposed = CanvasRect::new(0.0, 203.0, 300.0, 197.0);
        let result = engine().snap_for_resize(
            proposed,
            CanvasResizeEdges::TOP | CanvasResizeEdges::RIGHT,
            &[neighbor()],
        );
        assert_eq!(result.frame, proposed);
    }

    #[test]
    fn resize_without_snap_beyond_threshold_returns_proposed() {
        let proposed = CanvasRect::new(0.0, 400.0, 280.0, 200.0);
        let result = engine().snap_for_resize(proposed, CanvasResizeEdges::RIGHT, &[neighbor()]);
        assert_eq!(result.frame, proposed);
        assert!(result.guides.is_empty());
    }
}
