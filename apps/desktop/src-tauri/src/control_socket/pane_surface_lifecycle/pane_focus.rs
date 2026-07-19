use super::*;

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
    let _ = model.focus_surface(&selected);
    let next = model.to_app_session(snapshot).unwrap();
    ok_transition(
        next,
        json!({"window_id":pane.window_id,"workspace_id":pane.workspace_id,"pane_id":pane_id}),
        vec![owned_event(
            "pane.focused",
            &pane.window_id,
            &pane.workspace_id,
            Some(pane_id),
            Some(&selected),
            json!({}),
        )],
        vec![
            LifecycleEffect::ActivateWindow {
                window_id: pane.window_id,
            },
            LifecycleEffect::PersistSession,
        ],
    )
}
