#![cfg(windows)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use support::{process_proof_log_dir, DesktopFixture, PipeRpc};

const FROZEN_METHODS: [&str; 4] = [
    "mobile.terminal.create",
    "mobile.terminal.input",
    "terminal.create",
    "terminal.input",
];

#[test]
fn production_pipe_create_is_cold_and_input_materializes_owned_conpty() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build process-proof async runtime");
    runtime.block_on(async {
        run_process_proof()
            .await
            .unwrap_or_else(|error| panic!("terminal create/input process proof failed: {error}"));
    });
}

async fn run_process_proof() -> Result<(), String> {
    let desktop_exe = Path::new(env!("CARGO_BIN_EXE_cmux-desktop"));
    let mut desktop = DesktopFixture::launch(desktop_exe, &process_proof_log_dir())?;
    let owned_desktop_pid = desktop.pid;
    let mut rpc = desktop.connect(Duration::from_secs(20)).await?;
    if rpc.call("system.ping", json!({})).await? != json!("pong") {
        return Err("owned desktop returned an unexpected ping".into());
    }

    let capabilities = rpc.call("system.capabilities", json!({})).await?;
    let methods = capabilities
        .get("methods")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("system.capabilities omitted methods: {capabilities}"))?;
    for method in FROZEN_METHODS {
        let count = methods
            .iter()
            .filter(|candidate| candidate.as_str() == Some(method))
            .count();
        if count != 1 {
            return Err(format!("{method} advertised {count} times"));
        }
    }

    let current_before = rpc.call("workspace.current", json!({})).await?;
    let workspace_id = current_before
        .get("workspace_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("workspace.current omitted workspace_id: {current_before}"))?
        .to_owned();
    let focused_before = current_before.get("surface_id").cloned();

    let created = rpc
        .call("terminal.create", json!({"workspace_id": workspace_id}))
        .await?;
    let surface_id = created
        .get("created_terminal_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("terminal.create omitted created_terminal_id: {created}"))?
        .to_owned();
    let created_terminal = find_terminal(&created, &surface_id)?;
    if created_terminal.get("is_ready").and_then(Value::as_bool) != Some(false) {
        return Err(format!(
            "terminal.create was not metadata-only: {created_terminal}"
        ));
    }
    let current_after = rpc.call("workspace.current", json!({})).await?;
    if current_after.get("workspace_id") != Some(&json!(workspace_id))
        || current_after.get("surface_id") != focused_before.as_ref()
    {
        return Err(format!(
            "terminal.create stole selection: before={current_before}, after={current_after}"
        ));
    }
    let debug_before = rpc
        .call("debug.terminals", json!({"workspace_id": workspace_id}))
        .await?;
    if terminal_debug_row(&debug_before, &surface_id)?
        .get("runtime_surface_ready")
        .and_then(Value::as_bool)
        != Some(false)
    {
        return Err(format!(
            "cold terminal already had a runtime: {debug_before}"
        ));
    }
    poll_persisted_surface(desktop.profile_path(), &surface_id, Duration::from_secs(5))?;

    let first = rpc
        .call(
            "terminal.input",
            json!({
                "workspace_id": workspace_id,
                "surface_id": surface_id,
                "text": "echo create-input-proof\r",
            }),
        )
        .await?;
    if first.get("queued").and_then(Value::as_bool) != Some(true) {
        return Err(format!("first cold terminal.input was not queued: {first}"));
    }
    let root_pid = poll_runtime_root_pid(
        &mut rpc,
        &workspace_id,
        &surface_id,
        Duration::from_secs(10),
    )
    .await?;
    if root_pid == 0 || root_pid == u64::from(owned_desktop_pid) {
        return Err(format!(
            "invalid owned ConPTY evidence: desktop={owned_desktop_pid}, root={root_pid}"
        ));
    }
    let root_pid =
        u32::try_from(root_pid).map_err(|_| format!("ConPTY root PID exceeds u32: {root_pid}"))?;
    let root_created_at = cmux_process::process_creation_time(root_pid)
        .ok_or_else(|| format!("could not record ConPTY process identity for {root_pid}"))?;

    let mobile = rpc
        .call(
            "mobile.terminal.create",
            json!({"workspace_id": workspace_id}),
        )
        .await?;
    let mobile_surface = mobile
        .get("created_terminal_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("mobile.terminal.create omitted id: {mobile}"))?;
    if find_terminal(&mobile, mobile_surface)?
        .get("is_ready")
        .and_then(Value::as_bool)
        != Some(false)
    {
        return Err(format!("mobile create eagerly materialized: {mobile}"));
    }
    let oversized = "x".repeat(1024 * 1024 + 1);
    let full = rpc
        .call_error(
            "mobile.terminal.input",
            json!({
                "workspace_id": workspace_id,
                "surface_id": mobile_surface,
                "text": oversized,
            }),
        )
        .await?;
    if full.pointer("/error/code").and_then(Value::as_str) != Some("input_queue_full") {
        return Err(format!("mobile cold queue limit drifted: {full}"));
    }
    drop(rpc);
    desktop.stop()?;
    poll_process_identity_exit(root_pid, root_created_at, Duration::from_secs(5))?;
    Ok(())
}

fn poll_process_identity_exit(pid: u32, created_at: u64, wait: Duration) -> Result<(), String> {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if cmux_process::process_creation_time(pid) != Some(created_at) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(format!(
        "owned ConPTY root identity {pid}/{created_at} survived desktop teardown"
    ))
}

fn find_terminal<'a>(payload: &'a Value, surface_id: &str) -> Result<&'a Value, String> {
    payload
        .get("workspaces")
        .and_then(Value::as_array)
        .and_then(|workspaces| workspaces.first())
        .and_then(|workspace| workspace.get("terminals"))
        .and_then(Value::as_array)
        .and_then(|terminals| {
            terminals
                .iter()
                .find(|terminal| terminal.get("id").and_then(Value::as_str) == Some(surface_id))
        })
        .ok_or_else(|| format!("created terminal {surface_id} absent: {payload}"))
}

