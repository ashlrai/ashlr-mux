#![cfg(windows)]

mod support;

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use support::{process_proof_log_dir, DesktopFixture};

#[test]
fn production_pipe_set_font_is_event_only_and_reports_subscribers() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build process-proof async runtime");
    runtime.block_on(async {
        run_process_proof()
            .await
            .unwrap_or_else(|error| panic!("terminal set-font process proof failed: {error}"));
    });
}

async fn run_process_proof() -> Result<(), String> {
    let desktop_exe = Path::new(env!("CARGO_BIN_EXE_cmux-desktop"));
    let mut desktop = DesktopFixture::launch(desktop_exe, &process_proof_log_dir())?;
    let mut rpc = desktop.connect(Duration::from_secs(20)).await?;
    if rpc.call("system.ping", json!({})).await? != json!("pong") {
        return Err("owned desktop returned an unexpected ping".into());
    }

    let capabilities = rpc.call("system.capabilities", json!({})).await?;
    let methods = capabilities
        .get("methods")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("system.capabilities omitted methods: {capabilities}"))?;
    let mobile_count = methods
        .iter()
        .filter(|method| method.as_str() == Some("mobile.terminal.set_font"))
        .count();
    let bare_count = methods
        .iter()
        .filter(|method| method.as_str() == Some("terminal.set_font"))
        .count();
    if mobile_count != 1 || bare_count != 0 {
        return Err(format!("set-font public identity drifted: {capabilities}"));
    }

    let session_before = rpc.call("workspace.current", json!({})).await?;
    let without_subscriber = rpc
        .call("mobile.terminal.set_font", json!({"font_size": 12.5}))
        .await?;
    if without_subscriber != json!({"ok": true, "font_size": 12.5, "delivered": false}) {
        return Err(format!(
            "no-subscriber response drifted: {without_subscriber}"
        ));
    }
    if rpc.call("workspace.current", json!({})).await? != session_before {
        return Err("set-font mutated session or focus without a subscriber".into());
    }

    let (mut events, ack) = desktop
        .subscribe(&["terminal.set_font"], Duration::from_secs(5))
        .await?;
    if ack.get("type").and_then(Value::as_str) != Some("ack")
        || ack.pointer("/filters/names") != Some(&json!(["terminal.set_font"]))
        || ack.get("heartbeat").is_some()
    {
        return Err(format!("events.stream acknowledgement drifted: {ack}"));
    }

    let with_subscriber = rpc
        .call(
            "mobile.terminal.set_font",
            json!({
                "font_size": "13.75",
                "workspace_id": "workspace-scope",
                "surface_id": "surface-scope",
            }),
        )
        .await?;
    if with_subscriber != json!({"ok": true, "font_size": 13.75, "delivered": true}) {
        return Err(format!("subscriber response drifted: {with_subscriber}"));
    }
    let event = events.next().await?;
    for (pointer, expected) in [
        ("/type", json!("event")),
        ("/protocol", json!("cmux-events")),
        ("/version", json!(1)),
        ("/name", json!("terminal.set_font")),
        ("/category", json!("terminal")),
        ("/source", json!("socket.v2")),
        ("/workspace_id", json!("workspace-scope")),
        ("/surface_id", json!("surface-scope")),
        ("/pane_id", Value::Null),
        ("/window_id", Value::Null),
        ("/payload/font_size", json!(13.75)),
        ("/payload/workspace_id", json!("workspace-scope")),
        ("/payload/surface_id", json!("surface-scope")),
    ] {
        if event.pointer(pointer) != Some(&expected) {
            return Err(format!("event field {pointer} drifted: {event}"));
        }
    }

    for (input, message, data) in [
        (json!({}), "Missing or invalid font_size", None),
        (
            json!({"font_size": "large"}),
            "Missing or invalid font_size",
            None,
        ),
        (
            json!({"font_size": false}),
            "font_size must be a positive number of points",
            Some(json!({"font_size": 0.0})),
        ),
        (
            json!({"font_size": -1}),
            "font_size must be a positive number of points",
            Some(json!({"font_size": -1.0})),
        ),
    ] {
        let error = rpc.call_error("mobile.terminal.set_font", input).await?;
        if error.pointer("/error/code").and_then(Value::as_str) != Some("invalid_params")
            || error.pointer("/error/message").and_then(Value::as_str) != Some(message)
            || error.pointer("/error/data") != data.as_ref()
        {
            return Err(format!("set-font validation drifted: {error}"));
        }
    }
    if rpc.call("workspace.current", json!({})).await? != session_before {
        return Err("set-font mutated session or focus".into());
    }

    drop(events);
    drop(rpc);
    desktop.stop()?;
    Ok(())
}
