//! Local agent-hook installer commands for the Windows CLI.
//!
//! The broad `cmux hooks setup` contract covers many agents. This module starts
//! with the Claude Code integration path because the Windows desktop menu needs a
//! concrete CLI target that previews the config diff and asks for confirmation.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

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
    Claude { yes: bool },
    Kiro { yes: bool },
    Nested { agent: String, yes: bool },
    Cursor { yes: bool },
    Antigravity { yes: bool },
    OpenCode { yes: bool, project: bool },
}

const OPENCODE_SESSION_PLUGIN_SOURCE: &str =
    include_str!("../../../Resources/opencode-session-plugin.js");
const OPENCODE_FEED_PLUGIN_SOURCE: &str = include_str!("../../../Resources/opencode-plugin.js");
const OPENCODE_SESSION_PLUGIN_SPEC: &str = "./plugins/cmux-session.js";
const OPENCODE_SESSION_PLUGIN_MARKER: &str = "cmux-opencode-session-plugin-marker";
const OPENCODE_FEED_PLUGIN_MARKER: &str = "cmux-feed-plugin-marker";

#[derive(Debug, Clone, Copy)]
struct NestedAgentDef {
    name: &'static str,
    display_name: &'static str,
    config_dir: &'static str,
    config_file: &'static str,
    env_override: Option<&'static str>,
    env_subdir: Option<&'static str>,
    lifecycle_timeout: u64,
    feed_timeout: u64,
    events: &'static [(&'static str, &'static str)],
    feed_events: &'static [&'static str],
}

pub fn run_hooks_command(command: &str, args: &[String]) -> Result<String, CliError> {
    match parse_hooks_request(command, args)? {
        HooksRequest::Claude { yes } => install_claude_code_integration(yes),
        HooksRequest::Kiro { yes } => install_kiro_hooks(yes),
        HooksRequest::Nested { agent, yes } => {
            install_nested_hooks(nested_agent(&agent).expect("parsed nested agent"), yes)
        }
        HooksRequest::Cursor { yes } => install_cursor_hooks(yes),
        HooksRequest::Antigravity { yes } => install_antigravity_hooks(yes),
        HooksRequest::OpenCode { yes, project } => install_opencode_hooks(yes, project),
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
            None | Some("install") | Some("setup") => Ok(HooksRequest::Claude { yes }),
            Some("uninstall") => Err(unsupported_hooks_command("hooks claude uninstall")),
            Some(other) => Err(CliError::new(format!(
                "unsupported Claude hooks action '{other}'; use 'cmux hooks claude install'"
            ))),
        },
        Some("kiro") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Kiro { yes }),
            Some(other) => Err(CliError::new(format!(
                "unsupported Kiro hooks action '{other}'; use 'cmux hooks kiro install'"
            ))),
        },
        Some("cursor") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Cursor { yes }),
            Some(other) => Err(CliError::new(format!(
                "unsupported Cursor hooks action '{other}'; use 'cmux hooks cursor install'"
            ))),
        },
        Some("antigravity") | Some("agy") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Antigravity { yes }),
            Some(other) => Err(CliError::new(format!(
                "unsupported Antigravity hooks action '{other}'; use 'cmux hooks antigravity install'"
            ))),
        },
        Some("opencode") => parse_opencode_tokens(&tokens[1..], yes),
        Some(agent) if nested_agent(agent).is_some() => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Nested {
                agent: agent.to_string(),
                yes,
            }),
            Some(other) => Err(CliError::new(format!(
                "unsupported {agent} hooks action '{other}'; use 'cmux hooks {agent} install'"
            ))),
        },
        Some("setup") => parse_setup_tokens(&tokens[1..], yes),
        Some("uninstall") => Err(unsupported_hooks_command("hooks uninstall")),
        Some(other) => Err(CliError::new(format!(
            "hook installation for '{other}' is not available yet"
        ))),
        None => Err(CliError::new("Usage: cmux hooks AGENT install [--yes|-y]")),
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
        Some("claude") | Some("claude-code") => Ok(HooksRequest::Claude { yes }),
        Some("kiro") => Ok(HooksRequest::Kiro { yes }),
        Some("cursor") => Ok(HooksRequest::Cursor { yes }),
        Some("antigravity") | Some("agy") => Ok(HooksRequest::Antigravity { yes }),
        Some("opencode") => Ok(HooksRequest::OpenCode {
            yes,
            project: false,
        }),
        Some(agent) if nested_agent(agent).is_some() => Ok(HooksRequest::Nested {
            agent: agent.to_string(),
            yes,
        }),
        Some(other) => Err(CliError::new(format!(
            "hook installation for '{other}' is not available yet"
        ))),
        None => Err(CliError::new("cmux hooks setup requires --agent AGENT")),
    }
}

