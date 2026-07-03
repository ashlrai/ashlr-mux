//! DiffComment model + per-repo JSON store.
//!
//! Faithful port of Swift `DiffCommentStore` (`Sources/DiffCommentStore.swift`).
//! One JSON file per git repo, keyed by a hash of the canonical repo root path,
//! under `<data_dir>/cmux/diff-comments/<repoKey>.json`. On Windows `data_dir`
//! is `%APPDATA%` (Roaming); on macOS it is `Application Support`, matching the
//! canonical layout.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[cfg(feature = "ts")]
use ts_rs::TS;

// NOTE (cmux core-types): the `#[cfg_attr(feature = "ts", ...)]` derives below
// are the only thing the optional `ts` feature adds. They are inert in the
// default build (ts-rs is an optional dep), so `cargo test`/`clippy` for the
// default configuration are unaffected. `#[ts(export)]` makes the type emit a
// `.ts` file into `TS_RS_EXPORT_DIR` when the export-bindings tests run. Mirrors
// the pattern in `cmux-core`.

/// A review comment left on a line range in the diff viewer.
///
/// `endLine` (on `side`) is the anchor line the comment renders under;
/// `lineText` is that line's content at save time so the comment can be
/// re-anchored when the same diff is regenerated with shifted line numbers.
///
/// Field declaration order is deliberately the alphabetical order of the
/// *serialized* (camelCase) keys. serde serializes struct fields in declaration
/// order, so this yields byte-stable, sorted-key JSON independent of the
/// `serde_json/preserve_order` feature — mirroring Swift's
/// `JSONEncoder.OutputFormatting.sortedKeys`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(TS), ts(export, rename_all = "camelCase"))]
pub struct DiffComment {
    /// Set when a submission delivered this comment to an agent; consumed
    /// comments never re-enter the pending pool. ISO8601 string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub consumed_at: Option<String>,
    /// ISO8601 string; preserved across updates.
    pub created_at: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub end_line: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub end_side: Option<String>,
    pub file_path: String,
    /// UUID string.
    pub id: String,
    /// The anchor line's content at save time.
    pub line_text: String,
    pub message: String,
    /// `"additions"` or `"deletions"`.
    pub side: String,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub start_line: i64,
    /// Formatted text block appended to a submission when the workspace's
    /// pending pool is consumed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub submission_text: Option<String>,
    /// ISO8601 string; refreshed on every save.
    pub updated_at: String,
}

/// On-disk shape of a per-repo comments file: `{ repoRoot, comments: [...] }`.
///
/// Fields are declared in sorted-key order (`comments` < `repoRoot`) so the
/// serialized output is deterministic without relying on any serde_json feature.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RepoCommentsFile {
    comments: Vec<DiffComment>,
    repo_root: String,
}

/// Persists diff viewer review comments per git repository.
#[derive(Debug, Clone)]
pub struct DiffCommentStore {
    directory: PathBuf,
}

impl DiffCommentStore {
    /// Creates a store rooted at an explicit directory (used by tests and any
    /// caller that wants to override the default location).
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// Creates a store rooted at the default OS location:
    /// `<data_dir>/cmux/diff-comments`. Returns `None` when the platform data
    /// directory cannot be resolved.
    pub fn with_default_directory() -> Option<Self> {
        Some(Self::new(Self::default_directory()?))
    }

    /// `<data_dir>/cmux/diff-comments`. On Windows this is `%APPDATA%\cmux\...`.
    pub fn default_directory() -> Option<PathBuf> {
        Some(dirs::data_dir()?.join("cmux").join("diff-comments"))
    }

    /// All comments persisted for `repo_root`. Returns an empty vector when the
    /// file is missing or unreadable/unparseable (mirrors the Swift store, which
    /// treats any load failure as "no comments").
    pub fn list(&self, repo_root: &str) -> Vec<DiffComment> {
        self.load_file(repo_root).comments
    }

    /// Inserts a new comment or replaces an existing one with the same `id`.
    /// On update the original `created_at` is preserved and `updated_at` is left
    /// as supplied by the caller (the Swift store leaves timestamp policy to the
    /// bridge, which sets both on construction). Returns the stored comment.
    pub fn upsert(&self, comment: DiffComment, repo_root: &str) -> io::Result<DiffComment> {
        let mut file = self.load_file(repo_root);
        let stored = if let Some(existing) = file.comments.iter_mut().find(|c| c.id == comment.id) {
            let mut updated = comment;
            updated.created_at = existing.created_at.clone();
            *existing = updated.clone();
            updated
        } else {
            file.comments.push(comment.clone());
            comment
        };
        self.save_file(&file, repo_root)?;
        Ok(stored)
    }

