#[test]
fn maps_canonical_last_window_command() {
    let command = mapped("last-window", &["--window", "2"]);
    assert_eq!(command.method, "workspace.last");
    assert_eq!(command.params, serde_json::json!({"window_ref":"window:2"}));
}

#[test]
fn maps_canonical_adjacent_window_commands() {
    let next = mapped("next-window", &["--window", "2"]);
    assert_eq!(next.method, "workspace.next");
    assert_eq!(next.params, serde_json::json!({"window_ref":"window:2"}));

    let previous = mapped("previous-window", &["--window", "2"]);
    assert_eq!(previous.method, "workspace.previous");
    assert_eq!(
        previous.params,
        serde_json::json!({"window_ref":"window:2"})
    );
}
