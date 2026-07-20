use super::*;

fn notification() -> crate::notifications::ControlNotificationSnapshot {
    crate::notifications::ControlNotificationSnapshot {
        id: "550e8400-e29b-41d4-a716-446655440001".into(),
        workspace_id: "550e8400-e29b-41d4-a716-446655440002".into(),
        surface_id: Some("550e8400-e29b-41d4-a716-446655440003".into()),
        title: "Build complete".into(),
        subtitle: "Parity".into(),
        body: "All checks passed".into(),
        created_at: 0,
        is_read: false,
    }
}

#[test]
fn notification_store_events_match_canonical_redaction_and_identity() {
    let notification = notification();
    let created = notification_created_event_spec(&notification, &["replaced-id".into()]);
    assert_eq!(created.name, "notification.created");
    assert_eq!(created.category, "notification");
    assert_eq!(created.source, "notification.store");
    assert_eq!(
        created.workspace_id.as_deref(),
        Some(notification.workspace_id.as_str())
    );
    assert_eq!(created.surface_id, notification.surface_id);
    assert_eq!(
        created.payload,
        json!({
            "notification_id": notification.id,
            "workspace_id": notification.workspace_id,
            "surface_id": notification.surface_id,
            "title": null,
            "title_length": 14,
            "subtitle": null,
            "subtitle_length": 6,
            "body": null,
            "body_length": 17,
            "created_at": "1970-01-01T00:00:00Z",
            "is_read": false,
            "redacted_fields": ["title", "subtitle", "body"],
            "delivery": "store",
            "replaced_notification_ids": ["replaced-id"],
        })
    );

    let removed = notification_removed_event_spec(&notification);
    assert_eq!(removed.name, "notification.removed");
    assert_eq!(removed.source, "notification.store");
    assert!(removed.payload.get("delivery").is_none());
    assert!(removed.payload.get("replaced_notification_ids").is_none());
    assert_eq!(removed.payload["title"], Value::Null);
    assert_eq!(removed.payload["title_length"], 14);
}

#[test]
fn notification_batch_and_request_events_match_canonical_contracts() {
    let notification = notification();
    let ids = vec![notification.id.clone(), "second-id".into()];
    let read = notification_batch_event_spec(
        "notification.read",
        &ids,
        Some(&notification.workspace_id),
        notification.surface_id.as_deref(),
    )
    .expect("non-empty read event");
    assert_eq!(read.source, "notification.store");
    assert_eq!(read.payload, json!({"notification_ids": ids, "count": 2}));
    assert!(notification_batch_event_spec("notification.cleared", &[], None, None).is_none());

    let result = json!({
        "id": notification.id,
        "workspace_id": notification.workspace_id,
        "surface_id": notification.surface_id,
        "dismissed": 1,
    });
    let requested = notification_v2_request_event_spec(
        "notification.dismiss_requested",
        "notification.dismiss",
        json!({"id": notification.id}),
        result.clone(),
        Some(&notification.workspace_id),
        notification.surface_id.as_deref(),
    );
    assert_eq!(requested.source, "socket.v2");
    assert_eq!(
        requested.payload,
        json!({
            "method": "notification.dismiss",
            "params": {"id": notification.id},
            "result": result,
        })
    );

    let legacy = notification_v1_request_event_spec(
        "notification.clear_requested",
        "clear_notifications",
        "",
        None,
    );
    assert_eq!(legacy.source, "socket.v1");
    assert_eq!(
        legacy.payload,
        json!({"command": "clear_notifications", "args": ""})
    );
}
