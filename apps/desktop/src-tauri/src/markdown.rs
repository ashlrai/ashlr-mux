//! Tauri mount for the markdown/mermaid viewer's `cmuxLib` bridge.
//!
//! Faithful port of the native `cmuxLib` message-handler in Swift
//! `MarkdownWebRenderer.Coordinator` (`Sources/Panels/MarkdownWebRenderer.swift`):
//! `userContentController` (`:439`), `handleLibRequest` (`:645`),
//! `resolveMarkdownFile` (`:605`), plus the host→webview push scripts
//! `renderMarkdownScript` (`:411`) and `applyTheme` (`:352`). The heavy diagram
//! libraries and the file-link resolver already live in the headless
//! `cmux-markdown` core; this module owns the per-webview state and the JS
//! payload strings.
//!
//! Per-webview isolation (macOS uses one WKWebView per panel with a per-panel file
//! base URL + image jail): state is keyed by the Tauri webview **label** so two
//! markdown panels cannot leak each other's `requested_libs` or misjail images.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use cmux_markdown::{file_link, MarkdownViewerAssets, MarkdownWebTheme};
use serde_json::{json, Value};
use tauri::{Manager, State};

/// Per-webview markdown context (Swift `Coordinator`'s `filePath` +
/// `requestedLibs`).
#[derive(Default)]
struct PanelCtx {
    /// The markdown document this webview is rendering (used to jail local images
    /// and to resolve relative in-document links). Set by a later UI slice; empty
    /// until then, which still resolves absolute / cwd-relative links.
    file_path: String,
    /// Libraries already injected into this webview (load-once dedup).
    requested_libs: HashSet<String>,
}

/// Managed state for the markdown surface: per-webview contexts + the lazily
/// loaded, shared viewer assets.
#[derive(Default)]
pub struct MarkdownState {
    panels: Mutex<HashMap<String, PanelCtx>>,
    assets: Mutex<Option<Arc<MarkdownViewerAssets>>>,
}

impl MarkdownState {
    /// The shared viewer assets, loaded on first use from the bundle's resources
    /// directory. Returns `None` when the bundled `markdown-viewer` assets are
    /// absent (e.g. an unbundled dev run) — callers degrade gracefully rather
    /// than panicking (Swift `preconditionFailure`s).
    pub fn assets(&self, resources_root: &Path) -> Option<Arc<MarkdownViewerAssets>> {
        let mut guard = self.assets.lock().expect("markdown assets lock poisoned");
        if guard.is_none() {
            if let Ok(loaded) = MarkdownViewerAssets::load(resources_root) {
                *guard = Some(Arc::new(loaded));
            }
        }
        guard.clone()
    }

    /// The markdown document the given webview label is currently rendering (used
    /// to jail its `cmux-local-image` requests). Empty when the label is unknown
    /// or its document has not been set yet.
    pub fn markdown_file_for(&self, label: &str) -> String {
        self.panels
            .lock()
            .expect("markdown panels lock poisoned")
            .get(label)
            .map(|ctx| ctx.file_path.clone())
            .unwrap_or_default()
    }
}

