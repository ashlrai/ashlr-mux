//! Parity batch 1B (window lifecycle family) — CLI wire-contract tests.
//!
//! Canonical evidence: `CLI/cmux.swift` at pinned commit
//! `e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`:
//! - `new-window` dispatch: 4294-4296 (v1 `new_window`, raw reply, no --json wrap)
//! - `focus-window` / `close-window`: 4298-4310 + `normalizeWindowHandle` 6072-6101
//! - `window` namespace: 7894-7918 (hints), 8050-8110 (displays/display)
//! - `surface-resume`: 6563-6812 (subcommands, --shell vs `--` argv, ambient env)
//! - subcommand usage text: 15477-15510, 16234-16262; header rule 17051-17058
//!
//! The scripted server here asserts the CLI's exact wire behavior (v1 text
//! lines vs v2 JSON requests); the desktop backend handlers may not exist yet.

#![cfg(windows)]

use std::collections::HashMap;
use std::process::{Command, Output};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::BufReader;
use tokio::net::windows::named_pipe::ServerOptions;

const WINDOW_ID: &str = "44444444-4444-4444-8444-444444444444";
const OTHER_WINDOW_ID: &str = "55555555-5555-4555-8555-555555555555";
const WORKSPACE_ID: &str = "11111111-1111-4111-8111-111111111111";
const SURFACE_ID: &str = "22222222-2222-4222-8222-222222222222";

/// One observed inbound frame: either a raw v1 command line or a parsed v2
/// request (`method`, `params`).
#[derive(Debug, Clone, PartialEq)]
enum WireFrame {
    V1(String),
    V2(String, Value),
}

/// Spawn a scripted control server on a fresh pipe. Every accepted connection
/// reads newline-framed lines until EOF. A line starting with `{` is treated as
/// a v2 request and answered from `v2_responses` (method → result); any other
/// line is a v1 command answered with `v1_reply`. Every inbound frame is
/// recorded to the returned receiver in arrival order.
fn spawn_scripted_server(
    tag: &str,
    v1_reply: &str,
    v2_responses: HashMap<String, Value>,
) -> (String, mpsc::Receiver<WireFrame>) {
    let pipe = format!(
        r"\\.\pipe\cmux-wl-{tag}-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    );
    let (frame_tx, frame_rx) = mpsc::channel();
    let server_pipe = pipe.clone();
    let v1_reply = v1_reply.to_owned();
    thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async move {
                let mut server = ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&server_pipe)
                    .unwrap();
                loop {
                    if server.connect().await.is_err() {
                        return;
                    }
                    let connected = std::mem::replace(
                        &mut server,
                        match ServerOptions::new().create(&server_pipe) {
                            Ok(next) => next,
                            Err(_) => return,
                        },
                    );
                    let (reader, mut writer) = tokio::io::split(connected);
                    let mut reader = BufReader::new(reader);
                    while let Ok(Some(frame)) =
                        cmux_ipc::read_frame(&mut reader, cmux_ipc::MAX_RPC_FRAME_BYTES).await
                    {
                        let line = String::from_utf8(frame).unwrap();
                        let reply = if line.trim_start().starts_with('{') {
                            let request: Value = serde_json::from_str(&line).unwrap();
                            let method = request["method"].as_str().unwrap_or_default().to_owned();
                            let params = request.get("params").cloned().unwrap_or(json!({}));
                            let _ = frame_tx.send(WireFrame::V2(method.clone(), params));
                            let result = v2_responses.get(&method).cloned().unwrap_or(Value::Null);
                            serde_json::to_string(&json!({
                                "id": request.get("id").cloned().unwrap_or(Value::Null),
                                "ok": true,
                                "result": result,
                            }))
                            .unwrap()
                        } else {
                            let _ = frame_tx.send(WireFrame::V1(line));
                            v1_reply.clone()
                        };
                        if cmux_ipc::write_frame(&mut writer, &reply).await.is_err() {
                            break;
                        }
                    }
                }
            });
    });
    (pipe, frame_rx)
}

