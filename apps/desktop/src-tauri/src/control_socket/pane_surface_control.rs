use super::*;

#[path = "pane_surface_control/pane_list.rs"]
mod pane_list;
pub(super) use pane_list::pane_list;

pub(super) const TERMINAL_INPUT_QUEUE_FULL_MESSAGE: &str = "The terminal can't accept more input right now. Wait a moment and retry, or reopen the terminal if it stays unavailable.";
pub(super) const TERMINAL_SURFACE_UNAVAILABLE_MESSAGE: &str =
    "The terminal surface is no longer available; reopen it or create a new terminal session.";
pub(super) const TERMINAL_PROCESS_EXITED_MESSAGE: &str =
    "The terminal session has ended; reopen it or create a new terminal session.";

pub(super) fn terminal_set_font_control(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let plan = match plan_terminal_set_font_request(params) {
        Ok(plan) => plan,
        Err(error) => {
            let data = error.font_size.and_then(|font_size| {
                if font_size.is_finite() {
                    JsonValue::try_from(json!({"font_size": font_size})).ok()
                } else {
                    Some(JsonValue::Double(font_size))
                }
            });
            return ControlCallResult::Err {
                code: error.code.into(),
                message: error.message.into(),
                data,
            };
        }
    };
    let mut payload = json!({"font_size": plan.font_size});
    if let Some(surface_id) = plan.surface_id.as_deref() {
        payload["surface_id"] = json!(surface_id);
    }
    if let Some(workspace_id) = plan.workspace_id.as_deref() {
        payload["workspace_id"] = json!(workspace_id);
    }
    let delivered = emit_transient_control_event(
        app,
        "terminal.set_font",
        "terminal",
        "socket.v2",
        None,
        plan.workspace_id,
        None,
        plan.surface_id,
        payload,
    );
    ok(json!({
        "ok": true,
        "font_size": plan.font_size,
        "delivered": delivered,
    }))
}

pub(super) fn terminal_request_plan(
    app: &AppHandle,
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<TerminalRequestPlan, ControlCallResult> {
    let current = snapshot(app);
    let active_window_id = control_active_window_id(app);
    plan_terminal_request_with_active_window(&current, method, params, active_window_id.as_deref())
        .map_err(|error| ControlCallResult::Err {
            code: error.code.to_string(),
            message: error.message.to_string(),
            data: None,
        })
}

pub(super) fn terminal_create_control(
    app: &AppHandle,
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let before = snapshot(app);
    let plan = match terminal_request_plan(app, method, params) {
        Ok(plan) => plan,
        Err(error) => return error,
    };

    match plan {
        TerminalRequestPlan::Create {
            window_index,
            workspace_index,
            pane_id,
            requested_workspace_id,
        } => {
            let workspace_id = before.windows[window_index].tab_manager.workspaces[workspace_index]
                .workspace_id
                .clone();
            let mut create_params = serde_json::Map::new();
            if let Some(workspace_id) = workspace_id.as_deref() {
                create_params.insert("workspace_id".into(), json!(workspace_id));
            }
            create_params.insert("pane_id".into(), json!(pane_id));
            create_params.insert("type".into(), json!("terminal"));
            create_params.insert("focus".into(), json!(false));
            let created = handle_pane_surface_lifecycle_request_with_terminal_policy(
                app,
                "surface.create",
                &create_params,
                TerminalCreateRuntimePolicy::Deferred,
            );
            let ControlCallResult::Ok(payload) = created else {
                return match created {
                    ControlCallResult::Err {
                        code,
                        message: _,
                        data,
                    } if code == "internal_error" => ControlCallResult::Err {
                        code,
                        message: "Failed to create terminal".into(),
                        data,
                    },
                    other => other,
                };
            };
            let payload = Value::from(payload);
            let Some(created_terminal_id) = payload.get("surface_id").and_then(Value::as_str)
            else {
                return ControlCallResult::Err {
                    code: "internal_error".into(),
                    message: "Failed to create terminal".into(),
                    data: None,
                };
            };
            let requested_terminal_id = match terminal_create_response_terminal_id(params) {
                Ok(terminal_id) => terminal_id,
                Err(error) => {
                    return ControlCallResult::Err {
                        code: error.code.into(),
                        message: error.message.into(),
                        data: None,
                    }
                }
            };
            terminal_mobile_workspace_list(
                app,
                &snapshot(app),
                window_index,
                requested_workspace_id.as_deref(),
                requested_terminal_id.as_deref(),
                created_terminal_id,
            )
        }
        TerminalRequestPlan::Input { .. } => unreachable!("input is dispatched after gate release"),
    }
}

pub(super) struct PreparedTerminalInput {
    pub(super) workspace_id: Option<String>,
    pub(super) surface_id: String,
    pub(super) demand: TerminalMaterializationDemand,
}

pub(super) enum TerminalInputTarget {
    Local(TerminalMaterializationSpec),
    Remote,
}

pub(super) fn prepare_terminal_input_control(
    app: &AppHandle,
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Result<PreparedTerminalInput, ControlCallResult> {
    let current = snapshot(app);
    let plan = terminal_request_plan(app, method, params)?;
    let TerminalRequestPlan::Input {
        window_index,
        workspace_index,
        surface_id,
        events,
    } = plan
    else {
        unreachable!("create is dispatched while the control gate is held")
    };
    let workspace = &current.windows[window_index].tab_manager.workspaces[workspace_index];
    let Some(target) = terminal_input_target(workspace, &surface_id) else {
        return Err(ControlCallResult::Err {
            code: "not_found".into(),
            message: "Terminal surface not found".into(),
            data: None,
        });
    };
    let state = app.state::<TerminalState>();
    let demand = match target {
        TerminalInputTarget::Local(spec) => {
            request_terminal_materialization(state.inner(), &surface_id, &spec, events)
        }
        TerminalInputTarget::Remote => {
            request_live_terminal_input(state.inner(), &surface_id, events)
        }
    };
    Ok(PreparedTerminalInput {
        workspace_id: workspace.workspace_id.clone(),
        surface_id,
        demand,
    })
}

pub(super) fn finish_terminal_input_control(
    app: &AppHandle,
    prepared: PreparedTerminalInput,
) -> ControlCallResult {
    let PreparedTerminalInput {
        workspace_id,
        surface_id,
        demand,
    } = prepared;
    let state = app.state::<TerminalState>();
    let outcome = match demand {
        TerminalMaterializationDemand::Noop => TerminalInputOutcome::Sent,
        TerminalMaterializationDemand::Live(events) => {
            terminal_apply_materialization_events_for_control(
                app,
                state.inner(),
                &surface_id,
                events,
            )
        }
        TerminalMaterializationDemand::Queued => TerminalInputOutcome::Queued,
        TerminalMaterializationDemand::Start(lease) => {
            let app = app.clone();
            let logging_surface_id = surface_id.clone();
            tauri::async_runtime::spawn_blocking(move || {
                let state = app.state::<TerminalState>();
                if let Err(error) =
                    materialize_terminal_for_input(&app, state.inner(), lease, None, None)
                {
                    eprintln!(
                        "[terminal] failed to materialize input target {logging_surface_id}: {error}"
                    );
                }
            });
            TerminalInputOutcome::Queued
        }
        TerminalMaterializationDemand::InputQueueFull => TerminalInputOutcome::InputQueueFull,
        TerminalMaterializationDemand::SurfaceUnavailable => {
            TerminalInputOutcome::SurfaceUnavailable
        }
    };
    let queued = match outcome {
        TerminalInputOutcome::Sent => false,
        TerminalInputOutcome::Queued => true,
        failure => return terminal_input_failure(&surface_id, failure),
    };
    ok(json!({
        "workspace_id": workspace_id,
        "surface_id": surface_id,
        "queued": queued,
    }))
}

pub(super) fn terminal_input_target(
    workspace: &SessionWorkspaceSnapshot,
    surface_id: &str,
) -> Option<TerminalInputTarget> {
    let surface = workspace
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|surface| surface.surface_id == surface_id)?;
    if matches!(
        surface.kind,
        SessionSurfaceKindSnapshot::RemoteTerminal { .. }
    ) {
        return Some(TerminalInputTarget::Remote);
    }
    if !matches!(surface.kind, SessionSurfaceKindSnapshot::Terminal) {
        return None;
    }
    let startup = surface.terminal_startup.as_ref();
    let working_directory = startup
        .and_then(|startup| startup.working_directory.clone())
        .or_else(|| surface.metadata.reported_directory.clone())
        .or_else(|| workspace.current_directory.clone());
    Some(TerminalInputTarget::Local(
        TerminalMaterializationSpec::new(
            working_directory,
            startup.and_then(|startup| startup.command.clone()),
            startup
                .and_then(|startup| startup.initial_input.clone())
                .unwrap_or_default()
                .into_bytes(),
            startup
                .and_then(|startup| startup.environment.clone())
                .unwrap_or_default(),
        ),
    ))
}

pub(super) fn terminal_input_failure(
    surface_id: &str,
    outcome: TerminalInputOutcome,
) -> ControlCallResult {
    let (code, message) = match outcome {
        TerminalInputOutcome::InputQueueFull => {
            ("input_queue_full", TERMINAL_INPUT_QUEUE_FULL_MESSAGE)
        }
        TerminalInputOutcome::ProcessExited => ("process_exited", TERMINAL_PROCESS_EXITED_MESSAGE),
        TerminalInputOutcome::SurfaceUnavailable => {
            ("surface_unavailable", TERMINAL_SURFACE_UNAVAILABLE_MESSAGE)
        }
        TerminalInputOutcome::Sent | TerminalInputOutcome::Queued => {
            unreachable!("successful input does not map to an error")
        }
    };
    ControlCallResult::Err {
        code: code.into(),
        message: message.into(),
        data: json!({"surface_id": surface_id}).try_into().ok(),
    }
}

pub(super) fn terminal_mobile_workspace_list(
    app: &AppHandle,
    current: &AppSessionSnapshot,
    window_index: usize,
    requested_workspace_id: Option<&str>,
    requested_terminal_id: Option<&str>,
    created_terminal_id: &str,
) -> ControlCallResult {
    let Some(window) = current.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".into(),
            message: "Workspace context is unavailable".into(),
            data: None,
        };
    };
    let selected_index = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok());
    let terminal_state = app.state::<TerminalState>();
    let notifications = app
        .try_state::<crate::notifications::NotificationCommandState>()
        .and_then(|state| crate::notifications::notification_list_for_control(state.inner()).ok());
    let groups = terminal_groups_for_mobile(
        window,
        requested_workspace_id.is_some() || requested_terminal_id.is_some(),
    );
    let workspaces = window
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .filter(|(_, workspace)| {
            requested_workspace_id.is_none()
                || workspace.workspace_id.as_deref() == requested_workspace_id
        })
        .map(|(workspace_index, workspace)| {
            let mut is_ready = |surface_id: &str| {
                terminal_grid_size_for_panel(terminal_state.inner(), surface_id).is_some()
            };
            let terminals =
                terminal_rows_for_mobile(workspace, requested_terminal_id, &mut is_ready);
            let latest = workspace.workspace_id.as_deref().and_then(|workspace_id| {
                notifications.as_ref().and_then(|center| {
                    center
                        .notifications
                        .iter()
                        .filter(|notification| notification.workspace_id == workspace_id)
                        .max_by_key(|notification| notification.created_at)
                })
            });
            let preview = latest.and_then(|notification| {
                mobile_workspace_preview(if notification.body.is_empty() {
                    &notification.title
                } else {
                    &notification.body
                })
            });
            let has_unread = latest.is_some_and(|notification| !notification.is_read)
                || workspace
                    .surfaces
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .any(|surface| surface.metadata.unread)
                || workspace
                    .panel_unreads
                    .as_deref()
                    .unwrap_or_default()
                    .iter()
                    .any(|entry| entry.is_unread);
            json!({
                "id": workspace.workspace_id,
                "window_id": window.window_id,
                "title": workspace_display_name(workspace),
                "current_directory": workspace.current_directory,
                "is_selected": selected_index == Some(workspace_index),
                "is_pinned": workspace.is_pinned.unwrap_or(false),
                "group_id": workspace.group_id,
                "preview": preview,
                "preview_at": latest.map(|notification| notification.created_at),
                "last_activity_at": latest
                    .map(|notification| notification.created_at as f64)
                    .unwrap_or(current.created_at as f64),
                "has_unread": has_unread,
                "terminals": terminals,
            })
        })
        .collect::<Vec<_>>();
    if requested_terminal_id.is_some_and(|requested| {
        !workspaces.iter().any(|workspace| {
            workspace
                .get("terminals")
                .and_then(Value::as_array)
                .is_some_and(|terminals| {
                    terminals.iter().any(|terminal| {
                        terminal.get("id").and_then(Value::as_str) == Some(requested)
                    })
                })
        })
    }) {
        let surface_id = requested_terminal_id.expect("requested terminal was present");
        return ControlCallResult::Err {
            code: "not_found".into(),
            message: "Terminal not found".into(),
            data: json!({"surface_id": surface_id}).try_into().ok(),
        };
    }
    ok(json!({
        "workspaces": workspaces,
        "groups": groups,
        "created_terminal_id": created_terminal_id,
    }))
}

