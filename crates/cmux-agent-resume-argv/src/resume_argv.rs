//! Port of `AgentResumeArgv.swift`.

use crate::normalized;

/// The argument-preservation policy `AgentResumeArgv` reaches for each kind.
///
/// In the Swift source these are static methods on the `AgentLaunchSanitizer`
/// enum. That policy lives in the `cmux-agent-launch-sanitizer` lane (not editable
/// from here and not yet ported), so it is modelled as an injected trait; the real
/// sanitizer implements it once ported. `None` means "this launch shape cannot be
/// safely replayed" and propagates to a `None`/`resolved(nil)` resume result,
/// exactly as the Swift `guard let … else { return nil }`.
pub trait LaunchArgumentSanitizer {
    /// Swift `AgentLaunchSanitizer.preservedClaudeTeamsLaunchArguments(args:)`.
    fn preserved_claude_teams_launch_arguments(&self, args: &[String]) -> Option<Vec<String>>;

    /// Swift `AgentLaunchSanitizer.preservedCodexForkArguments(args:)`.
    fn preserved_codex_fork_arguments(&self, args: &[String]) -> Option<Vec<String>>;

    /// Swift `AgentLaunchSanitizer.preservedArguments(kind:args:)`.
    fn preserved_arguments(&self, kind: &str, args: &[String]) -> Option<Vec<String>>;
}

/// The result of resolving a cmux wrapper launcher (the `claude-teams` /
/// `codex-teams` / `omo` style launchers cmux injects), checked before the
/// per-kind verb.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LauncherResolution {
    /// The launcher is a cmux wrapper; the payload is its resume argv, or `None`
    /// when the wrapper has no resumable form (e.g. one-shot `omx`/`omc`).
    Resolved(Option<Vec<String>>),
    /// The launcher is a plain agent executable; fall through to
    /// [`AgentResumeArgv::built_in_kind`].
    Passthrough,
}

/// Builds the argument vector for an agent's resume/continue command.
///
/// Single source of truth shared by the app-side resume builder and the
/// `cmux-cli` surface-restore publisher, so both emit identical resume commands.
/// Pure value logic over primitives (no process/socket), so it is testable in
/// isolation.
///
/// The type is a stateless value; construct one at the call site
/// (`AgentResumeArgv::new()`). Resolution order mirrors the historical app
/// builder: a cmux wrapper launcher ([`launcher_resolution`](Self::launcher_resolution))
/// is checked first, then the per-kind verb ([`built_in_kind`](Self::built_in_kind)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentResumeArgv;

impl AgentResumeArgv {
    /// The shell token that resolves cmux's `claude` wrapper at exec time.
    ///
    /// The claude resume/fork argv emits a bare `claude` executable and relies on
    /// cmux's `claude` wrapper (which re-injects the hook `--settings` on
    /// `--resume`) being reachable. The token routes through the
    /// `CMUX_CLAUDE_WRAPPER_SHIM` managed env var, guarding on `[ -x … ]` so a dead
    /// shim path degrades to bare `claude` (PATH resolution — hooks lost but resume
    /// works). It is POSIX command substitution, which fish and csh/tcsh reject, so
    /// any command containing it must reach those shells wrapped via
    /// [`portable_claude_resume_shell_command`](Self::portable_claude_resume_shell_command).
    /// See <https://github.com/manaflow-ai/cmux/issues/5639>.
    pub const CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN: &'static str =
        r#""$([ -x "${CMUX_CLAUDE_WRAPPER_SHIM:-}" ] && printf '%s' "$CMUX_CLAUDE_WRAPPER_SHIM" || printf claude)""#;

    /// Creates a resume-argv builder. The type holds no state.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Wraps a rendered claude resume/fork command so it parses in any login shell.
    ///
    /// `/bin/sh -c '<command>'` is the one spelling every dispatching shell parses
    /// identically; the user's shell still sources its own config, `sh` inherits
    /// `CMUX_CLAUDE_WRAPPER_SHIM`, and outside cmux the unset variable still falls
    /// back to bare `claude`.
    #[must_use]
    pub fn portable_claude_resume_shell_command(posix_command: &str) -> String {
        format!("/bin/sh -c {}", posix_single_quoted(posix_command))
    }

