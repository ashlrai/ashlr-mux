//! Launch-environment allowlist + sanitizer.
//!
//! Verbatim Swift→Rust port of
//! `Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/AgentLaunchEnvironmentPolicy.swift`.
//!
//! The contract (cross-cutting rule 7, M3 §"Key technical decisions"):
//!
//! * **Secrets never enter the launch env.** Only the curated
//!   [`SAFE_ENVIRONMENT_KEYS`] allowlist is forwarded; `AMP_API_KEY`,
//!   `OPENAI_API_KEY`, and peers are intentionally excluded — agents re-auth
//!   from their own config dirs on resume.
//! * **`NODE_OPTIONS` is sanitized**, not copied: cmux injects
//!   `--require <…>/cmux-*/restore-node-options.cjs` and a paired
//!   `--max-old-space-size=4096` heap cap into the *parent* process; both must
//!   be stripped before the value reaches an agent, or the agent re-loads
//!   cmux's bootstrap shim and inherits the wrong heap limit.
//! * **`CLAUDE_CONFIG_DIR` is rewritten** from the legacy
//!   `~/.subrouter/codex/claude` root to `~/.codex-accounts/claude` when that
//!   account directory exists.
//! * **`hermes-agent` keys** (`CUSTOM_BASE_URL`, `HERMES_CODEX_BASE_URL`) are
//!   dropped unless the launch `kind` is exactly `"hermes-agent"`.
//!
//! Parity is asserted byte-for-byte against the Swift original by the
//! `cmux-golden` harness (see `tests/agent_env_golden.rs`).

use std::collections::BTreeMap;

/// Keys whose presence is forwarded into the agent launch environment.
///
/// Mirrors `AgentLaunchEnvironmentPolicy.safeEnvironmentKeys`
/// (`AgentLaunchEnvironmentPolicy.swift:32-83`). `AMP_API_KEY` and other
/// secrets are deliberately absent.
pub const SAFE_ENVIRONMENT_KEYS: &[&str] = &[
    "AMP_LOG_FILE",
    "AMP_LOG_LEVEL",
    "AMP_SETTINGS_FILE",
    "AMP_URL",
    "ANTHROPIC_BASE_URL",
    "ANTHROPIC_MODEL",
    "CLAUDE_CONFIG_DIR",
    "CMUX_CUSTOM_CLAUDE_PATH",
    "CMUX_ROVODEV_SESSIONS_DIR",
    "CODEX_HOME",
    "CODEBUDDY_BASE_URL",
    "CODEBUDDY_CONFIG_DIR",
    "CODEBUDDY_ENV_FILE",
    "CODEBUDDY_INTERNET_ENVIRONMENT",
    "CODEBUDDY_MODEL",
    "CODEBUDDY_SMALL_FAST_MODEL",
    "COPILOT_GH_HOST",
    "COPILOT_HOME",
    "COPILOT_MODEL",
    "COPILOT_OFFLINE",
    "COPILOT_PROVIDER_BASE_URL",
    "COPILOT_PROVIDER_MAX_OUTPUT_TOKENS",
    "COPILOT_PROVIDER_MAX_PROMPT_TOKENS",
    "COPILOT_PROVIDER_MODEL_ID",
    "COPILOT_PROVIDER_TYPE",
    "COPILOT_PROVIDER_WIRE_API",
    "COPILOT_PROVIDER_WIRE_MODEL",
    "CUSTOM_BASE_URL",
    "GEMINI_CLI_HOME",
    "GH_HOST",
    "GROK_HOME",
    "GROK_SANDBOX",
    "HERMES_CODEX_BASE_URL",
    "HERMES_HOME",
    "KIRO_HOME",
    "KIRO_LOG_LEVEL",
    "KIRO_LOG_NO_COLOR",
    "NODE_OPTIONS",
    "OPENCODE_CONFIG_DIR",
    "PI_CACHE_RETENTION",
    "PI_CONFIG_DIR",
    "PI_CODING_AGENT_DIR",
    "PI_CODING_AGENT_SESSION_DIR",
    "PI_OFFLINE",
    "PI_PACKAGE_DIR",
    "PI_SKIP_VERSION_CHECK",
    "QODER_CONFIG_DIR",
    "USE_BUILTIN_RIPGREP",
];

