//! Shared path + URL helpers.
//!
//! The Windows path-normalization helpers (`canonicalize_normalized`,
//! `normalize_windows_path`, `lower_drive_letter`) are duplicated from
//! `cmux-diff`'s `comment_store` (where they are `pub(crate)` and therefore not
//! importable) so `cmux-markdown` does not take a dependency on `cmux-diff`. They
//! carry the same contract: strip the Windows `\\?\` verbatim prefix and
//! lowercase the drive letter so canonicalized paths compare stably.

use std::path::Path;

/// Percent-decode a string, mirroring Swift `removingPercentEncoding`: decode
/// valid `%XX` escapes, pass malformed escapes through unchanged, and fall back
/// to the original text if the decoded bytes are not valid UTF-8.
pub(crate) fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// If `s` is a `file:` URL, return the filesystem path it denotes (mirrors
/// Swift `URL(string:).path` for a file URL). Returns `None` for non-`file` URLs.
///
/// On Windows a `file:///C:/x` path decodes to `/C:/x`; the leading slash before
/// a drive letter is dropped so it becomes a valid `C:/x` filesystem path.
pub(crate) fn parse_file_url(s: &str) -> Option<String> {
    let rest = s.strip_prefix("file:").or_else(|| s.strip_prefix("FILE:"))?;
    let path = if let Some(after_slashes) = rest.strip_prefix("//") {
        match after_slashes.find('/') {
            Some(idx) => &after_slashes[idx..],
            None => "",
        }
    } else {
        rest
    };
    let decoded = percent_decode(path);
    let bytes = decoded.as_bytes();
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return Some(decoded[1..].to_string());
    }
    Some(decoded)
}

/// Canonicalize a path (resolving symlinks) and strip the Windows verbatim
/// prefix + lowercase the drive letter. Returns `None` when the path does not
/// exist. Used by the local-image jail, which serves only real files and treats
/// canonicalization as the security boundary (mirrors `cmux-diff` session jail
/// discipline; stricter than Swift `resolvingSymlinksInPath`, which tolerates
/// missing paths — documented divergence, safe because a servable image must
/// exist anyway and full symlink resolution is the correct anti-escape choice).
pub(crate) fn canonicalize_normalized(path: &Path) -> Option<String> {
    std::fs::canonicalize(path)
        .ok()
        .map(|p| normalize_windows_path(&p.to_string_lossy()))
}

/// Strips the Windows `\\?\` (and `\\?\UNC\`) verbatim prefix and lowercases the
/// drive letter. A no-op on non-verbatim / non-drive paths.
pub(crate) fn normalize_windows_path(s: &str) -> String {
    let stripped = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    };
    lower_drive_letter(&stripped)
}

/// Lowercases a leading `X:` drive letter (`C:\foo` -> `c:\foo`).
fn lower_drive_letter(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let mut out = String::with_capacity(s.len());
        out.push((bytes[0] as char).to_ascii_lowercase());
        out.push_str(&s[1..]);
        out
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_basic() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("no-escapes"), "no-escapes");
        assert_eq!(percent_decode("bad%zzescape"), "bad%zzescape");
    }

    #[test]
    fn parse_file_url_posix_and_windows() {
        assert_eq!(parse_file_url("file:///tmp/a.png").as_deref(), Some("/tmp/a.png"));
        assert_eq!(
            parse_file_url("file:///C:/img/a.png").as_deref(),
            Some("C:/img/a.png")
        );
        assert_eq!(
            parse_file_url("file://localhost/tmp/a.png").as_deref(),
            Some("/tmp/a.png")
        );
        assert_eq!(parse_file_url("https://x/a.png"), None);
    }

    #[test]
    fn normalize_strips_verbatim_and_lowercases_drive() {
        assert_eq!(normalize_windows_path(r"\\?\C:\foo"), r"c:\foo");
        assert_eq!(normalize_windows_path(r"\\?\UNC\server\share"), r"\\server\share");
        assert_eq!(normalize_windows_path("/plain/posix"), "/plain/posix");
    }
}
