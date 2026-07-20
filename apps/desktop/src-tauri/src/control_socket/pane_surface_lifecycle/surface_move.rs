use super::*;

pub(super) fn surface_move(
    snapshot: &AppSessionSnapshot,
    params: &Map<String, Value>,
    context: &LifecycleDispatchContext,
) -> LifecycleTransition {
    let Some(surface_id) = params.get("surface_id").and_then(Value::as_str) else {
        return error(
            snapshot,
            "invalid_params",
            "Missing or invalid surface_id",
            None,
        );
    };
    // Canonical rejects both anchors right after surface_id validation and
    // BEFORE the surface lookup (v2SurfaceMove, TerminalController.swift:
    // 4726-4736 at pinned e1825d40d; capture surface_move.both_anchors_rejected).
    // V3: canonical counts anchors through v2UUID — only uuid-resolvable
    // values participate; garbage text is treated as absent
    // (TerminalController.swift:4727-4733).
    let anchor_count = ["before_surface_id", "after_surface_id"]
        .iter()
        .filter(|key| {
            params
                .get(**key)
                .and_then(Value::as_str)
                .is_some_and(|value| Uuid::parse_str(value.trim()).is_ok())
        })
        .count();
    if anchor_count > 1 {
        return error(
            snapshot,
            "invalid_params",
            "Specify at most one of before_surface_id or after_surface_id",
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
    if model.surface(surface_id).is_none() {
        return error(
            snapshot,
            "not_found",
            "Surface not found",
            Some(json!({"surface_id":surface_id})),
        );
    }
    if model.surface(surface_id).is_some_and(|surface| {
        matches!(
            surface.kind,
            SessionSurfaceKindSnapshot::RemoteTerminal { .. }
        )
    }) {
        return error(
            snapshot,
            "invalid_params",
            "Remote mirrors cannot be moved as locally owned runtimes",
            Some(json!({"surface_id":surface_id})),
        );
    }
    let source_owner = model.owner_of_surface(surface_id).cloned();
    let source_is_dock = source_owner
        .as_ref()
        .and_then(|owner| model.pane(&owner.pane_id))
        .is_some_and(|pane| pane.container == ContainerKind::Dock);
    let destination_pane = params
        .get("pane_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            let mut destination_params = params.clone();
            destination_params.remove("surface_id");
            scope(snapshot, &destination_params, context)
                .ok()
                .and_then(|scope| model.focused_surface(&scope.workspace_id))
                .and_then(|id| model.owner_of_surface(id))
                .map(|owner| owner.pane_id.clone())
        });
    let Some(destination_pane) = destination_pane else {
        return error(snapshot, "not_found", "Destination pane not found", None);
    };
    let destination_is_dock = model
        .pane(&destination_pane)
        .is_some_and(|pane| pane.container == ContainerKind::Dock);
    let index = params
        .get("index")
        .and_then(Value::as_u64)
        .unwrap_or(usize::MAX as u64) as usize;
    if model
        .move_surface_transactionally(surface_id, &destination_pane, index, |_, _| Ok::<_, ()>(()))
        .is_err()
    {
        return error(snapshot, "internal_error", "Failed to move surface", None);
    }
    if super::super::bool_param(params, &["focus"]).unwrap_or(false) {
        let _ = model.focus_surface(surface_id);
    }
    let owner = model.owner_of_surface(surface_id).cloned().unwrap();
    let (window_id, workspace_id) = public_owner_ids(&model, &owner);
    let next = model.to_app_session(snapshot).unwrap();
    let result = json!({"window_id":window_id,"workspace_id":workspace_id,"pane_id":owner.pane_id,"surface_id":surface_id});
    let public_owner = cmux_core::surface_lifecycle::Owner {
        window_id,
        workspace_id,
        ..owner
    };
    let completion = socket_completion_event(
        "surface.moved",
        "surface.move",
        params,
        &result,
        &public_owner,
    );
    let mut effects = Vec::new();
    let mut dock_owners = Vec::new();
    if source_is_dock {
        if let Some(owner_id) = source_owner.as_ref().map(|owner| owner.window_id.clone()) {
            dock_owners.push(owner_id);
        }
    }
    if destination_is_dock && !dock_owners.contains(&public_owner.window_id) {
        dock_owners.push(public_owner.window_id.clone());
    }
    for owner_id in dock_owners {
        effects.push(LifecycleEffect::DockChanged {
            owner_id,
            phase: "post_persist",
        });
    }
    effects.push(LifecycleEffect::PersistSession);
    ok_transition(next, result, vec![completion], effects)
}
