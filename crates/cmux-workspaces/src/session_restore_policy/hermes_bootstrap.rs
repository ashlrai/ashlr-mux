//! Pure port of `WorkspaceHermesAgentCommandBootstrapper` from
//! `Packages/macOS/CmuxWorkspaces/Sources/CmuxWorkspaces/Session/WorkspaceHermesAgentCommandBootstrapper.swift:1-331`.
//!
//! Rewrites a restored `hermes-agent` shell command so replay reconfigures the
//! Codex-compatible provider/base-url/api-mode/model before running the agent,
//! and decides whether a restored tmux start command launches an OMX HUD.
//!
//! 100% pure. The only external inputs are the injected
//! [`WorkspaceHermesCodexEnvironment`] (config strings + two closures) and the
//! command string itself.
//!
//! PARITY: the Swift `shellWords(in:)` lexer is `Character`-indexed and stores
//! `String.Index` ranges that are used to splice the ORIGINAL command. The Rust
//! [`shell_words`] tracks BYTE offsets (`Range<usize>`) into the original
//! `&str` and splices on char boundaries. The lexer appends the UNQUOTED value
//! but the stored range covers the RAW (quoted) span — [`ShellWord::value`] and
//! [`ShellWord::range`] are kept separate exactly as Swift's `ShellWord` does
//! (Swift :224-227). The `isComplete && !value.isEmpty` drop rule for
//! unterminated quotes is preserved (Swift :302).

use std::ops::Range;

use super::WorkspaceHermesCodexEnvironment;
use super::WorkspaceSurfaceResumeBinding;

/// Trims the Swift `CharacterSet.whitespacesAndNewlines` set.
///
/// DIVERGENCE: Swift uses `trimmingCharacters(in: .whitespacesAndNewlines)`;
/// Rust `char::is_whitespace` covers the same ASCII space/tab/newline set and
/// the common Unicode whitespace that any restored command line ever contains
/// (matches the existing `cmux-resume` canonicalizer parity choice). They differ
/// only on exotic scalars that never appear in a restored command.
pub(super) fn trim_whitespace_and_newlines(value: &str) -> &str {
    value.trim_matches(|c: char| c.is_whitespace())
}

/// A single shell word: the cleaned (unquoted) value plus the RAW byte range it
/// occupies in the original command. Mirrors Swift `ShellWord`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:224-227`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellWord {
    value: String,
    range: Range<usize>,
}

/// Pure rewriter for restored Hermes agent commands.
///
/// Mirrors Swift `WorkspaceHermesAgentCommandBootstrapper`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:3-8`).
pub(super) struct WorkspaceHermesAgentCommandBootstrapper<'a> {
    hermes_codex_environment: &'a WorkspaceHermesCodexEnvironment,
}

const BOOTSTRAP_CONFIG_KEYS: [&str; 4] = [
    "model.provider",
    "model.base_url",
    "model.api_mode",
    "model.default",
];

impl<'a> WorkspaceHermesAgentCommandBootstrapper<'a> {
    /// Creates a bootstrapper over the injected Codex environment.
    pub(super) fn new(hermes_codex_environment: &'a WorkspaceHermesCodexEnvironment) -> Self {
        Self {
            hermes_codex_environment,
        }
    }

