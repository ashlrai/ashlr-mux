#![cfg(windows)]

use std::collections::HashMap;
use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use cmux_cli::control_command_for;
use cmux_ipc::{control_pipe_path, serve_named_pipe, ControlCallResult, JsonValue};
use serde_json::{json, Value};

type CapturedRequest = (String, serde_json::Map<String, Value>);

const WORKSPACE_ID: &str = "11111111-1111-4111-8111-111111111111";
const SURFACE_ID: &str = "22222222-2222-4222-8222-222222222222";
const OTHER_SURFACE_ID: &str = "33333333-3333-4333-8333-333333333333";
const WINDOW_ID: &str = "44444444-4444-4444-8444-444444444444";

fn spawn_server(tag: &str, result: ControlCallResult) -> (String, mpsc::Receiver<CapturedRequest>) {
    let pipe = control_pipe_path(&format!(
        "cmux-{tag}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
    .unwrap();
    let (request_tx, request_rx) = mpsc::channel();
    let server_pipe = pipe.clone();
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let _ = serve_named_pipe(&server_pipe, move || {
                    let request_tx = request_tx.clone();
                    let result = result.clone();
                    move |request: cmux_ipc::ControlRequest| {
                        request_tx.send((request.method, request.params)).unwrap();
                        result.clone()
                    }
                })
                .await;
            });
    });
    (pipe, request_rx)
}

fn spawn_method_server(
    tag: &str,
    responses: HashMap<String, Value>,
) -> (String, mpsc::Receiver<CapturedRequest>) {
    let pipe = control_pipe_path(&format!(
        "cmux-{tag}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
    .unwrap();
    let (request_tx, request_rx) = mpsc::channel();
    let server_pipe = pipe.clone();
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let _ = serve_named_pipe(&server_pipe, move || {
                    let request_tx = request_tx.clone();
                    let responses = responses.clone();
                    move |request: cmux_ipc::ControlRequest| {
                        let method = request.method.clone();
                        request_tx.send((request.method, request.params)).unwrap();
                        ok(responses.get(&method).cloned().unwrap_or(Value::Null))
                    }
                })
                .await;
            });
    });
    (pipe, request_rx)
}

fn ok(value: Value) -> ControlCallResult {
    ControlCallResult::Ok(JsonValue::try_from(value).unwrap())
}

fn executable(pipe: Option<&str>, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cmux"));
    command.args(args);
    command
        .env_remove("CMUX_SOCKET")
        .env_remove("CMUX_SOCKET_PASSWORD")
        .env_remove("CMUX_WORKSPACE_ID")
        .env_remove("CMUX_SURFACE_ID")
        .env_remove("CMUX_TAB_ID")
        .env_remove("CMUX_WINDOW_ID");
    if let Some(pipe) = pipe {
        command.env("CMUX_SOCKET_PATH", pipe);
    } else {
        command.env("CMUX_SOCKET_PATH", r"\\.\pipe\cmux-must-not-connect");
    }
    command.output().unwrap()
}

fn assert_failure(output: Output, expected_stderr: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8(output.stderr).unwrap(), expected_stderr);
}

fn assert_native_windows_command(command: &str) {
    let lower = command.to_ascii_lowercase();
    assert!(
        lower.contains("cmd.exe") || lower.contains("powershell") || lower.contains("pwsh"),
        "expected a native Windows shell command, got {command:?}"
    );
    assert!(!lower.contains("/bin/sh"), "{command}");
}

#[test]
fn tab_action_parser_maps_action_selectors_title_url_and_focus() {
    let mapped = control_command_for(
        "tab-action",
        &[
            "--action".into(),
            " Toggle-Full-Width-Tab ".into(),
            "--tab".into(),
            SURFACE_ID.into(),
            "--surface".into(),
            OTHER_SURFACE_ID.into(),
            "--workspace".into(),
            WORKSPACE_ID.into(),
            "--window".into(),
            WINDOW_ID.into(),
            "--url".into(),
            " https://example.test/a b ".into(),
            "--focus".into(),
            "true".into(),
            " trailing ".into(),
            " title ".into(),
        ],
    )
    .unwrap()
    .expect("tab-action must be mapped");

    assert_eq!(mapped.method, "tab.action");
    assert_eq!(
        mapped.params,
        json!({
            "action": "toggle_full_width_tab",
            "surface_id": SURFACE_ID,
            "workspace_id": WORKSPACE_ID,
            "window_id": WINDOW_ID,
            "url": "https://example.test/a b",
            "title": "trailing title",
            "focus": true,
        })
    );
}

