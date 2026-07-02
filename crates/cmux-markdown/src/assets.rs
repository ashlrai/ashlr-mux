//! Port of `MarkdownViewerAssets` (`Sources/Panels/MarkdownViewerAssets.swift`).
//!
//! Loads the bundled markdown-viewer assets from a resources directory and fills
//! the six `{{token}}` placeholders in `shell.html`. Heavy diagram libraries
//! (Mermaid/Vega) are still read lazily so an ordinary markdown file does not pay
//! their I/O cost. The `localizedStringsJSON` remote-image string map is embedded
//! verbatim (English fallbacks; localization is deferred alongside the rest of
//! the Windows-port i18n work, whose source of truth is
//! `Resources/Localizable.xcstrings`).
//!
//! Faithful to Swift, `shell_html` applies the six substitutions sequentially
//! (`replacingOccurrences`); real asset content never contains these tokens, so
//! the result is identical to a single-pass replacement. Like Swift, `isDark` is
//! accepted but ignored — both hljs stylesheets are always injected and the theme
//! is switched later via CSS variables / `matchMedia`.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The bundled assets subdirectory, relative to the resources root (mirrors the
/// Swift `subdirectory: "markdown-viewer"`).
pub const MARKDOWN_VIEWER_SUBDIR: &str = "markdown-viewer";

/// Loaded markdown-viewer assets + the lazy on-demand cache.
pub struct MarkdownViewerAssets {
    viewer_dir: PathBuf,
    marked_js: String,
    highlight_js: String,
    highlight_light_css: String,
    highlight_dark_css: String,
    github_markdown_css: String,
    shell_template: String,
    localized_strings_json: String,
    lazy_cache: Mutex<HashMap<String, String>>,
}

/// An asset that could not be found under the resources directory.
#[derive(Debug)]
pub struct MissingAsset {
    pub name: String,
    pub ext: String,
}

impl std::fmt::Display for MissingAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "missing bundled markdown viewer asset {}.{}", self.name, self.ext)
    }
}

impl std::error::Error for MissingAsset {}

impl MarkdownViewerAssets {
    /// Load the required assets from `<resources_root>/markdown-viewer`.
    ///
    /// `resources_root` is the Tauri bundle resources directory (the Windows
    /// analog of `Bundle.main`). Swift `preconditionFailure`s on a missing
    /// asset; the port returns [`MissingAsset`] so the caller decides.
    pub fn load(resources_root: &Path) -> Result<Self, MissingAsset> {
        let viewer_dir = resources_root.join(MARKDOWN_VIEWER_SUBDIR);
        Ok(MarkdownViewerAssets {
            marked_js: load_asset(&viewer_dir, "marked.min", "js")?,
            highlight_js: load_asset(&viewer_dir, "highlight.min", "js")?,
            highlight_light_css: load_asset(&viewer_dir, "highlight-github", "css")?,
            highlight_dark_css: load_asset(&viewer_dir, "highlight-github-dark", "css")?,
            github_markdown_css: load_asset(&viewer_dir, "github-markdown", "css")?,
            shell_template: load_asset(&viewer_dir, "shell", "html")?,
            localized_strings_json: localized_strings_json(),
            lazy_cache: Mutex::new(HashMap::new()),
            viewer_dir,
        })
    }

    /// Fill the six `shell.html` placeholders (port of `shellHTML(isDark:)`).
    /// `is_dark` is accepted for signature parity but ignored, exactly as Swift.
    pub fn shell_html(&self, is_dark: bool) -> String {
        let _ = is_dark;
        substitute_shell_template(
            &self.shell_template,
            &ShellSubstitutions {
                github_markdown_css: &self.github_markdown_css,
                highlight_light_css: &self.highlight_light_css,
                highlight_dark_css: &self.highlight_dark_css,
                marked_js: &self.marked_js,
                highlight_js: &self.highlight_js,
                localized_strings_json: &self.localized_strings_json,
            },
        )
    }

    /// Load and cache a bundled JS asset on demand (port of `lazyAsset(name:ext:)`).
    /// Returns `None` if the asset is absent (Swift `preconditionFailure`s; the
    /// port degrades so a missing optional diagram library cannot crash the host).
    pub fn lazy_asset(&self, name: &str, ext: &str) -> Option<String> {
        let key = format!("{name}.{ext}");
        if let Some(cached) = self.lazy_cache.lock().unwrap().get(&key) {
            return Some(cached.clone());
        }
        let source = load_asset(&self.viewer_dir, name, ext).ok()?;
        self.lazy_cache
            .lock()
            .unwrap()
            .insert(key, source.clone());
        Some(source)
    }

