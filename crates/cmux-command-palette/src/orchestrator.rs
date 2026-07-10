//! Port of `Orchestration/CommandPaletteSearchOrchestrator.swift`.
//!
//! Reuses [`cmux_mentions::palette::engine::SearchEngine`],
//! [`cmux_mentions::palette::corpus::SearchCorpusEntry`], and
//! [`cmux_mentions::compare::localized_case_insensitive_compare`] verbatim.
//!
//! DIVERGENCE (nucleo): Swift prefers an optional native nucleo FFI index and
//! merges Swift single-edit (typo) matches that nucleo cannot produce. This
//! port always takes the documented nucleo-unavailable fallback path (see the
//! crate docs): the [`NucleoSearchIndex`] placeholder is uninhabited, so
//! callers always pass `search_index = None` and the whole nucleo/merge branch
//! is unreachable. Each search therefore reduces to the Swift-engine path plus
//! the scope branch in [`CommandPaletteSearchOrchestrator::preview_search_matches`].
//! The pure decision/merge statics are still ported and tested so the port is
//! complete if a nucleo backend is ever wired in.

use std::cmp::Ordering;
use std::collections::HashMap;

use cmux_mentions::compare::localized_case_insensitive_compare;
use cmux_mentions::palette::corpus::SearchCorpusEntry;
use cmux_mentions::palette::engine::SearchEngine;

use crate::list_scope::CommandPaletteListScope;
use crate::resolved_match::CommandPaletteResolvedSearchMatch;
use crate::usage::CommandPaletteUsageEntry;

/// DIVERGENCE: uninhabited placeholder for Swift's
/// `CommandPaletteNucleoSearchIndex<String>`. It exists only so the
/// orchestrator methods keep the `search_index` parameter for signature parity;
/// because it has no values, callers can only ever pass `None`.
pub enum NucleoSearchIndex {}

/// One score-boost contribution, `(command_id, query_is_empty) -> boost`.
///
/// Mirrors the Swift `((String, Bool) -> Int)?` closures (history boost and the
/// caller-supplied additional boost).
pub type ScoreBoost<'a> = &'a dyn Fn(&str, bool) -> i64;

/// Orchestrates one palette search across the pure Swift engine (see the module
/// DIVERGENCE note for why the nucleo path is absent).
///
/// A stateless service value: construct one with [`CommandPaletteSearchOrchestrator::new`]
/// and drive a search through the instance methods. The pure decision/transform
/// helpers stay associated functions.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandPaletteSearchOrchestrator;

const SYNCHRONOUS_SEED_CORPUS_LIMIT: usize = 256;

impl CommandPaletteSearchOrchestrator {
    /// Creates a search orchestrator.
    pub fn new() -> Self {
        Self
    }

    /// Keys `values` by `key`, keeping the first element per key.
    ///
    /// Swift: `firstValueDictionary(_:keyedBy:)`.
    pub fn first_value_dictionary<Element, Key, KeyFn>(
        values: Vec<Element>,
        key: KeyFn,
    ) -> HashMap<Key, Element>
    where
        Key: std::hash::Hash + Eq,
        KeyFn: Fn(&Element) -> Key,
    {
        let mut dictionary: HashMap<Key, Element> = HashMap::with_capacity(values.len());
        for value in values {
            let k = key(&value);
            dictionary.entry(k).or_insert(value);
        }
        dictionary
    }

    /// Resolves matches for `query` over the corpus.
    ///
    /// DIVERGENCE (nucleo): `_search_index` is always `None` (see the module
    /// note), so this reduces to the Swift-engine path. `query_is_empty` is
    /// carried for signature parity — Swift only consults it inside the
    /// nucleo-only single-edit fallback gate, which is unreachable here.
    #[allow(clippy::too_many_arguments)]
    pub fn resolved_search_matches(
        &self,
        _search_index: Option<&NucleoSearchIndex>,
        search_corpus: &[SearchCorpusEntry<String>],
        query: &str,
        usage_history: &HashMap<String, CommandPaletteUsageEntry>,
        _query_is_empty: bool,
        history_timestamp: f64,
        additional_score_boost: Option<ScoreBoost<'_>>,
        result_limit: Option<usize>,
        should_cancel: Option<&dyn Fn() -> bool>,
    ) -> Vec<CommandPaletteResolvedSearchMatch> {
        let use_history = !usage_history.is_empty();
        // Swift composes historyBoost and additionalScoreBoost, defaulting the
        // engine's `historyBoost` to `{ _, _ in 0 }` when neither is present.
        let score_boost = |command_id: &String, query_is_empty: bool| -> i64 {
            let mut total = 0;
            if use_history {
                total += Self::history_boost(
                    command_id,
                    query_is_empty,
                    usage_history,
                    history_timestamp,
                );
            }
            if let Some(additional) = additional_score_boost {
                total += additional(command_id.as_str(), query_is_empty);
            }
            total
        };

        let entries: Vec<&SearchCorpusEntry<String>> = search_corpus.iter().collect();
        let results =
            SearchEngine::new(entries).search(query, result_limit, score_boost, should_cancel);

        results
            .into_iter()
            .map(|result| {
                CommandPaletteResolvedSearchMatch::new(
                    result.payload,
                    result.score,
                    result.title_match_indices,
                )
            })
            .collect()
    }

