//! Diff-viewer token/session registry — the trust model behind the
//! `cmux-diff-viewer://<token>/<request-path>` custom URL scheme.
//!
//! Faithful port of the session bookkeeping in Swift
//! `CmuxDiffViewerURLSchemeHandler` (`Sources/Panels/BrowserPanel.swift:1904`),
//! with the WebKit-specific streaming (`WKURLSchemeTask`, `HTTPURLResponse`,
//! response headers/CSP) and the git branch-picker routes removed. What remains
//! is the pure, headless, security-critical core:
//!
//!  * token validation (`is_valid_token`, Swift `isValidToken` :2458),
//!  * request-path traversal rejection (`is_valid_request_path`, :2465),
//!  * the mime allowlist + extension match (`is_allowed_mime_type` :2485,
//!    `path_extension_matches_mime_type` :2489),
//!  * a trusted-root file jail (`isTrustedDiffViewerFileURL` :2624),
//!  * `register` / `has_active_session` / `registered_file` session lookup with
//!    24h expiry pruning (:1947 / :2000 / :2012 / `pruneExpiredSessionsLocked`
//!    :2629).
//!
//! The `WKURLSchemeHandler` adapter (URL parsing, byte streaming) and the Tauri
//! custom-scheme handler live above this in the desktop crate; this module is
//! deliberately transport-free so it is unit-testable with no live WebView2.
//!
//! On-disk manifest session-restore (`registerFromManifest` :2348) — which lets a
//! token survive an app restart — lives in the sibling `manifest` module and is
//! wired in here via `register_from_manifest`, the `has_active_session` fallback
//! (:2006-2009), and the `registered_file` reload-on-miss retry (:2033-2047).
//! There is deliberately NO file-count cap: `maxRegisteredFiles = 1024` guards
//! only the live RPC ingest (`TerminalController.swift:6194`), not `register`
//! (:1947) or the manifest path (:2362), so neither does this port.

use std::collections::HashMap;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use crate::comment_store::canonical_repo_root;

/// 24 hours — Swift `maxSessionAge` (`BrowserPanel.swift:1941`).
pub const DEFAULT_MAX_SESSION_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// One entry in a session's allowlist: a request path served from an on-disk
/// file with a fixed mime type. Mirrors Swift `RegisteredFile`
/// (`BrowserPanel.swift:1909`). `file_path` is stored canonicalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredFile {
    /// The scheme request path, e.g. `/index.html`. Always starts with `/`.
    pub request_path: String,
    /// Absolute on-disk path the request path resolves to.
    pub file_path: PathBuf,
    /// One of `text/html`, `text/javascript`, `text/x-diff`.
    pub mime_type: String,
}

/// Why a `register` call was rejected. Ports the five `NSError` codes the Swift
/// `register` throws (`BrowserPanel.swift:1947`); `Display` reproduces the
/// canonical `NSLocalizedDescriptionKey` messages verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSessionError {
    /// Token failed `is_valid_token` (Swift code 1).
    InvalidToken,
    /// The allowlist had no entries (Swift code 2).
    EmptyAllowlist,
    /// An entry had a bad request path, mime type, or extension mismatch
    /// (Swift code 3).
    InvalidEntry,
    /// An entry's file is outside the trusted root, missing, a directory, or
    /// unreadable (Swift code 4).
    FileNotReadable,
    /// Two entries shared a request path (Swift code 5).
    DuplicateEntry,
}

impl std::fmt::Display for DiffSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            Self::InvalidToken => "Invalid diff viewer token",
            Self::EmptyAllowlist => "Diff viewer allowlist is empty",
            Self::InvalidEntry => "Invalid diff viewer allowlist entry",
            Self::FileNotReadable => "Diff viewer file is not readable",
            Self::DuplicateEntry => "Duplicate diff viewer allowlist entry",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for DiffSessionError {}

struct Session {
    files_by_path: HashMap<String, RegisteredFile>,
    created_at: SystemTime,
}