/// Windows system environment variables forwarded to every agent so the child
/// process can actually run.
///
/// The macOS policy forwards *only* the agent-config allowlist
/// ([`SAFE_ENVIRONMENT_KEYS`]) and replaces the process env wholesale — fine for
/// a macOS GUI child. A Windows child, however, needs the base OS variables
/// (`SystemRoot`, `TEMP`, `PATHEXT`, the processor/program-files roots, the user
/// profile dirs) or it fails to start. These are *not* secrets, so forwarding
/// them does not violate rule 7; arbitrary user/secret variables outside this
/// set and the allowlist are still dropped. `PATH` is included so the resolver's
/// rewritten search path (carried in the launch plan) reaches the child.
///
/// Matched case-insensitively against the source env, since Windows var names
/// are case-insensitive (the inherited block commonly spells `Path`, `TEMP`,
/// `SystemRoot`).
pub const ESSENTIAL_WINDOWS_ENV_KEYS: &[&str] = &[
    "ALLUSERSPROFILE",
    "APPDATA",
    "CommonProgramFiles",
    "CommonProgramFiles(x86)",
    "CommonProgramW6432",
    "COMPUTERNAME",
    "ComSpec",
    "HOMEDRIVE",
    "HOMEPATH",
    "LOCALAPPDATA",
    "NUMBER_OF_PROCESSORS",
    "OS",
    "PATH",
    "PATHEXT",
    "PROCESSOR_ARCHITECTURE",
    "PROCESSOR_IDENTIFIER",
    "PROCESSOR_LEVEL",
    "PROCESSOR_REVISION",
    "ProgramData",
    "ProgramFiles",
    "ProgramFiles(x86)",
    "ProgramW6432",
    "PUBLIC",
    "SystemDrive",
    "SystemRoot",
    "TEMP",
    "TMP",
    "USERDOMAIN",
    "USERNAME",
    "USERPROFILE",
    "windir",
];

/// Keys only forwarded when `kind == "hermes-agent"`.
///
/// Mirrors `AgentLaunchEnvironmentPolicy.hermesAgentEnvironmentKeys`
/// (`AgentLaunchEnvironmentPolicy.swift:27-30`).
pub const HERMES_AGENT_ENVIRONMENT_KEYS: &[&str] = &["CUSTOM_BASE_URL", "HERMES_CODEX_BASE_URL"];

/// Filesystem + home context for the `CLAUDE_CONFIG_DIR` rewrite.
///
/// The Swift original closes over `FileManager` and `NSHomeDirectory()`
/// (`ClaudeConfigDirectoryPath.preferredPath`,
/// `AgentLaunchEnvironmentPolicy.swift:3-24`); we inject both so the policy is
/// deterministic and testable on any OS.
pub struct ClaudeConfigContext<'a> {
    /// The user's home directory in POSIX form (legacy macOS layout uses `/`).
    pub home_directory: String,
    /// Predicate: does this absolute path name an existing directory?
    pub directory_exists: Box<dyn Fn(&str) -> bool + 'a>,
}

impl<'a> ClaudeConfigContext<'a> {
    /// Build a context backed by the real filesystem and the process home dir
    /// (`USERPROFILE` on Windows, falling back to `HOME`).
    pub fn from_process_env(env: &BTreeMap<String, String>) -> Self {
        let home_directory = env
            .get("USERPROFILE")
            .or_else(|| env.get("HOME"))
            .cloned()
            .unwrap_or_default()
            .replace('\\', "/");
        Self {
            home_directory,
            directory_exists: Box::new(|path: &str| std::path::Path::new(path).is_dir()),
        }
    }

    /// A context that performs no rewrite (home empty, nothing exists). Useful
    /// when `CLAUDE_CONFIG_DIR` is known absent or rewriting is undesired.
    pub fn inert() -> Self {
        Self {
            home_directory: String::new(),
            directory_exists: Box::new(|_| false),
        }
    }
}

/// Port of `ClaudeConfigDirectoryPath.preferredPath`.
///
/// Returns the rewritten config-dir path. Trims; if empty, returns `raw`
/// unchanged. Expands a leading `~`. If the standardized path is the legacy
/// `<home>/.subrouter/codex/claude` root (or under it) AND the parallel
/// `<home>/.codex-accounts/claude` account dir exists, rewrites the prefix;
/// otherwise returns the standardized path.
pub fn claude_config_preferred_path(raw: &str, ctx: &ClaudeConfigContext) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return raw.to_string();
    }

    let home = standardize_posix_path(&expand_tilde(&ctx.home_directory, &ctx.home_directory));
    let standardized = standardize_posix_path(&expand_tilde(trimmed, &home));

    let legacy_root = standardize_posix_path(&format!("{home}/.subrouter/codex/claude"));
    let is_legacy =
        standardized == legacy_root || standardized.starts_with(&format!("{legacy_root}/"));
    if !is_legacy {
        return standardized;
    }

    let account_root = standardize_posix_path(&format!("{home}/.codex-accounts/claude"));
    let suffix = &standardized[legacy_root.len()..];
    let candidate = format!("{account_root}{suffix}");
    if (ctx.directory_exists)(&candidate) {
        candidate
    } else {
        standardized
    }
}

