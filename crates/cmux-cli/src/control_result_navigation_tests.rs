#[test]
fn last_window_keeps_canonical_handle_summary() {
    assert_eq!(
        format_control_result(
            "workspace.last",
            &serde_json::json!({"workspace_ref":"workspace:2"}),
        ),
        "OK workspace:2"
    );
}

#[test]
fn adjacent_window_commands_keep_canonical_handle_summary() {
    let result = serde_json::json!({"workspace_ref":"workspace:2"});
    for method in ["workspace.next", "workspace.previous"] {
        assert_eq!(format_control_result(method, &result), "OK workspace:2");
    }
}