/// The registry of live diff-viewer sessions, keyed by token. Serves the same
/// role as the `sessions` dictionary + `lock` in the Swift handler. Shareable
/// across threads (wrap in `Arc`); all mutation goes through the inner `Mutex`.
pub struct DiffSessionRegistry {
    sessions: Mutex<HashMap<String, Session>>,
    /// The raw trusted-root directory, kept so `register_from_manifest` can read
    /// `.manifest-<token>.json` from it (Swift `trustedRootURL`,
    /// `BrowserPanel.swift:1942`). `trusted_root_key` below is its
    /// canonicalized+normalized form used only for the jail prefix check.
    trusted_root: PathBuf,
    /// The only directory registered files may live under. Stored as its
    /// canonicalized+normalized string form so it composes with
    /// `canonical_repo_root` on candidate files. Ports Swift `trustedRootURL`
    /// (`BrowserPanel.swift:1942`), whose macOS value is
    /// `/tmp/cmux-diff-viewer-<uid>`; the Windows analogue is injected by the
    /// caller (kept out of this pure module for testability).
    trusted_root_key: String,
    max_age: Duration,
}

impl DiffSessionRegistry {
    /// Build a registry jailed to `trusted_root` with the default 24h expiry.
    pub fn new(trusted_root: impl AsRef<Path>) -> Self {
        Self::with_max_age(trusted_root, DEFAULT_MAX_SESSION_AGE)
    }

    /// Build a registry with an explicit session lifetime (used by tests).
    pub fn with_max_age(trusted_root: impl AsRef<Path>, max_age: Duration) -> Self {
        let trusted_root = trusted_root.as_ref().to_path_buf();
        let trusted_root_key = canonical_repo_root(&trusted_root.to_string_lossy());
        Self {
            sessions: Mutex::new(HashMap::new()),
            trusted_root,
            trusted_root_key,
            max_age,
        }
    }

    /// Register a token's allowlist. Faithful port of Swift `register`
    /// (`BrowserPanel.swift:1947`): validates the token and every entry, jails
    /// each file to the trusted root, rejects duplicates, prunes expired
    /// sessions, then stores the session. Any prior session for the token is
    /// replaced.
    pub fn register(
        &self,
        token: &str,
        files: Vec<RegisteredFile>,
        now: SystemTime,
    ) -> Result<(), DiffSessionError> {
        if !Self::is_valid_token(token) {
            return Err(DiffSessionError::InvalidToken);
        }
        if files.is_empty() {
            return Err(DiffSessionError::EmptyAllowlist);
        }

        let mut by_path: HashMap<String, RegisteredFile> = HashMap::new();
        for file in files {
            if !Self::is_valid_request_path(&file.request_path)
                || !Self::is_allowed_mime_type(&file.mime_type)
                || !Self::path_extension_matches_mime_type(&file.request_path, &file.mime_type)
            {
                return Err(DiffSessionError::InvalidEntry);
            }

            // Existence + not-a-directory + trusted-root jail. `metadata`
            // follows symlinks and errors when the path is missing, so
            // `is_file()` covers "exists, is a regular file" (the Swift
            // `fileExists && !isDirectory` pair). Windows has no POSIX
            // read-permission bit, so `isReadableFile` collapses into `is_file`.
            let meta = std::fs::metadata(&file.file_path);
            let is_file = meta.map(|m| m.is_file()).unwrap_or(false);
            if !is_file || !self.is_trusted_file(&file.file_path) {
                return Err(DiffSessionError::FileNotReadable);
            }

            if by_path.contains_key(&file.request_path) {
                return Err(DiffSessionError::DuplicateEntry);
            }

            let canonical =
                std::fs::canonicalize(&file.file_path).unwrap_or_else(|_| file.file_path.clone());
            by_path.insert(
                file.request_path.clone(),
                RegisteredFile {
                    request_path: file.request_path,
                    file_path: canonical,
                    mime_type: file.mime_type,
                },
            );
        }

        let mut sessions = self.sessions.lock().expect("diff session lock poisoned");
        Self::prune_expired(&mut sessions, self.max_age, now);
        sessions.insert(
            token.to_string(),
            Session {
                files_by_path: by_path,
                created_at: now,
            },
        );
        Ok(())
    }

