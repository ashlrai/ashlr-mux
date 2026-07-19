use super::*;

fn record_derived_event(app: &AppHandle, event: DerivedEventSpec) {
    record_event(
        app,
        event.name,
        event.category,
        event.source,
        event.window_id,
        event.workspace_id,
        event
            .payload
            .get("pane_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        event.surface_id,
        event.payload,
    );
}

pub(super) fn record_workspace_events(app: &AppHandle, events: Vec<DerivedEventSpec>) {
    for event in events {
        record_derived_event(app, event);
    }
}

pub(in crate::control_socket) fn workspace_reordered_event_spec(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    moved_workspace_ids: &[String],
) -> Option<DerivedEventSpec> {
    let window = snapshot.windows.get(window_index)?;
    let workspace_ids = window
        .tab_manager
        .workspaces
        .iter()
        .filter_map(|workspace| workspace.workspace_id.clone())
        .collect::<Vec<_>>();
    let workspace_id = moved_workspace_ids.first()?.clone();
    Some(DerivedEventSpec {
        name: "workspace.reordered",
        category: "workspace",
        source: "workspace.lifecycle",
        window_id: None,
        workspace_id: Some(workspace_id),
        surface_id: None,
        payload: json!({
            "workspace_ids": workspace_ids,
            "moved_workspace_ids": moved_workspace_ids,
            "pinned_workspace_ids": [],
            "count": window.tab_manager.workspaces.len(),
        }),
    })
}

pub(super) fn record_workspace_reordered_event(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    moved_workspace_ids: &[String],
) {
    if let Some(event) = workspace_reordered_event_spec(snapshot, window_index, moved_workspace_ids)
    {
        record_derived_event(app, event);
    }
}

pub(in crate::control_socket) fn workspace_moved_event_spec(
    params: &serde_json::Map<String, Value>,
    result: &Value,
) -> Option<DerivedEventSpec> {
    let window_id = result.get("window_id")?.as_str()?.to_owned();
    let workspace_id = result.get("workspace_id")?.as_str()?.to_owned();
    Some(DerivedEventSpec {
        name: "workspace.moved",
        category: "workspace",
        source: "socket.v2",
        window_id: Some(window_id),
        workspace_id: Some(workspace_id),
        surface_id: None,
        payload: json!({
            "method": "workspace.move_to_window",
            "params": params,
            "result": result,
        }),
    })
}

pub(super) fn record_workspace_moved_event(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    result: &Value,
) {
    if let Some(event) = workspace_moved_event_spec(params, result) {
        record_derived_event(app, event);
    }
}

pub(super) fn record_workspace_create_events(
    app: &AppHandle,
    previous: &AppSessionSnapshot,
    current: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    selected: bool,
) {
    let previous_workspace_id = previous.windows.get(window_index).and_then(|window| {
        window.selected_workspace_id.clone().or_else(|| {
            window
                .tab_manager
                .selected_workspace_index
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| window.tab_manager.workspaces.get(index))
                .and_then(|workspace| workspace.workspace_id.clone())
        })
    });
    let Some(events) = workspace_create_event_specs(
        current,
        window_index,
        workspace_index,
        selected,
        previous_workspace_id.as_deref(),
    ) else {
        return;
    };
    record_workspace_events(app, events);
}

pub(in crate::control_socket) fn workspace_create_event_specs(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    selected: bool,
    previous_workspace_id: Option<&str>,
) -> Option<Vec<DerivedEventSpec>> {
    workspace_create_event_specs_with_focus(
        snapshot,
        window_index,
        workspace_index,
        selected,
        selected,
        previous_workspace_id,
    )
}

pub(in crate::control_socket) fn workspace_group_created_event_specs(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) -> Option<Vec<DerivedEventSpec>> {
    let mut events = workspace_create_event_specs_with_focus(
        snapshot,
        window_index,
        workspace_index,
        true,
        false,
        None,
    )?;
    let window = snapshot.windows.get(window_index)?;
    let workspace = window.tab_manager.workspaces.get(workspace_index)?;
    if workspace.custom_title.is_none() {
        let title = format!("Terminal {}", window.tab_manager.workspaces.len());
        if let Some(created) = events
            .iter_mut()
            .find(|event| event.name == "workspace.created")
        {
            created.payload["title"] = json!(title);
        }
    }
    Some(events)
}

