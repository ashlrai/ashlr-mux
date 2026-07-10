//! Pure planner for sidebar tab/workspace drag-and-drop.
//!
//! A platform-neutral Rust port of the macOS `SidebarDropPlanner`, whose entire
//! surface is index math + `CGRect`/`CGPoint` arithmetic with no AppKit/SwiftUI
//! dependency. The Swift originals live at
//! `Packages/macOS/CmuxFoundation/Sources/CmuxFoundation/SidebarDrop/`:
//!
//! * `SidebarDropPlanner.swift:1-324` — [`SidebarDropPlanner`], the drop-indicator
//!   / final-insertion-index / cross-window-landing / workspace-hit-test math,
//!   all clamped to the legal pinned/unpinned regions.
//! * `SidebarDropIndicator.swift:1-20` — [`SidebarDropEdge`] and
//!   [`SidebarDropIndicator`].
//!
//! ## Platform mapping
//!
//! The only platform swap is geometry: CoreGraphics `CGRect`/`CGPoint` map to the
//! local [`Rect`] / [`Point`] `f64` value types. Only the operations the planner
//! actually uses are provided — [`Rect::min_y`], [`Rect::max_y`],
//! [`Rect::height`] (a field), and [`Rect::contains`]. `contains` mirrors
//! `CGRectContainsPoint` on a standardized (non-negative width/height) rect:
//! min-edge inclusive, max-edge exclusive (`minX <= x < maxX && minY <= y < maxY`).
//!
//! ## Parity notes pinned
//!
//! * **`Int` index math is signed.** Swift `Int` is used throughout for
//!   insertion positions and indices; [`resolved_target_index`] computes
//!   `clampedInsertion - 1`, which can go negative before the final clamp, so all
//!   index math here is `i64` (never `usize`) to mirror Swift exactly
//!   (`SidebarDropPlanner.swift:319-323`).
//! * **`legalInsertionRange` is a `ClosedRange<Int>`** — mapped to
//!   `Option<RangeInclusive<i64>>`; the clamp is
//!   `min(max(clamped, lower), upper)` (`SidebarDropPlanner.swift:307-309`). No
//!   oracle test drives it (all pass `nil`); [`tests::legal_insertion_range_clamps`]
//!   covers it with a hand-computed expectation from the formula.
//! * **`pinnedTabIds` is a `Set<UUID>`** — mapped to `&HashSet<Uuid>`; membership
//!   (`.contains`) and the "leading pinned segment" count are what the clamps use.
//! * **Empty-pinned short-circuit.** [`legal_insertion_position`] only applies the
//!   pinned/unpinned clamp when `!pinnedTabIds.isEmpty` *and* `pinnedCount > 0`
//!   (`SidebarDropPlanner.swift:292-305`); the cross-window clamp
//!   ([`legal_cross_window_insertion_position`]) applies even when `pinnedCount`
//!   is zero, so a pinned workspace dragged into a pin-less window still lands at
//!   the front (`SidebarDropPlanner.swift:141-161`).

use std::collections::HashSet;
use std::ops::RangeInclusive;

use uuid::Uuid;

/// A point in sidebar coordinate space — CoreGraphics `CGPoint`. Components are
/// `f64` to mirror Swift's `CGFloat`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// The x coordinate.
    pub x: f64,
    /// The y coordinate.
    pub y: f64,
}

impl Point {
    /// Construct a point (mirrors `CGPoint(x:y:)`).
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A rectangle in sidebar coordinate space — CoreGraphics `CGRect`. Components
/// are `f64` to mirror Swift's `CGFloat`. Only the operations the planner needs
/// are provided (see the module-level platform-mapping note).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Origin x (minimum x for a non-negative-width rect).
    pub x: f64,
    /// Origin y (minimum y for a non-negative-height rect).
    pub y: f64,
    /// Width.
    pub width: f64,
    /// Height.
    pub height: f64,
}

impl Rect {
    /// Construct a rect from origin + size (mirrors `CGRect(x:y:width:height:)`).
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The minimum-y edge (`CGRect.minY`) for a non-negative-height rect.
    pub fn min_y(&self) -> f64 {
        self.y
    }

    /// The maximum-y edge (`CGRect.maxY`) for a non-negative-height rect.
    pub fn max_y(&self) -> f64 {
        self.y + self.height
    }

    /// Whether the rect contains `point`, mirroring `CGRectContainsPoint` on a
    /// standardized rect: min-edge inclusive, max-edge exclusive.
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x
            && point.x < self.x + self.width
            && point.y >= self.y
            && point.y < self.y + self.height
    }
}

/// Which edge of a sidebar row a drop indicator is drawn against.
///
/// Port of Swift `SidebarDropEdge` (`SidebarDropIndicator.swift:4-7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarDropEdge {
    /// The row's top edge.
    Top,
    /// The row's bottom edge.
    Bottom,
}

/// Where the sidebar should render the drop indicator during a tab/workspace
/// drag: against the `Top` or `Bottom` edge of the row identified by `tab_id`,
/// or at the end of the list when `tab_id` is `None`.
///
/// Port of Swift `SidebarDropIndicator` (`SidebarDropIndicator.swift:12-20`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SidebarDropIndicator {
    /// The row this indicator is attached to, or `None` for the end of the list.
    pub tab_id: Option<Uuid>,
    /// Which edge of `tab_id` (or the list) the indicator sits against.
    pub edge: SidebarDropEdge,
}

impl SidebarDropIndicator {
    /// Construct an indicator (mirrors `init(tabId:edge:)`).
    pub fn new(tab_id: Option<Uuid>, edge: SidebarDropEdge) -> Self {
        Self { tab_id, edge }
    }
}

/// A candidate workspace-drop target row: its id, pinned flag, and on-screen
/// frame.
///
/// Port of the nested Swift `SidebarDropPlanner.WorkspaceDropTarget`
/// (`SidebarDropPlanner.swift:163-173`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkspaceDropTarget {
    /// The row's workspace id.
    pub workspace_id: Uuid,
    /// Whether the row is pinned.
    pub is_pinned: bool,
    /// The row's on-screen bounds.
    pub frame: Rect,
}