fn drain_frames(frame_rx: &mpsc::Receiver<WireFrame>) -> Vec<WireFrame> {
    let mut frames = Vec::new();
    while let Ok(frame) = frame_rx.recv_timeout(Duration::from_millis(400)) {
        frames.push(frame);
    }
    frames
}

fn base_command(pipe: Option<&str>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_cmux"));
    command
        .env_remove("CMUX_SOCKET")
        .env_remove("CMUX_SOCKET_PASSWORD")
        .env_remove("CMUX_WORKSPACE_ID")
        .env_remove("CMUX_SURFACE_ID")
        .env_remove("CMUX_TAB_ID")
        .env_remove("CMUX_WINDOW_ID")
        .env_remove("CMUX_QUIET")
        .env_remove("PWD");
    match pipe {
        Some(pipe) => command.env("CMUX_SOCKET_PATH", pipe),
        None => command.env("CMUX_SOCKET_PATH", r"\\.\pipe\cmux-wl-must-not-connect"),
    };
    command
}

fn executable(pipe: Option<&str>, args: &[&str]) -> Output {
    let mut command = base_command(pipe);
    command.args(args);
    command.output().unwrap()
}

fn assert_success_stdout(output: Output, expected_stdout: &str) {
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), expected_stdout);
    assert!(output.stderr.is_empty());
}

fn assert_failure(output: Output, expected_stderr: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(String::from_utf8(output.stderr).unwrap(), expected_stderr);
}

fn window_list_result() -> Value {
    json!({
        "windows": [
            {"id": WINDOW_ID, "ref": "window:3", "index": 0},
            {"id": OTHER_WINDOW_ID, "ref": "window:7", "index": 1},
        ]
    })
}

// ---------------------------------------------------------------------------
// cli:new-window (CLI/cmux.swift:4294-4296)
// ---------------------------------------------------------------------------

#[test]
fn new_window_sends_bare_v1_line_and_prints_raw_reply() {
    let reply = format!("OK {WINDOW_ID}");
    let (pipe, frame_rx) = spawn_scripted_server("nw-ok", &reply, HashMap::new());
    let output = executable(Some(&pipe), &["new-window"]);
    assert_success_stdout(output, &format!("OK {WINDOW_ID}\n"));
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V1("new_window".into())]
    );
}

#[test]
fn new_window_ignores_json_flag_and_trailing_arguments() {
    let reply = format!("OK {WINDOW_ID}");
    // Global --json before the command.
    let (pipe, frame_rx) = spawn_scripted_server("nw-json", &reply, HashMap::new());
    let output = executable(Some(&pipe), &["--json", "new-window"]);
    assert_success_stdout(output, &format!("OK {WINDOW_ID}\n"));
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V1("new_window".into())]
    );

    // Post-command --json plus trailing arguments (ignored by canonical).
    let (pipe, frame_rx) = spawn_scripted_server("nw-extra", &reply, HashMap::new());
    let output = executable(Some(&pipe), &["new-window", "--json", "extra"]);
    assert_success_stdout(output, &format!("OK {WINDOW_ID}\n"));
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V1("new_window".into())]
    );
}

#[test]
fn new_window_surfaces_v1_error_verbatim() {
    let (pipe, _frame_rx) =
        spawn_scripted_server("nw-err", "ERROR: Failed to create window", HashMap::new());
    let output = executable(Some(&pipe), &["new-window"]);
    assert_failure(output, "Error: ERROR: Failed to create window\n");
}

// ---------------------------------------------------------------------------
// cli:focus-window / cli:close-window (CLI/cmux.swift:4298-4310, 6072-6101)
// ---------------------------------------------------------------------------

#[test]
fn focus_window_uuid_passes_through_without_a_list_call() {
    let (pipe, frame_rx) = spawn_scripted_server("fw-uuid", "OK", HashMap::new());
    let output = executable(Some(&pipe), &["focus-window", "--window", WINDOW_ID]);
    assert_success_stdout(output, "OK\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V1(format!("focus_window {WINDOW_ID}"))]
    );
}

