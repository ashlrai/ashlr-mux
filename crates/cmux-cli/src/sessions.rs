//! Local inspection of persisted agent-hook sessions.
//!
//! Canonical cmux writes one `*-hook-sessions.json` registry per supported
//! agent under `~/.cmuxterm`.  `sessions` and `session-debug` read those files
//! directly, so this command deliberately has no control-socket dependency.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use cmux_agent::{
    argv_looks_like_shell_wrapper, launcher_describes_kind, selected_environment,
    ClaudeConfigContext,
};
use cmux_agent_launch_sanitizer::{
    preserved_arguments, preserved_claude_teams_launch_arguments, preserved_codex_fork_arguments,
    removing_saved_working_directory_options,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::invocation::CliError;

pub const SESSIONS_USAGE: &str = r#"Usage: cmux sessions list [options]
       cmux sessions [options]

Print saved agent session records from ~/.cmuxterm/*-hook-sessions.json.
This command does not require a running cmux socket.
By default, broad output shows active, restorable, or transcript-backed records.
Pass --all to inspect every saved hook record.

Options:
  --agent <name>        Filter to one agent, for example codex or claude
  --session <id>        Filter to one agent session id
  --workspace <id>      Filter to one saved workspace id
  --surface <id>        Filter to one saved surface id
  --cwd <text>          Filter by saved cwd or launch working directory
  --state-dir <path>    Override hook state directory
  --codex-home <path>   Override the default Codex home used for transcript checks
  --limit <n>           Limit text output (default: 100)
  --all                 Print all matches
  --json                Print structured JSON

Codex rows include whether the saved id exists in CODEX_HOME/session_index.jsonl
and whether a matching transcript file exists under CODEX_HOME/sessions or
CODEX_HOME/archived_sessions.

Compatibility aliases:
  cmux sessions debug [options]
  cmux session-debug [options]"#;

#[derive(Clone, Copy)]
struct AgentSpec {
    name: &'static str,
    display_name: &'static str,
    store_suffix: &'static str,
    config_dir_env: Option<&'static str>,
    aliases: &'static [&'static str],
}

const AGENT_SPECS: &[AgentSpec] = &[
    AgentSpec {
        name: "claude",
        display_name: "Claude Code",
        store_suffix: "claude",
        config_dir_env: Some("CLAUDE_CONFIG_DIR"),
        aliases: &["claude-code", "claude_code"],
    },
    AgentSpec {
        name: "codex",
        display_name: "Codex",
        store_suffix: "codex",
        config_dir_env: Some("CODEX_HOME"),
        aliases: &[],
    },
    AgentSpec {
        name: "grok",
        display_name: "Grok",
        store_suffix: "grok",
        config_dir_env: Some("GROK_HOME"),
        aliases: &[],
    },
    AgentSpec {
        name: "opencode",
        display_name: "OpenCode",
        store_suffix: "opencode",
        config_dir_env: Some("OPENCODE_CONFIG_DIR"),
        aliases: &[],
    },
    AgentSpec {
        name: "pi",
        display_name: "Pi",
        store_suffix: "pi",
        config_dir_env: Some("PI_CODING_AGENT_DIR"),
        aliases: &[],
    },
    AgentSpec {
        name: "omp",
        display_name: "OMP",
        store_suffix: "omp",
        config_dir_env: None,
        aliases: &[],
    },
    AgentSpec {
        name: "amp",
        display_name: "Amp",
        store_suffix: "amp",
        config_dir_env: None,
        aliases: &[],
    },
    AgentSpec {
        name: "cursor",
        display_name: "Cursor",
        store_suffix: "cursor",
        config_dir_env: None,
        aliases: &[],
    },
    AgentSpec {
        name: "gemini",
        display_name: "Gemini",
        store_suffix: "gemini",
        config_dir_env: None,
        aliases: &[],
    },
    AgentSpec {
        name: "kiro",
        display_name: "Kiro",
        store_suffix: "kiro",
        config_dir_env: Some("KIRO_HOME"),
        aliases: &[],
    },
    AgentSpec {
        name: "antigravity",
        display_name: "Antigravity",
        store_suffix: "antigravity",
        config_dir_env: None,
        aliases: &["agy"],
    },
    AgentSpec {
        name: "rovodev",
        display_name: "Rovo Dev",
        store_suffix: "rovodev",
        config_dir_env: None,
        aliases: &["rovo"],
    },
    AgentSpec {
        name: "hermes-agent",
        display_name: "Hermes Agent",
        store_suffix: "hermes-agent",
        config_dir_env: Some("HERMES_HOME"),
        aliases: &[],
    },
    AgentSpec {
        name: "copilot",
        display_name: "Copilot",
        store_suffix: "copilot",
        config_dir_env: Some("COPILOT_HOME"),
        aliases: &[],
    },
    AgentSpec {
        name: "codebuddy",
        display_name: "CodeBuddy",
        store_suffix: "codebuddy",
        config_dir_env: Some("CODEBUDDY_CONFIG_DIR"),
        aliases: &[],
    },
    AgentSpec {
        name: "factory",
        display_name: "Factory",
        store_suffix: "factory",
        config_dir_env: None,
        aliases: &[],
    },
    AgentSpec {
        name: "qoder",
        display_name: "Qoder",
        store_suffix: "qoder",
        config_dir_env: Some("QODER_CONFIG_DIR"),
        aliases: &[],
    },
    AgentSpec {
        name: "kimi",
        display_name: "Kimi Code",
        store_suffix: "kimi",
        config_dir_env: Some("KIMI_CODE_HOME"),
        aliases: &[],
    },
];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionRecord {
    session_id: String,
    workspace_id: String,
    surface_id: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    transcript_path: Option<String>,
    #[serde(default)]
    pid: Option<i64>,
    #[serde(default)]
    launch_command: Option<LaunchCommand>,
    #[serde(default)]
    is_restorable: Option<bool>,
    #[serde(default)]
    agent_lifecycle: Option<String>,
    #[serde(default)]
    runtime_status: Option<String>,
    #[serde(default)]
    active_prompt_turn_id: Option<String>,
    #[serde(default)]
    last_prompt_turn_id: Option<String>,
    started_at: f64,
    updated_at: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LaunchCommand {
    #[serde(default)]
    launcher: Option<String>,
    #[serde(default)]
    executable_path: Option<String>,
    arguments: Vec<String>,
    #[serde(default)]
    working_directory: Option<String>,
    #[serde(default)]
    environment: Option<BTreeMap<String, String>>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActiveSessionRecord {
    session_id: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionStore {
    #[serde(default)]
    sessions: HashMap<String, SessionRecord>,
    #[serde(default)]
    active_sessions_by_workspace: HashMap<String, ActiveSessionRecord>,
    #[serde(default)]
    active_sessions_by_surface: HashMap<String, ActiveSessionRecord>,
}

#[derive(Default)]
struct ParsedOptions {
    agent: Option<String>,
    session: Option<String>,
    workspace: Option<String>,
    surface: Option<String>,
    cwd: Option<String>,
    state_dir: Option<String>,
    codex_home: Option<String>,
    limit: Option<String>,
    include_all: bool,
    json: bool,
}

#[derive(Default)]
struct CodexIndex {
    indexed_ids: HashSet<String>,
    transcript_by_id: HashMap<String, String>,
}

#[derive(Clone)]
struct SessionEntry {
    updated_at: f64,
    payload: Value,
}

struct ProcessIdentity {
    executable_path: Option<String>,
    arguments: Vec<String>,
    start_time: f64,
}

/// Execute canonical `sessions` / `session-debug` behavior using an injected
/// environment. The injection keeps tests and callers away from real user data.
pub fn run_sessions_command(
    raw_args: &[String],
    json_output: bool,
    environment: &BTreeMap<String, String>,
) -> Result<String, CliError> {
    let (options, help) = parse_options(raw_args, json_output)?;
    if help {
        return Ok(SESSIONS_USAGE.to_string());
    }

    let home = environment
        .get("HOME")
        .or_else(|| environment.get("USERPROFILE"))
        .cloned()
        .unwrap_or_default();
    let fallback_state_dir = if home.is_empty() {
        ".cmuxterm"
    } else {
        "~/.cmuxterm"
    };
    let state_dir = expand_tilde(
        options
            .state_dir
            .as_deref()
            .or_else(|| {
                environment
                    .get("CMUX_AGENT_HOOK_STATE_DIR")
                    .map(String::as_str)
            })
            .unwrap_or(fallback_state_dir),
        &home,
    );
    let fallback_codex_home = if home.is_empty() {
        ".codex"
    } else {
        "~/.codex"
    };
    let default_codex_home = expand_tilde(
        options
            .codex_home
            .as_deref()
            .or_else(|| environment.get("CODEX_HOME").map(String::as_str))
            .unwrap_or(fallback_codex_home),
        &home,
    );

    let selected_specs = selected_agent_specs(options.agent.as_deref())?;
    let session_filter = normalized(options.session.as_deref()).map(str::to_lowercase);
    let workspace_filter =
        normalized_id_ref(options.workspace.as_deref()).map(|s| s.to_lowercase());
    let surface_filter = normalized_id_ref(options.surface.as_deref()).map(|s| s.to_lowercase());
    let cwd_filter = normalized(options.cwd.as_deref()).map(str::to_lowercase);
    let has_record_filter = session_filter.is_some()
        || workspace_filter.is_some()
        || surface_filter.is_some()
        || cwd_filter.is_some();
    let limit = if options.include_all {
        usize::MAX
    } else if let Some(raw) = options.limit.as_deref() {
        raw.parse::<usize>()
            .ok()
            .filter(|value| *value > 0)
            .ok_or_else(|| CliError::new("sessions list: --limit must be a positive integer"))?
    } else {
        100
    };

    let mut stores = Vec::new();
    let mut entries = Vec::new();
    let mut codex_indexes: HashMap<String, CodexIndex> = HashMap::new();

    for spec in selected_specs {
        let store_path =
            PathBuf::from(&state_dir).join(format!("{}-hook-sessions.json", spec.store_suffix));
        if !store_path.exists() {
            stores.push(json!({
                "agent": spec.name,
                "exists": false,
                "path": path_string(&store_path),
                "session_count": 0
            }));
            continue;
        }

        let bytes = fs::read(&store_path).map_err(|error| {
            CliError::new(format!("failed to read {}: {error}", store_path.display()))
        })?;
        let store: SessionStore = serde_json::from_slice(&bytes).map_err(|error| {
            CliError::new(format!(
                "failed to decode {}: {error}",
                store_path.display()
            ))
        })?;
        stores.push(json!({
            "agent": spec.name,
            "exists": true,
            "path": path_string(&store_path),
            "session_count": store.sessions.len()
        }));

        for raw_record in store.sessions.values() {
            let record = if spec.name == "claude" {
                resolved_claude_workflow_record(raw_record, &home)
            } else {
                raw_record.clone()
            };
            let raw_id = raw_record.session_id.to_lowercase();
            let resolved_id = record.session_id.to_lowercase();
            if session_filter
                .as_ref()
                .is_some_and(|wanted| wanted != &raw_id && wanted != &resolved_id)
            {
                continue;
            }
            if workspace_filter
                .as_ref()
                .is_some_and(|wanted| wanted != &record.workspace_id.to_lowercase())
                || surface_filter
                    .as_ref()
                    .is_some_and(|wanted| wanted != &record.surface_id.to_lowercase())
            {
                continue;
            }
            if let Some(wanted) = cwd_filter.as_ref() {
                let cwd = record.cwd.as_deref().unwrap_or_default().to_lowercase();
                let launch_cwd = record
                    .launch_command
                    .as_ref()
                    .and_then(|command| command.working_directory.as_deref())
                    .unwrap_or_default()
                    .to_lowercase();
                if !cwd.contains(wanted) && !launch_cwd.contains(wanted) {
                    continue;
                }
            }

            let workspace_active = store.active_sessions_by_workspace.get(&record.workspace_id);
            let surface_active = store.active_sessions_by_surface.get(&record.surface_id);
            let active_for_workspace =
                active_record_matches(workspace_active, &record.session_id, &raw_record.session_id);
            let active_for_surface =
                active_record_matches(surface_active, &record.session_id, &raw_record.session_id);

            let mut payload = Map::new();
            payload.insert("agent".into(), json!(spec.name));
            payload.insert("agent_display_name".into(), json!(spec.display_name));
            payload.insert("session_id".into(), json!(record.session_id));
            if raw_record.session_id != record.session_id {
                payload.insert("hook_session_id".into(), json!(raw_record.session_id));
            }
            payload.insert("workspace_id".into(), json!(record.workspace_id));
            payload.insert("surface_id".into(), json!(record.surface_id));
            payload.insert("store_path".into(), json!(path_string(&store_path)));
            payload.insert("started_at".into(), json!(iso8601(record.started_at)));
            payload.insert("updated_at".into(), json!(iso8601(record.updated_at)));
            payload.insert("updated_at_unix".into(), json!(record.updated_at));
            insert_option(&mut payload, "cwd", record.cwd.as_ref());
            insert_option(
                &mut payload,
                "transcript_path",
                record.transcript_path.as_ref(),
            );
            insert_option(&mut payload, "pid", record.pid.as_ref());
            insert_option(
                &mut payload,
                "runtime_status",
                record.runtime_status.as_ref(),
            );
            insert_option(
                &mut payload,
                "agent_lifecycle",
                record.agent_lifecycle.as_ref(),
            );
            insert_option(
                &mut payload,
                "last_prompt_turn_id",
                record.last_prompt_turn_id.as_ref(),
            );
            insert_option(
                &mut payload,
                "active_prompt_turn_id",
                record.active_prompt_turn_id.as_ref(),
            );
            insert_option(
                &mut payload,
                "launch_working_directory",
                record
                    .launch_command
                    .as_ref()
                    .and_then(|command| command.working_directory.as_ref()),
            );
            payload.insert(
                "launch_arguments".into(),
                json!(record
                    .launch_command
                    .as_ref()
                    .map(|command| command.arguments.clone())
                    .unwrap_or_default()),
            );

            for (key, value) in fork_diagnostics(spec.name, &record, &home) {
                payload.insert(key, value);
            }
            payload.insert("active_for_workspace".into(), json!(active_for_workspace));
            payload.insert("active_for_surface".into(), json!(active_for_surface));
            insert_option(
                &mut payload,
                "active_workspace_session_id",
                workspace_active.map(|active| &active.session_id),
            );
            insert_option(
                &mut payload,
                "active_surface_session_id",
                surface_active.map(|active| &active.session_id),
            );
            insert_option(&mut payload, "is_restorable", record.is_restorable.as_ref());

            let transcript_backed = if spec.name == "codex" {
                let codex_home = record
                    .launch_command
                    .as_ref()
                    .and_then(|command| command.environment.as_ref())
                    .and_then(|env| env.get("CODEX_HOME"))
                    .and_then(|value| normalized(Some(value)))
                    .map(|value| expand_tilde(value, &home))
                    .unwrap_or_else(|| default_codex_home.clone());
                let index = codex_indexes
                    .entry(codex_home.clone())
                    .or_insert_with(|| build_codex_index(Path::new(&codex_home)));
                let saved_path = record
                    .transcript_path
                    .as_deref()
                    .and_then(|value| normalized(Some(value)))
                    .map(|value| expand_tilde(value, &home));
                let indexed_path = index.transcript_by_id.get(&record.session_id).cloned();
                let found = indexed_path.is_some()
                    || saved_path
                        .as_ref()
                        .is_some_and(|path| Path::new(path).exists());
                payload.insert("session_home".into(), json!(codex_home));
                payload.insert(
                    "session_dir".into(),
                    json!(path_string(&PathBuf::from(&codex_home).join("sessions"))),
                );
                payload.insert(
                    "codex_indexed".into(),
                    json!(index.indexed_ids.contains(&record.session_id)),
                );
                payload.insert("codex_transcript_found".into(), json!(found));
                payload.insert(
                    "codex_transcript_path".into(),
                    indexed_path
                        .or(saved_path)
                        .map_or(Value::Null, Value::String),
                );
                found
            } else if let Some(env_key) = spec.config_dir_env {
                if let Some(value) = record
                    .launch_command
                    .as_ref()
                    .and_then(|command| command.environment.as_ref())
                    .and_then(|env| env.get(env_key))
                    .and_then(|value| normalized(Some(value)))
                {
                    let session_home = expand_tilde(value, &home);
                    payload.insert("session_home".into(), json!(session_home));
                    payload.insert("session_dir".into(), json!(session_home));
                } else {
                    payload.insert("session_home".into(), Value::Null);
                    payload.insert("session_dir".into(), Value::Null);
                }
                transcript_path_exists(record.transcript_path.as_deref(), &home)
            } else {
                payload.insert("session_home".into(), Value::Null);
                payload.insert("session_dir".into(), Value::Null);
                transcript_path_exists(record.transcript_path.as_deref(), &home)
            };
            payload.insert("transcript_backed".into(), json!(transcript_backed));

            let launch_backed = record.launch_command.is_some()
                && has_durable_resume_evidence(spec.name, record.launch_command.as_ref());
            payload.insert("launch_backed".into(), json!(launch_backed));
            let default_visible = active_for_workspace
                || active_for_surface
                || record.is_restorable == Some(true)
                || launch_backed
                || transcript_backed;
            payload.insert("default_visible".into(), json!(default_visible));
            if !options.include_all && !has_record_filter && !default_visible {
                continue;
            }
            entries.push(SessionEntry {
                updated_at: record.updated_at,
                payload: Value::Object(payload),
            });
        }
    }

    entries.sort_by(|left, right| {
        right
            .updated_at
            .total_cmp(&left.updated_at)
            .then_with(|| session_id(&left.payload).cmp(session_id(&right.payload)))
    });
    let total_matches = entries.len();
    let visible = entries.iter().take(limit).cloned().collect::<Vec<_>>();

    if options.json {
        return serde_json::to_string_pretty(&json!({
            "default_codex_home": default_codex_home,
            "limit": if limit == usize::MAX { Value::Null } else { json!(limit) },
            "sessions": visible.into_iter().map(|entry| entry.payload).collect::<Vec<_>>(),
            "state_dir": state_dir,
            "stores": stores,
            "total_matches": total_matches
        }))
        .map_err(|error| CliError::new(format!("failed to encode sessions output: {error}")));
    }

    if visible.is_empty() {
        return Ok(format!(
            "No saved agent sessions matched.\nstate_dir={state_dir}"
        ));
    }
    let mut lines = visible
        .iter()
        .map(|entry| render_session_line(&entry.payload))
        .collect::<Vec<_>>();
    if total_matches > visible.len() {
        lines.push(format!(
            "... {} more. Pass --all or --limit <n>.",
            total_matches - visible.len()
        ));
    }
    Ok(lines.join("\n"))
}

fn parse_options(
    raw_args: &[String],
    json_output: bool,
) -> Result<(ParsedOptions, bool), CliError> {
    let mut args = raw_args.to_vec();
    if let Some(first) = args.first().map(|value| value.trim().to_lowercase()) {
        if first == "debug" || first == "list" {
            args.remove(0);
        } else if first == "help" {
            return Ok((ParsedOptions::default(), true));
        } else if !first.starts_with('-') {
            return Err(CliError::new(format!(
                "Unknown sessions subcommand: {first}. Usage: cmux sessions list [options]"
            )));
        }
    }

    let (agent, args) = take_option(&args, "--agent");
    let (session, args) = take_option(&args, "--session");
    let (workspace, args) = take_option(&args, "--workspace");
    let (surface, args) = take_option(&args, "--surface");
    let (cwd, args) = take_option(&args, "--cwd");
    let (state_dir, args) = take_option(&args, "--state-dir");
    let (codex_home, args) = take_option(&args, "--codex-home");
    let (limit, args) = take_option(&args, "--limit");

    let mut options = ParsedOptions {
        agent,
        session,
        workspace,
        surface,
        cwd,
        state_dir,
        codex_home,
        limit,
        include_all: false,
        json: json_output,
    };
    let mut remaining = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--all" => options.include_all = true,
            "--json" => options.json = true,
            _ => remaining.push(arg),
        }
    }
    if let Some(flag) = remaining.iter().find(|argument| argument.starts_with('-')) {
        return Err(CliError::new(format!(
            "sessions list: unknown flag '{flag}'"
        )));
    }
    if let Some(argument) = remaining.first() {
        return Err(CliError::new(format!(
            "sessions list: unexpected argument '{argument}'"
        )));
    }
    Ok((options, false))
}

fn take_option(args: &[String], name: &str) -> (Option<String>, Vec<String>) {
    let mut value = None;
    let mut remaining = Vec::new();
    let mut index = 0;
    let mut past_terminator = false;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--" {
            past_terminator = true;
            remaining.push(arg.clone());
            index += 1;
            continue;
        }
        if !past_terminator {
            if let Some(inline) = arg.strip_prefix(&format!("{name}=")) {
                value = Some(inline.to_string());
                index += 1;
                continue;
            }
            if arg == name && index + 1 < args.len() {
                value = Some(args[index + 1].clone());
                index += 2;
                continue;
            }
        }
        remaining.push(arg.clone());
        index += 1;
    }
    (value, remaining)
}

fn selected_agent_specs(raw: Option<&str>) -> Result<Vec<AgentSpec>, CliError> {
    let Some(raw) = raw else {
        return Ok(AGENT_SPECS.to_vec());
    };
    let normalized = raw.trim().to_lowercase();
    if normalized.is_empty() {
        return Err(CliError::new("sessions list: --agent requires a value"));
    }
    AGENT_SPECS
        .iter()
        .find(|spec| spec.name == normalized || spec.aliases.contains(&normalized.as_str()))
        .copied()
        .map(|spec| vec![spec])
        .ok_or_else(|| CliError::new(format!("sessions list: unknown agent '{raw}'")))
}

fn active_record_matches(active: Option<&ActiveSessionRecord>, resolved: &str, raw: &str) -> bool {
    active.is_some_and(|active| active.session_id == resolved || active.session_id == raw)
}

fn insert_option<T: serde::Serialize>(map: &mut Map<String, Value>, key: &str, value: Option<&T>) {
    map.insert(
        key.to_string(),
        value
            .and_then(|value| serde_json::to_value(value).ok())
            .unwrap_or(Value::Null),
    );
}

fn normalized(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn normalized_id_ref(value: Option<&str>) -> Option<String> {
    let normalized = normalized(value)?;
    uuid_substrings(normalized)
        .last()
        .cloned()
        .or_else(|| Some(normalized.to_string()))
}

fn uuid_substrings(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    if bytes.len() < 36 {
        return Vec::new();
    }
    (0..=bytes.len() - 36)
        .filter_map(|index| {
            let candidate = std::str::from_utf8(&bytes[index..index + 36]).ok()?;
            Uuid::parse_str(candidate)
                .ok()
                .map(|_| candidate.to_lowercase())
        })
        .collect()
}

fn expand_tilde(value: &str, home: &str) -> String {
    if home.is_empty() {
        return value.to_string();
    }
    if value == "~" {
        return home.to_string();
    }
    if let Some(rest) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        return path_string(&PathBuf::from(home).join(rest));
    }
    value.to_string()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn transcript_path_exists(path: Option<&str>, home: &str) -> bool {
    normalized(path)
        .map(|path| Path::new(&expand_tilde(path, home)).exists())
        .unwrap_or(false)
}

fn build_codex_index(home: &Path) -> CodexIndex {
    let mut index = CodexIndex::default();
    if let Ok(contents) = fs::read_to_string(home.join("session_index.jsonl")) {
        for line in contents.lines() {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                if let Some(id) = value
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| normalized(Some(id)))
                {
                    index.indexed_ids.insert(id.to_string());
                }
            }
        }
    }
    for root in [home.join("sessions"), home.join("archived_sessions")] {
        collect_codex_transcripts(&root, &mut index.transcript_by_id);
    }
    index
}

fn collect_codex_transcripts(root: &Path, by_id: &mut HashMap<String, String>) {
    let Ok(children) = fs::read_dir(root) else {
        return;
    };
    for child in children.flatten() {
        let path = child.path();
        if child.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_codex_transcripts(&path, by_id);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            for id in uuid_substrings(name) {
                by_id.entry(id).or_insert_with(|| path_string(&path));
            }
        }
    }
}

fn resolved_claude_workflow_record(record: &SessionRecord, home: &str) -> SessionRecord {
    if !safe_session_filename(&record.session_id)
        || record
            .transcript_path
            .as_deref()
            .and_then(|value| normalized(Some(value)))
            .is_some_and(|path| regular_nonempty_file(Path::new(&expand_tilde(path, home))))
    {
        return record.clone();
    }
    let roots = claude_config_roots(record, home);
    let mut project_dirs = Vec::new();
    let mut seen = HashSet::new();
    let cwd_candidates = [
        record
            .launch_command
            .as_ref()
            .and_then(|command| command.working_directory.as_deref())
            .and_then(|value| normalized(Some(value))),
        record
            .cwd
            .as_deref()
            .and_then(|value| normalized(Some(value))),
    ];
    for root in roots {
        let projects_root = PathBuf::from(root).join("projects");
        for cwd in cwd_candidates.into_iter().flatten() {
            append_workflow_project(
                projects_root.join(encode_claude_project_dir(cwd)),
                &record.session_id,
                &mut seen,
                &mut project_dirs,
            );
        }
        if let Ok(children) = fs::read_dir(&projects_root) {
            for child in children.flatten().filter(|child| child.path().is_dir()) {
                append_workflow_project(
                    child.path(),
                    &record.session_id,
                    &mut seen,
                    &mut project_dirs,
                );
            }
        }
    }
    let mut matches = Vec::new();
    for project in project_dirs {
        collect_claude_workflow_transcripts(&project, &record.session_id, 4, &mut matches);
    }
    if matches.len() != 1 {
        return record.clone();
    }
    let mut resolved = record.clone();
    resolved.session_id = matches[0].0.clone();
    resolved.transcript_path = Some(path_string(&matches[0].1));
    resolved
}

fn append_workflow_project(
    project: PathBuf,
    session_id: &str,
    seen: &mut HashSet<PathBuf>,
    output: &mut Vec<PathBuf>,
) {
    if project.join(session_id).is_dir() && seen.insert(project.clone()) {
        output.push(project);
    }
}

fn collect_claude_workflow_transcripts(
    directory: &Path,
    excluded_id: &str,
    remaining_depth: usize,
    matches: &mut Vec<(String, PathBuf)>,
) {
    let Ok(children) = fs::read_dir(directory) else {
        return;
    };
    for child in children.flatten() {
        let path = child.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            if id != excluded_id && safe_session_filename(id) && regular_nonempty_file(&path) {
                matches.push((id.to_string(), path));
            }
        } else if remaining_depth > 0 && path.is_dir() {
            collect_claude_workflow_transcripts(&path, excluded_id, remaining_depth - 1, matches);
        }
    }
}

fn claude_config_roots(record: &SessionRecord, home: &str) -> Vec<String> {
    if let Some(configured) = record
        .launch_command
        .as_ref()
        .and_then(|command| command.environment.as_ref())
        .and_then(|env| env.get("CLAUDE_CONFIG_DIR"))
        .and_then(|value| normalized(Some(value)))
    {
        return vec![preferred_claude_config_path(configured, home)];
    }
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();
    let accounts = PathBuf::from(home).join(".codex-accounts/claude");
    if let Ok(children) = fs::read_dir(accounts) {
        let mut paths = children
            .flatten()
            .filter(|child| child.path().is_dir())
            .map(|child| path_string(&child.path()))
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            if seen.insert(path.clone()) {
                roots.push(path);
            }
        }
    }
    for path in [
        PathBuf::from(home).join(".claude"),
        PathBuf::from(home).join(".subrouter/codex/claude"),
    ] {
        let preferred = preferred_claude_config_path(&path_string(&path), home);
        if seen.insert(preferred.clone()) {
            roots.push(preferred);
        }
    }
    roots
}

fn preferred_claude_config_path(raw: &str, home: &str) -> String {
    let expanded = PathBuf::from(expand_tilde(raw.trim(), home));
    let legacy_root = PathBuf::from(home).join(".subrouter/codex/claude");
    if let Ok(suffix) = expanded.strip_prefix(&legacy_root) {
        let candidate = PathBuf::from(home)
            .join(".codex-accounts/claude")
            .join(suffix);
        if candidate.is_dir() {
            return path_string(&candidate);
        }
    }
    path_string(&expanded)
}

fn claude_transcript_exists(record: &SessionRecord, home: &str) -> bool {
    if !safe_session_filename(&record.session_id) {
        return false;
    }
    let cwd = record
        .cwd
        .as_deref()
        .and_then(|value| normalized(Some(value)))
        .or_else(|| {
            record
                .launch_command
                .as_ref()
                .and_then(|command| command.working_directory.as_deref())
                .and_then(|value| normalized(Some(value)))
        });
    for root in claude_config_roots(record, home) {
        let projects = PathBuf::from(root).join("projects");
        if let Some(cwd) = cwd {
            if claude_transcript_in_project(
                &projects.join(encode_claude_project_dir(cwd)),
                &record.session_id,
            ) {
                return true;
            }
        }
        if let Ok(children) = fs::read_dir(projects) {
            for child in children.flatten().filter(|child| child.path().is_dir()) {
                if claude_transcript_in_project(&child.path(), &record.session_id) {
                    return true;
                }
            }
        }
    }
    false
}

fn claude_transcript_in_project(project: &Path, session_id: &str) -> bool {
    regular_nonempty_file(&project.join(format!("{session_id}.jsonl")))
        || regular_nonempty_file(
            &project
                .join(session_id)
                .join("messages")
                .join(format!("{session_id}.jsonl")),
        )
}

fn safe_session_filename(session_id: &str) -> bool {
    !session_id.is_empty()
        && session_id != "."
        && session_id != ".."
        && !session_id.contains(['/', '\\'])
}

fn encode_claude_project_dir(path: &str) -> String {
    // Claude's Windows project directories replace the drive separator too;
    // leaving `C:` here would make `PathBuf::join` produce a drive-relative
    // path and escape the configured `projects` root.
    path.replace(['/', '\\', '.', ':'], "-")
}

fn regular_nonempty_file(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false)
}

fn has_durable_resume_evidence(kind: &str, launch: Option<&LaunchCommand>) -> bool {
    if launch
        .and_then(|launch| launch.source.as_deref())
        .and_then(|source| normalized(Some(source)))
        .is_some_and(|source| source.eq_ignore_ascii_case("rejected"))
    {
        return false;
    }
    if kind != "codex" {
        return true;
    }
    let Some(launch) = launch else { return true };
    if launch
        .environment
        .as_ref()
        .and_then(|env| env.get("CODEX_HOME"))
        .and_then(|value| normalized(Some(value)))
        .is_some()
    {
        return true;
    }
    let source = launch
        .source
        .as_deref()
        .and_then(|value| normalized(Some(value)));
    if source.is_some_and(|source| source.eq_ignore_ascii_case("default")) {
        return true;
    }
    if launch.arguments.is_empty() {
        return false;
    }
    if source.is_some_and(|source| source.eq_ignore_ascii_case("environment"))
        && codex_environment_is_weak(launch.environment.as_ref())
    {
        return false;
    }
    source.is_none()
        || source.is_some_and(|source| {
            source.eq_ignore_ascii_case("environment") || source.eq_ignore_ascii_case("process")
        })
}

fn codex_environment_is_weak(environment: Option<&BTreeMap<String, String>>) -> bool {
    let value = |key| {
        environment
            .and_then(|environment| environment.get(key))
            .and_then(|value| normalized(Some(value)))
    };
    value("CODEX_HOME").is_none()
        && (value("ANTHROPIC_BASE_URL").is_some() || value("CLAUDE_CONFIG_DIR").is_some())
}

fn fork_diagnostics(agent: &str, record: &SessionRecord, home: &str) -> Map<String, Value> {
    let stored_pid_exists = stored_pid_exists(record.pid);
    let hook_record_restorable = if agent == "claude" {
        record
            .transcript_path
            .as_deref()
            .and_then(|value| normalized(Some(value)))
            .is_some_and(|path| regular_nonempty_file(Path::new(&expand_tilde(path, home))))
            || claude_transcript_exists(record, home)
    } else {
        record.is_restorable != Some(false)
    };
    let trusted = trusted_launch_command(agent, record);
    let fork_arguments = hook_record_restorable
        .then(|| build_fork_arguments(agent, record, trusted))
        .flatten();
    let fork_command_available = fork_arguments.is_some();
    let (fork_supported, unavailable_reason) = fork_support(
        agent,
        record,
        trusted,
        hook_record_restorable,
        fork_command_available,
    );
    let fork_startup_input_available = fork_arguments.as_ref().is_some_and(|arguments| {
        fork_startup_input_available(arguments, agent, record, trusted, home)
    });
    let process = record.pid.and_then(process_identity);
    let stale_pid = record.pid.is_some_and(|_| {
        hook_record_restorable && !stored_process_matches_record(agent, record, process.as_ref())
    });

    let mut diagnostics = Map::new();
    diagnostics.insert(
        "fork_command_available".into(),
        json!(fork_command_available),
    );
    diagnostics.insert("fork_supported".into(), json!(fork_supported));
    diagnostics.insert("fork_unavailable_reason".into(), json!(unavailable_reason));
    diagnostics.insert(
        "fork_startup_input_available".into(),
        json!(fork_startup_input_available),
    );
    diagnostics.insert(
        "hook_record_restorable".into(),
        json!(hook_record_restorable),
    );
    diagnostics.insert(
        "stale_pid_blocks_restore_in_0_64_17".into(),
        json!(stale_pid),
    );
    diagnostics.insert(
        "stored_pid_exists".into(),
        stored_pid_exists.map_or(Value::Null, Value::Bool),
    );
    if let Some(process) = process {
        diagnostics.insert("stored_pid_arguments".into(), json!(process.arguments));
    }
    diagnostics
}

fn trusted_launch_command<'a>(agent: &str, record: &'a SessionRecord) -> Option<&'a LaunchCommand> {
    let launch = record.launch_command.as_ref()?;
    if launcher_describes_kind(launch.launcher.as_deref(), agent)
        && !argv_looks_like_shell_wrapper(&launch.arguments)
    {
        Some(launch)
    } else {
        None
    }
}

fn build_fork_arguments(
    agent: &str,
    record: &SessionRecord,
    launch: Option<&LaunchCommand>,
) -> Option<Vec<String>> {
    let session_id = normalized(Some(&record.session_id))?;
    let arguments = launch
        .map(|launch| launch.arguments.as_slice())
        .unwrap_or_default();
    let executable_path = launch.and_then(|launch| launch.executable_path.as_deref());
    let launcher = launch.and_then(|launch| launch.launcher.as_deref());
    let (executable, mut tail) = command_parts(executable_path, arguments, "cmux");
    match launcher {
        Some("claudeTeams") => {
            if tail.first().map(String::as_str) == Some("claude-teams") {
                tail.remove(0);
            }
            let mut result = vec![
                executable,
                "claude-teams".into(),
                "--resume".into(),
                session_id.into(),
                "--fork-session".into(),
            ];
            result.extend(preserved_claude_teams_launch_arguments(&tail)?);
            return Some(result);
        }
        Some("codexTeams") => {
            if tail.first().map(String::as_str) == Some("codex-teams") {
                tail.remove(0);
            }
            let mut result = vec![
                executable,
                "codex-teams".into(),
                "fork".into(),
                session_id.into(),
            ];
            result.extend(preserved_codex_fork_arguments(&tail)?);
            return Some(result);
        }
        Some("omo") => {
            if tail.first().map(String::as_str) == Some("omo") {
                tail.remove(0);
            }
            let mut result = vec![
                executable,
                "omo".into(),
                "--session".into(),
                session_id.into(),
                "--fork".into(),
            ];
            result.extend(preserved_arguments("opencode", &tail)?);
            return Some(result);
        }
        Some("omx" | "omc") => return None,
        _ => {}
    }

    match agent {
        "claude" => {
            let (_, tail) = command_parts(executable_path, arguments, "claude");
            let mut result = vec![
                "claude".into(),
                "--resume".into(),
                session_id.into(),
                "--fork-session".into(),
            ];
            result.extend(preserved_arguments("claude", &tail)?);
            Some(result)
        }
        "codex" => {
            let (executable, tail) = command_parts(executable_path, arguments, "codex");
            let mut result = vec![executable, "fork".into(), session_id.into()];
            result.extend(preserved_codex_fork_arguments(&tail)?);
            Some(result)
        }
        "opencode" | "pi" | "omp" => {
            let (executable, tail) = command_parts(executable_path, arguments, agent);
            let mut result = vec![
                executable,
                "--session".into(),
                session_id.into(),
                "--fork".into(),
            ];
            result.extend(preserved_arguments(agent, &tail)?);
            Some(result)
        }
        _ => None,
    }
}

fn command_parts(
    executable_path: Option<&str>,
    arguments: &[String],
    fallback: &str,
) -> (String, Vec<String>) {
    let executable = executable_path
        .and_then(|value| normalized(Some(value)))
        .or_else(|| arguments.first().and_then(|value| normalized(Some(value))))
        .unwrap_or(fallback)
        .to_string();
    let tail = arguments.get(1..).unwrap_or_default().to_vec();
    (executable, tail)
}

fn fork_support(
    agent: &str,
    record: &SessionRecord,
    launch: Option<&LaunchCommand>,
    restorable: bool,
    command_available: bool,
) -> (bool, &'static str) {
    if !restorable {
        return (false, "record_marked_non_restorable");
    }
    if !command_available {
        return (false, "agent_has_no_fork_command");
    }
    if agent != "opencode" {
        return (true, "available");
    }
    if launch.and_then(|launch| launch.launcher.as_deref()) == Some("omo") {
        return (true, "available");
    }
    let working_directory = launch
        .and_then(|launch| launch.working_directory.as_deref())
        .or(record.cwd.as_deref())
        .and_then(|value| normalized(Some(value)));
    if working_directory.is_some_and(|path| !Path::new(path).is_dir()) {
        return (true, "available");
    }
    let executable = launch.and_then(|launch| {
        launch
            .executable_path
            .as_deref()
            .and_then(|value| normalized(Some(value)))
            .or_else(|| {
                launch
                    .arguments
                    .first()
                    .and_then(|value| normalized(Some(value)))
            })
    });
    if executable.is_some_and(|executable| {
        (executable.starts_with('/') || Path::new(executable).is_absolute())
            && !Path::new(executable).is_file()
    }) {
        return (false, "opencode_executable_missing");
    }
    (false, "opencode_version_unverified")
}

fn fork_startup_input_available(
    arguments: &[String],
    agent: &str,
    record: &SessionRecord,
    launch: Option<&LaunchCommand>,
    home: &str,
) -> bool {
    let mut parts = Vec::new();
    if let Some(environment) = launch.and_then(|launch| launch.environment.as_ref()) {
        let context = ClaudeConfigContext {
            home_directory: home.replace('\\', "/"),
            directory_exists: Box::new(|path| Path::new(path).is_dir()),
        };
        let selected = selected_environment(environment, Some(agent), &context);
        if !selected.is_empty() {
            parts.push("env".to_string());
            let mut preserved_claude_keys = Vec::new();
            for (key, value) in selected {
                parts.push(format!("{key}={value}"));
                if agent == "claude" && CLAUDE_AUTH_SELECTION_KEYS.contains(&key.as_str()) {
                    preserved_claude_keys.push(key);
                }
            }
            if !preserved_claude_keys.is_empty() {
                parts.push("CMUX_PRESERVE_CLAUDE_AUTH_SELECTION_ENV=1".into());
                parts.push(format!(
                    "CMUX_PRESERVE_CLAUDE_AUTH_SELECTION_ENV_KEYS={}",
                    preserved_claude_keys.join(",")
                ));
            }
        }
    }
    parts.extend_from_slice(arguments);
    let working_directory = launch
        .and_then(|launch| launch.working_directory.as_deref())
        .or(record.cwd.as_deref())
        .and_then(|value| normalized(Some(value)));
    let parts = removing_saved_working_directory_options(&parts, working_directory);
    let command = render_agent_shell_command(&parts, agent);
    let command = if let Some(working_directory) = working_directory {
        let quoted = shell_single_quoted(working_directory);
        format!("cd -- {quoted} 2>/dev/null || [ ! -d {quoted} ] && {command}")
    } else {
        command
    };
    command.len() < 900
}

const CLAUDE_AUTH_SELECTION_KEYS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CONFIG_DIR",
];

const CLAUDE_WRAPPER_TOKEN: &str = r#""$([ -x "${CMUX_CLAUDE_WRAPPER_SHIM:-}" ] && printf '%s' "$CMUX_CLAUDE_WRAPPER_SHIM" || printf claude)""#;
const CODEX_WRAPPER_TOKEN: &str = r#""$([ -x "${CMUX_CODEX_WRAPPER_SHIM:-}" ] && printf '%s' "$CMUX_CODEX_WRAPPER_SHIM" || printf codex)""#;

fn render_agent_shell_command(parts: &[String], agent: &str) -> String {
    let wrapper = match agent {
        "claude" => Some(("claude", CLAUDE_WRAPPER_TOKEN)),
        "codex" => Some(("codex", CODEX_WRAPPER_TOKEN)),
        _ => None,
    };
    let mut replaced = false;
    let rendered = parts
        .iter()
        .map(|part| {
            if let Some((executable, token)) = wrapper {
                if !replaced && part == executable {
                    replaced = true;
                    return token.to_string();
                }
            }
            shell_single_quoted(part)
        })
        .collect::<Vec<_>>()
        .join(" ");
    if replaced {
        format!("/bin/sh -c {}", shell_single_quoted(&rendered))
    } else {
        rendered
    }
}

fn shell_single_quoted(value: &str) -> String {
    if !value.is_ascii() {
        let octal = value
            .as_bytes()
            .iter()
            .map(|byte| format!("\\{byte:03o}"))
            .collect::<String>();
        return format!(r#""$(printf '{octal}')""#);
    }
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn stored_pid_exists(pid: Option<i64>) -> Option<bool> {
    let pid = pid?;
    if pid <= 0 || pid > i64::from(i32::MAX) {
        return None;
    }
    Some(process_exists(pid))
}

fn process_exists(pid: i64) -> bool {
    u32::try_from(pid)
        .ok()
        .and_then(cmux_process::process_creation_time)
        .is_some()
}

fn stored_process_matches_record(
    agent: &str,
    record: &SessionRecord,
    process: Option<&ProcessIdentity>,
) -> bool {
    let Some(process) = process else { return false };
    if process.start_time > record.updated_at + 5.0 {
        return false;
    }
    let recorded = record
        .launch_command
        .as_ref()
        .and_then(|launch| {
            launch
                .executable_path
                .as_deref()
                .and_then(|value| normalized(Some(value)))
                .or_else(|| {
                    launch
                        .arguments
                        .first()
                        .and_then(|value| normalized(Some(value)))
                })
        })
        .and_then(executable_basename);
    let live = process
        .executable_path
        .as_deref()
        .and_then(|value| normalized(Some(value)))
        .or_else(|| process.arguments.first().map(String::as_str))
        .and_then(executable_basename);
    let (Some(recorded), Some(live)) = (recorded, live) else {
        return true;
    };
    if recorded.eq_ignore_ascii_case(&live) {
        return true;
    }
    if agent != "claude" || !(live.eq_ignore_ascii_case("node") || live.eq_ignore_ascii_case("bun"))
    {
        return false;
    }
    process.arguments.iter().skip(1).any(|argument| {
        let normalized = argument.replace('\\', "/").to_lowercase();
        executable_basename(argument).is_some_and(|name| name.eq_ignore_ascii_case("claude"))
            || normalized.contains("/.claude/")
            || normalized.contains("/claude/versions/")
    })
}

fn executable_basename(value: &str) -> Option<String> {
    Path::new(value)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
}

#[cfg(windows)]
fn process_identity(pid: i64) -> Option<ProcessIdentity> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessCommandLineInformation,
    };
    use windows::Win32::Foundation::{CloseHandle, LocalFree, HLOCAL, UNICODE_STRING};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Shell::CommandLineToArgvW;

    let pid = u32::try_from(pid).ok()?;
    let created = cmux_process::process_creation_time(pid)?;
    let start_time = created as f64 / 10_000_000.0 - 11_644_473_600.0;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;

        let mut executable_buffer = vec![0u16; 32_768];
        let mut executable_length = executable_buffer.len() as u32;
        let executable_path = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(executable_buffer.as_mut_ptr()),
            &mut executable_length,
        )
        .ok()
        .map(|()| String::from_utf16_lossy(&executable_buffer[..executable_length as usize]));

        let mut required = 0u32;
        let _ = NtQueryInformationProcess(
            handle,
            ProcessCommandLineInformation,
            std::ptr::null_mut(),
            0,
            &mut required,
        );
        let arguments = if required >= std::mem::size_of::<UNICODE_STRING>() as u32 {
            let word_size = std::mem::size_of::<usize>();
            let mut storage = vec![0usize; (required as usize).div_ceil(word_size)];
            let status = NtQueryInformationProcess(
                handle,
                ProcessCommandLineInformation,
                storage.as_mut_ptr().cast(),
                (storage.len() * word_size) as u32,
                &mut required,
            );
            if status.0 >= 0 {
                let command_line = &*storage.as_ptr().cast::<UNICODE_STRING>();
                let wide = std::slice::from_raw_parts(
                    command_line.Buffer.0,
                    usize::from(command_line.Length) / 2,
                );
                let mut terminated = wide.to_vec();
                terminated.push(0);
                let mut count = 0i32;
                let argv = CommandLineToArgvW(PCWSTR(terminated.as_ptr()), &mut count);
                if argv.is_null() || count <= 0 {
                    Vec::new()
                } else {
                    let values = (0..count as usize)
                        .filter_map(|index| PCWSTR((*argv.add(index)).0).to_string().ok())
                        .collect::<Vec<_>>();
                    let _ = LocalFree(Some(HLOCAL(argv.cast())));
                    values
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let _ = CloseHandle(handle);
        Some(ProcessIdentity {
            executable_path,
            arguments,
            start_time,
        })
    }
}

#[cfg(not(windows))]
fn process_identity(_pid: i64) -> Option<ProcessIdentity> {
    None
}

fn iso8601(value: f64) -> String {
    let nanos = (value * 1_000_000_000.0) as i128;
    let Ok(timestamp) = OffsetDateTime::from_unix_timestamp_nanos(nanos) else {
        return value.to_string();
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        timestamp.year(),
        timestamp.month() as u8,
        timestamp.day(),
        timestamp.hour(),
        timestamp.minute(),
        timestamp.second(),
        timestamp.millisecond()
    )
}

fn render_session_line(payload: &Value) -> String {
    let text = |key: &str| payload.get(key).and_then(Value::as_str).unwrap_or("-");
    let yes_no = |key: &str| {
        if payload.get(key).and_then(Value::as_bool) == Some(true) {
            "yes"
        } else {
            "no"
        }
    };
    let agent = text("agent");
    let mut parts = vec![
        format!("{agent} {}", text("session_id")),
        format!("workspace={}", text("workspace_id")),
        format!("surface={}", text("surface_id")),
        format!("cwd={}", text("cwd")),
        format!("active_ws={}", yes_no("active_for_workspace")),
        format!("active_surface={}", yes_no("active_for_surface")),
        format!("updated={}", text("updated_at")),
    ];
    if agent == "codex" {
        parts.push(format!("session_home={}", text("session_home")));
        parts.push(format!("codex_indexed={}", yes_no("codex_indexed")));
        parts.push(format!(
            "codex_transcript={}",
            yes_no("codex_transcript_found")
        ));
    } else {
        parts.push(format!("session_dir={}", text("session_dir")));
    }
    parts.push(format!("fork_command={}", yes_no("fork_command_available")));
    parts.push(format!("fork={}", yes_no("fork_supported")));
    if payload
        .get("stored_pid_exists")
        .is_some_and(Value::is_boolean)
    {
        parts.push(format!("pid_exists={}", yes_no("stored_pid_exists")));
    }
    parts.join("  ")
}

fn session_id(payload: &Value) -> &str {
    payload
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn environment(root: &Path, state: &Path, codex: &Path) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("HOME".to_string(), path_string(root)),
            ("CMUX_AGENT_HOOK_STATE_DIR".to_string(), path_string(state)),
            ("CODEX_HOME".to_string(), path_string(codex)),
        ])
    }

    #[test]
    fn default_visibility_filters_and_all_match_canonical_store_contract() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let codex = root.path().join("codex");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&codex).unwrap();
        let transcript = root.path().join("saved.jsonl");
        fs::write(&transcript, "{}\n").unwrap();
        fs::write(
            state.join("codex-hook-sessions.json"),
            serde_json::to_vec_pretty(&json!({
                "version": 1,
                "activeSessionsByWorkspace": {"workspace-active": {"sessionId": "active", "updatedAt": 20}},
                "activeSessionsBySurface": {"surface-active": {"sessionId": "active", "updatedAt": 20}},
                "sessions": {
                    "active": {"sessionId":"active","workspaceId":"workspace-active","surfaceId":"surface-active","cwd":"C:/repo/active","startedAt":10,"updatedAt":20},
                    "stale": {"sessionId":"stale","workspaceId":"workspace-stale","surfaceId":"surface-stale","cwd":"C:/repo/stale","startedAt":11,"updatedAt":21},
                    "launch": {"sessionId":"launch","workspaceId":"workspace-launch","surfaceId":"surface-launch","cwd":"C:/repo/launch","startedAt":12,"updatedAt":22,"launchCommand":{"launcher":"codex","executablePath":"codex","arguments":["codex","--model","gpt-5"],"workingDirectory":"C:/repo/launch","source":"process"}},
                    "transcript": {"sessionId":"transcript","workspaceId":"workspace-transcript","surfaceId":"surface-transcript","cwd":"C:/repo/transcript","transcriptPath":path_string(&transcript),"startedAt":13,"updatedAt":23}
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let env = environment(root.path(), &state, &codex);

        let output = run_sessions_command(
            &strings(&["list", "--agent", "codex", "--json"]),
            false,
            &env,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["total_matches"], 3);
        let ids = value["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|session| session["session_id"].as_str().unwrap())
            .collect::<HashSet<_>>();
        assert_eq!(ids, HashSet::from(["active", "launch", "transcript"]));

        let output = run_sessions_command(
            &strings(&["--agent", "codex", "--cwd", "C:/repo", "--json"]),
            false,
            &env,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["total_matches"], 4);

        let output = run_sessions_command(
            &strings(&["--agent=codex", "--all", "--limit", "bad", "--json"]),
            false,
            &env,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["limit"], Value::Null);
        assert_eq!(value["total_matches"], 4);
    }

    #[test]
    fn debug_alias_refs_and_codex_diagnostics_are_local_and_structured() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let codex = root.path().join("codex");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&codex).unwrap();
        let workspace = "33B0D372-292E-42BF-97B6-E37CCA79AB84";
        let surface = "A2AECAA9-EE1C-4999-B7A9-EE4BB4CDA5D8";
        fs::write(
            state.join("codex-hook-sessions.json"),
            serde_json::to_vec(&json!({"sessions": {"sid": {
                "sessionId":"sid","workspaceId":workspace,"surfaceId":surface,"pid":987654321,
                "startedAt":10,"updatedAt":20,
                "launchCommand":{"launcher":"codex","arguments":[],"workingDirectory":"Z:/remote/repo","environment":{"CODEX_HOME":path_string(&codex)},"source":"environment"}
            }}})).unwrap(),
        ).unwrap();
        let env = environment(root.path(), &state, &codex);
        let output = run_sessions_command(
            &strings(&[
                "debug",
                "--agent",
                "codex",
                "--workspace",
                &format!("workspace:1:{workspace}"),
                "--surface",
                &format!("surface:9:{surface}"),
                "--json",
            ]),
            false,
            &env,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        let session = &value["sessions"][0];
        assert_eq!(session["fork_command_available"], true);
        assert_eq!(session["fork_supported"], true);
        assert_eq!(session["fork_unavailable_reason"], "available");
        assert_eq!(session["fork_startup_input_available"], true);
        assert_eq!(session["stored_pid_exists"], false);
        assert_eq!(session["stale_pid_blocks_restore_in_0_64_17"], true);
    }

    #[test]
    fn claude_requires_a_nonempty_transcript_for_fork_restoration() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        fs::create_dir_all(&state).unwrap();
        let transcript = root.path().join("claude.jsonl");
        fs::write(&transcript, "{}\n").unwrap();
        fs::write(
            state.join("claude-hook-sessions.json"),
            serde_json::to_vec(&json!({"sessions": {
                "backed": {"sessionId":"backed","workspaceId":"w1","surfaceId":"s1","transcriptPath":path_string(&transcript),"isRestorable":false,"startedAt":10,"updatedAt":20,"launchCommand":{"launcher":"claude","arguments":["claude"],"source":"environment"}},
                "missing": {"sessionId":"missing","workspaceId":"w2","surfaceId":"s2","isRestorable":true,"startedAt":11,"updatedAt":21,"launchCommand":{"launcher":"claude","arguments":["claude"],"source":"environment"}}
            }})).unwrap(),
        ).unwrap();
        let env = environment(root.path(), &state, &root.path().join("codex"));
        let backed: Value = serde_json::from_str(
            &run_sessions_command(
                &strings(&["--agent", "claude", "--session", "backed", "--json"]),
                false,
                &env,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(backed["sessions"][0]["hook_record_restorable"], true);
        assert_eq!(backed["sessions"][0]["fork_supported"], true);

        let missing: Value = serde_json::from_str(
            &run_sessions_command(
                &strings(&["--agent", "claude", "--session", "missing", "--json"]),
                false,
                &env,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(missing["sessions"][0]["hook_record_restorable"], false);
        assert_eq!(
            missing["sessions"][0]["fork_unavailable_reason"],
            "record_marked_non_restorable"
        );
    }

    #[test]
    fn codex_index_and_nested_transcript_are_reported_without_executing_an_agent() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let codex = root.path().join("codex");
        let session_id = "019ee74a-3c84-7de3-84f1-ece32f4ecfbb";
        let transcript = codex
            .join("sessions")
            .join("2026")
            .join("07")
            .join("11")
            .join(format!("rollout-{session_id}.jsonl"));
        fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::write(&transcript, "{}\n").unwrap();
        fs::write(
            codex.join("session_index.jsonl"),
            format!("{{\"id\":\"{session_id}\"}}\nnot-json\n"),
        )
        .unwrap();
        fs::write(
            state.join("codex-hook-sessions.json"),
            serde_json::to_vec(&json!({"sessions": {session_id: {
                "sessionId":session_id,"workspaceId":"w","surfaceId":"s","startedAt":10,"updatedAt":20
            }}}))
            .unwrap(),
        )
        .unwrap();
        let env = environment(root.path(), &state, &codex);
        let output = run_sessions_command(
            &strings(&["--agent", "codex", "--session", session_id, "--json"]),
            false,
            &env,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        let session = &value["sessions"][0];
        assert_eq!(session["codex_indexed"], true);
        assert_eq!(session["codex_transcript_found"], true);
        assert_eq!(session["codex_transcript_path"], path_string(&transcript));
        assert_eq!(session["transcript_backed"], true);
    }

    #[test]
    fn claude_workflow_container_resolves_one_sibling_transcript() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let config = root.path().join("claude-config");
        let repo = root.path().join("repo.with.dot");
        let container_id = "workflow-container";
        let resolved_id = "resolved-claude-session";
        let project = config
            .join("projects")
            .join(encode_claude_project_dir(&path_string(&repo)));
        let transcript = project
            .join(container_id)
            .join("messages")
            .join(format!("{resolved_id}.jsonl"));
        fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        fs::create_dir_all(&state).unwrap();
        fs::write(&transcript, "{}\n").unwrap();
        fs::write(
            state.join("claude-hook-sessions.json"),
            serde_json::to_vec(&json!({"sessions": {container_id: {
                "sessionId":container_id,"workspaceId":"w","surfaceId":"s","cwd":path_string(&repo),
                "startedAt":10,"updatedAt":20,
                "launchCommand":{"launcher":"claude","arguments":["claude"],"workingDirectory":path_string(&repo),"environment":{"CLAUDE_CONFIG_DIR":path_string(&config)},"source":"environment"}
            }}}))
            .unwrap(),
        )
        .unwrap();
        let env = environment(root.path(), &state, &root.path().join("codex"));
        let output = run_sessions_command(
            &strings(&["--agent", "claude", "--session", container_id, "--json"]),
            false,
            &env,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        let session = &value["sessions"][0];
        assert_eq!(session["session_id"], resolved_id);
        assert_eq!(session["hook_session_id"], container_id);
        assert_eq!(session["transcript_path"], path_string(&transcript));
        assert_eq!(session["hook_record_restorable"], true);
        assert_eq!(session["fork_supported"], true);
    }

    #[test]
    fn local_opencode_diagnostic_never_runs_the_captured_executable() {
        let root = tempfile::tempdir().unwrap();
        let state = root.path().join("state");
        let repo = root.path().join("repo");
        let marker = root.path().join("must-not-exist");
        fs::create_dir_all(&state).unwrap();
        fs::create_dir_all(&repo).unwrap();
        fs::write(
            state.join("opencode-hook-sessions.json"),
            serde_json::to_vec(&json!({"sessions": {"sid": {
                "sessionId":"sid","workspaceId":"w","surfaceId":"s","cwd":path_string(&repo),
                "startedAt":10,"updatedAt":20,
                "launchCommand":{"launcher":"opencode","executablePath":"opencode","arguments":["opencode",path_string(&marker)],"workingDirectory":path_string(&repo),"source":"environment"}
            }}}))
            .unwrap(),
        )
        .unwrap();
        let env = environment(root.path(), &state, &root.path().join("codex"));
        let output = run_sessions_command(
            &strings(&["--agent", "opencode", "--session", "sid", "--json"]),
            false,
            &env,
        )
        .unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert_eq!(
            value["sessions"][0]["fork_unavailable_reason"],
            "opencode_version_unverified"
        );
        assert!(!marker.exists());
    }

    #[cfg(windows)]
    #[test]
    fn live_windows_process_identity_carries_arguments_and_creation_time() {
        let identity = process_identity(i64::from(std::process::id())).unwrap();
        assert!(identity.start_time > 0.0);
        assert!(identity.executable_path.is_some());
        assert!(!identity.arguments.is_empty());
        assert!(identity.arguments[0].to_lowercase().contains("cmux_cli"));
    }

    #[test]
    fn errors_and_empty_output_match_canonical_words() {
        let env = BTreeMap::from([("HOME".to_string(), "C:/empty-home".to_string())]);
        assert_eq!(
            run_sessions_command(&strings(&["wat"]), false, &env)
                .unwrap_err()
                .message,
            "Unknown sessions subcommand: wat. Usage: cmux sessions list [options]"
        );
        assert_eq!(
            run_sessions_command(&strings(&["--agent="]), false, &env)
                .unwrap_err()
                .message,
            "sessions list: --agent requires a value"
        );
        assert_eq!(
            run_sessions_command(&strings(&["--limit", "0"]), false, &env)
                .unwrap_err()
                .message,
            "sessions list: --limit must be a positive integer"
        );
        assert!(run_sessions_command(&[], false, &env)
            .unwrap()
            .starts_with("No saved agent sessions matched.\nstate_dir="));
        assert_eq!(
            run_sessions_command(&strings(&["help"]), false, &env).unwrap(),
            SESSIONS_USAGE
        );
    }
}
