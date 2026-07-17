use super::*;

pub(super) fn snapshot(app: &AppHandle) -> AppSessionSnapshot {
    let state = app.state::<SessionState>();
    current_session_snapshot(&state)
}

pub(super) fn invalid_params(message: &str) -> ControlCallResult {
    ControlCallResult::Err {
        code: "invalid_params".to_string(),
        message: message.to_string(),
        data: None,
    }
}

pub(super) fn not_supported(message: &str) -> ControlCallResult {
    ControlCallResult::Err {
        code: "not_supported".to_string(),
        message: message.to_string(),
        data: None,
    }
}

pub(super) fn is_unported_browser_automation_method(_method: &str) -> bool {
    false
}

pub(super) fn ok(value: Value) -> ControlCallResult {
    match JsonValue::try_from(value) {
        Ok(value) => ControlCallResult::Ok(value),
        Err(error) => ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Could not encode control response: {error}"),
            data: None,
        },
    }
}

#[cfg(test)]
pub(super) fn workspace_list_payload(snapshot: &AppSessionSnapshot) -> Value {
    let Some(window) = snapshot.windows.first() else {
        return Value::Null;
    };
    let selected = selected_workspace_index(snapshot);
    json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "workspaces": window.tab_manager.workspaces.iter().enumerate()
            .map(|(index, workspace)| workspace_summary(workspace, index, index == selected))
            .collect::<Vec<_>>(),
        "workspace_groups": workspace_group_summaries(
            &window.tab_manager.workspaces,
            &window.tab_manager.workspace_groups,
        ),
    })
}

pub(super) fn workspace_list_from_params_for_app(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(window_index) = workspace_routed_window_index(snapshot, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let mut payload = workspace_list_payload_for_window(snapshot, window_index);
    apply_workspace_handle_refs(app, &mut payload);
    ok(payload)
}

pub(super) fn workspace_list_payload_for_window(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
) -> Value {
    let Some(window) = snapshot.windows.get(window_index) else {
        return Value::Null;
    };
    let selected = selected_workspace_index_for_window(snapshot, window_index);
    json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| window_ref(window_index)),
        "workspaces": window
            .tab_manager
            .workspaces
            .iter()
            .enumerate()
            .map(|(index, workspace)| canonical_workspace_summary(workspace, index, Some(index) == selected))
            .collect::<Vec<_>>(),
    })
}

pub(super) fn extension_sidebar_snapshot_payload_for_app(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Value {
    extension_sidebar_snapshot_payload_with_events(
        snapshot,
        extension_sidebar_events_context(app),
        custom_sidebar_asset_map_from_params(params),
    )
}

#[allow(dead_code)]
pub(super) fn extension_sidebar_snapshot_payload(snapshot: &AppSessionSnapshot) -> Value {
    extension_sidebar_snapshot_payload_with_events(
        snapshot,
        extension_sidebar_empty_events_context(),
        json!({}),
    )
}

pub(super) fn extension_sidebar_snapshot_payload_with_events(
    snapshot: &AppSessionSnapshot,
    events: Value,
    assets: Value,
) -> Value {
    let latest_seq = events
        .get("latest_seq")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let Some(window) = snapshot.windows.first() else {
        let data = extension_sidebar_data_context(Vec::new(), None, None, 0, events.clone());
        return json!({
            "protocol": "cmux-extension-sidebar-snapshot",
            "version": 1,
            "window_id": Value::Null,
            "window_ref": Value::Null,
            "selected_workspace_id": Value::Null,
            "selected_workspace_ref": Value::Null,
            "selectedId": Value::Null,
            "selectedTitle": Value::Null,
            "workspace_count": 0,
            "workspaceCount": 0,
            "unread_total": 0,
            "unreadTotal": 0,
            "seq": latest_seq,
            "latest_seq": latest_seq,
            "events": events,
            "assets": assets,
            "data": data,
            "workspaces": [],
            "workspace_groups": [],
        });
    };

    let selected = selected_workspace_index(snapshot);
    let workspaces: Vec<Value> = window
        .tab_manager
        .workspaces
        .iter()
        .enumerate()
        .map(|(index, workspace)| extension_sidebar_workspace(workspace, index, index == selected))
        .collect();
    let selected_workspace = window.tab_manager.workspaces.get(selected);

    let selected_workspace_id =
        selected_workspace.and_then(|workspace| workspace.workspace_id.clone());
    let selected_title = selected_workspace.map(workspace_display_name);
    let unread_total = workspaces
        .iter()
        .filter_map(|workspace| workspace.get("unread").and_then(Value::as_u64))
        .sum::<u64>();
    let data = extension_sidebar_data_context(
        window
            .tab_manager
            .workspaces
            .iter()
            .enumerate()
            .map(|(index, workspace)| {
                extension_sidebar_data_workspace(workspace, index, index == selected)
            })
            .collect(),
        selected_workspace_id.as_deref(),
        selected_title.as_deref(),
        unread_total,
        events.clone(),
    );

    json!({
        "protocol": "cmux-extension-sidebar-snapshot",
        "version": 1,
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "selected_workspace_id": selected_workspace_id.clone(),
        "selected_workspace_ref": selected_workspace.map(|_| workspace_ref(selected)),
        "selectedId": selected_workspace_id,
        "selected_title": selected_title.clone(),
        "selectedTitle": selected_title,
        "workspace_count": window.tab_manager.workspaces.len(),
        "workspaceCount": window.tab_manager.workspaces.len(),
        "unread_total": unread_total,
        "unreadTotal": unread_total,
        "seq": latest_seq,
        "latest_seq": latest_seq,
        "events": events,
        "assets": assets,
        "data": data,
        "workspaces": workspaces,
        "workspace_groups": workspace_group_summaries(&window.tab_manager.workspaces, &window.tab_manager.workspace_groups),
    })
}

pub(super) fn extension_sidebar_events_context(app: &AppHandle) -> Value {
    let Some(state) = app.try_state::<ControlEventState>() else {
        return extension_sidebar_empty_events_context();
    };
    let (boot_id, next_seq, retained_events) = {
        let guard = state
            .inner
            .lock()
            .expect("control event log mutex poisoned");
        (
            guard.boot_id.clone(),
            guard.next_seq,
            guard.events.iter().cloned().collect::<Vec<_>>(),
        )
    };
    extension_sidebar_events_context_from_retained(boot_id, next_seq, retained_events)
}

pub(super) fn extension_sidebar_empty_events_context() -> Value {
    extension_sidebar_events_context_from_retained(String::new(), 1, Vec::new())
}

pub(super) fn extension_sidebar_events_context_from_retained(
    boot_id: String,
    next_seq: u64,
    retained_events: Vec<Value>,
) -> Value {
    let latest_seq = next_seq.saturating_sub(1);
    let oldest_seq = retained_events
        .first()
        .and_then(|event| event.get("seq"))
        .and_then(Value::as_u64)
        .unwrap_or(next_seq);
    let latest = retained_events.last().cloned().unwrap_or(Value::Null);
    let mut category_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut name_counts: BTreeMap<String, u64> = BTreeMap::new();
    for event in &retained_events {
        if let Some(category) = event.get("category").and_then(Value::as_str) {
            *category_counts.entry(category.to_string()).or_default() += 1;
        }
        if let Some(name) = event.get("name").and_then(Value::as_str) {
            *name_counts.entry(name.to_string()).or_default() += 1;
        }
    }
    let retained_count = retained_events.len();
    let recent_start = retained_events.len().saturating_sub(50);
    let recent: Vec<Value> = retained_events.into_iter().skip(recent_start).collect();
    json!({
        "protocol": EVENT_STREAM_PROTOCOL,
        "version": EVENT_STREAM_VERSION,
        "boot_id": if boot_id.is_empty() { Value::Null } else { json!(boot_id) },
        "latest_seq": latest_seq,
        "seq": latest_seq,
        "next_seq": next_seq,
        "oldest_seq": oldest_seq,
        "retained_count": retained_count,
        "latest": latest,
        "recent": recent,
        "counts": category_counts,
        "category_counts": category_counts,
        "name_counts": name_counts,
    })
}

pub(super) fn sidebar_validate(params: &serde_json::Map<String, Value>) -> ControlCallResult {
    let name = sidebar_name_param(params);
    validate_custom_sidebars_in_dir(&custom_sidebar_directory(), name.as_deref())
}

pub(super) fn sidebar_open(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(name) = sidebar_name_param(params) else {
        return invalid_params("Missing custom sidebar name");
    };
    let dir = custom_sidebar_directory();
    let candidate = match custom_sidebar_candidate_for_name(&dir, &name) {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: format!("Custom sidebar '{name}' was not found in {}", dir.display()),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            }
        }
        Err(error) => {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Failed to inspect custom sidebars: {error}"),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            }
        }
    };
    let validation = validate_custom_sidebar_candidate(&candidate);
    if !validation
        .get("valid")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return ControlCallResult::Err {
            code: "invalid_sidebar".to_string(),
            message: format!("Custom sidebar '{name}' did not validate"),
            data: Some(validation.try_into().unwrap_or(JsonValue::Null)),
        };
    }
    let current = snapshot(app);
    let Some(panel_id) = surface_id_from_params_or_focused(&current, params) else {
        return invalid_params("Missing or invalid surface selector");
    };
    let state = app.state::<SessionState>();
    let path = candidate.path.to_string_lossy().to_string();
    match open_custom_sidebar_in_panel(app, &state, &panel_id, &path) {
        Ok(Some(snapshot)) => {
            let surface = surface_list_from_params(&snapshot, params);
            ok(json!({
                "accepted": true,
                "name": candidate.name,
                "kind": candidate.kind,
                "path": path,
                "surface_id": panel_id,
                "surface": match surface {
                    ControlCallResult::Ok(value) => Value::from(value),
                    _ => Value::Null,
                },
                "warnings": validation.get("warnings").cloned().unwrap_or_else(|| json!([])),
            }))
        }
        Ok(None) => ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!("unable to open custom sidebar in pane {panel_id}"),
            data: None,
        },
        Err(message) => ControlCallResult::Err {
            code: "internal".to_string(),
            message,
            data: None,
        },
    }
}

