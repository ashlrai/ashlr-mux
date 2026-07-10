//! Port of `MarkdownPanelFileLinkResolver` (`Sources/Panels/MarkdownPanelFileLinkResolver.swift`).
//!
//! Decides whether an in-document markdown link (a code span or href like
//! `foo.md`, `docs/foo.md`, `../foo.md`, `/tmp/foo.md`, `file:///…`) resolves to
//! an existing local markdown file, resolving it relative to the current
//! document's directory or the process working directory — mirroring the Swift
//! `resolve(rawPath:relativeToMarkdownFile:)` contract exactly.
//!
//! ## Windows adaptations (documented divergences from the canonical macOS source)
//!
//! The canonical source assumes POSIX path semantics (`/`-separated, absolute
//! paths start with `/`). The port preserves the *routing* faithfully but adapts
//! path recognition so it is correct on Windows:
//!
//! - Absolute-path detection uses [`std::path::Path::is_absolute`], which
//!   recognizes drive-letter (`C:\…`, `C:/…`) and UNC (`\\server\…`) roots as well
//!   as POSIX `/…`, standing in for Swift `NSString.isAbsolutePath`.
//! - A leading drive-letter token (`C:\`, `c:/`) is treated as a filesystem path,
//!   NOT a URL scheme, so a Windows path is never misclassified as a non-`file`
//!   URL and rejected. Swift's `URL(string:)` never sees Windows drive paths.
//! - `standardizingPath` is approximated by lexical normalization that collapses
//!   `.`/`..` components across BOTH separators and expands a leading `~`, which
//!   is the jail-relevant subset of Apple's behavior. Symlink resolution is left
//!   to the subsequent existence check (as on macOS, where `standardizingPath`
//!   does not resolve symlinks either).

use crate::path_util::{parse_file_url, percent_decode};
use std::path::{Path, PathBuf};

/// The markdown file extensions treated as viewer-openable, matching
/// `MarkdownPanelFileLinkResolver.markdownExtensions`.
const MARKDOWN_EXTENSIONS: [&str; 4] = ["md", "markdown", "mkd", "mdx"];

