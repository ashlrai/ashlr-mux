#![cfg(windows)]

use std::collections::HashMap;
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use cmux_ipc::{control_pipe_path, serve_named_pipe, ControlCallResult, JsonValue};
use serde_json::Value;

type CapturedRequest = (String, serde_json::Map<String, serde_json::Value>);

fn spawn_server(
    tag: &str,
    response: serde_json::Value,
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
                    let response = response.clone();
                    move |request: cmux_ipc::ControlRequest| {
                        request_tx.send((request.method, request.params)).unwrap();
                        ControlCallResult::Ok(JsonValue::try_from(response.clone()).unwrap())
                    }
                })
                .await;
            });
    });
    (pipe, request_rx)
}

fn spawn_method_server(
    tag: &str,
    responses: HashMap<String, serde_json::Value>,
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
                        let response = responses.get(&method).cloned().unwrap_or(Value::Null);
                        ControlCallResult::Ok(JsonValue::try_from(response).unwrap())
                    }
                })
                .await;
            });
    });
    (pipe, request_rx)
}

#[test]
fn executable_routes_tmux_absolute_resize_through_pane_metrics() {
    let (pipe, request_rx) = spawn_method_server(
        "tmux-resize-pane",
        HashMap::from([
            (
                "workspace.list".to_string(),
                serde_json::json!({"workspaces":[{
                    "id":"workspace-a", "ref":"workspace:1", "index":0
                }]}),
            ),
            (
                "pane.list".to_string(),
                serde_json::json!({"panes":[{
                    "id":"pane-b", "ref":"pane:2", "index":1,
                    "focused":true, "selected_surface_id":"surface-b",
                    "cell_width_px":9, "cell_height_px":18
                }]}),
            ),
            (
                "pane.resize".to_string(),
                serde_json::json!({"pane_ref":"pane:2"}),
            ),
        ]),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "__tmux-compat",
            "resize-pane",
            "-tworkspace-a.pane:2",
            "-x13",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());

    let requests = (0..3)
        .map(|_| request_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(requests[0].0, "workspace.list");
    assert_eq!(requests[1].0, "pane.list");
    assert_eq!(
        Value::Object(requests[1].1.clone()),
        serde_json::json!({"workspace_id":"workspace-a"})
    );
    assert_eq!(requests[2].0, "pane.resize");
    assert_eq!(
        Value::Object(requests[2].1.clone()),
        serde_json::json!({
            "workspace_id":"workspace-a", "pane_id":"pane-b",
            "absolute_axis":"horizontal", "target_pixels":117
        })
    );
}

#[test]
fn executable_routes_tmux_select_pane_through_pane_focus() {
    let (pipe, request_rx) = spawn_method_server(
        "tmux-select-pane",
        HashMap::from([
            (
                "workspace.list".to_string(),
                serde_json::json!({"workspaces":[{
                    "id":"workspace-a", "ref":"workspace:1", "index":0
                }]}),
            ),
            (
                "pane.list".to_string(),
                serde_json::json!({"panes":[{
                    "id":"pane-b", "ref":"pane:2", "index":1, "focused":false
                }]}),
            ),
            (
                "pane.focus".to_string(),
                serde_json::json!({"pane_ref":"pane:2"}),
            ),
        ]),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["__tmux-compat", "select-pane", "-tworkspace-a.pane:2"])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());

    let requests = (0..3)
        .map(|_| request_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(requests[0].0, "workspace.list");
    assert_eq!(requests[1].0, "pane.list");
    assert_eq!(requests[2].0, "pane.focus");
    assert_eq!(
        Value::Object(requests[2].1.clone()),
        serde_json::json!({"workspace_id":"workspace-a", "pane_id":"pane-b"})
    );
}

#[test]
fn executable_routes_tmux_select_window_through_workspace_select() {
    let (pipe, request_rx) = spawn_method_server(
        "tmux-select-window",
        HashMap::from([
            (
                "workspace.list".to_string(),
                serde_json::json!({"workspaces":[{
                    "id":"workspace-b", "ref":"workspace:2", "index":1, "title":"Beta"
                }]}),
            ),
            (
                "workspace.select".to_string(),
                serde_json::json!({"workspace_ref":"workspace:2"}),
            ),
        ]),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["__tmux-compat", "select-window", "-tBeta"])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let requests = (0..2)
        .map(|_| request_rx.recv_timeout(Duration::from_secs(5)).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(requests[0].0, "workspace.list");
    assert_eq!(requests[1].0, "workspace.select");
    assert_eq!(
        Value::Object(requests[1].1.clone()),
        serde_json::json!({"workspace_id":"workspace-b"})
    );
}

#[test]
fn executable_routes_last_window_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "last-window",
        serde_json::json!({
            "workspace_id":"workspace-b", "workspace_ref":"workspace:2",
            "window_id":"window-a", "window_ref":"window:1"
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["last-window", "--window", "window:1"])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK workspace:2\n"
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "workspace.last");
    assert_eq!(
        Value::Object(params),
        serde_json::json!({"window_ref":"window:1"})
    );
}

#[test]
fn executable_tmux_version_is_local_and_canonical() {
    for flag in ["-V", "-v"] {
        let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
            .args(["__tmux-compat", flag])
            .env("CMUX_SOCKET_PATH", r"\\.\pipe\cmux-must-not-connect")
            .env_remove("CMUX_SOCKET")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "tmux 3.4\n");
    }
}

