//! `cmux-mentions` — pure @-mention completion logic for the agent text box,
//! ported from the canonical macOS Swift `TextBoxMention*` family.
//!
//! Module map (one module per canonical Swift file):
//!
//! | Rust module            | Swift source                                  |
//! |------------------------|-----------------------------------------------|
//! | [`kind`]               | `TextBoxMentionKind.swift`                    |
//! | [`query`]              | `TextBoxMentionQuery.swift`                   |
//! | [`suggestion`]         | `TextBoxMentionSuggestion.swift`              |
//! | [`candidate`]          | `TextBoxMentionCandidate.swift`               |
//! | [`candidate_index`]    | `TextBoxMentionCandidateIndex.swift`          |
//! | [`completion_detector`]| `TextBoxMentionCompletionDetector.swift`      |
//! | [`markdown`]           | `TextBoxMentionMarkdown.swift`                |
//! | [`cached_index`]       | `TextBoxMentionCachedIndex.swift`             |
//! | [`index_store`]        | `TextBoxMentionIndexStore.swift` (pure parts) |
//!
//! [`palette`] is an internal support port of the exact `CmuxCommandPalette`
//! pieces `TextBoxMentionCandidateIndex.swift` imports (fuzzy matcher, search
//! corpus entry/result, search engine). It belongs to a future command-palette
//! crate; it lives here so the mention index can rank candidates today.
//!
//! # Intentionally NOT ported (host-coupled)
//!
//! - `TextBoxMentionCompletionController.swift` — a `@MainActor @Observable`
//!   UI state machine (popover visibility, selection index, async lookup
//!   `Task` generations, `onStateChanged` callbacks). It drives AppKit view
//!   state and Swift-concurrency task cancellation; it is the windowing
//!   host's job on Windows.
//! - `TextBoxMentionFileIndexRefreshTask.swift` — a wrapper around a detached
//!   Swift-concurrency `Task<TextBoxMentionCandidateIndex, Never>` used to
//!   coalesce in-flight filesystem scans. The pure metadata it carries
//!   (`id`, `startedAt`) is folded into the scope note on [`index_store`].
//! - The I/O half of `TextBoxMentionIndexStore.swift` — filesystem
//!   enumeration, ripgrep/`git check-ignore` subprocess scanning, skill-root
//!   discovery under `~/.codex`/`~/.agents`, and the actor/task coalescing
//!   around them. The pure filtering/ranking/path-normalization logic it
//!   mixes in IS ported in [`index_store`].
//!
//! # Cross-cutting divergences (see `// DIVERGENCE:` comments at use sites)
//!
//! - Swift `Character` is a grapheme cluster; this port indexes Unicode
//!   scalar values (`char`). Behavior differs only for combining sequences.
//! - `normalizeForSearch` performs no diacritic folding and no locale-aware
//!   case folding (plain Unicode lowercasing only).
//! - `localizedStandardCompare` / `localizedCaseInsensitiveCompare` are
//!   approximated without locale collation tables ([`compare`]).
//! - The candidate index never builds a nucleo accelerator index; it always
//!   takes Swift's documented nucleo-unavailable fallback path.

pub mod cached_index;
pub mod candidate;
pub mod candidate_index;
pub mod compare;
pub mod completion_detector;
pub mod index_store;
pub mod kind;
pub mod markdown;
pub mod palette;
pub mod query;
pub mod suggestion;

pub use cached_index::MentionCachedIndex;
pub use candidate::MentionCandidate;
pub use candidate_index::MentionCandidateIndex;
pub use completion_detector::mention_query_in;
pub use kind::MentionKind;
pub use query::{MentionQuery, Utf16Range};
pub use suggestion::MentionSuggestion;