    /// Resolves preview matches: full-corpus search for the commands scope,
    /// candidate-restricted Swift search for the switcher scope.
    #[allow(clippy::too_many_arguments)]
    pub fn preview_search_matches(
        &self,
        scope: CommandPaletteListScope,
        search_index: Option<&NucleoSearchIndex>,
        search_corpus: &[SearchCorpusEntry<String>],
        candidate_command_ids: &[String],
        search_corpus_by_id: &HashMap<String, SearchCorpusEntry<String>>,
        query: &str,
        usage_history: &HashMap<String, CommandPaletteUsageEntry>,
        query_is_empty: bool,
        history_timestamp: f64,
        additional_score_boost: Option<ScoreBoost<'_>>,
        result_limit: usize,
    ) -> Vec<CommandPaletteResolvedSearchMatch> {
        if result_limit == 0 {
            return Vec::new();
        }

        if scope == CommandPaletteListScope::Commands {
            return self.resolved_search_matches(
                search_index,
                search_corpus,
                query,
                usage_history,
                query_is_empty,
                history_timestamp,
                additional_score_boost,
                Some(result_limit),
                None,
            );
        }

        if candidate_command_ids.is_empty() {
            return Vec::new();
        }

        let mut seen_command_ids: std::collections::HashSet<&str> =
            std::collections::HashSet::new();
        let preview_entries: Vec<SearchCorpusEntry<String>> = candidate_command_ids
            .iter()
            .filter_map(|command_id| {
                if !seen_command_ids.insert(command_id.as_str()) {
                    return None;
                }
                search_corpus_by_id.get(command_id).cloned()
            })
            .collect();
        if preview_entries.is_empty() {
            return Vec::new();
        }

        self.resolved_search_matches(
            None,
            &preview_entries,
            query,
            usage_history,
            query_is_empty,
            history_timestamp,
            additional_score_boost,
            Some(result_limit),
            None,
        )
    }

    /// Merges Swift-engine fallback matches with (nucleo) matches, keeping the
    /// better duplicate and returning the top `limit` sorted by score.
    ///
    /// Swift: `mergedSwiftFallbackMatches(_:nucleoMatches:searchCorpusByID:limit:)`.
    pub fn merged_swift_fallback_matches(
        swift_matches: Vec<CommandPaletteResolvedSearchMatch>,
        nucleo_matches: Vec<CommandPaletteResolvedSearchMatch>,
        search_corpus_by_id: &HashMap<String, SearchCorpusEntry<String>>,
        limit: usize,
    ) -> Vec<CommandPaletteResolvedSearchMatch> {
        if limit == 0 {
            return Vec::new();
        }
        let mut matches_by_id: HashMap<String, CommandPaletteResolvedSearchMatch> =
            HashMap::with_capacity(swift_matches.len() + nucleo_matches.len());

        let merge = |matches_by_id: &mut HashMap<String, CommandPaletteResolvedSearchMatch>,
                     m: CommandPaletteResolvedSearchMatch| {
            match matches_by_id.get(&m.command_id) {
                Some(existing) => {
                    if Self::resolved_search_match_is_better(&m, existing, search_corpus_by_id) {
                        matches_by_id.insert(m.command_id.clone(), m);
                    }
                }
                None => {
                    matches_by_id.insert(m.command_id.clone(), m);
                }
            }
        };

        for m in nucleo_matches {
            merge(&mut matches_by_id, m);
        }
        for m in swift_matches {
            merge(&mut matches_by_id, m);
        }

        let mut merged: Vec<CommandPaletteResolvedSearchMatch> =
            matches_by_id.into_values().collect();
        merged.sort_by(|lhs, rhs| {
            if Self::resolved_search_match_is_better(lhs, rhs, search_corpus_by_id) {
                Ordering::Less
            } else if Self::resolved_search_match_is_better(rhs, lhs, search_corpus_by_id) {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });
        merged.truncate(limit);
        merged
    }

    /// Swift: `resolvedSearchMatchIsBetter(_:than:searchCorpusByID:)`.
    fn resolved_search_match_is_better(
        lhs: &CommandPaletteResolvedSearchMatch,
        rhs: &CommandPaletteResolvedSearchMatch,
        search_corpus_by_id: &HashMap<String, SearchCorpusEntry<String>>,
    ) -> bool {
        if lhs.score != rhs.score {
            return lhs.score > rhs.score;
        }
        let lhs_entry = search_corpus_by_id.get(&lhs.command_id);
        let rhs_entry = search_corpus_by_id.get(&rhs.command_id);
        let lhs_rank = lhs_entry.map(|entry| entry.rank).unwrap_or(i64::MAX);
        let rhs_rank = rhs_entry.map(|entry| entry.rank).unwrap_or(i64::MAX);
        if lhs_rank != rhs_rank {
            return lhs_rank < rhs_rank;
        }
        let lhs_title = lhs_entry
            .map(|entry| entry.title.as_str())
            .unwrap_or(lhs.command_id.as_str());
        let rhs_title = rhs_entry
            .map(|entry| entry.title.as_str())
            .unwrap_or(rhs.command_id.as_str());
        // DIVERGENCE: Swift `localizedCaseInsensitiveCompare` — approximated
        // without locale collation (see cmux_mentions::compare).
        match localized_case_insensitive_compare(lhs_title, rhs_title) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => lhs.command_id < rhs.command_id,
        }
    }

