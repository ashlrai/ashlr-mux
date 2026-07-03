//! Port of `AgentResumeWorkingDirectory.swift`.

use crate::cwd_namespacing::AgentCwdNamespacing;
use crate::normalized;

/// Resolves the working directory a restored agent session should run in.
///
/// One policy shared by the app-side resolver and the CLI surface-resume-binding
/// publisher: directory-namespaced agents pin the launch cwd, id-keyed agents keep
/// the runtime cwd.
///
/// The type is a stateless value; construct one at the call site
/// (`AgentResumeWorkingDirectory::new()`) rather than reaching through a static
/// namespace.
///
/// ```
/// use cmux_agent_resume_argv::AgentResumeWorkingDirectory;
/// // A Claude session launched in /repo that drifted into /repo/worktrees/x:
/// let cwd = AgentResumeWorkingDirectory::new().resolve(
///     "claude",
///     Some("/repo/worktrees/x"),
///     Some("/repo"),
/// );
/// assert_eq!(cwd.as_deref(), Some("/repo")); // so `claude --resume` finds the transcript filed under /repo
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentResumeWorkingDirectory;

impl AgentResumeWorkingDirectory {
    /// Creates a working-directory resolver. The type holds no state.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Classifies an agent by its raw kind id (e.g. `"claude"`, `"codex"`,
    /// `"hermes-agent"`).
    ///
    /// Returns [`AgentCwdNamespacing::CwdInFile`] for id-keyed agents that record
    /// the cwd in the session file; [`AgentCwdNamespacing::ByDirectory`] for
    /// everything else (including unknown kinds, which prefer the launch cwd).
    #[must_use]
    pub fn cwd_namespacing(&self, kind: &str) -> AgentCwdNamespacing {
        match kind {
            "codex" | "opencode" | "amp" | "antigravity" | "rovodev" | "hermes-agent" => {
                AgentCwdNamespacing::CwdInFile
            }
            _ => AgentCwdNamespacing::ByDirectory,
        }
    }

    /// The directory a resumed agent session should `cd` into.
    ///
    /// Directory-namespaced agents prefer the launch working directory; id-keyed
    /// agents keep the runtime cwd. Inputs are trimmed and empty values are treated
    /// as absent. Returns `None` when neither input is usable.
    #[must_use]
    pub fn resolve(
        &self,
        kind: &str,
        runtime_cwd: Option<&str>,
        launch_working_directory: Option<&str>,
    ) -> Option<String> {
        let runtime = normalized(runtime_cwd);
        let launch = normalized(launch_working_directory);
        match self.cwd_namespacing(kind) {
            AgentCwdNamespacing::CwdInFile => runtime.or(launch),
            AgentCwdNamespacing::ByDirectory => launch.or(runtime),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwd_namespacing_classifies_id_keyed_kinds_as_cwd_in_file() {
        for kind in ["codex", "opencode", "amp", "antigravity", "rovodev", "hermes-agent"] {
            assert_eq!(
                AgentResumeWorkingDirectory::new().cwd_namespacing(kind),
                AgentCwdNamespacing::CwdInFile,
                "{kind} should be cwd-in-file"
            );
        }
    }

    #[test]
    fn cwd_namespacing_classifies_directory_keyed_and_unknown_kinds_as_by_directory() {
        for kind in ["claude", "grok", "pi", "gemini", "cursor", "qoder", "copilot", "totally-unknown"]
        {
            assert_eq!(
                AgentResumeWorkingDirectory::new().cwd_namespacing(kind),
                AgentCwdNamespacing::ByDirectory,
                "{kind} should be by-directory"
            );
        }
    }

    #[test]
    fn resolve_by_directory_prefers_launch_over_runtime() {
        // Canonical doc-comment example: a Claude session launched in /repo that
        // drifted into /repo/worktrees/x resumes in /repo.
        assert_eq!(
            AgentResumeWorkingDirectory::new().resolve(
                "claude",
                Some("/repo/worktrees/x"),
                Some("/repo"),
            ),
            Some("/repo".to_owned())
        );
    }

    #[test]
    fn resolve_by_directory_falls_back_to_runtime_when_launch_absent() {
        assert_eq!(
            AgentResumeWorkingDirectory::new().resolve("claude", Some("/repo/worktrees/x"), None),
            Some("/repo/worktrees/x".to_owned())
        );
        // Empty/whitespace launch is treated as absent.
        assert_eq!(
            AgentResumeWorkingDirectory::new().resolve(
                "claude",
                Some("/repo/worktrees/x"),
                Some("   "),
            ),
            Some("/repo/worktrees/x".to_owned())
        );
    }

    #[test]
    fn resolve_cwd_in_file_prefers_runtime_over_launch() {
        assert_eq!(
            AgentResumeWorkingDirectory::new().resolve("codex", Some("/runtime"), Some("/launch")),
            Some("/runtime".to_owned())
        );
    }

    #[test]
    fn resolve_cwd_in_file_falls_back_to_launch_when_runtime_absent() {
        assert_eq!(
            AgentResumeWorkingDirectory::new().resolve("codex", None, Some("/launch")),
            Some("/launch".to_owned())
        );
        assert_eq!(
            AgentResumeWorkingDirectory::new().resolve("codex", Some(" \n "), Some("/launch")),
            Some("/launch".to_owned())
        );
    }

    #[test]
    fn resolve_returns_none_when_both_inputs_absent() {
        assert_eq!(AgentResumeWorkingDirectory::new().resolve("claude", None, None), None);
        assert_eq!(AgentResumeWorkingDirectory::new().resolve("codex", Some("  "), Some("")), None);
    }

    #[test]
    fn resolve_trims_the_returned_directory() {
        assert_eq!(
            AgentResumeWorkingDirectory::new().resolve("claude", None, Some("  /repo  ")),
            Some("/repo".to_owned())
        );
    }
}
