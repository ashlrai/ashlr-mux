//! Port of `Sources/TextBoxMentionCandidateIndex.swift`.
//!
//! DIVERGENCE: Swift builds a `CommandPaletteNucleoSearchIndex` accelerator
//! when the corpus has >= 32 entries and probes it before validating with the
//! pure Swift ranking path (`swiftRankedCandidates`), falling back to the
//! full pure path whenever the nucleo library is unavailable or its probe may
//! be incomplete. This port has no nucleo binding, so it always takes Swift's
//! documented nucleo-unavailable fallback: `swiftRankedCandidates` over the
//! full corpus. (`nucleoProbeLimitMultiplier` / `minimumNucleoProbeLimit` and
//! the `corpusByTargetPath` probe lookup exist only for that accelerator and
//! are therefore not ported.)

use crate::candidate::MentionCandidate;
use crate::compare::localized_standard_compare;
use crate::palette::corpus::SearchCorpusEntry;
use crate::palette::engine::SearchEngine;
use crate::palette::fuzzy::{prepared_query, token_can_match_without_single_edit, PreparedQuery};

/// Swift: `struct TextBoxMentionCandidateIndex: Sendable`.
#[derive(Debug, Clone)]
pub struct MentionCandidateIndex {
    corpus: Vec<SearchCorpusEntry<MentionCandidate>>,
    empty_query_candidates: Vec<MentionCandidate>,
}

impl MentionCandidateIndex {
    /// Swift: `init(candidates:)`.
    pub fn new(candidates: Vec<MentionCandidate>) -> Self {
        let corpus: Vec<SearchCorpusEntry<MentionCandidate>> = candidates
            .into_iter()
            .map(|candidate| {
                let rank = candidate.priority;
                let title = candidate.title.clone();
                let searchable_texts = vec![candidate.title.clone(), candidate.search_key.clone()];
                SearchCorpusEntry::new(candidate, rank, title, searchable_texts)
            })
            .collect();
        let mut sorted: Vec<&SearchCorpusEntry<MentionCandidate>> = corpus.iter().collect();
        sorted.sort_by(|lhs, rhs| {
            lhs.rank
                .cmp(&rhs.rank)
                .then_with(|| localized_standard_compare(&lhs.title, &rhs.title))
        });
        let empty_query_candidates = sorted
            .into_iter()
            .map(|entry| entry.payload.clone())
            .collect();
        Self {
            corpus,
            empty_query_candidates,
        }
    }

    /// Swift: `rankedCandidates(matching:limit:)` with the default
    /// always-false cancellation probe.
    pub fn ranked_candidates(&self, raw_query: &str, limit: usize) -> Vec<MentionCandidate> {
        self.ranked_candidates_cancellable(raw_query, limit, &|| false)
    }

    /// Swift: `rankedCandidates(matching:limit:shouldCancel:)`.
    pub fn ranked_candidates_cancellable(
        &self,
        raw_query: &str,
        limit: usize,
        should_cancel: &dyn Fn() -> bool,
    ) -> Vec<MentionCandidate> {
        if limit == 0 || should_cancel() {
            return Vec::new();
        }
        let query = raw_query.trim_matches(char::is_whitespace);
        if query.is_empty() {
            return self
                .empty_query_candidates
                .iter()
                .take(limit)
                .cloned()
                .collect();
        }

        Self::swift_ranked_candidates(&self.corpus, query, limit, should_cancel)
    }

    /// Swift: `swiftRankedCandidates(entries:query:limit:shouldCancel:)` —
    /// prefilter with `tokenCanMatchWithoutSingleEdit`, then rank the
    /// survivors with the command-palette search engine.
    fn swift_ranked_candidates(
        entries: &[SearchCorpusEntry<MentionCandidate>],
        query: &str,
        limit: usize,
        should_cancel: &dyn Fn() -> bool,
    ) -> Vec<MentionCandidate> {
        let prepared_query = prepared_query(query);
        let filtered_entries: Vec<&SearchCorpusEntry<MentionCandidate>> =
            if prepared_query.is_empty() {
                entries.iter().collect()
            } else {
                let mut matches: Vec<&SearchCorpusEntry<MentionCandidate>> =
                    Vec::with_capacity(entries.len().min(limit));
                for entry in entries {
                    if should_cancel() {
                        return Vec::new();
                    }
                    if mention_candidate_matches(entry, &prepared_query) {
                        matches.push(entry);
                    }
                }
                if should_cancel() {
                    return Vec::new();
                }
                matches
            };
        if filtered_entries.is_empty() {
            return Vec::new();
        }

        SearchEngine::new(filtered_entries)
            .search(query, Some(limit), |_, _| 0, Some(should_cancel))
            .into_iter()
            .map(|result| result.payload)
            .collect()
    }
}