pub(super) fn sidebar_reload(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let name = sidebar_name_param(params);
    let validation =
        match validate_custom_sidebars_in_dir(&custom_sidebar_directory(), name.as_deref()) {
            ControlCallResult::Ok(value) => Value::from(value),
            error => return error,
        };

    if name.is_some()
        && !validation
            .get("ok")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return ControlCallResult::Err {
            code: "invalid_sidebar".to_string(),
            message: format!(
                "Custom sidebar '{}' did not validate",
                name.as_deref().unwrap_or_default()
            ),
            data: Some(validation.try_into().unwrap_or(JsonValue::Null)),
        };
    }

    let payload = custom_sidebar_reload_payload(name.as_deref(), &validation);
    if let Err(error) = app.emit(CUSTOM_SIDEBAR_RELOAD_EVENT, payload.clone()) {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Failed to emit custom sidebar reload event: {error}"),
            data: None,
        };
    }

    ok(json!({
        "accepted": true,
        "event": CUSTOM_SIDEBAR_RELOAD_EVENT,
        "name": name,
        "all": payload.get("all").cloned().unwrap_or(Value::Bool(false)),
        "paths": payload.get("paths").cloned().unwrap_or_else(|| json!([])),
        "sidebars": payload.get("sidebars").cloned().unwrap_or_else(|| json!([])),
        "validation": validation,
    }))
}

pub(super) fn custom_sidebar_reload_payload(name: Option<&str>, validation: &Value) -> Value {
    let sidebars: Vec<Value> = validation
        .get("sidebars")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|sidebar| {
            sidebar
                .get("valid")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    let paths: Vec<Value> = sidebars
        .iter()
        .filter_map(|sidebar| sidebar.get("path").and_then(Value::as_str))
        .map(|path| json!(path))
        .collect();
    json!({
        "protocol": "cmux-custom-sidebar-reload",
        "version": 1,
        "event": CUSTOM_SIDEBAR_RELOAD_EVENT,
        "name": name,
        "all": name.is_none(),
        "paths": paths,
        "sidebars": sidebars,
    })
}

pub(super) fn sidebar_select(
    app: &AppHandle,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(name) = sidebar_name_param(params) else {
        return invalid_params("Missing custom sidebar name");
    };
    let dir = custom_sidebar_directory();
    let candidate = match custom_sidebar_candidate_for_name(&dir, &name) {
        Ok(Some(candidate)) => candidate,
        Ok(None) => {
            return ControlCallResult::Err {
                code: "not_found".to_string(),
                message: format!("Custom sidebar '{name}' was not found in {}", dir.display()),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
        Err(error) => {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Failed to inspect custom sidebars: {error}"),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };
    let validation = validate_custom_sidebar_candidate(&candidate);
    if !validation
        .get("valid")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return ControlCallResult::Err {
            code: "invalid_sidebar".to_string(),
            message: format!("Custom sidebar '{name}' did not validate"),
            data: Some(validation.try_into().unwrap_or(JsonValue::Null)),
        };
    }

    let payload = custom_sidebar_select_payload(&candidate, &validation);
    if let Err(error) = app.emit(CUSTOM_SIDEBAR_SELECT_EVENT, payload.clone()) {
        return ControlCallResult::Err {
            code: "internal_error".to_string(),
            message: format!("Failed to emit custom sidebar select event: {error}"),
            data: None,
        };
    }

    ok(payload)
}

pub(super) fn custom_sidebar_select_payload(
    candidate: &CustomSidebarCandidate,
    validation: &Value,
) -> Value {
    let path = candidate.path.to_string_lossy().to_string();
    json!({
        "accepted": true,
        "protocol": "cmux-custom-sidebar-select",
        "version": 1,
        "event": CUSTOM_SIDEBAR_SELECT_EVENT,
        "name": candidate.name,
        "kind": candidate.kind,
        "path": path,
        "sidebar": validation,
        "warnings": validation.get("warnings").cloned().unwrap_or_else(|| json!([])),
    })
}

pub(super) fn sidebar_name_param(params: &serde_json::Map<String, Value>) -> Option<String> {
    string_param(params, &["name", "sidebar", "id"])
        .map(|name| {
            Path::new(name.trim())
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_else(|| name.trim())
                .to_string()
        })
        .filter(|name| !name.is_empty())
}

pub(crate) fn custom_sidebar_directory() -> PathBuf {
    std::env::var_os("CMUX_SIDEBARS_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            event_log_home_directory()
                .map(|home| home.join(".config").join("cmux").join("sidebars"))
        })
        .unwrap_or_else(|| {
            PathBuf::from(".")
                .join(".config")
                .join("cmux")
                .join("sidebars")
        })
}

pub(super) fn custom_sidebar_asset_map_from_params(
    params: &serde_json::Map<String, Value>,
) -> Value {
    let source_path = string_param(params, &["source_path", "sourcePath", "path"]);
    custom_sidebar_asset_map_for_source(source_path.as_deref())
}

pub(super) fn custom_sidebar_asset_map_for_source(source_path: Option<&str>) -> Value {
    let Some(source_path) = canonical_custom_sidebar_source(source_path) else {
        return json!({});
    };
    let Some(asset_root) = custom_sidebar_asset_root_for_source(&source_path) else {
        return json!({});
    };
    let sidebar_name = source_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("sidebar");
    let mut assets = serde_json::Map::new();
    collect_custom_sidebar_assets(
        &source_path,
        &asset_root,
        &asset_root,
        sidebar_name,
        &mut assets,
        0,
    );
    Value::Object(assets)
}

pub(super) fn canonical_custom_sidebar_source(source_path: Option<&str>) -> Option<PathBuf> {
    let source_path = PathBuf::from(source_path?.trim());
    let source_path = fs::canonicalize(source_path).ok()?;
    if !source_path.is_file() {
        return None;
    }
    let sidebar_dir = fs::canonicalize(custom_sidebar_directory()).ok()?;
    source_path.starts_with(sidebar_dir).then_some(source_path)
}

pub(super) fn custom_sidebar_asset_root_for_source(source_path: &Path) -> Option<PathBuf> {
    let dir = source_path.parent()?;
    let stem = source_path.file_stem()?.to_str()?;
    let asset_root = dir.join(format!("{stem}.assets"));
    let asset_root = fs::canonicalize(asset_root).ok()?;
    asset_root.is_dir().then_some(asset_root)
}

pub(super) fn collect_custom_sidebar_assets(
    source_path: &Path,
    asset_root: &Path,
    current_dir: &Path,
    sidebar_name: &str,
    assets: &mut serde_json::Map<String, Value>,
    depth: usize,
) {
    if depth > 4 || assets.len() >= 256 {
        return;
    }
    let Ok(entries) = fs::read_dir(current_dir) else {
        return;
    };
    for entry in entries.flatten() {
        if assets.len() >= 256 {
            return;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_custom_sidebar_assets(
                source_path,
                asset_root,
                &path,
                sidebar_name,
                assets,
                depth + 1,
            );
            continue;
        }
        if !path.is_file() || custom_sidebar_asset_mime(&path).is_none() {
            continue;
        }
        let Ok(relative) = path.strip_prefix(asset_root) else {
            continue;
        };
        let relative_name = relative.to_string_lossy().replace('\\', "/");
        if relative_name.is_empty() || relative_name.contains("..") {
            continue;
        }
        let url = custom_sidebar_asset_url(source_path, sidebar_name, &relative_name);
        assets.insert(relative_name.clone(), json!(url));
        if let Some(stem) = relative_name.rsplit_once('.').map(|(stem, _)| stem) {
            assets.entry(stem.to_string()).or_insert_with(|| json!(url));
        }
    }
}

pub(super) fn custom_sidebar_asset_url(
    source_path: &Path,
    sidebar_name: &str,
    relative_name: &str,
) -> String {
    format!(
        "cmux-sidebar-asset://{}/{}?source={}",
        percent_encode_component(sidebar_name),
        relative_name
            .split('/')
            .map(percent_encode_component)
            .collect::<Vec<_>>()
            .join("/"),
        percent_encode_component(&source_path.to_string_lossy()),
    )
}

pub(crate) fn resolve_custom_sidebar_asset_request(uri: &str) -> Option<(PathBuf, String)> {
    let rest = strip_custom_sidebar_asset_scheme(uri)?;
    let (path_part, query) = rest.split_once('?')?;
    if path_part.contains('#') || query.contains('#') {
        return None;
    }
    let source_path = query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=')?;
        (key == "source").then(|| percent_decode_component(value))
    })?;
    let source_path = canonical_custom_sidebar_source(Some(&source_path))?;
    let asset_root = custom_sidebar_asset_root_for_source(&source_path)?;
    let relative = path_part
        .split_once('/')
        .map(|(_, relative)| relative)
        .unwrap_or_default();
    if relative.is_empty() || relative.contains("..") || relative.contains('\\') {
        return None;
    }
    let decoded_segments: Vec<String> = relative
        .split('/')
        .map(percent_decode_component)
        .filter(|segment| !segment.is_empty() && segment != "." && segment != "..")
        .collect();
    if decoded_segments.is_empty() {
        return None;
    }
    let mut candidate = asset_root.clone();
    for segment in decoded_segments {
        candidate.push(segment);
    }
    let candidate = fs::canonicalize(candidate).ok()?;
    if !candidate.starts_with(&asset_root) || !candidate.is_file() {
        return None;
    }
    let mime = custom_sidebar_asset_mime(&candidate)?;
    Some((candidate, mime.to_string()))
}

pub(super) fn strip_custom_sidebar_asset_scheme(uri: &str) -> Option<&str> {
    if let Some(rest) = uri.strip_prefix("cmux-sidebar-asset://") {
        return Some(rest);
    }
    let after = uri
        .strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))?;
    after.strip_prefix("cmux-sidebar-asset.localhost/")
}

pub(super) fn custom_sidebar_asset_mime(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        "avif" => Some("image/avif"),
        "ico" => Some("image/x-icon"),
        _ => None,
    }
}

pub(super) fn percent_encode_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.as_bytes() {
        let b = *byte;
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    encoded
}

pub(super) fn percent_decode_component(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                decoded.push(hex);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).to_string()
}

pub(super) fn validate_custom_sidebars_in_dir(dir: &Path, name: Option<&str>) -> ControlCallResult {
    let normalized_name = name.map(str::trim).filter(|name| !name.is_empty());
    let candidates = match discover_custom_sidebars(dir, normalized_name) {
        Ok(candidates) => candidates,
        Err(error) => {
            return ControlCallResult::Err {
                code: "io_error".to_string(),
                message: format!("Failed to inspect custom sidebars: {error}"),
                data: Some(
                    json!({ "sidebar_dir": dir })
                        .try_into()
                        .unwrap_or(JsonValue::Null),
                ),
            };
        }
    };

    if normalized_name.is_some() && candidates.is_empty() {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: format!(
                "Custom sidebar '{}' was not found in {}",
                normalized_name.unwrap_or_default(),
                dir.display()
            ),
            data: Some(
                json!({ "sidebar_dir": dir })
                    .try_into()
                    .unwrap_or(JsonValue::Null),
            ),
        };
    }

    let sidebars: Vec<Value> = candidates
        .iter()
        .map(|candidate| validate_custom_sidebar_candidate(candidate))
        .collect();
    let valid_count = sidebars
        .iter()
        .filter(|sidebar| {
            sidebar
                .get("valid")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();
    let invalid_count = sidebars.len().saturating_sub(valid_count);
    ok(json!({
        "protocol": "cmux-custom-sidebar-validation",
        "version": 1,
        "sidebar_dir": dir,
        "exists": dir.is_dir(),
        "name": normalized_name,
        "ok": invalid_count == 0,
        "valid_count": valid_count,
        "invalid_count": invalid_count,
        "sidebars": sidebars,
    }))
}

#[derive(Debug, Clone)]
pub(super) struct CustomSidebarCandidate {
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) kind: String,
    pub(super) shadowed_json_path: Option<PathBuf>,
    pub(super) manifest_path: Option<PathBuf>,
}