impl WorkspaceDropTarget {
    /// Construct a workspace drop target (mirrors
    /// `init(workspaceId:isPinned:frame:)`).
    pub fn new(workspace_id: Uuid, is_pinned: bool, frame: Rect) -> Self {
        Self {
            workspace_id,
            is_pinned,
            frame,
        }
    }
}

/// The action a workspace drop resolves to: create a new workspace at
/// `insertion_index` (with the indicator the user saw), or drop onto an existing
/// workspace row.
///
/// Port of the nested Swift `SidebarDropPlanner.WorkspaceDropAction`
/// (`SidebarDropPlanner.swift:183-186`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WorkspaceDropAction {
    /// Create a new workspace at `insertion_index`, rendering `indicator`.
    NewWorkspace {
        /// The clamped insertion index for the new workspace.
        insertion_index: i64,
        /// The indicator to render at drop time.
        indicator: SidebarDropIndicator,
    },
    /// Drop onto the existing workspace with this id.
    ExistingWorkspace(Uuid),
}

/// Pure planner for sidebar tab/workspace drag-and-drop.
///
/// Port of Swift `SidebarDropPlanner` (`SidebarDropPlanner.swift:8`). Stateless;
/// every method is a pure function of its arguments.
#[derive(Debug, Clone, Copy, Default)]
pub struct SidebarDropPlanner;

impl SidebarDropPlanner {
    /// Construct a planner (mirrors `init()`, `SidebarDropPlanner.swift:9`).
    pub fn new() -> Self {
        Self
    }

    /// The drop indicator to render while dragging `dragged_tab_id`, or `None`
    /// when the drop would be a no-op (dragging onto itself) or is not
    /// applicable.
    ///
    /// Port of `indicator(draggedTabId:targetTabId:tabIds:pinnedTabIds:legalInsertionRange:pointerY:targetHeight:)`
    /// (`SidebarDropPlanner.swift:11-51`).
    #[allow(clippy::too_many_arguments)]
    pub fn indicator(
        &self,
        dragged_tab_id: Option<Uuid>,
        target_tab_id: Option<Uuid>,
        tab_ids: &[Uuid],
        pinned_tab_ids: &HashSet<Uuid>,
        legal_insertion_range: Option<RangeInclusive<i64>>,
        pointer_y: Option<f64>,
        target_height: Option<f64>,
    ) -> Option<SidebarDropIndicator> {
        if tab_ids.len() <= 1 {
            return None;
        }
        let dragged_tab_id = dragged_tab_id?;
        let from_index = first_index(tab_ids, dragged_tab_id)?;

        let insertion_position: i64 = if let Some(target_tab_id) = target_tab_id {
            let target_tab_index = first_index(tab_ids, target_tab_id)?;
            let edge = match (pointer_y, target_height) {
                (Some(y), Some(h)) => self.edge_for_pointer(y, h),
                _ => preferred_edge(from_index, target_tab_id, tab_ids),
            };
            if edge == SidebarDropEdge::Bottom {
                target_tab_index + 1
            } else {
                target_tab_index
            }
        } else {
            tab_ids.len() as i64
        };

        let legal_insertion_position = legal_insertion_position(
            dragged_tab_id,
            insertion_position,
            tab_ids,
            pinned_tab_ids,
            legal_insertion_range.as_ref(),
        );
        let legal_target_index =
            resolved_target_index(from_index, legal_insertion_position, tab_ids.len() as i64);
        if legal_target_index == from_index {
            return None;
        }
        Some(indicator_for_insertion_position(
            legal_insertion_position,
            tab_ids,
        ))
    }

    /// The final array index the dragged tab should move to, after clamping,
    /// or `None` when the dragged tab is absent.
    ///
    /// Port of `targetIndex(draggedTabId:targetTabId:indicator:tabIds:pinnedTabIds:legalInsertionRange:)`
    /// (`SidebarDropPlanner.swift:53-84`).
    pub fn target_index(
        &self,
        dragged_tab_id: Uuid,
        target_tab_id: Option<Uuid>,
        indicator: Option<SidebarDropIndicator>,
        tab_ids: &[Uuid],
        pinned_tab_ids: &HashSet<Uuid>,
        legal_insertion_range: Option<RangeInclusive<i64>>,
    ) -> Option<i64> {
        let from_index = first_index(tab_ids, dragged_tab_id)?;

        let insertion_position: i64 = if let Some(indicator_insertion) =
            indicator.and_then(|indicator| insertion_position_for_indicator(&indicator, tab_ids))
        {
            indicator_insertion
        } else if let Some(target_tab_id) = target_tab_id {
            let target_tab_index = first_index(tab_ids, target_tab_id)?;
            // Swift: `(indicator?.tabId == targetTabId) ? (indicator?.edge ?? preferred) : preferred`.
            let edge = if indicator.map(|i| i.tab_id) == Some(Some(target_tab_id)) {
                indicator
                    .map(|i| i.edge)
                    .unwrap_or_else(|| preferred_edge(from_index, target_tab_id, tab_ids))
            } else {
                preferred_edge(from_index, target_tab_id, tab_ids)
            };
            if edge == SidebarDropEdge::Bottom {
                target_tab_index + 1
            } else {
                target_tab_index
            }
        } else {
            tab_ids.len() as i64
        };

        let legal_insertion_position = legal_insertion_position(
            dragged_tab_id,
            insertion_position,
            tab_ids,
            pinned_tab_ids,
            legal_insertion_range.as_ref(),
        );
        Some(resolved_target_index(
            from_index,
            legal_insertion_position,
            tab_ids.len() as i64,
        ))
    }

