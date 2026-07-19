use super::events::{record_workspace_events, workspace_close_event_specs};
use super::*;

pub(in crate::control_socket) fn workspace_close(
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
    let window = &current.windows[window_index];
    let workspace = &window.tab_manager.workspaces[index];
    let workspace_id = workspace.workspace_id.clone().unwrap_or_default();
    let identity = workspace_identity_payload(app, window, &workspace_id);
    if workspace.is_pinned == Some(true) {
        let mut data = identity;
        data["pinned"] = json!(true);
        return ControlCallResult::Err {
            code: "protected".to_string(),
            message: "Pinned workspaces can't be closed while pinned. Unpin the workspace first."
                .to_string(),
            data: data.try_into().ok(),
        };
    }
    let events = workspace_close_event_specs(&current, window_index, index).unwrap_or_default();
    let state = app.state::<SessionState>();
    let Some((_snapshot, changed)) = close_workspace_in_window_for_control(
        app,
        &state,
        window_index,
        index,
        DerivedEventPolicy::Suppress,
    ) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    if changed {
        record_workspace_events(app, events);
    }
    ok(identity)
}