pub(super) fn terminal_rows_for_mobile(
    workspace: &SessionWorkspaceSnapshot,
    requested_terminal_id: Option<&str>,
    is_ready: &mut impl FnMut(&str) -> bool,
) -> Vec<Value> {
    surfaces_for_workspace(workspace)
        .into_iter()
        .filter_map(|row| {
            let surface_id = row.get("id").and_then(Value::as_str)?;
            if requested_terminal_id.is_some_and(|requested| requested != surface_id) {
                return None;
            }
            let surface = workspace
                .surfaces
                .as_deref()
                .unwrap_or_default()
                .iter()
                .find(|surface| surface.surface_id == surface_id)?;
            if !matches!(
                surface.kind,
                SessionSurfaceKindSnapshot::Terminal
                    | SessionSurfaceKindSnapshot::RemoteTerminal { .. }
            ) {
                return None;
            }
            let title = surface
                .metadata
                .custom_title
                .clone()
                .or_else(|| panel_title(&workspace.panel_titles, surface_id))
                .or_else(|| surface.metadata.runtime_title.clone())
                .unwrap_or_else(|| "Terminal".into());
            let current_directory = surface
                .metadata
                .reported_directory
                .clone()
                .or_else(|| {
                    surface
                        .terminal_startup
                        .as_ref()
                        .and_then(|startup| startup.working_directory.clone())
                })
                .or_else(|| workspace.current_directory.clone());
            Some(json!({
                "id": surface.surface_id,
                "title": title,
                "current_directory": current_directory,
                "is_ready": is_ready(&surface.surface_id),
                "is_focused": workspace.focused_panel_id.as_deref()
                    == Some(surface.surface_id.as_str()),
            }))
        })
        .collect()
}

pub(super) fn terminal_groups_for_mobile(
    window: &SessionWindowSnapshot,
    targeted: bool,
) -> Vec<Value> {
    if targeted {
        return Vec::new();
    }
    window
        .tab_manager
        .workspace_groups
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|group| {
            let members = window
                .tab_manager
                .workspaces
                .iter()
                .filter(|workspace| workspace.group_id.as_deref() == Some(group.id.as_str()))
                .filter_map(|workspace| workspace.workspace_id.clone())
                .collect::<Vec<_>>();
            let anchor_workspace_id = group
                .anchor_workspace_id
                .as_ref()
                .filter(|anchor| members.contains(anchor))
                .cloned()
                .or_else(|| {
                    group
                        .anchor_member_index
                        .and_then(|index| usize::try_from(index).ok())
                        .and_then(|index| members.get(index).cloned())
                })
                .or_else(|| members.first().cloned());
            json!({
                "id": group.id,
                "name": group.name,
                "is_collapsed": group.is_collapsed,
                "is_pinned": group.is_pinned.unwrap_or(false),
                "anchor_workspace_id": anchor_workspace_id,
                "member_workspace_ids": members,
            })
        })
        .collect()
}

pub(super) fn mobile_workspace_preview(raw: &str) -> Option<String> {
    const MAX_LENGTH: usize = 140;
    const INPUT_CAP: usize = MAX_LENGTH * 16;

    let mut raw = raw.chars();
    let bounded = raw.by_ref().take(INPUT_CAP).collect::<String>();
    let input_was_truncated = raw.next().is_some();
    let collapsed = mobile_preview_without_ansi(&bounded)
        .chars()
        .map(|character| {
            if character.is_control() || character.is_whitespace() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        None
    } else if collapsed.chars().count() <= MAX_LENGTH && !input_was_truncated {
        Some(collapsed)
    } else {
        Some(format!(
            "{}…",
            collapsed.chars().take(MAX_LENGTH - 1).collect::<String>()
        ))
    }
}

pub(super) fn mobile_preview_without_ansi(raw: &str) -> String {
    let mut characters = raw.chars().peekable();
    let mut plain = String::with_capacity(raw.len());
    while let Some(character) = characters.next() {
        if character != '\u{1b}' {
            plain.push(character);
            continue;
        }
        match characters.peek().copied() {
            Some('[') => {
                characters.next();
                for character in characters.by_ref() {
                    if ('@'..='~').contains(&character) {
                        break;
                    }
                }
            }
            Some(']') => {
                characters.next();
                while let Some(character) = characters.next() {
                    if character == '\u{7}' {
                        break;
                    }
                    if character == '\u{1b}' && characters.peek() == Some(&'\\') {
                        characters.next();
                        break;
                    }
                }
            }
            Some(character)
                if ('@'..='Z').contains(&character) || ('\\'..='_').contains(&character) =>
            {
                characters.next();
            }
            _ => {}
        }
    }
    plain
}

pub(super) fn surface_split(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(orientation) = split_orientation_from_params(params) else {
        return invalid_params("Invalid split orientation");
    };
    let insert_first = insert_first_param(params);
    let (initial_terminal_command, initial_terminal_input, initial_terminal_environment) =
        terminal_startup_params(params);
    let state = app.state::<SessionState>();
    match split_panel_for_control(
        app,
        &state,
        &panel_id,
        orientation,
        insert_first,
        initial_terminal_command.as_deref(),
        initial_terminal_input.as_deref(),
        initial_terminal_environment,
    ) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(TerminalPanelCreateError::NotFound(message)) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message,
            data: None,
        },
        Err(TerminalPanelCreateError::Publication(message)) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_new_terminal_tab(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let (initial_terminal_command, initial_terminal_input, initial_terminal_environment) =
        terminal_startup_params(params);
    let state = app.state::<SessionState>();
    match new_terminal_tab_for_control(
        app,
        &state,
        &panel_id,
        initial_terminal_command.as_deref(),
        initial_terminal_input.as_deref(),
        initial_terminal_environment,
    ) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(TerminalPanelCreateError::NotFound(message)) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message,
            data: None,
        },
        Err(TerminalPanelCreateError::Publication(message)) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_split_browser(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(orientation) = split_orientation_from_params(params) else {
        return invalid_params("Invalid split orientation");
    };
    let url = raw_string_param(params, &["url"]);
    let state = app.state::<SessionState>();
    match split_browser_for_control(
        app,
        &state,
        &panel_id,
        orientation,
        insert_first_param(params),
        url.as_deref(),
    ) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(BrowserPanelCreateError::NotFound(message)) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message,
            data: None,
        },
        Err(BrowserPanelCreateError::Publication(message)) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn global_surface_location(
    snapshot: &AppSessionSnapshot,
    panel_id: &str,
) -> Option<(usize, usize)> {
    snapshot
        .windows
        .iter()
        .enumerate()
        .find_map(|(window_index, window)| {
            window
                .tab_manager
                .workspaces
                .iter()
                .enumerate()
                .find(|(_, workspace)| {
                    surfaces_for_workspace(workspace)
                        .iter()
                        .any(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id))
                })
                .map(|(workspace_index, _)| (window_index, workspace_index))
        })
}

