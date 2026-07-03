//! Port of `Reorder/WorkspaceReorderPlanner.swift`, `WorkspaceOrderSnapshot.swift`,
//! `WorkspaceReorderPlanItem.swift`, and `WorkspaceBatchReorderError.swift`.
//!
//! Pure batch-reorder planning over a snapshot of the window's tab order:
//! validate the request (duplicate before unknown), then compute the final id
//! order with the pinned-ahead-of-unpinned invariant and stable ordering for
//! workspaces the request does not mention. Applying the plan (rebuilding tabs,
//! renormalizing group sections) stays with the caller.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

/// The minimal per-workspace ordering facts a reorder plan needs: identity and
/// pinned state, captured in current tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceOrderSnapshot {
    /// The workspace's identity.
    pub id: Uuid,
    /// Whether the workspace is pinned (pinned rows stay ahead of unpinned).
    pub is_pinned: bool,
}

impl WorkspaceOrderSnapshot {
    /// Creates a snapshot entry.
    pub fn new(id: Uuid, is_pinned: bool) -> Self {
        Self { id, is_pinned }
    }
}

/// One planned workspace move: where the workspace sits now and where the
/// reorder will place it.
///
/// DIVERGENCE: Swift stores `fromIndex`/`toIndex` as `Int`; the port uses `i64`
/// to mirror the signed semantics faithfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceReorderPlanItem {
    /// The workspace being moved.
    pub workspace_id: Uuid,
    /// The workspace's current index.
    pub from_index: i64,
    /// The index the reorder will place it at.
    pub to_index: i64,
}

impl WorkspaceReorderPlanItem {
    /// Creates a plan item.
    pub fn new(workspace_id: Uuid, from_index: i64, to_index: i64) -> Self {
        Self {
            workspace_id,
            from_index,
            to_index,
        }
    }
}

/// Why a batch workspace reorder request was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkspaceBatchReorderError {
    /// The request listed the workspace more than once.
    #[error("workspace {0} listed more than once")]
    DuplicateWorkspace(Uuid),
    /// The request named a workspace that is not in this window.
    #[error("workspace {0} not found in window")]
    WorkspaceNotFound(Uuid),
}

/// Stateless batch-reorder planner. The window owns one as a plain value.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorkspaceReorderPlanner;

impl WorkspaceReorderPlanner {
    /// Creates the stateless planner.
    pub fn new() -> Self {
        Self
    }

