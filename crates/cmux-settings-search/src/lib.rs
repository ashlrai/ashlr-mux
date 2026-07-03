//! `cmux-settings-search` — faithful Rust port of the cmux macOS settings search
//! scorer.
//!
//! Ports (source of truth `Sources/SettingsSearchAliases.swift`, with the two
//! keyed enums from `Sources/SettingsNavigation.swift`):
//!
//! - [`SettingsNavigationTarget`] — the top-level settings sections.
//! - [`SettingsSearchEntry`] / [`SettingsSearchEntryKind`] — searchable records.
//! - [`SettingsSearchAliasIndex`] — the section/setting alias keyword tables.
//! - [`SettingsSearchIndex`] — the normalization + scoring core
//!   (`normalized`, `normalized_tokens`, `normalized_query_tokens`,
//!   `is_search_stop_word`, `match_score`, the inner `match_score_token`,
//!   `contains_at_word_boundary`, `is_subsequence`, `is_light_typo`,
//!   `edit_distance`), plus the `setting` builder and the `section_id` /
//!   `setting_id` helpers.
//!
//! This crate is pure logic — no UserDefaults, AppKit, or process spawning — so
//! every test runs headless. Sanctioned platform divergences (Foundation folding
//! approximation, ASCII-scoped punctuation classification, scalar-vs-grapheme
//! iteration, `localized(...)` collapsing to the English default, and the
//! headless-empty `keyboardShortcutActionAliases`) are documented at each site.

pub mod aliases;
pub mod entry;
pub mod index;
pub mod target;