    /// Where a workspace dragged in from *another window* should land in this
    /// window's sidebar, plus the indicator to render while it hovers.
    ///
    /// Unlike [`indicator`](Self::indicator) / [`target_index`](Self::target_index),
    /// the dragged workspace is **not** a member of `tab_ids` — it lives in a
    /// different window — so there is no source index to remove and the returned
    /// index is a plain insertion position in `0..=tab_ids.len()`.
    ///
    /// Port of `crossWindowInsertion(targetTabId:draggedIsPinned:indicator:tabIds:pinnedTabIds:pointerY:targetHeight:)`
    /// (`SidebarDropPlanner.swift:108-139`).
    #[allow(clippy::too_many_arguments)]
    pub fn cross_window_insertion(
        &self,
        target_tab_id: Option<Uuid>,
        dragged_is_pinned: bool,
        indicator: Option<SidebarDropIndicator>,
        tab_ids: &[Uuid],
        pinned_tab_ids: &HashSet<Uuid>,
        pointer_y: Option<f64>,
        target_height: Option<f64>,
    ) -> (i64, SidebarDropIndicator) {
        let proposed: i64 = if let Some(indicator_insertion) =
            indicator.and_then(|indicator| insertion_position_for_indicator(&indicator, tab_ids))
        {
            indicator_insertion
        } else if let Some(target_tab_index) = target_tab_id.and_then(|id| first_index(tab_ids, id))
        {
            let edge = match (pointer_y, target_height) {
                (Some(y), Some(h)) => self.edge_for_pointer(y, h),
                _ => SidebarDropEdge::Top,
            };
            if edge == SidebarDropEdge::Bottom {
                target_tab_index + 1
            } else {
                target_tab_index
            }
        } else {
            tab_ids.len() as i64
        };

        let legal_insertion = legal_cross_window_insertion_position(
            proposed,
            dragged_is_pinned,
            tab_ids,
            pinned_tab_ids,
        );
        (
            legal_insertion,
            indicator_for_insertion_position(legal_insertion, tab_ids),
        )
    }

    /// Returns whether sidebar rows should publish frame anchors for workspace
    /// drop targeting.
    ///
    /// Port of `shouldCollectWorkspaceDropTargets(draggedTabId:isBonsplitWorkspaceDropActive:)`
    /// (`SidebarDropPlanner.swift:176-181`).
    pub fn should_collect_workspace_drop_targets(
        &self,
        dragged_tab_id: Option<Uuid>,
        is_bonsplit_workspace_drop_active: bool,
    ) -> bool {
        dragged_tab_id.is_some() || is_bonsplit_workspace_drop_active
    }

