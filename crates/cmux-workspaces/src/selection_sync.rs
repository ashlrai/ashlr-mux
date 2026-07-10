//! Port of `CmuxFoundation/Sidebar/SidebarWorkspaceSelectionSyncPolicy.swift`.
//!
//! Pure policy reconciling the sidebar's multi-workspace selection against the
//! live workspace list, and computing shift-click anchor indices. Operates only
//! on workspace UUIDs and indices; holds no state and touches no UI.
//!
//! DIVERGENCE: the Swift methods hang off a stateless `struct`; the port exposes
//! them as free functions (there is no state to carry). Index parameters/returns
//! use `i64` to mirror Swift's signed `Int` and its `indices.contains` bounds
//! check (which tolerates arbitrary/negative inputs).

use std::collections::HashSet;

use uuid::Uuid;

fn first_index_of(live_workspace_ids: &[Uuid], target: Uuid) -> Option<i64> {
    live_workspace_ids
        .iter()
        .position(|id| *id == target)
        .map(|i| i as i64)
}

fn first_index_where_selected(
    live_workspace_ids: &[Uuid],
    selected_workspace_ids: &HashSet<Uuid>,
) -> Option<i64> {
    live_workspace_ids
        .iter()
        .position(|id| selected_workspace_ids.contains(id))
        .map(|i| i as i64)
}

fn contains_index(live_workspace_ids: &[Uuid], index: i64) -> bool {
    index >= 0 && (index as usize) < live_workspace_ids.len()
}

/// Filters a previous selection down to workspaces that still exist, falling
/// back to the provided selected workspace when nothing survives.
pub fn reconciled_selection(
    previous_selection_ids: &HashSet<Uuid>,
    live_workspace_ids: &[Uuid],
    fallback_selected_workspace_id: Option<Uuid>,
) -> HashSet<Uuid> {
    let live_id_set: HashSet<Uuid> = live_workspace_ids.iter().copied().collect();
    let live_selection_ids: HashSet<Uuid> = previous_selection_ids
        .iter()
        .copied()
        .filter(|id| live_id_set.contains(id))
        .collect();
    if !live_selection_ids.is_empty() {
        return live_selection_ids;
    }
    if let Some(fallback) = fallback_selected_workspace_id {
        if live_id_set.contains(&fallback) {
            return HashSet::from([fallback]);
        }
    }
    HashSet::new()
}

/// Index of the preferred (or first selected) workspace in the live list.
pub fn anchor_index(
    preferred_workspace_id: Option<Uuid>,
    selected_workspace_ids: &HashSet<Uuid>,
    live_workspace_ids: &[Uuid],
) -> Option<i64> {
    if let Some(preferred) = preferred_workspace_id {
        if selected_workspace_ids.contains(&preferred) {
            if let Some(index) = first_index_of(live_workspace_ids, preferred) {
                return Some(index);
            }
        }
    }
    first_index_where_selected(live_workspace_ids, selected_workspace_ids)
}

/// Workspace id at an existing anchor index, if the index is still valid.
pub fn anchor_workspace_id(
    existing_anchor_index: Option<i64>,
    live_workspace_ids: &[Uuid],
) -> Option<Uuid> {
    let index = existing_anchor_index?;
    if !contains_index(live_workspace_ids, index) {
        return None;
    }
    Some(live_workspace_ids[index as usize])
}

/// Anchor index to use for a shift-click range, deriving one from the current
/// selection or focus when no anchor exists yet.
pub fn shift_click_anchor_index(
    existing_anchor_index: Option<i64>,
    selected_workspace_ids: &HashSet<Uuid>,
    focused_workspace_id: Option<Uuid>,
    live_workspace_ids: &[Uuid],
) -> Option<i64> {
    if let Some(existing) = existing_anchor_index {
        if contains_index(live_workspace_ids, existing) {
            return Some(existing);
        }
    }
    if selected_workspace_ids.len() == 1 {
        if let Some(selected) = selected_workspace_ids.iter().next() {
            if let Some(index) = first_index_of(live_workspace_ids, *selected) {
                return Some(index);
            }
        }
    }
    if let Some(focused) = focused_workspace_id {
        return first_index_of(live_workspace_ids, focused);
    }
    None
}

/// Resulting anchor index after a workspace click (shift vs plain).
pub fn anchor_index_after_workspace_click(
    is_shift_click: bool,
    resolved_shift_anchor_index: Option<i64>,
    clicked_index: i64,
) -> i64 {
    if is_shift_click {
        resolved_shift_anchor_index.unwrap_or(clicked_index)
    } else {
        clicked_index
    }
}

