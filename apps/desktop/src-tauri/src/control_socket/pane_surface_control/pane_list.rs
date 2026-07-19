use super::*;

pub(in crate::control_socket) fn pane_list(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let current = snapshot(app);
    let Some(requested_window) = split_off_window_index(app, &current, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let window_index = requested_window.unwrap_or(0);
    let Some(requested_workspace) = split_off_workspace_index(&current, params, window_index)
    else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(window) = current.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let workspace_index = requested_workspace.unwrap_or_else(|| {
        window
            .tab_manager
            .selected_workspace_index
            .and_then(|index| usize::try_from(index).ok())
            .unwrap_or(0)
    });
    let Some(workspace) = window.tab_manager.workspaces.get(workspace_index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let window_label = window.window_id.as_deref().unwrap_or("main");
    let (width, height) = app
        .get_webview_window(window_label)
        .and_then(|window| window.inner_size().ok())
        .map(|size| (f64::from(size.width), f64::from(size.height)))
        .unwrap_or((1.0, 1.0));
    let Some(layout) = workspace.layout.as_ref() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let mut pane_rows = Vec::new();
    pane_frames(
        layout,
        PanePixelFrame {
            x: 0.0,
            y: 0.0,
            width,
            height,
        },
        &mut pane_rows,
    );
    let terminal_state = app.state::<TerminalState>();
    let workspace_surface_ids = surfaces_for_workspace(workspace)
        .into_iter()
        .filter_map(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    let panes = pane_rows
        .into_iter()
        .enumerate()
        .map(|(index, (pane, frame))| {
            let selected = pane
                .selected_panel_id
                .as_deref()
                .filter(|panel_id| pane.panel_ids.iter().any(|id| id == panel_id));
            let mut row = json!({
                "id": pane.pane_id,
                "ref": pane_ref(index),
                "index": index,
                "focused": workspace.focused_panel_id.as_ref().is_some_and(|focused| pane.panel_ids.contains(focused)),
                "surface_ids": pane.panel_ids,
                "surface_refs": pane.panel_ids.iter().filter_map(|panel_id| workspace_surface_ids.iter().position(|id| id == panel_id).map(surface_ref)).collect::<Vec<_>>(),
                "selected_surface_id": selected,
                "selected_surface_ref": selected.and_then(|selected| workspace_surface_ids.iter().position(|id| id == selected)).map(surface_ref),
                "surface_count": pane.panel_ids.len(),
                "pixel_frame": {"x": frame.x, "y": frame.y, "width": frame.width, "height": frame.height},
            });
            if let Some(size) = selected.and_then(|panel_id| {
                terminal_grid_size_for_panel(terminal_state.inner(), panel_id)
            }) {
                if let Some(object) = row.as_object_mut() {
                    object.insert("columns".to_string(), json!(size.columns));
                    object.insert("rows".to_string(), json!(size.screen_lines));
                    object.insert(
                        "cell_width_px".to_string(),
                        json!((frame.width / size.columns.max(1) as f64).round().max(1.0) as u64),
                    );
                    object.insert(
                        "cell_height_px".to_string(),
                        json!((frame.height / size.screen_lines.max(1) as f64).round().max(1.0) as u64),
                    );
                }
            }
            row
        })
        .collect::<Vec<_>>();
    let (window_id, window_ref) = pane_response_window_identity(window, window_index);
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "panes": panes,
        "window_id": window_id,
        "window_ref": window_ref,
        "container_frame": {"width": width, "height": height},
    }))
}