    /// The action a point-based workspace drop resolves to, or `None` when there
    /// are no targets.
    ///
    /// Port of `workspaceAction(for:targets:)`
    /// (`SidebarDropPlanner.swift:188-212`).
    pub fn workspace_action(
        &self,
        point: Point,
        targets: &[WorkspaceDropTarget],
    ) -> Option<WorkspaceDropAction> {
        if targets.is_empty() {
            return None;
        }
        // Swift: `targets.sorted { $0.frame.minY < $1.frame.minY }`.
        let mut ordered_targets = targets.to_vec();
        ordered_targets.sort_by(|a, b| {
            a.frame
                .min_y()
                .partial_cmp(&b.frame.min_y())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(containing_target) = ordered_targets
            .iter()
            .find(|t| t.frame.contains(point))
            .copied()
        {
            return self.workspace_action_in_target(point, &containing_target, &ordered_targets);
        }

        let proposed_insertion: i64 = if let Some(before_target) = ordered_targets
            .iter()
            .find(|t| point.y < t.frame.min_y())
            .copied()
        {
            ordered_targets
                .iter()
                .position(|t| *t == before_target)
                .map(|i| i as i64)
                .unwrap_or(0)
        } else {
            ordered_targets.len() as i64
        };
        let insertion_index =
            legal_new_workspace_insertion_index(proposed_insertion, &ordered_targets);
        Some(WorkspaceDropAction::NewWorkspace {
            insertion_index,
            indicator: workspace_indicator(insertion_index, &ordered_targets),
        })
    }

    /// Resolves a drop within a specific hovered target: top/bottom edge bands
    /// create a new workspace before/after the row; the center drops onto it.
    ///
    /// Port of the private `workspaceAction(for:in:orderedTargets:)`
    /// (`SidebarDropPlanner.swift:214-236`).
    fn workspace_action_in_target(
        &self,
        point: Point,
        target: &WorkspaceDropTarget,
        ordered_targets: &[WorkspaceDropTarget],
    ) -> Option<WorkspaceDropAction> {
        let target_index = ordered_targets.iter().position(|t| t == target)? as i64;
        let edge_band = (target.frame.height * 0.25)
            .max(10.0)
            .min(target.frame.height / 2.0);
        if point.y <= target.frame.min_y() + edge_band {
            let insertion_index =
                legal_new_workspace_insertion_index(target_index, ordered_targets);
            return Some(WorkspaceDropAction::NewWorkspace {
                insertion_index,
                indicator: workspace_indicator(insertion_index, ordered_targets),
            });
        }
        if point.y >= target.frame.max_y() - edge_band {
            let insertion_index =
                legal_new_workspace_insertion_index(target_index + 1, ordered_targets);
            return Some(WorkspaceDropAction::NewWorkspace {
                insertion_index,
                indicator: workspace_indicator(insertion_index, ordered_targets),
            });
        }
        Some(WorkspaceDropAction::ExistingWorkspace(target.workspace_id))
    }

    /// The edge (top/bottom) a pointer at `location_y` selects within a row of
    /// `target_height`.
    ///
    /// Port of `edgeForPointer(locationY:targetHeight:)`
    /// (`SidebarDropPlanner.swift:313-317`).
    pub fn edge_for_pointer(&self, location_y: f64, target_height: f64) -> SidebarDropEdge {
        if target_height <= 0.0 {
            return SidebarDropEdge::Top;
        }
        let clamped_y = location_y.max(0.0).min(target_height);
        if clamped_y < target_height / 2.0 {
            SidebarDropEdge::Top
        } else {
            SidebarDropEdge::Bottom
        }
    }
}

/// `tabIds.firstIndex(of:)` as a signed index (Swift `Int`).
fn first_index(tab_ids: &[Uuid], id: Uuid) -> Option<i64> {
    tab_ids.iter().position(|t| *t == id).map(|i| i as i64)
}

/// Clamp a cross-window insertion so a pinned workspace lands inside the leading
/// pinned block and an unpinned one lands after it. The clamp applies even when
/// `pinnedCount` is zero.
///
/// Port of the private `legalCrossWindowInsertionPosition(...)`
/// (`SidebarDropPlanner.swift:148-161`).
fn legal_cross_window_insertion_position(
    proposed_insertion_position: i64,
    dragged_is_pinned: bool,
    tab_ids: &[Uuid],
    pinned_tab_ids: &HashSet<Uuid>,
) -> i64 {
    let clamped_insertion = proposed_insertion_position.clamp(0, tab_ids.len() as i64);
    let pinned_count = pinned_count(tab_ids, pinned_tab_ids);
    if dragged_is_pinned {
        clamped_insertion.min(pinned_count)
    } else {
        clamped_insertion.max(pinned_count)
    }
}

/// Clamp a new-workspace insertion below the leading pinned block.
///
/// Port of the private `legalNewWorkspaceInsertionIndex(_:orderedTargets:)`
/// (`SidebarDropPlanner.swift:238-249`).
fn legal_new_workspace_insertion_index(
    proposed_insertion: i64,
    ordered_targets: &[WorkspaceDropTarget],
) -> i64 {
    let clamped = proposed_insertion.clamp(0, ordered_targets.len() as i64);
    let pinned_count = ordered_targets.iter().filter(|t| t.is_pinned).count() as i64;
    clamped.max(pinned_count)
}

/// The indicator for a new-workspace insertion index over ordered targets.
///
/// Port of the private `workspaceIndicator(forInsertionIndex:orderedTargets:)`
/// (`SidebarDropPlanner.swift:251-260`).
fn workspace_indicator(
    insertion_index: i64,
    ordered_targets: &[WorkspaceDropTarget],
) -> SidebarDropIndicator {
    let clamped_insertion = insertion_index.clamp(0, ordered_targets.len() as i64);
    if clamped_insertion >= ordered_targets.len() as i64 {
        return SidebarDropIndicator::new(None, SidebarDropEdge::Bottom);
    }
    SidebarDropIndicator::new(
        Some(ordered_targets[clamped_insertion as usize].workspace_id),
        SidebarDropEdge::Top,
    )
}

/// The indicator for an insertion position over `tabIds`.
///
/// Port of the private `indicatorForInsertionPosition(_:tabIds:)`
/// (`SidebarDropPlanner.swift:262-268`).
fn indicator_for_insertion_position(
    insertion_position: i64,
    tab_ids: &[Uuid],
) -> SidebarDropIndicator {
    let clamped_insertion = insertion_position.clamp(0, tab_ids.len() as i64);
    if clamped_insertion >= tab_ids.len() as i64 {
        return SidebarDropIndicator::new(None, SidebarDropEdge::Bottom);
    }
    SidebarDropIndicator::new(
        Some(tab_ids[clamped_insertion as usize]),
        SidebarDropEdge::Top,
    )
}

/// Recover the insertion position an indicator represents, or `None` when its
/// `tabId` is absent from `tabIds`.
///
/// Port of the private `insertionPositionForIndicator(_:tabIds:)`
/// (`SidebarDropPlanner.swift:270-276`).
fn insertion_position_for_indicator(
    indicator: &SidebarDropIndicator,
    tab_ids: &[Uuid],
) -> Option<i64> {
    if let Some(tab_id) = indicator.tab_id {
        let target_tab_index = first_index(tab_ids, tab_id)?;
        return Some(if indicator.edge == SidebarDropEdge::Bottom {
            target_tab_index + 1
        } else {
            target_tab_index
        });
    }
    Some(tab_ids.len() as i64)
}

/// The preferred edge for a same-window reorder: `bottom` when dragging down
/// past the target, `top` when dragging up.
///
/// Port of the private `preferredEdge(fromIndex:targetTabId:tabIds:)`
/// (`SidebarDropPlanner.swift:278-281`).
fn preferred_edge(from_index: i64, target_tab_id: Uuid, tab_ids: &[Uuid]) -> SidebarDropEdge {
    match first_index(tab_ids, target_tab_id) {
        Some(target_index) if from_index < target_index => SidebarDropEdge::Bottom,
        Some(_) => SidebarDropEdge::Top,
        None => SidebarDropEdge::Top,
    }
}

/// Clamp an insertion position to the legal pinned/unpinned region and (if
/// present) the explicit legal insertion range.
///
/// Port of the private `legalInsertionPosition(...)`
/// (`SidebarDropPlanner.swift:283-311`).
fn legal_insertion_position(
    dragged_tab_id: Uuid,
    proposed_insertion_position: i64,
    tab_ids: &[Uuid],
    pinned_tab_ids: &HashSet<Uuid>,
    legal_insertion_range: Option<&RangeInclusive<i64>>,
) -> i64 {
    let mut clamped_insertion = proposed_insertion_position.clamp(0, tab_ids.len() as i64);

    if !pinned_tab_ids.is_empty() {
        let pinned_count = pinned_count(tab_ids, pinned_tab_ids);
        if pinned_count > 0 {
            if pinned_tab_ids.contains(&dragged_tab_id) {
                clamped_insertion = clamped_insertion.min(pinned_count);
            } else {
                clamped_insertion = clamped_insertion.max(pinned_count);
            }
        }
    }

    if let Some(range) = legal_insertion_range {
        return clamped_insertion.max(*range.start()).min(*range.end());
    }
    clamped_insertion
}

/// The final array index after removing the source and re-inserting at
/// `insertionPosition`.
///
/// Port of the private `resolvedTargetIndex(from:insertionPosition:totalCount:)`
/// (`SidebarDropPlanner.swift:319-323`). All math is signed: `clampedInsertion - 1`
/// can transiently go negative before the final `max(0, …)`.
fn resolved_target_index(source_index: i64, insertion_position: i64, total_count: i64) -> i64 {
    let clamped_insertion = insertion_position.clamp(0, total_count);
    let adjusted = if clamped_insertion > source_index {
        clamped_insertion - 1
    } else {
        clamped_insertion
    };
    adjusted.clamp(0, total_count.max(1) - 1).max(0)
}

/// The number of `tabIds` present in `pinnedTabIds` — the length of the leading
/// pinned segment invariant the clamps enforce (Swift counts over `tabIds`, not
/// the set, `SidebarDropPlanner.swift:155-159`).
fn pinned_count(tab_ids: &[Uuid], pinned_tab_ids: &HashSet<Uuid>) -> i64 {
    tab_ids
        .iter()
        .filter(|t| pinned_tab_ids.contains(*t))
        .count() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic distinct UUIDs (Swift tests use random `UUID()`; the planner
    /// only ever compares by identity / array order, so fixed ids preserve every
    /// outcome).
    fn uid(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn pinned(ids: &[Uuid]) -> HashSet<Uuid> {
        ids.iter().copied().collect()
    }

    /// Mirrors the Swift test helper `workspaceDropTargets(_:pinnedIds:)`
    /// (`SidebarWorkspaceDropPlannerTests.swift:419-430`): each row at
    /// `y = index * 40`, `width 180`, `height 32`.
    fn workspace_drop_targets(ids: &[Uuid], pinned_ids: &[Uuid]) -> Vec<WorkspaceDropTarget> {
        let pinned_set: HashSet<Uuid> = pinned_ids.iter().copied().collect();
        ids.iter()
            .enumerate()
            .map(|(index, id)| {
                WorkspaceDropTarget::new(
                    *id,
                    pinned_set.contains(id),
                    Rect::new(0.0, (index as f64) * 40.0, 180.0, 32.0),
                )
            })
            .collect()
    }

    // MARK: - indicator / targetIndex (SidebarOrderingTests.swift:620-844)

    /// Ported from `testNoIndicatorForNoOpEdges`
    /// (`SidebarOrderingTests.swift:621-643`).
    #[test]
    fn no_indicator_for_no_op_edges() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let tab_ids = [first, second, third];

        assert_eq!(
            SidebarDropPlanner::new().indicator(
                Some(first),
                Some(first),
                &tab_ids,
                &pinned(&[]),
                None,
                None,
                None,
            ),
            None
        );
        assert_eq!(
            SidebarDropPlanner::new().indicator(
                Some(third),
                None,
                &tab_ids,
                &pinned(&[]),
                None,
                None,
                None,
            ),
            None
        );
    }

    /// Ported from `testNoIndicatorWhenOnlyOneTabExists`
    /// (`SidebarOrderingTests.swift:645-663`).
    #[test]
    fn no_indicator_when_only_one_tab_exists() {
        let only = uid(1);
        assert_eq!(
            SidebarDropPlanner::new().indicator(
                Some(only),
                None,
                &[only],
                &pinned(&[]),
                None,
                None,
                None,
            ),
            None
        );
        assert_eq!(
            SidebarDropPlanner::new().indicator(
                Some(only),
                Some(only),
                &[only],
                &pinned(&[]),
                None,
                None,
                None,
            ),
            None
        );
    }

    /// Ported from `testIndicatorAppearsForRealMoveToEnd`
    /// (`SidebarOrderingTests.swift:665-679`).
    #[test]
    fn indicator_appears_for_real_move_to_end() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let tab_ids = [first, second, third];

        let indicator = SidebarDropPlanner::new()
            .indicator(Some(second), None, &tab_ids, &pinned(&[]), None, None, None)
            .expect("expected an indicator for a real move to end");
        assert_eq!(indicator.tab_id, None);
        assert_eq!(indicator.edge, SidebarDropEdge::Bottom);
    }

    /// Ported from `testTargetIndexForMoveToEndFromMiddle`
    /// (`SidebarOrderingTests.swift:681-695`).
    #[test]
    fn target_index_for_move_to_end_from_middle() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let tab_ids = [first, second, third];

        let index = SidebarDropPlanner::new().target_index(
            second,
            None,
            Some(SidebarDropIndicator::new(None, SidebarDropEdge::Bottom)),
            &tab_ids,
            &pinned(&[]),
            None,
        );
        assert_eq!(index, Some(2));
    }

    /// Ported from `testNoIndicatorForSelfDropInMiddle`
    /// (`SidebarOrderingTests.swift:697-711`).
    #[test]
    fn no_indicator_for_self_drop_in_middle() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let tab_ids = [first, second, third];

        assert_eq!(
            SidebarDropPlanner::new().indicator(
                Some(second),
                Some(second),
                &tab_ids,
                &pinned(&[]),
                None,
                None,
                None,
            ),
            None
        );
    }

