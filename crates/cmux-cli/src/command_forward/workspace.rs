use super::{
    apply_surface_selector, apply_window_scope_selector, apply_window_selector_value,
    apply_workspace_scope_selector, apply_workspace_selector, apply_workspace_target_selector,
    browser_workspace_params, has_explicit_workspace_selector, lifecycle_scope_values,
    looks_like_workspace_selector, normalize_action_name, parse_frozen_option, split_subcommand,
    surface_selector_params, terminal_environment_param, CliError, ControlCommand, ParsedArgs,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(super) fn workspace_subcommand(args: &[String]) -> Result<Option<ControlCommand>, CliError> {
    let Some((subcommand, rest)) = split_subcommand(args) else {
        return Ok(Some(ControlCommand::new(
            "workspace.current",
            serde_json::json!({}),
        )));
    };
    let mapped = match subcommand.as_str() {
        "list" | "ls" => ControlCommand::new("workspace.list", serde_json::json!({})),
        "current" | "show" => ControlCommand::new("workspace.current", serde_json::json!({})),
        "new" | "create" => ControlCommand::new("workspace.create", workspace_create_params(rest)?),
        "new-browser" | "create-browser" | "browser" => {
            ControlCommand::new("workspace.create_browser", browser_workspace_params(rest)?)
        }
        "close" => ControlCommand::new(
            "workspace.close",
            workspace_close_params(rest, "workspace close")?,
        ),
        "close-many" | "close-workspaces" => ControlCommand::new(
            "workspace.close_many",
            workspace_multi_selector_params(rest)?,
        ),
        "select" | "focus" => {
            ControlCommand::new("workspace.select", workspace_selector_params(rest)?)
        }
        "reorder" | "move" => {
            ControlCommand::new("workspace.reorder", workspace_reorder_params(rest)?)
        }
        "next" => ControlCommand::new("workspace.next", serde_json::json!({})),
        "previous" | "prev" => ControlCommand::new("workspace.previous", serde_json::json!({})),
        "rename" => ControlCommand::new(
            "workspace.rename",
            workspace_rename_params(rest, "workspace rename")?,
        ),
        "description" | "set-description" => ControlCommand::new(
            "workspace.set_description",
            workspace_description_params(rest)?,
        ),
        "set-progress" | "progress" => {
            ControlCommand::new("workspace.set_progress", workspace_progress_params(rest)?)
        }
        "clear-progress" => {
            ControlCommand::new("workspace.clear_progress", workspace_selector_params(rest)?)
        }
        "set-status" | "status" => {
            ControlCommand::new("workspace.set_status", workspace_status_params(rest)?)
        }
        "clear-status" => ControlCommand::new(
            "workspace.clear_status",
            workspace_status_key_params(rest, "workspace clear-status")?,
        ),
        "list-status" => {
            ControlCommand::new("workspace.list_status", workspace_selector_params(rest)?)
        }
        "set-agent-pid" | "agent-pid" => ControlCommand::new(
            "workspace.set_agent_pid",
            workspace_agent_pid_params(rest, "workspace set-agent-pid")?,
        ),
        "clear-agent-pid" => ControlCommand::new(
            "workspace.clear_agent_pid",
            workspace_status_key_params(rest, "workspace clear-agent-pid")?,
        ),
        "report-pr" | "pr" => ControlCommand::new(
            "workspace.report_pr",
            workspace_pr_params(rest, "workspace report-pr")?,
        ),
        "report-review" | "review" => ControlCommand::new(
            "workspace.report_review",
            workspace_pr_params(rest, "workspace report-review")?,
        ),
        "clear-pr" => ControlCommand::new("workspace.clear_pr", surface_selector_params(rest)?),
        "report-meta" | "set-meta" | "meta" => ControlCommand::new(
            "workspace.report_meta",
            workspace_meta_params(rest, "workspace report-meta")?,
        ),
        "clear-meta" => ControlCommand::new(
            "workspace.clear_meta",
            workspace_status_key_params(rest, "workspace clear-meta")?,
        ),
        "list-meta" => ControlCommand::new("workspace.list_meta", workspace_selector_params(rest)?),
        "report-meta-block" | "set-meta-block" | "meta-block" => ControlCommand::new(
            "workspace.report_meta_block",
            workspace_meta_block_params(rest, "workspace report-meta-block")?,
        ),
        "clear-meta-block" => ControlCommand::new(
            "workspace.clear_meta_block",
            workspace_status_key_params(rest, "workspace clear-meta-block")?,
        ),
        "list-meta-blocks" => ControlCommand::new(
            "workspace.list_meta_blocks",
            workspace_selector_params(rest)?,
        ),
        "reset-sidebar" => {
            ControlCommand::new("workspace.reset_sidebar", workspace_selector_params(rest)?)
        }
        "log" => ControlCommand::new("workspace.log", workspace_log_params(rest)?),
        "clear-log" => ControlCommand::new("workspace.clear_log", workspace_selector_params(rest)?),
        "list-log" => ControlCommand::new("workspace.list_log", workspace_list_log_params(rest)?),
        "sidebar-state" => {
            ControlCommand::new("workspace.sidebar_state", workspace_selector_params(rest)?)
        }
        "reset-color" => {
            ControlCommand::new("workspace.reset_color", workspace_selector_params(rest)?)
        }
        "pin" => ControlCommand::new(
            "workspace.set_pinned",
            workspace_bool_params(rest, "pinned", true)?,
        ),
        "unpin" => ControlCommand::new(
            "workspace.set_pinned",
            workspace_bool_params(rest, "pinned", false)?,
        ),
        "mark-unread" => ControlCommand::new(
            "workspace.set_unread",
            workspace_bool_params(rest, "unread", true)?,
        ),
        "mark-read" => ControlCommand::new(
            "workspace.set_unread",
            workspace_bool_params(rest, "unread", false)?,
        ),
        "group" => workspace_group_subcommand(rest)?,
        "restore-previous-launch" | "restore-previous" | "restore-session" => {
            ControlCommand::new("session.restore_previous_launch", serde_json::json!({}))
        }
        "equalize-splits" | "equalize" => {
            ControlCommand::new("workspace.equalize_splits", serde_json::json!({}))
        }
        _ => return Ok(None),
    };
    Ok(Some(mapped))
}

pub(super) fn workspace_group_subcommand(args: &[String]) -> Result<ControlCommand, CliError> {
    let Some((subcommand, rest)) = split_subcommand(args) else {
        return Err(CliError::new(
            "workspace-group requires a subcommand. Try: list, create, ungroup, delete, rename, collapse, expand, pin, unpin, add, remove, set-anchor, new-workspace, set-color, set-icon, move, focus",
        ));
    };
    let parsed = ParsedArgs::parse(rest)?;
    let mut params = serde_json::Map::new();
    apply_window_scope_selector(&parsed, &mut params);

    let method = match subcommand.as_str() {
        "list" => "workspace.group.list",
        "create" => {
            let name = parsed
                .value(&["--name"])
                .cloned()
                .or_else(|| parsed.first_positional().map(str::to_owned))
                .unwrap_or_default();
            params.insert("name".into(), serde_json::json!(name));
            if let Some(cwd) = parsed.value(&["--cwd"]) {
                params.insert(
                    "cwd".into(),
                    serde_json::json!(resolve_workspace_group_path(cwd)),
                );
            }
            if let Some(from) = parsed.value(&["--from"]) {
                // Swift String.split omits empty subsequences before trimming.
                let child_workspace_ids: Vec<String> = from
                    .split(',')
                    .filter(|part| !part.is_empty())
                    .map(|part| part.trim().to_owned())
                    .collect();
                params.insert(
                    "child_workspace_ids".into(),
                    serde_json::json!(child_workspace_ids),
                );
            }
            "workspace.group.create"
        }
        "ungroup" | "delete" | "collapse" | "expand" | "pin" | "unpin" | "focus" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            match subcommand.as_str() {
                "ungroup" => "workspace.group.ungroup",
                "delete" => "workspace.group.delete",
                "collapse" => "workspace.group.collapse",
                "expand" => "workspace.group.expand",
                "pin" => "workspace.group.pin",
                "unpin" => "workspace.group.unpin",
                "focus" => "workspace.group.focus",
                _ => unreachable!(),
            }
        }
        "rename" => {
            let group_id = workspace_group_id(&parsed, &subcommand)?;
            let name = parsed.value(&["--name"]).cloned().or_else(|| {
                parsed
                    .positionals
                    .iter()
                    .find(|candidate| candidate.as_str() != group_id)
                    .cloned()
            });
            let name = name.ok_or_else(|| CliError::new("rename requires --name <name>"))?;
            params.insert("group_id".into(), serde_json::json!(group_id));
            params.insert("name".into(), serde_json::json!(name));
            "workspace.group.rename"
        }
        "add" => {
            let Some(group_id) = parsed.value(&["--group"]) else {
                return Err(CliError::new("add requires --group <id> --workspace <id>"));
            };
            let Some(workspace_id) = parsed.value(&["--workspace"]) else {
                return Err(CliError::new("add requires --group <id> --workspace <id>"));
            };
            params.insert("group_id".into(), serde_json::json!(group_id));
            params.insert("workspace_id".into(), serde_json::json!(workspace_id));
            "workspace.group.add"
        }
        "remove" => {
            let workspace_id = parsed
                .value(&["--workspace"])
                .cloned()
                .or_else(|| parsed.first_positional().map(str::to_owned))
                .ok_or_else(|| CliError::new("remove requires --workspace <id>"))?;
            params.insert("workspace_id".into(), serde_json::json!(workspace_id));
            "workspace.group.remove"
        }
        "set-anchor" => {
            let Some(group_id) = parsed.value(&["--group"]) else {
                return Err(CliError::new(
                    "set-anchor requires --group <id> --workspace <id>",
                ));
            };
            let Some(workspace_id) = parsed.value(&["--workspace"]) else {
                return Err(CliError::new(
                    "set-anchor requires --group <id> --workspace <id>",
                ));
            };
            params.insert("group_id".into(), serde_json::json!(group_id));
            params.insert("workspace_id".into(), serde_json::json!(workspace_id));
            "workspace.group.set_anchor"
        }
        "new-workspace" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            if let Some(placement) = parsed.value(&["--placement"]) {
                params.insert("placement".into(), serde_json::json!(placement));
            }
            "workspace.group.new_workspace"
        }
        "set-color" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            params.insert(
                "hex".into(),
                serde_json::json!(parsed.value(&["--hex"]).cloned().unwrap_or_default()),
            );
            "workspace.group.set_color"
        }
        "set-icon" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            params.insert(
                "symbol".into(),
                serde_json::json!(parsed.value(&["--symbol"]).cloned().unwrap_or_default()),
            );
            "workspace.group.set_icon"
        }
        "move" => {
            params.insert(
                "group_id".into(),
                serde_json::json!(workspace_group_id(&parsed, &subcommand)?),
            );
            if let Some(to_index) = parsed.value(&["--to-index"]) {
                let to_index = to_index
                    .parse::<i64>()
                    .map_err(|_| CliError::new("move --to-index must be an integer"))?;
                params.insert("to_index".into(), serde_json::json!(to_index));
            } else if let Some(before) = parsed.value(&["--before"]) {
                params.insert("before_group_id".into(), serde_json::json!(before));
            } else if let Some(after) = parsed.value(&["--after"]) {
                params.insert("after_group_id".into(), serde_json::json!(after));
            } else {
                return Err(CliError::new(
                    "move requires --to-index <n>, --before <group>, or --after <group>",
                ));
            }
            "workspace.group.move"
        }
        _ => {
            return Err(CliError::new(format!(
                "Unknown workspace-group subcommand: {subcommand}"
            )))
        }
    };
    Ok(ControlCommand::new(
        method,
        serde_json::Value::Object(params),
    ))
}

