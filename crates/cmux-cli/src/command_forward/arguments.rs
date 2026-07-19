fn parse_frozen_option(args: &[String], name: &str) -> (Option<String>, Vec<String>) {
    let mut remaining = Vec::new();
    let mut value = None;
    let mut skip_next = false;
    let mut past_terminator = false;
    let prefix = format!("{name}=");
    for (index, argument) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
        } else if argument == "--" {
            past_terminator = true;
            remaining.push(argument.clone());
        } else if !past_terminator && argument.starts_with(&prefix) {
            value = Some(argument[prefix.len()..].to_string());
        } else if !past_terminator && argument == name && index + 1 < args.len() {
            value = Some(args[index + 1].clone());
            skip_next = true;
        } else {
            remaining.push(argument.clone());
        }
    }
    (value, remaining)
}

fn normalize_action_name(action: &str) -> String {
    action.to_lowercase().replace('-', "_")
}

fn is_frozen_handle_ref(value: &str) -> bool {
    value.split_once(':').is_some_and(|(kind, index)| {
        matches!(
            kind.to_ascii_lowercase().as_str(),
            "window" | "workspace" | "pane" | "surface"
        ) && index.parse::<i64>().is_ok()
    })
}

fn apply_lifecycle_workspace_value(
    raw: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    let value = raw.trim();
    if value.is_empty() {
        params.remove("resolve_current_workspace");
        params.insert("suppress_ambient_workspace".into(), serde_json::json!(true));
        return Ok(());
    }
    if let Ok(index) = value.parse::<i64>() {
        params.insert("workspace_index".into(), serde_json::json!(index));
    } else if is_frozen_handle_ref(value) {
        params.insert("workspace_ref".into(), serde_json::json!(value));
    } else if uuid::Uuid::parse_str(value).is_ok() {
        params.insert("workspace_id".into(), serde_json::json!(value));
    } else {
        return Err(CliError::new(format!(
            "Invalid workspace handle: {value} (expected UUID, ref like workspace:1, or index)"
        )));
    }
    Ok(())
}

fn lifecycle_scope_values(
    workspace: Option<&str>,
    window: Option<&str>,
) -> Result<serde_json::Map<String, serde_json::Value>, CliError> {
    let mut params = serde_json::Map::new();
    if let Some(workspace) = workspace {
        apply_lifecycle_workspace_value(workspace, &mut params)?;
    }
    apply_lifecycle_window_value(window, &mut params)?;
    if !params.contains_key("workspace_id")
        && !params.contains_key("workspace_ref")
        && !params.contains_key("workspace_index")
        && !params.contains_key("suppress_ambient_workspace")
    {
        params.insert("resolve_current_workspace".into(), serde_json::json!(true));
    }
    Ok(params)
}

fn apply_lifecycle_window_value(
    raw: Option<&str>,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    let Some(raw) = raw else {
        return Ok(());
    };
    let value = raw.trim();
    if value.is_empty() {
        params.insert("suppress_ambient_window".into(), serde_json::json!(true));
        return Ok(());
    }
    if let Ok(index) = value.parse::<i64>() {
        params.insert("window_index".into(), serde_json::json!(index));
    } else if is_frozen_handle_ref(value) {
        params.insert("window_ref".into(), serde_json::json!(value));
    } else if uuid::Uuid::parse_str(value).is_ok() {
        params.insert("window_id".into(), serde_json::json!(value));
    } else {
        return Err(CliError::new(format!(
            "Invalid window handle: {value} (expected UUID, ref like window:1, or index)"
        )));
    }
    Ok(())
}

fn join_then_trim(parts: &[String]) -> Option<String> {
    let joined = parts.join(" ");
    let trimmed = joined.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn parse_optional_bool(value: Option<&str>, name: &str) -> Result<Option<bool>, CliError> {
    value
        .map(|value| match value.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            _ => Err(CliError::new(format!("{name} must be true|false"))),
        })
        .transpose()
}