pub(super) fn split_off_window_index(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<Option<usize>> {
    let Some(selector) = raw_string_param(params, &["window_ref", "window_id"]) else {
        return Some(None);
    };
    let identity = crate::window::current_control_window(app, Some(&selector))?;
    snapshot
        .windows
        .iter()
        .position(|window| window.window_id.as_deref() == Some(identity.label.as_str()))
        .map(Some)
}

pub(super) fn split_off_workspace_index(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    window_index: usize,
) -> Option<Option<usize>> {
    let window = snapshot.windows.get(window_index)?;
    if let Some(reference) = string_param(params, &["workspace_ref"]) {
        return one_based_ref_index(&reference, "workspace")
            .filter(|index| *index < window.tab_manager.workspaces.len())
            .map(Some);
    }
    if let Some(workspace_id) = string_param(params, &["workspace_id"]) {
        return window
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
            .map(Some);
    }
    Some(None)
}

pub(super) fn surface_split_off(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(direction) = string_param(params, &["direction"]) else {
        return invalid_params("Missing or invalid direction (left|right|up|down)");
    };
    let Some(orientation) = split_orientation_from_params(params) else {
        return invalid_params("Missing or invalid direction (left|right|up|down)");
    };
    let normalized_direction = direction.to_ascii_lowercase();
    let insert_first = matches!(normalized_direction.as_str(), "left" | "up" | "l" | "u");
    let current = snapshot(app);
    let requested_window_resolution = split_off_window_index(app, &current, params);
    let requested_window_index = requested_window_resolution.flatten();

    let direct_panel_id = string_param(params, &["surface_id", "panel_id"]);
    let (window_index, workspace_index, panel_id) = if let Some(panel_id) = direct_panel_id {
        let Some((window_index, workspace_index)) = global_surface_location(&current, &panel_id)
        else {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: Some(
                    json!({"surface_id": panel_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        };
        (window_index, workspace_index, panel_id)
    } else if let Some(surface_reference) = string_param(params, &["surface_ref"]) {
        if requested_window_resolution.is_none() {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found in window".to_string(),
                data: None,
            };
        }
        let window_index = requested_window_index.unwrap_or(0);
        let requested_workspace_index =
            match split_off_workspace_index(&current, params, window_index) {
                Some(index) => index,
                None => {
                    return ControlCallResult::Err {
                        code: "not_found".to_string(),
                        message: "Surface not found in workspace".to_string(),
                        data: None,
                    };
                }
            };
        let window = &current.windows[window_index];
        let workspace_index = requested_workspace_index.unwrap_or_else(|| {
            window
                .tab_manager
                .selected_workspace_index
                .and_then(|index| usize::try_from(index).ok())
                .unwrap_or(0)
        });
        let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found in workspace".to_string(),
                data: None,
            };
        };
        let Some(surface_index) = one_based_ref_index(&surface_reference, "surface") else {
            return invalid_params("Missing or invalid surface_id");
        };
        let Some(panel_id) = surfaces_for_workspace(workspace)
            .get(surface_index)
            .and_then(|surface| surface.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: None,
            };
        };
        (window_index, workspace_index, panel_id)
    } else {
        return invalid_params("Missing or invalid surface_id");
    };

    if requested_window_resolution.is_none()
        || requested_window_index.is_some_and(|requested| requested != window_index)
    {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Surface not found in window".to_string(),
            data: None,
        };
    }
    let requested_workspace_index = match split_off_workspace_index(
        &current,
        params,
        requested_window_index.unwrap_or(window_index),
    ) {
        Some(index) => index,
        None => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found in workspace".to_string(),
                data: None,
            };
        }
    };
    if requested_workspace_index.is_some_and(|requested| requested != workspace_index) {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Surface not found in workspace".to_string(),
            data: None,
        };
    }

    let workspace = &current.windows[window_index].tab_manager.workspaces[workspace_index];
    let Some((_, source_pane_id, _)) = surface_pane_details(workspace, &panel_id) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Source pane not found".to_string(),
            data: None,
        };
    };
    let state = app.state::<SessionState>();
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let result = match split_off_surface_for_control(
        app,
        &state,
        window_index,
        workspace_index,
        &panel_id,
        orientation,
        insert_first,
        focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(
            session_ops::SplitOffSurfaceError::WouldEmptySourcePane,
        )) => {
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message: "splitting off would leave the source pane empty".to_string(),
                data: Some(
                    json!({"surface_id": panel_id, "pane_id": source_pane_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        Err(PaneTopologyControlError::Operation(
            session_ops::SplitOffSurfaceError::SurfaceNotFound,
        )) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
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
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let Some((pane_index, pane_id, _)) = surface_pane_details(workspace, &panel_id) else {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: "Failed to split pane".to_string(),
            data: None,
        };
    };
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window.window_id.as_deref().unwrap_or("main"))
        .map(|summary| summary.identity);
    let (window_id, window_ref) =
        pane_response_window_identity(window, window_index, window_identity.as_ref());
    if focus {
        if let Some(identity) = window_identity.as_ref() {
            let _ = crate::window::activate_control_window(app, &identity.label);
        }
    }
    let surface_ref_value = surfaces_for_workspace(workspace)
        .iter()
        .position(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id.as_str()))
        .map(surface_ref);
    ok(json!({
        "window_id": window_id,
        "window_ref": window_ref,
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_value,
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
    }))
}

pub(super) fn global_pane_location(
    snapshot: &AppSessionSnapshot,
    pane_id: &str,
) -> Option<(usize, usize, usize)> {
    snapshot
        .windows
        .iter()
        .enumerate()
        .find_map(|(window_index, window)| {
            window.tab_manager.workspaces.iter().enumerate().find_map(
                |(workspace_index, workspace)| {
                    pane_index_by_id(workspace, pane_id)
                        .map(|pane_index| (window_index, workspace_index, pane_index))
                },
            )
        })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PanePixelFrame {
    pub(super) x: f64,
    pub(super) y: f64,
    pub(super) width: f64,
    pub(super) height: f64,
}

pub(super) fn pane_frames(
    layout: &SessionWorkspaceLayoutSnapshot,
    frame: PanePixelFrame,
    rows: &mut Vec<(SessionPaneLayoutSnapshot, PanePixelFrame)>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => rows.push((pane.clone(), frame)),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            let divider = if split.divider_position.is_finite() {
                split.divider_position.clamp(0.1, 0.9)
            } else {
                0.5
            };
            let (first, second) = match split.orientation {
                SessionSplitOrientation::Horizontal => {
                    let first_width = frame.width * divider;
                    (
                        PanePixelFrame {
                            width: first_width,
                            ..frame
                        },
                        PanePixelFrame {
                            x: frame.x + first_width,
                            width: frame.width - first_width,
                            ..frame
                        },
                    )
                }
                SessionSplitOrientation::Vertical => {
                    let first_height = frame.height * divider;
                    (
                        PanePixelFrame {
                            height: first_height,
                            ..frame
                        },
                        PanePixelFrame {
                            y: frame.y + first_height,
                            height: frame.height - first_height,
                            ..frame
                        },
                    )
                }
            };
            pane_frames(&split.first, first, rows);
            pane_frames(&split.second, second, rows);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaneFocusResolveError {
    WorkspaceNotFound,
    PaneNotFound,
}

pub(super) fn resolve_pane_focus_target(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    window_index: usize,
) -> Result<(usize, usize, String), PaneFocusResolveError> {
    let window = snapshot
        .windows
        .get(window_index)
        .ok_or(PaneFocusResolveError::WorkspaceNotFound)?;
    let workspace_index = split_off_workspace_index(snapshot, params, window_index)
        .ok_or(PaneFocusResolveError::WorkspaceNotFound)?
        .or_else(|| {
            window
                .tab_manager
                .selected_workspace_index
                .and_then(|index| usize::try_from(index).ok())
        })
        .unwrap_or(0);
    let workspace = window
        .tab_manager
        .workspaces
        .get(workspace_index)
        .ok_or(PaneFocusResolveError::WorkspaceNotFound)?;
    if let Some(pane_id) = string_param(params, &["pane_id"]) {
        return pane_index_by_id(workspace, &pane_id)
            .map(|pane_index| (workspace_index, pane_index, pane_id))
            .ok_or(PaneFocusResolveError::PaneNotFound);
    }
    let pane_index = string_param(params, &["pane_ref"])
        .and_then(|reference| one_based_ref_index(&reference, "pane"))
        .ok_or(PaneFocusResolveError::PaneNotFound)?;
    let (_, pane_id) =
        pane_at_index(workspace, pane_index).ok_or(PaneFocusResolveError::PaneNotFound)?;
    Ok((workspace_index, pane_index, pane_id))
}

pub(super) fn pane_focus(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    if current.windows.is_empty() {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    }
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    if !params.contains_key("pane_id") && !params.contains_key("pane_ref") {
        return invalid_params("Missing or invalid pane_id");
    }
    let window_index = requested_window.unwrap_or_else(|| {
        crate::window::current_control_window(app, None)
            .and_then(|identity| {
                current
                    .windows
                    .iter()
                    .position(|window| window.window_id.as_deref() == Some(identity.label.as_str()))
            })
            .unwrap_or(0)
    });
    let (workspace_index, pane_index, pane_id) =
        match resolve_pane_focus_target(&current, params, window_index) {
            Ok(target) => target,
            Err(PaneFocusResolveError::WorkspaceNotFound) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "Workspace not found".to_string(),
                    data: None,
                };
            }
            Err(PaneFocusResolveError::PaneNotFound) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "Pane not found".to_string(),
                    data: Some(
                        json!({"pane_id": string_param(params, &["pane_id", "pane_ref"])})
                            .try_into()
                            .unwrap_or(JsonValue::Null),
                    ),
                };
            }
        };
    let state = app.state::<SessionState>();
    let result = match focus_pane_for_control(app, &state, window_index, workspace_index, &pane_id)
    {
        Ok(snapshot) => snapshot,
        Err(PaneTopologyControlError::Operation(PaneFocusControlError::WorkspaceNotFound)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Workspace not found".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneFocusControlError::PaneNotFound)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Pane not found".to_string(),
                data: Some(
                    json!({"pane_id": pane_id})
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
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let window_label = window.window_id.as_deref().unwrap_or("main");
    let identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window_label)
        .map(|summary| summary.identity);
    if let Some(identity) = identity.as_ref() {
        let _ = crate::window::activate_control_window(app, &identity.label);
    }
    ok(json!({
        "window_id": identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": identity.as_ref().map(|identity| identity.reference.clone()),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
    }))
}

fn pane_response_window_identity(
    _window: &SessionWindowSnapshot,
    _window_index: usize,
    native_identity: Option<&cmux_core::window_display::WindowControlIdentity>,
) -> (Option<String>, Option<String>) {
    (
        native_identity.map(|identity| identity.id.clone()),
        native_identity.map(|identity| identity.reference.clone()),
    )
}

pub(super) fn pane_snapshot_at_index(
    workspace: &SessionWorkspaceSnapshot,
    target_index: usize,
) -> Option<&SessionPaneLayoutSnapshot> {
    fn visit<'a>(
        layout: &'a SessionWorkspaceLayoutSnapshot,
        target_index: usize,
        index: &mut usize,
    ) -> Option<&'a SessionPaneLayoutSnapshot> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                let current = *index;
                *index += 1;
                (current == target_index).then_some(pane)
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit(&split.first, target_index, index)
                    .or_else(|| visit(&split.second, target_index, index))
            }
        }
    }
    let mut index = 0;
    visit(workspace.layout.as_ref()?, target_index, &mut index)
}

