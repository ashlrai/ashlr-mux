//! tmux resume-binding derivation.
//!
//! Ported from `TmuxResumeParser.swift` (enum `TmuxResumeParser`), plus the
//! minimal [`SurfaceResumeBindingSnapshot`] output type it constructs (ported
//! from `SessionPersistence.swift`).
//!
//! Given an observed process (name/path/argv/env) this decides whether it is a
//! resumable tmux client and reconstructs a safe
//! `tmux [-L/-S sock] attach -t <session>` argv.
//!
//! # Safety gate
//!
//! The reconstruction is SECURITY-RELEVANT: it must only ever emit an `attach`
//! (or a `new -A` that names an existing-or-created session), never an arbitrary
//! tmux invocation. [`parse_top_level_arguments`] enforces that truth table:
//! only `attach`/`attach-session`/`a` with a `-t` target, or `new`/`new-session`
//! with BOTH `-A` and a `-s` name, are accepted; everything else is rejected.

use std::collections::HashMap;

/// The subset of `SurfaceResumeBindingSnapshot` (macOS `SessionPersistence.swift`)
/// that the tmux resume path produces.
///
/// The full snapshot lives in the session-persistence layer; this crate defines
/// the value it emits so it stays self-contained. The initializer
/// ([`SurfaceResumeBindingSnapshot::new`]) reproduces the Swift init's field
/// normalization for the fields the tmux path sets.
///
// DIVERGENCE: The Swift struct also carries `approvalPolicy`/`approvalRecordId`
// and an `agent-hook` command-canonicalization branch. The tmux resume path
// never sets those (it always uses `source == "process-detected"` and no
// approval policy), so they are omitted here; the authoritative full type lives
// in the session-persistence crate.
// NOTE: no `Eq` — `updated_at` is `f64` (Swift `TimeInterval`), which is only
// `PartialEq`. Mirrors Swift's `Equatable` conformance over `Double`.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SurfaceResumeBindingSnapshot {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub command: String,
    pub cwd: Option<String>,
    #[serde(rename = "checkpointId")]
    pub checkpoint_id: Option<String>,
    pub source: Option<String>,
    pub environment: Option<HashMap<String, String>>,
    #[serde(rename = "autoResume")]
    pub auto_resume: Option<bool>,
    #[serde(rename = "updatedAt")]
    pub updated_at: f64,
}

impl SurfaceResumeBindingSnapshot {
    /// Constructs a snapshot, reproducing the Swift initializer's normalization
    /// for the process-detected tmux path.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: Option<&str>,
        kind: Option<&str>,
        command: &str,
        cwd: Option<&str>,
        checkpoint_id: Option<&str>,
        source: Option<&str>,
        environment: Option<HashMap<String, String>>,
        auto_resume: Option<bool>,
        updated_at: f64,
    ) -> Self {
        let normalized_cwd = normalized(cwd);
        let normalized_source = normalized(source);
        Self {
            name: normalized(name),
            kind: normalized(kind),
            // Swift `sanitizedStartupCommand`: for a non-`agent-hook` source it is
            // just the trimmed command. The tmux path is always
            // `source == "process-detected"`, so this is the whole behavior here.
            command: command.trim().to_string(),
            cwd: normalized_cwd,
            checkpoint_id: normalized(checkpoint_id),
            source: normalized_source,
            environment: normalized_environment(environment),
            auto_resume,
            updated_at,
        }
    }

    /// `source == "process-detected"`.
    pub fn is_process_detected(&self) -> bool {
        self.source.as_deref() == Some("process-detected")
    }

    /// `autoResume == true`.
    pub fn allows_automatic_resume(&self) -> bool {
        self.auto_resume == Some(true)
    }
}

