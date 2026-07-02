//! Pure `diff_comments_rpc` contract mapper.
//!
//! Faithful port of the JS bridge dispatch in Swift `DiffCommentsBridge`
//! (`Sources/Panels/DiffCommentsBridge.swift:147`), decoupled from WebKit. The
//! Tauri command above this resolves the calling frame's session token (via the
//! diff-viewer URL) into a `token_active` bool using `DiffSessionRegistry`, then
//! hands the raw request here; this module validates and dispatches to the
//! (already-ported) `DiffCommentStore` and returns the exact `NativeReply`
//! envelope the reused `webviews/src/comments/bridge.ts` expects.
//!
//! Reply envelopes (Swift `userContentController` :91):
//!   * success           -> `{ "ok": true, "value": <result> }`
//!   * BridgeError        -> `{ "ok": false, "error": { "code", "userMessage" } }`
//!   * internal/IO error  -> `{ "ok": false, "error": {} }`  (Swift's catch-all)
//!
//! DEFERRED (matches macOS but out of this slice): the `DiffCommentSubmissionPool`
//! pending-pool registration on list/save and its removal on delete — those need
//! the workspace/TextBox surfaces that do not exist on Windows yet. The store
//! reads/writes are fully wired.

use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::comment_store::{DiffComment, DiffCommentStore};

const CODE_NOT_ALLOWED: &str = "not_allowed";
const CODE_INVALID_REQUEST: &str = "invalid_request";
// Swift localizes `diffComments.bridge.notAllowed`; the Windows i18n pipeline is
// a later phase, so we ship the canonical English default. TODO(i18n).
const MSG_NOT_ALLOWED: &str = "This page cannot use diff comments.";

/// Dispatch one diff-comments bridge request against `store`.
///
/// `token_active` is the trust gate (Swift `isTrustedDiffViewerFrame`): the
/// caller has already resolved the frame's token and asked the session registry
/// whether it is live. `now` is the ISO8601 timestamp stamped onto a saved
/// comment's `createdAt`/`updatedAt` (the store preserves an existing
/// `createdAt` on update). Never panics; always returns a reply envelope.
pub fn dispatch_comment_rpc(
    store: &DiffCommentStore,
    token_active: bool,
    request: &Value,
    now: &str,
) -> Value {
    if !token_active {
        return error_reply(CODE_NOT_ALLOWED, MSG_NOT_ALLOWED);
    }

    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return error_reply(CODE_INVALID_REQUEST, "Malformed bridge request");
    };

    // Swift: `body["params"] as? [String: Any] ?? [:]`.
    let empty = Map::new();
    let params = request
        .get("params")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    let repo_root = params
        .get("repoRoot")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(repo_root) = repo_root else {
        return error_reply(CODE_INVALID_REQUEST, "Missing repoRoot");
    };

    match method {
        "comments.list" => {
            let comments: Vec<Value> = store.list(repo_root).iter().map(comment_json).collect();
            ok_reply(json!({ "comments": comments }))
        }
        "comments.save" => {
            let Some(comment) = params
                .get("comment")
                .and_then(Value::as_object)
                .and_then(|c| comment_from_json(c, now))
            else {
                return error_reply(CODE_INVALID_REQUEST, "Malformed comment");
            };
            match store.upsert(comment, repo_root) {
                Ok(saved) => ok_reply(json!({ "comment": comment_json(&saved) })),
                Err(_) => internal_error_reply(),
            }
        }
        "comments.delete" => {
            // Swift requires a well-formed UUID string.
            let Some(id) = params
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| Uuid::parse_str(s).is_ok())
            else {
                return error_reply(CODE_INVALID_REQUEST, "Missing comment id");
            };
            match store.delete(id, repo_root) {
                Ok(deleted) => ok_reply(json!({ "deleted": deleted })),
                Err(_) => internal_error_reply(),
            }
        }
        other => error_reply(
            CODE_INVALID_REQUEST,
            &format!("Unsupported method '{other}'"),
        ),
    }
}

