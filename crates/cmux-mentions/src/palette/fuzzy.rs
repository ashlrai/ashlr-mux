//! Port of `Packages/macOS/CmuxCommandPalette/.../Search/CommandPaletteFuzzyMatcher.swift`
//! — the exact subset `TextBoxMentionCandidateIndex.swift` depends on
//! (preparation, scoring, prefilter, match-index highlighting).
//!
//! DIVERGENCE (module-wide): Swift `Character` arrays are grapheme clusters;
//! this port uses Unicode scalar values (`char`). Behavior differs only for
//! combining sequences, which Swift's diacritic folding largely removes
//! before matching anyway.

use std::collections::{HashMap, HashSet};

const TOKEN_BOUNDARY_CHARS: [char; 6] = [' ', '-', '_', '/', '.', ':'];

fn is_token_boundary(character: char) -> bool {
    TOKEN_BOUNDARY_CHARS.contains(&character)
}

/// Half-open `[start, end)` character range of one word in a candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WordSegment {
    pub start: usize,
    pub end: usize,
}

/// 128-bit presence mask of ASCII scalars used to cheaply prune candidates
/// that cannot contain a token's characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AsciiScalarMask {
    pub low: u64,
    pub high: u64,
}

impl AsciiScalarMask {
    pub fn from_text(text: &str) -> Self {
        let mut low: u64 = 0;
        let mut high: u64 = 0;
        for scalar in text.chars().filter(char::is_ascii) {
            let value = scalar as u32;
            if value < 64 {
                low |= 1u64 << value;
            } else {
                high |= 1u64 << (value - 64);
            }
        }
        Self { low, high }
    }

    /// Number of scalars present here but absent from `candidate`.
    pub fn missing_bit_count(self, candidate: AsciiScalarMask) -> u32 {
        (self.low & !candidate.low).count_ones() + (self.high & !candidate.high).count_ones()
    }
}

/// One normalized query token with precomputed characters, ASCII mask, and
/// score bounds.
#[derive(Debug, Clone)]
pub struct PreparedToken {
    pub normalized_text: String,
    pub characters: Vec<char>,
    pub ascii_mask: AsciiScalarMask,
    /// Whether single-edit (typo) fallback matching applies (length >= 4).
    pub allows_single_edit: bool,
    pub contains_token_boundary_character: bool,
    pub score_upper_bound: i64,
    pub score_upper_bound_without_exact_match: i64,
}

impl PreparedToken {
    pub fn new(normalized_text: String) -> Self {
        let characters: Vec<char> = normalized_text.chars().collect();
        let ascii_mask = AsciiScalarMask::from_text(&normalized_text);
        let allows_single_edit = characters.len() >= 4;
        let contains_token_boundary_character = characters.iter().copied().any(is_token_boundary);
        let count = characters.len() as i64;
        Self {
            normalized_text,
            characters,
            ascii_mask,
            allows_single_edit,
            contains_token_boundary_character,
            score_upper_bound: 8000.max(3500 + count * 300),
            score_upper_bound_without_exact_match: 6799.max(3500 + count * 300),
        }
    }

    /// Fast pre-check: whether `candidate` could possibly match this token
    /// within the allowed edit budget.
    pub fn could_match(&self, candidate: &PreparedCandidateText) -> bool {
        let missing_characters = self.ascii_mask.missing_bit_count(candidate.ascii_mask);
        missing_characters <= u32::from(self.allows_single_edit)
    }
}

/// One normalized candidate string with precomputed characters, word
/// segments, and ASCII mask.
#[derive(Debug, Clone)]
pub struct PreparedCandidateText {
    pub normalized_text: String,
    pub characters: Vec<char>,
    pub word_segments: Vec<WordSegment>,
    pub ascii_mask: AsciiScalarMask,
}

