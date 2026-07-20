use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::control_socket) struct CallerNotificationTarget {
    pub(in crate::control_socket) workspace_id: String,
    pub(in crate::control_socket) surface_id: String,
}

pub(in crate::control_socket) fn normalized_notification_tty(raw: Option<&str>) -> Option<&str> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() || trimmed == "not a tty" {
        return None;
    }
    trimmed.rsplit('/').next()
}

fn workspace_has_surface(workspace: &SessionWorkspaceSnapshot, surface_id: &str) -> bool {
    surfaces_for_workspace(workspace)
        .iter()
        .any(|surface| surface.get("id").and_then(Value::as_str) == Some(surface_id))
}

fn target(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    surface_id: Option<&str>,
) -> Option<CallerNotificationTarget> {
    let workspace = snapshot
        .windows
        .get(window_index)?
        .tab_manager
        .workspaces
        .get(workspace_index)?;
    let workspace_id = workspace.workspace_id.clone()?;
    let surface_id = surface_id
        .filter(|surface_id| workspace_has_surface(workspace, surface_id))
        .map(str::to_owned)
        .or_else(|| workspace.focused_panel_id.clone())
        .or_else(|| {
            surfaces_for_workspace(workspace)
                .first()
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })?;
    Some(CallerNotificationTarget {
        workspace_id,
        surface_id,
    })
}

fn workspace_location(snapshot: &AppSessionSnapshot, workspace_id: &str) -> Option<(usize, usize)> {
    snapshot
        .windows
        .iter()
        .enumerate()
        .find_map(|(window_index, window)| {
            window
                .tab_manager
                .workspaces
                .iter()
                .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
                .map(|workspace_index| (window_index, workspace_index))
        })
}

fn surface_location(snapshot: &AppSessionSnapshot, surface_id: &str) -> Option<(usize, usize)> {
    snapshot
        .windows
        .iter()
        .enumerate()
        .find_map(|(window_index, window)| {
            window
                .tab_manager
                .workspaces
                .iter()
                .position(|workspace| workspace_has_surface(workspace, surface_id))
                .map(|workspace_index| (window_index, workspace_index))
        })
}

fn candidate_windows(
    snapshot: &AppSessionSnapshot,
    preferred_workspace_id: Option<&str>,
    preferred_surface_id: Option<&str>,
    fallback_window_index: usize,
) -> Vec<usize> {
    let mut candidates = Vec::new();
    let mut push = |index: Option<usize>| {
        if let Some(index) = index.filter(|index| *index < snapshot.windows.len()) {
            if !candidates.contains(&index) {
                candidates.push(index);
            }
        }
    };
    push(preferred_workspace_id.and_then(|id| workspace_location(snapshot, id).map(|row| row.0)));
    push(preferred_surface_id.and_then(|id| surface_location(snapshot, id).map(|row| row.0)));
    push(Some(fallback_window_index));
    for index in 0..snapshot.windows.len() {
        push(Some(index));
    }
    candidates
}

fn tty_target(
    snapshot: &AppSessionSnapshot,
    candidates: &[usize],
    caller_tty: &str,
) -> Option<CallerNotificationTarget> {
    for &window_index in candidates {
        let window = snapshot.windows.get(window_index)?;
        for (workspace_index, workspace) in window.tab_manager.workspaces.iter().enumerate() {
            for row in workspace.panel_ttys.as_deref().unwrap_or_default() {
                if workspace_has_surface(workspace, &row.panel_id)
                    && normalized_notification_tty(Some(&row.tty)) == Some(caller_tty)
                {
                    return target(snapshot, window_index, workspace_index, Some(&row.panel_id));
                }
            }
        }
    }
    None
}

pub(super) fn resolve_caller_notification_target_with_fallback(
    snapshot: &AppSessionSnapshot,
    preferred_workspace_id: Option<&str>,
    preferred_surface_id: Option<&str>,
    caller_tty: Option<&str>,
    prefer_tty: bool,
    fallback_window_index: usize,
) -> Option<CallerNotificationTarget> {
    let candidates = candidate_windows(
        snapshot,
        preferred_workspace_id,
        preferred_surface_id,
        fallback_window_index,
    );
    let tty = normalized_notification_tty(caller_tty)
        .and_then(|tty| tty_target(snapshot, &candidates, tty));
    if prefer_tty && tty.is_some() {
        return tty;
    }

    if let Some(workspace_id) = preferred_workspace_id {
        if let Some((window_index, workspace_index)) = workspace_location(snapshot, workspace_id) {
            if let Some(surface_id) = preferred_surface_id {
                if let Some(resolved) =
                    target(snapshot, window_index, workspace_index, Some(surface_id))
                {
                    if resolved.surface_id == surface_id {
                        return Some(resolved);
                    }
                }
            }
            if let Some(tty) = tty.filter(|tty| tty.workspace_id == workspace_id) {
                return Some(tty);
            }
            return target(snapshot, window_index, workspace_index, None);
        }
    }

    if tty.is_some() {
        return tty;
    }
    if let Some(surface_id) = preferred_surface_id {
        if let Some((window_index, workspace_index)) = surface_location(snapshot, surface_id) {
            return target(snapshot, window_index, workspace_index, Some(surface_id));
        }
    }
    for window_index in candidates {
        let Some(workspace_index) = selected_workspace_index_for_window(snapshot, window_index)
        else {
            continue;
        };
        if let Some(target) = target(snapshot, window_index, workspace_index, None) {
            return Some(target);
        }
    }
    None
}

#[cfg(test)]
pub(in crate::control_socket) fn resolve_caller_notification_target(
    snapshot: &AppSessionSnapshot,
    preferred_workspace_id: Option<&str>,
    preferred_surface_id: Option<&str>,
    caller_tty: Option<&str>,
    prefer_tty: bool,
) -> Option<CallerNotificationTarget> {
    resolve_caller_notification_target_with_fallback(
        snapshot,
        preferred_workspace_id,
        preferred_surface_id,
        caller_tty,
        prefer_tty,
        0,
    )
}

pub(super) fn notification_workspace_has_surface(
    workspace: &SessionWorkspaceSnapshot,
    surface_id: &str,
) -> bool {
    workspace_has_surface(workspace, surface_id)
}
