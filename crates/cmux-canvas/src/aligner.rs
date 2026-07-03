//! Executes alignment/distribution commands. Port of `CanvasAligner.swift`.
//!
//! The aligner never mutates a layout; it returns new frames keyed by pane so
//! the caller can apply them through `CanvasLayout::set_frames`.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use crate::geometry::{CanvasAlignmentCommand, CanvasMetrics, CanvasRect};
use crate::layout::CanvasLayout;
use crate::pane::{CanvasPane, CanvasPaneID};

/// Computes the frame updates produced by an alignment command.
#[derive(Clone, Copy, Debug)]
pub struct CanvasAligner {
    /// The metrics supplying the canonical gap.
    pub metrics: CanvasMetrics,
}

impl CanvasAligner {
    /// Creates an aligner.
    pub fn new(metrics: CanvasMetrics) -> CanvasAligner {
        CanvasAligner { metrics }
    }

    /// Computes the frames produced by applying a command to a set of panes.
    ///
    /// Identifiers absent from the layout are ignored; fewer than two resolved
    /// panes yields no changes. `reference` is the pane whose width/height the
    /// equalize commands copy; when `None` or not part of `ids`, the widest
    /// (respectively tallest) pane is used.
    pub fn frames(
        &self,
        command: CanvasAlignmentCommand,
        ids: &[CanvasPaneID],
        layout: &CanvasLayout,
        reference: Option<CanvasPaneID>,
    ) -> HashMap<CanvasPaneID, CanvasRect> {
        let selection = Self::resolved_selection(ids, layout);
        if selection.len() < 2 {
            return HashMap::new();
        }

        match command {
            CanvasAlignmentCommand::AlignLeft => {
                let target = selection
                    .iter()
                    .map(|p| p.frame.min_x())
                    .reduce(f64::min)
                    .unwrap_or(0.0);
                Self::changed_frames(&selection, |frame| {
                    CanvasRect::new(target, frame.y, frame.width, frame.height)
                })
            }
            CanvasAlignmentCommand::AlignRight => {
                let target = selection
                    .iter()
                    .map(|p| p.frame.max_x())
                    .reduce(f64::max)
                    .unwrap_or(0.0);
                Self::changed_frames(&selection, |frame| {
                    CanvasRect::new(target - frame.width, frame.y, frame.width, frame.height)
                })
            }
            CanvasAlignmentCommand::AlignTop => {
                let target = selection
                    .iter()
                    .map(|p| p.frame.min_y())
                    .reduce(f64::min)
                    .unwrap_or(0.0);
                Self::changed_frames(&selection, |frame| {
                    CanvasRect::new(frame.x, target, frame.width, frame.height)
                })
            }
            CanvasAlignmentCommand::AlignBottom => {
                let target = selection
                    .iter()
                    .map(|p| p.frame.max_y())
                    .reduce(f64::max)
                    .unwrap_or(0.0);
                Self::changed_frames(&selection, |frame| {
                    CanvasRect::new(frame.x, target - frame.height, frame.width, frame.height)
                })
            }
            CanvasAlignmentCommand::EqualizeWidths => {
                let target = Self::reference_pane(reference, &selection, true).frame.width;
                Self::changed_frames(&selection, |frame| {
                    CanvasRect::new(frame.x, frame.y, target, frame.height)
                })
            }
            CanvasAlignmentCommand::EqualizeHeights => {
                let target = Self::reference_pane(reference, &selection, false).frame.height;
                Self::changed_frames(&selection, |frame| {
                    CanvasRect::new(frame.x, frame.y, frame.width, target)
                })
            }
            CanvasAlignmentCommand::DistributeHorizontally => {
                self.distributed_frames(&selection, true)
            }
            CanvasAlignmentCommand::DistributeVertically => {
                self.distributed_frames(&selection, false)
            }
            CanvasAlignmentCommand::Tidy => self.tidied_frames(&selection),
        }
    }

    fn resolved_selection(ids: &[CanvasPaneID], layout: &CanvasLayout) -> Vec<CanvasPane> {
        let mut seen: HashSet<CanvasPaneID> = HashSet::new();
        let mut result: Vec<CanvasPane> = Vec::new();
        for id in ids {
            if !seen.insert(*id) {
                continue;
            }
            if let Some(frame) = layout.frame(*id) {
                result.push(CanvasPane::new(*id, frame));
            }
        }
        result
    }

