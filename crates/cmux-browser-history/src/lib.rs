//! cmux-browser-history — omnibar frecency suggestion engine.
//!
//! Headless port of `BrowserHistorySuggestionEngine` from the canonical macOS
//! `Packages/macOS/CmuxBrowser` package: a fuzzy + frecency omnibar scorer with
//! fixed-clock recency decay and URL-normalization dedup (www / default-port /
//! trailing-slash). A distinct domain from `cmux-mentions` (which is an
//! @-mention corpus). Pure logic with an injected clock.
//!
//! Port fidelity: every method mirrors the Swift source method-for-method. The
//! Swift oracle drives recency from a `Date`; here time is an injected `i64`
//! epoch-seconds `now` (and the entry timestamps are `i64` epoch seconds) so the
//! logic is deterministic without a datetime crate. `URLComponents` parsing is
//! reproduced with the `url` crate (see per-site `DIVERGENCE` notes for the two
//! spots where the two parsers differ).

use std::collections::HashSet;

use url::Url;

pub mod session_history;

pub use session_history::{
    NavigationAvailability, RealignOutcome, RestoredSessionHistory,
    SessionHistoryTraversalDecision, SessionHistoryURLSanitizer, SessionNavigationHistorySnapshot,
};

/// One persisted browser-history record: a visited URL with its display title
/// and the visit/typed statistics that feed omnibar frecency scoring.
///
/// Port of `BrowserHistoryEntry`. Timestamps are epoch seconds (`i64`) rather
/// than Foundation `Date`s, and `id` is an opaque string (the Swift `UUID` is
/// SwiftUI list identity only and plays no part in scoring), so no `uuid`/date
/// crates are pulled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserHistoryEntry {
    /// Stable identity (opaque; not used by scoring).
    pub id: String,
    /// Absolute URL string exactly as recorded at visit time.
    pub url: String,
    /// Page title, or `None` when unknown.
    pub title: Option<String>,
    /// Epoch seconds of the most recent visit.
    pub last_visited: i64,
    /// Total number of recorded visits to this URL.
    pub visit_count: i64,
    /// Number of times the user typed (rather than followed a link to) this URL.
    pub typed_count: i64,
    /// Epoch seconds of the most recent typed navigation, or `None` if never typed.
    pub last_typed_at: Option<i64>,
}

impl BrowserHistoryEntry {
    /// Creates a history entry. `typed_count` and `last_typed_at` carry the
    /// "never typed" state (`0` / `None`) for link-followed visits — mirror the
    /// defaults on the Swift `init`.
    pub fn new(
        id: impl Into<String>,
        url: impl Into<String>,
        title: Option<String>,
        last_visited: i64,
        visit_count: i64,
        typed_count: i64,
        last_typed_at: Option<i64>,
    ) -> Self {
        Self {
            id: id.into(),
            url: url.into(),
            title,
            last_visited,
            visit_count,
            typed_count,
            last_typed_at,
        }
    }
}

/// A history entry with its match fields precomputed (lowercased URL, host,
/// path+query, title) so omnibar scoring avoids re-parsing the URL and
/// re-lowercasing on every keystroke.
///
/// Port of `BrowserHistorySuggestionCandidate`.
#[derive(Debug, Clone)]
pub struct BrowserHistorySuggestionCandidate {
    /// The source history entry this candidate scores.
    pub entry: BrowserHistoryEntry,
    /// Full lowercased URL string.
    pub url_lower: String,
    /// Lowercased URL with any `http://`/`https://` prefix removed.
    pub url_sans_scheme_lower: String,
    /// Lowercased host component.
    pub host_lower: String,
    /// Lowercased path joined with the query (`path?query`), percent-encoded.
    pub path_and_query_lower: String,
    /// Lowercased, whitespace-trimmed page title.
    pub title_lower: String,
}

/// Pure omnibar matching/scoring over browser history. Stateless: every method
/// is a deterministic transform of its inputs, so it is trivially unit-testable
/// with a fixed clock and no filesystem.
///
/// Port of `BrowserHistorySuggestionEngine`.
#[derive(Debug, Clone, Copy, Default)]
pub struct BrowserHistorySuggestionEngine;

impl BrowserHistorySuggestionEngine {
    /// Creates a scoring engine. The engine holds no state.
    pub fn new() -> Self {
        Self
    }

