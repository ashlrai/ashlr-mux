//! `cmux-ssh` — pure ssh / EternalTerminal command-line detection parser.
//!
//! Port of the canonical macOS Swift sources
//! `Sources/RemoteShellSessionParsing.swift` and
//! `Sources/TerminalSSHSessionDetector.swift`. Only the pure argv/string
//! parsing + selection core is ported here — the live-I/O surface (spawning
//! `/bin/ps`, reading `KERN_PROCARGS2`, tty enumeration) stays in the host
//! layer and is intentionally omitted.
//!
//! The public entry point is [`detect_for_testing`] (Swift
//! `TerminalSSHSessionDetector.detectForTesting(ttyName:processes:argumentsByPID:)`),
//! which selects the foreground remote-shell process for a tty and turns its
//! argv into a normalized [`DetectedSSHSession`].
//!
//! The [`reconnect_input_filter`] module ports the pure byte-stream core of
//! `CLI/SSHPTYAttachReconnectInputFilter.swift` (probe-reply suppression
//! during `ssh-pty-attach` reconnect).
//!
//! The [`ssh_url`] module ports the pure core of `Sources/CmuxSSHURLRequest.swift`
//! (the `…://ssh?…` deep-link and standard `ssh://` URL parser —
//! [`CmuxSSHURLRequest`] / [`CmuxSSHURLParseError`]).
//!
//! The [`ssh_batch`] module ports the pure batch (non-interactive) SSH argv
//! composition + option-normalization core of
//! `Remote/WorkspaceRemoteConfiguration+SSHBatchCommands.swift` and
//! `Remote/WorkspaceRemoteConfiguration+SSHOptionNormalization.swift`
//! ([`SshBatchConfiguration`] daemon-transport / socket-forward /
//! reverse-relay argv, plus [`durable_ssh_options`] / [`trimmed_ssh_options`] /
//! [`normalized_optional_value`]).

pub mod reconnect_input_filter;
pub mod ssh_batch;
pub mod ssh_url;

pub use ssh_batch::{
    durable_ssh_options, forked_workspace_ssh_options, normalized_optional_value,
    trimmed_ssh_options, SshBatchConfiguration,
};

pub use ssh_url::{CmuxSSHURLParseError, CmuxSSHURLRequest};

pub use reconnect_input_filter::{
    ReconnectInputFilter, SequenceMatch, MAX_PENDING_PROBE_BYTES,
    PENDING_PROBE_CONTINUATION_TIMEOUT_MS,
};

use std::collections::HashMap;

// ---------------------------------------------------------------------------
// RemoteShellTransport (Swift: enum RemoteShellTransport)
// ---------------------------------------------------------------------------

/// The remote-shell command a foreground process is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteShellTransport {
    /// `ssh`.
    Ssh,
    /// EternalTerminal `et`.
    EternalTerminal,
}

impl RemoteShellTransport {
    /// Swift: `init?(executableName:)`.
    pub fn from_executable_name(executable_name: &str) -> Option<Self> {
        match normalized_executable_name(executable_name).as_str() {
            "ssh" => Some(Self::Ssh),
            "et" => Some(Self::EternalTerminal),
            _ => None,
        }
    }

    /// Swift: `var executableName`.
    pub fn executable_name(self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
            Self::EternalTerminal => "et",
        }
    }
}

// ---------------------------------------------------------------------------
// DetectedSSHSession (Swift: struct DetectedSSHSession: Equatable)
// ---------------------------------------------------------------------------

/// The normalized shape of a detected ssh / EternalTerminal session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedSSHSession {
    /// `[user@]host` destination (IPv6 literals bracketed as needed).
    pub destination: String,
    /// `-p` / `Port` value.
    pub port: Option<i64>,
    /// `-i` / `IdentityFile`.
    pub identity_file: Option<String>,
    /// `-F` config file (ssh only).
    pub config_file: Option<String>,
    /// `-J` / `ProxyJump`.
    pub jump_host: Option<String>,
    /// `-S` / `ControlPath`.
    pub control_path: Option<String>,
    /// `-4`.
    pub use_ipv4: bool,
    /// `-6`.
    pub use_ipv6: bool,
    /// `-A` / forward agent.
    pub forward_agent: bool,
    /// `-C` compression.
    pub compression_enabled: bool,
    /// Remaining, unfiltered `-o` options preserved verbatim.
    pub ssh_options: Vec<String>,
}

impl DetectedSSHSession {
    /// Swift: `private func scpArguments(localPath:remotePath:)`. Pure argv
    /// construction; exposed for parity with `scpArgumentsForTesting`.
    pub fn scp_arguments(&self, local_path: &str, remote_path: &str) -> Vec<String> {
        let mut args: Vec<String> = vec![
            "-q".into(),
            "-o".into(),
            "ConnectTimeout=6".into(),
            "-o".into(),
            "ServerAliveInterval=20".into(),
            "-o".into(),
            "ServerAliveCountMax=2".into(),
            "-o".into(),
            "BatchMode=yes".into(),
            "-o".into(),
            "ControlMaster=no".into(),
        ];

        if self.use_ipv4 {
            args.push("-4".into());
        } else if self.use_ipv6 {
            args.push("-6".into());
        }
        if self.forward_agent {
            args.push("-A".into());
        }
        if self.compression_enabled {
            args.push("-C".into());
        }
        if let Some(config_file) = &self.config_file {
            if !config_file.trim().is_empty() {
                args.push("-F".into());
                args.push(config_file.clone());
            }
        }
        if let Some(jump_host) = &self.jump_host {
            if !jump_host.trim().is_empty() {
                args.push("-J".into());
                args.push(jump_host.clone());
            }
        }
        if let Some(port) = self.port {
            args.push("-P".into());
            args.push(port.to_string());
        }
        if let Some(identity_file) = &self.identity_file {
            if !identity_file.trim().is_empty() {
                args.push("-i".into());
                args.push(identity_file.clone());
            }
        }
        if let Some(control_path) = &self.control_path {
            if !control_path.trim().is_empty()
                && !has_ssh_option_key(&self.ssh_options, "ControlPath")
            {
                args.push("-o".into());
                args.push(format!("ControlPath={control_path}"));
            }
        }
        if !has_ssh_option_key(&self.ssh_options, "StrictHostKeyChecking") {
            args.push("-o".into());
            args.push("StrictHostKeyChecking=accept-new".into());
        }
        for option in &self.ssh_options {
            args.push("-o".into());
            args.push(option.clone());
        }

        args.push(local_path.to_string());
        args.push(format!(
            "{}:{}",
            scp_remote_destination(&self.destination),
            remote_path
        ));
        args
    }
}

// ---------------------------------------------------------------------------
// ProcessSnapshot (Swift: TerminalSSHSessionDetector.ProcessSnapshot)
// ---------------------------------------------------------------------------

/// A single `ps` row for a tty (already parsed into fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSnapshot {
    /// Process id.
    pub pid: i32,
    /// Process group id.
    pub pgid: i32,
    /// Foreground process group id of the controlling terminal.
    pub tpgid: i32,
    /// Controlling tty name.
    pub tty: String,
    /// Lower-cased `ucomm` executable name.
    pub executable_name: String,
}

// ---------------------------------------------------------------------------
// Pure public entry point (Swift: detectForTesting)
// ---------------------------------------------------------------------------