pub(super) fn discover_custom_sidebars(
    dir: &Path,
    name: Option<&str>,
) -> std::io::Result<Vec<CustomSidebarCandidate>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut by_name: BTreeMap<String, CustomSidebarCandidate> = BTreeMap::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(extension) = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
        else {
            continue;
        };
        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".manifest.json"))
        {
            continue;
        }
        if extension != "swift" && extension != "json" {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if name.is_some_and(|name| name != stem) {
            continue;
        }
        let candidate = CustomSidebarCandidate {
            name: stem.to_string(),
            kind: extension.clone(),
            path: path.clone(),
            shadowed_json_path: None,
            manifest_path: custom_sidebar_manifest_path(dir, stem),
        };
        match by_name.get_mut(stem) {
            Some(existing) if existing.kind == "swift" && extension == "json" => {
                existing.shadowed_json_path = Some(path);
            }
            Some(existing) if existing.kind == "json" && extension == "swift" => {
                let shadowed_json_path = Some(existing.path.clone());
                *existing = CustomSidebarCandidate {
                    shadowed_json_path,
                    ..candidate
                };
            }
            Some(_) => {}
            None => {
                by_name.insert(stem.to_string(), candidate);
            }
        }
    }
    Ok(by_name.into_values().collect())
}

pub(super) fn custom_sidebar_candidate_for_name(
    dir: &Path,
    name: &str,
) -> std::io::Result<Option<CustomSidebarCandidate>> {
    Ok(discover_custom_sidebars(dir, Some(name))?
        .into_iter()
        .next())
}

pub(super) fn validate_custom_sidebar_candidate(candidate: &CustomSidebarCandidate) -> Value {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let manifest = candidate
        .manifest_path
        .as_ref()
        .map(|path| custom_sidebar_manifest_summary(path));
    if manifest
        .as_ref()
        .and_then(|manifest| manifest.get("valid"))
        .and_then(Value::as_bool)
        == Some(false)
    {
        warnings.push("custom sidebar capability manifest is invalid".to_string());
    }
    match fs::read_to_string(&candidate.path) {
        Ok(source) if source.trim().is_empty() => {
            errors.push("sidebar file is empty".to_string());
        }
        Ok(source) if candidate.kind == "json" => {
            if let Err(error) = serde_json::from_str::<Value>(&source) {
                errors.push(format!("invalid JSON: {error}"));
            }
        }
        Ok(_) if candidate.kind == "swift" => {
            warnings.push(
                "SwiftUI syntax interpretation is not yet available on Windows/Tauri".to_string(),
            );
        }
        Ok(_) => {}
        Err(error) => {
            errors.push(format!("failed to read sidebar file: {error}"));
        }
    }
    json!({
        "name": candidate.name,
        "kind": candidate.kind,
        "path": candidate.path,
        "valid": errors.is_empty(),
        "errors": errors,
        "warnings": warnings,
        "shadowed_json_path": candidate.shadowed_json_path,
        "manifest_path": candidate.manifest_path,
        "manifest": manifest,
    })
}

pub(super) fn custom_sidebar_manifest_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let path = dir.join(format!("{name}.manifest.json"));
    path.is_file().then_some(path)
}

pub(super) fn custom_sidebar_manifest_for_source(source_path: Option<&str>) -> Option<Value> {
    let source_path = PathBuf::from(source_path?.trim());
    let dir = source_path.parent()?;
    let stem = source_path.file_stem()?.to_str()?;
    let manifest_path = custom_sidebar_manifest_path(dir, stem)?;
    Some(custom_sidebar_manifest_summary(&manifest_path))
}

pub(super) fn custom_sidebar_manifest_summary(path: &Path) -> Value {
    let mut errors = Vec::new();
    let mut requested_methods = Vec::new();
    let mut trusted = false;

    match fs::read_to_string(path) {
        Ok(source) => match serde_json::from_str::<Value>(&source) {
            Ok(Value::Object(manifest)) => {
                trusted = manifest
                    .get("trusted")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                for key in ["capabilities", "allowed_methods", "methods"] {
                    if let Some(Value::Array(values)) = manifest.get(key) {
                        requested_methods.extend(values.iter().filter_map(|value| {
                            value
                                .as_str()
                                .map(str::trim)
                                .filter(|method| !method.is_empty())
                                .map(str::to_string)
                        }));
                    }
                }
            }
            Ok(_) => errors.push("manifest must be a JSON object".to_string()),
            Err(error) => errors.push(format!("invalid manifest JSON: {error}")),
        },
        Err(error) => errors.push(format!("failed to read manifest: {error}")),
    }

    requested_methods.sort();
    requested_methods.dedup();
    let allowed_requested_methods: Vec<String> = requested_methods
        .iter()
        .filter(|method| custom_sidebar_action_policy_allows(method))
        .cloned()
        .collect();
    let denied_requested_methods: Vec<String> = requested_methods
        .iter()
        .filter(|method| !custom_sidebar_action_policy_allows(method))
        .cloned()
        .collect();

    json!({
        "path": path,
        "valid": errors.is_empty(),
        "errors": errors,
        "trusted": trusted,
        "requested_methods": requested_methods,
        "allowed_requested_methods": allowed_requested_methods,
        "denied_requested_methods": denied_requested_methods,
        "policy": CUSTOM_SIDEBAR_ACTION_POLICY,
        "enforced": true,
    })
}

pub(super) fn extension_sidebar_data_context(
    workspaces: Vec<Value>,
    selected_workspace_id: Option<&str>,
    selected_title: Option<&str>,
    unread_total: u64,
    events: Value,
) -> Value {
    json!({
        "workspaces": workspaces,
        "workspaceCount": workspaces.len(),
        "selectedTitle": selected_title.unwrap_or(""),
        "selectedId": selected_workspace_id.unwrap_or(""),
        "unreadTotal": unread_total,
        "clock": extension_sidebar_clock_context(),
        "events": events,
    })
}

pub(super) fn extension_sidebar_clock_context() -> Value {
    let now = OffsetDateTime::now_utc();
    json!({
        "time": format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second()),
        "hour": now.hour(),
        "minute": now.minute(),
        "second": now.second(),
        "weekday": now.weekday().number_from_sunday(),
        "epoch": now.unix_timestamp(),
    })
}

pub(super) fn extension_sidebar_data_workspace(
    workspace: &SessionWorkspaceSnapshot,
    index: usize,
    selected: bool,
) -> Value {
    let tabs = extension_sidebar_data_tabs(workspace);
    let ports = workspace_listening_ports(workspace);
    let unread = tabs
        .iter()
        .filter(|tab| tab.get("unread").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let pull_requests = extension_sidebar_pull_requests(workspace);
    let (branch, dirty) = extension_sidebar_branch_summary(workspace);
    let mut object = serde_json::Map::new();

    if let Some(id) = workspace.workspace_id.as_deref() {
        object.insert("id".to_string(), json!(id));
    }
    object.insert(
        "title".to_string(),
        json!(workspace_display_name(workspace)),
    );
    object.insert("selected".to_string(), json!(selected));
    object.insert(
        "pinned".to_string(),
        json!(workspace.is_pinned.unwrap_or(false)),
    );
    object.insert("index".to_string(), json!(index));
    object.insert(
        "directory".to_string(),
        json!(workspace.current_directory.clone().unwrap_or_default()),
    );
    object.insert("ports".to_string(), json!(ports));
    object.insert("portCount".to_string(), json!(ports.len()));
    object.insert("unread".to_string(), json!(unread));
    object.insert("tabs".to_string(), json!(tabs));
    object.insert(
        "tabCount".to_string(),
        json!(surfaces_for_workspace(workspace).len()),
    );

    insert_non_empty_string(
        &mut object,
        "description",
        workspace.custom_description.as_deref(),
    );
    insert_non_empty_string(&mut object, "color", workspace.custom_color.as_deref());
    if let Some(branch) = branch {
        object.insert("branch".to_string(), json!(branch));
        object.insert("dirty".to_string(), json!(dirty));
    }
    if let Some(first_pull_request) = pull_requests.first() {
        object.insert("pr".to_string(), first_pull_request.clone());
        object.insert("prs".to_string(), json!(pull_requests));
    }
    if let Some(progress) = workspace.sidebar_progress.as_ref() {
        let mut progress_object = serde_json::Map::new();
        progress_object.insert("value".to_string(), json!(progress.value));
        insert_non_empty_string(&mut progress_object, "label", progress.label.as_deref());
        object.insert("progress".to_string(), Value::Object(progress_object));
    }
    if let Some(remote) = workspace.remote.as_ref() {
        let target = remote
            .destination
            .as_deref()
            .or(remote.detail.as_deref())
            .unwrap_or("");
        object.insert(
            "remote".to_string(),
            json!({
                "target": target,
                "state": remote.state,
                "connected": remote.connected,
            }),
        );
    }

    Value::Object(object)
}

pub(super) fn extension_sidebar_data_tabs(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    surfaces_for_workspace(workspace)
        .into_iter()
        .map(|surface| {
            let panel_id = surface.get("id").and_then(Value::as_str);
            let mut object = serde_json::Map::new();
            if let Some(panel_id) = panel_id {
                object.insert("id".to_string(), json!(panel_id));
            }
            object.insert(
                "title".to_string(),
                surface
                    .get("title")
                    .cloned()
                    .unwrap_or_else(|| json!("terminal")),
            );
            object.insert(
                "focused".to_string(),
                json!(surface
                    .get("focused")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)),
            );
            object.insert(
                "pinned".to_string(),
                json!(surface
                    .get("pinned")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)),
            );
            insert_non_empty_string(
                &mut object,
                "directory",
                workspace.current_directory.as_deref(),
            );
            if let Some(panel_id) = panel_id {
                if let Some((branch, dirty)) = extension_sidebar_panel_branch(workspace, panel_id) {
                    object.insert("branch".to_string(), json!(branch));
                    object.insert("dirty".to_string(), json!(dirty));
                }
                let ports = panel_listening_ports(workspace, panel_id);
                if !ports.is_empty() {
                    object.insert("ports".to_string(), json!(ports));
                }
            }
            object.insert(
                "unread".to_string(),
                json!(surface
                    .get("unread")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)),
            );
            Value::Object(object)
        })
        .collect()
}

