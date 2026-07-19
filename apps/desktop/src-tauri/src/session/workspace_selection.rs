use super::*;

pub(super) fn select_workspace_in_window_candidate(
    snapshot: &mut AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) -> Result<((), bool), WorkspaceSelectControlError> {
    let window = snapshot
        .windows
        .get_mut(window_index)
        .ok_or(WorkspaceSelectControlError::WindowNotFound)?;
    if workspace_index >= window.tab_manager.workspaces.len() {
        return Ok(((), false));
    }
    let target_workspace_id = window.tab_manager.workspaces[workspace_index]
        .workspace_id
        .as_deref();
    let changed = window.tab_manager.selected_workspace_index != Some(workspace_index as i64)
        || window.selected_workspace_id.as_deref() != target_workspace_id;
    if changed {
        session_ops::select_workspace(&mut window.tab_manager, workspace_index as i64);
        sync_window_selected_workspace_id(window);
    }
    Ok(((), changed))
}