    /// Marks comments as delivered to an agent so they never re-enter the
    /// pending submission pool. `at` is an ISO8601 timestamp string. Faithful
    /// port of Swift `markConsumed`.
    pub fn mark_consumed(&self, ids: &[String], repo_root: &str, at: &str) -> io::Result<bool> {
        if ids.is_empty() {
            return Ok(false);
        }
        let mut file = self.load_file(repo_root);
        let mut changed = false;
        for comment in file.comments.iter_mut() {
            if ids.iter().any(|id| id == &comment.id) {
                comment.consumed_at = Some(at.to_string());
                changed = true;
            }
        }
        if changed {
            self.save_file(&file, repo_root)?;
        }
        Ok(changed)
    }

    /// Removes the comment with `id`. Idempotent: returns `true` only when a
    /// comment was actually removed.
    pub fn delete(&self, id: &str, repo_root: &str) -> io::Result<bool> {
        let mut file = self.load_file(repo_root);
        let before = file.comments.len();
        file.comments.retain(|c| c.id != id);
        if file.comments.len() == before {
            return Ok(false);
        }
        self.save_file(&file, repo_root)?;
        Ok(true)
    }

    fn load_file(&self, repo_root: &str) -> RepoCommentsFile {
        let path = self.file_path(repo_root);
        let empty = || RepoCommentsFile {
            comments: Vec::new(),
            repo_root: canonical_repo_root(repo_root),
        };
        match std::fs::read(&path) {
            Ok(data) => serde_json::from_slice::<RepoCommentsFile>(&data).unwrap_or_else(|_| empty()),
            Err(_) => empty(),
        }
    }

    fn save_file(&self, file: &RepoCommentsFile, repo_root: &str) -> io::Result<()> {
        let path = self.file_path(repo_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Struct fields are declared in sorted-key order, so plain
        // `to_string_pretty` already yields sorted keys deterministically.
        let mut json = serde_json::to_string_pretty(file)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        json.push('\n');
        std::fs::write(&path, json)
    }

    fn file_path(&self, repo_root: &str) -> PathBuf {
        self.directory.join(format!("{}.json", repo_key(repo_root)))
    }
}

/// Canonicalizes a repo root path for stable keying.
///
/// Mirrors Swift `canonicalRepoRoot` (`standardizedFileURL.resolvingSymlinksInPath`).
/// On Windows `std::fs::canonicalize` resolves symlinks and produces an absolute
/// path with a `\\?\` verbatim prefix; we strip that prefix and lowercase the
/// drive letter so the same repo always hashes to the same key regardless of the
/// caller's drive-letter casing. When the path does not exist (canonicalize
/// fails) we fall back to lexical normalization of the raw input.
pub(crate) fn canonical_repo_root(raw: &str) -> String {
    match std::fs::canonicalize(raw) {
        Ok(p) => normalize_windows_path(&p.to_string_lossy()),
        Err(_) => normalize_windows_path(raw),
    }
}

/// Strips the Windows `\\?\` (and `\\?\UNC\`) verbatim prefix and lowercases the
/// drive letter. A no-op on non-verbatim / non-drive paths, so it is safe on any
/// platform.
fn normalize_windows_path(s: &str) -> String {
    let stripped = if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.to_string()
    };
    lower_drive_letter(&stripped)
}

/// Lowercases a leading `X:` drive letter (`C:\foo` -> `c:\foo`).
fn lower_drive_letter(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let mut out = String::with_capacity(s.len());
        out.push((bytes[0] as char).to_ascii_lowercase());
        out.push_str(&s[1..]);
        out
    } else {
        s.to_string()
    }
}