pub(super) fn resolve_pane_surfaces_target(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    window_index: usize,
) -> Option<(usize, usize, String)> {
    let window = snapshot.windows.get(window_index)?;
    let requested_workspace = split_off_workspace_index(snapshot, params, window_index)?;
    let selected_workspace = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .unwrap_or(0);
    if let Some(pane_id) = string_param(params, &["pane_id"]) {
        if let Some(workspace_index) = requested_workspace {
            let workspace = window.tab_manager.workspaces.get(workspace_index)?;
            return pane_index_by_id(workspace, &pane_id)
                .map(|pane_index| (workspace_index, pane_index, pane_id));
        }
        return window.tab_manager.workspaces.iter().enumerate().find_map(
            |(workspace_index, workspace)| {
                pane_index_by_id(workspace, &pane_id)
                    .map(|pane_index| (workspace_index, pane_index, pane_id.clone()))
            },
        );
    }
    let workspace_index = requested_workspace.unwrap_or(selected_workspace);
    let workspace = window.tab_manager.workspaces.get(workspace_index)?;
    if let Some(reference) = string_param(params, &["pane_ref"]) {
        let pane_index = one_based_ref_index(&reference, "pane")?;
        let (_, pane_id) = pane_at_index(workspace, pane_index)?;
        return Some((workspace_index, pane_index, pane_id));
    }
    workspace
        .focused_panel_id
        .as_deref()
        .and_then(|panel_id| surface_pane_details(workspace, panel_id))
        .and_then(|(pane_index, pane_id, _)| {
            pane_id.map(|pane_id| (workspace_index, pane_index, pane_id))
        })
}

pub(super) fn pane_surfaces(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let lifecycle = match session_ops::read_surface_lifecycle(&current) {
        Ok(model) => model,
        Err(error) => {
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message: format!("Invalid surface lifecycle: {error}"),
                data: None,
            };
        }
    };
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or_else(|| {
        crate::window::current_control_window(app, None)
            .and_then(|identity| {
                current
                    .windows
                    .iter()
                    .position(|window| window.window_id.as_deref() == Some(identity.label.as_str()))
            })
            .unwrap_or(0)
    });
    let Some((workspace_index, pane_index, pane_id)) =
        resolve_pane_surfaces_target(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Pane or workspace not found".to_string(),
            data: None,
        };
    };
    let window = &current.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let pane = pane_snapshot_at_index(workspace, pane_index)
        .expect("resolved pane index remains in the immutable snapshot");
    let workspace_surface_ids = surfaces_for_workspace(workspace)
        .into_iter()
        .filter_map(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    let surfaces = pane
        .panel_ids
        .iter()
        .enumerate()
        .map(|(index, panel_id)| {
            let surface_type = lifecycle
                .surface(panel_id)
                .map(|surface| surface_kind_label(&surface.kind))
                .unwrap_or_else(|| pane.surface_kind.as_deref().unwrap_or("terminal"));
            json!({
                "id": panel_id,
                "ref": workspace_surface_ids.iter().position(|id| id == panel_id).map(surface_ref),
                "index": index,
                "title": lifecycle.surface(panel_id).and_then(|surface| surface.metadata.custom_title.clone()).or_else(|| panel_title(&workspace.panel_titles, panel_id)).unwrap_or_else(|| surface_type.to_string()),
                "type": surface_type,
                "selected": pane.selected_panel_id.as_deref() == Some(panel_id.as_str()),
            })
        })
        .collect::<Vec<_>>();
    let window_label = window.window_id.as_deref().unwrap_or("main");
    let identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window_label)
        .map(|summary| summary.identity);
    let (window_id, window_ref) =
        pane_response_window_identity(window, window_index, identity.as_ref());
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
        "surfaces": surfaces,
        "window_id": window_id,
        "window_ref": window_ref,
    }))
}

pub(super) fn pane_swap(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    if !params.contains_key("pane_id") && !params.contains_key("pane_ref") {
        return invalid_params("Missing or invalid pane_id");
    }
    if !params.contains_key("target_pane_id") && !params.contains_key("target_pane_ref") {
        return invalid_params("Missing or invalid target_pane_id");
    }
    let current = snapshot(app);
    let source = if let Some(pane_id) = string_param(params, &["pane_id"]) {
        global_pane_location(&current, &pane_id).map(|location| (location, pane_id))
    } else {
        (|| {
            let requested_window = split_off_window_index(app, &current, params)?;
            let window_index = requested_window.unwrap_or(0);
            let requested_workspace = split_off_workspace_index(&current, params, window_index)?;
            let workspace_index = requested_workspace.unwrap_or_else(|| {
                current.windows[window_index]
                    .tab_manager
                    .selected_workspace_index
                    .and_then(|index| usize::try_from(index).ok())
                    .unwrap_or(0)
            });
            let pane_index = string_param(params, &["pane_ref"])
                .and_then(|reference| one_based_ref_index(&reference, "pane"))?;
            let workspace = current
                .windows
                .get(window_index)?
                .tab_manager
                .workspaces
                .get(workspace_index)?;
            pane_at_index(workspace, pane_index)
                .map(|(_, pane_id)| ((window_index, workspace_index, pane_index), pane_id))
        })()
    };
    let Some(((window_index, workspace_index, source_pane_index), source_pane_id)) = source else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Source pane not found".to_string(),
            data: None,
        };
    };
    let workspace = &current.windows[window_index].tab_manager.workspaces[workspace_index];
    let target = if let Some(target_pane_id) = string_param(params, &["target_pane_id"]) {
        global_pane_location(&current, &target_pane_id)
            .filter(|(target_window, target_workspace, _)| {
                *target_window == window_index && *target_workspace == workspace_index
            })
            .map(|(_, _, pane_index)| (pane_index, target_pane_id))
    } else {
        string_param(params, &["target_pane_ref"])
            .and_then(|reference| one_based_ref_index(&reference, "pane"))
            .and_then(|pane_index| pane_at_index(workspace, pane_index))
    };
    let Some((target_pane_index, target_pane_id)) = target else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Target pane not found in source workspace".to_string(),
            data: None,
        };
    };
    if source_pane_id == target_pane_id {
        return invalid_params("pane_id and target_pane_id must be different");
    }
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    let (swap, result) = match swap_panes_for_control(
        app,
        &state,
        window_index,
        workspace_index,
        &source_pane_id,
        &target_pane_id,
        focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(session_ops::PaneSwapError::SamePane)) => {
            return invalid_params("pane_id and target_pane_id must be different");
        }
        Err(PaneTopologyControlError::Operation(
            session_ops::PaneSwapError::SourcePaneNotFound,
        )) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Source pane not found".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(
            session_ops::PaneSwapError::TargetPaneNotFound,
        )) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Target pane not found in source workspace".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(
            session_ops::PaneSwapError::BothPanesNeedSurface,
        )) => {
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message: "Both panes must have a selected surface".to_string(),
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
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let surface_ref_value = |panel_id: &str| {
        surfaces_for_workspace(workspace)
            .iter()
            .position(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id))
            .map(surface_ref)
    };
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window.window_id.as_deref().unwrap_or("main"))
        .map(|summary| summary.identity);
    let (window_id, window_ref) =
        pane_response_window_identity(window, window_index, window_identity.as_ref());
    if focus {
        if let Some(identity) = window_identity.as_ref() {
            let _ = crate::window::activate_control_window(app, &identity.label);
        }
    }
    ok(json!({
        "window_id": window_id,
        "window_ref": window_ref,
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": source_pane_id,
        "pane_ref": pane_ref(source_pane_index),
        "target_pane_id": target_pane_id,
        "target_pane_ref": pane_ref(target_pane_index),
        "source_surface_id": swap.source_surface_id,
        "source_surface_ref": surface_ref_value(&swap.source_surface_id),
        "target_surface_id": swap.target_surface_id,
        "target_surface_ref": surface_ref_value(&swap.target_surface_id),
    }))
}