pub(super) fn insert_non_empty_string(
    object: &mut serde_json::Map<String, Value>,
    key: &str,
    value: Option<&str>,
) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        object.insert(key.to_string(), json!(value));
    }
}

pub(super) fn extension_sidebar_workspace(
    workspace: &SessionWorkspaceSnapshot,
    index: usize,
    selected: bool,
) -> Value {
    let mut summary = workspace_summary(workspace, index, selected);
    let tabs = extension_sidebar_tabs(workspace);
    let ports = workspace_listening_ports(workspace);
    let unread = tabs
        .iter()
        .filter(|tab| tab.get("unread").and_then(Value::as_bool).unwrap_or(false))
        .count();
    let pull_requests = extension_sidebar_pull_requests(workspace);
    let first_pull_request = pull_requests.first().cloned();
    let (branch, dirty) = extension_sidebar_branch_summary(workspace);

    if let Some(object) = summary.as_object_mut() {
        object.insert("directory".to_string(), json!(workspace.current_directory));
        object.insert("root_path".to_string(), json!(workspace.current_directory));
        object.insert(
            "project_root_path".to_string(),
            json!(workspace.current_directory),
        );
        object.insert("ports".to_string(), json!(ports));
        object.insert("port_count".to_string(), json!(ports.len()));
        object.insert("portCount".to_string(), json!(ports.len()));
        object.insert("tabs".to_string(), json!(tabs));
        object.insert(
            "tab_count".to_string(),
            json!(surfaces_for_workspace(workspace).len()),
        );
        object.insert(
            "tabCount".to_string(),
            json!(surfaces_for_workspace(workspace).len()),
        );
        object.insert("unread".to_string(), json!(unread));
        object.insert("branch".to_string(), json!(branch));
        object.insert("dirty".to_string(), json!(dirty));
        object.insert(
            "branch_summary".to_string(),
            json!(branch.map(|branch| {
                if dirty {
                    format!("{branch}*")
                } else {
                    branch
                }
            })),
        );
        object.insert("pr".to_string(), first_pull_request.unwrap_or(Value::Null));
        object.insert("prs".to_string(), json!(pull_requests));
        object.insert(
            "pull_request_urls".to_string(),
            json!(extension_sidebar_pull_request_urls(workspace)),
        );
        object.insert(
            "panel_directories".to_string(),
            json!(extension_sidebar_panel_directories(workspace)),
        );
        object.insert(
            "git_branches".to_string(),
            json!(workspace.panel_git_branches.clone().unwrap_or_default()),
        );
        object.insert("progress".to_string(), json!(workspace.sidebar_progress));
        object.insert("latestMessage".to_string(), Value::Null);
        object.insert("latestPrompt".to_string(), Value::Null);
        object.insert("latestAt".to_string(), Value::Null);
    }

    summary
}

pub(super) fn extension_sidebar_tabs(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    surfaces_for_workspace(workspace)
        .into_iter()
        .map(|mut surface| {
            let panel_id = surface
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string);
            if let Some(object) = surface.as_object_mut() {
                object.insert(
                    "directory".to_string(),
                    json!(workspace.current_directory.clone()),
                );
                object.insert(
                    "ports".to_string(),
                    panel_id
                        .as_deref()
                        .map(|panel_id| json!(panel_listening_ports(workspace, panel_id)))
                        .unwrap_or_else(|| json!([])),
                );
                object.insert(
                    "branch".to_string(),
                    panel_id
                        .as_deref()
                        .and_then(|panel_id| extension_sidebar_panel_branch(workspace, panel_id))
                        .map(|(branch, _dirty)| branch)
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                );
                object.insert(
                    "dirty".to_string(),
                    json!(panel_id.as_deref().is_some_and(|panel_id| {
                        extension_sidebar_panel_branch(workspace, panel_id)
                            .map(|(_branch, dirty)| dirty)
                            .unwrap_or(false)
                    })),
                );
            }
            surface
        })
        .collect()
}

pub(super) fn extension_sidebar_branch_summary(
    workspace: &SessionWorkspaceSnapshot,
) -> (Option<String>, bool) {
    if let Some(entry) = workspace
        .panel_git_branches
        .as_ref()
        .and_then(|branches| branches.first())
    {
        return (Some(entry.branch.clone()), entry.is_dirty);
    }
    workspace
        .git_branch
        .as_ref()
        .map(|branch| (Some(branch.branch.clone()), branch.is_dirty))
        .unwrap_or((None, false))
}

pub(super) fn extension_sidebar_panel_branch(
    workspace: &SessionWorkspaceSnapshot,
    panel_id: &str,
) -> Option<(String, bool)> {
    workspace
        .panel_git_branches
        .as_ref()
        .and_then(|branches| {
            branches
                .iter()
                .find(|entry| entry.panel_id == panel_id)
                .map(|entry| (entry.branch.clone(), entry.is_dirty))
        })
        .or_else(|| {
            workspace
                .git_branch
                .as_ref()
                .map(|entry| (entry.branch.clone(), entry.is_dirty))
        })
}

pub(super) fn extension_sidebar_pull_requests(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    workspace
        .panel_pull_requests
        .as_ref()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    json!({
                        "panel_id": entry.panel_id,
                        "number": entry.number,
                        "label": entry.label,
                        "url": entry.url,
                        "status": entry.status,
                        "stale": entry.is_stale,
                        "is_stale": entry.is_stale,
                        "branch": entry.branch,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn extension_sidebar_pull_request_urls(
    workspace: &SessionWorkspaceSnapshot,
) -> Vec<String> {
    workspace
        .panel_pull_requests
        .as_ref()
        .map(|entries| entries.iter().map(|entry| entry.url.clone()).collect())
        .unwrap_or_default()
}

pub(super) fn extension_sidebar_panel_directories(
    workspace: &SessionWorkspaceSnapshot,
) -> BTreeMap<String, String> {
    workspace
        .current_directory
        .as_ref()
        .map(|directory| {
            surfaces_for_workspace(workspace)
                .into_iter()
                .filter_map(|surface| {
                    surface
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|panel_id| (panel_id.to_string(), directory.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn workspace_current(snapshot: &AppSessionSnapshot) -> ControlCallResult {
    let params = serde_json::Map::new();
    workspace_current_from_params(snapshot, &params)
}

pub(super) fn workspace_current_from_params(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(window_index) = workspace_routed_window_index(snapshot, params) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let Some(window) = snapshot.windows.get(window_index) else {
        return ControlCallResult::Err {
            code: "unavailable".to_string(),
            message: "TabManager not available".to_string(),
            data: None,
        };
    };
    let Some(index) = selected_workspace_index_for_window(snapshot, window_index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No workspace selected".to_string(),
            data: None,
        };
    };
    let workspace = window.tab_manager.workspaces.get(index);
    let identity_id = workspace
        .and_then(|workspace| workspace.workspace_id.clone())
        .or_else(|| window.selected_workspace_id.clone());
    let Some(identity_id) = identity_id else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "No workspace selected".to_string(),
            data: None,
        };
    };
    ok(json!({
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| window_ref(window_index)),
        "workspace_id": identity_id,
        "workspace_ref": workspace_ref(index),
        "workspace": workspace.map(|workspace| canonical_workspace_summary(workspace, index, true)),
    }))
}

pub(super) fn workspace_current_from_params_for_app(
    app: &AppHandle,
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    match workspace_current_from_params(snapshot, params) {
        ControlCallResult::Ok(value) => {
            let mut payload: Value = value.into();
            apply_workspace_handle_refs(app, &mut payload);
            ok(payload)
        }
        error => error,
    }
}

pub(super) fn apply_workspace_handle_refs(app: &AppHandle, payload: &mut Value) {
    if let Some(window_id) = payload.get("window_id").and_then(Value::as_str) {
        payload["window_ref"] = json!(control_handle_ref(app, "window", window_id));
    }
    if let Some(workspaces) = payload.get_mut("workspaces").and_then(Value::as_array_mut) {
        for workspace in workspaces {
            if let Some(id) = workspace
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
            {
                workspace["ref"] = json!(control_handle_ref(app, "workspace", &id));
            }
        }
    }
    if let Some(workspace) = payload.get_mut("workspace") {
        if let Some(id) = workspace
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
        {
            workspace["ref"] = json!(control_handle_ref(app, "workspace", &id));
        }
    }
    if let Some(id) = payload
        .get("workspace_id")
        .and_then(Value::as_str)
        .map(str::to_string)
    {
        payload["workspace_ref"] = json!(control_handle_ref(app, "workspace", &id));
    }
}

pub(super) fn workspace_from_params_or_selected<'a>(
    snapshot: &'a AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<&'a SessionWorkspaceSnapshot> {
    let index = workspace_index_from_params_or_selected(snapshot, params)?;
    snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(index))
}

pub(super) fn recent_sidebar_log_entries(
    workspace: &SessionWorkspaceSnapshot,
    limit: usize,
) -> Vec<Value> {
    let entries = workspace.sidebar_log_entries.as_deref().unwrap_or_default();
    let start = entries.len().saturating_sub(limit);
    entries[start..]
        .iter()
        .rev()
        .map(|entry| json!(entry))
        .collect()
}

#[allow(dead_code)]
pub(super) fn surface_list(snapshot: &AppSessionSnapshot) -> ControlCallResult {
    let params = serde_json::Map::new();
    surface_list_from_params(snapshot, &params)
}

pub(super) fn surface_list_from_params(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> ControlCallResult {
    let Some(window) = snapshot.windows.first() else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(index) = workspace_index_from_workspace_scope_or_selected(snapshot, params) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    let Some(workspace) = window.tab_manager.workspaces.get(index) else {
        return ControlCallResult::Err {
            code: "not_found".to_string(),
            message: "Workspace not found".to_string(),
            data: None,
        };
    };
    ok(json!({
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(index),
        "surfaces": surfaces_for_workspace(workspace),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
    }))
}

pub(super) fn selected_workspace_index(snapshot: &AppSessionSnapshot) -> usize {
    snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.selected_workspace_index)
        .and_then(|index| usize::try_from(index).ok())
        .unwrap_or(0)
}

pub(super) fn selected_workspace_index_for_window(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
) -> Option<usize> {
    snapshot
        .windows
        .get(window_index)?
        .tab_manager
        .selected_workspace_index
        .and_then(|index| usize::try_from(index).ok())
}

pub(super) fn window_ref(index: usize) -> String {
    format!("window:{}", index + 1)
}

/// Resolve the v2 routing selectors to a tab manager without changing focus.
/// An explicit window selector is authoritative: an invalid value never falls
/// through to a workspace/surface in another window.
pub(super) fn workspace_routed_window_index(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    let has_non_null_window_selector = params
        .get("window_id")
        .or_else(|| params.get("window_ref"))
        .is_some_and(|value| !value.is_null());
    if has_non_null_window_selector {
        let selector = raw_string_param(params, &["window_id", "window_ref"])?;
        if let Some(index) = one_based_ref_index(&selector, "window") {
            return (index < snapshot.windows.len()).then_some(index);
        }
        return snapshot
            .windows
            .iter()
            .position(|window| window.window_id.as_deref() == Some(selector.as_str()));
    }

    if let Some(group_id) = string_param(params, &["group_id"]) {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window
                .tab_manager
                .workspaces
                .iter()
                .any(|workspace| workspace.group_id.as_deref() == Some(group_id.as_str()))
        }) {
            return Some(index);
        }
    }

    if let Some(workspace_id) = string_param(params, &["workspace_id"]) {
        if one_based_ref_index(&workspace_id, "workspace").is_none() {
            if let Some(index) = snapshot.windows.iter().position(|window| {
                window.tab_manager.workspaces.iter().any(|workspace| {
                    workspace.workspace_id.as_deref() == Some(workspace_id.as_str())
                })
            }) {
                return Some(index);
            }
        }
    }

    if let Some(surface_id) = string_param(params, &["surface_id", "terminal_id", "tab_id"]) {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window.tab_manager.workspaces.iter().any(|workspace| {
                surfaces_for_workspace(workspace).iter().any(|surface| {
                    surface.get("id").and_then(Value::as_str) == Some(surface_id.as_str())
                })
            })
        }) {
            return Some(index);
        }
    }

    if let Some(pane_id) = string_param(params, &["pane_id"]) {
        if let Some(index) = snapshot.windows.iter().position(|window| {
            window.tab_manager.workspaces.iter().any(|workspace| {
                pane_event_summaries(workspace)
                    .iter()
                    .any(|pane| pane.id.as_deref() == Some(pane_id.as_str()))
            })
        }) {
            return Some(index);
        }
    }

    (!snapshot.windows.is_empty()).then_some(0)
}