    /// Rewrites `binding` for startup replay when it is a `hermes-agent`
    /// agent-hook binding. Mirrors Swift `bindingForStartup(_:)`
    /// (`WorkspaceHermesAgentCommandBootstrapper.swift:10-48`).
    pub(super) fn binding_for_startup<B: WorkspaceSurfaceResumeBinding>(&self, binding: B) -> B {
        if binding.source() != Some("agent-hook") || binding.kind() != Some("hermes-agent") {
            return binding;
        }

        let mut environment = binding.environment().cloned().unwrap_or_default();
        environment = self
            .hermes_codex_environment
            .apply_default_codex_base_url(environment);
        let base_url = match normalized_surface_resume_value(
            environment
                .get(&self.hermes_codex_environment.custom_base_url_environment_key)
                .map(String::as_str),
        ) {
            Some(base_url) => base_url,
            None => return binding,
        };
        environment.insert(
            self.hermes_codex_environment
                .custom_base_url_environment_key
                .clone(),
            base_url.clone(),
        );

        let mut result = binding;
        result.set_environment(if environment.is_empty() {
            None
        } else {
            Some(environment.clone())
        });
        let replaced = self.command_by_replacing_openai_codex_provider(result.command());
        result.set_command(replaced);
        let removed = self.command_by_removing_bootstrap_prefix(result.command());
        result.set_command(removed);
        let agent_command_words = words_after_cwd_guard(&shell_words(result.command()));
        if command_sets_model_api_mode(&agent_command_words)
            || !self.command_allows_codex_bootstrap(&agent_command_words)
        {
            return result;
        }
        let hermes_executable = command_executable(&agent_command_words);

        let mut bootstrap = vec![
            format!(
                "{} config set model.provider {} >/dev/null",
                shell_quote(&hermes_executable),
                shell_quote(&self.hermes_codex_environment.default_provider)
            ),
            format!(
                "{} config set model.base_url {} >/dev/null",
                shell_quote(&hermes_executable),
                shell_quote(&base_url)
            ),
            format!(
                "{} config set model.api_mode {} >/dev/null",
                shell_quote(&hermes_executable),
                shell_quote(&self.hermes_codex_environment.codex_responses_api_mode)
            ),
        ];
        if let Some(model) = self
            .hermes_codex_environment
            .default_codex_model(&environment)
        {
            bootstrap.push(format!(
                "{} config set model.default {} >/dev/null",
                shell_quote(&hermes_executable),
                shell_quote(&model)
            ));
        }
        let inserted = command_by_inserting_bootstrap(&bootstrap, result.command());
        result.set_command(inserted);
        result
    }

    /// Returns a restorable tmux start command when the command launches an OMX
    /// HUD. Mirrors Swift `restorableTmuxStartCommand(_:)`
    /// (`WorkspaceHermesAgentCommandBootstrapper.swift:50-57`).
    pub(super) fn restorable_tmux_start_command(&self, raw_command: Option<&str>) -> Option<String> {
        let command = trim_whitespace_and_newlines(raw_command?);
        if command.is_empty() || !terminal_command_looks_like_omx_hud(command) {
            return None;
        }
        Some(command.to_string())
    }

    /// Mirrors Swift `commandByReplacingOpenAICodexProvider(_:)`
    /// (`WorkspaceHermesAgentCommandBootstrapper.swift:73-97`).
    fn command_by_replacing_openai_codex_provider(&self, command: &str) -> String {
        let words = shell_words(command);
        let mut replacements: Vec<(Range<usize>, String)> = Vec::new();
        for index in 0..words.len() {
            let word = &words[index];
            if word.value == "--provider"
                && index + 1 < words.len()
                && words[index + 1].value == "openai-codex"
            {
                replacements.push((
                    words[index + 1].range.clone(),
                    shell_quote(&self.hermes_codex_environment.default_provider),
                ));
            } else if word.value == "--provider=openai-codex" {
                replacements.push((
                    word.range.clone(),
                    shell_quote(&format!(
                        "--provider={}",
                        self.hermes_codex_environment.default_provider
                    )),
                ));
            }
        }
        let mut result = command.to_string();
        for (range, replacement) in replacements.into_iter().rev() {
            result.replace_range(range, &replacement);
        }
        result
    }

    /// Mirrors Swift `commandByRemovingBootstrapPrefix(_:)`
    /// (`WorkspaceHermesAgentCommandBootstrapper.swift:99-123`).
    fn command_by_removing_bootstrap_prefix(&self, command: &str) -> String {
        let words = shell_words(command);
        let mut scan_index = command_start_index_after_cwd_guard(&words);
        if scan_index >= words.len() {
            return command.to_string();
        }
        let remove_start_index = scan_index;
        let mut removed_bootstrap = false;

        while let Some(end_index) = bootstrap_command_end_index(&words, scan_index) {
            removed_bootstrap = true;
            scan_index = end_index;
            if scan_index < words.len() && words[scan_index].value == "&&" {
                scan_index += 1;
                continue;
            }
            break;
        }

        if !removed_bootstrap || scan_index >= words.len() {
            return command.to_string();
        }
        let remove_start = words[remove_start_index].range.start;
        let remove_end = words[scan_index].range.start;
        format!("{}{}", &command[..remove_start], &command[remove_end..])
    }