fn ok_reply(value: Value) -> Value {
    json!({ "ok": true, "value": value })
}

fn error_reply(code: &str, user_message: &str) -> Value {
    json!({ "ok": false, "error": { "code": code, "userMessage": user_message } })
}

/// Swift's catch-all for a non-`BridgeError` throw (`replyHandler(["ok": false,
/// "error": [:]])`). Used here for a store IO failure.
fn internal_error_reply() -> Value {
    json!({ "ok": false, "error": {} })
}

/// `DiffComment` -> the JS wire object. Faithful port of Swift `commentJSON`
/// (`DiffCommentsBridge.swift:226`): `submissionText` is ALWAYS present (empty
/// string when unset), `consumedAt` is NEVER emitted, and `endSide` appears only
/// when set. This deliberately differs from `DiffComment`'s own serde shape, so
/// the mapping is written out by hand.
fn comment_json(comment: &DiffComment) -> Value {
    let mut obj = Map::new();
    obj.insert("id".into(), json!(comment.id));
    obj.insert("filePath".into(), json!(comment.file_path));
    obj.insert("side".into(), json!(comment.side));
    obj.insert("startLine".into(), json!(comment.start_line));
    obj.insert("endLine".into(), json!(comment.end_line));
    obj.insert("lineText".into(), json!(comment.line_text));
    obj.insert("message".into(), json!(comment.message));
    obj.insert(
        "submissionText".into(),
        json!(comment.submission_text.clone().unwrap_or_default()),
    );
    obj.insert("createdAt".into(), json!(comment.created_at));
    obj.insert("updatedAt".into(), json!(comment.updated_at));
    if let Some(end_side) = &comment.end_side {
        obj.insert("endSide".into(), json!(end_side));
    }
    Value::Object(obj)
}

