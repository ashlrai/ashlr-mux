//! Port of `Packages/macOS/CmuxFoundation/Sources/CmuxFoundation/Workspace/WorkspaceMountPlan.swift`.
//!
//! Value object deciding which workspaces stay mounted to minimize layer-tree
//! traversal. Operates only on workspace UUIDs, ordering, and pinning flags;
//! holds no state and touches no UI. Construct it with the current mount state
//! and read [`WorkspaceMountPlan::mounted_workspace_ids`].
//!
//! DIVERGENCE: none. The struct, its constants, and the `mountedWorkspaceIds`
//! computed property are mirrored 1:1. Swift's `Set<UUID>` for `pinnedIds` is a
//! `HashSet<Uuid>`; the pinned iteration is deterministic because it is sorted
//! by `orderedTabIds` index before use (Swift file:63-69).

use std::collections::HashSet;

use uuid::Uuid;

/// Value object deciding which workspaces stay mounted.
///
/// Swift `WorkspaceMountPlan` (`WorkspaceMountPlan.swift:7`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceMountPlan {
    current: Vec<Uuid>,
    selected: Option<Uuid>,
    pinned_ids: HashSet<Uuid>,
    ordered_tab_ids: Vec<Uuid>,
    is_cycle_hot: bool,
    max_mounted: i64,
}

impl WorkspaceMountPlan {
    /// Keep only the selected workspace mounted to minimize layer-tree traversal.
    ///
    /// Swift `maxMountedWorkspaces` (`WorkspaceMountPlan.swift:9`).
    pub const MAX_MOUNTED_WORKSPACES: i64 = 1;

    /// During workspace cycling, keep only a minimal handoff pair (selected + retiring).
    ///
    /// Swift `maxMountedWorkspacesDuringCycle` (`WorkspaceMountPlan.swift:11`).
    pub const MAX_MOUNTED_WORKSPACES_DURING_CYCLE: i64 = 2;

    /// Swift `init(...)` (`WorkspaceMountPlan.swift:20-34`).
    pub fn new(
        current: Vec<Uuid>,
        selected: Option<Uuid>,
        pinned_ids: HashSet<Uuid>,
        ordered_tab_ids: Vec<Uuid>,
        is_cycle_hot: bool,
        max_mounted: i64,
    ) -> Self {
        Self {
            current,
            selected,
            pinned_ids,
            ordered_tab_ids,
            is_cycle_hot,
            max_mounted,
        }
    }

    /// The workspace ids that should remain mounted, in priority order.
    ///
    /// Swift `mountedWorkspaceIds` (`WorkspaceMountPlan.swift:37-87`).
    pub fn mounted_workspace_ids(&self) -> Vec<Uuid> {
        let existing: HashSet<Uuid> = self.ordered_tab_ids.iter().copied().collect();
        let clamped_max = self.max_mounted.max(1);
        let mut ordered: Vec<Uuid> = self
            .current
            .iter()
            .copied()
            .filter(|id| existing.contains(id))
            .collect();

        // Selected-to-front (`WorkspaceMountPlan.swift:42-45`).
        if let Some(selected) = self.selected {
            if existing.contains(&selected) {
                ordered.retain(|id| *id != selected);
                ordered.insert(0, selected);
            }
        }

        // Cycle-warm (`WorkspaceMountPlan.swift:47-53`).
        if self.is_cycle_hot {
            if let Some(selected) = self.selected {
                let warm_ids = Self::cycle_warm_ids(selected, &self.ordered_tab_ids);
                for id in warm_ids.iter().rev() {
                    ordered.retain(|existing_id| existing_id != id);
                    ordered.insert(0, *id);
                }
            }
        }

        // Cycle-hot with no pinned handoff → keep only the selected
        // (`WorkspaceMountPlan.swift:55-59`).
        if self.is_cycle_hot && self.pinned_ids.is_empty() {
            if let Some(selected) = self.selected {
                ordered.retain(|id| *id == selected);
            }
        }

        // Ensure pinned ids (retiring handoff workspaces) are always retained at
        // highest priority. Runs after warming to prevent neighbor warming from
        // evicting the retiring workspace (`WorkspaceMountPlan.swift:61-80`).
        let mut prioritized_pinned_ids: Vec<Uuid> = self
            .pinned_ids
            .iter()
            .copied()
            .filter(|id| existing.contains(id) && Some(*id) != self.selected)
            .collect();
        prioritized_pinned_ids.sort_by(|lhs, rhs| {
            let lhs_index = self
                .ordered_tab_ids
                .iter()
                .position(|id| id == lhs)
                .unwrap_or(usize::MAX);
            let rhs_index = self
                .ordered_tab_ids
                .iter()
                .position(|id| id == rhs)
                .unwrap_or(usize::MAX);
            lhs_index.cmp(&rhs_index)
        });

        if let Some(selected) = self.selected {
            if existing.contains(&selected) {
                ordered.retain(|id| *id != selected);
                ordered.insert(0, selected);
            }
        }

        // Swift uses `(selected != nil) ? 1 : 0` — NOT gated on `existing`
        // (`WorkspaceMountPlan.swift:74`). The counter increments once per pinned
        // id, so `base + i` reproduces Swift's running `pinnedInsertionIndex`.
        let base_insertion_index: usize = if self.selected.is_some() { 1 } else { 0 };
        for (i, pinned_id) in prioritized_pinned_ids.into_iter().enumerate() {
            ordered.retain(|id| *id != pinned_id);
            let insertion_index = (base_insertion_index + i).min(ordered.len());
            ordered.insert(insertion_index, pinned_id);
        }

        // Clamp to max (`WorkspaceMountPlan.swift:82-84`).
        if ordered.len() > clamped_max as usize {
            ordered.truncate(clamped_max as usize);
        }

        ordered
    }