    /// Mirrors Swift `commandAllowsCodexBootstrap(_:)`
    /// (`WorkspaceHermesAgentCommandBootstrapper.swift:154-159`).
    fn command_allows_codex_bootstrap(&self, words: &[ShellWord]) -> bool {
        match provider_argument(words) {
            None => true,
            Some(provider) => {
                provider == self.hermes_codex_environment.default_provider
                    || provider == "openai-codex"
            }
        }
    }
}

/// Mirrors Swift `commandByInsertingBootstrap(_:into:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:59-71`).
fn command_by_inserting_bootstrap(bootstrap: &[String], command: &str) -> String {
    let bootstrap_command = bootstrap.join(" && ") + " && ";
    let words = shell_words(command);
    let command_start = command_start_index_after_cwd_guard(&words);
    if command_start >= words.len() {
        return bootstrap_command + command;
    }
    let insert_index = words[command_start].range.start;
    format!(
        "{}{}{}",
        &command[..insert_index],
        bootstrap_command,
        &command[insert_index..]
    )
}

/// Mirrors Swift `bootstrapCommandEndIndex(_:startIndex:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:125-141`).
fn bootstrap_command_end_index(words: &[ShellWord], start_index: usize) -> Option<usize> {
    if !(start_index + 4 < words.len()
        && command_word_is_executable(&words[start_index].value)
        && words[start_index + 1].value == "config"
        && words[start_index + 2].value == "set"
        && BOOTSTRAP_CONFIG_KEYS.contains(&words[start_index + 3].value.as_str()))
    {
        return None;
    }
    let mut end_index = start_index + 5;
    if end_index < words.len() && words[end_index].value == ">/dev/null" {
        end_index += 1;
    }
    Some(end_index)
}

/// Mirrors Swift `commandSetsModelAPIMode(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:150-152`).
fn command_sets_model_api_mode(words: &[ShellWord]) -> bool {
    words.iter().any(|word| word.value.contains("model.api_mode"))
}

/// Mirrors Swift `providerArgument(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:161-174`).
fn provider_argument(words: &[ShellWord]) -> Option<String> {
    let mut index = 0;
    while index < words.len() {
        let word = &words[index].value;
        if word == "--provider" && index + 1 < words.len() {
            return Some(words[index + 1].value.clone());
        }
        if let Some(stripped) = word.strip_prefix("--provider=") {
            return Some(stripped.to_string());
        }
        index += 1;
    }
    None
}

/// Mirrors Swift `commandExecutable(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:176-187`).
fn command_executable(words: &[ShellWord]) -> String {
    for word in words {
        if word.value == "env" || is_shell_assignment(&word.value) {
            continue;
        }
        if command_word_is_executable(&word.value) {
            return word.value.clone();
        }
    }
    "hermes".to_string()
}

/// Mirrors Swift `commandWordIsExecutable(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:189-192`).
///
/// Swift uses `(value as NSString).lastPathComponent`; the Rust
/// [`posix_last_path_component`] reproduces its trailing-slash stripping over
/// forward-slash POSIX paths (the canonical-fidelity path semantics).
fn command_word_is_executable(value: &str) -> bool {
    let basename = posix_last_path_component(value);
    basename == "hermes" || basename == "hermes-agent"
}

/// POSIX-lexical `lastPathComponent`: strip trailing `/`, then take the segment
/// after the final `/`. Matches `NSString.lastPathComponent` for the executable
/// basenames this compares against.
fn posix_last_path_component(value: &str) -> &str {
    let trimmed = value.trim_end_matches('/');
    if trimmed.is_empty() {
        return value;
    }
    match trimmed.rfind('/') {
        Some(pos) => &trimmed[pos + 1..],
        None => trimmed,
    }
}

/// Mirrors Swift `wordsAfterCwdGuard(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:194-198`).
fn words_after_cwd_guard(words: &[ShellWord]) -> Vec<ShellWord> {
    let command_start = command_start_index_after_cwd_guard(words);
    if command_start >= words.len() {
        return Vec::new();
    }
    words[command_start..].to_vec()
}