pub(super) fn canonical_workspace_target_index(
    snapshot: &AppSessionSnapshot,
    window_index: usize,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    let selector = raw_string_param(params, &["workspace_id"])?;
    let workspaces = &snapshot.windows.get(window_index)?.tab_manager.workspaces;
    if let Some(index) = one_based_ref_index(&selector, "workspace") {
        return (index < workspaces.len()).then_some(index);
    }
    workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(selector.as_str()))
}

pub(super) fn workspace_index_for_id(
    snapshot: &AppSessionSnapshot,
    workspace_id: &str,
) -> Option<usize> {
    snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
}

pub(super) fn workspace_index_from_params(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    if let Some(workspace_ref) = string_param(params, &["workspace_ref", "ref"]) {
        if let Some(index) = one_based_ref_index(&workspace_ref, "workspace") {
            if snapshot
                .windows
                .first()
                .is_some_and(|window| index < window.tab_manager.workspaces.len())
            {
                return Some(index);
            }
            return None;
        }
    }

    let workspace_id = params
        .get("workspace_id")
        .or_else(|| params.get("id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    workspace_index_for_id(snapshot, workspace_id)
}

pub(super) fn workspace_reorder_destination_index(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    from_index: usize,
) -> Option<i64> {
    if params.contains_key("to_index")
        || params.contains_key("to")
        || params.contains_key("target_index")
    {
        return None;
    }

    let index = i64_param(params, &["index"]);
    let before = workspace_index_from_selector_keys(
        snapshot,
        params,
        &["before_workspace_ref", "before_ref"],
        &["before_workspace_id", "before_workspace"],
    );
    let after = workspace_index_from_selector_keys(
        snapshot,
        params,
        &["after_workspace_ref", "after_ref"],
        &["after_workspace_id", "after_workspace"],
    );
    match (index, before, after) {
        (Some(index), None, None) => Some(index),
        (None, Some(target), None) => {
            let destination = if from_index < target {
                target.saturating_sub(1)
            } else {
                target
            };
            Some(destination as i64)
        }
        (None, None, Some(target)) => {
            let destination = if from_index < target {
                target
            } else {
                target.saturating_add(1)
            };
            Some(destination as i64)
        }
        _ => None,
    }
}

pub(super) fn workspace_reorder_window_matches(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> bool {
    let Some(window) = snapshot.windows.first() else {
        return false;
    };
    let reference_matches = string_param(params, &["window_ref"])
        .map(|reference| one_based_ref_index(&reference, "window") == Some(0))
        .unwrap_or(true);
    let id_matches = string_param(params, &["window_id"])
        .map(|id| window.window_id.as_deref() == Some(id.as_str()))
        .unwrap_or(true);
    reference_matches && id_matches
}

pub(super) fn workspace_index_from_selector_keys(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
    ref_keys: &[&str],
    id_keys: &[&str],
) -> Option<usize> {
    if let Some(workspace_ref) = string_param(params, ref_keys) {
        let index = one_based_ref_index(&workspace_ref, "workspace")?;
        if snapshot
            .windows
            .first()
            .is_some_and(|window| index < window.tab_manager.workspaces.len())
        {
            return Some(index);
        }
        return None;
    }

    let workspace_id = string_param(params, id_keys)?;
    snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .iter()
        .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id.as_str()))
}

pub(super) fn workspace_indices_from_params(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<Vec<i64>> {
    let mut indices = Vec::new();

    if let Some(values) = params
        .get("workspace_refs")
        .or_else(|| params.get("refs"))
        .and_then(Value::as_array)
    {
        let parsed: Option<Vec<i64>> = values
            .iter()
            .map(|value| {
                let workspace_ref = value.as_str()?.trim();
                let index = one_based_ref_index(workspace_ref, "workspace")?;
                if !snapshot
                    .windows
                    .first()
                    .is_some_and(|window| index < window.tab_manager.workspaces.len())
                {
                    return None;
                }
                Some(index as i64)
            })
            .collect();
        indices.extend(parsed?);
    }

    if let Some(values) = params
        .get("workspace_ids")
        .or_else(|| params.get("ids"))
        .and_then(Value::as_array)
    {
        let window = snapshot.windows.first()?;
        let parsed: Option<Vec<i64>> = values
            .iter()
            .map(|value| {
                let workspace_id = value.as_str()?.trim();
                window
                    .tab_manager
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.workspace_id.as_deref() == Some(workspace_id))
                    .map(|index| index as i64)
            })
            .collect();
        indices.extend(parsed?);
    }

    if !indices.is_empty() {
        return Some(indices);
    }

    workspace_index_from_params(snapshot, params).map(|index| vec![index as i64])
}

pub(super) fn workspace_index_from_params_or_selected(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    if params.is_empty()
        || !params.contains_key("workspace_id")
            && !params.contains_key("id")
            && !params.contains_key("workspace_ref")
            && !params.contains_key("ref")
    {
        let selected = selected_workspace_index(snapshot);
        if snapshot
            .windows
            .first()
            .is_some_and(|window| selected < window.tab_manager.workspaces.len())
        {
            return Some(selected);
        }
        return None;
    }
    workspace_index_from_params(snapshot, params)
}

pub(super) fn workspace_index_from_workspace_scope_or_selected(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<usize> {
    if let Some(workspace_ref) = string_param(params, &["workspace_ref"]) {
        if let Some(index) = one_based_ref_index(&workspace_ref, "workspace") {
            if snapshot
                .windows
                .first()
                .is_some_and(|window| index < window.tab_manager.workspaces.len())
            {
                return Some(index);
            }
        }
        return None;
    }
    if let Some(workspace_id) = string_param(params, &["workspace_id"]) {
        return snapshot
            .windows
            .first()?
            .tab_manager
            .workspaces
            .iter()
            .position(|workspace| {
                workspace.workspace_id.as_deref() == Some(workspace_id.as_str())
            });
    }
    let selected = selected_workspace_index(snapshot);
    if snapshot
        .windows
        .first()
        .is_some_and(|window| selected < window.tab_manager.workspaces.len())
    {
        return Some(selected);
    }
    None
}

pub(super) fn string_param(
    params: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter().find_map(|key| {
        params
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

pub(super) fn raw_string_param(
    params: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<String> {
    keys.iter()
        .find_map(|key| params.get(*key).and_then(Value::as_str).map(str::to_owned))
}

pub(super) fn string_vec_param(
    params: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<Vec<String>> {
    keys.iter().find_map(|key| {
        let value = params.get(*key)?;
        match value {
            Value::Array(values) => {
                let entries: Vec<String> = values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect();
                Some(entries)
            }
            Value::String(raw) => Some(
                raw.lines()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect(),
            ),
            _ => None,
        }
    })
}

pub(super) fn string_map_param(
    params: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<BTreeMap<String, String>> {
    for key in keys {
        let Some(object) = params.get(*key).and_then(Value::as_object) else {
            continue;
        };
        let map: BTreeMap<String, String> = object
            .iter()
            .filter_map(|(key, value)| {
                let key = key.trim();
                if key.is_empty() {
                    return None;
                }
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|value| (key.to_string(), value.to_string()))
            })
            .collect();
        if !map.is_empty() {
            return Some(map);
        }
    }
    None
}

pub(super) fn first_present_trimmed_string_map_param(
    params: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<BTreeMap<String, String>> {
    keys.iter().find_map(|key| {
        let object = params.get(*key)?.as_object()?;
        Some(
            object
                .iter()
                .filter_map(|(key, value)| {
                    let key = key.trim();
                    (!key.is_empty()).then(|| {
                        value
                            .as_str()
                            .map(|value| (key.to_string(), value.to_string()))
                    })?
                })
                .collect(),
        )
    })
}

pub(super) fn bool_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| {
            value
                .as_bool()
                .or_else(|| value.as_f64().map(|number| number != 0.0))
                .or_else(|| {
                    let normalized = value.as_str()?.trim().to_ascii_lowercase();
                    match normalized.as_str() {
                        "1" | "true" | "yes" | "on" => Some(true),
                        "0" | "false" | "no" | "off" => Some(false),
                        _ => None,
                    }
                })
        })
    })
}

pub(super) fn usize_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<usize> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| match value {
            Value::Number(number) => number
                .as_u64()
                .and_then(|value| usize::try_from(value).ok()),
            Value::String(raw) => raw.trim().parse::<usize>().ok(),
            _ => None,
        })
    })
}

