use super::*;

fn pane_ids(layout: &SessionWorkspaceLayoutSnapshot, ids: &mut Vec<String>) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => {
            if let Some(pane_id) = &pane.pane_id {
                ids.push(pane_id.clone());
            }
        }
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            pane_ids(&split.first, ids);
            pane_ids(&split.second, ids);
        }
    }
}

fn resolve_pane_id(
    workspace: &cmux_core::session::SessionWorkspaceSnapshot,
    params: &Map<String, Value>,
    id_key: &str,
    ref_key: &str,
) -> Option<String> {
    if let Some(pane_id) = params.get(id_key).and_then(Value::as_str) {
        return find_pane(workspace.layout.as_ref(), pane_id).map(|_| pane_id.to_owned());
    }
    let reference = params.get(ref_key).and_then(Value::as_str)?;
    let pane_index = super::super::one_based_ref_index(reference, "pane")?;
    let mut ids = Vec::new();
    pane_ids(workspace.layout.as_ref()?, &mut ids);
    ids.get(pane_index).cloned()
}

fn swap_error(
    snapshot: &AppSessionSnapshot,
    failure: session_ops::PaneSwapError,
) -> LifecycleTransition {
    match failure {
        session_ops::PaneSwapError::SamePane => error(
            snapshot,
            "invalid_params",
            "pane_id and target_pane_id must be different",
            None,
        ),
        session_ops::PaneSwapError::SourcePaneNotFound => {
            error(snapshot, "not_found", "Source pane not found", None)
        }
        session_ops::PaneSwapError::TargetPaneNotFound => error(
            snapshot,
            "not_found",
            "Target pane not found in source workspace",
            None,
        ),
        session_ops::PaneSwapError::BothPanesNeedSurface => error(
            snapshot,
            "invalid_state",
            "Both panes must have a selected surface",
            None,
        ),
    }
}

pub(super) fn pane_swap(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    if !params.contains_key("pane_id") && !params.contains_key("pane_ref") {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid pane_id",
            None,
        );
    }
    if !params.contains_key("target_pane_id") && !params.contains_key("target_pane_ref") {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid target_pane_id",
            None,
        );
    }
    let swap_scope = match scope(snapshot, params, context) {
        Ok(scope) => scope,
        Err((code, message)) => return error(snapshot, code, message, None),
    };
    let workspace = &snapshot.windows[swap_scope.window_index]
        .tab_manager
        .workspaces[swap_scope.workspace_index];
    let Some(source_pane_id) = resolve_pane_id(workspace, params, "pane_id", "pane_ref") else {
        return error(snapshot, "not_found", "Source pane not found", None);
    };
    let Some(target_pane_id) =
        resolve_pane_id(workspace, params, "target_pane_id", "target_pane_ref")
    else {
        return error(
            snapshot,
            "not_found",
            "Target pane not found in source workspace",
            None,
        );
    };
    if source_pane_id == target_pane_id {
        return swap_error(snapshot, session_ops::PaneSwapError::SamePane);
    }

    let source_surface_id = find_pane(workspace.layout.as_ref(), &source_pane_id)
        .and_then(|pane| pane.selected_panel_id.clone());
    let target_surface_id = find_pane(workspace.layout.as_ref(), &target_pane_id)
        .and_then(|pane| pane.selected_panel_id.clone());
    let (Some(source_surface_id), Some(target_surface_id)) = (source_surface_id, target_surface_id)
    else {
        return swap_error(snapshot, session_ops::PaneSwapError::BothPanesNeedSurface);
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
    let Some(source_kind) = model
        .surface(&source_surface_id)
        .map(|surface| kind_name(&surface.kind))
    else {
        return error(
            snapshot,
            "internal_error",
            "Invalid surface lifecycle state",
            None,
        );
    };
    let Some(target_kind) = model
        .surface(&target_surface_id)
        .map(|surface| kind_name(&surface.kind))
    else {
        return error(
            snapshot,
            "internal_error",
            "Invalid surface lifecycle state",
            None,
        );
    };

    let focus = params
        .get("focus")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut next = snapshot.clone();
    let workspace = &mut next.windows[swap_scope.window_index].tab_manager.workspaces
        [swap_scope.workspace_index];
    if let Err(failure) =
        session_ops::swap_selected_pane_surfaces(workspace, &source_pane_id, &target_pane_id)
    {
        return swap_error(snapshot, failure);
    }
    set_published_selection(workspace, &target_pane_id, &source_surface_id);
    set_published_selection(workspace, &source_pane_id, &target_surface_id);
    if focus {
        let window = &mut next.windows[swap_scope.window_index];
        window.tab_manager.selected_workspace_index = swap_scope.workspace_index.try_into().ok();
        window.selected_workspace_id = Some(swap_scope.workspace_id.clone());
    }

    let result = json!({
        "window_id": swap_scope.window_id,
        "workspace_id": swap_scope.workspace_id,
        "pane_id": source_pane_id,
        "target_pane_id": target_pane_id,
        "source_surface_id": source_surface_id,
        "target_surface_id": target_surface_id,
    });
    let placeholder_id = Uuid::new_v4().to_string();
    let [target_selected, _] = selection_events(
        &swap_scope.window_id,
        &swap_scope.workspace_id,
        &target_pane_id,
        &source_surface_id,
        Some(&target_surface_id),
        source_kind,
        true,
    );
    let [target_focused, _] = focused_pane_events(
        &swap_scope.window_id,
        &swap_scope.workspace_id,
        &target_pane_id,
        &source_surface_id,
        source_kind,
    );
    let [source_selected, _] = selection_events(
        &swap_scope.window_id,
        &swap_scope.workspace_id,
        &source_pane_id,
        &target_surface_id,
        Some(&source_surface_id),
        target_kind,
        true,
    );
    let [source_focused, surface_focused] = focused_pane_events(
        &swap_scope.window_id,
        &swap_scope.workspace_id,
        &source_pane_id,
        &target_surface_id,
        target_kind,
    );
    let events = vec![
        owned_event(
            "surface.created",
            &swap_scope.window_id,
            &swap_scope.workspace_id,
            Some(&target_pane_id),
            Some(&placeholder_id),
            json!({
                "focused": false,
                "kind": "terminal",
                "origin": "terminal_tab",
                "pane_id": target_pane_id,
                "surface_id": placeholder_id,
            }),
        ),
        target_selected,
        target_focused,
        source_selected,
        source_focused,
        surface_focused,
        owned_event(
            "surface.closed",
            &swap_scope.window_id,
            &swap_scope.workspace_id,
            Some(&target_pane_id),
            Some(&placeholder_id),
            json!({
                "kind": "terminal",
                "origin": "tab_close",
                "pane_id": target_pane_id,
                "surface_id": placeholder_id,
            }),
        ),
        LifecycleEvent {
            name: "pane.swapped",
            category: "pane",
            source: "socket.v2",
            window_id: Some(swap_scope.window_id.clone()),
            workspace_id: Some(swap_scope.workspace_id.clone()),
            pane_id: Some(source_pane_id.clone()),
            surface_id: None,
            payload: json!({
                "method": "pane.swap",
                "params": {
                    "focus": focus,
                    "pane_id": source_pane_id,
                    "target_pane_id": target_pane_id,
                },
                "result": result,
            }),
        },
    ];
    let mut effects = Vec::with_capacity(2);
    if focus {
        effects.push(LifecycleEffect::ActivateWindow {
            window_id: swap_scope.window_id.clone(),
        });
    }
    effects.push(LifecycleEffect::PersistSession);
    ok_transition(next, result, events, effects)
}
