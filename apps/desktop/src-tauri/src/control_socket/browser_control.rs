use super::*;

pub(super) fn browser_open_split(
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
    let Some(orientation) = split_orientation_from_params(params) else {
        return invalid_params("Invalid split orientation");
    };
    let Some(workspace) = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let before_surface_ids = surfaces_for_workspace(workspace)
        .iter()
        .filter_map(|surface| surface.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
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
        Ok(snapshot) => {
            let Some(new_panel_id) =
                new_browser_surface_id(&snapshot, workspace_index, &before_surface_ids)
            else {
                return surface_list_from_params(&snapshot, params);
            };
            match browser_surface_payload(&snapshot, workspace_index, &new_panel_id) {
                Some(payload) => ok(payload),
                None => surface_list_from_params(&snapshot, params),
            }
        }
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

pub(super) fn browser_navigate(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(url) = raw_string_param(params, &["url"]) else {
        return invalid_params("Missing browser URL");
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
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.navigate requires a browser surface");
    }
    let state = app.state::<SessionState>();
    match open_browser_url_in_panel(app, &state, &panel_id, Some(&url)) {
        Ok(Some(snapshot)) => {
            match browser_surface_payload(&snapshot, workspace_index, &panel_id) {
                Some(payload) => ok(payload),
                None => invalid_params("Missing or invalid surface selector"),
            }
        }
        Ok(None) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to navigate browser surface {panel_id}"),
            data: None,
        },
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_open_markdown(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(file_path) = string_param(params, &["file_path", "path"]) else {
        return invalid_params("Missing markdown file path");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match open_markdown_file_in_panel(app, &state, &panel_id, &file_path) {
        Ok(Some(snapshot)) => surface_list_from_params(&snapshot, params),
        Ok(None) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to open markdown file in pane {panel_id}"),
            data: None,
        },
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_open_file(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(file_path) = string_param(params, &["file_path", "path"]) else {
        return invalid_params("Missing file path");
    };
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match open_file_in_panel(app, &state, &panel_id, &file_path) {
        Ok(Some(snapshot)) => surface_list_from_params(&snapshot, params),
        Ok(None) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to open file in pane {panel_id}"),
            data: None,
        },
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn surface_open_diff(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let diff_state = app.state::<DiffState>();
    let (token, request_path, created_token) = match string_param(params, &["token", "diff_token"])
    {
        Some(token) => (
            token,
            string_param(params, &["request_path", "path"])
                .unwrap_or_else(|| "/index.html".to_string()),
            None,
        ),
        None => match diff_state.create_starter_session(SystemTime::now()) {
            Ok(created) => {
                let created_token = created.token.clone();
                (created.token, created.request_path, Some(created_token))
            }
            Err(message) => {
                return ControlCallResult::Err {
                    code: "internal_error".to_string(),
                    message,
                    data: None,
                }
            }
        },
    };
    let state = app.state::<SessionState>();
    match open_diff_viewer_in_panel(app, &state, &diff_state, &panel_id, &token, &request_path) {
        Ok(Some(snapshot)) => surface_list_from_params(&snapshot, params),
        Ok(None) => {
            if let Some(token) = created_token.as_deref() {
                diff_state.unregister_starter_session(token);
            }
            ControlCallResult::Err {
                code: "not_found".to_string(),
                message: format!("unable to open diff viewer in pane {panel_id}"),
                data: None,
            }
        }
        Err(message) => {
            if let Some(token) = created_token.as_deref() {
                diff_state.unregister_starter_session(token);
            }
            ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            }
        }
    }
}

pub(super) fn surface_select_adjacent(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    next: bool,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    let snapshot = match select_adjacent_panel_for_control(app, &state, &panel_id, next) {
        Ok(snapshot) => snapshot,
        Err(PaneTopologyControlError::Operation(error)) => match error {},
        Err(PaneTopologyControlError::Publication(message)) => {
            return ControlCallResult::Err {
                code: "internal".to_string(),
                message,
                data: None,
            };
        }
    };
    surface_list_from_params(&snapshot, params)
}

pub(super) fn surface_toggle_split_zoom(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match toggle_split_zoom_for_control(app, &state, &panel_id) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_back(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match browser_go_back_for_control(app, &state, &panel_id) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_forward(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match browser_go_forward_for_control(app, &state, &panel_id) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_reload(
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
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.reload requires a browser surface");
    }
    let state = app.state::<BrowserWebviewState>();
    let reply = match browser_webview_command_for_control(state.inner(), &panel_id, "reload") {
        Ok(reply) => reply,
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    let Some(mut payload) = browser_surface_payload(&current, workspace_index, &panel_id)
        .and_then(|value| value.as_object().cloned())
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    payload.insert("reloaded".to_string(), json!(true));
    payload.insert("webview_attached".to_string(), json!(reply.attached));
    ok(Value::Object(payload))
}

pub(super) fn browser_url_get(
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
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.url.get requires a browser surface");
    }
    match browser_surface_payload(&current, workspace_index, &panel_id) {
        Some(payload) => ok(payload),
        None => invalid_params("Missing or invalid surface selector"),
    }
}

pub(super) fn browser_focus_webview(
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
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.focus_webview requires a browser surface");
    }
    let Some(workspace_id) = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
        .and_then(|workspace| workspace.workspace_id.clone())
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let session_state = app.state::<SessionState>();
    let (_changed, snapshot) =
        match select_workspace_surface(app, &session_state, &workspace_id, &panel_id) {
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
    let browser_state = app.state::<BrowserWebviewState>();
    let reply = match browser_webview_command_for_control(browser_state.inner(), &panel_id, "focus")
    {
        Ok(reply) => reply,
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    let Some(mut payload) = browser_surface_payload(&snapshot, workspace_index, &panel_id)
        .and_then(|value| value.as_object().cloned())
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    payload.insert("focused".to_string(), json!(true));
    payload.insert("webview_attached".to_string(), json!(reply.attached));
    payload.insert("focus_scope".to_string(), json!("webview"));
    ok(Value::Object(payload))
}

pub(super) fn browser_is_webview_focused(
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
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.is_webview_focused requires a browser surface");
    }
    let Some(mut payload) = browser_surface_payload(&current, workspace_index, &panel_id)
        .and_then(|value| value.as_object().cloned())
    else {
        return invalid_params("Missing or invalid surface selector");
    };
    let focused = payload
        .get("surface")
        .and_then(Value::as_object)
        .and_then(|surface| surface.get("focused"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    payload.insert("focused".to_string(), json!(focused));
    payload.insert("focus_scope".to_string(), json!("surface"));
    payload.insert("webview_focus_verified".to_string(), json!(false));
    ok(Value::Object(payload))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserGetter {
    Text,
    Html,
    Value,
    Attr,
    Title,
    Count,
    Box,
    Styles,
    Visible,
    Enabled,
    Checked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserAction {
    Click,
    DblClick,
    Hover,
    Focus,
    Type,
    Fill,
    Press,
    KeyDown,
    KeyUp,
    Check,
    Uncheck,
    Select,
    Scroll,
    ScrollIntoView,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserLocator {
    Role,
    Text,
    Label,
    Placeholder,
    Alt,
    Title,
    TestId,
    First,
    Last,
    Nth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserCookieAction {
    Get,
    Set,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserStorageAction {
    Get,
    Set,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserConsoleAction {
    List,
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BrowserDialogAction {
    Accept,
    Dismiss,
}

pub(super) fn browser_snapshot(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.snapshot") else {
        return invalid_params("browser.snapshot requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, browser_snapshot_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => {
                let snapshot = value
                    .get("snapshot")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let refs = value.get("refs").cloned().unwrap_or_else(|| json!({}));
                ok(json!({
                    "surface_id": panel_id,
                    "panel_id": panel_id,
                    "snapshot": snapshot,
                    "refs": refs,
                }))
            }
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_screenshot(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.screenshot") else {
        return invalid_params("browser.screenshot requires a browser surface");
    };
    let value = match run_browser_eval_script(app, &panel_id, browser_screenshot_probe_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => value,
            Err(message) => {
                return ControlCallResult::Err {
                    code: "javascript_error".to_string(),
                    message,
                    data: None,
                }
            }
        },
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    let width = value
        .get("width")
        .and_then(Value::as_u64)
        .unwrap_or(1024)
        .clamp(1, 2048) as u32;
    let height = value
        .get("height")
        .and_then(Value::as_u64)
        .unwrap_or(768)
        .clamp(1, 2048) as u32;
    let rgb = [
        value
            .get("r")
            .and_then(Value::as_u64)
            .unwrap_or(255)
            .min(255) as u8,
        value
            .get("g")
            .and_then(Value::as_u64)
            .unwrap_or(255)
            .min(255) as u8,
        value
            .get("b")
            .and_then(Value::as_u64)
            .unwrap_or(255)
            .min(255) as u8,
    ];
    let png = solid_png(width, height, rgb);
    let png_base64 = BASE64_STANDARD.encode(&png);
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    payload.insert("png_base64".to_string(), json!(png_base64));
    payload.insert("mime".to_string(), json!("image/png"));
    payload.insert("width".to_string(), json!(width));
    payload.insert("height".to_string(), json!(height));
    payload.insert("captureMode".to_string(), json!("dom-background-raster"));
    if let Some(path) = raw_string_param(params, &["path", "out", "file_path", "filePath"]) {
        if let Err(error) = fs::write(&path, &png) {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Could not write browser screenshot to {path}: {error}"),
                data: None,
            };
        }
        payload.insert("path".to_string(), json!(path));
    }
    ok(Value::Object(payload))
}

pub(super) fn browser_eval(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(script) = raw_string_param(params, &["script", "expression"]) else {
        return invalid_params("browser.eval requires a script");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.eval") else {
        return invalid_params("browser.eval requires a browser surface");
    };
    let wrapped_script = browser_eval_wrapper_script(&script);
    match run_browser_eval_script(app, &panel_id, &wrapped_script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "value": value,
            })),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_add_init_script(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(script) = raw_string_param(params, &["script"]) else {
        return invalid_params("browser.addinitscript requires a script");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.addinitscript") else {
        return invalid_params("browser.addinitscript requires a browser surface");
    };
    let browser_state = app.state::<BrowserWebviewState>();
    match browser_add_init_script_for_control(app, browser_state.inner(), &panel_id, &script) {
        Ok(reply) => ok(json!({
            "surface_id": panel_id,
            "panel_id": panel_id,
            "added": true,
            "webview": reply,
        })),
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_wait(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.wait") else {
        return invalid_params("browser.wait requires a browser surface");
    };
    let timeout_ms = usize_param(params, &["timeout_ms", "timeoutMs"])
        .unwrap_or(5_000)
        .clamp(1, 60_000) as u64;
    let condition = if let Some(selector) = raw_string_param(params, &["selector"]) {
        browser_wait_selector_script(&selector)
    } else if let Some(text) = raw_string_param(params, &["text_contains", "textContains", "text"])
    {
        browser_wait_text_script(&text)
    } else if let Some(expression) = raw_string_param(params, &["function", "expression"]) {
        browser_wait_function_script(&expression)
    } else if let Some(load_state) = string_param(params, &["load_state", "loadState"]) {
        browser_wait_load_state_script(&load_state)
    } else if let Some(url_contains) = raw_string_param(params, &["url_contains", "urlContains"]) {
        browser_wait_url_script(&url_contains)
    } else {
        return invalid_params(
            "browser.wait requires selector, text_contains, function, load_state, or url_contains",
        );
    };
    let deadline = SystemTime::now() + Duration::from_millis(timeout_ms);
    let mut last_error = String::new();
    while SystemTime::now() < deadline {
        match run_browser_eval_script(app, &panel_id, &condition) {
            Ok(value) => match unwrap_browser_eval_result(value) {
                Ok(value) if value.as_bool().unwrap_or(false) => {
                    return ok(json!({
                        "surface_id": panel_id,
                        "panel_id": panel_id,
                        "value": true,
                    }));
                }
                Ok(_) => {}
                Err(message) => last_error = message,
            },
            Err(message) => {
                return ControlCallResult::Err {
                    code: "surface_unavailable".to_string(),
                    message,
                    data: None,
                }
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    ControlCallResult::Err {
        code: "timeout".to_string(),
        message: if last_error.is_empty() {
            format!("browser.wait timed out after {timeout_ms}ms")
        } else {
            format!("browser.wait timed out after {timeout_ms}ms: {last_error}")
        },
        data: None,
    }
}

pub(super) fn browser_action(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_action_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_action_method(action)
        ));
    };
    let selector = raw_string_param(params, &["selector"]);
    if browser_action_requires_selector(action) && selector.as_deref().is_none_or(str::is_empty) {
        return invalid_params(&format!(
            "{} requires a selector",
            browser_action_method(action)
        ));
    }
    let text = raw_string_param(params, &["text", "value"]).unwrap_or_default();
    let value = raw_string_param(params, &["value"]).unwrap_or_default();
    let key = raw_string_param(params, &["key"]).unwrap_or_default();
    let dx = f64_param(params, &["dx"]).unwrap_or(0.0);
    let dy = f64_param(params, &["dy"]).unwrap_or(0.0);
    let script = browser_action_script(action, selector.as_deref(), &text, &value, &key, dx, dy);
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => {
                let mut payload = serde_json::Map::new();
                payload.insert("surface_id".to_string(), json!(panel_id));
                payload.insert("panel_id".to_string(), json!(panel_id));
                payload.insert("value".to_string(), value);
                if bool_param(params, &["snapshot_after", "snapshotAfter"]).unwrap_or(false) {
                    if let Ok(snapshot) =
                        run_browser_eval_script(app, &panel_id, browser_snapshot_script())
                            .and_then(unwrap_browser_eval_result)
                    {
                        payload.insert("post_action_snapshot".to_string(), snapshot);
                    }
                }
                ok(Value::Object(payload))
            }
            Err(message) => ControlCallResult::Err {
                code: if message.contains("No element matches selector") {
                    "not_found".to_string()
                } else {
                    "javascript_error".to_string()
                },
                message: browser_not_found_message(&message),
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_find(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    locator: BrowserLocator,
) -> ControlCallResult {
    let panel_id = if locator == BrowserLocator::Nth {
        let mut surface_params = params.clone();
        surface_params.remove("index");
        browser_automation_panel_id(app, &surface_params, browser_locator_method(locator))
    } else {
        browser_automation_panel_id(app, params, browser_locator_method(locator))
    };
    let Some(panel_id) = panel_id else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_locator_method(locator)
        ));
    };
    let script = match browser_locator_script(locator, params) {
        Ok(script) => script,
        Err(message) => return invalid_params(&message),
    };
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_locator_payload(&panel_id, value)),
            Err(message) => ControlCallResult::Err {
                code: "not_found".to_string(),
                message: browser_not_found_message(&message),
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_frame_select(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(selector) = raw_string_param(params, &["selector"]) else {
        return invalid_params("browser.frame.select requires a selector");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.frame.select") else {
        return invalid_params("browser.frame.select requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_frame_select_script(&selector)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "selected": true,
                "frame": value,
            })),
            Err(message) => ControlCallResult::Err {
                code: "not_found".to_string(),
                message: browser_not_found_message(&message),
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_frame_main(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.frame.main") else {
        return invalid_params("browser.frame.main requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, browser_frame_main_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(_) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "selected": false,
            })),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_dialog(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserDialogAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_dialog_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_dialog_method(action)
        ));
    };
    let text = raw_string_param(params, &["text"]);
    match run_browser_eval_script(
        app,
        &panel_id,
        &browser_dialog_script(action, text.as_deref()),
    ) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_dialog_payload(&panel_id, action, value)),
            Err(message) => ControlCallResult::Err {
                code: "not_found".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_download_wait(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(path) = raw_string_param(params, &["path", "file_path", "filePath"]) else {
        return invalid_params("browser.download.wait requires a path");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.download.wait") else {
        return invalid_params("browser.download.wait requires a browser surface");
    };
    let timeout_ms = usize_param(params, &["timeout_ms", "timeoutMs"])
        .unwrap_or(30_000)
        .clamp(1, 300_000) as u64;
    let deadline = SystemTime::now() + Duration::from_millis(timeout_ms);
    while SystemTime::now() < deadline {
        if fs::metadata(&path).is_ok_and(|metadata| metadata.is_file()) {
            return ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "path": path,
                "downloaded": true,
            }));
        }
        thread::sleep(Duration::from_millis(100));
    }
    ControlCallResult::Err {
        code: "timeout".to_string(),
        message: format!("browser.download.wait timed out after {timeout_ms}ms for {path}"),
        data: None,
    }
}

pub(super) fn browser_addscript(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(script) = raw_string_param(params, &["script"]) else {
        return invalid_params("browser.addscript requires a script");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.addscript") else {
        return invalid_params("browser.addscript requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_eval_wrapper_script(&script)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "value": value,
            })),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_addstyle(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(css) = raw_string_param(params, &["css", "style"]) else {
        return invalid_params("browser.addstyle requires css");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.addstyle") else {
        return invalid_params("browser.addstyle requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_addstyle_script(&css)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "value": value,
                "added": value.as_bool().unwrap_or(true),
            })),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_cookies(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserCookieAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_cookie_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_cookie_method(action)
        ));
    };
    let name = string_param(params, &["name"]);
    let value = raw_string_param(params, &["value"]);
    if matches!(
        action,
        BrowserCookieAction::Set | BrowserCookieAction::Clear
    ) && name.is_none()
    {
        return invalid_params(&format!(
            "{} requires a cookie name",
            browser_cookie_method(action)
        ));
    }
    if action == BrowserCookieAction::Set && value.is_none() {
        return invalid_params("browser.cookies.set requires a cookie value");
    }
    let script = browser_cookie_script(action, name.as_deref(), value.as_deref());
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_cookie_payload(&panel_id, action, value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_storage(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserStorageAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_storage_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_storage_method(action)
        ));
    };
    let storage_type = string_param(params, &["type", "storage"])
        .unwrap_or_else(|| "local".to_string())
        .to_ascii_lowercase();
    if !matches!(storage_type.as_str(), "local" | "session") {
        return invalid_params("browser.storage type must be local or session");
    }
    let key = raw_string_param(params, &["key"]);
    let value = params.get("value").cloned().unwrap_or(Value::Null);
    if matches!(
        action,
        BrowserStorageAction::Get | BrowserStorageAction::Set
    ) && key.is_none()
    {
        return invalid_params(&format!(
            "{} requires a key",
            browser_storage_method(action)
        ));
    }
    if action == BrowserStorageAction::Set && value.is_null() {
        return invalid_params("browser.storage.set requires a value");
    }
    let script = browser_storage_script(action, &storage_type, key.as_deref(), &value);
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_storage_payload(&panel_id, action, value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_tab_list(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(workspace_index) = workspace_index_from_workspace_scope_or_selected(&current, params)
    else {
        return invalid_params("Missing or invalid workspace selector");
    };
    let tabs = current
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
        .map(surfaces_for_workspace)
        .unwrap_or_default()
        .into_iter()
        .filter(|surface| {
            surface
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "browser")
        })
        .map(|surface| {
            json!({
                "id": surface.get("id").cloned().unwrap_or(Value::Null),
                "surface_id": surface.get("id").cloned().unwrap_or(Value::Null),
                "title": surface.get("title").cloned().unwrap_or(Value::Null),
                "url": surface.get("browser_url").cloned().unwrap_or(Value::Null),
                "focused": surface.get("focused").cloned().unwrap_or(json!(false)),
            })
        })
        .collect::<Vec<_>>();
    ok(json!({
        "tabs": tabs,
        "count": tabs.len(),
    }))
}

pub(super) fn browser_tab_new(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let mut open_params = params.clone();
    if !open_params.contains_key("url") {
        open_params.insert("url".to_string(), json!("about:blank"));
    }
    browser_open_split(app, &open_params)
}

pub(super) fn browser_tab_switch(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(target_surface_id) = string_param(
        params,
        &[
            "target_surface_id",
            "targetSurfaceId",
            "target",
            "tab_id",
            "tabId",
        ],
    ) else {
        return invalid_params("browser.tab.switch requires a target surface id");
    };
    let mut focus_params = params.clone();
    focus_params.insert("surface_id".to_string(), json!(target_surface_id));
    surface_focus(app, &focus_params)
}

pub(super) fn browser_tab_close(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(target_surface_id) = string_param(
        params,
        &[
            "target_surface_id",
            "targetSurfaceId",
            "target",
            "tab_id",
            "tabId",
        ],
    )
    .or_else(|| string_param(params, &["surface_id", "panel_id", "id"])) else {
        return invalid_params("browser.tab.close requires a target surface id");
    };
    let mut close_params = params.clone();
    close_params.insert("surface_id".to_string(), json!(target_surface_id));
    surface_close(app, &close_params)
}

pub(super) fn browser_console(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    action: BrowserConsoleAction,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_console_method(action))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_console_method(action)
        ));
    };
    let script = match action {
        BrowserConsoleAction::List => browser_console_list_script(),
        BrowserConsoleAction::Clear => browser_console_clear_script(),
    };
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_console_payload(&panel_id, action, value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_errors_list(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.errors.list") else {
        return invalid_params("browser.errors.list requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_errors_list_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_errors_payload(&panel_id, value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_state_save(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(path) = raw_string_param(params, &["path", "file_path", "filePath"]) else {
        return invalid_params("browser.state.save requires a path");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.state.save") else {
        return invalid_params("browser.state.save requires a browser surface");
    };
    let state = match run_browser_eval_script(app, &panel_id, browser_state_capture_script()) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => value,
            Err(message) => {
                return ControlCallResult::Err {
                    code: "javascript_error".to_string(),
                    message,
                    data: None,
                }
            }
        },
        Err(message) => {
            return ControlCallResult::Err {
                code: "surface_unavailable".to_string(),
                message,
                data: None,
            }
        }
    };
    let encoded = match serde_json::to_string_pretty(&state) {
        Ok(encoded) => encoded,
        Err(error) => {
            return ControlCallResult::Err {
                code: "internal_error".to_string(),
                message: format!("Could not serialize browser state: {error}"),
                data: None,
            }
        }
    };
    if let Err(error) = fs::write(&path, encoded) {
        return ControlCallResult::Err {
            code: "io_error".to_string(),
            message: format!("Could not write browser state to {path}: {error}"),
            data: None,
        };
    }
    ok(browser_state_payload(&panel_id, &path, "saved", state))
}

pub(super) fn browser_state_load(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(path) = raw_string_param(params, &["path", "file_path", "filePath"]) else {
        return invalid_params("browser.state.load requires a path");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.state.load") else {
        return invalid_params("browser.state.load requires a browser surface");
    };
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) => {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Could not read browser state from {path}: {error}"),
                data: None,
            }
        }
    };
    let state: Value = match serde_json::from_str(&raw) {
        Ok(state) => state,
        Err(error) => {
            return ControlCallResult::Err {
                code: "invalid_params".to_string(),
                message: format!("Browser state file is not valid JSON: {error}"),
                data: None,
            }
        }
    };
    match run_browser_eval_script(app, &panel_id, &browser_state_restore_script(&state)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_state_payload(&panel_id, &path, "loaded", value)),
            Err(message) => ControlCallResult::Err {
                code: "javascript_error".to_string(),
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_highlight(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(selector) = raw_string_param(params, &["selector"]) else {
        return invalid_params("browser.highlight requires a selector");
    };
    let Some(panel_id) = browser_automation_panel_id(app, params, "browser.highlight") else {
        return invalid_params("browser.highlight requires a browser surface");
    };
    match run_browser_eval_script(app, &panel_id, &browser_highlight_script(&selector)) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(json!({
                "surface_id": panel_id,
                "panel_id": panel_id,
                "highlighted": value.as_bool().unwrap_or(true),
                "value": value,
            })),
            Err(message) => ControlCallResult::Err {
                code: "not_found".to_string(),
                message: browser_not_found_message(&message),
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_get_title(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    browser_get_selector_value(app, params, BrowserGetter::Title)
}

pub(super) fn browser_get_selector_value(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    getter: BrowserGetter,
) -> ControlCallResult {
    let Some(panel_id) = browser_automation_panel_id(app, params, browser_getter_method(getter))
    else {
        return invalid_params(&format!(
            "{} requires a browser surface",
            browser_getter_method(getter)
        ));
    };
    let selector = raw_string_param(params, &["selector"]);
    if matches!(
        getter,
        BrowserGetter::Text
            | BrowserGetter::Html
            | BrowserGetter::Value
            | BrowserGetter::Attr
            | BrowserGetter::Box
            | BrowserGetter::Styles
            | BrowserGetter::Visible
            | BrowserGetter::Enabled
            | BrowserGetter::Checked
    ) && selector.as_deref().is_none_or(str::is_empty)
    {
        return invalid_params(&format!(
            "{} requires a selector",
            browser_getter_method(getter)
        ));
    }
    if getter == BrowserGetter::Count && selector.as_deref().is_none_or(str::is_empty) {
        return invalid_params("browser.get.count requires a selector");
    }
    let attr = if getter == BrowserGetter::Attr {
        match string_param(params, &["attribute", "attr", "name"]) {
            Some(attr) => Some(attr),
            None => return invalid_params("browser.get.attr requires an attribute name"),
        }
    } else if getter == BrowserGetter::Styles {
        string_param(params, &["property", "name"])
    } else {
        None
    };
    let script = browser_getter_script(getter, selector.as_deref(), attr.as_deref());
    match run_browser_eval_script(app, &panel_id, &script) {
        Ok(value) => match unwrap_browser_eval_result(value) {
            Ok(value) => ok(browser_getter_payload(&panel_id, getter, value)),
            Err(message) => ControlCallResult::Err {
                code: if message.contains("No element matches selector") {
                    "not_found".to_string()
                } else {
                    "javascript_error".to_string()
                },
                message,
                data: None,
            },
        },
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_automation_panel_id(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    _method: &str,
) -> Option<String> {
    let current = snapshot(app);
    let workspace_index = workspace_index_from_workspace_scope_or_selected(&current, params)?;
    let panel_id = surface_id_from_params_or_workspace_focused(&current, workspace_index, params)?;
    surface_is_browser(&current, workspace_index, &panel_id).then_some(panel_id)
}

pub(super) fn run_browser_eval_script(
    app: &AppHandle,
    panel_id: &str,
    script: &str,
) -> Result<Value, String> {
    let state = app.state::<BrowserWebviewState>();
    browser_eval_for_control(state.inner(), panel_id, script)
}

pub(super) fn browser_eval_wrapper_script(script: &str) -> String {
    let encoded = serde_json::to_string(script).expect("serializing JS source cannot fail");
    let dialog_hook =
        serde_json::to_string(browser_dialog_hook_script()).expect("dialog hook JSON");
    format!(
        r#"(() => {{
  {context}
  (0, eval)({dialog_hook});
  const __cmuxScript = {encoded};
  try {{
    const value = Function('window', 'document', `return (function() {{ return eval(${{JSON.stringify(__cmuxScript)}}); }}).call(window);`)(window, document);
    return {{ ok: true, value }};
  }} catch (error) {{
    return {{
      ok: false,
      error: String((error && (error.stack || error.message)) || error)
    }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

pub(super) fn browser_js_context_prelude() -> &'static str {
    r#"const __cmuxTopWindow = globalThis;
  const __cmuxFrameSelector = __cmuxTopWindow.__cmuxSelectedFrameSelector || '';
  const __cmuxFrameElement = __cmuxFrameSelector ? __cmuxTopWindow.document.querySelector(__cmuxFrameSelector) : null;
  const window = (__cmuxFrameElement && __cmuxFrameElement.contentWindow) || __cmuxTopWindow;
  const document = window.document;"#
}

pub(super) fn browser_dialog_hook_script() -> &'static str {
    r#"(() => {
  if (!globalThis.__cmuxDialogCapture) {
    const state = { queue: [] };
    const push = (type, message, defaultValue) => {
      state.queue.push({ type, message: String(message || ''), defaultValue: defaultValue == null ? null : String(defaultValue), timestamp: Date.now() });
    };
    globalThis.alert = (message) => { push('alert', message, null); };
    globalThis.confirm = (message) => { push('confirm', message, null); return true; };
    globalThis.prompt = (message, defaultValue) => { push('prompt', message, defaultValue); return defaultValue == null ? '' : String(defaultValue); };
    globalThis.__cmuxDialogCapture = state;
  }
  return globalThis.__cmuxDialogCapture;
})()"#
}

pub(super) fn browser_screenshot_probe_script() -> &'static str {
    r#"(() => {
  const __cmuxTopWindow = globalThis;
  const __cmuxFrameSelector = __cmuxTopWindow.__cmuxSelectedFrameSelector || '';
  const __cmuxFrameElement = __cmuxFrameSelector ? __cmuxTopWindow.document.querySelector(__cmuxFrameSelector) : null;
  const window = (__cmuxFrameElement && __cmuxFrameElement.contentWindow) || __cmuxTopWindow;
  const document = window.document;
  try {
    const parseColor = (value) => {
      const match = String(value || '').match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/i);
      if (!match) { return null; }
      return { r: Number(match[1]), g: Number(match[2]), b: Number(match[3]) };
    };
    let element = document.body || document.documentElement;
    let color = null;
    while (element && !color) {
      color = parseColor(window.getComputedStyle(element).backgroundColor);
      if (color && (color.r !== 0 || color.g !== 0 || color.b !== 0)) { break; }
      element = element.parentElement;
    }
    color = color || { r: 255, g: 255, b: 255 };
    return {
      ok: true,
      value: {
        width: Math.max(1, Math.round(window.innerWidth || document.documentElement.clientWidth || 1024)),
        height: Math.max(1, Math.round(window.innerHeight || document.documentElement.clientHeight || 768)),
        r: color.r,
        g: color.g,
        b: color.b
      }
    };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
}

pub(super) fn browser_frame_select_script(selector: &str) -> String {
    let encoded = serde_json::to_string(selector).expect("frame selector JSON is infallible");
    format!(
        r#"(() => {{
  const selector = {encoded};
  try {{
    const frame = document.querySelector(selector);
    if (!frame || !frame.contentWindow || !frame.contentWindow.document) {{
      throw new Error(`No accessible frame matches selector: ${{selector}}`);
    }}
    window.__cmuxSelectedFrameSelector = selector;
    return {{ ok: true, value: {{ selector, url: String(frame.contentWindow.location.href || '') }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

pub(super) fn browser_frame_main_script() -> &'static str {
    r#"(() => {
  try {
    delete window.__cmuxSelectedFrameSelector;
    return { ok: true, value: true };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
}

pub(super) fn browser_dialog_script(action: BrowserDialogAction, text: Option<&str>) -> String {
    let hook = serde_json::to_string(browser_dialog_hook_script()).expect("dialog hook JSON");
    let encoded_text = serde_json::to_string(text.unwrap_or_default()).expect("dialog text JSON");
    let accepted = action == BrowserDialogAction::Accept;
    format!(
        r#"(() => {{
  try {{
    (0, eval)({hook});
    const text = {encoded_text};
    const queue = globalThis.__cmuxDialogCapture.queue;
    const dialog = queue.shift();
    if (!dialog) {{ throw new Error('No pending browser dialog'); }}
    return {{ ok: true, value: {{ ...dialog, accepted: {accepted}, text }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

pub(super) fn browser_snapshot_script() -> &'static str {
    r#"(() => {
  try {
    const refs = {};
    const lines = ['- document ' + JSON.stringify(document.title || '')];
    const cssPath = (element) => {
      if (!element || element.nodeType !== 1) { return ''; }
      if (element.id) { return '#' + CSS.escape(element.id); }
      const parts = [];
      let current = element;
      while (current && current.nodeType === 1 && current !== document.documentElement) {
        let part = current.tagName.toLowerCase();
        if (current.classList && current.classList.length) {
          part += '.' + Array.from(current.classList).slice(0, 2).map((name) => CSS.escape(name)).join('.');
        }
        const parent = current.parentElement;
        if (parent) {
          const siblings = Array.from(parent.children).filter((child) => child.tagName === current.tagName);
          if (siblings.length > 1) {
            part += `:nth-of-type(${siblings.indexOf(current) + 1})`;
          }
        }
        parts.unshift(part);
        current = parent;
      }
      return parts.join(' > ');
    };
    const describe = (element) => {
      const tag = element.tagName.toLowerCase();
      const id = element.id ? `#${element.id}` : '';
      const label = element.getAttribute('aria-label') || element.getAttribute('title') || element.getAttribute('placeholder') || (element.innerText || element.value || '').trim();
      return `${tag}${id}${label ? ' ' + JSON.stringify(label.slice(0, 80)) : ''}`;
    };
    let index = 1;
    for (const element of Array.from(document.querySelectorAll('a,button,input,select,textarea,label,[role],[data-testid],h1,h2,h3,p,div,span')).slice(0, 120)) {
      const ref = `e${index++}`;
      const selector = cssPath(element);
      refs[ref] = { selector, tag: element.tagName.toLowerCase(), text: (element.innerText || element.value || '').trim().slice(0, 200) };
      lines.push(`  - ${ref} ${describe(element)}`);
    }
    window.__cmuxSnapshotRefs = refs;
    return { ok: true, value: { snapshot: lines.join('\n'), refs } };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
}

pub(super) fn browser_locator_script(
    locator: BrowserLocator,
    params: &serde_json::Map<String, Value>,
) -> Result<String, String> {
    let (kind, primary, secondary, index) = match locator {
        BrowserLocator::Role => (
            "role",
            string_param(params, &["role"])
                .ok_or_else(|| "browser.find.role requires a role".to_string())?,
            raw_string_param(params, &["name"]).unwrap_or_default(),
            0usize,
        ),
        BrowserLocator::Text => (
            "text",
            raw_string_param(params, &["text"])
                .ok_or_else(|| "browser.find.text requires text".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Label => (
            "label",
            raw_string_param(params, &["label", "text"])
                .ok_or_else(|| "browser.find.label requires a label".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Placeholder => (
            "placeholder",
            raw_string_param(params, &["placeholder"])
                .ok_or_else(|| "browser.find.placeholder requires a placeholder".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Alt => (
            "alt",
            raw_string_param(params, &["alt", "text"])
                .ok_or_else(|| "browser.find.alt requires alt text".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Title => (
            "title",
            raw_string_param(params, &["title"])
                .ok_or_else(|| "browser.find.title requires a title".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::TestId => (
            "testid",
            raw_string_param(params, &["testid", "test_id", "testId"])
                .ok_or_else(|| "browser.find.testid requires a testid".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::First => (
            "first",
            raw_string_param(params, &["selector"])
                .ok_or_else(|| "browser.find.first requires a selector".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Last => (
            "last",
            raw_string_param(params, &["selector"])
                .ok_or_else(|| "browser.find.last requires a selector".to_string())?,
            String::new(),
            0usize,
        ),
        BrowserLocator::Nth => (
            "nth",
            raw_string_param(params, &["selector"])
                .ok_or_else(|| "browser.find.nth requires a selector".to_string())?,
            String::new(),
            usize_param(params, &["index", "nth"]).unwrap_or(0),
        ),
    };
    let encoded_kind = serde_json::to_string(kind).expect("locator kind JSON is infallible");
    let encoded_primary =
        serde_json::to_string(&primary).expect("locator value JSON is infallible");
    let encoded_secondary =
        serde_json::to_string(&secondary).expect("locator secondary JSON is infallible");
    Ok(format!(
        r#"(() => {{
  {context}
  const kind = {encoded_kind};
  const primary = {encoded_primary};
  const secondary = {encoded_secondary};
  const nthIndex = {index};
  try {{
    const normalize = (value) => String(value || '').trim().toLowerCase();
    const includes = (value, needle) => normalize(value).includes(normalize(needle));
    const cssPath = (element) => {{
      if (!element || element.nodeType !== 1) {{ return ''; }}
      if (element.id) {{ return '#' + CSS.escape(element.id); }}
      const parts = [];
      let current = element;
      while (current && current.nodeType === 1 && current !== document.documentElement) {{
        let part = current.tagName.toLowerCase();
        const parent = current.parentElement;
        if (parent) {{
          const siblings = Array.from(parent.children).filter((child) => child.tagName === current.tagName);
          if (siblings.length > 1) {{
            part += `:nth-of-type(${{siblings.indexOf(current) + 1}})`;
          }}
        }}
        parts.unshift(part);
        current = parent;
      }}
      return parts.join(' > ');
    }};
    const implicitRole = (element) => {{
      const tag = element.tagName.toLowerCase();
      const type = normalize(element.getAttribute('type'));
      if (element.getAttribute('role')) {{ return normalize(element.getAttribute('role')); }}
      if (tag === 'button') {{ return 'button'; }}
      if (tag === 'a' && element.hasAttribute('href')) {{ return 'link'; }}
      if (tag === 'select') {{ return 'combobox'; }}
      if (tag === 'textarea') {{ return 'textbox'; }}
      if (tag === 'input') {{
        if (type === 'checkbox') {{ return 'checkbox'; }}
        if (type === 'radio') {{ return 'radio'; }}
        if (type === 'submit' || type === 'button') {{ return 'button'; }}
        return 'textbox';
      }}
      return '';
    }};
    const accessibleName = (element) => {{
      const aria = element.getAttribute('aria-label');
      if (aria) {{ return aria; }}
      const labelledBy = element.getAttribute('aria-labelledby');
      if (labelledBy) {{
        return labelledBy.split(/\s+/).map((id) => document.getElementById(id)?.innerText || '').join(' ').trim();
      }}
      if (element.id) {{
        const label = document.querySelector(`label[for="${{CSS.escape(element.id)}}"]`);
        if (label) {{ return label.innerText || label.textContent || ''; }}
      }}
      return element.innerText || element.value || element.getAttribute('title') || element.getAttribute('alt') || '';
    }};
    let candidates = [];
    if (kind === 'role') {{
      candidates = Array.from(document.querySelectorAll('*')).filter((element) => {{
        if (implicitRole(element) !== normalize(primary)) {{ return false; }}
        return !secondary || includes(accessibleName(element), secondary);
      }});
    }} else if (kind === 'text') {{
      candidates = Array.from(document.querySelectorAll('body *')).filter((element) => includes(element.innerText || element.textContent || '', primary));
    }} else if (kind === 'label') {{
      const labels = Array.from(document.querySelectorAll('label')).filter((label) => includes(label.innerText || label.textContent || '', primary));
      candidates = labels.map((label) => label.htmlFor ? document.getElementById(label.htmlFor) : label.querySelector('input,textarea,select,button')).filter(Boolean);
    }} else if (kind === 'placeholder') {{
      candidates = Array.from(document.querySelectorAll('[placeholder]')).filter((element) => includes(element.getAttribute('placeholder'), primary));
    }} else if (kind === 'alt') {{
      candidates = Array.from(document.querySelectorAll('[alt]')).filter((element) => includes(element.getAttribute('alt'), primary));
    }} else if (kind === 'title') {{
      candidates = Array.from(document.querySelectorAll('[title]')).filter((element) => includes(element.getAttribute('title'), primary));
    }} else if (kind === 'testid') {{
      candidates = Array.from(document.querySelectorAll('[data-testid], [data-test-id], [data-test]')).filter((element) => [element.getAttribute('data-testid'), element.getAttribute('data-test-id'), element.getAttribute('data-test')].some((value) => normalize(value) === normalize(primary)));
    }} else if (kind === 'first' || kind === 'last' || kind === 'nth') {{
      candidates = Array.from(document.querySelectorAll(primary));
    }}
    const element = kind === 'last' ? candidates[candidates.length - 1] : candidates[kind === 'nth' ? nthIndex : 0];
    if (!element) {{ throw new Error(`No element matches locator: ${{kind}} ${{primary}}`); }}
    window.__cmuxSnapshotRefs = window.__cmuxSnapshotRefs || {{}};
    const ref = `e${{Object.keys(window.__cmuxSnapshotRefs).length + 1}}`;
    const selector = cssPath(element);
    window.__cmuxSnapshotRefs[ref] = {{ selector, tag: element.tagName.toLowerCase(), text: (element.innerText || element.value || '').trim().slice(0, 200) }};
    return {{ ok: true, value: {{ element_ref: '@' + ref, selector, ref, tag: element.tagName.toLowerCase(), text: (element.innerText || element.value || '').trim() }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    ))
}

pub(super) fn browser_addstyle_script(css: &str) -> String {
    let encoded_css = serde_json::to_string(css).expect("CSS JSON is infallible");
    format!(
        r#"(() => {{
  const css = {encoded_css};
  try {{
    const style = document.createElement('style');
    style.setAttribute('data-cmux-added-style', 'true');
    style.textContent = css;
    (document.head || document.documentElement).appendChild(style);
    return {{ ok: true, value: true }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

pub(super) fn browser_cookie_script(
    action: BrowserCookieAction,
    name: Option<&str>,
    value: Option<&str>,
) -> String {
    let encoded_name = serde_json::to_string(name.unwrap_or_default()).expect("cookie name JSON");
    let encoded_value =
        serde_json::to_string(value.unwrap_or_default()).expect("cookie value JSON");
    let action_name = match action {
        BrowserCookieAction::Get => "get",
        BrowserCookieAction::Set => "set",
        BrowserCookieAction::Clear => "clear",
    };
    let encoded_action = serde_json::to_string(action_name).expect("cookie action JSON");
    format!(
        r#"(() => {{
  const action = {encoded_action};
  const name = {encoded_name};
  const value = {encoded_value};
  try {{
    const parseCookies = () => String(document.cookie || '').split(';').map((part) => part.trim()).filter(Boolean).map((part) => {{
      const split = part.indexOf('=');
      const rawName = split >= 0 ? part.slice(0, split) : part;
      const rawValue = split >= 0 ? part.slice(split + 1) : '';
      return {{ name: decodeURIComponent(rawName), value: decodeURIComponent(rawValue) }};
    }});
    if (action === 'set') {{
      document.cookie = `${{encodeURIComponent(name)}}=${{encodeURIComponent(value)}}; path=/`;
      return {{ ok: true, value: {{ cookies: parseCookies().filter((cookie) => !name || cookie.name === name), set: true }} }};
    }}
    if (action === 'clear') {{
      document.cookie = `${{encodeURIComponent(name)}}=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT`;
      return {{ ok: true, value: {{ cookies: parseCookies().filter((cookie) => !name || cookie.name === name), cleared: true }} }};
    }}
    const cookies = parseCookies().filter((cookie) => !name || cookie.name === name);
    return {{ ok: true, value: {{ cookies }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

pub(super) fn browser_storage_script(
    action: BrowserStorageAction,
    storage_type: &str,
    key: Option<&str>,
    value: &Value,
) -> String {
    let encoded_type = serde_json::to_string(storage_type).expect("storage type JSON");
    let encoded_key = serde_json::to_string(key.unwrap_or_default()).expect("storage key JSON");
    let encoded_value = serde_json::to_string(value).expect("storage value JSON");
    let action_name = match action {
        BrowserStorageAction::Get => "get",
        BrowserStorageAction::Set => "set",
        BrowserStorageAction::Clear => "clear",
    };
    let encoded_action = serde_json::to_string(action_name).expect("storage action JSON");
    format!(
        r#"(() => {{
  const action = {encoded_action};
  const storageType = {encoded_type};
  const key = {encoded_key};
  const encodedValue = {encoded_value};
  try {{
    const store = storageType === 'session' ? window.sessionStorage : window.localStorage;
    const decode = (raw) => {{
      if (raw === null || raw === undefined) {{ return null; }}
      try {{ return JSON.parse(raw); }} catch (_error) {{ return raw; }}
    }};
    if (action === 'set') {{
      store.setItem(key, JSON.stringify(encodedValue));
      return {{ ok: true, value: {{ type: storageType, key, value: encodedValue, set: true }} }};
    }}
    if (action === 'clear') {{
      if (key) {{ store.removeItem(key); }} else {{ store.clear(); }}
      return {{ ok: true, value: {{ type: storageType, key: key || null, cleared: true }} }};
    }}
    return {{ ok: true, value: {{ type: storageType, key, value: decode(store.getItem(key)) }} }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

pub(super) fn browser_console_hook_script() -> &'static str {
    r#"(() => {
  try {
    if (!window.__cmuxConsoleCapture) {
      const state = {
        entries: [],
        errors: [],
        originals: {
          log: console.log.bind(console),
          info: console.info.bind(console),
          warn: console.warn.bind(console),
          error: console.error.bind(console),
          debug: console.debug.bind(console),
        },
      };
      const stringify = (value) => {
        try {
          if (typeof value === 'string') { return value; }
          if (value instanceof Error) { return value.stack || value.message || String(value); }
          return JSON.stringify(value);
        } catch (_error) {
          return String(value);
        }
      };
      const pushEntry = (level, args) => {
        state.entries.push({
          level,
          text: Array.from(args).map(stringify).join(' '),
          args: Array.from(args).map(stringify),
          timestamp: Date.now(),
        });
      };
      for (const level of ['log', 'info', 'warn', 'error', 'debug']) {
        console[level] = function (...args) {
          pushEntry(level, args);
          return state.originals[level](...args);
        };
      }
      window.addEventListener('error', (event) => {
        state.errors.push({
          message: String(event.message || ''),
          source: event.filename || null,
          line: event.lineno || null,
          column: event.colno || null,
          stack: event.error && event.error.stack ? String(event.error.stack) : null,
          timestamp: Date.now(),
        });
      });
      window.addEventListener('unhandledrejection', (event) => {
        state.errors.push({
          message: stringify(event.reason),
          source: 'unhandledrejection',
          line: null,
          column: null,
          stack: event.reason && event.reason.stack ? String(event.reason.stack) : null,
          timestamp: Date.now(),
        });
      });
      window.__cmuxConsoleCapture = state;
    }
    return window.__cmuxConsoleCapture;
  } catch (error) {
    return null;
  }
})()"#
}

pub(super) fn browser_console_list_script() -> String {
    let hook = serde_json::to_string(browser_console_hook_script()).expect("console hook JSON");
    r#"(() => {
  try {
    (0, eval)(__CMUX_CONSOLE_HOOK__);
    const entries = (window.__cmuxConsoleCapture && window.__cmuxConsoleCapture.entries) || [];
    return { ok: true, value: { entries, count: entries.length } };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
        .replace("__CMUX_CONSOLE_HOOK__", &hook)
}

pub(super) fn browser_console_clear_script() -> String {
    let hook = serde_json::to_string(browser_console_hook_script()).expect("console hook JSON");
    r#"(() => {
  try {
    (0, eval)(__CMUX_CONSOLE_HOOK__);
    const capture = window.__cmuxConsoleCapture;
    const cleared = capture ? capture.entries.length : 0;
    if (capture) { capture.entries = []; }
    return { ok: true, value: { entries: [], count: 0, cleared } };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
        .replace("__CMUX_CONSOLE_HOOK__", &hook)
}

pub(super) fn browser_errors_list_script() -> String {
    let hook = serde_json::to_string(browser_console_hook_script()).expect("console hook JSON");
    r#"(() => {
  try {
    (0, eval)(__CMUX_CONSOLE_HOOK__);
    const errors = (window.__cmuxConsoleCapture && window.__cmuxConsoleCapture.errors) || [];
    return { ok: true, value: { errors, count: errors.length } };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
        .replace("__CMUX_CONSOLE_HOOK__", &hook)
}

pub(super) fn browser_state_capture_script() -> &'static str {
    r#"(() => {
  try {
    const entries = (store) => {
      const values = {};
      for (let index = 0; index < store.length; index += 1) {
        const key = store.key(index);
        values[key] = store.getItem(key);
      }
      return values;
    };
    const cookies = String(document.cookie || '').split(';').map((part) => part.trim()).filter(Boolean).map((part) => {
      const split = part.indexOf('=');
      const rawName = split >= 0 ? part.slice(0, split) : part;
      const rawValue = split >= 0 ? part.slice(split + 1) : '';
      return { name: decodeURIComponent(rawName), value: decodeURIComponent(rawValue) };
    });
    const value = {
      version: 1,
      url: String(window.location.href || ''),
      savedAt: Date.now(),
      localStorage: entries(window.localStorage),
      sessionStorage: entries(window.sessionStorage),
      cookies,
    };
    return { ok: true, value };
  } catch (error) {
    return { ok: false, error: String((error && (error.stack || error.message)) || error) };
  }
})()"#
}

pub(super) fn browser_state_restore_script(state: &Value) -> String {
    let encoded_state = serde_json::to_string(state).expect("browser state JSON is infallible");
    format!(
        r#"(() => {{
  const state = {encoded_state};
  try {{
    const restoreStore = (store, values) => {{
      store.clear();
      for (const [key, value] of Object.entries(values || {{}})) {{
        store.setItem(key, String(value));
      }}
    }};
    restoreStore(window.localStorage, state.localStorage);
    restoreStore(window.sessionStorage, state.sessionStorage);
    for (const cookie of Array.from(String(document.cookie || '').split(';')).map((part) => part.trim()).filter(Boolean)) {{
      const split = cookie.indexOf('=');
      const rawName = split >= 0 ? cookie.slice(0, split) : cookie;
      document.cookie = `${{rawName}}=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT`;
    }}
    for (const cookie of state.cookies || []) {{
      if (!cookie || !cookie.name) {{ continue; }}
      document.cookie = `${{encodeURIComponent(cookie.name)}}=${{encodeURIComponent(cookie.value || '')}}; path=/`;
    }}
    return {{
      ok: true,
      value: {{
        version: state.version || 1,
        localStorageCount: Object.keys(state.localStorage || {{}}).length,
        sessionStorageCount: Object.keys(state.sessionStorage || {{}}).length,
        cookieCount: Array.isArray(state.cookies) ? state.cookies.length : 0
      }}
    }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#
    )
}

pub(super) fn browser_highlight_script(selector: &str) -> String {
    let encoded = serde_json::to_string(selector).expect("highlight selector JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const raw = {encoded};
  const ref = String(raw || '').startsWith('@') ? String(raw).slice(1) : raw;
  const selector = (window.__cmuxSnapshotRefs && window.__cmuxSnapshotRefs[ref] && window.__cmuxSnapshotRefs[ref].selector) || raw;
  try {{
    const element = document.querySelector(selector);
    if (!element) {{ throw new Error(`No element matches selector: ${{selector}}`); }}
    const previous = document.querySelector('[data-cmux-highlight-overlay="true"]');
    if (previous) {{ previous.remove(); }}
    const rect = element.getBoundingClientRect();
    const overlay = document.createElement('div');
    overlay.setAttribute('data-cmux-highlight-overlay', 'true');
    Object.assign(overlay.style, {{
      position: 'fixed',
      pointerEvents: 'none',
      zIndex: '2147483647',
      left: `${{rect.left}}px`,
      top: `${{rect.top}}px`,
      width: `${{rect.width}}px`,
      height: `${{rect.height}}px`,
      border: '3px solid #00A3FF',
      boxShadow: '0 0 0 9999px rgba(0, 163, 255, 0.10)',
      borderRadius: '6px'
    }});
    document.documentElement.appendChild(overlay);
    return {{ ok: true, value: true }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

pub(super) fn browser_wait_selector_script(selector: &str) -> String {
    let encoded = serde_json::to_string(selector).expect("selector JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const raw = {encoded};
  const ref = String(raw || '').startsWith('@') ? String(raw).slice(1) : raw;
  const selector = (window.__cmuxSnapshotRefs && window.__cmuxSnapshotRefs[ref] && window.__cmuxSnapshotRefs[ref].selector) || raw;
  return {{ ok: true, value: Boolean(document.querySelector(selector)) }};
}})()"#,
        context = browser_js_context_prelude(),
    )
}

pub(super) fn browser_wait_text_script(text: &str) -> String {
    let encoded = serde_json::to_string(text).expect("text JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const needle = {encoded};
  return {{ ok: true, value: String(document.body ? (document.body.innerText || document.body.textContent || '') : '').includes(needle) }};
}})()"#,
        context = browser_js_context_prelude(),
    )
}

pub(super) fn browser_wait_function_script(expression: &str) -> String {
    let encoded = serde_json::to_string(expression).expect("expression JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const expression = {encoded};
  try {{
    return {{ ok: true, value: Boolean((0, eval)(expression)) }};
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

pub(super) fn browser_wait_load_state_script(load_state: &str) -> String {
    let normalized = load_state.trim().to_ascii_lowercase();
    let target = match normalized.as_str() {
        "domcontentloaded" | "interactive" => "interactive",
        "complete" | "load" | "loaded" => "complete",
        _ => "complete",
    };
    let encoded = serde_json::to_string(target).expect("load state JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const target = {encoded};
  const state = document.readyState;
  const value = target === 'interactive' ? (state === 'interactive' || state === 'complete') : state === 'complete';
  return {{ ok: true, value }};
}})()"#,
        context = browser_js_context_prelude(),
    )
}

pub(super) fn browser_wait_url_script(url_contains: &str) -> String {
    let encoded = serde_json::to_string(url_contains).expect("URL token JSON is infallible");
    format!(
        r#"(() => {{
  {context}
  const needle = {encoded};
  return {{ ok: true, value: String(window.location.href || '').includes(needle) }};
}})()"#,
        context = browser_js_context_prelude(),
    )
}

pub(super) fn browser_action_script(
    action: BrowserAction,
    selector: Option<&str>,
    text: &str,
    value: &str,
    key: &str,
    dx: f64,
    dy: f64,
) -> String {
    let encoded_selector =
        serde_json::to_string(selector.unwrap_or_default()).expect("selector JSON is infallible");
    let encoded_text = serde_json::to_string(text).expect("text JSON is infallible");
    let encoded_value = serde_json::to_string(value).expect("value JSON is infallible");
    let encoded_key = serde_json::to_string(key).expect("key JSON is infallible");
    let body = match action {
        BrowserAction::Click => "element.click(); return { ok: true, value: true };",
        BrowserAction::DblClick => "element.dispatchEvent(new MouseEvent('dblclick', { bubbles: true, cancelable: true, view: window })); return { ok: true, value: true };",
        BrowserAction::Hover => "for (const type of ['mouseover', 'mouseenter', 'mousemove']) { element.dispatchEvent(new MouseEvent(type, { bubbles: true, cancelable: true, view: window })); } return { ok: true, value: true };",
        BrowserAction::Focus => "element.focus(); return { ok: true, value: true };",
        BrowserAction::Type => "element.focus(); element.value = String(element.value || '') + __cmuxText; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: element.value ?? true };",
        BrowserAction::Fill => "element.focus(); element.value = __cmuxText; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: element.value ?? true };",
        BrowserAction::Press => "const pressTarget = document.activeElement || document.body; for (const type of ['keydown', 'keypress', 'keyup']) { pressTarget.dispatchEvent(new KeyboardEvent(type, { key: __cmuxKey, bubbles: true, cancelable: true })); } return { ok: true, value: true };",
        BrowserAction::KeyDown => "const downTarget = document.activeElement || document.body; downTarget.dispatchEvent(new KeyboardEvent('keydown', { key: __cmuxKey, bubbles: true, cancelable: true })); return { ok: true, value: true };",
        BrowserAction::KeyUp => "const upTarget = document.activeElement || document.body; upTarget.dispatchEvent(new KeyboardEvent('keyup', { key: __cmuxKey, bubbles: true, cancelable: true })); return { ok: true, value: true };",
        BrowserAction::Check => "element.checked = true; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: Boolean(element.checked) };",
        BrowserAction::Uncheck => "element.checked = false; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: Boolean(element.checked) };",
        BrowserAction::Select => "element.value = __cmuxValue; for (const type of ['input', 'change']) { element.dispatchEvent(new Event(type, { bubbles: true })); } return { ok: true, value: element.value };",
        BrowserAction::Scroll => "const target = element || window; if (target === window) { window.scrollBy(__cmuxDx, __cmuxDy); } else { target.scrollBy(__cmuxDx, __cmuxDy); } return { ok: true, value: true };",
        BrowserAction::ScrollIntoView => "element.scrollIntoView({ block: 'center', inline: 'nearest' }); return { ok: true, value: true };",
    };
    let needs_element = browser_action_requires_selector(action);
    let element_lookup = if needs_element {
        "const element = document.querySelector(__cmuxSelector); if (!element) { throw new Error(`No element matches selector: ${__cmuxSelector}`); }"
    } else if action == BrowserAction::Scroll {
        "const element = __cmuxSelector ? document.querySelector(__cmuxSelector) : null; if (__cmuxSelector && !element) { throw new Error(`No element matches selector: ${__cmuxSelector}`); }"
    } else {
        "const element = null;"
    };
    format!(
        r#"(() => {{
  {context}
  const __cmuxRawSelector = {encoded_selector};
  const __cmuxRef = String(__cmuxRawSelector || '').startsWith('@') ? String(__cmuxRawSelector).slice(1) : __cmuxRawSelector;
  const __cmuxSelector = (window.__cmuxSnapshotRefs && window.__cmuxSnapshotRefs[__cmuxRef] && window.__cmuxSnapshotRefs[__cmuxRef].selector) || __cmuxRawSelector;
  const __cmuxText = {encoded_text};
  const __cmuxValue = {encoded_value};
  const __cmuxKey = {encoded_key};
  const __cmuxDx = {dx};
  const __cmuxDy = {dy};
  try {{
    {element_lookup}
    {body}
  }} catch (error) {{
    return {{ ok: false, error: String((error && (error.stack || error.message)) || error) }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

pub(super) fn browser_action_requires_selector(action: BrowserAction) -> bool {
    matches!(
        action,
        BrowserAction::Click
            | BrowserAction::DblClick
            | BrowserAction::Hover
            | BrowserAction::Focus
            | BrowserAction::Type
            | BrowserAction::Fill
            | BrowserAction::Check
            | BrowserAction::Uncheck
            | BrowserAction::Select
            | BrowserAction::ScrollIntoView
    )
}

pub(super) fn browser_action_method(action: BrowserAction) -> &'static str {
    match action {
        BrowserAction::Click => "browser.click",
        BrowserAction::DblClick => "browser.dblclick",
        BrowserAction::Hover => "browser.hover",
        BrowserAction::Focus => "browser.focus",
        BrowserAction::Type => "browser.type",
        BrowserAction::Fill => "browser.fill",
        BrowserAction::Press => "browser.press",
        BrowserAction::KeyDown => "browser.keydown",
        BrowserAction::KeyUp => "browser.keyup",
        BrowserAction::Check => "browser.check",
        BrowserAction::Uncheck => "browser.uncheck",
        BrowserAction::Select => "browser.select",
        BrowserAction::Scroll => "browser.scroll",
        BrowserAction::ScrollIntoView => "browser.scroll_into_view",
    }
}

pub(super) fn browser_locator_method(locator: BrowserLocator) -> &'static str {
    match locator {
        BrowserLocator::Role => "browser.find.role",
        BrowserLocator::Text => "browser.find.text",
        BrowserLocator::Label => "browser.find.label",
        BrowserLocator::Placeholder => "browser.find.placeholder",
        BrowserLocator::Alt => "browser.find.alt",
        BrowserLocator::Title => "browser.find.title",
        BrowserLocator::TestId => "browser.find.testid",
        BrowserLocator::First => "browser.find.first",
        BrowserLocator::Last => "browser.find.last",
        BrowserLocator::Nth => "browser.find.nth",
    }
}

pub(super) fn browser_locator_payload(panel_id: &str, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            payload.insert(key.clone(), value.clone());
        }
        if let Some(element_ref) = object
            .get("element_ref")
            .or_else(|| object.get("elementRef"))
        {
            payload.insert("elementRef".to_string(), element_ref.clone());
        }
    } else {
        payload.insert("value".to_string(), value);
    }
    Value::Object(payload)
}

pub(super) fn browser_cookie_method(action: BrowserCookieAction) -> &'static str {
    match action {
        BrowserCookieAction::Get => "browser.cookies.get",
        BrowserCookieAction::Set => "browser.cookies.set",
        BrowserCookieAction::Clear => "browser.cookies.clear",
    }
}

pub(super) fn browser_storage_method(action: BrowserStorageAction) -> &'static str {
    match action {
        BrowserStorageAction::Get => "browser.storage.get",
        BrowserStorageAction::Set => "browser.storage.set",
        BrowserStorageAction::Clear => "browser.storage.clear",
    }
}

pub(super) fn browser_console_method(action: BrowserConsoleAction) -> &'static str {
    match action {
        BrowserConsoleAction::List => "browser.console.list",
        BrowserConsoleAction::Clear => "browser.console.clear",
    }
}

pub(super) fn browser_dialog_method(action: BrowserDialogAction) -> &'static str {
    match action {
        BrowserDialogAction::Accept => "browser.dialog.accept",
        BrowserDialogAction::Dismiss => "browser.dialog.dismiss",
    }
}

pub(super) fn browser_cookie_payload(
    panel_id: &str,
    action: BrowserCookieAction,
    value: Value,
) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    if let Some(cookies) = value.get("cookies") {
        payload.insert("cookies".to_string(), cookies.clone());
    } else {
        payload.insert("cookies".to_string(), json!([]));
    }
    match action {
        BrowserCookieAction::Set => {
            payload.insert("set".to_string(), json!(true));
        }
        BrowserCookieAction::Clear => {
            payload.insert("cleared".to_string(), json!(true));
        }
        BrowserCookieAction::Get => {}
    }
    Value::Object(payload)
}

pub(super) fn browser_dialog_payload(
    panel_id: &str,
    action: BrowserDialogAction,
    value: Value,
) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            payload.insert(key.clone(), value.clone());
        }
    }
    payload.insert(
        "accepted".to_string(),
        json!(action == BrowserDialogAction::Accept),
    );
    Value::Object(payload)
}

pub(super) fn browser_console_payload(
    panel_id: &str,
    action: BrowserConsoleAction,
    value: Value,
) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    let entries = value.get("entries").cloned().unwrap_or_else(|| json!([]));
    let count = value
        .get("count")
        .cloned()
        .unwrap_or_else(|| json!(entries.as_array().map_or(0, Vec::len)));
    payload.insert("entries".to_string(), entries);
    payload.insert("count".to_string(), count);
    if action == BrowserConsoleAction::Clear {
        payload.insert(
            "cleared".to_string(),
            value.get("cleared").cloned().unwrap_or_else(|| json!(true)),
        );
    }
    Value::Object(payload)
}

pub(super) fn browser_errors_payload(panel_id: &str, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    let errors = value.get("errors").cloned().unwrap_or_else(|| json!([]));
    let count = value
        .get("count")
        .cloned()
        .unwrap_or_else(|| json!(errors.as_array().map_or(0, Vec::len)));
    payload.insert("errors".to_string(), errors);
    payload.insert("count".to_string(), count);
    Value::Object(payload)
}

pub(super) fn browser_state_payload(panel_id: &str, path: &str, verb: &str, state: Value) -> Value {
    let local_storage_count = state
        .get("localStorage")
        .and_then(Value::as_object)
        .map_or_else(
            || {
                state
                    .get("localStorageCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize
            },
            serde_json::Map::len,
        );
    let session_storage_count = state
        .get("sessionStorage")
        .and_then(Value::as_object)
        .map_or_else(
            || {
                state
                    .get("sessionStorageCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize
            },
            serde_json::Map::len,
        );
    let cookie_count = state.get("cookies").and_then(Value::as_array).map_or_else(
        || {
            state
                .get("cookieCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize
        },
        Vec::len,
    );
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    payload.insert("path".to_string(), json!(path));
    payload.insert(verb.to_string(), json!(true));
    payload.insert("localStorageCount".to_string(), json!(local_storage_count));
    payload.insert(
        "sessionStorageCount".to_string(),
        json!(session_storage_count),
    );
    payload.insert("cookieCount".to_string(), json!(cookie_count));
    Value::Object(payload)
}

pub(super) fn solid_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let width = width.max(1);
    let height = height.max(1);
    let row_len = 1usize + width as usize * 3;
    let mut raw = Vec::with_capacity(row_len * height as usize);
    for _ in 0..height {
        raw.push(0);
        for _ in 0..width {
            raw.extend_from_slice(&rgb);
        }
    }

    let mut png = Vec::new();
    png.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    png_chunk(&mut png, b"IHDR", &ihdr);
    png_chunk(&mut png, b"IDAT", &zlib_store(&raw));
    png_chunk(&mut png, b"IEND", &[]);
    png
}

pub(super) fn zlib_store(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    let mut offset = 0usize;
    while offset < raw.len() {
        let remaining = raw.len() - offset;
        let len = remaining.min(u16::MAX as usize);
        let final_block = offset + len >= raw.len();
        out.push(if final_block { 0x01 } else { 0x00 });
        let len_u16 = len as u16;
        out.extend_from_slice(&len_u16.to_le_bytes());
        out.extend_from_slice(&(!len_u16).to_le_bytes());
        out.extend_from_slice(&raw[offset..offset + len]);
        offset += len;
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

pub(super) fn png_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(kind.len() + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

pub(super) fn adler32(data: &[u8]) -> u32 {
    const MOD: u32 = 65_521;
    let mut a = 1u32;
    let mut b = 0u32;
    for byte in data {
        a = (a + u32::from(*byte)) % MOD;
        b = (b + a) % MOD;
    }
    (b << 16) | a
}

pub(super) fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

pub(super) fn browser_storage_payload(
    panel_id: &str,
    action: BrowserStorageAction,
    value: Value,
) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    if let Some(object) = value.as_object() {
        for (key, value) in object {
            payload.insert(key.clone(), value.clone());
        }
    } else {
        payload.insert("value".to_string(), value);
    }
    match action {
        BrowserStorageAction::Set => {
            payload.insert("set".to_string(), json!(true));
        }
        BrowserStorageAction::Clear => {
            payload.insert("cleared".to_string(), json!(true));
        }
        BrowserStorageAction::Get => {}
    }
    Value::Object(payload)
}

pub(super) fn browser_not_found_message(message: &str) -> String {
    if message.contains("No element matches selector") {
        format!("{message}; hint: verify the selector or refresh browser.snapshot refs; snapshot unavailable in this error path")
    } else {
        message.to_string()
    }
}

pub(super) fn browser_getter_script(
    getter: BrowserGetter,
    selector: Option<&str>,
    attr: Option<&str>,
) -> String {
    let encoded_selector =
        serde_json::to_string(selector.unwrap_or_default()).expect("selector JSON is infallible");
    let encoded_attr =
        serde_json::to_string(attr.unwrap_or_default()).expect("attribute JSON is infallible");
    let body = match getter {
        BrowserGetter::Text => {
            "const value = element.innerText ?? element.textContent ?? ''; return { ok: true, value };"
        }
        BrowserGetter::Html => {
            "const value = element.outerHTML ?? element.innerHTML ?? ''; return { ok: true, value };"
        }
        BrowserGetter::Value => {
            "const value = element.value ?? element.getAttribute('value') ?? ''; return { ok: true, value };"
        }
        BrowserGetter::Attr => {
            "const attr = __cmuxAttr; const value = element.getAttribute(attr); return { ok: true, value };"
        }
        BrowserGetter::Title => "return { ok: true, value: document.title || '' };",
        BrowserGetter::Count => {
            "return { ok: true, value: document.querySelectorAll(__cmuxSelector).length };"
        }
        BrowserGetter::Box => {
            "const rect = element.getBoundingClientRect(); const value = { x: rect.x, y: rect.y, top: rect.top, right: rect.right, bottom: rect.bottom, left: rect.left, width: rect.width, height: rect.height }; return { ok: true, value };"
        }
        BrowserGetter::Styles => {
            "const style = window.getComputedStyle(element); if (__cmuxAttr) { return { ok: true, value: style.getPropertyValue(__cmuxAttr) || style[__cmuxAttr] || '' }; } const value = {}; for (const name of style) { value[name] = style.getPropertyValue(name); } return { ok: true, value };"
        }
        BrowserGetter::Visible => {
            "const style = window.getComputedStyle(element); const rect = element.getBoundingClientRect(); const value = style.visibility !== 'hidden' && style.display !== 'none' && Number(style.opacity || '1') !== 0 && rect.width > 0 && rect.height > 0; return { ok: true, value };"
        }
        BrowserGetter::Enabled => {
            "const ariaDisabled = String(element.getAttribute('aria-disabled') || '').toLowerCase(); const value = !element.disabled && ariaDisabled !== 'true'; return { ok: true, value };"
        }
        BrowserGetter::Checked => {
            "const value = Boolean(element.checked); return { ok: true, value };"
        }
    };
    let needs_element = !matches!(getter, BrowserGetter::Title | BrowserGetter::Count);
    let element_lookup = if needs_element {
        "const element = document.querySelector(__cmuxSelector); if (!element) { throw new Error(`No element matches selector: ${__cmuxSelector}`); }"
    } else {
        ""
    };
    format!(
        r#"(() => {{
  {context}
  const __cmuxRawSelector = {encoded_selector};
  const __cmuxRef = String(__cmuxRawSelector || '').startsWith('@') ? String(__cmuxRawSelector).slice(1) : __cmuxRawSelector;
  const __cmuxSelector = (window.__cmuxSnapshotRefs && window.__cmuxSnapshotRefs[__cmuxRef] && window.__cmuxSnapshotRefs[__cmuxRef].selector) || __cmuxRawSelector;
  const __cmuxAttr = {encoded_attr};
  try {{
    {element_lookup}
    {body}
  }} catch (error) {{
    return {{
      ok: false,
      error: String((error && (error.stack || error.message)) || error)
    }};
  }}
}})()"#,
        context = browser_js_context_prelude(),
    )
}

pub(super) fn unwrap_browser_eval_result(value: Value) -> Result<Value, String> {
    let Some(object) = value.as_object() else {
        return Ok(value);
    };
    match object.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(object.get("value").cloned().unwrap_or(Value::Null)),
        Some(false) => Err(object
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("browser JavaScript execution failed")
            .to_string()),
        None => Ok(value),
    }
}

pub(super) fn browser_getter_method(getter: BrowserGetter) -> &'static str {
    match getter {
        BrowserGetter::Text => "browser.get.text",
        BrowserGetter::Html => "browser.get.html",
        BrowserGetter::Value => "browser.get.value",
        BrowserGetter::Attr => "browser.get.attr",
        BrowserGetter::Title => "browser.get.title",
        BrowserGetter::Count => "browser.get.count",
        BrowserGetter::Box => "browser.get.box",
        BrowserGetter::Styles => "browser.get.styles",
        BrowserGetter::Visible => "browser.is.visible",
        BrowserGetter::Enabled => "browser.is.enabled",
        BrowserGetter::Checked => "browser.is.checked",
    }
}

pub(super) fn browser_getter_payload(panel_id: &str, getter: BrowserGetter, value: Value) -> Value {
    let mut payload = serde_json::Map::new();
    payload.insert("surface_id".to_string(), json!(panel_id));
    payload.insert("panel_id".to_string(), json!(panel_id));
    payload.insert("value".to_string(), value.clone());
    match getter {
        BrowserGetter::Text => {
            payload.insert("text".to_string(), value);
        }
        BrowserGetter::Html => {
            payload.insert("html".to_string(), value);
        }
        BrowserGetter::Title => {
            payload.insert("title".to_string(), value);
        }
        BrowserGetter::Count => {
            payload.insert("count".to_string(), value);
        }
        BrowserGetter::Box => {
            payload.insert("box".to_string(), value);
        }
        BrowserGetter::Styles => {
            payload.insert("styles".to_string(), value);
        }
        BrowserGetter::Attr
        | BrowserGetter::Value
        | BrowserGetter::Visible
        | BrowserGetter::Enabled
        | BrowserGetter::Checked => {}
    }
    Value::Object(payload)
}

pub(super) fn debug_terminals(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let selected_workspace = selected_workspace_index(&current);
    let workspace_filter = workspace_index_from_params(&current, params)
        .or_else(|| params.contains_key("workspace_id").then_some(usize::MAX));
    let terminal_state = app.state::<TerminalState>();
    let runtime_by_panel: BTreeMap<String, Value> =
        terminal_runtime_snapshots(terminal_state.inner())
            .into_iter()
            .filter_map(|runtime| {
                let panel_id = runtime.panel_id.clone()?;
                Some((
                    panel_id,
                    json!({
                        "terminal_id": runtime.id,
                        "root_pid": runtime.root_pid,
                        "process_root_pid": runtime.root_pid,
                        "descendant_pids": runtime.descendant_pids,
                        "child_pids": runtime.child_pids,
                        "process_count": runtime.process_count,
                        "foreground_pid": runtime.foreground_pid,
                        "foreground_process_name": runtime.foreground_process_name,
                        "foreground_process_source": runtime.foreground_process_source,
                        "process_error": runtime.process_error,
                    }),
                ))
            })
            .collect();
    let mut terminals = Vec::new();
    let Some(window) = current.windows.first() else {
        return ok(json!({"terminals": terminals}));
    };
    for (workspace_index, workspace) in window.tab_manager.workspaces.iter().enumerate() {
        if workspace_filter.is_some_and(|filter| filter != workspace_index) {
            continue;
        }
        for (surface_index, surface) in surfaces_for_workspace(workspace).into_iter().enumerate() {
            let surface_type = surface
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("terminal");
            if surface_type != "terminal" {
                continue;
            }
            let panel_id = surface
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let runtime = runtime_by_panel.get(&panel_id);
            terminals.push(json!({
                "workspace_id": workspace.workspace_id,
                "workspace_ref": workspace_ref(workspace_index),
                "workspace_selected": workspace_index == selected_workspace,
                "surface_id": panel_id,
                "panel_id": panel_id,
                "surface_ref": surface_ref(surface_index),
                "type": surface_type,
                "title": surface.get("title").cloned().unwrap_or(Value::Null),
                "tty": surface.get("tty").cloned().unwrap_or(Value::Null),
                "tty_name": surface.get("tty_name").cloned().unwrap_or(Value::Null),
                "runtime_surface_ready": runtime.is_some(),
                "terminal_id": runtime.and_then(|value| value.get("terminal_id")).cloned().unwrap_or(Value::Null),
                "root_pid": runtime.and_then(|value| value.get("root_pid")).cloned().unwrap_or(Value::Null),
                "process_root_pid": runtime.and_then(|value| value.get("process_root_pid")).cloned().unwrap_or(Value::Null),
                "descendant_pids": runtime.and_then(|value| value.get("descendant_pids")).cloned().unwrap_or_else(|| json!([])),
                "child_pids": runtime.and_then(|value| value.get("child_pids")).cloned().unwrap_or_else(|| json!([])),
                "process_count": runtime.and_then(|value| value.get("process_count")).cloned().unwrap_or(Value::Null),
                "foreground_pid": runtime.and_then(|value| value.get("foreground_pid")).cloned().unwrap_or(Value::Null),
                "foreground_process_name": runtime.and_then(|value| value.get("foreground_process_name")).cloned().unwrap_or(Value::Null),
                "foreground_process_source": runtime.and_then(|value| value.get("foreground_process_source")).cloned().unwrap_or(Value::Null),
                "process_error": runtime.and_then(|value| value.get("process_error")).cloned().unwrap_or(Value::Null),
                "running": runtime.is_some() || !surface.get("tty").unwrap_or(&Value::Null).is_null(),
            }));
        }
    }
    ok(json!({"terminals": terminals}))
}

pub(super) fn debug_browser_start_direct_proxy(
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
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("debug.browser.start_direct_proxy requires a browser surface");
    }

    let target_host = raw_string_param(params, &["target_host", "targetHost"])
        .map(|host| host.trim().to_string())
        .filter(|host| !host.is_empty());
    let target_port = optional_u16_param(params, "target_port")
        .flatten()
        .or_else(|| optional_u16_param(params, "targetPort").flatten());
    let target_override = match (target_host, target_port) {
        (Some(host), Some(port)) => Some(ProxyTarget { host, port }),
        (None, None) => None,
        _ => return invalid_params(
            "debug.browser.start_direct_proxy target override requires targetHost and targetPort",
        ),
    };

    let state = app.state::<SessionState>();
    match start_direct_browser_proxy_for_control(app, &state, &current, &panel_id, target_override)
    {
        Ok((_snapshot, proxy_url)) => ok(json!({
            "surface_id": panel_id,
            "panel_id": panel_id,
            "proxy_url": proxy_url,
            "proxyUrl": proxy_url,
        })),
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn debug_browser_attach_webview(
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
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("debug.browser.attach_webview requires a browser surface");
    }

    let Some(surface) = browser_surface_payload(&current, workspace_index, &panel_id) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let current_url = surface
        .get("surface")
        .and_then(Value::as_object)
        .and_then(|surface| surface.get("browser_url"))
        .and_then(Value::as_str)
        .or_else(|| surface.get("url").and_then(Value::as_str))
        .unwrap_or("about:blank");
    let current_proxy_url = surface
        .get("surface")
        .and_then(Value::as_object)
        .and_then(|surface| surface.get("browser_proxy_url"))
        .and_then(Value::as_str);
    let url = raw_string_param(params, &["url"]).unwrap_or_else(|| current_url.to_string());
    let proxy_url = raw_string_param(params, &["proxy_url", "proxyUrl"])
        .or_else(|| current_proxy_url.map(str::to_string));
    let visible = bool_param(params, &["visible"]).unwrap_or(true);

    let browser_state = app.state::<BrowserWebviewState>();
    match browser_attach_webview_for_control(
        app,
        browser_state.inner(),
        "main",
        &panel_id,
        Some(&url),
        proxy_url.as_deref(),
        visible,
    ) {
        Ok(reply) => ok(json!(reply)),
        Err(message) => ControlCallResult::Err {
            code: "surface_unavailable".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_reopen_closed(app: &AppHandle) -> ControlCallResult {
    let state = app.state::<SessionState>();
    match reopen_closed_browser_tab_for_control(app, &state) {
        Ok(snapshot) => workspace_current(&snapshot),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_clear_history(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match clear_browser_history_for_control(app, &state, &panel_id) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_toggle_omnibar(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match toggle_browser_omnibar_for_control(app, &state, &panel_id) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_toggle_focus_mode(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match toggle_browser_focus_mode_for_control(app, &state, &panel_id) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_toggle_developer_tools(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    match toggle_browser_developer_tools_for_control(app, &state, &panel_id) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_show_developer_tools(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let panel = string_param(params, &["panel"]).unwrap_or_else(|| "inspector".to_string());
    let state = app.state::<SessionState>();
    match show_browser_developer_tools_for_control(app, &state, &panel_id, &panel) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_set_zoom(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let Some(zoom) = f64_param(params, &["zoom", "scale"]) else {
        return invalid_params("Missing or invalid browser zoom");
    };
    let state = app.state::<SessionState>();
    match set_browser_zoom_for_control(app, &state, &panel_id, zoom) {
        Ok(snapshot) => surface_list_from_params(&snapshot, params),
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_network_requests(
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
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.network.requests requires a browser surface");
    }
    let query = BrowserNetworkRequestsQuery {
        url_contains: raw_string_param(params, &["url_contains", "urlContains", "url"]),
        method: raw_string_param(params, &["method"]),
        since_id: raw_string_param(params, &["since_id", "sinceId", "after_id", "afterId"]),
        limit: usize_param(params, &["limit"]).map(|value| value.min(200)),
    };
    let state = app.state::<BrowserWebviewState>();
    match browser_network_requests_for_control(state.inner(), &panel_id, query) {
        Ok(reply) => ok(json!(reply)),
        Err(message) => ControlCallResult::Err {
            code: "internal_error".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn browser_network_clear(
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
    if !surface_is_browser(&current, workspace_index, &panel_id) {
        return invalid_params("browser.network.clear requires a browser surface");
    }
    let state = app.state::<BrowserWebviewState>();
    match browser_clear_network_requests_for_control(state.inner(), &panel_id) {
        Ok(reply) => ok(json!(reply)),
        Err(message) => ControlCallResult::Err {
            code: "internal_error".to_string(),
            message,
            data: None,
        },
    }
}