/// Expand a single leading `~` to `home` (mirrors `expandingTildeInPath` for
/// the cases this policy hits: a bare `~` or a `~/…` prefix).
fn expand_tilde(path: &str, home: &str) -> String {
    if path == "~" {
        home.to_string()
    } else if let Some(rest) = path.strip_prefix("~/") {
        if home.is_empty() {
            format!("/{rest}")
        } else {
            format!("{home}/{rest}")
        }
    } else {
        path.to_string()
    }
}

/// Minimal `standardizingPath` analogue over POSIX `/` separators: normalize
/// backslashes, collapse repeated slashes and `.` segments, resolve `..`
/// against prior segments, and drop a trailing slash (except for root).
fn standardize_posix_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let is_absolute = normalized.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for segment in normalized.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if matches!(segments.last(), Some(&prev) if prev != "..") {
                    segments.pop();
                } else if !is_absolute {
                    segments.push("..");
                }
            }
            other => segments.push(other),
        }
    }
    let joined = segments.join("/");
    if is_absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// Port of `AgentLaunchEnvironmentPolicy.selectedEnvironment(from:kind:)`.
///
/// Returns the curated launch environment: every allowlisted key present in
/// `env` (value-preserved, except the two special-cased keys), with
/// `NODE_OPTIONS` sanitized and the hermes keys dropped unless `kind` is
/// `"hermes-agent"`.
pub fn selected_environment(
    env: &BTreeMap<String, String>,
    kind: Option<&str>,
    claude_ctx: &ClaudeConfigContext,
) -> BTreeMap<String, String> {
    let mut result: BTreeMap<String, String> = BTreeMap::new();

    // SAFE_ENVIRONMENT_KEYS is already in sorted order; NODE_OPTIONS handled below.
    for &key in SAFE_ENVIRONMENT_KEYS {
        if key == "NODE_OPTIONS" {
            continue;
        }
        if let Some(value) = sanitized_value(key, env.get(key).map(String::as_str), claude_ctx) {
            result.insert(key.to_string(), value);
        }
    }

    if let Some(node_options) = selected_node_options(env) {
        result.insert("NODE_OPTIONS".to_string(), node_options);
    }

    if kind != Some("hermes-agent") {
        for &key in HERMES_AGENT_ENVIRONMENT_KEYS {
            result.remove(key);
        }
    }

    result
}

/// Assemble the full launch environment for a Windows agent: the curated
/// agent-config allowlist ([`selected_environment`]) plus the Windows
/// essential-system-var passthrough ([`ESSENTIAL_WINDOWS_ENV_KEYS`], matched
/// case-insensitively against `source_env`).
///
/// Allowlisted values win over essential ones if a key appears in both (it
/// won't in practice — the two sets are disjoint). The essential var is copied
/// under its *actual* casing from `source_env` so the child sees the exact
/// Windows spelling (`Path`, `SystemRoot`, …). Working-directory / OpenCode
/// credential overrides are applied separately by the launch-plan conversion.
pub fn launch_environment(
    source_env: &BTreeMap<String, String>,
    kind: Option<&str>,
    claude_ctx: &ClaudeConfigContext,
) -> BTreeMap<String, String> {
    let mut environment = selected_environment(source_env, kind, claude_ctx);
    for &essential in ESSENTIAL_WINDOWS_ENV_KEYS {
        if let Some((actual_key, value)) = get_ignore_ascii_case(source_env, essential) {
            environment
                .entry(actual_key.clone())
                .or_insert_with(|| value.clone());
        }
    }
    environment
}

/// Case-insensitive (ASCII) lookup into an environment map, returning the actual
/// stored key/value so the caller can preserve the source casing.
fn get_ignore_ascii_case<'a>(
    env: &'a BTreeMap<String, String>,
    key: &str,
) -> Option<(&'a String, &'a String)> {
    env.iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
}