/// Swift: `TerminalSSHSessionDetector.detectForTesting(ttyName:processes:argumentsByPID:)`.
///
/// Selects the foreground remote-shell process for `tty_name` (highest pid,
/// then highest pgid) and parses its argv into a [`DetectedSSHSession`].
pub fn detect_for_testing(
    tty_name: &str,
    processes: &[ProcessSnapshot],
    arguments_by_pid: &HashMap<i32, Vec<String>>,
) -> Option<DetectedSSHSession> {
    let normalized_tty = normalize_tty_name(tty_name);
    if normalized_tty.is_empty() {
        return None;
    }

    let mut candidates: Vec<&ProcessSnapshot> = processes
        .iter()
        .filter(|p| is_foreground_remote_shell_process(p, &normalized_tty))
        .collect();
    // Swift sort: descending pid, then descending pgid.
    candidates.sort_by(|a, b| b.pid.cmp(&a.pid).then_with(|| b.pgid.cmp(&a.pgid)));

    for candidate in candidates {
        let Some(transport) = RemoteShellTransport::from_executable_name(&candidate.executable_name)
        else {
            continue;
        };
        let Some(arguments) = arguments_by_pid.get(&candidate.pid) else {
            continue;
        };
        if let Some(session) = parse_command_line(arguments, transport) {
            return Some(session);
        }
    }

    None
}

/// Swift: `parseCommandLine(_:for:)`.
pub fn parse_command_line(
    arguments: &[String],
    transport: RemoteShellTransport,
) -> Option<DetectedSSHSession> {
    match transport {
        RemoteShellTransport::Ssh => parse_ssh_command_line(arguments),
        RemoteShellTransport::EternalTerminal => parse_eternal_terminal_command_line(arguments),
    }
}

// ---------------------------------------------------------------------------
// TTY / snapshot helpers (Swift: TerminalSSHSessionDetector)
// ---------------------------------------------------------------------------

/// Swift: `normalizeTTYName(_:)`. `/`-split basename of a POSIX tty name.
pub fn normalize_tty_name(tty_name: &str) -> String {
    let trimmed = tty_name.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    // Swift `split(separator:"/")` omits empty subsequences; `.last`.
    match trimmed.split('/').rfind(|s| !s.is_empty()) {
        Some(last) => last.to_string(),
        None => trimmed.to_string(),
    }
}

/// Swift: `isForegroundRemoteShellProcess(_:ttyName:)`.
fn is_foreground_remote_shell_process(process: &ProcessSnapshot, tty_name: &str) -> bool {
    normalize_tty_name(&process.tty) == normalize_tty_name(tty_name)
        && RemoteShellTransport::from_executable_name(&process.executable_name).is_some()
        && process.pgid > 0
        && process.tpgid > 0
        && process.pgid == process.tpgid
}

/// Swift: `parseProcessSnapshot(_:)`. Splits a `ps` row into at most five
/// whitespace-delimited fields (the fifth preserves internal spaces).
pub fn parse_process_snapshot(line: &str) -> Option<ProcessSnapshot> {
    // Swift: line.split(maxSplits: 4, whereSeparator: \.isWhitespace) with
    // omittingEmptySubsequences == true. Swift's split loop breaks out at the
    // single separator Character that terminates the fourth field, and the
    // fifth subsequence is everything AFTER that one separator, WITHOUT
    // left-trimming. A remainder that is entirely whitespace (e.g. two
    // trailing spaces after the fourth field) is therefore still a non-empty
    // fifth part, and the row parses with an empty (trimmed) executable name;
    // exactly one trailing separator leaves an empty remainder -> four parts
    // -> nil. Matched exactly below.
    //
    // DIVERGENCE: Swift's separator predicate sees Characters (grapheme
    // clusters), so a space followed by a combining mark is ONE whitespace
    // Character there, while this port splits after the space scalar and
    // leaves the combining mark in the next field. See the grapheme note at
    // `drop_first_chars`. ASCII `ps` output is unaffected.
    let mut parts: Vec<&str> = Vec::with_capacity(5);
    let mut rest = line;
    for _ in 0..4 {
        rest = rest.trim_start_matches(|c: char| c.is_whitespace());
        if rest.is_empty() {
            break;
        }
        match rest.find(|c: char| c.is_whitespace()) {
            Some(idx) => {
                parts.push(&rest[..idx]);
                rest = &rest[idx..];
            }
            None => {
                parts.push(rest);
                rest = "";
            }
        }
    }
    // Drop exactly one leading separator (Swift consumed it as the fourth
    // split); keep any further whitespace as part of the fifth field.
    let mut remainder_chars = rest.chars();
    if remainder_chars.next().is_some() {
        let remainder = remainder_chars.as_str();
        if !remainder.is_empty() {
            parts.push(remainder);
        }
    }

    if parts.len() != 5 {
        return None;
    }
    let pid = parts[0].parse::<i32>().ok()?;
    let pgid = parts[1].parse::<i32>().ok()?;
    let tpgid = parts[2].parse::<i32>().ok()?;

    Some(ProcessSnapshot {
        pid,
        pgid,
        tpgid,
        tty: parts[3].to_string(),
        executable_name: parts[4].trim().to_lowercase(),
    })
}

// ---------------------------------------------------------------------------
// scp destination helpers (Swift: DetectedSSHSession static helpers)
// ---------------------------------------------------------------------------

/// Swift: `hasSSHOptionKey(_:key:)`.
fn has_ssh_option_key(options: &[String], key: &str) -> bool {
    let lowered_key = key.to_lowercase();
    options
        .iter()
        .any(|o| option_key(o).as_deref() == Some(lowered_key.as_str()))
}

