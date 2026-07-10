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
//! Per-panel isolation (macOS uses one WKWebView per panel with a per-panel file
//! base URL + image jail): the Windows port hosts markdown inside sandboxed
//! iframes within one Tauri webview, so state is keyed by the markdown
//! **panel_id** rather than the outer webview label.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use cmux_markdown::{
    file_link,
    typography::{font_family, font_size, max_width},
    MarkdownTypography, MarkdownViewerAssets, MarkdownWebTheme,
};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State};

/// Per-panel markdown context (Swift `Coordinator`'s `filePath` +
/// `requestedLibs`).
struct PanelCtx {
    /// The markdown document this webview is rendering (used to jail local images
    /// and to resolve relative in-document links). Set by a later UI slice; empty
    /// until then, which still resolves absolute / cwd-relative links.
    file_path: String,
    /// Libraries already injected into this webview (load-once dedup).
    requested_libs: HashSet<String>,
    /// The panel's logical markdown point size. Defaults to the built-in 15pt.
    point_size: f64,
}

impl Default for PanelCtx {
    fn default() -> Self {
        Self {
            file_path: String::new(),
            requested_libs: HashSet::new(),
            point_size: font_size::DEFAULT_POINT_SIZE,
        }
    }
}

/// Managed state for the markdown surface: per-panel contexts + the lazily
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

    /// The markdown document the given panel id is currently rendering (used
    /// to jail its `cmux-local-image` requests). Empty when the label is unknown
    /// or its document has not been set yet.
    pub fn markdown_file_for(&self, panel_id: &str) -> String {
        self.panels
            .lock()
            .expect("markdown panels lock poisoned")
            .get(panel_id)
            .map(|ctx| ctx.file_path.clone())
            .unwrap_or_default()
    }

    /// Bind the markdown document path for a panel id (the write half of
    /// [`markdown_file_for`]). Port of `Coordinator.bind(panelId:workspaceId:
    /// filePath:)` (`MarkdownWebRenderer.swift:190`), which assigns
    /// `self.filePath = filePath`. Mirrors the `cmux_lib_rpc` keying
    /// (`panels.entry(panel_id).or_default()`, `:96`/`:110`) so a `set_document`
    /// on a never-seen panel creates the ctx, and mutates **only** `file_path` — the
    /// per-webview `requested_libs` dedup set is left untouched. Pure state
    /// mutation, unit-testable without a `Webview`.
    pub fn set_document(&self, panel_id: &str, path: String) {
        self.panels
            .lock()
            .expect("markdown panels lock poisoned")
            .entry(panel_id.to_string())
            .or_default()
            .file_path = path;
    }

    pub fn page_zoom_for(&self, panel_id: &str) -> f64 {
        let point_size = self
            .panels
            .lock()
            .expect("markdown panels lock poisoned")
            .get(panel_id)
            .map(|ctx| ctx.point_size)
            .unwrap_or(font_size::DEFAULT_POINT_SIZE);
        font_size::page_zoom(point_size)
    }

    pub fn zoom_in(&self, panel_id: &str) -> f64 {
        let point_size = self.update_point_size(panel_id, |current| {
            font_size::clamp(current + font_size::STEP_POINT_SIZE)
        });
        font_size::page_zoom(point_size)
    }

    pub fn zoom_out(&self, panel_id: &str) -> f64 {
        let point_size = self.update_point_size(panel_id, |current| {
            font_size::clamp(current - font_size::STEP_POINT_SIZE)
        });
        font_size::page_zoom(point_size)
    }

    #[cfg(test)]
    pub fn zoom_reset(&self, panel_id: &str) -> f64 {
        let point_size = self.update_point_size(panel_id, |_| font_size::DEFAULT_POINT_SIZE);
        font_size::page_zoom(point_size)
    }

    pub fn set_point_size(&self, panel_id: &str, point_size: f64) -> f64 {
        let point_size = self.update_point_size(panel_id, |_| font_size::clamp(point_size));
        font_size::page_zoom(point_size)
    }

    pub fn apply_typography_defaults(&self, panel_id: &str, typography: &MarkdownTypography) {
        let mut panels = self.panels.lock().expect("markdown panels lock poisoned");
        panels.entry(panel_id.to_string()).or_default().point_size = typography.font_size as f64;
    }

    fn update_point_size(&self, panel_id: &str, update: impl FnOnce(f64) -> f64) -> f64 {
        let mut panels = self.panels.lock().expect("markdown panels lock poisoned");
        let ctx = panels.entry(panel_id.to_string()).or_default();
        ctx.point_size = update(ctx.point_size);
        ctx.point_size
    }
}

