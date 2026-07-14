#![cfg(windows)]

use cmux_cli::control_command_for;
use serde_json::{json, Value};
use std::process::{Command, Output};

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

fn mapped(command: &str, args: &[&str]) -> (String, Value) {
    let command = control_command_for(command, &strings(args))
        .unwrap()
        .expect("workspace-group command must map to the control socket");
    (command.method, command.params)
}

fn executable(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(args)
        .env_remove("CMUX_SOCKET")
        .env_remove("CMUX_SOCKET_PASSWORD")
        .env_remove("CMUX_WORKSPACE_ID")
        .env_remove("CMUX_SURFACE_ID")
        .env_remove("CMUX_TAB_ID")
        .env_remove("CMUX_WINDOW_ID")
        .env("CMUX_SOCKET_PATH", r"\\.\pipe\cmux-workspace-group-red")
        .output()
        .unwrap()
}

fn assert_failure(args: &[&str], expected_stderr: &str) {
    let output = executable(args);
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected result: {output:?}"
    );
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8(output.stderr).unwrap(), expected_stderr);
}

#[test]
fn workspace_group_maps_all_seventeen_canonical_methods() {
    let cases = [
        (vec!["list"], "workspace.group.list"),
        (vec!["create", "Build"], "workspace.group.create"),
        (
            vec!["ungroup", "workspace_group:1"],
            "workspace.group.ungroup",
        ),
        (
            vec!["delete", "workspace_group:1"],
            "workspace.group.delete",
        ),
        (
            vec!["rename", "workspace_group:1", "Backend"],
            "workspace.group.rename",
        ),
        (
            vec!["collapse", "workspace_group:1"],
            "workspace.group.collapse",
        ),
        (
            vec!["expand", "workspace_group:1"],
            "workspace.group.expand",
        ),
        (vec!["pin", "workspace_group:1"], "workspace.group.pin"),
        (vec!["unpin", "workspace_group:1"], "workspace.group.unpin"),
        (
            vec![
                "add",
                "--group",
                "workspace_group:1",
                "--workspace",
                "workspace:2",
            ],
            "workspace.group.add",
        ),
        (vec!["remove", "workspace:2"], "workspace.group.remove"),
        (
            vec![
                "set-anchor",
                "--group",
                "workspace_group:1",
                "--workspace",
                "workspace:2",
            ],
            "workspace.group.set_anchor",
        ),
        (
            vec!["new-workspace", "workspace_group:1"],
            "workspace.group.new_workspace",
        ),
        (
            vec!["set-color", "workspace_group:1"],
            "workspace.group.set_color",
        ),
        (
            vec!["set-icon", "workspace_group:1"],
            "workspace.group.set_icon",
        ),
        (
            vec!["move", "workspace_group:1", "--to-index", "2"],
            "workspace.group.move",
        ),
        (vec!["focus", "workspace_group:1"], "workspace.group.focus"),
    ];

    for (args, expected_method) in cases {
        assert_eq!(
            mapped("workspace-group", &args).0,
            expected_method,
            "{args:?}"
        );
    }
}

#[test]
fn workspace_group_parser_pins_canonical_parameter_shapes() {
    assert_eq!(
        mapped(
            "workspace-group",
            &[
                "create",
                "ignored",
                "--name",
                "Backend",
                "--cwd",
                "C:/repo",
                "--from",
                " workspace:2,workspace:3 ",
                "--window",
                "window:2",
            ],
        ),
        (
            "workspace.group.create".into(),
            json!({
                "name":"Backend",
                "cwd":"C:/repo",
                "child_workspace_ids":["workspace:2", "workspace:3"],
                "window_ref":"window:2",
            }),
        )
    );
    let relative = mapped(
        "workspace-group",
        &["create", "Build", "--cwd", "relative/repo"],
    );
    assert_eq!(
        relative.1["cwd"],
        json!(std::env::current_dir()
            .unwrap()
            .join("relative/repo")
            .to_string_lossy())
    );
    assert_eq!(
        mapped(
            "workspace-group",
            &[
                "rename",
                "ignored",
                "--group",
                "workspace_group:2",
                "--name",
                "API"
            ],
        ),
        (
            "workspace.group.rename".into(),
            json!({"group_id":"workspace_group:2", "name":"API"}),
        )
    );
    assert_eq!(
        mapped("workspace-group", &["set-color", "workspace_group:1"]),
        (
            "workspace.group.set_color".into(),
            json!({"group_id":"workspace_group:1", "hex":""})
        )
    );
    assert_eq!(
        mapped("workspace-group", &["set-icon", "workspace_group:1"]),
        (
            "workspace.group.set_icon".into(),
            json!({"group_id":"workspace_group:1", "symbol":""})
        )
    );
    assert_eq!(
        mapped(
            "workspace-group",
            &[
                "move",
                "workspace_group:1",
                "--to-index",
                "3",
                "--before",
                "workspace_group:2",
                "--after",
                "workspace_group:3"
            ],
        ),
        (
            "workspace.group.move".into(),
            json!({"group_id":"workspace_group:1", "to_index":3})
        )
    );
}

#[test]
fn nested_workspace_group_spelling_uses_the_same_canonical_parser() {
    for args in [
        vec!["group", "list"],
        vec!["group", "rename", "workspace_group:1", "--name", "Ops"],
        vec!["group", "focus", "workspace_group:1"],
    ] {
        assert_eq!(
            mapped("workspace", &args),
            mapped("workspace-group", &args[1..]),
            "nested spelling drifted for {args:?}"
        );
    }
}

#[test]
fn workspace_group_has_exact_canonical_local_errors() {
    for (args, message) in [
        (
            vec!["workspace-group"],
            "Error: workspace-group requires a subcommand. Try: list, create, ungroup, delete, rename, collapse, expand, pin, unpin, add, remove, set-anchor, new-workspace, set-color, set-icon, move, focus\n",
        ),
        (
            vec!["workspace-group", "collapse"],
            "Error: workspace-group collapse requires a group id or --group <id>\n",
        ),
        (
            vec!["workspace-group", "rename", "workspace_group:1"],
            "Error: rename requires --name <name>\n",
        ),
        (
            vec!["workspace-group", "add", "--group", "workspace_group:1"],
            "Error: add requires --group <id> --workspace <id>\n",
        ),
        (
            vec!["workspace-group", "move", "workspace_group:1", "--to-index", "wat"],
            "Error: move --to-index must be an integer\n",
        ),
        (
            vec!["workspace-group", "wat"],
            "Error: Unknown workspace-group subcommand: wat\n",
        ),
    ] {
        assert_failure(&args, message);
    }
}

#[test]
fn workspace_group_help_matches_the_frozen_canonical_surface() {
    let direct = executable(&["workspace-group", "--help"]);
    let nested = executable(&["help", "workspace-group"]);
    assert!(direct.status.success());
    assert!(nested.status.success());
    assert_eq!(direct.stdout, nested.stdout);
    let help = String::from_utf8(direct.stdout).unwrap();
    for required in [
        "Usage: cmux workspace-group <subcommand> [flags]",
        "ungroup <group>",
        "delete <group>",
        "new-workspace <group> [--placement afterCurrent|top|end]",
        "move <group> --to-index <n> | --before <group> | --after <group>",
        "<group> accepts a UUID or a workspace_group:N ref",
    ] {
        assert!(help.contains(required), "missing help contract: {required}");
    }
}