#[test]
fn tab_action_normalizes_only_case_and_hyphens_while_accepting_unknown_actions() {
    let mapped = control_command_for(
        "tab-action",
        &["--action".into(), "  Custom Action-Name  ".into()],
    )
    .unwrap()
    .expect("tab-action must be mapped");

    assert_eq!(mapped.params["action"], "custom action_name");
}

#[test]
fn respawn_pane_parser_preserves_trailing_command_text_and_builds_native_wrapper() {
    let mapped = control_command_for(
        "respawn-pane",
        &[
            "--workspace".into(),
            WORKSPACE_ID.into(),
            "--surface".into(),
            SURFACE_ID.into(),
            "--window".into(),
            WINDOW_ID.into(),
            "--".into(),
            "echo".into(),
            "two words".into(),
            "&&".into(),
            "echo".into(),
            "done".into(),
        ],
    )
    .unwrap()
    .expect("respawn-pane must be mapped");

    assert_eq!(mapped.method, "surface.respawn");
    assert_eq!(mapped.params["workspace_id"], WORKSPACE_ID);
    assert_eq!(mapped.params["surface_id"], SURFACE_ID);
    assert_eq!(mapped.params["window_id"], WINDOW_ID);
    assert_eq!(
        mapped.params["tmux_start_command"],
        "echo two words && echo done"
    );
    let wrapper = mapped.params["command"].as_str().expect("native wrapper");
    assert_ne!(wrapper, "echo two words && echo done");
    assert_native_windows_command(wrapper);
    assert!(wrapper.contains("echo two words && echo done"), "{wrapper}");
}

#[test]
fn respawn_wrapper_survives_powershell_terminal_launch_and_cmd_metacharacters() {
    let command_text = r#"powershell.exe -NoLogo -NoProfile -NonInteractive -Command "[Console]::Write('space \"quote\" & value')""#;
    let mapped = control_command_for("respawn-pane", &["--command".into(), command_text.into()])
        .unwrap()
        .expect("respawn-pane must be mapped");
    assert_eq!(mapped.params["tmux_start_command"], command_text);

    let wrapper = mapped.params["command"].as_str().unwrap();
    let output = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            wrapper,
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={} wrapper={wrapper:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "space \"quote\" & value"
    );
}