    /// Renders claude command `parts` through
    /// [`rendering_claude_wrapper_executable`](Self::rendering_claude_wrapper_executable)
    /// and joins them, wrapping via
    /// [`portable_claude_resume_shell_command`](Self::portable_claude_resume_shell_command)
    /// only when the wrapper token was actually substituted.
    ///
    /// Claude-launcher resumes that resolve to cmux's own CLI emit no bare `claude`
    /// executable, so the `/bin/sh -c` layer is applied exactly when the token is
    /// present.
    #[must_use]
    pub fn rendered_portable_claude_resume_shell_command(
        parts: &[String],
        quote: impl Fn(&str) -> String,
    ) -> String {
        let rendered = Self::rendering_claude_wrapper_executable(parts, quote);
        let joined = rendered.join(" ");
        if rendered
            .iter()
            .any(|part| part == Self::CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN)
        {
            Self::portable_claude_resume_shell_command(&joined)
        } else {
            joined
        }
    }

    /// Renders shell command `parts` to quoted tokens, substituting
    /// [`CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN`](Self::CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN)
    /// for the first bare `claude` executable token.
    ///
    /// Only the first element equal to `claude` — the wrapper executable emitted by
    /// the claude resume/fork builders — is replaced; every other token (including
    /// any later argument that happens to be the word `claude`) is quoted normally.
    /// Call only for the claude kind.
    #[must_use]
    pub fn rendering_claude_wrapper_executable(
        parts: &[String],
        quote: impl Fn(&str) -> String,
    ) -> Vec<String> {
        let mut replaced = false;
        parts
            .iter()
            .map(|part| {
                if !replaced && part == "claude" {
                    replaced = true;
                    Self::CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN.to_owned()
                } else {
                    quote(part)
                }
            })
            .collect()
    }

    /// Resolves a resume argv from a cmux wrapper launcher, or
    /// [`LauncherResolution::Passthrough`] when the launcher is a plain agent
    /// executable.
    ///
    /// - `launcher`: the captured launcher token (e.g. `"claudeTeams"`, `"omo"`), or `None`.
    /// - `session_id`: the session/thread id to resume.
    /// - `executable_path`: the captured executable path, if any.
    /// - `arguments`: the captured launch arguments (argv, including the executable as element 0).
    #[must_use]
    pub fn launcher_resolution(
        &self,
        sanitizer: &dyn LaunchArgumentSanitizer,
        launcher: Option<&str>,
        session_id: &str,
        executable_path: Option<&str>,
        arguments: &[String],
    ) -> LauncherResolution {
        match launcher {
            Some("claudeTeams") => {
                let (executable, mut tail) =
                    command_parts(executable_path, arguments, "cmux");
                if tail.first().map(String::as_str) == Some("claude-teams") {
                    tail.remove(0);
                }
                match sanitizer.preserved_claude_teams_launch_arguments(&tail) {
                    None => LauncherResolution::Resolved(None),
                    Some(preserved) => LauncherResolution::Resolved(Some(join_argv(
                        &[executable, "claude-teams".to_owned(), "--resume".to_owned(), session_id.to_owned()],
                        preserved,
                    ))),
                }
            }
            Some("codexTeams") => {
                let (executable, mut tail) =
                    command_parts(executable_path, arguments, "cmux");
                if tail.first().map(String::as_str) == Some("codex-teams") {
                    tail.remove(0);
                }
                match sanitizer.preserved_codex_fork_arguments(&tail) {
                    None => LauncherResolution::Resolved(None),
                    Some(preserved) => LauncherResolution::Resolved(Some(join_argv(
                        &[executable, "codex-teams".to_owned(), "resume".to_owned(), session_id.to_owned()],
                        preserved,
                    ))),
                }
            }
            Some("omo") => {
                let (executable, mut tail) =
                    command_parts(executable_path, arguments, "cmux");
                if tail.first().map(String::as_str) == Some("omo") {
                    tail.remove(0);
                }
                match sanitizer.preserved_arguments("opencode", &tail) {
                    None => LauncherResolution::Resolved(None),
                    Some(preserved) => LauncherResolution::Resolved(Some(join_argv(
                        &[executable, "omo".to_owned(), "--session".to_owned(), session_id.to_owned()],
                        preserved,
                    ))),
                }
            }
            Some("omx") | Some("omc") => LauncherResolution::Resolved(None),
            _ => LauncherResolution::Passthrough,
        }
    }

