//! Canonical catalog of agents selectable for workspace/tab auto-naming, and
//! the pure decision for which agent summarizes a naming pass.
//!
//! Port of `Packages/macOS/CmuxSettings/Sources/CmuxSettings/Values/AutoNamingAgentCatalog.swift`.
//! The Swift original lives in `CmuxSettings` (shared by the Settings picker and
//! the bundled `cmux` CLI) so the picker and the summarizer dispatch share one
//! source of truth for which agents exist and which can actually summarize. It
//! is entirely pure: [`resolve_summarizer`] takes an injected `is_installed`
//! probe (Swift `isInstalled:(String)->Bool`) so no process probing lives in the
//! core — the catalog is a static data table.
//!
//! Swift parity source: `AutoNamingAgentCatalog.swift:1-135`.

/// One selectable option for the workspace/tab auto-naming agent.
///
/// The naming agent is stored as an open string (the agent `slug`, or
/// [`AUTO_SLUG`]) so it stays fully customizable — a power user can name a
/// custom agent in `~/.config/cmux/cmux.json` even if it is not listed here.
///
/// Swift parity: `AutoNamingAgentCatalog.swift:12-28` (`AutoNamingAgentOption`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AutoNamingAgentOption {
    /// CLI agent name; must match the hook `AgentHookDef.name` / the `<agent>`
    /// segment of `cmux hooks <agent> auto-name`.
    pub slug: &'static str,
    /// Brand display name shown in the picker (proper noun — not localized).
    pub display_name: &'static str,
    /// Whether cmux currently knows how to drive this agent as a summarizer.
    /// Unsupported agents are still selectable (the user asked for any agent),
    /// but naming falls back to each session's own agent.
    pub summarizer_supported: bool,
}

impl AutoNamingAgentOption {
    const fn new(slug: &'static str, display_name: &'static str, summarizer_supported: bool) -> Self {
        Self {
            slug,
            display_name,
            summarizer_supported,
        }
    }
}

/// Sentinel meaning "name each session with its own agent" — the default,
/// identical to the original auto-naming behavior.
///
/// Swift parity: `AutoNamingAgentCatalog.swift:39` (`autoSlug`).
pub const AUTO_SLUG: &str = "auto";

/// Agents whose binary cmux knows how to invoke for a one-shot summarization.
/// Keep in sync with the CLI summarizer dispatch.
///
/// Swift parity: `AutoNamingAgentCatalog.swift:43-45` (`supportedSlugs`).
pub const SUPPORTED_SLUGS: [&str; 6] = ["claude", "codex", "grok", "opencode", "pi", "omp"];

/// All agents offered in the picker, in display order. Supported agents first,
/// then the remainder (selectable but fall back to the session's own agent
/// until cmux learns to drive them).
///
/// Swift parity: `AutoNamingAgentCatalog.swift:50-68` (`agents`).
pub const AGENTS: [AutoNamingAgentOption; 17] = [
    AutoNamingAgentOption::new("claude", "Claude Code", true),
    AutoNamingAgentOption::new("codex", "Codex", true),
    AutoNamingAgentOption::new("grok", "Grok", true),
    AutoNamingAgentOption::new("opencode", "OpenCode", true),
    AutoNamingAgentOption::new("pi", "Pi", true),
    AutoNamingAgentOption::new("omp", "OMP", true),
    AutoNamingAgentOption::new("amp", "Amp", false),
    AutoNamingAgentOption::new("cursor", "Cursor", false),
    AutoNamingAgentOption::new("gemini", "Gemini", false),
    AutoNamingAgentOption::new("kiro", "Kiro", false),
    AutoNamingAgentOption::new("antigravity", "Antigravity", false),
    AutoNamingAgentOption::new("rovodev", "Rovo Dev", false),
    AutoNamingAgentOption::new("hermes-agent", "Hermes Agent", false),
    AutoNamingAgentOption::new("copilot", "Copilot", false),
    AutoNamingAgentOption::new("codebuddy", "CodeBuddy", false),
    AutoNamingAgentOption::new("factory", "Factory", false),
    AutoNamingAgentOption::new("qoder", "Qoder", false),
];

/// Supported agents, in display order (picker "primary" group).
///
/// Swift parity: `AutoNamingAgentCatalog.swift:71-73` (`supportedAgents`).
pub fn supported_agents() -> Vec<AutoNamingAgentOption> {
    AGENTS
        .iter()
        .copied()
        .filter(|option| option.summarizer_supported)
        .collect()
}

