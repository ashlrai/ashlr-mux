//! `cmux-diff` — the diff viewer's review-comment store, ported faithfully from
//! the canonical macOS Swift sources `Sources/DiffCommentStore.swift` and the
//! `comments.list` / `comments.save` / `comments.delete` contract in
//! `Sources/Panels/DiffCommentsBridge.swift`.
//!
//! `comment_store` is the persistence layer; `session` is the
//! `cmux-diff-viewer://` URL-scheme token/session trust model (validators +
//! trusted-root jail + expiry); `manifest` is the on-disk session-restore loader
//! wired into `session`; `submission_pool` is the workspace-scoped pending
//! review-comment pool. Only the Tauri `diff_comments_rpc` command wiring (and
//! `submission_pool`'s bridge glue, which needs a workspace id the RPC layer does
//! not yet carry) remains deferred (see the crate README/task notes).

pub mod comment_store;
pub mod manifest;
pub mod rpc;
pub mod session;
pub mod submission_pool;

pub use comment_store::{DiffComment, DiffCommentStore};
pub use manifest::{diff_viewer_restorable, load_manifest_files};
pub use rpc::dispatch_comment_rpc;
pub use session::{DiffSessionError, DiffSessionRegistry, RegisteredFile};
pub use submission_pool::{DiffCommentSubmissionPool, Entry as SubmissionEntry};
