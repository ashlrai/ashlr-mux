use super::*;

pub(in crate::control_socket) fn workspace_create_cwd_param(
    params: &serde_json::Map<String, Value>,
    inherited: Option<&str>,
) -> Result<Option<String>, ()> {
    let working_directory = params
        .get("working_directory")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if working_directory.is_some() {
        return Ok(working_directory);
    }
    match params.get("cwd") {
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(()),
        None => Ok(inherited.map(str::to_string)),
    }
}

pub(in crate::control_socket) fn workspace_create_initial_env(
    params: &serde_json::Map<String, Value>,
) -> BTreeMap<String, String> {
    raw_string_map_param(params, "initial_env")
        .into_iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            (!key.is_empty()).then(|| (key.to_string(), value))
        })
        .collect()
}

pub(in crate::control_socket) fn workspace_create_workspace_env(
    params: &serde_json::Map<String, Value>,
) -> BTreeMap<String, String> {
    raw_string_map_param(params, "workspace_env")
        .into_iter()
        .filter_map(|(key, value)| {
            let key = key.trim();
            (!key.is_empty()
                && !value.is_empty()
                && !key.contains('\0')
                && !key.contains('=')
                && !value.contains('\0'))
            .then(|| (key.to_string(), value))
        })
        .collect()
}

fn raw_string_map_param(
    params: &serde_json::Map<String, Value>,
    key: &str,
) -> BTreeMap<String, String> {
    params
        .get(key)
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect()
}

pub(in crate::control_socket) fn canonical_layout_is_valid(
    layout: &cmux_config::CmuxLayoutNode,
) -> bool {
    match layout {
        cmux_config::CmuxLayoutNode::Pane(pane) => !pane.surfaces.is_empty(),
        cmux_config::CmuxLayoutNode::Split(split) => {
            split.children.len() == 2 && split.children.iter().all(canonical_layout_is_valid)
        }
    }
}
