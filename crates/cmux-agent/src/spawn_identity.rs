//! Canonical workspace/surface identity resolution for spawned agents.
//!
//! Port of `Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/AgentSpawnIdentity.swift`
//! (lines 22-79, the stateless `AgentSpawnIdentity` struct; the resolve
//! algorithm is lines 46-71).
//!
//! cmux's CLI agent launchers run inside a terminal surface and inherit that
//! surface's own `CMUX_SURFACE_ID` / `CMUX_WORKSPACE_ID`. They also query the
//! daemon for the operator's currently *focused* pane (for the tmux-compat
//! shim). The agent's canonical identity must come from the launch surface,
//! **not** the focused pane: stamping the focused pane desyncs
//! `CMUX_SURFACE_ID` from the inherited `CMUX_PANEL_ID`, so an agent launched
//! in surface B while surface A is focused records surface A and later
//! restores into the wrong surface
//! (<https://github.com/manaflow-ai/cmux/issues/4920>, the codex "jumble after
//! reload" symptom).
//!
//! Fully pure, zero I/O — no Windows adaptation needed.

/// The canonical `(workspace_id, surface_id)` pair to stamp into a spawned
/// agent's hook environment. Named fields (crate convention, cf.
/// `PasswordSources` in `cmux-ipc`) prevent silent transposition of the two
/// optional strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnIdentity {
    pub workspace_id: Option<String>,
    pub surface_id: Option<String>,
}

/// Resolves the canonical identity, preferring the launcher's own inherited
/// identity over the operator's focused pane.
///
/// Mirrors `AgentSpawnIdentity.resolve` (AgentSpawnIdentity.swift:46-71):
/// - Inputs are trimmed (`.whitespacesAndNewlines` == Rust [`str::trim`]) and
///   empty values treated as absent.
/// - `workspace_id` = own workspace, else focused workspace.
/// - `surface_id`: the own surface is authoritative ONLY when paired with an
///   own workspace (an orphan own surface with no own workspace is not
///   trusted); otherwise the focused surface is borrowed ONLY when the focused
///   workspace equals the resolved workspace (Swift `Optional` equality —
///   deliberately true when BOTH are `None`), so the result is always a
///   coherent pair the daemon will accept; otherwise `None`, leaving the
///   hook's PID/TTY resolution to pick the agent's real pane.
///
/// Observed-Swift edge pinned by oracle: with no workspace anywhere,
/// `focused_workspace == workspace_id` is `None == None` ⇒ an orphan focused
/// surface passes through: `(None, None, None, Some("A"))` ⇒ `(None, Some("A"))`.
pub fn resolve_spawn_identity(
    own_workspace_id: Option<&str>,
    own_surface_id: Option<&str>,
    focused_workspace_id: Option<&str>,
    focused_surface_id: Option<&str>,
) -> SpawnIdentity {
    let own_workspace = normalized(own_workspace_id);
    let own_surface = normalized(own_surface_id);
    let focused_workspace = normalized(focused_workspace_id);
    let workspace_id = own_workspace.clone().or_else(|| focused_workspace.clone());

    let surface_id = if own_surface.is_some() && own_workspace.is_some() {
        // The own surface is the launch identity, authoritative only paired
        // with its own workspace (AgentSpawnIdentity.swift:58-62).
        own_surface
    } else if focused_workspace == workspace_id {
        // Borrow the focused surface only when the focused pane is in the
        // resolved workspace (AgentSpawnIdentity.swift:63-66).
        normalized(focused_surface_id)
    } else {
        None
    };
    SpawnIdentity {
        workspace_id,
        surface_id,
    }
}

/// Mirrors the private `normalized(_:)` helper
/// (AgentSpawnIdentity.swift:73-78): trim, empty ⇒ `None`.
fn normalized(value: Option<&str>) -> Option<String> {
    let trimmed = value?.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(
        own_workspace: Option<&str>,
        own_surface: Option<&str>,
        focused_workspace: Option<&str>,
        focused_surface: Option<&str>,
    ) -> (Option<String>, Option<String>) {
        let identity = resolve_spawn_identity(
            own_workspace,
            own_surface,
            focused_workspace,
            focused_surface,
        );
        (identity.workspace_id, identity.surface_id)
    }

    fn some(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    /// Oracles from `AgentSpawnIdentityTests.swift:8-94` (all 8 Swift tests)
    /// plus the reachable-but-untested Swift edge and trimming oracles.
    #[test]
    fn resolve_oracles() {
        #[allow(clippy::type_complexity)]
        let cases: &[(
            (Option<&str>, Option<&str>, Option<&str>, Option<&str>),
            (Option<String>, Option<String>),
        )] = &[
            // Prefers the launcher's own surface over the focused pane (#4920).
            (
                (Some("WS-B"), Some("B"), Some("WS-A"), Some("A")),
                (some("WS-B"), some("B")),
            ),
            // Falls back to the focused pane only when no own identity.
            (
                (None, None, Some("WS-A"), Some("A")),
                (some("WS-A"), some("A")),
            ),
            // Blank own identity is treated as absent.
            (
                (Some("   "), Some(""), Some("WS-A"), Some("A")),
                (some("WS-A"), some("A")),
            ),
            // Own workspace + focused surface in a DIFFERENT workspace yields a
            // nil surface, never an impossible cross-workspace pair.
            (
                (Some("WS-B"), None, Some("WS-A"), Some("A")),
                (some("WS-B"), None),
            ),
            // Own workspace + focused surface in the SAME workspace borrows it.
            (
                (Some("WS-A"), None, Some("WS-A"), Some("A")),
                (some("WS-A"), some("A")),
            ),
            // Orphan own surface (no own workspace) is distrusted; the coherent
            // focused pair wins.
            (
                (None, Some("B"), Some("WS-A"), Some("A")),
                (some("WS-A"), some("A")),
            ),
            // Orphan own surface with no focused context yields nil for
            // PID/TTY recovery.
            ((None, Some("B"), None, None), (None, None)),
            // No identity anywhere yields nil.
            ((None, None, None, None), (None, None)),
            // OBSERVED SWIFT BEHAVIOR (reachable, untested upstream): with no
            // workspace anywhere, `focusedWorkspace == workspaceId` is
            // None == None ⇒ true, so an orphan focused surface passes through.
            ((None, None, None, Some("A")), (None, some("A"))),
            // Trimming oracles: tab/newline/NBSP padding is stripped
            // (.whitespacesAndNewlines includes U+00A0), values kept trimmed.
            (
                (
                    Some("\tWS-B\n"),
                    Some("\u{00A0}B\u{00A0}"),
                    Some(" WS-A "),
                    Some(" A "),
                ),
                (some("WS-B"), some("B")),
            ),
            // Whitespace-only focused surface is absent, not borrowed.
            (
                (Some("WS-A"), None, Some("WS-A"), Some(" \n ")),
                (some("WS-A"), None),
            ),
        ];
        for (inputs, expected) in cases {
            let (ow, os, fw, fs) = *inputs;
            assert_eq!(&resolved(ow, os, fw, fs), expected, "resolve({inputs:?})");
        }
    }
}
