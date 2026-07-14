#![cfg(windows)]

use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;

use cmux_cli::control_command_for;
use cmux_ipc::{control_pipe_path, serve_named_pipe, ControlCallResult, JsonValue};
use serde_json::{json, Value};

const WINDOW_ID: &str = "11111111-1111-4111-8111-111111111111";
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
            json!({"action":"rename", "title":"build   logs"}),
        ),
        (
            vec!["set-color", " #c0392b "],
            json!({"action":"set_color", "color":"#c0392b"}),
        ),
        (
            vec!["set-description", " first ", " second "],
            json!({"action":"set_description", "description":"first   second"}),
        ),
        (vec!["pin", "ignored", "text"], json!({"action":"pin"})),
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
        assert_failure(executable(None, &arguments), message);
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
fn workspace_action_help_is_complete_in_both_help_forms() {
    let direct = executable(None, &["workspace-action", "--help"]);
    let nested = executable(None, &["help", "workspace-action"]);
    assert!(direct.status.success());
    assert!(nested.status.success());
    assert_eq!(direct.stdout, nested.stdout);
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