/// Mirrors Swift `commandStartIndexAfterCwdGuard(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:200-209`).
///
/// Skips a leading `{ ... &&` or `cd ... &&` cwd guard so the real agent command
/// is inspected. Returns `0` when there is no guard.
fn command_start_index_after_cwd_guard(words: &[ShellWord]) -> usize {
    let first = match words.first() {
        Some(first) => first,
        None => return 0,
    };
    if !(first.value == "{" || first.value == "cd") {
        return 0;
    }
    match words.iter().position(|word| word.value == "&&") {
        Some(and_index) => and_index + 1,
        None => 0,
    }
}

/// Mirrors Swift `isShellAssignment(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:211-222`).
fn is_shell_assignment(value: &str) -> bool {
    let equal_index = match value.find('=') {
        Some(index) => index,
        None => return false,
    };
    if equal_index == 0 {
        return false;
    }
    let key = &value[..equal_index];
    let first = match key.chars().next() {
        Some(first) => first,
        None => return false,
    };
    if !(first == '_' || first.is_alphabetic()) {
        return false;
    }
    key.chars()
        .all(|c| c == '_' || c.is_alphabetic() || c.is_numeric())
}

/// Mirrors Swift `shellWords(in:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:229-307`).
///
/// Appends the cleaned (unquoted) value while the returned range covers the raw
/// span, so callers can splice the original command. Drops an unterminated word
/// (`isComplete && !value.isEmpty`).
fn shell_words(command: &str) -> Vec<ShellWord> {
    let chars: Vec<(usize, char)> = command.char_indices().collect();
    let count = chars.len();
    let byte_at = |index: usize| -> usize {
        if index < count {
            chars[index].0
        } else {
            command.len()
        }
    };

    let mut words: Vec<ShellWord> = Vec::new();
    let mut index = 0usize;
    while index < count {
        while index < count && chars[index].1.is_whitespace() {
            index += 1;
        }
        if index >= count {
            break;
        }

        let start = byte_at(index);
        let mut value = String::new();
        let mut is_complete = true;
        while index < count && !chars[index].1.is_whitespace() {
            let character = chars[index].1;
            if character == '\'' {
                index += 1;
                let mut found_end_quote = false;
                while index < count {
                    let quoted = chars[index].1;
                    if quoted == '\'' {
                        index += 1;
                        found_end_quote = true;
                        break;
                    }
                    value.push(quoted);
                    index += 1;
                }
                if !found_end_quote {
                    is_complete = false;
                    break;
                }
            } else if character == '"' {
                index += 1;
                let mut found_end_quote = false;
                while index < count {
                    let quoted = chars[index].1;
                    if quoted == '"' {
                        index += 1;
                        found_end_quote = true;
                        break;
                    }
                    if quoted == '\\' {
                        let next = index + 1;
                        if next >= count {
                            is_complete = false;
                            index = count;
                            break;
                        }
                        value.push(chars[next].1);
                        index = next + 1;
                        continue;
                    }
                    value.push(quoted);
                    index += 1;
                }
                if !found_end_quote || !is_complete {
                    is_complete = false;
                    break;
                }
            } else if character == '\\' {
                let next = index + 1;
                if next >= count {
                    is_complete = false;
                    index = count;
                    break;
                }
                value.push(chars[next].1);
                index = next + 1;
            } else {
                value.push(character);
                index += 1;
            }
        }
        if is_complete && !value.is_empty() {
            words.push(ShellWord {
                value,
                range: start..byte_at(index),
            });
        }
    }
    words
}