#[test]
fn focus_window_ref_resolves_client_side_through_window_list() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "fw-ref",
        "OK",
        HashMap::from([("window.list".to_owned(), window_list_result())]),
    );
    let output = executable(Some(&pipe), &["focus-window", "--window", "window:7"]);
    assert_success_stdout(output, "OK\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![
            WireFrame::V2("window.list".into(), json!({})),
            WireFrame::V1(format!("focus_window {OTHER_WINDOW_ID}")),
        ]
    );
}

#[test]
fn focus_window_index_resolves_client_side_through_window_list() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "fw-index",
        "OK",
        HashMap::from([("window.list".to_owned(), window_list_result())]),
    );
    let output = executable(Some(&pipe), &["focus-window", "--window", "1"]);
    assert_success_stdout(output, "OK\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![
            WireFrame::V2("window.list".into(), json!({})),
            WireFrame::V1(format!("focus_window {OTHER_WINDOW_ID}")),
        ]
    );
}

#[test]
fn focus_window_resolution_failures_use_exact_canonical_messages() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "fw-noref",
        "OK",
        HashMap::from([("window.list".to_owned(), window_list_result())]),
    );
    let output = executable(Some(&pipe), &["focus-window", "--window", "window:9"]);
    assert_failure(output, "Error: Window not found: window:9\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2("window.list".into(), json!({}))]
    );

    let (pipe, frame_rx) = spawn_scripted_server(
        "fw-noindex",
        "OK",
        HashMap::from([("window.list".to_owned(), window_list_result())]),
    );
    let output = executable(Some(&pipe), &["focus-window", "--window", "5"]);
    assert_failure(output, "Error: Window index not found\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2("window.list".into(), json!({}))]
    );
}

#[test]
fn focus_window_invalid_handle_fails_without_touching_the_pipe() {
    let output = executable(None, &["focus-window", "--window", "bogus"]);
    assert_failure(
        output,
        "Error: Invalid window handle: bogus (expected UUID, ref like window:1, or index)\n",
    );
}

#[test]
fn focus_window_requires_the_per_command_window_flag() {
    // Missing entirely.
    assert_failure(
        executable(None, &["focus-window"]),
        "Error: focus-window requires --window\n",
    );
    // Blank value.
    assert_failure(
        executable(None, &["focus-window", "--window", "  "]),
        "Error: focus-window requires --window\n",
    );
    // QUIRK: the GLOBAL --window override is NOT consulted (CLI/cmux.swift:4299
    // reads optionValue(commandArgs) only).
    assert_failure(
        executable(None, &["--window", WINDOW_ID, "focus-window"]),
        "Error: focus-window requires --window\n",
    );
    // optionValue stops scanning at `--` (CLI/cmux.swift:17128).
    assert_failure(
        executable(None, &["focus-window", "--", "--window", WINDOW_ID]),
        "Error: focus-window requires --window\n",
    );
}

#[test]
fn close_window_mirrors_focus_window_with_the_close_v1_command() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "cw-ref",
        "OK",
        HashMap::from([("window.list".to_owned(), window_list_result())]),
    );
    let output = executable(Some(&pipe), &["close-window", "--window", "window:3"]);
    assert_success_stdout(output, "OK\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![
            WireFrame::V2("window.list".into(), json!({})),
            WireFrame::V1(format!("close_window {WINDOW_ID}")),
        ]
    );

    assert_failure(
        executable(None, &["close-window"]),
        "Error: close-window requires --window\n",
    );
    assert_failure(
        executable(None, &["--window", WINDOW_ID, "close-window"]),
        "Error: close-window requires --window\n",
    );
}

#[test]
fn close_window_surfaces_server_error_verbatim() {
    let (pipe, _frame_rx) =
        spawn_scripted_server("cw-err", "ERROR: Window not found", HashMap::new());
    let output = executable(Some(&pipe), &["close-window", "--window", WINDOW_ID]);
    assert_failure(output, "Error: ERROR: Window not found\n");
}