pub(super) fn u32_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<u32> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| match value {
            Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
            Value::String(raw) => raw.trim().parse::<u32>().ok(),
            _ => None,
        })
    })
}

pub(super) fn i64_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| match value {
            Value::Number(number) => number.as_i64(),
            Value::String(raw) => raw.trim().parse::<i64>().ok(),
            _ => None,
        })
    })
}

pub(super) fn optional_u16_param(
    params: &serde_json::Map<String, Value>,
    key: &str,
) -> Option<Option<u16>> {
    let Some(value) = params.get(key) else {
        return Some(None);
    };
    if value.is_null() {
        return Some(None);
    }
    let parsed = match value {
        Value::Number(number) => number.as_u64(),
        Value::String(raw) => raw.trim().parse::<u64>().ok(),
        _ => None,
    }?;
    if (1..=u16::MAX as u64).contains(&parsed) {
        Some(Some(parsed as u16))
    } else {
        None
    }
}

pub(super) fn pull_request_status_param(
    params: &serde_json::Map<String, Value>,
) -> Option<SessionPullRequestStatusSnapshot> {
    let raw = string_param(params, &["status", "state"]).unwrap_or_else(|| "open".to_string());
    match raw.trim().to_ascii_lowercase().as_str() {
        "open" | "opened" => Some(SessionPullRequestStatusSnapshot::Open),
        "merged" | "merge" => Some(SessionPullRequestStatusSnapshot::Merged),
        "closed" | "close" => Some(SessionPullRequestStatusSnapshot::Closed),
        _ => None,
    }
}

pub(super) fn shell_activity_param(
    params: &serde_json::Map<String, Value>,
) -> Option<SessionPanelShellActivityStateSnapshot> {
    let raw = string_param(params, &["state", "shell_state", "shellState", "activity"])?;
    match raw.trim().to_ascii_lowercase().as_str() {
        "prompt" | "idle" | "promptidle" | "prompt_idle" | "prompt-idle" => {
            Some(SessionPanelShellActivityStateSnapshot::PromptIdle)
        }
        "running" | "busy" | "command" | "commandrunning" | "command_running"
        | "command-running" => Some(SessionPanelShellActivityStateSnapshot::CommandRunning),
        "unknown" | "clear" => Some(SessionPanelShellActivityStateSnapshot::Unknown),
        _ => None,
    }
}

pub(super) fn insert_first_param(params: &serde_json::Map<String, Value>) -> bool {
    bool_param(params, &["insert_first", "before"]).unwrap_or(false)
}

pub(super) fn terminal_startup_params(
    params: &serde_json::Map<String, Value>,
) -> (
    Option<String>,
    Option<String>,
    Option<BTreeMap<String, String>>,
) {
    (
        string_param(
            params,
            &["initial_terminal_command", "initialCommand", "command"],
        ),
        string_param(params, &["initial_terminal_input", "initialInput", "input"]),
        string_map_param(
            params,
            &["initial_terminal_environment", "environment", "env"],
        ),
    )
}

pub(super) fn f64_param(params: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        params.get(*key).and_then(|value| {
            value.as_f64().or_else(|| {
                value
                    .as_str()
                    .map(str::trim)
                    .and_then(|value| value.parse::<f64>().ok())
            })
        })
    })
}

pub(super) fn v2_double_param(
    params: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Option<f64> {
    keys.iter().find_map(|key| {
        let value = params.get(*key)?;
        value
            .as_bool()
            .map(|value| if value { 1.0 } else { 0.0 })
            .or_else(|| value.as_f64())
            .or_else(|| value.as_str().and_then(|value| value.parse::<f64>().ok()))
            .filter(|value| value.is_finite())
    })
}

pub(super) fn initial_divider_position_param(
    params: &serde_json::Map<String, Value>,
) -> Result<Option<f64>, ()> {
    match params.get("initial_divider_position") {
        None | Some(Value::Null) => Ok(None),
        Some(_) => v2_double_param(params, &["initial_divider_position"])
            .map(|value| Some(value.clamp(0.1, 0.9)))
            .ok_or(()),
    }
}

pub(super) fn ports_param(params: &serde_json::Map<String, Value>) -> Option<Vec<u16>> {
    let value = params
        .get("ports")
        .or_else(|| params.get("listening_ports"))
        .or_else(|| params.get("listeningPorts"))
        .or_else(|| params.get("port"))?;
    let raw_ports: Vec<i64> = if let Some(values) = value.as_array() {
        values
            .iter()
            .map(|value| {
                value.as_i64().or_else(|| {
                    value
                        .as_str()
                        .map(str::trim)
                        .and_then(|raw| raw.parse::<i64>().ok())
                })
            })
            .collect::<Option<Vec<_>>>()?
    } else if let Some(port) = value.as_i64() {
        vec![port]
    } else {
        value
            .as_str()?
            .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
            .filter(|part| !part.trim().is_empty())
            .map(|part| part.trim().parse::<i64>().ok())
            .collect::<Option<Vec<_>>>()?
    };
    let mut ports = Vec::new();
    for port in raw_ports {
        if !(1..=65535).contains(&port) {
            return None;
        }
        let port = port as u16;
        if !ports.contains(&port) {
            ports.push(port);
        }
    }
    ports.sort_unstable();
    Some(ports)
}

pub(super) fn one_based_ref_index(value: &str, prefix: &str) -> Option<usize> {
    let (actual_prefix, raw_index) = value.trim().split_once(':')?;
    if actual_prefix != prefix {
        return None;
    }
    let index = raw_index.trim().parse::<usize>().ok()?;
    index.checked_sub(1)
}

pub(super) fn split_orientation_from_params(
    params: &serde_json::Map<String, Value>,
) -> Option<SessionSplitOrientation> {
    match string_param(params, &["orientation", "direction"])
        .unwrap_or_else(|| "horizontal".to_string())
        .to_ascii_lowercase()
        .as_str()
    {
        "horizontal" | "h" | "right" | "left" | "r" | "l" => {
            Some(SessionSplitOrientation::Horizontal)
        }
        "vertical" | "v" | "down" | "up" | "d" | "u" => Some(SessionSplitOrientation::Vertical),
        _ => None,
    }
}

pub(super) fn surface_kind_from_params(params: &serde_json::Map<String, Value>) -> Option<String> {
    let Some(kind) = string_param(params, &["type", "kind"]) else {
        return Some("invalid".to_string());
    };
    match kind.to_ascii_lowercase().as_str() {
        "terminal" | "shell" => None,
        "agent" | "browser" | "markdown" | "file" | "diff" | "custom-sidebar" => {
            Some(kind.to_ascii_lowercase())
        }
        _ => Some("invalid".to_string()),
    }
}

pub(super) fn surface_id_from_params_or_focused(
    snapshot: &AppSessionSnapshot,
    params: &serde_json::Map<String, Value>,
) -> Option<String> {
    let workspace_index = workspace_index_from_workspace_scope_or_selected(snapshot, params)?;
    surface_id_from_params_or_workspace_focused(snapshot, workspace_index, params)
}

pub(super) fn surface_id_from_params_or_workspace_focused(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    params: &serde_json::Map<String, Value>,
) -> Option<String> {
    if params.contains_key("index") {
        return None;
    }

    let workspace = snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .get(workspace_index)?;
    let surfaces = surfaces_for_workspace(workspace);

    if let Some(surface_ref) = string_param(params, &["surface_ref", "ref"]) {
        if let Some(index) = one_based_ref_index(&surface_ref, "surface") {
            return surfaces
                .get(index)
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }

    if let Some(surface_id) = string_param(params, &["surface_id", "panel_id", "id"]) {
        if surfaces.iter().any(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == surface_id)
        }) {
            return Some(surface_id);
        }
        return None;
    }

    surfaces
        .iter()
        .find(|surface| {
            surface
                .get("focused")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .or_else(|| surfaces.first())
        .and_then(|surface| surface.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn surface_ref_for_panel(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> Option<String> {
    let workspace = snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .get(workspace_index)?;
    surfaces_for_workspace(workspace)
        .iter()
        .position(|surface| {
            surface
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| id == panel_id)
        })
        .map(surface_ref)
}

pub(super) fn surface_is_terminal(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
    else {
        return false;
    };
    surfaces_for_workspace(workspace).iter().any(|surface| {
        surface
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == panel_id)
            && surface
                .get("type")
                .and_then(Value::as_str)
                .is_none_or(|surface_type| surface_type == "terminal")
    })
}

pub(super) fn surface_is_browser(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> bool {
    let Some(workspace) = snapshot
        .windows
        .first()
        .and_then(|window| window.tab_manager.workspaces.get(workspace_index))
    else {
        return false;
    };
    surfaces_for_workspace(workspace).iter().any(|surface| {
        surface
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == panel_id)
            && surface
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|surface_type| surface_type == "browser")
    })
}

pub(super) fn terminal_key_sequence(key: &str) -> Option<&'static str> {
    let normalized = key.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "enter" | "return" => Some("\r"),
        "tab" => Some("\t"),
        "escape" | "esc" => Some("\x1b"),
        "backspace" | "bs" => Some("\x7f"),
        "delete" | "del" => Some("\x1b[3~"),
        "up" | "arrow-up" | "arrowup" => Some("\x1b[A"),
        "down" | "arrow-down" | "arrowdown" => Some("\x1b[B"),
        "right" | "arrow-right" | "arrowright" => Some("\x1b[C"),
        "left" | "arrow-left" | "arrowleft" => Some("\x1b[D"),
        "home" => Some("\x1b[H"),
        "end" => Some("\x1b[F"),
        "pageup" | "page-up" => Some("\x1b[5~"),
        "pagedown" | "page-down" => Some("\x1b[6~"),
        "ctrl-c" | "ctrl+c" | "sigint" => Some("\x03"),
        "ctrl-d" | "ctrl+d" | "eof" => Some("\x04"),
        "ctrl-l" | "ctrl+l" => Some("\x0c"),
        "ctrl-z" | "ctrl+z" => Some("\x1a"),
        _ => None,
    }
}