/// Lowercase hex of the first 24 chars (12 bytes) of `SHA256(canonicalRepoRoot)`.
/// Faithful port of Swift `repoKey(forRepoRoot:)`.
pub(crate) fn repo_key(repo_root: &str) -> String {
    let canonical = canonical_repo_root(repo_root);
    let digest = Sha256::digest(canonical.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in digest.iter() {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex.truncate(24);
    hex
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// A unique temp directory that is removed on drop. Kept inside the lib
    /// (`#[cfg(test)]`) so the process-spawning Windows Application Control
    /// (os 4551) gotcha for standalone test .exes does not apply.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let pid = std::process::id();
            let path = std::env::temp_dir().join(format!("cmux-diff-test-{tag}-{pid}-{n}"));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self { path }
        }

        fn child(&self, name: &str) -> PathBuf {
            let p = self.path.join(name);
            std::fs::create_dir_all(&p).expect("create child dir");
            p
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn sample_comment(id: &str) -> DiffComment {
        DiffComment {
            consumed_at: None,
            created_at: "2026-07-01T00:00:00Z".to_string(),
            end_line: 42,
            end_side: None,
            file_path: "src/main.rs".to_string(),
            id: id.to_string(),
            line_text: "let x = 1;".to_string(),
            message: "nit".to_string(),
            side: "additions".to_string(),
            start_line: 40,
            submission_text: Some("review: nit".to_string()),
            updated_at: "2026-07-01T00:00:00Z".to_string(),
        }
    }

    fn store_for(tmp: &TempDir) -> DiffCommentStore {
        DiffCommentStore::new(tmp.child("store"))
    }

    #[test]
    fn round_trip_save_list_delete() {
        let tmp = TempDir::new("roundtrip");
        let repo = tmp.child("repo").to_string_lossy().to_string();
        let store = store_for(&tmp);

        assert!(store.list(&repo).is_empty(), "starts empty");

        let c = sample_comment("11111111-1111-1111-1111-111111111111");
        let saved = store.upsert(c.clone(), &repo).unwrap();
        assert_eq!(saved, c);

        let listed = store.list(&repo);
        assert_eq!(listed, vec![c.clone()]);

        // Idempotent delete of a missing id changes nothing.
        assert!(!store.delete("does-not-exist", &repo).unwrap());
        assert_eq!(store.list(&repo).len(), 1);

        assert!(store.delete(&c.id, &repo).unwrap());
        assert!(store.list(&repo).is_empty());
    }

    #[test]
    fn created_at_preserved_on_update() {
        let tmp = TempDir::new("update");
        let repo = tmp.child("repo").to_string_lossy().to_string();
        let store = store_for(&tmp);

        let mut c = sample_comment("22222222-2222-2222-2222-222222222222");
        c.created_at = "2026-01-01T00:00:00Z".to_string();
        c.updated_at = "2026-01-01T00:00:00Z".to_string();
        store.upsert(c.clone(), &repo).unwrap();

        // Same id, different createdAt/message/updatedAt.
        let mut edit = c.clone();
        edit.created_at = "2099-12-31T23:59:59Z".to_string(); // should be ignored
        edit.updated_at = "2026-02-02T00:00:00Z".to_string();
        edit.message = "edited".to_string();
        let stored = store.upsert(edit, &repo).unwrap();

        assert_eq!(stored.created_at, "2026-01-01T00:00:00Z", "createdAt preserved");
        assert_eq!(stored.updated_at, "2026-02-02T00:00:00Z", "updatedAt refreshed");
        assert_eq!(stored.message, "edited");

        let listed = store.list(&repo);
        assert_eq!(listed.len(), 1, "upsert replaces, not appends");
        assert_eq!(listed[0].created_at, "2026-01-01T00:00:00Z");
    }

    #[test]
    fn idempotent_delete_returns_changed_flag() {
        let tmp = TempDir::new("idempotent");
        let repo = tmp.child("repo").to_string_lossy().to_string();
        let store = store_for(&tmp);

        let c = sample_comment("33333333-3333-3333-3333-333333333333");
        store.upsert(c.clone(), &repo).unwrap();

        assert!(store.delete(&c.id, &repo).unwrap(), "first delete changed");
        assert!(!store.delete(&c.id, &repo).unwrap(), "second delete no-op");
        assert!(!store.delete(&c.id, &repo).unwrap(), "third delete no-op");
    }

    #[test]
    fn mark_consumed_sets_timestamp() {
        let tmp = TempDir::new("consumed");
        let repo = tmp.child("repo").to_string_lossy().to_string();
        let store = store_for(&tmp);

        let a = sample_comment("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa");
        let b = sample_comment("bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        store.upsert(a.clone(), &repo).unwrap();
        store.upsert(b.clone(), &repo).unwrap();

        assert!(!store.mark_consumed(&[], &repo, "t").unwrap(), "empty ids no-op");

        let changed = store
            .mark_consumed(std::slice::from_ref(&a.id), &repo, "2026-07-01T12:00:00Z")
            .unwrap();
        assert!(changed);

        let listed = store.list(&repo);
        let got_a = listed.iter().find(|c| c.id == a.id).unwrap();
        let got_b = listed.iter().find(|c| c.id == b.id).unwrap();
        assert_eq!(got_a.consumed_at.as_deref(), Some("2026-07-01T12:00:00Z"));
        assert_eq!(got_b.consumed_at, None, "untouched comment unchanged");
    }

    #[test]
    fn repo_key_is_stable_and_distinct() {
        let tmp = TempDir::new("key");
        let repo_a = tmp.child("repo-a").to_string_lossy().to_string();
        let repo_b = tmp.child("repo-b").to_string_lossy().to_string();

        // Same path -> same key.
        assert_eq!(repo_key(&repo_a), repo_key(&repo_a));
        // Different path -> different key.
        assert_ne!(repo_key(&repo_a), repo_key(&repo_b));
        // Key shape: 24 lowercase hex chars.
        let key = repo_key(&repo_a);
        assert_eq!(key.len(), 24);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn repo_key_ignores_drive_letter_case_on_windows() {
        // On Windows, canonicalization lowercases the drive letter so callers
        // that differ only by drive case land on the same file. On non-Windows
        // this still holds trivially (both canonicalize identically).
        let tmp = TempDir::new("drivecase");
        let repo = tmp.child("repo");
        let lower = repo.to_string_lossy().to_string();
        // Uppercase the drive letter if present.
        let upper = {
            let bytes = lower.as_bytes();
            if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                let mut s = String::new();
                s.push((bytes[0] as char).to_ascii_uppercase());
                s.push_str(&lower[1..]);
                s
            } else {
                lower.clone()
            }
        };
        assert_eq!(repo_key(&lower), repo_key(&upper));
    }

    #[test]
    fn stored_file_has_sorted_keys() {
        let tmp = TempDir::new("sorted");
        let repo = tmp.child("repo").to_string_lossy().to_string();
        let store = store_for(&tmp);

        let c = sample_comment("44444444-4444-4444-4444-444444444444");
        store.upsert(c, &repo).unwrap();

        let path = store.file_path(&repo);
        let raw = std::fs::read_to_string(&path).unwrap();

        // Top-level keys: comments before repoRoot.
        let comments_at = raw.find("\"comments\"").expect("comments key");
        let repo_root_at = raw.find("\"repoRoot\"").expect("repoRoot key");
        assert!(comments_at < repo_root_at, "top-level keys sorted");

        // Per-comment keys sorted (a subset check of the alphabetical order).
        let order = [
            "\"createdAt\"",
            "\"endLine\"",
            "\"filePath\"",
            "\"id\"",
            "\"lineText\"",
            "\"message\"",
            "\"side\"",
            "\"startLine\"",
            "\"submissionText\"",
            "\"updatedAt\"",
        ];
        let mut last = 0usize;
        for key in order {
            let at = raw.find(key).unwrap_or_else(|| panic!("missing {key}"));
            assert!(at >= last, "key {key} out of sorted order");
            last = at;
        }
    }

    #[test]
    fn key_order_is_deterministic_across_saves() {
        let tmp = TempDir::new("determinism");
        let repo = tmp.child("repo").to_string_lossy().to_string();
        let store = store_for(&tmp);

        let c = sample_comment("55555555-5555-5555-5555-555555555555");
        store.upsert(c.clone(), &repo).unwrap();
        let path = store.file_path(&repo);
        let first = std::fs::read_to_string(&path).unwrap();

        // Re-save the identical content; bytes must be identical.
        store.upsert(c, &repo).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first, second, "serialization is byte-stable");
    }

    #[test]
    fn optional_none_fields_are_omitted() {
        let tmp = TempDir::new("omit");
        let repo = tmp.child("repo").to_string_lossy().to_string();
        let store = store_for(&tmp);

        let c = DiffComment {
            end_side: None,
            submission_text: None,
            consumed_at: None,
            ..sample_comment("66666666-6666-6666-6666-666666666666")
        };
        store.upsert(c, &repo).unwrap();
        let raw = std::fs::read_to_string(store.file_path(&repo)).unwrap();
        assert!(!raw.contains("endSide"), "None endSide omitted");
        assert!(!raw.contains("submissionText"), "None submissionText omitted");
        assert!(!raw.contains("consumedAt"), "None consumedAt omitted");
    }

    #[test]
    fn persists_across_store_instances() {
        let tmp = TempDir::new("persist");
        let repo = tmp.child("repo").to_string_lossy().to_string();
        let dir = tmp.child("store");

        let c = sample_comment("77777777-7777-7777-7777-777777777777");
        DiffCommentStore::new(dir.clone())
            .upsert(c.clone(), &repo)
            .unwrap();

        // A fresh store instance over the same directory sees the file.
        let reopened = DiffCommentStore::new(dir);
        assert_eq!(reopened.list(&repo), vec![c]);
    }
}
