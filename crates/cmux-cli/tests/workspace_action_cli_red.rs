#![cfg(windows)]

use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;

use cmux_cli::control_command_for;
use cmux_ipc::{control_pipe_path, serve_named_pipe, ControlCallResult, JsonValue};
use serde_json::{json, Value};

const WINDOW_ID: &str = "11111111-1111-4111-8111-111111111111";
const OTHER_WINDOW_ID: &str = "33333333-3333-4333-8333-333333333333";
const WORKSPACE_ID: &str = "22222222-2222-4222-8222-222222222222";

type CapturedRequest = (String, serde_json::Map<String, Value>);

fn ok(value: Value) -> ControlCallResult {
    ControlCallResult::Ok(JsonValue::try_from(value).unwrap())
}

fn spawn_server(tag: &str, result: ControlCallResult) -> (String, mpsc::Receiver<CapturedRequest>) {
    let pipe = control_pipe_path(&format!(
        "cmux-workspace-action-{tag}-{}-{}",
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

fn executable(pipe: Option<&str>, args: &[&str]) -> Output {
    executable_with_env(pipe, args, &[])
}

fn executable_with_env(pipe: Option<&str>, args: &[&str], env: &[(&str, &str)]) -> Output {
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
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().unwrap()
}

fn assert_failure(output: Output, expected_stderr: &str) {
    assert_eq!(
        output.status.code(),
        Some(1),
        "unexpected process result: {output:?}"
    );
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8(output.stderr).unwrap(), expected_stderr);
}

#[test]
fn workspace_action_parser_maps_all_fields_and_canonical_normalization() {
    let mapped = control_command_for(
        "workspace-action",
        &[
            "--action".into(),
            "Set-Description".into(),
            "--workspace".into(),
            "workspace:3".into(),
            "--window".into(),
            "window:2".into(),
            "--title".into(),
            " ignored title ".into(),
            "--color".into(),
            " Amber ".into(),
            "--description".into(),
            "  Ship checklist\r\n- verify  ".into(),
        ],
    )
    .unwrap()
    .expect("workspace-action must be mapped");

    assert_eq!(mapped.method, "workspace.action");
    assert_eq!(
        mapped.params,
        json!({
            "action": "set_description",
            "workspace_ref": "workspace:3",
            "window_ref": "window:2",
            "title": "ignored title",
            "color": "Amber",
            "description": "Ship checklist\r\n- verify",
        })
    );
}

#[test]
fn workspace_action_infers_only_the_value_owned_by_the_selected_action() {
    let cases = [
        (
            vec!["rename", " build ", " logs "],
            json!({"action":"rename", "title":"build   logs", "resolve_current_workspace":true}),
        ),
        (
            vec!["set-color", " #c0392b "],
            json!({"action":"set_color", "color":"#c0392b", "resolve_current_workspace":true}),
        ),
        (
            vec!["set-description", " first ", " second "],
            json!({"action":"set_description", "description":"first   second", "resolve_current_workspace":true}),
        ),
        (
            vec!["pin", "ignored", "text"],
            json!({"action":"pin", "resolve_current_workspace":true}),
        ),
    ];

    for (arguments, expected) in cases {
        let arguments = arguments.into_iter().map(String::from).collect::<Vec<_>>();
        let mapped = control_command_for("workspace-action", &arguments)
            .unwrap()
            .expect("workspace-action must be mapped");
        assert_eq!(mapped.params, expected);
    }
}

#[test]
fn workspace_action_parser_has_exact_canonical_local_errors() {
    for (arguments, message) in [
        (
            vec![],
            "Error: workspace-action requires --action <name>\n",
        ),
        (
            vec!["pin", "--wat"],
            "Error: workspace-action: unknown flag '--wat'\n",
        ),
        (
            vec!["rename"],
            "Error: workspace-action rename requires --title <text> (or a trailing title)\n",
        ),
        (
            vec!["set-color"],
            "Error: workspace-action set-color requires --color <name|#hex> (or a trailing color)\n",
        ),
        (
            vec!["set-description"],
            "Error: workspace-action set-description requires --description <text> (or trailing text)\n",
        ),
    ] {
        let mut command_args = vec!["workspace-action"];
        command_args.extend(arguments);
        assert_failure(executable(None, &command_args), message);
    }
}

#[test]
fn workspace_action_process_sends_one_typed_request_and_formats_canonical_summary() {
    let (pipe, request_rx) = spawn_server(
        "typed-request",
        ok(json!({
            "action": "close_others",
            "workspace_id": WORKSPACE_ID,
            "workspace_ref": "workspace:3",
            "window_id": WINDOW_ID,
            "window_ref": "window:2",
            "closed": 4,
        })),
    );

    let output = executable(
        Some(&pipe),
        &[
            "workspace-action",
            "close-others",
            "--workspace",
            WORKSPACE_ID,
            "--window",
            WINDOW_ID,
        ],
    );
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK action=close_others workspace=workspace:3 window=window:2 closed=4\n"
    );
    assert!(output.stderr.is_empty());

    let (method, params) = request_rx.recv().unwrap();
    assert_eq!(method, "workspace.action");
    assert_eq!(params["action"], "close_others");
    assert_eq!(params["workspace_id"], WORKSPACE_ID);
    assert_eq!(params["window_id"], WINDOW_ID);
}

#[test]
fn workspace_action_output_matches_clear_color_and_id_format_contracts() {
    let payload = json!({
        "action": "clear_color",
        "workspace_id": WORKSPACE_ID,
        "workspace_ref": "workspace:3",
        "window_id": WINDOW_ID,
        "window_ref": "window:2",
        "color": null,
    });

    let (plain_pipe, _plain_rx) = spawn_server("clear-color-plain", ok(payload.clone()));
    let plain = executable(
        Some(&plain_pipe),
        &[
            "workspace-action",
            "clear-color",
            "--workspace",
            WORKSPACE_ID,
        ],
    );
    assert!(plain.status.success(), "{plain:?}");
    assert_eq!(
        String::from_utf8(plain.stdout).unwrap(),
        "OK action=clear_color workspace=workspace:3 window=window:2\n"
    );

    let (uuid_pipe, _uuid_rx) = spawn_server("clear-color-uuids", ok(payload.clone()));
    let uuid = executable(
        Some(&uuid_pipe),
        &[
            "--id-format",
            "uuids",
            "workspace-action",
            "clear-color",
            "--workspace",
            WORKSPACE_ID,
        ],
    );
    assert!(uuid.status.success(), "{uuid:?}");
    assert_eq!(
        String::from_utf8(uuid.stdout).unwrap(),
        format!("OK action=clear_color workspace={WORKSPACE_ID} window={WINDOW_ID}\n")
    );

    for (tag, id_format, expected) in [
        (
            "json-refs",
            "refs",
            json!({
                "action":"clear_color", "workspace_ref":"workspace:3",
                "window_ref":"window:2", "color":null
            }),
        ),
        (
            "json-uuids",
            "uuids",
            json!({
                "action":"clear_color", "workspace_id":WORKSPACE_ID,
                "window_id":WINDOW_ID, "color":null
            }),
        ),
    ] {
        let (pipe, _request_rx) = spawn_server(tag, ok(payload.clone()));
        let output = executable(
            Some(&pipe),
            &[
                "--json",
                "--id-format",
                id_format,
                "workspace-action",
                "clear-color",
                "--workspace",
                WORKSPACE_ID,
            ],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap(),
            expected
        );
    }
}

#[test]
fn global_window_prefocuses_but_per_command_window_only_scopes() {
    let response = ok(json!({
        "action":"pin",
        "workspace_id":WORKSPACE_ID,
        "workspace_ref":"workspace:1",
        "window_id":WINDOW_ID,
        "window_ref":"window:1",
        "pinned":true,
    }));
    let (global_pipe, global_rx) = spawn_server("global-window", response.clone());
    let global = executable(
        Some(&global_pipe),
        &[
            "--window",
            WINDOW_ID,
            "workspace-action",
            "pin",
            "--workspace",
            WORKSPACE_ID,
        ],
    );
    assert!(global.status.success(), "{global:?}");
    assert_eq!(global_rx.recv().unwrap().0, "window.focus");
    let (method, params) = global_rx.recv().unwrap();
    assert_eq!(method, "workspace.action");
    assert_eq!(params["window_id"], WINDOW_ID);

    let (scoped_pipe, scoped_rx) = spawn_server("scoped-window", response);
    let scoped = executable(
        Some(&scoped_pipe),
        &[
            "workspace-action",
            "pin",
            "--workspace",
            WORKSPACE_ID,
            "--window",
            WINDOW_ID,
        ],
    );
    assert!(scoped.status.success(), "{scoped:?}");
    let (method, params) = scoped_rx.recv().unwrap();
    assert_eq!(method, "workspace.action");
    assert_eq!(params["window_id"], WINDOW_ID);
    assert!(
        scoped_rx.try_recv().is_err(),
        "per-command scope must not focus"
    );

    let (mixed_pipe, mixed_rx) = spawn_server(
        "mixed-window",
        ok(json!({
            "action":"pin", "workspace_id":WORKSPACE_ID,
            "workspace_ref":"workspace:1", "window_id":OTHER_WINDOW_ID,
            "window_ref":"window:2", "pinned":true
        })),
    );
    let mixed = executable(
        Some(&mixed_pipe),
        &[
            "--window",
            WINDOW_ID,
            "workspace-action",
            "pin",
            "--workspace",
            WORKSPACE_ID,
            "--window",
            OTHER_WINDOW_ID,
        ],
    );
    assert!(mixed.status.success(), "{mixed:?}");
    let (method, params) = mixed_rx.recv().unwrap();
    assert_eq!(method, "window.focus");
    assert_eq!(params["window_id"], WINDOW_ID);
    let (method, params) = mixed_rx.recv().unwrap();
    assert_eq!(method, "workspace.action");
    assert_eq!(params["window_id"], OTHER_WINDOW_ID);
}

#[test]
fn empty_ambient_workspace_falls_back_to_the_selected_workspace() {
    let response = ok(json!({
        "action":"pin", "workspace_id":WORKSPACE_ID,
        "workspace_ref":"workspace:1", "window_id":WINDOW_ID,
        "window_ref":"window:1", "pinned":true
    }));
    let (pipe, requests) = spawn_server("empty-ambient-workspace", response);
    let output = executable_with_env(
        Some(&pipe),
        &["workspace-action", "pin"],
        &[("CMUX_WORKSPACE_ID", "")],
    );
    assert!(output.status.success(), "{output:?}");
    let (method, params) = requests.recv().unwrap();
    assert_eq!(method, "workspace.current");
    assert!(params.get("workspace_id").is_none());
    let (method, params) = requests.recv().unwrap();
    assert_eq!(method, "workspace.action");
    assert_eq!(params["workspace_id"], WORKSPACE_ID);
}

#[test]
fn workspace_action_help_is_complete_in_both_help_forms() {
    let direct = executable(None, &["workspace-action", "--help"]);
    let nested = executable(None, &["help", "workspace-action"]);
    assert!(direct.status.success());
    assert!(nested.status.success());
    assert_eq!(
        String::from_utf8(nested.stdout).unwrap(),
        "Usage: cmux <path>|<command> [options]\n"
    );
    let text = String::from_utf8(direct.stdout).unwrap();
    for required in [
        "Usage: cmux workspace-action --action <name> [flags]",
        "close-others | close-above | close-below",
        "set-description | clear-description",
        "set-color | clear-color",
        "--workspace <id|ref|index>",
        "--window <id|ref|index>",
    ] {
        assert!(
            text.contains(required),
            "missing {required:?} from {text:?}"
        );
    }
}
