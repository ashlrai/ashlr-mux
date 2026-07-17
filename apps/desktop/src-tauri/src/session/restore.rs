use super::*;

pub(super) fn is_nonrestorable_remote_mirror(kind: &SessionSurfaceKindSnapshot) -> bool {
    matches!(
        kind,
        SessionSurfaceKindSnapshot::RemoteTerminal {
            remote_session_id: Some(remote_session_id),
            remote_context: None,
            arrival_generation: Some(_),
        } if remote_session_id.strip_prefix('%').is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
    )
}

pub(super) fn drop_nonrestorable_remote_mirrors(snapshot: &mut AppSessionSnapshot) {
    for workspace in snapshot
        .windows
        .iter_mut()
        .flat_map(|window| &mut window.tab_manager.workspaces)
    {
        let remote_surface_ids = workspace
            .surfaces
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|surface| is_nonrestorable_remote_mirror(&surface.kind))
            .map(|surface| surface.surface_id.clone())
            .collect::<HashSet<_>>();
        if remote_surface_ids.is_empty() {
            continue;
        }
        for surface_id in &remote_surface_ids {
            session_ops::close_panel(&mut workspace.layout, surface_id);
        }
        if let Some(surfaces) = &mut workspace.surfaces {
            surfaces.retain(|surface| !remote_surface_ids.contains(&surface.surface_id));
        }
        if workspace
            .focused_panel_id
            .as_ref()
            .is_some_and(|focused| remote_surface_ids.contains(focused))
        {
            workspace.focused_panel_id = workspace
                .surfaces
                .as_ref()
                .and_then(|surfaces| surfaces.first())
                .map(|surface| surface.surface_id.clone());
        }
    }
}

pub(super) const MAX_MANUALLY_RESTORED_WINDOWS: usize = 12;

pub(super) fn nonblank(value: Option<&str>) -> bool {
    value.is_some_and(|value| !value.trim().is_empty())
}

pub(super) fn has_rows<T>(rows: Option<&Vec<T>>) -> bool {
    rows.is_some_and(|rows| !rows.is_empty())
}

pub(super) fn legacy_layout_has_nonterminal_state(layout: &SessionWorkspaceLayoutSnapshot) -> bool {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            pane.surface_kind
                .as_deref()
                .is_some_and(|kind| kind != "terminal")
                || nonblank(pane.markdown_file_path.as_deref())
                || nonblank(pane.file_path.as_deref())
                || nonblank(pane.diff_viewer_token.as_deref())
                || nonblank(pane.diff_viewer_request_path.as_deref())
                || nonblank(pane.browser_url.as_deref())
                || nonblank(pane.browser_proxy_url.as_deref())
                || has_rows(pane.browser_back_history.as_ref())
                || has_rows(pane.browser_forward_history.as_ref())
                || pane.browser_omnibar_visible.is_some()
                || pane.browser_focus_mode_active.is_some()
                || pane.browser_developer_tools_visible.is_some()
                || nonblank(pane.browser_developer_tools_panel.as_deref())
                || pane.browser_page_zoom.is_some()
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            legacy_layout_has_nonterminal_state(&split.first)
                || legacy_layout_has_nonterminal_state(&split.second)
        }
    }
}