// ---------------------------------------------------------------------------
// cli:window (CLI/cmux.swift:7894-7918, 8050-8110, 3244-3247)
// ---------------------------------------------------------------------------

#[test]
fn window_subcommand_hints_are_byte_exact_including_default_display_quirk() {
    // No subcommand: hint INCLUDES default-display (CLI/cmux.swift:7901-7902).
    assert_failure(
        executable(None, &["window"]),
        "Error: window requires a subcommand. Try: display, displays, default-display\n",
    );
    // Unknown subcommand: hint OMITS default-display (CLI/cmux.swift:7917-7918).
    assert_failure(
        executable(None, &["window", "bogus"]),
        "Error: Unknown window subcommand: bogus. Try: display, displays\n",
    );
}

#[test]
fn window_displays_formats_text_rows_with_main_suffix() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "wd-list",
        "OK",
        HashMap::from([(
            "window.displays".to_owned(),
            json!({"displays": [
                {"name": "LG HDR 4K", "index": 0, "main": true},
                {"name": "Dell U2720Q", "index": 1},
            ]}),
        )]),
    );
    let output = executable(Some(&pipe), &["window", "displays"]);
    assert_success_stdout(output, "0: LG HDR 4K  (main)\n1: Dell U2720Q\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2("window.displays".into(), json!({}))]
    );

    let (pipe, _frame_rx) = spawn_scripted_server(
        "wd-empty",
        "OK",
        HashMap::from([("window.displays".to_owned(), json!({"displays": []}))]),
    );
    let output = executable(Some(&pipe), &["window", "displays"]);
    assert_success_stdout(output, "No displays found.\n");
}

#[test]
fn window_display_sends_the_display_name_and_formats_moved_counts() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "wd-move",
        "OK",
        HashMap::from([(
            "window.display".to_owned(),
            json!({"display": "LG HDR 4K", "moved": [WINDOW_ID, OTHER_WINDOW_ID]}),
        )]),
    );
    let output = executable(Some(&pipe), &["window", "display", "LG HDR 4K"]);
    assert_success_stdout(output, "Moved 2 windows to LG HDR 4K.\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2(
            "window.display".into(),
            json!({"display": "LG HDR 4K"})
        )]
    );

    let (pipe, _frame_rx) = spawn_scripted_server(
        "wd-one",
        "OK",
        HashMap::from([(
            "window.display".to_owned(),
            json!({"display": "LG", "moved": [WINDOW_ID]}),
        )]),
    );
    let output = executable(Some(&pipe), &["window", "display", "LG"]);
    assert_success_stdout(output, "Moved 1 window to LG.\n");
}

#[test]
fn window_display_missing_name_and_list_alias_match_canonical() {
    assert_failure(
        executable(None, &["window", "display"]),
        "Error: window display requires a display name. Usage: cmux window display \"LG HDR 4K\"  (list names with: cmux window displays)\n",
    );

    let (pipe, frame_rx) = spawn_scripted_server(
        "wd-alias",
        "OK",
        HashMap::from([("window.displays".to_owned(), json!({"displays": []}))]),
    );
    let output = executable(Some(&pipe), &["window", "display", "--list"]);
    assert_success_stdout(output, "No displays found.\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2("window.displays".into(), json!({}))]
    );
}

#[test]
fn window_display_honors_the_global_window_override_with_client_side_resolution() {
    // Opposite of focus-window/close-window: the GLOBAL --window IS honored,
    // normalized via window.list (CLI/cmux.swift:8087-8091).
    let (pipe, frame_rx) = spawn_scripted_server(
        "wd-override",
        "OK",
        HashMap::from([
            ("window.list".to_owned(), window_list_result()),
            (
                "window.display".to_owned(),
                json!({"display": "LG", "moved": [WINDOW_ID]}),
            ),
        ]),
    );
    let output = executable(
        Some(&pipe),
        &["--window", "window:3", "window", "display", "LG"],
    );
    assert_success_stdout(output, "Moved 1 window to LG.\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![
            WireFrame::V2("window.list".into(), json!({})),
            WireFrame::V2(
                "window.display".into(),
                json!({"display": "LG", "window_id": WINDOW_ID})
            ),
        ]
    );
}

