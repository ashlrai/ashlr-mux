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
use std::sync::Mutex;
use std::time::SystemTime;

use cmux_diff::{
    dispatch_comment_rpc, DiffCommentStore, DiffCommentSubmissionPool, DiffSessionRegistry,
    RegisteredFile,
};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, State};

/// Managed state for the diff surface: the per-repo comment store and the
/// diff-viewer session/token registry that trust-gates bridge calls.
pub struct DiffState {
    registry: DiffSessionRegistry,
    store: DiffCommentStore,
    submission_pool: Mutex<DiffCommentSubmissionPool>,
    trusted_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedDiffSession {
    pub token: String,
    pub request_path: String,
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
        let store_dir = app
            .path()
            .app_data_dir()?
            .join("cmux")
            .join("diff-comments");
        let trusted_root = std::env::temp_dir().join("cmux-diff-viewer");
        Ok(Self::with_dirs(store_dir, trusted_root))
    }

    /// Pure constructor from explicit directories (used by `new` and by tests).
    pub fn with_dirs(store_dir: PathBuf, trusted_root: impl AsRef<Path>) -> Self {
        let trusted_root = trusted_root.as_ref().to_path_buf();
        Self {
            registry: DiffSessionRegistry::new(&trusted_root),
            store: DiffCommentStore::new(store_dir),
            submission_pool: Mutex::new(DiffCommentSubmissionPool::new()),
            trusted_root,
        }
    }

    /// Create and register a minimal diff-viewer document. This is the starter
    /// session used by the UI when the user opens a diff pane before a richer
    /// repo-diff data feed has produced a manifest.
    pub fn create_starter_session(&self, now: SystemTime) -> Result<CreatedDiffSession, String> {
        let token = format!("tok-{}", uuid::Uuid::new_v4());
        let request_path = "/index.html".to_string();
        let session_dir = self.trusted_root.join(&token);
        std::fs::create_dir_all(&session_dir)
            .map_err(|error| format!("Could not create diff session directory: {error}"))?;
        let index_path = session_dir.join("index.html");
        std::fs::write(&index_path, starter_diff_html())
            .map_err(|error| format!("Could not write diff viewer document: {error}"))?;
        self.registry
            .register(
                &token,
                vec![RegisteredFile {
                    request_path: request_path.clone(),
                    file_path: index_path,
                    mime_type: "text/html".to_string(),
                }],
                now,
            )
            .map_err(|error| error.to_string())?;
        Ok(CreatedDiffSession {
            token,
            request_path,
        })
    }

    /// Resolve a `cmux-diff-viewer` scheme request to the on-disk file + MIME to
    /// serve, if its token has a live session that registered the path. Delegates
    /// to the pure [`crate::schemes::resolve_diff_request`] against this state's
    /// registry (the byte read stays in the scheme handler).
    pub fn resolve_diff_request(&self, uri: &str, now: SystemTime) -> Option<(PathBuf, String)> {
        crate::schemes::resolve_diff_request(&self.registry, uri, now)
    }

    /// Whether `token` + `request_path` resolve to a currently-served diff file.
    /// This uses the registry's live lookup, which also falls back to
    /// manifest-based re-registration on an in-memory miss, so a persisted diff
    /// surface can be reopened after restart.
    pub fn has_registered_request(&self, token: &str, request_path: &str, now: SystemTime) -> bool {
        self.registry
            .registered_file(token, request_path, now)
            .is_some()
    }

    #[cfg(test)]
    fn pending_count(&self, workspace_id: Option<&str>) -> usize {
        self.submission_pool
            .lock()
            .expect("diff submission pool mutex poisoned")
            .pending_count(workspace_id)
    }
}