fn apply_validated_surface_selector(
    raw: &str,
    allow_tab_ref: bool,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        params.insert("suppress_ambient_surface".into(), serde_json::json!(true));
        return Ok(());
    }
    let value = if allow_tab_ref {
        trimmed
            .split_once(':')
            .filter(|(kind, index)| {
                kind.eq_ignore_ascii_case("tab") && index.parse::<i64>().is_ok()
            })
            .map(|(_, index)| format!("surface:{index}"))
            .unwrap_or_else(|| trimmed.to_string())
    } else {
        trimmed.to_string()
    };
    let valid_reference = is_frozen_handle_ref(&value);
    if value.parse::<i64>().is_ok() || valid_reference {
        if let Ok(index) = value.parse::<i64>() {
            params.insert("surface_index".into(), serde_json::json!(index));
        } else {
            params.insert("surface_ref".into(), serde_json::json!(value));
        }
    } else if uuid::Uuid::parse_str(&value).is_ok() {
        params.insert("surface_id".into(), serde_json::json!(value));
    } else {
        return Err(CliError::new(format!(
            "Invalid surface handle: {trimmed} (expected UUID, ref like surface:1, or index)"
        )));
    }
    Ok(())
}

fn native_windows_shell() -> String {
    std::env::var("ComSpec")
        .ok()
        .map(|shell| shell.trim().to_string())
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| r"C:\Windows\System32\cmd.exe".to_string())
}

fn native_shell_wrapper(shell: &str, command: &str) -> String {
    format!(
        "& '{}' /d /s /c '{}'",
        powershell_single_quoted_content(shell),
        powershell_single_quoted_content(command)
    )
}

fn powershell_single_quoted_content(value: &str) -> String {
    value.replace('\'', "''")
}

fn surface_read_text_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if !parsed.positionals.is_empty() {
        return Err(CliError::new(format!(
            "{command_label}: unexpected arguments: {}",
            parsed.positionals.join(" ")
        )));
    }
    if let Some(flag) = parsed
        .flags
        .iter()
        .find(|flag| flag.as_str() != "--scrollback")
    {
        return Err(CliError::new(format!(
            "{command_label}: unexpected argument: {flag}"
        )));
    }

    let mut params = serde_json::Map::new();
    apply_window_scope_selector(&parsed, &mut params);
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if parsed.has_flag("--scrollback") {
        params.insert("scrollback".to_string(), serde_json::json!(true));
    }
    if let Some(lines) = parsed.value(&["--lines"]) {
        let lines = lines
            .parse::<usize>()
            .map_err(|_| CliError::new("--lines must be greater than 0"))?;
        if lines == 0 {
            return Err(CliError::new("--lines must be greater than 0"));
        }
        params.insert("lines".to_string(), serde_json::json!(lines));
        params.insert("scrollback".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

fn surface_tty_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;

    let tty = parsed
        .value(&["--tty", "--tty-name", "--ttyName", "--name"])
        .cloned()
        .or_else(|| {
            if has_surface_selector(&params) {
                return parsed.first_positional().map(str::to_string);
            }
            match parsed.positionals.as_slice() {
                [first, second, ..] if looks_like_surface_selector(first) => {
                    apply_surface_selector_value(first, "panel_id", &mut params);
                    Some(second.clone())
                }
                [first, ..] => Some(first.clone()),
                [] => None,
            }
        })
        .ok_or_else(|| CliError::new("report-tty requires a TTY name"))?;
    params.insert("tty".to_string(), serde_json::json!(tty));
    Ok(serde_json::Value::Object(params))
}

fn looks_like_surface_selector(value: &str) -> bool {
    value.starts_with("surface:") || value.starts_with("surface-")
}

fn surface_shell_state_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;

    let state = parsed
        .value(&["--state", "--shell-state", "--shellState", "--activity"])
        .cloned()
        .or_else(|| {
            if has_surface_selector(&params) {
                return parsed.first_positional().map(str::to_string);
            }
            match parsed.positionals.as_slice() {
                [first, second, ..] if looks_like_surface_selector(first) => {
                    apply_surface_selector_value(first, "panel_id", &mut params);
                    Some(second.clone())
                }
                [first, ..] => Some(first.clone()),
                [] => None,
            }
        })
        .ok_or_else(|| CliError::new("report-shell-state requires a state"))?;
    params.insert("state".to_string(), serde_json::json!(state));
    Ok(serde_json::Value::Object(params))
}