pub(super) fn pane_break(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or(0);
    let Some(requested_workspace) = split_off_workspace_index(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(window) = current.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let workspace_index = requested_workspace.unwrap_or_else(|| {
        window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(0)
    });
    let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let surfaces = surfaces_for_workspace(workspace);
    let source_pane_id = if let Some(pane_id) = string_param(params, &["pane_id"]) {
        pane_index_by_id(workspace, &pane_id).map(|_| pane_id)
    } else if let Some(pane_reference) = string_param(params, &["pane_ref"]) {
        one_based_ref_index(&pane_reference, "pane")
            .and_then(|pane_index| pane_at_index(workspace, pane_index))
            .map(|(_, pane_id)| pane_id)
    } else {
        first_or_focused_pane(workspace).map(|(_, pane_id)| pane_id)
    };
    let explicit_surface_id = string_param(params, &["surface_id"]);
    let panel_id = if let Some(panel_id) = explicit_surface_id.as_ref() {
        surfaces
            .iter()
            .any(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id.as_str()))
            .then(|| panel_id.clone())
    } else if let Some(surface_reference) = string_param(params, &["surface_ref"]) {
        one_based_ref_index(&surface_reference, "surface").and_then(|surface_index| {
            surfaces
                .get(surface_index)
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
    } else if let Some(pane_id) = source_pane_id.as_deref() {
        surfaces
            .iter()
            .find(|surface| {
                surface.get("pane_id").and_then(Value::as_str) == Some(pane_id)
                    && surface
                        .get("selected_in_pane")
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
            })
            .and_then(|surface| surface.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
    } else {
        surfaces
            .iter()
            .find(|surface| {
                surface
                    .get("focused")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
            })
            .or_else(|| surfaces.first())
            .and_then(|surface| surface.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let Some(panel_id) = panel_id else {
        if let Some(surface_id) = explicit_surface_id {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: Some(
                    json!({"surface_id": surface_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No source surface to break".to_string(),
            data: None,
        };
    };
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    let (broken, result) = match break_pane_for_control(
        app,
        &state,
        window_index,
        workspace_index,
        &panel_id,
        focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(
            session_ops::PaneBreakError::WorkspaceNotFound,
        )) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Workspace not found".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(session_ops::PaneBreakError::SurfaceNotFound)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: Some(
                    json!({"surface_id": panel_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        Err(PaneTopologyControlError::Operation(session_ops::PaneBreakError::DetachFailed)) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: "Failed to detach source surface".to_string(),
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
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[broken.workspace_index];
    let pane_id = surface_pane_details(workspace, &broken.surface_id)
        .and_then(|(_, pane_id, _)| pane_id)
        .expect("state layer mints destination pane ids");
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window.window_id.as_deref().unwrap_or("main"))
        .map(|summary| summary.identity);
    let (window_id, window_ref) =
        pane_response_window_identity(window, window_index, window_identity.as_ref());
    if focus {
        if let Some(identity) = window_identity.as_ref() {
            let _ = crate::window::activate_control_window(app, &identity.label);
        }
    }
    ok(json!({
        "window_id": window_id,
        "window_ref": window_ref,
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(broken.workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(0),
        "surface_id": broken.surface_id,
        "surface_ref": surface_ref(0),
    }))
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PaneJoinSourceError {
    Missing,
    SourcePaneUnresolved(String),
}

pub(super) fn resolve_pane_join_source(
    current: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<String, PaneJoinSourceError> {
    if let Some(surface_id) = string_param(params, &["surface_id"]) {
        return Ok(surface_id);
    }
    if params.contains_key("surface_ref") {
        let workspace_index = workspace_index_from_workspace_scope_or_selected(current, params)
            .ok_or(PaneJoinSourceError::Missing)?;
        let workspace = current
            .windows
            .first()
            .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
            .ok_or(PaneJoinSourceError::Missing)?;
        return surface_id_from_selector_keys(workspace, params, &["surface_ref"], &["surface_id"])
            .ok_or(PaneJoinSourceError::Missing);
    }
    let (workspace_index, pane_id) = if let Some(pane_id) = string_param(params, &["pane_id"]) {
        pane_location_by_id(current, &pane_id)
            .map(|(workspace_index, _, pane_id)| (workspace_index, pane_id))
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_id))?
    } else if let Some(pane_reference) = string_param(params, &["pane_ref"]) {
        let workspace_index = workspace_index_from_workspace_scope_or_selected(current, params)
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_reference.clone()))?;
        let workspace = current
            .windows
            .first()
            .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_reference.clone()))?;
        let pane_index = one_based_ref_index(&pane_reference, "pane")
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_reference.clone()))?;
        pane_at_index(workspace, pane_index)
            .map(|(_, pane_id)| (workspace_index, pane_id))
            .ok_or(PaneJoinSourceError::SourcePaneUnresolved(pane_reference))?
    } else {
        return Err(PaneJoinSourceError::Missing);
    };
    let workspace = &current.windows[0].tab_manager.workspaces[workspace_index];
    surfaces_for_workspace(workspace)
        .into_iter()
        .find(|surface| {
            surface.get("pane_id").and_then(Value::as_str) == Some(pane_id.as_str())
                && surface
                    .get("selected_in_pane")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .and_then(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or(PaneJoinSourceError::SourcePaneUnresolved(pane_id))
}

pub(super) fn pane_join(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    if !params.contains_key("target_pane_id") && !params.contains_key("target_pane_ref") {
        return invalid_params("Missing or invalid target_pane_id");
    }
    let current = snapshot(app);
    let source_panel_id = match resolve_pane_join_source(&current, params) {
        Ok(panel_id) => panel_id,
        Err(PaneJoinSourceError::Missing) => {
            return invalid_params("Missing surface_id (or pane_id with selected surface)");
        }
        Err(PaneJoinSourceError::SourcePaneUnresolved(pane_id)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Unable to resolve selected surface in source pane".to_string(),
                data: Some(
                    json!({"pane_id": pane_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    let mut move_params = serde_json::Map::new();
    move_params.insert("surface_id".to_string(), json!(source_panel_id));
    for key in [
        "target_pane_id",
        "target_pane_ref",
        "workspace_id",
        "workspace_ref",
        "window_id",
        "window_ref",
        "focus",
    ] {
        if let Some(value) = params.get(key) {
            let move_key = key.strip_prefix("target_").unwrap_or(key);
            move_params.insert(move_key.to_string(), value.clone());
        }
    }
    surface_move(app, &move_params)
}

pub(super) fn pane_last(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or(0);
    let Some(requested_workspace) = split_off_workspace_index(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(window) = current.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let workspace_index = requested_workspace.unwrap_or_else(|| {
        window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(0)
    });
    if window.tab_manager.workspaces.get(workspace_index).is_none() {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    }
    let state = app.state::<SessionState>();
    let (focused, result) =
        match focus_last_pane_for_control(app, &state, window_index, workspace_index) {
            Ok(result) => result,
            Err(PaneTopologyControlError::Operation(PaneLastControlError::WorkspaceNotFound)) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "Workspace not found".to_string(),
                    data: None,
                };
            }
            Err(PaneTopologyControlError::Operation(PaneLastControlError::Pane(
                session_ops::PaneLastError::NoFocusedPane,
            ))) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "No focused pane".to_string(),
                    data: None,
                };
            }
            Err(PaneTopologyControlError::Operation(PaneLastControlError::Pane(
                session_ops::PaneLastError::NoAlternatePane,
            ))) => {
                return ControlCallResult::Err {
                    code: "not_found".to_string(),
                    message: "No alternate pane available".to_string(),
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
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window.window_id.as_deref().unwrap_or("main"))
        .map(|summary| summary.identity);
    let (window_id, window_ref) =
        pane_response_window_identity(window, window_index, window_identity.as_ref());
    if let Some(identity) = window_identity.as_ref() {
        let _ = crate::window::activate_control_window(app, &identity.label);
    }
    let surface_ref_value = focused.surface_id.as_deref().and_then(|surface_id| {
        surfaces_for_workspace(workspace)
            .iter()
            .position(|surface| surface.get("id").and_then(Value::as_str) == Some(surface_id))
            .map(surface_ref)
    });
    let pane_index = pane_index_by_id(workspace, &focused.pane_id).unwrap_or(0);
    ok(json!({
        "window_id": window_id,
        "window_ref": window_ref,
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": focused.pane_id,
        "pane_ref": pane_ref(pane_index),
        "surface_id": focused.surface_id,
        "surface_ref": surface_ref_value,
    }))
}

pub(super) fn pane_resize(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let intent = if params.contains_key("absolute_axis") || params.contains_key("target_pixels") {
        let axis = match string_param(params, &["absolute_axis"]).as_deref() {
            Some("horizontal") => SessionSplitOrientation::Horizontal,
            Some("vertical") => SessionSplitOrientation::Vertical,
            _ => return invalid_params("absolute_axis must be 'horizontal' or 'vertical'"),
        };
        let Some(target_pixels) =
            f64_param(params, &["target_pixels"]).filter(|value| value.is_finite() && *value > 0.0)
        else {
            return invalid_params("target_pixels must be > 0");
        };
        PaneResizeControlIntent::Absolute {
            axis,
            target_pixels,
        }
    } else {
        let direction = match string_param(params, &["direction"]).as_deref() {
            Some("left") => session_ops::PaneResizeDirection::Left,
            Some("right") => session_ops::PaneResizeDirection::Right,
            Some("up") => session_ops::PaneResizeDirection::Up,
            Some("down") => session_ops::PaneResizeDirection::Down,
            _ => {
                return invalid_params(
                    "direction must be one of left|right|up|down and amount must be > 0",
                );
            }
        };
        let Some(amount) = i64_param(params, &["amount"])
            .filter(|amount| *amount > 0)
            .and_then(|amount| u64::try_from(amount).ok())
        else {
            return invalid_params(
                "direction must be one of left|right|up|down and amount must be > 0",
            );
        };
        PaneResizeControlIntent::Relative { direction, amount }
    };

    let current = snapshot(app);
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or(0);
    let Some(requested_workspace) = split_off_workspace_index(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(window) = current.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let workspace_index = requested_workspace.unwrap_or_else(|| {
        window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(0)
    });
    let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some((pane_index, pane_id)) = resolve_resize_pane(workspace, params) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Pane not found".to_string(),
            data: None,
        };
    };

    let window_label = window.window_id.as_deref().unwrap_or("main");
    let (width, height) = app
        .get_webview_window(window_label)
        .and_then(|window| window.inner_size().ok())
        .map(|size| (f64::from(size.width), f64::from(size.height)))
        .unwrap_or((1.0, 1.0));
    let state = app.state::<SessionState>();
    let (resized, result) = match resize_pane_for_control(
        app,
        &state,
        window_index,
        workspace_index,
        &pane_id,
        intent.clone(),
        width,
        height,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::WorkspaceNotFound)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Workspace not found".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
            session_ops::PaneResizeError::PaneNotFoundInTree,
        ))) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Pane not found in split tree".to_string(),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
            session_ops::PaneResizeError::NoOrientationSplitAncestor,
        ))) => {
            let message = match intent {
                PaneResizeControlIntent::Absolute { .. } => {
                    "No split ancestor for absolute pane resize".to_string()
                }
                PaneResizeControlIntent::Relative { direction, .. } => format!(
                    "No {} split ancestor for pane",
                    match direction {
                        session_ops::PaneResizeDirection::Left
                        | session_ops::PaneResizeDirection::Right => "horizontal",
                        session_ops::PaneResizeDirection::Up
                        | session_ops::PaneResizeDirection::Down => "vertical",
                    }
                ),
            };
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message,
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
            session_ops::PaneResizeError::NoAdjacentBorder,
        ))) => {
            let direction = match intent {
                PaneResizeControlIntent::Relative { direction, .. } => match direction {
                    session_ops::PaneResizeDirection::Left => "left",
                    session_ops::PaneResizeDirection::Right => "right",
                    session_ops::PaneResizeDirection::Up => "up",
                    session_ops::PaneResizeDirection::Down => "down",
                },
                PaneResizeControlIntent::Absolute { .. } => unreachable!(),
            };
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message: format!("Pane has no adjacent border in direction {direction}"),
                data: None,
            };
        }
        Err(PaneTopologyControlError::Operation(PaneResizeControlError::Pane(
            session_ops::PaneResizeError::MissingSplitIdentity,
        ))) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: "Failed to resize pane".to_string(),
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
    let window = &result.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let window_identity = crate::window::control_window_summaries(app)
        .into_iter()
        .find(|summary| summary.identity.label == window_label)
        .map(|summary| summary.identity);
    let mut payload = json!({
        "window_id": window_identity.as_ref().map(|identity| identity.id.clone()),
        "window_ref": window_identity.as_ref().map(|identity| identity.reference.clone()),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
        "split_id": resized.split_id,
        "old_divider_position": resized.old_divider_position,
        "new_divider_position": resized.new_divider_position,
    });
    if let Some(object) = payload.as_object_mut() {
        match intent {
            PaneResizeControlIntent::Relative { direction, amount } => {
                let direction = match direction {
                    session_ops::PaneResizeDirection::Left => "left",
                    session_ops::PaneResizeDirection::Right => "right",
                    session_ops::PaneResizeDirection::Up => "up",
                    session_ops::PaneResizeDirection::Down => "down",
                };
                object.insert("direction".to_string(), json!(direction));
                object.insert("amount".to_string(), json!(amount));
            }
            PaneResizeControlIntent::Absolute {
                axis,
                target_pixels,
            } => {
                object.insert(
                    "absolute_axis".to_string(),
                    json!(match axis {
                        SessionSplitOrientation::Horizontal => "horizontal",
                        SessionSplitOrientation::Vertical => "vertical",
                    }),
                );
                object.insert("target_pixels".to_string(), json!(target_pixels));
            }
        }
    }
    ok(payload)
}

pub(super) fn surface_close(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    surface_list_from_params(&close_panel_for_control(app, &state, &panel_id), params)
}

pub(super) fn surface_set_kind(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let kind = surface_kind_from_params(params);
    if matches!(kind.as_deref(), Some("invalid")) {
        return invalid_params("Invalid surface type");
    }
    let state = app.state::<SessionState>();
    match set_surface_kind_for_control(app, &state, &panel_id, kind) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_set_title(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(title) = raw_string_param(params, &["title", "name"]) else {
        return invalid_params("Missing surface title");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_title_for_control(app, &state, &panel_id, &title) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_set_pinned(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(pinned) = bool_param(params, &["pinned", "is_pinned"]) else {
        return invalid_params("Missing or invalid surface pinned flag");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_pinned_for_control(app, &state, &panel_id, pinned) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_set_unread(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(unread) = bool_param(params, &["unread", "is_unread"]) else {
        return invalid_params("Missing or invalid surface unread flag");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_unread_for_control(app, &state, &panel_id, unread) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SurfaceMoveResolution {
    pub(super) source_workspace_index: usize,
    pub(super) panel_id: String,
    pub(super) target_workspace_index: usize,
    pub(super) target_pane_id: String,
    pub(super) destination_index: Option<i64>,
    pub(super) focus: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SurfaceMoveResolveError {
    ConflictingAnchors,
    SourceNotFound,
    DestinationNotFound,
}

pub(super) fn pane_at_index(
    workspace: &SessionWorkspaceSnapshot,
    target_index: usize,
) -> Option<(usize, String)> {
    fn visit(
        layout: &SessionWorkspaceLayoutSnapshot,
        target_index: usize,
        index: &mut usize,
    ) -> Option<(usize, String)> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                let current = *index;
                *index += 1;
                (current == target_index)
                    .then(|| pane.pane_id.clone().map(|id| (current, id)))
                    .flatten()
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit(&split.first, target_index, index)
                    .or_else(|| visit(&split.second, target_index, index))
            }
        }
    }
    let mut index = 0;
    visit(workspace.layout.as_ref()?, target_index, &mut index)
}

pub(super) fn resolve_resize_pane(
    workspace: &SessionWorkspaceSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<(usize, String)> {
    if let Some(pane_id) = string_param(params, &["pane_id"]) {
        return pane_index_by_id(workspace, &pane_id).map(|index| (index, pane_id));
    }
    if let Some(reference) = string_param(params, &["pane_ref"]) {
        return one_based_ref_index(&reference, "pane")
            .and_then(|index| pane_at_index(workspace, index));
    }
    workspace
        .focused_panel_id
        .as_deref()
        .and_then(|panel_id| surface_pane_details(workspace, panel_id))
        .and_then(|(index, pane_id, _)| pane_id.map(|pane_id| (index, pane_id)))
}

pub(super) fn pane_index_by_id(
    workspace: &SessionWorkspaceSnapshot,
    pane_id: &str,
) -> Option<usize> {
    fn visit(
        layout: &SessionWorkspaceLayoutSnapshot,
        pane_id: &str,
        index: &mut usize,
    ) -> Option<usize> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                let current = *index;
                *index += 1;
                (pane.pane_id.as_deref() == Some(pane_id)).then_some(current)
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit(&split.first, pane_id, index).or_else(|| visit(&split.second, pane_id, index))
            }
        }
    }
    let mut index = 0;
    visit(workspace.layout.as_ref()?, pane_id, &mut index)
}

pub(super) fn pane_location_by_id(
    snapshot: &AppSessionSnapshot,
    pane_id: &str,
) -> Option<(usize, usize, String)> {
    snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .find_map(|(workspace_index, workspace)| {
            pane_index_by_id(workspace, pane_id)
                .map(|pane_index| (workspace_index, pane_index, pane_id.to_string()))
        })
}

pub(super) fn first_or_focused_pane(
    workspace: &SessionWorkspaceSnapshot,
) -> Option<(usize, String)> {
    if let Some(panel_id) = workspace.zoomed_panel_id.as_deref() {
        if let Some((pane_index, Some(pane_id), _)) = surface_pane_details(workspace, panel_id) {
            return Some((pane_index, pane_id));
        }
    }
    pane_at_index(workspace, 0)
}

pub(super) fn surface_location_by_id(
    snapshot: &AppSessionSnapshot,
    panel_id: &str,
) -> Option<(usize, String)> {
    snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .find(|(_, workspace)| {
            surfaces_for_workspace(workspace)
                .iter()
                .any(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id))
        })
        .map(|(workspace_index, _)| (workspace_index, panel_id.to_string()))
}

pub(super) fn surface_location_from_keys(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    ref_keys: &[&str],
    id_keys: &[&str],
    reference_workspace_index: usize,
) -> Option<(usize, String)> {
    if string_param(params, ref_keys).is_some() {
        let workspace = snapshot
            .windows
            .first()?
            .tab_manager
            .workspaces
            .get(reference_workspace_index)?;
        return surface_id_from_selector_keys(workspace, params, ref_keys, id_keys)
            .map(|panel_id| (reference_workspace_index, panel_id));
    }
    let panel_id = string_param(params, id_keys)?;
    surface_location_by_id(snapshot, &panel_id)
}

pub(super) fn resolve_surface_move(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<SurfaceMoveResolution, SurfaceMoveResolveError> {
    let window = snapshot
        .windows
        .first()
        .ok_or(SurfaceMoveResolveError::SourceNotFound)?;
    let selected_workspace_index = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .filter(|index| *index < window.tab_manager.workspaces.len())
        .unwrap_or(0);
    let (source_workspace_index, panel_id) = surface_location_from_keys(
        snapshot,
        params,
        &["surface_ref"],
        &["surface_id", "panel_id"],
        selected_workspace_index,
    )
    .ok_or(SurfaceMoveResolveError::SourceNotFound)?;
    let source_workspace = &window.tab_manager.workspaces[source_workspace_index];
    let (source_pane_index, source_pane_id, _) = surface_pane_details(source_workspace, &panel_id)
        .ok_or(SurfaceMoveResolveError::SourceNotFound)?;
    let source_pane_id = source_pane_id.ok_or(SurfaceMoveResolveError::DestinationNotFound)?;

    let before_specified = ["before_surface_ref", "before_surface_id"]
        .iter()
        .any(|key| params.contains_key(*key));
    let after_specified = ["after_surface_ref", "after_surface_id"]
        .iter()
        .any(|key| params.contains_key(*key));
    if before_specified && after_specified {
        return Err(SurfaceMoveResolveError::ConflictingAnchors);
    }

    let has_workspace = params.contains_key("workspace_ref") || params.contains_key("workspace_id");
    let requested_workspace_index = has_workspace
        .then(|| workspace_index_from_workspace_scope_or_selected(snapshot, params))
        .flatten();
    let anchor_reference_workspace = requested_workspace_index.unwrap_or(source_workspace_index);
    let anchor = if before_specified {
        surface_location_from_keys(
            snapshot,
            params,
            &["before_surface_ref"],
            &["before_surface_id"],
            anchor_reference_workspace,
        )
        .map(|location| (location, false))
    } else if after_specified {
        surface_location_from_keys(
            snapshot,
            params,
            &["after_surface_ref"],
            &["after_surface_id"],
            anchor_reference_workspace,
        )
        .map(|location| (location, true))
    } else {
        None
    };

    let (target_workspace_index, _target_pane_index, target_pane_id, destination_index) =
        if before_specified || after_specified {
            let ((workspace_index, anchor_id), after_anchor) =
                anchor.ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            let workspace = &window.tab_manager.workspaces[workspace_index];
            let (pane_index, pane_id, anchor_index) =
                surface_pane_details(workspace, &anchor_id)
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            (
                workspace_index,
                pane_index,
                pane_id.ok_or(SurfaceMoveResolveError::DestinationNotFound)?,
                Some(anchor_index as i64 + i64::from(after_anchor)),
            )
        } else if params.contains_key("pane_ref") || params.contains_key("pane_id") {
            if let Some(pane_id) = string_param(params, &["pane_id"]) {
                let (workspace_index, pane_index, pane_id) =
                    pane_location_by_id(snapshot, &pane_id)
                        .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                (
                    workspace_index,
                    pane_index,
                    pane_id,
                    i64_param(params, &["index"]),
                )
            } else {
                let reference = string_param(params, &["pane_ref"])
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                let pane_index = one_based_ref_index(&reference, "pane")
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                let workspace_index = requested_workspace_index.unwrap_or(source_workspace_index);
                let workspace = window
                    .tab_manager
                    .workspaces
                    .get(workspace_index)
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                let (pane_index, pane_id) = pane_at_index(workspace, pane_index)
                    .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
                (
                    workspace_index,
                    pane_index,
                    pane_id,
                    i64_param(params, &["index"]),
                )
            }
        } else if has_workspace {
            let workspace_index =
                requested_workspace_index.ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            let workspace = &window.tab_manager.workspaces[workspace_index];
            let (pane_index, pane_id) = first_or_focused_pane(workspace)
                .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            (
                workspace_index,
                pane_index,
                pane_id,
                i64_param(params, &["index"]),
            )
        } else if params.contains_key("window_ref") || params.contains_key("window_id") {
            if !workspace_reorder_window_matches(snapshot, params) {
                return Err(SurfaceMoveResolveError::DestinationNotFound);
            }
            let workspace = &window.tab_manager.workspaces[selected_workspace_index];
            let (pane_index, pane_id) = first_or_focused_pane(workspace)
                .ok_or(SurfaceMoveResolveError::DestinationNotFound)?;
            (
                selected_workspace_index,
                pane_index,
                pane_id,
                i64_param(params, &["index"]),
            )
        } else {
            (
                source_workspace_index,
                source_pane_index,
                source_pane_id,
                i64_param(params, &["index"]),
            )
        };

    Ok(SurfaceMoveResolution {
        source_workspace_index,
        panel_id,
        target_workspace_index,
        target_pane_id,
        destination_index,
        focus: bool_param(params, &["focus"]).unwrap_or(false),
    })
}

pub(super) fn surface_move(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let resolution = match resolve_surface_move(&current, params) {
        Ok(resolution) => resolution,
        Err(SurfaceMoveResolveError::ConflictingAnchors) => {
            return invalid_params("Specify at most one of before_surface_id or after_surface_id");
        }
        Err(SurfaceMoveResolveError::SourceNotFound) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Surface not found".to_string(),
                data: None,
            };
        }
        Err(SurfaceMoveResolveError::DestinationNotFound) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Destination pane not found".to_string(),
                data: None,
            };
        }
    };
    let state = app.state::<SessionState>();
    let result = match move_surface_for_control(
        app,
        &state,
        resolution.source_workspace_index,
        &resolution.panel_id,
        resolution.target_workspace_index,
        &resolution.target_pane_id,
        resolution.destination_index,
        resolution.focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(SurfacePositionControlError::InvalidRequest)) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: "Failed to move surface".to_string(),
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
    let window = &result.windows[0];
    let workspace = &window.tab_manager.workspaces[resolution.target_workspace_index];
    let Some((pane_index, pane_id, _)) = surface_pane_details(workspace, &resolution.panel_id)
    else {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: "Moved surface unavailable".to_string(),
            data: None,
        };
    };
    let surface_ref_value = surfaces_for_workspace(workspace)
        .iter()
        .position(|surface| {
            surface.get("id").and_then(Value::as_str) == Some(resolution.panel_id.as_str())
        })
        .map(surface_ref);
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(resolution.target_workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
        "surface_id": resolution.panel_id,
        "surface_ref": surface_ref_value,
    }))
}