    fn changed_frames<F: Fn(CanvasRect) -> CanvasRect>(
        selection: &[CanvasPane],
        transform: F,
    ) -> HashMap<CanvasPaneID, CanvasRect> {
        let mut result: HashMap<CanvasPaneID, CanvasRect> = HashMap::new();
        for pane in selection {
            let updated = transform(pane.frame);
            if updated != pane.frame {
                result.insert(pane.id, updated);
            }
        }
        result
    }

    fn reference_pane(
        reference: Option<CanvasPaneID>,
        selection: &[CanvasPane],
        widest: bool,
    ) -> &CanvasPane {
        if let Some(reference) = reference {
            if let Some(pane) = selection.iter().find(|p| p.id == reference) {
                return pane;
            }
        }
        // Deterministic fallback: largest along the equalized dimension, then by id.
        selection
            .iter()
            .max_by(|lhs, rhs| {
                let l = if widest { lhs.frame.width } else { lhs.frame.height };
                let r = if widest { rhs.frame.width } else { rhs.frame.height };
                if l != r {
                    l.partial_cmp(&r).unwrap_or(Ordering::Equal)
                } else {
                    lhs.id.cmp(&rhs.id).reverse()
                }
            })
            .unwrap()
    }

    fn distributed_frames(
        &self,
        selection: &[CanvasPane],
        horizontally: bool,
    ) -> HashMap<CanvasPaneID, CanvasRect> {
        let mut sorted: Vec<CanvasPane> = selection.to_vec();
        sorted.sort_by(|lhs, rhs| {
            let l = if horizontally {
                lhs.frame.min_x()
            } else {
                lhs.frame.min_y()
            };
            let r = if horizontally {
                rhs.frame.min_x()
            } else {
                rhs.frame.min_y()
            };
            if l != r {
                l.partial_cmp(&r).unwrap_or(Ordering::Equal)
            } else {
                lhs.id.cmp(&rhs.id)
            }
        });
        let mut result: HashMap<CanvasPaneID, CanvasRect> = HashMap::new();
        let first = match sorted.first() {
            Some(first) => first,
            None => return result,
        };
        let mut cursor = if horizontally {
            first.frame.max_x()
        } else {
            first.frame.max_y()
        };
        for pane in sorted.iter().skip(1) {
            let mut frame = pane.frame;
            if horizontally {
                frame.x = cursor + self.metrics.gap;
                cursor = frame.max_x();
            } else {
                frame.y = cursor + self.metrics.gap;
                cursor = frame.max_y();
            }
            if frame != pane.frame {
                result.insert(pane.id, frame);
            }
        }
        result
    }

