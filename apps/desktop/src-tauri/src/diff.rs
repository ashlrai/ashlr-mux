//! Tauri mount for the diff-viewer review-comments bridge.
//!
//! Faithful port of the native `cmuxDiffComments` message-handler in Swift
//! `DiffCommentsBridge` (`Sources/Panels/DiffCommentsBridge.swift:14`, dispatch at
//! `:147`). All comment logic already lives in the headless `cmux-diff` core
//! (`dispatch_comment_rpc` + `DiffCommentStore` + `DiffSessionRegistry`); this
//! command is a thin **trust gate + delegate**: it resolves the calling frame's
//! session token, asks the registry whether that token is live, then hands the
//! raw request to the core, returning the exact `NativeReply` envelope the reused
//! `webviews/src/comments/bridge.ts` expects.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use cmux_diff::{dispatch_comment_rpc, DiffCommentStore, DiffSessionRegistry};
use serde_json::Value;
use tauri::{AppHandle, Manager, State};

/// Managed state for the diff surface: the per-repo comment store and the
/// diff-viewer session/token registry that trust-gates bridge calls.
pub struct DiffState {
    registry: DiffSessionRegistry,
    store: DiffCommentStore,
}

impl DiffState {
    /// Build the state from the running app's data directory (comment store) and
    /// the diff-viewer trusted-root directory (session registry). Needs the
    /// `AppHandle` for `app_data_dir`, so it is constructed in `setup()`.
    ///
    /// DIVERGENCE (documented): macOS roots the comment store at
    /// `Application Support/cmux/diff-comments` and the session jail at
    /// `/tmp/cmux-diff-viewer-<uid>`. Windows has no shared `/tmp`; the port roots
    /// the store under Tauri's per-app `app_data_dir` (`%APPDATA%\<id>`) and the
    /// jail under `%TEMP%\cmux-diff-viewer`, mirroring the canonical layout on the
    /// platform's own directory conventions. The registry only registers files
    /// once the diff data-feed lands (deferred), so the jail root is inert today.
    pub fn new(app: &AppHandle) -> tauri::Result<Self> {
        let store_dir = app.path().app_data_dir()?.join("cmux").join("diff-comments");
        let trusted_root = std::env::temp_dir().join("cmux-diff-viewer");
        Ok(Self::with_dirs(store_dir, trusted_root))
    }

    /// Pure constructor from explicit directories (used by `new` and by tests).
    pub fn with_dirs(store_dir: PathBuf, trusted_root: impl AsRef<Path>) -> Self {
        Self {
            registry: DiffSessionRegistry::new(trusted_root),
            store: DiffCommentStore::new(store_dir),
        }
    }

    /// Resolve a `cmux-diff-viewer` scheme request to the on-disk file + MIME to
    /// serve, if its token has a live session that registered the path. Delegates
    /// to the pure [`crate::schemes::resolve_diff_request`] against this state's
    /// registry (the byte read stays in the scheme handler).
    pub fn resolve_diff_request(
        &self,
        uri: &str,
        now: SystemTime,
    ) -> Option<(PathBuf, String)> {
        crate::schemes::resolve_diff_request(&self.registry, uri, now)
    }
}

/// The single `cmuxDiffComments` request seam: trust-gate the calling frame, then
/// delegate to the `cmux-diff` core. Mirrors the injected-arg command pattern in
/// `agent_session::agent_session_rpc`. Returns the RAW `NativeReply` envelope
/// (`{ok,value}` / `{ok:false,error}`); `bridge.ts` unwraps it itself.
///
/// DEFERRED to the UI checkpoint (does NOT block this headless build): whether
/// `webview.url()` yields the calling IFRAME's `cmux-diff-viewer://<token>/…` URL
/// (the frozen `bridge.ts` sends NO token in the message). The token SOURCE is
/// kept to a single swappable line below so a later slice can re-point it (e.g.
/// to a per-message token or a per-webview registration) without touching the
/// gate/delegate logic.
#[tauri::command]
pub async fn diff_comments_rpc(
    webview: tauri::Webview,
    state: State<'_, DiffState>,
    message: Value,
) -> Result<Value, ()> {
    let now = SystemTime::now();
    // Token SOURCE — the single swappable line (see the DEFERRED note above).
    let token = webview
        .url()
        .ok()
        .and_then(|url| token_from_diff_viewer_url(url.as_str()));
    let token_active = token
        .as_deref()
        .is_some_and(|t| state.registry.has_active_session(t, now));
    Ok(dispatch_comment_rpc(&state.store, token_active, &message, &iso_now()))
}

/// Extract the diff-viewer session token from a calling-frame URL, accepting both
/// the macOS custom-scheme form (`cmux-diff-viewer://<token>/…`) and the Windows
/// WebView2 rewrite (`http://cmux-diff-viewer.localhost/<token>/…`). Reuses the
/// single scheme parser in `schemes`, so the diff scheme handler and this trust
/// gate can never disagree on token extraction. Returns `None` for any non
/// diff-viewer URL.
pub fn token_from_diff_viewer_url(url: &str) -> Option<String> {
    crate::schemes::parse_diff_viewer_uri(url).map(|(token, _path)| token)
}