    /// Ported from `testPointerEdgeTopCanSuppressNoOpWhenDraggingFirstOverSecond`
    /// (`SidebarOrderingTests.swift:713-729`).
    #[test]
    fn pointer_edge_top_can_suppress_no_op_when_dragging_first_over_second() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let tab_ids = [first, second, third];

        assert_eq!(
            SidebarDropPlanner::new().indicator(
                Some(first),
                Some(second),
                &tab_ids,
                &pinned(&[]),
                None,
                Some(2.0),
                Some(40.0),
            ),
            None
        );
    }

    /// Ported from `testPointerEdgeBottomAllowsMoveWhenDraggingFirstOverSecond`
    /// (`SidebarOrderingTests.swift:731-757`).
    #[test]
    fn pointer_edge_bottom_allows_move_when_dragging_first_over_second() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let tab_ids = [first, second, third];

        let indicator = SidebarDropPlanner::new().indicator(
            Some(first),
            Some(second),
            &tab_ids,
            &pinned(&[]),
            None,
            Some(38.0),
            Some(40.0),
        );
        assert_eq!(indicator.map(|i| i.tab_id), Some(Some(third)));
        assert_eq!(indicator.map(|i| i.edge), Some(SidebarDropEdge::Top));
        assert_eq!(
            SidebarDropPlanner::new().target_index(
                first,
                Some(second),
                indicator,
                &tab_ids,
                &pinned(&[]),
                None,
            ),
            Some(1)
        );
    }

    /// Ported from `testEquivalentBoundaryInputsResolveToSingleCanonicalIndicator`
    /// (`SidebarOrderingTests.swift:759-786`).
    #[test]
    fn equivalent_boundary_inputs_resolve_to_single_canonical_indicator() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let tab_ids = [first, second, third];

        let from_bottom_of_first = SidebarDropPlanner::new().indicator(
            Some(third),
            Some(first),
            &tab_ids,
            &pinned(&[]),
            None,
            Some(38.0),
            Some(40.0),
        );
        let from_top_of_second = SidebarDropPlanner::new().indicator(
            Some(third),
            Some(second),
            &tab_ids,
            &pinned(&[]),
            None,
            Some(2.0),
            Some(40.0),
        );

        assert_eq!(from_bottom_of_first.map(|i| i.tab_id), Some(Some(second)));
        assert_eq!(
            from_bottom_of_first.map(|i| i.edge),
            Some(SidebarDropEdge::Top)
        );
        assert_eq!(from_top_of_second.map(|i| i.tab_id), Some(Some(second)));
        assert_eq!(
            from_top_of_second.map(|i| i.edge),
            Some(SidebarDropEdge::Top)
        );
    }

    /// Ported from `testPointerEdgeBottomSuppressesNoOpWhenDraggingLastOverSecond`
    /// (`SidebarOrderingTests.swift:788-804`).
    #[test]
    fn pointer_edge_bottom_suppresses_no_op_when_dragging_last_over_second() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let tab_ids = [first, second, third];

        assert_eq!(
            SidebarDropPlanner::new().indicator(
                Some(third),
                Some(second),
                &tab_ids,
                &pinned(&[]),
                None,
                Some(38.0),
                Some(40.0),
            ),
            None
        );
    }

    /// Ported from
    /// `testIndicatorSnapsUnpinnedDropToFirstUnpinnedBoundaryWhenHoveringPinnedWorkspace`
    /// (`SidebarOrderingTests.swift:806-825`).
    #[test]
    fn indicator_snaps_unpinned_drop_to_first_unpinned_boundary() {
        let (pinned_a, pinned_b, unpinned_a, unpinned_b) = (uid(1), uid(2), uid(3), uid(4));
        let tab_ids = [pinned_a, pinned_b, unpinned_a, unpinned_b];
        let pinned_ids = pinned(&[pinned_a, pinned_b]);

        let indicator = SidebarDropPlanner::new().indicator(
            Some(unpinned_b),
            Some(pinned_a),
            &tab_ids,
            &pinned_ids,
            None,
            Some(2.0),
            Some(40.0),
        );

        assert_eq!(indicator.map(|i| i.tab_id), Some(Some(unpinned_a)));
        assert_eq!(indicator.map(|i| i.edge), Some(SidebarDropEdge::Top));
    }

    /// Ported from
    /// `testTargetIndexSnapsUnpinnedDropToFirstUnpinnedBoundaryWhenHoveringPinnedWorkspace`
    /// (`SidebarOrderingTests.swift:827-844`).
    #[test]
    fn target_index_snaps_unpinned_drop_to_first_unpinned_boundary() {
        let (pinned_a, pinned_b, unpinned_a, unpinned_b) = (uid(1), uid(2), uid(3), uid(4));
        let tab_ids = [pinned_a, pinned_b, unpinned_a, unpinned_b];
        let pinned_ids = pinned(&[pinned_a, pinned_b]);

        let target_index = SidebarDropPlanner::new().target_index(
            unpinned_b,
            Some(pinned_a),
            Some(SidebarDropIndicator::new(
                Some(pinned_a),
                SidebarDropEdge::Top,
            )),
            &tab_ids,
            &pinned_ids,
            None,
        );

        assert_eq!(target_index, Some(2));
        let _ = unpinned_a;
    }

    // MARK: - Cross-window insertion (SidebarOrderingTests.swift:846-965)

    /// Ported from `testCrossWindowInsertionAppendsWhenDroppingOnEmptyArea`
    /// (`SidebarOrderingTests.swift:848-861`).
    #[test]
    fn cross_window_insertion_appends_when_dropping_on_empty_area() {
        let (a, b) = (uid(1), uid(2));
        let result = SidebarDropPlanner::new().cross_window_insertion(
            None,
            false,
            None,
            &[a, b],
            &pinned(&[]),
            None,
            None,
        );

        assert_eq!(result.0, 2);
        assert_eq!(
            result.1,
            SidebarDropIndicator::new(None, SidebarDropEdge::Bottom)
        );
    }

    /// Ported from `testCrossWindowInsertionTopEdgeInsertsBeforeTarget`
    /// (`SidebarOrderingTests.swift:863-879`).
    #[test]
    fn cross_window_insertion_top_edge_inserts_before_target() {
        let (a, b, c) = (uid(1), uid(2), uid(3));
        let result = SidebarDropPlanner::new().cross_window_insertion(
            Some(b),
            false,
            None,
            &[a, b, c],
            &pinned(&[]),
            Some(2.0),
            Some(40.0),
        );

        assert_eq!(result.0, 1);
        assert_eq!(
            result.1,
            SidebarDropIndicator::new(Some(b), SidebarDropEdge::Top)
        );
    }

    /// Ported from `testCrossWindowInsertionBottomEdgeInsertsAfterTarget`
    /// (`SidebarOrderingTests.swift:881-897`).
    #[test]
    fn cross_window_insertion_bottom_edge_inserts_after_target() {
        let (a, b, c) = (uid(1), uid(2), uid(3));
        let result = SidebarDropPlanner::new().cross_window_insertion(
            Some(b),
            false,
            None,
            &[a, b, c],
            &pinned(&[]),
            Some(38.0),
            Some(40.0),
        );

        assert_eq!(result.0, 2);
        assert_eq!(
            result.1,
            SidebarDropIndicator::new(Some(c), SidebarDropEdge::Top)
        );
    }

    /// Ported from `testCrossWindowInsertionClampsUnpinnedWorkspaceBelowPinnedRegion`
    /// (`SidebarOrderingTests.swift:899-914`).
    #[test]
    fn cross_window_insertion_clamps_unpinned_workspace_below_pinned_region() {
        let (pinned_a, pinned_b, unpinned) = (uid(1), uid(2), uid(3));
        let result = SidebarDropPlanner::new().cross_window_insertion(
            Some(pinned_a),
            false,
            Some(SidebarDropIndicator::new(
                Some(pinned_a),
                SidebarDropEdge::Top,
            )),
            &[pinned_a, pinned_b, unpinned],
            &pinned(&[pinned_a, pinned_b]),
            None,
            None,
        );

        // An unpinned workspace cannot land above the two pinned rows.
        assert_eq!(result.0, 2);
        assert_eq!(
            result.1,
            SidebarDropIndicator::new(Some(unpinned), SidebarDropEdge::Top)
        );
    }

    /// Ported from `testCrossWindowInsertionClampsPinnedWorkspaceToFrontWhenNoExistingPins`
    /// (`SidebarOrderingTests.swift:916-931`).
    #[test]
    fn cross_window_insertion_clamps_pinned_workspace_to_front_when_no_existing_pins() {
        let (a, b) = (uid(1), uid(2));
        let result = SidebarDropPlanner::new().cross_window_insertion(
            None,
            true,
            None,
            &[a, b],
            &pinned(&[]),
            None,
            None,
        );

        // It cannot sit below the unpinned rows — clamp to the front.
        assert_eq!(result.0, 0);
        assert_eq!(
            result.1,
            SidebarDropIndicator::new(Some(a), SidebarDropEdge::Top)
        );
    }

    /// Ported from `testCrossWindowInsertionClampsPinnedWorkspaceIntoPinnedRegion`
    /// (`SidebarOrderingTests.swift:933-948`).
    #[test]
    fn cross_window_insertion_clamps_pinned_workspace_into_pinned_region() {
        let (pinned_a, unpinned_a, unpinned_b) = (uid(1), uid(2), uid(3));
        let result = SidebarDropPlanner::new().cross_window_insertion(
            Some(unpinned_b),
            true,
            Some(SidebarDropIndicator::new(None, SidebarDropEdge::Bottom)),
            &[pinned_a, unpinned_a, unpinned_b],
            &pinned(&[pinned_a]),
            None,
            None,
        );

        // A pinned workspace cannot land below the single pinned row.
        assert_eq!(result.0, 1);
        assert_eq!(
            result.1,
            SidebarDropIndicator::new(Some(unpinned_a), SidebarDropEdge::Top)
        );
    }

    /// Ported from `testCrossWindowInsertionRecoversIndicatorPositionAtDropTime`
    /// (`SidebarOrderingTests.swift:950-965`).
    #[test]
    fn cross_window_insertion_recovers_indicator_position_at_drop_time() {
        let (a, b, c) = (uid(1), uid(2), uid(3));
        let result = SidebarDropPlanner::new().cross_window_insertion(
            Some(b),
            false,
            Some(SidebarDropIndicator::new(Some(c), SidebarDropEdge::Top)),
            &[a, b, c],
            &pinned(&[]),
            None,
            None,
        );

        assert_eq!(result.0, 2);
        assert_eq!(
            result.1,
            SidebarDropIndicator::new(Some(c), SidebarDropEdge::Top)
        );
    }

    // MARK: - shouldCollect / workspaceAction
    //         (SidebarWorkspaceDropPlannerTests.swift:14-305)

    /// Ported from `testWorkspaceDropTargetCollectionStaysDisabledWhenNoDragIsActive`
    /// (`SidebarWorkspaceDropPlannerTests.swift:14-16`).
    #[test]
    fn workspace_drop_target_collection_stays_disabled_when_no_drag_is_active() {
        assert!(!SidebarDropPlanner::new().should_collect_workspace_drop_targets(None, false));
    }

    /// Ported from `testWorkspaceDropTargetCollectionTurnsOnDuringDrag`
    /// (`SidebarWorkspaceDropPlannerTests.swift:18-20`).
    #[test]
    fn workspace_drop_target_collection_turns_on_during_drag() {
        assert!(
            SidebarDropPlanner::new().should_collect_workspace_drop_targets(Some(uid(1)), false)
        );
    }

    /// Ported from `testWorkspaceDropTargetCollectionTurnsOnDuringBonsplitWorkspaceDrop`
    /// (`SidebarWorkspaceDropPlannerTests.swift:22-27`).
    #[test]
    fn workspace_drop_target_collection_turns_on_during_bonsplit_workspace_drop() {
        assert!(SidebarDropPlanner::new().should_collect_workspace_drop_targets(None, true));
    }

    /// Ported from `testWorkspaceDropCenterTargetsExistingWorkspace`
    /// (`SidebarWorkspaceDropPlannerTests.swift:198-209`).
    #[test]
    fn workspace_drop_center_targets_existing_workspace() {
        let (first, second) = (uid(1), uid(2));
        let targets = workspace_drop_targets(&[first, second], &[]);

        let action = SidebarDropPlanner::new().workspace_action(Point::new(12.0, 56.0), &targets);

        assert_eq!(action, Some(WorkspaceDropAction::ExistingWorkspace(second)));
    }

    /// Ported from `testWorkspaceDropTopEdgeCreatesWorkspaceBeforeTarget`
    /// (`SidebarWorkspaceDropPlannerTests.swift:211-228`).
    #[test]
    fn workspace_drop_top_edge_creates_workspace_before_target() {
        let (first, second) = (uid(1), uid(2));
        let targets = workspace_drop_targets(&[first, second], &[]);

        let action = SidebarDropPlanner::new().workspace_action(Point::new(12.0, 42.0), &targets);

        assert_eq!(
            action,
            Some(WorkspaceDropAction::NewWorkspace {
                insertion_index: 1,
                indicator: SidebarDropIndicator::new(Some(second), SidebarDropEdge::Top),
            })
        );
    }

    /// Ported from `testWorkspaceDropBottomEdgeCreatesWorkspaceAfterTarget`
    /// (`SidebarWorkspaceDropPlannerTests.swift:230-247`).
    #[test]
    fn workspace_drop_bottom_edge_creates_workspace_after_target() {
        let (first, second) = (uid(1), uid(2));
        let targets = workspace_drop_targets(&[first, second], &[]);

        let action = SidebarDropPlanner::new().workspace_action(Point::new(12.0, 65.0), &targets);

        assert_eq!(
            action,
            Some(WorkspaceDropAction::NewWorkspace {
                insertion_index: 2,
                indicator: SidebarDropIndicator::new(None, SidebarDropEdge::Bottom),
            })
        );
    }

    /// Ported from `testWorkspaceDropGapCreatesWorkspaceBeforeNextTarget`
    /// (`SidebarWorkspaceDropPlannerTests.swift:249-266`).
    #[test]
    fn workspace_drop_gap_creates_workspace_before_next_target() {
        let (first, second) = (uid(1), uid(2));
        let targets = workspace_drop_targets(&[first, second], &[]);

        let action = SidebarDropPlanner::new().workspace_action(Point::new(12.0, 36.0), &targets);

        assert_eq!(
            action,
            Some(WorkspaceDropAction::NewWorkspace {
                insertion_index: 1,
                indicator: SidebarDropIndicator::new(Some(second), SidebarDropEdge::Top),
            })
        );
    }

    /// Ported from `testWorkspaceDropAfterLastRowCreatesWorkspaceAtEnd`
    /// (`SidebarWorkspaceDropPlannerTests.swift:268-285`).
    #[test]
    fn workspace_drop_after_last_row_creates_workspace_at_end() {
        let (first, second) = (uid(1), uid(2));
        let targets = workspace_drop_targets(&[first, second], &[]);

        let action = SidebarDropPlanner::new().workspace_action(Point::new(12.0, 92.0), &targets);

        assert_eq!(
            action,
            Some(WorkspaceDropAction::NewWorkspace {
                insertion_index: 2,
                indicator: SidebarDropIndicator::new(None, SidebarDropEdge::Bottom),
            })
        );
    }

    /// Ported from `testWorkspaceDropKeepsNewWorkspaceAfterPinnedRows`
    /// (`SidebarWorkspaceDropPlannerTests.swift:287-305`).
    #[test]
    fn workspace_drop_keeps_new_workspace_after_pinned_rows() {
        let (pinned_a, pinned_b, unpinned) = (uid(1), uid(2), uid(3));
        let targets =
            workspace_drop_targets(&[pinned_a, pinned_b, unpinned], &[pinned_a, pinned_b]);

        let action = SidebarDropPlanner::new().workspace_action(Point::new(12.0, 2.0), &targets);

        assert_eq!(
            action,
            Some(WorkspaceDropAction::NewWorkspace {
                insertion_index: 2,
                indicator: SidebarDropIndicator::new(Some(unpinned), SidebarDropEdge::Top),
            })
        );
    }

    /// Empty targets yield no action (`SidebarDropPlanner.swift:192`).
    #[test]
    fn workspace_action_empty_targets_is_none() {
        assert_eq!(
            SidebarDropPlanner::new().workspace_action(Point::new(0.0, 0.0), &[]),
            None
        );
    }

    // MARK: - Parity-risk edge cases (no dedicated Swift oracle; hand-computed)

    /// `edgeForPointer` guards a non-positive height (`.top`) and clamps the
    /// pointer to `[0, targetHeight]` before the half-height split
    /// (`SidebarDropPlanner.swift:313-317`). The `< targetHeight/2` split makes
    /// the exact midpoint resolve to `.bottom`.
    #[test]
    fn edge_for_pointer_guards_and_clamps() {
        let planner = SidebarDropPlanner::new();
        assert_eq!(planner.edge_for_pointer(5.0, 0.0), SidebarDropEdge::Top);
        assert_eq!(planner.edge_for_pointer(-100.0, 40.0), SidebarDropEdge::Top);
        assert_eq!(
            planner.edge_for_pointer(1000.0, 40.0),
            SidebarDropEdge::Bottom
        );
        // Exactly half → not `< half` → `.bottom`.
        assert_eq!(
            planner.edge_for_pointer(20.0, 40.0),
            SidebarDropEdge::Bottom
        );
        assert_eq!(planner.edge_for_pointer(19.9, 40.0), SidebarDropEdge::Top);
    }

    /// The explicit `legalInsertionRange` clamp is applied last inside
    /// `legalInsertionPosition`: `min(max(clamped, lower), upper)`
    /// (`SidebarDropPlanner.swift:307-309`), *before* `resolvedTargetIndex`
    /// applies its source-removal shift. Moving `first` (index 0) to the end
    /// proposes insertion 3 → without a range that resolves to index 2; clamping
    /// the insertion into `1..=1` yields insertion 1 → resolved index 0 (the
    /// source-removal shift subtracts one because insertion > sourceIndex).
    #[test]
    fn legal_insertion_range_clamps() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let tab_ids = [first, second, third];

        // Without a range: move `first` to end → insertion 3, resolved index 2.
        assert_eq!(
            SidebarDropPlanner::new()
                .target_index(first, None, None, &tab_ids, &pinned(&[]), None,),
            Some(2)
        );
        // With range 1..=1: insertion clamps to 1, then the removal shift → 0.
        assert_eq!(
            SidebarDropPlanner::new().target_index(
                first,
                None,
                None,
                &tab_ids,
                &pinned(&[]),
                Some(1..=1),
            ),
            Some(0)
        );
    }

    /// `contains` mirrors `CGRectContainsPoint`: min-edge inclusive, max-edge
    /// exclusive.
    #[test]
    fn rect_contains_is_half_open() {
        let rect = Rect::new(0.0, 40.0, 180.0, 32.0);
        assert!(rect.contains(Point::new(0.0, 40.0))); // min corner inclusive
        assert!(!rect.contains(Point::new(180.0, 50.0))); // max-x exclusive
        assert!(!rect.contains(Point::new(10.0, 72.0))); // max-y exclusive
        assert!(rect.contains(Point::new(179.9, 71.9)));
        assert_eq!(rect.min_y(), 40.0);
        assert_eq!(rect.max_y(), 72.0);
    }

    /// `insertionPositionForIndicator` returns `None` when the indicator's
    /// `tabId` is absent from `tabIds`, so `targetIndex` falls through to the
    /// `targetTabId`/pointer branch (`SidebarDropPlanner.swift:64-74, 270-276`).
    #[test]
    fn target_index_falls_through_when_indicator_tab_missing() {
        let (first, second, third) = (uid(1), uid(2), uid(3));
        let stranger = uid(99);
        let tab_ids = [first, second, third];

        // Indicator points at a tab not in the list → recover fails → use target.
        // Move `first` onto `second`: preferred edge (from 0 < 1) = bottom →
        // insertion 2 → resolved index 1.
        let index = SidebarDropPlanner::new().target_index(
            first,
            Some(second),
            Some(SidebarDropIndicator::new(
                Some(stranger),
                SidebarDropEdge::Top,
            )),
            &tab_ids,
            &pinned(&[]),
            None,
        );
        assert_eq!(index, Some(1));
    }

    /// `targetIndex` returns `None` when the dragged tab is absent
    /// (`SidebarDropPlanner.swift:61`).
    #[test]
    fn target_index_none_when_dragged_absent() {
        let (first, second) = (uid(1), uid(2));
        assert_eq!(
            SidebarDropPlanner::new().target_index(
                uid(99),
                Some(first),
                None,
                &[first, second],
                &pinned(&[]),
                None,
            ),
            None
        );
    }
}
