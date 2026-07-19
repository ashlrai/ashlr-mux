use super::*;

fn selected_workspace_id(window: &cmux_core::session::SessionWindowSnapshot) -> Option<&str> {
    window.selected_workspace_id.as_deref().or_else(|| {
        usize::try_from(window.tab_manager.selected_workspace_index.unwrap_or(0))
            .ok()
            .and_then(|index| window.tab_manager.workspaces.get(index))
            .and_then(|workspace| workspace.workspace_id.as_deref())
    })
}

fn workspace_selected_event(
    snapshot: &AppSessionSnapshot,
    scope: &Scope,
    previous_workspace_id: Option<&str>,
) -> LifecycleEvent {
    let window = &snapshot.windows[scope.window_index];
    let workspace = &window.tab_manager.workspaces[scope.workspace_index];
    LifecycleEvent {
        name: "workspace.selected",
        category: "workspace",
        source: "workspace.lifecycle",
        window_id: None,
        workspace_id: Some(scope.workspace_id.clone()),
        pane_id: None,
        surface_id: None,
        payload: json!({
            "workspace_id": scope.workspace_id,
            "title": workspace_display_name(workspace),
            "custom_title": workspace.custom_title,
            "cwd": workspace.current_directory,
            "index": scope.workspace_index,
            "selected": true,
            "tab_count": window.tab_manager.workspaces.len(),
            "previous_workspace_id": previous_workspace_id,
        }),
    }
}

fn focus_events(
    previous: &AppSessionSnapshot,
    current: &AppSessionSnapshot,
    scope: &Scope,
    pane_id: &str,
    surface_id: &str,
    kind: &str,
    called_from_cli: bool,
) -> Vec<LifecycleEvent> {
    let previous_window = &previous.windows[scope.window_index];
    let previous_workspace_id = selected_workspace_id(previous_window);
    let event_window = if called_from_cli {
        &current.windows[scope.window_index]
    } else {
        previous_window
    };
    let mut events = vec![super::super::window_lifecycle::window_lifecycle_event(
        "window.focused",
        "focus_request",
        event_window,
        &scope.window_id,
        true,
        true,
    )];
    if !called_from_cli && previous_workspace_id != Some(scope.workspace_id.as_str()) {
        events.push(workspace_selected_event(
            current,
            scope,
            previous_workspace_id,
        ));
    }
    events.extend(focus_selection_events(
        &scope.window_id,
        &scope.workspace_id,
        pane_id,
        surface_id,
        kind,
    ));
    events
}

pub(super) fn pane_focus(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    if let Err((code, message)) = resolve_window_index(snapshot, params, context) {
        return error(snapshot, code, message, None);
    }
    let Some(pane_id) = params.get("pane_id").and_then(Value::as_str) else {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid pane_id",
            None,
        );
    };
    let focus_scope = match scope(snapshot, params, context) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let mut model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    let Some(pane) = model.pane(pane_id).cloned().filter(|pane| {
        pane.window_id == focus_scope.window_id && pane.workspace_id == focus_scope.workspace_id
    }) else {
        return error(
            snapshot,
            "not_found",
            "Pane not found",
            Some(json!({"pane_id": pane_id})),
        );
    };
    let selected = pane.selected_surface_id.clone();
    if selected.is_empty() {
        return error(snapshot, "not_found", "Pane has no surface", None);
    }
    let Some(kind) = model
        .surface(&selected)
        .map(|surface| kind_name(&surface.kind))
    else {
        return error(
            snapshot,
            "internal_error",
            "Invalid surface lifecycle state",
            None,
        );
    };
    let _ = model.focus_surface(&selected);
    let mut next = model.to_app_session(snapshot).unwrap();
    let window = &mut next.windows[focus_scope.window_index];
    window.tab_manager.selected_workspace_index = focus_scope.workspace_index.try_into().ok();
    window.selected_workspace_id = Some(focus_scope.workspace_id.clone());
    window.tab_manager.workspaces[focus_scope.workspace_index].focused_pane_id =
        Some(pane_id.to_owned());
    let events = focus_events(
        snapshot,
        &next,
        &focus_scope,
        pane_id,
        &selected,
        kind,
        called_from_cli(params, "focus-pane"),
    );
    ok_transition(
        next,
        json!({"window_id":pane.window_id,"workspace_id":pane.workspace_id,"pane_id":pane_id}),
        events,
        vec![
            LifecycleEffect::ActivateWindow {
                window_id: pane.window_id,
            },
            LifecycleEffect::PersistSession,
        ],
    )
}
