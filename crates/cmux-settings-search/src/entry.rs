//! `SettingsSearchEntry` and its `Kind` — the searchable records the scorer
//! ranks.
//!
//! Faithful 1:1 port of `enum SettingsSearchEntryKind` (Swift
//! `Sources/SettingsNavigation.swift` lines 263-266) and
//! `struct SettingsSearchEntry` (lines 268-299). Only the fields the scorer and
//! alias builders reference are kept (the spec calls for a minimal transcription
//! of these two types).

use std::collections::HashSet;

use crate::index::SettingsSearchIndex;
use crate::target::SettingsNavigationTarget;

/// `enum SettingsSearchEntryKind` (Swift lines 263-266).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsSearchEntryKind {
    Section,
    Setting,
}

/// `struct SettingsSearchEntry` (Swift lines 268-299).
///
/// The three `normalized*` fields are precomputed in the initializer exactly as
/// Swift does: `normalizedSearchText` is the folded `"<title> <subtitle> <searchText>"`
/// string, `normalizedSearchWords` its tokenization, and `normalizedSearchWordSet`
/// the set of those tokens.
#[derive(Debug, Clone)]
pub struct SettingsSearchEntry {
    pub id: String,
    pub kind: SettingsSearchEntryKind,
    pub target: SettingsNavigationTarget,
    pub title: String,
    pub subtitle: Option<String>,
    pub symbol_name: String,
    pub normalized_search_text: String,
    pub normalized_search_words: Vec<String>,
    pub normalized_search_word_set: HashSet<String>,
}

impl SettingsSearchEntry {
    /// `init(id:kind:target:title:subtitle:symbolName:searchText:)`
    /// (Swift lines 279-298).
    ///
    /// The combined string is `"\(title) \(subtitle ?? "") \(searchText)"`; note
    /// that a `None` subtitle produces a doubled space, exactly as the Swift
    /// `subtitle ?? ""` interpolation does. The tokenizer collapses the empty
    /// run, so this is behaviourally identical downstream — the raw
    /// `normalized_search_text` string still carries the doubled space, matching
    /// Swift byte-for-byte.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<String>,
        kind: SettingsSearchEntryKind,
        target: SettingsNavigationTarget,
        title: impl Into<String>,
        subtitle: Option<String>,
        symbol_name: impl Into<String>,
        search_text: impl AsRef<str>,
    ) -> SettingsSearchEntry {
        let title = title.into();
        let combined = format!(
            "{} {} {}",
            title,
            subtitle.as_deref().unwrap_or(""),
            search_text.as_ref()
        );
        let normalized_search_text = SettingsSearchIndex::normalized(&combined);
        let normalized_search_words =
            SettingsSearchIndex::normalized_tokens(&normalized_search_text);
        let normalized_search_word_set: HashSet<String> =
            normalized_search_words.iter().cloned().collect();
        SettingsSearchEntry {
            id: id.into(),
            kind,
            target,
            title,
            subtitle,
            symbol_name: symbol_name.into(),
            normalized_search_text,
            normalized_search_words,
            normalized_search_word_set,
        }
    }
}