/// Swift: `optionKey(_:)`.
fn option_key(option: &str) -> Option<String> {
    let trimmed = option.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .split(|c: char| c == '=' || c.is_whitespace())
        .find(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
}

/// Swift: `scpRemoteDestination(_:)`.
fn scp_remote_destination(destination: &str) -> String {
    let trimmed_destination = destination.trim();
    if trimmed_destination.is_empty() {
        return destination.to_string();
    }

    // Swift split(separator:"@", maxSplits:1, omittingEmptySubsequences:false).
    let mut split = trimmed_destination.splitn(2, '@');
    let first = split.next().unwrap_or("");
    let second = split.next();
    let (user_part, host_part) = match second {
        Some(host) => (Some(first), host),
        None => (None, trimmed_destination),
    };

    if !should_bracket_ipv6_literal(host_part) {
        return trimmed_destination.to_string();
    }

    let bracketed_host = format!("[{host_part}]");
    match user_part {
        Some(user) => format!("{user}@{bracketed_host}"),
        None => bracketed_host,
    }
}

/// Swift: `shouldBracketIPv6Literal(_:)`.
fn should_bracket_ipv6_literal(host: &str) -> bool {
    let trimmed_host = host.trim();
    !trimmed_host.is_empty()
        && trimmed_host.contains(':')
        && !trimmed_host.starts_with('[')
        && !trimmed_host.ends_with(']')
}

// ---------------------------------------------------------------------------
// RemoteShellSessionParsing helpers
// ---------------------------------------------------------------------------

const ET_NO_ARGUMENT_FLAGS: &str = "efhNx";
const ET_VALUE_ARGUMENT_FLAGS: &str = "cklprtu";
const SSH_NO_ARGUMENT_FLAGS: &str = "46AaCfGgKkMNnqsTtVvXxYy";
const SSH_VALUE_ARGUMENT_FLAGS: &str = "BbcDEeFIiJLlmOopQRSWw";

const ET_LONG_VALUE_OPTIONS: &[&str] = &[
    "command",
    "host",
    "jport",
    "jserverfifo",
    "jumphost",
    "keepalive",
    "logdir",
    "port",
    "reversetunnel",
    "serverfifo",
    "ssh-socket",
    "terminal-path",
    "tunnel",
    "username",
];
const ET_LONG_NO_ARGUMENT_OPTIONS: &[&str] = &[
    "forward-ssh-agent",
    "help",
    "kill-other-sessions",
    "logtostdout",
    "macserver",
    "no-terminal",
    "noexit",
    "silent",
    "version",
];
const FILTERED_SSH_OPTION_KEYS: &[&str] = &[
    "batchmode",
    "controlmaster",
    "controlpersist",
    "forkafterauthentication",
    "localcommand",
    "permitlocalcommand",
    "remotecommand",
    "requesttty",
    "sendenv",
    "sessiontype",
    "setenv",
    "stdioforward",
];

/// Swift: `normalizedExecutableName(_:)`.
fn normalized_executable_name(executable_name: &str) -> String {
    let trimmed = executable_name.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    match trimmed.split('/').rfind(|s| !s.is_empty()) {
        Some(last) => last.to_lowercase(),
        None => trimmed.to_lowercase(),
    }
}

// DIVERGENCE: Swift operates on Characters (extended grapheme clusters):
// `String.count`, `dropFirst(n)`, and `Set<Character>.contains` all see one
// element per grapheme. This port operates on Unicode scalar values (`char`),
// so an argument containing a combining mark can take a different branch:
// e.g. `-i\u{301}` is 2 Characters in Swift (the flag Character "i\u{301}"
// matches no flag set, so parsing fails) but 3 chars here (clustered `-i`
// value branch, identity file "\u{301}"). Affected sites: `drop_first_chars`,
// the `char_count` / `second_char` / `flags` logic in `parse_ssh_command_line`,
// `short_options` in `parse_eternal_terminal_command_line`, and the whitespace
// split in `parse_process_snapshot`. The same class also covers every
// prefix/separator match: Swift `hasPrefix("-")` / `hasPrefix("--ssh-option=")`
// and `Character == "=" / "@" / ":"` comparisons are grapheme-aware (a
// combining mark right after the prefix defeats the match — `-\u{301}x` is a
// DESTINATION in Swift), while this port's `starts_with` / `strip_prefix` /
// scalar `split` match the raw scalar and keep the combining mark in the
// following slice. ASCII argv — the entirety of real ssh/et usage — is
// unaffected. Pinned by `ssh_combining_mark_flag_divergence`.
/// Skip the first `n` Unicode scalar values of `s` (Swift `String.dropFirst(n)`).
fn drop_first_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((idx, _)) => &s[idx..],
        None => "",
    }
}

/// Swift: `isBoolLiteral(_:)`.
fn is_bool_literal(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "0" | "1" | "false" | "no" | "true" | "yes"
    )
}

/// Swift: `sshOptionKey(_:)`.
fn ssh_option_key(option: &str) -> Option<String> {
    let trimmed = option.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed
        .split(|c: char| c == '=' || c.is_whitespace())
        .find(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
}

/// Swift: `sshOptionValue(_:)`.
fn ssh_option_value(option: &str) -> Option<String> {
    let trimmed = option.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Some(eq) = trimmed.find('=') {
        let value = trimmed[eq + 1..].trim();
        return if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        };
    }

    // Swift split(maxSplits:1, whereSeparator: \.isWhitespace) omitting empties;
    // the remainder is trimmed afterward, so split at the first whitespace char.
    match trimmed.split_once(|c: char| c.is_whitespace()) {
        Some((_, rest)) => {
            let value = rest.trim();
            if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        }
        None => None,
    }
}

/// Swift: `resolveDestination(_:loginName:)`.
fn resolve_destination(destination: &str, login_name: Option<&str>) -> String {
    let trimmed_destination = destination.trim();
    if trimmed_destination.is_empty() {
        return String::new();
    }
    match login_name.map(str::trim) {
        Some(login) if !login.is_empty() && !trimmed_destination.contains('@') => {
            format!("{login}@{trimmed_destination}")
        }
        _ => trimmed_destination.to_string(),
    }
}

/// Swift: `resolveEternalTerminalDestination(_:loginName:)`.
fn resolve_eternal_terminal_destination(destination: &str, login_name: Option<&str>) -> String {
    let trimmed_destination = destination.trim();
    if trimmed_destination.is_empty() {
        return String::new();
    }

    let mut split = trimmed_destination.splitn(2, '@');
    let first = split.next().unwrap_or("");
    if let Some(second) = split.next() {
        let user = first;
        let host = strip_eternal_terminal_server_port(second);
        if user.is_empty() || host.is_empty() {
            return String::new();
        }
        return format!("{user}@{host}");
    }

    let host = strip_eternal_terminal_server_port(trimmed_destination);
    resolve_destination(&host, login_name)
}

/// Swift: `stripEternalTerminalServerPort(fromHost:)`.
///
/// Removes a trailing `:port` from an EternalTerminal server address, bracketing
/// full/compressed IPv6 literals per the subtle colon-count heuristic.
fn strip_eternal_terminal_server_port(host: &str) -> String {
    let trimmed_host = host.trim();
    if trimmed_host.is_empty() {
        return String::new();
    }

    if trimmed_host.starts_with('[') {
        if let Some(close_idx) = trimmed_host.find(']') {
            // '[' and ']' are single-byte ASCII → these slices are on char
            // boundaries even when the host between them is multibyte.
            let bracketed_host = &trimmed_host[1..close_idx];
            let remainder = &trimmed_host[close_idx + 1..];
            if let Some(port) = remainder.strip_prefix(':') {
                if port.parse::<i64>().is_ok() {
                    return format!("[{bracketed_host}]");
                }
            }
            return trimmed_host.to_string();
        }
        // No closing bracket: fall through to the colon-count logic below,
        // matching Swift's combined `if hasPrefix && let closingBracket`.
    }

    let colon_count = trimmed_host.chars().filter(|&c| c == ':').count();
    let Some(last_colon) = trimmed_host.rfind(':') else {
        return trimmed_host.to_string();
    };

    let port = &trimmed_host[last_colon + 1..]; // ':' is single-byte ASCII.
    if port.parse::<i64>().is_err() {
        return trimmed_host.to_string();
    }
    let stripped_host = &trimmed_host[..last_colon];
    if colon_count == 1 {
        return stripped_host.to_string();
    }
    if colon_count == 8 && !trimmed_host.contains("::") {
        return format!("[{stripped_host}]");
    }
    // Compressed IPv6 with a trailing decimal hextet is ambiguous; preserve it.
    trimmed_host.to_string()
}

// DIVERGENCE: Swift `isIPv6Literal(_:)` used Darwin `inet_pton(AF_INET6, …)` to
// validate an IPv6 literal. Ported to Rust std `std::net::Ipv6Addr::from_str`
// (Foundation/Darwin C API -> Rust stdlib). Like the Swift original this helper
// is not referenced by the parsing paths; retained for parity and tested below.
/// True if `host` parses as an IPv6 literal.
pub fn is_ipv6_literal(host: &str) -> bool {
    use std::str::FromStr;
    std::net::Ipv6Addr::from_str(host).is_ok()
}

