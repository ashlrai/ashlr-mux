use std::collections::{HashMap, HashSet};

use serde_json::{json, Map, Value};

use crate::CliError;

#[derive(Debug, Clone, Default)]
pub struct TmuxCompatEnvironment<'a> {
    pub workspace_id: Option<&'a str>,
    pub pane_id: Option<&'a str>,
    pub launched_through_omx: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedArguments {
    flags: HashSet<String>,
    options: HashMap<String, Vec<String>>,
}

impl ParsedArguments {
    fn value(&self, flag: &str) -> Option<&str> {
        self.options
            .get(flag)
            .and_then(|values| values.last())
            .map(String::as_str)
    }

    fn has_flag(&self, flag: &str) -> bool {
        self.flags.contains(flag)
    }
}

fn split_command(args: &[String]) -> Result<(&str, &[String]), CliError> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if !arg.starts_with('-') || arg == "-" {
            return Ok((arg, &args[index + 1..]));
        }
        if arg == "--" {
            break;
        }
        if matches!(arg, "-V" | "-v") {
            return Ok((arg, &[]));
        }
        if ["-L", "-S", "-f"]
            .iter()
            .any(|flag| arg == *flag || arg.starts_with(flag))
            && matches!(arg, "-L" | "-S" | "-f")
        {
            index += 1;
        }
        index += 1;
    }
    Err(CliError::new("tmux shim requires a command"))
}

fn parse_resize_arguments(args: &[String]) -> Result<ParsedArguments, CliError> {
    let value_flags = ['t', 'x', 'y'];
    let bool_flags = ['D', 'L', 'R', 'U'];
    let mut parsed = ParsedArguments {
        flags: HashSet::new(),
        options: HashMap::new(),
    };
    let mut index = 0;
    let mut past_terminator = false;
    while index < args.len() {
        let arg = &args[index];
        if past_terminator {
            index += 1;
            continue;
        }
        if arg == "--" {
            past_terminator = true;
            index += 1;
            continue;
        }
        if !arg.starts_with('-') || arg == "-" || arg.starts_with("--") {
            index += 1;
            continue;
        }
        let cluster = arg[1..].chars().collect::<Vec<_>>();
        let mut cursor = 0;
        while cursor < cluster.len() {
            let flag = cluster[cursor];
            if bool_flags.contains(&flag) {
                parsed.flags.insert(format!("-{flag}"));
                cursor += 1;
                continue;
            }
            if value_flags.contains(&flag) {
                let remainder = cluster[cursor + 1..].iter().collect::<String>();
                let value = if remainder.is_empty() {
                    index += 1;
                    args.get(index)
                        .cloned()
                        .ok_or_else(|| CliError::new(format!("-{flag} requires a value")))?
                } else {
                    remainder
                };
                parsed
                    .options
                    .entry(format!("-{flag}"))
                    .or_default()
                    .push(value);
                break;
            }
            break;
        }
        index += 1;
    }
    Ok(parsed)
}

fn trimmed(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn stable_numeric_id(raw: &str) -> String {
    let source = if raw.trim().is_empty() {
        "cmux"
    } else {
        raw.trim()
    };
    let mut hash = 14_695_981_039_346_656_037_u64;
    for byte in source.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    let value = hash & 0x7fff_ffff_ffff_ffff;
    (if value == 0 { 1 } else { value }).to_string()
}

fn selector_token(raw: &str) -> (&str, bool) {
    let original = raw.trim();
    let token = original.trim_start_matches(['$', '@', '%']).trim();
    (token, token != original)
}

fn item_matches(item: &Map<String, Value>, raw: &str, allow_index: bool) -> bool {
    let (token, sigiled) = selector_token(raw);
    let id = item.get("id").and_then(Value::as_str);
    let reference = item.get("ref").and_then(Value::as_str);
    id == Some(token)
        || (!sigiled && reference == Some(token))
        || id.is_some_and(|id| stable_numeric_id(id) == token)
        || reference.is_some_and(|reference| stable_numeric_id(reference) == token)
        || (allow_index
            && !sigiled
            && item
                .get("index")
                .and_then(Value::as_i64)
                .is_some_and(|index| token.parse::<i64>().ok() == Some(index)))
}

fn call_object<F>(call: &mut F, method: &str, params: Value) -> Result<Map<String, Value>, CliError>
where
    F: FnMut(&str, &Value) -> Result<Value, CliError>,
{
    call(method, &params)?
        .as_object()
        .cloned()
        .ok_or_else(|| CliError::new(format!("{method} did not return an object")))
}

fn workspace_items<F>(call: &mut F) -> Result<Vec<Map<String, Value>>, CliError>
where
    F: FnMut(&str, &Value) -> Result<Value, CliError>,
{
    Ok(call_object(call, "workspace.list", json!({}))?
        .remove("workspaces")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| value.as_object().cloned())
        .collect())
}