impl PreparedCandidateText {
    pub fn new(normalized_text: String) -> Self {
        let characters: Vec<char> = normalized_text.chars().collect();
        let word_segments = word_segments(&characters);
        let ascii_mask = AsciiScalarMask::from_text(&normalized_text);
        Self {
            normalized_text,
            characters,
            word_segments,
            ascii_mask,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SingleEditWordPrefixEditKind {
    CandidateExtraCharacter,
    TokenExtraCharacter,
    SubstitutedCharacter,
    TransposedCharacters,
}

impl SingleEditWordPrefixEditKind {
    fn base_penalty(self) -> i64 {
        match self {
            Self::CandidateExtraCharacter => 0,
            Self::TokenExtraCharacter => 240,
            Self::TransposedCharacters => 24,
            Self::SubstitutedCharacter => 40,
        }
    }
}

#[derive(Debug, Clone)]
struct SingleEditWordPrefixMatch {
    matched_indices: HashSet<usize>,
    segment_start: usize,
    segment_length: usize,
    prefix_length: usize,
    edit_position: usize,
    edit_kind: SingleEditWordPrefixEditKind,
}

/// A normalized query split into prepared tokens.
#[derive(Debug, Clone)]
pub struct PreparedQuery {
    pub normalized_text: String,
    /// Normalized query tokens joined by single spaces.
    pub normalized_token_text: String,
    pub tokens: Vec<PreparedToken>,
}

impl PreparedQuery {
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

/// Swift: `CommandPaletteFuzzyMatcher.preparedQuery(_:)`.
pub fn prepared_query(query: &str) -> PreparedQuery {
    let normalized_query = normalize_for_search(query);
    let tokens: Vec<PreparedToken> = normalized_query
        .split(' ')
        .filter(|token| !token.is_empty())
        .map(|token| PreparedToken::new(token.to_string()))
        .collect();
    let normalized_token_text = tokens
        .iter()
        .map(|token| token.normalized_text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    PreparedQuery {
        normalized_text: normalized_query,
        normalized_token_text,
        tokens,
    }
}

/// Swift: `CommandPaletteFuzzyMatcher.normalizeForSearch(_:)` — trim,
/// diacritic-fold, case-fold.
///
/// DIVERGENCE: Swift folds with
/// `[.diacriticInsensitive, .caseInsensitive]` under the current locale;
/// dependency-free Rust performs plain Unicode lowercasing only (no diacritic
/// stripping, no locale rules). Mention corpora (paths, skill names) are
/// ASCII-dominant, where the results agree.
pub fn normalize_for_search(text: &str) -> String {
    text.trim_matches(char::is_whitespace)
        .chars()
        .flat_map(char::to_lowercase)
        .collect()
}

/// Swift: `prepareCandidateText(_:)`.
pub fn prepare_candidate_text(candidate: &str) -> Option<PreparedCandidateText> {
    let normalized_candidate = normalize_for_search(candidate);
    if normalized_candidate.is_empty() {
        return None;
    }
    Some(PreparedCandidateText::new(normalized_candidate))
}

/// Swift: `prepareNormalizedCandidateText(_:)`.
pub fn prepare_normalized_candidate_text(
    normalized_candidate: &str,
) -> Option<PreparedCandidateText> {
    if normalized_candidate.is_empty() {
        return None;
    }
    Some(PreparedCandidateText::new(normalized_candidate.to_string()))
}

/// Swift: `score(preparedQuery:preparedCandidate:)`.
pub fn score_prepared_candidate(
    prepared_query: &PreparedQuery,
    prepared_candidate: &PreparedCandidateText,
) -> Option<i64> {
    if prepared_query.is_empty() {
        return Some(0);
    }

    let mut total_score: i64 = 0;
    for token in &prepared_query.tokens {
        if !token.could_match(prepared_candidate) {
            return None;
        }
        total_score += score_token(token, prepared_candidate)?;
    }
    Some(total_score)
}

/// Swift: the full scoring entry point
/// `score(preparedQuery:preparedCandidates:exactCandidateTexts:wholeCandidatePrefixScoreByToken:)`.
pub fn score_prepared_candidates(
    prepared_query: &PreparedQuery,
    prepared_candidates: &[PreparedCandidateText],
    exact_candidate_texts: Option<&HashSet<String>>,
    whole_candidate_prefix_score_by_token: Option<&HashMap<String, i64>>,
) -> Option<i64> {
    if prepared_query.is_empty() {
        return Some(0);
    }
    if prepared_candidates.is_empty() {
        return None;
    }

    let mut total_score: i64 = 0;
    for token in &prepared_query.tokens {
        let has_exact_candidate_text =
            exact_candidate_texts.is_some_and(|texts| texts.contains(&token.normalized_text));
        if token.score_upper_bound == 8000 && has_exact_candidate_text {
            total_score += 8000;
            continue;
        }
        if exact_candidate_texts.is_some() && !has_exact_candidate_text {
            // Swift: `wholeCandidatePrefixScoreByToken?[token] ??
            // bestWholeCandidatePrefixScore(...)` — a provided map with a
            // missing key still falls back to the computed score.
            let prefix_score = whole_candidate_prefix_score_by_token
                .and_then(|scores| scores.get(&token.normalized_text).copied())
                .or_else(|| best_whole_candidate_prefix_score(token, prepared_candidates));
            if let Some(prefix_score) = prefix_score {
                if prefix_score >= token.score_upper_bound_without_exact_match {
                    total_score += prefix_score;
                    continue;
                }
            }
        }

        let mut best_token_score: Option<i64> = None;
        for candidate in prepared_candidates {
            if !token.could_match(candidate) {
                continue;
            }
            let Some(candidate_score) = score_token(token, candidate) else {
                continue;
            };
            best_token_score =
                Some(best_token_score.map_or(candidate_score, |best| best.max(candidate_score)));
            if best_token_score.unwrap_or(0) >= token.score_upper_bound {
                break;
            }
        }
        total_score += best_token_score?;
    }
    Some(total_score)
}

fn best_whole_candidate_prefix_score(
    token: &PreparedToken,
    prepared_candidates: &[PreparedCandidateText],
) -> Option<i64> {
    let mut best_score: Option<i64> = None;
    for candidate in prepared_candidates {
        if !candidate
            .normalized_text
            .starts_with(&token.normalized_text)
        {
            continue;
        }
        let score =
            6800 - 0i64.max(candidate.characters.len() as i64 - token.characters.len() as i64);
        best_score = Some(best_score.map_or(score, |best| best.max(score)));
    }
    best_score
}

const DEFAULT_MAX_PREFIX_LENGTH: usize = 16;

/// Swift: `wholeCandidatePrefixScoreByToken(preparedCandidates:maxPrefixLength: 16)`.
pub fn whole_candidate_prefix_score_by_token(
    prepared_candidates: &[PreparedCandidateText],
) -> HashMap<String, i64> {
    let mut scores: HashMap<String, i64> = HashMap::new();
    for candidate in prepared_candidates {
        let prefix_limit = candidate.characters.len().min(DEFAULT_MAX_PREFIX_LENGTH);
        for prefix_length in 1..=prefix_limit {
            let prefix: String = candidate.characters[..prefix_length].iter().collect();
            let score = 6800 - 0i64.max(candidate.characters.len() as i64 - prefix_length as i64);
            let entry = scores.entry(prefix).or_insert(i64::MIN);
            if score > *entry {
                *entry = score;
            }
        }
    }
    scores
}

/// Swift: `matchCharacterIndices(preparedQuery:preparedCandidate:)`.
pub fn match_character_indices(
    prepared_query: &PreparedQuery,
    prepared_candidate: &PreparedCandidateText,
) -> HashSet<usize> {
    if prepared_query.is_empty() {
        return HashSet::new();
    }

    let candidate_chars = &prepared_candidate.characters;
    let mut matched: HashSet<usize> = HashSet::new();

    for token in &prepared_query.tokens {
        if !token.could_match(prepared_candidate) {
            continue;
        }

        if token.normalized_text == prepared_candidate.normalized_text {
            matched.extend(0..candidate_chars.len());
            continue;
        }

        if starts_with_chars(candidate_chars, &token.characters) {
            matched.extend(0..token.characters.len().min(candidate_chars.len()));
            continue;
        }

        if let Some(start) = char_find(candidate_chars, &token.characters) {
            let end = candidate_chars.len().min(start + token.characters.len());
            matched.extend(start..end);
            continue;
        }

        if token.contains_token_boundary_character {
            if token.characters.len() > 3 {
                continue;
            }
            if let Some(subsequence) = subsequence_match_indices(token, prepared_candidate) {
                matched.extend(subsequence);
            }
            continue;
        }

        if let Some(initialism) = initialism_match_indices(token, prepared_candidate) {
            matched.extend(initialism);
            continue;
        }

        if let Some(stitched) = stitched_word_prefix_match_indices(token, prepared_candidate) {
            matched.extend(stitched);
            continue;
        }

        if let Some(single_edit_prefix) = single_edit_word_prefix_match(
            &token.characters,
            candidate_chars,
            &prepared_candidate.word_segments,
        ) {
            matched.extend(single_edit_prefix.matched_indices);
            continue;
        }

        if token.characters.len() > 3 {
            continue;
        }
        if let Some(subsequence) = subsequence_match_indices(token, prepared_candidate) {
            matched.extend(subsequence);
        }
    }

    matched
}

/// Swift: `tokenCanMatchWithoutSingleEdit(_:preparedCandidate:)` — whether
/// `token` matches `candidate` through any non-typo strategy.
pub fn token_can_match_without_single_edit(
    token: &PreparedToken,
    candidate: &PreparedCandidateText,
) -> bool {
    if token.normalized_text.is_empty() {
        return true;
    }

    if token.normalized_text == candidate.normalized_text {
        return true;
    }
    if starts_with_chars(&candidate.characters, &token.characters) {
        return true;
    }
    if char_find(&candidate.characters, &token.characters).is_some() {
        return true;
    }

    if token.contains_token_boundary_character {
        return token.characters.len() <= 3 && subsequence_score(token, candidate).is_some();
    }

    if best_word_score(&token.characters, candidate).is_some() {
        return true;
    }
    if initialism_score(&token.characters, candidate).is_some() {
        return true;
    }
    if stitched_word_prefix_score(&token.characters, candidate).is_some() {
        return true;
    }
    if token.characters.len() <= 3 && subsequence_score(token, candidate).is_some() {
        return true;
    }
    false
}

fn score_token(token: &PreparedToken, candidate: &PreparedCandidateText) -> Option<i64> {
    if token.normalized_text.is_empty() {
        return Some(0);
    }

    let candidate_chars = &candidate.characters;
    let token_chars = &token.characters;
    if token_chars.len() > candidate_chars.len() {
        return None;
    }

    if token.normalized_text == candidate.normalized_text {
        return Some(8000);
    }
    if starts_with_chars(candidate_chars, token_chars) {
        return Some(6800 - 0i64.max(candidate_chars.len() as i64 - token_chars.len() as i64));
    }

    let mut best_score: Option<i64> = None;
    let consider = |score: i64, best: &mut Option<i64>| {
        *best = Some(best.map_or(score, |current| current.max(score)));
    };

    if !token.contains_token_boundary_character {
        if let Some(word_score) = best_word_score(token_chars, candidate) {
            consider(word_score, &mut best_score);
        }
        if let Some(single_edit_prefix_score) =
            single_edit_word_prefix_score(token_chars, candidate)
        {
            consider(single_edit_prefix_score, &mut best_score);
        }
    }

    if let Some(distance) = char_find(candidate_chars, token_chars) {
        let length_penalty = 0i64.max(candidate_chars.len() as i64 - token_chars.len() as i64);
        let boundary_boost: i64 = if distance == 0 {
            220
        } else if is_token_boundary(candidate_chars[distance - 1]) {
            180
        } else {
            0
        };
        let contains_score = 4200 + boundary_boost - (distance as i64 * 9) - length_penalty;
        consider(contains_score, &mut best_score);
    }

    if !token.contains_token_boundary_character {
        if let Some(initialism) = initialism_score(token_chars, candidate) {
            consider(initialism, &mut best_score);
        }
        if let Some(stitched) = stitched_word_prefix_score(token_chars, candidate) {
            consider(stitched, &mut best_score);
        }
    }

    if token_chars.len() <= 3 {
        if let Some(subsequence) = subsequence_score(token, candidate) {
            consider(subsequence, &mut best_score);
        }
    }

    best_score.map(|score| score.max(1))
}

fn best_word_score(token_chars: &[char], candidate: &PreparedCandidateText) -> Option<i64> {
    if token_chars.is_empty() {
        return None;
    }

    let candidate_chars = &candidate.characters;
    let mut best: Option<i64> = None;
    for segment in &candidate.word_segments {
        let word_length = segment.end - segment.start;
        if token_chars.len() > word_length {
            continue;
        }

        let matches_prefix =
            candidate_chars[segment.start..segment.start + token_chars.len()] == *token_chars;
        if !matches_prefix {
            continue;
        }

        let length_penalty = (word_length as i64 - token_chars.len() as i64) * 6;
        let distance_penalty = segment.start as i64 * 8;
        let trailing_penalty = candidate_chars.len() as i64 - word_length as i64;
        let prefix_score = 5600 - distance_penalty - length_penalty - trailing_penalty;
        best = Some(best.map_or(prefix_score, |current| current.max(prefix_score)));
        if token_chars.len() == word_length {
            let exact_score = 6200 - distance_penalty - trailing_penalty;
            best = Some(best.map_or(exact_score, |current| current.max(exact_score)));
        }
    }

    best
}

fn single_edit_word_prefix_score(
    token_chars: &[char],
    candidate: &PreparedCandidateText,
) -> Option<i64> {
    let matched = single_edit_word_prefix_match(
        token_chars,
        &candidate.characters,
        &candidate.word_segments,
    )?;
    Some(single_edit_word_prefix_score_for_match(
        &matched,
        candidate.characters.len(),
    ))
}

fn single_edit_word_prefix_score_for_match(
    matched: &SingleEditWordPrefixMatch,
    candidate_length: usize,
) -> i64 {
    let length_penalty = 0i64.max(matched.segment_length as i64 - matched.prefix_length as i64) * 6;
    let distance_penalty = matched.segment_start as i64 * 8;
    let trailing_penalty = 0i64.max(candidate_length as i64 - matched.segment_length as i64);
    let edit_position_penalty =
        0i64.max(matched.edit_position as i64 - matched.segment_start as i64) * 10;
    5000 - matched.edit_kind.base_penalty()
        - distance_penalty
        - length_penalty
        - trailing_penalty
        - edit_position_penalty
}

fn initialism_score(token_chars: &[char], candidate: &PreparedCandidateText) -> Option<i64> {
    if token_chars.is_empty() {
        return None;
    }
    let candidate_chars = &candidate.characters;
    let segments = &candidate.word_segments;
    if token_chars.len() > segments.len() {
        return None;
    }

    let mut matched_starts: Vec<usize> = Vec::new();
    let mut search_word_index = 0;

    for &token_char in token_chars {
        let mut found = false;
        while search_word_index < segments.len() {
            let segment = segments[search_word_index];
            search_word_index += 1;
            if candidate_chars[segment.start] == token_char {
                matched_starts.push(segment.start);
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
    }

    let first_start = matched_starts.first().copied().unwrap_or(0) as i64;
    let skipped_words = 0i64.max(segments.len() as i64 - token_chars.len() as i64);
    Some(3000 + (token_chars.len() as i64 * 160) - (first_start * 5) - (skipped_words * 30))
}

fn token_prefix_matches(
    token_chars: &[char],
    token_start: usize,
    length: usize,
    candidate_chars: &[char],
    candidate_start: usize,
) -> bool {
    if token_start + length > token_chars.len() {
        return false;
    }
    if candidate_start + length > candidate_chars.len() {
        return false;
    }
    token_chars[token_start..token_start + length]
        == candidate_chars[candidate_start..candidate_start + length]
}

fn stitched_word_prefix_score(
    token_chars: &[char],
    candidate: &PreparedCandidateText,
) -> Option<i64> {
    if token_chars.len() < 4 {
        return None;
    }
    let candidate_chars = &candidate.characters;
    let segments = &candidate.word_segments;
    if segments.len() < 2 {
        return None;
    }

    let mut memo: HashMap<(usize, usize, usize), Option<i64>> = HashMap::new();

    fn dfs(
        token_chars: &[char],
        candidate_chars: &[char],
        segments: &[WordSegment],
        token_index: usize,
        word_index: usize,
        used_words: usize,
        memo: &mut HashMap<(usize, usize, usize), Option<i64>>,
    ) -> Option<i64> {
        if token_index == token_chars.len() {
            return if used_words >= 2 { Some(0) } else { None };
        }
        if word_index >= segments.len() {
            return None;
        }

        let state = (token_index, word_index, used_words);
        if let Some(cached) = memo.get(&state) {
            return *cached;
        }

        let mut best: Option<i64> = None;
        let remaining_chars = token_chars.len() - token_index;
        for segment_index in word_index..segments.len() {
            let segment = segments[segment_index];
            let segment_length = segment.end - segment.start;
            let max_chunk = segment_length.min(remaining_chars);
            if max_chunk == 0 {
                continue;
            }

            let skipped_words = segment_index - word_index;
            let skip_penalty = skipped_words as i64 * 120;
            for chunk_length in (1..=max_chunk).rev() {
                if !token_prefix_matches(
                    token_chars,
                    token_index,
                    chunk_length,
                    candidate_chars,
                    segment.start,
                ) {
                    continue;
                }
                let Some(suffix_score) = dfs(
                    token_chars,
                    candidate_chars,
                    segments,
                    token_index + chunk_length,
                    segment_index + 1,
                    2.min(used_words + 1),
                    memo,
                ) else {
                    continue;
                };

                let chunk_coverage = chunk_length as i64 * 220;
                let contiguity_bonus: i64 = if segment_index == word_index { 80 } else { 0 };
                let segment_remainder_penalty = (segment_length as i64 - chunk_length as i64) * 9;
                let distance_penalty = segment.start as i64 * 4;
                let chunk_score = chunk_coverage + contiguity_bonus
                    - segment_remainder_penalty
                    - distance_penalty
                    - skip_penalty;
                let total_score = suffix_score + chunk_score;
                best = Some(best.map_or(total_score, |current| current.max(total_score)));
            }
        }

        memo.insert(state, best);
        best
    }

    let stitched_score = dfs(token_chars, candidate_chars, segments, 0, 0, 0, &mut memo)?;
    let length_penalty = 0i64.max(candidate_chars.len() as i64 - token_chars.len() as i64);
    Some(3500 + stitched_score - length_penalty)
}

fn stitched_word_prefix_match_indices(
    token: &PreparedToken,
    candidate: &PreparedCandidateText,
) -> Option<HashSet<usize>> {
    let token_chars = &token.characters;
    let candidate_chars = &candidate.characters;
    if token_chars.len() < 4 {
        return None;
    }

    let segments = &candidate.word_segments;
    if segments.len() < 2 {
        return None;
    }

    let mut token_index = 0;
    let mut next_word_index = 0;
    let mut used_words = 0;
    let mut matched_indices: HashSet<usize> = HashSet::new();

    while token_index < token_chars.len() {
        let remaining_chars = token_chars.len() - token_index;
        let mut found_match = false;

        for (segment_index, &segment) in segments.iter().enumerate().skip(next_word_index) {
            let segment_length = segment.end - segment.start;
            let max_chunk = segment_length.min(remaining_chars);
            if max_chunk == 0 {
                continue;
            }

            for chunk_length in (1..=max_chunk).rev() {
                if !token_prefix_matches(
                    token_chars,
                    token_index,
                    chunk_length,
                    candidate_chars,
                    segment.start,
                ) {
                    continue;
                }

                matched_indices.extend(segment.start..(segment.start + chunk_length));
                token_index += chunk_length;
                next_word_index = segment_index + 1;
                used_words += 1;
                found_match = true;
                break;
            }

            if found_match {
                break;
            }
        }

        if !found_match {
            return None;
        }
    }

    if used_words < 2 {
        return None;
    }
    Some(matched_indices)
}

fn single_edit_word_prefix_match(
    token_chars: &[char],
    candidate_chars: &[char],
    segments: &[WordSegment],
) -> Option<SingleEditWordPrefixMatch> {
    if token_chars.len() < 4 {
        return None;
    }

    let mut best_match: Option<SingleEditWordPrefixMatch> = None;
    let mut best_score: Option<i64> = None;

    for &segment in segments {
        let Some(matched) =
            single_edit_word_prefix_match_in_segment(token_chars, candidate_chars, segment)
        else {
            continue;
        };

        let score = single_edit_word_prefix_score_for_match(&matched, candidate_chars.len());
        if let Some(best) = best_score {
            if score <= best {
                continue;
            }
        }
        best_score = Some(score);
        best_match = Some(matched);
    }

    best_match
}

fn single_edit_word_prefix_match_in_segment(
    token_chars: &[char],
    candidate_chars: &[char],
    segment: WordSegment,
) -> Option<SingleEditWordPrefixMatch> {
    if token_chars.len() < 4 {
        return None;
    }

    let segment_length = segment.end - segment.start;
    if segment_length + 1 < token_chars.len() {
        return None;
    }

    let exact_prefix_length = token_chars.len().min(segment_length);
    let mut mismatch_offset = 0;
    while mismatch_offset < exact_prefix_length
        && candidate_chars[segment.start + mismatch_offset] == token_chars[mismatch_offset]
    {
        mismatch_offset += 1;
    }

    if mismatch_offset == token_chars.len() {
        let prefix_length = token_chars.len() + 1;
        if segment_length < prefix_length {
            return None;
        }
        return Some(SingleEditWordPrefixMatch {
            matched_indices: (segment.start..segment.start + token_chars.len()).collect(),
            segment_start: segment.start,
            segment_length,
            prefix_length,
            edit_position: segment.start + token_chars.len(),
            edit_kind: SingleEditWordPrefixEditKind::CandidateExtraCharacter,
        });
    }

    if mismatch_offset == segment_length {
        let prefix_length = token_chars.len() - 1;
        if prefix_length == 0 {
            return None;
        }
        if token_chars.len() != segment_length + 1 {
            return None;
        }
        return Some(SingleEditWordPrefixMatch {
            matched_indices: (segment.start..segment.start + prefix_length).collect(),
            segment_start: segment.start,
            segment_length,
            prefix_length,
            edit_position: segment.start + prefix_length,
            edit_kind: SingleEditWordPrefixEditKind::TokenExtraCharacter,
        });
    }

    let mismatch_candidate_index = segment.start + mismatch_offset;

    if segment_length > token_chars.len()
        && token_prefix_matches(
            token_chars,
            mismatch_offset,
            token_chars.len() - mismatch_offset,
            candidate_chars,
            mismatch_candidate_index + 1,
        )
    {
        let mut matched_indices: HashSet<usize> =
            (segment.start..segment.start + token_chars.len() + 1).collect();
        matched_indices.remove(&mismatch_candidate_index);
        return Some(SingleEditWordPrefixMatch {
            matched_indices,
            segment_start: segment.start,
            segment_length,
            prefix_length: token_chars.len() + 1,
            edit_position: mismatch_candidate_index,
            edit_kind: SingleEditWordPrefixEditKind::CandidateExtraCharacter,
        });
    }

    if token_chars.len() >= 2
        && segment_length >= token_chars.len() - 1
        && token_prefix_matches(
            token_chars,
            mismatch_offset + 1,
            token_chars.len() - mismatch_offset - 1,
            candidate_chars,
            mismatch_candidate_index,
        )
    {
        return Some(SingleEditWordPrefixMatch {
            matched_indices: (segment.start..segment.start + token_chars.len() - 1).collect(),
            segment_start: segment.start,
            segment_length,
            prefix_length: token_chars.len() - 1,
            edit_position: mismatch_candidate_index,
            edit_kind: SingleEditWordPrefixEditKind::TokenExtraCharacter,
        });
    }

    if segment_length >= token_chars.len()
        && token_prefix_matches(
            token_chars,
            mismatch_offset + 1,
            token_chars.len() - mismatch_offset - 1,
            candidate_chars,
            mismatch_candidate_index + 1,
        )
    {
        let mut matched_indices: HashSet<usize> =
            (segment.start..segment.start + token_chars.len()).collect();
        matched_indices.remove(&mismatch_candidate_index);
        return Some(SingleEditWordPrefixMatch {
            matched_indices,
            segment_start: segment.start,
            segment_length,
            prefix_length: token_chars.len(),
            edit_position: mismatch_candidate_index,
            edit_kind: SingleEditWordPrefixEditKind::SubstitutedCharacter,
        });
    }

    if segment_length >= token_chars.len()
        && mismatch_offset + 1 < token_chars.len()
        && mismatch_candidate_index + 1 < segment.end
        && token_chars[mismatch_offset] == candidate_chars[mismatch_candidate_index + 1]
        && token_chars[mismatch_offset + 1] == candidate_chars[mismatch_candidate_index]
        && token_prefix_matches(
            token_chars,
            mismatch_offset + 2,
            token_chars.len() - mismatch_offset - 2,
            candidate_chars,
            mismatch_candidate_index + 2,
        )
    {
        return Some(SingleEditWordPrefixMatch {
            matched_indices: (segment.start..segment.start + token_chars.len()).collect(),
            segment_start: segment.start,
            segment_length,
            prefix_length: token_chars.len(),
            edit_position: mismatch_candidate_index,
            edit_kind: SingleEditWordPrefixEditKind::TransposedCharacters,
        });
    }

    None
}

fn word_segments(candidate_chars: &[char]) -> Vec<WordSegment> {
    let mut segments: Vec<WordSegment> = Vec::new();
    let mut index = 0;

    while index < candidate_chars.len() {
        while index < candidate_chars.len() && is_token_boundary(candidate_chars[index]) {
            index += 1;
        }
        if index >= candidate_chars.len() {
            break;
        }
        let start = index;
        while index < candidate_chars.len() && !is_token_boundary(candidate_chars[index]) {
            index += 1;
        }
        segments.push(WordSegment { start, end: index });
    }

    segments
}

fn subsequence_score(token: &PreparedToken, candidate: &PreparedCandidateText) -> Option<i64> {
    let token_chars = &token.characters;
    let candidate_chars = &candidate.characters;
    if token_chars.len() > candidate_chars.len() {
        return None;
    }

    let mut search_index: usize = 0;
    let mut previous_match: i64 = -1;
    let mut consecutive_run: i64 = 0;
    let mut score: i64 = 0;

    for &token_char in token_chars {
        let mut found_index: Option<usize> = None;
        while search_index < candidate_chars.len() {
            if candidate_chars[search_index] == token_char {
                found_index = Some(search_index);
                break;
            }
            search_index += 1;
        }
        let matched_index = found_index?;

        score += 90;
        if matched_index == 0 || is_token_boundary(candidate_chars[matched_index - 1]) {
            score += 140;
        }
        if matched_index as i64 == previous_match + 1 {
            consecutive_run += 1;
            score += 200.min(consecutive_run * 45);
        } else {
            consecutive_run = 0;
            score -= 120.min(0i64.max(matched_index as i64 - previous_match - 1) * 4);
        }

        previous_match = matched_index as i64;
        search_index = matched_index + 1;
    }

    score -= 0i64.max(candidate_chars.len() as i64 - token_chars.len() as i64);
    Some(score.max(1))
}

fn subsequence_match_indices(
    token: &PreparedToken,
    candidate: &PreparedCandidateText,
) -> Option<HashSet<usize>> {
    let token_chars = &token.characters;
    let candidate_chars = &candidate.characters;
    if token_chars.len() > candidate_chars.len() {
        return None;
    }

    let mut indices: HashSet<usize> = HashSet::new();
    let mut search_index = 0;

    for &token_char in token_chars {
        let mut found_index: Option<usize> = None;
        while search_index < candidate_chars.len() {
            if candidate_chars[search_index] == token_char {
                found_index = Some(search_index);
                break;
            }
            search_index += 1;
        }
        let match_index = found_index?;
        indices.insert(match_index);
        search_index = match_index + 1;
    }

    Some(indices)
}

fn initialism_match_indices(
    token: &PreparedToken,
    candidate: &PreparedCandidateText,
) -> Option<HashSet<usize>> {
    let token_chars = &token.characters;
    let candidate_chars = &candidate.characters;
    if token_chars.is_empty() {
        return None;
    }

    let segments = &candidate.word_segments;
    if token_chars.len() > segments.len() {
        return None;
    }

    let mut matched: HashSet<usize> = HashSet::new();
    let mut search_word_index = 0;

    for &token_char in token_chars {
        let mut found = false;
        while search_word_index < segments.len() {
            let segment = segments[search_word_index];
            search_word_index += 1;
            if candidate_chars[segment.start] == token_char {
                matched.insert(segment.start);
                found = true;
                break;
            }
        }
        if !found {
            return None;
        }
    }

    Some(matched)
}

fn starts_with_chars(haystack: &[char], needle: &[char]) -> bool {
    needle.len() <= haystack.len() && haystack[..needle.len()] == *needle
}

/// Character-space substring search (Swift `String.range(of:)` analog on
/// scalar arrays; returns the character index of the first occurrence).
fn char_find(haystack: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    (0..=haystack.len() - needle.len())
        .find(|&start| haystack[start..start + needle.len()] == *needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(text: &str) -> PreparedToken {
        PreparedToken::new(text.to_string())
    }

    fn candidate(text: &str) -> PreparedCandidateText {
        PreparedCandidateText::new(normalize_for_search(text))
    }

    #[test]
    fn prepared_query_tokenizes_and_normalizes() {
        let query = prepared_query("  Iterate  PR ");
        assert_eq!(query.normalized_text, "iterate  pr");
        assert_eq!(query.normalized_token_text, "iterate pr");
        assert_eq!(query.tokens.len(), 2);
        assert!(query.tokens[0].allows_single_edit);
        assert!(!query.tokens[1].allows_single_edit);
    }

    #[test]
    fn score_bounds_match_swift_formulas() {
        let short = token("abc");
        assert_eq!(short.score_upper_bound, 8000);
        assert_eq!(short.score_upper_bound_without_exact_match, 6799);
        let long = token("abcdefghijklmnopqr"); // 18 chars: 3500 + 5400 = 8900
        assert_eq!(long.score_upper_bound, 8900);
        assert_eq!(long.score_upper_bound_without_exact_match, 8900);
    }

    #[test]
    fn exact_match_scores_8000() {
        let query = prepared_query("iterate-pr");
        assert_eq!(
            score_prepared_candidate(&query, &candidate("iterate-pr")),
            Some(8000)
        );
    }

    #[test]
    fn prefix_match_scores_6800_minus_extra_length() {
        let query = prepared_query("iterate");
        assert_eq!(
            score_prepared_candidate(&query, &candidate("iterate-pr")),
            Some(6800 - 3)
        );
    }

    #[test]
    fn boundary_token_longer_than_three_rejects_non_substring() {
        // The prefilter behavior behind
        // `testTextBoxMentionCandidateIndexDoesNotReturnUnvalidatedNucleoRows`:
        // a boundary-containing token > 3 chars matches only exact/prefix/contains.
        let query = prepared_query("iterate-pr");
        assert!(!token_can_match_without_single_edit(
            &query.tokens[0],
            &candidate("agent-browser")
        ));
        assert!(token_can_match_without_single_edit(
            &query.tokens[0],
            &candidate("/iterate-pr")
        ));
    }

    #[test]
    fn plain_token_prefilter_requires_word_evidence() {
        // The prefilter behavior behind
        // `testTextBoxMentionCandidateIndexFiltersWeakPartialFuzzyRows`.
        let query = prepared_query("iterate");
        let tok = &query.tokens[0];
        assert!(token_can_match_without_single_edit(
            tok,
            &candidate("/iterate-pr")
        ));
        assert!(!token_can_match_without_single_edit(
            tok,
            &candidate("agent-browser")
        ));
        assert!(!token_can_match_without_single_edit(
            tok,
            &candidate("agent-cli-integration")
        ));
        assert!(!token_can_match_without_single_edit(
            tok,
            &candidate("pi-agent-rust")
        ));
    }

    #[test]
    fn single_edit_typo_still_scores() {
        // "iterte" (dropped 'a') against word "iterate": token-extra/candidate-extra family.
        let query = prepared_query("itrate");
        let scored = score_prepared_candidate(&query, &candidate("iterate"));
        assert!(scored.is_some());
        // But the non-typo prefilter rejects it.
        assert!(!token_can_match_without_single_edit(
            &query.tokens[0],
            &candidate("iterate")
        ));
    }

    #[test]
    fn initialism_matches_word_starts() {
        let query = prepared_query("ab");
        let cand = candidate("agent-browser");
        assert!(token_can_match_without_single_edit(&query.tokens[0], &cand));
        let indices = match_character_indices(&query, &cand);
        assert!(indices.contains(&0)); // 'a' of agent
        assert!(indices.contains(&6)); // 'b' of browser
    }

    #[test]
    fn word_segments_split_on_boundaries() {
        let cand = candidate("/iterate-pr");
        assert_eq!(
            cand.word_segments,
            vec![
                WordSegment { start: 1, end: 8 },
                WordSegment { start: 9, end: 11 }
            ]
        );
    }

    #[test]
    fn empty_query_scores_zero() {
        let query = prepared_query("   ");
        assert!(query.is_empty());
        assert_eq!(
            score_prepared_candidate(&query, &candidate("anything")),
            Some(0)
        );
    }
}
