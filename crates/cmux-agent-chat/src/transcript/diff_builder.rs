//! Builds the simple unified-ish diffs shown on file-edit cards.
//!
//! Ports `Parsing/TranscriptDiffBuilder.swift`. Transcripts carry whole
//! old/new strings rather than diffs, so this renders removals as `-` lines
//! followed by additions as `+` lines and counts lines on each side. It is
//! presentation-grade, not patch-grade.

/// A rendered change: the diff text plus line counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The rendered `-`/`+` line diff.
    pub diff: String,
    /// Count of added lines.
    pub additions: i64,
    /// Count of removed lines.
    pub deletions: i64,
}

/// Builds presentation-grade diffs for file-edit cards.
#[derive(Debug, Default, Clone, Copy)]
pub struct TranscriptDiffBuilder;

impl TranscriptDiffBuilder {
    /// Creates a diff builder.
    pub fn new() -> Self {
        TranscriptDiffBuilder
    }

    /// Renders an in-place replacement of `old_text` by `new_text`.
    pub fn replacement(&self, old_text: &str, new_text: &str) -> Change {
        let old_lines = lines(old_text);
        let new_lines = lines(new_text);
        let rendered: Vec<String> = old_lines
            .iter()
            .map(|line| format!("-{line}"))
            .chain(new_lines.iter().map(|line| format!("+{line}")))
            .collect();
        Change {
            diff: rendered.join("\n"),
            additions: new_lines.len() as i64,
            deletions: old_lines.len() as i64,
        }
    }

    /// Renders a whole-file write as pure additions.
    pub fn creation(&self, content: &str) -> Change {
        let new_lines = lines(content);
        let rendered: Vec<String> = new_lines.iter().map(|line| format!("+{line}")).collect();
        Change {
            diff: rendered.join("\n"),
            additions: new_lines.len() as i64,
            deletions: 0,
        }
    }

    /// Combines several changes into one (for multi-edit tools).
    pub fn combined(&self, changes: &[Change]) -> Change {
        Change {
            diff: changes
                .iter()
                .map(|change| change.diff.clone())
                .collect::<Vec<_>>()
                .join("\n"),
            additions: changes.iter().map(|change| change.additions).sum(),
            deletions: changes.iter().map(|change| change.deletions).sum(),
        }
    }
}

/// Splits text into lines, treating empty text as zero lines.
///
/// Mirrors Swift `components(separatedBy: "\n")` (which never omits empty
/// trailing/leading components), with the empty-string special case that
/// yields no lines rather than one empty line.
fn lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n').collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_renders_minus_then_plus() {
        let change = TranscriptDiffBuilder::new().replacement("let a = 1", "let a = 2\nlet b = 3");
        assert_eq!(change.diff, "-let a = 1\n+let a = 2\n+let b = 3");
        assert_eq!(change.additions, 2);
        assert_eq!(change.deletions, 1);
    }

    #[test]
    fn creation_is_pure_additions() {
        let change = TranscriptDiffBuilder::new().creation("one\ntwo\nthree");
        assert_eq!(change.diff, "+one\n+two\n+three");
        assert_eq!(change.additions, 3);
        assert_eq!(change.deletions, 0);
    }

    #[test]
    fn empty_text_is_zero_lines() {
        let change = TranscriptDiffBuilder::new().replacement("", "added");
        assert_eq!(change.diff, "+added");
        assert_eq!(change.additions, 1);
        assert_eq!(change.deletions, 0);
    }

    #[test]
    fn combined_concatenates_and_sums() {
        let builder = TranscriptDiffBuilder::new();
        let a = builder.replacement("x", "y");
        let b = builder.creation("z");
        let combined = builder.combined(&[a, b]);
        assert_eq!(combined.diff, "-x\n+y\n+z");
        assert_eq!(combined.additions, 2);
        assert_eq!(combined.deletions, 1);
    }
}
