//! `SettingsSearchIndex` scoring core.
//!
//! Faithful 1:1 port of the `extension SettingsSearchIndex` in
//! `Sources/SettingsSearchAliases.swift` (lines 179-319), plus the `setting`
//! builder and the `sectionID` / `settingID` helpers from
//! `Sources/SettingsNavigation.swift`.
//!
//! Divergences (sanctioned platform swaps), all bounded to the ASCII/English
//! domain that every alias, title, and test query in this repo lives in:
//!
//! - [`SettingsSearchIndex::normalized`] ports Foundation's
//!   `text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current)`.
//!   There is no ICU folding available headless, so this lowercases via
//!   `str::to_lowercase` (Unicode-aware) and strips a curated set of common
//!   Latin diacritics (Latin-1 Supplement / combining marks). Full Unicode case
//!   folding (e.g. `ß` -> `ss`) and diacritic folding of scripts outside that
//!   curated set are not reproduced. Every string in the ported data set is
//!   ASCII, so this is behaviourally identical for all in-repo entries.
//! - The token delimiter test ports Swift's
//!   `whitespacesAndNewlines || punctuationCharacters` scalar membership. Rust's
//!   std has no Unicode general-category table, so punctuation is classified
//!   exactly for the ASCII range (per the Unicode P categories) and treated as
//!   non-delimiter outside ASCII. Whitespace uses `char::is_whitespace`.
//! - The scalar-membership helpers ([`SettingsSearchIndex::contains_at_word_boundary`],
//!   [`SettingsSearchIndex::is_subsequence`], grapheme counts in
//!   [`SettingsSearchIndex::is_light_typo`]) operate over `char` (Unicode scalar)
//!   rather than Swift `Character` (grapheme cluster). Identical for the
//!   diacritic-folded ASCII domain here.
//! - Swift marks the scalar helpers `private`; they are exposed `pub` here so the
//!   oracle tests (and downstream verifiers) can assert their values directly.

use crate::aliases::SettingsSearchAliasIndex;
use crate::entry::{SettingsSearchEntry, SettingsSearchEntryKind};
use crate::target::SettingsNavigationTarget;

/// Namespace mirroring Swift's `enum SettingsSearchIndex`.
pub struct SettingsSearchIndex;

impl SettingsSearchIndex {
    /// `sectionID(for:)` — `"section:\(target.rawValue)"`.
    pub fn section_id(target: SettingsNavigationTarget) -> String {
        format!("section:{}", target.raw_value())
    }

    /// `settingID(for:idSuffix:)` — `"setting:\(target.rawValue):\(idSuffix)"`.
    pub fn setting_id(target: SettingsNavigationTarget, id_suffix: &str) -> String {
        format!("setting:{}:{}", target.raw_value(), id_suffix)
    }

    /// `setting(_:_:_:_:)` builder (Swift `SettingsSearchAliases.swift` lines
    /// 184-199). Composes the searchable string from the target rawValue, the
    /// id suffix, the section `searchText`, the row `searchText`, and the row
    /// aliases, exactly as Swift interpolates them.
    pub fn setting(
        target: SettingsNavigationTarget,
        id_suffix: &str,
        title: &str,
        search_text: &str,
    ) -> SettingsSearchEntry {
        let composed = format!(
            "{} {} {} {} {}",
            target.raw_value(),
            id_suffix,
            target.search_text(),
            search_text,
            SettingsSearchAliasIndex::aliases(target, id_suffix)
        );
        SettingsSearchEntry::new(
            Self::setting_id(target, id_suffix),
            SettingsSearchEntryKind::Setting,
            target,
            title,
            Some(target.title().to_string()),
            target.symbol_name(),
            composed,
        )
    }

    /// `normalized(_:)` (Swift line 180). See the module divergence note about
    /// the Foundation folding approximation.
    pub fn normalized(text: &str) -> String {
        let lowered = text.to_lowercase();
        let mut result = String::with_capacity(lowered.len());
        for character in lowered.chars() {
            if is_combining_mark(character) {
                continue;
            }
            match strip_diacritic(character) {
                Some(base) => result.push(base),
                None => result.push(character),
            }
        }
        result
    }

    /// `normalizedTokens(for:)` (Swift lines 201-210).
    ///
    /// Re-folds the query, then splits on runs of characters that are entirely
    /// whitespace/newline or punctuation, dropping empty subsequences (Swift's
    /// `split` defaults to `omittingEmptySubsequences: true`).
    pub fn normalized_tokens(query: &str) -> Vec<String> {
        let folded = Self::normalized(query);
        let mut tokens: Vec<String> = Vec::new();
        let mut current = String::new();
        for character in folded.chars() {
            if is_token_delimiter(character) {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            } else {
                current.push(character);
            }
        }
        if !current.is_empty() {
            tokens.push(current);
        }
        tokens
    }

