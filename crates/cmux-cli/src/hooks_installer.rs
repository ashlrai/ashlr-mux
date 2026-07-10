//! Local agent-hook installer commands for the Windows CLI.
//!
//! The broad `cmux hooks setup` contract covers many agents. This module starts
//! with the Claude Code integration path because the Windows desktop menu needs a
//! concrete CLI target that previews the config diff and asks for confirmation.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use base64::Engine as _;
use sha2::{Digest, Sha256};

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
    UninstallAll,
    Claude { yes: bool },
    Kiro { yes: bool },
    KiroUninstall,
    Nested { agent: String, yes: bool },
    NestedUninstall { agent: String },
    Cursor { yes: bool },
    CursorUninstall,
    Antigravity { yes: bool },
    AntigravityUninstall,
    OpenCode { yes: bool, project: bool },
    OpenCodeUninstall { project: bool },
    Pi { yes: bool },
    PiUninstall,
    Omp { yes: bool },
    OmpUninstall,
    Amp { yes: bool },
    AmpUninstall,
    Rovo { yes: bool },
    RovoUninstall,
    Hermes { yes: bool },
    HermesUninstall,
    Kimi { yes: bool },
    KimiUninstall,
    Codex { yes: bool },
    CodexUninstall,
}

const OPENCODE_SESSION_PLUGIN_SOURCE: &str =
    include_str!("../../../Resources/opencode-session-plugin.js");
const OPENCODE_FEED_PLUGIN_SOURCE: &str = include_str!("../../../Resources/opencode-plugin.js");
const OPENCODE_SESSION_PLUGIN_SPEC: &str = "./plugins/cmux-session.js";
const OPENCODE_SESSION_PLUGIN_MARKER: &str = "cmux-opencode-session-plugin-marker";
const OPENCODE_FEED_PLUGIN_MARKER: &str = "cmux-feed-plugin-marker";
const PI_EXTENSION_SOURCE: &str = include_str!("../../../Resources/pi-session-extension.ts");
const PI_EXTENSION_MARKER: &str = "cmux-pi-session-extension-marker";
const OMP_EXTENSION_SOURCE: &str = include_str!("../../../Resources/omp-session-extension.ts");
const OMP_EXTENSION_MARKER: &str = "cmux-omp-session-extension-marker";
const AMP_PLUGIN_SOURCE: &str = include_str!("../../../Resources/amp-session-plugin.ts");
const AMP_PLUGIN_MARKER: &str = "cmux-amp-session-extension-marker";

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
        HooksRequest::UninstallAll => uninstall_all_hooks(),
        HooksRequest::Claude { yes } => install_claude_code_integration(yes),
        HooksRequest::Kiro { yes } => install_kiro_hooks(yes),
        HooksRequest::KiroUninstall => uninstall_kiro_hooks(),
        HooksRequest::Nested { agent, yes } => {
            install_nested_hooks(nested_agent(&agent).expect("parsed nested agent"), yes)
        }
        HooksRequest::NestedUninstall { agent } => {
            uninstall_nested_hooks(nested_agent(&agent).expect("parsed nested agent"))
        }
        HooksRequest::Cursor { yes } => install_cursor_hooks(yes),
        HooksRequest::CursorUninstall => uninstall_cursor_hooks(),
        HooksRequest::Antigravity { yes } => install_antigravity_hooks(yes),
        HooksRequest::AntigravityUninstall => uninstall_antigravity_hooks(),
        HooksRequest::OpenCode { yes, project } => install_opencode_hooks(yes, project),
        HooksRequest::OpenCodeUninstall { project } => uninstall_opencode_hooks(project),
        HooksRequest::Pi { yes } => install_pi_hooks(yes),
        HooksRequest::PiUninstall => uninstall_pi_hooks(),
        HooksRequest::Omp { yes } => install_omp_hooks(yes),
        HooksRequest::OmpUninstall => uninstall_omp_hooks(),
        HooksRequest::Amp { yes } => install_amp_hooks(yes),
        HooksRequest::AmpUninstall => uninstall_amp_hooks(),
        HooksRequest::Rovo { yes } => install_rovo_hooks(yes),
        HooksRequest::RovoUninstall => uninstall_rovo_hooks(),
        HooksRequest::Hermes { yes } => install_hermes_hooks(yes),
        HooksRequest::HermesUninstall => uninstall_hermes_hooks(),
        HooksRequest::Kimi { yes } => install_kimi_hooks(yes),
        HooksRequest::KimiUninstall => uninstall_kimi_hooks(),
        HooksRequest::Codex { yes } => install_codex_hooks(yes),
        HooksRequest::CodexUninstall => uninstall_codex_hooks(),
    }
}

fn parse_hooks_request(command: &str, args: &[String]) -> Result<HooksRequest, CliError> {
    let (yes, tokens) = split_yes_flag(args);
    match command {
        "hooks" => parse_hooks_subcommand(&tokens, yes),
        "setup-hooks" => parse_setup_tokens(&tokens, yes),
        "uninstall-hooks" => parse_uninstall_tokens(&tokens),
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
            Some("uninstall") => Ok(HooksRequest::KiroUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported Kiro hooks action '{other}'; use 'cmux hooks kiro install|uninstall'"
            ))),
        },
        Some("cursor") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Cursor { yes }),
            Some("uninstall") => Ok(HooksRequest::CursorUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported Cursor hooks action '{other}'; use 'cmux hooks cursor install|uninstall'"
            ))),
        },
        Some("antigravity") | Some("agy") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Antigravity { yes }),
            Some("uninstall") => Ok(HooksRequest::AntigravityUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported Antigravity hooks action '{other}'; use 'cmux hooks antigravity install|uninstall'"
            ))),
        },
        Some("opencode") => parse_opencode_tokens(&tokens[1..], yes),
        Some("pi") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Pi { yes }),
            Some("uninstall") => Ok(HooksRequest::PiUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported Pi hooks action '{other}'; use 'cmux hooks pi install|uninstall'"
            ))),
        },
        Some("omp") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Omp { yes }),
            Some("uninstall") => Ok(HooksRequest::OmpUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported OMP hooks action '{other}'; use 'cmux hooks omp install|uninstall'"
            ))),
        },
        Some("amp") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Amp { yes }),
            Some("uninstall") => Ok(HooksRequest::AmpUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported Amp hooks action '{other}'; use 'cmux hooks amp install|uninstall'"
            ))),
        },
        Some("rovodev") | Some("rovo") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Rovo { yes }),
            Some("uninstall") => Ok(HooksRequest::RovoUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported Rovo Dev hooks action '{other}'; use 'cmux hooks rovodev install|uninstall'"
            ))),
        },
        Some("hermes-agent") | Some("hermes") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Hermes { yes }),
            Some("uninstall") => Ok(HooksRequest::HermesUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported Hermes Agent hooks action '{other}'; use 'cmux hooks hermes-agent install|uninstall'"
            ))),
        },
        Some("kimi") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Kimi { yes }),
            Some("uninstall") => Ok(HooksRequest::KimiUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported Kimi Code hooks action '{other}'; use 'cmux hooks kimi install|uninstall'"
            ))),
        },
        Some("codex") => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Codex { yes }),
            Some("uninstall") => Ok(HooksRequest::CodexUninstall),
            Some(other) => Err(CliError::new(format!(
                "unsupported Codex hooks action '{other}'; use 'cmux hooks codex install|uninstall'"
            ))),
        },
        Some(agent) if nested_agent(agent).is_some() => match tokens.get(1).map(String::as_str) {
            None | Some("install") | Some("setup") => Ok(HooksRequest::Nested {
                agent: agent.to_string(),
                yes,
            }),
            Some("uninstall") => Ok(HooksRequest::NestedUninstall {
                agent: agent.to_string(),
            }),
            Some(other) => Err(CliError::new(format!(
                "unsupported {agent} hooks action '{other}'; use 'cmux hooks {agent} install|uninstall'"
            ))),
        },
        Some("setup") => parse_setup_tokens(&tokens[1..], yes),
        Some("uninstall") => parse_uninstall_tokens(&tokens[1..]),
        Some(other) => Err(CliError::new(format!(
            "hook installation for '{other}' is not available yet"
        ))),
        None => Err(CliError::new("Usage: cmux hooks AGENT install [--yes|-y]")),
    }
}

