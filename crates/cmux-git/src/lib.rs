//! cmux-git — pure, filesystem-free git metadata parsing.
//!
//! Headless port of the pure subset of `GitMetadataService` from the canonical
//! macOS `Packages/macOS/CmuxGit` package: GitHub repo-slug detection from git
//! remote URLs, `git remote -v` slug extraction with upstream>origin>rest
//! ordering + dedup, git-config string parsing (inline-comment stripping,
//! `*`/`**` glob match), and repository-resolution path logic (see
//! [`repo_resolution`]: the pure `gitdir:`/`commondir` pointer parsers, the
//! `standardizedFileURL` lexical normalizer, and the `includeIf "gitdir:"`
//! matcher). The filesystem *shell* of resolve/includeIf — the upward fs-walk,
//! `fileExists` probing, and every `String(contentsOf:)` read — stays excluded
//! as host wiring.
//!
//! Ports:
//! - `GitMetadataService+Slugs.swift`: `githubRepositorySlug(fromRemoteURL:)`,
//!   `githubRepositorySlugs(fromGitRemoteVOutput:)`, `githubRemotePriority`,
//!   `normalizedGitHubRepositorySlug`.
//! - `GitMetadataService+Config.swift`: `gitRemoteVLines(fromConfig:)` (+ the
//!   `gitConfigRemoteName` / `gitConfigUnquotedValue` helpers it needs),
//!   `gitConfigLineRemovingInlineComment`, `gitConfigGlobMatches` (+ the
//!   `gitConfigGlobRegexPattern` / `gitConfigGlobCharacterClass` helpers).
//! - `GitMetadataService+Index.swift` (see [`git_index`]): the pure `index`
//!   parser `gitIndexSnapshot` + `gitIndexContentSignature` (FNV-1a), v4
//!   strip-length varint, big-endian readers, path validation, and
//!   comparable-mode mapping. The lstat/submodule/file-read I/O shell is
//!   excluded.

pub mod git_index;
pub mod pr_selection;
pub mod repo_resolution;

pub use git_index::{
    git_index_comparable_mode, git_index_content_signature, git_index_snapshot,
    is_valid_index_entry_path, read_big_endian_u16, read_big_endian_u32,
    read_git_index_v4_path_strip_length, GitIndexEntryStat, GitIndexSnapshot,
};
pub use pr_selection::{
    github_timestamp_date, is_badge_candidate, is_stale_merged, normalized_branch_name,
    preferred_pull_request, pull_request_map_by_normalized_branch, GitHubPullRequestProbeItem,
    PullRequestStatus, MERGED_BADGE_STALE_AFTER,
};
pub use repo_resolution::{
    expanded_pattern, git_common_directory, git_config_include_if_condition,
    git_directory_from_dot_git_file, gitdir_pattern_matches, include_if_condition_matches,
    should_stop_repository_search, standardize_posix_path, ResolvedGitRepository,
};

use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Character-class predicates matching Foundation's CharacterSet semantics.
// ---------------------------------------------------------------------------

/// Whether `c` is a newline per Swift's `Character.isNewline` (the set used by
/// `String.split(whereSeparator:)` and `CharacterSet.newlines`).
fn is_newline(c: char) -> bool {
    matches!(
        c,
        '\n' | '\u{0B}' | '\u{0C}' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}'
    )
}

/// Trims leading/trailing horizontal whitespace, matching Swift's
/// `trimmingCharacters(in: .whitespaces)` (whitespace, but not newlines).
fn trim_config_whitespace(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() && !is_newline(c))
}

// ---------------------------------------------------------------------------
// GitMetadataService+Slugs.swift
// ---------------------------------------------------------------------------

/// Sort priority for a remote name: `upstream` (0), `origin` (1), other (2).
///
/// Port of `githubRemotePriority(_:)`.
pub fn github_remote_priority(remote_name: &str) -> i32 {
    match remote_name.to_lowercase().as_str() {
        "upstream" => 0,
        "origin" => 1,
        _ => 2,
    }
}

