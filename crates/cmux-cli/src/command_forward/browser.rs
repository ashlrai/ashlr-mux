use super::{
    apply_surface_selector, apply_surface_selector_or_positional, apply_surface_selector_value,
    apply_workspace_scope_selector, has_surface_selector, has_surface_selector_from_args,
    looks_like_surface_selector, surface_selector_params, takes_value, CliError, ControlCommand,
    ParsedArgs,
};

pub(super) fn browser_subcommand(args: &[String]) -> Result<Option<ControlCommand>, CliError> {
    let Some((subcommand, rest_args)) = split_browser_subcommand(args) else {
        return Ok(Some(ControlCommand::new(
            "surface.open_browser",
            serde_json::json!({}),
        )));
    };
    let rest = rest_args.as_slice();
    let mapped = match subcommand.as_str() {
        "open" => ControlCommand::new("surface.open_browser", surface_open_browser_params(rest)?),
        "navigate" | "goto" => {
            ControlCommand::new("browser.navigate", browser_navigate_params(rest)?)
        }
        "split" => {
            ControlCommand::new("surface.split_browser", surface_split_browser_params(rest)?)
        }
        "open-split" | "open_split" => {
            ControlCommand::new("browser.open_split", surface_split_browser_params(rest)?)
        }
        "split-right" => ControlCommand::new(
            "surface.split_browser",
            surface_split_browser_params_with_direction(rest, "right")?,
        ),
        "split-down" => ControlCommand::new(
            "surface.split_browser",
            surface_split_browser_params_with_direction(rest, "down")?,
        ),
        "new-workspace" | "workspace" => {
            ControlCommand::new("workspace.create_browser", browser_workspace_params(rest)?)
        }
        "reopen-closed" | "reopen-closed-tab" | "reopen" => {
            ControlCommand::new("browser.reopen_closed", serde_json::json!({}))
        }
        "back" => ControlCommand::new("browser.back", surface_selector_params(rest)?),
        "forward" => ControlCommand::new("browser.forward", surface_selector_params(rest)?),
        "clear-history" => {
            ControlCommand::new("browser.clear_history", surface_selector_params(rest)?)
        }
        "toggle-omnibar" | "omnibar" => {
            ControlCommand::new("browser.toggle_omnibar", surface_selector_params(rest)?)
        }
        "toggle-focus-mode" | "focus-mode" => {
            ControlCommand::new("browser.toggle_focus_mode", surface_selector_params(rest)?)
        }
        "toggle-devtools" | "toggle-developer-tools" | "devtools" => ControlCommand::new(
            "browser.toggle_developer_tools",
            surface_selector_params(rest)?,
        ),
        "show-devtools" | "show-developer-tools" => ControlCommand::new(
            "browser.show_developer_tools",
            browser_show_developer_tools_params(rest)?,
        ),
        "network" if rest.first().map(String::as_str) == Some("clear") => ControlCommand::new(
            "browser.network.clear",
            browser_network_clear_params(&rest[1..])?,
        ),
        "network" | "network-requests" | "requests" => {
            let rest = browser_network_requests_rest(&subcommand, rest);
            ControlCommand::new(
                "browser.network.requests",
                browser_network_requests_params(&rest)?,
            )
        }
        "network-clear" | "clear-network" | "clear-requests" => {
            ControlCommand::new("browser.network.clear", browser_network_clear_params(rest)?)
        }
        "reload" => ControlCommand::new("browser.reload", surface_selector_params(rest)?),
        "url" | "url.get" | "get-url" | "current-url" => {
            ControlCommand::new("browser.url.get", surface_selector_params(rest)?)
        }
        "snapshot" => ControlCommand::new("browser.snapshot", browser_surface_only_params(rest)?),
        "eval" => ControlCommand::new("browser.eval", browser_eval_params(rest)?),
        "wait" => ControlCommand::new("browser.wait", browser_wait_params(rest)?),
        "click" => ControlCommand::new("browser.click", browser_selector_action_params(rest)?),
        "dblclick" | "double-click" => {
            ControlCommand::new("browser.dblclick", browser_selector_action_params(rest)?)
        }
        "hover" => ControlCommand::new("browser.hover", browser_selector_action_params(rest)?),
        "focus" => ControlCommand::new("browser.focus", browser_selector_action_params(rest)?),
        "type" => ControlCommand::new("browser.type", browser_text_action_params(rest)?),
        "fill" => ControlCommand::new("browser.fill", browser_text_action_params(rest)?),
        "press" | "key" => ControlCommand::new("browser.press", browser_key_action_params(rest)?),
        "keydown" | "key-down" => {
            ControlCommand::new("browser.keydown", browser_key_action_params(rest)?)
        }
        "keyup" | "key-up" => {
            ControlCommand::new("browser.keyup", browser_key_action_params(rest)?)
        }
        "check" => ControlCommand::new("browser.check", browser_selector_action_params(rest)?),
        "uncheck" => ControlCommand::new("browser.uncheck", browser_selector_action_params(rest)?),
        "select" => ControlCommand::new("browser.select", browser_select_action_params(rest)?),
        "scroll" => ControlCommand::new("browser.scroll", browser_scroll_params(rest)?),
        "scrollintoview" | "scrollinto" | "scroll-into-view" => ControlCommand::new(
            "browser.scroll_into_view",
            browser_selector_action_params(rest)?,
        ),
        "screenshot" => ControlCommand::new("browser.screenshot", browser_screenshot_params(rest)?),
        "get" => browser_get_command(rest)?,
        "is" => browser_is_command(rest)?,
        "find" => browser_find_command(rest)?,
        "frame" => browser_frame_command(rest)?,
        "dialog" => browser_dialog_command(rest)?,
        "download" => browser_download_command(rest)?,
        "cookies" => browser_cookies_command(rest)?,
        "storage" => browser_storage_command(rest)?,
        "tab" => browser_tab_command(rest)?,
        "console" => browser_console_command(rest)?,
        "errors" => browser_errors_command(rest)?,
        "highlight" => {
            ControlCommand::new("browser.highlight", browser_selector_action_params(rest)?)
        }
        "state" => browser_state_command(rest)?,
        "addinitscript" | "add-init-script" => ControlCommand::new(
            "browser.addinitscript",
            browser_script_params(rest, "script")?,
        ),
        "addscript" | "add-script" => {
            ControlCommand::new("browser.addscript", browser_script_params(rest, "script")?)
        }
        "addstyle" | "add-style" => {
            ControlCommand::new("browser.addstyle", browser_script_params(rest, "css")?)
        }
        "focus-webview" => {
            ControlCommand::new("browser.focus_webview", surface_selector_params(rest)?)
        }
        "is-webview-focused" | "webview-focused" | "is-focused" => {
            ControlCommand::new("browser.is_webview_focused", surface_selector_params(rest)?)
        }
        "viewport" | "set-viewport" => {
            ControlCommand::new("browser.viewport.set", browser_viewport_set_params(rest)?)
        }
        "geo" | "geolocation" | "set-geolocation" => ControlCommand::new(
            "browser.geolocation.set",
            browser_geolocation_set_params(rest)?,
        ),
        "offline" | "set-offline" => {
            ControlCommand::new("browser.offline.set", browser_offline_set_params(rest)?)
        }
        "trace" => browser_trace_command(rest)?,
        "screencast" => browser_screencast_command(rest)?,
        "input" => browser_input_command(rest)?,
        "zoom" | "set-zoom" => {
            ControlCommand::new("browser.set_zoom", browser_set_zoom_params(rest)?)
        }
        _ => return Ok(None),
    };
    Ok(Some(mapped))
}

