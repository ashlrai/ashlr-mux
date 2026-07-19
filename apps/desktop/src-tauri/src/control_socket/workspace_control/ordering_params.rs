use super::*;

#[derive(Debug)]
pub(in crate::control_socket) enum WorkspaceReorderManyOrderError {
    Missing,
    Invalid(String),
}

pub(in crate::control_socket) fn workspace_reorder_many_order(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Result<Vec<Uuid>, WorkspaceReorderManyOrderError> {
    let values: Vec<&str> = if let Some(raw) = params.get("workspace_ids") {
        match raw {
            Value::Array(values) => values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| WorkspaceReorderManyOrderError::Invalid(value.to_string()))
                })
                .collect::<Result<_, _>>()?,
            Value::String(value) => vec![value],
            value => {
                return Err(WorkspaceReorderManyOrderError::Invalid(value.to_string()));
            }
        }
    } else if let Some(raw) = params.get("order") {
        match raw {
            Value::String(value) => value.split(',').collect(),
            value => {
                return Err(WorkspaceReorderManyOrderError::Invalid(value.to_string()));
            }
        }
    } else {
        return Err(WorkspaceReorderManyOrderError::Missing);
    };
    if values.is_empty() {
        return Err(WorkspaceReorderManyOrderError::Missing);
    }

    values
        .into_iter()
        .map(|raw| {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(WorkspaceReorderManyOrderError::Invalid(raw.to_string()));
            }
            let workspace_id = if let Some(index) = one_based_ref_index(raw, "workspace") {
                snapshot
                    .windows
                    .first()
                    .and_then(|window| window.tab_manager.workspaces.get(index))
                    .and_then(|workspace| workspace.workspace_id.as_deref())
                    .ok_or_else(|| WorkspaceReorderManyOrderError::Invalid(raw.to_string()))?
            } else if workspace_index_for_id(snapshot, raw).is_some()
                || Uuid::parse_str(raw).is_ok()
            {
                raw
            } else {
                return Err(WorkspaceReorderManyOrderError::Invalid(raw.to_string()));
            };
            Uuid::parse_str(workspace_id)
                .map_err(|_| WorkspaceReorderManyOrderError::Invalid(raw.to_string()))
        })
        .collect()
}