pub(super) fn workspace_summary(
    workspace: &SessionWorkspaceSnapshot,
    index: usize,
    selected: bool,
) -> Value {
    json!({
        "id": workspace.workspace_id,
        "ref": workspace_ref(index),
        "title": workspace_display_name(workspace),
        "custom_title": workspace.custom_title,
        "has_custom_title": workspace.custom_title.as_ref().is_some_and(|title| !title.trim().is_empty()),
        "description": workspace.custom_description,
        "selected": selected,
        "pinned": workspace.is_pinned.unwrap_or(false),
        "listening_ports": workspace_listening_ports(workspace),
        "agent_listening_ports": workspace.agent_listening_ports.clone().unwrap_or_default(),
        "agent_pids": workspace.agent_pids.clone().unwrap_or_default(),
        "panel_ttys": workspace.panel_ttys.clone().unwrap_or_default(),
        "panel_shell_activity": workspace.panel_shell_activity.clone().unwrap_or_default(),
        "remote": workspace_remote_payload(workspace),
        "current_directory": workspace.current_directory,
        "initial_terminal_command": workspace.initial_terminal_command,
        "initial_terminal_input": workspace.initial_terminal_input,
        "initial_terminal_environment": workspace.initial_terminal_environment,
        "zoomed_panel_id": workspace.zoomed_panel_id,
        "restorable_agent_panels": restorable_agent_panel_summaries(workspace),
        "custom_color": workspace.custom_color,
        "group_id": workspace.group_id,
        "git_branch": workspace.git_branch,
        "panel_git_branches": workspace.panel_git_branches,
        "panel_pull_requests": workspace.panel_pull_requests,
        "sidebar_progress": workspace.sidebar_progress,
        "sidebar_status_entries": workspace.sidebar_status_entries,
        "sidebar_metadata_entries": workspace.sidebar_metadata_entries,
        "sidebar_metadata_blocks": workspace.sidebar_metadata_blocks,
        "sidebar_log_entries": workspace.sidebar_log_entries,
        "latest_conversation_message": Value::Null,
        "latest_submitted_message": Value::Null,
        "latest_submitted_at": Value::Null,
    })
}

pub(super) fn canonical_workspace_summary(
    workspace: &SessionWorkspaceSnapshot,
    index: usize,
    selected: bool,
) -> Value {
    json!({
        "id": workspace.workspace_id,
        "ref": workspace_ref(index),
        "index": index,
        "title": workspace_display_name(workspace),
        "custom_title": workspace.custom_title,
        "has_custom_title": workspace.custom_title.as_ref().is_some_and(|title| !title.trim().is_empty()),
        "description": workspace.custom_description,
        "selected": selected,
        "pinned": workspace.is_pinned.unwrap_or(false),
        "listening_ports": workspace_listening_ports(workspace),
        "remote": workspace_remote_payload(workspace),
        "current_directory": workspace.current_directory,
        "custom_color": workspace.custom_color,
        "latest_conversation_message": Value::Null,
        "latest_submitted_message": Value::Null,
        "latest_submitted_at": Value::Null,
    })
}

pub(super) fn workspace_remote_payload(workspace: &SessionWorkspaceSnapshot) -> Value {
    workspace.remote.as_ref().map_or_else(
        || {
            json!({
                "enabled": false,
                "state": "disconnected",
                "connected": false,
                "active_terminal_sessions": 0,
                "daemon": {
                    "state": "unavailable",
                    "capabilities": [],
                },
                "detected_ports": [],
                "forwarded_ports": [],
                "conflicted_ports": [],
                "detail": Value::Null,
                "transport": Value::Null,
                "destination": Value::Null,
                "port": Value::Null,
                "local_proxy_port": Value::Null,
                "persistent_daemon_slot": Value::Null,
                "proxy": {
                    "state": "unavailable",
                    "host": Value::Null,
                    "port": Value::Null,
                    "schemes": ["socks5", "http_connect"],
                    "url": Value::Null,
                    "error_code": Value::Null,
                },
            })
        },
        |remote| json!(remote),
    )
}

pub(super) fn restorable_agent_panel_summaries(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    workspace
        .restorable_agent_snapshots
        .as_ref()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    json!({
                        "panel_id": entry.panel_id,
                        "kind": entry.snapshot.kind,
                        "session_id": entry.snapshot.session_id,
                        "working_directory": entry.snapshot.working_directory,
                        "resume_command": entry.snapshot.resume_command,
                        "fork_command": entry.snapshot.fork_command,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn workspace_group_summaries(
    workspaces: &[SessionWorkspaceSnapshot],
    groups: &Option<Vec<cmux_core::session::SessionWorkspaceGroupSnapshot>>,
) -> Vec<Value> {
    let Some(groups) = groups.as_ref() else {
        return Vec::new();
    };
    groups
        .iter()
        .map(|group| {
            let members: Vec<_> = workspaces
                .iter()
                .enumerate()
                .filter(|(_index, workspace)| {
                    workspace.group_id.as_deref() == Some(group.id.as_str())
                })
                .map(|(index, workspace)| {
                    json!({
                        "workspace_id": workspace.workspace_id,
                        "workspace_ref": workspace_ref(index),
                    })
                })
                .collect();
            json!({
                "id": group.id.clone(),
                "name": group.name.clone(),
                "collapsed": group.is_collapsed,
                "pinned": group.is_pinned.unwrap_or(false),
                "anchor_workspace_id": group.anchor_workspace_id.clone(),
                "anchor_member_index": group.anchor_member_index,
                "custom_color": group.custom_color.clone(),
                "icon_symbol": group.icon_symbol.clone(),
                "members": members,
            })
        })
        .collect()
}

pub(super) fn workspace_display_name(workspace: &SessionWorkspaceSnapshot) -> String {
    workspace
        .custom_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .or_else(|| {
            workspace
                .process_title
                .trim()
                .is_empty()
                .then_some("Workspace")
                .or(Some(workspace.process_title.as_str()))
        })
        .unwrap_or("Workspace")
        .to_string()
}

pub(super) fn surfaces_for_workspace(workspace: &SessionWorkspaceSnapshot) -> Vec<Value> {
    let mut rows = Vec::new();
    if let Some(layout) = workspace.layout.as_ref() {
        collect_surfaces(layout, workspace, &mut rows);
    }
    rows
}

pub(super) fn surface_kind_label(kind: &SessionSurfaceKindSnapshot) -> &'static str {
    match kind {
        SessionSurfaceKindSnapshot::Terminal => "terminal",
        SessionSurfaceKindSnapshot::Browser { .. } => "browser",
        SessionSurfaceKindSnapshot::AgentSession { .. } => "agent-session",
        SessionSurfaceKindSnapshot::Markdown { .. } => "markdown",
        SessionSurfaceKindSnapshot::File { .. } => "file",
        SessionSurfaceKindSnapshot::Diff { .. } => "diff",
        SessionSurfaceKindSnapshot::ProjectSidebar => "project-sidebar",
        SessionSurfaceKindSnapshot::RightSidebarTool => "right-sidebar-tool",
        SessionSurfaceKindSnapshot::RemoteTerminal { .. } => "remote-terminal",
    }
}

pub(super) fn collect_surfaces(
    layout: &SessionWorkspaceLayoutSnapshot,
    workspace: &SessionWorkspaceSnapshot,
    rows: &mut Vec<Value>,
) {
    match layout {
        SessionWorkspaceLayoutSnapshot::Pane(pane) => collect_pane_surfaces(pane, workspace, rows),
        SessionWorkspaceLayoutSnapshot::Split(split) => {
            collect_surfaces(&split.first, workspace, rows);
            collect_surfaces(&split.second, workspace, rows);
        }
    }
}

pub(super) fn collect_pane_surfaces(
    pane: &SessionPaneLayoutSnapshot,
    workspace: &SessionWorkspaceSnapshot,
    rows: &mut Vec<Value>,
) {
    for (index_in_pane, panel_id) in pane.panel_ids.iter().enumerate() {
        let index = rows.len();
        let selected = pane
            .selected_panel_id
            .as_deref()
            .map(|selected| selected == panel_id)
            .unwrap_or(index_in_pane == 0);
        let focused = workspace.focused_panel_id.as_deref() == Some(panel_id.as_str());
        let record = workspace
            .surfaces
            .as_ref()
            .and_then(|records| records.iter().find(|record| record.surface_id == *panel_id));
        let surface_type = record
            .map(|record| surface_kind_label(&record.kind))
            .unwrap_or_else(|| pane.surface_kind.as_deref().unwrap_or("terminal"));
        let (browser_back_history, browser_forward_history) =
            match record.map(|record| &record.kind) {
                Some(SessionSurfaceKindSnapshot::Browser {
                    back_history,
                    forward_history,
                    ..
                }) => (back_history.as_deref(), forward_history.as_deref()),
                _ => (
                    pane.browser_back_history.as_deref(),
                    pane.browser_forward_history.as_deref(),
                ),
            };
        let browser_back_count = browser_back_history.map(<[String]>::len).unwrap_or(0);
        let browser_forward_count = browser_forward_history.map(<[String]>::len).unwrap_or(0);
        let browser_availability = cmux_core::session_ops::browser_navigation_availability(
            browser_back_history,
            browser_forward_history,
        );
        let terminal_startup = panel_terminal_startup(&workspace.panel_terminal_startups, panel_id);
        let restorable_agent =
            panel_restorable_agent(&workspace.restorable_agent_snapshots, panel_id);
        let authoritative_resume = record
            .and_then(|record| record.terminal_startup.as_ref())
            .and_then(|startup| startup.resume_binding.as_deref());
        let (initial_command, initial_input, initial_environment) =
            match record.and_then(|record| record.terminal_startup.as_ref()) {
                Some(startup) => (
                    startup.command.clone(),
                    startup.initial_input.clone(),
                    startup.environment.clone(),
                ),
                None => match terminal_startup {
                    Some(startup) => (
                        startup.initial_terminal_command.clone(),
                        startup.initial_terminal_input.clone(),
                        startup.initial_terminal_environment.clone(),
                    ),
                    None => (
                        workspace.initial_terminal_command.clone(),
                        workspace.initial_terminal_input.clone(),
                        workspace.initial_terminal_environment.clone(),
                    ),
                },
            };
        let authoritative_title = record.and_then(|record| record.metadata.custom_title.clone());
        let (markdown_file_path, file_path, diff_viewer_token, diff_viewer_request_path) =
            match record.map(|record| &record.kind) {
                Some(SessionSurfaceKindSnapshot::Markdown { path }) => {
                    (path.clone(), None, None, None)
                }
                Some(SessionSurfaceKindSnapshot::File { path }) => (None, path.clone(), None, None),
                Some(SessionSurfaceKindSnapshot::Diff {
                    token,
                    request_path,
                }) => (None, None, token.clone(), request_path.clone()),
                _ => (
                    pane.markdown_file_path.clone(),
                    pane.file_path.clone(),
                    pane.diff_viewer_token.clone(),
                    pane.diff_viewer_request_path.clone(),
                ),
            };
        let (
            browser_url,
            browser_proxy_url,
            browser_omnibar_visible,
            browser_focus_mode_active,
            browser_developer_tools_visible,
            browser_developer_tools_panel,
            browser_page_zoom,
        ) = match record.map(|record| &record.kind) {
            Some(SessionSurfaceKindSnapshot::Browser {
                url,
                proxy_url,
                omnibar_visible,
                focus_mode_active,
                developer_tools_visible,
                developer_tools_panel,
                page_zoom,
                ..
            }) => (
                url.clone(),
                proxy_url.clone(),
                omnibar_visible.unwrap_or(true),
                focus_mode_active.unwrap_or(false),
                developer_tools_visible.unwrap_or(false),
                developer_tools_panel.clone(),
                *page_zoom,
            ),
            _ => (
                pane.browser_url.clone(),
                pane.browser_proxy_url.clone(),
                pane.browser_omnibar_visible.unwrap_or(true),
                pane.browser_focus_mode_active.unwrap_or(false),
                pane.browser_developer_tools_visible.unwrap_or(false),
                pane.browser_developer_tools_panel.clone(),
                pane.browser_page_zoom,
            ),
        };
        rows.push(json!({
            "id": panel_id,
            "ref": surface_ref(index),
            "type": surface_type,
            "title": authoritative_title.clone().or_else(|| panel_title(&workspace.panel_titles, panel_id)).unwrap_or_else(|| surface_type.to_string()),
            "focused": focused,
            "pane_id": pane.pane_id,
            "pane_ref": pane.pane_id.as_ref().map(|_| format!("pane:{}", index + 1)),
            "selected_in_pane": selected,
            "custom_title": authoritative_title.or_else(|| panel_title(&workspace.panel_titles, panel_id)),
            "pinned": record.map(|record| record.metadata.pinned).unwrap_or_else(|| panel_pinned(&workspace.panel_pins, panel_id)),
            "unread": record.map(|record| record.metadata.unread).unwrap_or_else(|| panel_unread(&workspace.panel_unreads, panel_id)),
            "requested_working_directory": record.and_then(|record| record.terminal_startup.as_ref()).and_then(|startup| startup.working_directory.clone()).or_else(|| record.and_then(|record| record.metadata.reported_directory.clone())).or_else(|| workspace.current_directory.clone()),
            "initial_command": initial_command,
            "initial_input": initial_input,
            "initial_environment": initial_environment,
            "listening_ports": panel_listening_ports(workspace, panel_id),
            "tty": panel_tty(workspace, panel_id),
            "tty_name": panel_tty(workspace, panel_id),
            "shell_activity": panel_shell_activity(workspace, panel_id),
            "shell_activity_state": panel_shell_activity(workspace, panel_id),
            "tmux_start_command": record.and_then(|record| record.terminal_startup.as_ref()).and_then(|startup| startup.tmux_start_command.clone()),
            "resume_binding": authoritative_resume.or(restorable_agent).map(restorable_agent_binding_payload),
            "markdown_file_path": markdown_file_path,
            "file_path": file_path,
            "diff_viewer_token": diff_viewer_token,
            "diff_viewer_request_path": diff_viewer_request_path,
            "browser_url": browser_url,
            "browser_proxy_url": browser_proxy_url,
            "browser_can_go_back": browser_availability.can_go_back,
            "browser_can_go_forward": browser_availability.can_go_forward,
            "browser_back_history_count": browser_back_count,
            "browser_forward_history_count": browser_forward_count,
            "browser_omnibar_visible": browser_omnibar_visible,
            "browser_focus_mode_active": browser_focus_mode_active,
            "browser_developer_tools_visible": browser_developer_tools_visible,
            "browser_developer_tools_panel": browser_developer_tools_panel,
            "browser_page_zoom": browser_page_zoom,
        }));
    }
}

pub(super) fn workspace_listening_ports(workspace: &SessionWorkspaceSnapshot) -> Vec<u16> {
    let mut ports: Vec<u16> = workspace
        .listening_ports
        .as_ref()
        .into_iter()
        .flat_map(|ports| ports.iter().copied())
        .chain(
            workspace
                .panel_listening_ports
                .as_ref()
                .into_iter()
                .flat_map(|entries| entries.iter())
                .flat_map(|entry| entry.ports.iter().copied()),
        )
        .chain(
            workspace
                .agent_listening_ports
                .as_ref()
                .into_iter()
                .flat_map(|ports| ports.iter().copied()),
        )
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

pub(super) fn panel_listening_ports(
    workspace: &SessionWorkspaceSnapshot,
    panel_id: &str,
) -> Vec<u16> {
    workspace
        .panel_listening_ports
        .as_ref()
        .and_then(|entries| entries.iter().find(|entry| entry.panel_id == panel_id))
        .map(|entry| {
            let mut ports = entry.ports.clone();
            ports.sort_unstable();
            ports.dedup();
            ports
        })
        .unwrap_or_default()
}

pub(super) fn panel_tty(workspace: &SessionWorkspaceSnapshot, panel_id: &str) -> Option<String> {
    workspace.panel_ttys.as_ref().and_then(|entries| {
        entries
            .iter()
            .find(|entry| entry.panel_id == panel_id)
            .map(|entry| entry.tty.clone())
    })
}

pub(super) fn panel_shell_activity(
    workspace: &SessionWorkspaceSnapshot,
    panel_id: &str,
) -> Option<String> {
    workspace.panel_shell_activity.as_ref().and_then(|entries| {
        entries
            .iter()
            .find(|entry| entry.panel_id == panel_id)
            .map(|entry| match entry.state {
                SessionPanelShellActivityStateSnapshot::Unknown => "unknown",
                SessionPanelShellActivityStateSnapshot::PromptIdle => "promptIdle",
                SessionPanelShellActivityStateSnapshot::CommandRunning => "commandRunning",
            })
            .map(str::to_string)
    })
}

pub(super) fn panel_title(
    panel_titles: &Option<Vec<cmux_core::session::SessionPanelTitleSnapshot>>,
    panel_id: &str,
) -> Option<String> {
    panel_titles.as_ref()?.iter().find_map(|entry| {
        (entry.panel_id == panel_id)
            .then(|| entry.custom_title.clone())
            .flatten()
    })
}

pub(super) fn panel_pinned(
    panel_pins: &Option<Vec<cmux_core::session::SessionPanelPinSnapshot>>,
    panel_id: &str,
) -> bool {
    panel_pins
        .as_ref()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.panel_id == panel_id)
                .map(|entry| entry.is_pinned)
        })
        .unwrap_or(false)
}