fn parse_opencode_tokens(tokens: &[String], yes: bool) -> Result<HooksRequest, CliError> {
    let mut project = false;
    for token in tokens {
        match token.as_str() {
            "install" | "setup" => {}
            "--project" => project = true,
            "uninstall" => return Err(unsupported_hooks_command("hooks opencode uninstall")),
            other => {
                return Err(CliError::new(format!(
                    "unsupported OpenCode hooks option '{other}'; use 'cmux hooks opencode install [--project]'"
                )))
            }
        }
    }
    Ok(HooksRequest::OpenCode { yes, project })
}

fn install_opencode_hooks(yes: bool, project: bool) -> Result<String, CliError> {
    let config_dir = opencode_config_dir()?;
    let plugin_dir = if project {
        std::env::current_dir()
            .map_err(|error| {
                CliError::new(format!("failed to resolve current directory: {error}"))
            })?
            .join(".opencode")
            .join("plugins")
    } else {
        config_dir.join("plugins")
    };
    let feed_path = plugin_dir.join("cmux-feed.js");
    let feed_before = read_optional_text(&feed_path)?;
    ensure_cmux_plugin(&feed_path, &feed_before, OPENCODE_FEED_PLUGIN_MARKER)?;

    let mut preview = unified_diff(&feed_path, &feed_before, OPENCODE_FEED_PLUGIN_SOURCE);
    let mut registration = None;
    let mut session_before = String::new();
    let session_path = config_dir.join("plugins").join("cmux-session.js");
    if !project {
        session_before = read_optional_text(&session_path)?;
        ensure_cmux_plugin(
            &session_path,
            &session_before,
            OPENCODE_SESSION_PLUGIN_MARKER,
        )?;
        let config_path = config_dir.join("opencode.json");
        let config_before = read_config_or_empty_object(&config_path)?;
        let plan = plan_opencode_registration_update(&config_before, &config_path)?;
        preview.push_str(&unified_diff(
            &session_path,
            &session_before,
            OPENCODE_SESSION_PLUGIN_SOURCE,
        ));
        preview.push_str(&plan.diff);
        registration = Some((config_path, plan));
    }

    let feed_changed = feed_before != OPENCODE_FEED_PLUGIN_SOURCE;
    let session_changed = !project && session_before != OPENCODE_SESSION_PLUGIN_SOURCE;
    let registration_changed = registration.as_ref().is_some_and(|(_, plan)| plan.changed);
    if !feed_changed && !session_changed && !registration_changed {
        return Ok(format!(
            "OpenCode hooks already up to date at {}\n",
            feed_path.display()
        ));
    }
    if !confirm_hook_change(&preview, yes)? {
        return Ok("Aborted.\n".to_string());
    }

    if session_changed {
        write_text_exact(&session_path, OPENCODE_SESSION_PLUGIN_SOURCE)?;
    }
    if let Some((config_path, plan)) = registration {
        if plan.changed {
            write_config(&config_path, &plan.after)?;
        }
    }
    if feed_changed {
        write_text_exact(&feed_path, OPENCODE_FEED_PLUGIN_SOURCE)?;
    }

    if project {
        Ok(format!(
            "OpenCode plugin installed at {}\n",
            feed_path.display()
        ))
    } else {
        Ok(format!(
            "OpenCode hooks installed at {}\nOpenCode plugin installed at {}\n",
            session_path.display(),
            feed_path.display()
        ))
    }
}

