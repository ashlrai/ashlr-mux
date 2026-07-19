use super::*;

pub(in crate::control_socket) fn pane_surface_default_title(
    workspace: &SessionWorkspaceSnapshot,
    panel_id: &str,
    surface_type: &str,
) -> String {
    let anchor_matches = workspace
        .surfaces
        .as_deref()
        .and_then(|surfaces| surfaces.first())
        .map(|surface| surface.surface_id == panel_id)
        .unwrap_or_else(|| {
            surfaces_for_workspace(workspace)
                .first()
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                == Some(panel_id)
        });
    if anchor_matches {
        if let Some(title) = workspace
            .custom_title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
        {
            return title.to_string();
        }
        if !workspace.process_title.trim().is_empty() {
            return workspace.process_title.clone();
        }
    }
    match surface_type {
        "terminal" => "Terminal".into(),
        _ => surface_type.to_string(),
    }
}

pub(in crate::control_socket) fn pane_surfaces(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let lifecycle = match session_ops::read_surface_lifecycle(&current) {
        Ok(model) => model,
        Err(error) => {
            return ControlCallResult::Err {
                code: "invalid_state".to_string(),
                message: format!("Invalid surface lifecycle: {error}"),
                data: None,
            };
        }
    };
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or_else(|| {
        crate::window::current_control_window(app, None)
            .and_then(|identity| {
                current
                    .windows
                    .iter()
                    .position(|window| window.window_id.as_deref() == Some(identity.label.as_str()))
            })
            .unwrap_or(0)
    });
    let Some((workspace_index, pane_index, pane_id)) =
        resolve_pane_surfaces_target(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Pane or workspace not found".to_string(),
            data: None,
        };
    };
    let window = &current.windows[window_index];
    let workspace = &window.tab_manager.workspaces[workspace_index];
    let pane = pane_snapshot_at_index(workspace, pane_index)
        .expect("resolved pane index remains in the immutable snapshot");
    let workspace_surface_ids = surfaces_for_workspace(workspace)
        .into_iter()
        .filter_map(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    let surfaces = pane
        .panel_ids
        .iter()
        .enumerate()
        .map(|(index, panel_id)| {
            let surface = lifecycle.surface(panel_id);
            let surface_type = surface
                .map(|surface| surface_kind_label(&surface.kind))
                .unwrap_or_else(|| pane.surface_kind.as_deref().unwrap_or("terminal"));
            let reference = indexed_response_ref_with(
                "surface",
                Some(panel_id),
                workspace_surface_ids.iter().position(|id| id == panel_id),
                &mut |kind, id| control_handle_ref(app, kind, id),
            );
            let title = surface
                .and_then(|surface| surface.metadata.custom_title.clone())
                .or_else(|| panel_title(&workspace.panel_titles, panel_id))
                .or_else(|| surface.and_then(|surface| surface.metadata.runtime_title.clone()))
                .unwrap_or_else(|| pane_surface_default_title(workspace, panel_id, surface_type));
            json!({
                "id": panel_id,
                "ref": reference,
                "index": index,
                "title": title,
                "type": surface_type,
                "selected": pane.selected_panel_id.as_deref() == Some(panel_id.as_str()),
            })
        })
        .collect::<Vec<_>>();
    let (window_id, window_ref) = pane_response_window_identity(window, window_index);
    let pane_reference =
        indexed_response_ref_with("pane", Some(&pane_id), Some(pane_index), &mut |kind, id| {
            control_handle_ref(app, kind, id)
        });
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "pane_id": pane_id,
        "pane_ref": pane_reference,
        "surfaces": surfaces,
        "window_id": window_id,
        "window_ref": window_ref,
    }))
}