fn parse_setup_tokens(tokens: &[String], yes: bool) -> Result<HooksRequest, CliError> {
    let mut agent: Option<&str> = None;
    let mut uninstall = false;
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
            "--uninstall" => uninstall = true,
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

    if uninstall {
        return agent.map_or(Ok(HooksRequest::UninstallAll), uninstall_request);
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
        Some("pi") => Ok(HooksRequest::Pi { yes }),
        Some("omp") => Ok(HooksRequest::Omp { yes }),
        Some("amp") => Ok(HooksRequest::Amp { yes }),
        Some("rovodev") | Some("rovo") => Ok(HooksRequest::Rovo { yes }),
        Some("hermes-agent") | Some("hermes") => Ok(HooksRequest::Hermes { yes }),
        Some("kimi") => Ok(HooksRequest::Kimi { yes }),
        Some("codex") => Ok(HooksRequest::Codex { yes }),
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

fn parse_uninstall_tokens(tokens: &[String]) -> Result<HooksRequest, CliError> {
    let mut agent = None;
    let mut index = 0;
    while index < tokens.len() {
        match tokens[index].as_str() {
            "--agent" => {
                index += 1;
                agent = Some(
                    tokens
                        .get(index)
                        .ok_or_else(|| CliError::new("missing value for --agent"))?
                        .as_str(),
                );
            }
            value if value.starts_with("--agent=") => {
                agent = Some(value.trim_start_matches("--agent="));
            }
            "--yes" | "-y" | "--uninstall" => {}
            value if value.starts_with('-') => {
                return Err(CliError::new(format!(
                    "unsupported hooks uninstall option '{value}'"
                )))
            }
            value => agent = Some(value),
        }
        index += 1;
    }
    agent.map_or(Ok(HooksRequest::UninstallAll), uninstall_request)
}

fn uninstall_request(agent: &str) -> Result<HooksRequest, CliError> {
    Ok(match agent {
        "kiro" => HooksRequest::KiroUninstall,
        "cursor" => HooksRequest::CursorUninstall,
        "antigravity" | "agy" => HooksRequest::AntigravityUninstall,
        "opencode" => HooksRequest::OpenCodeUninstall { project: false },
        "pi" => HooksRequest::PiUninstall,
        "omp" => HooksRequest::OmpUninstall,
        "amp" => HooksRequest::AmpUninstall,
        "rovodev" | "rovo" => HooksRequest::RovoUninstall,
        "hermes-agent" | "hermes" => HooksRequest::HermesUninstall,
        "kimi" => HooksRequest::KimiUninstall,
        "codex" => HooksRequest::CodexUninstall,
        agent if nested_agent(agent).is_some() => HooksRequest::NestedUninstall {
            agent: agent.to_string(),
        },
        other => return Err(CliError::new(format!("Unknown hooks target: {other}"))),
    })
}

fn uninstall_all_hooks() -> Result<String, CliError> {
    let mut output = "cmux hooks uninstall: uninstalling agent hooks\n\n".to_string();
    for agent in [
        "kiro",
        "gemini",
        "grok",
        "copilot",
        "codebuddy",
        "factory",
        "qoder",
        "cursor",
        "antigravity",
        "opencode",
        "pi",
        "omp",
        "amp",
        "rovodev",
        "hermes-agent",
        "kimi",
        "codex",
    ] {
        output.push_str(&format!("  {agent}:\n"));
        output.push_str(&run_hooks_command(
            "hooks",
            &[agent.to_string(), "uninstall".to_string()],
        )?);
        output.push('\n');
    }
    output.push_str("Done: 17 uninstalled, 0 skipped\n");
    Ok(output)
}

fn parse_opencode_tokens(tokens: &[String], yes: bool) -> Result<HooksRequest, CliError> {
    let mut project = false;
    let mut uninstall = false;
    for token in tokens {
        match token.as_str() {
            "install" | "setup" => {}
            "--project" => project = true,
            "uninstall" => uninstall = true,
            other => {
                return Err(CliError::new(format!(
                    "unsupported OpenCode hooks option '{other}'; use 'cmux hooks opencode install|uninstall [--project]'"
                )))
            }
        }
    }
    if uninstall {
        Ok(HooksRequest::OpenCodeUninstall { project })
    } else {
        Ok(HooksRequest::OpenCode { yes, project })
    }
}

fn install_pi_hooks(yes: bool) -> Result<String, CliError> {
    let path = pi_extension_path(&pi_config_dir()?);
    install_marked_extension("Pi", &path, PI_EXTENSION_SOURCE, PI_EXTENSION_MARKER, yes)
}

fn uninstall_pi_hooks() -> Result<String, CliError> {
    let path = pi_extension_path(&pi_config_dir()?);
    remove_marked_extension("Pi", "extension", &path, PI_EXTENSION_MARKER)
}

fn pi_config_dir() -> Result<PathBuf, CliError> {
    if let Ok(raw) = std::env::var("PI_CODING_AGENT_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return expand_home_path(PathBuf::from(trimmed));
        }
    }
    home_dir()
        .map(|home| home.join(".pi").join("agent"))
        .ok_or_else(|| CliError::new("unable to determine Pi config directory"))
}

fn pi_extension_path(config_dir: &Path) -> PathBuf {
    config_dir.join("extensions").join("cmux-session.ts")
}

fn install_omp_hooks(yes: bool) -> Result<String, CliError> {
    let path = omp_extension_path(&omp_config_dir()?);
    install_marked_extension(
        "OMP",
        &path,
        OMP_EXTENSION_SOURCE,
        OMP_EXTENSION_MARKER,
        yes,
    )
}

fn uninstall_omp_hooks() -> Result<String, CliError> {
    let path = omp_extension_path(&omp_config_dir()?);
    remove_marked_extension("OMP", "extension", &path, OMP_EXTENSION_MARKER)
}

fn install_marked_extension(
    display_name: &str,
    path: &Path,
    source: &str,
    marker: &str,
    yes: bool,
) -> Result<String, CliError> {
    let before = read_optional_text(path)?;
    ensure_cmux_plugin(path, &before, marker)?;
    if before == source {
        return Ok(format!(
            "{display_name} hooks already up to date at {}\n",
            path.display()
        ));
    }
    let preview = unified_diff(path, &before, source);
    if !confirm_hook_change(&preview, yes)? {
        return Ok("Aborted.\n".to_string());
    }
    write_text_exact(path, source)?;
    Ok(format!(
        "{display_name} hooks installed at {}\n",
        path.display()
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkedRemoval {
    Missing,
    Refuse,
    Remove,
}

fn marked_removal(existing: Option<&str>, marker: &str) -> MarkedRemoval {
    match existing {
        None => MarkedRemoval::Missing,
        Some(contents) if contents.contains(marker) => MarkedRemoval::Remove,
        Some(_) => MarkedRemoval::Refuse,
    }
}

fn marked_removal_at_path(path: &Path, marker: &str) -> Result<MarkedRemoval, CliError> {
    match fs::read_to_string(path) {
        Ok(contents) => Ok(marked_removal(Some(&contents), marker)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(MarkedRemoval::Missing),
        Err(error) => Err(CliError::new(format!(
            "failed to read {}: {error}",
            path.display()
        ))),
    }
}

fn remove_marked_extension(
    display_name: &str,
    noun: &str,
    path: &Path,
    marker: &str,
) -> Result<String, CliError> {
    match marked_removal_at_path(path, marker)? {
        MarkedRemoval::Missing => Ok(format!(
            "No {display_name} cmux {noun} found at {}\n",
            path.display()
        )),
        MarkedRemoval::Refuse => Ok(format!(
            "Refusing to remove {}: missing cmux marker\n",
            path.display()
        )),
        MarkedRemoval::Remove => {
            fs::remove_file(path).map_err(|error| {
                CliError::new(format!("failed to remove {}: {error}", path.display()))
            })?;
            Ok(format!(
                "Removed {display_name} cmux {noun} from {}\n",
                path.display()
            ))
        }
    }
}

fn omp_config_dir() -> Result<PathBuf, CliError> {
    if let Some(agent_dir) = nonempty_env("PI_CODING_AGENT_DIR") {
        return expand_home_path(PathBuf::from(agent_dir));
    }
    let home = if let Some(home) = nonempty_env("HOME") {
        expand_home_path(PathBuf::from(home))?
    } else {
        home_dir().ok_or_else(|| CliError::new("unable to determine OMP config directory"))?
    };
    let config = nonempty_env("PI_CONFIG_DIR").unwrap_or_else(|| ".omp".to_string());
    let config = expand_home_path(PathBuf::from(config))?;
    let root = if config.is_absolute() {
        config
    } else {
        home.join(config)
    };
    Ok(root.join("agent"))
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn omp_extension_path(config_dir: &Path) -> PathBuf {
    config_dir.join("extensions").join("cmux-omp-session.ts")
}

fn install_amp_hooks(yes: bool) -> Result<String, CliError> {
    let path = amp_plugin_path(&amp_config_dir()?);
    install_marked_extension("Amp", &path, AMP_PLUGIN_SOURCE, AMP_PLUGIN_MARKER, yes)
}

fn uninstall_amp_hooks() -> Result<String, CliError> {
    let path = amp_plugin_path(&amp_config_dir()?);
    remove_marked_extension("Amp", "plugin", &path, AMP_PLUGIN_MARKER)
}

fn amp_config_dir() -> Result<PathBuf, CliError> {
    home_dir()
        .map(|home| home.join(".config").join("amp"))
        .ok_or_else(|| CliError::new("unable to determine Amp config directory"))
}

fn amp_plugin_path(config_dir: &Path) -> PathBuf {
    config_dir.join("plugins").join("cmux-session.ts")
}

const ROVO_BEGIN_MARKER: &str = "# cmux hooks rovodev begin";
const ROVO_END_MARKER: &str = "# cmux hooks rovodev end";

fn install_rovo_hooks(yes: bool) -> Result<String, CliError> {
    let path = rovo_hooks_path()?;
    let before = read_optional_text(&path)?;
    let plan = plan_rovo_hooks_update(&before, &path)?;
    if !plan.changed {
        return Ok(format!(
            "Rovo Dev hooks already up to date at {}\n",
            path.display()
        ));
    }
    if !confirm_hook_change(&plan.diff, yes)? {
        return Ok("Aborted.\n".to_string());
    }
    write_text_exact(&path, &plan.after)?;
    Ok(format!("Rovo Dev hooks installed at {}\n", path.display()))
}

fn uninstall_rovo_hooks() -> Result<String, CliError> {
    let path = rovo_hooks_path()?;
    let before = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(format!("No config.yml found at {}\n", path.display()))
        }
        Err(error) => {
            return Err(CliError::new(format!(
                "failed to read {}: {error}",
                path.display()
            )))
        }
    };
    let plan = plan_rovo_hooks_uninstall(&before, &path);
    if !plan.changed {
        return Ok(format!("Removed 0 cmux hook(s) from {}\n", path.display()));
    }
    write_text_exact(&path, &plan.after)?;
    Ok(format!(
        "Removed Rovo Dev cmux hooks from {}\n",
        path.display()
    ))
}

fn rovo_hooks_path() -> Result<PathBuf, CliError> {
    home_dir()
        .map(|home| home.join(".rovodev").join("config.yml"))
        .ok_or_else(|| CliError::new("unable to determine Rovo Dev config directory"))
}

fn plan_rovo_hooks_uninstall(before: &str, path: &Path) -> ClaudeIntegrationPlan {
    let before = normalize_yaml_text(before);
    let after = serialize_yaml_lines(&remove_rovo_blocks(yaml_lines(&before)));
    ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    }
}

fn plan_rovo_hooks_update(before: &str, path: &Path) -> Result<ClaudeIntegrationPlan, CliError> {
    let before = normalize_yaml_text(before);
    let mut lines = remove_rovo_blocks(yaml_lines(&before));
    let events = [
        ("on_complete", "cmux hooks rovodev stop"),
        ("on_error", "cmux hooks rovodev stop"),
        ("on_tool_permission", "cmux hooks rovodev prompt-submit"),
    ];
    if let Some(events_index) = rovo_events_index(&lines) {
        let indent = format!("{}  ", leading_whitespace(&lines[events_index]));
        let block = rovo_event_block(&events, &indent, true);
        lines.splice(events_index + 1..events_index + 1, block);
    } else if let Some(event_hooks_index) = rovo_event_hooks_index(&lines) {
        let child_indent = format!("{}  ", leading_whitespace(&lines[event_hooks_index]));
        let mut block = vec![
            format!("{child_indent}{ROVO_BEGIN_MARKER}"),
            format!("{child_indent}events:"),
        ];
        block.extend(rovo_event_block(
            &events,
            &format!("{child_indent}  "),
            false,
        ));
        block.push(format!("{child_indent}{ROVO_END_MARKER}"));
        lines.splice(event_hooks_index + 1..event_hooks_index + 1, block);
    } else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.extend([
            ROVO_BEGIN_MARKER.to_string(),
            "eventHooks:".to_string(),
            "  events:".to_string(),
        ]);
        lines.extend(rovo_event_block(&events, "    ", false));
        lines.push(ROVO_END_MARKER.to_string());
    }
    let after = serialize_yaml_lines(&lines);
    Ok(ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    })
}

fn yaml_lines(content: &str) -> Vec<String> {
    let mut lines: Vec<_> = content
        .replace("\r\n", "\n")
        .split('\n')
        .map(str::to_string)
        .collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn serialize_yaml_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn normalize_yaml_text(content: &str) -> String {
    serialize_yaml_lines(&yaml_lines(content))
}

fn remove_rovo_blocks(mut lines: Vec<String>) -> Vec<String> {
    let mut index = 0;
    while index < lines.len() {
        if lines[index].trim() != ROVO_BEGIN_MARKER {
            index += 1;
            continue;
        }
        let Some(end) = ((index + 1)..lines.len())
            .find(|candidate| lines[*candidate].trim() == ROVO_END_MARKER)
        else {
            index += 1;
            continue;
        };
        let start = if index > 0 && lines[index - 1].trim().is_empty() {
            index - 1
        } else {
            index
        };
        lines.drain(start..=end);
        index = start;
    }
    lines
}

fn rovo_event_hooks_index(lines: &[String]) -> Option<usize> {
    lines.iter().position(|line| yaml_key(line, "eventHooks"))
}

fn rovo_events_index(lines: &[String]) -> Option<usize> {
    let root = rovo_event_hooks_index(lines)?;
    let indent = format!("{}  ", leading_whitespace(&lines[root]));
    for (index, line) in lines.iter().enumerate().skip(root + 1) {
        if line
            .chars()
            .next()
            .is_some_and(|value| !value.is_whitespace())
        {
            return None;
        }
        if line
            .strip_prefix(&indent)
            .is_some_and(|suffix| yaml_key(suffix, "events"))
        {
            return Some(index);
        }
    }
    None
}

fn yaml_key(line: &str, key: &str) -> bool {
    line.strip_prefix(key)
        .and_then(|suffix| suffix.strip_prefix(':'))
        .is_some_and(|suffix| suffix.trim().is_empty() || suffix.trim_start().starts_with('#'))
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn rovo_event_block(events: &[(&str, &str)], indent: &str, markers: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if markers {
        lines.push(format!("{indent}{ROVO_BEGIN_MARKER}"));
    }
    for (name, command) in events {
        lines.push(format!("{indent}- name: {name}"));
        lines.push(format!("{indent}  commands:"));
        lines.push(format!(
            "{indent}    - command: {}",
            yaml_double_quoted(command)
        ));
    }
    if markers {
        lines.push(format!("{indent}{ROVO_END_MARKER}"));
    }
    lines
}

const HERMES_BEGIN_MARKER: &str = "# cmux hooks hermes-agent begin";
const HERMES_END_MARKER: &str = "# cmux hooks hermes-agent end";
const HERMES_RESTORE_PREFIX: &str = "# cmux hooks hermes-agent begin restore-line-base64:";

#[derive(Clone)]
struct HermesEvent {
    name: &'static str,
    command: String,
    timeout: u64,
}

fn hermes_events() -> Vec<HermesEvent> {
    let mut events = Vec::new();
    for (name, action) in [
        ("on_session_start", "session-start"),
        ("pre_llm_call", "prompt-submit"),
        ("post_llm_call", "agent-response"),
        ("pre_approval_request", "notification"),
        ("post_approval_response", "approval-response"),
        ("on_session_end", "session-end"),
        ("on_session_finalize", "session-finalize"),
        ("on_session_reset", "session-start"),
    ] {
        events.push(HermesEvent {
            name,
            command: format!("sh -c 'cmux hooks hermes-agent {action}'"),
            timeout: 5,
        });
    }
    for name in [
        "pre_tool_call",
        "post_tool_call",
        "pre_approval_request",
        "post_approval_response",
    ] {
        events.push(HermesEvent {
            name,
            command: format!("sh -c 'cmux hooks feed --source hermes-agent --event {name}'"),
            timeout: 120,
        });
    }
    events
}

fn install_hermes_hooks(yes: bool) -> Result<String, CliError> {
    let config_dir = hermes_config_dir()?;
    if !config_dir.is_dir() {
        return Ok(format!(
            "{} does not exist. Install Hermes Agent first.\n",
            config_dir.display()
        ));
    }
    let config_path = config_dir.join("config.yaml");
    let allowlist_path = config_dir.join("shell-hooks-allowlist.json");
    let config_before = read_optional_text(&config_path)?;
    let config_plan = plan_hermes_hooks_update(&config_before, &config_path);
    let allowlist_before = read_optional_text(&allowlist_path)?;
    let allowlist_plan = plan_hermes_allowlist_update(&allowlist_before)?;
    if config_plan.changed && !confirm_hook_change(&config_plan.diff, yes)? {
        return Ok("Aborted.\n".to_string());
    }
    let mut output = String::new();
    if config_plan.changed {
        write_text_exact(&config_path, &config_plan.after)?;
        output.push_str(&format!(
            "Hermes Agent hooks installed at {}\n",
            config_path.display()
        ));
    } else {
        output.push_str(&format!(
            "Hermes Agent hooks already up to date at {}\n",
            config_path.display()
        ));
    }
    if allowlist_plan.changed {
        write_text_exact(&allowlist_path, &allowlist_plan.after)?;
        output.push_str(&format!(
            "Approved Hermes Agent cmux shell hooks in {}\n",
            allowlist_path.display()
        ));
    }
    Ok(output)
}

fn uninstall_hermes_hooks() -> Result<String, CliError> {
    let config_dir = hermes_config_dir()?;
    let config_path = config_dir.join("config.yaml");
    let allowlist_path = config_dir.join("shell-hooks-allowlist.json");
    let mut output = String::new();
    match fs::read_to_string(&config_path) {
        Ok(before) => {
            let plan = plan_hermes_hooks_uninstall(&before, &config_path);
            if plan.changed {
                write_text_exact(&config_path, &plan.after)?;
                output.push_str(&format!(
                    "Removed Hermes Agent cmux hooks from {}\n",
                    config_path.display()
                ));
            } else {
                output.push_str(&format!(
                    "Removed 0 cmux hook(s) from {}\n",
                    config_path.display()
                ));
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => output.push_str(&format!(
            "No config.yaml found at {}\n",
            config_path.display()
        )),
        Err(error) => {
            return Err(CliError::new(format!(
                "failed to read {}: {error}",
                config_path.display()
            )))
        }
    }
    let allowlist_before = match fs::read_to_string(&allowlist_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(output),
        Err(error) => {
            return Err(CliError::new(format!(
                "failed to read {}: {error}",
                allowlist_path.display()
            )))
        }
    };
    let plan = plan_hermes_allowlist_uninstall(&allowlist_before)?;
    if plan.changed {
        write_text_exact(&allowlist_path, &plan.after)?;
        output.push_str(&format!(
            "Removed Hermes Agent cmux shell hook approvals from {}\n",
            allowlist_path.display()
        ));
    }
    Ok(output)
}

fn hermes_config_dir() -> Result<PathBuf, CliError> {
    if let Some(path) = nonempty_env("HERMES_HOME") {
        expand_home_path(PathBuf::from(path))
    } else {
        home_dir()
            .map(|home| home.join(".hermes"))
            .ok_or_else(|| CliError::new("unable to determine Hermes Agent config directory"))
    }
}

fn plan_hermes_hooks_update(before: &str, path: &Path) -> ClaudeIntegrationPlan {
    let before = normalize_yaml_text(before);
    let mut lines = remove_hermes_blocks(yaml_lines(&before));
    let groups = hermes_event_groups();
    if let Some(hooks_index) = hermes_hooks_index(&lines) {
        let hooks_restore = if yaml_inline_empty_line(&lines[hooks_index]) {
            let original = lines[hooks_index].clone();
            lines[hooks_index] = "hooks:".to_string();
            Some(original)
        } else {
            None
        };
        let child_indent = format!("{}  ", leading_whitespace(&lines[hooks_index]));
        let existing = hermes_direct_event_indexes(&lines, hooks_index, &child_indent);
        let mut missing = Vec::new();
        let mut matched = Vec::new();
        for group in &groups {
            if let Some(index) = existing
                .iter()
                .find_map(|(name, index)| (name == &group.0).then_some(*index))
            {
                matched.push((group, index));
            } else {
                missing.push(group);
            }
        }
        matched.sort_by_key(|(_, index)| std::cmp::Reverse(*index));
        for (group, event_index) in matched {
            let restore = if yaml_inline_empty_line(&lines[event_index]) {
                let original = lines[event_index].clone();
                let colon = original.find(':').unwrap_or(original.len() - 1);
                lines[event_index] = original[..=colon].to_string();
                Some(original)
            } else {
                None
            };
            let indent = format!("{}  ", leading_whitespace(&lines[event_index]));
            let block = hermes_entries_block(&group.1, &indent, true, restore.as_deref());
            lines.splice(event_index + 1..event_index + 1, block);
        }
        if !missing.is_empty() {
            let block =
                hermes_sections_block(&missing, &child_indent, true, hooks_restore.as_deref());
            lines.splice(hooks_index + 1..hooks_index + 1, block);
        }
    } else {
        if lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(HERMES_BEGIN_MARKER.to_string());
        lines.push("hooks:".to_string());
        lines.extend(hermes_sections_block(
            &groups.iter().collect::<Vec<_>>(),
            "  ",
            false,
            None,
        ));
        lines.push(HERMES_END_MARKER.to_string());
    }
    let after = serialize_yaml_lines(&lines);
    ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    }
}

fn plan_hermes_hooks_uninstall(before: &str, path: &Path) -> ClaudeIntegrationPlan {
    let after = serialize_yaml_lines(&remove_hermes_blocks(yaml_lines(before)));
    ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, before, &after),
        before: before.to_string(),
        after,
    }
}

fn hermes_event_groups() -> Vec<(String, Vec<HermesEvent>)> {
    let mut groups: Vec<(String, Vec<HermesEvent>)> = Vec::new();
    for event in hermes_events() {
        if let Some((_, events)) = groups.iter_mut().find(|(name, _)| name == event.name) {
            events.push(event);
        } else {
            groups.push((event.name.to_string(), vec![event]));
        }
    }
    groups
}

fn hermes_sections_block(
    groups: &[&(String, Vec<HermesEvent>)],
    indent: &str,
    markers: bool,
    restore: Option<&str>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if markers {
        lines.push(format!("{indent}{}", hermes_begin_line(restore)));
    }
    for group in groups {
        lines.push(format!("{indent}{}:", group.0));
        lines.extend(hermes_hook_entries(&group.1, &format!("{indent}  ")));
    }
    if markers {
        lines.push(format!("{indent}{HERMES_END_MARKER}"));
    }
    lines
}

fn hermes_entries_block(
    events: &[HermesEvent],
    indent: &str,
    markers: bool,
    restore: Option<&str>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if markers {
        lines.push(format!("{indent}{}", hermes_begin_line(restore)));
    }
    lines.extend(hermes_hook_entries(events, indent));
    if markers {
        lines.push(format!("{indent}{HERMES_END_MARKER}"));
    }
    lines
}

fn hermes_hook_entries(events: &[HermesEvent], indent: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for event in events {
        lines.push(format!(
            "{indent}- command: {}",
            yaml_double_quoted(&event.command)
        ));
        lines.push(format!("{indent}  timeout: {}", event.timeout));
    }
    lines
}

fn hermes_begin_line(restore: Option<&str>) -> String {
    match restore {
        Some(line) => format!(
            "{HERMES_RESTORE_PREFIX} {}",
            base64::engine::general_purpose::STANDARD.encode(line)
        ),
        None => HERMES_BEGIN_MARKER.to_string(),
    }
}

fn remove_hermes_blocks(mut lines: Vec<String>) -> Vec<String> {
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if trimmed != HERMES_BEGIN_MARKER && !trimmed.starts_with(HERMES_RESTORE_PREFIX) {
            index += 1;
            continue;
        }
        let Some(end) = ((index + 1)..lines.len())
            .find(|candidate| lines[*candidate].trim() == HERMES_END_MARKER)
        else {
            index += 1;
            continue;
        };
        if let Some(encoded) = trimmed.strip_prefix(HERMES_RESTORE_PREFIX) {
            if index > 0 {
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded.trim())
                {
                    if let Ok(restored) = String::from_utf8(bytes) {
                        lines[index - 1] = restored;
                        lines.drain(index..=end);
                        continue;
                    }
                }
            }
        }
        let start = if index > 0 && lines[index - 1].trim().is_empty() {
            index - 1
        } else {
            index
        };
        lines.drain(start..=end);
        index = start;
    }
    lines
}