    /// Remove exactly one live session from the in-memory registry.
    ///
    /// Returns whether the token was registered. Repeated calls are harmless,
    /// and invalid tokens cannot affect any registered session.
    pub fn unregister(&self, token: &str) -> bool {
        if !Self::is_valid_token(token) {
            return false;
        }
        self.sessions
            .lock()
            .expect("diff session lock poisoned")
            .remove(token)
            .is_some()
    }

    /// Whether `token` currently has a live (or manifest-restorable) session.
    /// Used to trust-gate native bridge calls (e.g. `diff_comments_rpc`) from
    /// diff-viewer pages. Faithful port of Swift `hasActiveSession`
    /// (`BrowserPanel.swift:2000`): on an in-memory miss it falls back to
    /// restoring the token from its on-disk manifest (:2006-2009), so a surface
    /// survives an app restart.
    pub fn has_active_session(&self, token: &str, now: SystemTime) -> bool {
        if !Self::is_valid_token(token) {
            return false;
        }
        let is_registered = {
            let mut sessions = self.sessions.lock().expect("diff session lock poisoned");
            Self::prune_expired(&mut sessions, self.max_age, now);
            sessions.contains_key(token)
        };
        if is_registered {
            return true;
        }
        // `register_from_manifest` takes the lock itself, so call it unlocked.
        self.register_from_manifest(token, now)
    }

    /// Re-registers a token from its on-disk `.manifest-<token>.json` under the
    /// trusted root so its surface can be served again after an app restart (the
    /// in-memory registry is lost, but the manifest + files persist). Faithful
    /// port of Swift `registerFromManifest` (`BrowserPanel.swift:2348`): loads the
    /// manifest file list, then re-runs the full `register` validation
    /// (standardize + trusted-root jail + mime/extension checks). Returns `true`
    /// when the token is registered and ready to serve.
    pub fn register_from_manifest(&self, token: &str, now: SystemTime) -> bool {
        let Some(files) = crate::manifest::load_manifest_files(&self.trusted_root, token) else {
            return false;
        };
        self.register(token, files, now).is_ok()
    }

    /// Look up the file a `(token, request_path)` resolves to, if the session is
    /// live. Faithful port of the session-lookup half of Swift
    /// `registeredFile(for:)` (`BrowserPanel.swift:2012`); URL decomposition
    /// (scheme/host/query checks) belongs to the scheme-handler adapter above this
    /// module. `request_path` is re-validated so an adapter bug cannot smuggle a
    /// traversal path.
    ///
    /// On a miss against an *active* session, the on-disk manifest may have grown
    /// out-of-band (the branch-picker regenerate route appends a freshly-written
    /// page without updating this in-memory allowlist), so it reloads the
    /// manifest once and retries (:2033-2047).
    pub fn registered_file(
        &self,
        token: &str,
        request_path: &str,
        now: SystemTime,
    ) -> Option<RegisteredFile> {
        if !Self::is_valid_token(token) || !Self::is_valid_request_path(request_path) {
            return None;
        }
        let (has_session, file) = {
            let mut sessions = self.sessions.lock().expect("diff session lock poisoned");
            Self::prune_expired(&mut sessions, self.max_age, now);
            let has_session = sessions.contains_key(token);
            let file = sessions
                .get(token)
                .and_then(|s| s.files_by_path.get(request_path))
                .cloned();
            (has_session, file)
        };
        if let Some(file) = file {
            return Some(file);
        }
        // Miss on an active session: reload the manifest from disk once and retry
        // so a newly-appended entry resolves instead of missing.
        // (`register_from_manifest` takes the lock itself, so call it unlocked.)
        if !has_session || !self.register_from_manifest(token, now) {
            return None;
        }
        let sessions = self.sessions.lock().expect("diff session lock poisoned");
        sessions
            .get(token)
            .and_then(|s| s.files_by_path.get(request_path))
            .cloned()
    }