/// Selectable-but-not-yet-driveable agents (picker "other" group).
///
/// Swift parity: `AutoNamingAgentCatalog.swift:76-78` (`otherAgents`).
pub fn other_agents() -> Vec<AutoNamingAgentOption> {
    AGENTS
        .iter()
        .copied()
        .filter(|option| !option.summarizer_supported)
        .collect()
}

/// The option whose `slug` equals `slug`, if any.
///
/// Swift parity: `AutoNamingAgentCatalog.swift:80-82` (`option(forSlug:)`).
pub fn option_for_slug(slug: &str) -> Option<AutoNamingAgentOption> {
    AGENTS.iter().copied().find(|option| option.slug == slug)
}

/// True only when cmux can drive `slug` as a summarizer. Custom/unknown slugs
/// return false (they fall back to the session's own agent).
///
/// Swift parity: `AutoNamingAgentCatalog.swift:86-88` (`summarizerSupported(slug:)`).
pub fn summarizer_supported(slug: &str) -> bool {
    SUPPORTED_SLUGS.contains(&slug)
}

/// Display name for any slug, falling back to the raw slug for custom agents not
/// in the catalog.
///
/// Swift parity: `AutoNamingAgentCatalog.swift:92-94` (`displayName(forSlug:)`).
pub fn display_name_for_slug(slug: &str) -> String {
    option_for_slug(slug)
        .map(|option| option.display_name.to_string())
        .unwrap_or_else(|| slug.to_string())
}

/// Outcome of resolving which agent should summarize a naming pass.
///
/// Swift parity: `AutoNamingAgentCatalog.swift:97-109` (`SummarizerDecision`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummarizerDecision {
    /// The agent that should actually run the summarization.
    pub agent: String,
    /// `Some` when a supported override was chosen but its binary is missing, so
    /// we fell back to the session's own agent. Carries the chosen agent so the
    /// app can surface a "not installed" note.
    pub missing_override: Option<String>,
}

impl SummarizerDecision {
    fn new(agent: impl Into<String>, missing_override: Option<String>) -> Self {
        Self {
            agent: agent.into(),
            missing_override,
        }
    }
}