/// Swift: `consumeSSHOption(...)`. Applies a single `-o key=value` (or
/// `key value`) option to the accumulated session state.
#[allow(clippy::too_many_arguments)]
fn consume_ssh_option(
    option: &str,
    port: &mut Option<i64>,
    identity_file: &mut Option<String>,
    control_path: &mut Option<String>,
    jump_host: &mut Option<String>,
    login_name: &mut Option<String>,
    ssh_options: &mut Vec<String>,
) -> bool {
    let trimmed = option.trim();
    if trimmed.is_empty() {
        return false;
    }
    let key = ssh_option_key(trimmed);
    let value = ssh_option_value(trimmed);

    match key.as_deref() {
        Some("port") => {
            if let Some(v) = value.as_deref() {
                if let Ok(parsed) = v.parse::<i64>() {
                    *port = Some(parsed);
                    return true;
                }
            }
            false
        }
        Some("identityfile") => match value {
            Some(v) if !v.is_empty() => {
                *identity_file = Some(v);
                true
            }
            _ => false,
        },
        Some("controlpath") => match value {
            Some(v) if !v.is_empty() => {
                *control_path = Some(v);
                true
            }
            _ => false,
        },
        Some("proxyjump") => match value {
            Some(v) if !v.is_empty() => {
                *jump_host = Some(v);
                true
            }
            _ => false,
        },
        Some("user") => match value {
            Some(v) if !v.is_empty() => {
                *login_name = Some(v);
                true
            }
            _ => false,
        },
        Some(k) if FILTERED_SSH_OPTION_KEYS.contains(&k) => true,
        _ => {
            ssh_options.push(trimmed.to_string());
            true
        }
    }
}

// ---------------------------------------------------------------------------
// ssh command-line parser (Swift: parseSSHCommandLine)
// ---------------------------------------------------------------------------

/// Mutable accumulator for the SSH `consumeValue` closure.
struct SshValueSink<'a> {
    port: &'a mut Option<i64>,
    identity_file: &'a mut Option<String>,
    config_file: &'a mut Option<String>,
    jump_host: &'a mut Option<String>,
    control_path: &'a mut Option<String>,
    login_name: &'a mut Option<String>,
    ssh_options: &'a mut Vec<String>,
}

/// Swift: nested `consumeValue(_:for:)` inside `parseSSHCommandLine`.
fn ssh_consume_value(sink: &mut SshValueSink<'_>, value: &str, option: char) -> bool {
    let trimmed_value = value.trim();
    if trimmed_value.is_empty() {
        return false;
    }
    match option {
        'p' => match trimmed_value.parse::<i64>() {
            Ok(parsed) => {
                *sink.port = Some(parsed);
                true
            }
            Err(_) => false,
        },
        'i' => {
            *sink.identity_file = Some(trimmed_value.to_string());
            true
        }
        'F' => {
            *sink.config_file = Some(trimmed_value.to_string());
            true
        }
        'J' => {
            *sink.jump_host = Some(trimmed_value.to_string());
            true
        }
        'S' => {
            *sink.control_path = Some(trimmed_value.to_string());
            true
        }
        'l' => {
            *sink.login_name = Some(trimmed_value.to_string());
            true
        }
        'o' => consume_ssh_option(
            trimmed_value,
            sink.port,
            sink.identity_file,
            sink.control_path,
            sink.jump_host,
            sink.login_name,
            sink.ssh_options,
        ),
        _ => SSH_VALUE_ARGUMENT_FLAGS.contains(option),
    }
}

/// Swift: `parseSSHCommandLine(_:)`.
fn parse_ssh_command_line(arguments: &[String]) -> Option<DetectedSSHSession> {
    if arguments.is_empty() {
        return None;
    }

    let mut index = 0;
    if normalized_executable_name(&arguments[0]) == RemoteShellTransport::Ssh.executable_name() {
        index = 1;
    }

    let mut destination: Option<String> = None;
    let mut port: Option<i64> = None;
    let mut identity_file: Option<String> = None;
    let mut config_file: Option<String> = None;
    let mut jump_host: Option<String> = None;
    let mut control_path: Option<String> = None;
    let mut login_name: Option<String> = None;
    let mut use_ipv4 = false;
    let mut use_ipv6 = false;
    let mut forward_agent = false;
    let mut compression_enabled = false;
    let mut ssh_options: Vec<String> = Vec::new();

    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == "--" {
            index += 1;
            if index < arguments.len() {
                destination = Some(arguments[index].clone());
            }
            break;
        }
        if !argument.starts_with('-') || argument == "-" {
            destination = Some(argument.to_string());
            break;
        }

        // DIVERGENCE: scalar count / scalar flags, not Swift grapheme
        // Characters — see the note at `drop_first_chars`.
        let char_count = argument.chars().count();
        let second_char = argument.chars().nth(1);

        if char_count > 2 {
            if let Some(option) = second_char {
                if SSH_VALUE_ARGUMENT_FLAGS.contains(option) {
                    let mut sink = SshValueSink {
                        port: &mut port,
                        identity_file: &mut identity_file,
                        config_file: &mut config_file,
                        jump_host: &mut jump_host,
                        control_path: &mut control_path,
                        login_name: &mut login_name,
                        ssh_options: &mut ssh_options,
                    };
                    if !ssh_consume_value(&mut sink, drop_first_chars(argument, 2), option) {
                        return None;
                    }
                    index += 1;
                    continue;
                }
            }
        }

        if char_count == 2 {
            if let Some(option) = second_char {
                if SSH_VALUE_ARGUMENT_FLAGS.contains(option) {
                    let next_index = index + 1;
                    if next_index >= arguments.len() {
                        return None;
                    }
                    let mut sink = SshValueSink {
                        port: &mut port,
                        identity_file: &mut identity_file,
                        config_file: &mut config_file,
                        jump_host: &mut jump_host,
                        control_path: &mut control_path,
                        login_name: &mut login_name,
                        ssh_options: &mut ssh_options,
                    };
                    if !ssh_consume_value(&mut sink, &arguments[next_index], option) {
                        return None;
                    }
                    index += 2;
                    continue;
                }
            }
        }

        let flags: Vec<char> = argument.chars().skip(1).collect();
        if flags.is_empty() || !flags.iter().all(|&c| SSH_NO_ARGUMENT_FLAGS.contains(c)) {
            return None;
        }
        for flag in flags {
            match flag {
                '4' => {
                    use_ipv4 = true;
                    use_ipv6 = false;
                }
                '6' => {
                    use_ipv6 = true;
                    use_ipv4 = false;
                }
                'A' => forward_agent = true,
                'C' => compression_enabled = true,
                _ => {}
            }
        }
        index += 1;
    }

    let destination = destination?;
    let final_destination = resolve_destination(&destination, login_name.as_deref());
    if final_destination.is_empty() {
        return None;
    }

    Some(DetectedSSHSession {
        destination: final_destination,
        port,
        identity_file,
        config_file,
        jump_host,
        control_path,
        use_ipv4,
        use_ipv6,
        forward_agent,
        compression_enabled,
        ssh_options,
    })
}

// ---------------------------------------------------------------------------
// EternalTerminal command-line parser (Swift: parseEternalTerminalCommandLine)
// ---------------------------------------------------------------------------