pub(super) fn panel_unread(
    panel_unreads: &Option<Vec<cmux_core::session::SessionPanelUnreadSnapshot>>,
    panel_id: &str,
) -> bool {
    panel_unreads
        .as_ref()
        .and_then(|entries| {
            entries
                .iter()
                .find(|entry| entry.panel_id == panel_id)
                .map(|entry| entry.is_unread)
        })
        .unwrap_or(false)
}

pub(super) fn panel_terminal_startup<'a>(
    panel_terminal_startups: &'a Option<
        Vec<cmux_core::session::SessionPanelTerminalStartupSnapshot>,
    >,
    panel_id: &str,
) -> Option<&'a cmux_core::session::SessionPanelTerminalStartupSnapshot> {
    panel_terminal_startups
        .as_ref()?
        .iter()
        .find(|entry| entry.panel_id == panel_id)
}

pub(super) fn panel_restorable_agent<'a>(
    restorable_agent_snapshots: &'a Option<
        Vec<cmux_core::session::SessionPanelRestorableAgentSnapshot>,
    >,
    panel_id: &str,
) -> Option<&'a cmux_core::session::SessionRestorableAgentSnapshot> {
    restorable_agent_snapshots
        .as_ref()?
        .iter()
        .find(|entry| entry.panel_id == panel_id)
        .map(|entry| &entry.snapshot)
}

pub(super) fn restorable_agent_binding_payload(
    snapshot: &cmux_core::session::SessionRestorableAgentSnapshot,
) -> Value {
    json!({
        "kind": snapshot.kind,
        "session_id": snapshot.session_id,
        "working_directory": snapshot.working_directory,
        "launch_command": snapshot.launch_command,
        "resume_command": snapshot.resume_command,
        "fork_command": snapshot.fork_command,
    })
}

pub(super) fn new_browser_surface_id(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    before_surface_ids: &[String],
) -> Option<String> {
    let workspace = snapshot
        .windows
        .first()?
        .tab_manager
        .workspaces
        .get(workspace_index)?;
    surfaces_for_workspace(workspace)
        .iter()
        .find(|surface| {
            surface
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|surface_type| surface_type == "browser")
                && surface
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| !before_surface_ids.iter().any(|before| before == id))
        })
        .and_then(|surface| surface.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            surfaces_for_workspace(workspace)
                .iter()
                .find(|surface| {
                    surface
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|surface_type| surface_type == "browser")
                        && surface
                            .get("focused")
                            .and_then(Value::as_bool)
                            .unwrap_or(false)
                })
                .and_then(|surface| surface.get("id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
}

pub(super) fn browser_surface_payload(
    snapshot: &AppSessionSnapshot,
    workspace_index: usize,
    panel_id: &str,
) -> Option<Value> {
    let window = snapshot.windows.first()?;
    let workspace = window.tab_manager.workspaces.get(workspace_index)?;
    let surfaces = surfaces_for_workspace(workspace);
    let surface_index = surfaces.iter().position(|surface| {
        surface
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id == panel_id)
    })?;
    let surface = surfaces.get(surface_index)?.clone();
    let url = surface
        .get("browser_url")
        .cloned()
        .filter(|value| !value.is_null())
        .unwrap_or_else(|| json!("about:blank"));
    Some(json!({
        "id": panel_id,
        "surface_id": panel_id,
        "panel_id": panel_id,
        "surface_ref": surface_ref(surface_index),
        "workspace_id": workspace.workspace_id,
        "workspace_ref": workspace_ref(workspace_index),
        "window_id": window.window_id,
        "window_ref": window.window_id.as_ref().map(|_| "window:1"),
        "url": url,
        "browser_url": surface.get("browser_url").cloned().unwrap_or(Value::Null),
        "surface": surface,
    }))
}

pub(super) fn workspace_ref(index: usize) -> String {
    format!("workspace:{}", index + 1)
}

pub(super) fn surface_ref(index: usize) -> String {
    format!("surface:{}", index + 1)
}

pub(super) fn pane_ref(index: usize) -> String {
    format!("pane:{}", index + 1)
}