fn workspace_create_event_specs_with_focus(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    focus_initial_surface: bool,
    selected: bool,
    previous_workspace_id: Option<&str>,
) -> Option<Vec<DerivedEventSpec>> {
    let window = snapshot.windows.get(window_index)?;
    let workspace = window.tab_manager.workspaces.get(workspace_index)?;
    let workspace_id = workspace.workspace_id.clone()?;
    let surface_id = workspace.focused_panel_id.as_deref()?;
    let surface = surfaces_for_workspace(workspace)
        .into_iter()
        .find(|surface| surface.get("id").and_then(Value::as_str) == Some(surface_id))?;
    let pane_id = surface.get("pane_id")?.as_str()?.to_owned();
    let kind = surface.get("type")?.as_str()?.to_owned();
    let workspace_created_payload = json!({
        "workspace_id": workspace_id,
        "title": workspace_display_name(workspace),
        "custom_title": workspace.custom_title,
        "cwd": workspace.current_directory,
        "index": workspace_index,
        "selected": selected,
        "tab_count": window.tab_manager.workspaces.len(),
        "previous_workspace_id": null,
    });
    let mut events = Vec::new();
    if focus_initial_surface {
        events.extend([
            DerivedEventSpec {
                name: "surface.selected",
                category: "surface",
                source: "workspace.lifecycle",
                window_id: None,
                workspace_id: Some(workspace_id.clone()),
                surface_id: Some(surface_id.to_owned()),
                payload: json!({
                    "surface_id": surface_id,
                    "pane_id": pane_id,
                    "kind": kind,
                    "focused": true,
                    "previous_surface_id": null,
                    "origin": "bonsplit_selection",
                }),
            },
            DerivedEventSpec {
                name: "pane.focused",
                category: "pane",
                source: "workspace.lifecycle",
                window_id: None,
                workspace_id: Some(workspace_id.clone()),
                surface_id: Some(surface_id.to_owned()),
                payload: json!({
                    "pane_id": pane_id,
                    "selected_surface_id": surface_id,
                    "origin": "bonsplit_selection",
                }),
            },
            DerivedEventSpec {
                name: "surface.focused",
                category: "surface",
                source: "workspace.lifecycle",
                window_id: None,
                workspace_id: Some(workspace_id.clone()),
                surface_id: Some(surface_id.to_owned()),
                payload: json!({
                    "surface_id": surface_id,
                    "pane_id": pane_id,
                    "kind": kind,
                    "origin": "bonsplit_selection",
                }),
            },
        ]);
    }
    events.push(DerivedEventSpec {
        name: "workspace.created",
        category: "workspace",
        source: "workspace.lifecycle",
        window_id: None,
        workspace_id: Some(workspace_id.clone()),
        surface_id: None,
        payload: workspace_created_payload,
    });
    events.push(DerivedEventSpec {
        name: "surface.created",
        category: "surface",
        source: "workspace.lifecycle",
        window_id: None,
        workspace_id: Some(workspace_id.clone()),
        surface_id: Some(surface_id.to_owned()),
        payload: json!({
            "surface_id": surface_id,
            "pane_id": pane_id,
            "kind": kind,
            "focused": selected,
            "origin": "workspace_initial",
        }),
    });
    if selected {
        events.push(workspace_selected_event_spec(
            snapshot,
            window_index,
            workspace_index,
            previous_workspace_id,
        )?);
    }
    Some(events)
}

pub(in crate::control_socket) fn workspace_close_event_specs(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) -> Option<Vec<DerivedEventSpec>> {
    let window = snapshot.windows.get(window_index)?;
    let workspace = window.tab_manager.workspaces.get(workspace_index)?;
    let workspace_id = workspace.workspace_id.clone()?;
    let mut events = surfaces_for_workspace(workspace)
        .into_iter()
        .filter_map(|surface| {
            let surface_id = surface.get("id")?.as_str()?.to_owned();
            let pane_id = surface.get("pane_id")?.as_str()?.to_owned();
            let kind = surface.get("type")?.as_str()?.to_owned();
            Some(DerivedEventSpec {
                name: "surface.closed",
                category: "surface",
                source: "workspace.lifecycle",
                window_id: None,
                workspace_id: Some(workspace_id.clone()),
                surface_id: Some(surface_id.clone()),
                payload: json!({
                    "kind": kind,
                    "origin": "workspace_teardown",
                    "pane_id": pane_id,
                    "surface_id": surface_id,
                }),
            })
        })
        .collect::<Vec<_>>();
    events.push(DerivedEventSpec {
        name: "workspace.closed",
        category: "workspace",
        source: "workspace.lifecycle",
        window_id: None,
        workspace_id: Some(workspace_id.clone()),
        surface_id: None,
        payload: json!({
            "workspace_id": workspace_id,
            "title": workspace_display_name(workspace),
            "custom_title": workspace.custom_title,
            "cwd": workspace.current_directory,
            "index": null,
            "selected": false,
            "tab_count": window.tab_manager.workspaces.len().saturating_sub(1),
            "previous_workspace_id": null,
        }),
    });
    Some(events)
}

pub(super) fn record_workspace_rename_event(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
    result: &Value,
) {
    let Some(event) = workspace_rename_event_spec(params, result) else {
        return;
    };
    record_derived_event(app, event);
}

pub(in crate::control_socket) fn workspace_rename_event_spec(
    params: &serde_json::Map<String, Value>,
    result: &Value,
) -> Option<DerivedEventSpec> {
    let window_id = result.get("window_id")?.as_str()?.to_owned();
    let workspace_id = result.get("workspace_id")?.as_str()?.to_owned();
    Some(DerivedEventSpec {
        name: "workspace.renamed",
        category: "workspace",
        source: "socket.v2",
        window_id: Some(window_id),
        workspace_id: Some(workspace_id),
        surface_id: None,
        payload: json!({
            "method": "workspace.rename",
            "params": params,
            "result": result,
        }),
    })
}

pub(in crate::control_socket) fn workspace_selected_event_spec(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    previous_workspace_id: Option<&str>,
) -> Option<DerivedEventSpec> {
    let window = snapshot.windows.get(window_index)?;
    let workspace = window.tab_manager.workspaces.get(workspace_index)?;
    let workspace_id = workspace.workspace_id.clone()?;
    Some(DerivedEventSpec {
        name: "workspace.selected",
        category: "workspace",
        source: "workspace.lifecycle",
        window_id: None,
        workspace_id: Some(workspace_id.clone()),
        surface_id: None,
        payload: json!({
            "workspace_id": workspace_id,
            "title": workspace_display_name(workspace),
            "custom_title": workspace.custom_title,
            "cwd": workspace.current_directory,
            "index": workspace_index,
            "selected": true,
            "tab_count": window.tab_manager.workspaces.len(),
            "previous_workspace_id": previous_workspace_id,
        }),
    })
}

pub(super) fn record_workspace_selected_event(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
    previous_workspace_id: Option<&str>,
) {
    let Some(event) = workspace_selected_event_spec(
        snapshot,
        window_index,
        workspace_index,
        previous_workspace_id,
    ) else {
        return;
    };
    record_derived_event(app, event);
}