/// Swift: `mentionCandidate(_:matches:)` — every query token must match some
/// searchable text without relying on the single-edit (typo) fallback.
fn mention_candidate_matches(
    entry: &SearchCorpusEntry<MentionCandidate>,
    prepared_query: &PreparedQuery,
) -> bool {
    if prepared_query.is_empty() {
        return true;
    }
    for token in &prepared_query.tokens {
        let mut token_matches_candidate = false;
        for candidate in &entry.prepared_searchable_texts {
            if token_can_match_without_single_edit(token, candidate) {
                token_matches_candidate = true;
                break;
            }
        }
        if !token_matches_candidate {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn skill_candidate(skill_name: &str) -> MentionCandidate {
        MentionCandidate {
            title: format!("/{skill_name}"),
            subtitle: format!("/tmp/skills/{skill_name}/SKILL.md"),
            target_path: format!("/tmp/skills/{skill_name}/SKILL.md"),
            system_image_name: "sparkle.magnifyingglass".into(),
            search_key: skill_name.to_string(),
            priority: 0,
        }
    }

    // Oracle: cmuxTests/TextBoxMentionCompletionTests.swift
    // `testTextBoxMentionCandidateIndexDoesNotReturnUnvalidatedNucleoRows`.
    #[test]
    fn does_not_return_unvalidated_nucleo_rows() {
        let mut skill_names: Vec<String> = [
            "agent-browser",
            "agent-cli-integration",
            "algorithmic-complexity-audit",
            "auto-issue",
            "cleanup-dev-builds",
            "close-issues",
            "pi-agent-rust",
            "xcodebuildmcp-cli",
        ]
        .iter()
        .map(|name| name.to_string())
        .collect();
        skill_names.extend((0..40).map(|index| format!("zzz-distractor-{index:02}")));
        let candidates: Vec<MentionCandidate> = skill_names
            .iter()
            .map(|name| skill_candidate(name))
            .collect();

        let matches = MentionCandidateIndex::new(candidates).ranked_candidates("iterate-pr", 500);

        assert!(matches.is_empty());
    }

    // Oracle: `testTextBoxMentionCandidateIndexFiltersWeakPartialFuzzyRows`.
    #[test]
    fn filters_weak_partial_fuzzy_rows() {
        let candidates: Vec<MentionCandidate> = [
            "agent-browser",
            "agent-cli-integration",
            "pi-agent-rust",
            "iterate-pr",
        ]
        .iter()
        .map(|name| skill_candidate(name))
        .collect();

        let matches = MentionCandidateIndex::new(candidates).ranked_candidates("iterate", 500);

        assert_eq!(
            matches.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            vec!["/iterate-pr"]
        );
    }

    // Oracle: `testTextBoxMentionCandidateIndexStopsPrefilterWhenCancelled`.
    #[test]
    fn stops_prefilter_when_cancelled() {
        let candidates: Vec<MentionCandidate> = [
            "agent-browser",
            "agent-cli-integration",
            "pi-agent-rust",
            "iterate-pr",
        ]
        .iter()
        .map(|name| skill_candidate(name))
        .collect();
        let cancellation_checks = Cell::new(0u32);

        let matches = MentionCandidateIndex::new(candidates).ranked_candidates_cancellable(
            "iterate",
            500,
            &|| {
                cancellation_checks.set(cancellation_checks.get() + 1);
                cancellation_checks.get() > 1
            },
        );

        assert!(matches.is_empty());
        assert!(cancellation_checks.get() > 1);
    }

    // Oracle behavior from `testTextBoxMentionSkillSuggestionsPreferExactNameOverPathOnlyFuzzyMatches`
    // (the pure ranking half: exact name wins, weak fuzzy rows are absent).
    #[test]
    fn prefers_exact_name_over_path_only_fuzzy_matches() {
        let mut skill_names: Vec<String> = [
            "agent-browser",
            "agent-cli-integration",
            "algorithmic-complexity-audit",
            "auto-issue",
            "cleanup-dev-builds",
            "close-issues",
            "pi-agent-rust",
            "xcodebuildmcp-cli",
            "iterate-pr",
        ]
        .iter()
        .map(|name| name.to_string())
        .collect();
        skill_names.extend((0..40).map(|index| format!("zzz-distractor-{index:02}")));
        let candidates: Vec<MentionCandidate> = skill_names
            .iter()
            .map(|name| skill_candidate(name))
            .collect();
        let index = MentionCandidateIndex::new(candidates);

        let matches = index.ranked_candidates("iterate-pr", 500);
        assert_eq!(
            matches.first().map(|c| c.title.as_str()),
            Some("/iterate-pr")
        );
        assert!(!matches.iter().any(|c| c.title == "/pi-agent-rust"));
        assert!(!matches.iter().any(|c| c.title == "/agent-browser"));
    }

    #[test]
    fn empty_query_orders_by_priority_then_natural_title() {
        let mut low_priority = skill_candidate("zz-local-skill");
        low_priority.priority = 0;
        let mut high_priority = skill_candidate("aaa-global-000");
        high_priority.priority = 3;
        let mut middle = skill_candidate("beta");
        middle.priority = 0;

        let index = MentionCandidateIndex::new(vec![
            high_priority.clone(),
            low_priority.clone(),
            middle.clone(),
        ]);
        let matches = index.ranked_candidates("", 500);
        assert_eq!(
            matches.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(),
            vec!["/beta", "/zz-local-skill", "/aaa-global-000"]
        );
    }

    #[test]
    fn zero_limit_returns_empty() {
        let index = MentionCandidateIndex::new(vec![skill_candidate("iterate-pr")]);
        assert!(index.ranked_candidates("iterate", 0).is_empty());
        assert!(index.ranked_candidates("", 0).is_empty());
    }

    #[test]
    fn empty_query_respects_limit() {
        let index = MentionCandidateIndex::new(vec![
            skill_candidate("alpha"),
            skill_candidate("beta"),
            skill_candidate("gamma"),
        ]);
        assert_eq!(index.ranked_candidates("", 2).len(), 2);
    }
}