fn panel_key(panel_id: Option<&str>, webview_label: &str) -> String {
    panel_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(webview_label)
        .to_string()
}

fn dispatch_markdown_panel_js(panel_id: &str, js: &str) -> String {
    let panel_literal = serde_json::to_string(panel_id).unwrap_or_else(|_| "\"\"".to_string());
    let js_literal = serde_json::to_string(js).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        "(function(panelId, payload) {{\n\
        \x20 var frames = document.querySelectorAll('iframe.cmux-markdown-surface');\n\
        \x20 for (var i = 0; i < frames.length; i++) {{\n\
        \x20   var frame = frames[i];\n\
        \x20   if (frame.getAttribute('data-cmux-markdown-panel-id') !== panelId) {{ continue; }}\n\
        \x20   var win = frame.contentWindow;\n\
        \x20   if (!win || typeof win.eval !== 'function') {{ return; }}\n\
        \x20   try {{ win.eval(payload); }} catch (_error) {{}}\n\
        \x20   return;\n\
        \x20 }}\n\
        }})({panel_literal}, {js_literal});"
    )
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
    app: AppHandle,
    webview: tauri::Webview,
    state: State<'_, MarkdownState>,
    session_state: State<'_, crate::session::SessionState>,
    message: Value,
) -> Result<Value, ()> {
    let panel_id = panel_key(
        message.get("panelId").and_then(Value::as_str),
        webview.label(),
    );

    // (a) library injection.
    if let Some(lib) = message.get("lib").and_then(Value::as_str) {
        let resources_root = webview.path().resource_dir().ok();
        let assets = resources_root.and_then(|root| state.assets(&root));
        if let Some(assets) = assets {
            let js = {
                let mut panels = state.panels.lock().expect("markdown panels lock poisoned");
                let ctx = panels.entry(panel_id.clone()).or_default();
                build_lib_injection(&assets, lib, &mut ctx.requested_libs)
            };
            if let Some(js) = js {
                let _ = webview.eval(dispatch_markdown_panel_js(&panel_id, &js));
                // runtime-only delivery line
            }
        }
        return Ok(json!({ "ok": true }));
    }

    // (b)/(c) file-link actions.
    if let Some(action) = message.get("action").and_then(Value::as_str) {
        let md_file = {
            let mut panels = state.panels.lock().expect("markdown panels lock poisoned");
            panels
                .entry(panel_id.clone())
                .or_default()
                .file_path
                .clone()
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
                    let _ = webview.eval(dispatch_markdown_panel_js(&panel_id, &js));
                    // runtime-only delivery line
                }
            }
            "openMarkdownFile" => {
                if let Some(path) = message.get("path").and_then(Value::as_str) {
                    if let Some(resolved) = resolve_markdown_file(path, &md_file, &cwd) {
                        let _ = crate::session::open_markdown_file_in_panel(
                            &app,
                            &session_state,
                            &panel_id,
                            &resolved,
                        );
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

pub fn set_markdown_zoom_js(page_zoom: f64) -> String {
    let zoom = if page_zoom.is_finite() && page_zoom > 0.0 {
        page_zoom
    } else {
        1.0
    };
    format!("window.__cmuxSetMarkdownZoom && window.__cmuxSetMarkdownZoom({zoom});")
}

pub fn apply_typography_js(typography: &MarkdownTypography) -> String {
    let font_size_px = font_size::clamp(typography.font_size as f64);
    let max_width_px = max_width::clamp(typography.max_width as f64);
    let font_family = font_family::css_value(&typography.font_family);
    let payload = json!({
        "fontSize": font_size_px,
        "maxWidth": max_width_px,
        "fontFamily": font_family,
    });
    format!("window.__cmuxApplyTypography && window.__cmuxApplyTypography({payload});")
}

// ---------------------------------------------------------------------------
// Host → webview push commands. These are the inbound seam for a later slice to
// render markdown / apply a theme into a panel; they wire the pure builders above
// to `webview.eval` so the crate stays fully connected (no dead public helpers).
// ---------------------------------------------------------------------------

/// Bind a panel webview's markdown document path **before** its markdown is
/// rendered. Port of `Coordinator.bind(...)` (`MarkdownWebRenderer.swift:190`),
/// invoked at `updateNSView:99` — strictly before the render dispatch
/// `update(markdown:theme:)` at `:106`, in the same view pass. This ordering is
/// load-bearing: the per-webview `file_path` is what the local-image jail
/// (`:562`) and the file-link resolver (`:625`) read **synchronously** at request
/// time, so an empty path makes every `cmux-local-image://` fail (403). The
/// caller enforces the sequence by `await`ing this command, then calling
/// [`markdown_render`].
///
/// DEFERRED (host WebView2 side-effect, runtime-only): this command performs only
/// the pure state write; it does not itself `eval` a render (that is the separate
/// `markdown_render` seam). Fusing set→eval into one command — set `file_path`
/// first, then `webview.eval(render_markdown_js(..))` — would make the ordering
/// un-invertible (closer to Swift's single `updateNSView` pass) and is the
/// natural next step once the frontend render trigger is wired.
#[tauri::command]
pub async fn markdown_set_document(
    webview: tauri::Webview,
    state: State<'_, MarkdownState>,
    path: String,
    panel_id: Option<String>,
) -> Result<Value, ()> {
    // Store the raw path; the ported jail (`resolve_local_image`) does all
    // Windows-path standardization at request time — no pre-canonicalization here
    // (parity with Swift storing the raw `filePath` and normalizing only in the
    // jail, `:567–580`).
    state.set_document(&panel_key(panel_id.as_deref(), webview.label()), path);
    Ok(json!({ "ok": true }))
}

/// Push a markdown document into a panel webview (`__cmuxRenderMarkdown`).
#[tauri::command]
pub async fn markdown_render(
    webview: tauri::Webview,
    state: State<'_, MarkdownState>,
    markdown: String,
    panel_id: Option<String>,
) -> Result<(), ()> {
    let panel_id = panel_key(panel_id.as_deref(), webview.label());
    let js = format!(
        "{}\n{}",
        render_markdown_js(&markdown),
        set_markdown_zoom_js(state.page_zoom_for(&panel_id))
    );
    let _ = webview.eval(dispatch_markdown_panel_js(&panel_id, &js));
    Ok(())
}

/// Apply a theme derived from the panel's 8-bit sRGB background color
/// (`__cmuxApplyTheme` + the six CSS custom properties).
#[tauri::command]
pub async fn markdown_apply_theme(
    webview: tauri::Webview,
    background: [u8; 3],
    panel_id: Option<String>,
) -> Result<(), ()> {
    let theme = MarkdownWebTheme::resolve((background[0], background[1], background[2]));
    let panel_id = panel_key(panel_id.as_deref(), webview.label());
    let _ = webview.eval(dispatch_markdown_panel_js(
        &panel_id,
        &apply_theme_js(&theme),
    ));
    Ok(())
}

/// Apply the current markdown typography defaults to a panel iframe and seed
/// that panel's zoom state from the configured font size.
#[tauri::command]
pub async fn markdown_apply_typography(
    webview: tauri::Webview,
    state: State<'_, MarkdownState>,
    panel_id: Option<String>,
) -> Result<(), ()> {
    let config = crate::config::current_markdown_config();
    let typography = MarkdownTypography::apply(
        config.font_size as f64,
        &config.font_family,
        config.max_width as f64,
    );
    let panel_id = panel_key(panel_id.as_deref(), webview.label());
    state.apply_typography_defaults(&panel_id, &typography);
    let _ = webview.eval(dispatch_markdown_panel_js(
        &panel_id,
        &apply_typography_js(&typography),
    ));
    Ok(())
}

#[tauri::command]
pub async fn markdown_zoom_in(
    webview: tauri::Webview,
    state: State<'_, MarkdownState>,
    panel_id: Option<String>,
) -> Result<(), ()> {
    let panel_id = panel_key(panel_id.as_deref(), webview.label());
    let _ = webview.eval(dispatch_markdown_panel_js(
        &panel_id,
        &set_markdown_zoom_js(state.zoom_in(&panel_id)),
    ));
    Ok(())
}

#[tauri::command]
pub async fn markdown_zoom_out(
    webview: tauri::Webview,
    state: State<'_, MarkdownState>,
    panel_id: Option<String>,
) -> Result<(), ()> {
    let panel_id = panel_key(panel_id.as_deref(), webview.label());
    let _ = webview.eval(dispatch_markdown_panel_js(
        &panel_id,
        &set_markdown_zoom_js(state.zoom_out(&panel_id)),
    ));
    Ok(())
}

#[tauri::command]
pub async fn markdown_zoom_reset(
    webview: tauri::Webview,
    state: State<'_, MarkdownState>,
    panel_id: Option<String>,
) -> Result<(), ()> {
    let panel_id = panel_key(panel_id.as_deref(), webview.label());
    let config = crate::config::current_markdown_config();
    let _ = webview.eval(dispatch_markdown_panel_js(
        &panel_id,
        &set_markdown_zoom_js(state.set_point_size(&panel_id, config.font_size as f64)),
    ));
    Ok(())
}

#[tauri::command]
pub fn markdown_read_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read markdown file {path}: {error}"))
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
        assert!(
            js.contains("\n;"),
            "each source is terminated with newline-semicolon"
        );

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
        let resolved =
            resolve_markdown_file("linked.md", &md.to_string_lossy(), "/nonexistent-cwd");
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
    fn dispatch_markdown_panel_js_targets_the_matching_iframe() {
        let js = dispatch_markdown_panel_js(
            "surface-9",
            "window.__cmuxApplyTheme && window.__cmuxApplyTheme();",
        );
        assert!(js.contains("iframe.cmux-markdown-surface"));
        assert!(js.contains("data-cmux-markdown-panel-id"));
        assert!(js.contains("\"surface-9\""), "{js}");
        assert!(js.contains("window.__cmuxApplyTheme"), "{js}");
    }

    // ---- G1: markdown_set_document — per-webview document-path writer -------
    // Testable half = the state mutation + set-before-request ordering. The
    // `markdown_set_document` command's only runtime-only line is the ack; the
    // `set_document` helper is exercised directly here (no `Webview` needed).

    /// Build a `file:` URL for a path (mirror of the jail's test helper) so the
    /// ordering oracle can construct a real `cmux-local-image://` request.
    fn file_url(p: &Path) -> String {
        let s = p.to_string_lossy().replace('\\', "/");
        if s.starts_with('/') {
            format!("file://{s}")
        } else {
            // Windows drive path -> file:///C:/...
            format!("file:///{s}")
        }
    }

    #[test]
    fn set_document_then_markdown_file_for_roundtrips() {
        let state = MarkdownState::default();
        state.set_document("panel-1", "/docs/a.md".to_string());
        assert_eq!(state.markdown_file_for("panel-1"), "/docs/a.md");
    }

    #[test]
    fn markdown_file_for_empty_before_set() {
        // The "empty until set" precondition the jail depends on.
        let state = MarkdownState::default();
        assert_eq!(state.markdown_file_for("panel-x"), "");
    }

    #[test]
    fn set_document_is_per_panel_isolated() {
        let state = MarkdownState::default();
        state.set_document("panel-1", "/a/x.md".to_string());
        state.set_document("panel-2", "/b/y.md".to_string());
        // No cross-panel leak: each label observes only its own document.
        assert_eq!(state.markdown_file_for("panel-1"), "/a/x.md");
        assert_eq!(state.markdown_file_for("panel-2"), "/b/y.md");
    }

    #[test]
    fn set_document_overwrites() {
        let state = MarkdownState::default();
        state.set_document("panel-1", "/a/first.md".to_string());
        state.set_document("panel-1", "/a/second.md".to_string());
        // A panel re-pointed at a new doc: latest write wins.
        assert_eq!(state.markdown_file_for("panel-1"), "/a/second.md");
    }

    #[test]
    fn set_document_preserves_requested_libs() {
        // Seed the lib dedup set for a label via the same keyed ctx the command
        // path uses, then set_document, and assert the mutation is field-scoped to
        // `file_path` (the `requested_libs` set is untouched).
        let (_dir, assets) = fixture_assets();
        let state = MarkdownState::default();
        {
            let mut panels = state.panels.lock().unwrap();
            let ctx = panels.entry("panel-1".to_string()).or_default();
            assert!(build_lib_injection(&assets, "mermaid", &mut ctx.requested_libs).is_some());
        }
        state.set_document("panel-1", "/docs/a.md".to_string());
        let panels = state.panels.lock().unwrap();
        let ctx = panels.get("panel-1").expect("ctx exists");
        assert_eq!(ctx.file_path, "/docs/a.md");
        assert!(
            ctx.requested_libs.contains("mermaid"),
            "set_document must not disturb the lib dedup set"
        );
    }

    #[test]
    fn set_document_before_request_lets_the_jail_resolve_the_sibling_image() {
        // Ordering oracle: with the document path SET, a sibling image now
        // resolves through the jail; with the pre-set empty path it does not.
        // This is the concrete "set before render/request" proof.
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("index.md");
        std::fs::write(&md, "x").unwrap();
        let img = dir.path().join("img.png");
        std::fs::write(&img, b"\x89PNG").unwrap();
        let request = format!("cmux-local-image://image?url={}", file_url(&img));

        // Before set: empty path ⇒ the jail has no directory to resolve against.
        assert!(
            crate::schemes::resolve_local_image_request(&request, "").is_none(),
            "empty document path must not resolve any local image"
        );

        // After set: the jailed sibling image resolves.
        let state = MarkdownState::default();
        state.set_document("p", md.to_string_lossy().into_owned());
        let resolved =
            crate::schemes::resolve_local_image_request(&request, &state.markdown_file_for("p"));
        assert!(
            resolved.is_some(),
            "a sibling image must resolve once the document path is set"
        );
        assert!(resolved.unwrap().0.ends_with("img.png"));
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

    #[test]
    fn set_markdown_zoom_js_calls_the_shell_hook() {
        let js = set_markdown_zoom_js(1.6);
        assert!(js.contains("window.__cmuxSetMarkdownZoom"));
        assert!(js.contains("1.6"));
    }

    #[test]
    fn apply_typography_js_calls_the_shell_hook_with_clamped_values() {
        let typography = MarkdownTypography::apply(120.0, "  Fancy Serif  ", 200.0);
        let js = apply_typography_js(&typography);
        assert!(js.contains("window.__cmuxApplyTypography"));
        assert!(js.contains("\"fontSize\":96"));
        assert!(js.contains("\"maxWidth\":320"));
        assert!(js.contains("\\\"Fancy Serif\\\""), "{js}");
    }

    #[test]
    fn typography_defaults_seed_panel_zoom() {
        let state = MarkdownState::default();
        let typography = MarkdownTypography::apply(18.0, "", 980.0);
        state.apply_typography_defaults("panel-z", &typography);
        assert!((state.page_zoom_for("panel-z") - (18.0 / 15.0)).abs() < 1e-9);
        assert!((state.zoom_in("panel-z") - (19.0 / 15.0)).abs() < 1e-9);
        assert!((state.set_point_size("panel-z", 200.0) - (96.0 / 15.0)).abs() < 1e-9);
    }

    #[test]
    fn zoom_state_defaults_steps_and_resets() {
        let state = MarkdownState::default();
        assert!((state.page_zoom_for("panel-z") - 1.0).abs() < 1e-9);
        assert!((state.zoom_in("panel-z") - (16.0 / 15.0)).abs() < 1e-9);
        assert!((state.zoom_out("panel-z") - 1.0).abs() < 1e-9);
        assert!((state.zoom_reset("panel-z") - 1.0).abs() < 1e-9);
    }
}