    /// Builds the resume argv for a built-in agent kind, or `None` if the kind is
    /// unknown or its launch arguments cannot be preserved.
    ///
    /// - `kind`: the agent's raw kind identifier (e.g. `"claude"`, `"codex"`, `"hermes-agent"`).
    /// - `session_id`: the session/thread id to resume.
    /// - `executable_path`: the captured executable path, if any.
    /// - `arguments`: the captured launch arguments (argv, including the executable as element 0).
    #[must_use]
    pub fn built_in_kind(
        &self,
        sanitizer: &dyn LaunchArgumentSanitizer,
        kind: &str,
        session_id: &str,
        executable_path: Option<&str>,
        arguments: &[String],
    ) -> Option<Vec<String>> {
        match kind {
            "claude" => self.claude_resume_argv(sanitizer, session_id, executable_path, arguments),
            "codex" => {
                let (executable, tail) = command_parts(executable_path, arguments, "codex");
                let preserved = sanitizer.preserved_codex_fork_arguments(&tail)?;
                Some(join_argv(
                    &[executable, "resume".to_owned(), session_id.to_owned()],
                    preserved,
                ))
            }
            "grok" => self.with_option(
                sanitizer, "grok", "grok", "-r", session_id, executable_path, arguments,
            ),
            "pi" => self.with_option(
                sanitizer, "pi", "pi", "--session", session_id, executable_path, arguments,
            ),
            "omp" => self.with_option(
                sanitizer, "omp", "omp", "--session", session_id, executable_path, arguments,
            ),
            "amp" => {
                let (executable, tail) = command_parts(executable_path, arguments, "amp");
                let preserved = sanitizer.preserved_arguments("amp", &tail)?;
                // [executable, "threads", "continue"] + preserved + [sessionId]
                let mut result =
                    vec![executable, "threads".to_owned(), "continue".to_owned()];
                result.extend(preserved);
                result.push(session_id.to_owned());
                Some(result)
            }
            "cursor" => self.with_option(
                sanitizer,
                "cursor",
                "cursor-agent",
                "--resume",
                session_id,
                executable_path,
                arguments,
            ),
            "gemini" => self.with_option(
                sanitizer, "gemini", "gemini", "--resume", session_id, executable_path, arguments,
            ),
            "kiro" => {
                let (executable, tail) = command_parts(executable_path, arguments, "kiro-cli");
                let preserved = sanitizer.preserved_arguments("kiro", &tail)?;
                Some(join_argv(
                    &[
                        executable,
                        "chat".to_owned(),
                        "--resume-id".to_owned(),
                        session_id.to_owned(),
                    ],
                    preserved,
                ))
            }
            "antigravity" => self.with_option(
                sanitizer,
                "antigravity",
                "agy",
                "--conversation",
                session_id,
                executable_path,
                arguments,
            ),
            "opencode" => {
                let (executable, tail) = command_parts(executable_path, arguments, "opencode");
                let preserved = sanitizer.preserved_arguments("opencode", &tail)?;
                Some(join_argv(
                    &[executable, "--session".to_owned(), session_id.to_owned()],
                    preserved,
                ))
            }
            "rovodev" => {
                let (executable, tail) = command_parts(executable_path, arguments, "acli");
                let preserved = sanitizer.preserved_arguments("rovodev", &tail)?;
                Some(join_argv(
                    &[
                        executable,
                        "rovodev".to_owned(),
                        "run".to_owned(),
                        "--restore".to_owned(),
                        session_id.to_owned(),
                    ],
                    preserved,
                ))
            }
            "hermes-agent" => {
                let (executable, tail) = command_parts(executable_path, arguments, "hermes");
                let preserved = sanitizer.preserved_arguments("hermes-agent", &tail)?;
                // [executable] + preserved + ["--resume", sessionId]
                let mut result = vec![executable];
                result.extend(preserved);
                result.push("--resume".to_owned());
                result.push(session_id.to_owned());
                Some(result)
            }
            "copilot" => self.with_option(
                sanitizer, "copilot", "copilot", "--resume", session_id, executable_path, arguments,
            ),
            "codebuddy" => self.with_option(
                sanitizer,
                "codebuddy",
                "codebuddy",
                "--resume",
                session_id,
                executable_path,
                arguments,
            ),
            "factory" => self.with_option(
                sanitizer, "factory", "droid", "--resume", session_id, executable_path, arguments,
            ),
            "qoder" => self.with_option(
                sanitizer, "qoder", "qodercli", "--resume", session_id, executable_path, arguments,
            ),
            _ => None,
        }
    }