/// The single `cmuxLib` request seam. Routes the three message shapes:
///   (a) `{ lib }`                                   → inject the lazy library
///   (b) `{ action:"resolveMarkdownFile", requestId, path }` → resolve + reply
///   (c) `{ action:"openMarkdownFile", path }`       → resolve + host-open
///
/// Mirrors the injected-arg command pattern in `agent_session::agent_session_rpc`
/// / `terminal`. `shell.html` awaits the out-of-band `window.__cmuxLibLoaded(name)`
/// (not this command's return), so the eval is the delivery mechanism and the
/// return value is a bare ack.
#[tauri::command]
pub async fn cmux_lib_rpc(
    webview: tauri::Webview,
    state: State<'_, MarkdownState>,
    message: Value,
) -> Result<Value, ()> {
    let label = webview.label().to_string();

    // (a) library injection.
    if let Some(lib) = message.get("lib").and_then(Value::as_str) {
        let resources_root = webview.path().resource_dir().ok();
        let assets = resources_root.and_then(|root| state.assets(&root));
        if let Some(assets) = assets {
            let js = {
                let mut panels = state.panels.lock().expect("markdown panels lock poisoned");
                let ctx = panels.entry(label).or_default();
                build_lib_injection(&assets, lib, &mut ctx.requested_libs)
            };
            if let Some(js) = js {
                let _ = webview.eval(js); // runtime-only delivery line
            }
        }
        return Ok(json!({ "ok": true }));
    }

    // (b)/(c) file-link actions.
    if let Some(action) = message.get("action").and_then(Value::as_str) {
        let md_file = {
            let mut panels = state.panels.lock().expect("markdown panels lock poisoned");
            panels.entry(label).or_default().file_path.clone()
        };
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        match action {
            "resolveMarkdownFile" => {
                if let (Some(request_id), Some(path)) = (
                    message.get("requestId").and_then(Value::as_str),
                    message.get("path").and_then(Value::as_str),
                ) {
                    let resolved = resolve_markdown_file(path, &md_file, &cwd);
                    let js = markdown_file_resolved_js(request_id, resolved.as_deref());
                    let _ = webview.eval(js); // runtime-only delivery line
                }
            }
            "openMarkdownFile" => {
                if let Some(path) = message.get("path").and_then(Value::as_str) {
                    if let Some(_resolved) = resolve_markdown_file(path, &md_file, &cwd) {
                        // DEFERRED (UI checkpoint): opening a resolved markdown file
                        // spawns a new markdown surface in the owning pane
                        // (`Coordinator.openMarkdownFile` → `newMarkdownSurface`).
                        // The pane/workspace surfaces are not ported yet, so the
                        // resolve is validated here and the open is a no-op stub.
                    }
                }
            }
            _ => {}
        }
    }

    Ok(json!({ "ok": true }))
}

// ---------------------------------------------------------------------------
// Pure payload builders (host → webview) — asserted headless; the `webview.eval`
// call above is the only runtime-only line.
// ---------------------------------------------------------------------------

/// Build the concatenated JS injection for a `{lib}` request, or `None` when the
/// lib is unknown or already loaded into this webview. Port of `handleLibRequest`
/// (`MarkdownWebRenderer.swift:645`): mermaid loads one bundle, `vega-lite` loads
/// vega → vega-lite → vega-embed IN ORDER; each source is terminated with `\n;`
/// and the whole is suffixed with the out-of-band `window.__cmuxLibLoaded(name)`
/// callback the shell awaits. Load-once dedup marks the lib as requested only for
/// a known lib (an unknown lib returns `None` without marking), so a failed/absent
/// asset does not permanently block a later retry of a different lib.
pub fn build_lib_injection(
    assets: &MarkdownViewerAssets,
    lib: &str,
    requested: &mut HashSet<String>,
) -> Option<String> {
    if requested.contains(lib) {
        return None;
    }
    let sources: &[(&str, &str)] = match lib {
        "mermaid" => &[("mermaid.min", "js")],
        "vega-lite" => &[
            ("vega.min", "js"),
            ("vega-lite.min", "js"),
            ("vega-embed.min", "js"),
        ],
        _ => return None,
    };

    requested.insert(lib.to_string());

    let mut injection = String::new();
    for (name, ext) in sources {
        if let Some(src) = assets.lazy_asset(name, ext) {
            if !src.is_empty() {
                injection.push_str(&src);
                injection.push_str("\n;");
            }
        }
    }
    // JSON-encode the lib name so it splices safely into JS.
    let name_literal = serde_json::to_string(lib).unwrap_or_else(|_| "\"\"".to_string());
    injection.push_str(&format!(
        "\nwindow.__cmuxLibLoaded && window.__cmuxLibLoaded({name_literal});"
    ));
    Some(injection)
}

/// Resolve a raw in-document link to an existing local markdown file, or `None`.
/// Port of `resolvedMarkdownFilePath` (`MarkdownWebRenderer.swift:621`): trim,
/// gate on `is_markdown_path_like`, then delegate to the ported
/// `file_link::resolve` (relative to the document, then the working directory).
fn resolve_markdown_file(raw_path: &str, markdown_file_path: &str, cwd: &str) -> Option<String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() || !file_link::is_markdown_path_like(trimmed) {
        return None;
    }
    file_link::resolve(trimmed, markdown_file_path, cwd)
}

