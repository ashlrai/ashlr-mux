//! On-disk transcript-history / JSONL parsing subsystem.
//!
//! Ports the canonical macOS `Packages/Shared/CmuxAgentChat/Sources/
//! CmuxAgentChat/{Model,Parsing}` value types and parsers. This is DISTINCT
//! from the crate's live provider transports (`super::claude`, `super::codex`,
//! `super::opencode`), which are stream-json / JSON-RPC / SSE accumulators for
//! the live webview host: those consume a running agent's stdout, while this
//! subsystem reads the agent's *persisted* session JSONL (`~/.claude/
//! projects/**`, `~/.codex/sessions/**`) to rebuild history. Everything here
//! is stateless value-type + parser logic.
//!
//! The two parser modules are deliberately named [`claude`] and [`codex`] to
//! mirror the Swift filenames; they live under `transcript::` so they do not
//! collide with the crate's top-level `claude` / `codex` transport modules.

pub mod batch_assembler;
pub mod claude;
pub mod codex;
pub mod diff_builder;
pub mod json_value;
pub mod model;
pub mod parse_state;
pub mod text_budget;
pub mod timestamp;
pub mod tool_completion;

// Value types (Model/*.swift).
pub use model::{
    ChatAttachment, ChatAttachmentMedia, ChatFileEdit, ChatFileEditOperation, ChatMessage,
    ChatMessageKind, ChatPermissionRequest, ChatPermissionResolution, ChatProse, ChatQuestion,
    ChatQuestionOption, ChatRole, ChatStatusEvent, ChatStatusTransition, ChatTerminalCapture,
    ChatThought, ChatToolUse, ChatToolUseStatus, ChatUnsupportedPayload,
};

// Parsers and their I/O (Parsing/*.swift).
pub use claude::ClaudeTranscriptParser;
pub use codex::CodexTranscriptParser;
pub use parse_state::{ChatTranscriptParseResult, ChatTranscriptParseState};
pub use timestamp::{Timestamp, TranscriptTimestampParser};