fn resolve_workspace<F>(
    selector: Option<&str>,
    environment: &TmuxCompatEnvironment<'_>,
    call: &mut F,
) -> Result<String, CliError>
where
    F: FnMut(&str, &Value) -> Result<Value, CliError>,
{
    let selector = trimmed(selector).or_else(|| trimmed(environment.workspace_id));
    if let Some(raw) = selector {
        let mut token = raw;
        if let Some((_, suffix)) = token.rsplit_once(':') {
            token = if suffix.is_empty() {
                token.trim_end_matches(':')
            } else {
                suffix
            };
        }
        let items = workspace_items(call)?;
        if let Some(item) = items.iter().find(|item| {
            item_matches(item, raw, true)
                || item_matches(item, token, true)
                || item
                    .get("title")
                    .and_then(Value::as_str)
                    .is_some_and(|title| title.trim() == raw.trim())
        }) {
            return item
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| CliError::new("Workspace target not found"));
        }
        return Err(CliError::new(format!("Workspace target not found: {raw}")));
    }
    let current = call_object(call, "workspace.current", json!({}))?;
    current
        .get("workspace_id")
        .or_else(|| current.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CliError::new("Workspace target not found"))
}

fn pane_items<F>(workspace_id: &str, call: &mut F) -> Result<Vec<Map<String, Value>>, CliError>
where
    F: FnMut(&str, &Value) -> Result<Value, CliError>,
{
    Ok(
        call_object(call, "pane.list", json!({"workspace_id": workspace_id}))?
            .remove("panes")
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_object().cloned())
            .collect(),
    )
}

fn pane_id(items: &[Map<String, Value>], selector: &str) -> Option<String> {
    items
        .iter()
        .find(|item| item_matches(item, selector, true))
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn target_selectors(raw: Option<&str>) -> (Option<&str>, Option<&str>) {
    let Some(raw) = trimmed(raw) else {
        return (None, None);
    };
    if raw.starts_with('%') || raw.starts_with("pane:") {
        return (None, Some(raw));
    }
    if let Some((workspace, pane)) = raw.rsplit_once('.') {
        return (Some(workspace), Some(pane));
    }
    (Some(raw), None)
}

struct ResolvedPaneTarget {
    workspace_id: String,
    pane_id: String,
    panes: Vec<Map<String, Value>>,
}

fn resolve_target<F>(
    raw: Option<&str>,
    environment: &TmuxCompatEnvironment<'_>,
    call: &mut F,
) -> Result<ResolvedPaneTarget, CliError>
where
    F: FnMut(&str, &Value) -> Result<Value, CliError>,
{
    let (workspace_selector, pane_selector) = target_selectors(raw);
    if let (None, Some(pane_selector)) = (workspace_selector, pane_selector) {
        if let Some(caller_workspace) = trimmed(environment.workspace_id) {
            if let Ok(workspace_id) = resolve_workspace(Some(caller_workspace), environment, call) {
                let panes = pane_items(&workspace_id, call)?;
                if let Some(id) = pane_id(&panes, pane_selector) {
                    return Ok(ResolvedPaneTarget {
                        workspace_id,
                        pane_id: id,
                        panes,
                    });
                }
            }
        }
        for workspace in workspace_items(call)? {
            let Some(workspace_id) = workspace.get("id").and_then(Value::as_str) else {
                continue;
            };
            let panes = pane_items(workspace_id, call)?;
            if let Some(id) = pane_id(&panes, pane_selector) {
                return Ok(ResolvedPaneTarget {
                    workspace_id: workspace_id.to_string(),
                    pane_id: id,
                    panes,
                });
            }
        }
        return Err(CliError::new("Pane target not found"));
    }

    let workspace_id = resolve_workspace(workspace_selector, environment, call)?;
    let panes = pane_items(&workspace_id, call)?;
    let pane_id = if let Some(selector) = pane_selector {
        pane_id(&panes, selector)
    } else if trimmed(environment.workspace_id) == Some(workspace_id.as_str()) {
        trimmed(environment.pane_id).and_then(|selector| pane_id(&panes, selector))
    } else {
        None
    }
    .or_else(|| {
        panes
            .iter()
            .find(|pane| pane.get("focused").and_then(Value::as_bool) == Some(true))
            .and_then(|pane| pane.get("id"))
            .and_then(Value::as_str)
            .map(str::to_string)
    })
    .ok_or_else(|| CliError::new("Pane target not found"))?;
    Ok(ResolvedPaneTarget {
        workspace_id,
        pane_id,
        panes,
    })
}

fn parse_cells(raw: Option<&str>) -> Option<i64> {
    raw.map(|raw| raw.replace('%', ""))?.parse().ok()
}

fn command_looks_like_omx_hud(command: &str, launched_through_omx: bool) -> bool {
    let lower = command.to_ascii_lowercase();
    let hud_is_word = lower
        .split(|character: char| {
            !(character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        })
        .any(|word| word == "hud");
    hud_is_word && (launched_through_omx || lower.contains("omx") || lower.contains("oh-my-codex"))
}

fn pane_looks_like_omx_hud<F>(
    workspace_id: &str,
    pane: &Map<String, Value>,
    environment: &TmuxCompatEnvironment<'_>,
    call: &mut F,
) -> bool
where
    F: FnMut(&str, &Value) -> Result<Value, CliError>,
{
    let Some(surface_id) = pane.get("selected_surface_id").and_then(Value::as_str) else {
        return false;
    };
    let Ok(mut payload) = call_object(call, "surface.list", json!({"workspace_id": workspace_id}))
    else {
        return false;
    };
    let surfaces = payload
        .remove("surfaces")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    if let Some(surface) = surfaces.iter().find_map(|surface| {
        let surface = surface.as_object()?;
        (surface.get("id").and_then(Value::as_str) == Some(surface_id)).then_some(surface)
    }) {
        for key in [
            "tmux_start_command",
            "pane_start_command",
            "initial_command",
        ] {
            if surface
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|command| {
                    command_looks_like_omx_hud(command, environment.launched_through_omx)
                })
            {
                return true;
            }
        }
    }
    call_object(
        call,
        "surface.read_text",
        json!({"workspace_id": workspace_id, "surface_id": surface_id, "lines": 4}),
    )
    .ok()
    .and_then(|payload| {
        payload
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_lowercase)
    })
    .is_some_and(|text| {
        text.contains("[omx#") && text.contains("turns:") && text.contains("session:")
    })
}

