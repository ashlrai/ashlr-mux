//! A compiled regular expression with a value-typed, capture-aware replacement
//! helper — the Rust equivalent of `SentryRegexPattern.swift`.
//!
//! Most rules compile to a stock `regex::Regex`. The single home-directory rule
//! needs a lookahead assertion, which stock `regex` does not support, so it
//! compiles to a `fancy_regex::Regex` instead.

use fancy_regex::Regex as FancyRegex;
use regex::Regex;

/// One match handed to a replacement closure: the whole matched text plus its
/// optional capture group 1 (the "prefix to keep" in cmux's redaction loop).
pub(crate) struct MatchInfo<'t> {
    /// The full matched substring (Swift `SentryRegexMatch.value`).
    pub full: &'t str,
    /// Capture group 1, if the pattern has one (Swift `match.captureGroup(1)`).
    pub group1: Option<&'t str>,
}

/// One matched span: `(start, end, optional group-1 span)`, all byte offsets.
type MatchSpan = (usize, usize, Option<(usize, usize)>);

/// A compiled pattern backed by either the stock or the fancy regex engine.
#[derive(Debug, Clone)]
pub(crate) enum Pattern {
    Stock(Regex),
    Fancy(FancyRegex),
}

impl Pattern {
    /// Compiles a stock (case-sensitive) pattern. Panics on an invalid pattern —
    /// these are compile-time constants, so an invalid one is a programmer error
    /// that must fail loudly, mirroring the Swift `try!`.
    pub(crate) fn stock(pattern: &str) -> Self {
        Pattern::Stock(Regex::new(pattern).expect("valid built-in regex"))
    }

    /// Compiles a stock pattern with the case-insensitive flag, matching the
    /// Swift `SentryRegexPattern` default of `.caseInsensitive`.
    pub(crate) fn stock_ci(pattern: &str) -> Self {
        Pattern::Stock(Regex::new(&format!("(?i){pattern}")).expect("valid built-in regex"))
    }

    /// Compiles a fancy-regex pattern (used only where a lookahead is required).
    pub(crate) fn fancy(pattern: &str) -> Self {
        Pattern::Fancy(FancyRegex::new(pattern).expect("valid built-in regex"))
    }

    /// Collects the non-overlapping match spans (and group-1 spans) left-to-right.
    ///
    /// This mirrors `NSRegularExpression.matches(in:range:)`: non-overlapping,
    /// leftmost-first. Returned as owned byte ranges so the caller can splice the
    /// source string without borrow conflicts.
    fn match_ranges(&self, text: &str) -> Vec<MatchSpan> {
        let mut out = Vec::new();
        match self {
            Pattern::Stock(re) => {
                for caps in re.captures_iter(text) {
                    let whole = caps.get(0).expect("group 0 always present");
                    let group1 = caps.get(1).map(|m| (m.start(), m.end()));
                    out.push((whole.start(), whole.end(), group1));
                }
            }
            Pattern::Fancy(re) => {
                for caps in re.captures_iter(text) {
                    let caps = caps.expect("fancy-regex match without engine error");
                    let whole = caps.get(0).expect("group 0 always present");
                    let group1 = caps.get(1).map(|m| (m.start(), m.end()));
                    out.push((whole.start(), whole.end(), group1));
                }
            }
        }
        out
    }

    /// Replaces every match with the string returned by `replacement`.
    ///
    /// Faithful to `SentryRegexPattern.replace(in:with:)`: nothing matched →
    /// returns `text` unchanged. Swift rewrites end-to-start over the original
    /// string; this builds the result front-to-back, which yields the identical
    /// output because every replacement is computed against the original span.
    pub(crate) fn replace_all<F>(&self, text: &str, replacement: F) -> String
    where
        F: Fn(&MatchInfo) -> String,
    {
        let ranges = self.match_ranges(text);
        if ranges.is_empty() {
            return text.to_string();
        }
        let mut result = String::with_capacity(text.len());
        let mut last = 0usize;
        for (start, end, group1) in ranges {
            result.push_str(&text[last..start]);
            let info = MatchInfo {
                full: &text[start..end],
                group1: group1.map(|(s, e)| &text[s..e]),
            };
            result.push_str(&replacement(&info));
            last = end;
        }
        result.push_str(&text[last..]);
        result
    }
}
