use super::*;

pub(in crate::control_socket) fn notification_dismiss(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let id = notification_uuid_param(params, &["id"]);
    let all_read = bool_param(params, &["all_read"]).unwrap_or(false);
    if id.is_some() == all_read {
        return invalid_params("Select exactly one of id or all_read");
    }
    let current = snapshot(app);
    let state = app.state::<crate::notifications::NotificationCommandState>();
    match crate::notifications::notification_dismiss_for_control(
        state.inner(),
        id.as_deref(),
        all_read,
    ) {
        Ok(crate::notifications::ControlNotificationDismissOutcome::Dismissed(notification)) => {
            record_notification_events(app, vec![notification_removed_event_spec(&notification)]);
            let mut payload = notification_payload(app, &current, &notification, None);
            payload
                .as_object_mut()
                .expect("notification payload is an object")
                .insert("dismissed".to_owned(), json!(1));
            record_notification_v2_request(
                app,
                "notification.dismiss_requested",
                "notification.dismiss",
                notification_public_params(params),
                &payload,
            );
            ok(payload)
        }
        Ok(crate::notifications::ControlNotificationDismissOutcome::AllRead {
            dismissed,
            removed,
        }) => {
            record_notification_events(
                app,
                removed
                    .iter()
                    .map(notification_removed_event_spec)
                    .collect(),
            );
            let result = json!({"dismissed": dismissed, "all_read": true});
            record_notification_v2_request(
                app,
                "notification.dismiss_requested",
                "notification.dismiss",
                notification_public_params(params),
                &result,
            );
            ok(result)
        }
        Err(message) if message == "Notification not found" => ControlCallResult::Err {
            code: "not_found".to_owned(),
            message,
            data: id
                .as_deref()
                .and_then(|id| notification_error_data(json!({"id": id}))),
        },
        Err(message) => ControlCallResult::Err {
            code: "notification_store_failed".to_owned(),
            message,
            data: None,
        },
    }
}