    /// Swift `cycleWarmIds(selected:orderedTabIds:)` (`WorkspaceMountPlan.swift:89-94`).
    ///
    /// Keeps warming focused to the selected workspace; retiring/target
    /// workspaces are pinned by handoff logic, so warming adjacent neighbors
    /// here just adds layout work.
    fn cycle_warm_ids(selected: Uuid, ordered_tab_ids: &[Uuid]) -> Vec<Uuid> {
        if !ordered_tab_ids.contains(&selected) {
            return vec![selected];
        }
        vec![selected]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[Uuid]) -> HashSet<Uuid> {
        ids.iter().copied().collect()
    }

    // Swift `testDefaultPolicyMountsOnlySelectedWorkspace`
    // (`WorkspaceUnitTests.swift:7101`).
    #[test]
    fn default_policy_mounts_only_selected_workspace() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let ordered_tab_ids = vec![a, b];

        let next = WorkspaceMountPlan::new(
            vec![a],
            Some(b),
            set(&[]),
            ordered_tab_ids,
            false,
            WorkspaceMountPlan::MAX_MOUNTED_WORKSPACES,
        )
        .mounted_workspace_ids();

        assert_eq!(next, vec![b]);
    }

    // Swift `testSelectedWorkspaceMovesToFrontAndMountCountIsBounded`
    // (`WorkspaceUnitTests.swift:7118`).
    #[test]
    fn selected_workspace_moves_to_front_and_mount_count_is_bounded() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let ordered_tab_ids = vec![a, b, c];

        let next =
            WorkspaceMountPlan::new(vec![a, b, c], Some(c), set(&[]), ordered_tab_ids, false, 2)
                .mounted_workspace_ids();

        assert_eq!(next, vec![c, a]);
    }

    // Swift `testMissingWorkspacesArePruned` (`WorkspaceUnitTests.swift:7136`).
    #[test]
    fn missing_workspaces_are_pruned() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();

        let next = WorkspaceMountPlan::new(vec![b, a], None, set(&[]), vec![a], false, 2)
            .mounted_workspace_ids();

        assert_eq!(next, vec![a]);
    }

    // Swift `testSelectedWorkspaceIsInsertedWhenAbsentFromCurrentCache`
    // (`WorkspaceUnitTests.swift:7152`).
    #[test]
    fn selected_workspace_is_inserted_when_absent_from_current_cache() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let ordered_tab_ids = vec![a, b];

        let next = WorkspaceMountPlan::new(vec![a], Some(b), set(&[]), ordered_tab_ids, false, 2)
            .mounted_workspace_ids();

        assert_eq!(next, vec![b, a]);
    }

    // Swift `testMaxMountedIsClampedToAtLeastOne`
    // (`WorkspaceUnitTests.swift:7169`).
    #[test]
    fn max_mounted_is_clamped_to_at_least_one() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let ordered_tab_ids = vec![a, b];

        let next = WorkspaceMountPlan::new(vec![a, b], None, set(&[]), ordered_tab_ids, false, 0)
            .mounted_workspace_ids();

        assert_eq!(next, vec![a]);
    }

    // Swift `testCycleHotModeKeepsOnlySelectedWhenNoPinnedHandoff`
    // (`WorkspaceUnitTests.swift:7186`).
    #[test]
    fn cycle_hot_mode_keeps_only_selected_when_no_pinned_handoff() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let d = Uuid::new_v4();
        let ordered_tab_ids = vec![a, b, c, d];

        let next = WorkspaceMountPlan::new(
            vec![a],
            Some(c),
            set(&[]),
            ordered_tab_ids,
            true,
            WorkspaceMountPlan::MAX_MOUNTED_WORKSPACES_DURING_CYCLE,
        )
        .mounted_workspace_ids();

        assert_eq!(next, vec![c]);
    }

    // Swift `testCycleHotModeRespectsMaxMountedLimit`
    // (`WorkspaceUnitTests.swift:7205`).
    #[test]
    fn cycle_hot_mode_respects_max_mounted_limit() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let ordered_tab_ids = vec![a, b, c];

        let next =
            WorkspaceMountPlan::new(vec![a, b, c], Some(b), set(&[]), ordered_tab_ids, true, 2)
                .mounted_workspace_ids();

        assert_eq!(next, vec![b]);
    }

    // Swift `testPinnedIdsAreRetainedAcrossReconcile`
    // (`WorkspaceUnitTests.swift:7223`).
    #[test]
    fn pinned_ids_are_retained_across_reconcile() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let ordered_tab_ids = vec![a, b, c];

        let next = WorkspaceMountPlan::new(vec![a], Some(c), set(&[a]), ordered_tab_ids, false, 2)
            .mounted_workspace_ids();

        assert_eq!(next, vec![c, a]);
    }

    // Swift `testCycleHotModeKeepsRetiringWorkspaceWhenPinned`
    // (`WorkspaceUnitTests.swift:7241`).
    #[test]
    fn cycle_hot_mode_keeps_retiring_workspace_when_pinned() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let ordered_tab_ids = vec![a, b];

        let next = WorkspaceMountPlan::new(
            vec![a],
            Some(b),
            set(&[a]),
            ordered_tab_ids,
            true,
            WorkspaceMountPlan::MAX_MOUNTED_WORKSPACES_DURING_CYCLE,
        )
        .mounted_workspace_ids();

        assert_eq!(next, vec![b, a]);
    }

    // --- Parity-risk edge cases (hand-computed from the Swift formula). ---

    // Pinned insertion index starts at 1 whenever `selected.is_some()` — even
    // when the selected id is NOT in `existing` (Swift `WorkspaceMountPlan.swift:74`
    // gates only on `selected != nil`). Here `selected` is absent from
    // orderedTabIds, so it is never front-inserted; the pinned id (which IS in
    // existing) is inserted at min(1, len) and survives.
    #[test]
    fn pinned_insertion_index_is_one_when_selected_present_but_not_existing() {
        let a = Uuid::new_v4();
        let selected = Uuid::new_v4(); // not in orderedTabIds
        let ordered_tab_ids = vec![a];

        // ordered after filter = [a]; selected not in existing so no front-insert.
        // prioritized pinned = [a]; pinned_insertion_index starts at 1.
        // remove a → []; insert at min(1,0)=0 → [a]. clampedMax=2.
        let next = WorkspaceMountPlan::new(
            vec![a],
            Some(selected),
            set(&[a]),
            ordered_tab_ids,
            false,
            2,
        )
        .mounted_workspace_ids();

        assert_eq!(next, vec![a]);
    }

    // Multiple pinned ids are ordered by their orderedTabIds index (Swift
    // `WorkspaceMountPlan.swift:63-69`), inserted after the selected front slot.
    #[test]
    fn multiple_pinned_ids_ordered_by_tab_index() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let selected = Uuid::new_v4();
        // c precedes b in tab order; both pinned. selected is first.
        let ordered_tab_ids = vec![selected, c, b, a];

        // clampedMax large enough to keep all. Expect selected, then pinned by
        // tab-index order: c (idx1) before b (idx2).
        let next = WorkspaceMountPlan::new(
            vec![a, b, c],
            Some(selected),
            set(&[b, c]),
            ordered_tab_ids,
            false,
            10,
        )
        .mounted_workspace_ids();

        assert_eq!(next, vec![selected, c, b, a]);
    }

    // Empty everything → empty result (clampedMax has no bearing on an empty
    // ordered list).
    #[test]
    fn empty_inputs_yield_empty() {
        let next = WorkspaceMountPlan::new(vec![], None, set(&[]), vec![], false, 1)
            .mounted_workspace_ids();
        assert!(next.is_empty());
    }

    // No selection, no pinned: pure filter of `current` down to `existing`,
    // preserving current order, clamped. (Swift `WorkspaceMountPlan.swift:40`.)
    #[test]
    fn no_selection_preserves_current_order_filtered_to_existing() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let ordered_tab_ids = vec![a, b, c];

        let next =
            WorkspaceMountPlan::new(vec![c, a, b], None, set(&[]), ordered_tab_ids, false, 10)
                .mounted_workspace_ids();

        assert_eq!(next, vec![c, a, b]);
    }
}
