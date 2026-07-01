//! Pure side-channel describing the I/O a [`crate::ProcessStore`] wants performed.
//!
//! The store is a pure, headless, OS-free state machine (see
//! [`crate::process_store`]). But Codex and OpenCode both need a read→write
//! feedback loop — the store must *react* to a consumed line (or a `writeLine`
//! call) by writing frames to the child's stdin, or by making an HTTP call — and
//! that I/O cannot happen inside the pure crate.
//!
//! So the store never performs I/O. Instead it appends [`TransportAction`]s — a
//! pure, `serde_json`-only description of exactly what to write / call — which the
//! host (the `src-tauri` actor) drains via
//! [`ProcessStore::take_transport_actions`](crate::ProcessStore::take_transport_actions)
//! after every interaction and executes against the process/HTTP handles it owns.
//!
//! This is the seam that keeps the [`crate::AgentTransport`] trait unchanged (the
//! Claude write path still flows through it) while giving Codex/OpenCode their
//! reactive writes a testable home: unit tests drive the store and assert the
//! emitted `Vec<TransportAction>` with no transport and no spawned process, so the
//! crate stays os-4551-safe.

/// One unit of host I/O the store wants performed, in order.
///
/// All variants carry the store's `session_id` so the host can route to the right
/// live child / HTTP context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportAction {
    /// Write these exact bytes to the session's child stdin.
    ///
    /// `line` is the output of [`crate::codex::encode_line`] — a JSON object with
    /// a single trailing `\n` (`0x0A`). The host writes it **raw** and must NOT
    /// re-append a newline (doing so would corrupt the JSON-RPC framing).
    WriteStdin {
        /// The store session id.
        session_id: String,
        /// The already-`\n`-terminated frame bytes.
        line: String,
    },
    /// Tear the session's child process tree down (best-effort). Emitted on the
    /// startup-failure path; any `provider.exit` the failure implies was already
    /// emitted by the store, so this is pure I/O cleanup.
    Terminate {
        /// The store session id.
        session_id: String,
    },
    /// OpenCode call **A**: `POST {base_url}/session` to create the loopback
    /// session, then (on success) begin the `/event` SSE stream. Emitted once,
    /// when the loopback URL is first sniffed from the child's output.
    OpenCodeCreateSession {
        /// The store session id.
        session_id: String,
        /// The captured loopback base URL (raw string).
        base_url: String,
    },
    /// OpenCode call **C**: `POST {base_url}/session/{opencode_session_id}/prompt_async`
    /// to submit a user prompt. Fire-and-forget (the assistant reply arrives over
    /// the `/event` SSE stream, not this response).
    OpenCodePostPrompt {
        /// The store session id.
        session_id: String,
        /// The captured loopback base URL (raw string).
        base_url: String,
        /// The created OpenCode loopback session id.
        opencode_session_id: String,
        /// The prompt text (verbatim; permission mode is not sent for OpenCode).
        text: String,
    },
}