    /// Precomputes the lowercased/parsed match fields for `entry`.
    ///
    /// Mirrors `candidate(for:)`. The URL is parsed with the `url` crate as the
    /// `URLComponents` stand-in; the crate already lowercases the scheme and
    /// host, and its `path()`/`query()` return the percent-encoded forms that
    /// Foundation's `percentEncodedPath`/`percentEncodedQuery` produce.
    pub fn candidate(&self, entry: BrowserHistoryEntry) -> BrowserHistorySuggestionCandidate {
        let url_lower = entry.url.to_lowercase();
        let url_sans_scheme_lower = Self::stripping_https_scheme_prefix(&url_lower).to_string();

        // URLComponents(string: entry.url) — parse the ORIGINAL (unlowered) URL.
        let parsed = Url::parse(&entry.url).ok();
        let host_lower = parsed
            .as_ref()
            .and_then(|u| u.host_str().map(str::to_lowercase))
            .unwrap_or_default();
        // DIVERGENCE: for an authority-only URL (e.g. "https://example.com") the
        // `url` crate reports path "/" whereas Foundation's percentEncodedPath is
        // "". Harmless for omnibar substring scoring; all recorded history URLs in
        // practice (and every test URL) carry an explicit path.
        let path = parsed
            .as_ref()
            .map(|u| u.path().to_lowercase())
            .unwrap_or_default();
        let query = parsed
            .as_ref()
            .and_then(|u| u.query())
            .map(str::to_lowercase)
            .unwrap_or_default();
        let path_and_query_lower = if query.is_empty() {
            path
        } else {
            format!("{path}?{query}")
        };

        let title_lower = entry.title.as_deref().unwrap_or("").trim().to_lowercase();

        BrowserHistorySuggestionCandidate {
            entry,
            url_lower,
            url_sans_scheme_lower,
            host_lower,
            path_and_query_lower,
            title_lower,
        }
    }

    /// Scores `candidate` against a normalized lowercased `query` and its
    /// `query_tokens`, returning `None` when the candidate does not match.
    /// Single-character queries require a strong (prefix) match; otherwise any
    /// substring or all-token match qualifies. `now` (epoch seconds) drives the
    /// recency decay.
    ///
    /// Mirrors `score(candidate:query:queryTokens:now:)`.
    pub fn score(
        &self,
        candidate: &BrowserHistorySuggestionCandidate,
        query: &str,
        query_tokens: &[String],
        now: i64,
    ) -> Option<f64> {
        let query_includes_scheme = query.starts_with("http://") || query.starts_with("https://");
        let url_match_value: &str = if query_includes_scheme {
            &candidate.url_lower
        } else {
            &candidate.url_sans_scheme_lower
        };

        let is_single_character_query = query.chars().count() == 1;
        if is_single_character_query {
            let has_single_char_strong_match = candidate.host_lower.starts_with(query)
                || candidate.title_lower.starts_with(query)
                || url_match_value.starts_with(query);
            if !has_single_char_strong_match {
                return None;
            }
        }

        let query_matches = url_match_value.contains(query)
            || candidate.host_lower.contains(query)
            || candidate.path_and_query_lower.contains(query)
            || candidate.title_lower.contains(query);

        let token_matches = !query_tokens.is_empty()
            && query_tokens.iter().all(|token| {
                candidate.url_sans_scheme_lower.contains(token)
                    || candidate.host_lower.contains(token)
                    || candidate.path_and_query_lower.contains(token)
                    || candidate.title_lower.contains(token)
            });

        if !(query_matches || token_matches) {
            return None;
        }

        let mut score = 0.0_f64;

        if url_match_value == query {
            score += 1200.0;
        }
        if candidate.host_lower == query {
            score += 980.0;
        }
        if candidate.host_lower.starts_with(query) {
            score += 680.0;
        }
        if url_match_value.starts_with(query) {
            score += 560.0;
        }
        if candidate.title_lower.starts_with(query) {
            score += 420.0;
        }
        if candidate.path_and_query_lower.starts_with(query) {
            score += 300.0;
        }

        if candidate.host_lower.contains(query) {
            score += 210.0;
        }
        if candidate.path_and_query_lower.contains(query) {
            score += 165.0;
        }
        if candidate.title_lower.contains(query) {
            score += 145.0;
        }

        for token in query_tokens {
            if candidate.host_lower == *token {
                score += 260.0;
            } else if candidate.host_lower.starts_with(token) {
                score += 170.0;
            } else if candidate.host_lower.contains(token) {
                score += 110.0;
            }

            if candidate.path_and_query_lower.starts_with(token) {
                score += 80.0;
            } else if candidate.path_and_query_lower.contains(token) {
                score += 52.0;
            }

            if candidate.title_lower.starts_with(token) {
                score += 74.0;
            } else if candidate.title_lower.contains(token) {
                score += 48.0;
            }
        }

        // Blend recency and repeat visits so history feels closer to browser
        // frecency. `now.timeIntervalSince(lastVisited)` becomes `now - lastVisited`.
        let age_hours = ((now - candidate.entry.last_visited) as f64 / 3600.0).max(0.0);
        let recency_score = (110.0 - age_hours / 3.0).max(0.0);
        let frequency_score = (candidate.entry.visit_count.max(1) as f64).ln_1p() * 38.0;
        let frequency_score = frequency_score.min(120.0);
        let typed_frequency_score = (candidate.entry.typed_count.max(0) as f64).ln_1p() * 80.0;
        let typed_frequency_score = typed_frequency_score.min(190.0);
        let typed_recency_score = match candidate.entry.last_typed_at {
            Some(last_typed_at) => {
                let typed_age_hours = ((now - last_typed_at) as f64 / 3600.0).max(0.0);
                (85.0 - typed_age_hours / 4.0).max(0.0)
            }
            None => 0.0,
        };
        score += recency_score + frequency_score + typed_frequency_score + typed_recency_score;

        Some(score)
    }

