//! cmux-agent-launch-sanitizer — pure value logic that sanitizes a captured
//! agent-CLI launch argv into a shape safe to replay on session restore.
//!
//! Faithful headless port of the canonical macOS Swift enum
//! `AgentLaunchSanitizer` and its policy tables:
//!
//! - `AgentLaunchSanitizer.swift`
//! - `AgentLaunchSanitizerPrimaryPolicies.swift`
//! - `AgentLaunchSanitizerAdditionalPolicies.swift`
//! - `AgentLaunchSanitizerClaudeTeamsPolicy.swift`
//! - `AgentLaunchPromptBoundaryOptions.swift`
//! - `AgentLaunchOptionalValueHeuristics.swift`
//! - the `argumentsByReplacingOpenAICodexProvider` half of
//!   `HermesAgentCodexEnvironment.swift`
//!
//! Like the Swift source this is pure value logic over `String` tokens — no
//! process spawn, socket, filesystem, or windowing — so it runs headless and is
//! testable in isolation.
//!
//! # Sanctioned divergences from the Swift source
//!
//! 1. **Platform swap only.** The single sanctioned divergence class. Concretely:
//!    - **JSON.** Swift's `--settings` hook-stripping uses Foundation
//!      `JSONSerialization`; this port uses `serde_json`. Both parse a top-level
//!      object (fragments/arrays/scalars ⇒ "not applicable"), re-serialize
//!      compactly with **sorted keys** at every nesting level (serde_json's
//!      default `BTreeMap`-backed map == Foundation's `.sortedKeys`), and emit
//!      UTF-8 without escaping `/` or non-ASCII. The observable output is
//!      identical for the settings objects cmux injects.
//!    - **`NSString.expandingTildeInPath`** (only reached by
//!      [`removing_saved_working_directory_options`] when a path begins with `~`)
//!      resolves the home directory from the `HOME` / `USERPROFILE` environment
//!      instead of `NSHomeDirectory()`. Non-tilde inputs — the overwhelmingly
//!      common case, and the only one the fast-path equality check reaches — are
//!      byte-for-byte identical.
//! 2. **Whitespace.** Swift's `.trimmingCharacters(in: .whitespacesAndNewlines)`
//!    maps to [`str::trim`] and `.rangeOfCharacter(from: .whitespacesAndNewlines)`
//!    to [`char::is_whitespace`]; the Unicode `White_Space` set is the same set of
//!    scalars as Swift's `whitespacesAndNewlines`.
//! 3. **Length gate counts Unicode scalars, not grapheme clusters.**
//!    [`looks_like_codex_session_identifier`]'s `>= 20` length gate uses
//!    `trimmed.chars().count()` (a count of Unicode scalars) where Swift's
//!    `trimmed.count` counts extended grapheme clusters. The two counts differ
//!    only for tokens containing a multi-scalar grapheme cluster (e.g. a base
//!    character plus a combining mark, which is several scalars but one
//!    grapheme). Such a token can never occur in the domain this gate accepts —
//!    codex session ids are ASCII (hex digits plus `-`, or the `019…` ULID
//!    prefix), and every ASCII scalar is its own single grapheme — so the two
//!    counts always coincide here and the divergence is operationally
//!    unreachable. Documented rather than "fixed" to avoid pulling in a
//!    `unicode-segmentation` dependency for a case that cannot arise; there is no
//!    dependency-free way to count grapheme clusters exactly.
//!
//! No Windows *path adaptation* is performed: the sanitizer only ever compares
//! and reorders argv string tokens (the one path-shaped transform — opencode's
//! `\` → `/` normalization before the `$bunfs` worker-js check — is transcribed
//! verbatim from Swift, backslashes included).

mod policies;
mod prompt_boundary;

use policies::{
    amp_policy, antigravity_policy, claude_policy, claude_teams_policy, code_buddy_policy,
    codex_policy, copilot_policy, cursor_policy, factory_policy, gemini_policy, grok_policy,
    hermes_agent_policy, kiro_policy, open_code_policy, pi_policy, qoder_policy, rovo_dev_policy,
    Policy,
};
use prompt_boundary::{consume_prompt_boundary_option, is_option_token};
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Small shared helpers (byte-for-byte semantics of the Swift primitives).
// ---------------------------------------------------------------------------

