//! cmux-agent-resume-argv — pure value logic for an agent's resume/continue argv.
//!
//! Faithful headless port of the canonical macOS Swift package
//! `Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch`:
//!
//! - `AgentResumeArgv.swift` → [`AgentResumeArgv`] (launcher resolution, per-kind
//!   resume argv, the claude-wrapper shell token, and the portable `/bin/sh -c`
//!   wrapping helpers).
//! - `AgentResumeWorkingDirectory.swift` → [`AgentResumeWorkingDirectory`].
//! - `AgentCwdNamespacing.swift` → [`AgentCwdNamespacing`].
//!
//! Like the Swift source this is pure value logic over primitives — no process
//! spawn, socket, or windowing — so it is testable in isolation and runs
//! headless.
//!
//! # Sanctioned divergences from the Swift source
//!
//! 1. **Sanitizer dependency is injected, not global.** Swift's [`AgentResumeArgv`]
//!    reaches the argument-preservation policy through the static
//!    `AgentLaunchSanitizer` enum. That logic lives in a sibling lane/crate
//!    (`cmux-agent-launch-sanitizer`) that this lane must not edit and which is not
//!    yet ported, so the three entrypoints it calls are modelled as the
//!    [`LaunchArgumentSanitizer`] trait, passed in by the caller. The real
//!    sanitizer implements this trait once it lands; the argv-assembly ordering,
//!    marker strings, and `nil`/`None` propagation are preserved exactly.
//! 2. **`posixSingleQuoted` re-ported in-crate.** The spec suggested reusing
//!    `cmux_ipc::shell_quote`, but that symbol is private to `cmux-ipc` (not
//!    exported) and its char-class passes "safe" tokens through *unquoted*, whereas
//!    Swift's `posixSingleQuoted` *always* single-quotes. The `/bin/sh -c` wrapper
//!    requires unconditional quoting, so the two-line function is re-ported verbatim
//!    as [`posix_single_quoted`] rather than weakened.
//! 3. **Shell target stays POSIX on Windows.** The resume command's shell target is
//!    always POSIX (the remote/`$SHELL -lic` wrapper), so `/bin/sh` and POSIX
//!    single-quoting are preserved even on Windows — no backslash-path rewriting.
//! 4. `Foundation`'s `.trimmingCharacters(in: .whitespacesAndNewlines)` maps to
//!    Rust's [`str::trim`] (the Unicode `White_Space` set is identical to Swift's
//!    `whitespacesAndNewlines`), and the two byte-identical private `normalized`
//!    helpers in the Swift sources are consolidated into one crate-internal
//!    [`normalized`] helper.

mod cwd_namespacing;
mod resume_argv;
mod working_directory;

pub use cwd_namespacing::AgentCwdNamespacing;
pub use resume_argv::{AgentResumeArgv, LaunchArgumentSanitizer, LauncherResolution};
pub use working_directory::AgentResumeWorkingDirectory;

/// Trims `value` (whitespace and newlines) and maps an absent or empty result to
/// `None`, mirroring the private `normalized(_:)` helpers in
/// `AgentResumeArgv.swift` and `AgentResumeWorkingDirectory.swift`.
///
/// Swift trims `.whitespacesAndNewlines`; [`str::trim`] trims the Unicode
/// `White_Space` set, which is the identical set of scalars.
pub(crate) fn normalized(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

#[cfg(test)]
mod normalized_tests {
    use super::normalized;

    #[test]
    fn trims_and_maps_empty_to_none() {
        assert_eq!(normalized(Some("  x  ")), Some("x".to_owned()));
        assert_eq!(normalized(Some("\n\t x \r\n")), Some("x".to_owned()));
        assert_eq!(normalized(Some("")), None);
        assert_eq!(normalized(Some("   \t\n ")), None);
        assert_eq!(normalized(None), None);
        // Interior whitespace is preserved; only the ends are trimmed.
        assert_eq!(normalized(Some("  a b  ")), Some("a b".to_owned()));
    }
}
