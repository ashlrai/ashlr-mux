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