/// Swift `value.rangeOfCharacter(from: .whitespacesAndNewlines) != nil`.
pub(crate) fn contains_whitespace_or_newline(value: &str) -> bool {
    value.chars().any(char::is_whitespace)
}

/// The substring before the first `=`, or `None` if there is no `=`.
/// Mirrors `String(arg[..<arg.firstIndex(of: "=")!])`.
pub(crate) fn option_before_equals(arg: &str) -> Option<&str> {
    arg.find('=').map(|idx| &arg[..idx])
}

// ---------------------------------------------------------------------------
// Public API — mirrors the `public static func`s of `AgentLaunchSanitizer`.
// ---------------------------------------------------------------------------

/// `AgentLaunchSanitizer.sanitizedLaunchArguments(_:launcher:fallbackKind:)`.
///
/// Routes the top-level launcher (`claudeTeams` / `codexTeams` / `omo` / `omx` /
/// `omc`) then the `fallbackKind` (`codex` / `rovodev` / default), rebuilding the
/// executable + wrapper-verb prefix around the preserved tail. Returns `None`
/// when the shape is not safe to replay.
#[must_use]
pub fn sanitized_launch_arguments(
    arguments: &[String],
    launcher: &str,
    fallback_kind: &str,
) -> Option<Vec<String>> {
    let executable = arguments.first()?;
    if executable.is_empty() {
        return None;
    }
    let executable = executable.clone();
    let mut tail: Vec<String> = arguments[1..].to_vec();

    match launcher {
        "claudeTeams" => {
            if tail.first().map(String::as_str) == Some("claude-teams") {
                tail.remove(0);
            }
            let preserved = preserved_claude_teams_launch_arguments(&tail)?;
            let mut out = vec![executable, "claude-teams".to_owned()];
            out.extend(preserved);
            return Some(out);
        }
        "codexTeams" => {
            if tail.first().map(String::as_str) == Some("codex-teams") {
                tail.remove(0);
            }
            let preserved = preserved_codex_launch_arguments(&tail)?;
            let mut out = vec![executable, "codex-teams".to_owned()];
            out.extend(preserved);
            return Some(out);
        }
        "omo" => {
            if tail.first().map(String::as_str) == Some("omo") {
                tail.remove(0);
            }
            let preserved = preserved_arguments("opencode", &tail)?;
            let mut out = vec![executable, "omo".to_owned()];
            out.extend(preserved);
            return Some(out);
        }
        "omx" | "omc" => return None,
        _ => {}
    }

    match fallback_kind {
        "codex" => {
            let preserved = preserved_codex_launch_arguments(&tail)?;
            let mut out = vec![executable];
            out.extend(preserved);
            Some(out)
        }
        "rovodev" => {
            let preserved = preserved_arguments(fallback_kind, &tail)?;
            let mut out = vec![executable, "rovodev".to_owned(), "run".to_owned()];
            out.extend(preserved);
            Some(out)
        }
        _ => {
            let preserved = preserved_arguments(fallback_kind, &tail)?;
            let mut out = vec![executable];
            out.extend(preserved);
            Some(out)
        }
    }
}

