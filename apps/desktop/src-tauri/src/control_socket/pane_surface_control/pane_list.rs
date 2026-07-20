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

fn pane_list_collect_panel_ids(
    layout: &SessionWorkspaceLayoutSnapshot,
    panel_ids: &mut std::collections::HashSet<String>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            panel_ids.extend(pane.panel_ids.iter().cloned());
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            pane_list_collect_panel_ids(&split.first, panel_ids);
            pane_list_collect_panel_ids(&split.second, panel_ids);
        }
    }
}

fn pane_list_active_panel_ids(snapshot: &AppSessionSnapshot) -> std::collections::HashSet<String> {
    let mut panel_ids = std::collections::HashSet::new();
    for workspace in snapshot
        .windows
        .iter()
        .flat_map(|window| &window.tab_manager.workspaces)
    {
        if let Some(layout) = workspace.layout.as_ref() {
            pane_list_collect_panel_ids(layout, &mut panel_ids);
        }
    }
    panel_ids
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
    capture_fallback: Option<PanePixelFrame>,
    native_fallback: impl FnOnce() -> PanePixelFrame,
) -> PanePixelFrame {
    match authority {
        PaneGeometryAuthority::Uninitialized if capture_fallback.is_some() => PanePixelFrame {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        },
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

pub(in crate::control_socket) fn pane_list_capture_portal_frame(
    window_width: f64,
    window_height: f64,
) -> Option<PanePixelFrame> {
    const SIDEBAR_WIDTH: f64 = 240.0;
    const TITLEBAR_HEIGHT: f64 = 28.0;

    (window_width.is_finite()
        && window_height.is_finite()
        && window_width > SIDEBAR_WIDTH
        && window_height > TITLEBAR_HEIGHT)
        .then_some(PanePixelFrame {
            x: SIDEBAR_WIDTH,
            y: TITLEBAR_HEIGHT,
            width: window_width - SIDEBAR_WIDTH,
            height: window_height - TITLEBAR_HEIGHT,
        })
}

pub(in crate::control_socket) fn pane_list_provisional_grid_fields(
    frame: PanePixelFrame,
    root_frame: PanePixelFrame,
) -> Option<(u64, u64, u64, u64)> {
    // TerminalSurface's fixed 14px Cascadia/Consolas stack measures 8x17 in
    // WebView2. This projection only bridges the React resize callback after a
    // workspace becomes selected; measured runtime metrics replace it afterward.
    const CELL_WIDTH_PX: u64 = 8;
    const CELL_HEIGHT_PX: u64 = 17;
    // The canonical live grid excludes two terminal-chrome rows. The outer
    // WebView scrollbar similarly consumes one column at the portal's right edge.
    const TERMINAL_VERTICAL_CHROME_ROWS: u64 = 2;

    if ![
        frame.x,
        frame.y,
        frame.width,
        frame.height,
        root_frame.x,
        root_frame.y,
        root_frame.width,
        root_frame.height,
    ]
    .into_iter()
    .all(f64::is_finite)
        || frame.width <= 0.0
        || frame.height <= 0.0
        || root_frame.width <= 0.0
        || root_frame.height <= 0.0
    {
        return None;
    }

    let mut columns = (frame.width / CELL_WIDTH_PX as f64).floor() as u64;
    let touches_right_edge =
        ((frame.x + frame.width) - (root_frame.x + root_frame.width)).abs() < 0.5;
    if touches_right_edge {
        columns = columns.saturating_sub(1);
    }
    let rows = ((frame.height / CELL_HEIGHT_PX as f64).floor() as u64)
        .saturating_sub(TERMINAL_VERTICAL_CHROME_ROWS);
    (columns > 0 && rows > 0).then_some((columns, rows, CELL_WIDTH_PX, CELL_HEIGHT_PX))
}

pub(in crate::control_socket) fn pane_list_preferred_grid_fields(
    projected: Option<(u64, u64, u64, u64)>,
    retained: Option<(u64, u64, u64, u64)>,
    suppress_first_projection: bool,
) -> Option<(u64, u64, u64, u64)> {
    if suppress_first_projection {
        None
    } else {
        projected.or(retained)
    }
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
    let workspace_is_selected = window
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .unwrap_or(0)
        == workspace_index;
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
    let pane_geometry_state = app.state::<PaneGeometryState>();
    pane_geometry_state.retain_grid_fields(&pane_list_active_panel_ids(&current));
    let capture_headless = crate::window::capture_windows_hidden();
    let capture_portal = capture_headless
        .then(|| {
            let configured_windows = &app.config().app.windows;
            configured_windows
                .iter()
                .find(|configured| configured.label == window_label)
                .or_else(|| configured_windows.first())
                .and_then(|configured| {
                    pane_list_capture_portal_frame(configured.width, configured.height)
                })
        })
        .flatten();
    let mut geometry_authority = workspace
        .workspace_id
        .as_deref()
        .map_or(PaneGeometryAuthority::Uninitialized, |workspace_id| {
            pane_geometry_state.authority_for(window_label, workspace_id)
        });
    let latest_geometry = pane_geometry_state.latest_for_window(window_label);
    let capture_activation_bootstrap = workspace_is_selected
        && !matches!(geometry_authority, PaneGeometryAuthority::Rendered(_))
        && latest_geometry.is_none()
        && capture_portal.is_some();
    if workspace_is_selected && !matches!(geometry_authority, PaneGeometryAuthority::Rendered(_)) {
        if let Some(fallback) = latest_geometry
            .map(|geometry| PanePixelFrame {
                x: geometry.x,
                y: geometry.y,
                width: geometry.width,
                height: geometry.height,
            })
            .or(capture_portal)
        {
            let geometry = crate::pane_geometry::WorkspacePaneGeometry {
                x: fallback.x,
                y: fallback.y,
                width: fallback.width,
                height: fallback.height,
            };
            if capture_activation_bootstrap {
                if let Some(workspace_id) = workspace.workspace_id.as_deref() {
                    let _ = pane_geometry_state.report(window_label, workspace_id, geometry);
                }
            }
            geometry_authority = PaneGeometryAuthority::Rendered(geometry);
        }
    }
    let capture_fallback = capture_headless.then_some(PanePixelFrame {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    });
    let root_frame = pane_list_root_frame(geometry_authority, capture_fallback, || {
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
            let grid_fields = pane
                .surface_kind
                .as_deref()
                .is_none_or(|kind| kind == "terminal")
                .then(|| {
                    let projected = workspace_is_selected
                        .then(|| pane_list_provisional_grid_fields(frame, root_frame))
                        .flatten();
                    let retained = selected.and_then(|panel_id| {
                        terminal_pane_grid_fields_for_panel(terminal_state.inner(), panel_id)
                            .or_else(|| pane_geometry_state.grid_fields_for_panel(panel_id))
                    });
                    let fields = pane_list_preferred_grid_fields(
                        projected,
                        retained,
                        capture_activation_bootstrap,
                    );
                    if let (Some(panel_id), Some(projected)) = (selected, projected) {
                        terminal_remember_pane_grid_fields(
                            terminal_state.inner(),
                            panel_id,
                            projected,
                        );
                        pane_geometry_state.remember_grid_fields(panel_id, projected);
                    }
                    fields
                })
                .flatten();
            if let Some((columns, rows, cell_width_px, cell_height_px)) = grid_fields {
                if let Some(object) = row.as_object_mut() {
                    object.insert("columns".to_string(), json!(columns));
                    object.insert("rows".to_string(), json!(rows));
                    object.insert("cell_width_px".to_string(), json!(cell_width_px));
                    object.insert("cell_height_px".to_string(), json!(cell_height_px));
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
