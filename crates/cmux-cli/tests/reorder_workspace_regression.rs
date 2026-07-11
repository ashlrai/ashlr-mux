#![cfg(windows)]

use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use cmux_ipc::{control_pipe_path, serve_named_pipe, ControlCallResult, JsonValue};

#[test]
fn executable_routes_canonical_reorder_workspace_and_formats_dry_run() {
    let pipe = control_pipe_path(&format!(
        "cmux-reorder-workspace-{}-{}",
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
                    move |request: cmux_ipc::ControlRequest| {
                        request_tx.send((request.method, request.params)).unwrap();
                        ControlCallResult::Ok(
                            JsonValue::try_from(serde_json::json!({
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
                            }))
                            .unwrap(),
                        )
                    }
                })
                .await;
            });
    });

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