fn surface_split_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(direction) = parsed.value(&["--direction", "--orientation", "--layout"]) {
        params.insert("direction".to_string(), serde_json::json!(direction));
    }
    if parsed.has_flag("--before") {
        params.insert("insert_first".to_string(), serde_json::json!(true));
    }
    apply_terminal_startup(&parsed, &mut params)?;
    Ok(serde_json::Value::Object(params))
}

fn surface_terminal_tab_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    apply_terminal_startup(&parsed, &mut params)?;
    Ok(serde_json::Value::Object(params))
}

fn surface_open_markdown_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    let path = parsed
        .value(&["--path", "--file"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
        .ok_or_else(|| CliError::new("markdown requires a file path"))?;
    params.insert("file_path".to_string(), serde_json::json!(path));
    Ok(serde_json::Value::Object(params))
}

fn surface_title_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    let title = parsed
        .value(&["--title", "--name"])
        .cloned()
        .or_else(|| surface_title_from_positionals(&parsed))
        .ok_or_else(|| CliError::new("surface rename requires a title"))?;
    params.insert("title".to_string(), serde_json::json!(title));
    Ok(serde_json::Value::Object(params))
}

fn surface_bool_params(
    args: &[String],
    key: &str,
    value: bool,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    params.insert(key.to_string(), serde_json::json!(value));
    Ok(serde_json::Value::Object(params))
}

fn surface_open_diff_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(token) = parsed.value(&["--token", "--diff-token"]) {
        params.insert("token".to_string(), serde_json::json!(token));
    }
    if let Some(request_path) = parsed.value(&["--request-path", "--diff-path"]) {
        params.insert("request_path".to_string(), serde_json::json!(request_path));
    }
    Ok(serde_json::Value::Object(params))
}

fn surface_set_type_params(
    args: &[String],
    fixed_type: Option<&str>,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    let surface_type = fixed_type
        .map(str::to_string)
        .or_else(|| parsed.value(&["--type", "--kind"]).cloned())
        .or_else(|| parsed.first_positional().map(str::to_string))
        .ok_or_else(|| CliError::new("surface set-type requires a type"))?;
    params.insert("type".to_string(), serde_json::json!(surface_type));
    Ok(serde_json::Value::Object(params))
}

fn surface_workspace_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn surface_focus_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    if !has_surface_selector(&params) {
        return Err(CliError::new("focus-panel requires --panel"));
    }
    Ok(serde_json::Value::Object(params))
}

fn pane_focus_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    let pane = parsed
        .value(&["--pane"])
        .map(String::as_str)
        .or_else(|| parsed.first_positional())
        .ok_or_else(|| CliError::new("focus-pane requires --pane <id|ref>"))?;
    apply_pane_target_selector(pane, &mut params);
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn surface_send_text_params(
    args: &[String],
    require_panel: bool,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if require_panel && !has_surface_selector(&params) {
        return Err(CliError::new("send-panel requires --panel"));
    }
    let text = parsed.positionals.join(" ");
    if text.is_empty() {
        return Err(CliError::new(if require_panel {
            "send-panel requires text"
        } else {
            "send requires text"
        }));
    }
    params.insert(
        "text".to_string(),
        serde_json::json!(unescape_send_text(&text)),
    );
    Ok(serde_json::Value::Object(params))
}

fn surface_send_key_params(
    args: &[String],
    require_panel: bool,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if require_panel && !has_surface_selector(&params) {
        return Err(CliError::new("send-key-panel requires --panel"));
    }
    let key = parsed
        .positionals
        .first()
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| {
            CliError::new(if require_panel {
                "send-key-panel requires a key"
            } else {
                "send-key requires a key"
            })
        })?;
    params.insert("key".to_string(), serde_json::json!(key));
    Ok(serde_json::Value::Object(params))
}