pub(super) fn surface_reorder(
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
    let Some((workspace_index, panel_id)) = surface_reorder_source(&current, params) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Surface not found".to_string(),
            data: None,
        };
    };
    let workspace = &current.windows[0].tab_manager.workspaces[workspace_index];
    let Some((pane_index, pane_id, _source_index)) = surface_pane_details(workspace, &panel_id)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Surface not found".to_string(),
            data: None,
        };
    };
    let index = i64_param(params, &["index"]);
    let before = surface_id_from_selector_keys(
        workspace,
        params,
        &["before_surface_ref"],
        &["before_surface_id"],
    );
    let after = surface_id_from_selector_keys(
        workspace,
        params,
        &["after_surface_ref"],
        &["after_surface_id"],
    );
    let target_count =
        usize::from(index.is_some()) + usize::from(before.is_some()) + usize::from(after.is_some());
    if target_count != 1 {
        return invalid_params(
            "Specify exactly one of index, before_surface_id, or after_surface_id",
        );
    }
    let destination_index = if let Some(index) = index {
        index
    } else {
        let (anchor, after_anchor) = match (before.as_deref(), after.as_deref()) {
            (Some(anchor), None) => (anchor, false),
            (None, Some(anchor)) => (anchor, true),
            _ => {
                return invalid_params(
                    "Specify exactly one of index, before_surface_id, or after_surface_id",
                );
            }
        };
        let Some((anchor_pane_index, _, anchor_index)) = surface_pane_details(workspace, anchor)
        else {
            return invalid_params("Anchor surface must be in the same pane");
        };
        if anchor_pane_index != pane_index {
            return invalid_params("Anchor surface must be in the same pane");
        }
        anchor_index as i64 + i64::from(after_anchor)
    };
    let focus = bool_param(params, &["focus"]).unwrap_or(false);
    let state = app.state::<SessionState>();
    let result = match reorder_surface_for_control(
        app,
        &state,
        workspace_index,
        &panel_id,
        destination_index,
        focus,
    ) {
        Ok(result) => result,
        Err(PaneTopologyControlError::Operation(SurfacePositionControlError::InvalidRequest)) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: "Failed to reorder surface".to_string(),
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
    let window = &result.windows[0];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let surface_ref_value = surfaces_for_workspace(workspace)
        .iter()
        .position(|surface| surface.get("id").and_then(Value::as_str) == Some(panel_id.as_str()))
        .map(surface_ref);
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_ref(pane_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_value,
    }))
}