fn opencode_config_dir() -> Result<PathBuf, CliError> {
    if let Some(path) = std::env::var_os("OPENCODE_CONFIG_DIR").filter(|path| !path.is_empty()) {
        return expand_home_path(PathBuf::from(path));
    }
    home_dir()
        .map(|home| home.join(".config").join("opencode"))
        .ok_or_else(|| CliError::new("unable to determine OpenCode config directory"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn expand_home_path(path: PathBuf) -> Result<PathBuf, CliError> {
    let text = path.to_string_lossy();
    if text == "~" {
        return home_dir()
            .ok_or_else(|| CliError::new("unable to expand OpenCode config directory"));
    }
    if let Some(rest) = text.strip_prefix("~/").or_else(|| text.strip_prefix("~\\")) {
        return home_dir()
            .map(|home| home.join(rest))
            .ok_or_else(|| CliError::new("unable to expand OpenCode config directory"));
    }
    Ok(path)
}

fn plan_opencode_registration_update(
    before: &str,
    path: &Path,
) -> Result<ClaudeIntegrationPlan, CliError> {
    let before = normalize_config_text(before)?;
    let mut value: serde_json::Value = serde_json::from_str(&before)
        .map_err(|error| CliError::new(format!("failed to parse OpenCode config: {error}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| CliError::new("OpenCode config must be a JSON object"))?;
    let plugins = object
        .get("plugin")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut plugins: Vec<_> = plugins
        .into_iter()
        .filter(|entry| !opencode_session_plugin_entry(entry))
        .collect();
    plugins.push(serde_json::json!(OPENCODE_SESSION_PLUGIN_SPEC));
    object.insert("plugin".to_string(), serde_json::Value::Array(plugins));
    let after = serde_json::to_string_pretty(&value)
        .map_err(|error| CliError::new(format!("failed to encode OpenCode config: {error}")))?;
    Ok(ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    })
}

fn opencode_session_plugin_entry(entry: &serde_json::Value) -> bool {
    let value = entry
        .as_str()
        .or_else(|| entry.as_array()?.first()?.as_str());
    value.is_some_and(|value| {
        value == OPENCODE_SESSION_PLUGIN_SPEC
            || value == "cmux-session"
            || value.ends_with("/plugins/cmux-session.js")
            || value.ends_with("/cmux-session.js")
    })
}

fn read_optional_text(path: &Path) -> Result<String, CliError> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(contents),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(CliError::new(format!(
            "failed to read {}: {error}",
            path.display()
        ))),
    }
}

fn ensure_cmux_plugin(path: &Path, existing: &str, marker: &str) -> Result<(), CliError> {
    if !existing.is_empty() && !existing.contains(marker) {
        return Err(CliError::new(format!(
            "{} exists and is not a cmux plugin; leaving it alone",
            path.display()
        )));
    }
    Ok(())
}

fn write_text_exact(path: &Path, contents: &str) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            CliError::new(format!(
                "failed to create config directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    fs::write(path, contents)
        .map_err(|error| CliError::new(format!("failed to write {}: {error}", path.display())))
}

fn install_antigravity_hooks(yes: bool) -> Result<String, CliError> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| CliError::new("unable to determine Antigravity config directory"))?;
    let path = home.join(".gemini").join("config").join("hooks.json");
    let before = read_config_or_empty_object(&path)?;
    let plan = plan_antigravity_hooks_update(&before, &path)?;
    if !plan.changed {
        return Ok(format!(
            "Antigravity hooks already up to date at {}\n",
            path.display()
        ));
    }
    if !confirm_hook_change(&plan.diff, yes)? {
        return Ok("Aborted.\n".to_string());
    }
    write_config(&path, &plan.after)?;
    Ok(format!(
        "Antigravity hooks installed at {}\n",
        path.display()
    ))
}

fn plan_antigravity_hooks_update(
    before: &str,
    path: &Path,
) -> Result<ClaudeIntegrationPlan, CliError> {
    let before = normalize_config_text(before)?;
    let mut value: serde_json::Value = serde_json::from_str(&before)
        .map_err(|error| CliError::new(format!("failed to parse Antigravity config: {error}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| CliError::new("Antigravity config must be a JSON object"))?;
    let mut group = serde_json::Map::new();
    for (event, action) in [
        ("SessionStart", "session-start"),
        ("PreInvocation", "prompt-submit"),
        ("Stop", "stop"),
        ("turn-completion", "stop"),
        ("Notification", "notification"),
        ("SessionEnd", "session-end"),
    ] {
        group.insert(
            event.to_string(),
            serde_json::json!([{
                "type":"command",
                "command":format!("cmux hooks antigravity {action}"),
                "timeout":10
            }]),
        );
    }
    for event in ["PreToolUse", "PostToolUse"] {
        group.insert(
            event.to_string(),
            serde_json::json!([{
                "matcher":"*",
                "hooks":[{
                    "type":"command",
                    "command":format!("cmux hooks feed --source antigravity --event {event}"),
                    "timeout":120
                }]
            }]),
        );
    }
    object.insert("cmux".to_string(), serde_json::Value::Object(group));
    let after = serde_json::to_string_pretty(&value)
        .map_err(|error| CliError::new(format!("failed to encode Antigravity config: {error}")))?;
    Ok(ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    })
}

fn install_cursor_hooks(yes: bool) -> Result<String, CliError> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| CliError::new("unable to determine Cursor config directory"))?;
    let path = home.join(".cursor").join("hooks.json");
    let before = read_config_or_empty_object(&path)?;
    let plan = plan_cursor_hooks_update(&before, &path)?;
    if !plan.changed {
        return Ok(format!(
            "Cursor hooks already up to date at {}\n",
            path.display()
        ));
    }
    if !confirm_hook_change(&plan.diff, yes)? {
        return Ok("Cancelled. No files were changed.\n".to_string());
    }
    write_config(&path, &plan.after)?;
    Ok(format!("Cursor hooks installed at {}\n", path.display()))
}

fn plan_cursor_hooks_update(before: &str, path: &Path) -> Result<ClaudeIntegrationPlan, CliError> {
    let before = normalize_config_text(before)?;
    let mut value: serde_json::Value = serde_json::from_str(&before)
        .map_err(|error| CliError::new(format!("failed to parse Cursor config: {error}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| CliError::new("Cursor config must be a JSON object"))?;
    object.insert("version".to_string(), serde_json::json!(1));
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| CliError::new("Cursor config key 'hooks' must be an object"))?;
    let mut prepared_events = HashSet::new();
    for (event, command) in [
        ("beforeSubmitPrompt", "cmux hooks cursor prompt-submit"),
        ("stop", "cmux hooks cursor stop"),
        ("afterAgentResponse", "cmux hooks cursor agent-response"),
        ("beforeShellExecution", "cmux hooks cursor shell-exec"),
        ("afterShellExecution", "cmux hooks cursor shell-done"),
        (
            "beforeShellExecution",
            "cmux hooks feed --source cursor --event beforeShellExecution",
        ),
    ] {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| CliError::new(format!("Cursor hook '{event}' must be an array")))?;
        if prepared_events.insert(event) {
            entries.retain(|entry| {
                !entry
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|command| {
                        command.contains("cmux hooks cursor")
                            || command.contains("hooks feed --source cursor")
                    })
            });
        }
        entries.push(serde_json::json!({"command":command}));
    }
    let after = serde_json::to_string_pretty(&value)
        .map_err(|error| CliError::new(format!("failed to encode Cursor config: {error}")))?;
    Ok(ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    })
}

fn nested_agent(name: &str) -> Option<&'static NestedAgentDef> {
    static AGENTS: &[NestedAgentDef] = &[
        NestedAgentDef {
            name: "gemini",
            display_name: "Gemini",
            config_dir: ".gemini",
            config_file: "settings.json",
            env_override: None,
            env_subdir: None,
            lifecycle_timeout: 10_000,
            feed_timeout: 120_000,
            events: &[
                ("SessionStart", "session-start"),
                ("BeforeAgent", "prompt-submit"),
                ("AfterAgent", "stop"),
                ("SessionEnd", "session-end"),
            ],
            feed_events: &["PreToolUse"],
        },
        NestedAgentDef {
            name: "grok",
            display_name: "Grok",
            config_dir: ".grok/hooks",
            config_file: "cmux-session.json",
            env_override: Some("GROK_HOME"),
            env_subdir: Some("hooks"),
            lifecycle_timeout: 5,
            feed_timeout: 120,
            events: &[
                ("SessionStart", "session-start"),
                ("UserPromptSubmit", "prompt-submit"),
                ("Stop", "stop"),
                ("Notification", "notification"),
                ("SessionEnd", "session-end"),
            ],
            feed_events: &["PreToolUse"],
        },
        NestedAgentDef {
            name: "copilot",
            display_name: "Copilot",
            config_dir: ".copilot",
            config_file: "config.json",
            env_override: Some("COPILOT_HOME"),
            env_subdir: None,
            lifecycle_timeout: 5_000,
            feed_timeout: 120_000,
            events: &[
                ("SessionStart", "session-start"),
                ("Stop", "stop"),
                ("Notification", "stop"),
                ("SessionEnd", "session-end"),
            ],
            feed_events: &["PreToolUse"],
        },
        NestedAgentDef {
            name: "codebuddy",
            display_name: "CodeBuddy",
            config_dir: ".codebuddy",
            config_file: "settings.json",
            env_override: Some("CODEBUDDY_CONFIG_DIR"),
            env_subdir: None,
            lifecycle_timeout: 5_000,
            feed_timeout: 120_000,
            events: &[
                ("SessionStart", "session-start"),
                ("Stop", "stop"),
                ("Notification", "stop"),
                ("SessionEnd", "session-end"),
            ],
            feed_events: &["PreToolUse"],
        },
        NestedAgentDef {
            name: "factory",
            display_name: "Factory",
            config_dir: ".factory",
            config_file: "settings.json",
            env_override: None,
            env_subdir: None,
            lifecycle_timeout: 5_000,
            feed_timeout: 120_000,
            events: &[
                ("SessionStart", "session-start"),
                ("Stop", "stop"),
                ("Notification", "stop"),
                ("SessionEnd", "session-end"),
            ],
            feed_events: &["PreToolUse"],
        },
        NestedAgentDef {
            name: "qoder",
            display_name: "Qoder",
            config_dir: ".qoder",
            config_file: "settings.json",
            env_override: Some("QODER_CONFIG_DIR"),
            env_subdir: None,
            lifecycle_timeout: 5_000,
            feed_timeout: 120_000,
            events: &[
                ("SessionStart", "session-start"),
                ("Stop", "stop"),
                ("SessionEnd", "session-end"),
            ],
            feed_events: &["PreToolUse"],
        },
    ];
    AGENTS.iter().find(|agent| agent.name == name)
}

