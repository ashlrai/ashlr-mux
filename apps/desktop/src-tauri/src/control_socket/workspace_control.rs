use super::*;

#[path = "workspace_control/activity_controls.rs"]
mod activity_controls;
pub(super) use activity_controls::*;

#[path = "workspace_control/create_params.rs"]
mod create_params;
pub(super) use create_params::{
    canonical_layout_is_valid, workspace_create_cwd_param, workspace_create_initial_env,
    workspace_create_workspace_env,
};

#[path = "workspace_control/events.rs"]
mod events;
use events::{record_workspace_create_events, record_workspace_selected_event};
#[cfg(test)]
pub(super) use events::{
    workspace_close_event_specs, workspace_create_event_specs, workspace_rename_event_spec,
    workspace_selected_event_spec,
};

#[path = "workspace_control/close.rs"]
mod close;
pub(super) use close::workspace_close;

#[path = "workspace_control/window_list.rs"]
mod window_list;
pub(super) use window_list::{window_list, workspace_list_with_recoverable_active};

#[path = "workspace_control/rename.rs"]
mod rename;
pub(super) use rename::workspace_rename;

#[path = "workspace_control/navigation.rs"]
mod navigation;
pub(super) use navigation::{workspace_last, workspace_select_relative};
#[cfg(test)]
pub(super) use navigation::{workspace_relative_target, WorkspaceNavigationTargetError};

pub(super) fn workspace_create(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window_index) = workspace_routed_window_index_for_app(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let inherited_directory = current.windows[window_index]
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| {
            current.windows[window_index]
                .tab_manager
                .workspaces
                .get(index)
        })
        .and_then(|workspace| workspace.current_directory.as_deref());
    let current_directory = match workspace_create_cwd_param(params, inherited_directory) {
        Ok(directory) => directory,
        Err(()) => return invalid_params("cwd must be a string"),
    };
    let initial_command = string_param(params, &["initial_command"]);
    let initial_environment = workspace_create_initial_env(params);
    let workspace_environment = workspace_create_workspace_env(params);
    let title = string_param(params, &["title"]);
    let description = raw_string_param(params, &["description"]);
    let group_id = string_param(params, &["group_id"]);
    if params.contains_key("group_id")
        && group_id
            .as_deref()
            .is_none_or(|group_id| Uuid::parse_str(group_id).is_err())
    {
        return invalid_params("Missing or invalid group_id");
    }
    if let Some(group_id) = group_id.as_deref() {
        let group_exists = current.windows[window_index]
            .tab_manager
            .workspace_groups
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|group| group.id == group_id);
        if !group_exists {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Group not found".to_string(),
                data: json!({"group_id": group_id}).try_into().ok(),
            };
        }
    }
    if params
        .get("layout")
        .is_some_and(|layout| !layout.is_object())
    {
        return invalid_params("layout must be a valid JSON object");
    }
    let layout = match params.get("layout") {
        Some(layout) => match serde_json::from_value::<cmux_config::CmuxLayoutNode>(layout.clone()) {
            Ok(layout) if canonical_layout_is_valid(&layout) => Some(layout),
            Ok(_) => return invalid_params("Invalid layout: every split requires exactly two children and every pane requires a surface"),
            Err(error) => return invalid_params(&format!("Invalid layout: {error}")),
        },
        None => None,
    };
    let placement = raw_string_param(params, &["group_placement", "placement"]);
    if params.contains_key("group_placement") || params.contains_key("placement") {
        if string_param(params, &["group_id"]).is_none() {
            return invalid_params("group_id is required for group placement");
        }
        let placement_value = placement.clone().unwrap_or_default();
        if !matches!(placement_value.as_str(), "afterCurrent" | "top" | "end") {
            return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: "Invalid group_placement".to_string(),
                data: json!({"group_placement": placement_value}).try_into().ok(),
            };
        }
    }
    let reference_key_present = params.contains_key("group_reference_workspace_id")
        || params.contains_key("reference_workspace_id");
    let reference_selector = raw_string_param(
        params,
        &["group_reference_workspace_id", "reference_workspace_id"],
    );
    if reference_key_present && reference_selector.is_none() {
        return invalid_params("Missing or invalid group_reference_workspace_id");
    }
    if reference_selector
        .as_deref()
        .is_some_and(|reference| Uuid::parse_str(reference).is_err())
    {
        return invalid_params("Missing or invalid group_reference_workspace_id");
    }
    let reference_index = reference_selector.as_deref().and_then(|selector| {
        resolve_workspace_identity_in_window(app, &current, window_index, selector)
    });
    if let (Some(group_id), Some(reference_selector)) =
        (group_id.as_deref(), reference_selector.as_deref())
    {
        let belongs = reference_index.is_some_and(|index| {
            current.windows[window_index].tab_manager.workspaces[index]
                .group_id
                .as_deref()
                == Some(group_id)
        });
        if !belongs {
            return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: "Reference workspace must be a member of the target group".to_string(),
                data: json!({"group_reference_workspace_id": reference_selector})
                    .try_into()
                    .ok(),
            };
        }
    }
    if placement.as_deref() == Some("afterCurrent") && reference_index.is_none() {
        return invalid_params("Missing or invalid group_reference_workspace_id");
    }
    let group_insert_index = group_id.as_deref().and_then(|group_id| {
        workspace_group_insert_index(
            &current.windows[window_index].tab_manager,
            group_id,
            placement.as_deref().unwrap_or("top"),
            reference_index,
        )
    });
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    let (result, created_index) = match new_workspace_in_window_for_control(
        app,
        &state,
        window_index,
        current_directory.as_deref(),
        layout
            .is_none()
            .then_some(initial_command)
            .flatten()
            .as_deref(),
        layout
            .is_none()
            .then_some(initial_environment)
            .filter(|env| !env.is_empty()),
        title.as_deref(),
        description.as_deref(),
        (!workspace_environment.is_empty()).then_some(workspace_environment),
        group_id.as_deref(),
        layout,
        group_insert_index,
        focus,
        DerivedEventPolicy::Suppress,
    ) {
        Ok(Some(created)) => created,
        Ok(None) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message: "Failed to create workspace".to_string(),
                data: None,
            };
        }
        Err(message) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    record_workspace_create_events(app, &current, &result, window_index, created_index, focus);
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[created_index];
    let surface_id = surfaces_for_workspace(workspace)
        .first()
        .and_then(|surface| surface.get("id"))
        .cloned()
        .unwrap_or(Value::Null);
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_deref().map(|id| control_handle_ref(app, "window", id)),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace.workspace_id.as_deref().map(|id| control_handle_ref(app, "workspace", id)),
        "group_id": workspace.group_id,
        "group_ref": workspace.group_id.as_deref().map(|id| control_handle_ref(app, "workspace_group", id)),
        "surface_id": surface_id,
        "surface_ref": surface_id.as_str().map(|id| control_handle_ref(app, "surface", id)),
    }))
}

pub(super) fn resolve_workspace_identity_in_window(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    selector: &str,
) -> Option<usize> {
    let id = if one_based_ref_index(selector, "workspace").is_some() {
        resolve_control_handle_ref(app, "workspace", selector)?
    } else {
        selector.to_string()
    };
    snapshot
        .windows
        .get(window_index)?
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(id.as_str()))
}

pub(super) fn workspace_group_insert_index(
    tabs: &cmux_core::session::SessionTabManagerSnapshot,
    group_id: &str,
    placement: &str,
    reference_index: Option<usize>,
) -> Option<usize> {
    let members: Vec<usize> = tabs
        .workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| {
            (workspace.group_id.as_deref() == Some(group_id)).then_some(index)
        })
        .collect();
    match placement {
        "top" => members.first().map(|index| index + 1),
        "end" => members.last().map(|index| index + 1),
        "afterCurrent" => reference_index
            .map(|index| index + 1)
            .or_else(|| members.first().map(|index| index + 1)),
        _ => None,
    }
}