#[test]
fn executable_routes_list_pane_surfaces_and_formats_rows() {
    let (pipe, request_rx) = spawn_server(
        "list-pane-surfaces",
        serde_json::json!({"surfaces":[
            {"id":"surface-a", "ref":"surface:1", "index":0, "title":"Shell", "type":"terminal", "selected":true},
            {"id":"surface-b", "ref":"surface:2", "index":1, "title":"Logs", "type":"terminal", "selected":false}
        ]}),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "list-pane-surfaces",
            "--workspace",
            "workspace:2",
            "--pane",
            "pane:1",
            "--window",
            "window:1",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "* surface:1  Shell  [selected]\n  surface:2  Logs\n"
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "pane.surfaces");
    assert_eq!(
        Value::Object(params),
        serde_json::json!({
            "workspace_ref":"workspace:2", "pane_ref":"pane:1", "window_ref":"window:1"
        })
    );
}

#[test]
fn executable_routes_focus_pane_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "focus-pane",
        serde_json::json!({
            "window_ref": "window:1",
            "workspace_ref": "workspace:2",
            "pane_ref": "pane:3",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["focus-pane", "3", "--workspace", "2", "--window", "1"])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK pane:3 workspace:2\n"
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "pane.focus");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "__cmux_cli_command": "focus-pane",
            "pane_ref": "pane:3",
            "workspace_ref": "workspace:2",
            "window_ref": "window:1",
        })
    );
}

#[test]
fn executable_routes_canonical_reorder_workspace_and_formats_dry_run() {
    let (pipe, request_rx) = spawn_server(
        "reorder-workspace",
        serde_json::json!({
            "workspace_id": "ws-3",
            "workspace_ref": "workspace:1",
            "window_id": "win-1",
            "window_ref": "window:1",
            "from_index": 2,
            "to_index": 0,
            "index": 0,
            "dry_run": true,
            "plan": [{
                "workspace_id": "ws-3",
                "workspace_ref": "workspace:1",
                "window_id": "win-1",
                "window_ref": "window:1",
                "from_index": 2,
                "to_index": 0,
            }],
            "events": [],
        }),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "reorder-workspace",
            "--workspace",
            "workspace:3",
            "--index",
            "0",
            "--window",
            "window:1",
            "--dry-run",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK plan workspace=workspace:1 window=window:1 index=0\n"
    );

    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "workspace.reorder");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "workspace_ref": "workspace:3",
            "index": 0,
            "window_ref": "window:1",
            "dry_run": true,
        })
    );
}

#[test]
fn executable_routes_atomic_reorder_workspaces_and_formats_plan() {
    let first = "00000000-0000-0000-0000-000000000003";
    let second = "00000000-0000-0000-0000-000000000001";
    let (pipe, request_rx) = spawn_server(
        "reorder-workspaces",
        serde_json::json!({
            "window_id": "win-1",
            "window_ref": "window:1",
            "dry_run": false,
            "plan": [
                {
                    "workspace_id": first,
                    "workspace_ref": "workspace:1",
                    "window_id": "win-1",
                    "window_ref": "window:1",
                    "from_index": 2,
                    "to_index": 0,
                },
                {
                    "workspace_id": second,
                    "workspace_ref": "workspace:2",
                    "window_id": "win-1",
                    "window_ref": "window:1",
                    "from_index": 0,
                    "to_index": 1,
                },
            ],
            "events": [],
        }),
    );

    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "reorder-workspaces",
            "--order",
            &format!("workspace:3,{second}"),
            "--window",
            "window:1",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK workspace=workspace:1 window=window:1 index=0\n\
         OK workspace=workspace:2 window=window:1 index=1\n"
    );

    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "workspace.reorder_many");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "workspace_ids": ["workspace:3", second],
            "window_ref": "window:1",
        })
    );
}

#[test]
fn executable_routes_reorder_surface_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "reorder-surface",
        serde_json::json!({
            "window_ref": "window:1",
            "workspace_ref": "workspace:1",
            "pane_ref": "pane:2",
            "surface_ref": "surface:1",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "reorder-surface",
            "surface:3",
            "--before",
            "surface:1",
            "--workspace",
            "workspace:1",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK surface=surface:1 pane=pane:2 workspace=workspace:1\n"
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.reorder");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "surface_ref": "surface:3",
            "before_surface_ref": "surface:1",
            "workspace_ref": "workspace:1",
            "focus": false,
        })
    );
}

