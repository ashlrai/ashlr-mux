//! Port of `CommandPaletteSearchCorpusEntry.swift`,
//! `CommandPaletteSearchCorpusResult.swift`, and
//! `CommandPaletteSearchWordText.swift` (the entry-side pieces the mention
//! index needs).

use std::collections::{HashMap, HashSet};

use super::fuzzy::{
    normalize_for_search, prepare_normalized_candidate_text, whole_candidate_prefix_score_by_token,
    PreparedCandidateText, WordSegment,
};

/// One searchable palette entry: a payload plus precomputed normalized
/// title/searchable texts, prefix scores, and the nucleo search blob.
#[derive(Debug, Clone)]
pub struct SearchCorpusEntry<P> {
    pub payload: P,
    /// Stable tie-break rank; lower wins.
    pub rank: i64,
    pub title: String,
    pub prepared_title: Option<PreparedCandidateText>,
    /// Normalized title word text excluding symbol-only segments.
    pub normalized_title_search_word_text: String,
    pub prepared_searchable_texts: Vec<PreparedCandidateText>,
    /// Set of normalized searchable texts for exact-match checks.
    pub searchable_text_set: HashSet<String>,
    /// Precomputed best prefix scores keyed by prefix text.
    pub searchable_prefix_score_by_token: HashMap<String, i64>,
    /// Newline-joined trimmed searchable texts (Swift hands this to the
    /// nucleo index; kept for parity even though this port has no nucleo).
    pub nucleo_search_text: String,
}

impl<P> SearchCorpusEntry<P> {
    /// Swift: `init(payload:rank:title:searchableTexts:)`.
    pub fn new(payload: P, rank: i64, title: String, searchable_texts: Vec<String>) -> Self {
        let normalized_title = normalize_for_search(&title);
        let prepared_title = prepare_normalized_candidate_text(&normalized_title);
        let normalized_title_search_word_text = prepared_title
            .as_ref()
            .map(|title| normalized_search_word_text(&title.characters, &title.word_segments))
            .unwrap_or_default();

        let mut nucleo_search_texts: Vec<String> = Vec::with_capacity(searchable_texts.len());
        let mut normalized_texts: Vec<String> = Vec::with_capacity(searchable_texts.len());
        let mut seen: HashSet<String> = HashSet::new();
        for text in &searchable_texts {
            let trimmed_text = text.trim_matches(char::is_whitespace);
            if !trimmed_text.is_empty() {
                nucleo_search_texts.push(trimmed_text.to_string());
            }
            let normalized_text = normalize_for_search(text);
            if normalized_text.is_empty() {
                continue;
            }
            if !seen.insert(normalized_text.clone()) {
                continue;
            }
            normalized_texts.push(normalized_text);
        }

        let prepared_searchable_texts: Vec<PreparedCandidateText> = normalized_texts
            .iter()
            .filter_map(|text| prepare_normalized_candidate_text(text))
            .collect();
        let searchable_prefix_score_by_token =
            whole_candidate_prefix_score_by_token(&prepared_searchable_texts);
        Self {
            payload,
            rank,
            title,
            prepared_title,
            normalized_title_search_word_text,
            prepared_searchable_texts,
            searchable_text_set: normalized_texts.into_iter().collect(),
            searchable_prefix_score_by_token,
            nucleo_search_text: nucleo_search_texts.join("\n"),
        }
    }
}

/// One scored search hit produced by the search engine.
#[derive(Debug, Clone)]
pub struct SearchCorpusResult<P> {
    pub payload: P,
    pub rank: i64,
    pub title: String,
    /// Final score including any history boost.
    pub score: i64,
    /// Title character indices to highlight.
    pub title_match_indices: HashSet<usize>,
}

/// Swift: `commandPaletteNormalizedSearchWordText(characters:segments:)`.
fn normalized_search_word_text(characters: &[char], segments: &[WordSegment]) -> String {
    let mut words: Vec<String> = Vec::with_capacity(segments.len());

    for segment in segments {
        let word_characters = &characters[segment.start..segment.end];
        if !word_characters
            .iter()
            .any(|&character| contains_search_word_scalar(character))
        {
            continue;
        }
        words.push(word_characters.iter().collect());
    }

    words.join(" ")
}

/// Swift: `commandPaletteContainsSearchWordScalar(_:)` — membership in
/// `CharacterSet.alphanumerics`.
///
/// DIVERGENCE: Apple's `.alphanumerics` is L* + M* + N*; Rust
/// `char::is_alphanumeric` is Alphabetic + Nd/Nl/No (no combining marks).
fn contains_search_word_scalar(character: char) -> bool {
    character.is_alphanumeric()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_precomputes_normalized_texts() {
        let entry = SearchCorpusEntry::new(
            "payload",
            3,
            "/Iterate-PR".to_string(),
            vec!["/Iterate-PR".to_string(), "iterate-pr".to_string()],
        );
        assert_eq!(entry.rank, 3);
        assert!(entry.searchable_text_set.contains("/iterate-pr"));
        assert!(entry.searchable_text_set.contains("iterate-pr"));
        assert_eq!(entry.prepared_searchable_texts.len(), 2);
        assert_eq!(entry.normalized_title_search_word_text, "iterate pr");
        assert_eq!(entry.nucleo_search_text, "/Iterate-PR\niterate-pr");
    }

    #[test]
    fn duplicate_normalized_texts_are_deduplicated() {
        let entry = SearchCorpusEntry::new((), 0, "a".to_string(), vec!["A".into(), "a".into()]);
        assert_eq!(entry.prepared_searchable_texts.len(), 1);
    }
}
