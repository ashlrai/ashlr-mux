use super::*;
use crate::pane_geometry::{PaneGeometryAuthority, PaneGeometryState};

pub(in crate::control_socket) fn pane_list_reference_fields_with(
    pane: &SessionPaneLayoutSnapshot,
    pane_index: usize,
    selected: Option<&str>,
    mint: &mut impl FnMut(&'static str, &str) -> String,
) -> (String, Vec<String>, Option<String>) {
    (
        pane.pane_id
            .as_deref()
            .map(|pane_id| mint("pane", pane_id))
            .unwrap_or_else(|| pane_ref(pane_index)),
        pane.panel_ids
            .iter()
            .map(|panel_id| mint("surface", panel_id))
            .collect(),
        selected.map(|selected| mint("surface", selected)),
    )
}

pub(in crate::control_socket) fn pane_list_window_size_with(
    snapshot: &AppSessionSnapshot,
    window_id: &str,
    mut inner_size_for_label: impl FnMut(&str) -> Option<(f64, f64)>,
) -> (f64, f64) {
    let label = pane_list_window_label(snapshot, window_id);
    inner_size_for_label(label).unwrap_or((1.0, 1.0))
}

pub(in crate::control_socket) fn pane_list_window_label<'a>(
    snapshot: &AppSessionSnapshot,
    window_id: &'a str,
) -> &'a str {
    if snapshot
        .windows
        .first()
        .and_then(|window| window.window_id.as_deref())
        == Some(window_id)
    {
        "main"
    } else {
        window_id
    }
}

pub(in crate::control_socket) fn pane_list_logical_size(
    physical_width: u32,
    physical_height: u32,
    scale_factor: f64,
) -> (f64, f64) {
    let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    (
        f64::from(physical_width) / scale_factor,
        f64::from(physical_height) / scale_factor,
    )
}

pub(in crate::control_socket) fn pane_list_root_frame(
    authority: PaneGeometryAuthority,
    native_fallback: impl FnOnce() -> PanePixelFrame,
) -> PanePixelFrame {
    match authority {
        PaneGeometryAuthority::Uninitialized => native_fallback(),
        PaneGeometryAuthority::WorkspaceUnrendered => PanePixelFrame {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        },
        PaneGeometryAuthority::Rendered(geometry) => PanePixelFrame {
            x: geometry.x,
            y: geometry.y,
            width: geometry.width,
            height: geometry.height,
        },
    }
}

pub(in crate::control_socket) fn pane_list_container_size(
    root_frame: PanePixelFrame,
) -> (f64, f64) {
    (root_frame.width, root_frame.height)
}

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
    let window_id = window.window_id.as_deref().unwrap_or("main");
    let window_label = pane_list_window_label(&current, window_id);
    let Some(layout) = workspace.layout.as_ref() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let mut pane_rows = Vec::new();
    let geometry_authority = workspace.workspace_id.as_deref().map_or(
        PaneGeometryAuthority::Uninitialized,
        |workspace_id| {
            app.state::<PaneGeometryState>()
                .authority_for(window_label, workspace_id)
        },
    );
    let root_frame = pane_list_root_frame(geometry_authority, || {
        let (width, height) = pane_list_window_size_with(&current, window_id, |label| {
            let window = app.get_webview_window(label)?;
            let size = window.inner_size().ok()?;
            let scale_factor = window.scale_factor().ok()?;
            Some(pane_list_logical_size(
                size.width,
                size.height,
                scale_factor,
            ))
        });
        PanePixelFrame {
            x: 0.0,
            y: 0.0,
            width,
            height,
        }
    });
    let container_size = pane_list_container_size(root_frame);
    pane_frames(layout, root_frame, &mut pane_rows);
    let terminal_state = app.state::<TerminalState>();
    let panes = pane_rows
        .into_iter()
        .enumerate()
        .map(|(index, (pane, frame))| {
            let selected = pane
                .selected_panel_id
                .as_deref()
                .filter(|panel_id| pane.panel_ids.iter().any(|id| id == panel_id));
            let (pane_reference, surface_refs, selected_surface_ref) =
                pane_list_reference_fields_with(
                    &pane,
                    index,
                    selected,
                    &mut |kind, id| control_handle_ref(app, kind, id),
                );
            let mut row = json!({
                "id": pane.pane_id,
                "ref": pane_reference,
                "index": index,
                "focused": workspace.focused_panel_id.as_ref().is_some_and(|focused| pane.panel_ids.contains(focused)),
                "surface_ids": pane.panel_ids,
                "surface_refs": surface_refs,
                "selected_surface_id": selected,
                "selected_surface_ref": selected_surface_ref,
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
        "container_frame": {"width": container_size.0, "height": container_size.1},
    }))
}
