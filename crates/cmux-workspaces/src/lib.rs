//! cmux-workspaces — pure workspace / tab / group runtime logic.
//!
//! Headless port of the pure ordering, group-invariant, batch-reorder, sidebar
//! render-projection, selection-sync, new-workspace-placement, and closed-item
//! history logic from the canonical macOS `Packages/macOS/CmuxWorkspaces` package
//! (+ a few app-side helpers under `Sources/`). Operates over `WorkspaceRow`
//! value snapshots — no GUI/GPU/agent/Tauri dependency.
//!
//! DIVERGENCE: Swift mutates the reference-type `Tab.groupId` (and reassigns
//! `WorkspacesModel.tabs` / `.workspaceGroups`) in place inside the model. The
//! Rust port has no reference-type workspaces, so every "mutating" invariant
//! helper is a pure function returning a NEW ordered `Vec<WorkspaceRow>` (and,
//! where the Swift also reorders groups, a new `Vec<WorkspaceGroup>`). The
//! behavior is pinned against the Swift test vectors.

mod avatar;
mod closed_history;
mod focus_history;
mod group;
mod group_invariants;
mod mount_plan;
mod ordering;
mod placement;
mod render_items;
mod reorder;
mod row;
mod selection_sync;
mod session_restore_policy;
mod surface_list;
mod tab_colors;

// Reuse the config crate's placement enum rather than redefining it.
pub use cmux_config::NewWorkspacePlacement;

pub use avatar::{
    parse_hex_color, resolve_gradient_source, wrapped_palette_slot, AvatarColor,
    MachineAvatarGradient, MachineAvatarPalette,
};
pub use closed_history::{
    has_usable_restored_content, records_by_remapping_panel_anchor_ids,
    records_by_remapping_panel_workspace_ids, records_by_remapping_workspace_window_ids,
    records_by_removing_panel_records, ClosedItemHistory, ClosedItemHistoryEntry,
    ClosedItemHistoryRecord, ClosedPanelHistoryEntry, ClosedPanelSplitPlacement,
    ClosedWindowHistoryEntry, ClosedWorkspaceHistoryEntry, MenuSnapshot, PanelSnapshot,
    SplitOrientation, WindowSnapshot, WorkspaceSnapshot,
};
pub use focus_history::{
    FocusHistoryEntry, FocusHistoryHost, FocusHistoryMenuDirection, FocusHistoryMenuItem,
    FocusHistoryMenuPosition, FocusHistoryMenuSnapshot, FocusHistoryModel, FocusHistoryRecord,
    FocusedAt,
};
pub use group::WorkspaceGroup;
pub use group_invariants::{
    assign_group, dissolve_groups_anchored_by, expand_workspace_group_for_selection_if_needed,
    move_workspace_group_members_after_anchors, normalize_workspace_group_contiguity,
    normalize_workspace_group_runs_preserving_order, sync_workspace_groups_order_to_anchor_order,
};
pub use mount_plan::WorkspaceMountPlan;
pub use ordering::{
    anchor_first, clamped_grouped_member_reorder_index, clamped_reorder_index,
    clamped_top_level_reorder_index, is_global_pinned_row, is_workspace_group_anchor,
    leading_global_pinned_row_count, sidebar_top_level_pinned_workspace_ids,
    sidebar_top_level_workspace_ids, top_level_workspace_ids,
    top_level_workspace_ids_preserving_order,
};
pub use placement::insertion_index;
pub use render_items::{render_items, SidebarWorkspaceRenderItem, SidebarWorkspaceRenderItemId};
pub use reorder::{
    WorkspaceBatchReorderError, WorkspaceOrderSnapshot, WorkspaceReorderPlanItem,
    WorkspaceReorderPlanner,
};
pub use row::WorkspaceRow;
pub use selection_sync::{
    anchor_index, anchor_index_after_workspace_click, anchor_index_after_workspace_reorder,
    anchor_workspace_id, reconciled_selection, shift_click_anchor_index,
};
pub use session_restore_policy::{
    WorkspaceHermesCodexEnvironment, WorkspaceSessionRemoteRestorePanelSnapshot,
    WorkspaceSessionRemoteRestoreSnapshot, WorkspaceSessionRemoteRestoreTerminalSnapshot,
    WorkspaceSessionRestorePolicyService, WorkspaceSurfaceResumeBinding,
    WorkspaceSurfaceResumeStartupLaunch,
};
pub use surface_list::{Pane, SurfaceTree};
pub use tab_colors::{
    add_custom_color, backup_palette_map, brightened_for_dark_appearance_rgb,
    custom_palette_entries, current_color_hex, default_color_hex, default_palette,
    display_color_hex, effective_palette_map, finder_like_cmp, invalid_color_message, luminance,
    normalize_hex, normalized_color_name, normalized_custom_color, palette,
    palette_cache_fingerprint, persist_palette_map, remove_color, resolve_set_color_input,
    resolved_color_hex, set_color, PaletteStoreSnapshot, PalettePersistOutcome, SetColorError,
    TabColorEntry, DEFAULT_PALETTE, INVALID_COLOR_MESSAGE, LEGACY_CUSTOM_COLORS_KEY,
    LEGACY_DEFAULT_OVERRIDES_KEY, MISSING_COLOR_MESSAGE, PALETTE_KEY,
};
