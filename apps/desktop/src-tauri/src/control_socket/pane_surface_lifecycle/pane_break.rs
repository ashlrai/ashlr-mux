use super::*;

fn source_pane_id(
    workspace: &cmux_core::session::SessionWorkspaceSnapshot,
    model: &SurfaceLifecycleModel,
    params: &Map<String, Value>,
) -> Option<String> {
    if params.contains_key("pane_id") || params.contains_key("pane_ref") {
        return resolve_pane_id(workspace, params, "pane_id", "pane_ref");
    }
    workspace
        .focused_panel_id
        .as_deref()
        .and_then(|surface_id| model.owner_of_surface(surface_id))
        .map(|owner| owner.pane_id.clone())
        .or_else(|| {
            let mut ids = Vec::new();
            if let Some(layout) = workspace.layout.as_ref() {
                pane_ids(layout, &mut ids);
            }
            ids.into_iter().next()
        })
}

fn source_surface_id(
    workspace: &cmux_core::session::SessionWorkspaceSnapshot,
    source_pane_id: Option<&str>,
    params: &Map<String, Value>,
) -> Option<String> {
    if let Some(surface_id) = params.get("surface_id").and_then(Value::as_str) {
        return surfaces_for_workspace(workspace)
            .iter()
            .any(|surface| surface.get("id").and_then(Value::as_str) == Some(surface_id))
            .then(|| surface_id.to_owned());
    }
    if let Some(reference) = params.get("surface_ref").and_then(Value::as_str) {
        let index = super::super::one_based_ref_index(reference, "surface")?;
        return surfaces_for_workspace(workspace)
            .get(index)
            .and_then(|surface| surface.get("id"))
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    source_pane_id
        .and_then(|pane_id| find_pane(workspace.layout.as_ref(), pane_id))
        .and_then(|pane| pane.selected_panel_id.clone())
}

fn break_error(
    snapshot: &AppSessionSnapshot,
    failure: session_ops::PaneBreakError,
    surface_id: &str,
) -> LifecycleTransition {
    match failure {
        session_ops::PaneBreakError::WorkspaceNotFound => {
            error(snapshot, "not_found", "Workspace not found", None)
        }
        session_ops::PaneBreakError::SurfaceNotFound => error(
            snapshot,
            "not_found",
            "Surface not found",
            Some(json!({"surface_id": surface_id})),
        ),
        session_ops::PaneBreakError::DetachFailed => error(
            snapshot,
            "internal_error",
            "Failed to detach source surface",
            None,
        ),
    }
}

pub(super) fn pane_break(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    let break_scope = match scope(snapshot, params, context) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let model = match SurfaceLifecycleModel::from_app_session(snapshot) {
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
    let source_workspace = &snapshot.windows[break_scope.window_index]
        .tab_manager
        .workspaces[break_scope.workspace_index];
    let source_pane_id = source_pane_id(source_workspace, &model, params);
    let Some(surface_id) = source_surface_id(source_workspace, source_pane_id.as_deref(), params)
    else {
        if let Some(surface_id) = params.get("surface_id").and_then(Value::as_str) {
            return error(
                snapshot,
                "not_found",
                "Surface not found",
                Some(json!({"surface_id": surface_id})),
            );
        }
        return error(snapshot, "not_found", "No source surface to break", None);
    };
    let Some(source_owner) = model.owner_of_surface(&surface_id).cloned() else {
        return error(
            snapshot,
            "not_found",
            "Surface not found",
            Some(json!({"surface_id": surface_id})),
        );
    };
    let source_kind = model
        .surface(&surface_id)
        .map(|surface| kind_name(&surface.kind))
        .unwrap_or("terminal");
    let focused_before = source_workspace.focused_panel_id.clone();
    let focus = params
        .get("focus")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut next = snapshot.clone();
    let broken = match session_ops::break_surface_to_new_workspace(
        &mut next.windows[break_scope.window_index].tab_manager,
        break_scope.workspace_index,
        &surface_id,
        focus,
    ) {
        Ok(broken) => broken,
        Err(failure) => return break_error(snapshot, failure, &surface_id),
    };
    if !crate::session::finalize_broken_pane_snapshot(&mut next, break_scope.window_index, &broken)
    {
        return break_error(
            snapshot,
            session_ops::PaneBreakError::DetachFailed,
            &surface_id,
        );
    }
    let projected = match SurfaceLifecycleModel::from_app_session(&next) {
        Ok(projected) => projected,
        Err(_) => {
            return error(
                snapshot,
                "internal_error",
                "Invalid surface lifecycle state",
                None,
            )
        }
    };
    let destination = projected
        .owner_of_surface(&surface_id)
        .cloned()
        .expect("broken surface has a destination owner");
    let (window_id, destination_workspace_id) = public_owner_ids(&projected, &destination);
    let source_workspace_index = next.windows[break_scope.window_index]
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(&break_scope.workspace_id))
        .expect("source workspace identity remains stable");
    let focused_after = next.windows[break_scope.window_index]
        .tab_manager
        .workspaces[source_workspace_index]
        .focused_panel_id
        .clone();

    let mut events = Vec::with_capacity(6);
    if focused_before.as_deref() == Some(&surface_id) {
        if let Some(fallback_surface_id) = focused_after.as_deref() {
            if fallback_surface_id != surface_id {
                if let Some(fallback_owner) = projected.owner_of_surface(fallback_surface_id) {
                    let fallback_kind = projected
                        .surface(fallback_surface_id)
                        .map(|surface| kind_name(&surface.kind))
                        .unwrap_or("terminal");
                    let previous = published_selection(
                        &next.windows[break_scope.window_index]
                            .tab_manager
                            .workspaces[source_workspace_index],
                        &fallback_owner.pane_id,
                    );
                    let [selected, _] = selection_events(
                        &break_scope.window_id,
                        &break_scope.workspace_id,
                        &fallback_owner.pane_id,
                        fallback_surface_id,
                        previous.as_deref(),
                        fallback_kind,
                        true,
                    );
                    let [pane_focused, surface_focused] = focused_pane_events(
                        &break_scope.window_id,
                        &break_scope.workspace_id,
                        &fallback_owner.pane_id,
                        fallback_surface_id,
                        fallback_kind,
                    );
                    events.extend([selected, pane_focused, surface_focused]);
                    set_published_selection(
                        &mut next.windows[break_scope.window_index]
                            .tab_manager
                            .workspaces[source_workspace_index],
                        &fallback_owner.pane_id,
                        fallback_surface_id,
                    );
                }
            }
        }
    }
    events.push(owned_event(
        "surface.closed",
        &break_scope.window_id,
        &break_scope.workspace_id,
        Some(&source_owner.pane_id),
        Some(&surface_id),
        json!({
            "kind": source_kind,
            "origin": "detach",
            "pane_id": source_owner.pane_id,
            "surface_id": surface_id,
        }),
    ));
    events.push(owned_event(
        "surface.created",
        &window_id,
        &destination_workspace_id,
        Some(&destination.pane_id),
        Some(&surface_id),
        json!({
            "focused": false,
            "kind": source_kind,
            "origin": "detach_attach",
            "pane_id": destination.pane_id,
            "surface_id": surface_id,
        }),
    ));
    let result = json!({
        "window_id": window_id,
        "workspace_id": destination_workspace_id,
        "pane_id": destination.pane_id,
        "surface_id": surface_id,
    });
    events.push(LifecycleEvent {
        name: "pane.broken",
        category: "pane",
        source: "socket.v2",
        window_id: Some(window_id.clone()),
        workspace_id: Some(destination_workspace_id.clone()),
        pane_id: Some(destination.pane_id.clone()),
        surface_id: Some(surface_id.clone()),
        payload: json!({
            "method": "pane.break",
            "params": {
                "focus": focus,
                "pane_id": source_owner.pane_id,
                "workspace_id": break_scope.workspace_id,
            },
            "result": result,
        }),
    });
    let mut effects = Vec::with_capacity(2);
    if focus {
        effects.push(LifecycleEffect::ActivateWindow {
            window_id: window_id.clone(),
        });
    }
    effects.push(LifecycleEffect::PersistSession);
    ok_transition(next, result, events, effects)
}