    /// The embedded remote-image localized-strings JSON.
    pub fn localized_strings_json(&self) -> &str {
        &self.localized_strings_json
    }
}

/// The six substitution values for the shell template.
struct ShellSubstitutions<'a> {
    github_markdown_css: &'a str,
    highlight_light_css: &'a str,
    highlight_dark_css: &'a str,
    marked_js: &'a str,
    highlight_js: &'a str,
    localized_strings_json: &'a str,
}

fn substitute_shell_template(template: &str, subs: &ShellSubstitutions<'_>) -> String {
    template
        .replace("{{githubMarkdownCSS}}", subs.github_markdown_css)
        .replace("{{highlightLightCSS}}", subs.highlight_light_css)
        .replace("{{highlightDarkCSS}}", subs.highlight_dark_css)
        .replace("{{markedJS}}", subs.marked_js)
        .replace("{{highlightJS}}", subs.highlight_js)
        .replace("{{localizedStringsJSON}}", subs.localized_strings_json)
}

/// Load a bundled asset, preferring a zlib-compressed `<name>.<ext>.deflate`
/// variant over the plain file (port of `loadAsset(name:ext:)`).
fn load_asset(viewer_dir: &Path, name: &str, ext: &str) -> Result<String, MissingAsset> {
    let deflate = viewer_dir.join(format!("{name}.{ext}.deflate"));
    if deflate.is_file() {
        if let Some(s) = load_deflated_text_asset(&deflate) {
            return Ok(s);
        }
        // A present-but-corrupt compressed asset is fatal in Swift
        // (`preconditionFailure`); surface it as missing rather than silently
        // falling through to a (likely absent) plain variant.
        return Err(MissingAsset {
            name: name.to_string(),
            ext: format!("{ext}.deflate"),
        });
    }

    let plain = viewer_dir.join(format!("{name}.{ext}"));
    match std::fs::read(&plain) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
        Err(_) => Err(MissingAsset {
            name: name.to_string(),
            ext: ext.to_string(),
        }),
    }
}

fn load_deflated_text_asset(path: &Path) -> Option<String> {
    let compressed = std::fs::read(path).ok()?;
    let mut decoder = flate2::read::ZlibDecoder::new(&compressed[..]);
    let mut out = String::new();
    decoder.read_to_string(&mut out).ok()?;
    Some(out)
}

