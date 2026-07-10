//! Pure request-mapping helpers for the Phase-4 custom URI schemes.
//!
//! Faithful port of the URL-scheme routing in the macOS handlers
//! (`CmuxDiffViewerURLSchemeHandler` — `Sources/Panels/BrowserPanel.swift:1904`,
//! `2012` — and `MarkdownWebRenderer`'s `cmux-local-image` / `cmux-remote-image`
//! scheme tasks, `Sources/Panels/MarkdownWebRenderer.swift:469-551`), with the
//! WebKit `WKURLSchemeTask` streaming removed. Each helper takes a URL **string**
//! and returns the path/bytes + MIME to serve, so the parse + lookup + MIME logic
//! is unit-testable with no live WebView2. The actual `std::fs::read` of a
//! resolved on-disk file is deliberately kept OUT of these helpers (it happens in
//! the async scheme closures in `lib.rs`); only [`resolve_md_request`] returns
//! bytes, and only because [`MarkdownViewerAssets`] already holds its shell/CSS/JS
//! in memory (the lazy libs are read + cached inside the assets object, not here).
//!
//! ## Windows scheme-origin note (documented divergence)
//! WebView2 rewrites a custom scheme `foo://<host>/<path>` request to
//! `http://foo.localhost/<path>` (see the tauri
//! `register_asynchronous_uri_scheme_protocol` docs). So every parser here accepts
//! BOTH the canonical macOS custom-scheme form (`cmux-diff-viewer://<token>/…`)
//! and the Windows `http[s]://<scheme>.localhost/…` rewrite, keeping the routing
//! identical to macOS regardless of which origin the WebView delivers.

use std::path::Path;
use std::path::PathBuf;
use std::time::SystemTime;

use cmux_diff::DiffSessionRegistry;
use cmux_markdown::{resolve_local_image, MarkdownViewerAssets};

// ---------------------------------------------------------------------------
// cmux-diff-viewer://<token>/<request-path>
// ---------------------------------------------------------------------------

/// Split a `cmux-diff-viewer://<token>/<request-path>` URL (or its Windows
/// `http://cmux-diff-viewer.localhost/<token>/<request-path>` rewrite) into
/// `(token, request_path)`, where `request_path` is normalized to start with `/`.
///
/// The query/fragment are stripped. A URL with an empty token yields `None`. This
/// is the single parser shared by both the diff scheme handler and the
/// `diff_comments_rpc` trust gate (`diff::token_from_diff_viewer_url`).
pub fn parse_diff_viewer_uri(uri: &str) -> Option<(String, String)> {
    let rest = strip_scheme(uri, "cmux-diff-viewer")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let (token, path) = match rest.split_once('/') {
        Some((t, p)) => (t, p),
        None => (rest, ""),
    };
    if token.is_empty() {
        return None;
    }
    Some((token.to_string(), format!("/{path}")))
}

/// Resolve a `cmux-diff-viewer` request to the on-disk file + MIME to serve, if
/// the URL's token has a live session that registered the request path. Port of
/// the session-lookup in Swift `registeredFile(for:)`
/// (`BrowserPanel.swift:2012`); the byte read is left to the caller.
pub fn resolve_diff_request(
    registry: &DiffSessionRegistry,
    uri: &str,
    now: SystemTime,
) -> Option<(PathBuf, String)> {
    // Swift `registeredFile(for:)` (`BrowserPanel.swift:2013-2019`) hard-requires
    // `url.query == nil` AND `url.fragment == nil`: a file request that carries a
    // query or fragment resolves to NO file (the task fails). The shared
    // `parse_diff_viewer_uri` strips `?`/`#` because the trust GATE
    // (`token_from_diff_viewer_url`) must ignore the fragment the in-page router
    // rewrites onto live bridge URLs — but the file-SERVING path must not. Enforce
    // the stricter Swift guard here so a `…/asset.js?v=1` (or `#frag`) request is
    // rejected exactly as on macOS rather than being liberally served.
    if uri.contains('?') || uri.contains('#') {
        return None;
    }
    let (token, request_path) = parse_diff_viewer_uri(uri)?;
    let file = registry.registered_file(&token, &request_path, now)?;
    Some((file.file_path, file.mime_type))
}

