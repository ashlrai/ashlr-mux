use super::*;

pub(super) fn surface_focus(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
) -> LifecycleTransition {
    let Some(surface_id) = params.get("surface_id").and_then(Value::as_str) else {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid surface_id",
            None,
        );
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
    let Some(owner) = model.owner_of_surface(surface_id).cloned() else {
        return error(
            snapshot,
            "not_found",
            "Surface not found",
            Some(json!({"surface_id":surface_id})),
        );
    };
    let selected_before = model
        .pane(&owner.pane_id)
        .map(|pane| pane.selected_surface_id.clone())
        .filter(|selected| !selected.is_empty());
    // An explicit workspace identity wins and fails closed on owner mismatch.
    if params
        .get("workspace_id")
        .and_then(Value::as_str)
        .is_some_and(|explicit| owner.workspace_id != explicit)
    {
        return error(
            snapshot,
            "not_found",
            "Surface not found",
            Some(json!({"surface_id":surface_id})),
        );
    }
    let (window_id, workspace_id) = public_owner_ids(&model, &owner);
    let is_dock = model
        .pane(&owner.pane_id)
        .is_some_and(|pane| pane.container == ContainerKind::Dock);
    let _ = model.focus_surface(surface_id);
    let mut next = model.to_app_session(snapshot).unwrap();
    if !is_dock {
        for window in &mut next.windows {
            if window.window_id.as_deref() != Some(window_id.as_str()) {
                continue;
            }
            if let Some(index) = window.tab_manager.workspaces.iter().position(|workspace| {
                workspace.workspace_id.as_deref() == Some(workspace_id.as_str())
            }) {
                window.tab_manager.selected_workspace_index = index.try_into().ok();
                window.selected_workspace_id = Some(workspace_id.clone());
                window.tab_manager.workspaces[index].focused_pane_id = Some(owner.pane_id.clone());
                set_published_selection(
                    &mut window.tab_manager.workspaces[index],
                    &owner.pane_id,
                    surface_id,
                );
            }
        }
    }
    let mut effects = vec![LifecycleEffect::ActivateWindow {
        window_id: window_id.clone(),
    }];
    if is_dock {
        effects.extend([
            LifecycleEffect::DockReveal {
                owner_id: window_id.clone(),
            },
            LifecycleEffect::DockChanged {
                owner_id: window_id.clone(),
                phase: "post_persist",
            },
        ]);
    }
    effects.push(LifecycleEffect::PersistSession);
    let events = if called_from_cli(params, "focus-panel") {
        cli_focus_events(
            &model,
            &next,
            &window_id,
            &workspace_id,
            &owner.pane_id,
            surface_id,
            selected_before.as_deref(),
        )
    } else {
        vec![owned_event(
            "surface.focused",
            &window_id,
            &workspace_id,
            Some(&owner.pane_id),
            Some(surface_id),
            json!({}),
        )]
    };
    ok_transition(
        next,
        json!({"window_id":window_id,"workspace_id":workspace_id,"surface_id":surface_id}),
        events,
        effects,
    )
}

#[allow(clippy::too_many_arguments)]
fn cli_focus_events(
    model: &SurfaceLifecycleModel,
    next: &AppSessionSnapshot,
    window_id: &str,
    workspace_id: &str,
    pane_id: &str,
    surface_id: &str,
    selected_before: Option<&str>,
) -> Vec<LifecycleEvent> {
    let window = next
        .windows
        .iter()
        .find(|window| window.window_id.as_deref() == Some(window_id))
        .expect("surface owner window remains in the lifecycle snapshot");
    let mut events = vec![super::super::window_lifecycle::window_lifecycle_event(
        "window.focused",
        "focus_request",
        window,
        window_id,
        true,
        true,
    )];
    if let Some(selected) = selected_before {
        let selected_kind = model
            .surface(selected)
            .map(|surface| kind_name(&surface.kind))
            .unwrap_or("terminal");
        events.extend(selection_events::focused_pane_events(
            window_id,
            workspace_id,
            pane_id,
            selected,
            selected_kind,
        ));
        if selected != surface_id {
            let target_kind = model
                .surface(surface_id)
                .map(|surface| kind_name(&surface.kind))
                .unwrap_or("terminal");
            events.extend(selection_events(
                window_id,
                workspace_id,
                pane_id,
                surface_id,
                Some(selected),
                target_kind,
                true,
            ));
        }
    }
    events
}
