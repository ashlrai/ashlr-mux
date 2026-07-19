use super::events::record_workspace_rename_event;
use super::*;

pub(in crate::control_socket) fn workspace_rename(
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
    let Some(title) = string_param(params, &["title"]) else {
        return invalid_params("Missing or invalid title");
    };
    let Some(index) = canonical_workspace_target_index(&current, window_index, params) else {
        return workspace_not_found(app, params);
    };
    let workspace_id = current.windows[window_index].tab_manager.workspaces[index]
        .workspace_id
        .clone()
        .unwrap_or_default();
    let state = app.state::<SessionState>();
    let rename_result = rename_workspace_in_window_for_control(
        app,
        &state,
        window_index,
        index,
        &title,
        DerivedEventPolicy::Suppress,
    );
    let Some(result) = (match rename_result {
        Ok(result) => result,
        Err(message) => {
            return ControlCallResult::Err {
                code: "remote_rename_failed".to_string(),
                message,
                data: None,
            }
        }
    }) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let mut payload = workspace_identity_payload(app, &result.windows[window_index], &workspace_id);
    payload["title"] = json!(title);
    record_workspace_rename_event(app, params, &payload);
    ok(payload)
}