/// Swift: nested `consumeETValue(_:for: String)`.
fn consume_et_value_str(
    value: &str,
    option: &str,
    destination: &mut Option<String>,
    jump_host: &mut Option<String>,
    login_name: &mut Option<String>,
) -> bool {
    let trimmed_value = value.trim();
    if trimmed_value.is_empty() {
        return false;
    }
    match option {
        "host" => {
            *destination = Some(trimmed_value.to_string());
        }
        "jumphost" => {
            let resolved_jump_host = resolve_eternal_terminal_destination(trimmed_value, None);
            if resolved_jump_host.is_empty() {
                return false;
            }
            *jump_host = Some(resolved_jump_host);
        }
        "jport" | "keepalive" | "port" if trimmed_value.parse::<i64>().is_err() => {
            return false;
        }
        "username" => {
            *login_name = Some(trimmed_value.to_string());
        }
        _ => {}
    }
    true
}

/// Swift: nested `consumeETValue(_:for: Character)`.
fn consume_et_value_char(
    value: &str,
    option: char,
    destination: &mut Option<String>,
    jump_host: &mut Option<String>,
    login_name: &mut Option<String>,
) -> bool {
    let trimmed_value = value.trim();
    if trimmed_value.is_empty() {
        return false;
    }
    match option {
        'k' | 'p' => trimmed_value.parse::<i64>().is_ok(),
        'u' => consume_et_value_str(trimmed_value, "username", destination, jump_host, login_name),
        _ => true,
    }
}

