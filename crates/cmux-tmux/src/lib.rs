//! `cmux-tmux` — pure parsers for the remote-tmux control-mode family plus the
//! tmux resume-binding derivation.
//!
//! Ported verbatim (modulo documented `// DIVERGENCE:` notes) from the canonical
//! macOS Swift sources:
//!
//! - [`control_stream`] ← `RemoteTmuxControlStreamParser.swift` — incremental
//!   `tmux -CC` byte-stream parser (keeps `%output` byte-oriented so multi-byte
//!   UTF-8 split across notifications survives).
//! - [`raw_layout`] ← `RemoteTmuxRawLayoutParser.swift` — recursive
//!   `WxH,X,Y{…}[…]` layout → node tree.
//! - [`session_list`] ← `RemoteTmuxSessionListParser.swift` — `:`-delimited
//!   `list-sessions -F` output → sessions.
//! - [`control_message`] ← `RemoteTmuxControlMessageDecoding.swift` — stateless
//!   payload decoders (pane-state seed sequence, window reorder, stderr triage).
//! - [`models`] ← `RemoteTmuxLayoutNode.swift`, `RemoteTmuxLayoutContent.swift`,
//!   `RemoteTmuxSession.swift`, `RemoteTmuxControlMessage.swift`.
//! - [`resume`] ← `TmuxResumeParser.swift` (+ the minimal
//!   `SurfaceResumeBindingSnapshot` output type from `SessionPersistence.swift`).
//!
//! All parsers are pure functions over `String`/`[u8]` with no network, view, or
//! actor dependencies.

pub mod control_message;
pub mod control_stream;
pub mod models;
pub mod raw_layout;
pub mod resume;
pub mod session_list;

pub use control_stream::RemoteTmuxControlStreamParser;
pub use models::{
    RemoteTmuxControlMessage, RemoteTmuxLayoutContent, RemoteTmuxLayoutNode, RemoteTmuxSession,
};
pub use resume::SurfaceResumeBindingSnapshot;