fn hermes_hooks_index(lines: &[String]) -> Option<usize> {
    lines.iter().position(|line| {
        leading_whitespace(line).is_empty()
            && line
                .strip_prefix("hooks:")
                .is_some_and(yaml_inline_empty_suffix)
    })
}

fn yaml_inline_empty_line(line: &str) -> bool {
    line.find(':')
        .is_some_and(|colon| yaml_inline_empty_suffix(&line[colon + 1..]))
}

fn yaml_inline_empty_suffix(suffix: &str) -> bool {
    let value = suffix.split('#').next().unwrap_or_default().trim();
    value.is_empty() || value == "{}" || value == "[]"
}

fn hermes_direct_event_indexes(
    lines: &[String],
    hooks_index: usize,
    child_indent: &str,
) -> Vec<(String, usize)> {
    let mut indexes = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(hooks_index + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line.starts_with(child_indent) {
            break;
        }
        if leading_whitespace(line) != child_indent {
            continue;
        }
        let Some(colon) = trimmed.find(':') else {
            continue;
        };
        if yaml_inline_empty_suffix(&trimmed[colon + 1..]) {
            indexes.push((trimmed[..colon].to_string(), index));
        }
    }
    indexes
}

fn plan_hermes_allowlist_update(before: &str) -> Result<ClaudeIntegrationPlan, CliError> {
    let (before_value, mut object) = parse_hermes_allowlist(before)?;
    let approvals = object
        .remove("approvals")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut passthrough = Vec::new();
    let mut keyed = std::collections::BTreeMap::new();
    for approval in approvals {
        let event = approval.get("event").and_then(serde_json::Value::as_str);
        let command = approval.get("command").and_then(serde_json::Value::as_str);
        if let (Some(event), Some(command)) = (event, command) {
            keyed.insert(format!("{event}\0{command}"), approval);
        } else {
            passthrough.push(approval);
        }
    }
    let approved_at = current_rfc3339();
    for event in hermes_events() {
        let key = format!("{}\0{}", event.name, event.command);
        keyed.entry(key).or_insert_with(|| {
            serde_json::json!({
                "event":event.name,
                "command":event.command,
                "approved_at":approved_at
            })
        });
    }
    passthrough.extend(keyed.into_values());
    object.insert(
        "approvals".to_string(),
        serde_json::Value::Array(passthrough),
    );
    let after = serde_json::to_string_pretty(&object)
        .map_err(|error| CliError::new(format!("failed to encode Hermes allowlist: {error}")))?;
    let before = if before.trim().is_empty() {
        serde_json::to_string_pretty(&serde_json::json!({"approvals":[]})).unwrap()
    } else {
        serde_json::to_string_pretty(&before_value).map_err(|error| {
            CliError::new(format!("failed to normalize Hermes allowlist: {error}"))
        })?
    };
    let path = PathBuf::from("shell-hooks-allowlist.json");
    Ok(ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(&path, &before, &after),
        before,
        after,
    })
}

