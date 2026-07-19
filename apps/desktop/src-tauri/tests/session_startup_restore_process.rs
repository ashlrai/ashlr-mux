#![cfg(windows)]

mod support;

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use support::{process_proof_log_dir, DesktopFixture, PipeRpc};

const SOURCE_TITLE: &str = "startup-restore-source";
const TARGET_TITLE: &str = "startup-restore-target";
const MOVED_TITLE: &str = "startup-restore-moved";

#[test]
fn normal_restart_restores_multiwindow_surface_ownership_and_focus() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build startup-restore runtime");
    runtime.block_on(async {
        run_process_proof()
            .await
            .unwrap_or_else(|error| panic!("startup restore process proof failed: {error}"));
    });
}

async fn run_process_proof() -> Result<(), String> {
    let desktop_exe = Path::new(env!("CARGO_BIN_EXE_cmux-desktop"));
    let mut desktop = DesktopFixture::launch(desktop_exe, &process_proof_log_dir())?;
    let mut rpc = desktop.connect(Duration::from_secs(20)).await?;
    rpc.call("system.ping", json!({})).await?;
    tokio::time::sleep(Duration::from_millis(500)).await;

    let source_workspace_id = field(
        &rpc.call("workspace.create", json!({"title":SOURCE_TITLE}))
            .await?,
        "workspace_id",
    )?;
    let auxiliary_window_id = field(&rpc.call("window.create", json!({})).await?, "window_id")?;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let target_workspace_id = field(
        &rpc.call(
            "workspace.create",
            json!({"window_id":auxiliary_window_id, "title":TARGET_TITLE}),
        )
        .await?,
        "workspace_id",
    )?;

    let moved_surface_id = field(
        &rpc.call(
            "surface.create",
            json!({"workspace_id":source_workspace_id, "type":"terminal"}),
        )
        .await?,
        "surface_id",
    )?;
    rpc.call(
        "surface.set_title",
        json!({"workspace_id":source_workspace_id, "surface_id":moved_surface_id, "title":MOVED_TITLE}),
    )
    .await?;
    let target_pane_id = field(
        &rpc.call(
            "surface.create",
            json!({"workspace_id":target_workspace_id, "type":"terminal"}),
        )
        .await?,
        "pane_id",
    )?;
    rpc.call(
        "surface.move",
        json!({"surface_id":moved_surface_id, "pane_id":target_pane_id, "focus":true}),
    )
    .await?;

    let closed_surface_id = field(
        &rpc.call(
            "surface.create",
            json!({"workspace_id":target_workspace_id, "type":"terminal"}),
        )
        .await?,
        "surface_id",
    )?;
    rpc.call("surface.close", json!({"surface_id":closed_surface_id}))
        .await?;
    rpc.call("surface.focus", json!({"surface_id":moved_surface_id}))
        .await?;

    drop(rpc);
    desktop.restart()?;
    let mut rpc = desktop.connect(Duration::from_secs(20)).await?;

    let windows = rows(&rpc.call("window.list", json!({})).await?, "windows")?;
    if windows.len() != 2 {
        return Err(format!(
            "restored {} windows instead of 2: {windows:?}",
            windows.len()
        ));
    }
    let workspaces = rows(&rpc.call("workspace.list", json!({})).await?, "workspaces")?;
    let source = row_with_title(&workspaces, SOURCE_TITLE)?;
    let target = row_with_title(&workspaces, TARGET_TITLE)?;
    if source.get("window_id") == target.get("window_id") {
        return Err(format!(
            "cross-window ownership collapsed: source={source}, target={target}"
        ));
    }

    let source_id = field(source, "workspace_id")?;
    let target_id = field(target, "workspace_id")?;
    let source_surfaces = surface_rows(&mut rpc, &source_id).await?;
    let target_surfaces = surface_rows(&mut rpc, &target_id).await?;
    if contains_title(&source_surfaces, MOVED_TITLE)
        || !contains_title(&target_surfaces, MOVED_TITLE)
        || source_surfaces.len() != 1
        || target_surfaces.len() != 3
    {
        return Err(format!(
            "moved/closed state drifted: source={source_surfaces:?}, target={target_surfaces:?}"
        ));
    }
    let current = rpc
        .call("surface.current", json!({"workspace_id":target_id}))
        .await?;
    let focused_id = field(&current, "surface_id")?;
    let focused = target_surfaces
        .iter()
        .find(|surface| surface.get("surface_id").and_then(Value::as_str) == Some(&focused_id))
        .ok_or_else(|| format!("focused surface missing from target list: {current}"))?;
    if focused.get("title").and_then(Value::as_str) != Some(MOVED_TITLE) {
        return Err(format!("focus did not follow moved surface: {focused}"));
    }

    drop(rpc);
    desktop.stop()
}

async fn surface_rows(rpc: &mut PipeRpc, workspace_id: &str) -> Result<Vec<Value>, String> {
    rows(
        &rpc.call("surface.list", json!({"workspace_id":workspace_id}))
            .await?,
        "surfaces",
    )
}

fn rows(payload: &Value, key: &str) -> Result<Vec<Value>, String> {
    payload
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| format!("response omitted {key}: {payload}"))
}

fn field(payload: &Value, key: &str) -> Result<String, String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("response omitted {key}: {payload}"))
}

fn row_with_title<'a>(rows: &'a [Value], title: &str) -> Result<&'a Value, String> {
    rows.iter()
        .find(|row| row.get("title").and_then(Value::as_str) == Some(title))
        .ok_or_else(|| format!("workspace {title:?} missing after restart: {rows:?}"))
}

fn contains_title(rows: &[Value], title: &str) -> bool {
    rows.iter()
        .any(|row| row.get("title").and_then(Value::as_str) == Some(title))
}