    /// Builds the claude resume argv, routing it through cmux's `claude` wrapper so
    /// cmux hooks fire on the resumed session.
    ///
    /// The argv keeps a bare `claude` executable as its logical value (the captured
    /// executable is intentionally ignored for claude — the wrapper resolves the
    /// real binary); the wrapper is reached at render time via
    /// [`CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN`](Self::CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN).
    fn claude_resume_argv(
        &self,
        sanitizer: &dyn LaunchArgumentSanitizer,
        session_id: &str,
        executable_path: Option<&str>,
        arguments: &[String],
    ) -> Option<Vec<String>> {
        // `parts.executable` is computed (for parity) but discarded: claude always
        // emits a bare `claude` executable. Only `parts.tail` is used.
        let (_executable, tail) = command_parts(executable_path, arguments, "claude");
        let preserved = sanitizer.preserved_arguments("claude", &tail)?;
        Some(join_argv(
            &["claude".to_owned(), "--resume".to_owned(), session_id.to_owned()],
            preserved,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn with_option(
        &self,
        sanitizer: &dyn LaunchArgumentSanitizer,
        kind: &str,
        fallback_executable: &str,
        option: &str,
        session_id: &str,
        executable_path: Option<&str>,
        arguments: &[String],
    ) -> Option<Vec<String>> {
        let (executable, tail) = command_parts(executable_path, arguments, fallback_executable);
        let preserved = sanitizer.preserved_arguments(kind, &tail)?;
        Some(join_argv(
            &[executable, option.to_owned(), session_id.to_owned()],
            preserved,
        ))
    }
}

/// Splits `arguments` into the resolved executable and its tail, mirroring
/// `AgentResumeArgv.commandParts(executablePath:arguments:fallbackExecutable:)`.
///
/// The executable is the first usable of: the captured `executable_path`, then
/// `arguments[0]`, then `fallback_executable`. The tail is `arguments` minus its
/// first element (empty when `arguments` is empty).
fn command_parts(
    executable_path: Option<&str>,
    arguments: &[String],
    fallback_executable: &str,
) -> (String, Vec<String>) {
    let executable = normalized(executable_path)
        .or_else(|| normalized(arguments.first().map(String::as_str)))
        .unwrap_or_else(|| fallback_executable.to_owned());
    let tail = if arguments.is_empty() {
        Vec::new()
    } else {
        arguments[1..].to_vec()
    };
    (executable, tail)
}

/// Concatenates a leading fixed-order argv with the preserved tail, matching the
/// Swift `[…] + preserved` idiom.
fn join_argv(head: &[String], mut preserved: Vec<String>) -> Vec<String> {
    let mut result = head.to_vec();
    result.append(&mut preserved);
    result
}

/// Single-quotes `value` as one POSIX `sh` word, escaping embedded quotes as
/// `'\''`. Re-port of the private `posixSingleQuoted(_:)` free function in
/// `AgentResumeArgv.swift`; always wraps (unlike `cmux_ipc::shell_quote`, which
/// passes safe tokens through).
fn posix_single_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    /// Sanitizer that echoes its `args` back verbatim, so tests exercise the
    /// argv-assembly ordering that this crate owns (the preservation policy is the
    /// sibling lane's concern).
    struct EchoSanitizer;
    impl LaunchArgumentSanitizer for EchoSanitizer {
        fn preserved_claude_teams_launch_arguments(&self, args: &[String]) -> Option<Vec<String>> {
            Some(args.to_vec())
        }
        fn preserved_codex_fork_arguments(&self, args: &[String]) -> Option<Vec<String>> {
            Some(args.to_vec())
        }
        fn preserved_arguments(&self, _kind: &str, args: &[String]) -> Option<Vec<String>> {
            Some(args.to_vec())
        }
    }

    /// Sanitizer that rejects every shape (`nil`), to test `None`/`Resolved(None)`
    /// propagation.
    struct NoneSanitizer;
    impl LaunchArgumentSanitizer for NoneSanitizer {
        fn preserved_claude_teams_launch_arguments(&self, _args: &[String]) -> Option<Vec<String>> {
            None
        }
        fn preserved_codex_fork_arguments(&self, _args: &[String]) -> Option<Vec<String>> {
            None
        }
        fn preserved_arguments(&self, _kind: &str, _args: &[String]) -> Option<Vec<String>> {
            None
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SanitizerCall {
        func: &'static str,
        kind: Option<String>,
        args: Vec<String>,
    }

    /// Records the `(function, kind, args)` it was called with and returns a
    /// configured result, so tests can assert the exact tail forwarded to the
    /// sanitizer (e.g. that the launcher's leading verb was stripped).
    struct RecordingSanitizer {
        result: Option<Vec<String>>,
        calls: RefCell<Vec<SanitizerCall>>,
    }
    impl RecordingSanitizer {
        fn new(result: Option<Vec<String>>) -> Self {
            Self { result, calls: RefCell::new(Vec::new()) }
        }
        fn record(&self, func: &'static str, kind: Option<&str>, args: &[String]) {
            self.calls.borrow_mut().push(SanitizerCall {
                func,
                kind: kind.map(str::to_owned),
                args: args.to_vec(),
            });
        }
    }
    impl LaunchArgumentSanitizer for RecordingSanitizer {
        fn preserved_claude_teams_launch_arguments(&self, args: &[String]) -> Option<Vec<String>> {
            self.record("claude_teams", None, args);
            self.result.clone()
        }
        fn preserved_codex_fork_arguments(&self, args: &[String]) -> Option<Vec<String>> {
            self.record("codex_fork", None, args);
            self.result.clone()
        }
        fn preserved_arguments(&self, kind: &str, args: &[String]) -> Option<Vec<String>> {
            self.record("arguments", Some(kind), args);
            self.result.clone()
        }
    }

    // ---- built_in_kind: one oracle per kind branch (echo sanitizer) ------------

    #[test]
    fn built_in_claude_ignores_captured_executable_and_uses_bare_claude() {
        // Captured executable is the real claude binary; the resume argv must still
        // emit a bare `claude` (resolved to the wrapper at render time).
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "claude",
            "SID",
            Some("/opt/homebrew/bin/claude"),
            &v(&["claude", "--flag", "value"]),
        );
        assert_eq!(got, Some(v(&["claude", "--resume", "SID", "--flag", "value"])));
    }

    #[test]
    fn built_in_codex_uses_resume_verb_and_fork_sanitizer() {
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "codex",
            "SID",
            None,
            &v(&["codex", "--flag", "value"]),
        );
        assert_eq!(got, Some(v(&["codex", "resume", "SID", "--flag", "value"])));
    }

    #[test]
    fn built_in_grok_uses_dash_r_option() {
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "grok",
            "SID",
            None,
            &v(&["grok", "--flag"]),
        );
        assert_eq!(got, Some(v(&["grok", "-r", "SID", "--flag"])));
    }

    #[test]
    fn built_in_pi_and_omp_use_session_option() {
        let a = AgentResumeArgv::new().built_in_kind(&EchoSanitizer, "pi", "SID", None, &v(&["pi"]));
        assert_eq!(a, Some(v(&["pi", "--session", "SID"])));
        let b =
            AgentResumeArgv::new().built_in_kind(&EchoSanitizer, "omp", "SID", None, &v(&["omp"]));
        assert_eq!(b, Some(v(&["omp", "--session", "SID"])));
    }

    #[test]
    fn built_in_amp_places_session_id_last_after_preserved() {
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "amp",
            "SID",
            None,
            &v(&["amp", "--flag", "value"]),
        );
        assert_eq!(
            got,
            Some(v(&["amp", "threads", "continue", "--flag", "value", "SID"]))
        );
    }