// ---------------------------------------------------------------------------
// cli:surface-resume (CLI/cmux.swift:6563-6812)
// ---------------------------------------------------------------------------

fn resume_ok_result() -> Value {
    json!({
        "window_id": Value::Null,
        "workspace_id": WORKSPACE_ID,
        "surface_id": SURFACE_ID,
        "cleared": false,
        "resume_binding": {"command": "opencode --resume"},
    })
}

#[test]
fn surface_resume_set_quotes_argv_and_always_sends_source_and_cwd() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-set",
        "OK",
        HashMap::from([("surface.resume.set".to_owned(), resume_ok_result())]),
    );
    let mut command = base_command(Some(&pipe));
    command.env("PWD", r"C:\work\repo");
    command.args([
        "surface-resume",
        "set",
        "--surface",
        SURFACE_ID,
        "--kind",
        "opencode",
        "--checkpoint",
        "ses_123",
        "--",
        "opencode",
        "--session",
        "ses_123",
    ]);
    let output = command.output().unwrap();
    assert_success_stdout(output, "OK\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2(
            "surface.resume.set".into(),
            json!({
                "surface_id": SURFACE_ID,
                "kind": "opencode",
                "checkpoint_id": "ses_123",
                "source": "cli",
                "cwd": r"C:\work\repo",
                "command": "'opencode' '--session' 'ses_123'",
            })
        )]
    );
}

#[test]
fn surface_resume_set_shell_wins_and_is_trimmed() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-shell",
        "OK",
        HashMap::from([("surface.resume.set".to_owned(), resume_ok_result())]),
    );
    let mut command = base_command(Some(&pipe));
    command.env("PWD", r"C:\work\repo");
    command.args([
        "surface-resume",
        "set",
        "--surface",
        SURFACE_ID,
        "--shell",
        "  tmux attach -t work  ",
    ]);
    let output = command.output().unwrap();
    assert_success_stdout(output, "OK\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2(
            "surface.resume.set".into(),
            json!({
                "surface_id": SURFACE_ID,
                "source": "cli",
                "cwd": r"C:\work\repo",
                "command": "tmux attach -t work",
            })
        )]
    );
}

#[test]
fn surface_resume_set_argument_contract_errors_are_exact_and_pre_socket() {
    for (args, expected) in [
        (
            vec![
                "surface-resume",
                "set",
                "--shell",
                "tmux attach",
                "stray",
            ],
            "Error: surface resume set: unexpected argument 'stray' after --shell. Quote the full shell command or use -- <argv...>\n",
        ),
        (
            vec![
                "surface-resume",
                "set",
                "stray",
                "--",
                "opencode",
            ],
            "Error: surface resume set: unexpected argument 'stray' before --\n",
        ),
        (
            vec!["surface-resume", "set"],
            "Error: surface resume set requires --shell <command> or -- <argv...>\n",
        ),
        (
            // The space-separated blank value is caught by the pre-socket
            // value-option validation (CLI/cmux.swift:6737-6759)...
            vec!["surface-resume", "set", "--shell", "   "],
            "Error: surface resume set: --shell requires a value\n",
        ),
        (
            // ...while the inline form reaches the non-empty-command check
            // (CLI/cmux.swift:6612-6615).
            vec!["surface-resume", "set", "--shell=   "],
            "Error: surface resume set requires a non-empty command\n",
        ),
        (
            vec!["surface-resume", "set", "--surface"],
            "Error: surface resume set: --surface requires a value\n",
        ),
        (
            vec!["surface-resume", "set", "--name", "--kind", "agent", "--", "x"],
            "Error: surface resume set: --name requires a value\n",
        ),
        (
            vec!["surface-resume", "clear", "--source"],
            "Error: surface resume clear: --source requires a value\n",
        ),
        (
            vec!["surface-resume", "frobnicate"],
            "Error: Unsupported surface resume subcommand: frobnicate\n",
        ),
    ] {
        assert_failure(executable(None, &args), expected);
    }
}

