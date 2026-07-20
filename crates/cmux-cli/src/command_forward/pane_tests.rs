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

#[test]
fn resize_pane_uses_the_ambient_workspace_when_no_scope_is_explicit() {
    let resized = mapped("resize-pane", &[]).with_ambient_workspace_id(Some(" workspace-3 "));

    assert_eq!(resized.method, "pane.resize");
    assert_eq!(
        resized.params,
        serde_json::json!({
            "amount": 1,
            "direction": "right",
            "workspace_id": "workspace-3",
        })
    );
}

#[test]
fn maps_canonical_new_pane_command_and_flags() {
    let created = mapped(
        "new-pane",
        &[
            "--workspace",
            "workspace:2",
            "--window",
            "window:1",
            "--type",
            "browser",
            "--direction",
            "down",
            "--placement",
            "dock",
            "--url",
            "https://example.com/parity-pane",
            "--focus",
            "false",
        ],
    );
    assert_eq!(created.method, "pane.create");
    assert_eq!(
        created.params,
        serde_json::json!({
            "workspace_ref": "workspace:2",
            "window_ref": "window:1",
            "type": "browser",
            "direction": "down",
            "placement": "dock",
            "url": "https://example.com/parity-pane",
            "focus": false,
        })
    );
    assert_eq!(
        mapped("new-pane", &[]).params,
        serde_json::json!({"direction": "right", "focus": false})
    );
    assert_eq!(
        control_command_for("new-pane", &args(&["--focus", "maybe"]))
            .unwrap_err()
            .message,
        "--focus must be true|false"
    );
}

#[test]
fn new_pane_uses_the_ambient_workspace_when_no_scope_is_explicit() {
    let created = mapped("new-pane", &[]).with_ambient_workspace_id(Some(" workspace-2 "));

    assert_eq!(created.method, "pane.create");
    assert_eq!(
        created.params,
        serde_json::json!({
            "direction": "right",
            "focus": false,
            "workspace_id": "workspace-2",
        })
    );
}

#[test]
fn new_pane_help_describes_the_public_canonical_contract() {
    let help = crate::dispatch::subcommand_help_text("new-pane");
    assert_eq!(
        help,
        "cmux new-pane\n\nUsage: cmux new-pane [flags]\n\nCreate a new pane in the workspace.\n\nFlags:\n  --type <terminal|browser>           Pane type (default: terminal)\n  --direction <left|right|up|down>    Split direction (default: right)\n  --placement <workspace|dock>        Target container (default: workspace).\n                                      dock splits the right-sidebar Dock.\n  --workspace <id|ref|index>          Target workspace (default: $CMUX_WORKSPACE_ID)\n  --window <id|ref|index>             Window context for workspace refs and indexes\n  --url <url>                         URL for browser panes\n  --focus <true|false>                Focus the new pane (default: false)\n\nExample:\n  cmux new-pane\n  cmux new-pane --type browser --direction down --url https://example.com\n  cmux new-pane --type browser --placement dock --url https://example.com"
    );
    for expected in [
        "Usage: cmux new-pane [flags]",
        "--type <terminal|browser>",
        "--placement <workspace|dock>",
        "--workspace <id|ref|index>",
        "--window <id|ref|index>",
        "--url <url>",
        "--focus <true|false>",
    ] {
        assert!(help.contains(expected), "missing {expected:?} in {help:?}");
    }
    assert!(!help.contains("--panel"));
}

#[test]
fn resize_pane_help_matches_the_canonical_contract() {
    assert_eq!(
        crate::dispatch::subcommand_help_text("resize-pane"),
        "cmux resize-pane\n\nUsage: cmux resize-pane [--pane <id|ref|index>] [--workspace <id|ref|index>] [--window <id|ref|index>] [-L|-R|-U|-D] [--amount <n>]\n\ntmux-compatible pane resize command.\n\nFlags:\n  --pane <id|ref|index>        Pane to resize (default: focused pane)\n  --workspace <id|ref|index>   Workspace context (default: $CMUX_WORKSPACE_ID)\n  --window <id|ref|index>      Window context for workspace/pane refs and indexes\n  -L|-R|-U|-D            Direction (default: -R)\n  --amount <n>           Resize amount (default: 1)"
    );
}
