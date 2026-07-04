//! Trust classification for captured agent launch commands.
//!
//! Port of `Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/AgentLaunchCaptureTrust.swift`
//! (lines 12-158, the `AgentLaunchCaptureTrust` enum namespace).
//!
//! `CMUX_AGENT_LAUNCH_*` is exported into the agent's process environment by the
//! cmux launch wrappers and therefore leaks to every descendant process: an agent
//! started from inside another agent's session (codex under claude, claude under
//! codex, …) inherits the ANCESTOR's launch capture. A hook that stores that
//! capture verbatim poisons resume/fork for the session — the rendered command
//! runs the wrong binary with the wrong flags. These pure predicates decide
//! whether a capture can be trusted for the agent kind it is stored under.
//!
//! Parity notes:
//! - Swift trims `.whitespacesAndNewlines`; Rust [`str::trim`] trims the Unicode
//!   `White_Space` set, which is the identical set of scalars (crate-wide
//!   convention pinned in `cmux-agent-resume-argv`).
//! - Swift `lowercased()` and Rust [`str::to_lowercase`] both perform full
//!   Unicode lowercasing; inputs are ASCII agent slugs in practice.
//! - Basenames split on `'/'` ONLY (`NSString.lastPathComponent` /
//!   `URL(fileURLWithPath:).lastPathComponent` semantics), never `'\'`, and no
//!   extension stripping — kept VERBATIM for byte parity with the canonical
//!   macOS behavior. KNOWN Windows-adaptation decision point (deliberately not
//!   taken): a captured Windows argv0 like `C:\...\codex.exe` yields the whole
//!   string as its basename and never matches; if real Windows captures need to
//!   match, a sanctioned divergence (split both separators + PATHEXT-style
//!   suffix stripping, mirroring the resolver module) must be introduced
//!   explicitly at that time.

use std::collections::BTreeSet;

/// Wrapper launchers that legitimately differ from the hook kind they launch.
///
/// Mirrors `AgentLaunchCaptureTrust.wrapperLaunchersByKind`
/// (AgentLaunchCaptureTrust.swift:14-19), verbatim.
const WRAPPER_LAUNCHERS_BY_KIND: &[(&str, &[&str])] = &[
    ("claude", &["claudeteams"]),
    ("codex", &["codexteams"]),
    ("opencode", &["omo", "omx", "omc"]),
    ("pi", &["omp"]),
];

/// Mirrors `AgentLaunchCaptureTrust.nativeProcessAliasesByKind`
/// (AgentLaunchCaptureTrust.swift:21-37), verbatim.
const NATIVE_PROCESS_ALIASES_BY_KIND: &[(&str, &[&str])] = &[
    ("antigravity", &["agy"]),
    ("claude", &["claude"]),
    ("codex", &["codex"]),
    ("codebuddy", &["codebuddy"]),
    ("copilot", &["copilot"]),
    ("cursor", &["cursor-agent", "cursor"]),
    ("factory", &["droid", "factory"]),
    ("gemini", &["gemini"]),
    ("grok", &["grok", "grok-macos-aarch64", "grok-macos-aarch"]),
    ("kiro", &["kiro", "kiro-cli"]),
    ("omp", &["omp"]),
    ("opencode", &["opencode", "omo", "omx", "omc"]),
    ("pi", &["pi", "omp"]),
    ("qoder", &["qodercli", "qoder"]),
    ("rovodev", &["rovodev", "rovo", "rovo-dev"]),
];

