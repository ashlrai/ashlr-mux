#![cfg(windows)]

mod support;

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};
use support::{process_proof_log_dir, DesktopFixture, PipeRpc};
use uuid::Uuid;

const SOURCE_TITLE: &str = "startup-restore-source";
const TARGET_TITLE: &str = "startup-restore-target";

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
    let window_refs = windows
        .iter()
        .map(|window| field(window, "ref"))
        .collect::<Result<Vec<_>, _>>()?;
    if window_refs != ["window:1", "window:2"] {
        return Err(format!(
            "restored window refs drifted: {window_refs:?}; windows={windows:?}"
        ));
    }
    for window in &windows {
        let id = field(window, "id")?;
        Uuid::parse_str(&id)
            .map_err(|error| format!("restored window id is not canonical UUID {id:?}: {error}"))?;
    }

    let default_workspaces = rows(&rpc.call("workspace.list", json!({})).await?, "workspaces")?;
    if !default_workspaces
        .iter()
        .any(|row| row.get("title").and_then(Value::as_str) == Some(TARGET_TITLE))
    {
        return Err(format!(
            "selectorless routing did not retain the last restored window: {default_workspaces:?}"
        ));
    }
    let workspace_sets = vec![
        rows(
            &rpc.call("workspace.list", json!({"window_ref":"window:1"}))
                .await?,
            "workspaces",
        )?,
        rows(
            &rpc.call("workspace.list", json!({"window_ref":"window:2"}))
                .await?,
            "workspaces",
        )?,
    ];
    let (source_window, source) = row_with_title(&workspace_sets, SOURCE_TITLE)?;
    let (target_window, target) = row_with_title(&workspace_sets, TARGET_TITLE)?;
    if (source_window, target_window) != (0, 1) {
        return Err(format!(
            "restored window ownership/order drifted: source_window={source_window}, source={source}, target_window={target_window}, target={target}"
        ));
    }

    let source_id = field(source, "id")?;
    let target_id = field(target, "id")?;
    let source_surfaces = surface_rows(&mut rpc, &source_id).await?;
    let target_surfaces = surface_rows(&mut rpc, &target_id).await?;
    if source_surfaces.len() != 1 || target_surfaces.len() != 3 {
        return Err(format!(
            "moved/closed state drifted: source={source_surfaces:?}, target={target_surfaces:?}"
        ));
    }
    let target_refs = target_surfaces
        .iter()
        .map(|surface| field(surface, "ref"))
        .collect::<Result<Vec<_>, _>>()?;
    if target_refs != ["surface:5", "surface:6", "surface:4"] {
        return Err(format!(
            "restored surface ordering drifted: {target_refs:?}; target={target_surfaces:?}"
        ));
    }
    let current = rpc
        .call("surface.current", json!({"workspace_id":target_id}))
        .await?;
    let focused_id = field(&current, "surface_id")?;
    if focused_id != moved_surface_id {
        return Err(format!(
            "restored focus changed semantic surface: expected moved {moved_surface_id}, current={current}"
        ));
    }
    let focused_ref = field(&current, "surface_ref")?;
    if focused_ref != "surface:6" {
        return Err(format!(
            "restored focus ref drifted: expected surface:6, current={current}, target={target_surfaces:?}"
        ));
    }
    let focused = target_surfaces
        .iter()
        .find(|surface| surface.get("id").and_then(Value::as_str) == Some(&focused_id))
        .ok_or_else(|| format!("focused surface missing from target list: {current}"))?;
    if focused.get("ref").and_then(Value::as_str) != Some("surface:6")
        || focused.get("focused").and_then(Value::as_bool) != Some(true)
        || focused.get("selected_in_pane").and_then(Value::as_bool) != Some(true)
    {
        return Err(format!("restored focus/selection drifted: {focused}"));
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

fn row_with_title<'a>(
    windows: &'a [Vec<Value>],
    title: &str,
) -> Result<(usize, &'a Value), String> {
    windows
        .iter()
        .enumerate()
        .find_map(|(index, rows)| {
            rows.iter()
                .find(|row| row.get("title").and_then(Value::as_str) == Some(title))
                .map(|row| (index, row))
        })
        .ok_or_else(|| format!("workspace {title:?} missing after restart: {windows:?}"))
}