/// The `__cmuxMarkdownFileResolved` reply script (port of the payload built in
/// `resolveMarkdownFile`, `MarkdownWebRenderer.swift:611`): `{ requestId, exists,
/// path }`, with `path` an empty string when unresolved.
fn markdown_file_resolved_js(request_id: &str, resolved: Option<&str>) -> String {
    let payload = json!({
        "requestId": request_id,
        "exists": resolved.is_some(),
        "path": resolved.unwrap_or(""),
    });
    format!("window.__cmuxMarkdownFileResolved && window.__cmuxMarkdownFileResolved({payload});")
}

/// The host→webview markdown render push (port of `renderMarkdownScript`,
/// `MarkdownWebRenderer.swift:411`): pass the raw markdown through a JSON array
/// literal so backticks/quotes/backslashes need no hand-escaping, then hand it to
/// `window.__cmuxRenderMarkdown`, falling back to an escaped `<pre>` when the
/// renderer failed to initialize.
pub fn render_markdown_js(markdown: &str) -> String {
    let array_literal = serde_json::to_string(&[markdown]).unwrap_or_else(|_| "[\"\"]".to_string());
    format!(
        "(function(md) {{\n\
        \x20 if (window.__cmuxRenderMarkdown) {{\n\
        \x20   window.__cmuxRenderMarkdown(md);\n\
        \x20   return;\n\
        \x20 }}\n\
        \x20 var el = document.getElementById('content') || document.body;\n\
        \x20 function esc(s) {{\n\
        \x20   var div = document.createElement('div');\n\
        \x20   div.textContent = String(s == null ? '' : s);\n\
        \x20   return div.innerHTML;\n\
        \x20 }}\n\
        \x20 el.innerHTML = '<pre style=\"color:#f85149;white-space:pre-wrap\">Markdown renderer failed to initialize. Showing raw source.\\n\\n' + esc(md) + '</pre>';\n\
        }})({array_literal}[0]);"
    )
}

/// The host→webview theme push (port of `applyTheme`,
/// `MarkdownWebRenderer.swift:352`): set the six GitHub-CSS custom properties on
/// `#content`, force a transparent background, and invoke the page's optional
/// `window.__cmuxApplyTheme` hook. The variable map is exactly
/// [`MarkdownWebTheme::css_variables`].
pub fn apply_theme_js(theme: &MarkdownWebTheme) -> String {
    let vars: serde_json::Map<String, Value> = theme
        .css_variables()
        .iter()
        .map(|(name, value)| ((*name).to_string(), Value::String((*value).to_string())))
        .collect();
    let json = Value::Object(vars);
    format!(
        "(function(vars) {{\n\
        \x20 var content = document.getElementById('content');\n\
        \x20 if (!content) {{ return; }}\n\
        \x20 Object.keys(vars).forEach(function(name) {{\n\
        \x20   content.style.setProperty(name, vars[name]);\n\
        \x20 }});\n\
        \x20 content.style.background = 'transparent';\n\
        \x20 if (window.__cmuxApplyTheme) {{ window.__cmuxApplyTheme(); }}\n\
        }})({json});"
    )
}

// ---------------------------------------------------------------------------
// Host → webview push commands. These are the inbound seam for a later slice to
// render markdown / apply a theme into a panel; they wire the pure builders above
// to `webview.eval` so the crate stays fully connected (no dead public helpers).
// ---------------------------------------------------------------------------

/// Push a markdown document into a panel webview (`__cmuxRenderMarkdown`).
#[tauri::command]
pub async fn markdown_render(webview: tauri::Webview, markdown: String) -> Result<(), ()> {
    let _ = webview.eval(render_markdown_js(&markdown));
    Ok(())
}

