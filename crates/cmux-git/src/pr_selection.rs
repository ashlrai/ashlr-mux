//! Pure pull-request badge selection.
//!
//! Headless port of the filesystem-/network-free core of
//! `Packages/macOS/CmuxGit/Sources/CmuxGit/Probe/PullRequestProbeService+Selection.swift`.
//! Picks the pull request a workspace badge should show for a branch: filter to
//! badge candidates (parseable state, valid URL, not a stale merged PR), then
//! reduce by status priority (open > merged > closed), then most-recently
//! updated (lexical string compare), then highest number, keeping the first on
//! full ties. Also indexes candidates by normalized head-branch name.
//!
//! Model ports in this module:
//! - `Model/PullRequestStatus.swift:1-29` — [`PullRequestStatus`] +
//!   [`PullRequestStatus::from_github_state`].
//! - `Model/GitHubPullRequestProbeItem.swift:1-42` —
//!   [`GitHubPullRequestProbeItem`].
//! - `Parsing/GitMetadataService+Refs.swift:7-10` —
//!   [`normalized_branch_name`] (the only sibling dependency; the 3-line helper
//!   is ported here).
//!
//! The `mergedBadgeStaleAfter` constant is
//! `PullRequestProbeService.swift:55` (`14 * 24 * 60 * 60`).
//!
//! **Excluded** as host wiring / sibling lanes: the network fetch + repo cache,
//! `decodeJSON`, `PullRequestProbeService+Fetch.swift`, the refresh-policy
//! statics (`shouldSkipLookup` / `refreshAllowsRepoCache` / `shouldRefresh`),
//! and the `Workspace*` result-resolution.
//!
//! ## Divergences from Swift (documented for parity review)
//!
//! - **URL validity.** Swift uses Foundation `URL(string:)`, which is lenient
//!   (rejects essentially only empty / space-bearing strings and percent-encodes
//!   the rest). This crate reuses the strict `url` crate (as
//!   `github_repository_slug` already does), so the faithful contract is "valid
//!   iff non-empty and parses as an absolute URL". Real PR URLs and the empty
//!   string agree with Swift (covering both oracle cases); pathological inputs
//!   such as `"not a url"` are a sanctioned deviation. The Swift oracle test
//!   itself (`PullRequestProbeServiceTests.swift:50-52`) documents that such
//!   junk is not a stable cross-SDK fixture.
//! - **`updatedAt` tie-break is a LEXICAL string compare, not a parsed-date
//!   compare** — kept byte-for-byte to match Swift (`?? ""` then `>`).
//! - **Timestamp parsing.** Swift's `ISO8601DateFormatter` (which "changes
//!   silently between macOS versions" per the repo CLAUDE.md) is replaced by a
//!   tiny hand-rolled RFC-3339 parser used only by [`is_stale_merged`]. It
//!   accepts `YYYY-MM-DDTHH:MM:SS[.fff]Z` (the shape the probe always produces)
//!   and yields whole epoch seconds — only whole-second math is ever needed for
//!   the 14-day threshold. Fractional seconds are accepted but truncated. The
//!   trailing `Z` is *required* (the probe always emits UTC), which is stricter
//!   than `ISO8601DateFormatter` in the offset direction. On calendar validity
//!   it matches Foundation: the year must be exactly four digits, and invalid
//!   days-of-month (Feb 30, Feb 29 in a non-leap year, Apr 31, …) and leap
//!   seconds (`:60`) are rejected — the same inputs `ISO8601DateFormatter`
//!   returns `nil` for. (An earlier version only range-checked `1..=31` days,
//!   `0..=60` seconds, and used an unchecked year parse, accepting inputs
//!   Foundation rejects; that leniency divergence has been removed.)

use std::collections::HashMap;

/// Merged PRs older than this (in seconds) no longer earn a badge.
///
/// Port of `PullRequestProbeService.swift:55`
/// (`mergedBadgeStaleAfter = 14 * 24 * 60 * 60`).
pub const MERGED_BADGE_STALE_AFTER: i64 = 14 * 24 * 60 * 60;

