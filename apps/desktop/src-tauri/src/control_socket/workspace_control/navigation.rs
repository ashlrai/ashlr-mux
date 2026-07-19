use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::control_socket) struct WorkspaceRelativeTarget {
    pub(in crate::control_socket) window_index: usize,
    pub(in crate::control_socket) workspace_index: usize,
    pub(in crate::control_socket) workspace_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::control_socket) enum WorkspaceNavigationTargetError {
    TabManagerUnavailable,
    NoWorkspaceSelected,
}

pub(in crate::control_socket) fn workspace_relative_target(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    active_window_id: Option<&str>,
    delta: i64,
) -> Result<WorkspaceRelativeTarget, WorkspaceNavigationTargetError> {
    let window_index =
        workspace_routed_window_index_with_active_window(snapshot, params, active_window_id)
            .ok_or(WorkspaceNavigationTargetError::TabManagerUnavailable)?;
    let window = snapshot
        .windows
        .get(window_index)
        .ok_or(WorkspaceNavigationTargetError::TabManagerUnavailable)?;
    let count = window.tab_manager.workspaces.len();
    let selected = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < count)
        .ok_or(WorkspaceNavigationTargetError::NoWorkspaceSelected)?;
    let workspace_index = (selected as i64 + delta).rem_euclid(count as i64) as usize;
    let workspace_id = window.tab_manager.workspaces[workspace_index]
        .workspace_id
        .clone()
        .ok_or(WorkspaceNavigationTargetError::NoWorkspaceSelected)?;
    Ok(WorkspaceRelativeTarget {
        window_index,
        workspace_index,
        workspace_id,
    })
}

fn navigation_error(error: WorkspaceNavigationTargetError) -> ControlCallResult {
    match error {
        WorkspaceNavigationTargetError::TabManagerUnavailable => ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        },
        WorkspaceNavigationTargetError::NoWorkspaceSelected => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No workspace selected".to_string(),
            data: None,
        },
    }
}

pub(in crate::control_socket) fn workspace_select_relative(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    delta: i64,
) -> ControlCallResult {
    let current = snapshot(app);
    let active_window_id = control_active_window_id(app);
    let target =
        match workspace_relative_target(&current, params, active_window_id.as_deref(), delta) {
            Ok(target) => target,
            Err(error) => return navigation_error(error),
        };
    if let Some(window_id) = workspace_select_focus_selector(&current, target.window_index) {
        let _ = handle_window_lifecycle_request(
            app,
            "window.focus",
            serde_json::Map::from_iter([("window_id".to_string(), json!(window_id))]),
        );
    }
    let state = app.state::<SessionState>();
    match select_workspace_in_window_for_control(
        app,
        &state,
        target.window_index,
        target.workspace_index,
        DerivedEventPolicy::Record,
    ) {
        Ok(result) => ok(workspace_identity_payload(
            app,
            &result.windows[target.window_index],
            &target.workspace_id,
        )),
        Err(PaneTopologyControlError::Operation(WorkspaceSelectControlError::WindowNotFound)) => {
            navigation_error(WorkspaceNavigationTargetError::TabManagerUnavailable)
        }
        Err(PaneTopologyControlError::Publication(message)) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(in crate::control_socket) fn workspace_last(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window_index) = workspace_routed_window_index_for_app(app, &current, params) else {
        return navigation_error(WorkspaceNavigationTargetError::TabManagerUnavailable);
    };
    let state = app.state::<SessionState>();
    let (workspace_id, result) = match select_last_workspace_for_control(app, &state, window_index)
    {
        Ok(result) => result,
        Err(WorkspaceLastControlError::TabManagerUnavailable) => {
            return navigation_error(WorkspaceNavigationTargetError::TabManagerUnavailable);
        }
        Err(WorkspaceLastControlError::NoPreviousWorkspace) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "No previous workspace in history".to_string(),
                data: None,
            };
        }
    };
    let window = &result.windows[window_index];
    if let Some(window_id) = window.window_id.as_deref() {
        let _ = crate::window::focus_control_window(app, window_id);
    }
    ok(workspace_identity_payload(app, window, &workspace_id))
}