/// `AgentLaunchSanitizer.preservedArguments(kind:args:)`.
///
/// `None` means the kind/shape is not restorable; `Some(vec)` (possibly empty)
/// means it is restorable with those preserved arguments.
#[must_use]
pub fn preserved_arguments(kind: &str, args: &[String]) -> Option<Vec<String>> {
    match kind {
        "claude" => preserve_options(args, &claude_policy()),
        "codex" => preserve_options(args, &codex_policy()),
        "grok" => preserve_options(args, &grok_policy()),
        "pi" | "omp" => preserve_options(args, &pi_policy()),
        "amp" => {
            // Strip a leading `threads continue <id>` resume sub-subcommand (with
            // the documented aliases) so it is not double-added on replay.
            let mut tail: Vec<String> = args.to_vec();
            let threads_aliases = ["threads", "thread", "t"];
            let continue_aliases = ["continue", "c"];
            if tail
                .first()
                .is_some_and(|first| threads_aliases.contains(&first.as_str()))
            {
                tail.remove(0);
                if tail
                    .first()
                    .is_some_and(|next| continue_aliases.contains(&next.as_str()))
                {
                    tail.remove(0);
                    if tail.first().is_some_and(|candidate| !candidate.starts_with('-')) {
                        tail.remove(0);
                    }
                }
            }
            preserve_options(&tail, &amp_policy())
        }
        "cursor" => {
            let mut tail: Vec<String> = args.to_vec();
            if tail.first().map(String::as_str) == Some("agent") {
                tail.remove(0);
            }
            preserve_options(&tail, &cursor_policy())
        }
        "gemini" => preserve_options(args, &gemini_policy()),
        "kiro" => {
            let mut tail: Vec<String> = args.to_vec();
            if tail.first().map(String::as_str) == Some("chat") {
                tail.remove(0);
            } else if tail.first().is_some_and(|command| !command.starts_with('-')) {
                return None;
            }
            preserve_options(&tail, &kiro_policy())
        }
        "antigravity" => preserve_options(args, &antigravity_policy()),
        "opencode" => {
            let mut tail: Vec<String> = args.to_vec();
            while let Some(first) = tail.first() {
                let normalized = first.replace('\\', "/");
                let is_internal_argument = first == "tui-settings"
                    || (normalized.contains("/$bunfs/") && normalized.ends_with("/tui/worker.js"));
                if !is_internal_argument {
                    break;
                }
                tail.remove(0);
            }
            preserve_options(&tail, &open_code_policy())
        }
        "rovodev" => {
            let mut tail: Vec<String> = args.to_vec();
            if tail.first().map(String::as_str) == Some("rovodev") {
                tail.remove(0);
            }
            if tail.first().map(String::as_str) == Some("run") {
                tail.remove(0);
            } else if tail.first().is_some_and(|command| !command.starts_with('-')) {
                return None;
            }
            preserve_options(&tail, &rovo_dev_policy())
        }
        "hermes-agent" => {
            let mut tail: Vec<String> = args.to_vec();
            if tail.first().map(String::as_str) == Some("chat") {
                tail.remove(0);
            } else if tail.first().is_some_and(|command| !command.starts_with('-')) {
                return None;
            }
            let preserved = preserve_options(&tail, &hermes_agent_policy())?;
            Some(arguments_by_replacing_openai_codex_provider(preserved))
        }
        "copilot" => preserve_options(args, &copilot_policy()),
        "codebuddy" => preserve_options(args, &code_buddy_policy()),
        "factory" => preserve_options(args, &factory_policy()),
        "qoder" => preserve_options(args, &qoder_policy()),
        _ => None,
    }
}

/// `AgentLaunchSanitizer.preservedClaudeTeamsLaunchArguments(args:)`.
#[must_use]
pub fn preserved_claude_teams_launch_arguments(args: &[String]) -> Option<Vec<String>> {
    preserve_options(args, &claude_teams_policy())
}

/// `AgentLaunchSanitizer.claudeTeamsLaunchHasOption(_:args:)`.
///
/// Whether `option` appears as a real Claude *option* in claude-teams launch
/// `args`. Unlike restore preservation this does not stop at the first positional
/// (Claude honors options after a prompt positional), but it reuses the prompt
/// boundary handling so a flag-shaped token inside a `--tmux <prompt>` payload is
/// never promoted to an option.
#[must_use]
pub fn claude_teams_launch_has_option(option: &str, args: &[String]) -> bool {
    let policy = claude_teams_policy();
    let mut index = 0usize;
    let mut sink: Vec<String> = Vec::new();
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            return false;
        }
        if !arg.starts_with('-') || arg == "-" {
            index += 1;
            continue;
        }
        match consume_prompt_boundary_option(arg, args, &mut index, &policy, &mut sink) {
            None => return false,
            Some(true) => continue,
            Some(false) => {}
        }
        if arg == option || arg.starts_with(&format!("{option}=")) {
            return true;
        }
        let width = option_width(args, index, &policy);
        index += width.max(1);
    }
    false
}

/// `AgentLaunchSanitizer.preservedCodexForkArguments(args:)`.
#[must_use]
pub fn preserved_codex_fork_arguments(args: &[String]) -> Option<Vec<String>> {
    let mut tail: Vec<String> = args.to_vec();
    if let Some(fork_command) = codex_fork_command(&tail) {
        tail = drop_codex_fork_positionals(&tail, &fork_command);
    }
    preserve_options(&tail, &codex_policy())
}