    #[test]
    fn built_in_cursor_uses_cursor_agent_fallback_and_resume_option() {
        // Empty argv + no executable path => fallback executable `cursor-agent`.
        let got =
            AgentResumeArgv::new().built_in_kind(&EchoSanitizer, "cursor", "SID", None, &[]);
        assert_eq!(got, Some(v(&["cursor-agent", "--resume", "SID"])));
    }

    #[test]
    fn built_in_gemini_uses_resume_option() {
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "gemini",
            "SID",
            None,
            &v(&["gemini"]),
        );
        assert_eq!(got, Some(v(&["gemini", "--resume", "SID"])));
    }

    #[test]
    fn built_in_kiro_uses_chat_resume_id() {
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "kiro",
            "SID",
            None,
            &v(&["kiro-cli", "--flag"]),
        );
        assert_eq!(got, Some(v(&["kiro-cli", "chat", "--resume-id", "SID", "--flag"])));
    }

    #[test]
    fn built_in_antigravity_uses_agy_fallback_and_conversation_option() {
        let got =
            AgentResumeArgv::new().built_in_kind(&EchoSanitizer, "antigravity", "SID", None, &[]);
        assert_eq!(got, Some(v(&["agy", "--conversation", "SID"])));
    }

    #[test]
    fn built_in_opencode_uses_session_option() {
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "opencode",
            "SID",
            None,
            &v(&["opencode", "--flag"]),
        );
        assert_eq!(got, Some(v(&["opencode", "--session", "SID", "--flag"])));
    }

    #[test]
    fn built_in_rovodev_uses_acli_fallback_and_restore() {
        let got =
            AgentResumeArgv::new().built_in_kind(&EchoSanitizer, "rovodev", "SID", None, &[]);
        assert_eq!(got, Some(v(&["acli", "rovodev", "run", "--restore", "SID"])));
    }

    #[test]
    fn built_in_hermes_agent_places_resume_and_session_after_preserved() {
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "hermes-agent",
            "SID",
            None,
            &v(&["hermes", "--flag", "value"]),
        );
        assert_eq!(
            got,
            Some(v(&["hermes", "--flag", "value", "--resume", "SID"]))
        );
    }

    #[test]
    fn built_in_copilot_codebuddy_factory_qoder_options_and_fallbacks() {
        let copilot =
            AgentResumeArgv::new().built_in_kind(&EchoSanitizer, "copilot", "SID", None, &[]);
        assert_eq!(copilot, Some(v(&["copilot", "--resume", "SID"])));

        let codebuddy =
            AgentResumeArgv::new().built_in_kind(&EchoSanitizer, "codebuddy", "SID", None, &[]);
        assert_eq!(codebuddy, Some(v(&["codebuddy", "--resume", "SID"])));

        // factory falls back to the `droid` executable.
        let factory =
            AgentResumeArgv::new().built_in_kind(&EchoSanitizer, "factory", "SID", None, &[]);
        assert_eq!(factory, Some(v(&["droid", "--resume", "SID"])));

        // qoder falls back to the `qodercli` executable.
        let qoder =
            AgentResumeArgv::new().built_in_kind(&EchoSanitizer, "qoder", "SID", None, &[]);
        assert_eq!(qoder, Some(v(&["qodercli", "--resume", "SID"])));
    }

    #[test]
    fn built_in_unknown_kind_is_none() {
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "not-a-real-agent",
            "SID",
            None,
            &v(&["x"]),
        );
        assert_eq!(got, None);
    }

    #[test]
    fn built_in_kind_propagates_sanitizer_none() {
        // A kind that would otherwise assemble an argv still returns None when the
        // sanitizer rejects the launch shape.
        let got = AgentResumeArgv::new().built_in_kind(
            &NoneSanitizer,
            "codex",
            "SID",
            None,
            &v(&["codex", "--flag"]),
        );
        assert_eq!(got, None);
    }

    #[test]
    fn built_in_kind_forwards_only_the_tail_to_the_sanitizer() {
        // parts.tail = arguments minus argv[0]; that is what the sanitizer sees.
        let sanitizer = RecordingSanitizer::new(Some(v(&["--flag"])));
        let _ = AgentResumeArgv::new().built_in_kind(
            &sanitizer,
            "opencode",
            "SID",
            None,
            &v(&["opencode", "--flag"]),
        );
        assert_eq!(
            *sanitizer.calls.borrow(),
            vec![SanitizerCall {
                func: "arguments",
                kind: Some("opencode".to_owned()),
                args: v(&["--flag"]),
            }]
        );
    }

    // ---- executable resolution (command_parts) --------------------------------

    #[test]
    fn executable_prefers_captured_path_then_argv0_then_fallback() {
        // Captured executable path wins (and is trimmed).
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "codex",
            "SID",
            Some("  /bin/codex-real  "),
            &v(&["codex", "--flag"]),
        );
        assert_eq!(got, Some(v(&["/bin/codex-real", "resume", "SID", "--flag"])));

        // Blank captured path falls through to argv[0].
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "codex",
            "SID",
            Some("   "),
            &v(&["codex-argv0", "--flag"]),
        );
        assert_eq!(got, Some(v(&["codex-argv0", "resume", "SID", "--flag"])));

        // Blank captured path and blank argv[0] fall through to the fallback.
        let got = AgentResumeArgv::new().built_in_kind(
            &EchoSanitizer,
            "codex",
            "SID",
            None,
            &v(&["   ", "--flag"]),
        );
        assert_eq!(got, Some(v(&["codex", "resume", "SID", "--flag"])));
    }

    // ---- launcher_resolution ---------------------------------------------------

    #[test]
    fn launcher_claude_teams_strips_leading_verb_and_assembles_resume() {
        let got = AgentResumeArgv::new().launcher_resolution(
            &EchoSanitizer,
            Some("claudeTeams"),
            "SID",
            None,
            &v(&["cmux", "claude-teams", "--flag", "value"]),
        );
        assert_eq!(
            got,
            LauncherResolution::Resolved(Some(v(&[
                "cmux",
                "claude-teams",
                "--resume",
                "SID",
                "--flag",
                "value",
            ])))
        );
    }

    #[test]
    fn launcher_claude_teams_forwards_tail_without_leading_verb() {
        let sanitizer = RecordingSanitizer::new(Some(v(&["--flag"])));
        let _ = AgentResumeArgv::new().launcher_resolution(
            &sanitizer,
            Some("claudeTeams"),
            "SID",
            None,
            &v(&["cmux", "claude-teams", "--flag"]),
        );
        assert_eq!(
            *sanitizer.calls.borrow(),
            vec![SanitizerCall { func: "claude_teams", kind: None, args: v(&["--flag"]) }]
        );
    }

    #[test]
    fn launcher_claude_teams_keeps_tail_when_no_leading_verb() {
        // tail.first != "claude-teams" => no strip; the sanitizer sees the raw tail.
        let sanitizer = RecordingSanitizer::new(Some(v(&["--flag"])));
        let got = AgentResumeArgv::new().launcher_resolution(
            &sanitizer,
            Some("claudeTeams"),
            "SID",
            None,
            &v(&["cmux", "--flag"]),
        );
        assert_eq!(
            *sanitizer.calls.borrow(),
            vec![SanitizerCall { func: "claude_teams", kind: None, args: v(&["--flag"]) }]
        );
        assert_eq!(
            got,
            LauncherResolution::Resolved(Some(v(&[
                "cmux", "claude-teams", "--resume", "SID", "--flag",
            ])))
        );
    }

    #[test]
    fn launcher_codex_teams_uses_resume_verb_and_fork_sanitizer() {
        let sanitizer = RecordingSanitizer::new(Some(v(&["--flag"])));
        let got = AgentResumeArgv::new().launcher_resolution(
            &sanitizer,
            Some("codexTeams"),
            "SID",
            None,
            &v(&["cmux", "codex-teams", "--flag"]),
        );
        assert_eq!(
            *sanitizer.calls.borrow(),
            vec![SanitizerCall { func: "codex_fork", kind: None, args: v(&["--flag"]) }]
        );
        assert_eq!(
            got,
            LauncherResolution::Resolved(Some(v(&[
                "cmux", "codex-teams", "resume", "SID", "--flag",
            ])))
        );
    }

    #[test]
    fn launcher_omo_uses_session_verb_and_opencode_sanitizer() {
        let sanitizer = RecordingSanitizer::new(Some(v(&["--flag"])));
        let got = AgentResumeArgv::new().launcher_resolution(
            &sanitizer,
            Some("omo"),
            "SID",
            None,
            &v(&["cmux", "omo", "--flag"]),
        );
        assert_eq!(
            *sanitizer.calls.borrow(),
            vec![SanitizerCall {
                func: "arguments",
                kind: Some("opencode".to_owned()),
                args: v(&["--flag"]),
            }]
        );
        assert_eq!(
            got,
            LauncherResolution::Resolved(Some(v(&[
                "cmux", "omo", "--session", "SID", "--flag",
            ])))
        );
    }

    #[test]
    fn launcher_omx_and_omc_resolve_to_none() {
        for launcher in ["omx", "omc"] {
            let got = AgentResumeArgv::new().launcher_resolution(
                &EchoSanitizer,
                Some(launcher),
                "SID",
                None,
                &v(&["cmux"]),
            );
            assert_eq!(got, LauncherResolution::Resolved(None), "{launcher}");
        }
    }

    #[test]
    fn launcher_claude_teams_propagates_sanitizer_none() {
        let got = AgentResumeArgv::new().launcher_resolution(
            &NoneSanitizer,
            Some("claudeTeams"),
            "SID",
            None,
            &v(&["cmux", "claude-teams", "--flag"]),
        );
        assert_eq!(got, LauncherResolution::Resolved(None));
    }

    #[test]
    fn launcher_unknown_and_nil_are_passthrough() {
        let unknown = AgentResumeArgv::new().launcher_resolution(
            &EchoSanitizer,
            Some("claude"),
            "SID",
            None,
            &v(&["claude"]),
        );
        assert_eq!(unknown, LauncherResolution::Passthrough);

        let nil = AgentResumeArgv::new().launcher_resolution(
            &EchoSanitizer,
            None,
            "SID",
            None,
            &v(&["claude"]),
        );
        assert_eq!(nil, LauncherResolution::Passthrough);
    }

    // ---- claude wrapper token + portable shell command ------------------------

    #[test]
    fn claude_wrapper_token_is_the_exact_posix_command_substitution() {
        assert_eq!(
            AgentResumeArgv::CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN,
            "\"$([ -x \"${CMUX_CLAUDE_WRAPPER_SHIM:-}\" ] && printf '%s' \"$CMUX_CLAUDE_WRAPPER_SHIM\" || printf claude)\""
        );
    }

    #[test]
    fn portable_claude_resume_shell_command_single_quotes_simple_command() {
        assert_eq!(
            AgentResumeArgv::portable_claude_resume_shell_command("claude --resume abc"),
            "/bin/sh -c 'claude --resume abc'"
        );
    }

    #[test]
    fn portable_claude_resume_shell_command_escapes_embedded_single_quotes() {
        // `echo 'hi'` => each ' becomes '\'' , then the whole thing is wrapped.
        assert_eq!(
            AgentResumeArgv::portable_claude_resume_shell_command("echo 'hi'"),
            "/bin/sh -c 'echo '\\''hi'\\'''"
        );
    }

    #[test]
    fn rendering_replaces_only_the_first_bare_claude() {
        // A sentinel quote so it is obvious which tokens were quoted vs replaced.
        let quote = |s: &str| format!("[{s}]");
        let rendered = AgentResumeArgv::rendering_claude_wrapper_executable(
            &v(&["env", "A=1", "claude", "--resume", "claude"]),
            quote,
        );
        assert_eq!(
            rendered,
            vec![
                "[env]".to_owned(),
                "[A=1]".to_owned(),
                AgentResumeArgv::CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN.to_owned(),
                "[--resume]".to_owned(),
                // The later `claude` argument is quoted, NOT replaced.
                "[claude]".to_owned(),
            ]
        );
    }

    #[test]
    fn rendered_portable_command_is_unwrapped_when_no_claude_token() {
        // `cmux claude-teams …` emits no bare `claude`, so it is joined as-is.
        let joined = AgentResumeArgv::rendered_portable_claude_resume_shell_command(
            &v(&["cmux", "claude-teams", "--resume", "SID"]),
            |s: &str| s.to_owned(),
        );
        assert_eq!(joined, "cmux claude-teams --resume SID");
    }

    #[test]
    fn rendered_portable_command_wraps_when_claude_token_present() {
        // Identity quote keeps the assertion readable; the branch under test is the
        // "token present => wrap in /bin/sh -c" decision.
        let got = AgentResumeArgv::rendered_portable_claude_resume_shell_command(
            &v(&["claude", "--resume", "SID"]),
            |s: &str| s.to_owned(),
        );
        let joined = format!(
            "{} --resume SID",
            AgentResumeArgv::CLAUDE_WRAPPER_SHELL_EXECUTABLE_TOKEN
        );
        assert_eq!(got, AgentResumeArgv::portable_claude_resume_shell_command(&joined));
        // And it really is the /bin/sh -c form.
        assert!(got.starts_with("/bin/sh -c '"));
        assert!(got.ends_with('\''));
    }
}