fn install_nested_hooks(agent: &NestedAgentDef, yes: bool) -> Result<String, CliError> {
    let path = nested_hooks_path(agent)?;
    let before = read_config_or_empty_object(&path)?;
    let plan = plan_nested_hooks_update(&before, &path, agent)?;
    if !plan.changed {
        return Ok(format!(
            "{} hooks already up to date at {}\n",
            agent.display_name,
            path.display()
        ));
    }
    if !confirm_hook_change(&plan.diff, yes)? {
        return Ok("Cancelled. No files were changed.\n".to_string());
    }
    write_config(&path, &plan.after)?;
    Ok(format!(
        "{} hooks installed at {}\n",
        agent.display_name,
        path.display()
    ))
}

fn nested_hooks_path(agent: &NestedAgentDef) -> Result<PathBuf, CliError> {
    let home = || {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    };
    let override_directory = agent
        .env_override
        .and_then(std::env::var_os)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let used_override = override_directory.is_some();
    let mut directory = match override_directory {
        Some(directory) => directory,
        None => home()
            .ok_or_else(|| {
                CliError::new(format!(
                    "unable to determine {} config directory",
                    agent.display_name
                ))
            })?
            .join(agent.config_dir),
    };
    if used_override {
        if let Some(subdir) = agent.env_subdir {
            directory.push(subdir);
        }
    }
    Ok(directory.join(agent.config_file))
}