/// Derives a resume binding for an observed tmux client process, or `None` if it
/// is not a safely-resumable tmux client.
///
/// Mirrors Swift `TmuxResumeParser.binding(...)`. Only env keys `PWD`,
/// `CMUX_AGENT_LAUNCH_CWD`, and `TMUX_TMPDIR` are read.
pub fn binding(
    process_name: &str,
    process_path: Option<&str>,
    arguments: &[String],
    environment: &HashMap<String, String>,
    captured_at: f64,
) -> Option<SurfaceResumeBindingSnapshot> {
    let observed = ObservedTmuxProcess {
        process_name: process_name.to_string(),
        process_path: process_path.map(|s| s.to_string()),
        arguments: arguments.to_vec(),
    };
    let invocation = resume_invocation(&observed)?;

    let command = invocation
        .argv
        .iter()
        .map(|s| shell_single_quoted(s))
        .collect::<Vec<_>>()
        .join(" ");
    let cwd = normalized(
        environment
            .get("CMUX_AGENT_LAUNCH_CWD")
            .or_else(|| environment.get("PWD"))
            .map(|s| s.as_str()),
    );
    let resume_environment = tmux_resume_environment(environment);
    let name = match &invocation.session_name {
        Some(session) => format!("tmux {session}"),
        None => "tmux".to_string(),
    };
    Some(SurfaceResumeBindingSnapshot::new(
        Some(&name),
        Some("tmux"),
        &command,
        cwd.as_deref(),
        invocation.session_name.as_deref(),
        Some("process-detected"),
        resume_environment,
        Some(true),
        captured_at,
    ))
}

/// Whether an argument looks like a tmux executable name/path or a tmux client
/// process title. Mirrors Swift `argumentLooksLikeTmux`.
pub fn argument_looks_like_tmux(argument: &str) -> bool {
    let normalized = argument.to_lowercase();
    if argument_looks_like_tmux_client_process_title(&normalized) {
        return true;
    }
    let basename = last_path_component(&normalized);
    basename == "tmux" || argument_looks_like_tmux_client_process_title(basename)
}

/// Whether an argument looks like ANY tmux process title (`tmux:` prefix).
/// Mirrors Swift `argumentLooksLikeTmuxProcessTitle`.
pub fn argument_looks_like_tmux_process_title(argument: &str) -> bool {
    let normalized = argument.to_lowercase();
    if normalized.starts_with("tmux:") {
        return true;
    }
    let basename = last_path_component(&normalized);
    basename.starts_with("tmux:")
}

/// Whether an argument looks like a tmux SERVER process title (`tmux: server`).
/// Mirrors Swift `argumentLooksLikeTmuxServerProcessTitle`.
pub fn argument_looks_like_tmux_server_process_title(argument: &str) -> bool {
    let normalized = argument.to_lowercase();
    if normalized.starts_with("tmux: server") {
        return true;
    }
    let basename = last_path_component(&normalized);
    basename.starts_with("tmux: server")
}

fn argument_looks_like_tmux_client_process_title(argument: &str) -> bool {
    let normalized = argument.to_lowercase();
    if normalized.starts_with("tmux: client") {
        return true;
    }
    let basename = last_path_component(&normalized);
    basename.starts_with("tmux: client")
}

struct ObservedTmuxProcess {
    process_name: String,
    process_path: Option<String>,
    arguments: Vec<String>,
}

impl ObservedTmuxProcess {
    fn executable_basenames(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        if !self.process_name.is_empty() {
            names.push(self.process_name.clone());
        }
        if let Some(path) = &self.process_path {
            if !path.is_empty() {
                names.push(last_path_component(path).to_string());
            }
        }
        if let Some(first) = self.arguments.first() {
            if !first.is_empty() {
                names.push(last_path_component(first).to_string());
            }
        }
        // Preserve first-seen order, dropping duplicates (Swift's
        // `seen.insert().inserted` filter).
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        names.into_iter().filter(|n| seen.insert(n.clone())).collect()
    }

    fn is_tmux_process(&self) -> bool {
        self.executable_basenames()
            .iter()
            .any(|n| argument_looks_like_tmux(n))
    }

    fn has_tmux_server_process_title(&self) -> bool {
        self.executable_basenames()
            .iter()
            .any(|n| argument_looks_like_tmux_server_process_title(n))
    }
}

struct TmuxResumeInvocation {
    argv: Vec<String>,
    session_name: Option<String>,
}

