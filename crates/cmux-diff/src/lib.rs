//! `cmux-diff` — the diff viewer's review-comment store, ported faithfully from
//! the canonical macOS Swift sources `Sources/DiffCommentStore.swift` and the
//! `comments.list` / `comments.save` / `comments.delete` contract in
//! `Sources/Panels/DiffCommentsBridge.swift`.
//!
//! `comment_store` is the persistence layer; `session` is the
//! `cmux-diff-viewer://` URL-scheme token/session trust model (validators +
//! trusted-root jail + expiry). The `DiffCommentSubmissionPool`, the on-disk
//! manifest session-restore, and the Tauri `diff_comments_rpc` command remain
//! deferred (see the crate README/task notes).

pub mod comment_store;
pub mod rpc;
pub mod session;

pub use comment_store::{DiffComment, DiffCommentStore};
pub use rpc::dispatch_comment_rpc;
pub use session::{DiffSessionError, DiffSessionRegistry, RegisteredFile};