/// Anchor index to preserve after the workspace list is reordered.
pub fn anchor_index_after_workspace_reorder(
    preferred_anchor_workspace_id: Option<Uuid>,
    selected_workspace_ids: &HashSet<Uuid>,
    focused_workspace_id: Option<Uuid>,
    live_workspace_ids: &[Uuid],
) -> Option<i64> {
    if let Some(preferred) = preferred_anchor_workspace_id {
        if selected_workspace_ids.contains(&preferred) {
            if let Some(index) = first_index_of(live_workspace_ids, preferred) {
                return Some(index);
            }
        }
    }
    anchor_index(
        focused_workspace_id,
        selected_workspace_ids,
        live_workspace_ids,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[Uuid]) -> HashSet<Uuid> {
        ids.iter().copied().collect()
    }

    #[test]
    fn reconciled_selection_drops_stale_ids() {
        // Restore edge case: ids missing from the live list are dropped.
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let stale = Uuid::new_v4();
        let live = vec![a, b];
        let out = reconciled_selection(&set(&[a, stale]), &live, None);
        assert_eq!(out, set(&[a]));
    }

    #[test]
    fn reconciled_selection_falls_back_when_nothing_survives() {
        let a = Uuid::new_v4();
        let fallback = Uuid::new_v4();
        let stale = Uuid::new_v4();
        let live = vec![a, fallback];
        // No previous id survives → fall back to the live fallback.
        let out = reconciled_selection(&set(&[stale]), &live, Some(fallback));
        assert_eq!(out, set(&[fallback]));
        // Fallback also missing → empty.
        let missing_fallback = Uuid::new_v4();
        assert!(reconciled_selection(&set(&[stale]), &live, Some(missing_fallback)).is_empty());
        // No fallback → empty.
        assert!(reconciled_selection(&set(&[stale]), &live, None).is_empty());
    }

    #[test]
    fn anchor_index_prefers_preferred_then_first_selected() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let live = vec![a, b, c];
        // Preferred is selected → its index.
        assert_eq!(anchor_index(Some(c), &set(&[b, c]), &live), Some(2));
        // Preferred not selected → first selected in live order (b at 1).
        assert_eq!(anchor_index(Some(a), &set(&[b, c]), &live), Some(1));
        // No preferred → first selected.
        assert_eq!(anchor_index(None, &set(&[c]), &live), Some(2));
        // None selected → None.
        assert_eq!(anchor_index(None, &set(&[]), &live), None);
    }

    #[test]
    fn anchor_workspace_id_respects_bounds() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let live = vec![a, b];
        assert_eq!(anchor_workspace_id(Some(1), &live), Some(b));
        assert_eq!(anchor_workspace_id(Some(2), &live), None);
        assert_eq!(anchor_workspace_id(Some(-1), &live), None);
        assert_eq!(anchor_workspace_id(None, &live), None);
    }

    #[test]
    fn shift_click_anchor_index_derivation_order() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let focused = Uuid::new_v4();
        let live = vec![a, b, focused];
        // Existing valid index wins.
        assert_eq!(
            shift_click_anchor_index(Some(1), &set(&[]), None, &live),
            Some(1)
        );
        // Stale existing index → derive from single selection.
        assert_eq!(
            shift_click_anchor_index(Some(99), &set(&[b]), None, &live),
            Some(1)
        );
        // Multi-selection ignored → fall to focus.
        assert_eq!(
            shift_click_anchor_index(None, &set(&[a, b]), Some(focused), &live),
            Some(2)
        );
        // Nothing to derive from → None.
        assert_eq!(
            shift_click_anchor_index(None, &set(&[a, b]), None, &live),
            None
        );
    }

    #[test]
    fn anchor_index_after_click_switches_on_shift() {
        assert_eq!(anchor_index_after_workspace_click(false, Some(3), 5), 5);
        assert_eq!(anchor_index_after_workspace_click(true, Some(3), 5), 3);
        // Shift with no resolved anchor → clicked index.
        assert_eq!(anchor_index_after_workspace_click(true, None, 5), 5);
    }

    #[test]
    fn anchor_index_after_reorder_prefers_preferred() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let live = vec![a, b];
        // Preferred selected → its post-reorder index.
        assert_eq!(
            anchor_index_after_workspace_reorder(Some(b), &set(&[a, b]), None, &live),
            Some(1)
        );
        // Preferred not selected → fall to focus/first-selected path.
        assert_eq!(
            anchor_index_after_workspace_reorder(Some(a), &set(&[b]), Some(b), &live),
            Some(1)
        );
    }
}