fn starter_diff_html() -> String {
    let config = serde_json::json!({
        "payload": {
            "title": "Diff Viewer",
            "statusMessage": "Diff viewer ready. Open a repo diff to populate this session.",
        }
    });
    format!(
        r#"<!doctype html>
<html data-cmux-webview-kind="diff">
<head>
  <meta charset="utf-8">
  <title>Diff Viewer</title>
</head>
<body data-cmux-webview-kind="diff">
  <div id="root"></div>
  <script id="cmux-diff-viewer-config" type="application/json">{config}</script>
  <script type="module" src="/main.mjs"></script>
</body>
</html>
"#
    )
}

#[tauri::command]
pub fn diff_create_session(state: State<'_, DiffState>) -> Result<CreatedDiffSession, String> {
    state.create_starter_session(SystemTime::now())
}

/// The single `cmuxDiffComments` request seam: trust-gate the calling frame, then
/// delegate to the `cmux-diff` core. Mirrors the injected-arg command pattern in
/// `agent_session::agent_session_rpc`. Returns the RAW `NativeReply` envelope
/// (`{ok,value}` / `{ok:false,error}`); `bridge.ts` unwraps it itself.
///
/// The iframe relay supplies `token` and `panelId` because the Tauri invoke
/// originates from the main document rather than the diff-viewer iframe. When
/// absent, the command falls back to extracting a token from `webview.url()` for
/// direct/native callers.
#[tauri::command]
pub async fn diff_comments_rpc(
    webview: tauri::Webview,
    state: State<'_, DiffState>,
    session_state: State<'_, crate::session::SessionState>,
    message: Value,
    token: Option<String>,
    panel_id: Option<String>,
) -> Result<Value, ()> {
    let now = SystemTime::now();
    let token = diff_comments_token(
        token.as_deref(),
        webview.url().ok().map(|url| url.to_string()).as_deref(),
    );
    let token_active = token
        .as_deref()
        .is_some_and(|t| state.registry.has_active_session(t, now));
    let reply = dispatch_comment_rpc(&state.store, token_active, &message, &iso_now());
    apply_diff_comment_side_effects(
        &state,
        &session_state,
        &message,
        &reply,
        panel_id.as_deref(),
    );
    Ok(reply)
}

fn diff_comments_token(explicit_token: Option<&str>, webview_url: Option<&str>) -> Option<String> {
    explicit_token
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .or_else(|| webview_url.and_then(token_from_diff_viewer_url))
}