fn terminal_debug_row<'a>(payload: &'a Value, surface_id: &str) -> Result<&'a Value, String> {
    payload
        .get("terminals")
        .and_then(Value::as_array)
        .and_then(|terminals| {
            terminals.iter().find(|terminal| {
                terminal.get("surface_id").and_then(Value::as_str) == Some(surface_id)
            })
        })
        .ok_or_else(|| format!("debug.terminals omitted {surface_id}: {payload}"))
}

async fn poll_runtime_root_pid(
    rpc: &mut PipeRpc,
    workspace_id: &str,
    surface_id: &str,
    wait: Duration,
) -> Result<u64, String> {
    let deadline = Instant::now() + wait;
    let mut last = Value::Null;
    while Instant::now() < deadline {
        last = rpc
            .call("debug.terminals", json!({"workspace_id": workspace_id}))
            .await?;
        if let Some(root_pid) = terminal_debug_row(&last, surface_id)?
            .get("root_pid")
            .and_then(Value::as_u64)
        {
            return Ok(root_pid);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Err(format!("runtime omitted root_pid within {wait:?}: {last}"))
}

fn poll_persisted_surface(profile: &Path, surface_id: &str, wait: Duration) -> Result<(), String> {
    let deadline = Instant::now() + wait;
    while Instant::now() < deadline {
        if find_named_files(profile, "session-current.json")
            .into_iter()
            .any(|path| {
                std::fs::read_to_string(path)
                    .ok()
                    .is_some_and(|contents| contents.contains(surface_id))
            })
        {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    Err(format!(
        "persisted session under {} omitted {surface_id}",
        profile.display()
    ))
}

fn find_named_files(root: &Path, name: &str) -> Vec<std::path::PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut found = Vec::new();
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
                found.push(path);
            }
        }
    }
    found
}