#[test]
fn surface_resume_defaults_to_show_with_ambient_surface_env() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-show",
        "OK",
        HashMap::from([("surface.resume.get".to_owned(), resume_ok_result())]),
    );
    let mut command = base_command(Some(&pipe));
    command.env("CMUX_SURFACE_ID", SURFACE_ID);
    command.arg("surface-resume");
    let output = command.output().unwrap();
    assert_success_stdout(output, "opencode --resume\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2(
            "surface.resume.get".into(),
            json!({"surface_id": SURFACE_ID})
        )]
    );
}

#[test]
fn surface_resume_show_prints_no_resume_binding_when_binding_is_null() {
    let (pipe, _frame_rx) = spawn_scripted_server(
        "sr-none",
        "OK",
        HashMap::from([(
            "surface.resume.get".to_owned(),
            json!({"surface_id": SURFACE_ID, "cleared": false, "resume_binding": Value::Null}),
        )]),
    );
    let output = executable(Some(&pipe), &["surface-resume", "show"]);
    assert_success_stdout(output, "No resume binding\n");
}

#[test]
fn surface_resume_explicit_window_suppresses_ambient_env() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-window",
        "OK",
        HashMap::from([("surface.resume.get".to_owned(), resume_ok_result())]),
    );
    let mut command = base_command(Some(&pipe));
    command
        .env("CMUX_SURFACE_ID", SURFACE_ID)
        .env("CMUX_WORKSPACE_ID", WORKSPACE_ID);
    command.args(["surface-resume", "show", "--window", WINDOW_ID]);
    let output = command.output().unwrap();
    assert_success_stdout(output, "opencode --resume\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2(
            "surface.resume.get".into(),
            json!({"window_id": WINDOW_ID})
        )]
    );
}

#[test]
fn surface_resume_ambient_workspace_applies_only_without_surface_env() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-wsenv",
        "OK",
        HashMap::from([("surface.resume.get".to_owned(), resume_ok_result())]),
    );
    let mut command = base_command(Some(&pipe));
    command.env("CMUX_WORKSPACE_ID", WORKSPACE_ID);
    command.args(["surface-resume", "show"]);
    let output = command.output().unwrap();
    assert_success_stdout(output, "opencode --resume\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2(
            "surface.resume.get".into(),
            json!({"workspace_id": WORKSPACE_ID})
        )]
    );
}

#[test]
fn surface_resume_clear_sends_checkpoint_precedence_and_source_guards() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-clear",
        "OK",
        HashMap::from([(
            "surface.resume.clear".to_owned(),
            json!({"surface_id": SURFACE_ID, "cleared": true, "resume_binding": Value::Null}),
        )]),
    );
    let output = executable(
        Some(&pipe),
        &[
            "surface-resume",
            "clear",
            "--surface",
            SURFACE_ID,
            "--checkpoint",
            "old",
            "--checkpoint-id",
            "ses_9",
            "--source",
            "cli",
        ],
    );
    assert_success_stdout(output, "OK\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2(
            "surface.resume.clear".into(),
            json!({
                "surface_id": SURFACE_ID,
                "checkpoint_id": "ses_9",
                "source": "cli",
            })
        )]
    );
}

#[test]
fn surface_resume_workspace_ref_passes_through_without_window_scope() {
    // normalizeWorkspaceHandle: a ref without a window handle is passed through
    // verbatim (CLI/cmux.swift:6165-6168).
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-wsref",
        "OK",
        HashMap::from([("surface.resume.get".to_owned(), resume_ok_result())]),
    );
    let output = executable(
        Some(&pipe),
        &["surface-resume", "show", "--workspace", "workspace:2"],
    );
    assert_success_stdout(output, "opencode --resume\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2(
            "surface.resume.get".into(),
            json!({"workspace_id": "workspace:2"})
        )]
    );
}

