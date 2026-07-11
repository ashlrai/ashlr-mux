#![cfg(windows)]

use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use cmux_ipc::{control_pipe_path, serve_named_pipe, ControlCallResult, JsonValue};

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
        "OK workspace=workspace:1 window=window:2\n"
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
