use super::*;

use super::targeting::{
    normalized_notification_tty, notification_workspace_has_surface,
    resolve_caller_notification_target_with_fallback,
};

fn notification_store_error(message: String) -> ControlCallResult {
    ControlCallResult::Err {
        code: "notification_store_failed".to_owned(),
        message,
        data: None,
    }
}

fn workspace_not_found(workspace_id: Option<&str>) -> ControlCallResult {
    ControlCallResult::Err {
        code: "not_found".to_owned(),
        message: "Workspace not found".to_owned(),
        data: workspace_id.and_then(|workspace_id| {
            notification_error_data(json!({"workspace_id": workspace_id}))
        }),
    }
}

fn surface_not_found(surface_id: &str) -> ControlCallResult {
    ControlCallResult::Err {
        code: "not_found".to_owned(),
        message: "Surface not found".to_owned(),
        data: notification_error_data(json!({"surface_id": surface_id})),
    }
}

fn tab_manager_unavailable() -> ControlCallResult {
    ControlCallResult::Err {
        code: "unavailable".to_owned(),
        message: "TabManager not available".to_owned(),
        data: None,
    }
}

fn record_created_notification(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    method: &'static str,
    workspace_id: String,
    surface_id: String,
    title: String,
    subtitle: String,
    body: String,
    result: Value,
) -> ControlCallResult {
    let state = app.state::<crate::notifications::NotificationCommandState>();
    let outcome = match crate::notifications::notification_create_for_control(
        state.inner(),
        workspace_id.clone(),
        surface_id,
        title,
        subtitle,
        body,
    ) {
        Ok(outcome) => outcome,
        Err(message) => return notification_store_error(message),
    };

    if crate::config::reorder_on_notification_enabled() {
        let session_state = app.state::<SessionState>();
        match crate::session::move_workspace_to_top_for_notification_for_control(
            app,
            &session_state,
            &workspace_id,
        ) {
            Ok((true, reordered, window_index)) => record_workspace_reordered_event(
                app,
                &reordered,
                window_index,
                std::slice::from_ref(&workspace_id),
            ),
            Ok((false, _, _)) => {}
            Err(error) => eprintln!("[notification] failed to reorder workspace: {error}"),
        }
    }

    let replaced_ids = outcome
        .replaced
        .iter()
        .map(|notification| notification.id.clone())
        .collect::<Vec<_>>();
    let mut events = outcome
        .replaced
        .iter()
        .map(notification_removed_event_spec)
        .collect::<Vec<_>>();
    events.push(notification_created_event_spec(
        &outcome.notification,
        &replaced_ids,
    ));
    record_notification_events(app, events);
    record_notification_v2_request(
        app,
        "notification.requested",
        method,
        redacted_notification_request_params(params),
        &result,
    );
    ok(result)
}

fn targeted_result(
    app: &AppHandle,
    current: &AppSessionSnapshot,
    window_index: usize,
    workspace_id: &str,
    surface_id: &str,
) -> Value {
    let window_id = current
        .windows
        .get(window_index)
        .and_then(|window| window.window_id.as_deref());
    json!({
        "workspace_id": workspace_id,
        "workspace_ref": control_handle_ref(app, "workspace", workspace_id),
        "surface_id": surface_id,
        "surface_ref": control_handle_ref(app, "surface", surface_id),
        "window_id": window_id,
        "window_ref": window_id.map(|window_id| control_handle_ref(app, "window", window_id)),
    })
}

fn resolved_preferred_uuid(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    key: &str,
    kind: &'static str,
) -> Option<String> {
    let selector = string_param(params, &[key])?;
    Uuid::parse_str(&selector)
        .ok()
        .map(|id| id.to_string())
        .or_else(|| resolve_control_handle_ref(app, kind, &selector))
        .and_then(|id| Uuid::parse_str(&id).ok().map(|id| id.to_string()))
}

pub(in crate::control_socket) fn notification_create(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return workspace_not_found(None);
    };
    let Some(workspace_id) = current.windows[0].tab_manager.workspaces[workspace_index]
        .workspace_id
        .clone()
    else {
        return workspace_not_found(None);
    };
    let Some(surface_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_owned(),
            message: "Surface not found".to_owned(),
            data: string_param(params, &["surface_id"])
                .and_then(|surface_id| notification_error_data(json!({"surface_id": surface_id}))),
        };
    };
    let result = json!({"workspace_id": workspace_id, "surface_id": surface_id});
    record_created_notification(
        app,
        params,
        "notification.create",
        workspace_id,
        surface_id,
        raw_string_param(params, &["title"]).unwrap_or_else(|| "Notification".to_owned()),
        raw_string_param(params, &["subtitle"]).unwrap_or_default(),
        raw_string_param(params, &["body"]).unwrap_or_default(),
        result,
    )
}

