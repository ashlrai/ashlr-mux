#[test]
fn maps_canonical_swap_pane_command() {
    let swap = mapped(
        "swap-pane",
        &[
            "--pane",
            "pane:1",
            "--target-pane",
            "2",
            "--workspace",
            "workspace:3",
            "--window",
            "window:1",
            "--focus",
            "true",
        ],
    );
    assert_eq!(swap.method, "pane.swap");
    assert_eq!(
        swap.params,
        serde_json::json!({
            "pane_ref": "pane:1",
            "target_pane_ref": "pane:2",
            "workspace_ref": "workspace:3",
            "window_ref": "window:1",
            "focus": true,
        })
    );
    assert_eq!(
        control_command_for("swap-pane", &args(&["--target-pane", "pane:2"]))
            .unwrap_err()
            .message,
        "swap-pane requires --pane"
    );
}

#[test]
fn maps_canonical_break_pane_command_and_focus_alias() {
    let broken = mapped(
        "break-pane",
        &[
            "--pane",
            "2",
            "--surface",
            "surface:3",
            "--workspace",
            "workspace:1",
            "--window",
            "window:2",
            "--no-focus",
        ],
    );
    assert_eq!(broken.method, "pane.break");
    assert_eq!(
        broken.params,
        serde_json::json!({
            "pane_ref": "pane:2",
            "surface_ref": "surface:3",
            "workspace_ref": "workspace:1",
            "window_ref": "window:2",
            "focus": false,
        })
    );
    assert_eq!(
        mapped("break-pane", &[]).params,
        serde_json::json!({"focus": false})
    );
    assert_eq!(
        control_command_for("break-pane", &args(&["--focus", "true", "--no-focus"]),)
            .unwrap_err()
            .message,
        "--focus and --no-focus cannot be used together"
    );
}

#[test]
fn maps_canonical_join_pane_command_and_requires_target() {
    let joined = mapped(
        "join-pane",
        &[
            "--target-pane",
            "pane:3",
            "--pane",
            "2",
            "--surface",
            "surface-id",
            "--workspace",
            "1",
            "--focus",
            "true",
        ],
    );
    assert_eq!(joined.method, "pane.join");
    assert_eq!(
        joined.params,
        serde_json::json!({
            "target_pane_ref": "pane:3",
            "pane_ref": "pane:2",
            "surface_id": "surface-id",
            "workspace_ref": "workspace:1",
            "focus": true,
        })
    );
    assert_eq!(
        control_command_for("join-pane", &args(&[]))
            .unwrap_err()
            .message,
        "join-pane requires --target-pane"
    );
}

#[test]
fn maps_canonical_last_pane_command() {
    let last = mapped("last-pane", &["--workspace", "2", "--window", "window:1"]);
    assert_eq!(last.method, "pane.last");
    assert_eq!(
        last.params,
        serde_json::json!({
            "__cmux_cli_command": "last-pane",
            "workspace_ref": "workspace:2",
            "window_ref": "window:1",
        })
    );
}

#[test]
fn maps_canonical_resize_pane_command_and_direction_precedence() {
    let resized = mapped(
        "resize-pane",
        &[
            "--pane",
            "2",
            "--workspace",
            "workspace:3",
            "--window",
            "window:1",
            "-D",
            "-L",
            "--amount",
            "12",
        ],
    );
    assert_eq!(resized.method, "pane.resize");
    assert_eq!(
        resized.params,
        serde_json::json!({
            "pane_ref": "pane:2",
            "workspace_ref": "workspace:3",
            "window_ref": "window:1",
            "direction": "left",
            "amount": 12,
        })
    );
    assert_eq!(
        mapped("resize-pane", &["--amount", "not-a-number"]).params,
        serde_json::json!({"direction": "right", "amount": 1})
    );
    assert_eq!(
        control_command_for("resize-pane", &args(&["--amount", "0"]))
            .unwrap_err()
            .message,
        "--amount must be greater than 0"
    );
}