    /// `normalizedQueryTokens(for:)` (Swift lines 212-214). Drops search
    /// stop-words from the query tokens only.
    pub fn normalized_query_tokens(query: &str) -> Vec<String> {
        Self::normalized_tokens(query)
            .into_iter()
            .filter(|token| !Self::is_search_stop_word(token))
            .collect()
    }

    /// `isSearchStopWord(_:)` (Swift lines 216-223).
    pub fn is_search_stop_word(token: &str) -> bool {
        matches!(token, "setting" | "settings" | "preference" | "preferences")
    }

    /// `matchScore(entry:query:tokens:)` (Swift lines 225-248).
    ///
    /// `query` is the already-normalized, whitespace-trimmed query string (the
    /// Swift call site passes `normalized(query).trimmingCharacters(in: .whitespacesAndNewlines)`).
    /// Returns `None` when any token fails to match the entry (excluding it);
    /// otherwise `Some(score)` where a *lower* score sorts earlier.
    pub fn match_score(
        entry: &SettingsSearchEntry,
        query: &str,
        tokens: &[String],
    ) -> Option<i32> {
        let mut score: i32 = 0;
        for token in tokens {
            match Self::match_score_token(
                token,
                &entry.normalized_search_text,
                &entry.normalized_search_words,
                &entry.normalized_search_word_set,
            ) {
                Some(token_score) => score += token_score,
                None => return None,
            }
        }

        let title = Self::normalized(&entry.title);
        if title == query {
            score -= 1_000;
        }
        if title.starts_with(query) {
            score -= 800;
        }
        if Self::contains_at_word_boundary(query, &title) {
            score -= 700;
        }
        if entry.normalized_search_text.starts_with(query) {
            score -= 600;
        }
        if Self::contains_at_word_boundary(query, &entry.normalized_search_text) {
            score -= 500;
        }
        if entry.normalized_search_text.contains(query) {
            score -= 400;
        }
        if entry.kind == SettingsSearchEntryKind::Section {
            score += 25;
        }
        Some(score)
    }

    /// Inner `matchScore(token:text:words:wordSet:)` (Swift lines 250-259).
    /// Combines exact-word / prefix / word-boundary / contains / light-typo /
    /// subsequence bonuses. Returns `None` when the token matches nothing.
    pub fn match_score_token(
        token: &str,
        text: &str,
        words: &[String],
        word_set: &std::collections::HashSet<String>,
    ) -> Option<i32> {
        if word_set.contains(token) {
            return Some(0);
        }
        if words.iter().any(|word| word.starts_with(token)) {
            return Some(10);
        }
        if Self::contains_at_word_boundary(token, text) {
            return Some(20);
        }
        if text.contains(token) {
            return Some(30);
        }
        if words.iter().any(|word| Self::is_light_typo(token, word)) {
            return Some(50);
        }
        if words.iter().any(|word| Self::is_subsequence(token, word)) {
            return Some(60);
        }
        if Self::is_subsequence(token, text) {
            return Some(80);
        }
        None
    }

    /// `containsAtWordBoundary(_:in:)` (Swift lines 261-275). True when `needle`
    /// appears in `haystack` at the start of the string or immediately after a
    /// character that is neither a letter nor a number. An empty needle is a
    /// boundary match (Swift returns `true`).
    pub fn contains_at_word_boundary(needle: &str, haystack: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let mut search_start = 0usize;
        while let Some(offset) = haystack[search_start..].find(needle) {
            let match_start = search_start + offset;
            if match_start == 0 {
                return true;
            }
            // The character immediately preceding the match.
            let previous = haystack[..match_start]
                .chars()
                .next_back()
                .expect("match_start > 0 implies a preceding character");
            if !previous.is_alphabetic() && !previous.is_numeric() {
                return true;
            }
            // Advance past the whole match, matching Swift's `range.upperBound`.
            search_start = match_start + needle.len();
        }
        false
    }

    /// `isSubsequence(_:of:)` (Swift lines 277-285). True when `needle`'s
    /// characters appear in order (not necessarily contiguously) within
    /// `haystack`. An empty needle is always a subsequence.
    pub fn is_subsequence(needle: &str, haystack: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let mut needle_chars = needle.chars().peekable();
        for character in haystack.chars() {
            match needle_chars.peek() {
                Some(&expected) if expected == character => {
                    needle_chars.next();
                    if needle_chars.peek().is_none() {
                        return true;
                    }
                }
                _ => {}
            }
        }
        // Reached the end without consuming every needle character.
        false
    }