pub(super) fn workspace_has_restorable_user_state(workspace: &SessionWorkspaceSnapshot) -> bool {
    if nonblank(workspace.custom_title.as_deref())
        || nonblank(workspace.custom_description.as_deref())
        || nonblank(workspace.custom_color.as_deref())
        || nonblank(workspace.initial_terminal_command.as_deref())
        || nonblank(workspace.initial_terminal_input.as_deref())
        || workspace
            .initial_terminal_environment
            .as_ref()
            .is_some_and(|environment| !environment.is_empty())
        || workspace
            .workspace_environment
            .as_ref()
            .is_some_and(|environment| !environment.is_empty())
        || workspace.is_pinned == Some(true)
        || workspace.group_id.is_some()
        || workspace.remote.is_some()
        || workspace.sidebar_progress.is_some()
        || workspace.git_branch.is_some()
        || nonblank(workspace.layout_mode.as_deref())
        || workspace
            .layout
            .as_ref()
            .is_some_and(legacy_layout_has_nonterminal_state)
        || has_rows(workspace.panel_titles.as_ref())
        || has_rows(workspace.panel_pins.as_ref())
        || has_rows(workspace.panel_unreads.as_ref())
        || has_rows(workspace.restorable_agent_snapshots.as_ref())
        || has_rows(workspace.surface_resume_bindings.as_ref())
        || has_rows(workspace.panel_git_branches.as_ref())
        || has_rows(workspace.panel_pull_requests.as_ref())
        || has_rows(workspace.panel_listening_ports.as_ref())
        || has_rows(workspace.panel_terminal_startups.as_ref())
        || has_rows(workspace.sidebar_status_entries.as_ref())
        || has_rows(workspace.sidebar_metadata_entries.as_ref())
        || has_rows(workspace.sidebar_metadata_blocks.as_ref())
        || has_rows(workspace.sidebar_log_entries.as_ref())
        || has_rows(workspace.canvas_panes.as_ref())
    {
        return true;
    }

    workspace
        .surfaces
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|surface| {
            if !matches!(surface.kind, SessionSurfaceKindSnapshot::Terminal)
                || nonblank(surface.metadata.custom_title.as_deref())
                || surface.metadata.pinned
                || surface.metadata.unread
            {
                return true;
            }
            surface.terminal_startup.as_ref().is_some_and(|startup| {
                nonblank(startup.command.as_deref())
                    || nonblank(startup.initial_input.as_deref())
                    || startup
                        .environment
                        .as_ref()
                        .is_some_and(|environment| !environment.is_empty())
                    || nonblank(startup.tmux_start_command.as_deref())
                    || nonblank(startup.remote_pty_session_id.as_deref())
                    || startup.resume_binding.is_some()
            })
        })
}

pub(super) fn normalized_path_components(path: &Path) -> Vec<String> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                components.pop();
            }
            component => components.push(component.as_os_str().to_string_lossy().to_lowercase()),
        }
    }
    components
}

pub(super) fn crash_storage_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for key in ["USERPROFILE", "HOME"] {
        if let Some(home) = std::env::var(key)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
        {
            roots.push(
                PathBuf::from(home)
                    .join(".local")
                    .join("state")
                    .join("cmux")
                    .join("crash"),
            );
        }
    }
    if let Some(xdg_state_home) = std::env::var("XDG_STATE_HOME")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    {
        roots.push(PathBuf::from(xdg_state_home).join("cmux").join("crash"));
    }
    roots
}

pub(super) fn is_crash_storage_path(path: &str, roots: &[Vec<String>]) -> bool {
    let candidate = normalized_path_components(Path::new(path.trim()));
    !candidate.is_empty()
        && roots
            .iter()
            .any(|root| candidate.len() >= root.len() && candidate[..root.len()] == root[..])
}

pub(super) fn is_crash_diagnostic_workspace(
    workspace: &SessionWorkspaceSnapshot,
    roots: &[Vec<String>],
) -> bool {
    if workspace_has_restorable_user_state(workspace) {
        return false;
    }
    let mut paths = workspace
        .current_directory
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .into_iter()
        .collect::<Vec<_>>();
    for surface in workspace.surfaces.as_deref().unwrap_or_default() {
        if let Some(path) = surface
            .metadata
            .reported_directory
            .as_deref()
            .filter(|path| !path.trim().is_empty())
        {
            paths.push(path);
        }
        if let Some(path) = surface
            .terminal_startup
            .as_ref()
            .and_then(|startup| startup.working_directory.as_deref())
            .filter(|path| !path.trim().is_empty())
        {
            paths.push(path);
        }
    }
    !paths.is_empty()
        && paths
            .into_iter()
            .all(|path| is_crash_storage_path(path, roots))
}

