//! Workspace-scoped pool of saved-but-unsent diff review comments.
//!
//! Faithful port of Swift `DiffCommentSubmissionPool`
//! (`Sources/DiffCommentSubmissionPool.swift`) plus the bridge's `registerPending`
//! guard (`Sources/Panels/DiffCommentsBridge.swift:190`). The SwiftUI
//! `ObservableObject`/`@Published` machinery is dropped — every terminal TextBox
//! chip re-render is a UI concern that lives above this pure store; what remains
//! is the `[workspaceId -> [Entry]]` bookkeeping.
//!
//! Whichever TextBox in a workspace submits first calls `consume_all`, appends
//! the entries' `submission_text` to its outgoing submission, and either marks
//! them consumed in the store or `restore_pending`s them on a failed submit.
//!
//! NOT WIRED INTO `rpc::dispatch_comment_rpc` (deliberate, DEFERRED): the Swift
//! bridge calls `registerPending` from `comments.save` / `removePending` from
//! `comments.delete` with the resolved workspace id. `dispatch_comment_rpc` has
//! no workspace id, and threading one through would force a conflicting rpc
//! signature change, so that glue waits for the Tauri command layer. The
//! workspace id is a plain parameter here so the future command layer can supply
//! it without touching this module.

use std::collections::HashMap;

use crate::comment_store::{canonical_repo_root, DiffComment};

/// One pending comment ready to append to a submission. Faithful port of Swift
/// `DiffCommentSubmissionPool.Entry` (`:14`): the comment id, the canonicalized
/// repo root, and the formatted submission text.
///
/// DIVERGENCE (documented): the task brief lists a "consumed flag" field, but the
/// Swift `Entry` has exactly these three fields and no consumed flag — the
/// consumed check is a gate in `register_pending` (Swift `registerPending`
/// checks `comment.consumedAt == nil` before enqueue), not per-entry state. We
/// keep the faithful 3-field `Entry`. `comment_id`/`repo_root` are `String`s
/// (Swift uses `UUID`/`String`); the id stays a string to match the store and
/// the JS wire ids, avoiding a surprising UUID re-canonicalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub comment_id: String,
    /// Canonicalized repo root (Swift `DiffCommentStore.canonicalRepoRoot`).
    pub repo_root: String,
    pub submission_text: String,
}

/// Workspace-keyed pending pool. A workspace never holds an empty entry list —
/// `remove_pending` / `consume_all` drop the key when it empties, matching
/// Swift's `entriesByWorkspace[workspaceId] = nil`.
#[derive(Debug, Default)]
pub struct DiffCommentSubmissionPool {
    entries_by_workspace: HashMap<String, Vec<Entry>>,
}

impl DiffCommentSubmissionPool {
    /// A fresh, empty pool.
    pub fn new() -> Self {
        Self::default()
    }

    /// Upsert one entry for a workspace, deduped by comment id (replace, not
    /// append). Faithful port of Swift `setPending` (`:22`).
    pub fn set_pending(&mut self, entry: Entry, workspace_id: &str) {
        let entries = self
            .entries_by_workspace
            .entry(workspace_id.to_string())
            .or_default();
        if let Some(existing) = entries
            .iter_mut()
            .find(|e| e.comment_id == entry.comment_id)
        {
            *existing = entry;
        } else {
            entries.push(entry);
        }
    }

    /// Drop a comment from every workspace it appears in, dropping any workspace
    /// that empties. Faithful port of Swift `removePending` (`:32`).
    pub fn remove_pending(&mut self, comment_id: &str) {
        for entries in self.entries_by_workspace.values_mut() {
            entries.retain(|e| e.comment_id != comment_id);
        }
        // The pool never stores an empty list, so dropping empties globally is
        // equivalent to Swift's per-workspace `= nil` on the changed workspaces.
        self.entries_by_workspace
            .retain(|_, entries| !entries.is_empty());
    }

    /// The pending count for a workspace, or 0 for `None`/unknown. Faithful port
    /// of Swift `pendingCount` (`:41`), whose argument is optional.
    pub fn pending_count(&self, workspace_id: Option<&str>) -> usize {
        match workspace_id {
            Some(id) => self.entries_by_workspace.get(id).map_or(0, Vec::len),
            None => 0,
        }
    }

    /// Claim (and clear) every pending entry for a workspace. Faithful port of
    /// Swift `consumeAll` (`:49`).
    pub fn consume_all(&mut self, workspace_id: &str) -> Vec<Entry> {
        match self.entries_by_workspace.remove(workspace_id) {
            Some(entries) if !entries.is_empty() => entries,
            _ => Vec::new(),
        }
    }

    /// Put entries claimed by `consume_all` back (failed-submit rollback).
    /// Faithful port of Swift `restorePending` (`:56`).
    pub fn restore_pending(&mut self, entries: Vec<Entry>, workspace_id: &str) {
        for entry in entries {
            self.set_pending(entry, workspace_id);
        }
    }