    /// Truncates `result_ids` to `limit` preview candidates.
    ///
    /// Swift: `previewCandidateCommandIDs(resultIDs:limit:)`.
    pub fn preview_candidate_command_ids(result_ids: Vec<String>, limit: usize) -> Vec<String> {
        if limit == 0 {
            return Vec::new();
        }
        if result_ids.len() <= limit {
            return result_ids;
        }
        result_ids.into_iter().take(limit).collect()
    }

    /// Whether opening the palette should seed results synchronously instead of
    /// waiting for the async search task.
    ///
    /// Swift: `shouldSynchronouslySeedResults(hasVisibleResultsForScope:hasSearchIndex:corpusCount:)`.
    pub fn should_synchronously_seed_results(
        has_visible_results_for_scope: bool,
        has_search_index: bool,
        corpus_count: usize,
    ) -> bool {
        !has_visible_results_for_scope
            && (has_search_index || corpus_count <= SYNCHRONOUS_SEED_CORPUS_LIMIT)
    }

    /// Whether the visible empty state should be preserved while a search is
    /// pending, to avoid flashing stale results.
    ///
    /// Swift: `shouldPreserveEmptyStateWhileSearchPending(...)`.
    pub fn should_preserve_empty_state_while_search_pending(
        is_search_pending: bool,
        visible_results_scope_matches: bool,
        resolved_search_scope_matches: bool,
        resolved_search_fingerprint_matches: bool,
        resolved_results_are_empty: bool,
    ) -> bool {
        is_search_pending
            && visible_results_scope_matches
            && resolved_search_scope_matches
            && resolved_search_fingerprint_matches
            && resolved_results_are_empty
    }

