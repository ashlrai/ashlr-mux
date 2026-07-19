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

pub(super) fn record_resolved_workspace_rename_event(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) {
    let Some(event) = resolved_workspace_rename_event_spec(snapshot, window_index, workspace_index)
    else {
        return;
    };
    record_derived_event(app, event);
}

pub(in crate::control_socket) fn resolved_workspace_rename_event_spec(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    workspace_index: usize,
) -> Option<DerivedEventSpec> {
    let Some(window) = snapshot.windows.get(window_index) else {
        return None;
    };
    let summaries = session_event_summaries(snapshot);
    let key = window
        .window_id
        .clone()
        .unwrap_or_else(|| format!("window-{window_index}"));
    let Some(current) = summaries.get(&key) else {
        return None;
    };
    let Some(workspace) = current.workspaces.get(workspace_index) else {
        return None;
    };
    Some(workspace_renamed_event_spec(
        current,
        workspace,
        &workspace.title,
    ))
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