fn resume_invocation(observed: &ObservedTmuxProcess) -> Option<TmuxResumeInvocation> {
    if !observed.is_tmux_process() {
        return None;
    }
    if observed.has_tmux_server_process_title() {
        return None;
    }

    let executable = tmux_executable(observed);
    let tail = tmux_tail_arguments(observed);
    let parsed = parse_top_level_arguments(&tail);
    if !parsed.is_safe {
        return None;
    }

    let mut argv = vec![executable];
    argv.extend(parsed.socket_flags.iter().cloned());
    argv.push("attach".to_string());
    if let Some(session_name) = &parsed.session_name {
        argv.push("-t".to_string());
        argv.push(session_name.clone());
    }
    Some(TmuxResumeInvocation {
        argv,
        session_name: parsed.session_name,
    })
}

fn tmux_executable(observed: &ObservedTmuxProcess) -> String {
    if let Some(first) = observed.arguments.first().and_then(|s| normalized(Some(s.as_str()))) {
        if argument_looks_like_tmux(&first) && !argument_looks_like_tmux_process_title(&first) {
            return first;
        }
    }
    if let Some(path) = observed.process_path.as_deref().and_then(|s| normalized(Some(s))) {
        if argument_looks_like_tmux(&path) && !argument_looks_like_tmux_process_title(&path) {
            return path;
        }
    }
    "tmux".to_string()
}

fn tmux_tail_arguments(observed: &ObservedTmuxProcess) -> Vec<String> {
    match observed.arguments.first() {
        None => Vec::new(),
        Some(first) => {
            if argument_looks_like_tmux(first) {
                observed.arguments[1..].to_vec()
            } else {
                observed.arguments.clone()
            }
        }
    }
}

struct ParsedTmuxTopLevelArguments {
    socket_flags: Vec<String>,
    session_name: Option<String>,
    is_safe: bool,
}

fn parse_top_level_arguments(arguments: &[String]) -> ParsedTmuxTopLevelArguments {
    let mut index = 0usize;
    let mut socket_flags: Vec<String> = Vec::new();

    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--" {
            index += 1;
            break;
        }
        if !argument.starts_with('-') {
            break;
        }
        if append_socket_flag(argument, arguments, &mut index, &mut socket_flags) {
            continue;
        }
        index += top_level_option_width(argument, arguments, index);
    }

    if index >= arguments.len() {
        return ParsedTmuxTopLevelArguments {
            socket_flags,
            session_name: None,
            is_safe: false,
        };
    }

    let command = arguments[index].as_str();
    let command_args: Vec<String> = arguments[(index + 1)..].to_vec();
    match command {
        "attach-session" | "attach" | "a" => {
            let session_name = option_value(&command_args, 't', "target-session");
            if session_name.is_none() {
                return ParsedTmuxTopLevelArguments {
                    socket_flags,
                    session_name: None,
                    is_safe: false,
                };
            }
            ParsedTmuxTopLevelArguments {
                socket_flags,
                session_name,
                is_safe: true,
            }
        }
        "new-session" | "new" => {
            if !has_flag(&command_args, 'A', None) {
                return ParsedTmuxTopLevelArguments {
                    socket_flags,
                    session_name: None,
                    is_safe: false,
                };
            }
            let session_name = option_value(&command_args, 's', "session-name");
            if session_name.is_none() {
                return ParsedTmuxTopLevelArguments {
                    socket_flags,
                    session_name: None,
                    is_safe: false,
                };
            }
            ParsedTmuxTopLevelArguments {
                socket_flags,
                session_name,
                is_safe: true,
            }
        }
        _ => ParsedTmuxTopLevelArguments {
            socket_flags,
            session_name: None,
            is_safe: false,
        },
    }
}

fn append_socket_flag(
    argument: &str,
    arguments: &[String],
    index: &mut usize,
    socket_flags: &mut Vec<String>,
) -> bool {
    for option in ["L", "S"] {
        let short = format!("-{option}");
        if argument == short {
            let value_index = *index + 1;
            match arguments.get(value_index).and_then(|s| normalized(Some(s.as_str()))) {
                Some(value) => {
                    socket_flags.push(short.clone());
                    socket_flags.push(value);
                    *index += 2;
                }
                None => {
                    *index = value_index;
                }
            }
            return true;
        }
        if argument.starts_with(&short) && argument.len() > short.len() {
            let value = &argument[short.len()..];
            if let Some(normalized_value) = normalized(Some(value)) {
                socket_flags.push(short.clone());
                socket_flags.push(normalized_value);
            }
            *index += 1;
            return true;
        }
    }
    false
}