/// `AgentLaunchSanitizer.removingSavedWorkingDirectoryOptions(from:workingDirectory:)`.
///
/// Drops `--cd` / `-C` / `--cwd` / `--workspace` / `-w` options (and their
/// `=value` forms) when their value is the saved working directory, so a replay
/// does not re-pin a stale cwd. A `--` terminator passes the rest through
/// untouched. Returns the arguments unchanged when `working_directory` is
/// empty/absent.
#[must_use]
pub fn removing_saved_working_directory_options(
    args: &[String],
    working_directory: Option<&str>,
) -> Vec<String> {
    let Some(working_directory) = normalized_working_directory(working_directory) else {
        return args.to_vec();
    };
    let value_options: HashSet<&'static str> =
        ["--cd", "-C", "--cwd", "--workspace", "-w"].into_iter().collect();
    let option_prefixes: Vec<String> = value_options.iter().map(|opt| format!("{opt}=")).collect();
    let mut result: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            result.extend_from_slice(&args[index..]);
            break;
        }
        if value_options.contains(arg)
            && index + 1 < args.len()
            && working_directory_value_matches(args[index + 1].as_str(), &working_directory)
        {
            index += 2;
            continue;
        }
        if let Some(prefix) = option_prefixes.iter().find(|prefix| arg.starts_with(prefix.as_str())) {
            let value = &arg[prefix.len()..];
            if working_directory_value_matches(value, &working_directory) {
                index += 1;
                continue;
            }
        }
        result.push(arg.to_owned());
        index += 1;
    }
    result
}

// ---------------------------------------------------------------------------
// Private routing / codex-fork helpers.
// ---------------------------------------------------------------------------

/// `AgentLaunchSanitizer.preservedCodexLaunchArguments(args:)` (private).
fn preserved_codex_launch_arguments(args: &[String]) -> Option<Vec<String>> {
    if codex_fork_command(args).is_some() {
        return preserved_codex_fork_arguments(args);
    }
    preserved_arguments("codex", args)
}

struct CodexForkCommand {
    fork_index: usize,
    session_index: usize,
}

/// `codexForkCommand(in:)`.
fn codex_fork_command(args: &[String]) -> Option<CodexForkCommand> {
    let policy = codex_policy();
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            return None;
        }
        if !is_option_token(arg) || arg == "-" {
            if arg != "fork" {
                return None;
            }
            let session_index = codex_fork_command_session_index(args, index)?;
            return Some(CodexForkCommand {
                fork_index: index,
                session_index,
            });
        }
        let width = option_width(args, index, &policy);
        if policy.variadic_options.contains(arg) {
            let end = args.len().min(index + width);
            if index + 2 < end {
                for candidate_index in (index + 2)..end {
                    if args[candidate_index] == "fork" {
                        if let Some(session_index) =
                            codex_fork_command_session_index(args, candidate_index)
                        {
                            return Some(CodexForkCommand {
                                fork_index: candidate_index,
                                session_index,
                            });
                        }
                    }
                }
            }
        }
        index += width;
    }
    None
}

/// `codexForkCommandSessionIndex(_:forkIndex:)`.
fn codex_fork_command_session_index(args: &[String], fork_index: usize) -> Option<usize> {
    let policy = codex_policy();
    let mut index = fork_index + 1;
    while index < args.len() {
        let argument = args[index].as_str();
        if argument == "--" {
            return None;
        }
        if !argument.starts_with('-') || argument == "-" {
            return if looks_like_codex_session_identifier(argument) {
                Some(index)
            } else {
                None
            };
        }
        let width = option_width(args, index, &policy);
        if policy.variadic_options.contains(argument) {
            let end = args.len().min(index + width);
            if index + 2 < end {
                for (candidate_index, candidate) in
                    args.iter().enumerate().take(end).skip(index + 2)
                {
                    if looks_like_codex_session_identifier(candidate.as_str()) {
                        return Some(candidate_index);
                    }
                }
            }
        }
        index += width;
    }
    None
}

/// `looksLikeCodexSessionIdentifier(_:)`.
///
/// The `>= 20` length gate counts Unicode scalars (`chars().count()`) rather
/// than Swift's grapheme-cluster `String.count`. See crate-level sanctioned
/// divergence 3: the two counts coincide for the ASCII-only codex session-id
/// domain accepted below, so this is operationally unreachable.
fn looks_like_codex_session_identifier(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.chars().count() < 20 {
        return false;
    }
    if trimmed.starts_with("019") {
        return true;
    }
    trimmed.chars().all(|c| c.is_ascii_hexdigit() || c == '-') && trimmed.contains('-')
}