pub fn run_tmux_compat<F>(
    args: &[String],
    environment: &TmuxCompatEnvironment<'_>,
    mut call: F,
) -> Result<bool, CliError>
where
    F: FnMut(&str, &Value) -> Result<Value, CliError>,
{
    let (command, raw_args) = split_command(args)?;
    if !matches!(
        command.to_ascii_lowercase().as_str(),
        "resize-pane" | "resizep"
    ) {
        return Err(CliError::new(format!(
            "tmux compatibility command is not yet ported: {command}"
        )));
    }
    let parsed = parse_resize_arguments(raw_args)?;
    let has_direction = ["-L", "-R", "-U", "-D"]
        .iter()
        .any(|flag| parsed.has_flag(flag));
    let target = resolve_target(parsed.value("-t"), environment, &mut call)?;
    let pane = target
        .panes
        .iter()
        .find(|pane| pane.get("id").and_then(Value::as_str) == Some(target.pane_id.as_str()))
        .ok_or_else(|| CliError::new("Pane target not found"))?;

    if !has_direction {
        if let Some(target_cells) = parse_cells(parsed.value("-x")) {
            if target_cells <= 0 {
                return Ok(false);
            }
            let Some(cell_size) = pane
                .get("cell_width_px")
                .and_then(Value::as_i64)
                .filter(|size| *size > 0)
            else {
                return Ok(false);
            };
            call(
                "pane.resize",
                &json!({"workspace_id": target.workspace_id, "pane_id": target.pane_id, "absolute_axis": "horizontal", "target_pixels": target_cells * cell_size}),
            )?;
            return Ok(true);
        }
        if let Some(target_cells) = parse_cells(parsed.value("-y")) {
            if target_cells <= 0
                || pane_looks_like_omx_hud(&target.workspace_id, pane, environment, &mut call)
            {
                return Ok(false);
            }
            let Some(cell_size) = pane
                .get("cell_height_px")
                .and_then(Value::as_i64)
                .filter(|size| *size > 0)
            else {
                return Ok(false);
            };
            call(
                "pane.resize",
                &json!({"workspace_id": target.workspace_id, "pane_id": target.pane_id, "absolute_axis": "vertical", "target_pixels": target_cells * cell_size}),
            )?;
            return Ok(true);
        }
        return Ok(false);
    }

    let direction = if parsed.has_flag("-L") {
        "left"
    } else if parsed.has_flag("-U") {
        "up"
    } else if parsed.has_flag("-D") {
        "down"
    } else {
        "right"
    };
    let amount = parse_cells(parsed.value("-x").or_else(|| parsed.value("-y")))
        .unwrap_or(5)
        .max(1);
    call(
        "pane.resize",
        &json!({"workspace_id": target.workspace_id, "pane_id": target.pane_id, "direction": direction, "amount": amount}),
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn fixtures(method: &str, params: &Value) -> Result<Value, CliError> {
        match method {
            "workspace.list" => Ok(
                json!({"workspaces": [{"id":"workspace-a", "ref":"workspace:1", "index":0, "title":"Alpha"}]}),
            ),
            "pane.list" => Ok(json!({"panes": [
                {"id":"pane-a", "ref":"pane:1", "index":0, "focused":false, "selected_surface_id":"surface-a", "cell_width_px":8, "cell_height_px":16},
                {"id":"pane-b", "ref":"pane:2", "index":1, "focused":true, "selected_surface_id":"surface-b", "cell_width_px":8, "cell_height_px":16}
            ]})),
            "surface.list" => Ok(
                json!({"surfaces": [{"id":"surface-b", "initial_command":"node omx.js hud --watch"}]}),
            ),
            "pane.resize" => Ok(params.clone()),
            _ => Err(CliError::new(format!("unexpected call: {method}"))),
        }
    }

    #[test]
    fn absolute_width_resolves_attached_flags_and_multiplies_cell_width() {
        let mut resize = None;
        let changed = run_tmux_compat(
            &args(&["resizep", "-tworkspace-a.pane:2", "-x13"]),
            &TmuxCompatEnvironment::default(),
            |method, params| {
                if method == "pane.resize" {
                    resize = Some(params.clone());
                }
                fixtures(method, params)
            },
        )
        .unwrap();
        assert!(changed);
        assert_eq!(
            resize,
            Some(json!({
                "workspace_id":"workspace-a", "pane_id":"pane-b",
                "absolute_axis":"horizontal", "target_pixels":104
            }))
        );
    }

    #[test]
    fn directional_precedence_and_amount_match_tmux_compat() {
        let mut resize = None;
        run_tmux_compat(
            &args(&[
                "resize-pane",
                "-t",
                "workspace-a.pane:2",
                "-DU",
                "-y",
                "bad%",
            ]),
            &TmuxCompatEnvironment::default(),
            |method, params| {
                if method == "pane.resize" {
                    resize = Some(params.clone());
                }
                fixtures(method, params)
            },
        )
        .unwrap();
        assert_eq!(
            resize,
            Some(json!({
                "workspace_id":"workspace-a", "pane_id":"pane-b",
                "direction":"up", "amount":5
            }))
        );
    }

    #[test]
    fn absolute_height_is_a_noop_for_an_omx_hud() {
        let mut resized = false;
        let changed = run_tmux_compat(
            &args(&["resize-pane", "-t", "workspace-a.pane:2", "-y", "4"]),
            &TmuxCompatEnvironment::default(),
            |method, params| {
                resized |= method == "pane.resize";
                fixtures(method, params)
            },
        )
        .unwrap();
        assert!(!changed);
        assert!(!resized);
    }

    #[test]
    fn missing_value_and_command_errors_are_exact() {
        assert_eq!(
            run_tmux_compat(
                &args(&["resize-pane", "-x"]),
                &TmuxCompatEnvironment::default(),
                fixtures
            )
            .unwrap_err()
            .message,
            "-x requires a value"
        );
        assert_eq!(
            run_tmux_compat(&[], &TmuxCompatEnvironment::default(), fixtures)
                .unwrap_err()
                .message,
            "tmux shim requires a command"
        );
    }

    #[test]
    fn explicit_workspace_refs_win_over_tmux_zero_based_indexes() {
        let mut resized_workspace = None;
        run_tmux_compat(
            &args(&["resize-pane", "-t", "workspace:1.pane:2", "-R"]),
            &TmuxCompatEnvironment::default(),
            |method, params| match method {
                "workspace.list" => Ok(json!({"workspaces":[
                    {"id":"workspace-a", "ref":"workspace:1", "index":0},
                    {"id":"workspace-b", "ref":"workspace:2", "index":1}
                ]})),
                "pane.list" => Ok(json!({"panes":[
                    {"id":"pane-b", "ref":"pane:2", "index":1, "focused":true}
                ]})),
                "pane.resize" => {
                    resized_workspace = params
                        .get("workspace_id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    Ok(json!({}))
                }
                _ => Err(CliError::new("unexpected call")),
            },
        )
        .unwrap();
        assert_eq!(resized_workspace.as_deref(), Some("workspace-a"));
    }
}