fn apply_workspace_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if parsed.value(&["--index"]).is_some() {
        return Err(CliError::new(
            "workspace selectors require workspace:N or workspace id; --index is not supported",
        ));
    }
    if let Some(reference) = parsed.value(&["--ref", "--workspace-ref"]) {
        params.insert("workspace_ref".to_string(), serde_json::json!(reference));
        return Ok(());
    }
    if let Some(id) = parsed.value(&["--workspace", "--workspace-id", "--tab", "--id"]) {
        params.insert("workspace_id".to_string(), serde_json::json!(id));
        return Ok(());
    }
    if let Some(positional) = parsed.first_positional() {
        if positional.chars().all(|ch| ch.is_ascii_digit()) {
            params.insert(
                "workspace_ref".to_string(),
                serde_json::json!(format!("workspace:{positional}")),
            );
        } else if positional.starts_with("workspace:") {
            params.insert("workspace_ref".to_string(), serde_json::json!(positional));
        } else {
            params.insert("workspace_id".to_string(), serde_json::json!(positional));
        }
    }
    Ok(())
}

fn apply_workspace_target_selector(
    value: &str,
    key_prefix: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        params.insert(
            format!("{key_prefix}_ref"),
            serde_json::json!(format!("workspace:{value}")),
        );
    } else if value.starts_with("workspace:") {
        params.insert(format!("{key_prefix}_ref"), serde_json::json!(value));
    } else {
        params.insert(format!("{key_prefix}_id"), serde_json::json!(value));
    }
}

fn apply_workspace_scope_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if let Some(reference) = parsed.value(&["--workspace-ref"]) {
        params.insert("workspace_ref".to_string(), serde_json::json!(reference));
        return;
    }
    if let Some(value) = parsed.value(&["--workspace", "--workspace-id", "--tab"]) {
        if value.chars().all(|ch| ch.is_ascii_digit()) {
            params.insert(
                "workspace_ref".to_string(),
                serde_json::json!(format!("workspace:{value}")),
            );
        } else if value.starts_with("workspace:") {
            params.insert("workspace_ref".to_string(), serde_json::json!(value));
        } else {
            params.insert("workspace_id".to_string(), serde_json::json!(value));
        }
    }
}

fn apply_window_scope_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    let Some(value) = parsed.value(&["--window", "--window-id"]) else {
        return;
    };
    apply_window_selector_value(value, params);
}

fn apply_window_selector_value(
    value: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        params.insert(
            "window_ref".to_string(),
            serde_json::json!(format!("window:{value}")),
        );
    } else if value.starts_with("window:") {
        params.insert("window_ref".to_string(), serde_json::json!(value));
    } else {
        params.insert("window_id".to_string(), serde_json::json!(value));
    }
}

fn has_explicit_workspace_selector(parsed: &ParsedArgs) -> bool {
    parsed.value(&["--ref", "--workspace-ref"]).is_some()
        || parsed
            .value(&["--workspace", "--workspace-id", "--tab", "--id"])
            .is_some()
}

fn apply_surface_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if parsed.value(&["--index"]).is_some() {
        return Err(CliError::new(
            "surface selectors require surface:N or surface id; --index is not supported",
        ));
    }
    if let Some(reference) = parsed.value(&["--ref", "--surface-ref"]) {
        params.insert("surface_ref".to_string(), serde_json::json!(reference));
        return Ok(());
    }
    if let Some(value) = parsed.value(&["--surface"]) {
        apply_surface_selector_value(value, "surface_id", params);
        return Ok(());
    }
    if let Some(id) = parsed.value(&["--surface-id"]) {
        params.insert("surface_id".to_string(), serde_json::json!(id));
        return Ok(());
    }
    if let Some(value) = parsed.value(&["--panel"]) {
        apply_surface_selector_value(value, "panel_id", params);
        return Ok(());
    }
    if let Some(id) = parsed.value(&["--panel-id", "--id"]) {
        params.insert("panel_id".to_string(), serde_json::json!(id));
        return Ok(());
    }
    Ok(())
}

fn apply_surface_selector_value(
    value: &str,
    actual_id_key: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        params.insert(
            "surface_ref".to_string(),
            serde_json::json!(format!("surface:{value}")),
        );
    } else if value.starts_with("surface:") {
        params.insert("surface_ref".to_string(), serde_json::json!(value));
    } else {
        params.insert(actual_id_key.to_string(), serde_json::json!(value));
    }
}

