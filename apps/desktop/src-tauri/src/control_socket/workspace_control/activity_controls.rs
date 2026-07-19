use super::*;

pub(in crate::control_socket) fn right_sidebar_control(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if !right_sidebar_target_exists(&current, params) {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Right sidebar target not found".to_string(),
            data: None,
        };
    }

    let Some(action) = string_param(params, &["action"]) else {
        return invalid_params("right_sidebar requires action");
    };
    let mode = string_param(params, &["mode"]);
    let focus = bool_param(params, &["focus"]).unwrap_or(true);
    let state = app.state::<crate::right_sidebar::RightSidebarState>();
    match state.apply_control(&action, mode.as_deref(), focus) {
        Ok(crate::right_sidebar::RightSidebarControlOutcome::State(snapshot)) => {
            ok(json!(snapshot))
        }
        Ok(crate::right_sidebar::RightSidebarControlOutcome::Changed(change)) => {
            if let Err(error) = app.emit(
                crate::right_sidebar::RIGHT_SIDEBAR_CHANGED_EVENT,
                change.clone(),
            ) {
                return ControlCallResult::Err {
                    code: "right_sidebar_unavailable".to_string(),
                    message: error.to_string(),
                    data: None,
                };
            }
            ok(json!({"ok": true}))
        }
        Err(message) => invalid_params(&message),
    }
}

