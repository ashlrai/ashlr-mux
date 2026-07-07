//! Command-palette search bridge.
//!
//! A thin Tauri surface over
//! [`CommandPaletteSearchOrchestrator::preview_search_matches`] — the ported
//! fuzzy matcher + history/recency score boost. The web palette (D-area) sends
//! the corpus, the scope-stripped query, the switcher candidate ids, and the
//! usage history; this builds the [`SearchCorpusEntry`] corpus, runs the
//! orchestrator, and returns the resolved matches (command id + score + title
//! highlight indices).
//!
//! The scoring — `history_boost` (recency + frequency, thirded for a non-empty
//! query) composed with the fuzzy match score, and the `result_limit == 0`
//! short-circuit — lives entirely in the orchestrator, so this layer only
//! marshals data and never re-derives ranking.

use std::collections::HashMap;

use cmux_command_palette::{
    CommandPaletteListScope, CommandPaletteSearchOrchestrator, CommandPaletteUsageEntry,
};
use cmux_mentions::palette::corpus::SearchCorpusEntry;
use serde::{Deserialize, Serialize};

/// One entry in the searchable corpus, as sent by the web palette.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusEntryInput {
    /// Stable command identifier (the match payload).
    pub command_id: String,
    /// Tie-break rank; lower wins.
    #[serde(default)]
    pub rank: i64,
    /// User-visible title (drives highlight indices).
    pub title: String,
    /// Additional searchable text (keywords, directories, branches, …).
    #[serde(default)]
    pub searchable_texts: Vec<String>,
}

/// The full search request from the palette's query hook.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRequest {
    /// `"commands"` (`>`-prefixed) or `"switcher"`.
    pub scope: String,
    /// The already-scope-stripped matcher query (web `queryForMatching`).
    pub query: String,
    /// Candidate command ids for the switcher scope (unused in commands scope).
    #[serde(default)]
    pub candidate_command_ids: Vec<String>,
    /// The full searchable corpus.
    pub corpus: Vec<CorpusEntryInput>,
    /// Prior-usage records keyed by command id (drives the recency/frequency
    /// boost). Reuses the crate's serde-ready usage type directly.
    #[serde(default)]
    pub usage_history: HashMap<String, CommandPaletteUsageEntry>,
    /// Whether the (scope-stripped) query is empty.
    #[serde(default)]
    pub query_is_empty: bool,
    /// `now` seconds for the recency boost (Swift `systemUptime`-style).
    #[serde(default)]
    pub history_timestamp: f64,
    /// Max results to return; `0` short-circuits to an empty list.
    pub result_limit: usize,
}

/// One resolved match returned to the web renderer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchMatch {
    pub command_id: String,
    pub score: i64,
    /// Title character indices to highlight, ascending.
    pub title_match_indices: Vec<usize>,
}

fn scope_from_str(scope: &str) -> CommandPaletteListScope {
    match scope {
        "commands" => CommandPaletteListScope::Commands,
        _ => CommandPaletteListScope::Switcher,
    }
}

/// Pure search: builds the corpus, runs the orchestrator preview search, and
/// maps the results. Headless-testable (no Tauri).
pub fn run_search(request: SearchRequest) -> Vec<SearchMatch> {
    let SearchRequest {
        scope,
        query,
        candidate_command_ids,
        corpus,
        usage_history,
        query_is_empty,
        history_timestamp,
        result_limit,
    } = request;

    let scope = scope_from_str(&scope);
    let corpus: Vec<SearchCorpusEntry<String>> = corpus
        .into_iter()
        .map(|e| SearchCorpusEntry::new(e.command_id, e.rank, e.title, e.searchable_texts))
        .collect();

    // `preview_search_matches` only reads the by-id map in the switcher scope
    // (the commands scope searches the full corpus directly). Each entry holds
    // precomputed prepared texts + hash maps, so skip the clone-heavy build
    // entirely for the common commands path.
    let corpus_by_id: HashMap<String, SearchCorpusEntry<String>> =
        if scope == CommandPaletteListScope::Switcher {
            corpus
                .iter()
                .map(|entry| (entry.payload.clone(), entry.clone()))
                .collect()
        } else {
            HashMap::new()
        };

    let orchestrator = CommandPaletteSearchOrchestrator::new();
    let matches = orchestrator.preview_search_matches(
        scope,
        None,
        &corpus,
        &candidate_command_ids,
        &corpus_by_id,
        &query,
        &usage_history,
        query_is_empty,
        history_timestamp,
        None,
        result_limit,
    );

    matches
        .into_iter()
        .map(|m| {
            let mut title_match_indices: Vec<usize> = m.title_match_indices.into_iter().collect();
            title_match_indices.sort_unstable();
            SearchMatch {
                command_id: m.command_id,
                score: m.score,
                title_match_indices,
            }
        })
        .collect()
}

/// Runs a command-palette search and returns the resolved matches.
#[tauri::command]
pub fn command_palette_search(request: SearchRequest) -> Vec<SearchMatch> {
    run_search(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(command_id: &str, title: &str) -> CorpusEntryInput {
        CorpusEntryInput {
            command_id: command_id.to_string(),
            rank: 0,
            title: title.to_string(),
            // Include the title text so the matcher has a searchable body.
            searchable_texts: vec![title.to_string()],
        }
    }

    fn request(scope: &str, query: &str, result_limit: usize) -> SearchRequest {
        SearchRequest {
            scope: scope.to_string(),
            query: query.to_string(),
            candidate_command_ids: Vec::new(),
            corpus: vec![
                entry("cmd.rename", "Rename Workspace"),
                entry("cmd.split", "Split Right"),
                entry("cmd.close", "Close Pane"),
            ],
            usage_history: HashMap::new(),
            query_is_empty: query.is_empty(),
            history_timestamp: 0.0,
            result_limit,
        }
    }

    #[test]
    fn commands_scope_matches_by_title() {
        let matches = run_search(request("commands", "rename", 10));
        assert!(matches.iter().any(|m| m.command_id == "cmd.rename"));
        // The non-matching commands should not appear.
        assert!(!matches.iter().any(|m| m.command_id == "cmd.close"));
    }

    #[test]
    fn result_limit_zero_short_circuits_to_empty() {
        assert!(run_search(request("commands", "rename", 0)).is_empty());
    }

    #[test]
    fn a_non_matching_query_returns_no_matches() {
        assert!(run_search(request("commands", "zzzznotacommand", 10)).is_empty());
    }

    #[test]
    fn switcher_scope_restricts_to_candidate_ids() {
        let mut req = request("switcher", "e", 10);
        // Only `cmd.rename` is a candidate, so `cmd.close`/`cmd.split` cannot win
        // even if they'd otherwise match the query.
        req.candidate_command_ids = vec!["cmd.rename".to_string()];
        let matches = run_search(req);
        assert!(matches.iter().all(|m| m.command_id == "cmd.rename"));
    }

    #[test]
    fn switcher_scope_with_no_candidates_is_empty() {
        assert!(run_search(request("switcher", "rename", 10)).is_empty());
    }

    #[test]
    fn matched_titles_carry_ascending_highlight_indices() {
        let matches = run_search(request("commands", "rename", 10));
        let rename = matches
            .iter()
            .find(|m| m.command_id == "cmd.rename")
            .expect("rename should match");
        assert!(!rename.title_match_indices.is_empty());
        assert!(rename.title_match_indices.windows(2).all(|w| w[0] < w[1]));
    }
}