fn plan_nested_hooks_update(
    before: &str,
    path: &Path,
    agent: &NestedAgentDef,
) -> Result<ClaudeIntegrationPlan, CliError> {
    let before = normalize_config_text(before)?;
    let mut value: serde_json::Value = serde_json::from_str(&before).map_err(|error| {
        CliError::new(format!(
            "failed to parse {} config: {error}",
            agent.display_name
        ))
    })?;
    let object = value.as_object_mut().ok_or_else(|| {
        CliError::new(format!(
            "{} config must be a JSON object",
            agent.display_name
        ))
    })?;
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            CliError::new(format!(
                "{} config key 'hooks' must be an object",
                agent.display_name
            ))
        })?;
    for (event, action, timeout, feed) in agent
        .events
        .iter()
        .map(|(event, action)| (*event, *action, agent.lifecycle_timeout, false))
        .chain(
            agent
                .feed_events
                .iter()
                .map(|event| (*event, *event, agent.feed_timeout, true)),
        )
    {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| {
                CliError::new(format!(
                    "{} hook '{event}' must be an array",
                    agent.display_name
                ))
            })?;
        groups.retain(|group| !nested_group_is_owned(group, agent.name));
        let command = if feed {
            format!("cmux hooks feed --source {} --event {event}", agent.name)
        } else {
            format!("cmux hooks {} {action}", agent.name)
        };
        groups.push(serde_json::json!({
            "hooks":[{"type":"command","command":command,"timeout":timeout}]
        }));
    }
    let after = serde_json::to_string_pretty(&value).map_err(|error| {
        CliError::new(format!(
            "failed to encode {} config: {error}",
            agent.display_name
        ))
    })?;
    Ok(ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    })
}