/// `dropCodexForkPositionals(_:forkCommand:)`.
fn drop_codex_fork_positionals(args: &[String], fork_command: &CodexForkCommand) -> Vec<String> {
    let policy = codex_policy();
    let mut result: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            break;
        }
        if index == fork_command.fork_index {
            index += 1;
            continue;
        }
        if index == fork_command.session_index {
            index += 1;
            while index < args.len() && !args[index].starts_with('-') {
                index += 1;
            }
            continue;
        }
        if !arg.starts_with('-') || arg == "-" {
            index += 1;
            continue;
        }
        let width = option_width(args, index, &policy);
        let end = args.len().min(index + width);
        if policy.variadic_options.contains(arg)
            && fork_command.fork_index > index
            && fork_command.fork_index < end
        {
            if fork_command.fork_index > index + 1 {
                result.extend_from_slice(&args[index..fork_command.fork_index]);
            }
            index = fork_command.fork_index;
            continue;
        }
        if policy.variadic_options.contains(arg)
            && fork_command.session_index > index
            && fork_command.session_index < end
        {
            if fork_command.session_index > index + 1 {
                result.extend_from_slice(&args[index..fork_command.session_index]);
            }
            index = fork_command.session_index;
            continue;
        }
        result.extend_from_slice(&args[index..end]);
        index += width;
    }
    result
}

// ---------------------------------------------------------------------------
// Core option-preservation scanner (`preserveOptions` + `optionWidth`).
// ---------------------------------------------------------------------------

/// `preserveOptions(_:policy:)`.
fn preserve_options(args: &[String], policy: &Policy) -> Option<Vec<String>> {
    let mut result: Vec<String> = Vec::new();
    let mut index = 0usize;
    let mut consumed_first_positional = false;
    let mut skipping_resume_positionals = false;

    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            break;
        }

        if !arg.starts_with('-') || arg == "-" {
            if let Some(resume_subcommand) = policy.resume_subcommand {
                if arg == resume_subcommand {
                    skipping_resume_positionals = true;
                    index += 1;
                    continue;
                }
            }
            if skipping_resume_positionals {
                skipping_resume_positionals = false;
                index += 1;
                continue;
            }
            if policy.non_restorable_commands.contains(arg) {
                return None;
            }
            if policy.preserve_first_positional && !consumed_first_positional {
                result.push(arg.to_owned());
                consumed_first_positional = true;
                index += 1;
                continue;
            }
            break;
        }

        if should_drop_option(arg, &policy.reject_options) {
            return None;
        }

        if policy
            .dropped_option_prefixes
            .iter()
            .any(|&prefix| arg.starts_with(prefix))
        {
            index += 1;
            continue;
        }

        let runtime_only_width = runtime_only_option_width(arg);
        let width = runtime_only_width.unwrap_or_else(|| option_width(args, index, policy));
        if runtime_only_width.is_some() || should_drop_option(arg, &policy.dropped_options) {
            index += width;
            continue;
        }

        if policy.skip_claude_hook_settings {
            if let Some(replacement) = claude_hook_settings_replacement(args, index) {
                result.extend(replacement);
                index += width;
                continue;
            }
        }

        match consume_prompt_boundary_option(arg, args, &mut index, policy, &mut result) {
            None => return None,
            Some(true) => continue,
            Some(false) => {}
        }
        let end = args.len().min(index + width);
        result.extend_from_slice(&args[index..end]);
        index += width;
    }

    Some(result)
}

/// `shouldDropOption(_:droppedOptions:)`.
fn should_drop_option(arg: &str, dropped_options: &HashSet<&'static str>) -> bool {
    if dropped_options.contains(arg) {
        return true;
    }
    match option_before_equals(arg) {
        None => false,
        Some(option) => dropped_options.contains(option),
    }
}

/// `runtimeOnlyOptionWidth(_:)`. `runtimeOnlyOptionWidths == ["--use-system-ca": 1]`.
fn runtime_only_option_width(arg: &str) -> Option<usize> {
    if arg == "--use-system-ca" {
        return Some(1);
    }
    match option_before_equals(arg) {
        Some("--use-system-ca") => Some(1),
        _ => None,
    }
}