    fn tidied_frames(&self, selection: &[CanvasPane]) -> HashMap<CanvasPaneID, CanvasRect> {
        let origin_x = selection
            .iter()
            .map(|p| p.frame.min_x())
            .reduce(f64::min)
            .unwrap_or(0.0);
        let origin_y = selection
            .iter()
            .map(|p| p.frame.min_y())
            .reduce(f64::min)
            .unwrap_or(0.0);

        // Band panes into rows by vertical center.
        let mut sorted: Vec<CanvasPane> = selection.to_vec();
        sorted.sort_by(|lhs, rhs| {
            if lhs.frame.mid_y() != rhs.frame.mid_y() {
                lhs.frame
                    .mid_y()
                    .partial_cmp(&rhs.frame.mid_y())
                    .unwrap_or(Ordering::Equal)
            } else if lhs.frame.mid_x() != rhs.frame.mid_x() {
                lhs.frame
                    .mid_x()
                    .partial_cmp(&rhs.frame.mid_x())
                    .unwrap_or(Ordering::Equal)
            } else {
                lhs.id.cmp(&rhs.id)
            }
        });
        let mut rows: Vec<Vec<CanvasPane>> = Vec::new();
        let mut row_bottom = f64::NEG_INFINITY;
        for pane in sorted {
            if rows.is_empty() || pane.frame.mid_y() >= row_bottom {
                row_bottom = pane.frame.max_y();
                rows.push(vec![pane]);
            } else {
                row_bottom = row_bottom.max(pane.frame.max_y());
                let last = rows.len() - 1;
                rows[last].push(pane);
            }
        }

        let mut result: HashMap<CanvasPaneID, CanvasRect> = HashMap::new();
        let mut y = origin_y;
        for row in &rows {
            let mut ordered: Vec<CanvasPane> = row.clone();
            ordered.sort_by(|lhs, rhs| {
                if lhs.frame.mid_x() != rhs.frame.mid_x() {
                    lhs.frame
                        .mid_x()
                        .partial_cmp(&rhs.frame.mid_x())
                        .unwrap_or(Ordering::Equal)
                } else {
                    lhs.id.cmp(&rhs.id)
                }
            });
            let mut x = origin_x;
            let mut row_height: f64 = 0.0;
            for pane in &ordered {
                let frame = CanvasRect::new(x, y, pane.frame.width, pane.frame.height);
                if frame != pane.frame {
                    result.insert(pane.id, frame);
                }
                x = frame.max_x() + self.metrics.gap;
                row_height = row_height.max(pane.frame.height);
            }
            y += row_height + self.metrics.gap;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    // Port of CanvasAlignerTests.swift (11 @Test cases).
    use super::*;
    use crate::pane::Uuid;

    fn metrics() -> CanvasMetrics {
        CanvasMetrics::new(16.0, 8.0, CanvasMetrics::DEFAULT_MIN_PANE_SIZE)
    }
    fn aligner() -> CanvasAligner {
        CanvasAligner::new(metrics())
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
    fn align_left_uses_leftmost_edge() {
        let layout = layout(&[
            (1, CanvasRect::new(50.0, 0.0, 100.0, 100.0)),
            (2, CanvasRect::new(20.0, 200.0, 100.0, 100.0)),
            (3, CanvasRect::new(80.0, 400.0, 100.0, 100.0)),
        ]);
        let frames = aligner().frames(
            CanvasAlignmentCommand::AlignLeft,
            &layout.pane_ids(),
            &layout,
            None,
        );
        assert_eq!(frames[&id(1)].x, 20.0);
        assert_eq!(frames[&id(3)].x, 20.0);
        assert_eq!(frames.get(&id(2)), None);
    }

    #[test]
    fn align_right_uses_rightmost_edge() {
        let layout = layout(&[
            (1, CanvasRect::new(0.0, 0.0, 100.0, 100.0)),
            (2, CanvasRect::new(0.0, 200.0, 300.0, 100.0)),
        ]);
        let frames = aligner().frames(
            CanvasAlignmentCommand::AlignRight,
            &layout.pane_ids(),
            &layout,
            None,
        );
        assert_eq!(frames.get(&id(1)), Some(&CanvasRect::new(200.0, 0.0, 100.0, 100.0)));
        assert_eq!(frames.get(&id(2)), None);
    }

    #[test]
    fn align_top_and_bottom() {
        let layout = layout(&[
            (1, CanvasRect::new(0.0, 30.0, 100.0, 100.0)),
            (2, CanvasRect::new(200.0, 10.0, 100.0, 150.0)),
        ]);
        let tops = aligner().frames(
            CanvasAlignmentCommand::AlignTop,
            &layout.pane_ids(),
            &layout,
            None,
        );
        assert_eq!(tops[&id(1)].y, 10.0);
        let bottoms = aligner().frames(
            CanvasAlignmentCommand::AlignBottom,
            &layout.pane_ids(),
            &layout,
            None,
        );
        assert_eq!(bottoms[&id(1)].max_y(), 160.0);
        assert_eq!(bottoms.get(&id(2)), None);
    }

    #[test]
    fn equalize_widths_copies_reference_pane() {
        let layout = layout(&[
            (1, CanvasRect::new(0.0, 0.0, 250.0, 100.0)),
            (2, CanvasRect::new(300.0, 0.0, 400.0, 100.0)),
        ]);
        let frames = aligner().frames(
            CanvasAlignmentCommand::EqualizeWidths,
            &layout.pane_ids(),
            &layout,
            Some(id(1)),
        );
        assert_eq!(frames.get(&id(2)), Some(&CanvasRect::new(300.0, 0.0, 250.0, 100.0)));
        assert_eq!(frames.get(&id(1)), None);
    }

    #[test]
    fn equalize_heights_falls_back_to_tallest() {
        let layout = layout(&[
            (1, CanvasRect::new(0.0, 0.0, 100.0, 120.0)),
            (2, CanvasRect::new(200.0, 0.0, 100.0, 300.0)),
        ]);
        let frames = aligner().frames(
            CanvasAlignmentCommand::EqualizeHeights,
            &layout.pane_ids(),
            &layout,
            None,
        );
        assert_eq!(frames[&id(1)].height, 300.0);
        assert_eq!(frames.get(&id(2)), None);
    }

    #[test]
    fn distribute_horizontally_packs_at_gap() {
        let layout = layout(&[
            (1, CanvasRect::new(0.0, 0.0, 100.0, 100.0)),
            (2, CanvasRect::new(500.0, 50.0, 100.0, 100.0)),
            (3, CanvasRect::new(130.0, 25.0, 50.0, 100.0)),
        ]);
        let frames = aligner().frames(
            CanvasAlignmentCommand::DistributeHorizontally,
            &layout.pane_ids(),
            &layout,
            None,
        );
        assert_eq!(frames.get(&id(1)), None);
        assert_eq!(frames.get(&id(3)), Some(&CanvasRect::new(116.0, 25.0, 50.0, 100.0)));
        assert_eq!(frames.get(&id(2)), Some(&CanvasRect::new(182.0, 50.0, 100.0, 100.0)));
    }

    #[test]
    fn distribute_vertically_packs_at_gap() {
        let layout = layout(&[
            (1, CanvasRect::new(0.0, 0.0, 100.0, 100.0)),
            (2, CanvasRect::new(50.0, 400.0, 100.0, 80.0)),
        ]);
        let frames = aligner().frames(
            CanvasAlignmentCommand::DistributeVertically,
            &layout.pane_ids(),
            &layout,
            None,
        );
        assert_eq!(frames.get(&id(2)), Some(&CanvasRect::new(50.0, 116.0, 100.0, 80.0)));
    }

    #[test]
    fn tidy_packs_messy_panes_into_rows() {
        let layout = layout(&[
            (1, CanvasRect::new(7.0, 5.0, 200.0, 150.0)),
            (2, CanvasRect::new(260.0, -12.0, 200.0, 150.0)),
            (3, CanvasRect::new(30.0, 320.0, 200.0, 150.0)),
        ]);
        let frames = aligner().frames(
            CanvasAlignmentCommand::Tidy,
            &layout.pane_ids(),
            &layout,
            None,
        );
        assert_eq!(frames.get(&id(1)), Some(&CanvasRect::new(7.0, -12.0, 200.0, 150.0)));
        assert_eq!(frames.get(&id(2)), Some(&CanvasRect::new(223.0, -12.0, 200.0, 150.0)));
        assert_eq!(frames.get(&id(3)), Some(&CanvasRect::new(7.0, 154.0, 200.0, 150.0)));
    }

    #[test]
    fn tidy_preserves_sizes() {
        let layout = layout(&[
            (1, CanvasRect::new(0.0, 0.0, 320.0, 180.0)),
            (2, CanvasRect::new(900.0, 12.0, 200.0, 260.0)),
        ]);
        let frames = aligner().frames(
            CanvasAlignmentCommand::Tidy,
            &layout.pane_ids(),
            &layout,
            None,
        );
        let sizes: Vec<_> = layout
            .panes()
            .iter()
            .map(|p| frames.get(&p.id).map(|f| f.size()).unwrap_or(p.frame.size()))
            .collect();
        assert_eq!(
            sizes,
            vec![
                crate::geometry::CanvasSize::new(320.0, 180.0),
                crate::geometry::CanvasSize::new(200.0, 260.0)
            ]
        );
    }

    #[test]
    fn fewer_than_two_panes_is_a_no_op() {
        let layout = layout(&[(1, CanvasRect::new(0.0, 0.0, 100.0, 100.0))]);
        for command in CanvasAlignmentCommand::ALL {
            assert!(aligner()
                .frames(command, &layout.pane_ids(), &layout, None)
                .is_empty());
        }
    }

    #[test]
    fn unknown_ids_are_ignored() {
        let layout = layout(&[
            (1, CanvasRect::new(50.0, 0.0, 100.0, 100.0)),
            (2, CanvasRect::new(20.0, 200.0, 100.0, 100.0)),
        ]);
        let mut ids = layout.pane_ids();
        ids.push(id(99));
        let frames = aligner().frames(CanvasAlignmentCommand::AlignLeft, &ids, &layout, None);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[&id(1)].x, 20.0);
    }
}