fn split_browser_subcommand(args: &[String]) -> Option<(String, Vec<String>)> {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            return None;
        }
        if arg.starts_with("--") {
            index += if arg.contains('=') || !takes_value(arg) {
                1
            } else {
                2
            };
            continue;
        }
        if !is_browser_subcommand(arg)
            && args
                .get(index + 1)
                .is_some_and(|next| is_browser_subcommand(next))
        {
            let mut rest = Vec::with_capacity(args.len().saturating_sub(1));
            rest.extend(args[..index].iter().cloned());
            rest.push("--surface".to_string());
            rest.push(arg.clone());
            rest.extend(args[index + 2..].iter().cloned());
            return Some((args[index + 1].to_ascii_lowercase(), rest));
        }
        let mut rest = Vec::with_capacity(args.len().saturating_sub(1));
        rest.extend(args[..index].iter().cloned());
        rest.extend(args[index + 1..].iter().cloned());
        return Some((arg.to_ascii_lowercase(), rest));
    }
    None
}

fn is_browser_subcommand(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "open"
            | "navigate"
            | "goto"
            | "split"
            | "open-split"
            | "open_split"
            | "split-right"
            | "split-down"
            | "new-workspace"
            | "workspace"
            | "reopen-closed"
            | "reopen-closed-tab"
            | "reopen"
            | "back"
            | "forward"
            | "clear-history"
            | "toggle-omnibar"
            | "omnibar"
            | "toggle-focus-mode"
            | "focus-mode"
            | "toggle-devtools"
            | "toggle-developer-tools"
            | "devtools"
            | "show-devtools"
            | "show-developer-tools"
            | "network"
            | "network-requests"
            | "requests"
            | "network-clear"
            | "clear-network"
            | "clear-requests"
            | "reload"
            | "url"
            | "url.get"
            | "get-url"
            | "current-url"
            | "snapshot"
            | "eval"
            | "wait"
            | "click"
            | "dblclick"
            | "double-click"
            | "hover"
            | "focus"
            | "type"
            | "fill"
            | "press"
            | "key"
            | "keydown"
            | "key-down"
            | "keyup"
            | "key-up"
            | "check"
            | "uncheck"
            | "select"
            | "scroll"
            | "scrollintoview"
            | "scrollinto"
            | "scroll-into-view"
            | "screenshot"
            | "get"
            | "is"
            | "find"
            | "frame"
            | "dialog"
            | "download"
            | "cookies"
            | "storage"
            | "tab"
            | "console"
            | "errors"
            | "highlight"
            | "state"
            | "addinitscript"
            | "add-init-script"
            | "addscript"
            | "add-script"
            | "addstyle"
            | "add-style"
            | "focus-webview"
            | "is-webview-focused"
            | "webview-focused"
            | "is-focused"
            | "viewport"
            | "set-viewport"
            | "geo"
            | "geolocation"
            | "set-geolocation"
            | "offline"
            | "set-offline"
            | "trace"
            | "screencast"
            | "input"
            | "zoom"
            | "set-zoom"
    )
}