fn apply_surface_target_selector(
    value: &str,
    key_prefix: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|character| character.is_ascii_digit()) {
        params.insert(
            format!("{key_prefix}_ref"),
            serde_json::json!(format!("surface:{value}")),
        );
    } else if value.starts_with("surface:") {
        params.insert(format!("{key_prefix}_ref"), serde_json::json!(value));
    } else {
        params.insert(format!("{key_prefix}_id"), serde_json::json!(value));
    }
}

fn apply_pane_target_selector(
    value: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    apply_named_pane_target_selector(value, "pane", params);
}

fn apply_named_pane_target_selector(
    value: &str,
    key_prefix: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.chars().all(|character| character.is_ascii_digit()) {
        params.insert(
            format!("{key_prefix}_ref"),
            serde_json::json!(format!("pane:{value}")),
        );
    } else if value.starts_with("pane:") {
        params.insert(format!("{key_prefix}_ref"), serde_json::json!(value));
    } else {
        params.insert(format!("{key_prefix}_id"), serde_json::json!(value));
    }
}

fn apply_surface_selector_or_positional(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    apply_surface_selector(parsed, params)?;
    if has_surface_selector(params) {
        return Ok(());
    }
    if let Some(value) = parsed.first_positional() {
        apply_surface_selector_value(value, "panel_id", params);
    }
    Ok(())
}

fn has_surface_selector(params: &serde_json::Map<String, serde_json::Value>) -> bool {
    params.contains_key("surface_ref")
        || params.contains_key("surface_id")
        || params.contains_key("panel_id")
}

fn has_surface_selector_from_args(parsed: &ParsedArgs) -> bool {
    parsed.value(&["--ref", "--surface-ref"]).is_some()
        || parsed.value(&["--surface", "--surface-id"]).is_some()
        || parsed.value(&["--panel", "--panel-id", "--id"]).is_some()
}

fn apply_terminal_startup(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if let Some(command) = parsed.value(&["--command"]) {
        params.insert(
            "initial_terminal_command".to_string(),
            serde_json::json!(command),
        );
    }
    if let Some(input) = parsed.value(&["--input"]) {
        params.insert(
            "initial_terminal_input".to_string(),
            serde_json::json!(input),
        );
    }
    if let Some(environment) = terminal_environment_param(parsed)? {
        params.insert(
            "initial_terminal_environment".to_string(),
            serde_json::json!(environment),
        );
    }
    Ok(())
}

fn terminal_environment_param(
    parsed: &ParsedArgs,
) -> Result<Option<BTreeMap<String, String>>, CliError> {
    let mut environment = BTreeMap::new();
    for assignment in parsed.values(&["--env", "--environment"]) {
        let (key, value) = assignment.split_once('=').ok_or_else(|| {
            CliError::new(format!(
                "invalid environment assignment: {assignment} (expected KEY=VALUE)"
            ))
        })?;
        let key = key.trim();
        if key.is_empty() {
            return Err(CliError::new(format!(
                "invalid environment assignment: {assignment} (key cannot be empty)"
            )));
        }
        environment.insert(key.to_string(), value.to_string());
    }
    Ok((!environment.is_empty()).then_some(environment))
}

fn surface_title_from_positionals(parsed: &ParsedArgs) -> Option<String> {
    let title = parsed.positionals.join(" ");
    (!title.trim().is_empty()).then_some(title)
}

fn unescape_send_text(text: &str) -> String {
    let mut output = String::new();
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some('t') => output.push('\t'),
            Some('\\') => output.push('\\'),
            Some(other) => {
                output.push('\\');
                output.push(other);
            }
            None => output.push('\\'),
        }
    }
    output
}

fn looks_like_workspace_selector(value: &str) -> bool {
    value.starts_with("workspace:") || value.chars().all(|ch| ch.is_ascii_digit())
}

