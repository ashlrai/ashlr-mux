//! Local agent-hook installer commands for the Windows CLI.
//!
//! The broad `cmux hooks setup` contract covers many agents. This module starts
//! with the Claude Code integration path because the Windows desktop menu needs a
//! concrete CLI target that previews the config diff and asks for confirmation.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::invocation::CliError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeIntegrationPlan {
    pub before: String,
    pub after: String,
    pub diff: String,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HooksRequest {
    InstallClaude { yes: bool },
}

pub fn run_hooks_command(command: &str, args: &[String]) -> Result<String, CliError> {
    match parse_hooks_request(command, args)? {
        HooksRequest::InstallClaude { yes } => install_claude_code_integration(yes),
    }
}

fn parse_hooks_request(command: &str, args: &[String]) -> Result<HooksRequest, CliError> {
    let (yes, tokens) = split_yes_flag(args);
    match command {
        "hooks" => parse_hooks_subcommand(&tokens, yes),
        "setup-hooks" => parse_setup_tokens(&tokens, yes),
        "uninstall-hooks" => Err(unsupported_hooks_command("uninstall-hooks")),
        other => Err(CliError::new(format!(
            "unsupported hooks installer command '{other}'"
        ))),
    }
}

fn parse_hooks_subcommand(tokens: &[String], yes: bool) -> Result<HooksRequest, CliError> {
    match tokens.first().map(String::as_str) {
        Some("claude") | Some("claude-code") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::InstallClaude { yes }),
            Some("uninstall") => Err(unsupported_hooks_command("hooks claude uninstall")),
            Some(other) => Err(CliError::new(format!(
                "unsupported Claude hooks action '{other}'; use 'cmux hooks claude install'"
            ))),
        },
        Some("setup") => parse_setup_tokens(&tokens[1..], yes),
        Some("uninstall") => Err(unsupported_hooks_command("hooks uninstall")),
        Some(other) => Err(CliError::new(format!(
            "only Claude Code hook installation is available in this Windows port slice; \
             use 'cmux hooks claude install' instead of 'cmux hooks {other}'"
        ))),
        None => Err(CliError::new("Usage: cmux hooks claude install [--yes|-y]")),
    }
}

fn parse_setup_tokens(tokens: &[String], yes: bool) -> Result<HooksRequest, CliError> {
    let mut agent: Option<&str> = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--agent" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(CliError::new("missing value for --agent"));
                };
                agent = Some(value.as_str());
            }
            value if value.starts_with("--agent=") => {
                agent = Some(value.trim_start_matches("--agent="));
            }
            value if value.starts_with('-') => {
                return Err(CliError::new(format!(
                    "unsupported hooks setup option '{value}'"
                )));
            }
            value => {
                agent = Some(value);
            }
        }
        index += 1;
    }

    match agent {
        Some("claude") | Some("claude-code") => Ok(HooksRequest::InstallClaude { yes }),
        Some(other) => Err(CliError::new(format!(
            "only Claude Code hook installation is available in this Windows port slice; \
             '{other}' is not installed by this command yet"
        ))),
        None => Err(CliError::new(
            "cmux hooks setup requires --agent claude in the Windows port today",
        )),
    }
}

fn split_yes_flag(args: &[String]) -> (bool, Vec<String>) {
    let mut yes = false;
    let mut tokens = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--yes" | "-y" => yes = true,
            _ => tokens.push(arg.clone()),
        }
    }
    (yes, tokens)
}

fn unsupported_hooks_command(label: &str) -> CliError {
    CliError::new(format!(
        "'{label}' is not yet available in the Windows port; use \
         'cmux hooks claude install' to install the Claude Code integration"
    ))
}

fn install_claude_code_integration(yes: bool) -> Result<String, CliError> {
    let path = cmux_config::config_path()
        .ok_or_else(|| CliError::new("unable to determine cmux config path"))?;
    let before = read_config_or_empty_object(&path)?;
    let plan = plan_claude_code_integration_update(&before, &path)?;

    let mut output = String::new();
    output.push_str("Claude Code integration installer\n");
    output.push_str(&format!("Config file: {}\n\n", path.display()));

    if !plan.changed {
        output.push_str("automation.claudeCodeIntegration is already enabled.\n");
        return Ok(output);
    }

    output.push_str(&plan.diff);
    output.push('\n');

    if !yes {
        print!("{output}");
        print!("Type y to apply this change: ");
        io::stdout()
            .flush()
            .map_err(|error| CliError::new(format!("failed to flush stdout: {error}")))?;

        let mut answer = String::new();
        io::stdin()
            .read_line(&mut answer)
            .map_err(|error| CliError::new(format!("failed to read confirmation: {error}")))?;
        if !matches!(answer.trim(), "y" | "Y") {
            return Ok("Cancelled. No files were changed.\n".to_owned());
        }
        output.clear();
    }

    write_config(&path, &plan.after)?;
    output.push_str("Claude Code integration enabled.\n");
    output.push_str(
        "cmux terminals will route Claude Code through the bundled wrapper so hooks can report status, notifications, and resumable sessions.\n",
    );
    Ok(output)
}