/// The current UTC time as an ISO8601 `YYYY-MM-DDTHH:MM:SSZ` string, matching the
/// output of macOS `ISO8601DateFormatter` used to stamp a saved comment's
/// `createdAt`/`updatedAt`. Formatted from `OffsetDateTime` components (no
/// fractional seconds, `Z` suffix) to match byte-for-byte.
fn iso_now() -> String {
    let t = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        t.year(),
        u8::from(t.month()),
        t.day(),
        t.hour(),
        t.minute(),
        t.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Build a `DiffState` with a temp-dir comment store and a registry that has
    /// one token registered against a real file under its trusted root.
    fn state_with_registered_token(token: &str) -> (tempfile::TempDir, tempfile::TempDir, DiffState) {
        let store_dir = tempfile::tempdir().unwrap();
        let root_dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(root_dir.path()).unwrap();
        let file = root.join("index.html");
        std::fs::write(&file, "<html></html>").unwrap();

        let state = DiffState::with_dirs(store_dir.path().to_path_buf(), &root);
        state
            .registry
            .register(
                token,
                vec![cmux_diff::RegisteredFile {
                    request_path: "/index.html".into(),
                    file_path: file,
                    mime_type: "text/html".into(),
                }],
                SystemTime::now(),
            )
            .expect("register token");
        (store_dir, root_dir, state)
    }

    #[test]
    fn token_from_diff_viewer_url_parses_both_forms() {
        assert_eq!(
            token_from_diff_viewer_url("cmux-diff-viewer://tok-abcdef0123456789/index.html").as_deref(),
            Some("tok-abcdef0123456789")
        );
        assert_eq!(
            token_from_diff_viewer_url("http://cmux-diff-viewer.localhost/tok-abcdef0123456789/x.patch")
                .as_deref(),
            Some("tok-abcdef0123456789")
        );
        // Non diff-viewer URLs (the main app page, an https link) carry no token.
        assert!(token_from_diff_viewer_url("http://tauri.localhost/index.html").is_none());
        assert!(token_from_diff_viewer_url("https://example.com/index.html").is_none());
    }

    #[test]
    fn iso_now_matches_the_canonical_shape() {
        let s = iso_now();
        // `YYYY-MM-DDTHH:MM:SSZ` — 20 chars, `T` at 10, trailing `Z`, no fraction.
        assert_eq!(s.len(), 20, "{s}");
        assert_eq!(&s[10..11], "T");
        assert!(s.ends_with('Z'));
        assert!(!s.contains('.'));
    }

    // The command's gate+delegate is exercised through the core `dispatch` with
    // an explicitly-resolved `token_active`, since `webview.url()` needs a live
    // WebView2 (the token SOURCE is the deferred line). This proves the store +
    // registry wiring and the envelopes the delegate returns.

    fn delegate(state: &DiffState, token: Option<&str>, request: &Value) -> Value {
        let now = SystemTime::now();
        let token_active = token.is_some_and(|t| state.registry.has_active_session(t, now));
        dispatch_comment_rpc(&state.store, token_active, request, "2026-07-03T00:00:00Z")
    }

    #[test]
    fn active_token_lists_saves_and_deletes() {
        let token = "tok-abcdef0123456789";
        let (_store, _root, state) = state_with_registered_token(token);
        const REPO: &str = "/work/repo";

        // Empty list.
        let listed = delegate(
            &state,
            Some(token),
            &json!({ "method": "comments.list", "params": { "repoRoot": REPO } }),
        );
        assert_eq!(listed["ok"], json!(true));
        assert_eq!(listed["value"]["comments"], json!([]));

        // Save.
        let saved = delegate(
            &state,
            Some(token),
            &json!({
                "method": "comments.save",
                "params": {
                    "repoRoot": REPO,
                    "comment": {
                        "id": "550e8400-e29b-41d4-a716-446655440000",
                        "filePath": "src/main.rs",
                        "side": "additions",
                        "startLine": 10,
                        "endLine": 12,
                        "message": "nit",
                    }
                }
            }),
        );
        assert_eq!(saved["ok"], json!(true));
        assert_eq!(saved["value"]["comment"]["submissionText"], json!(""));
        assert_eq!(saved["value"]["comment"]["createdAt"], json!("2026-07-03T00:00:00Z"));

        // List now returns the saved comment.
        let listed = delegate(
            &state,
            Some(token),
            &json!({ "method": "comments.list", "params": { "repoRoot": REPO } }),
        );
        assert_eq!(listed["value"]["comments"].as_array().unwrap().len(), 1);

        // Delete is idempotent.
        let hit = delegate(
            &state,
            Some(token),
            &json!({ "method": "comments.delete", "params": { "repoRoot": REPO, "id": "550e8400-e29b-41d4-a716-446655440000" } }),
        );
        assert_eq!(hit["value"]["deleted"], json!(true));
        let miss = delegate(
            &state,
            Some(token),
            &json!({ "method": "comments.delete", "params": { "repoRoot": REPO, "id": "550e8400-e29b-41d4-a716-446655440000" } }),
        );
        assert_eq!(miss["value"]["deleted"], json!(false));
    }

    #[test]
    fn inactive_token_is_rejected_with_not_allowed() {
        let (_store, _root, state) = state_with_registered_token("tok-abcdef0123456789");
        // A token that was never registered → no active session → not_allowed.
        let reply = delegate(
            &state,
            Some("tok-never-registered0"),
            &json!({ "method": "comments.list", "params": { "repoRoot": "/work/repo" } }),
        );
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(reply["error"]["code"], json!("not_allowed"));

        // No token at all is likewise rejected.
        let none = delegate(
            &state,
            None,
            &json!({ "method": "comments.list", "params": { "repoRoot": "/work/repo" } }),
        );
        assert_eq!(none["error"]["code"], json!("not_allowed"));
    }
}
