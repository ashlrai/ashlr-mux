//! Port of the local-image path jail — `MarkdownWebRenderer.Coordinator.localImageFileURL`
//! and `localImageMimeType` (`Sources/Panels/MarkdownWebRenderer.swift:553-603`).
//!
//! The markdown viewer serves local images through a custom `cmux-local-image://`
//! scheme so a rendered document can reference an on-disk sibling image without
//! being granted arbitrary filesystem read access. This module is the security
//! boundary: it admits a requested file **only** when the (symlink-resolved) file
//! lives strictly under the (symlink-resolved) directory of the markdown document
//! being viewed AND its extension is an allowed image type. Anything else — a
//! traversal (`../secret.png`), a sibling-prefix directory (`…/docsEVIL/x.png`
//! against a `…/docs/` jail), a non-`file` URL, a missing `url` query, a
//! disallowed extension, or a markdown file that sits at a filesystem root — is
//! rejected.
//!
//! ## Windows adaptations (documented divergences)
//! - Swift rejects `markdownDirectory.path == "/"` (the POSIX root). Windows has
//!   no single `/` root, so the port rejects when the markdown directory has no
//!   parent (a drive or UNC root), and builds the jail root with a trailing
//!   [`std::path::MAIN_SEPARATOR`] instead of `/` — mirroring `cmux-diff`'s
//!   `session::is_trusted_file`.
//! - Swift's `standardizedFileURL.resolvingSymlinksInPath` tolerates a missing
//!   path; the port uses [`std::fs::canonicalize`], which requires existence.
//!   That is stricter and correct here: a servable image must exist anyway, and
//!   full symlink resolution before the prefix check is the right anti-escape
//!   choice (same discipline as the diff-viewer jail).

use crate::path_util::{canonicalize_normalized, parse_file_url, percent_decode};
use std::path::{Path, PathBuf};

/// The custom URL scheme the markdown viewer uses for local images, matching
/// `MarkdownWebRenderer.localImageURLScheme`.
pub const LOCAL_IMAGE_URL_SCHEME: &str = "cmux-local-image";

/// A local image that passed the jail, ready to be read and served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLocalImage {
    /// The canonical (symlink-resolved, Windows-normalized) on-disk path.
    pub path: PathBuf,
    /// The MIME type derived from the file extension.
    pub mime_type: String,
}

/// Resolve a `cmux-local-image://…?url=<file-url>` request against the directory
/// of `markdown_file_path`, returning the servable image only when it passes the
/// jail. Port of `localImageFileURL(from:)`.
pub fn resolve_local_image(
    request_url: &str,
    markdown_file_path: &str,
) -> Option<ResolvedLocalImage> {
    // 1. The request must use the local-image scheme and carry a `url` query
    //    item whose value is a `file:` URL.
    let scheme = url_scheme(request_url)?;
    if scheme.to_ascii_lowercase() != LOCAL_IMAGE_URL_SCHEME {
        return None;
    }
    let raw_file_url = query_value(request_url, "url")?;
    let requested_path = parse_file_url(&raw_file_url)?;

    // 2. The markdown document must have a non-empty path.
    let markdown_file_path = markdown_file_path.trim();
    if markdown_file_path.is_empty() {
        return None;
    }

    // 3. Resolve the markdown directory and reject a filesystem-root document.
    let markdown_dir = Path::new(markdown_file_path).parent()?;
    let markdown_root = canonicalize_normalized(markdown_dir)?;
    // Reject when the markdown directory is itself a filesystem root (Swift
    // rejects `== "/"`; Windows: no parent means a drive/UNC root).
    Path::new(&markdown_root).parent()?;

    // 4. Build the jail root with a trailing separator so a sibling directory
    //    whose name merely shares a prefix (`…/docsEVIL`) cannot pass the check.
    let sep = std::path::MAIN_SEPARATOR;
    let jail_root = if markdown_root.ends_with(sep) {
        markdown_root.clone()
    } else {
        format!("{markdown_root}{sep}")
    };

    // 5. Canonicalize the requested file and require it to sit under the jail and
    //    carry an allowed image extension.
    let candidate = canonicalize_normalized(Path::new(&requested_path))?;
    if !candidate.starts_with(&jail_root) {
        return None;
    }
    let mime = local_image_mime_type(extension_of(&candidate))?;
    Some(ResolvedLocalImage {
        path: PathBuf::from(candidate),
        mime_type: mime.to_string(),
    })
}

/// Port of `localImageMimeType(for:)`: the image extension allowlist. Returns
/// `None` for any extension that is not a supported inline image type.
pub fn local_image_mime_type(path_extension: &str) -> Option<&'static str> {
    match path_extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "avif" => Some("image/avif"),
        _ => None,
    }
}

/// The scheme of a URL string (`cmux-local-image://x` -> `cmux-local-image`), or
/// `None` when there is no scheme.
fn url_scheme(s: &str) -> Option<String> {
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
    Some(scheme.to_string())
}