    /// Splits a query into unique, order-preserving tokens on whitespace,
    /// punctuation, and symbols.
    ///
    /// Mirrors `tokenize(query:)`. Foundation splits on
    /// `whitespacesAndNewlines ∪ punctuationCharacters ∪ symbols`.
    // DIVERGENCE: rather than reproduce Foundation's exact Unicode category sets
    // (which would need a Unicode-category crate — not permitted here), a
    // separator is any non-alphanumeric char. This is identical for the ASCII
    // whitespace/punctuation/symbol chars that occur in omnibar queries; it only
    // differs on combining marks (category M) and control/format chars (category
    // C), which Foundation keeps and this treats as separators — irrelevant for
    // real queries.
    pub fn tokenize(&self, query: &str) -> Vec<String> {
        let mut tokens: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for raw in query.split(|c: char| !c.is_alphanumeric()) {
            let token = raw.trim();
            if token.is_empty() {
                continue;
            }
            if seen.contains(token) {
                continue;
            }
            seen.insert(token.to_string());
            tokens.push(token.to_string());
        }
        tokens
    }

    /// Removes a leading `https://` or `http://` from `value`, returning it
    /// unchanged when neither prefix is present.
    ///
    /// Mirrors `strippingHTTPSSchemePrefix(_:)`.
    pub fn stripping_https_scheme_prefix(value: &str) -> &str {
        if let Some(rest) = value.strip_prefix("https://") {
            return rest;
        }
        if let Some(rest) = value.strip_prefix("http://") {
            return rest;
        }
        value
    }

    /// The dedup key for an `http`/`https` URL string: scheme, host with a
    /// leading `www.` stripped, default port dropped, trailing-slash-normalized
    /// path, and lowercased query. Returns `None` for non-http(s) URLs or URLs
    /// without a host.
    ///
    /// Mirrors `normalizedHistoryKey(urlString:)` (and, via a parsed `Url`,
    /// `normalizedHistoryKey(url:)`).
    pub fn normalized_history_key(&self, url_string: &str) -> Option<String> {
        let url = Url::parse(url_string).ok()?;
        Self::normalized_history_key_from_components(&url)
    }

    /// The dedup key for an already-parsed [`Url`]. See
    /// [`normalized_history_key`](Self::normalized_history_key).
    pub fn normalized_history_key_from_url(&self, url: &Url) -> Option<String> {
        Self::normalized_history_key_from_components(url)
    }

