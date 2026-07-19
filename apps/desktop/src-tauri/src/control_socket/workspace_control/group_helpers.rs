use super::*;

pub(in crate::control_socket) fn workspace_group_error(
    code: &str,
    message: &str,
    data: Option<Value>,
) -> ControlCallResult {
    ControlCallResult::Err {
        code: code.to_string(),
        message: message.to_string(),
        data: data.and_then(|value| value.try_into().ok()),
    }
}

pub(in crate::control_socket) fn workspace_group_payload(
    app: &AppHandle,
    window: &SessionWindowSnapshot,
    group: &cmux_core::session::SessionWorkspaceGroupSnapshot,
) -> Value {
    workspace_group_payload_with(window, group, &mut |kind, id| {
        control_handle_ref(app, kind, id)
    })
}

pub(in crate::control_socket) fn workspace_group_payload_with(
    window: &SessionWindowSnapshot,
    group: &cmux_core::session::SessionWorkspaceGroupSnapshot,
    mint: &mut impl FnMut(&'static str, &str) -> String,
) -> Value {
    let members = window
        .tab_manager
        .workspaces
        .iter()
        .filter(|workspace| workspace.group_id.as_deref() == Some(group.id.as_str()))
        .filter_map(|workspace| workspace.workspace_id.as_deref())
        .collect::<Vec<_>>();
    let anchor_workspace_id = group
        .anchor_workspace_id
        .as_deref()
        .filter(|anchor| members.contains(anchor))
        .or_else(|| {
            group
                .anchor_member_index
                .and_then(|index| usize::try_from(index).ok())
                .and_then(|index| members.get(index).copied())
        })
        .or_else(|| members.first().copied());
    let member_workspace_ids = members.iter().map(|id| json!(id)).collect::<Vec<_>>();
    let member_workspace_refs = members
        .iter()
        .map(|id| json!(mint("workspace", id)))
        .collect::<Vec<_>>();
    json!({
        "id": group.id,
        "ref": mint("workspace_group", &group.id),
        "name": group.name,
        "is_collapsed": group.is_collapsed,
        "is_pinned": group.is_pinned.unwrap_or(false),
        "anchor_workspace_id": anchor_workspace_id,
        "anchor_workspace_ref": anchor_workspace_id.map(|id| mint("workspace", id)),
        "custom_color": group.custom_color,
        "icon_symbol": group.icon_symbol,
        "member_workspace_ids": member_workspace_ids,
        "member_workspace_refs": member_workspace_refs,
        "member_count": members.len(),
    })
}

pub(in crate::control_socket) fn workspace_group_member_ids(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    group_id: Uuid,
) -> Vec<String> {
    let group_id = group_id.to_string();
    snapshot
        .windows
        .get(window_index)
        .into_iter()
        .flat_map(|window| &window.tab_manager.workspaces)
        .filter(|workspace| workspace.group_id.as_deref() == Some(group_id.as_str()))
        .filter_map(|workspace| workspace.workspace_id.clone())
        .collect()
}

pub(in crate::control_socket) fn workspace_group_delete_member_ids(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    group_id: Uuid,
) -> Vec<String> {
    let Some(window) = snapshot.windows.get(window_index) else {
        return Vec::new();
    };
    let group_id = group_id.to_string();
    let anchor_id = window
        .tab_manager
        .workspace_groups
        .as_deref()
        .unwrap_or_default()
        .iter()
        .find(|group| group.id == group_id)
        .and_then(|group| group.anchor_workspace_id.as_deref());
    let members = window
        .tab_manager
        .workspaces
        .iter()
        .filter(|workspace| workspace.group_id.as_deref() == Some(group_id.as_str()))
        .filter_map(|workspace| workspace.workspace_id.as_deref())
        .collect::<Vec<_>>();
    members
        .iter()
        .copied()
        .filter(|workspace_id| Some(*workspace_id) != anchor_id)
        .chain(
            members
                .iter()
                .copied()
                .filter(|workspace_id| Some(*workspace_id) == anchor_id),
        )
        .map(str::to_owned)
        .collect()
}

pub(in crate::control_socket) fn workspace_group_create_cwd(
    tabs: &cmux_core::session::SessionTabManagerSnapshot,
    explicit_cwd: Option<String>,
    child_ids: &[Uuid],
    other_anchor_ids: &HashSet<Uuid>,
) -> Option<String> {
    fn normalized(cwd: String) -> Option<String> {
        let trimmed = cwd.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.starts_with("file://") {
            if let Ok(url) = url::Url::parse(trimmed) {
                if let Ok(path) = url.to_file_path() {
                    return Some(path.to_string_lossy().into_owned());
                }
            }
        }
        Some(trimmed.to_string())
    }

    if let Some(explicit_cwd) = explicit_cwd {
        return normalized(explicit_cwd).or_else(default_workspace_directory);
    }
    let first_child = child_ids.iter().find_map(|child_id| {
        tabs.workspaces.iter().find(|workspace| {
            workspace
                .workspace_id
                .as_deref()
                .and_then(|id| Uuid::parse_str(id).ok())
                == Some(*child_id)
                && workspace.is_pinned != Some(true)
                && !other_anchor_ids.contains(child_id)
        })
    });
    if let Some(child_cwd) = first_child.and_then(|workspace| workspace.current_directory.clone()) {
        return normalized(child_cwd);
    }
    tabs.selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| tabs.workspaces.get(index))
        .and_then(|workspace| workspace.current_directory.clone())
        .and_then(normalized)
}

