//! Ctrl-click link detection (M2 WS5).
//!
//! Scans a line of terminal text for openable targets — URLs and filesystem
//! paths — returning their byte ranges so the compositor can hit-test a
//! Ctrl-click against them (the Windows analogue of cmux's Cmd-click handling,
//! ported from Ghostty fork patch #12's URL/path bounds). Opening the target
//! via `ShellExecuteW` is the GUI half and lands with the input layer (WS4);
//! this module is the pure, parity-tested detection half.
//!
//! v1 scope: `http(s)://` and `file://` URLs, drive-letter (`C:\…`) and UNC
//! (`\\host\share`) Windows paths, and POSIX absolute paths. Paths containing
//! spaces are not detected (they need surrounding-quote heuristics — deferred).

use std::ops::Range;
use std::sync::OnceLock;

use regex::Regex;

/// What kind of target a [`Link`] points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Url,
    Path,
}

/// A detected link within a line: its kind, the byte range it occupies in the
/// source line, and the matched text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub kind: LinkKind,
    pub range: Range<usize>,
    pub text: String,
}

fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)\b(?:https?|file)://[^\s<>"'`]+"#).unwrap())
}

fn path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Drive-letter path (C:\ or C:/), UNC (\\host\share), or POSIX absolute
    // (/usr/...). Stops at whitespace and shell/quoting metacharacters.
    RE.get_or_init(|| Regex::new(r#"(?:[A-Za-z]:[\\/]|\\\\|/)[^\s<>"'`|*?]+"#).unwrap())
}

/// Trailing characters trimmed from a match — sentence/markup punctuation that
/// is almost never part of the actual target (e.g. `see https://a.com).`).
const TRAILING_TRIM: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '>', '\'', '"'];

/// Trim trailing punctuation from `m` (a byte range into `line`), returning the
/// tightened range, or `None` if nothing is left.
fn trim_trailing(line: &str, mut range: Range<usize>) -> Option<Range<usize>> {
    while range.end > range.start {
        let last = line[range.clone()].chars().next_back()?;
        if TRAILING_TRIM.contains(&last) {
            range.end -= last.len_utf8();
        } else {
            break;
        }
    }
    (range.end > range.start).then_some(range)
}

/// Find all openable links in `line`, ordered by start position. URLs take
/// precedence; path matches overlapping a URL are dropped (a `file://` URL is
/// not also reported as a path).
pub fn find_links(line: &str) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();

    for m in url_regex().find_iter(line) {
        if let Some(range) = trim_trailing(line, m.range()) {
            links.push(Link {
                kind: LinkKind::Url,
                text: line[range.clone()].to_owned(),
                range,
            });
        }
    }

    for m in path_regex().find_iter(line) {
        let overlaps_url = links
            .iter()
            .any(|l| m.start() < l.range.end && l.range.start < m.end());
        if overlaps_url {
            continue;
        }
        if let Some(range) = trim_trailing(line, m.range()) {
            links.push(Link {
                kind: LinkKind::Path,
                text: line[range.clone()].to_owned(),
                range,
            });
        }
    }

    links.sort_by_key(|l| l.range.start);
    links
}

/// The link, if any, whose range contains byte offset `column` — the hit-test
/// for a Ctrl-click at `column` in `line`.
pub fn link_at(line: &str, column: usize) -> Option<Link> {
    find_links(line)
        .into_iter()
        .find(|l| l.range.contains(&column))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_bare_https_url() {
        let links = find_links("see https://example.com/a/b for more");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, LinkKind::Url);
        assert_eq!(links[0].text, "https://example.com/a/b");
        assert_eq!(
            &"see https://example.com/a/b for more"[links[0].range.clone()],
            "https://example.com/a/b"
        );
    }

    #[test]
    fn trims_trailing_sentence_punctuation() {
        assert_eq!(find_links("visit https://a.com.")[0].text, "https://a.com");
        assert_eq!(find_links("(https://a.com)")[0].text, "https://a.com");
        assert_eq!(
            find_links("ok: https://a.com/x?y=1!")[0].text,
            "https://a.com/x?y=1"
        );
    }

    #[test]
    fn finds_file_url_not_as_path() {
        let links = find_links("open file:///C:/tmp/x.log now");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, LinkKind::Url);
        assert_eq!(links[0].text, "file:///C:/tmp/x.log");
    }

    #[test]
    fn finds_windows_drive_path() {
        let links = find_links(r"edit C:\Users\me\file.txt please");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].kind, LinkKind::Path);
        assert_eq!(links[0].text, r"C:\Users\me\file.txt");
    }

    #[test]
    fn finds_unc_path() {
        let links = find_links(r"share \\server\pub\readme.md");
        assert_eq!(links[0].kind, LinkKind::Path);
        assert_eq!(links[0].text, r"\\server\pub\readme.md");
    }

    #[test]
    fn finds_posix_absolute_path() {
        let links = find_links("tail /var/log/syslog now");
        assert_eq!(links[0].kind, LinkKind::Path);
        assert_eq!(links[0].text, "/var/log/syslog");
    }

    #[test]
    fn no_links_in_plain_text() {
        assert!(find_links("just some words, nothing here").is_empty());
    }

    #[test]
    fn multiple_links_ordered_by_position() {
        let links = find_links("a https://x.com b /etc/hosts c");
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].kind, LinkKind::Url);
        assert_eq!(links[1].kind, LinkKind::Path);
        assert!(links[0].range.start < links[1].range.start);
    }

    #[test]
    fn link_at_hit_tests_a_column() {
        let line = "go https://example.com end";
        // Column inside the URL.
        let hit = link_at(line, 10).expect("hit");
        assert_eq!(hit.text, "https://example.com");
        // Column in the trailing word.
        assert!(link_at(line, 24).is_none());
    }
}