fn lookup(table: &'static [(&str, &[&str])], key: &str) -> &'static [&'static str] {
    table
        .iter()
        .find(|(entry_key, _)| *entry_key == key)
        .map(|(_, values)| *values)
        .unwrap_or(&[])
}

/// True when `launcher` plausibly describes a launch of agent `kind`.
/// A `None`/empty launcher is trusted: hooks fall back to their own kind.
///
/// Mirrors `AgentLaunchCaptureTrust.launcherDescribesKind`
/// (AgentLaunchCaptureTrust.swift:41-52).
pub fn launcher_describes_kind(launcher: Option<&str>, kind: &str) -> bool {
    let Some(launcher) = launcher.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let normalized_launcher = launcher.to_lowercase();
    let normalized_kind = kind.trim().to_lowercase();
    if normalized_launcher == normalized_kind {
        return true;
    }
    lookup(WRAPPER_LAUNCHERS_BY_KIND, &normalized_kind).contains(&normalized_launcher.as_str())
}

/// True when a captured argv describes a shell dispatcher (`sh -c …`,
/// `zsh -lc …`) rather than an agent launch. This happens when the
/// launch-capture PID fallback resolves to the hook's own dispatch shell
/// instead of the agent process. Requires both a shell argv\[0\] basename and
/// a command-string flag, so an agent that merely shares a shell's name
/// (e.g. a wrapper script named `fish`) is not misclassified.
///
/// Mirrors `AgentLaunchCaptureTrust.argvLooksLikeShellWrapper`
/// (AgentLaunchCaptureTrust.swift:60-77).
pub fn argv_looks_like_shell_wrapper(arguments: &[String]) -> bool {
    let Some(argv0) = arguments
        .first()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    let name = ns_last_path_component(argv0).to_lowercase();
    const SHELLS: [&str; 8] = ["sh", "bash", "zsh", "dash", "fish", "csh", "tcsh", "ksh"];
    if !SHELLS.contains(&name.as_str()) {
        return false;
    }
    if arguments.len() < 2 {
        return false;
    }
    // A combined short option whose letters are shell mode flags and that
    // includes `c` (-c, -lc, -ic, -lic, …): the command-string form. The flag
    // is deliberately UNtrimmed, matching Swift.
    let flag = &arguments[1];
    if !flag.starts_with('-') || flag.starts_with("--") {
        return false;
    }
    let letters = &flag[1..];
    !letters.is_empty()
        && letters.contains('c')
        && letters.chars().all(|letter| "cilms".contains(letter))
}

/// True when PID-derived process metadata describes the same native agent as
/// the hook kind. This keeps unrelated parents, including Xcode test hosts
/// and the cmux app executable, from becoming persisted resume commands.
///
/// Mirrors `AgentLaunchCaptureTrust.nativeProcessDescribesKind`
/// (AgentLaunchCaptureTrust.swift:82-96).
pub fn native_process_describes_kind(
    process_name: Option<&str>,
    arguments: Option<&[String]>,
    kind: &str,
) -> bool {
    let Some(expected_kind) = normalized_agent_name(Some(kind)) else {
        return false;
    };
    let Some(arguments) = arguments else {
        return false;
    };
    let aliases = lookup(NATIVE_PROCESS_ALIASES_BY_KIND, &expected_kind);
    let cli_name = format!("{expected_kind}-cli");
    native_process_descriptors(process_name, arguments)
        .iter()
        .any(|descriptor| {
            *descriptor == expected_kind
                || aliases.contains(&descriptor.as_str())
                || *descriptor == cli_name
        })
}

/// True when the PID-derived metadata describes ANY known native agent
/// (map keys ∪ alias values).
///
/// Mirrors `AgentLaunchCaptureTrust.nativeProcessDescribesKnownAgent`
/// (AgentLaunchCaptureTrust.swift:98-106).
pub fn native_process_describes_known_agent(
    process_name: Option<&str>,
    arguments: &[String],
) -> bool {
    native_process_descriptors(process_name, arguments)
        .iter()
        .any(|descriptor| {
            NATIVE_PROCESS_ALIASES_BY_KIND
                .iter()
                .any(|(kind, aliases)| *kind == descriptor || aliases.contains(&descriptor.as_str()))
        })
}

/// Mirrors `AgentLaunchCaptureTrust.nativeProcessDescriptors`
/// (AgentLaunchCaptureTrust.swift:108-141), including the node/bun EARLY
/// RETURN: when the runtime is node or bun, only the claude-script scan runs
/// and the trailing codex/claude executable-substring checks are skipped.
fn native_process_descriptors(
    process_name: Option<&str>,
    arguments: &[String],
) -> BTreeSet<String> {
    let mut descriptors = BTreeSet::new();
    let name_base = process_basename(process_name);
    let executable_base = process_basename(arguments.first().map(String::as_str));
    if let Some(name_base) = &name_base {
        descriptors.insert(name_base.clone());
    }
    if let Some(executable_base) = &executable_base {
        descriptors.insert(executable_base.clone());
    }
    let is_script_runtime =
        |base: &Option<String>| matches!(base.as_deref(), Some("node") | Some("bun"));
    if is_script_runtime(&name_base) || is_script_runtime(&executable_base) {
        if arguments.iter().skip(1).any(|argument| {
            let lowered = argument.to_lowercase();
            process_basename(Some(argument)).as_deref() == Some("claude")
                || lowered.contains("/.claude/")
                || lowered.contains("/claude/versions/")
        }) {
            descriptors.insert("claude".to_string());
        }
        return descriptors;
    }

    let executable = arguments
        .first()
        .map(|value| value.to_lowercase())
        .unwrap_or_default();
    if name_base.as_deref() == Some("codex")
        || executable_base.as_deref() == Some("codex")
        || executable.contains("/codex/codex")
    {
        descriptors.insert("codex".to_string());
    }
    if name_base.as_deref() == Some("claude")
        || executable_base.as_deref() == Some("claude")
        || executable.contains("/claude/versions/")
    {
        descriptors.insert("claude".to_string());
    }
    descriptors
}

/// Mirrors `AgentLaunchCaptureTrust.normalizedAgentName`
/// (AgentLaunchCaptureTrust.swift:143-149).
fn normalized_agent_name(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_lowercase())
}

