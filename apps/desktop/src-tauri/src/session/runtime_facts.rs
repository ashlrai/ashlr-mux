use super::*;

/// Set or clear the custom title for `panel_id` in the active workspace. Pure —
/// delegates to [`session_ops::set_panel_title`]. Returns whether title
/// metadata actually changed.
pub(super) fn apply_set_panel_title(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    title: &str,
) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_panel_title(workspace, panel_id, title)
}

/// Set or clear the pinned state for `panel_id` in the active workspace. Pure —
/// delegates to [`session_ops::set_panel_pinned`]. Returns whether pin metadata
/// actually changed.
pub(super) fn apply_set_panel_pinned(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    pinned: bool,
) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_panel_pinned(workspace, panel_id, pinned)
}

/// Set or clear the unread state for `panel_id` in the active workspace,
/// stamping new unread markers for notification ordering. Returns whether
/// unread metadata actually changed.
#[cfg(test)]
pub(super) fn apply_set_panel_unread(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    unread: bool,
) -> bool {
    apply_set_panel_unread_at(snapshot, panel_id, unread, current_unix_timestamp_seconds())
}

pub(super) fn apply_set_panel_unread_at(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    unread: bool,
    unread_at: i64,
) -> bool {
    let Some(workspace) = active_workspace_slot(snapshot) else {
        return false;
    };
    session_ops::set_panel_unread_at(workspace, panel_id, unread, Some(unread_at))
}

pub(super) fn apply_set_panel_listening_ports(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    ports: &[u16],
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_panel_listening_ports(workspace, panel_id, ports)
}

#[cfg(test)]
pub(super) fn apply_set_panel_tty(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    tty: &str,
) -> bool {
    apply_set_panel_tty_at(
        snapshot,
        workspace_index,
        panel_id,
        tty,
        current_unix_timestamp_seconds(),
    )
}

pub(super) fn apply_set_panel_tty_at(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    tty: &str,
    updated_at: i64,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_panel_tty(workspace, panel_id, tty, updated_at)
}

#[cfg(test)]
pub(super) fn apply_set_panel_shell_activity(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    state: SessionPanelShellActivityStateSnapshot,
) -> bool {
    apply_set_panel_shell_activity_at(
        snapshot,
        workspace_index,
        panel_id,
        state,
        current_unix_timestamp_seconds(),
    )
}

pub(super) fn apply_set_panel_shell_activity_at(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    state: SessionPanelShellActivityStateSnapshot,
    updated_at: i64,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_panel_shell_activity(workspace, panel_id, state, updated_at)
}

pub(super) fn apply_set_workspace_agent_listening_ports(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    ports: &[u16],
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_agent_listening_ports(workspace, ports)
}

#[cfg(test)]
pub(super) fn apply_set_workspace_agent_pid(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    key: &str,
    pid: u32,
) -> bool {
    apply_set_workspace_agent_pid_at(
        snapshot,
        workspace_index,
        key,
        pid,
        current_unix_timestamp_seconds(),
    )
}

pub(super) fn apply_set_workspace_agent_pid_at(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    key: &str,
    pid: u32,
    updated_at: i64,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_agent_pid(workspace, key, pid, updated_at)
}

pub(super) fn apply_clear_workspace_agent_pid(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    key: &str,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    clear_workspace_agent_pid(workspace, key)
}

pub(super) fn apply_set_workspace_git_facts(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    git_branch: Option<SessionGitBranchSnapshot>,
    panel_git_branches: Vec<SessionPanelGitBranchSnapshot>,
    panel_pull_requests: Vec<SessionPanelPullRequestSnapshot>,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_git_facts(
        workspace,
        git_branch,
        panel_git_branches,
        panel_pull_requests,
    )
}

pub(super) fn apply_set_workspace_panel_pull_request(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
    number: i64,
    label: &str,
    url: &str,
    status: SessionPullRequestStatusSnapshot,
    branch: Option<String>,
    is_stale: bool,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    set_workspace_panel_pull_request(
        workspace, panel_id, number, label, url, status, branch, is_stale,
    )
}

pub(super) fn apply_clear_workspace_panel_pull_request(
    snapshot: &mut AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first_mut()
        .and_then(|window| window.tab_manager.workspaces.get_mut(workspace_index))
    else {
        return false;
    };
    clear_workspace_panel_pull_request(workspace, panel_id)
}