/// JS wire object -> `DiffComment` for a save. Faithful port of Swift
/// `comment(fromJSON:)` (`DiffCommentsBridge.swift:246`): requires a non-empty
/// `filePath`, `side`, integer `startLine`/`endLine`, and `message`; normalizes
/// `side` to `additions`/`deletions`; defaults `lineText` to `""`; and stamps
/// `createdAt`/`updatedAt` to `now` (the store preserves an existing `createdAt`
/// on update). `consumedAt` is always cleared.
///
/// DIVERGENCE (documented): Swift canonicalizes the id via `UUID.uuidString`
/// (uppercase). We validate the sender's id is a well-formed UUID and preserve
/// its original (JS-native, lowercase) form — regenerating a fresh v4 only when
/// it is missing/invalid — to avoid a surprising case flip on the web side. The
/// delete path re-validates the same way, so store lookups stay internally
/// consistent.
fn comment_from_json(json: &Map<String, Value>, now: &str) -> Option<DiffComment> {
    let file_path = json.get("filePath").and_then(Value::as_str)?;
    if file_path.is_empty() {
        return None;
    }
    let side_raw = json.get("side").and_then(Value::as_str)?;
    let start_line = json.get("startLine").and_then(Value::as_i64)?;
    let end_line = json.get("endLine").and_then(Value::as_i64)?;
    let message = json.get("message").and_then(Value::as_str)?;

    let id = match json.get("id").and_then(Value::as_str) {
        Some(s) if Uuid::parse_str(s).is_ok() => s.to_string(),
        _ => Uuid::new_v4().to_string(),
    };
    let side = if side_raw == "deletions" {
        "deletions"
    } else {
        "additions"
    };

    Some(DiffComment {
        consumed_at: None,
        created_at: now.to_string(),
        end_line,
        end_side: json
            .get("endSide")
            .and_then(Value::as_str)
            .map(str::to_string),
        file_path: file_path.to_string(),
        id,
        line_text: json
            .get("lineText")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        message: message.to_string(),
        side: side.to_string(),
        start_line,
        submission_text: json
            .get("submissionText")
            .and_then(Value::as_str)
            .map(str::to_string),
        updated_at: now.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TempStore {
        dir: PathBuf,
        store: DiffCommentStore,
    }

    impl TempStore {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            let mut dir = std::env::temp_dir();
            dir.push(format!("cmux-diff-rpc-{pid}-{n}"));
            std::fs::create_dir_all(&dir).expect("create temp dir");
            let store = DiffCommentStore::new(dir.clone());
            Self { dir, store }
        }
    }

    impl Drop for TempStore {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const REPO: &str = "/work/repo";
    const NOW: &str = "2026-07-02T00:00:00Z";

    fn save_request(comment: Value) -> Value {
        json!({ "method": "comments.save", "params": { "repoRoot": REPO, "comment": comment } })
    }

    fn valid_comment() -> Value {
        json!({
            "id": "550e8400-e29b-41d4-a716-446655440000",
            "filePath": "src/main.rs",
            "side": "additions",
            "startLine": 10,
            "endLine": 12,
            "lineText": "let x = 1;",
            "message": "nit",
        })
    }

    #[test]
    fn rejects_untrusted_frame() {
        let ts = TempStore::new();
        let reply = dispatch_comment_rpc(&ts.store, false, &json!({ "method": "comments.list" }), NOW);
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(reply["error"]["code"], json!("not_allowed"));
        assert_eq!(reply["error"]["userMessage"], json!(MSG_NOT_ALLOWED));
    }

    #[test]
    fn rejects_malformed_body_and_missing_repo_root() {
        let ts = TempStore::new();
        let no_method = dispatch_comment_rpc(&ts.store, true, &json!({ "params": {} }), NOW);
        assert_eq!(no_method["error"]["code"], json!("invalid_request"));
        assert_eq!(no_method["error"]["userMessage"], json!("Malformed bridge request"));

        let no_repo = dispatch_comment_rpc(&ts.store, true, &json!({ "method": "comments.list" }), NOW);
        assert_eq!(no_repo["error"]["userMessage"], json!("Missing repoRoot"));

        let blank_repo = dispatch_comment_rpc(
            &ts.store,
            true,
            &json!({ "method": "comments.list", "params": { "repoRoot": "   " } }),
            NOW,
        );
        assert_eq!(blank_repo["error"]["userMessage"], json!("Missing repoRoot"));
    }

    #[test]
    fn rejects_unknown_method() {
        let ts = TempStore::new();
        let reply = dispatch_comment_rpc(
            &ts.store,
            true,
            &json!({ "method": "comments.frobnicate", "params": { "repoRoot": REPO } }),
            NOW,
        );
        assert_eq!(reply["error"]["code"], json!("invalid_request"));
        assert_eq!(reply["error"]["userMessage"], json!("Unsupported method 'comments.frobnicate'"));
    }

    #[test]
    fn list_empty_store_returns_empty_array() {
        let ts = TempStore::new();
        let reply = dispatch_comment_rpc(
            &ts.store,
            true,
            &json!({ "method": "comments.list", "params": { "repoRoot": REPO } }),
            NOW,
        );
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["value"]["comments"], json!([]));
    }

    #[test]
    fn save_then_list_round_trip_and_wire_shape() {
        let ts = TempStore::new();
        let saved = dispatch_comment_rpc(&ts.store, true, &save_request(valid_comment()), NOW);
        assert_eq!(saved["ok"], json!(true));
        let c = &saved["value"]["comment"];
        assert_eq!(c["id"], json!("550e8400-e29b-41d4-a716-446655440000"));
        assert_eq!(c["filePath"], json!("src/main.rs"));
        assert_eq!(c["side"], json!("additions"));
        assert_eq!(c["startLine"], json!(10));
        assert_eq!(c["message"], json!("nit"));
        assert_eq!(c["createdAt"], json!(NOW));
        // Wire-shape invariants that differ from DiffComment's serde:
        assert_eq!(c["submissionText"], json!("")); // present, empty when unset
        assert!(c.get("consumedAt").is_none()); // never emitted
        assert!(c.get("endSide").is_none()); // omitted when absent

        let listed = dispatch_comment_rpc(
            &ts.store,
            true,
            &json!({ "method": "comments.list", "params": { "repoRoot": REPO } }),
            NOW,
        );
        assert_eq!(listed["value"]["comments"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn save_normalizes_side_and_defaults_line_text() {
        let ts = TempStore::new();
        let mut comment = valid_comment();
        comment["side"] = json!("weird-value");
        comment.as_object_mut().unwrap().remove("lineText");
        let reply = dispatch_comment_rpc(&ts.store, true, &save_request(comment), NOW);
        assert_eq!(reply["value"]["comment"]["side"], json!("additions"));
        assert_eq!(reply["value"]["comment"]["lineText"], json!(""));

        let mut del = valid_comment();
        del["side"] = json!("deletions");
        del["id"] = json!("550e8400-e29b-41d4-a716-44665544ffff");
        let reply = dispatch_comment_rpc(&ts.store, true, &save_request(del), NOW);
        assert_eq!(reply["value"]["comment"]["side"], json!("deletions"));
    }

    #[test]
    fn save_emits_end_side_when_present() {
        let ts = TempStore::new();
        let mut comment = valid_comment();
        comment["endSide"] = json!("deletions");
        let reply = dispatch_comment_rpc(&ts.store, true, &save_request(comment), NOW);
        assert_eq!(reply["value"]["comment"]["endSide"], json!("deletions"));
    }

    #[test]
    fn save_rejects_malformed_comment() {
        let ts = TempStore::new();
        let mut comment = valid_comment();
        comment.as_object_mut().unwrap().remove("message");
        let reply = dispatch_comment_rpc(&ts.store, true, &save_request(comment), NOW);
        assert_eq!(reply["error"]["code"], json!("invalid_request"));
        assert_eq!(reply["error"]["userMessage"], json!("Malformed comment"));
    }

    #[test]
    fn save_generates_id_when_missing_or_invalid() {
        let ts = TempStore::new();
        let mut comment = valid_comment();
        comment["id"] = json!("not-a-uuid");
        let reply = dispatch_comment_rpc(&ts.store, true, &save_request(comment), NOW);
        let id = reply["value"]["comment"]["id"].as_str().unwrap();
        assert!(Uuid::parse_str(id).is_ok(), "generated a valid uuid, got {id:?}");
        assert_ne!(id, "not-a-uuid");
    }

    #[test]
    fn delete_round_trip_and_unknown_id() {
        let ts = TempStore::new();
        dispatch_comment_rpc(&ts.store, true, &save_request(valid_comment()), NOW);

        let del = |id: &str| {
            dispatch_comment_rpc(
                &ts.store,
                true,
                &json!({ "method": "comments.delete", "params": { "repoRoot": REPO, "id": id } }),
                NOW,
            )
        };
        let hit = del("550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(hit["value"]["deleted"], json!(true));
        let miss = del("550e8400-e29b-41d4-a716-446655440001");
        assert_eq!(miss["value"]["deleted"], json!(false));
    }

    #[test]
    fn delete_rejects_missing_or_non_uuid_id() {
        let ts = TempStore::new();
        let no_id = dispatch_comment_rpc(
            &ts.store,
            true,
            &json!({ "method": "comments.delete", "params": { "repoRoot": REPO } }),
            NOW,
        );
        assert_eq!(no_id["error"]["userMessage"], json!("Missing comment id"));

        let bad_id = dispatch_comment_rpc(
            &ts.store,
            true,
            &json!({ "method": "comments.delete", "params": { "repoRoot": REPO, "id": "nope" } }),
            NOW,
        );
        assert_eq!(bad_id["error"]["userMessage"], json!("Missing comment id"));
    }
}
