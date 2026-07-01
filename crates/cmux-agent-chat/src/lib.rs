//! Pure, headless foundations of the cmux agent-session host contract.
//!
//! The live agent-session webview talks to its host over two seams:
//!
//! * a **request** seam (renderer → host): [`BridgeRequest`] decodes the
//!   `{id, method, params}` envelope and offers typed parameter getters, with
//!   [`BridgeError`] as the failure vocabulary.
//! * a **push** seam (host → renderer): [`AgentEvent`] serializes to the exact
//!   type-tagged, camelCase wire objects the renderer consumes.
//!
//! This crate is the FIRST SLICE: only the pure serde + parsing pieces that the
//! later provider transports (codex / claude / opencode) and the Tauri command
//! will sit on top of. The wire types are mirrored verbatim from the canonical
//! macOS Swift sources under `Sources/Panels/AgentSession*.swift` and validated
//! against `webviews/src/agent-session/shared/types.ts`.
//!
//! ## Deferred (NOT in this slice)
//!
//! The stateful provider transports (codex JSON-RPC, claude stream-json,
//! opencode SSE / http-loopback), the running-session / process-store host
//! state, app-context / theme assembly, and the separate mobile
//! transcript-history / JSONL subsystem (`Sources/Mobile/AgentChat/*`) all come
//! with later slices, together with their `tokio` / `reqwest` / `cmux-process`
//! / `cmux-agent` dependencies.

pub mod claude;
pub mod codex;
pub mod error;
pub mod event;
pub mod line_buffer;
pub mod opencode;
pub mod permission_mode;
pub mod request;

pub use claude::{write_claude_stream_json, ClaudeStreamAccumulator};
pub use codex::{
    encode_line, initialize_request, initialized_notification, thread_start_request,
    turn_start_request, unsupported_server_request_error, CodexAccumulator, SubmitRejection,
};
pub use error::BridgeError;
pub use event::{
    ActivityKind, ActivityStatus, AgentEvent, AgentSessionRateLimitRow, AgentSessionTheme,
    ProviderId, ProviderStream, RateLimitRole,
};
pub use line_buffer::OutputLineBuffer;
pub use opencode::{
    opencode_process_output_disposition, OpenCodeEvent, OpenCodeEventStreamParser,
    OpenCodeEventTextAccumulator, OpenCodeProcessOutputDisposition,
};
pub use permission_mode::PermissionMode;
pub use request::BridgeRequest;
