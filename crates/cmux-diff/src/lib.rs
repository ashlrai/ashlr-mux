//! `cmux-diff` — the diff viewer's review-comment store, ported faithfully from
//! the canonical macOS Swift sources `Sources/DiffCommentStore.swift` and the
//! `comments.list` / `comments.save` / `comments.delete` contract in
//! `Sources/Panels/DiffCommentsBridge.swift`.
//!
//! This slice is the persistence layer only. The `cmux-diff-viewer://`
//! URL-scheme/token session model, the `DiffCommentSubmissionPool`, and the
//! Tauri `diff_comments_rpc` command are intentionally deferred (see the crate
//! README/task notes).

pub mod comment_store;

pub use comment_store::{DiffComment, DiffCommentStore};