/// Pure decision for which agent summarizes a pass, given the user's override
/// `chosen`, the session's own `session_agent`, and a binary-availability probe.
/// Kept here (and dependency-injected) so it is unit testable without the CLI:
/// `auto` / empty / the session itself / an unsupported or uninstalled override
/// all collapse to `session_agent`, so naming never breaks; a supported-but-
/// missing override is reported via [`SummarizerDecision::missing_override`].
///
/// Precedence matches Swift exactly: trim, then the empty/`auto`/self
/// short-circuit runs *before* the supported gate, then the supported gate, then
/// the installed gate.
///
/// Swift parity: `AutoNamingAgentCatalog.swift:118-134` (`resolveSummarizer`).
pub fn resolve_summarizer(
    chosen: Option<&str>,
    session_agent: &str,
    is_installed: impl Fn(&str) -> bool,
) -> SummarizerDecision {
    // Swift: `chosen?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""`.
    // `str::trim` strips Unicode whitespace (incl. spaces, tabs, newlines),
    // matching `CharacterSet.whitespacesAndNewlines` for these inputs.
    let chosen = chosen.unwrap_or("").trim();
    if chosen.is_empty() || chosen == AUTO_SLUG || chosen == session_agent {
        return SummarizerDecision::new(session_agent, None);
    }
    if !summarizer_supported(chosen) {
        return SummarizerDecision::new(session_agent, None);
    }
    if !is_installed(chosen) {
        return SummarizerDecision::new(session_agent, Some(chosen.to_string()));
    }
    SummarizerDecision::new(chosen, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // Swift parity: `autoSlugAndSupportedMembership`
    // (`AutoNamingAgentCatalogTests.swift:7-14`).
    #[test]
    fn auto_slug_and_supported_membership() {
        assert_eq!(AUTO_SLUG, "auto");
        for slug in ["claude", "codex", "grok", "opencode", "pi", "omp"] {
            assert!(summarizer_supported(slug));
        }
        assert!(!summarizer_supported("gemini"));
        assert!(!summarizer_supported("totally-unknown"));
    }

    // Swift parity: `partitionsSupportedAndOther`
    // (`AutoNamingAgentCatalogTests.swift:16-26`).
    #[test]
    fn partitions_supported_and_other() {
        let supported: HashSet<&str> = supported_agents().iter().map(|o| o.slug).collect();
        let other: HashSet<&str> = other_agents().iter().map(|o| o.slug).collect();
        assert!(supported.is_disjoint(&other));
        assert!(supported.contains("claude"));
        assert!(other.contains("gemini"));
        // Catalog flag and membership helper must agree for every option.
        for option in AGENTS {
            assert_eq!(option.summarizer_supported, summarizer_supported(option.slug));
        }
    }

    // Every supported/other agent together is the full catalog with no overlap.
    #[test]
    fn supported_plus_other_covers_catalog() {
        assert_eq!(supported_agents().len(), 6);
        assert_eq!(other_agents().len(), 11);
        assert_eq!(supported_agents().len() + other_agents().len(), AGENTS.len());
    }

    // Swift parity: `displayNameFallsBackToSlugForCustomAgents`
    // (`AutoNamingAgentCatalogTests.swift:28-31`).
    #[test]
    fn display_name_falls_back_to_slug_for_custom_agents() {
        assert_eq!(display_name_for_slug("claude"), "Claude Code");
        assert_eq!(display_name_for_slug("my-custom-agent"), "my-custom-agent");
    }

    #[test]
    fn option_for_slug_finds_and_misses() {
        assert_eq!(option_for_slug("codex").map(|o| o.display_name), Some("Codex"));
        assert_eq!(option_for_slug("nope"), None);
    }

    // MARK: resolveSummarizer decision matrix

    // Swift parity: `autoEmptyOrSelfResolvesToSessionAgent`
    // (`AutoNamingAgentCatalogTests.swift:35-46`).
    #[test]
    fn auto_empty_or_self_resolves_to_session_agent() {
        let installed = |_: &str| true;
        assert_eq!(
            resolve_summarizer(None, "claude", installed),
            SummarizerDecision::new("claude", None)
        );
        assert_eq!(
            resolve_summarizer(Some("auto"), "claude", installed),
            SummarizerDecision::new("claude", None)
        );
        assert_eq!(
            resolve_summarizer(Some("   "), "grok", installed),
            SummarizerDecision::new("grok", None)
        );
        // Choosing the session's own agent is a no-op override.
        assert_eq!(
            resolve_summarizer(Some("codex"), "codex", installed),
            SummarizerDecision::new("codex", None)
        );
    }

    // Swift parity: `supportedOverrideUsesChosenWhenInstalled`
    // (`AutoNamingAgentCatalogTests.swift:48-52`).
    #[test]
    fn supported_override_uses_chosen_when_installed() {
        let decision = resolve_summarizer(Some("codex"), "claude", |s| s == "codex");
        assert_eq!(decision, SummarizerDecision::new("codex", None));
    }

    // Swift parity: `supportedOverrideFallsBackAndReportsWhenMissing`
    // (`AutoNamingAgentCatalogTests.swift:54-58`).
    #[test]
    fn supported_override_falls_back_and_reports_when_missing() {
        let decision = resolve_summarizer(Some("codex"), "claude", |_| false);
        assert_eq!(
            decision,
            SummarizerDecision::new("claude", Some("codex".to_string()))
        );
    }

    // Swift parity: `unsupportedOverrideFallsBackSilently`
    // (`AutoNamingAgentCatalogTests.swift:60-65`).
    #[test]
    fn unsupported_override_falls_back_silently() {
        // gemini is selectable but not driveable yet: fall back, no report.
        let decision = resolve_summarizer(Some("gemini"), "claude", |_| true);
        assert_eq!(decision, SummarizerDecision::new("claude", None));
    }

    // Parity-risk edge: surrounding whitespace/newlines are trimmed before every
    // gate (Swift `.whitespacesAndNewlines`). A padded supported override still
    // resolves to the trimmed agent.
    #[test]
    fn trims_whitespace_and_newlines_before_gating() {
        // Padded empty -> session agent.
        assert_eq!(
            resolve_summarizer(Some("\n\t "), "claude", |_| true),
            SummarizerDecision::new("claude", None)
        );
        // Padded "auto" short-circuits to session agent.
        assert_eq!(
            resolve_summarizer(Some("  auto\n"), "claude", |_| true),
            SummarizerDecision::new("claude", None)
        );
        // Padded supported override resolves to the trimmed slug.
        assert_eq!(
            resolve_summarizer(Some(" codex \n"), "claude", |s| s == "codex"),
            SummarizerDecision::new("codex", None)
        );
        // Padded self-selection is still a no-op override.
        assert_eq!(
            resolve_summarizer(Some(" claude "), "claude", |_| true),
            SummarizerDecision::new("claude", None)
        );
    }

    // Parity-risk edge: the empty/auto/self short-circuit runs BEFORE the
    // installed gate, so an uninstalled but supported override that equals the
    // session agent never reports a missing override.
    #[test]
    fn self_override_short_circuits_before_installed_gate() {
        let decision = resolve_summarizer(Some("codex"), "codex", |_| false);
        assert_eq!(decision, SummarizerDecision::new("codex", None));
    }
}
