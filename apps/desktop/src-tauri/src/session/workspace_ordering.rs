use super::*;

pub(crate) fn move_workspace_to_top_for_notification_for_control(
    app: &AppHandle,
    state: &SessionState,
    workspace_id: &str,
) -> Result<(bool, AppSessionSnapshot, usize), String> {
    let result =
        transact_value_if_changed_suppressing_derived_events(
            app,
            state,
            |snapshot| -> Result<((bool, usize), bool), std::convert::Infallible> {
                for (window_index, window) in snapshot.windows.iter_mut().enumerate() {
                    let Some(index) = window.tab_manager.workspaces.iter().position(|workspace| {
                        workspace.workspace_id.as_deref() == Some(workspace_id)
                    }) else {
                        continue;
                    };
                    let changed = session_ops::move_workspace_to_top_for_notification(
                        &mut window.tab_manager,
                        index as i64,
                    );
                    return Ok(((changed, window_index), changed));
                }
                Ok(((false, 0), false))
            },
        );
    match result {
        Ok(((changed, window_index), snapshot)) => Ok((changed, snapshot, window_index)),
        Err(PaneTopologyControlError::Publication(message)) => Err(message),
        Err(PaneTopologyControlError::Operation(error)) => match error {},
    }
}

pub(crate) fn transact_value_if_changed_suppressing_derived_events<R, E>(
    app: &AppHandle,
    state: &SessionState,
    mutation: impl FnOnce(&mut AppSessionSnapshot) -> Result<(R, bool), E>,
) -> Result<(R, AppSessionSnapshot), PaneTopologyControlError<E>> {
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Suppress);
    transact_value_if_changed_snapshot(&state.snapshot, &mut publication, mutation)
}

pub(crate) fn reorder_workspaces_in_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    index: i64,
    to_index: i64,
    uses_top_level_rows: bool,
) -> Result<AppSessionSnapshot, String> {
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Suppress);
    transact_snapshot_if_changed(&state.snapshot, &mut publication, |snapshot| {
        apply_reorder_workspaces_in_window(
            snapshot,
            window_index,
            index,
            to_index,
            uses_top_level_rows,
        )
    })
}

pub(crate) fn reorder_workspaces_many_in_window_for_control(
    app: &AppHandle,
    state: &SessionState,
    window_index: usize,
    ordered_workspace_ids: &[Uuid],
    dry_run: bool,
) -> Result<
    (Vec<WorkspaceReorderPlanItem>, AppSessionSnapshot),
    PaneTopologyControlError<ReorderWorkspacesManyControlError>,
> {
    if dry_run {
        let mut snapshot = state
            .snapshot_for_lifecycle()
            .map_err(PaneTopologyControlError::Publication)?;
        let window =
            snapshot
                .windows
                .get_mut(window_index)
                .ok_or(PaneTopologyControlError::Operation(
                    ReorderWorkspacesManyControlError::Unavailable,
                ))?;
        let plan = session_ops::reorder_workspaces_many(
            &mut window.tab_manager,
            ordered_workspace_ids,
            false,
        )
        .map_err(ReorderWorkspacesManyControlError::Batch)
        .map_err(PaneTopologyControlError::Operation)?;
        return Ok((plan, snapshot));
    }
    let mut publication =
        ProductionSnapshotPublicationOperations::new(app, state, DerivedEventPolicy::Suppress);
    transact_value_if_changed_snapshot(&state.snapshot, &mut publication, |snapshot| {
        let window = snapshot
            .windows
            .get_mut(window_index)
            .ok_or(ReorderWorkspacesManyControlError::Unavailable)?;
        let plan = session_ops::reorder_workspaces_many(
            &mut window.tab_manager,
            ordered_workspace_ids,
            false,
        )
        .map_err(ReorderWorkspacesManyControlError::Batch)?;
        let changed = plan.iter().any(|item| item.from_index != item.to_index);
        Ok((plan, changed))
    })
}

pub(crate) enum ReorderWorkspacesManyControlError {
    Unavailable,
    Batch(WorkspaceBatchReorderError),
}