fn top_level_option_width(argument: &str, arguments: &[String], index: usize) -> usize {
    if argument.contains('=') {
        return 1;
    }
    let value_options = ["-c", "-f"];
    if value_options.contains(&argument) && index + 1 < arguments.len() {
        return 2;
    }
    1
}

fn has_flag(arguments: &[String], short: char, long: Option<&str>) -> bool {
    for argument in arguments {
        if argument == "--" {
            break;
        }
        if let Some(long) = long {
            if argument == &format!("--{long}") {
                return true;
            }
        }
        if argument == &format!("-{short}") {
            return true;
        }
        if short_flag_cluster(argument, short) {
            return true;
        }
    }
    false
}

fn short_flag_cluster(argument: &str, short: char) -> bool {
    if !argument.starts_with('-') || argument.starts_with("--") {
        return false;
    }
    for option in argument.chars().skip(1) {
        if option == short {
            return true;
        }
        if VALUE_OPTION_CHARACTERS.contains(&option) {
            return false;
        }
    }
    false
}

/// tmux top-level short options that take a value. Mirrors Swift
/// `valueOptionCharacters`.
const VALUE_OPTION_CHARACTERS: [char; 9] = ['c', 'e', 'F', 'f', 'n', 's', 't', 'x', 'y'];

enum ClusterValueMatch {
    Inline(String),
    NextArgument,
}

fn cluster_value(argument: &str, short: char) -> Option<ClusterValueMatch> {
    if !argument.starts_with('-') || argument.starts_with("--") {
        return None;
    }
    let chars: Vec<char> = argument.chars().collect();
    let mut i = 1usize; // skip leading '-'
    while i < chars.len() {
        let option = chars[i];
        if option == short {
            if i + 1 < chars.len() {
                let value: String = chars[(i + 1)..].iter().collect();
                return Some(ClusterValueMatch::Inline(value));
            }
            return Some(ClusterValueMatch::NextArgument);
        }
        if VALUE_OPTION_CHARACTERS.contains(&option) {
            return None;
        }
        i += 1;
    }
    None
}

fn option_value(arguments: &[String], short: char, long: &str) -> Option<String> {
    let mut index = 0usize;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--" {
            break;
        }
        if argument == &format!("--{long}") || argument == &format!("-{short}") {
            return value_after(arguments, index);
        }
        let long_prefix = format!("--{long}=");
        if let Some(rest) = argument.strip_prefix(&long_prefix) {
            return normalized(Some(rest));
        }
        let short_prefix = format!("-{short}");
        if argument.starts_with(&short_prefix) && argument.len() > short_prefix.len() {
            return normalized(Some(&argument[short_prefix.len()..]));
        }
        if let Some(cluster) = cluster_value(argument, short) {
            match cluster {
                ClusterValueMatch::Inline(value) => return normalized(Some(&value)),
                ClusterValueMatch::NextArgument => return value_after(arguments, index),
            }
        }
        index += 1;
    }
    None
}

fn value_after(arguments: &[String], index: usize) -> Option<String> {
    let next_index = index + 1;
    let next = arguments.get(next_index)?;
    if next == "--" {
        return None;
    }
    normalized(Some(next))
}

/// Reimplemented locally (per lane directive — do NOT reach into cmux-cli).
/// Mirrors Swift `shellSingleQuoted`.
fn shell_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn tmux_resume_environment(environment: &HashMap<String, String>) -> Option<HashMap<String, String>> {
    let tmux_tmpdir = normalized(environment.get("TMUX_TMPDIR").map(|s| s.as_str()))?;
    let mut map = HashMap::new();
    map.insert("TMUX_TMPDIR".to_string(), tmux_tmpdir);
    Some(map)
}