    /// `isLightTypo(_:comparedTo:)` (Swift lines 287-294). Both strings must be
    /// at least 4 characters; the allowed edit distance is 2 when the shorter is
    /// at least 6 characters, otherwise 1; the length delta must not exceed the
    /// allowance; then the bounded edit distance must be within the allowance.
    pub fn is_light_typo(token: &str, word: &str) -> bool {
        let token_count = token.chars().count() as i32;
        let word_count = word.chars().count() as i32;
        if token_count < 4 || word_count < 4 {
            return false;
        }
        let allowed_distance = if token_count.min(word_count) >= 6 { 2 } else { 1 };
        if (token_count - word_count).abs() > allowed_distance {
            return false;
        }
        Self::edit_distance(token, word, allowed_distance) <= allowed_distance
    }

    /// `editDistance(_:_:maximum:)` (Swift lines 296-318). Row-based Levenshtein
    /// with the two early exits preserved exactly: a length delta larger than
    /// `maximum` short-circuits to `maximum + 1`, and any full row whose minimum
    /// exceeds `maximum` short-circuits to `maximum + 1`.
    pub fn edit_distance(lhs: &str, rhs: &str, maximum: i32) -> i32 {
        let left: Vec<char> = lhs.chars().collect();
        let right: Vec<char> = rhs.chars().collect();
        let left_len = left.len();
        let right_len = right.len();

        if (left_len as i32 - right_len as i32).abs() > maximum {
            return maximum + 1;
        }

        let mut previous: Vec<i32> = (0..=right_len as i32).collect();
        let mut current: Vec<i32> = vec![0; right_len + 1];

        for left_index in 1..=left_len {
            current[0] = left_index as i32;
            let mut row_minimum = current[0];
            for right_index in 1..=right_len {
                let cost = if left[left_index - 1] == right[right_index - 1] {
                    0
                } else {
                    1
                };
                current[right_index] = (previous[right_index] + 1)
                    .min(current[right_index - 1] + 1)
                    .min(previous[right_index - 1] + cost);
                row_minimum = row_minimum.min(current[right_index]);
            }
            if row_minimum > maximum {
                return maximum + 1;
            }
            std::mem::swap(&mut previous, &mut current);
        }

        previous[right_len]
    }
}

/// Whether a character is a token delimiter: whitespace/newline, or punctuation.
/// Mirrors the Swift closure
/// `whitespacesAndNewlines.contains(scalar) || punctuationCharacters.contains(scalar)`.
fn is_token_delimiter(character: char) -> bool {
    character.is_whitespace() || is_unicode_punctuation(character)
}

/// ASCII-range membership of the Unicode general punctuation categories
/// (Pc, Pd, Ps, Pe, Pi, Pf, Po). Notably this EXCLUDES the ASCII symbol
/// characters `$ + < = > ^ ` | ~` (categories Sc/Sm/Sk), which POSIX would call
/// punctuation but Foundation's `CharacterSet.punctuationCharacters` does not.
/// Non-ASCII characters return `false` (see module divergence note).
fn is_unicode_punctuation(character: char) -> bool {
    matches!(
        character,
        '!' | '"'
            | '#'
            | '%'
            | '&'
            | '\''
            | '('
            | ')'
            | '*'
            | ','
            | '-'
            | '.'
            | '/'
            | ':'
            | ';'
            | '?'
            | '@'
            | '\\'
            | '['
            | ']'
            | '_'
            | '{'
            | '}'
    )
}

/// Unicode combining diacritical marks (U+0300..=U+036F). Dropped by
/// [`SettingsSearchIndex::normalized`] to approximate diacritic-insensitive
/// folding for already-decomposed input.
fn is_combining_mark(character: char) -> bool {
    ('\u{0300}'..='\u{036F}').contains(&character)
}

/// Maps a curated set of precomposed lowercase Latin letters with diacritics to
/// their base letter, approximating Foundation's `.diacriticInsensitive` folding
/// over the common Latin range. `normalized` lowercases first, so only lowercase
/// forms are needed here.
fn strip_diacritic(character: char) -> Option<char> {
    let base = match character {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ð' | 'ď' | 'đ' => 'd',
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' | 'ħ' => 'h',
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' | 'ŧ' => 't',
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        _ => return None,
    };
    Some(base)
}