    /// Validates `ordered_workspace_ids` against `current` and returns the
    /// per-workspace move plan, or the first validation failure (duplicate
    /// entry, then unknown workspace).
    pub fn batch_reorder_plan(
        &self,
        ordered_workspace_ids: &[Uuid],
        current: &[WorkspaceOrderSnapshot],
    ) -> Result<Vec<WorkspaceReorderPlanItem>, WorkspaceBatchReorderError> {
        let mut seen: HashSet<Uuid> = HashSet::new();
        for workspace_id in ordered_workspace_ids {
            if !seen.insert(*workspace_id) {
                return Err(WorkspaceBatchReorderError::DuplicateWorkspace(*workspace_id));
            }
        }

        let current_indexes: HashMap<Uuid, i64> = current
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id, i as i64))
            .collect();
        for workspace_id in ordered_workspace_ids {
            if !current_indexes.contains_key(workspace_id) {
                return Err(WorkspaceBatchReorderError::WorkspaceNotFound(*workspace_id));
            }
        }

        let final_ids = self.batch_reorder_final_ids(ordered_workspace_ids, current);
        let final_indexes: HashMap<Uuid, i64> = final_ids
            .iter()
            .enumerate()
            .map(|(i, id)| (*id, i as i64))
            .collect();

        let plan = ordered_workspace_ids
            .iter()
            .map(|workspace_id| {
                WorkspaceReorderPlanItem::new(
                    *workspace_id,
                    current_indexes.get(workspace_id).copied().unwrap_or(0),
                    final_indexes.get(workspace_id).copied().unwrap_or(0),
                )
            })
            .collect();
        Ok(plan)
    }

    /// Computes the full final id order for the batch reorder: requested pinned
    /// ids, remaining pinned ids in current order, requested unpinned ids,
    /// remaining unpinned ids in current order.
    pub fn batch_reorder_final_ids(
        &self,
        ordered_workspace_ids: &[Uuid],
        current: &[WorkspaceOrderSnapshot],
    ) -> Vec<Uuid> {
        let ordered_set: HashSet<Uuid> = ordered_workspace_ids.iter().copied().collect();
        let snapshots_by_id: HashMap<Uuid, &WorkspaceOrderSnapshot> =
            current.iter().map(|s| (s.id, s)).collect();
        let is_pinned = |id: &Uuid| snapshots_by_id.get(id).map(|s| s.is_pinned) == Some(true);
        let is_unpinned = |id: &Uuid| snapshots_by_id.get(id).map(|s| s.is_pinned) == Some(false);

        let ordered_pinned_ids = ordered_workspace_ids.iter().copied().filter(is_pinned);
        let ordered_unpinned_ids = ordered_workspace_ids.iter().copied().filter(is_unpinned);
        let remaining_pinned_ids = current
            .iter()
            .map(|s| s.id)
            .filter(|id| !ordered_set.contains(id) && is_pinned(id));
        let remaining_unpinned_ids = current
            .iter()
            .map(|s| s.id)
            .filter(|id| !ordered_set.contains(id) && is_unpinned(id));

        ordered_pinned_ids
            .chain(remaining_pinned_ids)
            .chain(ordered_unpinned_ids)
            .chain(remaining_unpinned_ids)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(id: Uuid, pinned: bool) -> WorkspaceOrderSnapshot {
        WorkspaceOrderSnapshot::new(id, pinned)
    }

    #[test]
    fn plan_moves_requested_unpinned_workspaces_ahead_of_unmentioned_ones() {
        // Port of planMovesRequestedUnpinnedWorkspacesAheadOfUnmentionedOnes.
        let all: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let current: Vec<_> = all.iter().map(|id| snap(*id, false)).collect();
        let planner = WorkspaceReorderPlanner::new();

        let plan = planner
            .batch_reorder_plan(&[all[2], all[0]], &current)
            .expect("success");
        assert_eq!(
            plan,
            vec![
                WorkspaceReorderPlanItem::new(all[2], 2, 0),
                WorkspaceReorderPlanItem::new(all[0], 0, 1),
            ]
        );
        assert_eq!(
            planner.batch_reorder_final_ids(&[all[2], all[0]], &current),
            vec![all[2], all[0], all[1]]
        );
    }

    #[test]
    fn plan_keeps_pinned_workspaces_ahead_of_unpinned() {
        // Port of planKeepsPinnedWorkspacesAheadOfUnpinned.
        let pinned_a = Uuid::new_v4();
        let pinned_b = Uuid::new_v4();
        let unpinned_a = Uuid::new_v4();
        let unpinned_b = Uuid::new_v4();
        let current = vec![
            snap(pinned_a, true),
            snap(pinned_b, true),
            snap(unpinned_a, false),
            snap(unpinned_b, false),
        ];
        let planner = WorkspaceReorderPlanner::new();

        // Request interleaves an unpinned ahead of a pinned; final order still
        // puts every pinned id ahead of every unpinned id.
        let final_ids = planner.batch_reorder_final_ids(&[unpinned_b, pinned_b], &current);
        assert_eq!(final_ids, vec![pinned_b, pinned_a, unpinned_b, unpinned_a]);
    }

    #[test]
    fn plan_rejects_duplicates_before_unknown_workspaces() {
        // Port of planRejectsDuplicatesBeforeUnknownWorkspaces.
        let known = Uuid::new_v4();
        let unknown = Uuid::new_v4();
        let current = vec![snap(known, false)];
        let planner = WorkspaceReorderPlanner::new();

        let duplicate = planner.batch_reorder_plan(&[known, known, unknown], &current);
        assert_eq!(
            duplicate,
            Err(WorkspaceBatchReorderError::DuplicateWorkspace(known))
        );

        let missing = planner.batch_reorder_plan(&[unknown], &current);
        assert_eq!(
            missing,
            Err(WorkspaceBatchReorderError::WorkspaceNotFound(unknown))
        );
    }

    #[test]
    fn empty_request_plans_no_moves() {
        // Port of emptyRequestPlansNoMoves.
        let all: Vec<Uuid> = (0..2).map(|_| Uuid::new_v4()).collect();
        let current: Vec<_> = all.iter().map(|id| snap(*id, false)).collect();
        let planner = WorkspaceReorderPlanner::new();

        assert_eq!(planner.batch_reorder_plan(&[], &current), Ok(vec![]));
        assert_eq!(planner.batch_reorder_final_ids(&[], &current), all);
    }
}