/// Port of `AgentLaunchEnvironmentPolicy.sanitizedValue(key:value:)`.
///
/// Returns `None` for non-allowlisted keys or absent values. `CLAUDE_CONFIG_DIR`
/// is path-rewritten; `NODE_OPTIONS` is token-sanitized; all other allowlisted
/// keys pass their value through unchanged.
pub fn sanitized_value(
    key: &str,
    value: Option<&str>,
    claude_ctx: &ClaudeConfigContext,
) -> Option<String> {
    if !SAFE_ENVIRONMENT_KEYS.contains(&key) {
        return None;
    }
    match key {
        "CLAUDE_CONFIG_DIR" => value.map(|v| claude_config_preferred_path(v, claude_ctx)),
        "NODE_OPTIONS" => sanitized_node_options(value),
        _ => value.map(str::to_string),
    }
}

/// Port of `AgentLaunchEnvironmentPolicy.selectedNodeOptions(from:)`.
///
/// Honors cmux's own back-channel: if `CMUX_ORIGINAL_NODE_OPTIONS_PRESENT` is
/// `"1"`, sanitize the stashed original; if `"0"`, the parent had none, so emit
/// nothing; otherwise sanitize the live `NODE_OPTIONS`.
fn selected_node_options(env: &BTreeMap<String, String>) -> Option<String> {
    match normalized_value(
        env.get("CMUX_ORIGINAL_NODE_OPTIONS_PRESENT")
            .map(String::as_str),
    ) {
        Some(ref flag) if flag == "1" => {
            sanitized_node_options(env.get("CMUX_ORIGINAL_NODE_OPTIONS").map(String::as_str))
        }
        Some(ref flag) if flag == "0" => None,
        _ => sanitized_node_options(env.get("NODE_OPTIONS").map(String::as_str)),
    }
}

