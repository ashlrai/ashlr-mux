use super::*;

pub(in crate::control_socket) fn notification_open(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(id) = notification_uuid_param(params, &["id"]) else {
        return invalid_params("Missing or invalid notification id");
    };
    notification_open_selected(
        app,
        Some(&id),
        false,
        "notification.open_requested",
        "notification.open",
        notification_public_params(params),
    )
}

pub(in crate::control_socket) fn notification_jump_to_unread(app: &AppHandle) -> ControlCallResult {
    notification_open_selected(
        app,
        None,
        true,
        "notification.jump_to_unread_requested",
        "notification.jump_to_unread",
        json!({}),
    )
}

fn notification_open_selected(
    app: &AppHandle,
    id: Option<&str>,
    allow_empty: bool,
    requested_event_name: &'static str,
    method: &'static str,
    request_params: Value,
) -> ControlCallResult {
    let notification_state = app.state::<crate::notifications::NotificationCommandState>();
    let outcome = match crate::notifications::notification_open_target_for_control(
        notification_state.inner(),
        id,
    ) {
        Ok(Some(outcome)) => outcome,
        Ok(None) if allow_empty => {
            let result = json!({"opened": false});
            record_notification_v2_request(
                app,
                requested_event_name,
                method,
                request_params,
                &result,
            );
            return ok(result);
        }
        Ok(None) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Notification not found".to_string(),
                data: id.and_then(|id| notification_error_data(json!({"id": id}))),
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
    let notification = outcome.notification;
    let previous = snapshot(app);
    let session_state = app.state::<SessionState>();
    let target_surface = notification.surface_id.clone();
    let (opened, current) = if let Some(surface_id) = target_surface.as_deref() {
        let (changed, current) = match crate::session::select_workspace_surface_with_event_policy(
            app,
            &session_state,
            &notification.workspace_id,
            surface_id,
            DerivedEventPolicy::Suppress,
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
        (
            changed
                || crate::session::workspace_surface_is_selected(
                    &current,
                    &notification.workspace_id,
                    surface_id,
                ),
            current,
        )
    } else {
        let Some(index) = workspace_index_for_id(&previous, &notification.workspace_id) else {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Notification target not found".to_string(),
                data: notification_error_data(notification_payload(
                    app,
                    &previous,
                    &notification,
                    Some(false),
                )),
            };
        };
        match select_workspace_in_window_for_control(
            app,
            &session_state,
            0,
            index,
            DerivedEventPolicy::Suppress,
        ) {
            Ok(current) => (
                notification_workspace_is_selected(&current, &notification.workspace_id),
                current,
            ),
            Err(error) => {
                return ControlCallResult::Err {
                    code: "internal".to_string(),
                    message: match error {
                        PaneTopologyControlError::Operation(_) => {
                            "Notification target not found".to_string()
                        }
                        PaneTopologyControlError::Publication(message) => message,
                    },
                    data: None,
                };
            }
        }
    };
    if !opened {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Notification target not found".to_string(),
            data: notification_error_data(notification_payload(
                app,
                &current,
                &notification,
                Some(false),
            )),
        };
    }
    let mut events = target_surface
        .as_deref()
        .map_or_else(Vec::new, |surface_id| {
            notification_navigation_event_specs(
                &previous,
                &current,
                &notification.workspace_id,
                surface_id,
            )
        });
    if outcome.marked_read {
        if let Some(read) = notification_batch_event_spec(
            "notification.read",
            std::slice::from_ref(&notification.id),
            Some(&notification.workspace_id),
            notification.surface_id.as_deref(),
        ) {
            events.push(read);
        }
    }
    record_notification_events(app, events);
    let result = notification_payload(app, &current, &notification, Some(true));
    record_notification_v2_request(app, requested_event_name, method, request_params, &result);
    ok(result)
}