/// `optionWidth(_:index:policy:)`. The unused-in-practice `stopVariadicAtPositionals`
/// parameter (always empty at every Swift call site) is omitted.
fn option_width(args: &[String], index: usize, policy: &Policy) -> usize {
    let arg = args[index].as_str();
    if arg.contains('=') {
        return 1;
    }
    if policy.optional_value_options.contains(arg) {
        if index + 1 >= args.len() {
            return 1;
        }
        let value = args[index + 1].as_str();
        if let Some(choices) = policy.optional_value_choices.get(arg) {
            return if choices.contains(value) { 2 } else { 1 };
        }
        let following = if index + 2 < args.len() {
            Some(args[index + 2].as_str())
        } else {
            None
        };
        if policy.greedy_optional_value_options.contains(arg) && looks_like_greedy_optional_value(value)
        {
            return 2;
        }
        if !looks_like_optional_value(value, following) {
            return 1;
        }
        return 2;
    }
    if !policy.value_options.contains(arg) || index + 1 >= args.len() {
        return 1;
    }
    if policy.variadic_options.contains(arg) {
        let mut end = index + 1;
        while end < args.len() && !args[end].starts_with('-') {
            end += 1;
        }
        return (end - index).max(1);
    }
    2
}

/// `looksLikeOptionalValue(_:following:)`.
fn looks_like_optional_value(value: &str, following: Option<&str>) -> bool {
    if value.is_empty() || value.starts_with('-') || contains_whitespace_or_newline(value) {
        return false;
    }
    following.is_none() || value.contains(',') || following.is_some_and(|f| f.starts_with('-'))
}

/// `AgentLaunchOptionalValueHeuristics.looksLikeGreedyOptionalValue(_:)`.
fn looks_like_greedy_optional_value(value: &str) -> bool {
    if value.is_empty() || value.starts_with('-') {
        return false;
    }
    if !contains_whitespace_or_newline(value) {
        return true;
    }
    value.starts_with('/')
        || value.starts_with("~/")
        || value.starts_with("./")
        || value.starts_with("../")
}

// ---------------------------------------------------------------------------
// Claude cmux hook-settings stripping (`--settings` replacement).
// ---------------------------------------------------------------------------

enum ClaudeHookSettingsReplacement {
    Drop,
    Settings(String),
}

/// The cmux-injected keys stripped from a merged hook-settings object.
const CLAUDE_CMUX_SETTINGS_KEYS: [&str; 2] = ["hooks", "preferredNotifChannel"];

/// `claudeHookSettingsReplacement(_:index:)`.
///
/// `None` — not a cmux hook-settings span, handle normally. `Some(vec![])` — drop
/// the whole `--settings` span. `Some(vec)` — replace with the user's settings.
fn claude_hook_settings_replacement(args: &[String], index: usize) -> Option<Vec<String>> {
    let arg = args[index].as_str();
    if let Some(value) = arg.strip_prefix("--settings=") {
        return match claude_hook_settings_replacement_value(value) {
            None => None,
            Some(ClaudeHookSettingsReplacement::Drop) => Some(Vec::new()),
            Some(ClaudeHookSettingsReplacement::Settings(user_settings)) => {
                Some(vec![format!("--settings={user_settings}")])
            }
        };
    }
    if arg != "--settings" || index + 1 >= args.len() {
        return None;
    }
    let value = args[index + 1].as_str();
    match claude_hook_settings_replacement_value(value) {
        None => None,
        Some(ClaudeHookSettingsReplacement::Drop) => Some(Vec::new()),
        Some(ClaudeHookSettingsReplacement::Settings(user_settings)) => {
            Some(vec!["--settings".to_owned(), user_settings])
        }
    }
}

/// `claudeHookSettingsReplacementValue(_:)`.
fn claude_hook_settings_replacement_value(value: &str) -> Option<ClaudeHookSettingsReplacement> {
    if let Some(object) = claude_settings_object(value) {
        if !is_claude_hook_settings_object(&object) {
            return None;
        }
        return match user_claude_settings_json(&object) {
            None => Some(ClaudeHookSettingsReplacement::Drop),
            Some(user_settings) => Some(ClaudeHookSettingsReplacement::Settings(user_settings)),
        };
    }
    if is_legacy_claude_hook_settings_value(value) {
        Some(ClaudeHookSettingsReplacement::Drop)
    } else {
        None
    }
}