#[test]
fn tab_action_normalizes_workspace_indexes_and_preserves_ref_uuid_precedence() {
    let (pipe, request_rx) = spawn_method_server(
        "tab-action-workspace-index",
        HashMap::from([
            (
                "workspace.list".into(),
                json!({"workspaces":[{"index":3,"id":WORKSPACE_ID,"ref":"workspace:4"}]}),
            ),
            ("tab.action".into(), json!({"action":"pin"})),
        ]),
    );
    let output = executable(Some(&pipe), &["tab-action", "pin", "--workspace", "3"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        request_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        ("workspace.list".into(), serde_json::Map::new())
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "tab.action");
    assert_eq!(
        Value::Object(params),
        json!({"action":"pin","workspace_id":WORKSPACE_ID,"focus":false})
    );

    for (tag, selector, expected) in [
        ("ref", "workspace:4", "workspace:4"),
        ("uuid", WORKSPACE_ID, WORKSPACE_ID),
    ] {
        let (pipe, request_rx) = spawn_server(tag, ok(json!({"action":"pin"})));
        let output = executable(Some(&pipe), &["tab-action", "pin", "--workspace", selector]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(method, "tab.action");
        assert_eq!(params.get("workspace_id"), Some(&json!(expected)));
        assert!(params.get("workspace_ref").is_none());
        assert!(request_rx.try_recv().is_err());
    }
}

#[test]
fn tab_action_normalizes_numeric_and_tab_ref_surfaces_in_scope() {
    let mapped = control_command_for(
        "tab-action",
        &["pin".into(), "--tab".into(), "tab:9".into()],
    )
    .unwrap()
    .expect("tab-action must be mapped");
    assert_eq!(mapped.params["surface_ref"], "surface:9");

    let (pipe, request_rx) = spawn_method_server(
        "tab-action-surface-index",
        HashMap::from([
            (
                "surface.list".into(),
                json!({"surfaces":[{"index":2,"id":SURFACE_ID,"ref":"surface:9"}]}),
            ),
            ("tab.action".into(), json!({"action":"pin"})),
        ]),
    );
    let output = executable(
        Some(&pipe),
        &[
            "tab-action",
            "pin",
            "--workspace",
            WORKSPACE_ID,
            "--tab",
            "2",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.list");
    assert_eq!(Value::Object(params), json!({"workspace_id":WORKSPACE_ID}));
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "tab.action");
    assert_eq!(
        Value::Object(params),
        json!({
            "action":"pin", "workspace_id":WORKSPACE_ID,
            "surface_id":SURFACE_ID, "focus":false
        })
    );
}

#[test]
fn explicit_window_validates_surface_refs_before_tab_action() {
    let (pipe, request_rx) = spawn_method_server(
        "tab-action-window-surface-ref",
        HashMap::from([
            (
                "surface.list".into(),
                json!({"surfaces":[{"index":8,"id":SURFACE_ID,"ref":"surface:9"}]}),
            ),
            ("tab.action".into(), json!({"action":"pin"})),
        ]),
    );
    let output = executable(
        Some(&pipe),
        &[
            "tab-action",
            "pin",
            "--window",
            WINDOW_ID,
            "--workspace",
            WORKSPACE_ID,
            "--tab",
            "tab:9",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.list");
    assert_eq!(
        Value::Object(params),
        json!({
            "window_id":WINDOW_ID, "workspace_id":WORKSPACE_ID
        })
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "tab.action");
    assert_eq!(
        Value::Object(params),
        json!({
            "action":"pin", "window_id":WINDOW_ID,
            "workspace_id":WORKSPACE_ID, "surface_id":SURFACE_ID, "focus":false
        })
    );
}

#[test]
fn respawn_pane_resolves_numeric_surface_without_changing_focused_fallback() {
    let (pipe, request_rx) = spawn_method_server(
        "respawn-pane-surface-index",
        HashMap::from([
            (
                "surface.list".into(),
                json!({"surfaces":[{"index":2,"id":SURFACE_ID,"ref":"surface:9"}]}),
            ),
            (
                "surface.respawn".into(),
                json!({"surface_id":SURFACE_ID,"workspace_id":WORKSPACE_ID}),
            ),
        ]),
    );
    let output = executable(
        Some(&pipe),
        &[
            "respawn-pane",
            "--workspace",
            WORKSPACE_ID,
            "--surface",
            "2",
            "--command",
            "echo ok",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.list");
    assert_eq!(Value::Object(params), json!({"workspace_id":WORKSPACE_ID}));
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.respawn");
    let params = Value::Object(params);
    assert_eq!(params["workspace_id"], WORKSPACE_ID);
    assert_eq!(params["surface_id"], SURFACE_ID);
    assert_eq!(params["tmux_start_command"], "echo ok");
}

#[test]
fn no_context_lifecycle_commands_resolve_current_workspace_before_server_focus() {
    for (tag, args, final_method) in [
        (
            "tab-action-current-workspace",
            vec!["tab-action", "pin"],
            "tab.action",
        ),
        (
            "respawn-pane-current-workspace",
            vec!["respawn-pane", "--command", "echo ok"],
            "surface.respawn",
        ),
    ] {
        let (pipe, request_rx) = spawn_method_server(
            tag,
            HashMap::from([
                (
                    "workspace.current".into(),
                    json!({"workspace_id":WORKSPACE_ID,"workspace_ref":"workspace:4"}),
                ),
                (final_method.into(), json!({"action":"pin"})),
            ]),
        );
        let output = executable(Some(&pipe), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            request_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            ("workspace.current".into(), serde_json::Map::new())
        );
        let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(method, final_method);
        assert_eq!(params.get("workspace_id"), Some(&json!(WORKSPACE_ID)));
        assert!(params.get("surface_id").is_none());
    }
}

#[test]
fn help_command_and_flag_routes_remain_distinct() {
    let top = executable(None, &["help"]);
    assert!(top.status.success());
    assert_eq!(
        String::from_utf8(top.stdout).unwrap(),
        "Usage: cmux <path>|<command> [options]\n"
    );

    for args in [
        vec!["help", "tab-action"],
        vec!["help", "definitely-unknown"],
    ] {
        let output = executable(None, &args);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "Usage: cmux <path>|<command> [options]\n"
        );
        assert!(output.stderr.is_empty());
    }

    let subcommand = executable(None, &["tab-action", "--help"]);
    assert!(subcommand.status.success());
    assert!(String::from_utf8(subcommand.stdout)
        .unwrap()
        .starts_with("cmux tab-action\n\nUsage:"));
}

#[test]
fn parser_errors_are_exact_and_do_not_touch_the_pipe() {
    for (args, expected) in [
        (
            vec!["tab-action"],
            "Error: tab-action requires --action <name>\n",
        ),
        (
            vec!["tab-action", "pin", "--wat"],
            "Error: tab-action: unknown flag '--wat'\n",
        ),
        (
            vec!["tab-action", "rename"],
            "Error: tab-action rename requires --title <text> (or a trailing title)\n",
        ),
        (
            vec!["tab-action", "pin", "--focus", "maybe"],
            "Error: --focus must be true|false\n",
        ),
        (
            vec!["tab-action", "pin", "--surface", "not-a-handle"],
            "Error: Invalid surface handle: not-a-handle (expected UUID, ref like surface:1, or index)\n",
        ),
    ] {
        assert_failure(executable(None, &args), expected);
    }
}

#[test]
fn help_is_concrete_for_both_lifecycle_commands() {
    let tab = executable(None, &["tab-action", "--help"]);
    assert!(tab.status.success());
    assert!(tab.stderr.is_empty());
    let tab = String::from_utf8(tab.stdout).unwrap();
    assert_eq!(tab, concat!(
        "cmux tab-action\n\nUsage: cmux tab-action --action <name> [flags]\n\n",
        "Perform horizontal tab context-menu actions from CLI/socket.\n\nActions:\n",
        "  rename | clear-name\n  close-left | close-right | close-others\n",
        "  new-terminal-right | new-browser-right\n  move-to-new-workspace\n",
        "  reload | duplicate\n  pin | unpin | mark-unread | toggle-full-width-tab\n\nFlags:\n",
        "  --action <name>              Action name (required if not positional)\n",
        "  --tab <id|ref|index>         Target tab (accepts tab:<n> or surface:<n>; default: $CMUX_TAB_ID, then $CMUX_SURFACE_ID, then focused tab)\n",
        "  --surface <id|ref|index>     Alias for --tab (backward compatibility)\n",
        "  --workspace <id|ref|index>   Workspace context (default: current/$CMUX_WORKSPACE_ID)\n",
        "  --window <id|ref|index>      Window context for workspace/tab refs and indexes\n",
        "  --title <text>               Title for rename (or pass trailing title text)\n",
        "  --url <url>                  Optional URL for new-browser-right\n",
        "  --focus <true|false>         Focus the destination when supported (default: false for move-to-new-workspace)\n\nExample:\n",
        "  cmux tab-action --tab tab:3 --action pin\n  cmux tab-action --action close-right\n",
        "  cmux tab-action --tab tab:2 --action move-to-new-workspace\n",
        "  cmux tab-action --tab tab:2 --action rename --title \"build logs\"\n"
    ));

    let respawn = executable(None, &["respawn-pane", "--help"]);
    assert!(respawn.status.success());
    assert!(respawn.stderr.is_empty());
    let respawn = String::from_utf8(respawn.stdout).unwrap();
    assert_eq!(respawn, concat!(
        "cmux respawn-pane\n\nUsage: cmux respawn-pane [--workspace <id|ref|index>] [--surface <id|ref|index>] [--window <id|ref|index>] [--command <cmd> | <cmd>]\n\n",
        "Send a command (or default shell restart command) to a surface.\n\nFlags:\n",
        "  --workspace <id|ref|index>   Workspace context (default: $CMUX_WORKSPACE_ID)\n",
        "  --surface <id|ref|index>     Surface context (default: focused surface)\n",
        "  --window <id|ref|index>      Window context for workspace/surface refs and indexes\n",
        "  --command <cmd>        Command text (or pass trailing command text)\n"
    ));
}

#[test]
fn tab_action_executable_honors_ambient_and_explicit_selector_precedence() {
    let response = json!({
        "action": "rename",
        "surface_id": SURFACE_ID,
        "surface_ref": "surface:4",
        "workspace_id": WORKSPACE_ID,
        "workspace_ref": "workspace:2"
    });
    let (pipe, request_rx) = spawn_server("tab-action-precedence", ok(response));
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "tab-action",
            "--action",
            " ReName ",
            "--workspace",
            WORKSPACE_ID,
            "--tab",
            SURFACE_ID,
            "--surface",
            OTHER_SURFACE_ID,
            "build",
            " logs ",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .env("CMUX_WORKSPACE_ID", "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
        .env("CMUX_TAB_ID", "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb")
        .env("CMUX_SURFACE_ID", "cccccccc-cccc-4ccc-8ccc-cccccccccccc")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK action=rename tab=tab:4 workspace=workspace:2\n"
    );
    assert!(output.stderr.is_empty());

    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "tab.action");
    assert_eq!(
        Value::Object(params),
        json!({
            "action": "rename",
            "workspace_id": WORKSPACE_ID,
            "surface_id": SURFACE_ID,
            "title": "build logs",
            "focus": false,
        })
    );
}

#[test]
fn explicit_window_or_workspace_suppresses_inappropriate_ambient_tab_context() {
    let (pipe, request_rx) = spawn_method_server(
        "tab-action-window",
        HashMap::from([
            (
                "workspace.current".into(),
                json!({"workspace_id":WORKSPACE_ID, "workspace_ref":"workspace:2"}),
            ),
            ("tab.action".into(), json!({"action":"pin"})),
        ]),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["tab-action", "pin", "--window", WINDOW_ID])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .env("CMUX_WORKSPACE_ID", WORKSPACE_ID)
        .env("CMUX_TAB_ID", SURFACE_ID)
        .env("CMUX_SURFACE_ID", OTHER_SURFACE_ID)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "workspace.current");
    assert_eq!(Value::Object(params), json!({"window_id":WINDOW_ID}));
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "tab.action");
    assert_eq!(
        Value::Object(params),
        json!({
            "action":"pin", "window_id":WINDOW_ID,
            "workspace_id":WORKSPACE_ID, "focus":false
        })
    );

    let (pipe, request_rx) = spawn_server("tab-action-workspace", ok(json!({"action":"unpin"})));
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["tab-action", "unpin", "--workspace", WORKSPACE_ID])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .env("CMUX_TAB_ID", SURFACE_ID)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (_, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        Value::Object(params),
        json!({"action":"unpin", "workspace_id":WORKSPACE_ID, "focus":false})
    );

    let (pipe, request_rx) = spawn_server("tab-action-ambient", ok(json!({"action":"mark_read"})));
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["tab-action", "mark-read"])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .env("CMUX_WORKSPACE_ID", WORKSPACE_ID)
        .env("CMUX_TAB_ID", SURFACE_ID)
        .env("CMUX_SURFACE_ID", OTHER_SURFACE_ID)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (_, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        Value::Object(params),
        json!({
            "action":"mark_read", "workspace_id":WORKSPACE_ID,
            "surface_id":SURFACE_ID, "focus":false
        })
    );
}

#[test]
fn tab_action_formats_text_json_and_all_id_modes_exactly() {
    let payload = json!({
        "action":"new_terminal_right",
        "surface_id":SURFACE_ID, "surface_ref":"surface:4",
        "workspace_id":WORKSPACE_ID, "workspace_ref":"workspace:2",
        "closed":2, "full_width_tab_mode":true,
        "created_surface_id":OTHER_SURFACE_ID, "created_surface_ref":"surface:5",
        "created_workspace_id":"55555555-5555-4555-8555-555555555555",
        "created_workspace_ref":"workspace:6"
    });
    let (pipe, _request_rx) = spawn_server("tab-action-text", ok(payload.clone()));
    let output = executable(
        Some(&pipe),
        &["tab-action", "new-terminal-right", "--surface", SURFACE_ID],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK action=new_terminal_right tab=tab:4 workspace=workspace:2 closed=2 full_width_tab_mode=true created=tab:5 created_workspace=workspace:6\n");

    for (tag, id_format, expected) in [
        (
            "refs",
            "refs",
            json!({
                "action":"new_terminal_right", "surface_ref":"surface:4", "workspace_ref":"workspace:2",
                "closed":2, "full_width_tab_mode":true, "created_surface_ref":"surface:5",
                "created_workspace_ref":"workspace:6"
            }),
        ),
        (
            "uuids",
            "uuids",
            json!({
                "action":"new_terminal_right", "surface_id":SURFACE_ID, "workspace_id":WORKSPACE_ID,
                "closed":2, "full_width_tab_mode":true, "created_surface_id":OTHER_SURFACE_ID,
                "created_workspace_id":"55555555-5555-4555-8555-555555555555"
            }),
        ),
        ("both", "both", payload.clone()),
    ] {
        let (pipe, _request_rx) = spawn_server(tag, ok(payload.clone()));
        let output = executable(
            Some(&pipe),
            &[
                "--json",
                "--id-format",
                id_format,
                "tab-action",
                "new-terminal-right",
                "--surface",
                SURFACE_ID,
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            expected
        );
    }
}

#[test]
fn backend_errors_keep_nonzero_exit_empty_stdout_and_exact_stderr() {
    let (pipe, request_rx) = spawn_server(
        "tab-action-error",
        ControlCallResult::Err {
            code: "unavailable".into(),
            message: "TabManager not available".into(),
            data: None,
        },
    );
    let output = executable(Some(&pipe), &["tab-action", "pin", "--surface", SURFACE_ID]);
    assert_failure(output, "Error: unavailable: TabManager not available\n");
    assert_eq!(
        request_rx.recv_timeout(Duration::from_secs(5)).unwrap().0,
        "tab.action"
    );
}

#[test]
fn respawn_pane_executable_preserves_command_and_selector_precedence() {
    let (pipe, request_rx) = spawn_server(
        "respawn-pane-command",
        ok(json!({
            "surface_id":SURFACE_ID, "surface_ref":"surface:4",
            "workspace_id":WORKSPACE_ID, "workspace_ref":"workspace:2"
        })),
    );
    let command_text = "echo \"two words\" && echo it's";
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "respawn-pane",
            "--window",
            WINDOW_ID,
            "--workspace",
            WORKSPACE_ID,
            "--surface",
            SURFACE_ID,
            "--command",
            command_text,
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .env("CMUX_WORKSPACE_ID", "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa")
        .env("CMUX_SURFACE_ID", OTHER_SURFACE_ID)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK\n");
    assert!(output.stderr.is_empty());

    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.respawn");
    let params = Value::Object(params);
    assert_eq!(params["window_id"], WINDOW_ID);
    assert_eq!(params["workspace_id"], WORKSPACE_ID);
    assert_eq!(params["surface_id"], SURFACE_ID);
    assert_eq!(params["tmux_start_command"], command_text);
    let wrapper = params["command"].as_str().expect("native shell wrapper");
    assert_native_windows_command(wrapper);
    assert!(wrapper.contains(command_text), "{wrapper}");
}

#[test]
fn respawn_pane_default_and_json_output_are_exact() {
    let payload = json!({
        "surface_id":SURFACE_ID, "surface_ref":"surface:4",
        "workspace_id":WORKSPACE_ID, "workspace_ref":"workspace:2",
        "window_id":WINDOW_ID, "window_ref":"window:1", "type":"terminal"
    });
    let (pipe, request_rx) = spawn_server("respawn-pane-default", ok(payload.clone()));
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["--json", "--id-format", "refs", "respawn-pane"])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .env("CMUX_WORKSPACE_ID", WORKSPACE_ID)
        .env("CMUX_SURFACE_ID", SURFACE_ID)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        json!({
            "surface_ref":"surface:4", "workspace_ref":"workspace:2",
            "window_ref":"window:1", "type":"terminal"
        })
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.respawn");
    let params = Value::Object(params);
    assert_eq!(params["workspace_id"], WORKSPACE_ID);
    assert!(params.get("surface_id").is_none());
    let start_command = params["tmux_start_command"]
        .as_str()
        .expect("native default start command");
    assert_native_windows_command(start_command);
    let wrapper = params["command"]
        .as_str()
        .expect("native default shell wrapper");
    assert_native_windows_command(wrapper);
}

#[test]
fn tab_action_focus_accepts_all_frozen_boolean_spellings() {
    for (raw, expected) in [
        ("TRUE", true),
        ("1", true),
        ("Yes", true),
        ("ON", true),
        ("false", false),
        ("0", false),
        ("No", false),
        ("OFF", false),
    ] {
        let mapped = control_command_for(
            "tab-action",
            &[
                "pin".into(),
                "--workspace".into(),
                WORKSPACE_ID.into(),
                "--focus".into(),
                raw.into(),
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(mapped.params["focus"], expected, "{raw}");
    }
}

#[test]
fn lifecycle_command_specific_aliases_and_respawn_leftovers_match_frozen_parser() {
    for alias in [
        "--surface-id",
        "--surface-ref",
        "--workspace-id",
        "--workspace-ref",
        "--window-id",
    ] {
        assert_failure(
            executable(None, &["tab-action", "pin", alias, "value"]),
            &format!("Error: tab-action: unknown flag '{alias}'\n"),
        );
    }
    let mapped = control_command_for(
        "respawn-pane",
        &[
            "--workspace".into(),
            WORKSPACE_ID.into(),
            "--bogus".into(),
            "two words".into(),
        ],
    )
    .unwrap()
    .unwrap();
    assert_eq!(mapped.params["tmux_start_command"], "--bogus two words");
}

#[test]
fn explicit_empty_values_and_title_joining_are_frozen_exact() {
    let action = control_command_for(
        "tab-action",
        &[
            "--action".into(),
            "".into(),
            "--workspace".into(),
            WORKSPACE_ID.into(),
        ],
    )
    .unwrap()
    .unwrap();
    assert_eq!(action.params["action"], "");

    let rename = control_command_for(
        "tab-action",
        &[
            "rename".into(),
            "--workspace".into(),
            WORKSPACE_ID.into(),
            " alpha ".into(),
            " beta ".into(),
        ],
    )
    .unwrap()
    .unwrap();
    assert_eq!(rename.params["title"], "alpha   beta");

    let respawn = control_command_for(
        "respawn-pane",
        &[
            "--workspace".into(),
            WORKSPACE_ID.into(),
            "--command".into(),
            "".into(),
            "ignored".into(),
        ],
    )
    .unwrap()
    .unwrap();
    let default_shell =
        std::env::var("ComSpec").unwrap_or_else(|_| r"C:\Windows\System32\cmd.exe".into());
    assert_eq!(respawn.params["tmux_start_command"], default_shell);
}

#[test]
fn invalid_ambient_lifecycle_handles_fail_before_transport() {
    let workspace = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["tab-action", "pin"])
        .env("CMUX_SOCKET_PATH", r"\\.\pipe\cmux-must-not-connect")
        .env("CMUX_WORKSPACE_ID", "bad-workspace")
        .env_remove("CMUX_TAB_ID")
        .env_remove("CMUX_SURFACE_ID")
        .output()
        .unwrap();
    assert_failure(workspace, "Error: Invalid workspace handle: bad-workspace (expected UUID, ref like workspace:1, or index)\n");

    let surface = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["tab-action", "pin"])
        .env("CMUX_SOCKET_PATH", r"\\.\pipe\cmux-must-not-connect")
        .env("CMUX_WORKSPACE_ID", WORKSPACE_ID)
        .env("CMUX_TAB_ID", "bad-surface")
        .env_remove("CMUX_SURFACE_ID")
        .output()
        .unwrap();
    assert_failure(surface, "Error: Invalid surface handle: bad-surface (expected UUID, ref like surface:1, or index)\n");
}

#[test]
fn text_summary_uses_requested_action_and_tab_alias_payload_keys() {
    let payload = json!({
        "tab_id":SURFACE_ID, "tab_ref":"surface:4",
        "created_tab_id":OTHER_SURFACE_ID, "created_tab_ref":"surface:5"
    });
    let (pipe, _request_rx) = spawn_server("tab-alias-summary", ok(payload));
    let output = executable(
        Some(&pipe),
        &["tab-action", "pin", "--workspace", WORKSPACE_ID],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK action=pin tab=tab:4 created=tab:5\n"
    );
}

#[test]
fn signed_indexes_and_case_insensitive_handle_matching_are_canonical() {
    let (pipe, request_rx) = spawn_method_server(
        "signed-window-index",
        HashMap::from([
            (
                "window.list".into(),
                json!({"windows":[{"index":-2,"id":WINDOW_ID,"ref":"window:1"}]}),
            ),
            (
                "workspace.current".into(),
                json!({"workspace_id":WORKSPACE_ID}),
            ),
            ("tab.action".into(), json!({"action":"pin"})),
        ]),
    );
    let output = executable(Some(&pipe), &["tab-action", "pin", "--window", "-2"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        request_rx.recv_timeout(Duration::from_secs(5)).unwrap().0,
        "window.list"
    );
    assert_eq!(
        request_rx.recv_timeout(Duration::from_secs(5)).unwrap().0,
        "workspace.current"
    );
    assert_eq!(
        request_rx.recv_timeout(Duration::from_secs(5)).unwrap().0,
        "tab.action"
    );

    for (tag, args, expected) in [
        (
            "signed-workspace-miss",
            vec!["tab-action", "pin", "--workspace", "-3"],
            "Error: Workspace index not found\n",
        ),
        (
            "signed-surface-miss",
            vec![
                "tab-action",
                "pin",
                "--workspace",
                WORKSPACE_ID,
                "--surface",
                "-3",
            ],
            "Error: Surface index not found\n",
        ),
    ] {
        let (pipe, _rx) = spawn_method_server(
            tag,
            HashMap::from([
                ("workspace.list".into(), json!({"workspaces":[]})),
                ("surface.list".into(), json!({"surfaces":[]})),
            ]),
        );
        assert_failure(executable(Some(&pipe), &args), expected);
    }

    let upper_surface = SURFACE_ID.to_ascii_uppercase();
    let (pipe, request_rx) = spawn_method_server(
        "case-handles",
        HashMap::from([
            (
                "window.list".into(),
                json!({"windows":[{"id":WINDOW_ID,"ref":"window:1"}]}),
            ),
            (
                "workspace.list".into(),
                json!({"workspaces":[{"id":WORKSPACE_ID,"ref":"workspace:2"}]}),
            ),
            (
                "surface.list".into(),
                json!({"surfaces":[{"id":SURFACE_ID,"ref":"surface:3"}]}),
            ),
            ("tab.action".into(), json!({"action":"pin"})),
        ]),
    );
    let output = executable(
        Some(&pipe),
        &[
            "tab-action",
            "pin",
            "--window",
            "WINDOW:1",
            "--workspace",
            "WORKSPACE:2",
            "--surface",
            &upper_surface,
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for method in [
        "window.list",
        "workspace.list",
        "surface.list",
        "tab.action",
    ] {
        assert_eq!(
            request_rx.recv_timeout(Duration::from_secs(5)).unwrap().0,
            method
        );
    }
}

#[test]
fn respawn_refs_resolve_to_uuids_while_tab_action_keeps_raw_refs() {
    let (pipe, request_rx) = spawn_method_server(
        "respawn-ref-resolution",
        HashMap::from([
            (
                "window.list".into(),
                json!({"windows":[{"id":WINDOW_ID,"ref":"window:1"}]}),
            ),
            (
                "workspace.list".into(),
                json!({"workspaces":[{"id":WORKSPACE_ID,"ref":"workspace:2"}]}),
            ),
            (
                "surface.list".into(),
                json!({"surfaces":[{"id":SURFACE_ID,"ref":"surface:3"}]}),
            ),
            ("surface.respawn".into(), json!({"surface_id":SURFACE_ID})),
        ]),
    );
    let output = executable(
        Some(&pipe),
        &[
            "respawn-pane",
            "--workspace",
            "workspace:2",
            "--surface",
            "surface:3",
            "--command",
            "echo ok",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for method in [
        "window.list",
        "workspace.list",
        "surface.list",
        "surface.respawn",
    ] {
        let (actual, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(actual, method);
        if method == "surface.respawn" {
            assert_eq!(params.get("workspace_id"), Some(&json!(WORKSPACE_ID)));
            assert_eq!(params.get("surface_id"), Some(&json!(SURFACE_ID)));
        }
    }
}