pub(in crate::control_socket) fn parse_workspace_group_placement(
    raw: Option<&str>,
) -> Option<session_ops::WorkspaceGroupPlacement> {
    match raw?.trim().to_ascii_lowercase().as_str() {
        "aftercurrent" | "after-current" | "after_current" => {
            Some(session_ops::WorkspaceGroupPlacement::AfterCurrent)
        }
        "top" => Some(session_ops::WorkspaceGroupPlacement::Top),
        "end" => Some(session_ops::WorkspaceGroupPlacement::End),
        _ => None,
    }
}

pub(in crate::control_socket) fn workspace_group_move_index_param(
    params: &serde_json::Map<String, Value>,
) -> Option<i64> {
    match params.get("to_index")? {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|value| value as i64)),
        Value::Bool(value) => Some(i64::from(*value)),
        Value::String(raw) => raw.parse::<i64>().ok(),
        _ => None,
    }
}

pub(in crate::control_socket) const SF_SYMBOL_NAMES: &str = include_str!("../../sf_symbols_v7.txt");

pub(in crate::control_socket) fn normalized_workspace_group_icon_symbol(
    raw: Option<&str>,
) -> Option<String> {
    let symbol = raw?.trim();
    (!symbol.is_empty() && SF_SYMBOL_NAMES.lines().any(|candidate| candidate == symbol))
        .then(|| symbol.to_string())
}

pub(in crate::control_socket) fn workspace_group_parameter_description(value: &Value) -> String {
    fn nested(value: &Value) -> String {
        match value {
            Value::Null => "<null>".to_string(),
            Value::Bool(value) => i32::from(*value).to_string(),
            Value::Number(value) => value.to_string(),
            Value::String(value) => serde_json::to_string(value).unwrap_or_default(),
            Value::Array(values) => format!(
                "[{}]",
                values.iter().map(nested).collect::<Vec<_>>().join(", ")
            ),
            Value::Object(values) if values.is_empty() => "[:]".to_string(),
            Value::Object(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(|(key, value)| format!(
                        "{}: {}",
                        serde_json::to_string(key).unwrap_or_default(),
                        nested(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    if let Value::String(value) = value {
        value.clone()
    } else {
        nested(value)
    }
}

pub(in crate::control_socket) fn workspace_group_uuid_param(
    params: &serde_json::Map<String, Value>,
    key: &str,
) -> Option<Uuid> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| Uuid::parse_str(value).ok())
}

pub(in crate::control_socket) fn workspace_group_preflight(
    app: &AppHandle,
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Option<ControlCallResult> {
    if method == "workspace.group.create" {
        match params.get("child_workspace_ids") {
            None | Some(Value::Null) => return None,
            Some(Value::Array(values)) if values.iter().all(Value::is_string) => {
                let unresolved = values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .filter(|value| {
                        Uuid::parse_str(value).is_err()
                            && resolve_control_handle_ref(app, "workspace", value).is_none()
                    })
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if unresolved.is_empty() {
                    return None;
                }
                return Some(workspace_group_error(
                    "invalid_params",
                    &format!(
                        "Unresolved child workspace handles: {}",
                        unresolved.join(", ")
                    ),
                    Some(json!({"unresolved": unresolved})),
                ));
            }
            Some(value) => {
                return Some(workspace_group_error(
                    "invalid_params",
                    "child_workspace_ids must be an array of workspace handles",
                    Some(json!({
                        "child_workspace_ids": workspace_group_parameter_description(value)
                    })),
                ));
            }
        }
    }

    if let Some(message) = workspace_group_required_param_error(method, params) {
        return Some(invalid_params(message));
    }

    if method == "workspace.group.add" {
        let placement = raw_string_param(params, &["placement"]);
        if placement.as_deref().is_some_and(|raw| {
            !raw.trim().is_empty() && parse_workspace_group_placement(Some(raw)).is_none()
        }) {
            return Some(workspace_group_error(
                "invalid_params",
                "Invalid placement",
                Some(json!({"placement": placement})),
            ));
        }
        if params
            .get("reference_workspace_id")
            .is_some_and(|value| !value.is_null())
            && workspace_group_uuid_param(params, "reference_workspace_id").is_none()
        {
            return Some(invalid_params("Missing or invalid reference_workspace_id"));
        }
    }
    None
}

pub(in crate::control_socket) fn workspace_group_required_param_error(
    method: &str,
    params: &serde_json::Map<String, Value>,
) -> Option<&'static str> {
    let group_id = workspace_group_uuid_param(params, "group_id");
    let workspace_id = workspace_group_uuid_param(params, "workspace_id");
    match method {
        "workspace.group.list" => None,
        "workspace.group.rename"
            if group_id.is_none() || string_param(params, &["name"]).is_none() =>
        {
            Some("Missing group_id or name")
        }
        "workspace.group.add" | "workspace.group.set_anchor"
            if group_id.is_none() || workspace_id.is_none() =>
        {
            Some("Missing group_id or workspace_id")
        }
        "workspace.group.remove" if workspace_id.is_none() => {
            Some("Missing or invalid workspace_id")
        }
        _ if !matches!(method, "workspace.group.create" | "workspace.group.remove")
            && group_id.is_none() =>
        {
            Some("Missing or invalid group_id")
        }
        _ => None,
    }
}
