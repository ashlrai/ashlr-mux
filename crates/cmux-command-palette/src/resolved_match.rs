//! Port of `Orchestration/CommandPaletteResolvedSearchMatch.swift`.

use std::collections::HashSet;

/// One resolved match produced by the search orchestrator.
///
/// (Swift merges nucleo and Swift-engine results; this port always produces
/// Swift-engine results — see [`crate::orchestrator`].)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPaletteResolvedSearchMatch {
    /// The matched command's identifier.
    pub command_id: String,
    /// Final merged score.
    pub score: i64,
    /// Title character indices to highlight.
    pub title_match_indices: HashSet<usize>,
}

impl CommandPaletteResolvedSearchMatch {
    /// Creates a resolved match.
    pub fn new(command_id: String, score: i64, title_match_indices: HashSet<usize>) -> Self {
        Self {
            command_id,
            score,
            title_match_indices,
        }
    }
}