pub(super) fn apply_restorable_agent_snapshot(
    snapshot: &mut AppSessionSnapshot,
    workspace_id: Option<&str>,
    panel_id: &str,
    restorable: SessionRestorableAgentSnapshot,
) -> bool {
    let normalized_panel_id = panel_id.trim();
    if normalized_panel_id.is_empty() {
        return false;
    }
    let normalized_workspace_id = workspace_id.and_then(normalize_nonempty);
    for window in &mut snapshot.windows {
        let workspace = window.tab_manager.workspaces.iter_mut().find(|workspace| {
            let panel_matches = workspace
                .layout
                .as_ref()
                .is_some_and(|layout| session_ops::contains_panel(layout, normalized_panel_id));
            match normalized_workspace_id {
                Some(id) => workspace.workspace_id.as_deref() == Some(id) && panel_matches,
                None => panel_matches,
            }
        });
        let Some(workspace) = workspace else {
            continue;
        };
        let mut entries = workspace
            .restorable_agent_snapshots
            .take()
            .unwrap_or_default();
        match entries
            .iter_mut()
            .find(|entry| entry.panel_id == normalized_panel_id)
        {
            Some(entry) if entry.snapshot == restorable => {
                workspace.restorable_agent_snapshots = Some(entries);
                return false;
            }
            Some(entry) => {
                entry.snapshot = restorable;
            }
            None => entries.push(SessionPanelRestorableAgentSnapshot {
                panel_id: normalized_panel_id.to_string(),
                snapshot: restorable,
            }),
        }
        workspace.restorable_agent_snapshots = Some(entries);
        return true;
    }
    false
}

pub(crate) struct StartedAgentSessionSnapshot {
    pub panel_id: String,
    pub workspace_id: Option<String>,
    pub provider_id: String,
    pub session_id: String,
    pub executable_path: String,
    pub arguments: Vec<String>,
    pub working_directory: Option<String>,
}

pub(crate) fn record_started_agent_session(
    app: &AppHandle,
    state: &SessionState,
    started: StartedAgentSessionSnapshot,
) -> Result<bool, String> {
    let restorable = restorable_snapshot_from_started(&started);
    let (changed, _) = state
        .transact_value_if_changed(app, |snapshot| {
            let changed = apply_restorable_agent_snapshot(
                snapshot,
                started.workspace_id.as_deref(),
                &started.panel_id,
                restorable,
            );
            Ok::<(bool, bool), std::convert::Infallible>((changed, changed))
        })
        .map_err(collapse_infallible_publication_error)?;
    Ok(changed)
}

pub(super) fn restorable_snapshot_from_started(
    started: &StartedAgentSessionSnapshot,
) -> SessionRestorableAgentSnapshot {
    let launch_command = AgentLaunchCommandSnapshot {
        launcher: None,
        executable_path: normalize_nonempty(&started.executable_path).map(str::to_string),
        arguments: started.arguments.clone(),
        working_directory: started.working_directory.clone(),
        environment: None,
        source: Some("provider.start".to_string()),
    };
    SessionRestorableAgentSnapshot {
        kind: started.provider_id.clone(),
        session_id: started.session_id.clone(),
        working_directory: started.working_directory.clone(),
        launch_command: Some(launch_command),
        resume_command: agent_resume_command(
            &started.provider_id,
            &started.session_id,
            &started.executable_path,
        ),
        fork_command: agent_fork_command(
            &started.provider_id,
            &started.session_id,
            &started.executable_path,
        ),
    }
}

pub(super) fn agent_resume_command(
    provider_id: &str,
    session_id: &str,
    executable_path: &str,
) -> Option<String> {
    let session = powershell_single_quoted(session_id.trim());
    let executable = powershell_executable(executable_path, provider_id);
    match provider_id {
        "claude" => Some(format!("{executable} --resume {session}")),
        "codex" => Some(format!("{executable} resume {session}")),
        "opencode" => Some(format!("{executable} --session {session}")),
        _ => None,
    }
}

