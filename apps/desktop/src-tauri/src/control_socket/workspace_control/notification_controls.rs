use super::*;

#[path = "notification_controls/mutations.rs"]
mod mutations;
pub(in crate::control_socket) use mutations::{
    notification_clear, notification_create, notification_dismiss, notification_mark_read,
};

#[path = "notification_controls/navigation.rs"]
mod navigation;
pub(in crate::control_socket) use navigation::{notification_jump_to_unread, notification_open};

fn notification_payload(
    app: &AppHandle,
    current: &AppSessionSnapshot,
    notification: &crate::notifications::ControlNotificationSnapshot,
    opened: Option<bool>,
) -> Value {
    let workspace_ref_value = control_handle_ref(app, "workspace", &notification.workspace_id);
    let surface_ref_value = notification
        .surface_id
        .as_deref()
        .map(|surface_id| control_handle_ref(app, "surface", surface_id));
    let tab_title = current
        .windows
        .iter()
        .flat_map(|window| &window.tab_manager.workspaces)
        .find(|workspace| workspace.workspace_id.as_deref() == Some(&notification.workspace_id))
        .map(workspace_display_name);
    let mut payload = json!({
        "id": notification.id,
        "workspace_id": notification.workspace_id,
        "workspace_ref": workspace_ref_value,
        "surface_id": notification.surface_id,
        "surface_ref": surface_ref_value,
        "title": notification.title,
        "subtitle": notification.subtitle,
        "body": notification.body,
        "created_at": notification_created_at(notification.created_at),
        "tab_title": tab_title,
        "is_read": notification.is_read,
    });
    if let (Some(opened), Some(payload)) = (opened, payload.as_object_mut()) {
        payload.insert("opened".to_owned(), json!(opened));
    }
    payload
}

fn notification_public_params(params: &serde_json::Map<String, Value>) -> Value {
    Value::Object(
        params
            .iter()
            .filter(|(key, _)| !key.starts_with("__"))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )
}

fn redacted_notification_request_params(params: &serde_json::Map<String, Value>) -> Value {
    let Value::Object(mut redacted) = notification_public_params(params) else {
        unreachable!("notification params are an object")
    };
    let mut redacted_fields = Vec::new();
    for key in ["title", "subtitle", "body"] {
        let Some(text) = redacted.get(key).and_then(Value::as_str) else {
            continue;
        };
        let length = text.chars().count();
        redacted.insert(key.to_owned(), Value::Null);
        redacted.insert(format!("{key}_length"), json!(length));
        redacted_fields.push(key);
    }
    if !redacted_fields.is_empty() {
        redacted.insert("redacted_fields".into(), json!(redacted_fields));
    }
    Value::Object(redacted)
}

fn notification_request_ids<'a>(
    params: &'a Value,
    result: &'a Value,
) -> (Option<&'a str>, Option<&'a str>) {
    (
        result
            .get("workspace_id")
            .or_else(|| params.get("workspace_id"))
            .and_then(Value::as_str),
        result
            .get("surface_id")
            .or_else(|| params.get("surface_id"))
            .and_then(Value::as_str),
    )
}

fn notification_workspace_is_selected(current: &AppSessionSnapshot, workspace_id: &str) -> bool {
    let Some(window) = current.windows.first() else {
        return false;
    };
    window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| window.tab_manager.workspaces.get(index))
        .and_then(|workspace| workspace.workspace_id.as_deref())
        == Some(workspace_id)
}

fn record_notification_v2_request(
    app: &AppHandle,
    name: &'static str,
    method: &'static str,
    params: Value,
    result: &Value,
) {
    let (workspace_id, surface_id) = notification_request_ids(&params, result);
    let workspace_id = workspace_id.map(str::to_owned);
    let surface_id = surface_id.map(str::to_owned);
    record_notification_events(
        app,
        vec![notification_v2_request_event_spec(
            name,
            method,
            params,
            result.clone(),
            workspace_id.as_deref(),
            surface_id.as_deref(),
        )],
    );
}

fn notification_error_data(value: Value) -> Option<JsonValue> {
    value.try_into().ok()
}

fn notification_uuid_param(
    params: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    raw_string_param(params, keys)
        .and_then(|value| Uuid::parse_str(&value).ok())
        .map(|id| id.to_string())
}

pub(in crate::control_socket) fn notification_list(app: &AppHandle) -> ControlCallResult {
    let current = snapshot(app);
    let state = app.state::<crate::notifications::NotificationCommandState>();
    match crate::notifications::notification_list_for_control(state.inner()) {
        Ok(notifications) => ok(json!({
            "notifications": notifications
                .iter()
                .map(|notification| notification_payload(app, &current, notification, None))
                .collect::<Vec<_>>()
        })),
        Err(message) => ControlCallResult::Err {
            code: "notification_store_failed".to_string(),
            message,
            data: None,
        },
    }
}