/// Port of `isMarkdownPathLike(_:)`.
///
/// Keeps the check intentionally path-like: code spans such as `foo.md`,
/// `docs/foo.md`, `../foo.md`, or `/tmp/foo.md` qualify; URLs with a non-`file`
/// scheme do not.
pub fn is_markdown_path_like(raw_path: &str) -> bool {
    let trimmed = strip_fragment_and_query(raw_path);
    if trimmed.is_empty() {
        return false;
    }
    // A URL with a scheme other than `file` is never a local markdown path.
    if let Some(scheme) = url_scheme(&trimmed) {
        if scheme != "file" {
            return false;
        }
    }
    match path_extension(&trimmed) {
        Some(ext) => MARKDOWN_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

/// Port of `resolve(rawPath:relativeToMarkdownFile:)`.
///
/// `current_directory` stands in for Swift's `FileManager.currentDirectoryPath`;
/// it is threaded explicitly so the resolver stays pure and testable rather than
/// reaching for process-global state.
pub fn resolve(
    raw_path: &str,
    markdown_file_path: &str,
    current_directory: &str,
) -> Option<String> {
    let stripped = strip_fragment_and_query(raw_path);
    if stripped.is_empty() {
        return None;
    }

    let candidate_paths: Vec<PathBuf> = if let Some(file_path) = parse_file_url(&stripped) {
        vec![PathBuf::from(file_path)]
    } else if Path::new(&stripped).is_absolute() {
        vec![PathBuf::from(&stripped)]
    } else {
        let markdown_dir = Path::new(markdown_file_path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        vec![
            markdown_dir.join(&stripped),
            Path::new(current_directory).join(&stripped),
        ]
    };

    for path in candidate_paths {
        let standardized = standardizing_path(&path);
        let standardized_str = standardized.to_string_lossy();
        if !is_markdown_path_like(&standardized_str) {
            continue;
        }
        if standardized.is_file() {
            return Some(standardized_str.into_owned());
        }
    }
    None
}

/// Port of `stripFragmentAndQuery(_:)`: trim surrounding whitespace/newlines, cut
/// at the first `#` then the first `?`, and percent-decode the remainder.
fn strip_fragment_and_query(raw_path: &str) -> String {
    let mut s = raw_path.trim().to_string();
    if let Some(hash) = s.find('#') {
        s.truncate(hash);
    }
    if let Some(question) = s.find('?') {
        s.truncate(question);
    }
    percent_decode(&s)
}

/// Return the URL scheme of `s` if it is a genuine URL, else `None`.
///
/// A leading Windows drive-letter token (`C:\`, `c:/`, or a bare `C:`) is treated
/// as a filesystem path, never a scheme — see the module-level Windows notes.
fn url_scheme(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    // Drive path: `<letter>:` optionally followed by a separator (or nothing).
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        match bytes.get(2) {
            Some(b'/') | Some(b'\\') | None => return None,
            _ => {}
        }
    }
    let colon = s.find(':')?;
    if colon == 0 {
        return None;
    }
    let scheme = &s[..colon];
    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

/// Return the lowercase-able file extension of the last path component, matching
/// `NSString.pathExtension`. Returns `None` when there is no `.`-delimited
/// extension (an empty extension is treated as absent).
fn path_extension(s: &str) -> Option<String> {
    let last = s.rsplit(['/', '\\']).next().unwrap_or(s);
    // A leading dot is not an extension separator (`.gitignore` has no extension).
    let dot = last.rfind('.')?;
    if dot == 0 {
        return None;
    }
    let ext = &last[dot + 1..];
    if ext.is_empty() {
        None
    } else {
        Some(ext.to_string())
    }
}

/// Lexical approximation of `NSString.standardizingPath`: expand a leading `~`,
/// then collapse `.` and `..` components across both separators without touching
/// the filesystem. Symlinks are intentionally left unresolved (as on macOS).
fn standardizing_path(path: &Path) -> PathBuf {
    let raw = path.to_string_lossy();
    let expanded = expand_tilde(&raw);

    // Preserve a leading root (`/`, `C:\`, `\\`) while normalizing components.
    let p = Path::new(&expanded);
    let mut components: Vec<String> = Vec::new();
    let mut prefix = String::new();
    let mut has_root = false;

    use std::path::Component;
    for comp in p.components() {
        match comp {
            Component::Prefix(pre) => prefix = pre.as_os_str().to_string_lossy().into_owned(),
            Component::RootDir => has_root = true,
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(components.last().map(String::as_str), Some("..") | None) && !has_root {
                    components.push("..".to_string());
                } else if components.is_empty() {
                    // Absolute path: `..` above root is dropped (matches macOS).
                    if !has_root {
                        components.push("..".to_string());
                    }
                } else {
                    components.pop();
                }
            }
            Component::Normal(seg) => components.push(seg.to_string_lossy().into_owned()),
        }
    }

    let mut out = String::new();
    out.push_str(&prefix);
    let sep = if prefix.contains('\\') || raw.contains('\\') {
        '\\'
    } else {
        std::path::MAIN_SEPARATOR
    };
    if has_root {
        out.push(sep);
    }
    out.push_str(&components.join(&sep.to_string()));
    if out.is_empty() {
        out.push('.');
    }
    PathBuf::from(out)
}

/// Expand a leading `~` to the user's home directory (`$HOME` on unix,
/// `%USERPROFILE%` on Windows), matching the tilde handling in
/// `standardizingPath`.
fn expand_tilde(s: &str) -> String {
    if s == "~" || s.starts_with("~/") || s.starts_with("~\\") {
        if let Some(home) = home_dir() {
            let rest = &s[1..];
            return format!("{home}{rest}");
        }
    }
    s.to_string()
}

fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .filter(|h| !h.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_extensions_recognized() {
        for ext in ["md", "markdown", "mkd", "mdx", "MD", "Markdown"] {
            assert!(
                is_markdown_path_like(&format!("docs/readme.{ext}")),
                "{ext}"
            );
        }
    }

    #[test]
    fn non_markdown_extensions_rejected() {
        assert!(!is_markdown_path_like("docs/readme.txt"));
        assert!(!is_markdown_path_like("image.png"));
        assert!(!is_markdown_path_like("noextension"));
        assert!(!is_markdown_path_like(""));
    }

    #[test]
    fn http_urls_are_not_markdown_paths() {
        assert!(!is_markdown_path_like("https://example.com/readme.md"));
        assert!(!is_markdown_path_like("mailto:foo.md"));
    }

    #[test]
    fn file_urls_are_markdown_paths() {
        assert!(is_markdown_path_like("file:///tmp/readme.md"));
    }

    #[test]
    fn fragment_and_query_are_stripped() {
        assert!(is_markdown_path_like("readme.md#section"));
        assert!(is_markdown_path_like("readme.md?v=1"));
        assert_eq!(strip_fragment_and_query("  readme.md#x  "), "readme.md");
        assert_eq!(strip_fragment_and_query("a%20b.md"), "a b.md");
    }

    #[test]
    fn windows_drive_path_is_not_a_url() {
        assert_eq!(url_scheme("C:/docs/readme.md"), None);
        assert_eq!(url_scheme("c:\\docs\\readme.md"), None);
        assert!(is_markdown_path_like("C:/docs/readme.md"));
    }

    #[test]
    fn url_scheme_detection() {
        assert_eq!(url_scheme("https://x").as_deref(), Some("https"));
        assert_eq!(url_scheme("file:///x").as_deref(), Some("file"));
        assert_eq!(url_scheme("relative/path.md"), None);
    }

    #[test]
    fn standardizing_collapses_dot_segments() {
        assert_eq!(
            standardizing_path(Path::new("/a/b/../c/./d.md")),
            PathBuf::from(format!(
                "{}a{}c{}d.md",
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR,
                std::path::MAIN_SEPARATOR
            ))
        );
    }

    #[test]
    fn resolve_relative_to_markdown_dir() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("linked.md");
        std::fs::write(&target, "# hi").unwrap();
        let md = dir.path().join("index.md");
        std::fs::write(&md, "see [linked](linked.md)").unwrap();

        let resolved = resolve("linked.md", &md.to_string_lossy(), "/nonexistent-cwd");
        assert!(resolved.is_some(), "should resolve sibling markdown file");
        assert!(resolved.unwrap().ends_with("linked.md"));
    }

    #[test]
    fn resolve_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("abs.md");
        std::fs::write(&target, "# hi").unwrap();

        let resolved = resolve(
            &target.to_string_lossy(),
            "/some/other/index.md",
            "/nonexistent-cwd",
        );
        assert!(resolved.is_some());
    }

    #[test]
    fn resolve_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("index.md");
        std::fs::write(&md, "x").unwrap();
        assert_eq!(
            resolve("does-not-exist.md", &md.to_string_lossy(), "/nope"),
            None
        );
    }

    #[test]
    fn resolve_non_markdown_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("data.txt");
        std::fs::write(&target, "x").unwrap();
        let md = dir.path().join("index.md");
        std::fs::write(&md, "x").unwrap();
        assert_eq!(resolve("data.txt", &md.to_string_lossy(), "/nope"), None);
    }

    #[test]
    fn resolve_directory_is_not_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub.md");
        std::fs::create_dir(&sub).unwrap();
        let md = dir.path().join("index.md");
        std::fs::write(&md, "x").unwrap();
        assert_eq!(resolve("sub.md", &md.to_string_lossy(), "/nope"), None);
    }
}