pub(super) fn prune_workspace_groups(
    groups: Option<Vec<cmux_core::session::SessionWorkspaceGroupSnapshot>>,
    original_workspaces: &[SessionWorkspaceSnapshot],
    kept_workspaces: &[SessionWorkspaceSnapshot],
) -> Option<Vec<cmux_core::session::SessionWorkspaceGroupSnapshot>> {
    let groups = groups?;
    let pruned = groups
        .into_iter()
        .filter_map(|mut group| {
            let original_members = original_workspaces
                .iter()
                .filter(|workspace| workspace.group_id.as_deref() == Some(&group.id))
                .collect::<Vec<_>>();
            let kept_members = kept_workspaces
                .iter()
                .filter(|workspace| workspace.group_id.as_deref() == Some(&group.id))
                .collect::<Vec<_>>();
            if kept_members.is_empty() {
                return None;
            }
            let original_anchor = group.anchor_workspace_id.clone().or_else(|| {
                group
                    .anchor_member_index
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| original_members.get(index))
                    .and_then(|workspace| workspace.workspace_id.clone())
            });
            let anchor_index = original_anchor
                .as_ref()
                .and_then(|anchor| {
                    kept_members
                        .iter()
                        .position(|workspace| workspace.workspace_id.as_ref() == Some(anchor))
                })
                .unwrap_or(0);
            group.anchor_member_index = Some(anchor_index as i64);
            group.anchor_workspace_id = kept_members[anchor_index].workspace_id.clone();
            Some(group)
        })
        .collect::<Vec<_>>();
    (!pruned.is_empty()).then_some(pruned)
}

pub(super) fn prune_crash_diagnostic_workspaces(snapshot: &mut AppSessionSnapshot) {
    let roots = crash_storage_roots()
        .iter()
        .map(|root| normalized_path_components(root))
        .collect::<Vec<_>>();
    if roots.is_empty() {
        return;
    }
    snapshot.windows.retain_mut(|window| {
        let original_selection = window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok());
        let original_workspaces = std::mem::take(&mut window.tab_manager.workspaces);
        let kept = original_workspaces
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, workspace)| !is_crash_diagnostic_workspace(workspace, &roots))
            .collect::<Vec<_>>();
        if kept.len() == original_workspaces.len() {
            window.tab_manager.workspaces = original_workspaces;
            return true;
        }
        if kept.is_empty() {
            return false;
        }
        let kept_indices = kept.iter().map(|(index, _)| *index).collect::<Vec<_>>();
        window.tab_manager.workspaces = kept.into_iter().map(|(_, workspace)| workspace).collect();
        let selected_index = original_selection.map(|selected| {
            kept_indices
                .iter()
                .position(|index| *index == selected)
                .or_else(|| kept_indices.iter().rposition(|index| *index < selected))
                .unwrap_or(0)
        });
        window.tab_manager.selected_workspace_index = selected_index.map(|index| index as i64);
        window.selected_workspace_id = selected_index
            .and_then(|index| window.tab_manager.workspaces[index].workspace_id.clone());
        window.tab_manager.workspace_groups = prune_workspace_groups(
            window.tab_manager.workspace_groups.take(),
            &original_workspaces,
            &window.tab_manager.workspaces,
        );
        true
    });
}

pub(super) fn readopt_additive_workspace_ids(
    current: &AppSessionSnapshot,
    restored: &mut [SessionWindowSnapshot],
    persisted_ids: &[Vec<Option<String>>],
) {
    let mut used = current
        .windows
        .iter()
        .flat_map(|window| &window.tab_manager.workspaces)
        .filter_map(|workspace| workspace.workspace_id.as_deref())
        .filter(|id| Uuid::parse_str(id).is_ok())
        .map(str::to_owned)
        .collect::<HashSet<_>>();

    for (window, old_ids) in restored.iter_mut().zip(persisted_ids) {
        let mut aliases = HashMap::new();
        for (workspace, old_id) in window.tab_manager.workspaces.iter_mut().zip(old_ids) {
            let Some(old_id) = old_id
                .as_ref()
                .filter(|old_id| Uuid::parse_str(old_id).is_ok())
                .filter(|old_id| used.insert((*old_id).clone()))
            else {
                continue;
            };
            if let Some(normalized) = workspace.workspace_id.replace(old_id.clone()) {
                aliases.insert(normalized, old_id.clone());
            }
        }
        if let Some(selected) = &mut window.selected_workspace_id {
            if let Some(adopted) = aliases.get(selected) {
                *selected = adopted.clone();
            }
        }
        if let Some(groups) = &mut window.tab_manager.workspace_groups {
            for group in groups {
                if let Some(anchor) = &mut group.anchor_workspace_id {
                    if let Some(adopted) = aliases.get(anchor) {
                        *anchor = adopted.clone();
                    }
                }
            }
        }
    }
}

