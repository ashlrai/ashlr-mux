//! Defensive size limits applied to transcript-derived text.
//!
//! Ports `Parsing/TranscriptTextBudget.swift`. Transcript lines can carry
//! multi-megabyte payloads (whole-file writes, base64 images); every body,
//! diff, and detail string stored on a [`super::model::ChatMessage`] is
//! clamped here, in one place.

/// Defensive size limits applied to transcript-derived text.
#[derive(Debug, Clone, Copy)]
pub struct TranscriptTextBudget {
    /// Limit for message bodies, tool outputs, and diffs (~16KB).
    pub max_body_characters: usize,
    /// Limit for the expanded tool-input detail (~2KB).
    pub max_input_detail_characters: usize,
    /// Limit for the argument excerpt inside a one-line tool summary.
    pub max_summary_argument_characters: usize,
}

impl Default for TranscriptTextBudget {
    /// The default budget: 16384 / 2048 / 80 characters, matching the Swift
    /// `init` defaults.
    fn default() -> Self {
        TranscriptTextBudget {
            max_body_characters: 16_384,
            max_input_detail_characters: 2_048,
            max_summary_argument_characters: 80,
        }
    }
}

impl TranscriptTextBudget {
    /// Creates a budget with the default limits.
    pub fn new() -> Self {
        TranscriptTextBudget::default()
    }

    /// Clamps body-sized text (prose, thoughts, outputs, diffs).
    pub fn body(&self, text: &str) -> String {
        truncated(text, self.max_body_characters)
    }

    /// Clamps tool-input detail text.
    pub fn input_detail(&self, text: &str) -> String {
        truncated(text, self.max_input_detail_characters)
    }

    /// Clamps and single-lines an argument excerpt for a tool summary.
    pub fn summary_argument(&self, text: &str) -> String {
        let replaced = text.replace('\n', " ");
        let one_line = trim_whitespaces(&replaced);
        truncated(one_line, self.max_summary_argument_characters)
    }
}

/// Truncates text to a character limit, marking the cut with an ellipsis.
///
/// DIVERGENCE: Swift `String.count`/`prefix` operate on grapheme clusters;
/// this counts Unicode scalars (`char`s). The two agree for every ASCII /
/// simple-text payload the transcripts carry; only exotic combining sequences
/// would differ, and only in where the (defensive) cut lands.
fn truncated(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut clamped: String = text.chars().take(limit).collect();
    clamped.push('…');
    clamped
}

/// Trims leading/trailing spaces and tabs, mirroring Swift's
/// `CharacterSet.whitespaces` (which excludes newlines).
///
/// DIVERGENCE: only ASCII space and tab are trimmed; the rare Unicode `Zs`
/// separators Foundation also trims never appear in the transcript arguments
/// this excerpts.
fn trim_whitespaces(text: &str) -> &str {
    text.trim_matches(|c| c == ' ' || c == '\t')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_limit_is_unchanged() {
        let budget = TranscriptTextBudget::new();
        assert_eq!(budget.body("short"), "short");
    }

    #[test]
    fn body_truncates_with_ellipsis() {
        let budget = TranscriptTextBudget::new();
        let huge = "x".repeat(40_000);
        let clamped = budget.body(&huge);
        assert_eq!(clamped.chars().count(), 16_385);
        assert!(clamped.ends_with('…'));
    }

    #[test]
    fn input_detail_uses_its_own_limit() {
        let budget = TranscriptTextBudget::new();
        let huge = "y".repeat(4_000);
        let clamped = budget.input_detail(&huge);
        assert_eq!(clamped.chars().count(), 2_049);
    }

    #[test]
    fn summary_argument_single_lines_and_clamps() {
        let budget = TranscriptTextBudget::new();
        assert_eq!(budget.summary_argument("  keep me  "), "keep me");
        assert_eq!(budget.summary_argument("line one\nline two"), "line one line two");
        let long = "z".repeat(200);
        let clamped = budget.summary_argument(&long);
        assert_eq!(clamped.chars().count(), 81);
        assert!(clamped.ends_with('…'));
    }
}