fn nested_group_is_owned(group: &serde_json::Value, agent: &str) -> bool {
    group
        .get("hooks")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hook| hook.get("command").and_then(serde_json::Value::as_str))
        .any(|command| {
            command.contains(&format!("cmux hooks {agent}"))
                || command.contains(&format!("hooks feed --source {agent}"))
        })
}

fn confirm_hook_change(diff: &str, yes: bool) -> Result<bool, CliError> {
    if yes {
        return Ok(true);
    }
    print!("{diff}\nType y to apply this change: ");
    io::stdout()
        .flush()
        .map_err(|error| CliError::new(format!("failed to flush stdout: {error}")))?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|error| CliError::new(format!("failed to read confirmation: {error}")))?;
    Ok(matches!(answer.trim(), "y" | "Y"))
}

fn install_kiro_hooks(yes: bool) -> Result<String, CliError> {
    let path = kiro_hooks_path()?;
    let before = read_config_or_empty_object(&path)?;
    let plan = plan_kiro_hooks_update(&before, &path)?;
    if !plan.changed {
        return Ok(format!(
            "Kiro hooks already up to date at {}\n",
            path.display()
        ));
    }
    if !confirm_hook_change(&plan.diff, yes)? {
        return Ok("Cancelled. No files were changed.\n".to_string());
    }
    write_config(&path, &plan.after)?;
    Ok(format!(
        "Kiro hooks installed at {}\nKiro applies these hooks only when run as the cmux agent. Start Kiro with `kiro-cli chat --agent cmux`, or make it the default with `kiro-cli settings chat.defaultAgent cmux`.\n",
        path.display()
    ))
}

fn kiro_hooks_path() -> Result<PathBuf, CliError> {
    let base = std::env::var_os("KIRO_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(|home| PathBuf::from(home).join(".kiro"))
        })
        .ok_or_else(|| CliError::new("unable to determine Kiro config directory"))?;
    Ok(base.join("agents").join("cmux.json"))
}