    /// Recency/frequency boost for `command_id`; reduced to a third when the
    /// query is non-empty.
    ///
    /// Swift: `historyBoost(for:queryIsEmpty:history:now:)`.
    pub fn history_boost(
        command_id: &str,
        query_is_empty: bool,
        history: &HashMap<String, CommandPaletteUsageEntry>,
        now: f64,
    ) -> i64 {
        let Some(entry) = history.get(command_id) else {
            return 0;
        };

        let age_days = f64::max(0.0, now - entry.last_used_at) / 86_400.0;
        // Swift `Int(ageDays * 20)` truncates toward zero; `age_days >= 0` so
        // `as i64` matches.
        let recency_boost = i64::max(0, 320 - (age_days * 20.0) as i64);
        let count_boost = i64::min(180, entry.use_count * 12);
        let total_boost = recency_boost + count_boost;

        if query_is_empty {
            total_boost
        } else {
            i64::max(0, total_boost / 3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corpus(entries: &[(&str, i64, &str)]) -> Vec<SearchCorpusEntry<String>> {
        entries
            .iter()
            .map(|&(id, rank, title)| {
                SearchCorpusEntry::new(
                    id.to_string(),
                    rank,
                    title.to_string(),
                    vec![title.to_string()],
                )
            })
            .collect()
    }

    fn corpus_by_id(
        entries: &[SearchCorpusEntry<String>],
    ) -> HashMap<String, SearchCorpusEntry<String>> {
        entries
            .iter()
            .map(|entry| (entry.payload.clone(), entry.clone()))
            .collect()
    }

    // Swift: `firstValueDictionaryPreservesFirstDuplicateKey`.
    #[test]
    fn first_value_dictionary_preserves_first_duplicate_key() {
        let values = vec![
            ("palette.duplicate", "First"),
            ("palette.unique", "Unique"),
            ("palette.duplicate", "Second"),
        ];
        let by_id =
            CommandPaletteSearchOrchestrator::first_value_dictionary(values, |value| value.0);
        assert_eq!(by_id.get("palette.duplicate").map(|v| v.1), Some("First"));
        assert_eq!(by_id.get("palette.unique").map(|v| v.1), Some("Unique"));
        assert_eq!(by_id.len(), 2);
    }

    // Swift: `swiftFallbackMergeKeepsCombinedResultsSortedByScore`.
    #[test]
    fn swift_fallback_merge_keeps_combined_results_sorted_by_score() {
        let entries = corpus(&[
            ("palette.high", 0, "High Score"),
            ("palette.medium", 1, "Medium Score"),
            ("palette.fallback", 2, "Fallback Score"),
        ]);
        let by_id = corpus_by_id(&entries);

        let matches = CommandPaletteSearchOrchestrator::merged_swift_fallback_matches(
            vec![CommandPaletteResolvedSearchMatch::new(
                "palette.fallback".to_string(),
                25,
                Default::default(),
            )],
            vec![
                CommandPaletteResolvedSearchMatch::new(
                    "palette.medium".to_string(),
                    80,
                    Default::default(),
                ),
                CommandPaletteResolvedSearchMatch::new(
                    "palette.high".to_string(),
                    100,
                    Default::default(),
                ),
            ],
            &by_id,
            3,
        );
        assert_eq!(
            matches
                .iter()
                .map(|m| m.command_id.as_str())
                .collect::<Vec<_>>(),
            vec!["palette.high", "palette.medium", "palette.fallback"]
        );
    }

    // Swift: `previewCandidateCommandIDsAreBounded`.
    #[test]
    fn preview_candidate_command_ids_are_bounded() {
        let result_ids: Vec<String> = (0..500).map(|index| format!("command.{index}")).collect();
        let preview =
            CommandPaletteSearchOrchestrator::preview_candidate_command_ids(result_ids, 192);
        assert_eq!(preview.len(), 192);
        assert_eq!(preview.first().map(String::as_str), Some("command.0"));
        assert_eq!(preview.last().map(String::as_str), Some("command.191"));
    }

    // Swift: `synchronousSeedRunsOnlyWhenScopeHasNoVisibleResultsAndSearchIndexIsReady`.
    #[test]
    fn synchronous_seed_runs_only_when_scope_has_no_visible_results_and_index_ready() {
        assert!(
            CommandPaletteSearchOrchestrator::should_synchronously_seed_results(false, true, 5_000)
        );
        assert!(
            CommandPaletteSearchOrchestrator::should_synchronously_seed_results(false, false, 256)
        );
        assert!(
            !CommandPaletteSearchOrchestrator::should_synchronously_seed_results(false, false, 257)
        );
        assert!(
            !CommandPaletteSearchOrchestrator::should_synchronously_seed_results(true, true, 5_000)
        );
    }

    // Swift: the `pendingEmptyState*` family.
    #[test]
    fn pending_empty_state_preservation_truth_table() {
        // Not preserved when the search is not pending.
        assert!(
            !CommandPaletteSearchOrchestrator::should_preserve_empty_state_while_search_pending(
                false, true, true, true, true
            )
        );
        // Preserved when every condition holds.
        assert!(
            CommandPaletteSearchOrchestrator::should_preserve_empty_state_while_search_pending(
                true, true, true, true, true
            )
        );
        // Not preserved when any single condition is false.
        assert!(
            !CommandPaletteSearchOrchestrator::should_preserve_empty_state_while_search_pending(
                true, false, true, true, true
            )
        );
        assert!(
            !CommandPaletteSearchOrchestrator::should_preserve_empty_state_while_search_pending(
                true, true, false, true, true
            )
        );
        assert!(
            !CommandPaletteSearchOrchestrator::should_preserve_empty_state_while_search_pending(
                true, true, true, false, true
            )
        );
        assert!(
            !CommandPaletteSearchOrchestrator::should_preserve_empty_state_while_search_pending(
                true, true, true, true, false
            )
        );
    }

    #[test]
    fn history_boost_zero_when_absent_and_thirded_for_nonempty_query() {
        let mut history = HashMap::new();
        history.insert(
            "command.a".to_string(),
            CommandPaletteUsageEntry::new(10, 0.0),
        );
        // Absent id -> 0.
        assert_eq!(
            CommandPaletteSearchOrchestrator::history_boost("command.z", true, &history, 0.0),
            0
        );
        // now == lastUsedAt: recency 320, count min(180, 10*12=120) = 120, total 440.
        assert_eq!(
            CommandPaletteSearchOrchestrator::history_boost("command.a", true, &history, 0.0),
            440
        );
        // Non-empty query thirds it: 440 / 3 = 146.
        assert_eq!(
            CommandPaletteSearchOrchestrator::history_boost("command.a", false, &history, 0.0),
            146
        );
    }
}
