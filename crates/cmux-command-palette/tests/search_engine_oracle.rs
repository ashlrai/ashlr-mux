//! Oracle integration test — ports the non-nucleo half of
//! `CommandPaletteSearchEngineTests.swift` (:298-983) and the three
//! `optimizedResults` emoji tests from `CommandPaletteEmojiTitleSearchTests.swift`
//! (:5-75).
//!
//! `weighted_reference_score` independently reimplements
//! `commandPaletteWeightedReferenceScore` (`CommandPaletteSearchReferenceScoring.swift`)
//! from the public `cmux_mentions::palette::fuzzy` primitives, and
//! `reference_results` mirrors the Swift test's reference pipeline. The tests
//! assert the reused `SearchEngine::search` agrees with that oracle over the
//! same fixtures, exactly as the Swift suite does.
//!
//! Every nucleo / `searchIndex` / library-guard test in the Swift suite is
//! skipped: those self-skip headless (the FFI dylib is never built), and this
//! port always takes the Swift-engine fallback path.

use std::cell::Cell;
use std::cmp::max;
use std::collections::{HashMap, HashSet};

use cmux_command_palette::{
    CommandPaletteSearchOrchestrator, CommandPaletteSwitcherSearchIndexer,
    CommandPaletteSwitcherSearchMetadata, MetadataDetail,
};
use cmux_mentions::compare::localized_case_insensitive_compare;
use cmux_mentions::palette::corpus::SearchCorpusEntry;
use cmux_mentions::palette::engine::SearchEngine;
use cmux_mentions::palette::fuzzy::{
    match_character_indices, normalize_for_search, prepare_candidate_text,
    prepare_normalized_candidate_text, prepared_query, score_prepared_candidate,
    score_prepared_candidates, PreparedCandidateText, PreparedQuery, WordSegment,
};

#[derive(Clone)]
struct FixtureEntry {
    id: String,
    rank: i64,
    title: String,
    searchable_texts: Vec<String>,
}