fn browser_network_requests_rest(subcommand: &str, rest: &[String]) -> Vec<String> {
    if subcommand != "network" || rest.first().map(String::as_str) != Some("requests") {
        return rest.to_vec();
    }
    rest[1..].to_vec()
}

pub(super) fn surface_split_browser_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    surface_split_browser_params_inner(args, None)
}

fn surface_split_browser_params_with_direction(
    args: &[String],
    direction: &str,
) -> Result<serde_json::Value, CliError> {
    surface_split_browser_params_inner(args, Some(direction))
}

fn surface_split_browser_params_inner(
    args: &[String],
    default_direction: Option<&str>,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(direction) = parsed
        .value(&["--direction", "--orientation", "--layout"])
        .map(String::as_str)
        .or(default_direction)
    {
        params.insert("direction".to_string(), serde_json::json!(direction));
    }
    if parsed.has_flag("--before") {
        params.insert("insert_first".to_string(), serde_json::json!(true));
    }
    if let Some(url) = parsed
        .value(&["--url"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
    {
        params.insert("url".to_string(), serde_json::json!(url));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn surface_open_browser_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(url) = parsed
        .value(&["--url"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
    {
        params.insert("url".to_string(), serde_json::json!(url));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn browser_workspace_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    if let Some(url) = parsed
        .value(&["--url"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
    {
        params.insert("url".to_string(), serde_json::json!(url));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_show_developer_tools_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_surface_selector(&parsed, &mut params)?;
    if let Some(panel) = parsed.value(&["--devtools-panel", "--tool-panel"]) {
        params.insert("panel".to_string(), serde_json::json!(panel));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_network_requests_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    if let Some(value) = parsed.value(&["--url-contains", "--urlContains", "--url"]) {
        params.insert("url_contains".to_string(), serde_json::json!(value));
    }
    if let Some(value) = parsed.value(&["--method"]) {
        params.insert("method".to_string(), serde_json::json!(value));
    }
    if let Some(value) = parsed.value(&["--since-id", "--sinceId", "--after-id", "--afterId"]) {
        params.insert("since_id".to_string(), serde_json::json!(value));
    }
    if let Some(value) = parsed.value(&["--limit"]) {
        let limit = value.parse::<usize>().map_err(|_| {
            CliError::new(format!(
                "browser network --limit requires a non-negative integer: {value}"
            ))
        })?;
        params.insert("limit".to_string(), serde_json::json!(limit));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_network_clear_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    Ok(serde_json::Value::Object(params))
}

fn browser_navigate_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let url = parsed
        .value(&["--url"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser navigate requires a URL"))?;
    params.insert("url".to_string(), serde_json::json!(url));
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_surface_only_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let params = browser_automation_base_params(&parsed)?;
    Ok(serde_json::Value::Object(params))
}

fn browser_eval_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let script = parsed
        .value(&["--script", "--expression"])
        .cloned()
        .or_else(|| (!payload.is_empty()).then(|| payload.join(" ")))
        .ok_or_else(|| CliError::new("browser eval requires a script"))?;
    params.insert("script".to_string(), serde_json::json!(script));
    Ok(serde_json::Value::Object(params))
}

fn browser_wait_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if let Some(selector) = parsed
        .value(&["--selector"])
        .cloned()
        .or_else(|| payload.first().cloned())
    {
        params.insert("selector".to_string(), serde_json::json!(selector));
    }
    if let Some(text) = parsed.value(&["--text-contains", "--text"]) {
        params.insert("text_contains".to_string(), serde_json::json!(text));
    }
    if let Some(function) = parsed.value(&["--function", "--expression"]) {
        params.insert("function".to_string(), serde_json::json!(function));
    }
    if let Some(load_state) = parsed.value(&["--load-state", "--loadState"]) {
        params.insert("load_state".to_string(), serde_json::json!(load_state));
    }
    if let Some(url_contains) = parsed.value(&["--url-contains", "--urlContains"]) {
        params.insert("url_contains".to_string(), serde_json::json!(url_contains));
    }
    if let Some(timeout) = parsed.value(&["--timeout-ms", "--timeout"]) {
        params.insert("timeout_ms".to_string(), serde_json::json!(timeout));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_selector_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if let Some(selector) = parsed
        .value(&["--selector"])
        .cloned()
        .or_else(|| payload.first().cloned())
    {
        params.insert("selector".to_string(), serde_json::json!(selector));
    }
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_text_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if let Some(selector) = parsed
        .value(&["--selector"])
        .cloned()
        .or_else(|| payload.first().cloned())
    {
        params.insert("selector".to_string(), serde_json::json!(selector));
    }
    if let Some(text) = parsed
        .value(&["--text", "--value"])
        .cloned()
        .or_else(|| payload.get(1).cloned())
    {
        params.insert("text".to_string(), serde_json::json!(text));
    }
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_key_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let key = parsed
        .value(&["--key"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser key command requires a key"))?;
    params.insert("key".to_string(), serde_json::json!(key));
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_select_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let selector = parsed
        .value(&["--selector"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser select requires a selector"))?;
    let value = parsed
        .value(&["--value"])
        .cloned()
        .or_else(|| payload.get(1).cloned())
        .ok_or_else(|| CliError::new("browser select requires a value"))?;
    params.insert("selector".to_string(), serde_json::json!(selector));
    params.insert("value".to_string(), serde_json::json!(value));
    insert_snapshot_after_flag(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn browser_scroll_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_selector_action_params(args)?
        .as_object()
        .cloned()
        .unwrap_or_default();
    if let Some(dx) = parsed.value(&["--dx"]) {
        params.insert("dx".to_string(), serde_json::json!(dx));
    }
    if let Some(dy) = parsed.value(&["--dy"]) {
        params.insert("dy".to_string(), serde_json::json!(dy));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_screenshot_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    if let Some(path) = parsed.value(&["--path", "--out", "--file"]) {
        params.insert("path".to_string(), serde_json::json!(path));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_get_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(
        args,
        &[
            "text", "html", "value", "attr", "url", "title", "count", "box", "styles",
        ],
    ) else {
        return Err(CliError::new(
            "browser get requires text, html, value, attr, url, title, count, box, or styles",
        ));
    };
    if subcommand == "url" {
        return Ok(ControlCommand::new(
            "browser.url.get",
            browser_surface_only_params(&rest)?,
        ));
    }
    let method = format!("browser.get.{subcommand}");
    Ok(ControlCommand::new(
        method,
        browser_get_params(&subcommand, &rest)?,
    ))
}

fn browser_get_params(kind: &str, args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if !matches!(kind, "title") {
        if let Some(selector) = parsed
            .value(&["--selector"])
            .cloned()
            .or_else(|| payload.first().cloned())
        {
            params.insert("selector".to_string(), serde_json::json!(selector));
        }
    }
    if kind == "attr" {
        if let Some(attr) = parsed
            .value(&["--attr", "--attribute", "--name"])
            .cloned()
            .or_else(|| payload.get(1).cloned())
        {
            params.insert("attr".to_string(), serde_json::json!(attr));
        }
    }
    if kind == "styles" {
        if let Some(property) = parsed
            .value(&["--property", "--name"])
            .cloned()
            .or_else(|| payload.get(1).cloned())
        {
            params.insert("property".to_string(), serde_json::json!(property));
        }
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_is_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) =
        split_browser_nested_subcommand(args, &["visible", "enabled", "checked"])
    else {
        return Err(CliError::new(
            "browser is requires visible, enabled, or checked",
        ));
    };
    Ok(ControlCommand::new(
        format!("browser.is.{subcommand}"),
        browser_selector_action_params(&rest)?,
    ))
}

fn browser_find_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(
        args,
        &[
            "role",
            "text",
            "label",
            "placeholder",
            "alt",
            "title",
            "testid",
            "first",
            "last",
            "nth",
        ],
    ) else {
        return Err(CliError::new("browser find requires a locator kind"));
    };
    Ok(ControlCommand::new(
        format!("browser.find.{subcommand}"),
        browser_find_params(&subcommand, &rest)?,
    ))
}

fn browser_find_params(kind: &str, args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    match kind {
        "role" => {
            if let Some(role) = parsed
                .value(&["--role"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("role".to_string(), serde_json::json!(role));
            }
            if let Some(name) = parsed
                .value(&["--name"])
                .cloned()
                .or_else(|| payload.get(1).cloned())
            {
                params.insert("name".to_string(), serde_json::json!(name));
            }
        }
        "text" | "label" | "placeholder" | "alt" | "title" | "testid" => {
            let key = if kind == "testid" { "testid" } else { kind };
            if let Some(value) = parsed
                .value(&[&format!("--{key}")])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert(key.to_string(), serde_json::json!(value));
            }
        }
        "first" | "last" => {
            if let Some(selector) = parsed
                .value(&["--selector"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("selector".to_string(), serde_json::json!(selector));
            }
        }
        "nth" => {
            if let Some(selector) = parsed
                .value(&["--selector"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("selector".to_string(), serde_json::json!(selector));
            }
            if let Some(index) = parsed
                .value(&["--index"])
                .cloned()
                .or_else(|| payload.get(1).cloned())
            {
                params.insert("index".to_string(), serde_json::json!(index));
            }
        }
        _ => {}
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_frame_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if payload
        .first()
        .is_some_and(|value| value.eq_ignore_ascii_case("main"))
    {
        return Ok(ControlCommand::new(
            "browser.frame.main",
            browser_surface_only_params(args)?,
        ));
    }
    Ok(ControlCommand::new(
        "browser.frame.select",
        browser_selector_action_params(args)?,
    ))
}

fn browser_dialog_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["accept", "dismiss"])
    else {
        return Err(CliError::new("browser dialog requires accept or dismiss"));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    if let Some(text) = parsed.value(&["--text"]).cloned().or_else(|| {
        browser_payload_positionals_after_optional_surface(&parsed)
            .first()
            .cloned()
    }) {
        params.insert("text".to_string(), serde_json::json!(text));
    }
    Ok(ControlCommand::new(
        format!("browser.dialog.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_download_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["wait"]) else {
        return Err(CliError::new("browser download requires wait"));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let path = parsed
        .value(&["--path", "--file"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser download wait requires a path"))?;
    params.insert("path".to_string(), serde_json::json!(path));
    if let Some(timeout) = parsed.value(&["--timeout-ms", "--timeout"]) {
        params.insert("timeout_ms".to_string(), serde_json::json!(timeout));
    }
    Ok(ControlCommand::new(
        format!("browser.download.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_cookies_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["get", "set", "clear"])
    else {
        return Ok(ControlCommand::new(
            "browser.cookies.get",
            browser_surface_only_params(args)?,
        ));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    if let Some(name) = parsed
        .value(&["--name"])
        .cloned()
        .or_else(|| payload.first().cloned())
    {
        params.insert("name".to_string(), serde_json::json!(name));
    }
    if let Some(value) = parsed
        .value(&["--value"])
        .cloned()
        .or_else(|| payload.get(1).cloned())
    {
        params.insert("value".to_string(), serde_json::json!(value));
    }
    Ok(ControlCommand::new(
        format!("browser.cookies.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_storage_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let mut cursor = 0;
    let storage_type = payload
        .get(cursor)
        .filter(|value| matches!(value.as_str(), "local" | "session"))
        .cloned()
        .unwrap_or_else(|| "local".to_string());
    if matches!(
        payload.get(cursor).map(String::as_str),
        Some("local" | "session")
    ) {
        cursor += 1;
    }
    let action = payload
        .get(cursor)
        .map(String::as_str)
        .unwrap_or("get")
        .to_ascii_lowercase();
    if matches!(action.as_str(), "get" | "set" | "clear") {
        cursor += 1;
    }
    let mut params = browser_automation_base_params(&parsed)?;
    params.insert("type".to_string(), serde_json::json!(storage_type));
    if let Some(key) = parsed
        .value(&["--key"])
        .cloned()
        .or_else(|| payload.get(cursor).cloned())
    {
        params.insert("key".to_string(), serde_json::json!(key));
    }
    if let Some(value) = parsed
        .value(&["--value"])
        .cloned()
        .or_else(|| payload.get(cursor + 1).cloned())
    {
        params.insert("value".to_string(), serde_json::json!(value));
    }
    Ok(ControlCommand::new(
        format!(
            "browser.storage.{}",
            if matches!(action.as_str(), "get" | "set" | "clear") {
                action
            } else {
                "get".to_string()
            }
        ),
        serde_json::Value::Object(params),
    ))
}

fn browser_tab_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) =
        split_browser_nested_subcommand(args, &["new", "list", "switch", "close"])
    else {
        return Ok(ControlCommand::new(
            "browser.tab.list",
            browser_surface_only_params(args)?,
        ));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    match subcommand.as_str() {
        "new" => {
            if let Some(url) = parsed
                .value(&["--url"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("url".to_string(), serde_json::json!(url));
            }
        }
        "switch" | "close" => {
            if let Some(target) = parsed
                .value(&["--target", "--tab"])
                .cloned()
                .or_else(|| payload.first().cloned())
            {
                params.insert("target_surface_id".to_string(), serde_json::json!(target));
            }
        }
        _ => {}
    }
    Ok(ControlCommand::new(
        format!("browser.tab.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_console_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["list", "clear"]) else {
        return Ok(ControlCommand::new(
            "browser.console.list",
            browser_surface_only_params(args)?,
        ));
    };
    Ok(ControlCommand::new(
        format!("browser.console.{subcommand}"),
        browser_surface_only_params(&rest)?,
    ))
}

fn browser_errors_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["list"]) else {
        return Ok(ControlCommand::new(
            "browser.errors.list",
            browser_surface_only_params(args)?,
        ));
    };
    Ok(ControlCommand::new(
        format!("browser.errors.{subcommand}"),
        browser_surface_only_params(&rest)?,
    ))
}

fn browser_state_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["save", "load"]) else {
        return Err(CliError::new("browser state requires save or load"));
    };
    let parsed = ParsedArgs::parse(&rest)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let path = parsed
        .value(&["--path", "--file"])
        .cloned()
        .or_else(|| payload.first().cloned())
        .ok_or_else(|| CliError::new("browser state requires a path"))?;
    params.insert("path".to_string(), serde_json::json!(path));
    Ok(ControlCommand::new(
        format!("browser.state.{subcommand}"),
        serde_json::Value::Object(params),
    ))
}

fn browser_script_params(args: &[String], key: &str) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = browser_automation_base_params(&parsed)?;
    let payload = browser_payload_positionals_after_optional_surface(&parsed);
    let value = parsed
        .value(&["--script", "--css", "--text"])
        .cloned()
        .or_else(|| (!payload.is_empty()).then(|| payload.join(" ")))
        .ok_or_else(|| CliError::new(format!("browser {key} command requires a payload")))?;
    params.insert(key.to_string(), serde_json::json!(value));
    Ok(serde_json::Value::Object(params))
}

fn browser_automation_base_params(
    parsed: &ParsedArgs,
) -> Result<serde_json::Map<String, serde_json::Value>, CliError> {
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(parsed, &mut params);
    apply_surface_selector(parsed, &mut params)?;
    if !has_surface_selector(&params) {
        if let Some(value) = parsed
            .positionals
            .first()
            .filter(|value| looks_like_surface_selector(value))
        {
            apply_surface_selector_value(value, "panel_id", &mut params);
        }
    }
    Ok(params)
}

fn browser_payload_positionals_after_optional_surface(parsed: &ParsedArgs) -> Vec<String> {
    if has_surface_selector_from_args(parsed) {
        return parsed.positionals.clone();
    }
    let Some(first) = parsed.positionals.first() else {
        return Vec::new();
    };
    if looks_like_surface_selector(first) {
        return parsed.positionals[1..].to_vec();
    }
    parsed.positionals.clone()
}

fn insert_snapshot_after_flag(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if parsed.has_flag("--snapshot-after") {
        params.insert("snapshot_after".to_string(), serde_json::json!(true));
    }
}

fn browser_viewport_set_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    let positionals = browser_payload_positionals_after_surface(&parsed);
    if let Some(width) = parsed
        .value(&["--width"])
        .cloned()
        .or_else(|| positionals.first().cloned())
    {
        params.insert("width".to_string(), serde_json::json!(width));
    }
    if let Some(height) = parsed
        .value(&["--height"])
        .cloned()
        .or_else(|| positionals.get(1).cloned())
    {
        params.insert("height".to_string(), serde_json::json!(height));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_geolocation_set_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    let positionals = browser_payload_positionals_after_surface(&parsed);
    if let Some(latitude) = parsed
        .value(&["--latitude", "--lat"])
        .cloned()
        .or_else(|| positionals.first().cloned())
    {
        params.insert("latitude".to_string(), serde_json::json!(latitude));
    }
    if let Some(longitude) = parsed
        .value(&["--longitude", "--lon", "--lng"])
        .cloned()
        .or_else(|| positionals.get(1).cloned())
    {
        params.insert("longitude".to_string(), serde_json::json!(longitude));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_offline_set_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    let positionals = browser_payload_positionals_after_surface(&parsed);
    if let Some(enabled) = parsed
        .value(&["--enabled"])
        .cloned()
        .or_else(|| positionals.first().cloned())
    {
        params.insert("enabled".to_string(), serde_json::json!(enabled));
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_trace_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["start", "stop"]) else {
        return Err(CliError::new("browser trace requires start or stop"));
    };
    let method = match subcommand.as_str() {
        "start" => "browser.trace.start",
        "stop" => "browser.trace.stop",
        _ => {
            return Err(CliError::new(format!(
                "unknown browser trace command: {subcommand}"
            )))
        }
    };
    Ok(ControlCommand::new(
        method,
        browser_passthrough_params(&rest)?,
    ))
}

fn browser_screencast_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_browser_nested_subcommand(args, &["start", "stop"]) else {
        return Err(CliError::new("browser screencast requires start or stop"));
    };
    let method = match subcommand.as_str() {
        "start" => "browser.screencast.start",
        "stop" => "browser.screencast.stop",
        _ => {
            return Err(CliError::new(format!(
                "unknown browser screencast command: {subcommand}"
            )))
        }
    };
    Ok(ControlCommand::new(
        method,
        browser_passthrough_params(&rest)?,
    ))
}

fn browser_input_command(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) =
        split_browser_nested_subcommand(args, &["mouse", "keyboard", "touch"])
    else {
        return Err(CliError::new(
            "browser input requires mouse, keyboard, or touch",
        ));
    };
    let method = match subcommand.as_str() {
        "mouse" => "browser.input_mouse",
        "keyboard" => "browser.input_keyboard",
        "touch" => "browser.input_touch",
        _ => {
            return Err(CliError::new(format!(
                "unknown browser input command: {subcommand}"
            )))
        }
    };
    let mut params = browser_passthrough_params(&rest)?;
    if let Some(object) = params.as_object_mut() {
        object.insert("args".to_string(), serde_json::json!(rest));
    }
    Ok(ControlCommand::new(method, params))
}

fn split_browser_nested_subcommand(
    args: &[String],
    allowed: &[&str],
) -> Option<(String, Vec<String>)> {
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            return None;
        }
        if arg.starts_with("--") {
            index += if arg.contains('=') || !takes_value(arg) {
                1
            } else {
                2
            };
            continue;
        }
        let lowered = arg.to_ascii_lowercase();
        if allowed.contains(&lowered.as_str()) {
            let mut rest = Vec::with_capacity(args.len().saturating_sub(1));
            rest.extend(args[..index].iter().cloned());
            rest.extend(args[index + 1..].iter().cloned());
            return Some((lowered, rest));
        }
        if args
            .get(index + 1)
            .map(|value| value.to_ascii_lowercase())
            .is_some_and(|next| allowed.contains(&next.as_str()))
        {
            let mut rest = Vec::with_capacity(args.len().saturating_sub(1));
            rest.extend(args[..index].iter().cloned());
            rest.push(arg.clone());
            rest.extend(args[index + 2..].iter().cloned());
            return Some((args[index + 1].to_ascii_lowercase(), rest));
        }
        return None;
    }
    None
}

fn browser_passthrough_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector_or_positional(&parsed, &mut params)?;
    if !parsed.positionals.is_empty() {
        params.insert(
            "args".to_string(),
            serde_json::json!(browser_payload_positionals_after_surface(&parsed)),
        );
    }
    Ok(serde_json::Value::Object(params))
}

fn browser_payload_positionals_after_surface(parsed: &ParsedArgs) -> Vec<String> {
    if has_surface_selector_from_args(parsed) {
        return parsed.positionals.clone();
    }
    let Some(first) = parsed.positionals.first() else {
        return Vec::new();
    };
    if first.starts_with("surface") && parsed.positionals.len() > 1 {
        return parsed.positionals[1..].to_vec();
    }
    parsed.positionals.clone()
}

fn browser_set_zoom_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;
    let zoom = parsed
        .value(&["--zoom", "--scale"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_string))
        .ok_or_else(|| CliError::new("browser zoom requires a zoom value"))?;
    params.insert("zoom".to_string(), serde_json::json!(zoom));
    Ok(serde_json::Value::Object(params))
}
