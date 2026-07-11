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
    let tab = executable(None, &["help", "tab-action"]);
    assert!(tab.status.success());
    assert!(tab.stderr.is_empty());
    let tab = String::from_utf8(tab.stdout).unwrap();
    assert!(tab.starts_with("cmux tab-action\n\nUsage: cmux tab-action --action <name> [flags]\n"));
    for required in [
        "--tab <id|ref|index>",
        "--surface <id|ref|index>",
        "--workspace <id|ref|index>",
        "--window <id|ref|index>",
        "--title <text>",
        "--url <url>",
        "--focus <true|false>",
        "default: $CMUX_TAB_ID, then $CMUX_SURFACE_ID, then focused tab",
    ] {
        assert!(tab.contains(required), "missing {required:?} in {tab}");
    }

    let respawn = executable(None, &["help", "respawn-pane"]);
    assert!(respawn.status.success());
    assert!(respawn.stderr.is_empty());
    let respawn = String::from_utf8(respawn.stdout).unwrap();
    assert!(respawn.starts_with(
        "cmux respawn-pane\n\nUsage: cmux respawn-pane [--workspace <id|ref|index>] [--surface <id|ref|index>] [--window <id|ref|index>] [--command <cmd> | <cmd>]\n"
    ));
    assert!(respawn.contains("Surface context (default: focused surface)"));
    assert!(respawn.contains("--command <cmd>"));
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
    let (pipe, _) = spawn_server("tab-action-text", ok(payload.clone()));
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
        let (pipe, _) = spawn_server(tag, ok(payload.clone()));
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
