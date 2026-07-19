use super::*;

pub(in crate::control_socket) fn workspace_index_for_id(
    snapshot: &AppSessionSnapshot,
    workspace_id: &str,
) -> Option<usize> {
    workspace_index_for_id_in_window(snapshot, 0, workspace_id)
}

pub(in crate::control_socket) fn workspace_index_for_id_in_window(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_id: &str,
) -> Option<usize> {
    snapshot
        .windows
        .get(window_index)?
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
}

pub(in crate::control_socket) fn workspace_reorder_many_window_index_with_active(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    ordered_workspace_ids: &[Uuid],
    active_window_id: Option<&str>,
) -> Option<usize> {
    let has_window_selector = params
        .get("window_id")
        .or_else(|| params.get("window_ref"))
        .is_some_and(|value| !value.is_null());
    if has_window_selector {
        return workspace_routed_window_index_with_active_window(
            snapshot,
            params,
            active_window_id,
        );
    }
    ordered_workspace_ids
        .iter()
        .find_map(|workspace_id| {
            let workspace_id = workspace_id.to_string();
            snapshot.windows.iter().position(|window| {
                window.tab_manager.workspaces.iter().any(|workspace| {
                    workspace.workspace_id.as_deref() == Some(workspace_id.as_str())
                })
            })
        })
        .or_else(|| {
            workspace_routed_window_index_with_active_window(snapshot, params, active_window_id)
        })
}

pub(in crate::control_socket) fn workspace_index_from_params(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    if let Some(workspace_ref) = string_param(params, &["workspace_ref", "ref"]) {
        if let Some(index) = one_based_ref_index(&workspace_ref, "workspace") {
            if snapshot
                .windows
                .first()
                .is_some_and(|window| index < window.tab_manager.workspaces.len())
            {
                return Some(index);
            }
            return None;
        }
    }

    let workspace_id = params
        .get("workspace_id")
        .or_else(|| params.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    workspace_index_for_id(snapshot, workspace_id)
}

#[cfg(test)]
pub(in crate::control_socket) fn workspace_reorder_destination_index(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    from_index: usize,
) -> Option<i64> {
    workspace_reorder_destination_index_in_window(snapshot, 0, params, from_index)
}

pub(in crate::control_socket) fn workspace_reorder_destination_index_in_window(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    params: &serde_json::Map<String, Value>,
    from_index: usize,
) -> Option<i64> {
    if params.contains_key("to_index")
        || params.contains_key("to")
        || params.contains_key("target_index")
    {
        return None;
    }

    let index = i64_param(params, &["index"]);
    let before = workspace_index_from_selector_keys_in_window(
        snapshot,
        window_index,
        params,
        &["before_workspace_ref", "before_ref"],
        &["before_workspace_id", "before_workspace"],
    );
    let after = workspace_index_from_selector_keys_in_window(
        snapshot,
        window_index,
        params,
        &["after_workspace_ref", "after_ref"],
        &["after_workspace_id", "after_workspace"],
    );
    match (index, before, after) {
        (Some(index), None, None) => Some(index),
        (None, Some(target), None) => Some(if from_index < target {
            target.saturating_sub(1)
        } else {
            target
        } as i64),
        (None, None, Some(target)) => Some(if from_index < target {
            target
        } else {
            target.saturating_add(1)
        } as i64),
        _ => None,
    }
}

pub(in crate::control_socket) fn workspace_reorder_window_matches(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> bool {
    let Some(window) = snapshot.windows.first() else {
        return false;
    };
    let reference_matches = string_param(params, &["window_ref"])
        .map(|reference| one_based_ref_index(&reference, "window") == Some(0))
        .unwrap_or(true);
    let id_matches = string_param(params, &["window_id"])
        .map(|id| window.window_id.as_deref() == Some(id.as_str()))
        .unwrap_or(true);
    reference_matches && id_matches
}

fn workspace_index_from_selector_keys_in_window(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    params: &serde_json::Map<String, Value>,
    ref_keys: &[&str],
    id_keys: &[&str],
) -> Option<usize> {
    if let Some(workspace_ref) = string_param(params, ref_keys) {
        let index = one_based_ref_index(&workspace_ref, "workspace")?;
        return snapshot
            .windows
            .get(window_index)
            .is_some_and(|window| index < window.tab_manager.workspaces.len())
            .then_some(index);
    }

    let workspace_id = string_param(params, id_keys)?;
    snapshot
        .windows
        .get(window_index)?
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
}
