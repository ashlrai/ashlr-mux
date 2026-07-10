//! The agent that produced a workstream item.
//!
//! Swift parity source:
//! `Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/Workstream/WorkstreamSource.swift:1-27`.

use serde::{Deserialize, Serialize};

/// The agent that produced a `WorkstreamItem`. The raw value matches the
/// `_source` field on the wire frame that cmux hooks and the OpenCode plugin
/// emit, and matches Vibe Island's source tag 1:1.
///
/// Swift `WorkstreamSource` (`WorkstreamSource.swift:7-27`). Raw values are the
/// lowercased case names except `hermesAgent` → `"hermes-agent"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkstreamSource {
    Claude,
    Codex,
    Pi,
    Amp,
    Cursor,
    Opencode,
    Gemini,
    #[serde(rename = "hermes-agent")]
    HermesAgent,
    Copilot,
    Codebuddy,
    Factory,
    Qoder,
}

impl WorkstreamSource {
    /// Every case, in declaration order (Swift `CaseIterable`).
    pub const ALL: [Self; 12] = [
        Self::Claude,
        Self::Codex,
        Self::Pi,
        Self::Amp,
        Self::Cursor,
        Self::Opencode,
        Self::Gemini,
        Self::HermesAgent,
        Self::Copilot,
        Self::Codebuddy,
        Self::Factory,
        Self::Qoder,
    ];

    /// The wire `_source` raw value.
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
            Self::Amp => "amp",
            Self::Cursor => "cursor",
            Self::Opencode => "opencode",
            Self::Gemini => "gemini",
            Self::HermesAgent => "hermes-agent",
            Self::Copilot => "copilot",
            Self::Codebuddy => "codebuddy",
            Self::Factory => "factory",
            Self::Qoder => "qoder",
        }
    }

    /// Parses a wire-frame `_source` string. Unknown sources fall back to
    /// `None`; callers persist the raw string separately when they want to
    /// surface out-of-band agents without widening this enum.
    ///
    /// Swift `init?(wireName:)` (`WorkstreamSource.swift:24-26`).
    pub fn from_wire(wire_name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.wire_name() == wire_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_wire_parses_known_and_rejects_unknown() {
        assert_eq!(
            WorkstreamSource::from_wire("claude"),
            Some(WorkstreamSource::Claude)
        );
        assert_eq!(
            WorkstreamSource::from_wire("hermes-agent"),
            Some(WorkstreamSource::HermesAgent)
        );
        assert_eq!(WorkstreamSource::from_wire("nope"), None);
    }

    #[test]
    fn serialize_matches_wire_names() {
        assert_eq!(
            serde_json::to_string(&WorkstreamSource::HermesAgent).unwrap(),
            "\"hermes-agent\""
        );
        assert_eq!(
            serde_json::to_string(&WorkstreamSource::Opencode).unwrap(),
            "\"opencode\""
        );
        let decoded: WorkstreamSource = serde_json::from_str("\"codex\"").unwrap();
        assert_eq!(decoded, WorkstreamSource::Codex);
    }
}