impl FixtureEntry {
    fn new(id: &str, rank: i64, title: &str, searchable_texts: &[&str]) -> Self {
        Self {
            id: id.to_string(),
            rank,
            title: title.to_string(),
            searchable_texts: searchable_texts.iter().map(|text| text.to_string()).collect(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct FixtureResult {
    id: String,
    rank: i64,
    title: String,
    score: i64,
    title_match_indices: HashSet<usize>,
}

// ---------------------------------------------------------------------------
// Fixtures (ports of makeCommandEntries / makeSwitcherEntries / … )
// ---------------------------------------------------------------------------

fn make_command_entries(count: usize) -> Vec<FixtureEntry> {
    (0..count)
        .map(|index| {
            let (title, subtitle, keywords): (String, &str, Vec<&str>) = match index % 8 {
                0 => (
                    format!("Rename Workspace {index}"),
                    "Workspace",
                    vec!["rename", "workspace", "title", "project", "switch"],
                ),
                1 => (
                    format!("Rename Tab {index}"),
                    "Tab",
                    vec!["rename", "tab", "surface", "title"],
                ),
                2 => (
                    format!("Open Current Directory in IDE {index}"),
                    "Terminal",
                    vec!["open", "directory", "cwd", "ide", "vscode"],
                ),
                3 => (
                    format!("Toggle Sidebar {index}"),
                    "Layout",
                    vec!["toggle", "sidebar", "layout", "panel"],
                ),
                4 => (
                    format!("Apply Update If Available {index}"),
                    "Global",
                    vec!["apply", "update", "install", "upgrade"],
                ),
                5 => (
                    format!("Restart CLI Listener {index}"),
                    "Global",
                    vec!["restart", "cli", "listener", "socket", "cmux"],
                ),
                6 => (
                    format!("Show Notifications {index}"),
                    "Notifications",
                    vec!["notifications", "inbox", "unread", "alerts"],
                ),
                _ => (
                    format!("Split Browser Right {index}"),
                    "Layout",
                    vec!["split", "browser", "right", "layout", "web"],
                ),
            };
            let mut texts = vec![title.clone(), subtitle.to_string()];
            texts.extend(keywords.into_iter().map(String::from));
            FixtureEntry {
                id: format!("command.{index}"),
                rank: index as i64,
                title,
                searchable_texts: texts,
            }
        })
        .collect()
}

fn make_switcher_entries(count: usize) -> Vec<FixtureEntry> {
    (0..count)
        .map(|index| {
            let title = format!("Workspace {index} Phoenix");
            let keywords = CommandPaletteSwitcherSearchIndexer::new(
                vec![
                    "workspace".to_string(),
                    "switch".to_string(),
                    "go".to_string(),
                    title.clone(),
                ],
                CommandPaletteSwitcherSearchMetadata::new(
                    vec![format!(
                        "/Users/example/dev/cmuxterm-hq/worktrees/feature-{index}-rename-tab"
                    )],
                    vec![format!("feature/rename-tab-{index}")],
                    vec![3000 + (index as i64 % 20), 9200 + (index as i64 % 5)],
                    None,
                ),
                MetadataDetail::Workspace,
            )
            .keywords();
            let mut texts = vec![title.clone(), "Workspace".to_string()];
            texts.extend(keywords);
            FixtureEntry {
                id: format!("workspace.{index}"),
                rank: index as i64,
                title,
                searchable_texts: texts,
            }
        })
        .collect()
}

fn make_large_workspace_switcher_entries(count: usize) -> Vec<FixtureEntry> {
    (0..count)
        .map(|index| {
            let project_slug = format!("project-{index}-cmd-p-search-performance");
            let worktree_slug = format!("feature-{index}-palette-latency");
            let title = format!("Workspace {index} {project_slug}");
            let keywords = CommandPaletteSwitcherSearchIndexer::new(
                vec![
                    "workspace".to_string(),
                    "switch".to_string(),
                    "go".to_string(),
                    "open".to_string(),
                    title.clone(),
                    format!("Window {}", (index % 4) + 1),
                ],
                CommandPaletteSwitcherSearchMetadata::new(
                    vec![
                        format!("/Users/example/dev/cmuxterm-hq/worktrees/{worktree_slug}"),
                        format!("/Users/example/dev/cmuxterm-hq/worktrees/{worktree_slug}/repo"),
                    ],
                    vec![
                        format!("feature/palette-latency-{index}"),
                        format!("task/cmd-p-search-{}", index % 17),
                    ],
                    vec![
                        3000 + (index as i64 % 50),
                        4200 + (index as i64 % 25),
                        9200 + (index as i64 % 10),
                    ],
                    Some(format!("Palette performance fixture {index} for {project_slug}")),
                ),
                MetadataDetail::Workspace,
            )
            .keywords();
            let mut texts = vec![title.clone(), "Workspace".to_string()];
            texts.extend(keywords);
            FixtureEntry {
                id: format!("workspace.large.{index}"),
                rank: index as i64,
                title,
                searchable_texts: texts,
            }
        })
        .collect()
}

fn make_finder_command_entries() -> Vec<FixtureEntry> {
    vec![
        FixtureEntry::new(
            "command.find",
            0,
            "Find...",
            &["Find...", "Search", "find", "search"],
        ),
        FixtureEntry::new(
            "command.finder",
            1,
            "Open Current Directory in Finder",
            &[
                "Open Current Directory in Finder",
                "Terminal",
                "finder",
                "directory",
                "open",
            ],
        ),
        FixtureEntry::new(
            "command.filter",
            2,
            "Filter Sidebar Items",
            &["Filter Sidebar Items", "Sidebar", "filter", "sidebar", "items"],
        ),
    ]
}

fn make_update_command_entries() -> Vec<FixtureEntry> {
    vec![
        FixtureEntry::new(
            "command.checkForUpdates",
            0,
            "Check for Updates",
            &["Check for Updates", "Global", "update", "upgrade", "release"],
        ),
        FixtureEntry::new(
            "command.attemptUpdate",
            1,
            "Attempt Update",
            &[
                "Attempt Update",
                "Global",
                "attempt",
                "check",
                "update",
                "upgrade",
                "release",
            ],
        ),
        FixtureEntry::new(
            "command.applyUpdateIfAvailable",
            2,
            "Apply Update (If Available)",
            &[
                "Apply Update (If Available)",
                "Global",
                "apply",
                "install",
                "update",
                "available",
            ],
        ),
    ]
}

fn make_open_folder_entries() -> Vec<FixtureEntry> {
    vec![
        FixtureEntry::new(
            "palette.newWorkspace",
            0,
            "New Workspace",
            &["New Workspace", "Workspace", "create", "new", "workspace"],
        ),
        FixtureEntry::new(
            "palette.newWindow",
            1,
            "New Window",
            &["New Window", "Window", "create", "new", "window"],
        ),
        FixtureEntry::new(
            "palette.openFolder",
            2,
            "Open Folder...",
            &[
                "Open Folder...",
                "Workspace",
                "open",
                "folder",
                "repository",
                "project",
                "directory",
            ],
        ),
        FixtureEntry::new(
            "palette.openFolderInVSCodeInline",
            3,
            "Open Folder in VS Code (Inline)...",
            &[
                "Open Folder in VS Code (Inline)...",
                "VS Code Inline",
                "open",
                "folder",
                "directory",
                "project",
                "vs",
                "code",
                "inline",
                "editor",
                "browser",
            ],
        ),
    ]
}

// ---------------------------------------------------------------------------
// Engine (optimized) results
// ---------------------------------------------------------------------------

fn search_corpus(entries: &[FixtureEntry]) -> Vec<SearchCorpusEntry<String>> {
    entries
        .iter()
        .map(|entry| {
            SearchCorpusEntry::new(
                entry.id.clone(),
                entry.rank,
                entry.title.clone(),
                entry.searchable_texts.clone(),
            )
        })
        .collect()
}

fn optimized_results(
    entries: &[FixtureEntry],
    query: &str,
    result_limit: Option<usize>,
) -> Vec<FixtureResult> {
    let corpus = search_corpus(entries);
    let engine = SearchEngine::new(corpus.iter().collect());
    engine
        .search(query, result_limit, |_: &String, _| 0, None)
        .into_iter()
        .map(|result| FixtureResult {
            id: result.payload,
            rank: result.rank,
            title: result.title,
            score: result.score,
            title_match_indices: result.title_match_indices,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Independent reference oracle
// ---------------------------------------------------------------------------

/// Independent reimplementation of `commandPaletteNormalizedSearchWordText`
/// (`CommandPaletteSearchCorpusEntry.swift`): join the candidate's word segments
/// that contain at least one alphanumeric scalar.
fn normalized_search_word_text(characters: &[char], segments: &[WordSegment]) -> String {
    let mut words: Vec<String> = Vec::with_capacity(segments.len());
    for segment in segments {
        let word_characters = &characters[segment.start..segment.end];
        if !word_characters.iter().any(|&character| character.is_alphanumeric()) {
            continue;
        }
        words.push(word_characters.iter().collect());
    }
    words.join(" ")
}

/// Independent reimplementation of `commandPaletteTitleWordReferenceScore`.
fn title_word_reference_score(
    prepared_query: &PreparedQuery,
    prepared_title: &PreparedCandidateText,
) -> Option<i64> {
    if prepared_query.is_empty() {
        return None;
    }
    let title_bonus = 2000 * max(1, prepared_query.tokens.len() as i64);
    let title_search_word_text =
        normalized_search_word_text(&prepared_title.characters, &prepared_title.word_segments);
    if title_search_word_text == prepared_title.normalized_text {
        return None;
    }
    if title_search_word_text == prepared_query.normalized_token_text {
        let exact: i64 = prepared_query
            .tokens
            .iter()
            .map(|token| token.score_upper_bound)
            .sum();
        return Some(exact + title_bonus);
    }
    if title_search_word_text.starts_with(&prepared_query.normalized_token_text) {
        let prefix: i64 = prepared_query
            .tokens
            .iter()
            .map(|token| token.score_upper_bound_without_exact_match)
            .sum();
        return Some(prefix + title_bonus);
    }
    None
}

/// Independent reimplementation of `commandPaletteWeightedReferenceScore`.
fn weighted_reference_score(query: &str, title: &str, searchable_texts: &[String]) -> Option<i64> {
    let prepared_query = prepared_query(query);
    let normalized_candidates: Vec<String> = searchable_texts
        .iter()
        .map(|text| normalize_for_search(text))
        .collect();
    let prepared_candidates: Vec<PreparedCandidateText> = normalized_candidates
        .iter()
        .filter_map(|text| prepare_normalized_candidate_text(text))
        .collect();
    let exact_candidate_texts: HashSet<String> = normalized_candidates.iter().cloned().collect();
    let fuzzy_score = score_prepared_candidates(
        &prepared_query,
        &prepared_candidates,
        Some(&exact_candidate_texts),
        None,
    )?;

    let Some(prepared_title) = prepare_candidate_text(title) else {
        return Some(fuzzy_score);
    };
    let Some(title_score) = score_prepared_candidate(&prepared_query, &prepared_title) else {
        return Some(fuzzy_score);
    };

    let title_word_score =
        title_word_reference_score(&prepared_query, &prepared_title).unwrap_or(i64::MIN);
    Some(fuzzy_score.max(title_score + 2000).max(title_word_score))
}

fn reference_results(entries: &[FixtureEntry], query: &str) -> Vec<FixtureResult> {
    let query_is_empty = query.trim_matches(char::is_whitespace).is_empty();
    let mut results: Vec<FixtureResult> = if query_is_empty {
        entries
            .iter()
            .map(|entry| FixtureResult {
                id: entry.id.clone(),
                rank: entry.rank,
                title: entry.title.clone(),
                score: 0,
                title_match_indices: HashSet::new(),
            })
            .collect()
    } else {
        entries
            .iter()
            .filter_map(|entry| {
                let score =
                    weighted_reference_score(query, &entry.title, &entry.searchable_texts)?;
                let pq = prepared_query(query);
                let title_match_indices = prepare_candidate_text(&entry.title)
                    .map(|prepared_title| match_character_indices(&pq, &prepared_title))
                    .unwrap_or_default();
                Some(FixtureResult {
                    id: entry.id.clone(),
                    rank: entry.rank,
                    title: entry.title.clone(),
                    score,
                    title_match_indices,
                })
            })
            .collect()
    };

    results.sort_by(|lhs, rhs| {
        if lhs.score != rhs.score {
            return rhs.score.cmp(&lhs.score); // score descending
        }
        if lhs.rank != rhs.rank {
            return lhs.rank.cmp(&rhs.rank); // rank ascending
        }
        localized_case_insensitive_compare(&lhs.title, &rhs.title)
    });
    results
}

fn ids(results: &[FixtureResult]) -> Vec<&str> {
    results.iter().map(|result| result.id.as_str()).collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Swift: `optimizedSearchMatchesReferencePipeline`.
#[test]
fn optimized_search_matches_reference_pipeline() {
    let command_entries = make_command_entries(96);
    let switcher_entries = make_switcher_entries(64);
    let queries = [
        "rename",
        "rename tab",
        "workspace",
        "feature-12",
        "3004",
        "toggle side",
        "open dir",
        "phoenix",
        "apply update",
    ];

    for query in queries {
        assert_eq!(
            optimized_results(&command_entries, query, None),
            reference_results(&command_entries, query),
            "command corpus mismatch for query {query:?}"
        );
        assert_eq!(
            optimized_results(&switcher_entries, query, None),
            reference_results(&switcher_entries, query),
            "switcher corpus mismatch for query {query:?}"
        );
    }
}

// Swift: `multiTokenSearchCanMatchAcrossTitleAndKeywordFields`.
#[test]
fn multi_token_search_can_match_across_title_and_keyword_fields() {
    let entries = vec![
        FixtureEntry::new("workspace.projectA", 0, "Project A", &["Project A", "Workspace"]),
        FixtureEntry::new("workspace.notes", 1, "Notes", &["Notes", "Workspace"]),
    ];
    let results = optimized_results(&entries, "project workspace", None);
    assert_eq!(results.first().map(|r| r.id.as_str()), Some("workspace.projectA"));
}

// Swift: `limitedSearchReturnsSameTopResultsAsFullSearch`.
#[test]
fn limited_search_returns_same_top_results_as_full_search() {
    let entries = make_large_workspace_switcher_entries(800);
    let queries = [
        "workspace 799",
        "palette latency",
        "feature 401",
        "cmd-p-search",
        "project-642",
        "Window 3",
    ];
    for query in queries {
        let full = optimized_results(&entries, query, None);
        let limited = optimized_results(&entries, query, Some(48));
        let expected: Vec<&FixtureResult> = full.iter().take(48).collect();
        let actual: Vec<&FixtureResult> = limited.iter().collect();
        assert_eq!(actual, expected, "limited search mismatch for query {query:?}");
    }
}

// Swift: `limitedSearchStillFindsDeepWorkspaceMatch`.
#[test]
fn limited_search_still_finds_deep_workspace_match() {
    let entries = make_large_workspace_switcher_entries(5_000);
    let results = optimized_results(&entries, "workspace 4913", Some(10));
    assert_eq!(results.first().map(|r| r.id.as_str()), Some("workspace.large.4913"));
    assert!(results.len() <= 10);
}

// Swift: `limitedSearchReturnsOnlyRequestedResultCountForBroadWorkspaceQuery`.
#[test]
fn limited_search_returns_only_requested_result_count_for_broad_query() {
    let entries = make_large_workspace_switcher_entries(1_200);
    let results = optimized_results(&entries, "workspace", Some(100));
    assert_eq!(results.len(), 100);
    let full = optimized_results(&entries, "workspace", None);
    let expected: Vec<&FixtureResult> = full.iter().take(100).collect();
    let actual: Vec<&FixtureResult> = results.iter().collect();
    assert_eq!(actual, expected);
}

// Swift: `resolvedSearchMatchesReturnFullFinalResultSetWhenUnbounded`.
#[test]
fn resolved_search_matches_return_full_final_result_set_when_unbounded() {
    let entries = make_large_workspace_switcher_entries(150);
    let corpus = search_corpus(&entries);
    let orchestrator = CommandPaletteSearchOrchestrator::new();
    let matches = orchestrator.resolved_search_matches(
        None,
        &corpus,
        "workspace",
        &HashMap::new(),
        false,
        0.0,
        None,
        None,
        None,
    );
    assert_eq!(matches.len(), entries.len());
}

// Swift: `searchCancellationReturnsNoResults` (engine-level, non-nucleo).
#[test]
fn search_cancellation_returns_no_results() {
    let entries = make_command_entries(512);
    let corpus = search_corpus(&entries);
    let engine = SearchEngine::new(corpus.iter().collect());
    let cancellation_checks = Cell::new(0i32);
    let cancel_fn = || {
        cancellation_checks.set(cancellation_checks.get() + 1);
        cancellation_checks.get() >= 4
    };
    let should_cancel: &dyn Fn() -> bool = &cancel_fn;
    let results = engine.search("rename", None, |_: &String, _| 0, Some(should_cancel));
    assert!(results.is_empty());
    assert!(cancellation_checks.get() >= 4);
}

// Swift: `commandSearchPrefersOpenFolderForOpenFolderQuery`.
#[test]
fn command_search_prefers_open_folder_for_open_folder_query() {
    let entries = make_open_folder_entries();
    let results = optimized_results(&entries, "open folder", None);
    assert_eq!(
        ids(&results).into_iter().take(2).collect::<Vec<_>>(),
        vec!["palette.openFolder", "palette.openFolderInVSCodeInline"]
    );
}

// Swift: `searchMatchesSingleOmittedCharacterInCommandWordPrefix`.
#[test]
fn search_matches_single_omitted_character() {
    let entries = make_finder_command_entries();
    let results = optimized_results(&entries, "findr", None);
    assert_eq!(results.first().map(|r| r.id.as_str()), Some("command.finder"));
}

// Swift: `searchMatchesSingleInsertedCharacterInCommandWordPrefix`.
#[test]
fn search_matches_single_inserted_character() {
    let entries = make_finder_command_entries();
    let results = optimized_results(&entries, "findder", None);
    assert_eq!(results.first().map(|r| r.id.as_str()), Some("command.finder"));
}

// Swift: `searchMatchesSingleSubstitutedCharacterInCommandWordPrefix`.
#[test]
fn search_matches_single_substituted_character() {
    let entries = make_finder_command_entries();
    let results = optimized_results(&entries, "fander", None);
    assert_eq!(results.first().map(|r| r.id.as_str()), Some("command.finder"));
}

// Swift: `searchMatchesSingleTransposedCharacterInCommandWordPrefix`.
#[test]
fn search_matches_single_transposed_character() {
    let entries = make_finder_command_entries();
    let results = optimized_results(&entries, "fidner", None);
    assert_eq!(results.first().map(|r| r.id.as_str()), Some("command.finder"));
}

// Swift: `searchRejectsMultipleEditsInCommandWordPrefix`.
#[test]
fn search_rejects_multiple_edits() {
    let entries = make_finder_command_entries();
    let results = optimized_results(&entries, "fadnr", None);
    assert_ne!(results.first().map(|r| r.id.as_str()), Some("command.finder"));
}

// Swift: `searchPrefersTitleMatchOverKeywordOnlyMatchForCheckQuery`.
#[test]
fn search_prefers_title_match_over_keyword_only_match_for_check_query() {
    let entries = make_update_command_entries();
    let results = optimized_results(&entries, "check", None);
    assert_eq!(
        ids(&results).into_iter().take(2).collect::<Vec<_>>(),
        vec!["command.checkForUpdates", "command.attemptUpdate"]
    );
}

// Swift emoji: `searchPrefersEmojiPrefixedFullTitleWordsOverPartialTitlePrefix`.
#[test]
fn search_prefers_emoji_prefixed_full_title_words_over_partial_title_prefix() {
    let entries = vec![
        FixtureEntry::new(
            "workspace.fullTitleMatch",
            20,
            "🧪 Command Palette",
            &["🧪 Command Palette", "Workspace", "workspace", "switch", "go"],
        ),
        FixtureEntry::new(
            "workspace.partialTitlePrefix",
            0,
            "Command Palette Archive",
            &["Command Palette Archive", "Workspace", "workspace", "switch", "go"],
        ),
    ];
    let results = optimized_results(&entries, "command palette", Some(5));
    assert_eq!(results.first().map(|r| r.id.as_str()), Some("workspace.fullTitleMatch"));
}

// Swift emoji: `searchPrefersEmojiPrefixedTitleWordPrefixOverHiddenTokenMatches`.
#[test]
fn search_prefers_emoji_prefixed_title_word_prefix_over_hidden_token_matches() {
    let entries = vec![
        FixtureEntry::new(
            "workspace.titlePrefixMatch",
            20,
            "🧪 Command Palette Archive",
            &["🧪 Command Palette Archive", "Workspace", "workspace", "switch", "go"],
        ),
        FixtureEntry::new(
            "workspace.hiddenTokenMatches",
            0,
            "Other Workspace",
            &[
                "Other Workspace",
                "Workspace",
                "workspace",
                "switch",
                "go",
                "command",
                "palette",
            ],
        ),
    ];
    let results = optimized_results(&entries, "command palette", Some(5));
    assert_eq!(results.first().map(|r| r.id.as_str()), Some("workspace.titlePrefixMatch"));
}

// Swift emoji: `searchPrefersEmojiPrefixedFullTitleWordsWithRepeatedQuerySpaces`.
#[test]
fn search_prefers_emoji_prefixed_full_title_words_with_repeated_query_spaces() {
    let entries = vec![
        FixtureEntry::new(
            "workspace.fullTitleMatch",
            20,
            "🧪 Command Palette",
            &["🧪 Command Palette", "Workspace", "workspace", "switch", "go"],
        ),
        FixtureEntry::new(
            "workspace.hiddenTokenMatches",
            0,
            "Other Workspace",
            &["Other Workspace", "Workspace", "command", "palette"],
        ),
    ];
    let results = optimized_results(&entries, "command  palette", Some(5));
    assert_eq!(results.first().map(|r| r.id.as_str()), Some("workspace.fullTitleMatch"));
}