#[test]
fn executable_routes_move_surface_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "move-surface",
        serde_json::json!({
            "window_ref": "window:1",
            "workspace_ref": "workspace:2",
            "pane_ref": "pane:1",
            "surface_ref": "surface:2",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "move-surface",
            "surface:3",
            "--workspace",
            "workspace:2",
            "--pane",
            "pane:1",
            "--index",
            "0",
            "--focus",
            "true",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK surface=surface:2 pane=pane:1 workspace=workspace:2 window=window:1\n"
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.move");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "surface_ref": "surface:3",
            "workspace_ref": "workspace:2",
            "pane_ref": "pane:1",
            "index": 0,
            "focus": true,
        })
    );
}

#[test]
fn executable_routes_move_workspace_to_window_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "move-workspace-to-window",
        serde_json::json!({
            "workspace_ref": "workspace:1",
            "window_ref": "window:2",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "move-workspace-to-window",
            "--workspace",
            "workspace:2",
            "--window",
            "window:1",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK workspace:1 window:2\n"
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "workspace.move_to_window");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "workspace_ref": "workspace:2",
            "window_ref": "window:1",
        })
    );
}

#[test]
fn executable_routes_split_off_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "split-off",
        serde_json::json!({
            "surface_ref": "surface:2",
            "pane_ref": "pane:2",
            "workspace_ref": "workspace:1",
            "window_ref": "window:1",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "split-off",
            "--panel",
            "surface:3",
            "down",
            "--workspace",
            "workspace:1",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK surface=surface:2 pane=pane:2 workspace=workspace:1 window=window:1\n"
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.split_off");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "surface_ref": "surface:3",
            "workspace_ref": "workspace:1",
            "direction": "down",
            "focus": false,
        })
    );
}

#[test]
fn executable_routes_drag_surface_to_split_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "drag-surface-to-split",
        serde_json::json!({
            "surface_ref": "surface:2",
            "pane_ref": "pane:2",
            "workspace_ref": "workspace:1",
            "window_ref": "window:1",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "drag-surface-to-split",
            "--surface",
            "surface:3",
            "left",
            "--focus",
            "true",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "OK surface=surface:2 pane=pane:2 workspace=workspace:1 window=window:1\n"
    );
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "surface.drag_to_split");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "surface_ref": "surface:3",
            "direction": "left",
            "focus": true,
        })
    );
}

#[test]
fn executable_routes_swap_pane_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "swap-pane",
        serde_json::json!({
            "pane_ref": "pane:1",
            "target_pane_ref": "pane:2",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "swap-pane",
            "--pane",
            "pane:1",
            "--target-pane",
            "pane:2",
            "--focus",
            "false",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK\n");
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "pane.swap");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "pane_ref": "pane:1",
            "target_pane_ref": "pane:2",
            "focus": false,
        })
    );
}

#[test]
fn executable_routes_break_pane_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "break-pane",
        serde_json::json!({
            "surface_ref": "surface:1",
            "pane_ref": "pane:1",
            "workspace_ref": "workspace:2",
            "window_ref": "window:1",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "break-pane",
            "--pane",
            "pane:2",
            "--surface",
            "surface:3",
            "--workspace",
            "workspace:1",
            "--no-focus",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK\n");
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "pane.break");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "pane_ref": "pane:2",
            "surface_ref": "surface:3",
            "workspace_ref": "workspace:1",
            "focus": false,
        })
    );
}

#[test]
fn executable_routes_join_pane_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "join-pane",
        serde_json::json!({
            "surface_ref": "surface:1",
            "pane_ref": "pane:2",
            "workspace_ref": "workspace:1",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "join-pane",
            "--target-pane",
            "pane:2",
            "--surface",
            "surface:3",
            "--workspace",
            "workspace:1",
            "--no-focus",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK\n");
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "pane.join");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "target_pane_ref": "pane:2",
            "surface_ref": "surface:3",
            "workspace_ref": "workspace:1",
            "focus": false,
        })
    );
}

#[test]
fn executable_routes_last_pane_and_formats_result() {
    let (pipe, request_rx) = spawn_server(
        "last-pane",
        serde_json::json!({
            "pane_ref": "pane:2",
            "workspace_ref": "workspace:1",
            "window_ref": "window:1",
        }),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args(["last-pane", "--workspace", "workspace:2"])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK pane:2\n");
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "pane.last");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "__cmux_cli_command": "last-pane",
            "workspace_ref": "workspace:2",
        })
    );
}

#[test]
fn executable_routes_resize_pane_and_formats_result() {
    let (pipe, request_rx) = spawn_server("resize-pane", serde_json::json!({"pane_ref": "pane:2"}));
    let output = Command::new(env!("CARGO_BIN_EXE_cmux"))
        .args([
            "resize-pane",
            "--pane",
            "pane:3",
            "--workspace",
            "workspace:1",
            "-U",
            "--amount",
            "8",
        ])
        .env("CMUX_SOCKET_PATH", &pipe)
        .env_remove("CMUX_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "OK pane:2\n");
    let (method, params) = request_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(method, "pane.resize");
    assert_eq!(
        serde_json::Value::Object(params),
        serde_json::json!({
            "pane_ref": "pane:3",
            "workspace_ref": "workspace:1",
            "direction": "up",
            "amount": 8,
        })
    );
}