/// Normalizes a `owner/name(...)` path into a `owner/name` slug, dropping a
/// trailing `.git`, or `None` when it lacks both components.
///
/// Port of `normalizedGitHubRepositorySlug(_:)`. Swift's
/// `split(separator: "/")` omits empty subsequences, so runs of `/` collapse.
pub fn normalized_github_repository_slug(raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let components: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
    if components.len() < 2 {
        return None;
    }
    let owner = components[0];
    let mut repo = components[1];
    if let Some(stripped) = repo.strip_suffix(".git") {
        repo = stripped;
    }
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

/// The `owner/name` slug for a GitHub remote URL (SSH, HTTPS, HTTP, git, or
/// `ssh://` forms), or `None` for a non-GitHub URL.
///
/// Port of `githubRepositorySlug(fromRemoteURL:)`. The explicit prefix set is
/// matched by hand (git's `git@host:owner/repo` scp syntax and the `scheme://`
/// forms). Only the generic host-fallback path routes through the `url` crate;
/// per the port spec we do NOT feed scp-like `git@` strings to `url` because it
/// parses them differently from Foundation's `URL(string:).host`.
pub fn github_repository_slug(from_remote_url: &str) -> Option<String> {
    // `trimmingCharacters(in: .whitespacesAndNewlines)`.
    let trimmed = from_remote_url.trim();
    if trimmed.is_empty() {
        return None;
    }

    const GITHUB_PREFIXES: [&str; 5] = [
        "git@github.com:",
        "ssh://git@github.com/",
        "https://github.com/",
        "http://github.com/",
        "git://github.com/",
    ];
    for prefix in GITHUB_PREFIXES {
        if let Some(path) = trimmed.strip_prefix(prefix) {
            return normalized_github_repository_slug(path);
        }
    }

    // Host-fallback path: parse the URL and accept only `github.com`.
    // DIVERGENCE: Swift uses Foundation `URL(string:)` + `.host`/`.path`; we use
    // the `url` crate. A non-parseable string (e.g. a non-github scp URL such as
    // `git@gitlab.com:foo/bar.git`) yields `Err` here → `None`, matching Swift's
    // `URL(string:)` returning a URL whose `.host` is not `github.com`.
    let url = url::Url::parse(trimmed).ok()?;
    let host = url.host_str()?.to_lowercase();
    if host != "github.com" {
        return None;
    }
    normalized_github_repository_slug(url.path())
}

/// Extracts ordered, de-duplicated GitHub `owner/name` slugs from a
/// `git remote -v`-style output.
///
/// Port of `githubRepositorySlugs(fromGitRemoteVOutput:)`. Only `(fetch)` lines
/// for `github.com` remotes contribute; results are ordered `upstream`, then
/// `origin`, then other remotes alphabetically, de-duplicated by slug.
pub fn github_repository_slugs(from_git_remote_v_output: &str) -> Vec<String> {
    let mut slug_by_remote: HashMap<String, String> = HashMap::new();

    for line in from_git_remote_v_output.split(is_newline) {
        // `line.split(whereSeparator: \.isWhitespace)` omits empty subsequences.
        let parts: Vec<&str> = line
            .split(|c: char| c.is_whitespace())
            .filter(|s| !s.is_empty())
            .collect();
        if parts.len() < 3 {
            continue;
        }

        let remote_name = parts[0];
        let remote_url = parts[1];
        let remote_kind = parts[2];
        if remote_kind != "(fetch)" {
            continue;
        }
        let Some(repo_slug) = github_repository_slug(remote_url) else {
            continue;
        };
        // Last write wins per remote name, matching the Swift dictionary assign.
        slug_by_remote.insert(remote_name.to_string(), repo_slug);
    }

    let mut ordered_remote_names: Vec<&str> = slug_by_remote.keys().map(String::as_str).collect();
    ordered_remote_names.sort_by(|a, b| {
        let lhs_priority = github_remote_priority(a);
        let rhs_priority = github_remote_priority(b);
        lhs_priority.cmp(&rhs_priority).then_with(|| a.cmp(b))
    });

    let mut ordered_slugs: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for remote_name in ordered_remote_names {
        let repo_slug = slug_by_remote[remote_name].clone();
        if seen.insert(repo_slug.clone()) {
            ordered_slugs.push(repo_slug);
        }
    }
    ordered_slugs
}

// ---------------------------------------------------------------------------
// GitMetadataService+Config.swift
// ---------------------------------------------------------------------------

/// The remote name from a `[remote "<name>"]` section header, or `None`.
///
/// Port of `gitConfigRemoteName(fromSectionHeader:)`. The section name is
/// case-insensitive per git; the quoted subsection (the remote name) is
/// case-sensitive and extracted verbatim.
fn git_config_remote_name(header: &str) -> Option<String> {
    let prefix = "[remote \"";
    let suffix = "\"]";
    let prefix_len = prefix.chars().count();
    let suffix_len = suffix.chars().count();

    // Swift: `header.count > prefix.count + suffix.count - 1` (at least one
    // char between the quotes); the negated guard is `< prefix + suffix`.
    if header.chars().count() < prefix_len + suffix_len {
        return None;
    }
    if !header.to_lowercase().starts_with(prefix) {
        return None;
    }
    if !header.ends_with(suffix) {
        return None;
    }

    let chars: Vec<char> = header.chars().collect();
    let name: String = chars[prefix_len..chars.len() - suffix_len].iter().collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Strips surrounding double quotes from a config value, honoring backslash
/// escapes inside the quotes.
///
/// Port of `gitConfigUnquotedValue(_:)`.
fn git_config_unquoted_value(value: &str) -> String {
    let trimmed = trim_config_whitespace(value);
    let chars: Vec<char> = trimmed.chars().collect();

    let is_quoted =
        chars.len() >= 2 && chars.first() == Some(&'"') && chars.last() == Some(&'"');
    if !is_quoted {
        return trimmed.to_string();
    }

    let mut result = String::new();
    let mut is_escaped = false;
    for &character in &chars[1..chars.len() - 1] {
        if is_escaped {
            result.push(character);
            is_escaped = false;
            continue;
        }
        if character == '\\' {
            is_escaped = true;
            continue;
        }
        result.push(character);
    }
    if is_escaped {
        result.push('\\');
    }
    result
}

/// Removes a trailing inline `#`/`;` comment from a config line, ignoring
/// `#`/`;` inside double-quoted strings.
///
/// Port of `gitConfigLineRemovingInlineComment(_:)`.
pub fn git_config_line_removing_inline_comment(line: &str) -> String {
    let mut result = String::new();
    let mut is_inside_double_quoted_string = false;
    let mut is_escaped = false;
    let mut previous_was_whitespace = true;

    for character in line.chars() {
        if is_escaped {
            result.push(character);
            is_escaped = false;
            previous_was_whitespace = character.is_whitespace();
            continue;
        }

        if is_inside_double_quoted_string && character == '\\' {
            result.push(character);
            is_escaped = true;
            previous_was_whitespace = false;
            continue;
        }

        if character == '"' {
            result.push(character);
            is_inside_double_quoted_string = !is_inside_double_quoted_string;
            previous_was_whitespace = false;
            continue;
        }

        if !is_inside_double_quoted_string
            && previous_was_whitespace
            && (character == '#' || character == ';')
        {
            break;
        }

        result.push(character);
        previous_was_whitespace = character.is_whitespace();
    }

    result
}

/// Parses a single config string into `git remote -v` fetch lines.
///
/// Port of `gitRemoteVLines(fromConfig:)` (the test-only config entry point;
/// the fs-fixture include/includeIf traversal is intentionally excluded).
pub fn git_remote_v_lines(config: &str) -> Vec<String> {
    let mut current_remote_name: Option<String> = None;
    let mut lines: Vec<String> = Vec::new();

    for raw_line in config.split(is_newline) {
        let stripped = git_config_line_removing_inline_comment(raw_line);
        let line = trim_config_whitespace(&stripped);

        if line.starts_with('[') && line.ends_with(']') {
            current_remote_name = git_config_remote_name(line);
            continue;
        }

        let Some(ref current_remote_name) = current_remote_name else {
            continue;
        };

        // `line.split(separator: "=", maxSplits: 1)` — split on the first `=`.
        // Swift additionally omits empty subsequences; an empty key or empty
        // value is filtered out below (key != "url" / empty remote URL), so a
        // simple first-`=` split reproduces the same outcome.
        let Some(eq) = line.find('=') else {
            continue;
        };
        let key = trim_config_whitespace(&line[..eq]);
        let raw_value = trim_config_whitespace(&line[eq + 1..]);
        if key.to_lowercase() != "url" {
            continue;
        }

        let remote_url = git_config_unquoted_value(raw_value);
        if remote_url.is_empty() {
            continue;
        }
        lines.push(format!("{current_remote_name}\t{remote_url} (fetch)\n"));
    }

    lines
}

// ---------------------------------------------------------------------------
// Glob matching (gitConfigGlobMatches + regex/char-class helpers).
//
// DIVERGENCE: Swift translates the glob into an `NSRegularExpression`
// (anchored `^…$`) and falls back to POSIX `fnmatch` only if that regex fails
// to compile. This crate has no regex dependency (Cargo.toml pins `url` only),
// so we translate the same glob grammar into a token stream and match it with
// a small backtracking matcher that reproduces the anchored full-match
// semantics token-for-token: `*` → `[^/]*`, `**` → `.*`, `**/` → `(?:.*/)?`,
// `?` → `[^/]`, `[…]` → a character class. The `fnmatch` fallback path is
// unreachable for these patterns and is therefore omitted.
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum GlobToken {
    /// A literal character (matches that exact char, including `/`).
    Literal(char),
    /// `?` → `[^/]` (a single non-slash character).
    AnyNonSlashOne,
    /// `*` → `[^/]*` (zero or more non-slash characters).
    AnyNonSlashStar,
    /// `**` not followed by `/` → `.*` (anything, including `/`).
    AnyStar,
    /// `**/` → `(?:.*/)?` (empty, or any prefix ending in `/`).
    AnyPathSlashOptional,
    /// `[…]` → a character class of literal members and `a-z`-style ranges.
    CharClass { negated: bool, members: Vec<char> },
}

/// Parses a `[…]` character class out of the glob, or `None` when unterminated.
///
/// Port of `gitConfigGlobCharacterClass(_:startIndex:)`. Git uses `!` for
/// negation; a leading `^` is a literal member (not negation).
fn glob_char_class(chars: &[char], start_index: usize) -> Option<(bool, Vec<char>, usize)> {
    if start_index >= chars.len() || chars[start_index] != '[' {
        return None;
    }

    let mut index = start_index + 1;
    if index >= chars.len() {
        return None;
    }

    let mut negated = false;
    let mut members: Vec<char> = Vec::new();
    if chars[index] == '!' {
        negated = true;
        index += 1;
    } else if chars[index] == '^' {
        members.push('^'); // literal `^` member
        index += 1;
    }

    if index < chars.len() && chars[index] == ']' {
        members.push(']'); // literal `]` as the first member
        index += 1;
    }

    let mut has_terminator = false;
    while index < chars.len() {
        let character = chars[index];
        if character == ']' {
            has_terminator = true;
            index += 1;
            break;
        }
        // In Swift `\`, `[` are regex-escaped but remain literal members; the
        // token model stores the raw character in every case.
        members.push(character);
        index += 1;
    }

    if !has_terminator {
        return None;
    }
    Some((negated, members, index))
}

/// Translates a git-style glob into a token stream.
///
/// Port of `gitConfigGlobRegexPattern(_:)` (as tokens rather than a regex
/// string), treating `/` as a path separator.
fn glob_tokens(pattern: &str) -> Vec<GlobToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens: Vec<GlobToken> = Vec::new();
    let mut index = 0;

    while index < chars.len() {
        let character = chars[index];

        if character == '*' {
            let mut star_count = 1;
            while index + star_count < chars.len() && chars[index + star_count] == '*' {
                star_count += 1;
            }
            index += star_count;

            if star_count >= 2 {
                if index < chars.len() && chars[index] == '/' {
                    index += 1;
                    tokens.push(GlobToken::AnyPathSlashOptional);
                } else {
                    tokens.push(GlobToken::AnyStar);
                }
            } else {
                tokens.push(GlobToken::AnyNonSlashStar);
            }
            continue;
        }

        if character == '?' {
            tokens.push(GlobToken::AnyNonSlashOne);
            index += 1;
            continue;
        }

        if character == '[' {
            if let Some((negated, members, end_index)) = glob_char_class(&chars, index) {
                tokens.push(GlobToken::CharClass { negated, members });
                index = end_index;
                continue;
            }
        }

        tokens.push(GlobToken::Literal(character));
        index += 1;
    }

    tokens
}

/// Whether `c` is matched by a `[…]` character class, honoring `a-z` ranges and
/// `!`-negation (mirrors regex character-class semantics).
fn class_matches(members: &[char], negated: bool, c: char) -> bool {
    let mut matched = false;
    let mut i = 0;
    while i < members.len() {
        // `x-y` is a range when `-` is neither first nor last member.
        if i + 2 < members.len() && members[i + 1] == '-' {
            let lo = members[i];
            let hi = members[i + 2];
            if lo <= c && c <= hi {
                matched = true;
            }
            i += 3;
        } else {
            if members[i] == c {
                matched = true;
            }
            i += 1;
        }
    }
    matched != negated
}

/// Anchored full-match of a token stream against an input, with backtracking.
fn glob_match_tokens(tokens: &[GlobToken], input: &[char]) -> bool {
    let Some(token) = tokens.first() else {
        return input.is_empty();
    };
    let rest = &tokens[1..];

    match token {
        GlobToken::Literal(c) => {
            !input.is_empty() && input[0] == *c && glob_match_tokens(rest, &input[1..])
        }
        GlobToken::AnyNonSlashOne => {
            !input.is_empty() && input[0] != '/' && glob_match_tokens(rest, &input[1..])
        }
        GlobToken::AnyNonSlashStar => {
            let mut count = 0;
            while count < input.len() && input[count] != '/' {
                count += 1;
            }
            for k in (0..=count).rev() {
                if glob_match_tokens(rest, &input[k..]) {
                    return true;
                }
            }
            false
        }
        GlobToken::AnyStar => {
            for k in (0..=input.len()).rev() {
                if glob_match_tokens(rest, &input[k..]) {
                    return true;
                }
            }
            false
        }
        GlobToken::AnyPathSlashOptional => {
            // Option 1: match nothing.
            if glob_match_tokens(rest, input) {
                return true;
            }
            // Option 2: any prefix ending in `/`.
            for j in 1..=input.len() {
                if input[j - 1] == '/' && glob_match_tokens(rest, &input[j..]) {
                    return true;
                }
            }
            false
        }
        GlobToken::CharClass { negated, members } => {
            !input.is_empty()
                && class_matches(members, *negated, input[0])
                && glob_match_tokens(rest, &input[1..])
        }
    }
}

/// Matches a value against a git glob pattern (`*`, `**`, `?`, `[…]`), treating
/// `/` as a path separator.
///
/// Port of `gitConfigGlobMatches(_:pattern:caseInsensitive:)`.
pub fn git_config_glob_matches(value: &str, pattern: &str, case_insensitive: bool) -> bool {
    let candidate_value = if case_insensitive {
        value.to_lowercase()
    } else {
        value.to_string()
    };
    let candidate_pattern = if case_insensitive {
        pattern.to_lowercase()
    } else {
        pattern.to_string()
    };

    let tokens = glob_tokens(&candidate_pattern);
    let input: Vec<char> = candidate_value.chars().collect();
    glob_match_tokens(&tokens, &input)
}

// ---------------------------------------------------------------------------
// Tests — ported PURE (fs-free) cases from GitSlugAndConfigTests.swift.
// The GitRepositoryFixture / resolveGitRepository (includeIf/gitdir) cases are
// intentionally excluded as host wiring.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// `parsesGitHubRemoteForms` — SSH/HTTPS/HTTP/git/ssh:// remote forms table.
    #[test]
    fn parses_github_remote_forms() {
        let urls = [
            "git@github.com:manaflow-ai/cmux.git",
            "ssh://git@github.com/manaflow-ai/cmux.git",
            "https://github.com/manaflow-ai/cmux.git",
            "http://github.com/manaflow-ai/cmux",
            "git://github.com/manaflow-ai/cmux.git",
            "https://github.com/manaflow-ai/cmux",
        ];
        for url in urls {
            assert_eq!(
                github_repository_slug(url).as_deref(),
                Some("manaflow-ai/cmux"),
                "url: {url}"
            );
        }
    }

    /// `ignoresNonGitHubRemotes`.
    #[test]
    fn ignores_non_github_remotes() {
        assert_eq!(github_repository_slug("git@gitlab.com:foo/bar.git"), None);
        assert_eq!(github_repository_slug(""), None);
    }

    /// `ordersRemotesUpstreamThenOriginThenRest`.
    #[test]
    fn orders_remotes_upstream_then_origin_then_rest() {
        let output = "origin\thttps://github.com/me/fork.git (fetch)\n\
                      upstream\thttps://github.com/owner/repo.git (fetch)\n\
                      zeta\thttps://github.com/zeta/zeta.git (fetch)";
        assert_eq!(
            github_repository_slugs(output),
            vec![
                "owner/repo".to_string(),
                "me/fork".to_string(),
                "zeta/zeta".to_string(),
            ]
        );
    }

    /// `deduplicatesIdenticalSlugs`.
    #[test]
    fn deduplicates_identical_slugs() {
        let output = "origin\thttps://github.com/owner/repo.git (fetch)\n\
                      mirror\tgit@github.com:owner/repo.git (fetch)";
        assert_eq!(
            github_repository_slugs(output),
            vec!["owner/repo".to_string()]
        );
    }

    /// `ignoresPushOnlyLines`.
    #[test]
    fn ignores_push_only_lines() {
        let output = "origin\thttps://github.com/owner/repo.git (push)\n";
        assert!(github_repository_slugs(output).is_empty());
    }

    // MARK: config parsing

    fn slugs_from_config(config: &str) -> Vec<String> {
        github_repository_slugs(&git_remote_v_lines(config).join(""))
    }

    /// `remoteVLinesParseUrlFromConfig`.
    #[test]
    fn remote_v_lines_parse_url_from_config() {
        let config = "[remote \"origin\"]\n\
                      \turl = https://github.com/owner/repo.git\n\
                      \tfetch = +refs/heads/*:refs/remotes/origin/*";
        assert_eq!(slugs_from_config(config), vec!["owner/repo".to_string()]);
    }

    /// `configSectionAndKeyNamesAreCaseInsensitive` (pure — uses `slugs(fromConfig:)`).
    #[test]
    fn config_section_and_key_names_are_case_insensitive() {
        let config = "[Remote \"origin\"]\n\
                          URL = https://github.com/manaflow-ai/cmux.git";
        assert_eq!(
            slugs_from_config(config),
            vec!["manaflow-ai/cmux".to_string()]
        );
    }

    /// `inlineCommentsAreStrippedOutsideQuotes`.
    #[test]
    fn inline_comments_are_stripped_outside_quotes() {
        let line = git_config_line_removing_inline_comment("\turl = value # trailing comment");
        assert_eq!(
            line.trim_matches(|c: char| c == ' ' || c == '\t'),
            "url = value"
        );
    }

    /// `inlineCommentInsideQuotesIsKept`.
    #[test]
    fn inline_comment_inside_quotes_is_kept() {
        let line = git_config_line_removing_inline_comment("\turl = \"a#b\"");
        assert!(line.contains("a#b"));
    }

    /// `globMatchesSingleSegmentWildcard`.
    #[test]
    fn glob_matches_single_segment_wildcard() {
        assert!(git_config_glob_matches("/a/b", "/a/*", false));
        assert!(!git_config_glob_matches("/a/b/c", "/a/*", false));
    }

    /// `globDoubleStarMatchesAcrossSegments`.
    #[test]
    fn glob_double_star_matches_across_segments() {
        assert!(git_config_glob_matches("/a/b/c/d", "/a/**/d", false));
    }

    // Extra coverage for behavior exercised indirectly by the Swift oracle but
    // worth pinning directly on the ported primitives.

    #[test]
    fn normalized_slug_drops_dot_git_and_requires_two_components() {
        assert_eq!(
            normalized_github_repository_slug("/owner/repo.git").as_deref(),
            Some("owner/repo")
        );
        assert_eq!(normalized_github_repository_slug("owner").as_deref(), None);
        assert_eq!(normalized_github_repository_slug("///").as_deref(), None);
    }

    #[test]
    fn remote_priority_ordering() {
        assert_eq!(github_remote_priority("upstream"), 0);
        assert_eq!(github_remote_priority("UPSTREAM"), 0);
        assert_eq!(github_remote_priority("origin"), 1);
        assert_eq!(github_remote_priority("fork"), 2);
    }
}