/// `isLegacyClaudeHookSettingsValue(_:)`.
fn is_legacy_claude_hook_settings_value(value: &str) -> bool {
    value.contains("claude-hook") || value.contains("hooks claude")
}

/// `claudeSettingsObject(from:)` — a valid top-level JSON *object*, else `None`.
fn claude_settings_object(value: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    match serde_json::from_str::<serde_json::Value>(value).ok()? {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    }
}

/// `isClaudeHookSettingsObject(_:)`.
fn is_claude_hook_settings_object(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    if object.get("preferredNotifChannel").and_then(serde_json::Value::as_str)
        == Some("notifications_disabled")
    {
        return true;
    }
    match object.get("hooks") {
        Some(value) => contains_legacy_claude_hook_settings_value(value),
        None => false,
    }
}

/// `containsLegacyClaudeHookSettingsValue(_:)` — recursive over strings/arrays/objects.
fn contains_legacy_claude_hook_settings_value(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(string) => is_legacy_claude_hook_settings_value(string),
        serde_json::Value::Array(array) => {
            array.iter().any(contains_legacy_claude_hook_settings_value)
        }
        serde_json::Value::Object(object) => {
            object.values().any(contains_legacy_claude_hook_settings_value)
        }
        _ => false,
    }
}

/// `userClaudeSettingsJSON(fromMergedHookSettingsObject:)` — the object minus the
/// cmux keys, re-serialized compactly with sorted keys; `None` when nothing is
/// left. serde_json's default map is a `BTreeMap`, so all nesting levels sort,
/// matching Foundation's `.sortedKeys`.
fn user_claude_settings_json(object: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let mut object = object.clone();
    for key in CLAUDE_CMUX_SETTINGS_KEYS {
        object.remove(key);
    }
    if object.is_empty() {
        return None;
    }
    serde_json::to_string(&serde_json::Value::Object(object)).ok()
}

// ---------------------------------------------------------------------------
// Saved-working-directory helpers.
// ---------------------------------------------------------------------------

/// `normalizedWorkingDirectory(_:)`.
fn normalized_working_directory(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// `workingDirectoryValue(_:matches:)`.
fn working_directory_value_matches(value: &str, working_directory: &str) -> bool {
    if value == working_directory {
        return true;
    }
    expanding_tilde_in_path(value) == expanding_tilde_in_path(working_directory)
}

/// Port of `NSString.expandingTildeInPath` (see the module divergence note): a
/// leading `~` / `~/…` is expanded against the environment home directory; a
/// `~user` that cannot be resolved and any non-tilde path are returned unchanged.
fn expanding_tilde_in_path(path: &str) -> String {
    if !path.starts_with('~') {
        return path.to_owned();
    }
    let (component, remainder) = match path.find('/') {
        Some(idx) => (&path[..idx], &path[idx..]),
        None => (path, ""),
    };
    if component != "~" {
        return path.to_owned();
    }
    let Some(home) = home_directory() else {
        return path.to_owned();
    };
    let home = home.trim_end_matches('/');
    if remainder.is_empty() {
        home.to_owned()
    } else {
        format!("{home}{remainder}")
    }
}

fn home_directory() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .filter(|home| !home.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok().filter(|home| !home.is_empty()))
}

// ---------------------------------------------------------------------------
// Hermes provider rewrite (from `HermesAgentCodexEnvironment.swift`).
// ---------------------------------------------------------------------------

/// `HermesAgentCodexEnvironment.argumentsByReplacingOpenAICodexProvider(_:)`.
fn arguments_by_replacing_openai_codex_provider(arguments: Vec<String>) -> Vec<String> {
    const DEFAULT_PROVIDER: &str = "custom";
    let mut result: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < arguments.len() {
        let argument = &arguments[index];
        if argument == "--provider" && index + 1 < arguments.len() {
            result.push(argument.clone());
            let provider = &arguments[index + 1];
            result.push(if provider == "openai-codex" {
                DEFAULT_PROVIDER.to_owned()
            } else {
                provider.clone()
            });
            index += 2;
            continue;
        }
        if let Some(provider) = argument.strip_prefix("--provider=") {
            result.push(if provider == "openai-codex" {
                format!("--provider={DEFAULT_PROVIDER}")
            } else {
                argument.clone()
            });
        } else {
            result.push(argument.clone());
        }
        index += 1;
    }
    result
}

#[cfg(test)]
mod tests;
