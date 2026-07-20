use super::*;

pub(in crate::control_socket) fn notification_created_at(timestamp: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(timestamp).map_or_else(
        |_| timestamp.to_string(),
        |value| {
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
                value.year(),
                u8::from(value.month()),
                value.day(),
                value.hour(),
                value.minute(),
                value.second()
            )
        },
    )
}

fn redacted_notification_payload(
    notification: &crate::notifications::ControlNotificationSnapshot,
) -> serde_json::Map<String, Value> {
    serde_json::Map::from_iter([
        ("notification_id".into(), json!(notification.id)),
        ("workspace_id".into(), json!(notification.workspace_id)),
        ("surface_id".into(), json!(notification.surface_id)),
        ("title".into(), Value::Null),
        (
            "title_length".into(),
            json!(notification.title.chars().count()),
        ),
        ("subtitle".into(), Value::Null),
        (
            "subtitle_length".into(),
            json!(notification.subtitle.chars().count()),
        ),
        ("body".into(), Value::Null),
        (
            "body_length".into(),
            json!(notification.body.chars().count()),
        ),
        (
            "created_at".into(),
            json!(notification_created_at(notification.created_at)),
        ),
        ("is_read".into(), json!(notification.is_read)),
        (
            "redacted_fields".into(),
            json!(["title", "subtitle", "body"]),
        ),
    ])
}

fn notification_store_event_spec(
    name: &'static str,
    notification: &crate::notifications::ControlNotificationSnapshot,
    payload: serde_json::Map<String, Value>,
) -> DerivedEventSpec {
    DerivedEventSpec {
        name,
        category: "notification",
        source: "notification.store",
        window_id: None,
        workspace_id: Some(notification.workspace_id.clone()),
        surface_id: notification.surface_id.clone(),
        payload: Value::Object(payload),
    }
}

pub(in crate::control_socket) fn notification_created_event_spec(
    notification: &crate::notifications::ControlNotificationSnapshot,
    replaced_notification_ids: &[String],
) -> DerivedEventSpec {
    let mut payload = redacted_notification_payload(notification);
    payload.insert("delivery".into(), json!("store"));
    payload.insert(
        "replaced_notification_ids".into(),
        json!(replaced_notification_ids),
    );
    notification_store_event_spec("notification.created", notification, payload)
}

pub(in crate::control_socket) fn notification_removed_event_spec(
    notification: &crate::notifications::ControlNotificationSnapshot,
) -> DerivedEventSpec {
    notification_store_event_spec(
        "notification.removed",
        notification,
        redacted_notification_payload(notification),
    )
}

pub(in crate::control_socket) fn notification_batch_event_spec(
    name: &'static str,
    notification_ids: &[String],
    workspace_id: Option<&str>,
    surface_id: Option<&str>,
) -> Option<DerivedEventSpec> {
    (!notification_ids.is_empty()).then(|| DerivedEventSpec {
        name,
        category: "notification",
        source: "notification.store",
        window_id: None,
        workspace_id: workspace_id.map(str::to_owned),
        surface_id: surface_id.map(str::to_owned),
        payload: json!({
            "notification_ids": notification_ids,
            "count": notification_ids.len(),
        }),
    })
}

pub(in crate::control_socket) fn notification_v2_request_event_spec(
    name: &'static str,
    method: &'static str,
    params: Value,
    result: Value,
    workspace_id: Option<&str>,
    surface_id: Option<&str>,
) -> DerivedEventSpec {
    DerivedEventSpec {
        name,
        category: "notification",
        source: "socket.v2",
        window_id: None,
        workspace_id: workspace_id.map(str::to_owned),
        surface_id: surface_id.map(str::to_owned),
        payload: json!({"method": method, "params": params, "result": result}),
    }
}

pub(in crate::control_socket) fn notification_v1_request_event_spec(
    name: &'static str,
    command: &'static str,
    args: &str,
    workspace_id: Option<&str>,
) -> DerivedEventSpec {
    DerivedEventSpec {
        name,
        category: "notification",
        source: "socket.v1",
        window_id: None,
        workspace_id: workspace_id.map(str::to_owned),
        surface_id: None,
        payload: json!({"command": command, "args": args}),
    }
}

pub(in crate::control_socket) fn record_notification_events(
    app: &AppHandle,
    events: Vec<DerivedEventSpec>,
) {
    record_workspace_events(app, events);
}

fn selected_workspace_and_surface(
    snapshot: &AppSessionSnapshot,
) -> (Option<String>, Option<String>) {
    let Some(window) = snapshot.windows.first() else {
        return (None, None);
    };
    let workspace = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| window.tab_manager.workspaces.get(index));
    (
        workspace.and_then(|workspace| workspace.workspace_id.clone()),
        workspace.and_then(|workspace| workspace.focused_panel_id.clone()),
    )
}

pub(in crate::control_socket) fn notification_navigation_event_specs(
    previous: &AppSessionSnapshot,
    current: &AppSessionSnapshot,
    workspace_id: &str,
    surface_id: &str,
) -> Vec<DerivedEventSpec> {
    let (previous_workspace_id, previous_surface_id) = selected_workspace_and_surface(previous);
    let Some(window) = current.windows.first() else {
        return Vec::new();
    };
    let Some(workspace_index) = workspace_index_for_id(current, workspace_id) else {
        return Vec::new();
    };
    let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
        return Vec::new();
    };
    let Some(surface) = surfaces_for_workspace(workspace)
        .into_iter()
        .find(|surface| surface.get("id").and_then(Value::as_str) == Some(surface_id))
    else {
        return Vec::new();
    };
    let Some(pane_id) = surface.get("pane_id").and_then(Value::as_str) else {
        return Vec::new();
    };
    let Some(kind) = surface.get("type").and_then(Value::as_str) else {
        return Vec::new();
    };

    let mut events = Vec::new();
    if previous_workspace_id.as_deref() != Some(workspace_id) {
        if let Some(event) = workspace_selected_event_spec(
            current,
            0,
            workspace_index,
            previous_workspace_id.as_deref(),
        ) {
            events.push(event);
        }
    }
    if previous_surface_id.as_deref() != Some(surface_id) {
        events.extend([
            DerivedEventSpec {
                name: "surface.selected",
                category: "surface",
                source: "workspace.lifecycle",
                window_id: None,
                workspace_id: Some(workspace_id.to_owned()),
                surface_id: Some(surface_id.to_owned()),
                payload: json!({
                    "surface_id": surface_id,
                    "pane_id": pane_id,
                    "kind": kind,
                    "focused": true,
                    "previous_surface_id": previous_surface_id,
                    "origin": "bonsplit_selection",
                }),
            },
            DerivedEventSpec {
                name: "surface.focused",
                category: "surface",
                source: "workspace.lifecycle",
                window_id: None,
                workspace_id: Some(workspace_id.to_owned()),
                surface_id: Some(surface_id.to_owned()),
                payload: json!({
                    "surface_id": surface_id,
                    "pane_id": pane_id,
                    "kind": kind,
                    "origin": "bonsplit_selection",
                }),
            },
        ]);
    }
    events
}