pub(super) fn prepare_additive_restore(
    current: &AppSessionSnapshot,
    mut previous: AppSessionSnapshot,
) -> Result<Option<(Vec<SessionWindowSnapshot>, u64)>, String> {
    if previous.version != SESSION_SNAPSHOT_SCHEMA_VERSION || previous.windows.is_empty() {
        return Ok(None);
    }
    drop_nonrestorable_remote_mirrors(&mut previous);
    prune_crash_diagnostic_workspaces(&mut previous);
    previous.windows.truncate(MAX_MANUALLY_RESTORED_WINDOWS);
    if previous.windows.is_empty() {
        return Ok(None);
    }
    for window in &mut previous.windows {
        window.dock = None;
    }

    let raw_reseed = next_panel_counter(&previous);
    let persisted_workspace_ids = previous
        .windows
        .iter()
        .map(|window| {
            window
                .tab_manager
                .workspaces
                .iter()
                .map(|workspace| workspace.workspace_id.clone())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let live_window_count = current.windows.len();
    let mut normalized = current.clone();
    normalized.windows.extend(previous.windows);
    if !remint_noncanonical_identities(&mut normalized) {
        return Err("Previous session contains an identity reference without a definition".into());
    }
    let mut restored = normalized.windows.split_off(live_window_count);
    readopt_additive_workspace_ids(current, &mut restored, &persisted_workspace_ids);
    Ok(Some((restored, raw_reseed)))
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RestorePreviousLaunchOutcome {
    pub(crate) snapshot: AppSessionSnapshot,
    pub(crate) restored: bool,
}

pub(super) struct ManualRestorePublication<'a, T>(&'a mut T);

impl<T: SnapshotPublicationOperations> SnapshotPublicationOperations
    for ManualRestorePublication<'_, T>
{
    fn persist(&mut self, _candidate: &AppSessionSnapshot) -> Result<(), String> {
        Ok(())
    }

    fn update_event_baseline(&mut self, candidate: &AppSessionSnapshot) {
        self.0.update_event_baseline(candidate);
    }

    fn emit(&mut self, candidate: &AppSessionSnapshot) -> Result<(), String> {
        self.0.emit(candidate)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ManualRestoreRoute {
    Product,
    Control,
}

pub(super) trait ManualRestoreEffects {
    fn build_hidden(&mut self, window: &SessionWindowSnapshot) -> Result<(), String>;
    fn show_unfocused(&mut self, window_id: &str) -> Result<(), String>;
    fn close(&mut self, window_id: &str) -> Result<(), String>;
    fn activate(&mut self, window_id: &str) -> Result<(), String>;
    fn record_window_created(&mut self, window: &SessionWindowSnapshot);
}

#[cfg(test)]
pub(super) struct NoopManualRestoreEffects;

#[cfg(test)]
impl ManualRestoreEffects for NoopManualRestoreEffects {
    fn build_hidden(&mut self, _window: &SessionWindowSnapshot) -> Result<(), String> {
        Ok(())
    }

    fn show_unfocused(&mut self, _window_id: &str) -> Result<(), String> {
        Ok(())
    }

    fn close(&mut self, _window_id: &str) -> Result<(), String> {
        Ok(())
    }

    fn activate(&mut self, _window_id: &str) -> Result<(), String> {
        Ok(())
    }

    fn record_window_created(&mut self, _window: &SessionWindowSnapshot) {}
}

pub(super) fn restore_previous_launch_transaction_with_effects(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    effects: &mut impl ManualRestoreEffects,
    route: ManualRestoreRoute,
    load_previous: impl FnOnce() -> Option<AppSessionSnapshot>,
) -> Result<RestorePreviousLaunchOutcome, String> {
    let _restore_gate = authority.lock_gate();
    let current = authority
        .lock()
        .map_err(|_| "Session state is unavailable".to_string())?
        .clone();
    let Some(previous) = load_previous() else {
        return Ok(RestorePreviousLaunchOutcome {
            snapshot: current,
            restored: false,
        });
    };
    let Some((restored, reseed)) = prepare_additive_restore(&current, previous)? else {
        return Ok(RestorePreviousLaunchOutcome {
            snapshot: current,
            restored: false,
        });
    };

    let mut built_window_ids = Vec::with_capacity(restored.len());
    for window in &restored {
        let window_id = window
            .window_id
            .as_deref()
            .ok_or_else(|| "Restored window is missing its stable identity".to_string())?;
        if let Err(error) = effects.build_hidden(window) {
            return Err(compensate_manual_restore(effects, &built_window_ids, error));
        }
        built_window_ids.push(window_id.to_string());
        if let Err(error) = effects.show_unfocused(window_id) {
            return Err(compensate_manual_restore(effects, &built_window_ids, error));
        }
    }

    let mut candidate = current.clone();
    candidate.windows.extend(restored.iter().cloned());
    let publish_result = {
        let mut manual_publication = ManualRestorePublication(publication);
        publish_snapshot_transaction(
            authority,
            Some(&current),
            &candidate,
            &mut manual_publication,
        )
    };
    let snapshot = match publish_result {
        Ok(snapshot) => snapshot,
        Err(error) => {
            if let Ok(mut guard) = authority.lock() {
                *guard = current.clone();
            }
            publication.update_event_baseline(&current);
            return Err(compensate_manual_restore(effects, &built_window_ids, error));
        }
    };

    next_panel.fetch_max(reseed, Ordering::Relaxed);
    for window in &restored {
        effects.record_window_created(window);
    }
    if route == ManualRestoreRoute::Product {
        if let Some(window_id) = built_window_ids.first() {
            let _ = effects.activate(window_id);
        }
    }
    Ok(RestorePreviousLaunchOutcome {
        snapshot,
        restored: true,
    })
}

pub(super) fn compensate_manual_restore(
    effects: &mut impl ManualRestoreEffects,
    built_window_ids: &[String],
    primary: String,
) -> String {
    let failures = built_window_ids
        .iter()
        .rev()
        .filter_map(|window_id| effects.close(window_id).err())
        .collect::<Vec<_>>();
    if failures.is_empty() {
        primary
    } else {
        format!("{primary}; compensation failed: {}", failures.join("; "))
    }
}

#[cfg(test)]
pub(super) fn restore_previous_launch_transaction(
    authority: &GatedSnapshot,
    next_panel: &AtomicU64,
    publication: &mut impl SnapshotPublicationOperations,
    load_previous: impl FnOnce() -> Option<AppSessionSnapshot>,
) -> Result<RestorePreviousLaunchOutcome, String> {
    let mut effects = NoopManualRestoreEffects;
    restore_previous_launch_transaction_with_effects(
        authority,
        next_panel,
        publication,
        &mut effects,
        ManualRestoreRoute::Control,
        load_previous,
    )
}

pub(super) struct ProductionManualRestoreEffects<'a> {
    app: &'a AppHandle,
}

impl ManualRestoreEffects for ProductionManualRestoreEffects<'_> {
    fn build_hidden(&mut self, window: &SessionWindowSnapshot) -> Result<(), String> {
        let window_id = window
            .window_id
            .as_deref()
            .ok_or_else(|| "Restored window is missing its stable identity".to_string())?;
        crate::window::build_hidden_restored_window(self.app, window_id)
    }

    fn show_unfocused(&mut self, window_id: &str) -> Result<(), String> {
        crate::window::show_restored_window_unfocused(self.app, window_id)
    }

    fn close(&mut self, window_id: &str) -> Result<(), String> {
        crate::window::close_restored_window(self.app, window_id)
    }

    fn activate(&mut self, window_id: &str) -> Result<(), String> {
        crate::window::activate_restored_window(self.app, window_id)
    }

    fn record_window_created(&mut self, window: &SessionWindowSnapshot) {
        crate::control_socket::record_manual_restore_window_created(self.app, window);
    }
}

pub(super) fn restore_previous_launch_for_route(
    app: &AppHandle,
    state: &SessionState,
    route: ManualRestoreRoute,
) -> Result<RestorePreviousLaunchOutcome, String> {
    let mut publication = ProductionSnapshotPublicationOperations::for_manual_restore(app, state);
    let mut effects = ProductionManualRestoreEffects { app };
    restore_previous_launch_transaction_with_effects(
        &state.snapshot,
        &state.next_panel,
        &mut publication,
        &mut effects,
        route,
        || {
            session_snapshot_paths(app)
                .and_then(|(_current, previous)| load_snapshot_file(&previous))
        },
    )
}

pub(crate) fn restore_previous_launch_for_control(
    app: &AppHandle,
    state: &SessionState,
) -> Result<RestorePreviousLaunchOutcome, String> {
    restore_previous_launch_for_route(app, state, ManualRestoreRoute::Control)
}
