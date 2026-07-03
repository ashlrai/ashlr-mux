//! Port of `Orchestration/CommandPaletteUsageEntry.swift`.

use serde::{Deserialize, Serialize};

/// Persisted per-command usage stats backing the recency/frequency boost.
///
/// The serde field names match the Swift `Codable` shape (`useCount`,
/// `lastUsedAt`) so a persisted store is wire-compatible across the port.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandPaletteUsageEntry {
    /// Total times the command was run (Swift `Int`).
    pub use_count: i64,
    /// Unix timestamp of the most recent run (Swift `TimeInterval`).
    pub last_used_at: f64,
}

impl CommandPaletteUsageEntry {
    /// Creates a usage entry.
    pub fn new(use_count: i64, last_used_at: f64) -> Self {
        Self {
            use_count,
            last_used_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: A JSON round-trip is not asserted here because `serde_json` is not
    // a dependency of this crate. The `#[serde(rename_all = "camelCase")]`
    // attribute encodes the `useCount` / `lastUsedAt` Codable key shape, and
    // the `Serialize`/`Deserialize` derives are exercised by compilation.
    #[test]
    fn constructs_and_compares() {
        let entry = CommandPaletteUsageEntry::new(3, 1_234.5);
        assert_eq!(entry.use_count, 3);
        assert_eq!(entry.last_used_at, 1_234.5);
        assert_eq!(entry, CommandPaletteUsageEntry::new(3, 1_234.5));
    }
}
