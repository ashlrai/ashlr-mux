use super::*;

pub(crate) fn break_pane_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    workspace_index: usize,
    panel_id: &str,
    focus: bool,
) -> Result<
    (session_ops::PaneBreakResult, AppSessionSnapshot),
    PaneTopologyControlError<session_ops::PaneBreakError>,
> {
    state.transact_pane_topology(app, |snapshot| {
        let tabs = &mut snapshot
            .windows
            .get_mut(window_index)
            .ok_or(session_ops::PaneBreakError::WorkspaceNotFound)?
            .tab_manager;
        let broken =
            session_ops::break_surface_to_new_workspace(tabs, workspace_index, panel_id, focus)?;
        if !finalize_broken_pane_snapshot(snapshot, window_index, &broken) {
            return Err(session_ops::PaneBreakError::DetachFailed);
        }
        Ok(broken)
    })
}

pub(crate) fn finalize_broken_pane_snapshot(
    snapshot: &mut AppSessionSnapshot,
    window_index: usize,
    broken: &session_ops::PaneBreakResult,
) -> bool {
    if !remint_broken_pane_identity(snapshot, window_index, broken) {
        return false;
    }
    ensure_workspace_ids(snapshot);
    ensure_pane_ids(snapshot);
    true
}