fn plan_hermes_allowlist_uninstall(before: &str) -> Result<ClaudeIntegrationPlan, CliError> {
    let (_, mut object) = parse_hermes_allowlist(before)?;
    let owned: std::collections::HashSet<_> = hermes_events()
        .into_iter()
        .map(|event| format!("{}\0{}", event.name, event.command))
        .collect();
    let approvals = object
        .remove("approvals")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let approvals = approvals
        .into_iter()
        .filter(|approval| {
            let event = approval.get("event").and_then(serde_json::Value::as_str);
            let command = approval.get("command").and_then(serde_json::Value::as_str);
            !matches!((event, command), (Some(event), Some(command)) if owned.contains(&format!("{event}\0{command}")))
        })
        .collect();
    object.insert("approvals".to_string(), serde_json::Value::Array(approvals));
    let after = serde_json::to_string_pretty(&object)
        .map_err(|error| CliError::new(format!("failed to encode Hermes allowlist: {error}")))?;
    let path = PathBuf::from("shell-hooks-allowlist.json");
    Ok(ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(&path, before, &after),
        before: before.to_string(),
        after,
    })
}

fn parse_hermes_allowlist(
    before: &str,
) -> Result<
    (
        serde_json::Value,
        serde_json::Map<String, serde_json::Value>,
    ),
    CliError,
> {
    let value = if before.trim().is_empty() {
        serde_json::json!({"approvals":[]})
    } else {
        serde_json::from_str(before)
            .map_err(|error| CliError::new(format!("failed to parse Hermes allowlist: {error}")))?
    };
    let object = value
        .as_object()
        .cloned()
        .ok_or_else(|| CliError::new("Hermes allowlist must be a JSON object"))?;
    Ok((value, object))
}

fn yaml_double_quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}

fn current_rfc3339() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds / 86_400;
    let day_seconds = seconds % 86_400;
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        day_seconds / 3_600,
        (day_seconds % 3_600) / 60,
        day_seconds % 60
    )
}

const KIMI_BEGIN_MARKER: &str = "# cmux-kimi-hooks-7c3a9f12-4e8b-4d2a-9f15-6b8c0d1e2a3f begin";
const KIMI_END_MARKER: &str = "# cmux-kimi-hooks-7c3a9f12-4e8b-4d2a-9f15-6b8c0d1e2a3f end";

fn install_kimi_hooks(yes: bool) -> Result<String, CliError> {
    let path = kimi_hooks_path()?;
    let before = read_optional_text(&path)?;
    let plan = plan_kimi_hooks_update(&before, &path);
    if !plan.changed {
        return Ok(format!(
            "Kimi Code hooks already up to date at {}\n",
            path.display()
        ));
    }
    if !confirm_hook_change(&plan.diff, yes)? {
        return Ok("Aborted.\n".to_string());
    }
    write_text_exact(&path, &plan.after)?;
    Ok(format!("Kimi Code hooks installed at {}\n", path.display()))
}

fn uninstall_kimi_hooks() -> Result<String, CliError> {
    let path = kimi_hooks_path()?;
    let before = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(format!("No config.toml found at {}\n", path.display()))
        }
        Err(error) => {
            return Err(CliError::new(format!(
                "failed to read {}: {error}",
                path.display()
            )))
        }
    };
    let plan = plan_kimi_hooks_uninstall(&before, &path);
    if !plan.changed {
        return Ok(format!("Removed 0 cmux hook(s) from {}\n", path.display()));
    }
    write_text_exact(&path, &plan.after)?;
    Ok(format!(
        "Removed Kimi Code cmux hooks from {}\n",
        path.display()
    ))
}

fn kimi_hooks_path() -> Result<PathBuf, CliError> {
    let config_dir = if let Some(path) = nonempty_env("KIMI_CODE_HOME") {
        expand_home_path(PathBuf::from(path))?
    } else {
        home_dir()
            .map(|home| home.join(".kimi-code"))
            .ok_or_else(|| CliError::new("unable to determine Kimi Code config directory"))?
    };
    Ok(config_dir.join("config.toml"))
}

fn plan_kimi_hooks_update(before: &str, path: &Path) -> ClaudeIntegrationPlan {
    let before = normalize_toml_text(before);
    let mut lines = remove_kimi_blocks(toml_lines(&before));
    if lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    lines.push(KIMI_BEGIN_MARKER.to_string());
    for (event, command, timeout) in kimi_events() {
        lines.extend([
            "[[hooks]]".to_string(),
            format!("event = \"{}\"", toml_basic_string(event)),
            format!("command = \"{}\"", toml_basic_string(&command)),
            format!("timeout = {timeout}"),
            String::new(),
        ]);
    }
    lines.push(KIMI_END_MARKER.to_string());
    let after = toml_content(&lines);
    ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    }
}

fn plan_kimi_hooks_uninstall(before: &str, path: &Path) -> ClaudeIntegrationPlan {
    let before = normalize_toml_text(before);
    let after = toml_content(&remove_kimi_blocks(toml_lines(&before)));
    ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(path, &before, &after),
        before,
        after,
    }
}

fn kimi_events() -> Vec<(&'static str, String, u64)> {
    let mut events = Vec::new();
    for (event, action) in [
        ("SessionStart", "session-start"),
        ("UserPromptSubmit", "prompt-submit"),
        ("PermissionRequest", "notification"),
        ("Stop", "stop"),
        ("StopFailure", "notification"),
        ("Interrupt", "stop"),
        ("SessionEnd", "session-end"),
    ] {
        events.push((event, format!("cmux hooks kimi {action}"), 10));
    }
    for event in ["PreToolUse", "PostToolUse", "PermissionRequest"] {
        events.push((
            event,
            format!("cmux hooks feed --source kimi --event {event}"),
            120,
        ));
    }
    events
}

fn toml_lines(content: &str) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<_> = normalized.split('\n').map(str::to_string).collect();
    if normalized.ends_with('\n') && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn toml_content(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn normalize_toml_text(content: &str) -> String {
    toml_content(&toml_lines(content))
}

fn remove_kimi_blocks(mut lines: Vec<String>) -> Vec<String> {
    let mut index = 0;
    while index < lines.len() {
        if lines[index].trim() != KIMI_BEGIN_MARKER {
            index += 1;
            continue;
        }
        if let Some(end) =
            (index..lines.len()).find(|candidate| lines[*candidate].trim() == KIMI_END_MARKER)
        {
            lines.drain(index..=end);
        } else {
            lines.remove(index);
        }
    }
    lines
}

fn toml_basic_string(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\u{0008}' => escaped.push_str("\\b"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\u{000C}' => escaped.push_str("\\f"),
            '\r' => escaped.push_str("\\r"),
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            value if value.is_control() => {
                let scalar = value as u32;
                if scalar <= 0xFFFF {
                    escaped.push_str(&format!("\\u{scalar:04X}"));
                } else {
                    escaped.push_str(&format!("\\U{scalar:08X}"));
                }
            }
            value => escaped.push(value),
        }
    }
    escaped
}

const CODEX_FEATURE_BEGIN: &str =
    "# cmux-codex-hooks-feature-78f1e4ba-66df-4d35-93c1-67fdf1cbb7df begin";
const CODEX_FEATURE_END: &str =
    "# cmux-codex-hooks-feature-78f1e4ba-66df-4d35-93c1-67fdf1cbb7df end";
const CODEX_FEATURE_PREVIOUS: &str =
    "# cmux-codex-hooks-feature-78f1e4ba-66df-4d35-93c1-67fdf1cbb7df previous line: ";
const CODEX_TRUST_BEGIN: &str =
    "# cmux-codex-hook-trust-f5cc24da-7a09-4b20-a756-89e7786f6738 begin";
const CODEX_TRUST_END: &str = "# cmux-codex-hook-trust-f5cc24da-7a09-4b20-a756-89e7786f6738 end";

fn codex_agent_def() -> NestedAgentDef {
    NestedAgentDef {
        name: "codex",
        display_name: "Codex",
        config_dir: ".codex",
        config_file: "hooks.json",
        env_override: Some("CODEX_HOME"),
        env_subdir: None,
        lifecycle_timeout: 5,
        feed_timeout: 5,
        events: &[
            ("SessionStart", "session-start"),
            ("UserPromptSubmit", "prompt-submit"),
            ("Stop", "stop"),
        ],
        feed_events: &[
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "PreCompact",
            "PostCompact",
            "SubagentStart",
            "SubagentStop",
        ],
    }
}

fn install_codex_hooks(yes: bool) -> Result<String, CliError> {
    let config_dir = codex_config_dir()?;
    if !config_dir.is_dir() {
        return Ok(
            "Required agent configuration is missing. Run `cmux hooks setup` after installing your agent CLI.\n"
                .to_string(),
        );
    }
    let hooks_path = config_dir.join("hooks.json");
    let config_path = config_dir.join("config.toml");
    let hooks_before = read_config_or_empty_object(&hooks_path)?;
    let hooks_plan = plan_codex_hooks_update(&hooks_before, &hooks_path)?;
    let config_before = read_optional_text(&config_path)?;
    let config_plan =
        plan_codex_config_update(&config_before, &hooks_plan.after, &hooks_path, &config_path)?;
    let preview = format!("{}{}", hooks_plan.diff, config_plan.diff);
    if (hooks_plan.changed || config_plan.changed) && !confirm_hook_change(&preview, yes)? {
        return Ok("Aborted.\n".to_string());
    }
    let mut output = String::new();
    if hooks_plan.changed {
        write_config(&hooks_path, &hooks_plan.after)?;
        output.push_str(&format!(
            "Codex hooks installed at {}\n",
            hooks_path.display()
        ));
    } else {
        output.push_str(&format!(
            "Codex hooks already up to date at {}\n",
            hooks_path.display()
        ));
    }
    if config_plan.changed {
        write_text_exact(&config_path, &config_plan.after)?;
        output.push_str(&format!(
            "Enabled hooks and approved cmux hooks in {}\n",
            config_path.display()
        ));
    }
    Ok(output)
}

