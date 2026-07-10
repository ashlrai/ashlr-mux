//! Port of `CommandPaletteSearchEngine.swift` — the pure ranking engine over
//! a prepared corpus: scores entries with the fuzzy matcher, applies history
//! boosts, and returns the top results in deterministic order
//! (score, rank, title, index).

use std::cmp::Ordering;
use std::collections::HashSet;

use super::corpus::{SearchCorpusEntry, SearchCorpusResult};
use super::fuzzy::{
    match_character_indices, prepared_query, score_prepared_candidate, score_prepared_candidates,
    PreparedQuery,
};
use crate::compare::localized_case_insensitive_compare;

const TITLE_MATCH_BONUS: i64 = 2000;

/// The engine is a value object bound to one corpus: construct it with the
/// entries to rank, then call [`SearchEngine::search`] to produce results.
pub struct SearchEngine<'a, P> {
    entries: Vec<&'a SearchCorpusEntry<P>>,
}

#[derive(Clone, Copy)]
struct ScoredEntry {
    index: usize,
    score: i64,
}

impl<'a, P: Clone> SearchEngine<'a, P> {
    pub fn new(entries: Vec<&'a SearchCorpusEntry<P>>) -> Self {
        Self { entries }
    }

    fn scored_entry_is_better(&self, lhs: ScoredEntry, rhs: ScoredEntry) -> bool {
        if lhs.score != rhs.score {
            return lhs.score > rhs.score;
        }
        let lhs_entry = self.entries[lhs.index];
        let rhs_entry = self.entries[rhs.index];
        if lhs_entry.rank != rhs_entry.rank {
            return lhs_entry.rank < rhs_entry.rank;
        }
        // DIVERGENCE: Swift uses `localizedCaseInsensitiveCompare`; this port
        // approximates without locale collation (see crate::compare).
        match localized_case_insensitive_compare(&lhs_entry.title, &rhs_entry.title) {
            Ordering::Less => true,
            Ordering::Greater => false,
            Ordering::Equal => lhs.index < rhs.index,
        }
    }

    fn scored_entry_is_worse(&self, lhs: ScoredEntry, rhs: ScoredEntry) -> bool {
        self.scored_entry_is_better(rhs, lhs)
    }

    fn sift_up_worst_scored_entry_heap(&self, heap: &mut [ScoredEntry], start_index: usize) {
        let mut child = start_index;
        while child > 0 {
            let parent = (child - 1) / 2;
            if !self.scored_entry_is_worse(heap[child], heap[parent]) {
                break;
            }
            heap.swap(child, parent);
            child = parent;
        }
    }

    fn sift_down_worst_scored_entry_heap(&self, heap: &mut [ScoredEntry], start_index: usize) {
        let mut parent = start_index;
        loop {
            let left_child = (parent * 2) + 1;
            if left_child >= heap.len() {
                return;
            }

            let right_child = left_child + 1;
            let mut worst_child = left_child;
            if right_child < heap.len()
                && self.scored_entry_is_worse(heap[right_child], heap[left_child])
            {
                worst_child = right_child;
            }

            if !self.scored_entry_is_worse(heap[worst_child], heap[parent]) {
                return;
            }
            heap.swap(parent, worst_child);
            parent = worst_child;
        }
    }

    fn append_scored_entry(
        &self,
        scored_entry: ScoredEntry,
        scored_entries: &mut Vec<ScoredEntry>,
        limit: Option<usize>,
    ) {
        let Some(limit) = limit else {
            scored_entries.push(scored_entry);
            return;
        };

        if scored_entries.len() < limit {
            scored_entries.push(scored_entry);
            let last = scored_entries.len() - 1;
            self.sift_up_worst_scored_entry_heap(scored_entries, last);
            return;
        }

        let Some(&worst_entry) = scored_entries.first() else {
            return;
        };
        if !self.scored_entry_is_better(scored_entry, worst_entry) {
            return;
        }
        scored_entries[0] = scored_entry;
        self.sift_down_worst_scored_entry_heap(scored_entries, 0);
    }