/// The remote-image localized-strings map, serialized to JSON (port of
/// `localizedStringsJSON()`). English fallback values, copied verbatim from the
/// canonical source (including the `{url}` placeholder in `remoteImageURL`).
pub fn localized_strings_json() -> String {
    // BTreeMap so the serialization is deterministic (Swift's dictionary order is
    // unspecified; consumers key into the object, so order is irrelevant, but a
    // stable output keeps snapshots/tests reproducible).
    let strings: std::collections::BTreeMap<&str, &str> = [
        ("remoteImageBlocked", "Remote image blocked"),
        (
            "remoteImageConsentMessage",
            "cmux will not contact this image URL until you load this image.",
        ),
        ("remoteImageLoadImage", "Load this image"),
        ("remoteImageLoading", "Loading"),
        (
            "remoteImageHTTPSOnly",
            "Only HTTPS remote images can be loaded in the viewer.",
        ),
        ("remoteImageCopyURL", "Copy image URL"),
        ("remoteImageCopied", "Copied"),
        ("remoteImageOpenURL", "Open image URL"),
        (
            "remoteImageNotAllowed",
            "This remote image URL cannot be loaded in the viewer.",
        ),
        ("remoteImageURL", "Image URL: {url}"),
    ]
    .into_iter()
    .collect();

    serde_json::to_string(&strings).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn shell_substitution_fills_all_placeholders() {
        let template = "<style>{{githubMarkdownCSS}}</style>\
            <style>{{highlightLightCSS}}</style><style>{{highlightDarkCSS}}</style>\
            <script>{{markedJS}}</script><script>{{highlightJS}}</script>\
            <script>window.S={{localizedStringsJSON}}</script>";
        let out = substitute_shell_template(
            template,
            &ShellSubstitutions {
                github_markdown_css: "GH",
                highlight_light_css: "HL",
                highlight_dark_css: "HD",
                marked_js: "MJ",
                highlight_js: "HJ",
                localized_strings_json: "{}",
            },
        );
        assert!(!out.contains("{{"), "no placeholder should remain: {out}");
        for piece in ["GH", "HL", "HD", "MJ", "HJ"] {
            assert!(out.contains(piece), "missing {piece}");
        }
    }

    #[test]
    fn localized_strings_has_all_keys_and_parses() {
        let json = localized_strings_json();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = value.as_object().unwrap();
        for key in [
            "remoteImageBlocked",
            "remoteImageConsentMessage",
            "remoteImageLoadImage",
            "remoteImageLoading",
            "remoteImageHTTPSOnly",
            "remoteImageCopyURL",
            "remoteImageCopied",
            "remoteImageOpenURL",
            "remoteImageNotAllowed",
            "remoteImageURL",
        ] {
            assert!(obj.contains_key(key), "missing key {key}");
        }
        assert_eq!(obj.len(), 10);
        assert_eq!(obj["remoteImageURL"], "Image URL: {url}");
    }

    #[test]
    fn load_reads_plain_assets_and_substitutes() {
        let dir = tempfile::tempdir().unwrap();
        let viewer = dir.path().join(MARKDOWN_VIEWER_SUBDIR);
        std::fs::create_dir(&viewer).unwrap();
        for (name, ext, body) in [
            ("marked.min", "js", "MARKED"),
            ("highlight.min", "js", "HLJS"),
            ("highlight-github", "css", "LIGHTCSS"),
            ("highlight-github-dark", "css", "DARKCSS"),
            ("github-markdown", "css", "GHCSS"),
            ("shell", "html", "{{githubMarkdownCSS}}|{{markedJS}}|{{localizedStringsJSON}}"),
        ] {
            std::fs::write(viewer.join(format!("{name}.{ext}")), body).unwrap();
        }
        let assets = MarkdownViewerAssets::load(dir.path()).unwrap();
        let html = assets.shell_html(true);
        assert!(html.contains("GHCSS"));
        assert!(html.contains("MARKED"));
        assert!(html.contains("remoteImageBlocked"));
        assert!(!html.contains("{{"));
    }

    #[test]
    fn deflate_variant_is_preferred() {
        let dir = tempfile::tempdir().unwrap();
        let viewer = dir.path().join(MARKDOWN_VIEWER_SUBDIR);
        std::fs::create_dir(&viewer).unwrap();
        // Write a zlib-compressed shell.html.deflate and confirm it is used.
        let template = "DEFLATED:{{markedJS}}";
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(template.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        std::fs::write(viewer.join("shell.html.deflate"), compressed).unwrap();
        // Plus a DIFFERENT plain variant that must be ignored.
        std::fs::write(viewer.join("shell.html"), "PLAIN:{{markedJS}}").unwrap();
        for (name, ext) in [
            ("marked.min", "js"),
            ("highlight.min", "js"),
            ("highlight-github", "css"),
            ("highlight-github-dark", "css"),
            ("github-markdown", "css"),
        ] {
            std::fs::write(viewer.join(format!("{name}.{ext}")), name).unwrap();
        }
        let assets = MarkdownViewerAssets::load(dir.path()).unwrap();
        let html = assets.shell_html(false);
        assert!(html.starts_with("DEFLATED:"), "deflate variant should win: {html}");
    }

    #[test]
    fn missing_asset_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(MARKDOWN_VIEWER_SUBDIR)).unwrap();
        let result = MarkdownViewerAssets::load(dir.path());
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("missing bundled markdown viewer asset"));
    }

    #[test]
    fn lazy_asset_loads_and_caches() {
        let dir = tempfile::tempdir().unwrap();
        let viewer = dir.path().join(MARKDOWN_VIEWER_SUBDIR);
        std::fs::create_dir(&viewer).unwrap();
        for (name, ext, body) in [
            ("marked.min", "js", "m"),
            ("highlight.min", "js", "h"),
            ("highlight-github", "css", "l"),
            ("highlight-github-dark", "css", "d"),
            ("github-markdown", "css", "g"),
            ("shell", "html", "s"),
        ] {
            std::fs::write(viewer.join(format!("{name}.{ext}")), body).unwrap();
        }
        std::fs::write(viewer.join("mermaid.min.js"), "MERMAID").unwrap();
        let assets = MarkdownViewerAssets::load(dir.path()).unwrap();
        assert_eq!(assets.lazy_asset("mermaid.min", "js").as_deref(), Some("MERMAID"));
        assert_eq!(assets.lazy_asset("missing", "js"), None);
    }
}