pub(super) fn surface_reorder_source(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<(usize, String)> {
    let window = snapshot.windows.first()?;
    let has_workspace_scope =
        params.contains_key("workspace_id") || params.contains_key("workspace_ref");
    if has_workspace_scope || params.contains_key("surface_ref") {
        let workspace_index = workspace_index_from_workspace_scope_or_selected(snapshot, params)?;
        let workspace = window.tab_manager.workspaces.get(workspace_index)?;
        return surface_id_from_selector_keys(
            workspace,
            params,
            &["surface_ref"],
            &["surface_id", "panel_id"],
        )
        .map(|panel_id| (workspace_index, panel_id));
    }
    let surface_id = string_param(params, &["surface_id", "panel_id"])?;
    window
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .find(|(_, workspace)| {
            surfaces_for_workspace(workspace).iter().any(|surface| {
                surface.get("id").and_then(Value::as_str) == Some(surface_id.as_str())
            })
        })
        .map(|(index, _)| (index, surface_id))
}

pub(super) fn surface_id_from_selector_keys(
    workspace: &SessionWorkspaceSnapshot,
    params: &serde_json::Map<String, Value>,
    ref_keys: &[&str],
    id_keys: &[&str],
) -> Option<String> {
    let surfaces = surfaces_for_workspace(workspace);
    if let Some(reference) = string_param(params, ref_keys) {
        let index = one_based_ref_index(&reference, "surface")?;
        return surfaces
            .get(index)
            .and_then(|surface| surface.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string);
    }
    let id = string_param(params, id_keys)?;
    surfaces
        .iter()
        .any(|surface| surface.get("id").and_then(Value::as_str) == Some(id.as_str()))
        .then_some(id)
}

pub(super) fn surface_pane_details(
    workspace: &SessionWorkspaceSnapshot,
    panel_id: &str,
) -> Option<(usize, Option<String>, usize)> {
    fn visit(
        layout: &SessionWorkspaceLayoutSnapshot,
        panel_id: &str,
        pane_index: &mut usize,
    ) -> Option<(usize, Option<String>, usize)> {
        match layout {
            SessionWorkspaceLayoutSnapshot::Pane(pane) => {
                let current = *pane_index;
                *pane_index += 1;
                pane.panel_ids
                    .iter()
                    .position(|id| id == panel_id)
                    .map(|index| (current, pane.pane_id.clone(), index))
            }
            SessionWorkspaceLayoutSnapshot::Split(split) => {
                visit(&split.first, panel_id, pane_index)
                    .or_else(|| visit(&split.second, panel_id, pane_index))
            }
        }
    }
    let mut pane_index = 0;
    visit(workspace.layout.as_ref()?, panel_id, &mut pane_index)
}

pub(super) fn surface_report_ports(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(ports) = ports_param(params) else {
        return invalid_params("Missing or invalid listening ports");
    };
    surface_set_ports(app, params, &ports)
}

pub(super) fn surface_report_tty(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(tty) = string_param(params, &["tty", "tty_name", "ttyName", "name"]) else {
        return invalid_params("Missing TTY name");
    };
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_tty_for_control(app, &state, workspace_index, &panel_id, &tty) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_report_shell_state(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(shell_activity) = shell_activity_param(params) else {
        return invalid_params("state must be prompt, running, or unknown");
    };
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_shell_activity_for_control(
        app,
        &state,
        workspace_index,
        &panel_id,
        shell_activity,
    ) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_clear_ports(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    surface_set_ports(app, params, &[])
}

pub(super) fn surface_set_ports(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    ports: &[u16],
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_params_or_selected(&current, params) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match set_panel_listening_ports_for_control(app, &state, workspace_index, &panel_id, ports) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_ports_kick(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let snapshot = snapshot(app);
    let (workspace_index, panel_id) = match surface_ports_kick_target(&snapshot, params) {
        Ok(target) => target,
        Err(message) => return invalid_params(message),
    };
    let terminal_state = app.state::<TerminalState>();
    let session_state = app.state::<SessionState>();
    let scan = scan_panel_listening_ports(
        app,
        terminal_state.inner(),
        session_state.inner(),
        &panel_id,
    );
    let (scanner, ports, error) = match scan {
        Ok(result) => ("pid-tree", result.ports, None),
        Err(error) => ("unavailable", Vec::new(), Some(error)),
    };
    let agent_refresh = refresh_workspace_agent_ports(app, &snapshot, workspace_index);
    let (agent_ports, agent_error) = match agent_refresh {
        Ok(snapshot) => {
            let ports = snapshot
                .windows
                .first()
                .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
                .and_then(|workspace| workspace.agent_listening_ports.clone())
                .unwrap_or_default();
            (ports, None)
        }
        Err(error) => (Vec::new(), Some(error)),
    };
    ok(json!({
        "accepted": true,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "reason": string_param(params, &["reason"]).unwrap_or_else(|| "command".to_string()),
        "scanner": scanner,
        "listening_ports": ports,
        "agent_listening_ports": agent_ports,
        "error": error,
        "agent_error": agent_error,
    }))
}

pub(super) fn refresh_workspace_agent_ports(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
) -> Result<AppSessionSnapshot, String> {
    let workspace = snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
        .ok_or_else(|| "workspace not found".to_string())?;
    let root_pids: Vec<u32> = workspace
        .agent_pids
        .as_ref()
        .into_iter()
        .flat_map(|entries| entries.iter())
        .map(|entry| entry.pid)
        .collect();
    let mut ports = Vec::new();
    for root_pid in root_pids {
        ports.extend(scan_listening_ports_for_root_pid(root_pid)?);
    }
    ports.sort_unstable();
    ports.dedup();
    let state = app.state::<SessionState>();
    set_workspace_agent_listening_ports_for_control(app, &state, workspace_index, &ports)
}

pub(super) fn surface_ports_kick_target(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<(usize, String), &'static str> {
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(snapshot, params)
    else {
        return Err("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(snapshot, workspace_index, params)
    else {
        return Err("Missing or invalid surface selector");
    };
    Ok((workspace_index, panel_id))
}

pub(super) fn surface_focus(
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
    let Some(workspace_id) = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
        .and_then(|workspace| workspace.workspace_id.clone())
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let state = app.state::<SessionState>();
    let (changed, _snapshot) = match select_workspace_surface(app, &state, &workspace_id, &panel_id)
    {
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
    ok(json!({
        "accepted": true,
        "changed": changed,
        "workspace_id": workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
    }))
}

pub(super) fn surface_health(
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
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let surfaces = surfaces_for_workspace(workspace)
        .into_iter()
        .map(|surface| {
            let mut surface = surface.as_object().cloned().unwrap_or_default();
            surface.insert("in_window".to_string(), json!(true));
            surface.insert("healthy".to_string(), json!(true));
            Value::Object(surface)
        })
        .collect::<Vec<_>>();
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surfaces": surfaces,
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

pub(super) fn terminal_workspace_index(
    current: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<usize, ControlCallResult> {
    if current.windows.is_empty() {
        return Err(ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        });
    }
    workspace_index_from_workspace_scope_or_selected(current, params).ok_or_else(|| {
        ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        }
    })
}

pub(super) fn terminal_panel_id(
    current: &AppSessionSnapshot,
    workspace_index: usize,
    params: &serde_json::Map<String, Value>,
) -> Result<String, ControlCallResult> {
    let panel_id = surface_id_from_params_or_workspace_focused(current, workspace_index, params)
        .ok_or_else(|| ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No focused surface".to_string(),
            data: None,
        })?;
    if !surface_is_terminal(current, workspace_index, &panel_id) {
        return Err(ControlCallResult::Err {
            code: "invalid_params".to_string(),
            message: "Surface is not a terminal".to_string(),
            data: Some(
                json!({"surface_id": panel_id})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        });
    }
    Ok(panel_id)
}

pub(super) fn surface_read_text(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let workspace_index = match terminal_workspace_index(&current, params) {
        Ok(index) => index,
        Err(error) => return error,
    };
    let line_limit = if params.contains_key("lines") {
        match usize_param(params, &["lines"]) {
            Some(lines) if lines > 0 => Some(lines),
            _ => return invalid_params("lines must be greater than 0"),
        }
    } else {
        None
    };
    let panel_id = match terminal_panel_id(&current, workspace_index, params) {
        Ok(panel_id) => panel_id,
        Err(error) => return error,
    };
    let window = &current.windows[0];
    let include_scrollback =
        bool_param(params, &["scrollback"]).unwrap_or(false) || line_limit.is_some();
    let terminal_state = app.state::<TerminalState>();
    let text = match terminal_read_panel(
        terminal_state.inner(),
        &panel_id,
        include_scrollback,
        line_limit,
    ) {
        Ok(text) => text,
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: Some(
                    json!({"surface_id": panel_id.clone()})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    ok(json!({
        "text": text,
        "base64": BASE64_STANDARD.encode(text.as_bytes()),
        "workspace_id": window.tab_manager.workspaces[workspace_index].workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

pub(super) fn surface_clear_history(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let workspace_index = match terminal_workspace_index(&current, params) {
        Ok(index) => index,
        Err(error) => return error,
    };
    let panel_id = match terminal_panel_id(&current, workspace_index, params) {
        Ok(panel_id) => panel_id,
        Err(error) => return error,
    };
    let window = &current.windows[0];
    let terminal_state = app.state::<TerminalState>();
    if let Err(message) = terminal_clear_history_panel(terminal_state.inner(), &panel_id) {
        return ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: Some(
                json!({"surface_id": panel_id.clone()})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }
    ok(json!({
        "workspace_id": window.tab_manager.workspaces[workspace_index].workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

pub(super) fn surface_trigger_flash(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(window) = current.windows.first() else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let Some(panel_id) =
        surface_id_from_params_or_workspace_focused(&current, workspace_index, params)
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let payload = json!({"panelId": panel_id.clone()});
    if let Err(error) = app.emit(PANEL_FLASH_EVENT, payload) {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Failed to emit panel flash event: {error}"),
            data: None,
        };
    }
    ok(json!({
        "workspace_id": window.tab_manager.workspaces[workspace_index].workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

pub(super) fn surface_refresh_all(app: &AppHandle) -> ControlCallResult {
    if let Err(error) = app.emit(SURFACE_REFRESH_EVENT, json!({"refresh": true})) {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Failed to emit surface refresh event: {error}"),
            data: None,
        };
    }
    ok(json!({
        "accepted": true,
        "event": SURFACE_REFRESH_EVENT,
    }))
}

pub(super) fn surface_send_text(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(text) = raw_string_param(params, &["text"]) else {
        return invalid_params("Missing text");
    };
    surface_send_input(app, params, &text)
}

pub(super) fn surface_send_key(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(key) = string_param(params, &["key"]) else {
        return invalid_params("Missing key");
    };
    let Some(sequence) = terminal_key_sequence(&key) else {
        return ControlCallResult::Err {
            code: "invalid_params".to_string(),
            message: "Unknown key".to_string(),
            data: Some(json!({"key": key}).try_into().unwrap_or(JsonValue::Null)),
        };
    };
    surface_send_input(app, params, sequence)
}

pub(super) fn surface_send_input(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    data: &str,
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
    if !surface_is_terminal(&current, workspace_index, &panel_id) {
        return ControlCallResult::Err {
            code: "invalid_params".to_string(),
            message: "Surface is not a terminal".to_string(),
            data: Some(
                json!({"surface_id": panel_id.clone()})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }
    let terminal_state = app.state::<TerminalState>();
    if let Err(message) = terminal_write_panel(terminal_state.inner(), &panel_id, data) {
        return ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: Some(
                json!({"surface_id": panel_id.clone()})
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }
    ok(json!({
        "workspace_id": current.windows[0].tab_manager.workspaces[workspace_index].workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "surface_id": panel_id,
        "surface_ref": surface_ref_for_panel(&current, workspace_index, &panel_id),
        "queued": false,
        "window_id": current.windows[0].window_id,
        "window_ref": current.windows[0].window_id.as_ref().map(|_| "window:1"),
    }))
}

pub(super) fn surface_move_to_new_workspace(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match move_panel_to_new_workspace_for_control(app, &state, &panel_id) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_open_browser(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let url = string_param(params, &["url"]);
    let state = app.state::<SessionState>();
    match open_browser_url_in_panel(app, &state, &panel_id, url.as_deref()) {
        Ok(Some(snapshot)) => surface_list_from_params(&snapshot, params),
        Ok(None) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to open browser in pane {panel_id}"),
            data: None,
        },
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}
