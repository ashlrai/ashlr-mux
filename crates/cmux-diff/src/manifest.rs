//! On-disk diff-viewer manifest loader — the session-restore half of the
//! `cmux-diff-viewer://` trust model.
//!
//! Faithful port of the manifest helpers in Swift
//! `CmuxDiffViewerURLSchemeHandler`
//! (`Sources/Panels/BrowserPanel.swift`): `registeredFile(from:)` (:2331),
//! `localManifestFiles` (:2362), and `diffViewerRestorable` (:2390). WebKit
//! (`FileHandle`, `WKURLSchemeTask`) is stripped; what remains is the pure,
//! headless parse + head-sniff.
//!
//! A restored token survives an app restart: the in-memory `DiffSessionRegistry`
//! is lost, but the `.manifest-<token>.json` file + the referenced pages persist
//! in the trusted diff-viewer directory. `DiffSessionRegistry::register_from_manifest`
//! loads the file list here, then re-runs the same `register` validation
//! (standardize + trusted-root jail + extension/mime checks) so a restored
//! session is exactly as trusted as a freshly-registered one.
//!
//! NO FILE-COUNT CAP HERE (deliberate, matches Swift): `maxRegisteredFiles = 1024`
//! is enforced ONLY at the live RPC ingest (`TerminalController.swift:6194`), NOT
//! on the manifest path — Swift `register` (`:1947`) and `localManifestFiles`
//! (`:2362`) have no cap, so this loader adds none.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::session::{DiffSessionRegistry, RegisteredFile};

/// The manifest file name for a token: `.manifest-<token>.json`. Callers join
/// this onto the trusted-root directory. Mirrors the Swift
/// `trustedRootURL.appendingPathComponent(".manifest-\(token).json")` (:2364).
pub fn manifest_file_name(token: &str) -> String {
    format!(".manifest-{token}.json")
}

/// One `files[]` object -> a `RegisteredFile`, un-jailed. Faithful port of Swift
/// `registeredFile(from:)` (`:2331`): requires string `request_path`,
/// `file_path`, and `mime_type`; the file path is taken verbatim (NOT
/// standardized or jailed here — `DiffSessionRegistry::register` does the
/// canonicalize + trusted-root jail, exactly as the Swift `register` does at
/// `:1969`).
fn registered_file_from_json(object: &Map<String, Value>) -> Option<RegisteredFile> {
    let request_path = object.get("request_path").and_then(Value::as_str)?;
    let file_path = object.get("file_path").and_then(Value::as_str)?;
    let mime_type = object.get("mime_type").and_then(Value::as_str)?;
    Some(RegisteredFile {
        request_path: request_path.to_string(),
        file_path: PathBuf::from(file_path),
        mime_type: mime_type.to_string(),
    })
}

/// Parse a manifest's raw JSON bytes into its file list. Faithful port of the
/// parse core of Swift `localManifestFiles` (`:2362`).
///
/// Returns `None` (the whole manifest is rejected, all-or-nothing) when:
///  * the JSON is malformed, or the top level is not an object,
///  * there is no `files` array, or it is empty,
///  * any `files[]` element is not an object,
///  * ANY entry carries a string `remote_url` OR has an empty/absent
///    `file_path` (streamed remote PR diffs land here — the local-file scheme
///    handler cannot serve them, so the whole manifest is unusable), or
///  * any entry is missing a required string field (`registeredFile(from:)`
///    returns nil).
pub fn parse_manifest_files(data: &[u8]) -> Option<Vec<RegisteredFile>> {
    let value: Value = serde_json::from_slice(data).ok()?;
    // Swift `as? [String: Any]` then `object["files"] as? [[String: Any]]`.
    let object = value.as_object()?;
    let file_objects = object.get("files")?.as_array()?;
    if file_objects.is_empty() {
        return None;
    }

    let mut files: Vec<RegisteredFile> = Vec::with_capacity(file_objects.len());
    for file_object in file_objects {
        // Swift `as? [[String: Any]]` requires every element be a dictionary; a
        // single non-object element fails the whole cast -> nil.
        let file_object = file_object.as_object()?;
        // Swift `fileObject["file_path"] as? String ?? ""`: a present-but-
        // non-string file_path collapses to "" and is then rejected as empty.
        let file_path = file_object.get("file_path").and_then(Value::as_str).unwrap_or("");
        // Swift `fileObject["remote_url"] is String || filePath.isEmpty`.
        if file_object.get("remote_url").is_some_and(Value::is_string) || file_path.is_empty() {
            return None;
        }
        let file = registered_file_from_json(file_object)?;
        files.push(file);
    }
    Some(files)
}