    // --- pure validators (associated fns; no self, no I/O) ----------------

    /// Swift `isValidToken` (`:2458`): 16–80 chars, each alphanumeric or `-`.
    pub fn is_valid_token(token: &str) -> bool {
        let len = token.chars().count();
        if !(16..=80).contains(&len) {
            return false;
        }
        token.chars().all(|c| c.is_alphanumeric() || c == '-')
    }

    /// Swift `isValidRequestPath` (`:2465`): absolute, no backslash, no `//`, and
    /// every path component after the leading `/` is non-empty and not `.`/`..`.
    pub fn is_valid_request_path(path: &str) -> bool {
        if !path.starts_with('/') || path.contains('\\') || path.contains("//") {
            return false;
        }
        // Swift splits keeping empty subsequences then drops the leading empty
        // one; `split('/').skip(1)` matches (leading empty from the root `/`).
        let mut components = path.split('/').skip(1).peekable();
        if components.peek().is_none() {
            return false;
        }
        components.all(|c| !c.is_empty() && c != "." && c != "..")
    }

    /// Swift `isAllowedMimeType` (`:2485`).
    pub fn is_allowed_mime_type(mime: &str) -> bool {
        matches!(mime, "text/html" | "text/javascript" | "text/x-diff")
    }

    /// Swift `pathExtensionMatchesMimeType` (`:2489`).
    pub fn path_extension_matches_mime_type(path: &str, mime: &str) -> bool {
        match mime {
            "text/html" => path.ends_with(".html"),
            "text/javascript" => path.ends_with(".mjs") || path.ends_with(".js"),
            "text/x-diff" => path.ends_with(".patch"),
            _ => false,
        }
    }

    // --- internals --------------------------------------------------------

    /// Swift `isTrustedDiffViewerFileURL` (`:2624`): the file must live strictly
    /// under the trusted root (root itself and prefix-siblings are rejected).
    /// Both sides are canonicalized+normalized so `\\?\` verbatim prefixes and
    /// drive-letter case do not defeat the prefix check on Windows.
    fn is_trusted_file(&self, path: &Path) -> bool {
        let key = canonical_repo_root(&path.to_string_lossy());
        let prefix = format!("{}{}", self.trusted_root_key, MAIN_SEPARATOR);
        key.starts_with(&prefix)
    }

