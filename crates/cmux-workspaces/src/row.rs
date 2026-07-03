//! A value snapshot mirroring the pin/group/identity fields the Swift
//! `WorkspaceTabRepresenting` seam exposes (`Model/WorkspaceTabRepresenting.swift`).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The pure per-workspace ("tab") facts the ordering / group / reorder
/// algorithms read: identity, group membership, and pin state.
///
/// DIVERGENCE: the Swift seam is a reference-type protocol whose `groupId` /
/// `isPinned` are mutated in place. This is an immutable value snapshot; the
/// invariant helpers return new `Vec<WorkspaceRow>`s instead of mutating. The
/// Swift `currentDirectory` field is intentionally omitted — it feeds only the
/// group-creation cwd inheritance that lives on the (host-wired) coordinators,
/// never the pure ordering read paths ported here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceRow {
    /// The workspace's stable identity.
    pub id: Uuid,
    /// The owning `WorkspaceGroup.id`, or `None` when ungrouped.
    pub group_id: Option<Uuid>,
    /// Whether the workspace is pinned (pinned rows float above unpinned).
    pub is_pinned: bool,
}

impl WorkspaceRow {
    /// Creates a workspace row snapshot.
    pub fn new(id: Uuid, group_id: Option<Uuid>, is_pinned: bool) -> Self {
        Self {
            id,
            group_id,
            is_pinned,
        }
    }
}