/// Resolve a public bundled diff-webview asset (`/main.mjs`,
/// `/chunks/vendor.mjs`, ...). Session-registered files are resolved by
/// [`resolve_diff_request`]; this is only the static app bundle that generated
/// diff pages import. The asset path is jailed to
/// `<resources>/markdown-viewer/webviews-app`.
pub fn resolve_diff_asset_request(resources_root: &Path, uri: &str) -> Option<(PathBuf, String)> {
    if uri.contains('?') || uri.contains('#') {
        return None;
    }
    let (_token, request_path) = parse_diff_viewer_uri(uri)?;
    let relative = request_path.strip_prefix('/')?;
    if relative.is_empty()
        || relative.contains("..")
        || relative.contains('\\')
        || relative.starts_with('/')
    {
        return None;
    }
    let mime = diff_asset_mime(relative)?;
    let asset_root = resources_root.join("markdown-viewer").join("webviews-app");
    let asset_root = std::fs::canonicalize(asset_root).ok()?;
    let candidate = std::fs::canonicalize(asset_root.join(relative)).ok()?;
    if !candidate.starts_with(&asset_root) || !candidate.is_file() {
        return None;
    }
    Some((candidate, mime.to_string()))
}

fn diff_asset_mime(path: &str) -> Option<&'static str> {
    if path.ends_with(".html") {
        Some("text/html")
    } else if path.ends_with(".mjs") || path.ends_with(".js") {
        Some("text/javascript")
    } else if path.ends_with(".css") {
        Some("text/css")
    } else if path.ends_with(".wasm") {
        Some("application/wasm")
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// cmux-md://<host>/<asset>   (shell.html + lazy libs/css)
// ---------------------------------------------------------------------------

/// Serve a `cmux-md://…/<asset>` request from the loaded viewer assets: the
/// shell HTML for `/` or `/shell.html`, and a lazily-loaded top-level `*.js` /
/// `*.css` bundle (mermaid, vega, github-markdown, …) for a single-segment
/// filename. Returns the bytes + MIME to stream. Port of the static-asset arm of
/// `MarkdownWebRenderer`'s shell base-URL loading + `lazyAsset`.
pub fn resolve_md_request(
    assets: &MarkdownViewerAssets,
    uri: &str,
) -> Option<(Vec<u8>, &'static str)> {
    let path = md_request_path(uri)?;
    match path.as_str() {
        "/" | "/shell.html" => {
            // `is_dark` is ignored by the port (as in Swift); pass `false`.
            Some((assets.shell_html(false).into_bytes(), "text/html"))
        }
        p => {
            let name = &p[1..]; // drop the leading '/'
                                // Single-segment filenames only (no nested paths / traversal).
            if name.is_empty() || name.contains('/') || name.contains("..") {
                return None;
            }
            if let Some(stem) = name.strip_suffix(".js") {
                assets
                    .lazy_asset(stem, "js")
                    .map(|s| (s.into_bytes(), "text/javascript"))
            } else if let Some(stem) = name.strip_suffix(".css") {
                assets
                    .lazy_asset(stem, "css")
                    .map(|s| (s.into_bytes(), "text/css"))
            } else {
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// cmux-local-image://…?url=<file-url>   and   cmux-remote-image://…?url=<https>
// ---------------------------------------------------------------------------

/// Resolve a `cmux-local-image` request against the calling panel's markdown
/// document directory, returning the jailed on-disk image path + MIME. Thin
/// wrapper over the fully-ported [`cmux_markdown::resolve_local_image`] jail; the
/// byte read is left to the caller. `markdown_file_path` is the calling webview's
/// current document (keyed per-webview in `MarkdownState`).
pub fn resolve_local_image_request(
    request_url: &str,
    markdown_file_path: &str,
) -> Option<(PathBuf, String)> {
    let resolved = resolve_local_image(request_url, markdown_file_path)?;
    Some((resolved.path, resolved.mime_type))
}

/// Extract the `panelId` query value from a markdown request URL, if present.
/// Accepts both custom-scheme and WebView2-rewritten forms because only the
/// query string matters.
pub fn markdown_panel_id_from_request(uri: &str) -> Option<String> {
    let query = uri.split_once('?')?.1.split('#').next().unwrap_or_default();
    for pair in query.split('&') {
        let (name, value) = match pair.split_once('=') {
            Some(parts) => parts,
            None => (pair, ""),
        };
        if name == "panelId" && !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// Validate + canonicalize a `cmux-remote-image` request URL to the outbound
/// HTTPS image URL it is allowed to fetch (SSRF gate). Thin wrapper over
/// [`cmux_markdown::remote_image_url`]. The Tauri scheme handler owns the
/// DNS-pinned TLS fetch of this returned URL.
pub fn remote_image_request(request_url: &str) -> Option<String> {
    cmux_markdown::remote_image_url(request_url)
}

// ---------------------------------------------------------------------------
// internals
// ---------------------------------------------------------------------------

/// Strip a custom scheme prefix, accepting both `<scheme>://` and the Windows
/// `http[s]://<scheme>.localhost/` rewrite. Returns the remainder after the
/// authority (for the custom form) / after `.localhost/` (for the http form).
fn strip_scheme<'a>(uri: &'a str, scheme: &str) -> Option<&'a str> {
    let custom = format!("{scheme}://");
    if let Some(rest) = uri.strip_prefix(&custom) {
        return Some(rest);
    }
    let host = format!("{scheme}.localhost/");
    let after = uri
        .strip_prefix("http://")
        .or_else(|| uri.strip_prefix("https://"))?;
    after.strip_prefix(&host)
}

/// The request path of a `cmux-md` URL, normalized to a leading `/` with the
/// authority dropped and query/fragment stripped.
fn md_request_path(uri: &str) -> Option<String> {
    // For the custom form `cmux-md://<host>/<path>`, `strip_scheme` returns
    // `<host>/<path>` so we must drop the authority segment; for the http form it
    // already returns `<path>`.
    let rest = if let Some(after) = uri.strip_prefix("cmux-md://") {
        match after.split_once('/') {
            Some((_host, p)) => p,
            None => "",
        }
    } else {
        strip_scheme(uri, "cmux-md")?
    };
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    Some(format!("/{}", rest.trim_start_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::SystemTime;

    // --- parse_diff_viewer_uri ---

    #[test]
    fn parse_diff_uri_custom_and_http_forms() {
        assert_eq!(
            parse_diff_viewer_uri("cmux-diff-viewer://tok-abcdef0123456789/index.html"),
            Some(("tok-abcdef0123456789".into(), "/index.html".into()))
        );
        // Windows WebView2 rewrite.
        assert_eq!(
            parse_diff_viewer_uri(
                "http://cmux-diff-viewer.localhost/tok-abcdef0123456789/a/b.patch"
            ),
            Some(("tok-abcdef0123456789".into(), "/a/b.patch".into()))
        );
        // Query/fragment stripped; root path.
        assert_eq!(
            parse_diff_viewer_uri("cmux-diff-viewer://tok-xyz9876543210000/?q=1"),
            Some(("tok-xyz9876543210000".into(), "/".into()))
        );
        // Bare token, no slash → root path.
        assert_eq!(
            parse_diff_viewer_uri("cmux-diff-viewer://tok-only"),
            Some(("tok-only".into(), "/".into()))
        );
        // Wrong scheme / empty token.
        assert_eq!(parse_diff_viewer_uri("https://example.com/x"), None);
        assert_eq!(
            parse_diff_viewer_uri("cmux-diff-viewer:///index.html"),
            None
        );
    }

    // --- resolve_diff_request (parse + registry lookup + mime) ---

    #[test]
    fn resolve_diff_request_serves_registered_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let file = root.join("index.html");
        std::fs::write(&file, "<html></html>").unwrap();

        let registry = DiffSessionRegistry::new(&root);
        let now = SystemTime::now();
        let token = "tok-abcdef0123456789";
        registry
            .register(
                token,
                vec![cmux_diff::RegisteredFile {
                    request_path: "/index.html".into(),
                    file_path: file.clone(),
                    mime_type: "text/html".into(),
                }],
                now,
            )
            .expect("register");

        let uri = format!("cmux-diff-viewer://{token}/index.html");
        let (path, mime) = resolve_diff_request(&registry, &uri, now).expect("resolved");
        assert_eq!(mime, "text/html");
        assert!(path.ends_with("index.html"));

        // Unregistered path / unknown token miss.
        assert!(resolve_diff_request(
            &registry,
            &format!("cmux-diff-viewer://{token}/missing.html"),
            now
        )
        .is_none());
        assert!(resolve_diff_request(
            &registry,
            "cmux-diff-viewer://tok-not-registered-000/index.html",
            now
        )
        .is_none());

        // Swift `registeredFile(for:)` rejects a URL with any query or fragment,
        // even when the path IS registered — the file-serving path must not serve
        // `index.html?v=1` or `index.html#frag` that macOS would 404.
        assert!(
            resolve_diff_request(
                &registry,
                &format!("cmux-diff-viewer://{token}/index.html?v=1"),
                now
            )
            .is_none(),
            "a query on the file request must be rejected (Swift url.query == nil)"
        );
        assert!(
            resolve_diff_request(
                &registry,
                &format!("cmux-diff-viewer://{token}/index.html#frag"),
                now
            )
            .is_none(),
            "a fragment on the file request must be rejected (Swift url.fragment == nil)"
        );
    }

    #[test]
    fn resolve_diff_asset_request_serves_bundled_webview_assets() {
        let dir = tempfile::tempdir().unwrap();
        let app_root = dir.path().join("markdown-viewer").join("webviews-app");
        std::fs::create_dir_all(app_root.join("chunks")).unwrap();
        std::fs::write(app_root.join("main.mjs"), "import './chunks/vendor.mjs';").unwrap();
        std::fs::write(app_root.join("chunks").join("vendor.mjs"), "export {};").unwrap();

        let (main_path, main_mime) = resolve_diff_asset_request(
            dir.path(),
            "cmux-diff-viewer://tok-abcdef0123456789/main.mjs",
        )
        .expect("main asset");
        assert_eq!(main_mime, "text/javascript");
        assert!(main_path.ends_with("main.mjs"));

        let (chunk_path, chunk_mime) = resolve_diff_asset_request(
            dir.path(),
            "http://cmux-diff-viewer.localhost/tok-abcdef0123456789/chunks/vendor.mjs",
        )
        .expect("chunk asset");
        assert_eq!(chunk_mime, "text/javascript");
        assert!(chunk_path.ends_with("vendor.mjs"));
    }

    #[test]
    fn resolve_diff_asset_request_rejects_queries_traversal_and_unknown_types() {
        let dir = tempfile::tempdir().unwrap();
        let app_root = dir.path().join("markdown-viewer").join("webviews-app");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::write(app_root.join("main.mjs"), "export {};").unwrap();
        std::fs::write(app_root.join("secret.txt"), "nope").unwrap();

        assert!(resolve_diff_asset_request(
            dir.path(),
            "cmux-diff-viewer://tok-abcdef0123456789/main.mjs?v=1",
        )
        .is_none());
        assert!(resolve_diff_asset_request(
            dir.path(),
            "cmux-diff-viewer://tok-abcdef0123456789/../main.mjs",
        )
        .is_none());
        assert!(resolve_diff_asset_request(
            dir.path(),
            "cmux-diff-viewer://tok-abcdef0123456789/secret.txt",
        )
        .is_none());
    }

    // --- resolve_md_request (shell + lazy libs) ---

    /// Build a minimal but complete viewer-assets fixture under a temp dir.
    fn fixture_assets() -> (tempfile::TempDir, MarkdownViewerAssets) {
        let dir = tempfile::tempdir().unwrap();
        let viewer = dir.path().join("markdown-viewer");
        std::fs::create_dir(&viewer).unwrap();
        for (name, ext, body) in [
            ("marked.min", "js", "MARKED"),
            ("highlight.min", "js", "HLJS"),
            ("highlight-github", "css", "LIGHTCSS"),
            ("highlight-github-dark", "css", "DARKCSS"),
            ("github-markdown", "css", "GHCSS"),
            ("shell", "html", "<html>{{markedJS}}</html>"),
        ] {
            std::fs::write(viewer.join(format!("{name}.{ext}")), body).unwrap();
        }
        std::fs::write(viewer.join("mermaid.min.js"), "MERMAID_SRC").unwrap();
        let assets = MarkdownViewerAssets::load(dir.path()).unwrap();
        (dir, assets)
    }

    #[test]
    fn resolve_md_request_serves_shell_and_lazy_lib() {
        let (_dir, assets) = fixture_assets();

        let (html, mime) = resolve_md_request(&assets, "cmux-md://localhost/shell.html").unwrap();
        assert_eq!(mime, "text/html");
        let html = String::from_utf8(html).unwrap();
        assert!(
            html.contains("MARKED"),
            "shell substitutes marked.js: {html}"
        );

        // Root serves the shell too.
        assert_eq!(
            resolve_md_request(&assets, "cmux-md://localhost/")
                .unwrap()
                .1,
            "text/html"
        );

        // Windows http rewrite form + lazy mermaid lib.
        let (mermaid, mime) =
            resolve_md_request(&assets, "http://cmux-md.localhost/mermaid.min.js").unwrap();
        assert_eq!(mime, "text/javascript");
        assert_eq!(String::from_utf8(mermaid).unwrap(), "MERMAID_SRC");

        // Unknown asset / traversal rejected.
        assert!(resolve_md_request(&assets, "cmux-md://localhost/nope.js").is_none());
        assert!(resolve_md_request(&assets, "cmux-md://localhost/../secret.js").is_none());
    }

    // --- resolve_local_image_request (parse + jail + mime, byte read separated) ---

    #[test]
    fn resolve_local_image_request_jails_and_maps_mime() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("pic.png");
        let mut f = std::fs::File::create(&img).unwrap();
        f.write_all(b"\x89PNG").unwrap();
        let md = dir.path().join("doc.md");
        std::fs::write(&md, "x").unwrap();

        let file_url = {
            let s = img.to_string_lossy().replace('\\', "/");
            if s.starts_with('/') {
                format!("file://{s}")
            } else {
                format!("file:///{s}")
            }
        };
        let request = format!("cmux-local-image://image?url={file_url}");
        let (path, mime) =
            resolve_local_image_request(&request, &md.to_string_lossy()).expect("resolved");
        assert_eq!(mime, "image/png");
        assert!(path.ends_with("pic.png"));

        // Wrong scheme is rejected by the underlying jail.
        let bad = format!("https://image?url={file_url}");
        assert!(resolve_local_image_request(&bad, &md.to_string_lossy()).is_none());
    }

    #[test]
    fn markdown_panel_id_from_request_reads_the_query_value() {
        assert_eq!(
            markdown_panel_id_from_request(
                "cmux-local-image://image?panelId=surface-2&url=file:///C:/docs/a.png"
            ),
            Some("surface-2".to_string())
        );
        assert_eq!(
            markdown_panel_id_from_request(
                "http://cmux-local-image.localhost/?url=file:///C:/docs/a.png&panelId=panel-7"
            ),
            Some("panel-7".to_string())
        );
        assert_eq!(
            markdown_panel_id_from_request("cmux-local-image://image?url=file:///C:/docs/a.png"),
            None
        );
    }

    // --- remote_image_request (SSRF gate delegate) ---

    #[test]
    fn remote_image_request_admits_https_and_rejects_loopback() {
        // A public HTTPS host passes the gate and is canonicalized to its https URL.
        let ok = remote_image_request("cmux-remote-image://img?url=https://example.com/a.png");
        assert_eq!(ok.as_deref(), Some("https://example.com/a.png"));
        // Wrong scheme / disallowed target rejected.
        assert!(
            remote_image_request("cmux-remote-image://img?url=http://127.0.0.1/a.png").is_none()
        );
        assert!(remote_image_request("https://img?url=https://example.com/a.png").is_none());
    }
}