/// Swift: `parseEternalTerminalCommandLine(_:)`.
fn parse_eternal_terminal_command_line(arguments: &[String]) -> Option<DetectedSSHSession> {
    if arguments.is_empty() {
        return None;
    }

    let mut index = 0;
    if normalized_executable_name(&arguments[0])
        == RemoteShellTransport::EternalTerminal.executable_name()
    {
        index = 1;
    }

    let mut destination: Option<String> = None;
    let mut port: Option<i64> = None;
    let mut identity_file: Option<String> = None;
    let config_file: Option<String> = None;
    let mut jump_host: Option<String> = None;
    let mut control_path: Option<String> = None;
    let mut login_name: Option<String> = None;
    let use_ipv4 = false;
    let use_ipv6 = false;
    let mut forward_agent = false;
    let compression_enabled = false;
    let mut ssh_options: Vec<String> = Vec::new();

    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == "--" {
            let next_index = index + 1;
            if next_index < arguments.len() && destination.is_none() {
                destination = Some(arguments[next_index].clone());
            }
            break;
        }
        if !argument.starts_with('-') || argument == "-" {
            if destination.is_none() {
                destination = Some(argument.to_string());
            }
            break;
        }

        if argument == "--ssh-option" {
            let next_index = index + 1;
            if next_index >= arguments.len()
                || !consume_ssh_option(
                    &arguments[next_index],
                    &mut port,
                    &mut identity_file,
                    &mut control_path,
                    &mut jump_host,
                    &mut login_name,
                    &mut ssh_options,
                )
            {
                return None;
            }
            index += 2;
            continue;
        }
        if let Some(value) = argument.strip_prefix("--ssh-option=") {
            if !consume_ssh_option(
                value,
                &mut port,
                &mut identity_file,
                &mut control_path,
                &mut jump_host,
                &mut login_name,
                &mut ssh_options,
            ) {
                return None;
            }
            index += 1;
            continue;
        }

        if let Some(option_text) = argument.strip_prefix("--") {
            // Swift split(separator:"=", maxSplits:1, omittingEmptySubsequences:false).
            let (option_name, value_after_eq) = match option_text.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (option_text, None),
            };
            let has_value = value_after_eq.is_some();

            if option_name == "forward-ssh-agent" {
                forward_agent = true;
                index += 1;
                continue;
            }
            if option_name == "telemetry" {
                if !has_value
                    && index + 1 < arguments.len()
                    && is_bool_literal(&arguments[index + 1])
                {
                    index += 2;
                } else {
                    index += 1;
                }
                continue;
            }
            if option_name == "verbose" {
                if has_value {
                    if value_after_eq.unwrap().parse::<i64>().is_err() {
                        return None;
                    }
                } else if index + 1 < arguments.len()
                    && arguments[index + 1].parse::<i64>().is_ok()
                {
                    index += 1;
                }
                index += 1;
                continue;
            }
            if ET_LONG_VALUE_OPTIONS.contains(&option_name) {
                if let Some(value) = value_after_eq {
                    if !consume_et_value_str(
                        value,
                        option_name,
                        &mut destination,
                        &mut jump_host,
                        &mut login_name,
                    ) {
                        return None;
                    }
                    index += 1;
                } else {
                    let next_index = index + 1;
                    if next_index >= arguments.len()
                        || !consume_et_value_str(
                            &arguments[next_index],
                            option_name,
                            &mut destination,
                            &mut jump_host,
                            &mut login_name,
                        )
                    {
                        return None;
                    }
                    index += 2;
                }
                continue;
            }
            if ET_LONG_NO_ARGUMENT_OPTIONS.contains(&option_name) || has_value {
                index += 1;
                continue;
            }
            return None;
        }

        // DIVERGENCE: scalar flags, not Swift grapheme Characters — see the
        // note at `drop_first_chars`.
        let short_options: Vec<char> = argument.chars().skip(1).collect();
        let &option = short_options.first()?;
        if option == 'f' {
            forward_agent = true;
            if short_options.len() != 1 {
                return None;
            }
            index += 1;
            continue;
        }
        if option == 'v' {
            if short_options.len() > 1 {
                if drop_first_chars(argument, 2).parse::<i64>().is_err() {
                    return None;
                }
                index += 1;
            } else if index + 1 < arguments.len() && arguments[index + 1].parse::<i64>().is_ok() {
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if ET_VALUE_ARGUMENT_FLAGS.contains(option) {
            if short_options.len() > 1 {
                if !consume_et_value_char(
                    drop_first_chars(argument, 2),
                    option,
                    &mut destination,
                    &mut jump_host,
                    &mut login_name,
                ) {
                    return None;
                }
                index += 1;
            } else {
                let next_index = index + 1;
                if next_index >= arguments.len()
                    || !consume_et_value_char(
                        &arguments[next_index],
                        option,
                        &mut destination,
                        &mut jump_host,
                        &mut login_name,
                    )
                {
                    return None;
                }
                index += 2;
            }
            continue;
        }
        if !short_options
            .iter()
            .all(|&c| ET_NO_ARGUMENT_FLAGS.contains(c))
        {
            return None;
        }
        index += 1;
    }

    let destination = destination?;
    let final_destination =
        resolve_eternal_terminal_destination(&destination, login_name.as_deref());
    if final_destination.is_empty() {
        return None;
    }

    Some(DetectedSSHSession {
        destination: final_destination,
        port,
        identity_file,
        config_file,
        jump_host,
        control_path,
        use_ipv4,
        use_ipv6,
        forward_agent,
        compression_enabled,
        ssh_options,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn snap(pid: i32, pgid: i32, tpgid: i32, tty: &str, exe: &str) -> ProcessSnapshot {
        ProcessSnapshot {
            pid,
            pgid,
            tpgid,
            tty: tty.to_string(),
            executable_name: exe.to_string(),
        }
    }

    fn ssh(arguments: &[&str]) -> Option<DetectedSSHSession> {
        parse_command_line(&args(arguments), RemoteShellTransport::Ssh)
    }

    fn et(arguments: &[&str]) -> Option<DetectedSSHSession> {
        parse_command_line(&args(arguments), RemoteShellTransport::EternalTerminal)
    }

    // --- transport / basename ------------------------------------------------

    #[test]
    fn transport_from_executable_name() {
        assert_eq!(
            RemoteShellTransport::from_executable_name("ssh"),
            Some(RemoteShellTransport::Ssh)
        );
        assert_eq!(
            RemoteShellTransport::from_executable_name("/opt/homebrew/bin/et"),
            Some(RemoteShellTransport::EternalTerminal)
        );
        assert_eq!(
            RemoteShellTransport::from_executable_name("  /usr/bin/SSH  "),
            Some(RemoteShellTransport::Ssh)
        );
        assert_eq!(RemoteShellTransport::from_executable_name("bash"), None);
        assert_eq!(RemoteShellTransport::from_executable_name(""), None);
    }

    #[test]
    fn normalized_executable_name_cases() {
        assert_eq!(normalized_executable_name("/usr/bin/ssh"), "ssh");
        assert_eq!(normalized_executable_name("ET"), "et");
        assert_eq!(normalized_executable_name("  ssh  "), "ssh");
        assert_eq!(normalized_executable_name("/"), "/");
        assert_eq!(normalized_executable_name(""), "");
        assert_eq!(normalized_executable_name("foo/"), "foo");
    }

    // --- normalize_tty_name --------------------------------------------------

    #[test]
    fn normalize_tty_name_cases() {
        assert_eq!(normalize_tty_name("/dev/ttys004"), "ttys004");
        assert_eq!(normalize_tty_name("ttys004"), "ttys004");
        assert_eq!(normalize_tty_name("  /dev/ttys004  "), "ttys004");
        assert_eq!(normalize_tty_name("/"), "/");
        assert_eq!(normalize_tty_name(""), "");
    }

    // --- parse_process_snapshot ----------------------------------------------

    #[test]
    fn parse_process_snapshot_cases() {
        assert_eq!(
            parse_process_snapshot("2145 1967 1967 ttys004 ssh"),
            Some(snap(2145, 1967, 1967, "ttys004", "ssh"))
        );
        // Extra whitespace + executable name preserving internal spaces + lowercasing.
        assert_eq!(
            parse_process_snapshot("  10  20  30  ttys001   My Shell  "),
            Some(snap(10, 20, 30, "ttys001", "my shell"))
        );
        assert_eq!(parse_process_snapshot("only three fields here"), None);
        assert_eq!(parse_process_snapshot("x 1 1 tty ssh"), None);
    }

    #[test]
    fn parse_process_snapshot_whitespace_only_fifth_field() {
        // Swift split(maxSplits: 4) consumes ONE separator after the fourth
        // field and does NOT left-trim the remainder: two trailing spaces
        // leave a whitespace-only fifth subsequence, which trims to an empty
        // executable name.
        assert_eq!(
            parse_process_snapshot("2145 1967 1967 ttys004  "),
            Some(snap(2145, 1967, 1967, "ttys004", ""))
        );
        // Tab+space remainder after the consumed separator is likewise a
        // (whitespace-only) fifth field.
        assert_eq!(
            parse_process_snapshot("1 2 3 tty \t "),
            Some(snap(1, 2, 3, "tty", ""))
        );
        // Exactly one trailing separator: the remainder after it is empty ->
        // only four parts -> rejected.
        assert_eq!(parse_process_snapshot("2145 1967 1967 ttys004 "), None);
        // No trailing separator at all -> four parts -> rejected.
        assert_eq!(parse_process_snapshot("2145 1967 1967 ttys004"), None);
    }

    // --- ssh option key/value ------------------------------------------------

    #[test]
    fn ssh_option_key_value_cases() {
        assert_eq!(ssh_option_key("Port=2200").as_deref(), Some("port"));
        assert_eq!(ssh_option_key("Port 2200").as_deref(), Some("port"));
        assert_eq!(ssh_option_key("  ").as_deref(), None);
        assert_eq!(ssh_option_value("Port=2200").as_deref(), Some("2200"));
        assert_eq!(ssh_option_value("Port 2200").as_deref(), Some("2200"));
        assert_eq!(ssh_option_value("Port  2200  x").as_deref(), Some("2200  x"));
        assert_eq!(ssh_option_value("Port=").as_deref(), None);
        assert_eq!(ssh_option_value("Port").as_deref(), None);
    }

    // --- resolve_destination -------------------------------------------------

    #[test]
    fn resolve_destination_cases() {
        assert_eq!(resolve_destination("example.com", Some("lawrence")), "lawrence@example.com");
        assert_eq!(
            resolve_destination("bob@example.com", Some("lawrence")),
            "bob@example.com"
        );
        assert_eq!(resolve_destination("example.com", None), "example.com");
        assert_eq!(resolve_destination("  ", Some("lawrence")), "");
        assert_eq!(resolve_destination("example.com", Some("  ")), "example.com");
    }

    // --- strip_eternal_terminal_server_port ----------------------------------

    #[test]
    fn strip_server_port_bracketed_ipv6_with_port() {
        assert_eq!(
            strip_eternal_terminal_server_port("[2001:db8::1]:2022"),
            "[2001:db8::1]"
        );
    }

    #[test]
    fn strip_server_port_bracketed_ipv6_without_port() {
        assert_eq!(
            strip_eternal_terminal_server_port("[2001:db8::1]"),
            "[2001:db8::1]"
        );
        // Trailing non-numeric after bracket is preserved verbatim.
        assert_eq!(
            strip_eternal_terminal_server_port("[2001:db8::1]:ssh"),
            "[2001:db8::1]:ssh"
        );
    }

    #[test]
    fn strip_server_port_hostname_with_port() {
        assert_eq!(strip_eternal_terminal_server_port("example.com:2024"), "example.com");
    }

    #[test]
    fn strip_server_port_full_ipv6_gets_bracketed() {
        assert_eq!(
            strip_eternal_terminal_server_port("2001:db8:0:0:0:0:0:1:2022"),
            "[2001:db8:0:0:0:0:0:1]"
        );
    }

    #[test]
    fn strip_server_port_compressed_ipv6_is_ambiguous_preserved() {
        assert_eq!(
            strip_eternal_terminal_server_port("2001:db8::1:2022"),
            "2001:db8::1:2022"
        );
    }

    #[test]
    fn strip_server_port_no_port() {
        assert_eq!(strip_eternal_terminal_server_port("example.com"), "example.com");
        assert_eq!(strip_eternal_terminal_server_port("host:notaport"), "host:notaport");
        assert_eq!(strip_eternal_terminal_server_port("  "), "");
    }

    // --- is_ipv6_literal (DIVERGENCE) ----------------------------------------

    #[test]
    fn is_ipv6_literal_cases() {
        assert!(is_ipv6_literal("2001:db8::1"));
        assert!(is_ipv6_literal("::1"));
        assert!(!is_ipv6_literal("example.com"));
        assert!(!is_ipv6_literal("[2001:db8::1]"));
        assert!(!is_ipv6_literal("127.0.0.1"));
    }

    // === Golden tests ported from WorkspaceRemoteConnectionTests.swift ========

    #[test]
    fn detects_foreground_ssh_session_for_tty() {
        let mut by_pid = HashMap::new();
        by_pid.insert(
            2145,
            args(&[
                "ssh",
                "-o",
                "ControlMaster=auto",
                "-o",
                "ControlPath=/tmp/cmux-ssh-%C",
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-p",
                "2200",
                "-i",
                "/Users/test/.ssh/id_ed25519",
                "lawrence@example.com",
            ]),
        );
        let session = detect_for_testing(
            "/dev/ttys004",
            &[snap(2145, 1967, 1967, "ttys004", "ssh")],
            &by_pid,
        );

        assert_eq!(
            session,
            Some(DetectedSSHSession {
                destination: "lawrence@example.com".into(),
                port: Some(2200),
                identity_file: Some("/Users/test/.ssh/id_ed25519".into()),
                config_file: None,
                jump_host: None,
                control_path: Some("/tmp/cmux-ssh-%C".into()),
                use_ipv4: false,
                use_ipv6: false,
                forward_agent: false,
                compression_enabled: false,
                ssh_options: vec!["StrictHostKeyChecking=accept-new".into()],
            })
        );
    }

    #[test]
    fn detects_foreground_ssh_session_with_short_control_path_flag() {
        let mut by_pid = HashMap::new();
        by_pid.insert(
            2145,
            args(&["ssh", "-S", "/tmp/cmux-ssh-%C", "-p", "2200", "lawrence@example.com"]),
        );
        let session = detect_for_testing(
            "/dev/ttys004",
            &[snap(2145, 1967, 1967, "ttys004", "ssh")],
            &by_pid,
        )
        .expect("session");

        assert_eq!(session.control_path.as_deref(), Some("/tmp/cmux-ssh-%C"));
        let scp = session.scp_arguments("/tmp/local.png", "/tmp/cmux-drop-123.png");
        assert!(scp.iter().any(|a| a == "ControlPath=/tmp/cmux-ssh-%C"));
        assert!(!scp.iter().any(|a| a == "-S"));
    }

    #[test]
    fn detects_foreground_eternal_terminal_session_for_tty() {
        let mut by_pid = HashMap::new();
        by_pid.insert(2145, args(&["/opt/homebrew/bin/et", "lawrence@example.com"]));
        let session = detect_for_testing(
            "/dev/ttys004",
            &[snap(2145, 1967, 1967, "ttys004", "et")],
            &by_pid,
        );

        assert_eq!(
            session,
            Some(DetectedSSHSession {
                destination: "lawrence@example.com".into(),
                port: None,
                identity_file: None,
                config_file: None,
                jump_host: None,
                control_path: None,
                use_ipv4: false,
                use_ipv6: false,
                forward_agent: false,
                compression_enabled: false,
                ssh_options: vec![],
            })
        );
    }

    #[test]
    fn detects_et_without_treating_et_port_as_ssh_port() {
        let session = et(&[
            "et",
            "-u",
            "lawrence",
            "-p",
            "2022",
            "--jport",
            "2023",
            "example.com:2024",
        ])
        .expect("session");
        assert_eq!(session.destination, "lawrence@example.com");
        assert_eq!(session.port, None);

        let scp = session.scp_arguments("/tmp/local.png", "/tmp/cmux-drop-123.png");
        assert!(!scp.iter().any(|a| a == "-P"));
        assert_eq!(scp.last().unwrap(), "lawrence@example.com:/tmp/cmux-drop-123.png");
    }

    #[test]
    fn detects_et_with_bracketed_ipv6_server_port() {
        let session = et(&["et", "-u", "lawrence", "[2001:db8::1]:2022"]).expect("session");
        assert_eq!(session.destination, "lawrence@[2001:db8::1]");
        assert_eq!(session.port, None);

        let scp = session.scp_arguments("/tmp/local.png", "/tmp/cmux-drop-123.png");
        assert!(!scp.iter().any(|a| a == "-P"));
        assert_eq!(
            scp.last().unwrap(),
            "lawrence@[2001:db8::1]:/tmp/cmux-drop-123.png"
        );
    }

    #[test]
    fn detects_et_with_full_ipv6_server_port() {
        let session = et(&["et", "-u", "lawrence", "2001:db8:0:0:0:0:0:1:2022"]).expect("session");
        assert_eq!(session.destination, "lawrence@[2001:db8:0:0:0:0:0:1]");
        assert_eq!(session.port, None);

        let scp = session.scp_arguments("/tmp/local.png", "/tmp/cmux-drop-123.png");
        assert!(!scp.iter().any(|a| a == "-P"));
        assert_eq!(
            scp.last().unwrap(),
            "lawrence@[2001:db8:0:0:0:0:0:1]:/tmp/cmux-drop-123.png"
        );
    }

    #[test]
    fn detects_et_preserves_ambiguous_compressed_ipv6_literal() {
        let session = et(&["et", "-u", "lawrence", "2001:db8::1:2022"]).expect("session");
        assert_eq!(session.destination, "lawrence@2001:db8::1:2022");
        assert_eq!(session.port, None);

        let scp = session.scp_arguments("/tmp/local.png", "/tmp/cmux-drop-123.png");
        assert!(!scp.iter().any(|a| a == "-P"));
        assert_eq!(
            scp.last().unwrap(),
            "lawrence@[2001:db8::1:2022]:/tmp/cmux-drop-123.png"
        );
    }

    #[test]
    fn detects_et_ignores_options_after_destination() {
        let session = et(&["et", "lawrence@example.com", "--ssh-option", "Port=2200"])
            .expect("session");
        assert_eq!(session.destination, "lawrence@example.com");
        assert_eq!(session.port, None);

        let scp = session.scp_arguments("/tmp/local.png", "/tmp/cmux-drop-123.png");
        assert!(!scp.iter().any(|a| a == "-P"));
        assert_eq!(scp.last().unwrap(), "lawrence@example.com:/tmp/cmux-drop-123.png");
    }

    #[test]
    fn detects_et_strips_native_jumphost_server_port() {
        let session = et(&[
            "et",
            "--jumphost",
            "relay@bastion.example.com:2022",
            "lawrence@example.com",
        ])
        .expect("session");
        assert_eq!(session.jump_host.as_deref(), Some("relay@bastion.example.com"));

        let scp = session.scp_arguments("/tmp/local.png", "/tmp/cmux-drop-123.png");
        assert_eq!(session.port, None);
        assert!(!scp.iter().any(|a| a == "-P"));
        assert!(scp.iter().any(|a| a == "-J"));
        assert!(scp.iter().any(|a| a == "relay@bastion.example.com"));
        assert!(!scp.iter().any(|a| a == "relay@bastion.example.com:2022"));
        assert_eq!(scp.last().unwrap(), "lawrence@example.com:/tmp/cmux-drop-123.png");
    }

    #[test]
    fn detects_et_ssh_options_full() {
        let session = et(&[
            "et",
            "--ssh-option",
            "Port=2200",
            "--ssh-option=IdentityFile=/Users/test/.ssh/id_ed25519",
            "--ssh-option",
            "ControlPath=/tmp/cmux-ssh-%C",
            "--ssh-option",
            "StrictHostKeyChecking=accept-new",
            "--jumphost",
            "bastion.example.com",
            "--command",
            "uptime",
            "-x",
            "lawrence@example.com",
        ]);

        assert_eq!(
            session,
            Some(DetectedSSHSession {
                destination: "lawrence@example.com".into(),
                port: Some(2200),
                identity_file: Some("/Users/test/.ssh/id_ed25519".into()),
                config_file: None,
                jump_host: Some("bastion.example.com".into()),
                control_path: Some("/tmp/cmux-ssh-%C".into()),
                use_ipv4: false,
                use_ipv6: false,
                forward_agent: false,
                compression_enabled: false,
                ssh_options: vec!["StrictHostKeyChecking=accept-new".into()],
            })
        );
    }

    #[test]
    fn detects_foreground_ssh_session_with_lowercase_agent_flag() {
        let mut by_pid = HashMap::new();
        by_pid.insert(2145, args(&["ssh", "-a", "lawrence@example.com"]));
        let session = detect_for_testing(
            "/dev/ttys004",
            &[snap(2145, 1967, 1967, "ttys004", "ssh")],
            &by_pid,
        )
        .expect("session");
        assert_eq!(session.destination, "lawrence@example.com");
        assert!(!session.forward_agent);
    }

    #[test]
    fn detects_foreground_ssh_session_ignoring_bind_interface_value() {
        let mut by_pid = HashMap::new();
        by_pid.insert(2145, args(&["ssh", "-B", "en0", "lawrence@example.com"]));
        let session = detect_for_testing(
            "/dev/ttys004",
            &[snap(2145, 1967, 1967, "ttys004", "ssh")],
            &by_pid,
        )
        .expect("session");
        assert_eq!(session.destination, "lawrence@example.com");
    }

    #[test]
    fn ignores_background_ssh_process_for_tty() {
        let mut by_pid = HashMap::new();
        by_pid.insert(2145, args(&["ssh", "lawrence@example.com"]));
        // pgid != tpgid → background → not selected.
        let session = detect_for_testing(
            "ttys004",
            &[snap(2145, 2145, 1967, "ttys004", "ssh")],
            &by_pid,
        );
        assert_eq!(session, None);
    }

    #[test]
    fn brackets_ipv6_literal_scp_destination() {
        let session = DetectedSSHSession {
            destination: "lawrence@2001:db8::1".into(),
            port: None,
            identity_file: None,
            config_file: None,
            jump_host: None,
            control_path: None,
            use_ipv4: false,
            use_ipv6: false,
            forward_agent: false,
            compression_enabled: false,
            ssh_options: vec![],
        };
        let scp = session.scp_arguments("/tmp/local.png", "/tmp/cmux-drop-123.png");
        assert_eq!(
            scp.last().unwrap(),
            "lawrence@[2001:db8::1]:/tmp/cmux-drop-123.png"
        );
    }

    // === Additional focused parser coverage ==================================

    #[test]
    fn ssh_plain_destination_and_user_at_host() {
        assert_eq!(ssh(&["ssh", "example.com"]).unwrap().destination, "example.com");
        assert_eq!(
            ssh(&["ssh", "lawrence@example.com"]).unwrap().destination,
            "lawrence@example.com"
        );
    }

    #[test]
    fn ssh_login_name_flag_assembles_destination() {
        let session = ssh(&["ssh", "-l", "lawrence", "example.com"]).unwrap();
        assert_eq!(session.destination, "lawrence@example.com");
    }

    #[test]
    fn ssh_jump_host_and_identity_and_config() {
        let session = ssh(&[
            "ssh", "-J", "bastion", "-i", "/id", "-F", "/cfg", "example.com",
        ])
        .unwrap();
        assert_eq!(session.jump_host.as_deref(), Some("bastion"));
        assert_eq!(session.identity_file.as_deref(), Some("/id"));
        assert_eq!(session.config_file.as_deref(), Some("/cfg"));
    }

    #[test]
    fn ssh_clustered_value_flag() {
        // -p2200 clustered value.
        let session = ssh(&["ssh", "-p2200", "example.com"]).unwrap();
        assert_eq!(session.port, Some(2200));
    }

    #[test]
    fn ssh_o_filtered_option_is_dropped() {
        let session = ssh(&["ssh", "-o", "BatchMode=yes", "example.com"]).unwrap();
        assert!(session.ssh_options.is_empty());
    }

    #[test]
    fn ssh_o_proxyjump_and_user_and_port() {
        let session = ssh(&[
            "ssh",
            "-o",
            "ProxyJump=bastion",
            "-o",
            "User=lawrence",
            "-o",
            "Port=2200",
            "example.com",
        ])
        .unwrap();
        assert_eq!(session.jump_host.as_deref(), Some("bastion"));
        assert_eq!(session.destination, "lawrence@example.com");
        assert_eq!(session.port, Some(2200));
    }

    #[test]
    fn ssh_double_dash_terminator() {
        let session = ssh(&["ssh", "--", "-weirdhost"]).unwrap();
        assert_eq!(session.destination, "-weirdhost");
    }

    #[test]
    fn ssh_ipv4_ipv6_and_agent_and_compression_flags() {
        let session = ssh(&["ssh", "-6", "-A", "-C", "example.com"]).unwrap();
        assert!(session.use_ipv6);
        assert!(!session.use_ipv4);
        assert!(session.forward_agent);
        assert!(session.compression_enabled);

        // Later -4 resets -6 within a cluster.
        let session = ssh(&["ssh", "-46", "example.com"]).unwrap();
        assert!(session.use_ipv6);
        assert!(!session.use_ipv4);
    }

    #[test]
    fn ssh_combining_mark_flag_divergence() {
        // DIVERGENCE pin (see note at drop_first_chars): Swift counts grapheme
        // clusters — "-i\u{301}".count == 2 and the Character "i\u{301}" is in
        // no flag set, so Swift returns nil. Rust counts Unicode scalars (3),
        // takes the clustered `-i` value branch, and accepts "\u{301}" as the
        // identity file.
        let session = ssh(&["ssh", "-i\u{301}", "example.com"]).expect("session");
        assert_eq!(session.identity_file.as_deref(), Some("\u{301}"));
        assert_eq!(session.destination, "example.com");
    }

    #[test]
    fn ssh_missing_value_returns_none() {
        assert_eq!(ssh(&["ssh", "-p"]), None);
        assert_eq!(ssh(&["ssh", "-p", "notaport", "example.com"]), None);
    }

    #[test]
    fn ssh_no_destination_returns_none() {
        assert_eq!(ssh(&["ssh"]), None);
        assert_eq!(ssh(&["ssh", "-4"]), None);
    }

    #[test]
    fn et_forward_ssh_agent_short_and_long() {
        let session = et(&["et", "-f", "lawrence@example.com"]).unwrap();
        assert!(session.forward_agent);
        let session = et(&["et", "--forward-ssh-agent", "lawrence@example.com"]).unwrap();
        assert!(session.forward_agent);
    }

    #[test]
    fn et_host_option_sets_destination() {
        let session = et(&["et", "--host", "example.com", "-u", "lawrence"]).unwrap();
        assert_eq!(session.destination, "lawrence@example.com");
    }

    #[test]
    fn et_verbose_and_telemetry_are_skipped() {
        let session = et(&["et", "-v", "3", "--telemetry", "false", "example.com"]).unwrap();
        assert_eq!(session.destination, "example.com");
        let session = et(&["et", "--verbose=2", "example.com"]).unwrap();
        assert_eq!(session.destination, "example.com");
    }

    #[test]
    fn et_unknown_short_flag_returns_none() {
        // 'z' is neither a value flag nor a no-argument flag.
        assert_eq!(et(&["et", "-z", "example.com"]), None);
    }

    #[test]
    fn detect_selects_highest_pid_candidate() {
        let mut by_pid = HashMap::new();
        by_pid.insert(100, args(&["ssh", "alice@low.example.com"]));
        by_pid.insert(200, args(&["ssh", "bob@high.example.com"]));
        let session = detect_for_testing(
            "ttys004",
            &[
                snap(100, 100, 100, "ttys004", "ssh"),
                snap(200, 200, 200, "ttys004", "ssh"),
            ],
            &by_pid,
        )
        .unwrap();
        assert_eq!(session.destination, "bob@high.example.com");
    }

    #[test]
    fn detect_empty_tty_returns_none() {
        assert_eq!(detect_for_testing("", &[], &HashMap::new()), None);
    }
}
