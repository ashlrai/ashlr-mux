use super::*;

fn invalid_state(snapshot: &AppSessionSnapshot) -> LifecycleTransition {
    error(
        snapshot,
        "internal_error",
        "Invalid surface lifecycle state",
        None,
    )
}

fn pane_last_error(
    snapshot: &AppSessionSnapshot,
    failure: session_ops::PaneLastError,
) -> LifecycleTransition {
    match failure {
        session_ops::PaneLastError::NoFocusedPane => {
            error(snapshot, "not_found", "No focused pane", None)
        }
        session_ops::PaneLastError::NoAlternatePane => {
            error(snapshot, "not_found", "No alternate pane available", None)
        }
    }
}

pub(super) fn pane_last(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    let scope = match scope(snapshot, params, context) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let workspace =
        &snapshot.windows[scope.window_index].tab_manager.workspaces[scope.workspace_index];
    let focused_pane_id = workspace
        .focused_panel_id
        .as_deref()
        .and_then(|surface_id| session_ops::pane_id_containing_surface(workspace, surface_id));
    let focused = match session_ops::focus_alternate_pane(workspace, focused_pane_id) {
        Ok(focused) => focused,
        Err(error) => return pane_last_error(snapshot, error),
    };
    let Some(surface_id) = focused.surface_id else {
        return invalid_state(snapshot);
    };
    let mut model = match SurfaceLifecycleModel::from_app_session(snapshot) {
        Ok(model) => model,
        Err(_) => return invalid_state(snapshot),
    };
    let Some(kind) = model
        .surface(&surface_id)
        .map(|surface| kind_name(&surface.kind))
    else {
        return invalid_state(snapshot);
    };
    if model.focus_surface(&surface_id).is_err() {
        return invalid_state(snapshot);
    }
    let Ok(next) = model.to_app_session(snapshot) else {
        return invalid_state(snapshot);
    };
    let events = if called_from_cli(params, "last-pane") {
        selection_events::focused_pane_events(
            &scope.window_id,
            &scope.workspace_id,
            &focused.pane_id,
            &surface_id,
            kind,
        )
        .into()
    } else {
        focus_selection_events(
            &scope.window_id,
            &scope.workspace_id,
            &focused.pane_id,
            &surface_id,
            kind,
        )
        .into()
    };
    ok_transition(
        next,
        json!({
            "window_id": scope.window_id,
            "workspace_id": scope.workspace_id,
            "pane_id": focused.pane_id,
            "surface_id": surface_id,
        }),
        events,
        vec![
            LifecycleEffect::ActivateWindow {
                window_id: scope.window_id,
            },
            LifecycleEffect::PersistSession,
        ],
    )
}
