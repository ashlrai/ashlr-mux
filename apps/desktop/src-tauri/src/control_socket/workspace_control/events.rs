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

pub(super) fn record_workspace_close_events(app: &AppHandle, events: Vec<DerivedEventSpec>) {
    for event in events {
        record_derived_event(app, event);
    }
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