#[derive(Debug, Default)]
struct ParsedArgs {
    values: Vec<(String, String)>,
    flags: Vec<String>,
    positionals: Vec<String>,
}

impl ParsedArgs {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        let mut parsed = ParsedArgs::default();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if arg == "--" {
                parsed.positionals.extend(args[index + 1..].iter().cloned());
                break;
            }
            if let Some((key, value)) = arg.split_once('=') {
                if key.starts_with("--") {
                    parsed.values.push((key.to_string(), value.to_string()));
                    index += 1;
                    continue;
                }
            }
            if takes_value(arg) {
                let value = args
                    .get(index + 1)
                    .cloned()
                    .ok_or_else(|| CliError::new(format!("{arg} requires a value")))?;
                parsed.values.push((arg.clone(), value));
                index += 2;
            } else if arg.starts_with("--") {
                parsed.flags.push(arg.clone());
                index += 1;
            } else {
                parsed.positionals.push(arg.clone());
                index += 1;
            }
        }
        Ok(parsed)
    }

    fn value(&self, names: &[&str]) -> Option<&String> {
        self.values
            .iter()
            .rev()
            .find_map(|(name, value)| names.contains(&name.as_str()).then_some(value))
    }

    fn values(&self, names: &[&str]) -> Vec<&String> {
        self.values
            .iter()
            .filter_map(|(name, value)| names.contains(&name.as_str()).then_some(value))
            .collect()
    }

    fn has_flag(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }

    fn first_positional(&self) -> Option<&str> {
        self.positionals.first().map(String::as_str)
    }
}

fn takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "--after"
            | "--after-surface"
            | "--after-workspace"
            | "--action"
            | "--amount"
            | "--before"
            | "--before-surface"
            | "--before-workspace"
            | "--branch"
            | "--command"
            | "--color"
            | "--css"
            | "--cwd"
            | "--direction"
            | "--directory"
            | "--devtools-panel"
            | "--diff-path"
            | "--diff-token"
            | "--dx"
            | "--dy"
            | "--description"
            | "--body"
            | "--env"
            | "--env-file"
            | "--environment"
            | "--file"
            | "--format"
            | "--focus"
            | "--from"
            | "--function"
            | "--group"
            | "--group-id"
            | "--group-placement"
            | "--group-reference"
            | "--hex"
            | "--height"
            | "--href"
            | "--id"
            | "--icon"
            | "--index"
            | "--input"
            | "--key"
            | "--kind"
            | "--label"
            | "--lat"
            | "--latitude"
            | "--layout"
            | "--load-state"
            | "--loadState"
            | "--limit"
            | "--lines"
            | "--level"
            | "--lng"
            | "--lon"
            | "--longitude"
            | "--markdown"
            | "--method"
            | "--message"
            | "--name"
            | "--number"
            | "--orientation"
            | "--order"
            | "--out"
            | "--pane"
            | "--panel"
            | "--panel-id"
            | "--path"
            | "--placement"
            | "--pid"
            | "--property"
            | "--process-id"
            | "--processId"
            | "--priority"
            | "--pr"
            | "--ref"
            | "--request-path"
            | "--scale"
            | "--script"
            | "--selector"
            | "--state"
            | "--subtitle"
            | "--surface"
            | "--surface-id"
            | "--surface-ref"
            | "--symbol"
            | "--status"
            | "--title"
            | "--timeout"
            | "--timeout-ms"
            | "--token"
            | "--tool-panel"
            | "--tty"
            | "--tty-name"
            | "--ttyName"
            | "--text"
            | "--text-contains"
            | "--to"
            | "--to-index"
            | "--type"
            | "--target-index"
            | "--url"
            | "--url-contains"
            | "--urlContains"
            | "--since-id"
            | "--sinceId"
            | "--after-id"
            | "--afterId"
            | "--workspace"
            | "--workspace-id"
            | "--workspace-ref"
            | "--window"
            | "--window-id"
            | "--width"
            | "--zoom"
            | "--enabled"
            | "--attr"
            | "--attribute"
            | "--expression"
            | "--role"
            | "--target"
            | "--target-pane"
            | "--tab"
            | "--value"
            | "--progress"
    )
}