fn workspace_group_id(parsed: &ParsedArgs, subcommand: &str) -> Result<String, CliError> {
    parsed
        .value(&["--group"])
        .cloned()
        .or_else(|| parsed.first_positional().map(str::to_owned))
        .ok_or_else(|| {
            CliError::new(format!(
                "workspace-group {subcommand} requires a group id or --group <id>"
            ))
        })
}

fn resolve_workspace_group_path(raw: &str) -> String {
    let expanded = if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .map(|home| home.join(raw.trim_start_matches('~').trim_start_matches(['/', '\\'])))
            .unwrap_or_else(|| PathBuf::from(raw))
    } else {
        PathBuf::from(raw)
    };
    let resolved = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir().unwrap_or_default().join(expanded)
    };
    resolved.to_string_lossy().into_owned()
}

pub(super) fn legacy_workspace_create_params(
    args: &[String],
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    validate_legacy_flags(
        "new-workspace",
        &parsed,
        &[
            "--name",
            "--description",
            "--cwd",
            "--command",
            "--env",
            "--env-file",
            "--layout",
            "--window",
            "--focus",
            "--group",
            "--group-placement",
            "--group-reference",
        ],
    )?;
    let mut params = serde_json::Map::new();
    if let Some(cwd) = parsed.value(&["--cwd"]) {
        params.insert(
            "working_directory".to_string(),
            serde_json::json!(expand_workspace_path(cwd)?),
        );
    }
    if let Some(title) = parsed.value(&["--name"]) {
        params.insert("title".to_string(), serde_json::json!(title));
    }
    if let Some(description) = parsed.value(&["--description"]) {
        params.insert("description".to_string(), serde_json::json!(description));
    }
    if let Some(command) = parsed.value(&["--command"]) {
        params.insert(
            "__post_create_command".to_string(),
            serde_json::json!(command),
        );
    }
    let mut environment = BTreeMap::new();
    for path in parsed.values(&["--env-file"]) {
        let contents = std::fs::read_to_string(path)
            .map_err(|error| CliError::new(format!("failed to read env file {path}: {error}")))?;
        for raw_line in contents
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
        {
            let line = raw_line
                .strip_prefix("export ")
                .map(str::trim)
                .unwrap_or(raw_line);
            let (key, value) = parse_environment_assignment(line, &format!("--env-file '{path}'"))?;
            environment.insert(key, unquote_env_file_value(&value));
        }
    }
    for assignment in parsed.values(&["--env"]) {
        let (key, value) = parse_environment_assignment(assignment, "--env")?;
        environment.insert(key, value);
    }
    if !environment.is_empty() {
        params.insert("workspace_env".to_string(), serde_json::json!(environment));
    }
    if let Some(layout) = parsed.value(&["--layout"]) {
        let value: serde_json::Value = serde_json::from_str(layout)
            .map_err(|_| CliError::new("--layout must be a JSON object"))?;
        if !value.is_object() {
            return Err(CliError::new("--layout must be a JSON object"));
        }
        params.insert("layout".to_string(), value);
    }
    if let Some(focus) = parsed.value(&["--focus"]) {
        let focus = match focus.as_str() {
            "true" => true,
            "false" => false,
            _ => return Err(CliError::new("--focus must be true or false")),
        };
        params.insert("focus".to_string(), serde_json::json!(focus));
    }
    if let Some(group) = parsed.value(&["--group"]) {
        params.insert("group_id".into(), serde_json::json!(group));
    }
    if let Some(placement) = parsed.value(&["--group-placement"]) {
        params.insert("group_placement".into(), serde_json::json!(placement));
    }
    if let Some(reference) = parsed.value(&["--group-reference"]) {
        params.insert(
            "group_reference_workspace_id".into(),
            serde_json::json!(reference),
        );
    }
    apply_legacy_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn workspace_create_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    if let Some(cwd) = parsed.value(&["--cwd", "--path", "--directory"]) {
        params.insert("current_directory".to_string(), serde_json::json!(cwd));
    }
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
    if let Some(environment) = terminal_environment_param(&parsed)? {
        params.insert(
            "initial_terminal_environment".to_string(),
            serde_json::json!(environment),
        );
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn legacy_workspace_scope_params(
    args: &[String],
    command: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    validate_legacy_flags(command, &parsed, &["--window"])?;
    let mut params = serde_json::Map::new();
    apply_legacy_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_selector_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector(&parsed, &mut params)?;
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_close_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if command_label == "close-workspace" {
        validate_legacy_flags(command_label, &parsed, &["--workspace", "--window"])?;
        if parsed.value(&["--workspace"]).is_none() {
            return Err(CliError::new("close-workspace requires --workspace"));
        }
    } else if !has_explicit_workspace_selector(&parsed) && parsed.first_positional().is_none() {
        return Err(CliError::new(format!(
            "{command_label} requires a workspace target (workspace:N or workspace id)"
        )));
    }
    let mut params = serde_json::Map::new();
    if command_label == "close-workspace" {
        apply_legacy_workspace_selector(&parsed, &mut params);
        apply_legacy_window_scope_selector(&parsed, &mut params);
    } else {
        apply_workspace_selector(&parsed, &mut params)?;
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_multi_selector_params(
    args: &[String],
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if parsed.positionals.is_empty() {
        return Err(CliError::new(
            "close-workspaces requires at least one workspace",
        ));
    }
    let mut refs = Vec::new();
    let mut ids = Vec::new();
    for selector in &parsed.positionals {
        if selector.chars().all(|ch| ch.is_ascii_digit()) {
            refs.push(format!("workspace:{selector}"));
        } else if selector.starts_with("workspace:") {
            refs.push(selector.clone());
        } else {
            ids.push(selector.clone());
        }
    }
    let mut params = serde_json::Map::new();
    if !refs.is_empty() {
        params.insert("workspace_refs".to_string(), serde_json::json!(refs));
    }
    if !ids.is_empty() {
        params.insert("workspace_ids".to_string(), serde_json::json!(ids));
    }
    Ok(serde_json::Value::Object(params))
}

fn workspace_reorder_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if parsed.value(&["--to-index", "--target-index"]).is_some() {
        return Err(CliError::new(
            "workspace reorder requires a before/after workspace ref or id; --to-index is not supported",
        ));
    }
    let mut params = serde_json::Map::new();

    let from = parsed
        .value(&["--from", "--workspace", "--workspace-id", "--id"])
        .cloned()
        .or_else(|| parsed.positionals.first().cloned())
        .ok_or_else(|| CliError::new("workspace reorder requires a workspace"))?;
    if from.chars().all(|ch| ch.is_ascii_digit()) {
        params.insert(
            "workspace_ref".to_string(),
            serde_json::json!(format!("workspace:{from}")),
        );
    } else if from.starts_with("workspace:") {
        params.insert("workspace_ref".to_string(), serde_json::json!(from));
    } else {
        params.insert("workspace_id".to_string(), serde_json::json!(from));
    }

    if let Some(after) = parsed.value(&["--after", "--after-workspace"]) {
        apply_workspace_target_selector(after, "after_workspace", &mut params);
    } else {
        let to = parsed
            .value(&["--to", "--before", "--before-workspace"])
            .cloned()
            .or_else(|| parsed.positionals.get(1).cloned())
            .ok_or_else(|| CliError::new("workspace reorder requires a destination"))?;
        apply_workspace_target_selector(&to, "before_workspace", &mut params);
    }
    if parsed.has_flag("--top-level-rows") {
        params.insert("uses_top_level_rows".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn canonical_workspace_reorder_params(
    args: &[String],
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let workspace = parsed
        .value(&["--workspace"])
        .cloned()
        .or_else(|| parsed.positionals.first().cloned())
        .ok_or_else(|| CliError::new("reorder-workspace requires --workspace <id|ref|index>"))?;
    let mut params = serde_json::Map::new();
    apply_workspace_target_selector(&workspace, "workspace", &mut params);

    if let Some(before) = parsed.value(&["--before", "--before-workspace"]) {
        apply_workspace_target_selector(before, "before_workspace", &mut params);
    }
    if let Some(after) = parsed.value(&["--after", "--after-workspace"]) {
        apply_workspace_target_selector(after, "after_workspace", &mut params);
    }
    if let Some(index) = parsed.value(&["--index"]) {
        let index = index
            .parse::<i64>()
            .map_err(|_| CliError::new("--index must be an integer"))?;
        params.insert("index".to_string(), serde_json::json!(index));
    }
    apply_window_scope_selector(&parsed, &mut params);
    if parsed.has_flag("--dry-run") {
        params.insert("dry_run".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn canonical_workspaces_reorder_params(
    args: &[String],
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let order = parsed.value(&["--order"]).ok_or_else(|| {
        CliError::new("reorder-workspaces requires --order <id|ref|index>,<id|ref|index>,...")
    })?;
    if order.is_empty() {
        return Err(CliError::new(
            "reorder-workspaces requires at least one workspace in --order",
        ));
    }
    let workspace_ids: Vec<String> = order
        .split(',')
        .map(str::trim)
        .map(str::to_string)
        .collect();
    if workspace_ids.iter().any(String::is_empty) {
        return Err(CliError::new(
            "reorder-workspaces --order cannot contain empty workspace refs",
        ));
    }
    let workspace_ids: Vec<String> = workspace_ids
        .into_iter()
        .map(|workspace| {
            if workspace
                .chars()
                .all(|character| character.is_ascii_digit())
            {
                format!("workspace:{workspace}")
            } else {
                workspace
            }
        })
        .collect();
    let mut params = serde_json::Map::new();
    params.insert(
        "workspace_ids".to_string(),
        serde_json::json!(workspace_ids),
    );
    apply_window_scope_selector(&parsed, &mut params);
    if parsed.has_flag("--dry-run") {
        params.insert("dry_run".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn move_workspace_to_window_params(
    args: &[String],
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let workspace = parsed
        .value(&["--workspace"])
        .ok_or_else(|| CliError::new("move-workspace-to-window requires --workspace"))?;
    let window = parsed
        .value(&["--window"])
        .ok_or_else(|| CliError::new("move-workspace-to-window requires --window"))?;
    let mut params = serde_json::Map::new();
    apply_workspace_target_selector(workspace, "workspace", &mut params);
    apply_window_selector_value(window, &mut params);
    Ok(serde_json::Value::Object(params))
}

pub(super) fn legacy_workspace_rename_params(
    args: &[String],
    command_name: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    if command_name == "rename-workspace" || command_name == "rename-window" {
        validate_legacy_flags(
            command_name,
            &parsed,
            &["--workspace", "--window", "--title", "--name"],
        )?;
    }
    let mut params = serde_json::Map::new();
    if parsed.value(&["--workspace"]).is_some() {
        apply_legacy_workspace_selector(&parsed, &mut params);
    } else {
        params.insert("resolve_current_workspace".into(), serde_json::json!(true));
    }
    apply_legacy_window_scope_selector(&parsed, &mut params);
    let title = parsed
        .value(&["--title", "--name"])
        .cloned()
        .or_else(|| {
            let title = parsed.positionals.join(" ");
            (!title.trim().is_empty()).then_some(title)
        })
        .ok_or_else(|| CliError::new(format!("{command_name} requires a title")))?;
    if title.trim().is_empty() {
        return Err(CliError::new(format!("{command_name} requires a title")));
    }
    params.insert("title".to_string(), serde_json::json!(title));
    Ok(serde_json::Value::Object(params))
}

fn workspace_rename_params(
    args: &[String],
    command_name: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_free_text(&parsed, &mut params)?;
    let title = parsed
        .value(&["--title", "--name"])
        .cloned()
        .or_else(|| workspace_free_text_from_positionals(&parsed))
        .ok_or_else(|| CliError::new(format!("{command_name} requires a title")))?;
    params.insert("title".to_string(), serde_json::json!(title));
    Ok(serde_json::Value::Object(params))
}

pub(super) fn legacy_workspace_target_params(
    args: &[String],
    command: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    validate_legacy_flags(command, &parsed, &["--workspace", "--window"])?;
    if parsed.value(&["--workspace"]).is_none() {
        return Err(CliError::new(format!("{command} requires --workspace")));
    }
    let mut params = serde_json::Map::new();
    apply_legacy_workspace_selector(&parsed, &mut params);
    apply_legacy_window_scope_selector(&parsed, &mut params);
    Ok(serde_json::Value::Object(params))
}

fn apply_legacy_workspace_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    let Some(value) = parsed.value(&["--workspace"]) else {
        return;
    };
    if let Ok(index) = value.parse::<usize>() {
        params.insert("workspace_index".into(), serde_json::json!(index));
    } else {
        apply_workspace_selector_value(value, params);
    }
}

fn apply_legacy_window_scope_selector(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    let Some(value) = parsed.value(&["--window"]) else {
        return;
    };
    if let Ok(index) = value.parse::<usize>() {
        params.insert("window_index".into(), serde_json::json!(index));
    } else {
        apply_window_selector_value(value, params);
    }
}

fn apply_workspace_selector_value(
    value: &str,
    params: &mut serde_json::Map<String, serde_json::Value>,
) {
    if value.starts_with("workspace:") {
        params.insert("workspace_ref".into(), serde_json::json!(value));
    } else {
        params.insert("workspace_id".into(), serde_json::json!(value));
    }
}

fn parse_environment_assignment(
    assignment: &str,
    source: &str,
) -> Result<(String, String), CliError> {
    let (key, value) = assignment.split_once('=').ok_or_else(|| {
        CliError::new(format!(
            "new-workspace: {source} entry '{assignment}' must be in KEY=VALUE form"
        ))
    })?;
    if key.trim().is_empty() {
        return Err(CliError::new(format!(
            "new-workspace: {source} entry '{assignment}' has an empty key"
        )));
    }
    Ok((key.trim().to_string(), value.to_string()))
}

fn unquote_env_file_value(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        trimmed[1..trimmed.len() - 1].to_string()
    } else {
        trimmed.to_string()
    }
}

fn expand_workspace_path(raw: &str) -> Result<String, CliError> {
    let expanded = if raw == "~" || raw.starts_with("~/") || raw.starts_with("~\\") {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or_else(|| {
                CliError::new("new-workspace: unable to expand '~' without a home directory")
            })?;
        let suffix = raw.trim_start_matches('~').trim_start_matches(['/', '\\']);
        PathBuf::from(home).join(suffix)
    } else {
        PathBuf::from(raw)
    };
    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map_err(|error| {
                CliError::new(format!("new-workspace: failed to resolve --cwd: {error}"))
            })?
            .join(expanded)
    };
    let normalized = normalize_path(&absolute).to_string_lossy().into_owned();
    Ok(normalized
        .strip_prefix(r"\\?\")
        .unwrap_or(&normalized)
        .to_string())
}

fn normalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn validate_legacy_flags(
    command: &str,
    parsed: &ParsedArgs,
    allowed: &[&str],
) -> Result<(), CliError> {
    if let Some(flag) = parsed
        .flags
        .iter()
        .find(|flag| !allowed.contains(&flag.as_str()))
    {
        return Err(CliError::new(format!("{command}: unknown flag '{flag}'")));
    }
    if let Some((flag, _)) = parsed
        .values
        .iter()
        .find(|(flag, _)| !allowed.contains(&flag.as_str()))
    {
        return Err(CliError::new(format!("{command}: unknown flag '{flag}'")));
    }
    Ok(())
}

fn workspace_description_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_free_text(&parsed, &mut params)?;
    let description = parsed
        .value(&["--description", "--text", "--body"])
        .cloned()
        .or_else(|| workspace_free_text_from_positionals(&parsed))
        .ok_or_else(|| CliError::new("workspace description requires text"))?;
    params.insert("description".to_string(), serde_json::json!(description));
    Ok(serde_json::Value::Object(params))
}

fn workspace_bool_params(
    args: &[String],
    key: &str,
    value: bool,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector(&parsed, &mut params)?;
    params.insert(key.to_string(), serde_json::json!(value));
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_progress_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_free_text(&parsed, &mut params)?;
    let raw_value = parsed
        .value(&["--value", "--progress"])
        .cloned()
        .or_else(|| workspace_progress_value_from_positionals(&parsed))
        .ok_or_else(|| CliError::new("set-progress requires a progress value"))?;
    let value = raw_value
        .parse::<f64>()
        .map_err(|_| CliError::new("set-progress requires a numeric progress value"))?;
    params.insert("value".to_string(), serde_json::json!(value));
    if let Some(label) = parsed.value(&["--label", "--text"]) {
        params.insert("label".to_string(), serde_json::json!(label));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_status_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    let (key, value) = workspace_key_value_from_args(&parsed, "set-status")?;
    params.insert("key".to_string(), serde_json::json!(key));
    params.insert("value".to_string(), serde_json::json!(value));
    if let Some(priority) = parsed.value(&["--priority"]) {
        let priority = priority
            .parse::<i64>()
            .map_err(|_| CliError::new("set-status --priority requires an integer"))?;
        params.insert("priority".to_string(), serde_json::json!(priority));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_agent_pid_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    if !params.contains_key("workspace_ref")
        && !params.contains_key("workspace_id")
        && parsed
            .positionals
            .first()
            .is_some_and(|value| looks_like_workspace_selector(value))
        && (parsed.value(&["--key", "--name"]).is_some()
            || parsed
                .value(&["--pid", "--process-id", "--processId"])
                .is_some())
    {
        apply_workspace_selector(&parsed, &mut params)?;
    }
    let key_positionals = if (params.contains_key("workspace_ref")
        || params.contains_key("workspace_id"))
        && parsed
            .positionals
            .first()
            .is_some_and(|value| looks_like_workspace_selector(value))
    {
        parsed.positionals[1..].to_vec()
    } else {
        workspace_key_positionals(&parsed)
    };
    let key = parsed
        .value(&["--key", "--name"])
        .cloned()
        .or_else(|| key_positionals.first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a key")))?;
    let pid_raw = parsed
        .value(&["--pid", "--process-id", "--processId"])
        .cloned()
        .or_else(|| {
            if parsed.value(&["--key", "--name"]).is_some() {
                key_positionals.first().cloned()
            } else {
                key_positionals.get(1).cloned()
            }
        })
        .ok_or_else(|| CliError::new(format!("{command_label} requires a pid")))?;
    let pid = pid_raw
        .parse::<u32>()
        .map_err(|_| CliError::new(format!("{command_label} pid must be a positive integer")))?;
    if pid == 0 {
        return Err(CliError::new(format!(
            "{command_label} pid must be a positive integer"
        )));
    }
    params.insert("key".to_string(), serde_json::json!(key));
    params.insert("pid".to_string(), serde_json::json!(pid));
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_meta_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    let (key, value) = workspace_key_value_from_args(&parsed, command_label)?;
    params.insert("key".to_string(), serde_json::json!(key));
    params.insert("value".to_string(), serde_json::json!(value));
    if let Some(icon) = parsed.value(&["--icon"]) {
        params.insert("icon".to_string(), serde_json::json!(icon));
    }
    if let Some(color) = parsed.value(&["--color"]) {
        params.insert("color".to_string(), serde_json::json!(color));
    }
    if let Some(url) = parsed.value(&["--url"]) {
        params.insert("url".to_string(), serde_json::json!(url));
    }
    if let Some(format) = parsed.value(&["--format"]) {
        params.insert("format".to_string(), serde_json::json!(format));
    }
    if let Some(priority) = parsed.value(&["--priority"]) {
        let priority = priority.parse::<i64>().map_err(|_| {
            CliError::new(format!("{command_label} --priority requires an integer"))
        })?;
        params.insert("priority".to_string(), serde_json::json!(priority));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_meta_block_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    let key = parsed
        .value(&["--key", "--name"])
        .cloned()
        .or_else(|| workspace_key_positionals(&parsed).first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a key")))?;
    let markdown = parsed
        .value(&["--markdown", "--value", "--text"])
        .cloned()
        .or_else(|| {
            let positionals = workspace_key_positionals(&parsed);
            (positionals.len() > 1).then(|| positionals[1..].join(" "))
        })
        .ok_or_else(|| CliError::new(format!("{command_label} requires markdown")))?;
    params.insert("key".to_string(), serde_json::json!(key));
    params.insert("markdown".to_string(), serde_json::json!(markdown));
    if let Some(priority) = parsed.value(&["--priority"]) {
        let priority = priority.parse::<i64>().map_err(|_| {
            CliError::new(format!("{command_label} --priority requires an integer"))
        })?;
        params.insert("priority".to_string(), serde_json::json!(priority));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_pr_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_scope_selector(&parsed, &mut params);
    apply_surface_selector(&parsed, &mut params)?;

    let positional_start = if !params.contains_key("workspace_ref")
        && !params.contains_key("workspace_id")
        && parsed
            .positionals
            .first()
            .is_some_and(|value| value.starts_with("workspace:"))
        && parsed.positionals.len() > 2
    {
        params.insert(
            "workspace_ref".to_string(),
            serde_json::json!(parsed.positionals[0]),
        );
        1
    } else {
        0
    };
    let positionals = &parsed.positionals[positional_start..];
    let number = parsed
        .value(&["--number", "--pr"])
        .cloned()
        .or_else(|| positionals.first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a number")))?;
    let number = number
        .parse::<i64>()
        .map_err(|_| CliError::new(format!("{command_label} number must be an integer")))?;
    params.insert("number".to_string(), serde_json::json!(number));

    let url = parsed
        .value(&["--url", "--href"])
        .cloned()
        .or_else(|| positionals.get(1).cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a URL")))?;
    params.insert("url".to_string(), serde_json::json!(url));

    if let Some(label) = parsed.value(&["--label"]) {
        params.insert("label".to_string(), serde_json::json!(label));
    }
    if let Some(state) = parsed.value(&["--state", "--status"]) {
        params.insert("state".to_string(), serde_json::json!(state));
    }
    if let Some(branch) = parsed.value(&["--branch"]) {
        params.insert("branch".to_string(), serde_json::json!(branch));
    }
    if parsed.has_flag("--stale") {
        params.insert("stale".to_string(), serde_json::json!(true));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_status_key_params(
    args: &[String],
    command_label: &str,
) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_key_value(&parsed, &mut params)?;
    let key = parsed
        .value(&["--key", "--name"])
        .cloned()
        .or_else(|| workspace_key_positionals(&parsed).first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a key")))?;
    params.insert("key".to_string(), serde_json::json!(key));
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_log_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector_for_free_text(&parsed, &mut params)?;
    let message = parsed
        .value(&["--message", "--text"])
        .cloned()
        .or_else(|| workspace_free_text_from_positionals(&parsed))
        .ok_or_else(|| CliError::new("log requires a message"))?;
    params.insert("message".to_string(), serde_json::json!(message));
    if let Some(level) = parsed.value(&["--level"]) {
        params.insert("level".to_string(), serde_json::json!(level));
    }
    Ok(serde_json::Value::Object(params))
}

pub(super) fn workspace_list_log_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let parsed = ParsedArgs::parse(args)?;
    let mut params = serde_json::Map::new();
    apply_workspace_selector(&parsed, &mut params)?;
    if let Some(limit) = parsed.value(&["--limit"]) {
        let limit = limit
            .parse::<usize>()
            .map_err(|_| CliError::new("list-log --limit requires a positive integer"))?;
        params.insert("limit".to_string(), serde_json::json!(limit));
    }
    Ok(serde_json::Value::Object(params))
}

fn apply_workspace_selector_for_free_text(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if has_explicit_workspace_selector(parsed) {
        return apply_workspace_selector(parsed, params);
    }
    if parsed
        .positionals
        .first()
        .is_some_and(|value| looks_like_workspace_selector(value))
        && parsed.positionals.len() > 1
    {
        return apply_workspace_selector(parsed, params);
    }
    Ok(())
}

fn workspace_free_text_from_positionals(parsed: &ParsedArgs) -> Option<String> {
    let first = parsed.positionals.first()?;
    let text_start = if looks_like_workspace_selector(first) && parsed.positionals.len() > 1 {
        1
    } else {
        0
    };
    let text = parsed.positionals[text_start..].join(" ");
    (!text.trim().is_empty()).then_some(text)
}

fn workspace_progress_value_from_positionals(parsed: &ParsedArgs) -> Option<String> {
    let first = parsed.positionals.first()?;
    let value_index = if looks_like_workspace_selector(first) && parsed.positionals.len() > 1 {
        1
    } else {
        0
    };
    parsed.positionals.get(value_index).cloned()
}

fn apply_workspace_selector_for_key_value(
    parsed: &ParsedArgs,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), CliError> {
    if has_explicit_workspace_selector(parsed) {
        return apply_workspace_selector(parsed, params);
    }
    if parsed
        .positionals
        .first()
        .is_some_and(|value| looks_like_workspace_selector(value))
        && parsed.positionals.len() > 2
    {
        return apply_workspace_selector(parsed, params);
    }
    Ok(())
}

fn workspace_key_positionals(parsed: &ParsedArgs) -> Vec<String> {
    let start = if parsed
        .positionals
        .first()
        .is_some_and(|value| looks_like_workspace_selector(value))
        && parsed.positionals.len() > 2
    {
        1
    } else {
        0
    };
    parsed.positionals[start..].to_vec()
}

fn workspace_key_value_from_args(
    parsed: &ParsedArgs,
    command_label: &str,
) -> Result<(String, String), CliError> {
    let key = parsed
        .value(&["--key", "--name"])
        .cloned()
        .or_else(|| workspace_key_positionals(parsed).first().cloned())
        .ok_or_else(|| CliError::new(format!("{command_label} requires a key")))?;
    let value = parsed
        .value(&["--value", "--status", "--text"])
        .cloned()
        .or_else(|| {
            let positionals = workspace_key_positionals(parsed);
            (positionals.len() > 1).then(|| positionals[1..].join(" "))
        })
        .ok_or_else(|| CliError::new(format!("{command_label} requires a value")))?;
    Ok((key, value))
}

pub(super) fn workspace_action_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let (workspace, rem0) = parse_frozen_option(args, "--workspace");
    let (action_option, rem1) = parse_frozen_option(&rem0, "--action");
    let (title_option, rem2) = parse_frozen_option(&rem1, "--title");
    let (color_option, rem3) = parse_frozen_option(&rem2, "--color");
    let (description_option, rem4) = parse_frozen_option(&rem3, "--description");
    let (window, mut positional) = parse_frozen_option(&rem4, "--window");
    let action_raw = if let Some(action) = action_option {
        action
    } else if positional.is_empty() {
        return Err(CliError::new("workspace-action requires --action <name>"));
    } else {
        positional.remove(0)
    };
    if let Some(unknown) = positional.iter().find(|value| value.starts_with("--")) {
        return Err(CliError::new(format!(
            "workspace-action: unknown flag '{unknown}'"
        )));
    }
    let action = normalize_action_name(&action_raw);
    let inferred_raw = positional.join(" ");
    let inferred = inferred_raw.trim();
    let title = title_option
        .map(|value| value.trim().to_owned())
        .or_else(|| (action == "rename" && !inferred.is_empty()).then(|| inferred.to_owned()));
    if action == "rename" && title.as_deref().is_none_or(str::is_empty) {
        return Err(CliError::new(
            "workspace-action rename requires --title <text> (or a trailing title)",
        ));
    }
    let color = color_option
        .map(|value| value.trim().to_owned())
        .or_else(|| (action == "set_color" && !inferred.is_empty()).then(|| inferred.to_owned()));
    if action == "set_color" && color.as_deref().is_none_or(str::is_empty) {
        return Err(CliError::new(
            "workspace-action set-color requires --color <name|#hex> (or a trailing color)",
        ));
    }
    let description = description_option
        .map(|value| value.trim().to_owned())
        .or_else(|| {
            (action == "set_description" && !inferred.is_empty())
                .then(|| inferred_raw.trim().to_owned())
        });
    if action == "set_description" && description.as_deref().is_none_or(str::is_empty) {
        return Err(CliError::new(
            "workspace-action set-description requires --description <text> (or trailing text)",
        ));
    }

    let mut params = lifecycle_scope_values(workspace.as_deref(), window.as_deref())?;
    params.insert("action".into(), serde_json::json!(action));
    if let Some(title) = title.filter(|value| !value.is_empty()) {
        params.insert("title".into(), serde_json::json!(title));
    }
    if let Some(color) = color.filter(|value| !value.is_empty()) {
        params.insert("color".into(), serde_json::json!(color));
    }
    if let Some(description) = description.filter(|value| !value.is_empty()) {
        params.insert("description".into(), serde_json::json!(description));
    }
    Ok(serde_json::Value::Object(params))
}
