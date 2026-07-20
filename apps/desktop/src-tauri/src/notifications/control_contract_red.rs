#[test]
fn dismissing_an_unknown_control_notification_is_not_found() {
    let state = NotificationCommandState::default();

    let error = notification_dismiss_for_control(&state, Some("missing"), false)
        .expect_err("canonical rejects unknown notification ids");

    assert_eq!(error, "Notification not found");
}

#[test]
fn marking_an_unknown_control_notification_read_is_not_found() {
    let state = NotificationCommandState::default();

    let error = notification_mark_read_for_control(&state, Some("missing"), None, None, false)
        .expect_err("canonical rejects unknown notification ids");

    assert_eq!(error, "Notification not found");
}