/// Apply a theme derived from the panel's 8-bit sRGB background color
/// (`__cmuxApplyTheme` + the six CSS custom properties).
#[tauri::command]
pub async fn markdown_apply_theme(
    webview: tauri::Webview,
    background: [u8; 3],
) -> Result<(), ()> {
    let theme = MarkdownWebTheme::resolve((background[0], background[1], background[2]));
    let _ = webview.eval(apply_theme_js(&theme));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A viewer-assets fixture with the required files plus a lazy `mermaid.min.js`.
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
            ("shell", "html", "<html></html>"),
        ] {
            std::fs::write(viewer.join(format!("{name}.{ext}")), body).unwrap();
        }
        std::fs::write(viewer.join("mermaid.min.js"), "MERMAID_SRC").unwrap();
        let assets = MarkdownViewerAssets::load(dir.path()).unwrap();
        (dir, assets)
    }

    #[test]
    fn build_lib_injection_dedups_and_suffixes_callback() {
        let (_dir, assets) = fixture_assets();
        let mut requested = HashSet::new();

        let js = build_lib_injection(&assets, "mermaid", &mut requested).expect("first load");
        assert!(js.contains("MERMAID_SRC"), "concatenates the lazy source");
        assert!(
            js.contains("window.__cmuxLibLoaded && window.__cmuxLibLoaded(\"mermaid\");"),
            "JSON-encoded callback suffix: {js}"
        );
        assert!(js.contains("\n;"), "each source is terminated with newline-semicolon");

        // Load-once: the second call for the same lib returns None.
        assert!(build_lib_injection(&assets, "mermaid", &mut requested).is_none());
        assert!(requested.contains("mermaid"));
    }

    #[test]
    fn build_lib_injection_rejects_unknown_lib_without_marking() {
        let (_dir, assets) = fixture_assets();
        let mut requested = HashSet::new();
        assert!(build_lib_injection(&assets, "not-a-lib", &mut requested).is_none());
        // An unknown lib must NOT be recorded (so it never blocks a later retry).
        assert!(requested.is_empty());
    }

    #[test]
    fn resolve_markdown_file_gates_and_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("linked.md");
        std::fs::write(&target, "# hi").unwrap();
        let md = dir.path().join("index.md");
        std::fs::write(&md, "see [linked](linked.md)").unwrap();

        // Sibling markdown link resolves relative to the document.
        let resolved = resolve_markdown_file("linked.md", &md.to_string_lossy(), "/nonexistent-cwd");
        assert!(resolved.is_some());
        assert!(resolved.unwrap().ends_with("linked.md"));

        // Non-markdown extension and a missing file both fail the gate/resolve.
        assert!(resolve_markdown_file("linked.txt", &md.to_string_lossy(), "/nope").is_none());
        assert!(resolve_markdown_file("missing.md", &md.to_string_lossy(), "/nope").is_none());
        assert!(resolve_markdown_file("   ", &md.to_string_lossy(), "/nope").is_none());
    }

    #[test]
    fn markdown_file_resolved_js_shapes_reply() {
        let hit = markdown_file_resolved_js("req-1", Some("/abs/linked.md"));
        assert!(hit.starts_with("window.__cmuxMarkdownFileResolved &&"));
        assert!(hit.contains("\"exists\":true"));
        assert!(hit.contains("\"path\":\"/abs/linked.md\""));
        assert!(hit.contains("\"requestId\":\"req-1\""));

        let miss = markdown_file_resolved_js("req-2", None);
        assert!(miss.contains("\"exists\":false"));
        assert!(miss.contains("\"path\":\"\""));
    }

    #[test]
    fn render_markdown_js_passes_markdown_through_json_literal() {
        // Backticks / quotes / newlines must not break the injected script.
        let js = render_markdown_js("# Title\n`code` \"quoted\" \\ end");
        assert!(js.contains("window.__cmuxRenderMarkdown(md)"));
        // The markdown rides inside a JSON array literal indexed at [0].
        assert!(js.contains("[0]);"));
        // The raw double-quote is JSON-escaped, not left bare.
        assert!(js.contains("\\\"quoted\\\""), "{js}");
    }

    #[test]
    fn apply_theme_js_emits_the_six_css_variables() {
        let theme = MarkdownWebTheme::resolve((13, 17, 23));
        let js = apply_theme_js(&theme);
        for var in [
            "--bgColor-default",
            "--bgColor-muted",
            "--bgColor-neutral-muted",
            "--borderColor-default",
            "--borderColor-muted",
            "--borderColor-neutral-muted",
        ] {
            assert!(js.contains(var), "missing {var}");
        }
        assert!(js.contains("window.__cmuxApplyTheme"));
        assert!(js.contains("content.style.background = 'transparent';"));
    }
}