    /// Enqueue a just-saved comment iff it is unconsumed and carries non-empty
    /// submission text. Faithful port of the bridge guard `registerPending`
    /// (`Sources/Panels/DiffCommentsBridge.swift:190`), which canonicalizes the
    /// repo root before storage. Returns whether an entry was enqueued (Swift is
    /// `Void`; the bool is a convenience for callers/tests and adds no behavior).
    pub fn register_pending(
        &mut self,
        comment: &DiffComment,
        repo_root: &str,
        workspace_id: &str,
    ) -> bool {
        // Swift guard order: consumedAt == nil, then submissionText non-nil &&
        // non-empty.
        if comment.consumed_at.is_some() {
            return false;
        }
        let Some(submission_text) = comment.submission_text.as_deref().filter(|s| !s.is_empty())
        else {
            return false;
        };
        self.set_pending(
            Entry {
                comment_id: comment.id.clone(),
                repo_root: canonical_repo_root(repo_root),
                submission_text: submission_text.to_string(),
            },
            workspace_id,
        );
        true
    }

    /// Whether the pool holds no pending entries in any workspace. A Rust
    /// read-only convenience (no Swift analogue); useful to assert the
    /// drop-empties invariant.
    pub fn is_empty(&self) -> bool {
        self.entries_by_workspace.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::comment_store::canonical_repo_root;

    fn entry(id: &str, text: &str) -> Entry {
        Entry {
            comment_id: id.to_string(),
            repo_root: "/repo".to_string(),
            submission_text: text.to_string(),
        }
    }

    fn comment(id: &str, submission: Option<&str>, consumed: Option<&str>) -> DiffComment {
        DiffComment {
            consumed_at: consumed.map(str::to_string),
            created_at: "2026-07-02T00:00:00Z".to_string(),
            end_line: 1,
            end_side: None,
            file_path: "src/main.rs".to_string(),
            id: id.to_string(),
            line_text: "x".to_string(),
            message: "m".to_string(),
            side: "additions".to_string(),
            start_line: 1,
            submission_text: submission.map(str::to_string),
            updated_at: "2026-07-02T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn set_pending_dedups_by_comment_id() {
        let mut pool = DiffCommentSubmissionPool::new();
        pool.set_pending(entry("c1", "v1"), "ws-1");
        pool.set_pending(entry("c1", "v2"), "ws-1");
        assert_eq!(
            pool.pending_count(Some("ws-1")),
            1,
            "replaced, not appended"
        );
        let entries = pool.consume_all("ws-1");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].submission_text, "v2");
    }

    #[test]
    fn remove_pending_drops_empty_workspaces() {
        let mut pool = DiffCommentSubmissionPool::new();
        pool.set_pending(entry("c1", "a"), "ws-1");
        pool.set_pending(entry("c2", "b"), "ws-2");
        pool.set_pending(entry("c3", "c"), "ws-2");

        pool.remove_pending("c1"); // empties ws-1 -> workspace key dropped
        assert_eq!(pool.pending_count(Some("ws-1")), 0);
        assert_eq!(
            pool.pending_count(Some("ws-2")),
            2,
            "unaffected workspace intact"
        );

        pool.remove_pending("c2");
        assert_eq!(pool.pending_count(Some("ws-2")), 1);
        pool.remove_pending("c3");
        assert!(
            pool.is_empty(),
            "no empty lists linger once all entries removed"
        );
    }

    #[test]
    fn pending_count_none_and_unknown_workspace() {
        let mut pool = DiffCommentSubmissionPool::new();
        pool.set_pending(entry("c1", "a"), "ws-1");
        assert_eq!(pool.pending_count(None), 0);
        assert_eq!(pool.pending_count(Some("ws-unknown")), 0);
        assert_eq!(pool.pending_count(Some("ws-1")), 1);
    }

    #[test]
    fn consume_all_clears_and_restore_round_trips() {
        let mut pool = DiffCommentSubmissionPool::new();
        pool.set_pending(entry("c1", "a"), "ws-1");
        pool.set_pending(entry("c2", "b"), "ws-1");

        let taken = pool.consume_all("ws-1");
        assert_eq!(taken.len(), 2);
        assert!(pool.is_empty(), "consumeAll clears the workspace");
        // Consuming again yields nothing.
        assert!(pool.consume_all("ws-1").is_empty());

        pool.restore_pending(taken.clone(), "ws-1");
        assert_eq!(pool.pending_count(Some("ws-1")), 2, "restore round-trips");
    }

    #[test]
    fn register_pending_enqueues_valid_and_canonicalizes_repo() {
        let mut pool = DiffCommentSubmissionPool::new();
        let repo = "/work/repo";
        assert!(pool.register_pending(&comment("c1", Some("please fix"), None), repo, "ws-1"));
        assert_eq!(pool.pending_count(Some("ws-1")), 1);
        let entries = pool.consume_all("ws-1");
        assert_eq!(entries[0].comment_id, "c1");
        assert_eq!(entries[0].submission_text, "please fix");
        assert_eq!(
            entries[0].repo_root,
            canonical_repo_root(repo),
            "repo root canonicalized"
        );
    }

    #[test]
    fn register_pending_skips_consumed_or_missing_or_empty_submission() {
        let mut pool = DiffCommentSubmissionPool::new();
        // Consumed comment is skipped even with submission text.
        assert!(!pool.register_pending(
            &comment("c1", Some("text"), Some("2026-07-02T00:00:00Z")),
            "/r",
            "ws-1"
        ));
        // No submission text.
        assert!(!pool.register_pending(&comment("c2", None, None), "/r", "ws-1"));
        // Empty submission text.
        assert!(!pool.register_pending(&comment("c3", Some(""), None), "/r", "ws-1"));
        assert!(pool.is_empty(), "no guard-failing comment was enqueued");
    }
}