// ---------------------------------------------------------------------------
// Model/PullRequestStatus.swift
// ---------------------------------------------------------------------------

/// The lifecycle state of a GitHub pull request, as the probe reports it.
///
/// Port of `Model/PullRequestStatus.swift:7-29`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestStatus {
    /// The pull request is open.
    Open,
    /// The pull request was merged.
    Merged,
    /// The pull request was closed without merging.
    Closed,
}

impl PullRequestStatus {
    /// Parses GitHub's `state` string (`"OPEN"`, `"MERGED"`, `"CLOSED"`, any
    /// case, surrounding whitespace tolerated). Returns `None` for anything
    /// else.
    ///
    /// Port of `PullRequestStatus.init?(githubState:)`
    /// (`Model/PullRequestStatus.swift:17-28`): `trimmingCharacters(in:
    /// .whitespacesAndNewlines).uppercased()`.
    pub fn from_github_state(raw_state: &str) -> Option<Self> {
        match raw_state.trim().to_uppercase().as_str() {
            "OPEN" => Some(Self::Open),
            "MERGED" => Some(Self::Merged),
            "CLOSED" => Some(Self::Closed),
            _ => None,
        }
    }

    /// Badge priority: open (3) beats merged (2) beats closed (1).
    ///
    /// Port of the local `statusPriority(_:)` in `preferredPullRequest`
    /// (`PullRequestProbeService+Selection.swift:39-48`).
    fn priority(self) -> i32 {
        match self {
            Self::Open => 3,
            Self::Merged => 2,
            Self::Closed => 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Model/GitHubPullRequestProbeItem.swift
// ---------------------------------------------------------------------------

/// One pull request as the GitHub probe caches it: the fields needed to pick the
/// best PR for a branch and render a badge.
///
/// Port of `Model/GitHubPullRequestProbeItem.swift:8-42`. `state` is the raw
/// GitHub state string (the probe synthesizes `"MERGED"` when `mergedAt` is
/// set); parse it with [`PullRequestStatus::from_github_state`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubPullRequestProbeItem {
    /// The pull request number.
    pub number: i64,
    /// Raw GitHub state string (`"OPEN"`/`"MERGED"`/`"CLOSED"`, any case).
    pub state: String,
    /// The PR's html URL string.
    pub url: String,
    /// ISO-8601 `updatedAt` timestamp, if known.
    pub updated_at: Option<String>,
    /// ISO-8601 `mergedAt` timestamp, if the PR merged.
    pub merged_at: Option<String>,
    /// The PR's head (source) branch name, if known.
    pub head_ref_name: Option<String>,
    /// The PR's base (target) branch name, if known.
    pub base_ref_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsing/GitMetadataService+Refs.swift
// ---------------------------------------------------------------------------

/// Normalizes a branch name for keying: trims whitespace and maps empty to
/// `None`.
///
/// Port of `GitMetadataService.normalizedBranchName(_:)`
/// (`Parsing/GitMetadataService+Refs.swift:7-10`):
/// `trimmingCharacters(in: .whitespacesAndNewlines)`, empty → `nil`.
pub fn normalized_branch_name(branch: Option<&str>) -> Option<String> {
    let trimmed = branch.unwrap_or("").trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

// ---------------------------------------------------------------------------
// PullRequestProbeService+Selection.swift
// ---------------------------------------------------------------------------

/// Picks the pull request a badge should show: open beats merged beats closed,
/// then most recently updated (lexical string compare), then highest number.
/// Returns `None` when no item is a valid badge candidate.
///
/// Port of `preferredPullRequest(from:now:)`
/// (`PullRequestProbeService+Selection.swift:35-88`). `now` is injected as epoch
/// seconds; the reduce keeps the first item on a full tie (replace only when the
/// candidate is *strictly* preferred).
pub fn preferred_pull_request(
    pull_requests: &[GitHubPullRequestProbeItem],
    now: i64,
) -> Option<&GitHubPullRequestProbeItem> {
    let mut best: Option<&GitHubPullRequestProbeItem> = None;
    for pull_request in pull_requests {
        if !is_badge_candidate(pull_request, now) {
            continue;
        }
        match best {
            None => best = Some(pull_request),
            Some(current_best) => {
                if is_preferred(pull_request, current_best) {
                    best = Some(pull_request);
                }
            }
        }
    }
    best
}

/// Whether `candidate` should replace `current` as the preferred badge PR.
///
/// Port of the local `isPreferred(candidate:over:)`
/// (`PullRequestProbeService+Selection.swift:50-72`). Returns `false` when
/// either state is unparseable. `updatedAt` uses `?? ""` then a **lexical
/// string** compare (not a parsed-date compare) to match Swift exactly.
fn is_preferred(
    candidate: &GitHubPullRequestProbeItem,
    current: &GitHubPullRequestProbeItem,
) -> bool {
    let (Some(candidate_status), Some(current_status)) = (
        PullRequestStatus::from_github_state(&candidate.state),
        PullRequestStatus::from_github_state(&current.state),
    ) else {
        return false;
    };

    let candidate_priority = candidate_status.priority();
    let current_priority = current_status.priority();
    if candidate_priority != current_priority {
        return candidate_priority > current_priority;
    }

    let candidate_updated_at = candidate.updated_at.as_deref().unwrap_or("");
    let current_updated_at = current.updated_at.as_deref().unwrap_or("");
    if candidate_updated_at != current_updated_at {
        return candidate_updated_at > current_updated_at;
    }

    candidate.number > current.number
}

/// Indexes pull requests by normalized head-branch name, keeping the preferred
/// PR per branch and dropping non-candidates (unparseable state, invalid URL,
/// stale merged).
///
/// Port of `pullRequestMapByNormalizedBranch(from:now:)`
/// (`PullRequestProbeService+Selection.swift:7-30`). On a branch collision it
/// keeps `preferredPullRequest([currentBest, pullRequest]) ?? currentBest`.
pub fn pull_request_map_by_normalized_branch(
    pull_requests: &[GitHubPullRequestProbeItem],
    now: i64,
) -> HashMap<String, GitHubPullRequestProbeItem> {
    let mut pull_requests_by_branch: HashMap<String, GitHubPullRequestProbeItem> = HashMap::new();

    for pull_request in pull_requests {
        let Some(branch) = normalized_branch_name(pull_request.head_ref_name.as_deref()) else {
            continue;
        };
        if !is_badge_candidate(pull_request, now) {
            continue;
        }

        if let Some(current_best) = pull_requests_by_branch.get(&branch).cloned() {
            let pair = [current_best.clone(), pull_request.clone()];
            // `preferredPullRequest(...) ?? currentBest`.
            let chosen = preferred_pull_request(&pair, now)
                .cloned()
                .unwrap_or(current_best);
            pull_requests_by_branch.insert(branch, chosen);
        } else {
            pull_requests_by_branch.insert(branch, pull_request.clone());
        }
    }

    pull_requests_by_branch
}

/// Whether a PR can back a badge at all: parseable state, valid URL, and not a
/// stale merged PR.
///
/// Port of `isBadgeCandidate(_:now:)`
/// (`PullRequestProbeService+Selection.swift:92-101`). See the module-level
/// note on URL-validity divergence.
pub fn is_badge_candidate(pull_request: &GitHubPullRequestProbeItem, now: i64) -> bool {
    if PullRequestStatus::from_github_state(&pull_request.state).is_none()
        || !is_valid_url(&pull_request.url)
    {
        return false;
    }
    !is_stale_merged(pull_request, now)
}

/// Whether a merged PR is older than [`MERGED_BADGE_STALE_AFTER`].
///
/// Port of `isStaleMerged(_:now:)`
/// (`PullRequestProbeService+Selection.swift:104-113`):
/// `now.timeIntervalSince(mergedAt) > mergedBadgeStaleAfter`, i.e.
/// `now - mergedAt > MERGED_BADGE_STALE_AFTER`.
pub fn is_stale_merged(pull_request: &GitHubPullRequestProbeItem, now: i64) -> bool {
    if PullRequestStatus::from_github_state(&pull_request.state) != Some(PullRequestStatus::Merged)
    {
        return false;
    }
    let Some(merged_at) = github_timestamp_date(pull_request.merged_at.as_deref()) else {
        return false;
    };
    now - merged_at > MERGED_BADGE_STALE_AFTER
}

/// Parses a GitHub ISO-8601 timestamp (with or without fractional seconds) into
/// whole epoch seconds, trimming whitespace and mapping empty to `None`.
///
/// Port of `githubTimestampDate(from:)`
/// (`PullRequestProbeService+Selection.swift:116-127`). DIVERGENCE: hand-rolled
/// RFC-3339 parser (see module note) instead of `ISO8601DateFormatter`;
/// fractional seconds are accepted but truncated (only whole-second math is
/// needed for the 14-day threshold).
pub fn github_timestamp_date(raw_timestamp: Option<&str>) -> Option<i64> {
    let timestamp = raw_timestamp.unwrap_or("").trim();
    if timestamp.is_empty() {
        return None;
    }
    parse_rfc3339_utc_seconds(timestamp)
}

/// Whether a PR URL string is a valid badge URL.
///
/// DIVERGENCE from Swift `URL(string:) != nil`: reuses the strict `url` crate
/// (as `github_repository_slug` does). Contract: non-empty and parses as an
/// absolute URL. Real PR URLs and empty strings agree with Foundation; junk
/// strings are a sanctioned deviation (see module note).
fn is_valid_url(url: &str) -> bool {
    !url.is_empty() && url::Url::parse(url).is_ok()
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.fff]Z` into whole epoch seconds, or `None` if it
/// does not match that shape. Fractional seconds and any trailing `Z` are
/// accepted; the result is truncated to whole seconds.
fn parse_rfc3339_utc_seconds(input: &str) -> Option<i64> {
    // Require a trailing `Z` (UTC); the probe always emits it.
    let body = input.strip_suffix('Z')?;

    let (date_part, time_part) = body.split_once('T')?;

    // Date: YYYY-MM-DD. `ISO8601DateFormatter` requires a 4-digit year, so a
    // fixed-width all-digit field is parity (rejects `26-…`, signs, `+2026`).
    let mut date_fields = date_part.split('-');
    let year: i64 = parse_fixed(date_fields.next()?, 4)?;
    let month: i64 = parse_fixed(date_fields.next()?, 2)?;
    let day: i64 = parse_fixed(date_fields.next()?, 2)?;
    if date_fields.next().is_some() {
        return None;
    }

    // Time: HH:MM:SS with optional `.fff` fractional part (truncated).
    let time_core = match time_part.split_once('.') {
        Some((core, _fraction)) => core,
        None => time_part,
    };
    let mut time_fields = time_core.split(':');
    let hour: i64 = parse_fixed(time_fields.next()?, 2)?;
    let minute: i64 = parse_fixed(time_fields.next()?, 2)?;
    let second: i64 = parse_fixed(time_fields.next()?, 2)?;
    if time_fields.next().is_some() {
        return None;
    }

    // Range-validate as `ISO8601DateFormatter` would: it rejects out-of-range
    // fields, leap seconds (`:60` → nil), and invalid days-of-month (Feb 30,
    // Feb 29 in a non-leap year, Apr 31, …). We validate the day against the
    // month (leap-year aware) rather than a blanket `1..=31`.
    if !(1..=12).contains(&month)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=59).contains(&second)
        || !(1..=days_in_month(year, month)).contains(&day)
    {
        return None;
    }

    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// Parses a fixed-width, all-ASCII-digit numeric field (e.g. `"03"`), rejecting
/// signs and other non-digit characters that `str::parse` would accept.
fn parse_fixed(field: &str, expected_len: usize) -> Option<i64> {
    if field.len() != expected_len || !field.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    field.parse().ok()
}

/// Number of days in a proleptic-Gregorian month (leap-year aware).
///
/// Used to reject invalid days-of-month (Feb 30, Feb 29 in a non-leap year,
/// Apr 31, …) so the parser matches `ISO8601DateFormatter`'s calendar
/// validation. Assumes `month` is already range-checked to `1..=12`.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let is_leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
            if is_leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Days since the Unix epoch (1970-01-01) for a proleptic-Gregorian Y-M-D.
///
/// Howard Hinnant's `days_from_civil` algorithm; exact integer arithmetic (no
/// floating point, no timezone), matching the whole-second `timeIntervalSince`
/// math the staleness check needs.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

// ---------------------------------------------------------------------------
// Tests — ported from PullRequestProbeServiceTests.swift (selection cases) plus
// author-derived edge cases flagged in the port spec.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirror of the Swift `item(...)` test helper
    /// (`PullRequestProbeServiceTests.swift:9-27`).
    fn item(
        number: i64,
        state: &str,
        url: &str,
        updated_at: Option<&str>,
        merged_at: Option<&str>,
        head_ref_name: Option<&str>,
        base_ref_name: Option<&str>,
    ) -> GitHubPullRequestProbeItem {
        GitHubPullRequestProbeItem {
            number,
            state: state.to_string(),
            url: url.to_string(),
            updated_at: updated_at.map(str::to_string),
            merged_at: merged_at.map(str::to_string),
            head_ref_name: head_ref_name.map(str::to_string),
            base_ref_name: base_ref_name.map(str::to_string),
        }
    }

    // The Swift tests default `now` to `Date()`; the selection cases contain no
    // merged items, so staleness never fires. We pin `now` to the same instant
    // used by the stale-map oracle for determinism.
    const NOW_2026_04_20: i64 = 1_776_686_400; // 2026-04-20T12:00:00Z

    // MARK: preferredPullRequest

    /// `preferredPullRequestPrefersOpenOverMergedAndClosed`
    /// (`PullRequestProbeServiceTests.swift:31-38`).
    #[test]
    fn preferred_pull_request_prefers_open_over_merged_and_closed() {
        let candidates = [
            item(
                1889,
                "MERGED",
                "https://github.com/manaflow-ai/cmux/pull/1889",
                Some("2026-03-20T18:00:00Z"),
                // A merged item needs a non-stale mergedAt to stay a candidate;
                // Swift's default now=Date() (well past 2026) makes this stale
                // and the test still selects the OPEN one. We keep it a
                // candidate under the pinned now so priority (not staleness)
                // does the selecting, preserving the tested behavior.
                Some("2026-04-19T18:00:00Z"),
                None,
                None,
            ),
            item(
                1891,
                "OPEN",
                "https://github.com/manaflow-ai/cmux/pull/1891",
                Some("2026-03-19T18:00:00Z"),
                None,
                None,
                None,
            ),
            item(
                1800,
                "CLOSED",
                "https://github.com/manaflow-ai/cmux/pull/1800",
                Some("2026-03-21T18:00:00Z"),
                None,
                None,
                None,
            ),
        ];
        assert_eq!(
            preferred_pull_request(&candidates, NOW_2026_04_20),
            Some(&candidates[1])
        );
    }

    /// `preferredPullRequestPrefersMostRecentlyUpdatedWithinSameStatus`
    /// (`PullRequestProbeServiceTests.swift:40-44`).
    #[test]
    fn preferred_pull_request_prefers_most_recently_updated_within_same_status() {
        let older_open = item(
            1880,
            "OPEN",
            "https://github.com/manaflow-ai/cmux/pull/1880",
            Some("2026-03-18T18:00:00Z"),
            None,
            None,
            None,
        );
        let newer_open = item(
            1890,
            "OPEN",
            "https://github.com/manaflow-ai/cmux/pull/1890",
            Some("2026-03-20T18:00:00Z"),
            None,
            None,
            None,
        );
        let candidates = [older_open, newer_open.clone()];
        assert_eq!(
            preferred_pull_request(&candidates, NOW_2026_04_20),
            Some(&newer_open)
        );
    }

    /// `preferredPullRequestIgnoresMalformedCandidates`
    /// (`PullRequestProbeServiceTests.swift:46-57`): unparseable state and an
    /// empty URL are both rejected.
    #[test]
    fn preferred_pull_request_ignores_malformed_candidates() {
        let valid = item(
            1888,
            "OPEN",
            "https://github.com/manaflow-ai/cmux/pull/1888",
            Some("2026-03-20T18:00:00Z"),
            None,
            None,
            None,
        );
        let candidates = [
            item(
                9999,
                "WHATEVER",
                "https://github.com/manaflow-ai/cmux/pull/9999",
                Some("2026-03-21T18:00:00Z"),
                None,
                None,
                None,
            ),
            item(
                10000,
                "OPEN",
                "", // empty URL is rejected by URL(string:) on every macOS
                Some("2026-03-21T18:00:00Z"),
                None,
                None,
                None,
            ),
            valid.clone(),
        ];
        assert_eq!(
            preferred_pull_request(&candidates, NOW_2026_04_20),
            Some(&valid)
        );
    }

    // MARK: branch map + staleness

    /// `pullRequestMapDropsStaleMergedHeadPullRequestForLongLivedBaseBranch`
    /// (`PullRequestProbeServiceTests.swift:61-73`), fixed now=2026-04-20T12:00:00Z.
    #[test]
    fn pull_request_map_drops_stale_merged_head_pull_request_for_long_lived_base_branch() {
        let now = github_timestamp_date(Some("2026-04-20T12:00:00Z")).unwrap();
        assert_eq!(now, NOW_2026_04_20); // hand-computed epoch cross-check
        let pull_requests = [
            item(
                2400,
                "MERGED",
                "https://github.com/manaflow-ai/cmux/pull/2400",
                Some("2026-03-06T12:00:00Z"),
                Some("2026-03-06T12:00:00Z"),
                Some("develop"),
                Some("main"),
            ),
            item(
                2501,
                "MERGED",
                "https://github.com/manaflow-ai/cmux/pull/2501",
                Some("2026-04-19T12:00:00Z"),
                Some("2026-04-19T12:00:00Z"),
                Some("feature/recent-one"),
                Some("develop"),
            ),
            item(
                2502,
                "OPEN",
                "https://github.com/manaflow-ai/cmux/pull/2502",
                Some("2026-04-20T12:00:00Z"),
                None,
                Some("feature/recent-two"),
                Some("develop"),
            ),
        ];

        let by_branch = pull_request_map_by_normalized_branch(&pull_requests, now);
        assert!(!by_branch.contains_key("develop"));
        assert_eq!(
            by_branch.get("feature/recent-one").map(|p| p.number),
            Some(2501)
        );
        assert_eq!(
            by_branch.get("feature/recent-two").map(|p| p.number),
            Some(2502)
        );
    }

    // MARK: author-derived edge cases (flagged in the port spec)

    /// `is_stale_merged` boundary: exactly 14 days is NOT stale, 14 days + 1s
    /// IS stale (`now - mergedAt > MERGED_BADGE_STALE_AFTER`).
    #[test]
    fn is_stale_merged_boundary() {
        let merged = github_timestamp_date(Some("2026-03-06T12:00:00Z")).unwrap();
        assert_eq!(merged, 1_772_798_400); // hand-computed epoch
        let pr = item(
            1,
            "MERGED",
            "https://example.com/1",
            None,
            Some("2026-03-06T12:00:00Z"),
            None,
            None,
        );
        // Exactly 14 days later: not stale.
        assert!(!is_stale_merged(&pr, merged + MERGED_BADGE_STALE_AFTER));
        // 14 days + 1 second later: stale.
        assert!(is_stale_merged(&pr, merged + MERGED_BADGE_STALE_AFTER + 1));
        // A merged PR with no mergedAt is never stale.
        let pr_no_merge = item(2, "MERGED", "https://example.com/2", None, None, None, None);
        assert!(!is_stale_merged(
            &pr_no_merge,
            merged + 10 * MERGED_BADGE_STALE_AFTER
        ));
        // A non-merged PR is never stale regardless of mergedAt.
        let pr_open = item(
            3,
            "OPEN",
            "https://example.com/3",
            None,
            Some("2000-01-01T00:00:00Z"),
            None,
            None,
        );
        assert!(!is_stale_merged(&pr_open, merged));
    }

    /// `github_timestamp_date` parses fractional and plain seconds to the same
    /// whole-second epoch, and rejects empty / whitespace / `None`.
    #[test]
    fn github_timestamp_date_fractional_and_plain() {
        let plain = github_timestamp_date(Some("2026-03-06T12:00:00Z"));
        let fractional = github_timestamp_date(Some("2026-03-06T12:00:00.123Z"));
        assert_eq!(plain, Some(1_772_798_400));
        assert_eq!(fractional, Some(1_772_798_400));

        assert_eq!(github_timestamp_date(None), None);
        assert_eq!(github_timestamp_date(Some("")), None);
        assert_eq!(github_timestamp_date(Some("   \n ")), None);
        // Surrounding whitespace is trimmed before parsing.
        assert_eq!(
            github_timestamp_date(Some("  2026-03-06T12:00:00Z ")),
            Some(1_772_798_400)
        );
        // Malformed shapes yield None.
        assert_eq!(github_timestamp_date(Some("not a date")), None);
        assert_eq!(github_timestamp_date(Some("2026-13-06T12:00:00Z")), None);
        assert_eq!(github_timestamp_date(Some("2026-03-06 12:00:00Z")), None);
        // Missing trailing Z is rejected (probe always emits UTC `Z`).
        assert_eq!(github_timestamp_date(Some("2026-03-06T12:00:00")), None);
    }

    /// Calendar-validity parity with `ISO8601DateFormatter`: inputs Foundation
    /// returns `nil` for must also parse to `None`, not a bogus epoch. Pins the
    /// former leniency divergence (day-of-month never validated against the
    /// month, `:60` leap seconds accepted, unchecked non-4-digit year).
    #[test]
    fn github_timestamp_date_rejects_invalid_calendar_dates() {
        // Invalid day-of-month for the month (Foundation → nil).
        assert_eq!(github_timestamp_date(Some("2026-02-30T00:00:00Z")), None);
        assert_eq!(github_timestamp_date(Some("2025-02-29T00:00:00Z")), None); // non-leap
        assert_eq!(github_timestamp_date(Some("2026-04-31T00:00:00Z")), None);
        assert_eq!(github_timestamp_date(Some("2026-01-32T00:00:00Z")), None);
        assert_eq!(github_timestamp_date(Some("2026-06-00T00:00:00Z")), None); // day 0
                                                                               // Leap second is rejected (`ISO8601DateFormatter` → nil).
        assert_eq!(github_timestamp_date(Some("2026-06-30T23:59:60Z")), None);
        // Year must be exactly four digits — no 2-digit, signed, or long years.
        assert_eq!(github_timestamp_date(Some("26-03-06T12:00:00Z")), None);
        assert_eq!(github_timestamp_date(Some("+2026-03-06T12:00:00Z")), None);
        assert_eq!(github_timestamp_date(Some("02026-03-06T12:00:00Z")), None);

        // Sanity: genuinely valid leap-day still parses (Feb 29 in a leap year).
        assert_eq!(
            github_timestamp_date(Some("2024-02-29T00:00:00Z")),
            Some(days_from_civil(2024, 2, 29) * 86_400)
        );
    }

    /// `from_github_state` trims + upper-cases, else `None`.
    #[test]
    fn from_github_state_parsing() {
        assert_eq!(
            PullRequestStatus::from_github_state("open"),
            Some(PullRequestStatus::Open)
        );
        assert_eq!(
            PullRequestStatus::from_github_state("  MERGED\n"),
            Some(PullRequestStatus::Merged)
        );
        assert_eq!(
            PullRequestStatus::from_github_state("Closed"),
            Some(PullRequestStatus::Closed)
        );
        assert_eq!(PullRequestStatus::from_github_state("whatever"), None);
        assert_eq!(PullRequestStatus::from_github_state(""), None);
    }

    /// `normalized_branch_name` trims and maps empty to `None`.
    #[test]
    fn normalized_branch_name_trims_and_empties() {
        assert_eq!(
            normalized_branch_name(Some("main")).as_deref(),
            Some("main")
        );
        assert_eq!(
            normalized_branch_name(Some("  feature/x \n")).as_deref(),
            Some("feature/x")
        );
        assert_eq!(normalized_branch_name(Some("   ")), None);
        assert_eq!(normalized_branch_name(Some("")), None);
        assert_eq!(normalized_branch_name(None), None);
    }

    /// First-wins on a full tie: same status, same updatedAt, same number →
    /// the earlier item is kept (candidate is not *strictly* preferred).
    #[test]
    fn preferred_pull_request_keeps_first_on_full_tie() {
        let first = item(
            5,
            "OPEN",
            "https://example.com/a",
            Some("2026-03-20T18:00:00Z"),
            None,
            None,
            None,
        );
        let second = item(
            5,
            "OPEN",
            "https://example.com/b",
            Some("2026-03-20T18:00:00Z"),
            None,
            None,
            None,
        );
        let candidates = [first.clone(), second];
        assert_eq!(
            preferred_pull_request(&candidates, NOW_2026_04_20),
            Some(&first)
        );
    }

    /// Higher number breaks a same-status, same-updatedAt tie.
    #[test]
    fn preferred_pull_request_higher_number_breaks_tie() {
        let lower = item(
            5,
            "OPEN",
            "https://example.com/a",
            Some("2026-03-20T18:00:00Z"),
            None,
            None,
            None,
        );
        let higher = item(
            9,
            "OPEN",
            "https://example.com/b",
            Some("2026-03-20T18:00:00Z"),
            None,
            None,
            None,
        );
        let candidates = [lower, higher.clone()];
        assert_eq!(
            preferred_pull_request(&candidates, NOW_2026_04_20),
            Some(&higher)
        );
    }

    /// `preferred_pull_request` returns `None` when nothing is a candidate.
    #[test]
    fn preferred_pull_request_none_when_no_candidates() {
        let candidates = [
            item(
                1,
                "WHATEVER",
                "https://example.com/1",
                None,
                None,
                None,
                None,
            ),
            item(2, "OPEN", "", None, None, None, None),
        ];
        assert_eq!(preferred_pull_request(&candidates, NOW_2026_04_20), None);
    }

    /// Branch collision keeps the preferred PR (open over stale-eligible peers).
    #[test]
    fn map_collision_keeps_preferred() {
        let older_open = item(
            10,
            "OPEN",
            "https://example.com/10",
            Some("2026-03-18T18:00:00Z"),
            None,
            Some("shared"),
            None,
        );
        let newer_open = item(
            11,
            "OPEN",
            "https://example.com/11",
            Some("2026-03-25T18:00:00Z"),
            None,
            Some("shared"),
            None,
        );
        let by_branch =
            pull_request_map_by_normalized_branch(&[older_open, newer_open], NOW_2026_04_20);
        assert_eq!(by_branch.get("shared").map(|p| p.number), Some(11));
    }
}