pub use aliases::{setting_aliases, SettingsSearchAliasIndex};
pub use entry::{SettingsSearchEntry, SettingsSearchEntryKind};
pub use index::SettingsSearchIndex;
pub use target::SettingsNavigationTarget;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn words_and_set(text: &str) -> (Vec<String>, HashSet<String>) {
        let words = SettingsSearchIndex::normalized_tokens(text);
        let set: HashSet<String> = words.iter().cloned().collect();
        (words, set)
    }

    // ----- normalized -----

    #[test]
    fn normalized_lowercases_ascii() {
        assert_eq!(SettingsSearchIndex::normalized("MixedCASE"), "mixedcase");
        assert_eq!(SettingsSearchIndex::normalized("App.Language"), "app.language");
    }

    #[test]
    fn normalized_strips_precomposed_diacritics() {
        assert_eq!(SettingsSearchIndex::normalized("Café"), "cafe");
        assert_eq!(SettingsSearchIndex::normalized("naïve"), "naive");
        assert_eq!(SettingsSearchIndex::normalized("Ñoño"), "nono");
    }

    #[test]
    fn normalized_strips_combining_marks() {
        // "e" + U+0301 COMBINING ACUTE ACCENT folds to "e".
        assert_eq!(SettingsSearchIndex::normalized("e\u{0301}"), "e");
    }

    #[test]
    fn normalized_preserves_non_latin_letters() {
        // Han characters are letters: kept verbatim, no diacritic folding.
        assert_eq!(SettingsSearchIndex::normalized("日本語"), "日本語");
    }

    // ----- tokenizer -----

    #[test]
    fn tokens_split_on_dot_and_underscore_and_hyphen() {
        assert_eq!(
            SettingsSearchIndex::normalized_tokens("app.language"),
            vec!["app", "language"]
        );
        assert_eq!(
            SettingsSearchIndex::normalized_tokens("CMUX_PORT"),
            vec!["cmux", "port"]
        );
        assert_eq!(
            SettingsSearchIndex::normalized_tokens("sidebar-font-size"),
            vec!["sidebar", "font", "size"]
        );
    }

    #[test]
    fn tokens_keep_ascii_symbol_characters() {
        // `+` and `=` are Unicode Sm (not punctuation), so they stay in tokens.
        assert_eq!(
            SettingsSearchIndex::normalized_tokens("Show Branch + Directory"),
            vec!["show", "branch", "+", "directory"]
        );
        assert_eq!(SettingsSearchIndex::normalized_tokens("a=b"), vec!["a=b"]);
    }

    #[test]
    fn tokens_collapse_empty_runs() {
        assert_eq!(
            SettingsSearchIndex::normalized_tokens("  leading  and   trailing  "),
            vec!["leading", "and", "trailing"]
        );
    }

    #[test]
    fn tokens_refold_diacritics() {
        assert_eq!(
            SettingsSearchIndex::normalized_tokens("Café Münchén"),
            vec!["cafe", "munchen"]
        );
    }

    // ----- stop words -----

    #[test]
    fn stop_words_dropped_from_query_tokens_only() {
        assert!(SettingsSearchIndex::is_search_stop_word("setting"));
        assert!(SettingsSearchIndex::is_search_stop_word("settings"));
        assert!(SettingsSearchIndex::is_search_stop_word("preference"));
        assert!(SettingsSearchIndex::is_search_stop_word("preferences"));
        assert!(!SettingsSearchIndex::is_search_stop_word("appearance"));

        // normalized_query_tokens filters them; normalized_tokens keeps them.
        assert_eq!(
            SettingsSearchIndex::normalized_query_tokens("settings appearance preferences theme"),
            vec!["appearance", "theme"]
        );
        assert_eq!(
            SettingsSearchIndex::normalized_tokens("settings appearance preferences theme"),
            vec!["settings", "appearance", "preferences", "theme"]
        );
    }

    // ----- inner match_score_token: one assert per bonus branch -----

    #[test]
    fn token_exact_word_scores_zero() {
        let (words, set) = words_and_set("theme light");
        assert_eq!(
            SettingsSearchIndex::match_score_token("theme", "theme light", &words, &set),
            Some(0)
        );
    }

    #[test]
    fn token_prefix_scores_ten() {
        let (words, set) = words_and_set("theme");
        assert_eq!(
            SettingsSearchIndex::match_score_token("the", "theme", &words, &set),
            Some(10)
        );
    }

    #[test]
    fn token_word_boundary_scores_twenty() {
        // `b` sits after the symbol `+` (a non-letter, non-number, non-delimiter),
        // so `a+b` is a single word yet `b` matches at a word boundary in the text.
        let (words, set) = words_and_set("a+b");
        assert_eq!(words, vec!["a+b"]);
        assert_eq!(
            SettingsSearchIndex::match_score_token("b", "a+b", &words, &set),
            Some(20)
        );
    }

    #[test]
    fn token_mid_word_contains_scores_thirty() {
        let (words, set) = words_and_set("appearance");
        assert_eq!(
            SettingsSearchIndex::match_score_token("ppe", "appearance", &words, &set),
            Some(30)
        );
    }

    #[test]
    fn token_light_typo_scores_fifty() {
        // "sysfem" vs word "system": single substitution, allowed distance 2.
        let (words, set) = words_and_set("system");
        assert_eq!(
            SettingsSearchIndex::match_score_token("sysfem", "system", &words, &set),
            Some(50)
        );
    }

    #[test]
    fn token_subsequence_of_word_scores_sixty() {
        // "sym" is a subsequence of "system" but not a prefix/contains/typo.
        let (words, set) = words_and_set("system");
        assert_eq!(
            SettingsSearchIndex::match_score_token("sym", "system", &words, &set),
            Some(60)
        );
    }

    #[test]
    fn token_subsequence_of_text_scores_eighty() {
        // "apang" is a subsequence spanning "app language" but of no single word.
        let (words, set) = words_and_set("app language");
        assert_eq!(
            SettingsSearchIndex::match_score_token("apang", "app language", &words, &set),
            Some(80)
        );
    }

    #[test]
    fn token_no_match_returns_none() {
        let (words, set) = words_and_set("system");
        assert_eq!(
            SettingsSearchIndex::match_score_token("xyz", "system", &words, &set),
            None
        );
    }

    // ----- contains_at_word_boundary -----

    #[test]
    fn word_boundary_empty_needle_is_true() {
        assert!(SettingsSearchIndex::contains_at_word_boundary("", "anything"));
    }

    #[test]
    fn word_boundary_at_start_is_true() {
        assert!(SettingsSearchIndex::contains_at_word_boundary("app", "appearance"));
    }

    #[test]
    fn word_boundary_after_space_is_true() {
        assert!(SettingsSearchIndex::contains_at_word_boundary("theme", "app theme x"));
    }

    #[test]
    fn word_boundary_after_symbol_is_true() {
        assert!(SettingsSearchIndex::contains_at_word_boundary("b", "a+b"));
    }

    #[test]
    fn word_boundary_mid_word_is_false() {
        // "pear" occurs inside "appearance" preceded by a letter -> not a boundary.
        assert!(!SettingsSearchIndex::contains_at_word_boundary("pear", "appearance"));
        assert!(!SettingsSearchIndex::contains_at_word_boundary("a", "cat"));
    }

    // ----- is_subsequence -----

    #[test]
    fn subsequence_cases() {
        assert!(SettingsSearchIndex::is_subsequence("", "x"));
        assert!(SettingsSearchIndex::is_subsequence("ace", "abcde"));
        assert!(!SettingsSearchIndex::is_subsequence("aec", "abcde"));
        assert!(!SettingsSearchIndex::is_subsequence("abcdef", "abc"));
    }

    // ----- is_light_typo -----

    #[test]
    fn light_typo_one_insertion() {
        assert!(SettingsSearchIndex::is_light_typo("color", "colour"));
    }

    #[test]
    fn light_typo_requires_min_length_four() {
        assert!(!SettingsSearchIndex::is_light_typo("cat", "cats"));
    }

    #[test]
    fn light_typo_rejects_two_substitutions_when_allowed_one() {
        // len 4/4 -> allowed 1; two substitutions -> distance 2 -> rejected.
        assert!(!SettingsSearchIndex::is_light_typo("abcd", "abyz"));
    }

    #[test]
    fn light_typo_allows_two_when_both_at_least_six() {
        assert!(SettingsSearchIndex::is_light_typo("sysfem", "system"));
    }

    #[test]
    fn light_typo_length_delta_gate() {
        // |6 - 9| = 3 exceeds allowed 2 -> rejected before edit distance.
        assert!(!SettingsSearchIndex::is_light_typo("abcdef", "abcdefxyz"));
    }

    // ----- edit_distance -----

    #[test]
    fn edit_distance_classic_value() {
        assert_eq!(SettingsSearchIndex::edit_distance("kitten", "sitting", 10), 3);
    }

    #[test]
    fn edit_distance_length_delta_early_exit() {
        // |2 - 6| = 4 > maximum 1 -> returns maximum + 1 = 2, no DP performed.
        let result = SettingsSearchIndex::edit_distance("ab", "abcdef", 1);
        assert_eq!(result, 2);
        assert!(result > 1);
    }

    #[test]
    fn edit_distance_row_minimum_early_exit() {
        // Equal length, all characters differ (true distance 4). Bounded at 1,
        // the second row's minimum exceeds the maximum -> returns maximum + 1 = 2.
        let result = SettingsSearchIndex::edit_distance("abcd", "wxyz", 1);
        assert_eq!(result, 2);
        assert!(result > 1);
    }

    #[test]
    fn edit_distance_exact_within_bound() {
        assert_eq!(SettingsSearchIndex::edit_distance("color", "colour", 1), 1);
    }

    // ----- outer match_score with bonuses -----

    fn appearance_entry() -> SettingsSearchEntry {
        SettingsSearchEntry::new(
            "setting:app:appearance",
            SettingsSearchEntryKind::Setting,
            SettingsNavigationTarget::App,
            "Appearance",
            Some("App".to_string()),
            "gearshape",
            "theme light dark system",
        )
    }

    #[test]
    fn match_score_word_boundary_and_contains_bonuses() {
        let entry = appearance_entry();
        // normalized_search_text = "appearance app theme light dark system".
        // token "theme" -> 0; then -500 (boundary in text) and -400 (contains).
        let score = SettingsSearchIndex::match_score(&entry, "theme", &["theme".to_string()]);
        assert_eq!(score, Some(-900));
    }

    #[test]
    fn match_score_exact_title_stacks_all_title_bonuses() {
        let entry = appearance_entry();
        // token 0; title==query (-1000), title prefix (-800), title boundary (-700),
        // text prefix (-600), text boundary (-500), text contains (-400).
        let score =
            SettingsSearchIndex::match_score(&entry, "appearance", &["appearance".to_string()]);
        assert_eq!(score, Some(-4000));
    }

    #[test]
    fn match_score_section_bonus_adds_twenty_five() {
        let entry = SettingsSearchEntry::new(
            "section:app",
            SettingsSearchEntryKind::Section,
            SettingsNavigationTarget::App,
            "App",
            None,
            "gearshape",
            "settings general",
        );
        // normalized_search_text = "app  settings general" (doubled space from
        // the None subtitle). query "app": token 0; -1000/-800/-700/-600/-500/-400
        // then +25 for the section kind.
        let score = SettingsSearchIndex::match_score(&entry, "app", &["app".to_string()]);
        assert_eq!(score, Some(-3975));
    }

    #[test]
    fn match_score_returns_none_when_a_token_is_absent() {
        let entry = appearance_entry();
        let score = SettingsSearchIndex::match_score(
            &entry,
            "theme zzzz",
            &["theme".to_string(), "zzzz".to_string()],
        );
        assert_eq!(score, None);
    }

    // ----- ranking over several entries (descending relevance = ascending score) -----

    #[test]
    fn ranking_orders_by_ascending_score_then_offset() {
        // Three entries with a shared "theme" keyword but differing relevance.
        let exact_title = SettingsSearchEntry::new(
            "setting:browser:theme",
            SettingsSearchEntryKind::Setting,
            SettingsNavigationTarget::Browser,
            "Theme",
            Some("Browser".to_string()),
            "globe",
            "browser theme color scheme",
        );
        let keyword_only = SettingsSearchEntry::new(
            "setting:app:appearance",
            SettingsSearchEntryKind::Setting,
            SettingsNavigationTarget::App,
            "Appearance",
            Some("App".to_string()),
            "gearshape",
            "theme light dark system",
        );
        let subsequence_only = SettingsSearchEntry::new(
            "setting:app:telemetry",
            SettingsSearchEntryKind::Setting,
            SettingsNavigationTarget::App,
            "Telemetry",
            Some("App".to_string()),
            "gearshape",
            // Contains "t..h..e..m..e" only as a spanning subsequence, no "theme".
            "the metrics enable",
        );

        let query = "theme";
        let tokens = SettingsSearchIndex::normalized_query_tokens(query);
        let normalized_query = SettingsSearchIndex::normalized(query);
        let normalized_query = normalized_query.trim();

        let entries = [&exact_title, &keyword_only, &subsequence_only];
        let mut ranked: Vec<(usize, &str, i32)> = entries
            .iter()
            .enumerate()
            .filter_map(|(offset, entry)| {
                SettingsSearchIndex::match_score(entry, normalized_query, &tokens)
                    .map(|score| (offset, entry.id.as_str(), score))
            })
            .collect();
        ranked.sort_by(|lhs, rhs| lhs.2.cmp(&rhs.2).then(lhs.0.cmp(&rhs.0)));

        let order: Vec<&str> = ranked.iter().map(|item| item.1).collect();
        assert_eq!(
            order,
            vec![
                "setting:browser:theme",
                "setting:app:appearance",
                "setting:app:telemetry",
            ]
        );
        // Strictly descending relevance = strictly ascending score.
        assert!(ranked[0].2 < ranked[1].2);
        assert!(ranked[1].2 < ranked[2].2);
    }

    // ----- alias table + builders -----

    #[test]
    fn section_aliases_are_verbatim() {
        assert_eq!(
            SettingsSearchAliasIndex::section_aliases(SettingsNavigationTarget::Terminal),
            "shell scrollback scrollbar scroll bar ghostty tty pty"
        );
        assert_eq!(
            SettingsSearchAliasIndex::section_aliases(SettingsNavigationTarget::Reset),
            "factory defaults restore clear preferences"
        );
    }

    #[test]
    fn setting_aliases_lookup_hits_and_misses() {
        assert_eq!(
            setting_aliases("account:account"),
            Some(
                "auth authentication login logout signin sign-in signout sign-out email user profile stack team"
            )
        );
        assert_eq!(
            setting_aliases("reset:reset-all"),
            Some("factory reset restore defaults clear preferences")
        );
        assert_eq!(setting_aliases("app:does-not-exist"), None);
    }

    #[test]
    fn aliases_missing_key_is_empty_string() {
        assert_eq!(
            SettingsSearchAliasIndex::aliases(SettingsNavigationTarget::App, "no-such-suffix"),
            ""
        );
    }

    #[test]
    fn aliases_keyboard_shortcuts_appends_empty_action_list() {
        // Divergence: KeyboardShortcutSettings is unavailable headless, so the
        // action-label join is empty; the trailing space from the Swift
        // "\(aliases) \(actions)" interpolation is preserved.
        let expected = format!(
            "{} ",
            setting_aliases("keyboardShortcuts:shortcuts").unwrap()
        );
        assert_eq!(
            SettingsSearchAliasIndex::aliases(
                SettingsNavigationTarget::KeyboardShortcuts,
                "shortcuts"
            ),
            expected
        );
    }

    #[test]
    fn aliases_non_shortcut_key_has_no_trailing_space() {
        assert_eq!(
            SettingsSearchAliasIndex::aliases(SettingsNavigationTarget::Browser, "theme"),
            "browser.theme web page theme color scheme light dark system"
        );
    }

    // ----- setting builder + id helpers -----

    #[test]
    fn id_helpers_format() {
        assert_eq!(
            SettingsSearchIndex::section_id(SettingsNavigationTarget::SettingsJson),
            "section:settingsJSON"
        );
        assert_eq!(
            SettingsSearchIndex::setting_id(SettingsNavigationTarget::App, "appearance"),
            "setting:app:appearance"
        );
    }

    #[test]
    fn setting_builder_composes_search_text_and_matches() {
        let entry = SettingsSearchIndex::setting(
            SettingsNavigationTarget::App,
            "appearance",
            "Appearance",
            "theme light dark system",
        );
        assert_eq!(entry.id, "setting:app:appearance");
        assert_eq!(entry.kind, SettingsSearchEntryKind::Setting);
        assert_eq!(entry.subtitle.as_deref(), Some("App"));
        assert_eq!(entry.symbol_name, "gearshape");

        // The composed searchText folds in the alias table value, so an
        // alias-only keyword ("scheme") still matches the entry.
        assert!(entry.normalized_search_word_set.contains("scheme"));
        assert!(entry.normalized_search_word_set.contains("appearance"));
        // rawValue token "app" is present from the leading interpolation.
        assert!(entry.normalized_search_word_set.contains("app"));
    }

    #[test]
    fn target_round_trips_raw_value() {
        for target in SettingsNavigationTarget::ALL_CASES {
            assert_eq!(
                SettingsNavigationTarget::from_raw_value(target.raw_value()),
                Some(target)
            );
        }
        assert_eq!(SettingsNavigationTarget::from_raw_value("nope"), None);
        assert_eq!(
            SettingsNavigationTarget::TextBox.raw_value(),
            "textBox"
        );
        assert_eq!(
            SettingsNavigationTarget::SettingsJson.raw_value(),
            "settingsJSON"
        );
    }
}