    /// Swift `pruneExpiredSessionsLocked` (`:2629`): drop sessions older than
    /// `max_age`. A session stamped in the future (`duration_since` errors) is
    /// treated as not-yet-expired, matching Swift's `<=` comparison.
    fn prune_expired(sessions: &mut HashMap<String, Session>, max_age: Duration, now: SystemTime) {
        sessions.retain(|_, s| match now.duration_since(s.created_at) {
            Ok(age) => age <= max_age,
            Err(_) => true,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A unique temp directory removed on drop, used as a trusted root.
    struct TempRoot {
        path: PathBuf,
    }

    impl TempRoot {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            let mut path = std::env::temp_dir();
            path.push(format!("cmux-diff-session-{pid}-{n}"));
            std::fs::create_dir_all(&path).expect("create temp root");
            // Canonicalize so the stored trusted-root key matches canonicalized
            // candidate files (temp_dir on Windows can be an 8.3 / symlinked
            // path).
            let path = std::fs::canonicalize(&path).expect("canonicalize temp root");
            Self { path }
        }

        /// Write `contents` to `name` under the root and return its path.
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

    fn entry(request_path: &str, file_path: PathBuf, mime: &str) -> RegisteredFile {
        RegisteredFile {
            request_path: request_path.to_string(),
            file_path,
            mime_type: mime.to_string(),
        }
    }

    // --- token validation ---

    #[test]
    fn token_length_bounds() {
        assert!(!DiffSessionRegistry::is_valid_token(&"a".repeat(15)));
        assert!(DiffSessionRegistry::is_valid_token(&"a".repeat(16)));
        assert!(DiffSessionRegistry::is_valid_token(&"a".repeat(80)));
        assert!(!DiffSessionRegistry::is_valid_token(&"a".repeat(81)));
    }

    #[test]
    fn token_charset() {
        assert!(DiffSessionRegistry::is_valid_token("abcABC012-def-GHI789"));
        assert!(!DiffSessionRegistry::is_valid_token(
            "has spaces in it xxxx"
        ));
        assert!(!DiffSessionRegistry::is_valid_token(
            "under_score_not_allow"
        ));
        assert!(!DiffSessionRegistry::is_valid_token(
            "slash/not/allowed/xxx"
        ));
    }

    // --- request path traversal ---

    #[test]
    fn request_path_accepts_normal_paths() {
        assert!(DiffSessionRegistry::is_valid_request_path("/index.html"));
        assert!(DiffSessionRegistry::is_valid_request_path(
            "/assets/app.mjs"
        ));
        assert!(DiffSessionRegistry::is_valid_request_path("/a/b/c.patch"));
    }

    #[test]
    fn request_path_rejects_traversal_and_malformed() {
        for bad in [
            "/",        // no real component
            "relative", // not absolute
            "//double", // empty interior component
            "/a//b",    // empty interior component
            "/a/",      // empty trailing component
            "/a/../b",  // parent traversal
            "/a/./b",   // dot segment
            "/..",      // parent traversal at root
            "/a\\b",    // backslash
        ] {
            assert!(
                !DiffSessionRegistry::is_valid_request_path(bad),
                "should reject {bad:?}"
            );
        }
    }

    // --- mime allowlist + extension match ---

    #[test]
    fn mime_allowlist_and_extension_match() {
        assert!(DiffSessionRegistry::is_allowed_mime_type("text/html"));
        assert!(DiffSessionRegistry::is_allowed_mime_type("text/javascript"));
        assert!(DiffSessionRegistry::is_allowed_mime_type("text/x-diff"));
        assert!(!DiffSessionRegistry::is_allowed_mime_type("image/png"));

        assert!(DiffSessionRegistry::path_extension_matches_mime_type(
            "/a.html",
            "text/html"
        ));
        assert!(!DiffSessionRegistry::path_extension_matches_mime_type(
            "/a.js",
            "text/html"
        ));
        assert!(DiffSessionRegistry::path_extension_matches_mime_type(
            "/a.mjs",
            "text/javascript"
        ));
        assert!(DiffSessionRegistry::path_extension_matches_mime_type(
            "/a.js",
            "text/javascript"
        ));
        assert!(DiffSessionRegistry::path_extension_matches_mime_type(
            "/a.patch",
            "text/x-diff"
        ));
        assert!(!DiffSessionRegistry::path_extension_matches_mime_type(
            "/a.patch",
            "text/html"
        ));
    }

    // --- register round-trip + lookups ---

    #[test]
    fn register_round_trip_then_lookup() {
        let root = TempRoot::new();
        let f = root.file("index.html", "<html></html>");
        let reg = DiffSessionRegistry::new(&root.path);
        let now = SystemTime::now();

        reg.register(
            "tok-abcdef0123456789",
            vec![entry("/index.html", f.clone(), "text/html")],
            now,
        )
        .expect("register");

        assert!(reg.has_active_session("tok-abcdef0123456789", now));
        let got = reg
            .registered_file("tok-abcdef0123456789", "/index.html", now)
            .expect("file present");
        assert_eq!(got.request_path, "/index.html");
        assert_eq!(got.mime_type, "text/html");
        // Miss on unknown path / unknown token.
        assert!(reg
            .registered_file("tok-abcdef0123456789", "/missing.html", now)
            .is_none());
        assert!(!reg.has_active_session("other-token-1234567", now));
    }

    #[test]
    fn register_rejects_empty_allowlist() {
        let root = TempRoot::new();
        let reg = DiffSessionRegistry::new(&root.path);
        let err = reg
            .register("tok-abcdef0123456789", vec![], SystemTime::now())
            .unwrap_err();
        assert_eq!(err, DiffSessionError::EmptyAllowlist);
    }

    #[test]
    fn register_rejects_invalid_token() {
        let root = TempRoot::new();
        let f = root.file("index.html", "x");
        let reg = DiffSessionRegistry::new(&root.path);
        let err = reg
            .register(
                "short",
                vec![entry("/index.html", f, "text/html")],
                SystemTime::now(),
            )
            .unwrap_err();
        assert_eq!(err, DiffSessionError::InvalidToken);
    }

    #[test]
    fn register_rejects_bad_path_and_mime_mismatch() {
        let root = TempRoot::new();
        let f = root.file("index.html", "x");
        let reg = DiffSessionRegistry::new(&root.path);
        let now = SystemTime::now();

        let bad_path = reg.register(
            "tok-abcdef0123456789",
            vec![entry("/../escape.html", f.clone(), "text/html")],
            now,
        );
        assert_eq!(bad_path.unwrap_err(), DiffSessionError::InvalidEntry);

        let mime_mismatch = reg.register(
            "tok-abcdef0123456789",
            vec![entry("/index.html", f, "text/x-diff")],
            now,
        );
        assert_eq!(mime_mismatch.unwrap_err(), DiffSessionError::InvalidEntry);
    }

    #[test]
    fn register_jails_files_to_trusted_root() {
        let root = TempRoot::new();
        let outside = TempRoot::new();
        let f = outside.file("evil.html", "x");
        let reg = DiffSessionRegistry::new(&root.path);
        let err = reg
            .register(
                "tok-abcdef0123456789",
                vec![entry("/evil.html", f, "text/html")],
                SystemTime::now(),
            )
            .unwrap_err();
        assert_eq!(err, DiffSessionError::FileNotReadable);
    }

    #[test]
    fn register_rejects_missing_file() {
        let root = TempRoot::new();
        let missing = root.path.join("nope.html");
        let reg = DiffSessionRegistry::new(&root.path);
        let err = reg
            .register(
                "tok-abcdef0123456789",
                vec![entry("/nope.html", missing, "text/html")],
                SystemTime::now(),
            )
            .unwrap_err();
        assert_eq!(err, DiffSessionError::FileNotReadable);
    }

    #[test]
    fn register_rejects_duplicate_request_path() {
        let root = TempRoot::new();
        let a = root.file("a.html", "x");
        let b = root.file("b.html", "y");
        let reg = DiffSessionRegistry::new(&root.path);
        let err = reg
            .register(
                "tok-abcdef0123456789",
                vec![
                    entry("/same.html", a, "text/html"),
                    entry("/same.html", b, "text/html"),
                ],
                SystemTime::now(),
            )
            .unwrap_err();
        assert_eq!(err, DiffSessionError::DuplicateEntry);
    }

    // --- expiry ---

    #[test]
    fn sessions_expire_after_max_age() {
        let root = TempRoot::new();
        let f = root.file("index.html", "x");
        let reg = DiffSessionRegistry::with_max_age(&root.path, Duration::from_secs(24 * 60 * 60));
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        reg.register(
            "tok-abcdef0123456789",
            vec![entry("/index.html", f, "text/html")],
            t0,
        )
        .expect("register");

        let within = t0 + Duration::from_secs(23 * 60 * 60);
        assert!(reg.has_active_session("tok-abcdef0123456789", within));

        let after = t0 + Duration::from_secs(25 * 60 * 60);
        assert!(!reg.has_active_session("tok-abcdef0123456789", after));
        // And a lookup after expiry misses too.
        assert!(reg
            .registered_file("tok-abcdef0123456789", "/index.html", after)
            .is_none());
    }

    #[test]
    fn re_register_replaces_prior_session() {
        let root = TempRoot::new();
        let a = root.file("a.html", "x");
        let b = root.file("b.html", "y");
        let reg = DiffSessionRegistry::new(&root.path);
        let now = SystemTime::now();
        reg.register(
            "tok-abcdef0123456789",
            vec![entry("/a.html", a, "text/html")],
            now,
        )
        .expect("register a");
        reg.register(
            "tok-abcdef0123456789",
            vec![entry("/b.html", b, "text/html")],
            now,
        )
        .expect("register b");
        // Old path is gone, new path is present.
        assert!(reg
            .registered_file("tok-abcdef0123456789", "/a.html", now)
            .is_none());
        assert!(reg
            .registered_file("tok-abcdef0123456789", "/b.html", now)
            .is_some());
    }

    // --- manifest session-restore ---

    /// Write a `.manifest-<token>.json` under `root` for `(request_path,
    /// file_path, mime)` entries, JSON-escaping the (Windows-backslash) paths.
    fn write_manifest(root: &TempRoot, token: &str, entries: &[(&str, &Path, &str)]) {
        let files: Vec<serde_json::Value> = entries
            .iter()
            .map(|(rp, fp, mime)| {
                serde_json::json!({
                    "request_path": rp,
                    "file_path": fp.to_string_lossy(),
                    "mime_type": mime,
                })
            })
            .collect();
        let manifest = serde_json::json!({ "files": files }).to_string();
        root.file(&crate::manifest::manifest_file_name(token), &manifest);
    }

    #[test]
    fn has_active_session_restores_from_manifest() {
        let root = TempRoot::new();
        let f = root.file("index.html", "<html>ok</html>");
        let token = "tok-manifest-abc12345";
        write_manifest(&root, token, &[("/index.html", &f, "text/html")]);

        let reg = DiffSessionRegistry::new(&root.path);
        let now = SystemTime::now();
        // No in-memory session, but the manifest fallback restores it.
        assert!(reg.has_active_session(token, now));
        let got = reg
            .registered_file(token, "/index.html", now)
            .expect("restored file");
        assert_eq!(got.request_path, "/index.html");
        assert_eq!(got.mime_type, "text/html");
    }

    #[test]
    fn register_from_manifest_jails_files_outside_trusted_root() {
        let root = TempRoot::new();
        let outside = TempRoot::new();
        let evil = outside.file("evil.html", "<html>evil</html>");
        let token = "tok-outside-abcd12345";
        // The manifest points at a file outside the trusted root; the reused
        // `register` jail must reject it, leaving no active session.
        write_manifest(&root, token, &[("/evil.html", &evil, "text/html")]);

        let reg = DiffSessionRegistry::new(&root.path);
        let now = SystemTime::now();
        assert!(!reg.register_from_manifest(token, now));
        assert!(!reg.has_active_session(token, now));
        assert!(reg.registered_file(token, "/evil.html", now).is_none());
    }

    #[test]
    fn registered_file_reloads_manifest_on_miss() {
        let root = TempRoot::new();
        let a = root.file("a.html", "<html>a</html>");
        let token = "tok-reload-abcd1234567";
        write_manifest(&root, token, &[("/a.html", &a, "text/html")]);

        let reg = DiffSessionRegistry::new(&root.path);
        let now = SystemTime::now();
        // Prime the in-memory session via the manifest fallback.
        assert!(reg.has_active_session(token, now));
        assert!(reg.registered_file(token, "/a.html", now).is_some());
        // b.html is not registered yet.
        assert!(reg.registered_file(token, "/b.html", now).is_none());

        // Append b.html to the manifest out-of-band (as the branch-picker
        // regenerate route would), then miss -> reload -> hit.
        let b = root.file("b.html", "<html>b</html>");
        write_manifest(
            &root,
            token,
            &[("/a.html", &a, "text/html"), ("/b.html", &b, "text/html")],
        );
        assert!(reg.registered_file(token, "/b.html", now).is_some());
    }

    #[test]
    fn manifest_missing_or_malformed_yields_no_session() {
        let root = TempRoot::new();
        let token = "tok-missing-abcd12345";
        let reg = DiffSessionRegistry::new(&root.path);
        let now = SystemTime::now();
        // No manifest on disk.
        assert!(!reg.has_active_session(token, now));

        // Malformed manifest -> None -> no session.
        root.file(&crate::manifest::manifest_file_name(token), "not json");
        assert!(!reg.register_from_manifest(token, now));
        assert!(!reg.has_active_session(token, now));
    }
}