fn uninstall_codex_hooks() -> Result<String, CliError> {
    let config_dir = codex_config_dir()?;
    let hooks_path = config_dir.join("hooks.json");
    let Some(hooks_before) = read_existing_json_object(&hooks_path) else {
        return Ok(format!("No hooks.json found at {}\n", hooks_path.display()));
    };
    let (hooks_plan, removed) =
        plan_nested_hooks_uninstall(&hooks_before, &hooks_path, &codex_agent_def())?;
    write_config(&hooks_path, &hooks_plan.after)?;
    let mut output = format!(
        "Removed {removed} cmux hook(s) from {}\n",
        hooks_path.display()
    );
    let config_path = config_dir.join("config.toml");
    let config_before = match fs::read_to_string(&config_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(output),
        Err(error) => {
            return Err(CliError::new(format!(
                "failed to read {}: {error}",
                config_path.display()
            )))
        }
    };
    let config_plan = plan_codex_config_uninstall(&config_before, &config_path);
    if config_plan.changed {
        write_text_exact(&config_path, &config_plan.after)?;
        output.push_str(&format!(
            "Removed Codex hooks feature from {}\n",
            config_path.display()
        ));
    }
    Ok(output)
}

fn codex_config_dir() -> Result<PathBuf, CliError> {
    if let Some(path) = nonempty_env("CODEX_HOME") {
        expand_home_path(PathBuf::from(path))
    } else {
        home_dir()
            .map(|home| home.join(".codex"))
            .ok_or_else(|| CliError::new("unable to determine Codex config directory"))
    }
}

fn plan_codex_hooks_update(before: &str, path: &Path) -> Result<ClaudeIntegrationPlan, CliError> {
    plan_nested_hooks_update(before, path, &codex_agent_def())
}

#[derive(Clone)]
struct CodexTrustEntry {
    key: String,
    hash: String,
}

fn plan_codex_config_update(
    before: &str,
    hooks_json: &str,
    hooks_path: &Path,
    config_path: &Path,
) -> Result<ClaudeIntegrationPlan, CliError> {
    let before = normalize_toml_text(before);
    let mut lines = remove_codex_feature_block(toml_lines(&before));
    install_codex_feature(&mut lines);
    lines = remove_named_block(lines, CODEX_TRUST_BEGIN, CODEX_TRUST_END);
    let entries = codex_trust_entries(hooks_json, hooks_path)?;
    if !entries.is_empty() {
        if lines.last().is_some_and(|line| !line.is_empty()) {
            lines.push(String::new());
        }
        lines.push(CODEX_TRUST_BEGIN.to_string());
        for entry in entries {
            lines.push(format!(
                "[hooks.state.\"{}\"]",
                toml_basic_string(&entry.key)
            ));
            lines.push(format!(
                "trusted_hash = \"{}\"",
                toml_basic_string(&entry.hash)
            ));
        }
        lines.push(CODEX_TRUST_END.to_string());
    }
    let after = toml_content(&lines);
    Ok(ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(config_path, &before, &after),
        before,
        after,
    })
}

fn plan_codex_config_uninstall(before: &str, config_path: &Path) -> ClaudeIntegrationPlan {
    let before = normalize_toml_text(before);
    let lines = remove_named_block(
        remove_codex_feature_block(toml_lines(&before)),
        CODEX_TRUST_BEGIN,
        CODEX_TRUST_END,
    );
    let after = toml_content(&lines);
    ClaudeIntegrationPlan {
        changed: before != after,
        diff: unified_diff(config_path, &before, &after),
        before,
        after,
    }
}

fn remove_codex_feature_block(mut lines: Vec<String>) -> Vec<String> {
    let mut index = 0;
    while index < lines.len() {
        if lines[index] != CODEX_FEATURE_BEGIN {
            index += 1;
            continue;
        }
        let Some(end) = (index + 1..lines.len()).find(|item| lines[*item] == CODEX_FEATURE_END)
        else {
            lines.remove(index);
            continue;
        };
        let previous = lines[index + 1..end]
            .iter()
            .find_map(|line| line.strip_prefix(CODEX_FEATURE_PREVIOUS))
            .map(str::to_string);
        if let Some(previous) = previous {
            lines.splice(index..=end, [previous]);
            index += 1;
        } else {
            lines.drain(index..=end);
        }
    }
    lines
}

fn install_codex_feature(lines: &mut Vec<String>) {
    if let Some(features) = lines.iter().position(|line| line.trim() == "[features]") {
        let end = (features + 1..lines.len())
            .find(|index| lines[*index].trim_start().starts_with('['))
            .unwrap_or(lines.len());
        if let Some(hooks) =
            (features + 1..end).find(|index| toml_key(&lines[*index]) == Some("hooks"))
        {
            if !toml_true_value(&lines[hooks]) {
                let previous = lines[hooks].clone();
                lines.splice(
                    hooks..=hooks,
                    codex_feature_lines("hooks = true", Some(&previous)),
                );
            }
        } else {
            lines.splice(
                features + 1..features + 1,
                codex_feature_lines("hooks = true", None),
            );
        }
        return;
    }
    if let Some(hooks) = lines
        .iter()
        .position(|line| toml_dotted_feature_key(line).as_deref() == Some("hooks"))
    {
        if !toml_true_value(&lines[hooks]) {
            let previous = lines[hooks].clone();
            lines.splice(
                hooks..=hooks,
                codex_feature_lines("features.hooks = true", Some(&previous)),
            );
        }
        return;
    }
    if let Some(first_dotted) = lines
        .iter()
        .position(|line| toml_dotted_feature_key(line).is_some())
    {
        lines.splice(
            first_dotted..first_dotted,
            codex_feature_lines("features.hooks = true", None),
        );
        return;
    }
    if !lines.is_empty() && lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    lines.extend([
        "[features]".to_string(),
        CODEX_FEATURE_BEGIN.to_string(),
        "hooks = true".to_string(),
        CODEX_FEATURE_END.to_string(),
    ]);
}

fn codex_feature_lines(setting: &str, previous: Option<&str>) -> Vec<String> {
    let mut lines = vec![CODEX_FEATURE_BEGIN.to_string()];
    if let Some(previous) = previous {
        lines.push(format!("{CODEX_FEATURE_PREVIOUS}{previous}"));
    }
    lines.push(setting.to_string());
    lines.push(CODEX_FEATURE_END.to_string());
    lines
}

fn toml_key(line: &str) -> Option<&str> {
    let body = line.split('#').next()?.trim();
    let (key, _) = body.split_once('=')?;
    Some(key.trim())
}

fn toml_dotted_feature_key(line: &str) -> Option<String> {
    let key: String = toml_key(line)?
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    key.strip_prefix("features.").map(str::to_string)
}

fn toml_true_value(line: &str) -> bool {
    line.split('#')
        .next()
        .and_then(|body| body.split_once('='))
        .is_some_and(|(_, value)| value.trim() == "true")
}

fn remove_named_block(mut lines: Vec<String>, begin: &str, end: &str) -> Vec<String> {
    let mut index = 0;
    while index < lines.len() {
        if lines[index] != begin {
            index += 1;
            continue;
        }
        if let Some(end_index) = (index + 1..lines.len()).find(|item| lines[*item] == end) {
            let start = if index > 0 && lines[index - 1].is_empty() {
                index - 1
            } else {
                index
            };
            lines.drain(start..=end_index);
            index = start;
        } else {
            lines.remove(index);
        }
    }
    lines
}

fn codex_trust_entries(
    hooks_json: &str,
    hooks_path: &Path,
) -> Result<Vec<CodexTrustEntry>, CliError> {
    let value: serde_json::Value = serde_json::from_str(hooks_json)
        .map_err(|error| CliError::new(format!("failed to parse Codex hooks: {error}")))?;
    let hooks = value
        .get("hooks")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| CliError::new("Codex hooks key must be an object"))?;
    let source = normalized_codex_path(hooks_path);
    let mut entries = Vec::new();
    for event in [
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "PreCompact",
        "PostCompact",
        "SessionStart",
        "SubagentStart",
        "SubagentStop",
        "UserPromptSubmit",
        "Stop",
    ] {
        let Some(label) = codex_event_label(event) else {
            continue;
        };
        let Some(groups) = hooks.get(event).and_then(serde_json::Value::as_array) else {
            continue;
        };
        for (group_index, group) in groups.iter().enumerate() {
            let matcher = group.get("matcher").and_then(serde_json::Value::as_str);
            let Some(handlers) = group.get("hooks").and_then(serde_json::Value::as_array) else {
                continue;
            };
            for (handler_index, handler) in handlers.iter().enumerate() {
                let Some(command) = handler.get("command").and_then(serde_json::Value::as_str)
                else {
                    continue;
                };
                if !command.contains("cmux hooks codex")
                    && !command.contains("hooks feed --source codex")
                {
                    continue;
                }
                let timeout = handler
                    .get("timeout")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(600)
                    .max(1);
                entries.push(CodexTrustEntry {
                    key: format!("{source}:{label}:{group_index}:{handler_index}"),
                    hash: codex_command_hash(label, matcher, command, timeout),
                });
            }
        }
    }
    Ok(entries)
}

fn normalized_codex_path(path: &Path) -> String {
    fs::canonicalize(path)
        .or_else(|_| {
            let parent = path
                .parent()
                .ok_or_else(|| io::Error::other("missing parent"))?;
            fs::canonicalize(parent).map(|parent| parent.join(path.file_name().unwrap_or_default()))
        })
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn codex_event_label(event: &str) -> Option<&'static str> {
    Some(match event {
        "PreToolUse" => "pre_tool_use",
        "PermissionRequest" => "permission_request",
        "PostToolUse" => "post_tool_use",
        "PreCompact" => "pre_compact",
        "PostCompact" => "post_compact",
        "SessionStart" => "session_start",
        "SubagentStart" => "subagent_start",
        "SubagentStop" => "subagent_stop",
        "UserPromptSubmit" => "user_prompt_submit",
        "Stop" => "stop",
        _ => return None,
    })
}

fn codex_command_hash(label: &str, matcher: Option<&str>, command: &str, timeout: u64) -> String {
    let mut handler = std::collections::BTreeMap::new();
    handler.insert("async", serde_json::json!(false));
    handler.insert("command", serde_json::json!(command));
    handler.insert("timeout", serde_json::json!(timeout.max(1)));
    handler.insert("type", serde_json::json!("command"));
    let mut identity = std::collections::BTreeMap::new();
    identity.insert("event_name", serde_json::json!(label));
    identity.insert("hooks", serde_json::json!([handler]));
    if let Some(matcher) = matcher {
        identity.insert("matcher", serde_json::json!(matcher));
    }
    let data = serde_json::to_vec(&identity).unwrap_or_default();
    format!("sha256:{:x}", Sha256::digest(data))
}