#[test]
fn surface_resume_surface_index_resolves_through_surface_list() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-index",
        "OK",
        HashMap::from([
            (
                "surface.list".to_owned(),
                json!({"surfaces": [
                    {"id": SURFACE_ID, "ref": "surface:5", "index": 2},
                ]}),
            ),
            ("surface.resume.get".to_owned(), resume_ok_result()),
        ]),
    );
    let output = executable(Some(&pipe), &["surface-resume", "show", "--surface", "2"]);
    assert_success_stdout(output, "opencode --resume\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![
            WireFrame::V2("surface.list".into(), json!({})),
            WireFrame::V2(
                "surface.resume.get".into(),
                json!({"surface_id": SURFACE_ID})
            ),
        ]
    );
}

#[test]
fn surface_resume_window_scoped_surface_ref_validates_in_window() {
    // normalizeSurfaceHandle with an explicit window: workspace.list for the
    // window, then surface.list per workspace until the ref matches
    // (CLI/cmux.swift:6320-6330, 6398-6420).
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-winref",
        "OK",
        HashMap::from([
            (
                "workspace.list".to_owned(),
                json!({"workspaces": [{"id": WORKSPACE_ID, "ref": "workspace:1", "index": 0}]}),
            ),
            (
                "surface.list".to_owned(),
                json!({"surfaces": [{"id": SURFACE_ID, "ref": "surface:5", "index": 0}]}),
            ),
            ("surface.resume.get".to_owned(), resume_ok_result()),
        ]),
    );
    let output = executable(
        Some(&pipe),
        &[
            "surface-resume",
            "show",
            "--window",
            WINDOW_ID,
            "--surface",
            "surface:5",
        ],
    );
    assert_success_stdout(output, "opencode --resume\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![
            WireFrame::V2("workspace.list".into(), json!({"window_id": WINDOW_ID})),
            WireFrame::V2(
                "surface.list".into(),
                json!({"workspace_id": WORKSPACE_ID, "window_id": WINDOW_ID})
            ),
            WireFrame::V2(
                "surface.resume.get".into(),
                json!({"window_id": WINDOW_ID, "surface_id": SURFACE_ID})
            ),
        ]
    );
}

#[test]
fn surface_resume_is_reachable_through_the_surface_namespace() {
    let (pipe, frame_rx) = spawn_scripted_server(
        "sr-ns",
        "OK",
        HashMap::from([("surface.resume.set".to_owned(), resume_ok_result())]),
    );
    let mut command = base_command(Some(&pipe));
    command.env("PWD", r"C:\work\repo");
    command.args([
        "surface",
        "resume",
        "set",
        "--surface",
        SURFACE_ID,
        "--shell",
        "tmux attach",
    ]);
    let output = command.output().unwrap();
    assert_success_stdout(output, "OK\n");
    assert_eq!(
        drain_frames(&frame_rx),
        vec![WireFrame::V2(
            "surface.resume.set".into(),
            json!({
                "surface_id": SURFACE_ID,
                "source": "cli",
                "cwd": r"C:\work\repo",
                "command": "tmux attach",
            })
        )]
    );
}

// ---------------------------------------------------------------------------
// Byte-exact --help (CLI/cmux.swift:15477-15510, 16234-16262, 17051-17058)
// ---------------------------------------------------------------------------