pub(in crate::control_socket) fn notification_mark_read(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let id = notification_uuid_param(params, &["id"]);
    let all = bool_param(params, &["all"]).unwrap_or(false);
    let current = snapshot(app);
    let canonical_workspace_id = notification_uuid_param(params, &["tab_id", "workspace_id"]);
    let workspace_index = canonical_workspace_id
        .as_deref()
        .and_then(|id| workspace_index_for_id(&current, id))
        .or_else(|| {
            params
                .contains_key("workspace_ref")
                .then(|| workspace_index_from_params(&current, params))
                .flatten()
        });
    let workspace_id = canonical_workspace_id.or_else(|| {
        workspace_index.and_then(|index| {
            current.windows[0].tab_manager.workspaces[index]
                .workspace_id
                .clone()
        })
    });
    if usize::from(id.is_some()) + usize::from(workspace_id.is_some()) + usize::from(all) != 1 {
        return invalid_params("Select exactly one of id, tab_id, or all");
    }
    let has_surface = ["surface_ref", "surface_id", "panel_id"]
        .iter()
        .any(|key| params.get(*key).is_some_and(|value| !value.is_null()));
    let surface_id = if has_surface {
        let canonical_surface_id = notification_uuid_param(params, &["surface_id"]);
        if params.contains_key("surface_id") && canonical_surface_id.is_none() {
            return invalid_params("Missing or invalid surface_id");
        }
        if workspace_id.is_none() {
            return invalid_params("surface_id requires tab_id or workspace_id");
        }
        match canonical_surface_id.or_else(|| {
            workspace_index.and_then(|index| {
                surface_id_from_params_or_workspace_focused(&current, index, params)
            })
        }) {
            Some(id) => Some(id),
            None => return invalid_params("Missing or invalid surface_id"),
        }
    } else {
        None
    };
    let state = app.state::<crate::notifications::NotificationCommandState>();
    match crate::notifications::notification_mark_read_for_control(
        state.inner(),
        id.as_deref(),
        workspace_id.as_deref(),
        surface_id.as_deref(),
        all,
    ) {
        Ok(outcome) => {
            let mut result =
                serde_json::Map::from_iter([("marked_read".to_owned(), json!(outcome.marked))]);
            if let Some(id) = id {
                result.insert("id".to_owned(), json!(id));
            }
            if let Some(workspace_id) = workspace_id {
                result.insert("workspace_id".to_owned(), json!(workspace_id));
                result.insert(
                    "workspace_ref".to_owned(),
                    json!(control_handle_ref(app, "workspace", &workspace_id)),
                );
            }
            if has_surface {
                result.insert("surface_id".to_owned(), json!(surface_id));
                result.insert(
                    "surface_ref".to_owned(),
                    json!(surface_id
                        .as_deref()
                        .map(|surface_id| control_handle_ref(app, "surface", surface_id))),
                );
            }
            if all {
                result.insert("all".to_owned(), json!(true));
            }
            let result = Value::Object(result);
            record_notification_events(
                app,
                outcome
                    .changed
                    .iter()
                    .filter_map(|notification| {
                        notification_batch_event_spec(
                            "notification.read",
                            std::slice::from_ref(&notification.id),
                            Some(&notification.workspace_id),
                            notification.surface_id.as_deref(),
                        )
                    })
                    .collect(),
            );
            record_notification_v2_request(
                app,
                "notification.mark_read_requested",
                "notification.mark_read",
                notification_public_params(params),
                &result,
            );
            ok(result)
        }
        Err(message) if message == "Notification not found" => ControlCallResult::Err {
            code: "not_found".to_owned(),
            message,
            data: id
                .as_deref()
                .and_then(|id| notification_error_data(json!({"id": id}))),
        },
        Err(message) => ControlCallResult::Err {
            code: "notification_store_failed".to_owned(),
            message,
            data: None,
        },
    }
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
    match crate::notifications::notification_clear_for_control(
        state.inner(),
        workspace_id.as_deref(),
    ) {
        Ok(outcome) => {
            let result = json!({});
            if params.get("__cmux_cli_command").and_then(Value::as_str)
                == Some("clear_notifications")
            {
                let args = workspace_id.as_deref().unwrap_or_default();
                record_notification_events(
                    app,
                    vec![notification_v1_request_event_spec(
                        "notification.clear_requested",
                        "clear_notifications",
                        args,
                        workspace_id.as_deref(),
                    )],
                );
            } else {
                record_notification_v2_request(
                    app,
                    "notification.clear_requested",
                    "notification.clear",
                    notification_public_params(params),
                    &result,
                );
            }
            if let Some(cleared) = notification_batch_event_spec(
                "notification.cleared",
                &outcome.cleared_ids,
                workspace_id.as_deref(),
                None,
            ) {
                record_notification_events(app, vec![cleared]);
            }
            ok(result)
        }
        Err(message) => ControlCallResult::Err {
            code: "notification_store_failed".to_owned(),
            message,
            data: None,
        },
    }
}

pub(in crate::control_socket) fn notification_create(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_owned(),
            message: "Workspace not found".to_owned(),
            data: None,
        };
    };
    let Some(workspace_id) = current.windows[0].tab_manager.workspaces[workspace_index]
        .workspace_id
        .clone()
    else {
        return ControlCallResult::Err {
            code: "not_found".to_owned(),
            message: "Workspace not found".to_owned(),
            data: None,
        };
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
        Ok(outcome) => {
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
                    Err(error) => {
                        eprintln!("[notification] failed to reorder workspace: {error}");
                    }
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
            let result = json!({
                "workspace_id": workspace_id,
                "surface_id": surface_id,
            });
            record_notification_v2_request(
                app,
                "notification.requested",
                "notification.create",
                redacted_notification_request_params(params),
                &result,
            );
            ok(result)
        }
        Err(message) => ControlCallResult::Err {
            code: "notification_store_failed".to_string(),
            message,
            data: None,
        },
    }
}