fn right_sidebar_target_exists(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> bool {
    let has_workspace_target =
        params.contains_key("workspace_ref") || params.contains_key("workspace_id");
    if has_workspace_target && workspace_index_from_params(snapshot, params).is_none() {
        return false;
    }

    let has_window_target = params.contains_key("window_ref") || params.contains_key("window_id");
    if !has_window_target {
        return true;
    }
    let Some(window) = snapshot.windows.first() else {
        return false;
    };
    if let Some(reference) = string_param(params, &["window_ref"]) {
        return one_based_ref_index(&reference, "window") == Some(0);
    }
    string_param(params, &["window_id"])
        .is_some_and(|id| window.window_id.as_deref() == Some(id.as_str()))
}

pub(in crate::control_socket) fn feed_push(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    has_request_id: bool,
) -> ControlCallResult {
    let Some(event_value) = params.get("event") else {
        return invalid_params("feed.push requires an event object");
    };
    let event: cmux_agent::WorkstreamEvent = match serde_json::from_value(event_value.clone()) {
        Ok(event) => event,
        Err(error) => return invalid_params(&format!("feed.push event failed to decode: {error}")),
    };
    let wait_timeout = match params.get("wait_timeout_seconds") {
        None => 0.0,
        Some(value) => match value.as_f64() {
            Some(value) => value,
            None => return invalid_params("feed.push wait_timeout_seconds must be numeric"),
        },
    };
    if !(0.0..=120.0).contains(&wait_timeout) {
        return invalid_params("feed.push wait_timeout_seconds must be between 0 and 120");
    }
    if !has_request_id && wait_timeout > 0.0 {
        return invalid_params("feed.push without an id requires wait_timeout_seconds 0");
    }
    let state = app.state::<crate::feed::FeedState>();
    let insert = match state.ingest(event) {
        Ok(insert) => insert,
        Err(message) => {
            return ControlCallResult::Err {
                code: "feed_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    if let Ok(reply) = state.list() {
        let _ = app.emit(crate::feed::FEED_CHANGED_EVENT, &reply);
    }
    match state.wait_for_decision(&insert, Duration::from_secs_f64(wait_timeout)) {
        Ok(reply) => {
            if reply.status == "timed_out" {
                if let Ok(list) = state.list() {
                    let _ = app.emit(crate::feed::FEED_CHANGED_EVENT, &list);
                }
            }
            ok(json!(reply))
        }
        Err(message) => ControlCallResult::Err {
            code: "feed_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(in crate::control_socket) fn feed_list(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<crate::feed::FeedState>();
    match state.list() {
        Ok(reply) => ok(json!(reply)),
        Err(message) => ControlCallResult::Err {
            code: "feed_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(in crate::control_socket) fn feed_permission_reply(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(request_id) = string_param(params, &["request_id"]) else {
        return invalid_params("feed.permission.reply requires request_id");
    };
    let Some(mode) = string_param(params, &["mode"]) else {
        return invalid_params("feed.permission.reply requires mode");
    };
    let decision = match crate::feed::permission_decision(&mode) {
        Ok(decision) => decision,
        Err(message) => return invalid_params(&format!("feed.permission.reply {message}")),
    };
    feed_resolve_control(app, &request_id, decision)
}

pub(in crate::control_socket) fn feed_question_reply(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(request_id) = string_param(params, &["request_id"]) else {
        return invalid_params("feed.question.reply requires request_id");
    };
    let Some(selections) = string_vec_param(params, &["selections"]) else {
        return invalid_params("feed.question.reply requires selections: [string]");
    };
    feed_resolve_control(app, &request_id, crate::feed::question_decision(selections))
}

pub(in crate::control_socket) fn feed_exit_plan_reply(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(request_id) = string_param(params, &["request_id"]) else {
        return invalid_params("feed.exit_plan.reply requires request_id");
    };
    let Some(mode) = string_param(params, &["mode"]) else {
        return invalid_params("feed.exit_plan.reply requires mode");
    };
    let decision = match crate::feed::exit_plan_decision(&mode, string_param(params, &["feedback"]))
    {
        Ok(decision) => decision,
        Err(message) => return invalid_params(&format!("feed.exit_plan.reply {message}")),
    };
    feed_resolve_control(app, &request_id, decision)
}

fn feed_resolve_control(
    app: &AppHandle,
    request_id: &str,
    decision: cmux_agent::WorkstreamDecision,
) -> ControlCallResult {
    let state = app.state::<crate::feed::FeedState>();
    match state.resolve(request_id, decision) {
        Ok(reply) => {
            let _ = app.emit(crate::feed::FEED_CHANGED_EVENT, &reply);
            ok(json!({"status": "resolved", "request_id": request_id}))
        }
        Err(message) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message,
            data: None,
        },
    }
}

pub(in crate::control_socket) fn notification_list(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<crate::notifications::NotificationCommandState>();
    match crate::notifications::notification_list_for_control(state.inner()) {
        Ok(reply) => ok(json!(reply
            .notifications
            .into_iter()
            .map(|item| json!({
                "id": item.id,
                "workspace_id": item.workspace_id,
                "surface_id": item.surface_id,
                "panel_id": item.panel_id,
                "is_read": item.is_read,
                "title": item.title,
                "subtitle": item.subtitle,
                "body": item.body,
                "created_at": item.created_at,
                "tab_title": Value::Null,
            }))
            .collect::<Vec<_>>())),
        Err(message) => ControlCallResult::Err {
            code: "notification_store_failed".to_string(),
            message,
            data: None,
        },
    }
}

fn notification_mutation_result(
    result: Result<crate::notifications::NotificationCenterReply, String>,
) -> ControlCallResult {
    match result {
        Ok(reply) => ok(json!({
            "remaining_count": reply.total_count,
            "unread_count": reply.unread_count,
        })),
        Err(message) => ControlCallResult::Err {
            code: "notification_store_failed".to_string(),
            message,
            data: None,
        },
    }
}

pub(in crate::control_socket) fn notification_dismiss(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let id = string_param(params, &["id"]);
    let all_read = bool_param(params, &["all_read"]).unwrap_or(false);
    if id.is_some() == all_read {
        return invalid_params("notification.dismiss requires exactly one of id or all_read");
    }
    let state = app.state::<crate::notifications::NotificationCommandState>();
    notification_mutation_result(crate::notifications::notification_dismiss_for_control(
        state.inner(),
        id.as_deref(),
        all_read,
    ))
}

pub(in crate::control_socket) fn notification_mark_read(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let id = string_param(params, &["id"]);
    let all = bool_param(params, &["all"]).unwrap_or(false);
    let current = snapshot(app);
    let workspace_index = workspace_index_from_params(&current, params);
    if usize::from(id.is_some()) + usize::from(workspace_index.is_some()) + usize::from(all) != 1 {
        return invalid_params("notification.mark_read requires exactly one selector");
    }
    let workspace_id = workspace_index.and_then(|index| {
        current.windows[0].tab_manager.workspaces[index]
            .workspace_id
            .clone()
    });
    let has_surface = ["surface_ref", "surface_id", "panel_id"]
        .iter()
        .any(|key| params.contains_key(*key));
    let surface_id = if has_surface {
        let Some(index) = workspace_index else {
            return invalid_params("surface selector requires workspace selector");
        };
        match surface_id_from_params_or_workspace_focused(&current, index, params) {
            Some(id) => Some(id),
            None => return invalid_params("Missing or invalid surface selector"),
        }
    } else {
        None
    };
    let state = app.state::<crate::notifications::NotificationCommandState>();
    notification_mutation_result(crate::notifications::notification_mark_read_for_control(
        state.inner(),
        id.as_deref(),
        workspace_id.as_deref(),
        surface_id.as_deref(),
        all,
    ))
}

pub(in crate::control_socket) fn notification_clear(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let workspace_id =
        if params.contains_key("workspace_id") || params.contains_key("workspace_ref") {
            let Some(index) = workspace_index_from_params(&current, params) else {
                return invalid_params("Missing or invalid workspace selector");
            };
            current.windows[0].tab_manager.workspaces[index]
                .workspace_id
                .clone()
        } else {
            None
        };
    let state = app.state::<crate::notifications::NotificationCommandState>();
    notification_mutation_result(crate::notifications::notification_clear_for_control(
        state.inner(),
        workspace_id.as_deref(),
    ))
}

pub(in crate::control_socket) fn notification_create(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace_id) = current.windows[0].tab_manager.workspaces[workspace_index]
        .workspace_id
        .clone()
    else {
        return invalid_params("Workspace has no stable id");
    };
    let Some(surface_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let title = raw_string_param(params, &["title"]).unwrap_or_else(|| "Notification".to_string());
    let subtitle = raw_string_param(params, &["subtitle"]).unwrap_or_default();
    let body = raw_string_param(params, &["body"]).unwrap_or_default();
    let state = app.state::<crate::notifications::NotificationCommandState>();
    match crate::notifications::notification_create_for_control(
        state.inner(),
        workspace_id.clone(),
        surface_id.clone(),
        title,
        subtitle,
        body,
    ) {
        Ok(notification) => ok(json!({
            "id": notification.id,
            "workspace_id": workspace_id,
            "workspace_ref": workspace_ref(workspace_index),
            "surface_id": surface_id,
            "surface_ref": surface_ref_for_panel(&current, workspace_index, &surface_id),
            "title": notification.title,
            "subtitle": notification.subtitle,
            "body": notification.body,
            "created_at": notification.created_at,
            "is_read": false,
            "delivered": true,
        })),
        Err(message) => ControlCallResult::Err {
            code: "notification_delivery_failed".to_string(),
            message,
            data: None,
        },
    }
}

pub(in crate::control_socket) fn notification_open(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(id) = string_param(params, &["id"]) else {
        return invalid_params("notification.open requires id");
    };
    notification_open_selected(app, Some(&id), false)
}

pub(in crate::control_socket) fn notification_jump_to_unread(app: &AppHandle) -> ControlCallResult {
    notification_open_selected(app, None, true)
}

fn notification_open_selected(
    app: &AppHandle,
    id: Option<&str>,
    allow_empty: bool,
) -> ControlCallResult {
    let notification_state = app.state::<crate::notifications::NotificationCommandState>();
    let notification = match crate::notifications::notification_open_target_for_control(
        notification_state.inner(),
        id,
    ) {
        Ok(Some(notification)) => notification,
        Ok(None) if allow_empty => return ok(json!({"opened": false})),
        Ok(None) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Notification not found".to_string(),
                data: id.map(|id| json!({"id": id}).try_into().unwrap_or(JsonValue::Null)),
            };
        }
        Err(message) => {
            return ControlCallResult::Err {
                code: "notification_store_failed".to_string(),
                message,
                data: None,
            };
        }
    };
    let session_state = app.state::<SessionState>();
    let target_surface = notification
        .surface_id
        .clone()
        .or(notification.panel_id.clone());
    let opened = if let Some(surface_id) = target_surface.as_deref() {
        let (changed, snapshot) = match crate::session::select_workspace_surface(
            app,
            &session_state,
            &notification.tab_id,
            surface_id,
        ) {
            Ok(result) => result,
            Err(PaneTopologyControlError::Operation(error)) => match error {},
            Err(PaneTopologyControlError::Publication(message)) => {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message,
                    data: None,
                };
            }
        };
        changed
            || crate::session::workspace_surface_is_selected(
                &snapshot,
                &notification.tab_id,
                surface_id,
            )
    } else {
        let current = snapshot(app);
        match workspace_index_for_id(&current, &notification.tab_id) {
            Some(index) => match select_workspace_for_control(app, &session_state, index as i64) {
                Ok(_) => true,
                Err(message) => {
                    return ControlCallResult::Err {
                        code: "internal".to_string(),
                        message,
                        data: None,
                    };
                }
            },
            None => false,
        }
    };
    if !opened {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Notification target not found".to_string(),
            data: Some(
                json!({"id": notification.id})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }
    let current = snapshot(app);
    let workspace_index = workspace_index_for_id(&current, &notification.tab_id);
    let workspace_ref_value = workspace_index.map(workspace_ref);
    let surface_ref_value = workspace_index.and_then(|index| {
        target_surface
            .as_deref()
            .and_then(|surface| surface_ref_for_panel(&current, index, surface))
    });
    ok(json!({
        "id": notification.id,
        "workspace_id": notification.tab_id,
        "workspace_ref": workspace_ref_value,
        "surface_id": target_surface,
        "surface_ref": surface_ref_value,
        "title": notification.title,
        "subtitle": notification.subtitle,
        "body": notification.body,
        "created_at": notification.created_at,
        "is_read": true,
        "opened": true,
    }))
}