pub(in crate::control_socket) fn notification_create_for_surface(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(surface_id) = notification_uuid_param(params, &["surface_id"]) else {
        return invalid_params("Missing or invalid surface_id");
    };
    let current = snapshot(app);
    let Some(window_index) = workspace_routed_window_index_for_app(app, &current, params) else {
        return tab_manager_unavailable();
    };
    let Some(window) = current.windows.get(window_index) else {
        return tab_manager_unavailable();
    };
    let workspace_id_selector = notification_uuid_param(params, &["workspace_id"]);
    let workspace_index = if let Some(workspace_id) = workspace_id_selector.as_deref() {
        window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
    } else {
        window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| notification_workspace_has_surface(workspace, &surface_id))
            .or_else(|| selected_workspace_index_for_window(&current, window_index))
    };
    let Some(workspace_index) = workspace_index else {
        return workspace_not_found(None);
    };
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let Some(workspace_id) = workspace.workspace_id.clone() else {
        return workspace_not_found(None);
    };
    if !notification_workspace_has_surface(workspace, &surface_id) {
        return surface_not_found(&surface_id);
    }
    let result = targeted_result(app, &current, window_index, &workspace_id, &surface_id);
    record_created_notification(
        app,
        params,
        "notification.create_for_surface",
        workspace_id,
        surface_id,
        raw_string_param(params, &["title"]).unwrap_or_else(|| "Notification".to_owned()),
        raw_string_param(params, &["subtitle"]).unwrap_or_default(),
        raw_string_param(params, &["body"]).unwrap_or_default(),
        result,
    )
}

pub(in crate::control_socket) fn notification_create_for_target(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(workspace_id) = notification_uuid_param(params, &["workspace_id"]) else {
        return invalid_params("Missing or invalid workspace_id");
    };
    let Some(surface_id) = notification_uuid_param(params, &["surface_id"]) else {
        return invalid_params("Missing or invalid surface_id");
    };
    let current = snapshot(app);
    let Some(window_index) = workspace_routed_window_index_for_app(app, &current, params) else {
        return tab_manager_unavailable();
    };
    let Some(window) = current.windows.get(window_index) else {
        return tab_manager_unavailable();
    };
    let Some(workspace) = window
        .tab_manager
        .workspaces
        .iter()
        .find(|workspace| workspace.workspace_id.as_deref() == Some(&workspace_id))
    else {
        return workspace_not_found(Some(&workspace_id));
    };
    if !notification_workspace_has_surface(workspace, &surface_id) {
        return surface_not_found(&surface_id);
    }
    let result = targeted_result(app, &current, window_index, &workspace_id, &surface_id);
    record_created_notification(
        app,
        params,
        "notification.create_for_target",
        workspace_id,
        surface_id,
        raw_string_param(params, &["title"]).unwrap_or_else(|| "Notification".to_owned()),
        raw_string_param(params, &["subtitle"]).unwrap_or_default(),
        raw_string_param(params, &["body"]).unwrap_or_default(),
        result,
    )
}

pub(in crate::control_socket) fn notification_create_for_caller(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if current.windows.is_empty() {
        return tab_manager_unavailable();
    }
    let preferred_workspace_id =
        resolved_preferred_uuid(app, params, "preferred_workspace_id", "workspace");
    let preferred_surface_id =
        resolved_preferred_uuid(app, params, "preferred_surface_id", "surface");
    let caller_tty = string_param(params, &["caller_tty"]);
    let caller_tty = normalized_notification_tty(caller_tty.as_deref());
    let prefer_tty = bool_param(params, &["prefer_tty"]).unwrap_or(false);
    let fallback_window_index = control_active_window_id(app)
        .and_then(|active| {
            current
                .windows
                .iter()
                .position(|window| window.window_id.as_deref() == Some(&active))
        })
        .unwrap_or(0);
    let Some(target) = resolve_caller_notification_target_with_fallback(
        &current,
        preferred_workspace_id.as_deref(),
        preferred_surface_id.as_deref(),
        caller_tty,
        prefer_tty,
        fallback_window_index,
    ) else {
        return workspace_not_found(None);
    };
    let result = json!({
        "workspace_id": target.workspace_id,
        "surface_id": target.surface_id,
    });
    record_created_notification(
        app,
        params,
        "notification.create_for_caller",
        target.workspace_id,
        target.surface_id,
        string_param(params, &["title"]).unwrap_or_else(|| "Notification".to_owned()),
        string_param(params, &["subtitle"]).unwrap_or_default(),
        string_param(params, &["body"]).unwrap_or_default(),
        result,
    )
}