    /// Swift: `search(query:resultLimit:historyBoost:shouldCancel:)` — a
    /// `should_cancel` of `None` matches the Swift overload without a
    /// cancellation probe (which is otherwise checked every 16 entries).
    pub fn search(
        &self,
        query: &str,
        result_limit: Option<usize>,
        history_boost: impl Fn(&P, bool) -> i64,
        should_cancel: Option<&dyn Fn() -> bool>,
    ) -> Vec<SearchCorpusResult<P>> {
        if result_limit == Some(0) {
            return Vec::new();
        }
        let prepared_query = prepared_query(query);
        let query_is_empty = prepared_query.is_empty();
        let limited_result_count = result_limit.map(|limit| limit.min(self.entries.len()));
        let mut scored_entries: Vec<ScoredEntry> =
            Vec::with_capacity(limited_result_count.unwrap_or(self.entries.len()));

        let should_cancel_search = |index: usize| -> bool {
            match should_cancel {
                Some(probe) => index.is_multiple_of(16) && probe(),
                None => false,
            }
        };

        if query_is_empty {
            for (index, entry) in self.entries.iter().enumerate() {
                if should_cancel_search(index) {
                    return Vec::new();
                }
                self.append_scored_entry(
                    ScoredEntry {
                        index,
                        score: history_boost(&entry.payload, true),
                    },
                    &mut scored_entries,
                    limited_result_count,
                );
            }
        } else {
            for (index, entry) in self.entries.iter().enumerate() {
                if should_cancel_search(index) {
                    return Vec::new();
                }
                let Some(fuzzy_score) = weighted_score(&prepared_query, entry) else {
                    continue;
                };
                self.append_scored_entry(
                    ScoredEntry {
                        index,
                        score: fuzzy_score + history_boost(&entry.payload, false),
                    },
                    &mut scored_entries,
                    limited_result_count,
                );
            }
        }

        if let Some(probe) = should_cancel {
            if probe() {
                return Vec::new();
            }
        }

        scored_entries.sort_by(|&lhs, &rhs| {
            if self.scored_entry_is_better(lhs, rhs) {
                Ordering::Less
            } else if self.scored_entry_is_better(rhs, lhs) {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        });

        let output_count = result_limit
            .map(|limit| limit.min(scored_entries.len()))
            .unwrap_or(scored_entries.len());
        let mut results: Vec<SearchCorpusResult<P>> = Vec::with_capacity(output_count);
        for (output_index, scored_entry) in scored_entries.iter().take(output_count).enumerate() {
            if should_cancel_search(output_index) {
                return Vec::new();
            }
            let entry = self.entries[scored_entry.index];
            let title_match_indices: HashSet<usize> = if query_is_empty {
                HashSet::new()
            } else {
                entry
                    .prepared_title
                    .as_ref()
                    .map(|prepared_title| match_character_indices(&prepared_query, prepared_title))
                    .unwrap_or_default()
            };
            results.push(SearchCorpusResult {
                payload: entry.payload.clone(),
                rank: entry.rank,
                title: entry.title.clone(),
                score: scored_entry.score,
                title_match_indices,
            });
        }
        results
    }
}

fn weighted_score<P>(prepared_query: &PreparedQuery, entry: &SearchCorpusEntry<P>) -> Option<i64> {
    let fuzzy_score = score_prepared_candidates(
        prepared_query,
        &entry.prepared_searchable_texts,
        Some(&entry.searchable_text_set),
        Some(&entry.searchable_prefix_score_by_token),
    )?;
    if let Some(prepared_title) = &entry.prepared_title {
        if prepared_query
            .tokens
            .iter()
            .all(|token| token.could_match(prepared_title))
        {
            if let Some(title_score) = score_prepared_candidate(prepared_query, prepared_title) {
                return Some(
                    fuzzy_score.max(title_score + TITLE_MATCH_BONUS).max(
                        title_word_score(
                            prepared_query,
                            &prepared_title.normalized_text,
                            &entry.normalized_title_search_word_text,
                        )
                        .unwrap_or(i64::MIN),
                    ),
                );
            }
        }
    }
    Some(fuzzy_score)
}

/// Swift: `commandPaletteTitleWordScore(preparedQuery:titleNormalizedText:titleSearchWordText:)`.
fn title_word_score(
    prepared_query: &PreparedQuery,
    title_normalized_text: &str,
    title_search_word_text: &str,
) -> Option<i64> {
    if prepared_query.is_empty() || title_search_word_text == title_normalized_text {
        return None;
    }

    if title_search_word_text == prepared_query.normalized_token_text {
        let exact_token_score: i64 = prepared_query
            .tokens
            .iter()
            .map(|token| token.score_upper_bound)
            .sum();
        return Some(exact_token_score + scaled_title_match_bonus(prepared_query.tokens.len()));
    }

    if !title_search_word_text.starts_with(&prepared_query.normalized_token_text) {
        return None;
    }

    let prefix_token_score: i64 = prepared_query
        .tokens
        .iter()
        .map(|token| token.score_upper_bound_without_exact_match)
        .sum();
    Some(prefix_token_score + scaled_title_match_bonus(prepared_query.tokens.len()))
}

/// Swift: `commandPaletteScaledTitleMatchBonus(tokenCount:)`.
fn scaled_title_match_bonus(token_count: usize) -> i64 {
    2000 * 1i64.max(token_count as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, rank: i64) -> SearchCorpusEntry<String> {
        SearchCorpusEntry::new(
            title.to_string(),
            rank,
            title.to_string(),
            vec![title.to_string()],
        )
    }

    #[test]
    fn empty_query_returns_all_in_stable_order() {
        let entries = [entry("beta", 1), entry("alpha", 0)];
        let engine = SearchEngine::new(entries.iter().collect());
        let results = engine.search("", None, |_, _| 0, None);
        // Zero scores everywhere: rank breaks the tie.
        assert_eq!(
            results.iter().map(|r| r.title.as_str()).collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );
    }

    #[test]
    fn result_limit_keeps_best_entries() {
        let entries = [
            entry("match", 0),
            entry("match-longer", 1),
            entry("other", 2),
        ];
        let engine = SearchEngine::new(entries.iter().collect());
        let results = engine.search("match", Some(1), |_, _| 0, None);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "match");
    }

    #[test]
    fn zero_result_limit_returns_empty() {
        let entries = [entry("match", 0)];
        let engine = SearchEngine::new(entries.iter().collect());
        assert!(engine.search("match", Some(0), |_, _| 0, None).is_empty());
    }

    #[test]
    fn cancellation_probe_aborts_scan() {
        let entries = [entry("match", 0)];
        let engine = SearchEngine::new(entries.iter().collect());
        let cancelled = || true;
        assert!(engine
            .search("match", None, |_, _| 0, Some(&cancelled))
            .is_empty());
    }

    #[test]
    fn history_boost_reorders_results() {
        let entries = [entry("match-a", 0), entry("match-b", 0)];
        let engine = SearchEngine::new(entries.iter().collect());
        let results = engine.search(
            "match",
            None,
            |payload, _| if payload == "match-b" { 10_000 } else { 0 },
            None,
        );
        assert_eq!(results[0].title, "match-b");
    }
}