/// Mirrors `AgentLaunchCaptureTrust.processBasename`
/// (AgentLaunchCaptureTrust.swift:151-157): trim, empty ⇒ `None`, then
/// `URL(fileURLWithPath:).lastPathComponent` lowercased.
fn process_basename(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(ns_last_path_component(trimmed).to_lowercase())
}

/// `NSString.lastPathComponent` / `URL(fileURLWithPath:).lastPathComponent`
/// semantics: split on `'/'` ONLY, ignoring trailing slashes (`"foo/"` ⇒
/// `"foo"`), with an all-slash path collapsing to `"/"`. Never splits `'\'`
/// and never strips extensions (see the module-level Windows parity note).
fn ns_last_path_component(path: &str) -> &str {
    let without_trailing = path.trim_end_matches('/');
    if without_trailing.is_empty() {
        // "" stays "" (callers guard empties); "/", "//", … collapse to "/".
        return if path.is_empty() { "" } else { "/" };
    }
    without_trailing
        .rsplit('/')
        .next()
        .unwrap_or(without_trailing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    /// Oracles from `AgentLaunchCaptureTrustTests.swift:5-30` plus added
    /// whitespace/Unicode edges.
    #[test]
    fn launcher_describes_kind_oracles() {
        let cases: &[(Option<&str>, &str, bool)] = &[
            // testExactKindMatchIsTrusted
            (Some("codex"), "codex", true),
            (Some("Claude"), "claude", true),
            (Some("pi"), "pi", true),
            // testAbsentLauncherIsTrusted
            (None, "codex", true),
            (Some("  "), "codex", true),
            // testWrapperLaunchersDescribeTheirKind
            (Some("claudeTeams"), "claude", true),
            (Some("codexTeams"), "codex", true),
            (Some("omo"), "opencode", true),
            (Some("omx"), "opencode", true),
            (Some("omc"), "opencode", true),
            (Some("omp"), "pi", true),
            // testCrossAgentLauncherIsDistrusted
            (Some("claude"), "codex", false),
            (Some("codex"), "claude", false),
            (Some("claudeTeams"), "codex", false),
            (Some("omo"), "codex", false),
            // Added edges: Unicode whitespace (NBSP U+00A0, ideographic space
            // U+3000, line separator U+2028) is in Swift's
            // .whitespacesAndNewlines and Rust's White_Space alike.
            (Some("\u{00A0}codex\u{3000}"), "codex", true),
            (Some("\u{2028}\u{00A0} \t"), "codex", true), // trims to empty ⇒ trusted
            (Some("claudeteams"), " Claude\n", true),     // kind is trimmed + lowercased too
            (Some("omp"), "opencode", false),             // wrapper of a different kind
        ];
        for (launcher, kind, expected) in cases {
            assert_eq!(
                launcher_describes_kind(*launcher, kind),
                *expected,
                "launcher_describes_kind({launcher:?}, {kind:?})"
            );
        }
    }

    /// Oracles from `AgentLaunchCaptureTrustTests.swift:32-43` plus flag and
    /// basename edges.
    #[test]
    fn argv_looks_like_shell_wrapper_oracles() {
        let cases: &[(&[&str], bool)] = &[
            (&["sh", "-c", "eval x"], true),
            (&["/bin/zsh", "-lc", "codex"], true),
            (&["/bin/zsh", "-lic", "codex"], true),
            (&["/usr/local/bin/codex", "--yolo"], false),
            (&[], false),
            // An agent that merely shares a shell's basename must stay trusted.
            (&["/Users/alice/.local/bin/fish", "--resume", "x"], false),
            (&["sh"], false),
            // `--chrome` is a long option, not a shell command-string flag.
            (&["zsh", "--chrome"], false),
            // Added edges:
            (&["sh", "-c"], true),           // argc==2, letters == "c"
            (&["sh", "-"], false),           // letters empty after dropFirst
            (&["/bin/zsh/", "-lc"], true),   // trailing-slash argv0 still basenames to "zsh"
            (&["/", "-c"], false),           // basename of "/" is "/", not a shell
            (&["zsh", "-x"], false),         // 'x' outside the "cilms" letter set
            (&["zsh", "-il"], false),        // mode letters but no 'c'
            (&["zsh", " -c"], false),        // flag is UNtrimmed in Swift
            (&["  BASH  ", "-c", "x"], true), // argv0 trimmed + basename lowercased
        ];
        for (arguments, expected) in cases {
            assert_eq!(
                argv_looks_like_shell_wrapper(&argv(arguments)),
                *expected,
                "argv_looks_like_shell_wrapper({arguments:?})"
            );
        }
    }

    /// Oracles from `AgentLaunchCaptureTrustTests.swift:45-111`
    /// (`testPIDProcessMetadataMustMatchHookKind`) plus descriptor edges.
    #[test]
    fn native_process_describes_kind_oracles() {
        #[allow(clippy::type_complexity)]
        let cases: &[(Option<&str>, Option<&[&str]>, &str, bool)] = &[
            (
                Some("codex"),
                Some(&["/opt/homebrew/bin/codex", "--sandbox", "workspace-write"]),
                "codex",
                true,
            ),
            (
                Some("node"),
                Some(&["node", "/Users/alice/.claude/local/claude.js"]),
                "claude",
                true,
            ),
            (
                Some("grok-macos-aarch64"),
                Some(&["/Users/alice/.local/bin/grok-macos-aarch64", "-r", "session"]),
                "grok",
                true,
            ),
            (
                Some("kiro-cli"),
                Some(&["/Users/alice/.cargo/bin/kiro-cli", "chat"]),
                "kiro",
                true,
            ),
            // Self-name match with no alias-map entry.
            (
                Some("acme-agent"),
                Some(&["/Users/alice/bin/acme-agent", "--session", "native-session"]),
                "acme-agent",
                true,
            ),
            (
                Some("cmux DEV"),
                Some(&[
                    "/tmp/cmux-tests/Build/Products/Debug/cmux DEV.app/Contents/MacOS/cmux DEV",
                    "-NSTreatUnknownArgumentsAsOpen",
                ]),
                "codex",
                false,
            ),
            (Some("codex"), Some(&["/opt/homebrew/bin/codex"]), "claude", false),
            (Some("agy"), Some(&["/usr/local/bin/agy"]), "antigravity", true),
            // Added edges:
            (Some("codex"), None, "codex", false), // absent argv ⇒ never trusted
            (Some("codex"), Some(&["/opt/homebrew/bin/codex"]), "  ", false), // blank kind
            // The "<kind>-cli" suffix rule applies to kinds without a map entry.
            (Some("foo-cli"), Some(&["/usr/bin/foo-cli"]), "foo", true),
            // node/bun EARLY RETURN skips the codex substring checks entirely.
            (Some("node"), Some(&["node", "/opt/codex/codex"]), "codex", false),
            (
                Some("bun"),
                Some(&["bun", "/Users/a/.local/share/claude/versions/1.2/cli.js"]),
                "claude",
                true,
            ),
            // Executable-path substring rules (non-runtime argv0).
            (None, Some(&["/opt/Codex/codex-x86_64"]), "codex", true), // "/codex/codex" in lowered path
            (None, Some(&["/opt/claude/versions/2.0/bin/x"]), "claude", true),
            // Kind is trimmed + lowercased before alias lookup.
            (Some("agy"), Some(&["/usr/local/bin/agy"]), " Antigravity\n", true),
        ];
        for (process_name, arguments, kind, expected) in cases {
            let arguments = arguments.map(argv);
            assert_eq!(
                native_process_describes_kind(*process_name, arguments.as_deref(), kind),
                *expected,
                "native_process_describes_kind({process_name:?}, {arguments:?}, {kind:?})"
            );
        }
    }

    /// Oracle from `AgentLaunchCaptureTrustTests.swift:53-58` plus negatives.
    #[test]
    fn native_process_describes_known_agent_oracles() {
        let cases: &[(Option<&str>, &[&str], bool)] = &[
            (
                Some("codex"),
                &["/opt/homebrew/bin/codex", "--sandbox", "workspace-write"],
                true,
            ),
            // Alias values count as known names (droid ⇒ factory).
            (Some("droid"), &["/usr/local/bin/droid"], true),
            // Unmapped self-named agents are NOT "known".
            (Some("acme-agent"), &["/Users/alice/bin/acme-agent"], false),
            (Some("cmux DEV"), &["/Applications/cmux DEV.app/Contents/MacOS/cmux DEV"], false),
            (None, &[], false),
            // node runtime with a claude script yields the synthesized "claude" descriptor.
            (Some("node"), &["node", "/Users/alice/.claude/local/claude.js"], true),
        ];
        for (process_name, arguments, expected) in cases {
            assert_eq!(
                native_process_describes_known_agent(*process_name, &argv(arguments)),
                *expected,
                "native_process_describes_known_agent({process_name:?}, {arguments:?})"
            );
        }
    }

    /// Pins `NSString.lastPathComponent` semantics: '/'-only splitting,
    /// trailing-slash stripping, all-slash ⇒ "/", and NO backslash splitting
    /// (verbatim macOS behavior — see module doc for the Windows decision).
    #[test]
    fn ns_last_path_component_semantics() {
        let cases: &[(&str, &str)] = &[
            ("/usr/local/bin/codex", "codex"),
            ("foo", "foo"),
            ("foo/", "foo"),
            ("foo//", "foo"),
            ("/", "/"),
            ("//", "/"),
            ("/foo/bar", "bar"),
            ("C:\\Users\\x\\codex.exe", "c:\\users\\x\\codex.exe"), // no '\' split, lowercased by process_basename
        ];
        for (input, expected) in &cases[..cases.len() - 1] {
            assert_eq!(ns_last_path_component(input), *expected, "{input:?}");
        }
        // Backslash paths pass through whole (then lowercase in process_basename).
        assert_eq!(
            process_basename(Some("C:\\Users\\x\\codex.exe")).as_deref(),
            Some("c:\\users\\x\\codex.exe")
        );
    }
}
