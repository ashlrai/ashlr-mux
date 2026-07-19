use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::control_socket) enum PaneJoinSourceError {
    Missing,
    SourcePaneUnresolved(String),
}

pub(in crate::control_socket) fn resolve_pane_join_source(
    current: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<String, PaneJoinSourceError> {
    if let Some(surface_id) = string_param(params, &["surface_id"]) {
        return Ok(surface_id);
    }
    if params.contains_key("surface_ref") {
        let workspace_index = workspace_index_from_workspace_scope_or_selected(current, params)
            .ok_or(PaneJoinSourceError::Missing)?;
        let workspace = current
            .windows
            .first()
            .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
            .ok_or(PaneJoinSourceError::Missing)?;
        return surface_id_from_selector_keys(workspace, params, &["surface_ref"], &["surface_id"])
            .ok_or(PaneJoinSourceError::Missing);
    }
    let (workspace_index, pane_id) = if let Some(pane_id) = string_param(params, &["pane_id"]) {
        pane_location_by_id(current, &pane_id)
            .map(|(workspace_index, _, pane_id)| (workspace_index, pane_id))
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_id))?
    } else if let Some(pane_reference) = string_param(params, &["pane_ref"]) {
        let workspace_index = workspace_index_from_workspace_scope_or_selected(current, params)
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_reference.clone()))?;
        let workspace = current
            .windows
            .first()
            .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_reference.clone()))?;
        let pane_index = one_based_ref_index(&pane_reference, "pane")
            .ok_or_else(|| PaneJoinSourceError::SourcePaneUnresolved(pane_reference.clone()))?;
        pane_at_index(workspace, pane_index)
            .map(|(_, pane_id)| (workspace_index, pane_id))
            .ok_or(PaneJoinSourceError::SourcePaneUnresolved(pane_reference))?
    } else {
        return Err(PaneJoinSourceError::Missing);
    };
    let workspace = &current.windows[0].tab_manager.workspaces[workspace_index];
    surfaces_for_workspace(workspace)
        .into_iter()
        .find(|surface| {
            surface.get("pane_id").and_then(Value::as_str) == Some(pane_id.as_str())
                && surface
                    .get("selected_in_pane")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
        })
        .and_then(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .ok_or(PaneJoinSourceError::SourcePaneUnresolved(pane_id))
}

pub(in crate::control_socket) fn pane_join_move_params(
    params: &serde_json::Map<String, Value>,
    source_panel_id: &str,
) -> serde_json::Map<String, Value> {
    let mut move_params = serde_json::Map::new();
    move_params.insert("surface_id".to_string(), json!(source_panel_id));
    for key in [
        "target_pane_id",
        "target_pane_ref",
        "workspace_id",
        "workspace_ref",
        "window_id",
        "window_ref",
    ] {
        if let Some(value) = params.get(key) {
            let move_key = key.strip_prefix("target_").unwrap_or(key);
            move_params.insert(move_key.to_string(), value.clone());
        }
    }
    move_params.insert("focus".to_string(), json!(true));
    move_params
}

pub(in crate::control_socket) fn pane_join(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    if !params.contains_key("target_pane_id") && !params.contains_key("target_pane_ref") {
        return invalid_params("Missing or invalid target_pane_id");
    }
    let current = snapshot(app);
    let source_panel_id = match resolve_pane_join_source(&current, params) {
        Ok(panel_id) => panel_id,
        Err(PaneJoinSourceError::Missing) => {
            return invalid_params("Missing surface_id (or pane_id with selected surface)");
        }
        Err(PaneJoinSourceError::SourcePaneUnresolved(pane_id)) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: "Unable to resolve selected surface in source pane".to_string(),
                data: Some(
                    json!({"pane_id": pane_id})
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    surface_move(app, &pane_join_move_params(params, &source_panel_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_selects_the_surface_even_when_window_focus_is_disabled() {
        let params = serde_json::json!({
            "target_pane_id": "pane-target",
            "workspace_id": "workspace-1",
            "focus": false,
        });
        let move_params = pane_join_move_params(params.as_object().unwrap(), "surface-source");

        assert_eq!(move_params["surface_id"], json!("surface-source"));
        assert_eq!(move_params["pane_id"], json!("pane-target"));
        assert_eq!(move_params["workspace_id"], json!("workspace-1"));
        assert_eq!(move_params["focus"], json!(true));
    }
}