pub(super) fn workspace_create_browser(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let url = raw_string_param(params, &["url"]);
    let state = app.state::<SessionState>();
    match new_browser_workspace_for_control(app, &state, url.as_deref()) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn config_reload(app: &AppHandle) -> ControlCallResult {
    match crate::config::reload_config_for_control(app) {
        Ok(payload) => ok(json!({
            "path": payload.path,
            "reloaded": true,
        })),
        Err(message) => ControlCallResult::Err {
            code: "config_reload_failed".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn window_current(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let session = snapshot(app);
    let window_id = workspace_routed_window_index_for_app(app, &session, params)
        .and_then(|index| session.windows[index].window_id.as_deref());
    let Some(window_id) = window_id else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Current window not found".to_string(),
            data: None,
        };
    };
    let window_ref = control_handle_ref(app, "window", window_id);
    ok(json!({"window_id": window_id, "window_ref": window_ref}))
}

pub(super) fn window_displays(app: &AppHandle) -> ControlCallResult {
    match crate::window::available_displays(app) {
        Ok(displays) => ok(json!({
            "displays": displays.into_iter().map(|display| json!({
                "name": display.name,
                "index": display.index,
                "display_id": Value::Null,
                "main": display.is_main,
                "frame": {
                    "x": display.x,
                    "y": display.y,
                    "width": display.width,
                    "height": display.height,
                },
            })).collect::<Vec<_>>(),
        })),
        Err(message) => ControlCallResult::Err {
            code: "window_displays_failed".into(),
            message,
            data: None,
        },
    }
}

pub(super) fn window_display(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(display) = raw_string_param(params, &["display"]) else {
        return invalid_params("Missing or invalid display");
    };
    let selector = raw_string_param(params, &["window_id", "window_ref"]);
    match crate::window::move_control_windows_to_display(app, &display, selector.as_deref()) {
        Ok(result) => {
            let mut payload = serde_json::Map::from_iter([
                ("display".into(), json!(result.display)),
                (
                    "moved".into(),
                    json!(result
                        .moved
                        .iter()
                        .map(|identity| identity.id.clone())
                        .collect::<Vec<_>>()),
                ),
            ]);
            if selector.is_some() {
                if let Some(identity) = result.moved.first() {
                    payload.insert("window_id".into(), json!(identity.id));
                    payload.insert("window_ref".into(), json!(identity.reference));
                }
            }
            ok(Value::Object(payload))
        }
        Err(crate::window::WindowDisplayMoveError::WindowNotFound(selector)) => {
            let data = if selector.starts_with("window:") {
                json!({"window_ref": selector})
            } else {
                json!({"window_id": selector})
            };
            ControlCallResult::Err {
                code: "not_found".into(),
                message: "Window not found".into(),
                data: JsonValue::try_from(data).ok(),
            }
        }
        Err(crate::window::WindowDisplayMoveError::DisplayNotFound {
            requested,
            available,
        }) => ControlCallResult::Err {
            code: "not_found".into(),
            message: format!("Display not found: {requested}"),
            data: JsonValue::try_from(json!({
                "requested": requested,
                "available": available,
            }))
            .ok(),
        },
        Err(crate::window::WindowDisplayMoveError::Internal(message)) => ControlCallResult::Err {
            code: "window_display_failed".into(),
            message,
            data: None,
        },
    }
}

pub(super) fn session_restore_previous_launch(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<SessionState>();
    match restore_previous_launch_for_control(app, &state) {
        Ok(outcome) => session_restore_previous_result(&outcome),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn session_restore_previous_result(
    outcome: &RestorePreviousLaunchOutcome,
) -> ControlCallResult {
    if outcome.restored {
        ControlCallResult::Ok(
            JsonValue::try_from(json!({ "restored": true }))
                .expect("manual restore payload is valid JSON"),
        )
    } else {
        ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No previous session snapshot available".to_string(),
            data: None,
        }
    }
}

pub(super) fn workspace_reopen_closed(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<SessionState>();
    let snapshot = match reopen_closed_workspace_for_control(app, &state) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "No recently closed workspace".to_string(),
                data: None,
            };
        }
        Err(message) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    workspace_current(&snapshot)
}

pub(super) fn workspace_close_many(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(indices) = workspace_indices_from_params(&current, params) else {
        return invalid_params("Missing or invalid workspace selectors");
    };
    let state = app.state::<SessionState>();
    workspace_current(&close_workspaces_for_control(app, &state, &indices))
}

pub(super) fn workspace_select(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window_index) = workspace_routed_window_index_for_app(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    if raw_string_param(params, &["workspace_id"])
        .as_deref()
        .is_none_or(|workspace_id| Uuid::parse_str(workspace_id).is_err())
    {
        return invalid_params("Missing or invalid workspace_id");
    }
    let Some(index) = canonical_workspace_target_index(&current, window_index, params) else {
        return workspace_not_found(app, params);
    };
    let workspace_id = current.windows[window_index].tab_manager.workspaces[index]
        .workspace_id
        .clone()
        .unwrap_or_default();
    let previous_workspace_id = current.windows[window_index]
        .tab_manager
        .selected_workspace_index
        .and_then(|selected| usize::try_from(selected).ok())
        .and_then(|selected| {
            current.windows[window_index]
                .tab_manager
                .workspaces
                .get(selected)
        })
        .and_then(|workspace| workspace.workspace_id.clone());
    if let Some(window_id) = workspace_select_focus_selector(&current, window_index) {
        let _ = handle_window_lifecycle_request(
            app,
            "window.focus",
            serde_json::Map::from_iter([("window_id".to_string(), json!(window_id))]),
        );
    }
    let state = app.state::<SessionState>();
    let result = match select_workspace_in_window_for_control(
        app,
        &state,
        window_index,
        index,
        DerivedEventPolicy::Suppress,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(WorkspaceSelectControlError::WindowNotFound)) => {
            return ControlCallResult::Err {
                code: "unavailable".to_string(),
                message: "TabManager not available".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    if previous_workspace_id.as_deref() != Some(workspace_id.as_str()) {
        record_workspace_selected_event(
            app,
            &result,
            window_index,
            index,
            previous_workspace_id.as_deref(),
        );
    }
    ok(workspace_identity_payload(
        app,
        &result.windows[window_index],
        &workspace_id,
    ))
}

pub(super) fn workspace_select_focus_selector(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
) -> Option<&str> {
    snapshot.windows.get(window_index)?.window_id.as_deref()
}

pub(super) fn workspace_not_found(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let workspace_id = raw_string_param(params, &["workspace_id"]).unwrap_or_default();
    let workspace_reference = if one_based_ref_index(&workspace_id, "workspace").is_some() {
        workspace_id.clone()
    } else {
        control_handle_ref(app, "workspace", &workspace_id)
    };
    workspace_not_found_with_ref(&workspace_id, &workspace_reference)
}

pub(super) fn workspace_not_found_with_ref(
    workspace_id: &str,
    workspace_reference: &str,
) -> ControlCallResult {
    ControlCallResult::Err {
        code: "not_found".to_string(),
        message: "Workspace not found".to_string(),
        data: json!({
            "workspace_id": workspace_id,
            "workspace_ref": workspace_reference,
        })
        .try_into()
        .ok(),
    }
}

pub(super) fn workspace_identity_payload(
    app: &AppHandle,
    window: &cmux_core::session::SessionWindowSnapshot,
    workspace_id: &str,
) -> Value {
    json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_deref().map(|id| control_handle_ref(app, "window", id)),
        "workspace_id": workspace_id,
        "workspace_ref": control_handle_ref(app, "workspace", workspace_id),
    })
}

pub(super) fn workspace_id_for_window_move(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<String> {
    if let Some(reference) = string_param(params, &["workspace_ref"]) {
        let index = one_based_ref_index(&reference, "workspace")?;
        return snapshot
            .windows
            .first()?
            .tab_manager
            .workspaces
            .get(index)?
            .workspace_id
            .clone();
    }
    let workspace_id = string_param(params, &["workspace_id"])?;
    snapshot
        .windows
        .iter()
        .flat_map(|window| &window.tab_manager.workspaces)
        .any(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
        .then_some(workspace_id)
}

pub(super) fn focus_window_after_workspace_move(app: &AppHandle, label: &str, focus: bool) {
    if focus {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_focus();
        }
    }
}

pub(super) fn workspace_move_to_window(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if !params.contains_key("workspace_ref") && !params.contains_key("workspace_id") {
        return invalid_params("Missing or invalid workspace_id");
    }
    let Some(workspace_id) = workspace_id_for_window_move(&current, params) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: string_param(params, &["workspace_id"])
                .and_then(|workspace_id| json!({"workspace_id": workspace_id}).try_into().ok()),
        };
    };
    let Some(window_selector) = raw_string_param(params, &["window_ref", "window_id"]) else {
        return invalid_params("Missing or invalid window_id");
    };
    let Some(window_identity) =
        crate::window::current_control_window(app, Some(window_selector.as_str()))
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Window not found".to_string(),
            data: Some(
                json!({"window_id": window_selector})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    };
    let state = app.state::<SessionState>();
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let result = match move_workspace_to_window_for_control(
        app,
        &state,
        &workspace_id,
        &window_identity.label,
        focus,
    ) {
        Ok(result) => result,
        Err(MoveWorkspaceToWindowControlError::NotFound) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Workspace not found".to_string(),
                data: Some(
                    json!({"workspace_id": workspace_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        Err(MoveWorkspaceToWindowControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let Some(target_window) = result
        .windows
        .iter()
        .find(|window| window.window_id.as_deref() == Some(window_identity.label.as_str()))
    else {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: "Failed to move workspace".to_string(),
            data: None,
        };
    };
    focus_window_after_workspace_move(app, &window_identity.label, focus);
    let workspace_ref_value = target_window
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
        .map(workspace_ref);
    ok(json!({
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref_value,
        "window_id": window_identity.id,
        "window_ref": window_identity.reference,
    }))
}

pub(super) fn workspace_reorder(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if !workspace_reorder_window_matches(&current, params) {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    }
    let Some(index) = workspace_index_from_params(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace_id) = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(index))
        .and_then(|workspace| workspace.workspace_id.clone())
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(requested_index) = workspace_reorder_destination_index(&current, params, index) else {
        return invalid_params(
            "Specify exactly one target: index, before_workspace_id, or after_workspace_id",
        );
    };
    let uses_top_level_rows =
        bool_param(params, &["uses_top_level_rows", "top_level_rows"]).unwrap_or(false);
    let mut planned = current.clone();
    if let Some(window) = planned.windows.first_mut() {
        session_ops::reorder_workspaces_with_mode(
            &mut window.tab_manager,
            index as i64,
            requested_index,
            uses_top_level_rows,
        );
    }
    let Some(to_index) = workspace_index_for_id(&planned, &workspace_id) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let dry_run = bool_param(params, &["dry_run"]).unwrap_or(false);
    let result = if dry_run {
        planned
    } else {
        let state = app.state::<SessionState>();
        match reorder_workspaces_for_control(
            app,
            &state,
            index as i64,
            requested_index,
            uses_top_level_rows,
        ) {
            Ok(result) => result,
            Err(message) => {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
        }
    };
    let window = result.windows.first();
    let window_id = window.and_then(|window| window.window_id.clone());
    let window_ref = window_id.as_ref().map(|_| "window:1");
    let workspace_ref = workspace_ref(to_index);
    let plan = json!({
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref,
        "window_id": window_id,
        "window_ref": window_ref,
        "from_index": index,
        "to_index": to_index,
    });
    let events = if !dry_run && index != to_index {
        vec![plan.clone()]
    } else {
        Vec::new()
    };
    ok(json!({
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref,
        "window_id": window_id,
        "window_ref": window_ref,
        "from_index": index,
        "to_index": to_index,
        "index": to_index,
        "dry_run": dry_run,
        "plan": [plan],
        "events": events,
    }))
}

pub(super) fn workspace_reorder_many(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if !workspace_reorder_window_matches(&current, params) {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    }
    let ordered_workspace_ids = match workspace_reorder_many_order(&current, params) {
        Ok(ids) => ids,
        Err(WorkspaceReorderManyOrderError::Missing) => {
            return invalid_params("Missing workspace_ids");
        }
        Err(WorkspaceReorderManyOrderError::Invalid(workspace)) => {
            return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: "Invalid workspace id or ref".to_string(),
                data: Some(
                    json!({"workspace": workspace})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    let dry_run = bool_param(params, &["dry_run"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    let (plan, result) =
        match reorder_workspaces_many_for_control(app, &state, &ordered_workspace_ids, dry_run) {
            Ok(result) => result,
            Err(PaneTopologyControlError::Operation(
                ReorderWorkspacesManyControlError::Unavailable,
            )) => {
                return ControlCallResult::Err {
                    code: "unavailable".to_string(),
                    message: "TabManager not available".to_string(),
                    data: None,
                };
            }
            Err(PaneTopologyControlError::Operation(ReorderWorkspacesManyControlError::Batch(
                WorkspaceBatchReorderError::DuplicateWorkspace(workspace_id),
            ))) => {
                return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: "Duplicate workspace in order".to_string(),
                data: Some(
                    json!({
                        "workspace_id": workspace_id,
                        "workspace_ref": workspace_index_for_id(&current, &workspace_id.to_string())
                            .map(workspace_ref),
                    })
                    .try_into()
                    .unwrap_or(JsonValue::Null),
                ),
            };
            }
            Err(PaneTopologyControlError::Operation(ReorderWorkspacesManyControlError::Batch(
                WorkspaceBatchReorderError::WorkspaceNotFound(workspace_id),
            ))) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "Workspace not found".to_string(),
                    data: Some(
                        json!({
                            "workspace_id": workspace_id,
                            "workspace_ref": Value::Null,
                        })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                    ),
                };
            }
            Err(PaneTopologyControlError::Publication(message)) => {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
        };
    let plan_payloads: Vec<Value> = plan
        .iter()
        .map(|item| workspace_reorder_plan_payload(&result, item))
        .collect();
    let events: Vec<Value> = if dry_run {
        Vec::new()
    } else {
        plan.iter()
            .zip(&plan_payloads)
            .filter_map(|(item, payload)| {
                (item.from_index != item.to_index).then(|| payload.clone())
            })
            .collect()
    };
    let window_id = result
        .windows
        .first()
        .and_then(|window| window.window_id.clone());
    ok(json!({
        "window_id": window_id,
        "window_ref": window_id.as_ref().map(|_| "window:1"),
        "dry_run": dry_run,
        "plan": plan_payloads,
        "events": events,
    }))
}

pub(super) fn workspace_reorder_plan_payload(
    snapshot: &AppSessionSnapshot,
    item: &WorkspaceReorderPlanItem,
) -> Value {
    let workspace_id = item.workspace_id.to_string();
    let workspace_ref_value = workspace_index_for_id(snapshot, &workspace_id).map(workspace_ref);
    let window_id = snapshot
        .windows
        .first()
        .and_then(|window| window.window_id.clone());
    json!({
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref_value,
        "window_id": window_id,
        "window_ref": window_id.as_ref().map(|_| "window:1"),
        "from_index": item.from_index,
        "to_index": item.to_index,
    })
}

#[derive(Debug)]
pub(super) enum WorkspaceReorderManyOrderError {
    Missing,
    Invalid(String),
}

pub(super) fn workspace_reorder_many_order(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<Vec<Uuid>, WorkspaceReorderManyOrderError> {
    let values: Vec<&str> = if let Some(raw) = params.get("workspace_ids") {
        match raw {
            Value::Array(values) => values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| WorkspaceReorderManyOrderError::Invalid(value.to_string()))
                })
                .collect::<Result<_, _>>()?,
            Value::String(value) => vec![value],
            value => {
                return Err(WorkspaceReorderManyOrderError::Invalid(value.to_string()));
            }
        }
    } else if let Some(raw) = params.get("order") {
        match raw {
            Value::String(value) => value.split(',').collect(),
            value => {
                return Err(WorkspaceReorderManyOrderError::Invalid(value.to_string()));
            }
        }
    } else {
        return Err(WorkspaceReorderManyOrderError::Missing);
    };
    if values.is_empty() {
        return Err(WorkspaceReorderManyOrderError::Missing);
    }

    values
        .into_iter()
        .map(|raw| {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(WorkspaceReorderManyOrderError::Invalid(raw.to_string()));
            }
            let workspace_id = if let Some(index) = one_based_ref_index(raw, "workspace") {
                snapshot
                    .windows
                    .first()
                    .and_then(|window| window.tab_manager.workspaces.get(index))
                    .and_then(|workspace| workspace.workspace_id.as_deref())
                    .ok_or_else(|| WorkspaceReorderManyOrderError::Invalid(raw.to_string()))?
            } else if workspace_index_for_id(snapshot, raw).is_some()
                || Uuid::parse_str(raw).is_ok()
            {
                raw
            } else {
                return Err(WorkspaceReorderManyOrderError::Invalid(raw.to_string()));
            };
            Uuid::parse_str(workspace_id)
                .map_err(|_| WorkspaceReorderManyOrderError::Invalid(raw.to_string()))
        })
        .collect()
}

pub(super) fn workspace_equalize_splits(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<SessionState>();
    match equalize_dividers_for_control(app, &state) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_set_description(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(description) = raw_string_param(params, &["description", "text", "body"]) else {
        return invalid_params("Missing workspace description");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match set_workspace_description_for_control(app, &state, index as i64, &description) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_reset_color(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match reset_workspace_color_for_control(app, &state, index as i64) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_set_progress(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(value) = f64_param(params, &["value", "progress"]) else {
        return invalid_params("Missing or invalid workspace progress value");
    };
    if !value.is_finite() {
        return invalid_params("Workspace progress value must be finite");
    }
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let label = raw_string_param(params, &["label", "text"]);
    let state = app.state::<SessionState>();
    match set_workspace_sidebar_progress_for_control(
        app,
        &state,
        index as i64,
        value,
        label.as_deref(),
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_clear_progress(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_progress_for_control(app, &state, index as i64) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_set_status(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar status key");
    };
    let Some(value) = raw_string_param(params, &["value", "status", "text"]) else {
        return invalid_params("Missing sidebar status value");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let priority = i64_param(params, &["priority"]);
    let state = app.state::<SessionState>();
    match set_workspace_sidebar_status_for_control(
        app,
        &state,
        index as i64,
        &key,
        &value,
        priority,
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_clear_status(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar status key");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_status_for_control(app, &state, index as i64, &key) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_list_status(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace) = workspace_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    ok(json!({
        "status_entries": workspace.sidebar_status_entries.clone().unwrap_or_default(),
        "status_count": workspace.sidebar_status_entries.as_ref().map_or(0, Vec::len),
    }))
}

pub(super) fn workspace_set_agent_pid(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing agent PID key");
    };
    let Some(pid) = u32_param(params, &["pid", "process_id", "processId"]) else {
        return invalid_params("Missing or invalid agent PID");
    };
    if pid == 0 {
        return invalid_params("Agent PID must be positive");
    }
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match set_workspace_agent_pid_for_control(app, &state, index, &key, pid) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_clear_agent_pid(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing agent PID key");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    let snapshot = match clear_workspace_agent_pid_for_control(app, &state, index, &key) {
        Ok(snapshot) => snapshot,
        Err(message) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    let snapshot = refresh_workspace_agent_ports(app, &snapshot, index).unwrap_or(snapshot);
    workspace_current(&snapshot)
}

pub(super) fn workspace_report_pr(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    default_label: &str,
) -> ControlCallResult {
    let Some(number) = i64_param(params, &["number", "pr", "id"]) else {
        return invalid_params("Missing or invalid pull request number");
    };
    if number <= 0 {
        return invalid_params("Pull request number must be positive");
    }
    let Some(url) = string_param(params, &["url", "href"]) else {
        return invalid_params("Missing pull request URL");
    };
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(status) = pull_request_status_param(params) else {
        return invalid_params("Missing or invalid pull request state");
    };
    let label = raw_string_param(params, &["label"])
        .and_then(|label| (!label.trim().is_empty()).then(|| label.trim().to_string()))
        .unwrap_or_else(|| default_label.to_string());
    let branch = raw_string_param(params, &["branch"])
        .and_then(|branch| (!branch.trim().is_empty()).then(|| branch.trim().to_string()));
    let is_stale = bool_param(params, &["stale", "is_stale", "isStale"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    match set_workspace_panel_pull_request_for_control(
        app,
        &state,
        workspace_index,
        &panel_id,
        number,
        &label,
        &url,
        status,
        branch,
        is_stale,
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_clear_pr(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_panel_pull_request_for_control(app, &state, workspace_index, &panel_id) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_report_meta(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar metadata key");
    };
    let Some(value) = raw_string_param(params, &["value", "text", "markdown"]) else {
        return invalid_params("Missing sidebar metadata value");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let priority = i64_param(params, &["priority"]);
    let icon = string_param(params, &["icon"]);
    let color = string_param(params, &["color"]);
    let url = string_param(params, &["url", "href"]);
    let format = string_param(params, &["format"]);
    let state = app.state::<SessionState>();
    match set_workspace_sidebar_metadata_for_control(
        app,
        &state,
        index as i64,
        &key,
        &value,
        icon.as_deref(),
        color.as_deref(),
        url.as_deref(),
        priority,
        format.as_deref(),
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_clear_meta(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar metadata key");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_metadata_for_control(app, &state, index as i64, &key) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_list_meta(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace) = workspace_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    ok(json!({
        "metadata_entries": workspace.sidebar_metadata_entries.clone().unwrap_or_default(),
        "metadata_count": workspace.sidebar_metadata_entries.as_ref().map_or(0, Vec::len),
    }))
}

pub(super) fn workspace_report_meta_block(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar metadata block key");
    };
    let Some(markdown) = raw_string_param(params, &["markdown", "value", "text"]) else {
        return invalid_params("Missing sidebar metadata block markdown");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let priority = i64_param(params, &["priority"]);
    let state = app.state::<SessionState>();
    match set_workspace_sidebar_metadata_block_for_control(
        app,
        &state,
        index as i64,
        &key,
        &markdown,
        priority,
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_clear_meta_block(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key", "name"]) else {
        return invalid_params("Missing sidebar metadata block key");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_metadata_block_for_control(app, &state, index as i64, &key) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_list_meta_blocks(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace) = workspace_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    ok(json!({
        "metadata_blocks": workspace.sidebar_metadata_blocks.clone().unwrap_or_default(),
        "metadata_block_count": workspace.sidebar_metadata_blocks.as_ref().map_or(0, Vec::len),
    }))
}

pub(super) fn workspace_reset_sidebar(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match reset_workspace_sidebar_metadata_for_control(app, &state, index as i64) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_log(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(message) = raw_string_param(params, &["message", "text"]) else {
        return invalid_params("Missing sidebar log message");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let level = string_param(params, &["level"]).unwrap_or_else(|| "info".to_string());
    let state = app.state::<SessionState>();
    match append_workspace_sidebar_log_for_control(app, &state, index as i64, &message, &level) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_clear_log(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match clear_workspace_sidebar_log_for_control(app, &state, index as i64) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_list_log(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace) = workspace_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let limit = usize_param(params, &["limit"]).unwrap_or(20);
    let entries = recent_sidebar_log_entries(workspace, limit);
    ok(json!({
        "log_entries": entries,
        "log_count": workspace.sidebar_log_entries.as_ref().map_or(0, Vec::len),
    }))
}

pub(super) fn workspace_sidebar_state(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window) = current.windows.first() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(index),
        "ports": workspace_listening_ports(workspace),
        "agent_listening_ports": workspace.agent_listening_ports.clone().unwrap_or_default(),
        "agent_pids": workspace.agent_pids.clone().unwrap_or_default(),
        "agent_pid_count": workspace.agent_pids.as_ref().map_or(0, Vec::len),
        "panel_ttys": workspace.panel_ttys.clone().unwrap_or_default(),
        "tty_count": workspace.panel_ttys.as_ref().map_or(0, Vec::len),
        "panel_shell_activity": workspace.panel_shell_activity.clone().unwrap_or_default(),
        "shell_activity_count": workspace.panel_shell_activity.as_ref().map_or(0, Vec::len),
        "progress": workspace.sidebar_progress,
        "status_entries": workspace.sidebar_status_entries.clone().unwrap_or_default(),
        "status_count": workspace.sidebar_status_entries.as_ref().map_or(0, Vec::len),
        "metadata_entries": workspace.sidebar_metadata_entries.clone().unwrap_or_default(),
        "metadata_count": workspace.sidebar_metadata_entries.as_ref().map_or(0, Vec::len),
        "metadata_blocks": workspace.sidebar_metadata_blocks.clone().unwrap_or_default(),
        "metadata_block_count": workspace.sidebar_metadata_blocks.as_ref().map_or(0, Vec::len),
        "log_entries": recent_sidebar_log_entries(workspace, usize_param(params, &["limit"]).unwrap_or(20)),
        "log_count": workspace.sidebar_log_entries.as_ref().map_or(0, Vec::len),
    }))
}

pub(super) fn workspace_set_unread(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(unread) = bool_param(params, &["unread", "is_unread"]) else {
        return invalid_params("Missing or invalid workspace unread flag");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let preferred_panel_id =
        string_param(params, &["preferred_panel_id", "panel_id", "surface_id"]);
    let state = app.state::<SessionState>();
    match set_workspace_unread_for_control(
        app,
        &state,
        index as i64,
        preferred_panel_id.as_deref(),
        unread,
    ) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_set_pinned(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(pinned) = bool_param(params, &["pinned", "is_pinned"]) else {
        return invalid_params("Missing or invalid workspace pinned flag");
    };
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match set_workspace_pinned_for_control(app, &state, index as i64, pinned) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn workspace_remote_status(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window) = current.windows.first() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(index),
        "remote": workspace_remote_payload(workspace),
    }))
}

pub(super) fn workspace_remote_configure(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(destination) = string_param(params, &["destination", "host"]) else {
        return invalid_params("Missing destination");
    };
    let Some(port) = optional_u16_param(params, "port") else {
        return invalid_params("port must be 1-65535");
    };
    let Some(local_proxy_port) = optional_u16_param(params, "local_proxy_port") else {
        return invalid_params("local_proxy_port must be 1-65535");
    };
    let transport = string_param(params, &["transport"])
        .unwrap_or_else(|| "ssh".to_string())
        .to_ascii_lowercase();
    if !matches!(transport.as_str(), "ssh" | "websocket") {
        return invalid_params("transport must be ssh or websocket");
    }
    let auto_connect = bool_param(params, &["auto_connect"]).unwrap_or(true);
    let persistent_daemon_slot =
        string_param(params, &["persistent_daemon_slot", "persistentDaemonSlot"]);
    let remote_daemon_path = string_param(params, &["remote_daemon_path", "remoteDaemonPath"]);
    let Some(remote_daemon_relay_port) = optional_u16_param(params, "remote_daemon_relay_port")
    else {
        return invalid_params("remote_daemon_relay_port must be 1-65535");
    };
    let remote_daemon_relay_port = match remote_daemon_relay_port {
        Some(port) => Some(port),
        None => match optional_u16_param(params, "remoteDaemonRelayPort") {
            Some(value) => value,
            None => return invalid_params("remoteDaemonRelayPort must be 1-65535"),
        },
    };
    let identity_file = string_param(params, &["identity_file", "identityFile"]);
    let ssh_options = string_vec_param(params, &["ssh_options", "sshOptions"]).unwrap_or_default();
    let config = WorkspaceRemoteControlConfig {
        transport,
        destination,
        port,
        local_proxy_port,
        persistent_daemon_slot,
        remote_daemon_path,
        remote_daemon_relay_port,
        identity_file,
        ssh_options,
        auto_connect,
    };
    let state = app.state::<SessionState>();
    let Some(snapshot) = configure_workspace_remote_for_control(app, &state, index, config) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    workspace_remote_status_from_snapshot(&snapshot, index)
}

pub(super) fn workspace_remote_disconnect(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    let Some(snapshot) = clear_workspace_remote_for_control(app, &state, index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    workspace_remote_status_from_snapshot(&snapshot, index)
}

pub(super) fn workspace_remote_reconnect(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    match reconnect_workspace_remote_for_control(app, &state, index) {
        Ok(Some(snapshot)) => workspace_remote_status_from_snapshot(&snapshot, index),
        Ok(None) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        },
        Err(message) => ControlCallResult::Err {
            code: "invalid_state".to_string(),
            message: format!("Remote workspace is not configured: {message}"),
            data: None,
        },
    }
}

pub(super) fn workspace_remote_status_from_snapshot(
    snapshot: &AppSessionSnapshot,
    index: usize,
) -> ControlCallResult {
    let Some(window) = snapshot.windows.first() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(index),
        "remote": workspace_remote_payload(workspace),
    }))
}

pub(super) fn workspace_group_error(
    code: &str,
    message: &str,
    data: Option<Value>,
) -> ControlCallResult {
    ControlCallResult::Err {
        code: code.to_string(),
        message: message.to_string(),
        data: data.and_then(|value| value.try_into().ok()),
    }
}

pub(super) fn workspace_group_payload(
    app: &AppHandle,
    window: &SessionWindowSnapshot,
    group: &cmux_core::session::SessionWorkspaceGroupSnapshot,
) -> Value {
    workspace_group_payload_with(window, group, &mut |kind, id| {
        control_handle_ref(app, kind, id)
    })
}

pub(super) fn workspace_group_payload_with(
    window: &SessionWindowSnapshot,
    group: &cmux_core::session::SessionWorkspaceGroupSnapshot,
    mint: &mut impl FnMut(&'static str, &str) -> String,
) -> Value {
    let members = window
        .tab_manager
        .workspaces
        .iter()
        .filter(|workspace| workspace.group_id.as_deref() == Some(group.id.as_str()))
        .filter_map(|workspace| workspace.workspace_id.as_deref())
        .collect::<Vec<_>>();
    let anchor_workspace_id = group
        .anchor_workspace_id
        .as_deref()
        .filter(|anchor| members.contains(anchor))
        .or_else(|| {
            group
                .anchor_member_index
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| members.get(index).copied())
        })
        .or_else(|| members.first().copied());
    let member_workspace_ids = members.iter().map(|id| json!(id)).collect::<Vec<_>>();
    let member_workspace_refs = members
        .iter()
        .map(|id| json!(mint("workspace", id)))
        .collect::<Vec<_>>();
    json!({
        "id": group.id,
        "ref": mint("workspace_group", &group.id),
        "name": group.name,
        "is_collapsed": group.is_collapsed,
        "is_pinned": group.is_pinned.unwrap_or(false),
        "anchor_workspace_id": anchor_workspace_id,
        "anchor_workspace_ref": anchor_workspace_id.map(|id| mint("workspace", id)),
        "custom_color": group.custom_color,
        "icon_symbol": group.icon_symbol,
        "member_workspace_ids": member_workspace_ids,
        "member_workspace_refs": member_workspace_refs,
        "member_count": members.len(),
    })
}

pub(super) fn workspace_group_create_cwd(
    tabs: &cmux_core::session::SessionTabManagerSnapshot,
    explicit_cwd: Option<String>,
    child_ids: &[Uuid],
    other_anchor_ids: &HashSet<Uuid>,
) -> Option<String> {
    fn normalized(cwd: String) -> Option<String> {
        let trimmed = cwd.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with("file://") {
            if let Ok(url) = url::Url::parse(trimmed) {
                if let Ok(path) = url.to_file_path() {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
        }
        Some(trimmed.to_string())
    }

    if let Some(explicit_cwd) = explicit_cwd {
        return normalized(explicit_cwd).or_else(default_workspace_directory);
    }
    let first_child = child_ids.iter().find_map(|child_id| {
        tabs.workspaces.iter().find(|workspace| {
            workspace
                .workspace_id
                .as_deref()
                .and_then(|id| Uuid::parse_str(id).ok())
                == Some(*child_id)
                && workspace.is_pinned != Some(true)
                && !other_anchor_ids.contains(child_id)
        })
    });
    if let Some(child_cwd) = first_child.and_then(|workspace| workspace.current_directory.clone()) {
        return normalized(child_cwd);
    }
    tabs.selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| tabs.workspaces.get(index))
        .and_then(|workspace| workspace.current_directory.clone())
        .and_then(normalized)
}

pub(super) fn parse_workspace_group_placement(
    raw: Option<&str>,
) -> Option<session_ops::WorkspaceGroupPlacement> {
    match raw?.trim().to_ascii_lowercase().as_str() {
        "aftercurrent" | "after-current" | "after_current" => {
            Some(session_ops::WorkspaceGroupPlacement::AfterCurrent)
        }
        "top" => Some(session_ops::WorkspaceGroupPlacement::Top),
        "end" => Some(session_ops::WorkspaceGroupPlacement::End),
        _ => None,
    }
}

pub(super) fn workspace_group_move_index_param(
    params: &serde_json::Map<String, Value>,
) -> Option<i64> {
    match params.get("to_index")? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Value::Bool(value) => Some(i64::from(*value)),
        Value::String(raw) => raw.parse::<i64>().ok(),
        _ => None,
    }
}

pub(super) const SF_SYMBOL_NAMES: &str = include_str!("../sf_symbols_v7.txt");

pub(super) fn normalized_workspace_group_icon_symbol(raw: Option<&str>) -> Option<String> {
    let symbol = raw?.trim();
    (!symbol.is_empty() && SF_SYMBOL_NAMES.lines().any(|candidate| candidate == symbol))
        .then(|| symbol.to_string())
}

pub(super) fn workspace_group_parameter_description(value: &Value) -> String {
    fn nested(value: &Value) -> String {
        match value {
            Value::Null => "<null>".to_string(),
            Value::Bool(value) => i32::from(*value).to_string(),
            Value::Number(value) => value.to_string(),
            Value::String(value) => serde_json::to_string(value).unwrap_or_default(),
            Value::Array(values) => format!(
                "[{}]",
                values.iter().map(nested).collect::<Vec<_>>().join(", ")
            ),
            Value::Object(values) if values.is_empty() => "[:]".to_string(),
            Value::Object(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(|(key, value)| format!(
                        "{}: {}",
                        serde_json::to_string(key).unwrap_or_default(),
                        nested(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    if let Value::String(value) = value {
        value.clone()
    } else {
        nested(value)
    }
}

pub(super) fn workspace_group_uuid_param(
    params: &serde_json::Map<String, Value>,
    key: &str,
) -> Option<Uuid> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| Uuid::parse_str(value).ok())
}

pub(super) fn workspace_group_preflight(
    app: &AppHandle,
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Option<ControlCallResult> {
    if method == "workspace.group.create" {
        match params.get("child_workspace_ids") {
            None | Some(Value::Null) => return None,
            Some(Value::Array(values)) if values.iter().all(Value::is_string) => {
                let unresolved = values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .filter(|value| {
                        Uuid::parse_str(value).is_err()
                            && resolve_control_handle_ref(app, "workspace", value).is_none()
                    })
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if unresolved.is_empty() {
                    return None;
                }
                return Some(workspace_group_error(
                    "invalid_params",
                    &format!(
                        "Unresolved child workspace handles: {}",
                        unresolved.join(", ")
                    ),
                    Some(json!({"unresolved": unresolved})),
                ));
            }
            Some(value) => {
                return Some(workspace_group_error(
                    "invalid_params",
                    "child_workspace_ids must be an array of workspace handles",
                    Some(json!({
                        "child_workspace_ids": workspace_group_parameter_description(value)
                    })),
                ));
            }
        }
    }

    let group_id = workspace_group_uuid_param(params, "group_id");
    let workspace_id = workspace_group_uuid_param(params, "workspace_id");
    let error = match method {
        "workspace.group.list" => None,
        "workspace.group.rename"
            if group_id.is_none() || string_param(params, &["name"]).is_none() =>
        {
            Some("Missing group_id or name")
        }
        "workspace.group.add" | "workspace.group.set_anchor"
            if group_id.is_none() || workspace_id.is_none() =>
        {
            Some("Missing group_id or workspace_id")
        }
        "workspace.group.remove" if workspace_id.is_none() => {
            Some("Missing or invalid workspace_id")
        }
        _ if method != "workspace.group.create" && group_id.is_none() => {
            Some("Missing or invalid group_id")
        }
        _ => None,
    };
    if let Some(message) = error {
        return Some(invalid_params(message));
    }

    if method == "workspace.group.add" {
        let placement = raw_string_param(params, &["placement"]);
        if placement.as_deref().is_some_and(|raw| {
            !raw.trim().is_empty() && parse_workspace_group_placement(Some(raw)).is_none()
        }) {
            return Some(workspace_group_error(
                "invalid_params",
                "Invalid placement",
                Some(json!({"placement": placement})),
            ));
        }
        if params
            .get("reference_workspace_id")
            .is_some_and(|value| !value.is_null())
            && workspace_group_uuid_param(params, "reference_workspace_id").is_none()
        {
            return Some(invalid_params("Missing or invalid reference_workspace_id"));
        }
    }
    None
}

pub(super) fn workspace_group_list(
    app: &AppHandle,
    current: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(window_index) = workspace_routed_window_index_for_app(app, current, params) else {
        return workspace_group_error("unavailable", "TabManager not available", None);
    };
    let window = &current.windows[window_index];
    let groups = window
        .tab_manager
        .workspace_groups
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|group| workspace_group_payload(app, window, group))
        .collect::<Vec<_>>();
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_deref().map(|id| control_handle_ref(app, "window", id)),
        "groups": groups,
    }))
}

pub(super) enum WorkspaceGroupTransactionError {
    Mutation(cmux_core::session_ops::WorkspaceGroupMutationError),
    Publication(String),
}

pub(super) type WorkspaceGroupMutationResult =
    Result<(Value, bool), cmux_core::session_ops::WorkspaceGroupMutationError>;
pub(super) type WorkspaceGroupMutation =
    dyn FnOnce(&mut cmux_core::session::SessionTabManagerSnapshot) -> WorkspaceGroupMutationResult;

pub(super) fn workspace_group_transaction(
    app: &AppHandle,
    window_index: usize,
    mutation: impl FnOnce(
        &mut cmux_core::session::SessionTabManagerSnapshot,
    )
        -> Result<(Value, bool), cmux_core::session_ops::WorkspaceGroupMutationError>,
) -> Result<(Value, AppSessionSnapshot), WorkspaceGroupTransactionError> {
    let state = app.state::<SessionState>();
    state
        .transact_value_if_changed(app, |candidate| {
            let Some(window) = candidate.windows.get_mut(window_index) else {
                return Err(cmux_core::session_ops::WorkspaceGroupMutationError::GroupNotFound);
            };
            mutation(&mut window.tab_manager)
        })
        .map_err(|error| match error {
            PaneTopologyControlError::Operation(error) => {
                WorkspaceGroupTransactionError::Mutation(error)
            }
            PaneTopologyControlError::Publication(message) => {
                WorkspaceGroupTransactionError::Publication(message)
            }
        })
}

pub(super) fn workspace_group_control(
    app: &AppHandle,
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    if let Some(error) = workspace_group_preflight(app, method, params) {
        return error;
    }
    let current = snapshot(app);
    if method == "workspace.group.list" {
        return workspace_group_list(app, &current, params);
    }
    let Some(window_index) = workspace_routed_window_index_for_app(app, &current, params) else {
        return workspace_group_error("unavailable", "TabManager not available", None);
    };

    let parse_uuid = |key: &str| workspace_group_uuid_param(params, key);
    let group_id = || parse_uuid("group_id");
    let workspace_id = || parse_uuid("workspace_id");
    let group_exists = |id: Uuid| {
        current.windows[window_index]
            .tab_manager
            .workspace_groups
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|group| Uuid::parse_str(&group.id).ok() == Some(id))
    };
    let not_found_group = |id: Uuid| {
        workspace_group_error(
            "not_found",
            "Group not found",
            Some(json!({"group_id": id})),
        )
    };
    let transaction = |mutation: Box<WorkspaceGroupMutation>| {
        workspace_group_transaction(app, window_index, mutation)
    };
    match method {
        "workspace.group.create" => {
            let explicit_children = match params.get("child_workspace_ids") {
                None | Some(Value::Null) => None,
                Some(Value::Array(values)) if values.iter().all(Value::is_string) => Some(
                    values
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>(),
                ),
                Some(value) => {
                    return workspace_group_error(
                        "invalid_params",
                        "child_workspace_ids must be an array of workspace handles",
                        Some(json!({
                            "child_workspace_ids": workspace_group_parameter_description(value)
                        })),
                    )
                }
            };
            let raw_children = explicit_children.clone().unwrap_or_else(|| {
                let window = &current.windows[window_index];
                let selected = app
                    .state::<SidebarSelectionState>()
                    .selected_for_window(window.window_id.as_deref())
                    .into_iter()
                    .collect::<HashSet<_>>();
                let sidebar_children = window
                    .tab_manager
                    .workspaces
                    .iter()
                    .filter_map(|workspace| workspace.workspace_id.as_ref())
                    .filter(|workspace_id| selected.contains(*workspace_id))
                    .cloned()
                    .collect::<Vec<_>>();
                if !sidebar_children.is_empty() {
                    return sidebar_children;
                }
                string_param(params, &["workspace_id"])
                    .into_iter()
                    .chain(
                        window
                            .tab_manager
                            .selected_workspace_index
                            .and_then(|index| usize::try_from(index).ok())
                            .and_then(|index| window.tab_manager.workspaces.get(index))
                            .and_then(|workspace| workspace.workspace_id.clone()),
                    )
                    .take(1)
                    .collect()
            });
            let mut unresolved = Vec::new();
            let child_ids = raw_children
                .iter()
                .filter_map(|selector| {
                    let resolved = Uuid::parse_str(selector).ok().or_else(|| {
                        resolve_control_handle_ref(app, "workspace", selector)
                            .and_then(|id| Uuid::parse_str(&id).ok())
                    });
                    if resolved.is_none() {
                        unresolved.push(selector.clone());
                    }
                    resolved
                })
                .collect::<Vec<_>>();
            if !unresolved.is_empty() {
                return workspace_group_error(
                    "invalid_params",
                    &format!(
                        "Unresolved child workspace handles: {}",
                        unresolved.join(", ")
                    ),
                    Some(json!({"unresolved": unresolved})),
                );
            }
            let known_ids = current.windows[window_index]
                .tab_manager
                .workspaces
                .iter()
                .filter_map(|workspace| {
                    workspace
                        .workspace_id
                        .as_deref()
                        .and_then(|id| Uuid::parse_str(id).ok())
                })
                .collect::<HashSet<_>>();
            let unknown = child_ids
                .iter()
                .filter(|id| !known_ids.contains(id))
                .copied()
                .collect::<Vec<_>>();
            if !unknown.is_empty() {
                let values = unknown.iter().map(Uuid::to_string).collect::<Vec<_>>();
                return workspace_group_error(
                    "not_found",
                    &format!(
                        "Child workspace not found in target window: {}",
                        values.join(", ")
                    ),
                    Some(json!({"unknown_workspace_ids": values})),
                );
            }
            let other_anchors = current.windows[window_index]
                .tab_manager
                .workspace_groups
                .as_deref()
                .unwrap_or_default()
                .iter()
                .filter_map(|group| {
                    group
                        .anchor_workspace_id
                        .as_deref()
                        .and_then(|id| Uuid::parse_str(id).ok())
                })
                .collect::<HashSet<_>>();
            if explicit_children
                .as_ref()
                .is_some_and(|children| !children.is_empty())
                && !child_ids.is_empty()
                && child_ids.iter().all(|id| other_anchors.contains(id))
            {
                let values = child_ids.iter().map(Uuid::to_string).collect::<Vec<_>>();
                return workspace_group_error(
                    "invalid_state",
                    "All requested children are ineligible because they are already group anchors; ungroup them first",
                    Some(json!({"ineligible_workspace_ids": values})),
                );
            }
            let name = raw_string_param(params, &["name"]).unwrap_or_default();
            let cwd = workspace_group_create_cwd(
                &current.windows[window_index].tab_manager,
                raw_string_param(params, &["cwd"]),
                &child_ids,
                &other_anchors,
            );
            let new_group_id = Uuid::new_v4();
            let anchor_workspace_id = Uuid::new_v4();
            let panel_id = Uuid::new_v4().to_string();
            let pane_id = Uuid::new_v4().to_string();
            let state = app.state::<SessionState>();
            let result = state.transact_value_if_changed(app, |candidate| {
                let tabs = &mut candidate.windows[window_index].tab_manager;
                let mut anchor = session_ops::fresh_terminal_workspace(&panel_id);
                anchor.workspace_id = Some(anchor_workspace_id.to_string());
                anchor.current_directory = cwd.clone();
                if let Some(SessionWorkspaceLayoutSnapshot::Pane(pane)) = anchor.layout.as_mut() {
                    pane.pane_id = Some(pane_id.clone());
                }
                anchor.surfaces = Some(vec![cmux_core::session::SessionSurfaceSnapshot {
                    surface_id: panel_id.clone(),
                    pane_id: pane_id.clone(),
                    generation: 1,
                    kind: cmux_core::session::SessionSurfaceKindSnapshot::Terminal,
                    metadata: Default::default(),
                    terminal_startup: cwd.clone().map(|directory| {
                        cmux_core::session::SessionSurfaceTerminalStartupSnapshot {
                            working_directory: Some(directory),
                            ..Default::default()
                        }
                    }),
                    scrollback: None,
                }]);
                tabs.workspaces.push(anchor);
                let group = session_ops::create_workspace_group_snapshot(
                    tabs,
                    new_group_id,
                    &name,
                    anchor_workspace_id,
                    &child_ids,
                )?;
                if let Some(anchor) = tabs.workspaces.iter_mut().find(|workspace| {
                    workspace
                        .workspace_id
                        .as_deref()
                        .and_then(|id| Uuid::parse_str(id).ok())
                        == Some(anchor_workspace_id)
                }) {
                    anchor.process_title = group.name.clone();
                    anchor.custom_title = None;
                    anchor.custom_title_source = None;
                }
                Ok::<_, cmux_core::session_ops::WorkspaceGroupMutationError>((group, true))
            });
            let (_group, committed) = match result {
                Ok(result) => result,
                Err(PaneTopologyControlError::Operation(_)) => {
                    return workspace_group_error("not_created", "Group was not created", None)
                }
                Err(PaneTopologyControlError::Publication(message)) => {
                    return workspace_group_error("internal", &message, None)
                }
            };
            refresh_known_handle_refs(app, &committed);
            let window = &committed.windows[window_index];
            let Some(group) = window
                .tab_manager
                .workspace_groups
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|group| Uuid::parse_str(&group.id).ok() == Some(new_group_id))
            else {
                return workspace_group_error("not_created", "Group was not created", None);
            };
            ok(json!({"group": workspace_group_payload(app, window, group)}))
        }
        "workspace.group.ungroup" => {
            let Some(group_id) = group_id() else {
                return invalid_params("Missing or invalid group_id");
            };
            if !group_exists(group_id) {
                return not_found_group(group_id);
            }
            let result = transaction(Box::new(move |tabs| {
                session_ops::ungroup_workspace_group_snapshot(tabs, group_id)
                    .map(|_| (json!({"group_id": group_id}), true))
            }));
            match result {
                Ok((payload, _)) => ok(payload),
                Err(WorkspaceGroupTransactionError::Publication(message)) => {
                    workspace_group_error("internal", &message, None)
                }
                Err(WorkspaceGroupTransactionError::Mutation(_)) => {
                    workspace_group_error("internal", "Failed to ungroup workspace group", None)
                }
            }
        }
        "workspace.group.delete" => {
            let Some(group_id) = group_id() else {
                return invalid_params("Missing or invalid group_id");
            };
            if !group_exists(group_id) {
                return not_found_group(group_id);
            }
            let state = app.state::<SessionState>();
            match delete_workspace_group_for_control(
                app,
                &state,
                window_index,
                &group_id.to_string(),
            ) {
                Ok(Some((_snapshot, count))) => ok(json!({
                    "group_id": group_id,
                    "closed_workspace_count": count,
                })),
                Ok(None) => not_found_group(group_id),
                Err(message) => workspace_group_error("internal", &message, None),
            }
        }
        "workspace.group.rename" => {
            let Some(group_id) = group_id() else {
                return invalid_params("Missing group_id or name");
            };
            let Some(name) = string_param(params, &["name"]) else {
                return invalid_params("Missing group_id or name");
            };
            if !group_exists(group_id) {
                return not_found_group(group_id);
            }
            let response_name = name.clone();
            match transaction(Box::new(move |tabs| {
                session_ops::rename_workspace_group_snapshot(tabs, group_id, &name).map(|changed| {
                    (
                        json!({"group_id": group_id, "name": response_name}),
                        changed,
                    )
                })
            })) {
                Ok((payload, _)) => ok(payload),
                Err(WorkspaceGroupTransactionError::Publication(message)) => {
                    workspace_group_error("internal", &message, None)
                }
                Err(WorkspaceGroupTransactionError::Mutation(_)) => {
                    workspace_group_error("internal", "Failed to rename workspace group", None)
                }
            }
        }
        "workspace.group.collapse" | "workspace.group.expand" => {
            let Some(group_id) = group_id() else {
                return invalid_params("Missing or invalid group_id");
            };
            if !group_exists(group_id) {
                return not_found_group(group_id);
            }
            let collapsed = method.ends_with("collapse");
            match transaction(Box::new(move |tabs| {
                let changed =
                    session_ops::set_group_collapsed(tabs, &group_id.to_string(), collapsed);
                Ok((
                    json!({"group_id": group_id, "is_collapsed": collapsed}),
                    changed,
                ))
            })) {
                Ok((payload, _)) => ok(payload),
                Err(WorkspaceGroupTransactionError::Publication(message)) => {
                    workspace_group_error("internal", &message, None)
                }
                Err(WorkspaceGroupTransactionError::Mutation(_)) => {
                    workspace_group_error("internal", "Failed to update workspace group", None)
                }
            }
        }
        "workspace.group.pin" | "workspace.group.unpin" => {
            let Some(group_id) = group_id() else {
                return invalid_params("Missing or invalid group_id");
            };
            if !group_exists(group_id) {
                return not_found_group(group_id);
            }
            let pinned = method.ends_with(".pin");
            match transaction(Box::new(move |tabs| {
                session_ops::set_workspace_group_pinned_snapshot(tabs, group_id, pinned)
                    .map(|changed| (json!({"group_id": group_id, "is_pinned": pinned}), changed))
            })) {
                Ok((payload, _)) => ok(payload),
                Err(WorkspaceGroupTransactionError::Publication(message)) => {
                    workspace_group_error("internal", &message, None)
                }
                Err(WorkspaceGroupTransactionError::Mutation(_)) => {
                    workspace_group_error("internal", "Failed to update workspace group", None)
                }
            }
        }
        "workspace.group.add" => {
            let (Some(group_id), Some(workspace_id)) = (group_id(), workspace_id()) else {
                return invalid_params("Missing group_id or workspace_id");
            };
            let placement_raw = raw_string_param(params, &["placement"]);
            let placement = parse_workspace_group_placement(placement_raw.as_deref());
            if placement_raw
                .as_deref()
                .is_some_and(|raw| !raw.trim().is_empty() && placement.is_none())
            {
                return workspace_group_error(
                    "invalid_params",
                    "Invalid placement",
                    Some(json!({"placement": placement_raw})),
                );
            }
            let reference_present = params
                .get("reference_workspace_id")
                .is_some_and(|value| !value.is_null());
            let reference = parse_uuid("reference_workspace_id");
            if reference_present && reference.is_none() {
                return invalid_params("Missing or invalid reference_workspace_id");
            }
            if !group_exists(group_id)
                || !current.windows[window_index]
                    .tab_manager
                    .workspaces
                    .iter()
                    .any(|workspace| {
                        workspace.workspace_id.as_deref() == Some(workspace_id.to_string().as_str())
                    })
            {
                return workspace_group_error(
                    "not_found",
                    "Group or workspace not found",
                    Some(json!({"group_id": group_id, "workspace_id": workspace_id})),
                );
            }
            match transaction(Box::new(move |tabs| {
                session_ops::add_workspace_to_group_snapshot(
                    tabs,
                    group_id,
                    workspace_id,
                    placement,
                    reference,
                )
                .map(|changed| {
                    (
                        json!({"group_id": group_id, "workspace_id": workspace_id}),
                        changed,
                    )
                })
            })) {
                Ok((payload, _)) => ok(payload),
                Err(WorkspaceGroupTransactionError::Mutation(cmux_core::session_ops::WorkspaceGroupMutationError::InvalidReferenceWorkspace)) => workspace_group_error(
                    "invalid_params",
                    "Reference workspace must be a member of the target group",
                    reference.map(|id| json!({"reference_workspace_id": id})),
                ),
                Err(WorkspaceGroupTransactionError::Mutation(cmux_core::session_ops::WorkspaceGroupMutationError::WorkspaceIsOtherGroupAnchor)) => workspace_group_error(
                    "invalid_state",
                    "Workspace is the anchor of another group; ungroup it first",
                    Some(json!({"group_id": group_id, "workspace_id": workspace_id})),
                ),
                Err(WorkspaceGroupTransactionError::Publication(message)) => {
                    workspace_group_error("internal", &message, None)
                }
                Err(WorkspaceGroupTransactionError::Mutation(_)) => workspace_group_error(
                    "not_found",
                    "Group or workspace not found",
                    Some(json!({"group_id": group_id, "workspace_id": workspace_id})),
                ),
            }
        }
        "workspace.group.remove" => {
            let Some(workspace_id) = workspace_id() else {
                return invalid_params("Missing or invalid workspace_id");
            };
            match transaction(Box::new(move |tabs| {
                session_ops::remove_workspace_from_group_snapshot(tabs, workspace_id)
                    .map(|changed| (json!({"workspace_id": workspace_id}), changed))
            })) {
                Ok((payload, _)) => ok(payload),
                Err(WorkspaceGroupTransactionError::Publication(message)) => {
                    workspace_group_error("internal", &message, None)
                }
                Err(WorkspaceGroupTransactionError::Mutation(_)) => workspace_group_error(
                    "not_found",
                    "Workspace not in a group",
                    Some(json!({"workspace_id": workspace_id})),
                ),
            }
        }
        "workspace.group.set_anchor" => {
            let (Some(group_id), Some(workspace_id)) = (group_id(), workspace_id()) else {
                return invalid_params("Missing group_id or workspace_id");
            };
            match transaction(Box::new(move |tabs| {
                session_ops::set_workspace_group_anchor_snapshot(tabs, group_id, workspace_id).map(
                    |changed| {
                        (
                            json!({"group_id": group_id, "anchor_workspace_id": workspace_id}),
                            changed,
                        )
                    },
                )
            })) {
                Ok((payload, _)) => ok(payload),
                Err(WorkspaceGroupTransactionError::Publication(message)) => {
                    workspace_group_error("internal", &message, None)
                }
                Err(WorkspaceGroupTransactionError::Mutation(_)) => workspace_group_error(
                    "not_found",
                    "Group not found or workspace not a member",
                    Some(json!({"group_id": group_id, "workspace_id": workspace_id})),
                ),
            }
        }
        "workspace.group.new_workspace" => {
            let Some(group_id) = group_id() else {
                return invalid_params("Missing or invalid group_id");
            };
            if !group_exists(group_id) {
                return not_found_group(group_id);
            }
            let explicit = raw_string_param(params, &["placement"]);
            let placement = parse_workspace_group_placement(explicit.as_deref());
            if explicit
                .as_deref()
                .is_some_and(|raw| !raw.trim().is_empty() && placement.is_none())
            {
                let raw = explicit.as_deref().map(str::trim).unwrap_or_default();
                return workspace_group_error(
                    "invalid_params",
                    "placement must be one of: afterCurrent, top, end",
                    Some(json!({"placement": raw})),
                );
            }
            let group = current.windows[window_index]
                .tab_manager
                .workspace_groups
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|group| Uuid::parse_str(&group.id).ok() == Some(group_id))
                .expect("validated group");
            let anchor_id = group.anchor_workspace_id.as_deref();
            let cwd = anchor_id.and_then(|anchor| {
                current.windows[window_index]
                    .tab_manager
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id.as_deref() == Some(anchor))
                    .and_then(|workspace| workspace.current_directory.as_deref())
            });
            let local_config_cwd = current.windows[window_index]
                .tab_manager
                .selected_workspace_index
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| {
                    current.windows[window_index]
                        .tab_manager
                        .workspaces
                        .get(index)
                })
                .and_then(|workspace| workspace.current_directory.as_deref());
            let effective = placement.unwrap_or_else(|| {
                match crate::config::workspace_group_new_workspace_placement(
                    app,
                    cwd,
                    local_config_cwd,
                ) {
                    cmux_config::NewWorkspacePlacement::AfterCurrent => {
                        session_ops::WorkspaceGroupPlacement::AfterCurrent
                    }
                    cmux_config::NewWorkspacePlacement::Top => {
                        session_ops::WorkspaceGroupPlacement::Top
                    }
                    cmux_config::NewWorkspacePlacement::End => {
                        session_ops::WorkspaceGroupPlacement::End
                    }
                }
            });
            let effective = match effective {
                session_ops::WorkspaceGroupPlacement::AfterCurrent => "afterCurrent",
                session_ops::WorkspaceGroupPlacement::Top => "top",
                session_ops::WorkspaceGroupPlacement::End => "end",
            };
            let insert_index = workspace_group_insert_index(
                &current.windows[window_index].tab_manager,
                &group_id.to_string(),
                effective,
                None,
            );
            let state = app.state::<SessionState>();
            match new_workspace_in_window_for_control(
                app,
                &state,
                window_index,
                cwd,
                None,
                None,
                None,
                None,
                None,
                Some(&group_id.to_string()),
                None,
                insert_index,
                false,
                DerivedEventPolicy::Record,
            ) {
                Ok(Some((committed, index))) => {
                    let workspace_id = committed.windows[window_index].tab_manager.workspaces
                        [index]
                        .workspace_id
                        .as_deref()
                        .expect("created workspace id");
                    ok(json!({
                        "group_id": group_id,
                        "workspace_id": workspace_id,
                        "workspace_ref": control_handle_ref(app, "workspace", workspace_id),
                    }))
                }
                Ok(None) => not_found_group(group_id),
                Err(message) => workspace_group_error("internal", &message, None),
            }
        }
        "workspace.group.set_color" | "workspace.group.set_icon" => {
            let Some(group_id) = group_id() else {
                return invalid_params("Missing or invalid group_id");
            };
            if !group_exists(group_id) {
                return not_found_group(group_id);
            }
            if method.ends_with("set_color") {
                let value = raw_string_param(params, &["hex"])
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                let response = value.clone();
                match transaction(Box::new(move |tabs| {
                    session_ops::set_workspace_group_color_snapshot(tabs, group_id, value).map(
                        |changed| {
                            (
                                json!({"group_id": group_id, "custom_color": response}),
                                changed,
                            )
                        },
                    )
                })) {
                    Ok((payload, _)) => ok(payload),
                    Err(WorkspaceGroupTransactionError::Publication(message)) => {
                        workspace_group_error("internal", &message, None)
                    }
                    Err(WorkspaceGroupTransactionError::Mutation(_)) => {
                        workspace_group_error("internal", "Failed to update group color", None)
                    }
                }
            } else {
                let raw_symbol = raw_string_param(params, &["symbol"]);
                let value = normalized_workspace_group_icon_symbol(raw_symbol.as_deref());
                match transaction(Box::new(move |tabs| {
                    session_ops::set_workspace_group_icon_snapshot(tabs, group_id, value).map(
                        |changed| {
                            let stored = tabs
                                .workspace_groups
                                .as_deref()
                                .unwrap_or_default()
                                .iter()
                                .find(|group| group.id == group_id.to_string())
                                .and_then(|group| group.icon_symbol.clone());
                            (
                                json!({"group_id": group_id, "icon_symbol": stored}),
                                changed,
                            )
                        },
                    )
                })) {
                    Ok((payload, _)) => ok(payload),
                    Err(WorkspaceGroupTransactionError::Publication(message)) => {
                        workspace_group_error("internal", &message, None)
                    }
                    Err(WorkspaceGroupTransactionError::Mutation(_)) => {
                        workspace_group_error("internal", "Failed to update group icon", None)
                    }
                }
            }
        }
        "workspace.group.move" => {
            let Some(group_id) = group_id() else {
                return invalid_params("Missing or invalid group_id");
            };
            let groups = current.windows[window_index]
                .tab_manager
                .workspace_groups
                .as_deref()
                .unwrap_or_default();
            let Some(source_index) = groups
                .iter()
                .position(|group| Uuid::parse_str(&group.id).ok() == Some(group_id))
            else {
                return workspace_group_error(
                    "invalid_params",
                    "Missing or unresolvable target position",
                    Some(json!({"group_id": group_id})),
                );
            };
            let target = if let Some(index) = workspace_group_move_index_param(params) {
                Some(index)
            } else if let Some(before) = parse_uuid("before_group_id") {
                groups
                    .iter()
                    .position(|group| Uuid::parse_str(&group.id).ok() == Some(before))
                    .map(|index| if source_index < index { index - 1 } else { index } as i64)
            } else if let Some(after) = parse_uuid("after_group_id") {
                groups
                    .iter()
                    .position(|group| Uuid::parse_str(&group.id).ok() == Some(after))
                    .map(|index| if source_index < index { index } else { index + 1 } as i64)
            } else {
                None
            };
            let Some(target) = target else {
                return workspace_group_error(
                    "invalid_params",
                    "Missing or unresolvable target position",
                    Some(json!({"group_id": group_id})),
                );
            };
            match transaction(Box::new(move |tabs| {
                session_ops::move_workspace_group_snapshot(tabs, group_id, target)
                    .map(|changed| (json!({"group_id": group_id}), changed))
            })) {
                Ok((payload, _)) => ok(payload),
                Err(WorkspaceGroupTransactionError::Publication(message)) => {
                    workspace_group_error("internal", &message, None)
                }
                Err(WorkspaceGroupTransactionError::Mutation(_)) => workspace_group_error(
                    "invalid_params",
                    "Missing or unresolvable target position",
                    Some(json!({"group_id": group_id})),
                ),
            }
        }
        "workspace.group.focus" => {
            let Some(group_id) = group_id() else {
                return invalid_params("Missing or invalid group_id");
            };
            let Some(group) = current.windows[window_index]
                .tab_manager
                .workspace_groups
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|group| Uuid::parse_str(&group.id).ok() == Some(group_id))
            else {
                return workspace_group_error(
                    "not_found",
                    "Group or anchor not found",
                    Some(json!({"group_id": group_id})),
                );
            };
            let Some(anchor_id) = group.anchor_workspace_id.as_deref() else {
                return workspace_group_error(
                    "not_found",
                    "Group or anchor not found",
                    Some(json!({"group_id": group_id})),
                );
            };
            let Some(anchor_index) = current.windows[window_index]
                .tab_manager
                .workspaces
                .iter()
                .position(|workspace| workspace.workspace_id.as_deref() == Some(anchor_id))
            else {
                return workspace_group_error(
                    "not_found",
                    "Group or anchor not found",
                    Some(json!({"group_id": group_id})),
                );
            };
            if let Some(window_id) = current.windows[window_index].window_id.as_deref() {
                let _ = crate::window::focus_control_window(app, window_id);
            }
            let state = app.state::<SessionState>();
            match select_workspace_in_window_for_control(
                app,
                &state,
                window_index,
                anchor_index,
                DerivedEventPolicy::Record,
            ) {
                Ok(_) => ok(json!({
                    "group_id": group_id,
                    "anchor_workspace_id": anchor_id,
                    "anchor_workspace_ref": control_handle_ref(app, "workspace", anchor_id),
                })),
                Err(_) => workspace_group_error(
                    "not_found",
                    "Group or anchor not found",
                    Some(json!({"group_id": group_id})),
                ),
            }
        }
        _ => unreachable!("workspace-group dispatch is exhaustive"),
    }
}

pub(super) fn workspace_group_set_collapsed(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(group_id) = string_param(params, &["group_id", "id"]) else {
        return invalid_params("Missing workspace group id");
    };
    let Some(collapsed) = bool_param(params, &["collapsed", "is_collapsed"]) else {
        return invalid_params("Missing or invalid collapsed flag");
    };
    let state = app.state::<SessionState>();
    match set_group_collapsed_for_control(app, &state, &group_id, collapsed) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}