/// Loads the registered files for a token's on-disk manifest under `dir`, or
/// `None` when the token is invalid, the manifest is missing/malformed/empty, or
/// it references remote entries. Faithful port of Swift `localManifestFiles`
/// (`:2362`), including its leading `isValidToken` guard (`:2363`) — the token is
/// interpolated into the manifest file name, so rejecting an invalid token also
/// blocks path-separator injection.
pub fn load_manifest_files(dir: &Path, token: &str) -> Option<Vec<RegisteredFile>> {
    if !DiffSessionRegistry::is_valid_token(token) {
        return None;
    }
    let manifest_path = dir.join(manifest_file_name(token));
    let data = std::fs::read(&manifest_path).ok()?;
    parse_manifest_files(&data)
}

/// Whether a diff-viewer surface can be restored through the custom scheme.
/// Faithful port of Swift `diffViewerRestorable` (`:2390`): requires a local-only
/// manifest, an entry page for `request_path`, and an entry page that is neither
/// a pending placeholder (`data-cmux-diff-pending="true"`) nor a redirect stub
/// (`data-cmux-diff-redirect`) in its first 1024 bytes — both of those forms only
/// work against the local HTTP server, which is gone after a restart.
pub fn diff_viewer_restorable(dir: &Path, token: &str, request_path: &str) -> bool {
    let Some(files) = load_manifest_files(dir, token) else {
        return false;
    };
    let Some(entry) = files.iter().find(|f| f.request_path == request_path) else {
        return false;
    };
    let Ok(file) = std::fs::File::open(&entry.file_path) else {
        return false;
    };
    // Swift `handle.read(upToCount: 1024)`. `take(1024).read_to_end` reliably
    // yields the first <=1024 bytes even across a short read (a regular local
    // file never short-reads, so this matches the single Swift read).
    let mut head = Vec::with_capacity(1024);
    if file.take(1024).read_to_end(&mut head).is_err() {
        return false;
    }
    // Swift `String(data: head, encoding: .utf8)`: invalid UTF-8 yields nil, so
    // the marker check is skipped and the page is treated as restorable.
    if let Ok(text) = std::str::from_utf8(&head) {
        if text.contains(r#"data-cmux-diff-pending="true""#)
            || text.contains("data-cmux-diff-redirect")
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A unique temp directory removed on drop (mirrors the `session.rs`
    /// harness). Kept inside `#[cfg(test)]` so the Windows Application Control
    /// (os 4551) standalone-exe gotcha does not apply.
    struct TempRoot {
        path: PathBuf,
    }

    impl TempRoot {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            let mut path = std::env::temp_dir();
            path.push(format!("cmux-diff-manifest-{pid}-{n}"));
            std::fs::create_dir_all(&path).expect("create temp root");
            let path = std::fs::canonicalize(&path).expect("canonicalize temp root");
            Self { path }
        }

        fn file(&self, name: &str, contents: &str) -> PathBuf {
            let p = self.path.join(name);
            std::fs::write(&p, contents).expect("write temp file");
            p
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    /// Write a `.manifest-<token>.json` under `root` for the given entries
    /// (`(request_path, file_path, mime_type)`). `file_path` is JSON-escaped so
    /// Windows backslashes survive.
    fn write_manifest(root: &TempRoot, token: &str, entries: &[(&str, &Path, &str)]) {
        let files: Vec<Value> = entries
            .iter()
            .map(|(rp, fp, mime)| {
                json!({ "request_path": rp, "file_path": fp.to_string_lossy(), "mime_type": mime })
            })
            .collect();
        let manifest = json!({ "files": files }).to_string();
        root.file(&manifest_file_name(token), &manifest);
    }

    #[test]
    fn parse_accepts_valid_and_does_not_jail() {
        // A raw, non-existent path is preserved verbatim — jailing is register's
        // job, not the loader's.
        let data = json!({
            "files": [
                { "request_path": "/a.html", "file_path": "/anywhere/a.html", "mime_type": "text/html" }
            ]
        })
        .to_string();
        let files = parse_manifest_files(data.as_bytes()).expect("parsed");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].request_path, "/a.html");
        assert_eq!(files[0].file_path, PathBuf::from("/anywhere/a.html"));
        assert_eq!(files[0].mime_type, "text/html");
    }

    #[test]
    fn parse_rejects_remote_url_entry() {
        let data = json!({
            "files": [
                { "request_path": "/a.html", "file_path": "/x/a.html", "mime_type": "text/html", "remote_url": "https://x" }
            ]
        })
        .to_string();
        assert!(parse_manifest_files(data.as_bytes()).is_none());
    }

    #[test]
    fn parse_rejects_empty_or_absent_file_path() {
        let empty = json!({
            "files": [ { "request_path": "/a.html", "file_path": "", "mime_type": "text/html" } ]
        })
        .to_string();
        assert!(parse_manifest_files(empty.as_bytes()).is_none());

        let absent = json!({
            "files": [ { "request_path": "/a.html", "mime_type": "text/html" } ]
        })
        .to_string();
        assert!(parse_manifest_files(absent.as_bytes()).is_none());
    }

    #[test]
    fn parse_all_or_nothing_rejects_whole_manifest_on_one_bad_entry() {
        // First entry is fine, second is remote -> the WHOLE manifest is None.
        let data = json!({
            "files": [
                { "request_path": "/a.html", "file_path": "/x/a.html", "mime_type": "text/html" },
                { "request_path": "/b.html", "file_path": "", "mime_type": "text/html", "remote_url": "https://x" }
            ]
        })
        .to_string();
        assert!(parse_manifest_files(data.as_bytes()).is_none());
    }

    #[test]
    fn parse_rejects_malformed_empty_and_non_object() {
        assert!(parse_manifest_files(b"not json at all").is_none());
        assert!(parse_manifest_files(json!({ "files": [] }).to_string().as_bytes()).is_none());
        assert!(parse_manifest_files(json!({ "nope": 1 }).to_string().as_bytes()).is_none());
        // Top-level array is not an object.
        assert!(parse_manifest_files(json!([1, 2, 3]).to_string().as_bytes()).is_none());
        // A non-object element in files fails the whole cast.
        let bad_elem = json!({ "files": ["not-an-object"] }).to_string();
        assert!(parse_manifest_files(bad_elem.as_bytes()).is_none());
    }

    #[test]
    fn load_missing_manifest_is_none() {
        let root = TempRoot::new();
        assert!(load_manifest_files(&root.path, "tok-missing-abcd12345").is_none());
    }

    #[test]
    fn load_invalid_token_is_none() {
        let root = TempRoot::new();
        // Too short for isValidToken; also blocks path-separator injection.
        assert!(load_manifest_files(&root.path, "short").is_none());
    }

    #[test]
    fn restorable_true_for_plain_page() {
        let root = TempRoot::new();
        let f = root.file("index.html", "<!doctype html><html><body>diff</body></html>");
        let token = "tok-restore-abcd12345";
        write_manifest(&root, token, &[("/index.html", &f, "text/html")]);
        assert!(diff_viewer_restorable(&root.path, token, "/index.html"));
    }

    #[test]
    fn restorable_false_for_pending_and_redirect() {
        let root = TempRoot::new();
        let pending = root.file(
            "pending.html",
            r#"<!doctype html><html data-cmux-diff-pending="true"><body></body></html>"#,
        );
        let redirect = root.file(
            "redirect.html",
            r#"<!doctype html><html data-cmux-diff-redirect="1"><body></body></html>"#,
        );
        let token = "tok-restore2-abc12345";
        write_manifest(
            &root,
            token,
            &[
                ("/pending.html", &pending, "text/html"),
                ("/redirect.html", &redirect, "text/html"),
            ],
        );
        assert!(!diff_viewer_restorable(&root.path, token, "/pending.html"));
        assert!(!diff_viewer_restorable(&root.path, token, "/redirect.html"));
    }

    #[test]
    fn restorable_false_for_missing_manifest_or_unknown_request_path() {
        let root = TempRoot::new();
        let f = root.file("index.html", "<html>ok</html>");
        let token = "tok-restore3-abc12345";
        // No manifest yet.
        assert!(!diff_viewer_restorable(&root.path, token, "/index.html"));
        write_manifest(&root, token, &[("/index.html", &f, "text/html")]);
        // Request path not in the manifest.
        assert!(!diff_viewer_restorable(&root.path, token, "/other.html"));
    }
}