/// Extract the first query-item value for `key` from a URL string,
/// percent-decoding it (mirrors `URLComponents.queryItems`).
fn query_value(url: &str, key: &str) -> Option<String> {
    let query = url.split_once('?').map(|(_, q)| q)?;
    // Strip a trailing fragment.
    let query = query.split_once('#').map_or(query, |(q, _)| q);
    for pair in query.split('&') {
        let (k, v) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        // Query components encode spaces as '+'.
        if percent_decode(&k.replace('+', " ")) == key {
            return Some(percent_decode(&v.replace('+', " ")));
        }
    }
    None
}

/// The lowercase-able extension of the last path component (no extension -> "").
fn extension_of(path: &str) -> &str {
    let last = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match last.rfind('.') {
        Some(dot) if dot > 0 => &last[dot + 1..],
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_url(p: &Path) -> String {
        // Build a file: URL for a path, percent-encoding nothing exotic in tests.
        let s = p.to_string_lossy().replace('\\', "/");
        if s.starts_with('/') {
            format!("file://{s}")
        } else {
            // Windows drive path -> file:///C:/...
            format!("file:///{s}")
        }
    }

    fn request(image: &Path) -> String {
        format!("{}://image?url={}", LOCAL_IMAGE_URL_SCHEME, file_url(image))
    }

    #[test]
    fn mime_allowlist() {
        assert_eq!(local_image_mime_type("png"), Some("image/png"));
        assert_eq!(local_image_mime_type("JPG"), Some("image/jpeg"));
        assert_eq!(local_image_mime_type("jpeg"), Some("image/jpeg"));
        assert_eq!(local_image_mime_type("gif"), Some("image/gif"));
        assert_eq!(local_image_mime_type("webp"), Some("image/webp"));
        assert_eq!(local_image_mime_type("avif"), Some("image/avif"));
        assert_eq!(local_image_mime_type("svg"), None);
        assert_eq!(local_image_mime_type("txt"), None);
    }

    #[test]
    fn image_under_markdown_dir_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("pic.png");
        std::fs::write(&img, b"\x89PNG").unwrap();
        let md = dir.path().join("doc.md");
        std::fs::write(&md, "x").unwrap();

        let resolved = resolve_local_image(&request(&img), &md.to_string_lossy());
        assert!(resolved.is_some(), "sibling png should resolve");
        let resolved = resolved.unwrap();
        assert_eq!(resolved.mime_type, "image/png");
        assert!(resolved.path.ends_with("pic.png"));
    }

    #[test]
    fn traversal_outside_jail_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let jail = root.path().join("docs");
        std::fs::create_dir(&jail).unwrap();
        let secret = root.path().join("secret.png");
        std::fs::write(&secret, b"x").unwrap();
        let md = jail.join("doc.md");
        std::fs::write(&md, "x").unwrap();

        // The image lives in the PARENT of the markdown dir — must be rejected.
        let resolved = resolve_local_image(&request(&secret), &md.to_string_lossy());
        assert!(resolved.is_none(), "parent-dir image must be jailed out");
    }

    #[test]
    fn sibling_prefix_dir_is_rejected() {
        let root = tempfile::tempdir().unwrap();
        let jail = root.path().join("docs");
        std::fs::create_dir(&jail).unwrap();
        let evil = root.path().join("docsEVIL");
        std::fs::create_dir(&evil).unwrap();
        let evil_img = evil.join("x.png");
        std::fs::write(&evil_img, b"x").unwrap();
        let md = jail.join("doc.md");
        std::fs::write(&md, "x").unwrap();

        let resolved = resolve_local_image(&request(&evil_img), &md.to_string_lossy());
        assert!(
            resolved.is_none(),
            "a sibling dir sharing the jail's name prefix must be rejected"
        );
    }

    #[test]
    fn disallowed_extension_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let svg = dir.path().join("pic.svg");
        std::fs::write(&svg, b"<svg/>").unwrap();
        let md = dir.path().join("doc.md");
        std::fs::write(&md, "x").unwrap();
        assert!(resolve_local_image(&request(&svg), &md.to_string_lossy()).is_none());
    }

    #[test]
    fn wrong_scheme_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("pic.png");
        std::fs::write(&img, b"x").unwrap();
        let md = dir.path().join("doc.md");
        std::fs::write(&md, "x").unwrap();
        let req = format!("https://image?url={}", file_url(&img));
        assert!(resolve_local_image(&req, &md.to_string_lossy()).is_none());
    }

    #[test]
    fn missing_url_query_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("doc.md");
        std::fs::write(&md, "x").unwrap();
        let req = format!("{}://image?other=1", LOCAL_IMAGE_URL_SCHEME);
        assert!(resolve_local_image(&req, &md.to_string_lossy()).is_none());
    }

    #[test]
    fn non_file_url_value_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("doc.md");
        std::fs::write(&md, "x").unwrap();
        let req = format!("{}://image?url=https://evil/x.png", LOCAL_IMAGE_URL_SCHEME);
        assert!(resolve_local_image(&req, &md.to_string_lossy()).is_none());
    }

    #[test]
    fn empty_markdown_path_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("pic.png");
        std::fs::write(&img, b"x").unwrap();
        assert!(resolve_local_image(&request(&img), "   ").is_none());
    }

    #[test]
    fn query_value_percent_decodes() {
        assert_eq!(
            query_value("x://y?url=file%3A%2F%2F%2Ftmp%2Fa.png", "url").as_deref(),
            Some("file:///tmp/a.png")
        );
    }
}