pub(super) fn agent_fork_command(
    provider_id: &str,
    session_id: &str,
    executable_path: &str,
) -> Option<String> {
    let session = powershell_single_quoted(session_id.trim());
    let executable = powershell_executable(executable_path, provider_id);
    match provider_id {
        "claude" => Some(format!("{executable} --resume {session} --fork-session")),
        "codex" => Some(format!("{executable} fork {session}")),
        "opencode" => Some(format!("{executable} --session {session} --fork")),
        _ => None,
    }
}

pub(super) fn powershell_executable(executable_path: &str, fallback: &str) -> String {
    let executable = normalize_nonempty(executable_path).unwrap_or(fallback);
    if executable
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | '\\' | ':'))
    {
        executable.to_string()
    } else {
        format!("& {}", powershell_single_quoted(executable))
    }
}

pub(super) fn powershell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(super) fn normalize_nonempty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

pub(super) fn current_unix_timestamp_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

/// Mark a workspace read/unread by updating its representative panel unread
/// metadata, stamping new unread markers for notification ordering.
#[cfg(test)]
pub(super) fn apply_set_workspace_unread(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    preferred_panel_id: Option<&str>,
    unread: bool,
) -> bool {
    apply_set_workspace_unread_at(
        snapshot,
        index,
        preferred_panel_id,
        unread,
        current_unix_timestamp_seconds(),
    )
}

pub(super) fn apply_set_workspace_unread_at(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    preferred_panel_id: Option<&str>,
    unread: bool,
    unread_at: i64,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::set_workspace_unread_at(
            &mut window.tab_manager,
            index,
            preferred_panel_id,
            unread,
            Some(unread_at),
        ),
        None => false,
    }
}

/// Pin/unpin the workspace at `index` in the first window — canonical
/// `WorkspaceReorderCoordinator.setPinned` + `reorderTabForPinnedState`
/// (already-at-value no-op; ungrouped tabs move to the pinned boundary;
/// grouped tabs flip the flag only). Pure — delegates to
/// [`session_ops::set_workspace_pinned`]. Returns whether the pin state
/// actually changed.
pub(super) fn apply_set_workspace_pinned(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    pinned: bool,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::set_workspace_pinned(&mut window.tab_manager, index, pinned),
        None => false,
    }
}

/// Reorder the workspace at `index` toward `to_index` in the first window —
/// canonical `WorkspaceReorderCoordinator.reorderSidebarWorkspace`
/// (`WorkspaceReorderCoordinator.swift:243-257`) routing: a group-anchor mover
/// relocates its WHOLE group via the top-level path, every other mover takes
/// the plain clamped single move. Pure — delegates to
/// [`session_ops::reorder_workspaces`]. Returns whether the order actually
/// changed.
pub(super) fn apply_reorder_workspaces(
    snapshot: &mut AppSessionSnapshot,
    index: i64,
    to_index: i64,
    uses_top_level_rows: bool,
) -> bool {
    apply_reorder_workspaces_in_window(snapshot, 0, index, to_index, uses_top_level_rows)
}

pub(super) fn apply_reorder_workspaces_in_window(
    snapshot: &mut AppSessionSnapshot,
    window_index: usize,
    index: i64,
    to_index: i64,
    uses_top_level_rows: bool,
) -> bool {
    match snapshot.windows.get_mut(window_index) {
        Some(window) => session_ops::reorder_workspaces_with_mode(
            &mut window.tab_manager,
            index,
            to_index,
            uses_top_level_rows,
        ),
        None => false,
    }
}

/// Set the OSC/process title of the workspace owning `panel_id` (any workspace in
/// the first window, not only the active one). Pure — delegates to
/// [`session_ops::set_process_title`]. Returns whether a title actually changed.
pub(super) fn apply_set_process_title(
    snapshot: &mut AppSessionSnapshot,
    panel_id: &str,
    title: &str,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => session_ops::set_process_title(&mut window.tab_manager, panel_id, title),
        None => false,
    }
}

/// Set the collapsed flag of workspace group `group_id` in the first window.
/// Pure — delegates to [`session_ops::set_group_collapsed`]. Returns whether
/// the flag actually changed.
pub(super) fn apply_set_group_collapsed(
    snapshot: &mut AppSessionSnapshot,
    group_id: &str,
    collapsed: bool,
) -> bool {
    match snapshot.windows.first_mut() {
        Some(window) => {
            session_ops::set_group_collapsed(&mut window.tab_manager, group_id, collapsed)
        }
        None => false,
    }
}