/// Trims `CharacterSet.whitespacesAndNewlines`, returning `None` if the result is
/// empty. Mirrors Swift `normalized`.
fn normalized(raw_value: Option<&str>) -> Option<String> {
    let raw = raw_value?;
    let trimmed = raw.trim_matches(|c: char| c.is_whitespace());
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// MARK: - Snapshot init normalization helpers (from SessionPersistence.swift)

fn normalized_environment(
    environment: Option<HashMap<String, String>>,
) -> Option<HashMap<String, String>> {
    let environment = environment?;
    let mut result: HashMap<String, String> = HashMap::new();
    for (key, value) in environment {
        let trimmed_key = key.trim_matches(|c: char| c.is_whitespace());
        if trimmed_key.is_empty() || is_sensitive_environment_key(trimmed_key) {
            continue;
        }
        if !is_safe_environment_value(&value) {
            continue;
        }
        result.insert(trimmed_key.to_string(), value);
    }
    if result.is_empty() {
        None
    } else {
        Some(result)
    }
}

fn is_safe_environment_value(value: &str) -> bool {
    !value.chars().any(|c| (c as u32) < 0x20 || (c as u32) == 0x7F)
}

fn is_sensitive_environment_key(key: &str) -> bool {
    let uppercased = key.to_uppercase();
    const SENSITIVE_FRAGMENTS: [&str; 11] = [
        "API_KEY",
        "ACCESS_KEY",
        "AUTH_TOKEN",
        "BEARER_TOKEN",
        "PRIVATE_KEY",
        "PASSWORD",
        "PASSWD",
        "SECRET",
        "TOKEN",
        "CREDENTIAL",
        "COOKIE",
    ];
    SENSITIVE_FRAGMENTS
        .iter()
        .any(|frag| uppercased.contains(frag))
}

/// Foundation `(str as NSString).lastPathComponent`-like basename for the
/// resume heuristics.
///
// DIVERGENCE: Swift uses `NSString.pathComponents.last` / `.lastPathComponent`,
// which are POSIX-oriented (`/` separators). These heuristics inspect a
// remote-tmux client's argv/paths, which are POSIX on the SSH host regardless of
// the machine cmux runs on, so we split on `/` exactly like Foundation rather
// than on the Windows `\` separator.
fn last_path_component(s: &str) -> &str {
    let trimmed = s.strip_suffix('/').unwrap_or(s);
    match trimmed.rfind('/') {
        Some(idx) => &trimmed[idx + 1..],
        None => trimmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // ----- Safety gate: ACCEPT cases -----

    #[test]
    fn accepts_attach_with_target() {
        let b = binding(
            "tmux",
            Some("/usr/bin/tmux"),
            &args(&["tmux", "attach", "-t", "main"]),
            &env(&[]),
            100.0,
        )
        .unwrap();
        assert_eq!(b.command, "'tmux' 'attach' '-t' 'main'");
        assert_eq!(b.name.as_deref(), Some("tmux main"));
        assert_eq!(b.checkpoint_id.as_deref(), Some("main"));
        assert_eq!(b.kind.as_deref(), Some("tmux"));
        assert_eq!(b.source.as_deref(), Some("process-detected"));
        assert_eq!(b.auto_resume, Some(true));
    }

    #[test]
    fn accepts_attach_session_alias() {
        let b = binding(
            "tmux",
            None,
            &args(&["tmux", "attach-session", "-t", "dev"]),
            &env(&[]),
            1.0,
        );
        assert!(b.is_some());
        assert_eq!(b.unwrap().checkpoint_id.as_deref(), Some("dev"));
    }

    #[test]
    fn accepts_a_alias() {
        let b = binding("tmux", None, &args(&["tmux", "a", "-t", "x"]), &env(&[]), 1.0);
        assert!(b.is_some());
    }

    #[test]
    fn accepts_new_session_with_capital_a_and_name() {
        // `new -A -s foo` is a create-or-attach, which is safe.
        let b = binding(
            "tmux",
            None,
            &args(&["tmux", "new", "-A", "-s", "foo"]),
            &env(&[]),
            1.0,
        )
        .unwrap();
        assert_eq!(b.checkpoint_id.as_deref(), Some("foo"));
        // Reconstructed as an attach, never the original `new`.
        assert_eq!(b.command, "'tmux' 'attach' '-t' 'foo'");
    }

    #[test]
    fn accepts_socket_flags_before_command() {
        let b = binding(
            "tmux",
            None,
            &args(&["tmux", "-L", "mysock", "attach", "-t", "s"]),
            &env(&[]),
            1.0,
        )
        .unwrap();
        assert_eq!(b.command, "'tmux' '-L' 'mysock' 'attach' '-t' 's'");
    }

    #[test]
    fn accepts_inline_socket_flag() {
        let b = binding(
            "tmux",
            None,
            &args(&["tmux", "-Smysock", "attach", "-t", "s"]),
            &env(&[]),
            1.0,
        )
        .unwrap();
        assert_eq!(b.command, "'tmux' '-S' 'mysock' 'attach' '-t' 's'");
    }

    #[test]
    fn accepts_clustered_target_flag() {
        // `-t` clustered with a value: `attach -tmain`.
        let b = binding(
            "tmux",
            None,
            &args(&["tmux", "attach", "-tmain"]),
            &env(&[]),
            1.0,
        )
        .unwrap();
        assert_eq!(b.checkpoint_id.as_deref(), Some("main"));
    }

    // ----- Safety gate: REJECT cases -----

    #[test]
    fn rejects_non_tmux_process() {
        assert!(binding("bash", Some("/bin/bash"), &args(&["bash"]), &env(&[]), 1.0).is_none());
    }

    #[test]
    fn rejects_tmux_server_process_title() {
        // A server, not a client, must never be resumed.
        assert!(binding(
            "tmux: server (/tmp/tmux-1000/default)",
            None,
            &args(&["tmux: server"]),
            &env(&[]),
            1.0,
        )
        .is_none());
    }

    #[test]
    fn rejects_attach_without_target() {
        // `attach` with no `-t` is ambiguous → rejected.
        assert!(binding("tmux", None, &args(&["tmux", "attach"]), &env(&[]), 1.0).is_none());
    }

    #[test]
    fn rejects_new_session_without_capital_a() {
        // Plain `new -s foo` would CREATE a session, not resume → rejected.
        assert!(binding(
            "tmux",
            None,
            &args(&["tmux", "new", "-s", "foo"]),
            &env(&[]),
            1.0,
        )
        .is_none());
    }

    #[test]
    fn rejects_new_session_with_capital_a_but_no_name() {
        assert!(binding(
            "tmux",
            None,
            &args(&["tmux", "new", "-A"]),
            &env(&[]),
            1.0,
        )
        .is_none());
    }

    #[test]
    fn rejects_arbitrary_command() {
        // `kill-server` and friends must never be reconstructed.
        assert!(binding(
            "tmux",
            None,
            &args(&["tmux", "kill-server"]),
            &env(&[]),
            1.0,
        )
        .is_none());
        assert!(binding(
            "tmux",
            None,
            &args(&["tmux", "list-sessions"]),
            &env(&[]),
            1.0,
        )
        .is_none());
    }

    #[test]
    fn rejects_bare_tmux_no_command() {
        assert!(binding("tmux", None, &args(&["tmux"]), &env(&[]), 1.0).is_none());
    }

    #[test]
    fn rejects_target_after_double_dash_terminator() {
        // A `-t` appearing after `--` is a positional, not the target option.
        assert!(binding(
            "tmux",
            None,
            &args(&["tmux", "attach", "--", "-t", "main"]),
            &env(&[]),
            1.0,
        )
        .is_none());
    }

    // ----- Environment + cwd derivation -----

    #[test]
    fn cwd_prefers_agent_launch_cwd_over_pwd() {
        let b = binding(
            "tmux",
            None,
            &args(&["tmux", "attach", "-t", "s"]),
            &env(&[("PWD", "/home/pwd"), ("CMUX_AGENT_LAUNCH_CWD", "/home/agent")]),
            1.0,
        )
        .unwrap();
        assert_eq!(b.cwd.as_deref(), Some("/home/agent"));
    }

    #[test]
    fn cwd_falls_back_to_pwd() {
        let b = binding(
            "tmux",
            None,
            &args(&["tmux", "attach", "-t", "s"]),
            &env(&[("PWD", "/home/pwd")]),
            1.0,
        )
        .unwrap();
        assert_eq!(b.cwd.as_deref(), Some("/home/pwd"));
    }

    #[test]
    fn resume_environment_carries_only_tmux_tmpdir() {
        let b = binding(
            "tmux",
            None,
            &args(&["tmux", "attach", "-t", "s"]),
            &env(&[("TMUX_TMPDIR", "/tmp/tmux-1000"), ("PATH", "/usr/bin")]),
            1.0,
        )
        .unwrap();
        let e = b.environment.unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e.get("TMUX_TMPDIR").map(|s| s.as_str()), Some("/tmp/tmux-1000"));
    }

    #[test]
    fn resume_environment_absent_without_tmpdir() {
        let b = binding(
            "tmux",
            None,
            &args(&["tmux", "attach", "-t", "s"]),
            &env(&[("PATH", "/usr/bin")]),
            1.0,
        )
        .unwrap();
        assert!(b.environment.is_none());
    }

    // ----- Snapshot init normalization -----

    #[test]
    fn snapshot_filters_sensitive_env_keys() {
        let mut e = HashMap::new();
        e.insert("SAFE".to_string(), "ok".to_string());
        e.insert("MY_API_KEY".to_string(), "secret".to_string());
        e.insert("SESSION_TOKEN".to_string(), "abc".to_string());
        let snap = SurfaceResumeBindingSnapshot::new(
            None,
            None,
            "cmd",
            None,
            None,
            Some("process-detected"),
            Some(e),
            None,
            0.0,
        );
        let env = snap.environment.unwrap();
        assert!(env.contains_key("SAFE"));
        assert!(!env.contains_key("MY_API_KEY"));
        assert!(!env.contains_key("SESSION_TOKEN"));
    }

    #[test]
    fn snapshot_rejects_control_char_env_values() {
        let mut e = HashMap::new();
        e.insert("BAD".to_string(), "line1\nline2".to_string());
        e.insert("GOOD".to_string(), "clean".to_string());
        let snap = SurfaceResumeBindingSnapshot::new(
            None, None, "cmd", None, None, None, Some(e), None, 0.0,
        );
        let env = snap.environment.unwrap();
        assert!(!env.contains_key("BAD"));
        assert!(env.contains_key("GOOD"));
    }

    #[test]
    fn snapshot_trims_and_nils_empty_fields() {
        let snap = SurfaceResumeBindingSnapshot::new(
            Some("  "),
            Some(" tmux "),
            "  attach  ",
            Some(""),
            None,
            None,
            None,
            None,
            0.0,
        );
        assert_eq!(snap.name, None); // whitespace-only → None
        assert_eq!(snap.kind.as_deref(), Some("tmux"));
        assert_eq!(snap.command, "attach"); // trimmed
        assert_eq!(snap.cwd, None);
    }

    #[test]
    fn shell_single_quoted_escapes_apostrophes() {
        assert_eq!(shell_single_quoted("it's"), "'it'\\''s'");
        assert_eq!(shell_single_quoted("plain"), "'plain'");
    }

    #[test]
    fn snapshot_json_uses_camel_case_keys() {
        let snap = SurfaceResumeBindingSnapshot::new(
            Some("tmux main"),
            Some("tmux"),
            "'tmux' 'attach'",
            None,
            Some("main"),
            Some("process-detected"),
            None,
            Some(true),
            42.5,
        );
        let json = serde_json::to_value(&snap).unwrap();
        assert!(json.get("checkpointId").is_some());
        assert!(json.get("autoResume").is_some());
        assert!(json.get("updatedAt").is_some());
        let back: SurfaceResumeBindingSnapshot = serde_json::from_value(json).unwrap();
        assert_eq!(back, snap);
    }

    #[test]
    fn executable_taken_from_argv_when_it_is_a_real_path() {
        // argv[0] is a real tmux path (not a process title) → used as executable.
        let b = binding(
            "tmux",
            None,
            &args(&["/opt/homebrew/bin/tmux", "attach", "-t", "s"]),
            &env(&[]),
            1.0,
        )
        .unwrap();
        assert!(b.command.starts_with("'/opt/homebrew/bin/tmux' 'attach'"));
    }

    #[test]
    fn process_title_argv_does_not_leak_into_executable() {
        // argv[0] is a client process title, not a usable path → fall back to
        // literal "tmux"; the title is still recognized as a tmux client.
        let b = binding(
            "tmux: client (/tmp/tmux-1000/default)",
            None,
            &args(&["tmux: client", "attach", "-t", "s"]),
            &env(&[]),
            1.0,
        )
        .unwrap();
        assert!(b.command.starts_with("'tmux' 'attach'"));
    }
}