/// Port of `AgentLaunchEnvironmentPolicy.sanitizedNodeOptions(_:)`.
///
/// Drops cmux's injected `--require <…>/cmux-*/restore-node-options.cjs` (both
/// the spaced `--require <path>` / `-r <path>` and inline `--require=<path>`
/// forms) and the paired injected `--max-old-space-size[=]4096` heap cap that
/// immediately follows it. Returns `None` when nothing survives.
pub fn sanitized_node_options(raw: Option<&str>) -> Option<String> {
    let tokens: Vec<&str> = raw
        .map(|s| s.split_whitespace().collect())
        .unwrap_or_default();
    if tokens.is_empty() {
        return None;
    }

    let mut sanitized: Vec<&str> = Vec::new();
    let mut index = 0usize;
    let mut should_drop_injected_heap_cap = false;
    while index < tokens.len() {
        let token = tokens[index];

        if should_drop_injected_heap_cap && is_injected_node_heap_cap(&tokens, index) {
            index += node_heap_cap_width(&tokens, index);
            should_drop_injected_heap_cap = false;
            continue;
        }
        should_drop_injected_heap_cap = false;

        if is_require_option(token)
            && index + 1 < tokens.len()
            && is_cmux_node_options_restore_module_path(tokens[index + 1])
        {
            index += 2;
            should_drop_injected_heap_cap = true;
            continue;
        }
        if let Some(path) = inline_require_option_path(token) {
            if is_cmux_node_options_restore_module_path(path) {
                index += 1;
                should_drop_injected_heap_cap = true;
                continue;
            }
        }

        sanitized.push(token);
        index += 1;
    }

    let joined = sanitized.join(" ").trim().to_string();
    if joined.is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn normalized_value(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_require_option(token: &str) -> bool {
    token == "--require" || token == "-r"
}

fn inline_require_option_path(token: &str) -> Option<&str> {
    for prefix in ["--require=", "-r="] {
        if let Some(rest) = token.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    None
}

/// Mirror of `isCmuxNodeOptionsRestoreModulePath`. Strips surrounding quotes,
/// requires the basename to be `restore-node-options.cjs`, and requires a
/// `cmux-` path segment. Backslashes are normalized so Windows-injected paths
/// (`…\cmux-*\restore-node-options.cjs`) match identically to the macOS form.
fn is_cmux_node_options_restore_module_path(value: &str) -> bool {
    let trimmed = value.trim_matches(|c| c == '\'' || c == '"');
    let basename = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed);
    if basename != "restore-node-options.cjs" {
        return false;
    }
    trimmed.replace('\\', "/").contains("/cmux-")
}

fn is_injected_node_heap_cap(tokens: &[&str], index: usize) -> bool {
    let Some(&token) = tokens.get(index) else {
        return false;
    };
    if token == "--max-old-space-size" {
        return tokens.get(index + 1) == Some(&"4096");
    }
    token == "--max-old-space-size=4096"
}

fn node_heap_cap_width(tokens: &[&str], index: usize) -> usize {
    match tokens.get(index) {
        Some(&"--max-old-space-size") => 2.min(tokens.len() - index),
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn preserves_omp_config_roots_without_persisting_secrets() {
        // Mirrors the Swift suite test of the same name.
        let selected = selected_environment(
            &env(&[
                ("OPENAI_API_KEY", "secret-should-not-persist"),
                ("PI_CODING_AGENT_DIR", "/tmp/omp-agent"),
                ("PI_CONFIG_DIR", ".custom-omp"),
            ]),
            Some("omp"),
            &ClaudeConfigContext::inert(),
        );
        assert_eq!(
            selected,
            env(&[
                ("PI_CODING_AGENT_DIR", "/tmp/omp-agent"),
                ("PI_CONFIG_DIR", ".custom-omp"),
            ])
        );
    }

    #[test]
    fn secrets_are_never_forwarded() {
        let selected = selected_environment(
            &env(&[
                ("AMP_API_KEY", "secret"),
                ("OPENAI_API_KEY", "secret"),
                ("ANTHROPIC_API_KEY", "secret"),
                ("AMP_URL", "https://amp.example"),
            ]),
            None,
            &ClaudeConfigContext::inert(),
        );
        assert_eq!(selected, env(&[("AMP_URL", "https://amp.example")]));
    }

    #[test]
    fn hermes_keys_dropped_unless_hermes_kind() {
        let input = env(&[
            ("CUSTOM_BASE_URL", "https://hermes.example"),
            ("HERMES_CODEX_BASE_URL", "https://codex.example"),
            ("GH_HOST", "github.example"),
        ]);

        let without = selected_environment(&input, None, &ClaudeConfigContext::inert());
        assert_eq!(without, env(&[("GH_HOST", "github.example")]));

        let with =
            selected_environment(&input, Some("hermes-agent"), &ClaudeConfigContext::inert());
        assert_eq!(with, input);
    }

    #[test]
    fn node_options_strips_injected_require_and_heap_cap_spaced() {
        let sanitized = sanitized_node_options(Some(
            "--require /var/folders/cmux-abc/restore-node-options.cjs --max-old-space-size 4096 --enable-source-maps",
        ));
        assert_eq!(sanitized.as_deref(), Some("--enable-source-maps"));
    }

    #[test]
    fn node_options_strips_inline_require_and_heap_cap_joined() {
        let sanitized = sanitized_node_options(Some(
            "--require=/tmp/cmux-xyz/restore-node-options.cjs --max-old-space-size=4096",
        ));
        assert_eq!(sanitized, None);
    }

    #[test]
    fn node_options_strips_windows_injected_path() {
        let sanitized = sanitized_node_options(Some(
            "--require C:\\Users\\u\\AppData\\Local\\Temp\\cmux-abc\\restore-node-options.cjs --max-old-space-size=4096 --trace-warnings",
        ));
        assert_eq!(sanitized.as_deref(), Some("--trace-warnings"));
    }

    #[test]
    fn node_options_preserves_user_require() {
        // A user's own --require (not a cmux restore shim) survives, and the
        // following heap cap is NOT dropped because no injected require preceded it.
        let sanitized = sanitized_node_options(Some(
            "--require /home/u/my-setup.js --max-old-space-size=4096",
        ));
        assert_eq!(
            sanitized.as_deref(),
            Some("--require /home/u/my-setup.js --max-old-space-size=4096")
        );
    }

    #[test]
    fn node_options_back_channel_present_zero_emits_nothing() {
        let selected = selected_node_options(&env(&[
            ("CMUX_ORIGINAL_NODE_OPTIONS_PRESENT", "0"),
            ("NODE_OPTIONS", "--enable-source-maps"),
        ]));
        assert_eq!(selected, None);
    }

    #[test]
    fn node_options_back_channel_present_one_sanitizes_original() {
        let selected = selected_node_options(&env(&[
            ("CMUX_ORIGINAL_NODE_OPTIONS_PRESENT", "1"),
            (
                "CMUX_ORIGINAL_NODE_OPTIONS",
                "--require /x/cmux-1/restore-node-options.cjs --max-old-space-size=4096 --foo",
            ),
            ("NODE_OPTIONS", "--should-be-ignored"),
        ]));
        assert_eq!(selected.as_deref(), Some("--foo"));
    }

    #[test]
    fn claude_config_dir_rewritten_when_account_dir_exists() {
        let ctx = ClaudeConfigContext {
            home_directory: "/home/u".to_string(),
            directory_exists: Box::new(|path: &str| path == "/home/u/.codex-accounts/claude"),
        };
        let rewritten = claude_config_preferred_path("/home/u/.subrouter/codex/claude", &ctx);
        assert_eq!(rewritten, "/home/u/.codex-accounts/claude");
    }

    #[test]
    fn claude_config_dir_unchanged_when_account_dir_absent() {
        let ctx = ClaudeConfigContext {
            home_directory: "/home/u".to_string(),
            directory_exists: Box::new(|_| false),
        };
        let path = claude_config_preferred_path("/home/u/.subrouter/codex/claude", &ctx);
        assert_eq!(path, "/home/u/.subrouter/codex/claude");
    }

    #[test]
    fn claude_config_dir_non_legacy_path_passes_through_standardized() {
        let ctx = ClaudeConfigContext {
            home_directory: "/home/u".to_string(),
            directory_exists: Box::new(|_| true),
        };
        let path = claude_config_preferred_path("/home/u/.config/./claude/", &ctx);
        assert_eq!(path, "/home/u/.config/claude");
    }

    #[test]
    fn launch_environment_keeps_system_vars_and_allowlist_drops_secrets() {
        let source = env(&[
            ("AMP_API_KEY", "secret"),
            ("OPENAI_API_KEY", "secret"),
            ("RANDOM_USER_VAR", "should-not-cross"),
            ("ANTHROPIC_MODEL", "claude-opus-4-8"),
            ("PATH", "C:\\rt\\bin;C:\\Windows\\System32"),
            ("SystemRoot", "C:\\Windows"),
            ("TEMP", "C:\\Users\\u\\AppData\\Local\\Temp"),
            ("PATHEXT", ".COM;.EXE;.CMD"),
        ]);
        let launch = launch_environment(&source, None, &ClaudeConfigContext::inert());

        // Allowlisted agent config kept.
        assert_eq!(
            launch.get("ANTHROPIC_MODEL").map(String::as_str),
            Some("claude-opus-4-8")
        );
        // Essential Windows system vars kept (incl. rewritten PATH).
        assert_eq!(
            launch.get("PATH").map(String::as_str),
            Some("C:\\rt\\bin;C:\\Windows\\System32")
        );
        assert_eq!(
            launch.get("SystemRoot").map(String::as_str),
            Some("C:\\Windows")
        );
        assert_eq!(
            launch.get("TEMP").map(String::as_str),
            Some("C:\\Users\\u\\AppData\\Local\\Temp")
        );
        assert_eq!(
            launch.get("PATHEXT").map(String::as_str),
            Some(".COM;.EXE;.CMD")
        );
        // Secrets and arbitrary user vars never cross.
        assert!(!launch.contains_key("AMP_API_KEY"));
        assert!(!launch.contains_key("OPENAI_API_KEY"));
        assert!(!launch.contains_key("RANDOM_USER_VAR"));
    }

    #[test]
    fn launch_environment_matches_system_vars_case_insensitively() {
        // Inherited Windows blocks commonly spell these `Path` / `Systemroot`;
        // they must still be forwarded, under their actual source casing.
        let source = env(&[
            ("Path", "C:\\Windows\\System32"),
            ("Systemroot", "C:\\Windows"),
            ("tEmP", "C:\\Temp"),
        ]);
        let launch = launch_environment(&source, None, &ClaudeConfigContext::inert());
        assert_eq!(
            launch.get("Path").map(String::as_str),
            Some("C:\\Windows\\System32")
        );
        assert_eq!(
            launch.get("Systemroot").map(String::as_str),
            Some("C:\\Windows")
        );
        assert_eq!(launch.get("tEmP").map(String::as_str), Some("C:\\Temp"));
    }

    #[test]
    fn selected_environment_rewrites_claude_config_dir() {
        let ctx = ClaudeConfigContext {
            home_directory: "/home/u".to_string(),
            directory_exists: Box::new(|path: &str| path == "/home/u/.codex-accounts/claude/sub"),
        };
        let selected = selected_environment(
            &env(&[("CLAUDE_CONFIG_DIR", "/home/u/.subrouter/codex/claude/sub")]),
            None,
            &ctx,
        );
        assert_eq!(
            selected.get("CLAUDE_CONFIG_DIR").map(String::as_str),
            Some("/home/u/.codex-accounts/claude/sub")
        );
    }
}
