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

#[test]
fn focus_panel_normalizes_a_raw_panel_handle_to_surface_id() {
    let command = mapped(
        "focus-panel",
        &[
            "--panel",
            "4dc88e7e-402e-472e-b699-8a18aa011633",
            "--workspace",
            "workspace:2",
            "--window",
            "window:3",
        ],
    );

    assert_eq!(command.method, "surface.focus");
    assert_eq!(
        command.params,
        serde_json::json!({
                "__cmux_cli_command": "focus-panel",
                "surface_id": "4dc88e7e-402e-472e-b699-8a18aa011633",
                "workspace_ref": "workspace:2",
                "window_ref": "window:3",
            })
    );
}