/// Mirrors Swift `normalizedSurfaceResumeValue(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:309-312`).
fn normalized_surface_resume_value(value: Option<&str>) -> Option<String> {
    let trimmed = value.map(trim_whitespace_and_newlines)?;
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Mirrors Swift `shellQuote(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:314-316`).
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Mirrors Swift `terminalCommandLooksLikeOMXHud(_:)`
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:318-324`).
fn terminal_command_looks_like_omx_hud(command: &str) -> bool {
    let lowered = command.to_lowercase();
    if !terminal_command_text_contains_word(&lowered, "hud") {
        return false;
    }
    lowered.contains("omx") || lowered.contains("oh-my-codex")
}

/// Word-boundary containment matching Swift's
/// `(^|[^A-Za-z0-9_-])<word>([^A-Za-z0-9_-]|$)` case-insensitive regex
/// (`WorkspaceHermesAgentCommandBootstrapper.swift:326-330`). Hand-rolled to
/// avoid a regex dependency; the boundary char class treats `-` as a word char
/// (so `oh-my-codex` matches via the substring branch, never the `hud` word
/// branch). Callers pass an already-lowercased haystack.
fn terminal_command_text_contains_word(command: &str, word: &str) -> bool {
    let is_word_char = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    for (idx, _) in command.match_indices(word) {
        let before_ok = command[..idx]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word_char(c));
        let after = idx + word.len();
        let after_ok = command[after..]
            .chars()
            .next()
            .is_none_or(|c| !is_word_char(c));
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // --- shellWords tokenizer parity (author-derived; no dedicated Swift test) ---

    fn words(command: &str) -> Vec<(String, Range<usize>)> {
        shell_words(command)
            .into_iter()
            .map(|w| (w.value, w.range))
            .collect()
    }

    #[test]
    fn tokenizes_plain_words_with_raw_ranges() {
        // "cd /repo && hermes run"
        let got = words("cd /repo && hermes run");
        assert_eq!(
            got,
            vec![
                ("cd".to_string(), 0..2),
                ("/repo".to_string(), 3..8),
                ("&&".to_string(), 9..11),
                ("hermes".to_string(), 12..18),
                ("run".to_string(), 19..22),
            ]
        );
    }

    #[test]
    fn single_quoted_value_is_unquoted_but_range_is_raw() {
        // 'ab' occupies bytes 0..4, cleaned value "ab".
        let got = words("'ab' c");
        assert_eq!(
            got,
            vec![("ab".to_string(), 0..4), ("c".to_string(), 5..6)]
        );
    }

    #[test]
    fn double_quote_backslash_escapes_next_char() {
        // "a\"b" -> value a"b ; raw range covers all 6 bytes.
        let got = words(r#""a\"b""#);
        assert_eq!(got, vec![("a\"b".to_string(), 0..6)]);
    }

    #[test]
    fn adjacent_quote_and_bareword_join_into_one_word() {
        let got = words("a'b'c");
        assert_eq!(got, vec![("abc".to_string(), 0..5)]);
    }

    #[test]
    fn unterminated_single_quote_word_is_dropped() {
        // Swift: isComplete=false => not appended. The prior complete word stays.
        let got = words("ok 'dangling");
        assert_eq!(got, vec![("ok".to_string(), 0..2)]);
    }

    #[test]
    fn unterminated_double_quote_word_is_dropped() {
        let got = words("ok \"dangling");
        assert_eq!(got, vec![("ok".to_string(), 0..2)]);
    }

    #[test]
    fn trailing_backslash_drops_the_word() {
        // A backslash with no following char sets isComplete = false.
        let got = words(r"ok bad\");
        assert_eq!(got, vec![("ok".to_string(), 0..2)]);
    }

    #[test]
    fn empty_quoted_word_is_dropped_when_value_empty() {
        // '' -> value empty, isComplete true, dropped by !value.isEmpty.
        let got = words("'' x");
        assert_eq!(got, vec![("x".to_string(), 3..4)]);
    }

    // --- shell_quote ---

    #[test]
    fn shell_quote_wraps_and_escapes_single_quotes() {
        assert_eq!(shell_quote("hermes"), "'hermes'");
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    // --- is_shell_assignment ---

    #[test]
    fn shell_assignment_detection() {
        assert!(is_shell_assignment("FOO=bar"));
        assert!(is_shell_assignment("_x1=y"));
        assert!(!is_shell_assignment("=bar")); // equalIndex must be > start
        assert!(!is_shell_assignment("1FOO=bar")); // first char not letter/_
        assert!(!is_shell_assignment("no-equals"));
        assert!(!is_shell_assignment("fo-o=bar")); // '-' not allowed in key
    }

    // --- command_word_is_executable / basename ---

    #[test]
    fn executable_basename_recognizes_hermes() {
        assert!(command_word_is_executable("hermes"));
        assert!(command_word_is_executable("/usr/local/bin/hermes"));
        assert!(command_word_is_executable("hermes-agent"));
        assert!(command_word_is_executable("bin/hermes/")); // trailing slash stripped
        assert!(!command_word_is_executable("hermesx"));
        assert!(!command_word_is_executable("claude"));
    }

    // --- command_start_index_after_cwd_guard ---

    #[test]
    fn cwd_guard_skips_cd_prefix() {
        let w = shell_words("cd /repo && hermes run");
        assert_eq!(command_start_index_after_cwd_guard(&w), 3);
    }

    #[test]
    fn cwd_guard_skips_brace_prefix() {
        let w = shell_words("{ cd /repo && hermes run");
        // first == "{", first && at index 3 -> start 4 ("hermes")
        assert_eq!(command_start_index_after_cwd_guard(&w), 4);
        assert_eq!(w[4].value, "hermes");
    }

    #[test]
    fn cwd_guard_absent_returns_zero() {
        let w = shell_words("hermes run");
        assert_eq!(command_start_index_after_cwd_guard(&w), 0);
    }

    #[test]
    fn cwd_guard_prefix_without_and_returns_zero() {
        let w = shell_words("cd onlyone");
        assert_eq!(command_start_index_after_cwd_guard(&w), 0);
    }

    // --- provider_argument ---

    #[test]
    fn provider_argument_forms() {
        assert_eq!(
            provider_argument(&shell_words("hermes --provider openai-codex run")),
            Some("openai-codex".to_string())
        );
        assert_eq!(
            provider_argument(&shell_words("hermes --provider=custom run")),
            Some("custom".to_string())
        );
        assert_eq!(provider_argument(&shell_words("hermes run")), None);
        // --provider as the last word -> falls through to None.
        assert_eq!(provider_argument(&shell_words("hermes --provider")), None);
    }

    // --- OMX HUD word-boundary detection ---

    #[test]
    fn omx_hud_word_boundary() {
        assert!(terminal_command_looks_like_omx_hud("omx hud"));
        assert!(terminal_command_looks_like_omx_hud("oh-my-codex hud"));
        assert!(terminal_command_looks_like_omx_hud("OMX HUD")); // case-insensitive
        assert!(!terminal_command_looks_like_omx_hud("hudson omx")); // 'hud' inside word
        assert!(!terminal_command_looks_like_omx_hud("omx run")); // no 'hud'
        assert!(!terminal_command_looks_like_omx_hud("hud alone")); // no omx / oh-my-codex
    }

    // --- idempotent bootstrap removal + provider rewrite via a fake binding ---

    struct FakeBinding {
        source: Option<String>,
        kind: Option<String>,
        command: String,
        environment: Option<HashMap<String, String>>,
    }

    impl WorkspaceSurfaceResumeBinding for FakeBinding {
        fn source(&self) -> Option<&str> {
            self.source.as_deref()
        }
        fn kind(&self) -> Option<&str> {
            self.kind.as_deref()
        }
        fn command(&self) -> &str {
            &self.command
        }
        fn set_command(&mut self, command: String) {
            self.command = command;
        }
        fn cwd(&self) -> Option<&str> {
            None
        }
        fn environment(&self) -> Option<&HashMap<String, String>> {
            self.environment.as_ref()
        }
        fn set_environment(&mut self, environment: Option<HashMap<String, String>>) {
            self.environment = environment;
        }
        fn is_process_detected(&self) -> bool {
            false
        }
        fn is_agent_hook_binding(&self) -> bool {
            true
        }
        fn allows_automatic_resume(&self) -> bool {
            true
        }
        fn requires_prompt_approval(&self) -> bool {
            false
        }
        fn auto_resume(&self) -> Option<bool> {
            None
        }
        fn startup_input_with_launcher_script(
            &self,
            _temporary_directory: &std::path::Path,
            _allow_launcher_script: bool,
        ) -> Option<String> {
            None
        }
        fn startup_command_with_launcher_script(
            &self,
            _temporary_directory: &std::path::Path,
        ) -> Option<String> {
            None
        }
    }

    fn env() -> WorkspaceHermesCodexEnvironment {
        WorkspaceHermesCodexEnvironment::new(
            "OPENAI_BASE_URL".to_string(),
            "codex".to_string(),
            "responses".to_string(),
            |mut e| {
                e.insert(
                    "OPENAI_BASE_URL".to_string(),
                    "https://codex.example.test".to_string(),
                );
                e
            },
            |_| None,
        )
    }

    fn hermes_binding(command: &str) -> FakeBinding {
        FakeBinding {
            source: Some("agent-hook".to_string()),
            kind: Some("hermes-agent".to_string()),
            command: command.to_string(),
            environment: None,
        }
    }

    #[test]
    fn bootstrap_insertion_is_idempotent() {
        let env = env();
        let bootstrapper = WorkspaceHermesAgentCommandBootstrapper::new(&env);

        let once = bootstrapper.binding_for_startup(hermes_binding("hermes run"));
        let twice =
            bootstrapper.binding_for_startup(hermes_binding(&once.command));
        assert_eq!(
            once.command, twice.command,
            "bootstrap prefix must be removed before re-insertion so the command is stable"
        );
        assert!(once.command.contains("config set model.provider 'codex'"));
    }

    #[test]
    fn cwd_guard_bootstrap_inserted_after_cd_prefix() {
        let env = env();
        let bootstrapper = WorkspaceHermesAgentCommandBootstrapper::new(&env);
        let result = bootstrapper.binding_for_startup(hermes_binding("cd /repo && hermes run"));
        // The cwd guard stays before the bootstrap; bootstrap sits before hermes.
        assert!(result.command.starts_with("cd /repo && 'hermes' config set model.provider"));
        assert!(result.command.ends_with("&& hermes run"));
    }

    #[test]
    fn provider_openai_codex_is_rewritten_both_forms() {
        let env = env();
        let bootstrapper = WorkspaceHermesAgentCommandBootstrapper::new(&env);

        let spaced =
            bootstrapper.binding_for_startup(hermes_binding("hermes --provider openai-codex run"));
        assert!(spaced.command.contains("hermes --provider 'codex' run"));

        let joined =
            bootstrapper.binding_for_startup(hermes_binding("hermes --provider=openai-codex run"));
        assert!(joined.command.contains("hermes '--provider=codex' run"));
    }

    #[test]
    fn foreign_provider_blocks_bootstrap() {
        let env = env();
        let bootstrapper = WorkspaceHermesAgentCommandBootstrapper::new(&env);
        let result =
            bootstrapper.binding_for_startup(hermes_binding("hermes --provider anthropic run"));
        // commandAllowsCodexBootstrap == false => no bootstrap appended.
        assert!(!result.command.contains("config set model.provider"));
        assert_eq!(result.command, "hermes --provider anthropic run");
    }

    #[test]
    fn existing_api_mode_blocks_bootstrap() {
        let env = env();
        let bootstrapper = WorkspaceHermesAgentCommandBootstrapper::new(&env);
        // `model.api_mode` embedded in a non-bootstrap-shaped word survives the
        // removal pass, so `commandSetsModelAPIMode` is true and no bootstrap is
        // appended. (A `config set model.api_mode` prefix would instead be
        // stripped as a stale bootstrap and re-inserted — see
        // `bootstrap_insertion_is_idempotent`.)
        let result =
            bootstrapper.binding_for_startup(hermes_binding("hermes --set model.api_mode=responses run"));
        assert!(!result.command.contains("config set model.provider"));
        assert_eq!(result.command, "hermes --set model.api_mode=responses run");
    }

    #[test]
    fn non_hermes_binding_is_untouched() {
        let env = env();
        let bootstrapper = WorkspaceHermesAgentCommandBootstrapper::new(&env);
        let mut binding = hermes_binding("hermes run");
        binding.kind = Some("claude-agent".to_string());
        let result = bootstrapper.binding_for_startup(binding);
        assert_eq!(result.command, "hermes run");
        assert!(result.environment.is_none());
    }

    #[test]
    fn missing_base_url_returns_binding_unchanged() {
        // applyDefaultCodexBaseURL does not set the key -> normalized value nil.
        let env = WorkspaceHermesCodexEnvironment::new(
            "OPENAI_BASE_URL".to_string(),
            "codex".to_string(),
            "responses".to_string(),
            |e| e,
            |_| None,
        );
        let bootstrapper = WorkspaceHermesAgentCommandBootstrapper::new(&env);
        let result = bootstrapper.binding_for_startup(hermes_binding("hermes run"));
        assert_eq!(result.command, "hermes run");
    }
}
