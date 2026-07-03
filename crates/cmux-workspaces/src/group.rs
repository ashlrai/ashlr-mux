//! Port of `Values/WorkspaceGroup.swift`.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Named collapsible sidebar group containing one or more workspaces.
///
/// The membership relation lives on `WorkspaceRow.group_id`; this struct stores
/// the group's identity, display name, collapse/pin state, and the explicit
/// anchor workspace whose lifecycle gates the group itself.
///
/// The anchor workspace is always a real member workspace. It is rendered
/// IMPLICITLY as the group header (no separate sidebar row), and when closed
/// dissolves the group while keeping other members alive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceGroup {
    /// The group's stable identity.
    pub id: Uuid,
    /// The group's display name.
    pub name: String,
    /// Whether the group's member rows are collapsed in the sidebar.
    pub is_collapsed: bool,
    /// Whether the group is pinned.
    pub is_pinned: bool,
    /// Identifier of the member workspace that owns this group's lifecycle.
    /// Always points to a workspace whose `group_id == self.id`. Closing this
    /// workspace dissolves the group.
    pub anchor_workspace_id: Uuid,
    /// Group-level color override (hex string). When `None`, host wiring falls
    /// back to the cwd-config color, then to no tint.
    pub custom_color: Option<String>,
    /// SF symbol name for the header icon. When `None`, defaults to
    /// `folder.fill` at the host layer.
    pub icon_symbol: Option<String>,
}

impl WorkspaceGroup {
    /// Creates a group (memberwise; mirrors the Swift value shape).
    pub fn new(
        id: Uuid,
        name: String,
        is_collapsed: bool,
        is_pinned: bool,
        anchor_workspace_id: Uuid,
        custom_color: Option<String>,
        icon_symbol: Option<String>,
    ) -> Self {
        Self {
            id,
            name,
            is_collapsed,
            is_pinned,
            anchor_workspace_id,
            custom_color,
            icon_symbol,
        }
    }
}