pub fn plan_claude_code_integration_update(
    before: &str,
    path: &Path,
) -> Result<ClaudeIntegrationPlan, CliError> {
    let before = normalize_config_text(before)?;
    let mut raw: serde_json::Value = serde_json::from_str(&before)
        .map_err(|error| CliError::new(format!("failed to parse cmux.json: {error}")))?;
    let object = raw
        .as_object_mut()
        .ok_or_else(|| CliError::new("cmux config must be a JSON object"))?;
    let automation = object
        .entry("automation")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let automation = automation
        .as_object_mut()
        .ok_or_else(|| CliError::new("cmux config key 'automation' must be an object"))?;

    let changed = automation.get("claudeCodeIntegration") != Some(&serde_json::Value::Bool(true));
    automation.insert(
        "claudeCodeIntegration".to_owned(),
        serde_json::Value::Bool(true),
    );

    let after = serde_json::to_string_pretty(&raw)
        .map_err(|error| CliError::new(format!("failed to encode cmux.json: {error}")))?;
    cmux_config::decode_config(&after)
        .map_err(|error| CliError::new(format!("updated cmux.json did not validate: {error}")))?;

    Ok(ClaudeIntegrationPlan {
        diff: unified_diff(path, &before, &after),
        before,
        after,
        changed,
    })
}

fn normalize_config_text(text: &str) -> Result<String, CliError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Ok("{}".to_owned());
    }
    let value: serde_json::Value = serde_json::from_str(trimmed)
        .map_err(|error| CliError::new(format!("failed to parse cmux.json: {error}")))?;
    serde_json::to_string_pretty(&value)
        .map_err(|error| CliError::new(format!("failed to normalize cmux.json: {error}")))
}

fn read_config_or_empty_object(path: &Path) -> Result<String, CliError> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok("{}".to_owned()),
        Err(error) => Err(CliError::new(format!(
            "failed to read {}: {error}",
            path.display()
        ))),
    }
}

fn write_config(path: &Path, contents: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CliError::new(format!(
                "failed to create config directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    fs::write(path, format!("{contents}\n"))
        .map_err(|error| CliError::new(format!("failed to write {}: {error}", path.display())))
}

fn unified_diff(path: &Path, before: &str, after: &str) -> String {
    if before == after {
        return format!(
            "--- {}\n+++ {}\n(no changes)\n",
            path.display(),
            path.display()
        );
    }

    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let mut prefix = 0;
    while prefix < before_lines.len()
        && prefix < after_lines.len()
        && before_lines[prefix] == after_lines[prefix]
    {
        prefix += 1;
    }

    let mut suffix = 0;
    while suffix + prefix < before_lines.len()
        && suffix + prefix < after_lines.len()
        && before_lines[before_lines.len() - 1 - suffix]
            == after_lines[after_lines.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let before_end = before_lines.len().saturating_sub(suffix);
    let after_end = after_lines.len().saturating_sub(suffix);
    let context_start = prefix.saturating_sub(2);
    let before_context_end = (before_end + 2).min(before_lines.len());
    let after_context_end = (after_end + 2).min(after_lines.len());

    let mut diff = format!("--- {}\n+++ {}\n", path.display(), path.display());
    diff.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        context_start + 1,
        before_context_end.saturating_sub(context_start).max(1),
        context_start + 1,
        after_context_end.saturating_sub(context_start).max(1)
    ));

    for line in &before_lines[context_start..prefix] {
        diff.push_str(&format!(" {line}\n"));
    }
    for line in &before_lines[prefix..before_end] {
        diff.push_str(&format!("-{line}\n"));
    }
    for line in &after_lines[prefix..after_end] {
        diff.push_str(&format!("+{line}\n"));
    }
    for line in &after_lines[after_end..after_context_end] {
        diff.push_str(&format!(" {line}\n"));
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn path() -> PathBuf {
        PathBuf::from("C:/Users/me/AppData/Roaming/cmux/cmux.json")
    }

    #[test]
    fn plans_claude_integration_insert_into_empty_config() {
        let plan = plan_claude_code_integration_update("{}", &path()).expect("plan");
        assert!(plan.changed);
        assert!(plan.after.contains("\"automation\""));
        assert!(plan.after.contains("\"claudeCodeIntegration\": true"));
        assert!(plan
            .diff
            .contains("--- C:/Users/me/AppData/Roaming/cmux/cmux.json"));
        assert!(plan.diff.contains("+  \"automation\": {"));
    }

    #[test]
    fn plans_claude_integration_preserves_existing_keys() {
        let plan = plan_claude_code_integration_update(
            r#"{"app":{"appearance":"dark"},"automation":{"workspaceAutoNaming":true,"claudeCodeIntegration":false}}"#,
            &path(),
        )
        .expect("plan");
        let value: serde_json::Value = serde_json::from_str(&plan.after).expect("json");
        assert_eq!(value["app"]["appearance"], "dark");
        assert_eq!(value["automation"]["workspaceAutoNaming"], true);
        assert_eq!(value["automation"]["claudeCodeIntegration"], true);
    }

    #[test]
    fn already_enabled_plan_reports_no_change() {
        let plan = plan_claude_code_integration_update(
            r#"{"automation":{"claudeCodeIntegration":true}}"#,
            &path(),
        )
        .expect("plan");
        assert!(!plan.changed);
        assert!(plan.diff.contains("(no changes)"));
    }

    #[test]
    fn parses_supported_claude_install_spellings() {
        for (command, args) in [
            ("hooks", vec!["claude", "install"]),
            ("hooks", vec!["setup", "--agent", "claude"]),
            ("hooks", vec!["setup", "claude", "--yes"]),
            ("setup-hooks", vec!["claude", "-y"]),
        ] {
            let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(
                parse_hooks_request(command, &args).expect("request"),
                HooksRequest::InstallClaude {
                    yes: args.iter().any(|arg| arg == "--yes" || arg == "-y")
                }
            );
        }
    }
}