fn apply_diff_comment_side_effects(
    state: &DiffState,
    session_state: &crate::session::SessionState,
    request: &Value,
    reply: &Value,
    panel_id: Option<&str>,
) {
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        return;
    }
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return;
    };
    let Some(params) = request.get("params").and_then(Value::as_object) else {
        return;
    };
    match method {
        "comments.save" => {
            let Some(repo_root) = params
                .get("repoRoot")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                return;
            };
            let Some(workspace_id) =
                panel_id.and_then(|id| crate::session::workspace_id_for_panel(session_state, id))
            else {
                return;
            };
            let Some(comment_id) = reply
                .get("value")
                .and_then(|value| value.get("comment"))
                .and_then(|comment| comment.get("id"))
                .and_then(Value::as_str)
            else {
                return;
            };
            if let Some(comment) = state
                .store
                .list(repo_root)
                .into_iter()
                .find(|comment| comment.id == comment_id)
            {
                let mut pool = state
                    .submission_pool
                    .lock()
                    .expect("diff submission pool mutex poisoned");
                pool.register_pending(&comment, repo_root, &workspace_id);
            }
        }
        "comments.delete" => {
            let Some(comment_id) = params.get("id").and_then(Value::as_str) else {
                return;
            };
            state
                .submission_pool
                .lock()
                .expect("diff submission pool mutex poisoned")
                .remove_pending(comment_id);
        }
        _ => {}
    }
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
    fn state_with_registered_token(
        token: &str,
    ) -> (tempfile::TempDir, tempfile::TempDir, DiffState) {
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
            token_from_diff_viewer_url("cmux-diff-viewer://tok-abcdef0123456789/index.html")
                .as_deref(),
            Some("tok-abcdef0123456789")
        );
        assert_eq!(
            token_from_diff_viewer_url(
                "http://cmux-diff-viewer.localhost/tok-abcdef0123456789/x.patch"
            )
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

    #[test]
    fn create_starter_session_registers_a_served_index() {
        let store_dir = tempfile::tempdir().unwrap();
        let root_dir = tempfile::tempdir().unwrap();
        let state = DiffState::with_dirs(store_dir.path().to_path_buf(), root_dir.path());

        let created = state
            .create_starter_session(SystemTime::now())
            .expect("starter session");

        assert!(DiffSessionRegistry::is_valid_token(&created.token));
        assert_eq!(created.request_path, "/index.html");
        assert!(state.has_registered_request(
            &created.token,
            &created.request_path,
            SystemTime::now()
        ));
        let uri = format!(
            "cmux-diff-viewer://{}{}",
            created.token, created.request_path
        );
        let resolved = state
            .resolve_diff_request(&uri, SystemTime::now())
            .expect("registered request");
        assert_eq!(resolved.1, "text/html");
        let html = std::fs::read_to_string(resolved.0).expect("html");
        assert!(html.contains("Diff viewer ready"));
        assert!(html.contains("cmux-diff-viewer-config"));
        assert!(html.contains(r#""statusMessage":"Diff viewer ready."#));
        assert!(html.contains(r#"src="/main.mjs""#));
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

    fn delegate_with_side_effects(
        state: &DiffState,
        session_state: &crate::session::SessionState,
        token: Option<&str>,
        panel_id: Option<&str>,
        request: &Value,
    ) -> Value {
        let reply = delegate(state, token, request);
        apply_diff_comment_side_effects(state, session_state, request, &reply, panel_id);
        reply
    }

    #[test]
    fn explicit_diff_comments_token_takes_precedence_over_webview_url() {
        assert_eq!(
            diff_comments_token(
                Some("tok-explicit"),
                Some("cmux-diff-viewer://tok-from-url/index.html")
            )
            .as_deref(),
            Some("tok-explicit")
        );
        assert_eq!(
            diff_comments_token(None, Some("cmux-diff-viewer://tok-from-url/index.html"))
                .as_deref(),
            Some("tok-from-url")
        );
        assert_eq!(diff_comments_token(Some("  "), None), None);
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
        assert_eq!(
            saved["value"]["comment"]["createdAt"],
            json!("2026-07-03T00:00:00Z")
        );

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

    #[test]
    fn save_and_delete_update_pending_submission_pool_for_panel_workspace() {
        let token = "tok-abcdef0123456789";
        let (_store, _root, state) = state_with_registered_token(token);
        let session_state = crate::session::SessionState::default();
        let workspace_id = crate::session::workspace_id_for_panel(&session_state, "surface-1")
            .expect("initial workspace id");
        const REPO: &str = "/work/repo";
        const ID: &str = "550e8400-e29b-41d4-a716-446655440000";

        let save = json!({
            "method": "comments.save",
            "params": {
                "repoRoot": REPO,
                "comment": {
                    "id": ID,
                    "filePath": "src/main.rs",
                    "side": "additions",
                    "startLine": 10,
                    "endLine": 12,
                    "message": "nit",
                    "submissionText": "Please address this nit."
                }
            }
        });
        let saved = delegate_with_side_effects(
            &state,
            &session_state,
            Some(token),
            Some("surface-1"),
            &save,
        );
        assert_eq!(saved["ok"], json!(true));
        assert_eq!(state.pending_count(Some(&workspace_id)), 1);

        let delete =
            json!({ "method": "comments.delete", "params": { "repoRoot": REPO, "id": ID } });
        let deleted = delegate_with_side_effects(
            &state,
            &session_state,
            Some(token),
            Some("surface-1"),
            &delete,
        );
        assert_eq!(deleted["ok"], json!(true));
        assert_eq!(state.pending_count(Some(&workspace_id)), 0);
    }
}