fn install_opencode_hooks(yes: bool, project: bool) -> Result<String, CliError> {
    let config_dir = opencode_config_dir()?;
    let feed_path = opencode_feed_plugin_path(&config_dir, project)?;
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
        let plan = plan_opencode_registration_update(&config_before, &config_path, true)?;
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

fn uninstall_opencode_hooks(project: bool) -> Result<String, CliError> {
    let config_dir = opencode_config_dir()?;
    let mut output = String::new();
    if !project {
        output.push_str(&uninstall_opencode_session_plugin(&config_dir)?);
    }
    output.push_str(&uninstall_opencode_feed_plugins(&config_dir)?);
    Ok(output)
}

fn uninstall_opencode_session_plugin(config_dir: &Path) -> Result<String, CliError> {
    let path = config_dir.join("plugins").join("cmux-session.js");
    match marked_removal_at_path(&path, OPENCODE_SESSION_PLUGIN_MARKER)? {
        MarkedRemoval::Missing => Ok(format!(
            "No OpenCode cmux plugin found at {}\n",
            path.display()
        )),
        MarkedRemoval::Refuse => Ok(format!(
            "Refusing to remove {}: missing cmux marker\n",
            path.display()
        )),
        MarkedRemoval::Remove => {
            fs::remove_file(&path).map_err(|error| {
                CliError::new(format!("failed to remove {}: {error}", path.display()))
            })?;
            let config_path = config_dir.join("opencode.json");
            let before = read_config_or_empty_object(&config_path)?;
            let plan = plan_opencode_registration_update(&before, &config_path, false)?;
            if plan.changed {
                write_config(&config_path, &plan.after)?;
            }
            Ok(format!(
                "Removed OpenCode cmux plugin from {}\n",
                path.display()
            ))
        }
    }
}

fn uninstall_opencode_feed_plugins(config_dir: &Path) -> Result<String, CliError> {
    let global_path = opencode_feed_plugin_path(config_dir, false)?;
    let project_path = opencode_feed_plugin_path(config_dir, true)?;
    let mut output = String::new();
    for path in [global_path, project_path] {
        let existing = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(_) => {
                output.push_str(&format!("Skipping {} (no cmux marker)\n", path.display()));
                continue;
            }
        };
        if !existing.contains(OPENCODE_FEED_PLUGIN_MARKER) {
            output.push_str(&format!("Skipping {} (no cmux marker)\n", path.display()));
            continue;
        }
        fs::remove_file(&path).map_err(|error| {
            CliError::new(format!("failed to remove {}: {error}", path.display()))
        })?;
        output.push_str(&format!(
            "OpenCode plugin removed from {}\n",
            path.display()
        ));
    }
    Ok(output)
}

fn opencode_feed_plugin_path(config_dir: &Path, project: bool) -> Result<PathBuf, CliError> {
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
    Ok(plugin_dir.join("cmux-feed.js"))
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
    should_install: bool,
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
    if should_install {
        plugins.push(serde_json::json!(OPENCODE_SESSION_PLUGIN_SPEC));
    }
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
    let path = antigravity_hooks_path()?;
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

fn uninstall_antigravity_hooks() -> Result<String, CliError> {
    let path = antigravity_hooks_path()?;
    let before = match fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(_) => return Ok(format!("No hooks.json found at {}\n", path.display())),
    };
    let value: serde_json::Value = match serde_json::from_str(&before) {
        Ok(value) => value,
        Err(_) => {
            return Ok(format!(
                "Malformed hooks.json at {}. Fix or remove it before uninstalling hooks.\n",
                path.display()
            ))
        }
    };
    if !value.is_object() {
        return Ok(format!("Removed 0 cmux hook(s) from {}\n", path.display()));
    }
    let (plan, removed) = plan_antigravity_hooks_uninstall(&before, &path)?;
    if !removed {
        return Ok(format!("Removed 0 cmux hook(s) from {}\n", path.display()));
    }
    write_config(&path, &plan.after)?;
    Ok(format!(
        "Removed Antigravity cmux hooks from {}\n",
        path.display()
    ))
}

fn antigravity_hooks_path() -> Result<PathBuf, CliError> {
    let home = home_dir()
        .ok_or_else(|| CliError::new("unable to determine Antigravity config directory"))?;
    Ok(home.join(".gemini").join("config").join("hooks.json"))
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

fn plan_antigravity_hooks_uninstall(
    before: &str,
    path: &Path,
) -> Result<(ClaudeIntegrationPlan, bool), CliError> {
    let before = normalize_config_text(before)?;
    let mut value: serde_json::Value = serde_json::from_str(&before)
        .map_err(|error| CliError::new(format!("failed to parse Antigravity config: {error}")))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| CliError::new("Antigravity config must be a JSON object"))?;
    let removed = object
        .get("cmux")
        .is_some_and(|group| json_value_contains_owned_command(group, "antigravity"));
    if removed {
        object.remove("cmux");
    }
    let after = serde_json::to_string_pretty(&value)
        .map_err(|error| CliError::new(format!("failed to encode Antigravity config: {error}")))?;
    Ok((
        ClaudeIntegrationPlan {
            changed: before != after,
            diff: unified_diff(path, &before, &after),
            before,
            after,
        },
        removed,
    ))
}

fn json_value_contains_owned_command(value: &serde_json::Value, agent: &str) -> bool {
    match value {
        serde_json::Value::String(command) => agent_command_is_owned(command, agent),
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| json_value_contains_owned_command(value, agent)),
        serde_json::Value::Object(object) => object
            .values()
            .any(|value| json_value_contains_owned_command(value, agent)),
        _ => false,
    }
}

