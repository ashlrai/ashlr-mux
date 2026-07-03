//! Port of the pure `insertionIndex` arithmetic from
//! app `Sources/WorkspacePlacement+Resolution.swift`.
//!
//! `effectivePlacement` (settings reads) is intentionally left to host wiring;
//! only the pure index arithmetic is ported here, reusing
//! [`cmux_config::NewWorkspacePlacement`].
//!
//! DIVERGENCE: Swift uses `Int` throughout; the port uses `i64` to mirror the
//! signed clamping (`max(0, min(...))`) exactly.

use cmux_config::NewWorkspacePlacement;

/// The insertion index for a new workspace under `placement`, given the current
/// selection and pinned-prefix shape of the tab list. Pure arithmetic; clamps
/// every input into the valid range.
pub fn insertion_index(
    placement: NewWorkspacePlacement,
    selected_index: Option<i64>,
    selected_is_pinned: bool,
    pinned_count: i64,
    total_count: i64,
) -> i64 {
    let clamped_total_count = total_count.max(0);
    let clamped_pinned_count = pinned_count.min(clamped_total_count).max(0);

    match placement {
        // Keep pinned workspaces grouped at the top by inserting ahead of
        // unpinned items.
        NewWorkspacePlacement::Top => clamped_pinned_count,
        NewWorkspacePlacement::End => clamped_total_count,
        NewWorkspacePlacement::AfterCurrent => {
            let Some(selected_index) = selected_index else {
                return clamped_total_count;
            };
            if clamped_total_count == 0 {
                return clamped_total_count;
            }
            let clamped_selected_index = selected_index.min(clamped_total_count - 1).max(0);
            if selected_is_pinned {
                return clamped_pinned_count;
            }
            (clamped_selected_index + 1).min(clamped_total_count)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_inserts_after_pinned_prefix() {
        assert_eq!(
            insertion_index(NewWorkspacePlacement::Top, None, false, 2, 5),
            2
        );
        // pinnedCount clamps into [0, totalCount].
        assert_eq!(
            insertion_index(NewWorkspacePlacement::Top, None, false, 9, 3),
            3
        );
        assert_eq!(
            insertion_index(NewWorkspacePlacement::Top, None, false, -1, 3),
            0
        );
    }

    #[test]
    fn end_inserts_at_total() {
        assert_eq!(
            insertion_index(NewWorkspacePlacement::End, Some(0), false, 1, 4),
            4
        );
        // Negative total clamps to 0.
        assert_eq!(
            insertion_index(NewWorkspacePlacement::End, None, false, 0, -2),
            0
        );
    }

    #[test]
    fn after_current_places_after_selected_unpinned() {
        // selected at index 1 (unpinned) → insert at 2.
        assert_eq!(
            insertion_index(NewWorkspacePlacement::AfterCurrent, Some(1), false, 1, 5),
            2
        );
    }

    #[test]
    fn after_current_pinned_selection_inserts_at_pinned_boundary() {
        // A pinned selection inserts at the pinned boundary, not after itself.
        assert_eq!(
            insertion_index(NewWorkspacePlacement::AfterCurrent, Some(0), true, 2, 5),
            2
        );
    }

    #[test]
    fn after_current_without_selection_falls_back_to_end() {
        assert_eq!(
            insertion_index(NewWorkspacePlacement::AfterCurrent, None, false, 1, 5),
            5
        );
        // Empty list → 0 even with a (stale) selected index.
        assert_eq!(
            insertion_index(NewWorkspacePlacement::AfterCurrent, Some(3), false, 0, 0),
            0
        );
    }

    #[test]
    fn after_current_clamps_selected_index_into_range() {
        // Out-of-range selectedIndex clamps to totalCount-1, then +1 clamps to total.
        assert_eq!(
            insertion_index(NewWorkspacePlacement::AfterCurrent, Some(99), false, 0, 3),
            3
        );
        // Negative selectedIndex clamps to 0 → insert at 1.
        assert_eq!(
            insertion_index(NewWorkspacePlacement::AfterCurrent, Some(-4), false, 0, 3),
            1
        );
    }
}
