use super::*;

enum SourceResolutionError {
    Missing,
    Unresolved(Value),
}

fn resolve_source_surface(
    snapshot: &AppSessionSnapshot,
    model: &SurfaceLifecycleModel,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> Result<String, SourceResolutionError> {
    if let Some(surface_id) = params.get("surface_id").and_then(Value::as_str) {
        return model
            .owner_of_surface(surface_id)
            .map(|_| surface_id.to_owned())
            .ok_or(SourceResolutionError::Missing);
    }
    let join_scope =
        scope(snapshot, params, context).map_err(|_| SourceResolutionError::Missing)?;
    let workspace = &snapshot.windows[join_scope.window_index]
        .tab_manager
        .workspaces[join_scope.workspace_index];
    if let Some(reference) = params.get("surface_ref").and_then(Value::as_str) {
        let surface_id = super::super::one_based_ref_index(reference, "surface")
            .and_then(|index| surfaces_for_workspace(workspace).get(index).cloned())
            .and_then(|surface| surface.get("id").and_then(Value::as_str).map(str::to_owned));
        return surface_id.ok_or(SourceResolutionError::Missing);
    }
    let source_pane = if let Some(pane_id) = params.get("pane_id").and_then(Value::as_str) {
        model
            .pane(pane_id)
            .map(|_| pane_id.to_owned())
            .ok_or_else(|| SourceResolutionError::Unresolved(json!({"pane_id": pane_id})))?
    } else if params.contains_key("pane_ref") {
        resolve_pane_id(workspace, params, "pane_id", "pane_ref").ok_or_else(|| {
            let requested = params
                .get("pane_ref")
                .and_then(Value::as_str)
                .unwrap_or_default();
            SourceResolutionError::Unresolved(json!({"pane_id": requested}))
        })?
    } else {
        return Err(SourceResolutionError::Missing);
    };
    model
        .pane(&source_pane)
        .filter(|pane| !pane.selected_surface_id.is_empty())
        .map(|pane| pane.selected_surface_id.clone())
        .ok_or_else(|| SourceResolutionError::Unresolved(json!({"pane_id": source_pane})))
}

fn resolve_target_pane(
    snapshot: &AppSessionSnapshot,
    model: &SurfaceLifecycleModel,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> Option<String> {
    if let Some(pane_id) = params.get("target_pane_id").and_then(Value::as_str) {
        return model.pane(pane_id).map(|_| pane_id.to_owned());
    }
    let target_ref = params.get("target_pane_ref")?.clone();
    let mut target_params = params.clone();
    target_params.remove("pane_id");
    target_params.remove("pane_ref");
    target_params.remove("surface_id");
    target_params.remove("surface_ref");
    target_params.insert("pane_ref".into(), target_ref);
    let target_scope = scope(snapshot, &target_params, context).ok()?;
    let workspace = &snapshot.windows[target_scope.window_index]
        .tab_manager
        .workspaces[target_scope.workspace_index];
    resolve_pane_id(workspace, &target_params, "pane_id", "pane_ref")
}

pub(super) fn pane_join(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    if !params.contains_key("target_pane_id") && !params.contains_key("target_pane_ref") {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid target_pane_id",
            None,
        );
    }
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
    let surface_id = match resolve_source_surface(snapshot, &model, params, context) {
        Ok(surface_id) => surface_id,
        Err(SourceResolutionError::Unresolved(data)) => {
            return error(
                snapshot,
                "not_found",
                "Unable to resolve selected surface in source pane",
                Some(data),
            )
        }
        Err(SourceResolutionError::Missing) => {
            return error(
                snapshot,
                "invalid_params",
                "Missing surface_id (or pane_id with selected surface)",
                None,
            )
        }
    };
    if model.surface(&surface_id).is_some_and(|surface| {
        matches!(
            surface.kind,
            SessionSurfaceKindSnapshot::RemoteTerminal { .. }
        )
    }) {
        return error(
            snapshot,
            "invalid_params",
            "Remote mirrors cannot be moved as locally owned runtimes",
            Some(json!({"surface_id": surface_id})),
        );
    }
    let source_owner = model
        .owner_of_surface(&surface_id)
        .cloned()
        .expect("resolved source has an owner");
    let Some(target_pane_id) = resolve_target_pane(snapshot, &model, params, context) else {
        return error(snapshot, "not_found", "Destination pane not found", None);
    };
    let target_before = model
        .pane(&target_pane_id)
        .cloned()
        .expect("resolved target pane");
    let target_was_focused = model
        .focused_surface(&target_before.workspace_id)
        .and_then(|focused| model.owner_of_surface(focused))
        .is_some_and(|owner| owner.pane_id == target_pane_id);
    let previous = snapshot
        .windows
        .iter()
        .find(|window| window.window_id.as_deref() == Some(&target_before.window_id))
        .and_then(|window| {
            window.tab_manager.workspaces.iter().find(|workspace| {
                workspace.workspace_id.as_deref() == Some(&target_before.workspace_id)
            })
        })
        .and_then(|workspace| published_selection(workspace, &target_pane_id));
    let kind = model
        .surface(&surface_id)
        .map(|surface| kind_name(&surface.kind))
        .unwrap_or("terminal");

    if model
        .move_surface_transactionally(&surface_id, &target_pane_id, usize::MAX, |_, _| {
            Ok::<_, ()>(())
        })
        .is_err()
        || model.focus_surface(&surface_id).is_err()
    {
        return error(snapshot, "internal_error", "Failed to move surface", None);
    }
    let owner = model
        .owner_of_surface(&surface_id)
        .cloned()
        .expect("joined surface has an owner");
    let (window_id, workspace_id) = public_owner_ids(&model, &owner);
    let mut next = model
        .to_app_session(snapshot)
        .expect("joined lifecycle model projects to session state");
    if let Some(workspace) = next
        .windows
        .iter_mut()
        .find(|window| window.window_id.as_deref() == Some(&window_id))
        .and_then(|window| {
            window
                .tab_manager
                .workspaces
                .iter_mut()
                .find(|workspace| workspace.workspace_id.as_deref() == Some(&workspace_id))
        })
    {
        set_published_selection(workspace, &target_pane_id, &surface_id);
    }

    let [selected, surface_focused] = selection_events(
        &window_id,
        &workspace_id,
        &target_pane_id,
        &surface_id,
        previous.as_deref(),
        kind,
        true,
    );
    let mut events = vec![selected];
    if !target_was_focused {
        let [pane_focused, _] = focused_pane_events(
            &window_id,
            &workspace_id,
            &target_pane_id,
            &surface_id,
            kind,
        );
        events.push(pane_focused);
    }
    events.push(surface_focused);
    let result = json!({
        "window_id": window_id,
        "workspace_id": workspace_id,
        "pane_id": target_pane_id,
        "surface_id": surface_id,
    });
    let focus = params
        .get("focus")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    events.push(LifecycleEvent {
        name: "pane.joined",
        category: "pane",
        source: "socket.v2",
        window_id: Some(window_id.clone()),
        workspace_id: Some(workspace_id.clone()),
        pane_id: Some(target_pane_id.clone()),
        surface_id: Some(surface_id.clone()),
        payload: json!({
            "method": "pane.join",
            "params": {
                "focus": focus,
                "pane_id": source_owner.pane_id,
                "target_pane_id": target_pane_id,
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
