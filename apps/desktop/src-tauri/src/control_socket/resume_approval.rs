use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use cmux_core::session::{AppSessionSnapshot, SessionSurfaceResumeBindingSnapshot};
use cmux_ipc::ControlCallResult;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use super::window_lifecycle::ResumeApprovalDecision;

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    std::env::var("CMUX_SURFACE_RESUME_APPROVAL_STORE_PATH")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            app.path()
                .app_data_dir()
                .ok()
                .map(|root| root.join("cmux").join("resume-commands.json"))
        })
}

fn signing_secret(app: &AppHandle) -> Option<Vec<u8>> {
    if let Some(secret) = std::env::var("CMUX_SURFACE_RESUME_APPROVAL_SECRET_B64")
        .ok()
        .and_then(|encoded| BASE64_STANDARD.decode(encoded).ok())
        .filter(|secret| !secret.is_empty())
    {
        return Some(secret);
    }
    let path = app
        .path()
        .app_data_dir()
        .ok()?
        .join("cmux")
        .join(".surface-resume-approval-secret");
    if let Ok(secret) = fs::read(&path) {
        if !secret.is_empty() {
            return Some(secret);
        }
    }
    fs::create_dir_all(path.parent()?).ok()?;
    let mut generated = Vec::with_capacity(32);
    generated.extend_from_slice(Uuid::new_v4().as_bytes());
    generated.extend_from_slice(Uuid::new_v4().as_bytes());
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(&generated).ok()?;
            Some(generated)
        }
        Err(_) => fs::read(path).ok().filter(|secret| !secret.is_empty()),
    }
}

fn tmux_binding(
    binding: &SessionSurfaceResumeBindingSnapshot,
) -> cmux_tmux::SurfaceResumeBindingSnapshot {
    cmux_tmux::SurfaceResumeBindingSnapshot::new(
        binding.name.as_deref(),
        binding.kind.as_deref(),
        &binding.command,
        binding.cwd.as_deref(),
        binding.checkpoint_id.as_deref(),
        binding.source.as_deref(),
        binding.environment.as_ref().map(|environment| {
            environment
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<HashMap<_, _>>()
        }),
        Some(binding.auto_resume),
        binding.updated_at,
    )
}

pub(super) fn promptless_cli_decision(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    result: &ControlCallResult,
) -> Option<ResumeApprovalDecision> {
    let ControlCallResult::Ok(payload) = result else {
        return None;
    };
    let payload = serde_json::Value::from(payload.clone());
    let surface_id = payload.get("surface_id")?.as_str()?;
    let binding = snapshot
        .windows
        .iter()
        .flat_map(|window| &window.tab_manager.workspaces)
        .flat_map(|workspace| {
            workspace
                .surface_resume_bindings
                .as_deref()
                .unwrap_or_default()
        })
        .find(|record| record.surface_id == surface_id)
        .map(|record| &record.binding)?;
    if binding.source.as_deref() != Some("cli") {
        return None;
    }

    let binding = tmux_binding(binding);
    let path = store_path(app)?;
    let secret = signing_secret(app)?;
    let existing = cmux_resume::matching_record(&binding, &path, &secret);
    let applied = cmux_resume::applying_promptless_cli_manual_approval_if_needed(
        &binding,
        existing.as_ref(),
        &path,
        &secret,
    )
    .unwrap_or_else(|| cmux_resume::applying_stored_approval(&binding, &path, &secret));
    Some(ResumeApprovalDecision {
        auto_resume: applied.auto_resume,
        approval_policy: applied.approval_policy.raw_value().to_owned(),
        approval_record_id: applied.approval_record_id,
    })
}