pub fn plan_kiro_hooks_update(
    before: &str,
    path: &Path,
) -> Result<ClaudeIntegrationPlan, CliError> {
    let before = normalize_config_text(before)?;
    let mut value: serde_json::Value = serde_json::from_str(&before)
        .map_err(|error| CliError::new(format!("failed to parse Kiro config: {error}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| CliError::new("Kiro agent config must be a JSON object"))?;
    object
        .entry("name")
        .or_insert_with(|| serde_json::json!("cmux"));
    object.entry("description").or_insert_with(|| {
        serde_json::json!("CMUX notification and Feed bridge hooks for Kiro CLI.")
    });
    object
        .entry("tools")
        .or_insert_with(|| serde_json::json!(["*"]));
    let hooks = object
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| CliError::new("Kiro agent config key 'hooks' must be an object"))?;
    for (event, command, timeout) in [
        ("agentSpawn", "cmux hooks kiro session-start", 5_000),
        ("userPromptSubmit", "cmux hooks kiro prompt-submit", 5_000),
        ("stop", "cmux hooks kiro stop", 5_000),
        (
            "preToolUse",
            "cmux hooks feed --source kiro --event preToolUse",
            120_000,
        ),
        (
            "postToolUse",
            "cmux hooks feed --source kiro --event postToolUse",
            120_000,
        ),
    ] {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| CliError::new(format!("Kiro hook '{event}' must be an array")))?;
        entries.retain(|entry| {
            let command = entry.get("command").and_then(serde_json::Value::as_str);
            !command.is_some_and(|command| {
                command.contains("cmux hooks kiro") || command.contains("hooks feed --source kiro")
            })
        });
        entries.push(serde_json::json!({"command":command,"timeout_ms":timeout}));
    }
    let after = serde_json::to_string_pretty(&value)
        .map_err(|error| CliError::new(format!("failed to encode Kiro config: {error}")))?;
    Ok(ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    })
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
                HooksRequest::Claude {
                    yes: args.iter().any(|arg| arg == "--yes" || arg == "-y")
                }
            );
        }
    }

    #[test]
    fn parses_supported_kiro_install_spellings() {
        for (command, args) in [
            ("hooks", vec!["kiro", "install"]),
            ("hooks", vec!["setup", "--agent", "kiro", "--yes"]),
            ("setup-hooks", vec!["kiro", "-y"]),
        ] {
            let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(
                parse_hooks_request(command, &args).expect("request"),
                HooksRequest::Kiro {
                    yes: args.iter().any(|arg| arg == "--yes" || arg == "-y")
                }
            );
        }
    }

    #[test]
    fn kiro_plan_uses_custom_agent_shape_and_preserves_user_fields() {
        let plan = plan_kiro_hooks_update(
            r#"{"model":"claude-sonnet","tools":["fs_read"],"custom":true,"hooks":{"preToolUse":[{"command":"user-check","timeout_ms":42}]}}"#,
            &PathBuf::from("C:/Users/me/.kiro/agents/cmux.json"),
        )
        .expect("plan");
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["model"], "claude-sonnet");
        assert_eq!(value["tools"], serde_json::json!(["fs_read"]));
        assert_eq!(value["custom"], true);
        assert_eq!(value["name"], "cmux");
        assert!(value.get("version").is_none());
        assert_eq!(value["hooks"]["agentSpawn"][0]["timeout_ms"], 5_000);
        assert_eq!(value["hooks"]["preToolUse"][0]["timeout_ms"], 42);
        assert_eq!(value["hooks"]["preToolUse"][0]["command"], "user-check");
        assert_eq!(value["hooks"]["preToolUse"][1]["timeout_ms"], 120_000);
        assert!(value["hooks"]["preToolUse"][1]["command"]
            .as_str()
            .unwrap()
            .contains("hooks feed --source kiro --event preToolUse"));
        assert_eq!(value["hooks"]["postToolUse"][0]["timeout_ms"], 120_000);

        let fresh = plan_kiro_hooks_update("{}", &path()).unwrap();
        let fresh: serde_json::Value = serde_json::from_str(&fresh.after).unwrap();
        assert_eq!(fresh["tools"], serde_json::json!(["*"]));

        let repeated = plan_kiro_hooks_update(&plan.after, &path()).unwrap();
        assert!(!repeated.changed);
    }

    #[test]
    fn parses_nested_json_agent_install_spellings() {
        for agent in ["gemini", "grok", "copilot", "codebuddy", "factory", "qoder"] {
            let request = parse_hooks_request(
                "hooks",
                &[
                    agent.to_string(),
                    "install".to_string(),
                    "--yes".to_string(),
                ],
            )
            .unwrap();
            assert_eq!(
                request,
                HooksRequest::Nested {
                    agent: agent.to_string(),
                    yes: true,
                }
            );
        }
    }

    #[test]
    fn nested_json_plan_preserves_user_groups_and_agent_timeout_units() {
        let before = r#"{"theme":"dark","hooks":{"PreToolUse":[{"matcher":"user","hooks":[{"type":"command","command":"user-check","timeout":7}]}]}}"#;
        let gemini = nested_agent("gemini").unwrap();
        let plan = plan_nested_hooks_update(before, &path(), gemini).unwrap();
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["hooks"]["PreToolUse"][0]["matcher"], "user");
        assert_eq!(
            value["hooks"]["SessionStart"][0]["hooks"][0]["timeout"],
            10_000
        );
        assert_eq!(
            value["hooks"]["PreToolUse"][1]["hooks"][0]["timeout"],
            120_000
        );
        assert!(value["hooks"]["PreToolUse"][1]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("hooks feed --source gemini --event PreToolUse"));
        assert!(
            !plan_nested_hooks_update(&plan.after, &path(), gemini)
                .unwrap()
                .changed
        );

        let grok = nested_agent("grok").unwrap();
        let grok_plan = plan_nested_hooks_update("{}", &path(), grok).unwrap();
        let grok_value: serde_json::Value = serde_json::from_str(&grok_plan.after).unwrap();
        assert_eq!(
            grok_value["hooks"]["SessionStart"][0]["hooks"][0]["timeout"],
            5
        );
        assert_eq!(
            grok_value["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"],
            120
        );
    }

    #[test]
    fn cursor_flat_plan_preserves_user_hooks_and_sets_version() {
        let request = parse_hooks_request(
            "hooks",
            &["cursor".into(), "install".into(), "--yes".into()],
        )
        .unwrap();
        assert_eq!(request, HooksRequest::Cursor { yes: true });

        let plan = plan_cursor_hooks_update(
            r#"{"custom":true,"hooks":{"stop":[{"command":"user-stop"}]}}"#,
            &PathBuf::from("C:/Users/me/.cursor/hooks.json"),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["custom"], true);
        assert_eq!(value["version"], 1);
        assert_eq!(value["hooks"]["stop"][0]["command"], "user-stop");
        assert!(value["hooks"]["beforeSubmitPrompt"][0]["command"]
            .as_str()
            .unwrap()
            .contains("hooks cursor prompt-submit"));
        assert!(value["hooks"]["beforeShellExecution"][1]["command"]
            .as_str()
            .unwrap()
            .contains("hooks feed --source cursor --event beforeShellExecution"));
        assert!(
            !plan_cursor_hooks_update(&plan.after, &path())
                .unwrap()
                .changed
        );
    }

    #[test]
    fn antigravity_named_group_plan_preserves_other_groups_and_alias() {
        assert_eq!(
            parse_hooks_request("hooks", &["agy".into(), "install".into(), "--yes".into()])
                .unwrap(),
            HooksRequest::Antigravity { yes: true }
        );
        let plan = plan_antigravity_hooks_update(
            r#"{"other":{"PreToolUse":[{"command":"user-hook"}]},"cmux":{"old":true}}"#,
            &PathBuf::from("C:/Users/me/.gemini/config/hooks.json"),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["other"]["PreToolUse"][0]["command"], "user-hook");
        assert!(value["cmux"].get("old").is_none());
        assert_eq!(value["cmux"]["SessionStart"][0]["timeout"], 10);
        assert_eq!(value["cmux"]["PreToolUse"][0]["matcher"], "*");
        assert_eq!(value["cmux"]["PreToolUse"][0]["hooks"][0]["timeout"], 120);
        assert!(value["cmux"]["PostToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap()
            .contains("hooks feed --source antigravity --event PostToolUse"));
        assert!(
            !plan_antigravity_hooks_update(&plan.after, &path())
                .unwrap()
                .changed
        );
    }

    #[test]
    fn opencode_plan_registers_session_plugin_and_preserves_user_plugins() {
        assert_eq!(
            parse_hooks_request(
                "hooks",
                &[
                    "opencode".into(),
                    "install".into(),
                    "--project".into(),
                    "--yes".into(),
                ],
            )
            .unwrap(),
            HooksRequest::OpenCode {
                yes: true,
                project: true,
            }
        );
        assert_eq!(
            parse_hooks_request(
                "hooks",
                &[
                    "setup".into(),
                    "--agent".into(),
                    "opencode".into(),
                    "--yes".into(),
                ],
            )
            .unwrap(),
            HooksRequest::OpenCode {
                yes: true,
                project: false,
            }
        );
        let plan = plan_opencode_registration_update(
            r#"{"theme":"dark","plugin":["user-plugin",["tuple-plugin",{"flag":true}],"cmux-session","./plugins/cmux-session.js"]}"#,
            &PathBuf::from("C:/Users/me/.config/opencode/opencode.json"),
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["plugin"][0], "user-plugin");
        assert_eq!(value["plugin"][1][0], "tuple-plugin");
        assert_eq!(
            value["plugin"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|entry| entry.as_str() == Some("./plugins/cmux-session.js"))
                .count(),
            1
        );
        assert!(
            !plan_opencode_registration_update(&plan.after, &path())
                .unwrap()
                .changed
        );
        assert!(OPENCODE_SESSION_PLUGIN_SOURCE.contains("cmux-opencode-session-plugin-marker"));
        assert!(OPENCODE_FEED_PLUGIN_SOURCE.contains("cmux-feed-plugin-marker"));
    }
}