#[test]
fn window_lifecycle_help_texts_are_byte_exact() {
    let new_window = executable(None, &["new-window", "--help"]);
    assert!(new_window.status.success());
    assert_eq!(
        String::from_utf8(new_window.stdout).unwrap(),
        "cmux new-window\n\nUsage: cmux new-window\n\nCreate a new window.\n\nExample:\n  cmux new-window\n"
    );

    let focus = executable(None, &["focus-window", "--help"]);
    assert!(focus.status.success());
    assert_eq!(
        String::from_utf8(focus.stdout).unwrap(),
        concat!(
            "cmux focus-window\n\n",
            "Usage: cmux focus-window --window <id|ref|index>\n\n",
            "Focus (bring to front) the specified window.\n\n",
            "Flags:\n",
            "  --window <id|ref|index>   Window to focus (required)\n\n",
            "Example:\n",
            "  cmux focus-window --window 0\n",
            "  cmux focus-window --window window:1\n"
        )
    );

    let close = executable(None, &["close-window", "--help"]);
    assert!(close.status.success());
    assert_eq!(
        String::from_utf8(close.stdout).unwrap(),
        concat!(
            "cmux close-window\n\n",
            "Usage: cmux close-window --window <id|ref|index>\n\n",
            "Close the specified window.\n\n",
            "Flags:\n",
            "  --window <id|ref|index>   Window to close (required)\n\n",
            "Example:\n",
            "  cmux close-window --window 0\n",
            "  cmux close-window --window window:1\n"
        )
    );
}

#[test]
fn window_help_is_the_canonical_unknown_command_error() {
    // Canonical (pinned e1825d40d): the run() help gate fires BEFORE socket
    // resolution (cmux.swift:3208-3217). `subcommandUsage("window")` has no
    // case (15026-17050), so `dispatchSubcommandHelp` returns false and the
    // gate throws `unknownCommandError("window")` — it does NOT fall through
    // to the socket dispatch. unknownCommandError
    // (CMUXCLI+CommandSuggestions.swift:4-11) is
    // "Unknown command '<cmd>'.[ Did you mean '<s>'?] Run 'cmux --help' for
    // the full command list." with EXIT CODE 2, rendered by the top-level
    // catch as "Error: <message>" on stderr. No pipe contact.
    let output = executable(None, &["window", "--help"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        output.stdout.is_empty(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Error: Unknown command 'window'. Run 'cmux --help' for the full command list.\n"
    );
}

#[test]
fn unknown_command_help_suggests_a_close_command_name() {
    // suggestedCommandName: best edit-distance ≤ 2 candidate from
    // topLevelCommandNames, skipping "__"-prefixed entries
    // (CMUXCLI+CommandSuggestions.swift:13-27). "pingg" → "ping".
    let output = executable(None, &["pingg", "--help"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        "Error: Unknown command 'pingg'. Did you mean 'ping'? Run 'cmux --help' for the full command list.\n"
    );
}

#[test]
fn surface_resume_help_text_is_byte_exact() {
    let output = executable(None, &["surface-resume", "--help"]);
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!(
            "cmux surface-resume\n\n",
            "Usage: cmux surface resume set [flags] -- <argv...>\n",
            "       cmux surface resume set [flags] --shell <command>\n",
            "       cmux surface resume show [--json] [flags]\n",
            "       cmux surface resume get [--json] [flags]\n",
            "       cmux surface resume clear [flags]\n",
            "\n",
            "Attach restart command metadata to a terminal surface.\n",
            "Public CLI bindings are stored for inspection and manual restore.\n",
            "\n",
            "Flags:\n",
            "  --workspace <id|ref|index>   Workspace context (default: $CMUX_WORKSPACE_ID)\n",
            "  --surface <id|ref|index>     Surface context (default: $CMUX_SURFACE_ID)\n",
            "  --window <id|ref|index>      Window context for workspace and surface refs/indexes\n",
            "  --cwd <path>             Working directory for restore (default: $PWD)\n",
            "  --name <name>            Display name for the binding\n",
            "  --kind <kind>            Binding kind, for example agent or tmux\n",
            "  --checkpoint <id>        Provider checkpoint or session id\n",
            "  --checkpoint-id <id>     Same as --checkpoint and takes precedence\n",
            "  --source <source>        Binding source label\n",
            "\n",
            "Examples:\n",
            "  cmux surface resume set --kind tmux --shell \"tmux attach -t work\"\n",
            "  cmux surface resume set --kind opencode --checkpoint ses_123 -- opencode --session ses_123\n",
            "  cmux surface resume show --json\n"
        )
    );
}