    fn normalized_history_key_from_components(url: &Url) -> Option<String> {
        // `url` lowercases the scheme; guard to http(s) exactly like Swift.
        let scheme = url.scheme();
        if scheme != "http" && scheme != "https" {
            return None;
        }

        let mut host = url.host_str()?.to_lowercase();
        if let Some(stripped) = host.strip_prefix("www.") {
            host = stripped.to_string();
        }

        // DIVERGENCE: Foundation drops the port when it equals the scheme
        // default (80/443); the `url` crate has already done exactly that —
        // `Url::port()` returns `None` for a default port and the explicit
        // non-default port otherwise — so no manual defaulting is needed.
        let port_part = match url.port() {
            Some(port) => format!(":{port}"),
            None => String::new(),
        };

        let mut path = url.path().to_string();
        if path.is_empty() {
            path = "/".to_string();
        }
        while path.len() > 1 && path.ends_with('/') {
            path.pop();
        }

        let query_part = match url.query() {
            Some(query) if !query.is_empty() => format!("?{}", query.to_lowercase()),
            _ => String::new(),
        };

        Some(format!("{scheme}://{host}{port_part}{path}{query_part}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000;

    fn entry(
        url: &str,
        title: Option<&str>,
        visit_count: i64,
        typed_count: i64,
    ) -> BrowserHistoryEntry {
        BrowserHistoryEntry::new(
            "id",
            url,
            title.map(str::to_string),
            NOW,
            visit_count,
            typed_count,
            None,
        )
    }

    // Port of `candidateStripsSchemeAndLowercases`.
    #[test]
    fn candidate_strips_scheme_and_lowercases() {
        let engine = BrowserHistorySuggestionEngine::new();
        let c = engine.candidate(entry("HTTPS://Example.COM/Foo?A=B", Some("  Foo  "), 1, 0));
        assert_eq!(c.url_lower, "https://example.com/foo?a=b");
        assert_eq!(c.url_sans_scheme_lower, "example.com/foo?a=b");
        assert_eq!(c.host_lower, "example.com");
        assert_eq!(c.path_and_query_lower, "/foo?a=b");
        assert_eq!(c.title_lower, "foo");
    }

    // Port of `exactHostQueryOutranksSubstringMatch`.
    #[test]
    fn exact_host_query_outranks_substring_match() {
        let engine = BrowserHistorySuggestionEngine::new();
        let exact = engine.candidate(entry("https://go.dev/", Some("Go"), 1, 0));
        let other = engine.candidate(entry("https://golang.org/doc", Some("Golang Docs"), 1, 0));
        let tokens = engine.tokenize("go.dev");
        let exact_score = engine.score(&exact, "go.dev", &tokens, NOW);
        let other_score = engine.score(&other, "go.dev", &tokens, NOW);
        assert!(exact_score.is_some());
        assert!(exact_score.unwrap_or(0.0) > other_score.unwrap_or(0.0));
    }

    // Port of `singleCharacterQueryRequiresPrefixMatch`.
    #[test]
    fn single_character_query_requires_prefix_match() {
        let engine = BrowserHistorySuggestionEngine::new();
        let prefix = engine.candidate(entry("https://github.com/", Some("GitHub"), 1, 0));
        let substring_only =
            engine.candidate(entry("https://example.com/g", Some("Example"), 1, 0));
        let tokens = vec!["g".to_string()];
        assert!(engine.score(&prefix, "g", &tokens, NOW).is_some());
        assert!(engine.score(&substring_only, "g", &tokens, NOW).is_none());
    }

    // Port of `nonMatchScoresNil`.
    #[test]
    fn non_match_scores_nil() {
        let engine = BrowserHistorySuggestionEngine::new();
        let c = engine.candidate(entry("https://example.com/", Some("Example"), 1, 0));
        let tokens = vec!["zzzznomatch".to_string()];
        assert!(engine.score(&c, "zzzznomatch", &tokens, NOW).is_none());
    }

    // Port of `tokenizeDedupesAndSplitsOnPunctuation`.
    #[test]
    fn tokenize_dedupes_and_splits_on_punctuation() {
        let engine = BrowserHistorySuggestionEngine::new();
        assert_eq!(
            engine.tokenize("foo bar foo, baz"),
            vec!["foo", "bar", "baz"]
        );
    }

    // Port of `normalizedKeyDropsWWWDefaultPortAndTrailingSlash`.
    #[test]
    fn normalized_key_drops_www_default_port_and_trailing_slash() {
        let engine = BrowserHistorySuggestionEngine::new();
        assert_eq!(
            engine.normalized_history_key("https://www.example.com:443/path/"),
            Some("https://example.com/path".to_string())
        );
        assert_eq!(
            engine.normalized_history_key("http://example.com:80/"),
            Some("http://example.com/".to_string())
        );
        assert_eq!(engine.normalized_history_key("ftp://example.com/"), None);
    }

    // Port of `typedFrequencyRaisesScore`.
    #[test]
    fn typed_frequency_raises_score() {
        let engine = BrowserHistorySuggestionEngine::new();
        let typed = engine.candidate(entry("https://typed.example/", Some("T"), 1, 5));
        let untyped = engine.candidate(entry("https://typed.example/", Some("T"), 1, 0));
        let tokens = engine.tokenize("typed");
        let typed_score = engine.score(&typed, "typed", &tokens, NOW).unwrap_or(0.0);
        let untyped_score = engine.score(&untyped, "typed", &tokens, NOW).unwrap_or(0.0);
        assert!(typed_score > untyped_score);
    }
}
