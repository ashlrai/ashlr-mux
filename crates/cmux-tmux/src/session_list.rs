//! Parses the delimited output of `tmux list-sessions -F` into sessions.
//!
//! Ported from `RemoteTmuxSessionListParser.swift`.
//!
//! The expected per-line format (set by the SSH transport) is:
//! `#{session_id}:#{session_windows}:#{session_attached}:#{session_created}:#{session_name}`
//!
//! `session_name` is placed **last** because it is the only free-text field; the
//! leading fields are a `$N` id, integer counts, and a unix timestamp, none of
//! which contain the `:` delimiter. The name is therefore parsed as the whole
//! remainder after the fourth delimiter, so a name is reproduced verbatim even if
//! it somehow contained a `:` (tmux already rewrites `:` in session names to `_`,
//! so this is defense in depth).
//!
//! The delimiter is the printable `:` rather than a control character such as
//! tab: when the remote tmux client is not flagged UTF-8, tmux runs `-F` output
//! through `utf8_sanitize()`, which rewrites every non-printable-ASCII byte —
//! including tab — to `_`. A printable delimiter is preserved under any locale.
//!
//! Parsing is deliberately lenient: malformed or short lines are skipped rather
//! than failing the whole listing, so a single odd session never hides the rest
//! of the sidebar.

use crate::models::RemoteTmuxSession;

/// The field delimiter. A printable byte tmux preserves in `-F` output (it
/// rewrites control bytes like tab to `_`), and one tmux forbids inside a session
/// name (it rewrites `:` in names to `_`), so it cannot collide with any leading
/// field value.
pub const FIELD_DELIMITER: &str = ":";

/// The `-F` format string this parser expects, ordered to match [`parse`] with
/// the free-text `session_name` last.
pub const FORMAT_STRING: &str =
    "#{session_id}:#{session_windows}:#{session_attached}:#{session_created}:#{session_name}";

/// Parses raw `list-sessions` stdout into structured sessions.
///
/// Returns one [`RemoteTmuxSession`] per well-formed line, in input order.
pub fn parse(output: &str) -> Vec<RemoteTmuxSession> {
    let mut sessions: Vec<RemoteTmuxSession> = Vec::new();
    // Swift `output.split(separator: "\n", omittingEmptySubsequences: true)`.
    for raw_line in output.split('\n') {
        let mut line = raw_line;
        if line.ends_with('\r') {
            line = &line[..line.len() - 1];
        }
        if line.is_empty() {
            continue;
        }
        // Unbounded split: the first four fields are id/windows/attached/created,
        // and the name (which may itself contain `:`) is reassembled from the
        // remainder below, so a name with embedded delimiters is preserved.
        let fields: Vec<&str> = line.split(FIELD_DELIMITER).collect();
        // Need at least id + windows + attached + created + name.
        if fields.len() < 5 {
            continue;
        }
        let id = trim_whitespaces(fields[0]);
        if id.is_empty() {
            continue;
        }
        let window_count = trim_whitespaces(fields[1]).parse::<i64>().unwrap_or(0);
        let attached = trim_whitespaces(fields[2]).parse::<i64>().unwrap_or(0) > 0;
        let created_unix = trim_whitespaces(fields[3]).parse::<i64>().ok();
        // The name is the remainder, rejoined so an embedded delimiter (should one
        // ever survive) is preserved rather than truncating the name.
        let name = fields[4..].join(FIELD_DELIMITER);
        sessions.push(RemoteTmuxSession::new(
            id.to_string(),
            name,
            window_count,
            attached,
            created_unix,
        ));
    }
    sessions
}

/// Trims characters in Foundation's `CharacterSet.whitespaces` (space, tab, and
/// Unicode `Zs` separators) — but NOT newlines. Mirrors Swift
/// `trimmingCharacters(in: .whitespaces)`.
///
// DIVERGENCE: Rust `str::trim` would also strip the newline family, which
// `CharacterSet.whitespaces` deliberately excludes; we exclude those explicitly.
fn trim_whitespaces(s: &str) -> &str {
    s.trim_matches(|c: char| is_horizontal_whitespace(c))
}

fn is_horizontal_whitespace(c: char) -> bool {
    // `CharacterSet.whitespaces` = space, tab, and category Zs — never the
    // newline/line-separator family that `char::is_whitespace` also reports.
    c == '\t'
        || (c.is_whitespace()
            && !matches!(
                c,
                '\n' | '\r' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
            ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_lines() {
        let out = "$0:2:1:1700000000:main\n$1:1:0:1700000100:work\n";
        let sessions = parse(out);
        assert_eq!(sessions.len(), 2);
        assert_eq!(
            sessions[0],
            RemoteTmuxSession::new("$0".into(), "main".into(), 2, true, Some(1700000000))
        );
        assert_eq!(
            sessions[1],
            RemoteTmuxSession::new("$1".into(), "work".into(), 1, false, Some(1700000100))
        );
    }

    #[test]
    fn attached_is_true_only_when_positive() {
        let sessions = parse("$0:1:0:0:a\n$1:1:1:0:b\n$2:1:5:0:c\n");
        assert!(!sessions[0].attached);
        assert!(sessions[1].attached);
        assert!(sessions[2].attached);
    }

    #[test]
    fn short_lines_are_skipped_leniently() {
        // Only 4 fields → skipped; the following good line still parses.
        let sessions = parse("$0:1:0:0\n$1:2:1:1700000000:ok\n");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "$1");
    }

    #[test]
    fn empty_id_line_is_skipped() {
        let sessions = parse(":1:0:0:noid\n$5:1:0:0:good\n");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "$5");
    }

    #[test]
    fn name_with_embedded_colon_is_preserved() {
        // A name with a `:` (defense in depth) is reassembled from the remainder.
        let sessions = parse("$0:1:0:0:weird:name:here\n");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "weird:name:here");
    }

    #[test]
    fn non_numeric_counts_default_to_zero_and_none() {
        let sessions = parse("$0:abc:xyz:notime:n\n");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].window_count, 0);
        assert!(!sessions[0].attached);
        assert_eq!(sessions[0].created_unix, None);
    }

    #[test]
    fn crlf_line_endings_are_trimmed() {
        let sessions = parse("$0:1:0:0:main\r\n");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "main");
    }

    #[test]
    fn blank_lines_are_skipped() {
        let sessions = parse("\n\n$0:1:0:0:a\n\n");
        assert_eq!(sessions.len(), 1);
    }

    #[test]
    fn whitespace_around_numeric_fields_is_trimmed() {
        let sessions = parse("$0: 3 : 1 : 1700000000 :main\n");
        assert_eq!(sessions[0].window_count, 3);
        assert!(sessions[0].attached);
        assert_eq!(sessions[0].created_unix, Some(1700000000));
    }
}