fn install_cursor_hooks(yes: bool) -> Result<String, CliError> {
    let path = cursor_hooks_path()?;
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

fn uninstall_cursor_hooks() -> Result<String, CliError> {
    let path = cursor_hooks_path()?;
    uninstall_flat_hooks(&path, "hooks.json", "Cursor", "cursor")
}

fn cursor_hooks_path() -> Result<PathBuf, CliError> {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .ok_or_else(|| CliError::new("unable to determine Cursor config directory"))?;
    Ok(home.join(".cursor").join("hooks.json"))
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
    prune_flat_owned_hooks(hooks, "cursor");
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

fn uninstall_nested_hooks(agent: &NestedAgentDef) -> Result<String, CliError> {
    let path = nested_hooks_path(agent)?;
    let Some(before) = read_existing_json_object(&path) else {
        return Ok(format!(
            "No {} found at {}\n",
            agent.config_file,
            path.display()
        ));
    };
    let (plan, removed) = plan_nested_hooks_uninstall(&before, &path, agent)?;
    write_config(&path, &plan.after)?;
    Ok(format!(
        "Removed {removed} cmux hook(s) from {}\n",
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
    prune_nested_owned_hooks(hooks, agent.name);
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

fn plan_nested_hooks_uninstall(
    before: &str,
    path: &Path,
    agent: &NestedAgentDef,
) -> Result<(ClaudeIntegrationPlan, usize), CliError> {
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
    if !object
        .get("hooks")
        .is_some_and(serde_json::Value::is_object)
    {
        object.insert("hooks".to_string(), serde_json::json!({}));
    }
    let hooks = object
        .get_mut("hooks")
        .and_then(serde_json::Value::as_object_mut)
        .expect("hooks normalized to an object");
    let removed = prune_nested_owned_hooks(hooks, agent.name);
    let after = serde_json::to_string_pretty(&value).map_err(|error| {
        CliError::new(format!(
            "failed to encode {} config: {error}",
            agent.display_name
        ))
    })?;
    Ok((
        ClaudeIntegrationPlan {
            changed: before != after,
            diff: unified_diff(path, &before, &after),
            before,
            after,
        },
        removed,
    ))
}

fn prune_nested_owned_hooks(
    hooks: &mut serde_json::Map<String, serde_json::Value>,
    agent: &str,
) -> usize {
    let mut removed = 0;
    let mut empty_events = Vec::new();
    for (event, value) in hooks.iter_mut() {
        let Some(groups) = value.as_array_mut() else {
            continue;
        };
        groups.retain_mut(|group| {
            let Some(entries) = group
                .get_mut("hooks")
                .and_then(serde_json::Value::as_array_mut)
            else {
                return true;
            };
            let before = entries.len();
            entries.retain(|entry| {
                !entry
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|command| agent_command_is_owned(command, agent))
            });
            removed += before - entries.len();
            !entries.is_empty()
        });
        if groups.is_empty() {
            empty_events.push(event.clone());
        }
    }
    for event in empty_events {
        hooks.remove(&event);
    }
    removed
}

fn prune_flat_owned_hooks(
    hooks: &mut serde_json::Map<String, serde_json::Value>,
    agent: &str,
) -> usize {
    let mut removed = 0;
    let mut empty_events = Vec::new();
    for (event, value) in hooks.iter_mut() {
        let Some(entries) = value.as_array_mut() else {
            continue;
        };
        let before = entries.len();
        entries.retain(|entry| {
            !entry
                .get("command")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|command| agent_command_is_owned(command, agent))
        });
        removed += before - entries.len();
        if entries.is_empty() {
            empty_events.push(event.clone());
        }
    }
    for event in empty_events {
        hooks.remove(&event);
    }
    removed
}

fn plan_flat_hooks_uninstall(
    before: &str,
    path: &Path,
    display_name: &str,
    agent: &str,
) -> Result<(ClaudeIntegrationPlan, usize), CliError> {
    let before = normalize_config_text(before)?;
    let mut value: serde_json::Value = serde_json::from_str(&before).map_err(|error| {
        CliError::new(format!("failed to parse {display_name} config: {error}"))
    })?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| CliError::new(format!("{display_name} config must be a JSON object")))?;
    if !object
        .get("hooks")
        .is_some_and(serde_json::Value::is_object)
    {
        object.insert("hooks".to_string(), serde_json::json!({}));
    }
    let hooks = object
        .get_mut("hooks")
        .and_then(serde_json::Value::as_object_mut)
        .expect("hooks normalized to an object");
    let removed = prune_flat_owned_hooks(hooks, agent);
    let after = serde_json::to_string_pretty(&value).map_err(|error| {
        CliError::new(format!("failed to encode {display_name} config: {error}"))
    })?;
    Ok((
        ClaudeIntegrationPlan {
            changed: before != after,
            diff: unified_diff(path, &before, &after),
            before,
            after,
        },
        removed,
    ))
}

fn uninstall_flat_hooks(
    path: &Path,
    config_file: &str,
    display_name: &str,
    agent: &str,
) -> Result<String, CliError> {
    let Some(before) = read_existing_json_object(path) else {
        return Ok(format!("No {config_file} found at {}\n", path.display()));
    };
    let (plan, removed) = plan_flat_hooks_uninstall(&before, path, display_name, agent)?;
    write_config(path, &plan.after)?;
    Ok(format!(
        "Removed {removed} cmux hook(s) from {}\n",
        path.display()
    ))
}

fn read_existing_json_object(path: &Path) -> Option<String> {
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str::<serde_json::Value>(&contents)
        .ok()
        .is_some_and(|value| value.is_object())
        .then_some(contents)
}

fn agent_command_is_owned(command: &str, agent: &str) -> bool {
    let tokens: Vec<_> = command.split_whitespace().collect();
    tokens.iter().enumerate().any(|(index, token)| {
        let command_boundary = index == 0 || tokens[index - 1] == "&&";
        if !command_boundary || !cmux_executable_token(token) {
            return false;
        }
        let args = &tokens[index + 1..];
        matches!(args, ["hooks", candidate, ..] if *candidate == agent)
            || matches!(args, ["hooks", "feed", "--source", candidate, ..] if *candidate == agent)
    })
}

fn cmux_executable_token(token: &str) -> bool {
    let token = token.trim_matches(['\'', '"']);
    Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.eq_ignore_ascii_case("cmux") || name.eq_ignore_ascii_case("cmux.exe")
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

fn uninstall_kiro_hooks() -> Result<String, CliError> {
    let path = kiro_hooks_path()?;
    uninstall_flat_hooks(&path, "cmux.json", "Kiro", "kiro")
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
    prune_flat_owned_hooks(hooks, "kiro");
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
    fn nested_json_uninstall_preserves_mixed_user_groups_and_prunes_stale_events() {
        for agent in ["gemini", "grok", "copilot", "codebuddy", "factory", "qoder"] {
            assert_eq!(
                parse_hooks_request("hooks", &[agent.into(), "uninstall".into()]).unwrap(),
                HooksRequest::NestedUninstall {
                    agent: agent.to_string()
                }
            );
        }

        let gemini = nested_agent("gemini").unwrap();
        let before = r#"{
  "theme": "user",
  "hooks": {
    "SessionStart": [
      {"matcher": "mixed", "hooks": [
        {"type": "command", "command": "user command"},
        {"type": "command", "command": "cmux hooks gemini session-start"}
      ]},
      {"hooks": [{"type": "command", "command": "cmux hooks feed --source gemini --event SessionStart"}]},
      {"custom": "unknown shape"}
    ],
    "OldEvent": [{"hooks": [
      {"command": "cmux hooks gemini stale"},
      {"command": "user old command"},
      {"command": "echo cmux hooks gemini should-stay"}
    ]}],
    "UserScalar": "preserve"
  }
}"#;
        let (plan, removed) = plan_nested_hooks_uninstall(before, &path(), gemini).unwrap();
        assert_eq!(removed, 3);
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["theme"], "user");
        assert_eq!(value["hooks"]["SessionStart"].as_array().unwrap().len(), 2);
        assert_eq!(
            value["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "user command"
        );
        assert_eq!(value["hooks"]["SessionStart"][1]["custom"], "unknown shape");
        assert_eq!(
            value["hooks"]["OldEvent"][0]["hooks"][0]["command"],
            "user old command"
        );
        assert_eq!(
            value["hooks"]["OldEvent"][0]["hooks"][1]["command"],
            "echo cmux hooks gemini should-stay"
        );
        assert_eq!(value["hooks"]["UserScalar"], "preserve");
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
    fn flat_json_uninstall_preserves_user_entries_for_kiro_and_cursor() {
        assert_eq!(
            parse_hooks_request("hooks", &["kiro".into(), "uninstall".into()]).unwrap(),
            HooksRequest::KiroUninstall
        );
        assert_eq!(
            parse_hooks_request("hooks", &["cursor".into(), "uninstall".into()]).unwrap(),
            HooksRequest::CursorUninstall
        );
        let before = r#"{
  "version": 1,
  "theme": "user",
  "hooks": {
    "stop": [
      {"command": "user command"},
      {"command": "cmux hooks cursor stop"},
      {"command": "echo cmux hooks cursor should-stay"}
    ],
    "OldEvent": [{"command": "cmux hooks feed --source cursor --event OldEvent"}],
    "UserScalar": "preserve"
  }
}"#;
        let (plan, removed) =
            plan_flat_hooks_uninstall(before, &path(), "Cursor", "cursor").unwrap();
        assert_eq!(removed, 2);
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["theme"], "user");
        assert_eq!(value["hooks"]["stop"].as_array().unwrap().len(), 2);
        assert_eq!(value["hooks"]["stop"][0]["command"], "user command");
        assert_eq!(
            value["hooks"]["stop"][1]["command"],
            "echo cmux hooks cursor should-stay"
        );
        assert!(value["hooks"].get("OldEvent").is_none());
        assert_eq!(value["hooks"]["UserScalar"], "preserve");
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
    fn antigravity_uninstall_removes_only_an_owned_cmux_group() {
        assert_eq!(
            parse_hooks_request("hooks", &["antigravity".into(), "uninstall".into()]).unwrap(),
            HooksRequest::AntigravityUninstall
        );
        assert_eq!(
            parse_hooks_request("hooks", &["agy".into(), "uninstall".into()]).unwrap(),
            HooksRequest::AntigravityUninstall
        );
        let before = r#"{
  "theme": "user",
  "other": {"Stop": [{"command": "user command"}]},
  "cmux": {
    "Stop": [{"command": "cmux hooks antigravity stop"}],
    "UserEvent": [{"command": "user command inside reserved group"}]
  }
}"#;
        let (plan, removed) = plan_antigravity_hooks_uninstall(before, &path()).unwrap();
        assert!(removed);
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["theme"], "user");
        assert_eq!(value["other"]["Stop"][0]["command"], "user command");
        assert!(value.get("cmux").is_none());

        let mention_only = r#"{"cmux":{"Stop":[{"command":"echo cmux hooks antigravity stop"}]}}"#;
        let (plan, removed) = plan_antigravity_hooks_uninstall(mention_only, &path()).unwrap();
        assert!(!removed);
        assert!(
            serde_json::from_str::<serde_json::Value>(&plan.after).unwrap()["cmux"].is_object()
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
            true,
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
            !plan_opencode_registration_update(&plan.after, &path(), true)
                .unwrap()
                .changed
        );
        assert!(OPENCODE_SESSION_PLUGIN_SOURCE.contains("cmux-opencode-session-plugin-marker"));
        assert!(OPENCODE_FEED_PLUGIN_SOURCE.contains("cmux-feed-plugin-marker"));
    }

    #[test]
    fn opencode_uninstall_parses_scope_and_removes_only_session_registration() {
        assert_eq!(
            parse_hooks_request("hooks", &["opencode".into(), "uninstall".into()]).unwrap(),
            HooksRequest::OpenCodeUninstall { project: false }
        );
        assert_eq!(
            parse_hooks_request(
                "hooks",
                &["opencode".into(), "uninstall".into(), "--project".into()]
            )
            .unwrap(),
            HooksRequest::OpenCodeUninstall { project: true }
        );

        let path = Path::new("opencode.json");
        let before = r#"{
  "theme": "user",
  "plugin": [
    "user-plugin",
    "./plugins/cmux-session.js",
    ["cmux-session", {"enabled": true}],
    ["user-tuple", {"enabled": true}]
  ]
}"#;
        let plan = plan_opencode_registration_update(before, path, false).unwrap();
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["theme"], "user");
        assert_eq!(
            value["plugin"],
            serde_json::json!(["user-plugin", ["user-tuple", {"enabled": true}]])
        );
    }

    #[test]
    fn pi_extension_plan_uses_canonical_path_source_and_setup_alias() {
        assert_eq!(
            parse_hooks_request("hooks", &["pi".into(), "install".into(), "--yes".into()]).unwrap(),
            HooksRequest::Pi { yes: true }
        );
        assert_eq!(
            parse_hooks_request(
                "hooks",
                &["setup".into(), "--agent=pi".into(), "--yes".into()],
            )
            .unwrap(),
            HooksRequest::Pi { yes: true }
        );
        assert_eq!(
            pi_extension_path(Path::new("C:/Users/me/.pi/agent")),
            PathBuf::from("C:/Users/me/.pi/agent/extensions/cmux-session.ts")
        );
        assert!(PI_EXTENSION_SOURCE.contains("cmux-pi-session-extension-marker v2"));
        assert!(PI_EXTENSION_SOURCE.contains("[\"hooks\", \"feed\", \"--source\", \"pi\""));
    }

    #[test]
    fn omp_extension_plan_uses_canonical_path_source_and_setup_alias() {
        assert_eq!(
            parse_hooks_request("hooks", &["omp".into(), "install".into(), "--yes".into()])
                .unwrap(),
            HooksRequest::Omp { yes: true }
        );
        assert_eq!(
            parse_hooks_request(
                "hooks",
                &["setup".into(), "--agent=omp".into(), "--yes".into()],
            )
            .unwrap(),
            HooksRequest::Omp { yes: true }
        );
        assert_eq!(
            omp_extension_path(Path::new("C:/Users/me/.omp/agent")),
            PathBuf::from("C:/Users/me/.omp/agent/extensions/cmux-omp-session.ts")
        );
        assert!(OMP_EXTENSION_SOURCE.contains("cmux-omp-session-extension-marker v1"));
        assert!(OMP_EXTENSION_SOURCE.contains("[\"hooks\", \"omp\", subcommand]"));
    }

    #[test]
    fn amp_plugin_plan_uses_canonical_path_source_and_setup_alias() {
        assert_eq!(
            parse_hooks_request("hooks", &["amp".into(), "install".into(), "--yes".into()])
                .unwrap(),
            HooksRequest::Amp { yes: true }
        );
        assert_eq!(
            parse_hooks_request(
                "hooks",
                &["setup".into(), "--agent=amp".into(), "--yes".into()],
            )
            .unwrap(),
            HooksRequest::Amp { yes: true }
        );
        assert_eq!(
            amp_plugin_path(Path::new("C:/Users/me/.config/amp")),
            PathBuf::from("C:/Users/me/.config/amp/plugins/cmux-session.ts")
        );
        assert!(AMP_PLUGIN_SOURCE.contains("cmux-amp-session-extension-marker v2"));
        assert!(AMP_PLUGIN_SOURCE.contains("@i-know-the-amp-plugin-api-is-wip"));
    }

    #[test]
    fn rovo_yaml_plan_preserves_user_tree_and_replaces_owned_block() {
        assert_eq!(
            parse_hooks_request("hooks", &["rovo".into(), "install".into(), "--yes".into()])
                .unwrap(),
            HooksRequest::Rovo { yes: true }
        );
        let existing = "eventHooks:\n  nested:\n    events:\n      - name: user_hook\n        commands:\n          - command: \"echo user\"\n";
        let plan =
            plan_rovo_hooks_update(existing, Path::new("C:/Users/me/.rovodev/config.yml")).unwrap();
        assert!(plan
            .after
            .contains("eventHooks:\n  # cmux hooks rovodev begin\n  events:"));
        assert!(plan.after.contains("    events:\n      - name: user_hook"));
        assert!(plan.after.contains("- name: on_tool_permission"));
        assert!(plan.after.contains("cmux hooks rovodev prompt-submit"));
        assert!(
            !plan_rovo_hooks_update(&plan.after, &path())
                .unwrap()
                .changed
        );

        let dangling = "eventHooks:\n  events:\n    # cmux hooks rovodev begin\nsessions:\n  persistenceDir: /tmp/rovo\n";
        let dangling_plan = plan_rovo_hooks_update(dangling, &path()).unwrap();
        assert!(dangling_plan
            .after
            .contains("sessions:\n  persistenceDir: /tmp/rovo"));
        assert_eq!(
            dangling_plan
                .after
                .matches("# cmux hooks rovodev begin")
                .count(),
            2
        );
    }

    #[test]
    fn rovo_uninstall_removes_complete_blocks_and_preserves_dangling_markers() {
        assert_eq!(
            parse_hooks_request("hooks", &["rovodev".into(), "uninstall".into()]).unwrap(),
            HooksRequest::RovoUninstall
        );
        assert_eq!(
            parse_hooks_request("hooks", &["rovo".into(), "uninstall".into()]).unwrap(),
            HooksRequest::RovoUninstall
        );
        let before = "theme: user\n\neventHooks:\n  events:\n    # cmux hooks rovodev begin\n    - name: on_complete\n      commands:\n        - command: owned\n    # cmux hooks rovodev end\n    - name: user\n      commands:\n        - command: user\n";
        let plan = plan_rovo_hooks_uninstall(before, &path());
        assert!(plan.changed);
        assert!(!plan.after.contains(ROVO_BEGIN_MARKER));
        assert!(plan.after.contains("- name: user"));
        assert!(plan.after.contains("theme: user"));

        let dangling = "theme: user\n# cmux hooks rovodev begin\nuser: keep\n";
        let plan = plan_rovo_hooks_uninstall(dangling, &path());
        assert!(!plan.changed);
        assert_eq!(plan.after, dangling);
    }

    #[test]
    fn hermes_yaml_plan_preserves_user_hooks_and_builds_allowlist() {
        assert_eq!(
            parse_hooks_request(
                "hooks",
                &["hermes-agent".into(), "install".into(), "--yes".into()],
            )
            .unwrap(),
            HooksRequest::Hermes { yes: true }
        );
        let existing = "model: test\nhooks:\n  pre_tool_call:\n    - command: \"echo user\"\n      timeout: 10\n";
        let plan = plan_hermes_hooks_update(existing, Path::new("C:/Users/me/.hermes/config.yaml"));
        assert!(plan.after.contains("# cmux hooks hermes-agent begin"));
        assert!(plan.after.contains("- command: \"echo user\""));
        assert!(plan
            .after
            .contains("cmux hooks feed --source hermes-agent --event pre_tool_call"));
        assert!(plan.after.contains("timeout: 120"));
        assert_eq!(plan.after.matches("\n  pre_approval_request:").count(), 1);
        assert!(!plan_hermes_hooks_update(&plan.after, &path()).changed);

        let inline = plan_hermes_hooks_update("hooks: [] # intentionally empty\n", &path());
        assert!(inline.after.contains("restore-line-base64:"));
        assert!(!plan_hermes_hooks_update(&inline.after, &path()).changed);

        let allowlist = plan_hermes_allowlist_update(
            r#"{"approvals":[{"event":"custom","command":"echo user","scope":"user"}]}"#,
        )
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&allowlist.after).unwrap();
        assert!(value["approvals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["scope"] == "user"));
        assert!(value["approvals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["event"] == "pre_tool_call"));
        assert!(
            !plan_hermes_allowlist_update(&allowlist.after)
                .unwrap()
                .changed
        );
    }

    #[test]
    fn hermes_uninstall_restores_yaml_and_removes_only_owned_approvals() {
        assert_eq!(
            parse_hooks_request("hooks", &["hermes-agent".into(), "uninstall".into()]).unwrap(),
            HooksRequest::HermesUninstall
        );
        assert_eq!(
            parse_hooks_request("hooks", &["hermes".into(), "uninstall".into()]).unwrap(),
            HooksRequest::HermesUninstall
        );
        let original = "model: user\nhooks: [] # intentionally empty\n";
        let installed = plan_hermes_hooks_update(original, &path());
        let removed = plan_hermes_hooks_uninstall(&installed.after, &path());
        assert!(removed.changed);
        assert_eq!(removed.after, original);

        let owned = &hermes_events()[0];
        let before = serde_json::json!({
            "version": 1,
            "approvals": [
                {"event": owned.name, "command": owned.command, "approved_at": "old"},
                {"event": "user", "command": "user command", "approved_at": "keep"},
                {"custom": "passthrough"}
            ]
        })
        .to_string();
        let plan = plan_hermes_allowlist_uninstall(&before).unwrap();
        let value: serde_json::Value = serde_json::from_str(&plan.after).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["approvals"].as_array().unwrap().len(), 2);
        assert_eq!(value["approvals"][0]["event"], "user");
        assert_eq!(value["approvals"][1]["custom"], "passthrough");
    }

    #[test]
    fn codex_plan_installs_nested_hooks_feature_and_trust() {
        assert_eq!(
            parse_hooks_request("hooks", &["codex".into(), "install".into(), "--yes".into()])
                .unwrap(),
            HooksRequest::Codex { yes: true }
        );
        let hooks_path = Path::new("C:/Users/me/.codex/hooks.json");
        let hooks = plan_codex_hooks_update(r#"{"theme":"dark"}"#, hooks_path).unwrap();
        let hooks_value: serde_json::Value = serde_json::from_str(&hooks.after).unwrap();
        assert_eq!(hooks_value["theme"], "dark");
        assert_eq!(hooks_value["hooks"].as_object().unwrap().len(), 10);
        assert_eq!(
            hooks_value["hooks"]["SessionStart"][0]["hooks"][0]["timeout"],
            5
        );
        assert_eq!(
            hooks_value["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"],
            5
        );
        let config = plan_codex_config_update(
            "[features]\nhooks = false # user setting\n",
            &hooks.after,
            hooks_path,
            Path::new("C:/Users/me/.codex/config.toml"),
        )
        .unwrap();
        assert!(config.after.contains("hooks = true"));
        assert!(config
            .after
            .contains("previous line: hooks = false # user setting"));
        assert_eq!(config.after.matches("[hooks.state.\"").count(), 10);
        assert_eq!(config.after.matches("trusted_hash = \"sha256:").count(), 10);
        let dotted = plan_codex_config_update(
            "features.experimental = true\n",
            &hooks.after,
            hooks_path,
            &path(),
        )
        .unwrap();
        assert!(dotted.after.contains("features.hooks = true"));
        assert!(!dotted.after.contains("[features]"));
        assert!(
            !plan_codex_config_update(&config.after, &hooks.after, hooks_path, &path())
                .unwrap()
                .changed
        );
    }

    #[test]
    fn codex_uninstall_removes_owned_hooks_and_restores_owned_feature_state() {
        assert_eq!(
            parse_hooks_request("hooks", &["codex".into(), "uninstall".into()]).unwrap(),
            HooksRequest::CodexUninstall
        );
        let hooks_path = Path::new("hooks.json");
        let hooks_before = r#"{"hooks":{"UserEvent":[{"hooks":[{"command":"user command"}]}]}}"#;
        let installed_hooks = plan_codex_hooks_update(hooks_before, hooks_path).unwrap();
        let config_path = Path::new("config.toml");
        let config_before = "model = \"user\"\n[features]\nhooks = false\n";
        let installed_config = plan_codex_config_update(
            config_before,
            &installed_hooks.after,
            hooks_path,
            config_path,
        )
        .unwrap();
        let (removed_hooks, count) =
            plan_nested_hooks_uninstall(&installed_hooks.after, hooks_path, &codex_agent_def())
                .unwrap();
        assert_eq!(count, 10);
        assert!(removed_hooks.after.contains("user command"));
        assert!(!removed_hooks.after.contains("cmux hooks codex"));
        let removed_config = plan_codex_config_uninstall(&installed_config.after, config_path);
        assert!(removed_config.after.contains("hooks = false"));
        assert!(!removed_config.after.contains(CODEX_FEATURE_BEGIN));
        assert!(!removed_config.after.contains(CODEX_TRUST_BEGIN));
        assert!(removed_config.after.contains("model = \"user\""));
    }

    #[test]
    fn marker_owned_agents_parse_uninstall_and_refuse_user_files() {
        assert_eq!(
            parse_hooks_request("hooks", &["pi".into(), "uninstall".into()]).unwrap(),
            HooksRequest::PiUninstall
        );
        assert_eq!(
            parse_hooks_request("hooks", &["omp".into(), "uninstall".into()]).unwrap(),
            HooksRequest::OmpUninstall
        );
        assert_eq!(
            parse_hooks_request("hooks", &["amp".into(), "uninstall".into()]).unwrap(),
            HooksRequest::AmpUninstall
        );
        assert_eq!(
            marked_removal(Some("user extension"), PI_EXTENSION_MARKER),
            MarkedRemoval::Refuse
        );
        assert_eq!(
            marked_removal(Some(PI_EXTENSION_SOURCE), PI_EXTENSION_MARKER),
            MarkedRemoval::Remove
        );
        assert_eq!(
            marked_removal(None, PI_EXTENSION_MARKER),
            MarkedRemoval::Missing
        );
    }

    #[test]
    fn namespace_uninstall_spellings_route_filtered_and_batch_requests() {
        assert_eq!(
            parse_hooks_request(
                "hooks",
                &["uninstall".into(), "--agent".into(), "kimi".into()]
            )
            .unwrap(),
            HooksRequest::KimiUninstall
        );
        assert_eq!(
            parse_hooks_request("hooks", &["uninstall".into(), "rovo".into()]).unwrap(),
            HooksRequest::RovoUninstall
        );
        assert_eq!(
            parse_hooks_request("uninstall-hooks", &["--agent=codex".into()]).unwrap(),
            HooksRequest::CodexUninstall
        );
        assert_eq!(
            parse_hooks_request(
                "hooks",
                &[
                    "setup".into(),
                    "--uninstall".into(),
                    "--agent".into(),
                    "cursor".into()
                ]
            )
            .unwrap(),
            HooksRequest::CursorUninstall
        );
        assert_eq!(
            parse_hooks_request("hooks", &["uninstall".into()]).unwrap(),
            HooksRequest::UninstallAll
        );
        assert_eq!(
            parse_hooks_request("uninstall-hooks", &[]).unwrap(),
            HooksRequest::UninstallAll
        );
    }

    #[test]
    fn kimi_toml_plan_preserves_user_config_and_replaces_owned_block() {
        assert_eq!(
            parse_hooks_request("hooks", &["kimi".into(), "install".into(), "--yes".into()])
                .unwrap(),
            HooksRequest::Kimi { yes: true }
        );
        let plan = plan_kimi_hooks_update(
            "model = \"kimi-k2\"\ntelemetry = false\n",
            Path::new("C:/Users/me/.kimi-code/config.toml"),
        );
        assert!(plan
            .after
            .starts_with("model = \"kimi-k2\"\ntelemetry = false\n\n"));
        assert_eq!(plan.after.matches("[[hooks]]").count(), 10);
        assert!(plan.after.contains("event = \"PermissionRequest\""));
        assert!(plan
            .after
            .contains("cmux hooks feed --source kimi --event PermissionRequest"));
        assert!(plan.after.contains("timeout = 120"));
        assert!(!plan_kimi_hooks_update(&plan.after, &path()).changed);

        let orphan = plan_kimi_hooks_update(
            &format!("model = \"kimi\"\n{KIMI_BEGIN_MARKER}\ntelemetry = false\n"),
            &path(),
        );
        assert!(orphan.after.contains("telemetry = false"));
        assert_eq!(orphan.after.matches(KIMI_BEGIN_MARKER).count(), 1);
        assert_eq!(
            toml_basic_string("cmux hooks \"kimi\" \\\tpermission"),
            "cmux hooks \\\"kimi\\\" \\\\\\tpermission"
        );
    }

    #[test]
    fn kimi_uninstall_removes_complete_and_orphan_markers_without_user_toml() {
        assert_eq!(
            parse_hooks_request("hooks", &["kimi".into(), "uninstall".into()]).unwrap(),
            HooksRequest::KimiUninstall
        );
        let before = format!(
            "model = \"kimi\"\n\n{KIMI_BEGIN_MARKER}\n[[hooks]]\nevent = \"Stop\"\ncommand = \"owned\"\ntimeout = 10\n\n{KIMI_END_MARKER}\n[user]\nkeep = true\n"
        );
        let plan = plan_kimi_hooks_uninstall(&before, &path());
        assert!(plan.changed);
        assert!(!plan.after.contains(KIMI_BEGIN_MARKER));
        assert!(plan.after.contains("model = \"kimi\""));
        assert!(plan.after.contains("[user]\nkeep = true"));

        let orphan = format!("model = \"kimi\"\n{KIMI_BEGIN_MARKER}\n[user]\nkeep = true\n");
        let plan = plan_kimi_hooks_uninstall(&orphan, &path());
        assert!(plan.changed);
        assert!(!plan.after.contains(KIMI_BEGIN_MARKER));
        assert!(plan.after.contains("[user]\nkeep = true"));
    }
}
