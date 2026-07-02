//! Per-session pure state for a live agent session.
//!
//! Ported from the canonical macOS Swift `AgentSessionRunningSession`
//! (`Sources/Panels/AgentSessionRunningSession.swift`) together with the
//! per-line routing of `AgentSessionProcessStore.handleOutputLine`
//! (`Sources/Panels/AgentSessionProcessStore.swift`).
//!
//! [`RunningSession`] owns everything about one running provider that is pure and
//! OS-free: the identity fields ([`RunningSession::session_id`] etc.), the two
//! per-stream [`OutputLineBuffer`]s, the provider-specific accumulator
//! ([`ProviderAccumulator`]), the drained-stream set, and the pending exit
//! status. The live child process, stdio pipes, read tasks, termination timer,
//! and the OpenCode HTTP-loopback client all live in the (deferred) transport
//! layer — see [`crate::process_store`].
//!
//! ## Output routing parity (`handleOutputLine`)
//!
//! [`RunningSession::consume_output`] mirrors the Swift routing exactly:
//!
//! * **OpenCode** process stdio is *only* sniffed for the loopback server URL
//!   ([`opencode_process_output_disposition`]): a captured URL is recorded (no
//!   event, and the deferred transport then creates the HTTP session), a
//!   `stdout` line is suppressed, and a `stderr` line is emitted as
//!   `provider.output`. The OpenCode assistant text arrives over the SSE
//!   `/event` stream instead — feed those pre-parsed events through
//!   [`RunningSession::consume_opencode_event`] (or raw SSE lines through
//!   [`RunningSession::consume_opencode_sse_line`]).
//! * **Codex** `stdout` lines feed [`CodexAccumulator::consume_line`].
//! * **Claude** `stdout` lines feed
//!   [`ClaudeStreamAccumulator::consume_line_to_events`].
//! * everything else (any provider's `stderr`, plus Codex/Claude non-stdout)
//!   is emitted verbatim as `provider.output`.

use std::collections::{HashSet, VecDeque};

use serde_json::json;

use crate::claude::ClaudeStreamAccumulator;
use crate::codex::{
    encode_line, parse_server_request, unsupported_server_request_error, CodexAccumulator,
    SubmitRejection,
};
use crate::event::{AgentEvent, ProviderId, ProviderStream};
use crate::line_buffer::OutputLineBuffer;
use crate::opencode::{
    opencode_process_output_disposition, OpenCodeEvent, OpenCodeEventStreamParser,
    OpenCodeEventTextAccumulator, OpenCodeProcessOutputDisposition,
};
use crate::permission_mode::PermissionMode;

/// The provider-specific stateful accumulator for a running session.
///
/// One variant per [`ProviderId`]; each wraps the pure accumulator already
/// ported in the sibling provider modules. The OpenCode variant additionally
/// owns the SSE line parser and the loopback session id so the (deferred)
/// transport can drive the `/event` stream through it.
#[derive(Debug)]
pub enum ProviderAccumulator {
    /// Codex `app-server` JSON-RPC accumulator.
    Codex(CodexAccumulator),
    /// Claude `stream-json` accumulator.
    Claude(ClaudeStreamAccumulator),
    /// OpenCode SSE parser + assistant-text accumulator.
    OpenCode {
        /// SSE line framer for the `/event` stream.
        parser: OpenCodeEventStreamParser,
        /// Assistant-text de-duplicating accumulator.
        text: OpenCodeEventTextAccumulator,
    },
}

impl ProviderAccumulator {
    /// Construct the accumulator for `provider_id`.
    ///
    /// `working_directory` / `client_version` are only consumed by the Codex
    /// accumulator (`clientInfo.version` + `thread/start` cwd).
    pub fn new(
        provider_id: ProviderId,
        working_directory: Option<String>,
        client_version: impl Into<String>,
    ) -> Self {
        match provider_id {
            ProviderId::Codex => {
                Self::Codex(CodexAccumulator::new(working_directory, client_version))
            }
            ProviderId::Claude => Self::Claude(ClaudeStreamAccumulator::new()),
            ProviderId::Opencode => Self::OpenCode {
                parser: OpenCodeEventStreamParser::new(),
                text: OpenCodeEventTextAccumulator::new(),
            },
        }
    }
}

/// The pure per-session state of one running agent provider.
#[derive(Debug)]
pub struct RunningSession {
    session_id: String,
    provider_id: ProviderId,
    executable_path: String,
    arguments: Vec<String>,
    working_directory: Option<String>,
    stdout_buffer: OutputLineBuffer,
    stderr_buffer: OutputLineBuffer,
    accumulator: ProviderAccumulator,
    /// The captured OpenCode loopback base URL (raw string; `None` until sniffed).
    opencode_base_url: Option<String>,
    /// The OpenCode HTTP session id, once the loopback session is created.
    opencode_session_id: Option<String>,
    /// Whether `provider.started` has already been emitted for this session.
    started_emitted: bool,
    drained_streams: HashSet<ProviderStream>,
    pending_exit_status: Option<i32>,
    /// Codex: user prompts queued before the thread exists (Swift `queuedInputs`;
    /// capped at `MAX_QUEUED_INPUT_COUNT == 1`). The accumulator keeps the pure
    /// byte/count mirror; this holds the actual `(text, permission_mode)` FIFO.
    codex_queue: VecDeque<(String, PermissionMode)>,
    /// Reactive stdin frames the store should turn into
    /// [`TransportAction::WriteStdin`](crate::TransportAction::WriteStdin). Each is
    /// already `encode_line`-terminated. Drained by the store after each feed /
    /// write / start.
    pending_outbound: Vec<String>,
    /// Set when the Codex startup-failure edge fired: the store must emit
    /// `provider.exit(1)` and terminate the child (once).
    pending_terminate: bool,
    /// Set on the OpenCode loopback-URL `None → Some` transition: the store must
    /// emit an
    /// [`OpenCodeCreateSession`](crate::TransportAction::OpenCodeCreateSession).
    pending_opencode_create: bool,
}

impl RunningSession {
    /// Create a running session for a freshly-spawned provider.
    pub fn new(
        session_id: impl Into<String>,
        provider_id: ProviderId,
        executable_path: impl Into<String>,
        arguments: Vec<String>,
        working_directory: Option<String>,
        client_version: impl Into<String>,
    ) -> Self {
        let working_directory = working_directory
            .map(|dir| dir.trim_matches(char::is_whitespace).to_string())
            .filter(|dir| !dir.is_empty());
        let accumulator =
            ProviderAccumulator::new(provider_id, working_directory.clone(), client_version);
        Self {
            session_id: session_id.into(),
            provider_id,
            executable_path: executable_path.into(),
            arguments,
            working_directory,
            stdout_buffer: OutputLineBuffer::new(),
            stderr_buffer: OutputLineBuffer::new(),
            accumulator,
            opencode_base_url: None,
            opencode_session_id: None,
            started_emitted: false,
            drained_streams: HashSet::new(),
            pending_exit_status: None,
            codex_queue: VecDeque::new(),
            pending_outbound: Vec::new(),
            pending_terminate: false,
            pending_opencode_create: false,
        }
    }

    // ---- identity accessors ----

    /// The stable session id (`UUID().uuidString` on macOS).
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// The provider driving this session.
    pub fn provider_id(&self) -> ProviderId {
        self.provider_id
    }

    /// The resolved executable path.
    pub fn executable_path(&self) -> &str {
        &self.executable_path
    }

    /// The transport launch arguments.
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// The working directory (trimmed, non-empty), if any.
    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    // ---- provider.started bookkeeping ----

    /// Whether `provider.started` has already been emitted.
    pub fn started_emitted(&self) -> bool {
        self.started_emitted
    }

    /// Mark `provider.started` as emitted (idempotent).
    pub fn mark_started_emitted(&mut self) {
        self.started_emitted = true;
    }

    /// The `provider.started` event for this session.
    pub fn started_event(&self) -> AgentEvent {
        AgentEvent::ProviderStarted {
            session_id: self.session_id.clone(),
            provider_id: self.provider_id,
            executable_path: self.executable_path.clone(),
            arguments: self.arguments.clone(),
        }
    }

    // ---- OpenCode handshake state ----

    /// The captured OpenCode loopback base URL, if the process announced one.
    pub fn opencode_base_url(&self) -> Option<&str> {
        self.opencode_base_url.as_deref()
    }

    /// The OpenCode loopback session id, once created.
    pub fn opencode_session_id(&self) -> Option<&str> {
        self.opencode_session_id.as_deref()
    }

    /// Record the created OpenCode loopback session id (Swift
    /// `session.openCodeSessionID = id`).
    pub fn set_opencode_session_id(&mut self, id: impl Into<String>) {
        self.opencode_session_id = Some(id.into());
    }

    // ---- exit / drain state ----

    /// The recorded process exit status, once the child has terminated.
    pub fn pending_exit_status(&self) -> Option<i32> {
        self.pending_exit_status
    }

    /// Record the process exit status (Swift `pendingExitStatus`).
    pub fn set_pending_exit_status(&mut self, status: i32) {
        self.pending_exit_status = Some(status);
    }

    /// Whether the process has exited *and* both streams have drained.
    ///
    /// Faithful to Swift `finishSessionIfExitedAndDrained`'s guard: an exit
    /// status is present and `drainedStreams` is a superset of
    /// `{stdout, stderr}`.
    pub fn is_exited_and_drained(&self) -> bool {
        self.pending_exit_status.is_some()
            && self.drained_streams.contains(&ProviderStream::Stdout)
            && self.drained_streams.contains(&ProviderStream::Stderr)
    }

    /// The `provider.exit` event carrying `status`.
    pub fn exit_event(&self, status: i32) -> AgentEvent {
        AgentEvent::ProviderExit {
            session_id: self.session_id.clone(),
            provider_id: self.provider_id,
            status,
        }
    }

    // ---- output feeding ----

    /// Feed a raw stdout/stderr chunk, returning the [`AgentEvent`]s it produced.
    ///
    /// Faithful port of `consumeOutputData` + `handleOutputLine`. An **empty**
    /// `data` slice signals EOF for `stream`: any buffered partial line is
    /// flushed and routed, then the stream is marked drained. A non-empty chunk
    /// is split into lines by the stream's [`OutputLineBuffer`] and each line is
    /// routed per provider.
    pub fn consume_output(&mut self, stream: ProviderStream, data: &[u8]) -> Vec<AgentEvent> {
        let lines = if data.is_empty() {
            let flushed = self.buffer_mut(stream).flush();
            self.drained_streams.insert(stream);
            flushed
        } else {
            self.buffer_mut(stream).append(data)
        };
        let mut events = Vec::new();
        for line in lines {
            events.extend(self.handle_output_line(&line, stream));
        }
        events
    }

    fn buffer_mut(&mut self, stream: ProviderStream) -> &mut OutputLineBuffer {
        match stream {
            ProviderStream::Stdout => &mut self.stdout_buffer,
            ProviderStream::Stderr => &mut self.stderr_buffer,
        }
    }

    fn handle_output_line(&mut self, text: &str, stream: ProviderStream) -> Vec<AgentEvent> {
        if self.provider_id == ProviderId::Opencode {
            match opencode_process_output_disposition(text, stream) {
                OpenCodeProcessOutputDisposition::ServerUrl(base_url) => {
                    if self.opencode_base_url.is_none() {
                        self.opencode_base_url = Some(base_url);
                        // Flag the loopback-URL `None → Some` transition so the
                        // store emits `OpenCodeCreateSession` (POST /session). No
                        // renderer event is emitted here.
                        self.pending_opencode_create = true;
                    }
                    return Vec::new();
                }
                OpenCodeProcessOutputDisposition::Suppress => return Vec::new(),
                OpenCodeProcessOutputDisposition::Emit => {
                    return vec![self.output_event(stream, text.to_string())];
                }
            }
        }

        if stream == ProviderStream::Stdout {
            match &mut self.accumulator {
                ProviderAccumulator::Codex(_) => {
                    return self.handle_codex_line(text);
                }
                ProviderAccumulator::Claude(claude) => {
                    return claude.consume_line_to_events(text, &self.session_id, self.provider_id);
                }
                ProviderAccumulator::OpenCode { .. } => {
                    // Unreachable: the OpenCode branch returned above.
                }
            }
        }

        vec![self.output_event(stream, text.to_string())]
    }

    /// Consume one Codex `stdout` line and run the read→write reactive machine.
    ///
    /// Faithful port of the ordering in `CodexAppServerSession.handleLine` +
    /// `AgentSessionProcessStore`: after the pure [`CodexAccumulator::consume_line`]
    /// updates protocol state and yields renderer events, react in this exact
    /// order (Codex spec §7):
    ///
    /// 1. **initialize just resolved** → queue `initialized` then `thread/start`
    ///    (two separate newline-framed frames, in that order).
    /// 2. **thread id just became known** → drain the queued input (FIFO, ≤1) into
    ///    `turn/start` frames.
    /// 3. **the line was a server request** → queue the approval / `-32601` reply,
    ///    echoing the raw id verbatim.
    /// 4. **the startup-failure edge fired** → fail the queue and flag teardown.
    ///
    /// Every reactive frame lands in `pending_outbound` for the store to turn into
    /// [`WriteStdin`](crate::TransportAction::WriteStdin) actions.
    fn handle_codex_line(&mut self, text: &str) -> Vec<AgentEvent> {
        let session_id = self.session_id.clone();
        let (was_initialized, had_thread, events) = match &mut self.accumulator {
            ProviderAccumulator::Codex(codex) => {
                let was_initialized = codex.did_initialize();
                let had_thread = codex.thread_id().is_some();
                let events = codex.consume_line(text, &session_id);
                (was_initialized, had_thread, events)
            }
            // Unreachable: only called from the Codex branch above.
            _ => return Vec::new(),
        };

        // (1) initialize → `initialized`, then `thread/start`.
        if !was_initialized && self.codex_ref().is_some_and(CodexAccumulator::did_initialize) {
            if let Some(initialized) = self.codex_ref().map(CodexAccumulator::initialized) {
                self.pending_outbound.push(encode_line(&initialized));
            }
            if let Some(thread_start) = self.codex_mut().and_then(CodexAccumulator::thread_start) {
                self.pending_outbound.push(encode_line(&thread_start));
            }
        }

        // (2) thread id just became known → drain queued input.
        if !had_thread && self.codex_ref().is_some_and(|c| c.thread_id().is_some()) {
            self.drain_codex_queue();
        }

        // (3) server request → approval / unsupported reply (raw id echoed).
        if let Some((id, method, params)) = parse_server_request(text) {
            let reply = match self
                .codex_ref()
                .and_then(|codex| codex.approval_response(&method, params.as_ref()))
            {
                Some(result) => json!({ "id": id, "result": result }),
                None => unsupported_server_request_error(id, &method),
            };
            self.pending_outbound.push(encode_line(&reply));
        }

        // (4) startup-failure edge → fail the queue + flag teardown (once).
        if self
            .codex_mut()
            .is_some_and(CodexAccumulator::take_startup_failure_signal)
        {
            self.fail_codex_queue();
            self.pending_terminate = true;
        }

        events
    }

    fn output_event(&self, stream: ProviderStream, text: String) -> AgentEvent {
        AgentEvent::ProviderOutput {
            session_id: self.session_id.clone(),
            provider_id: self.provider_id,
            stream,
            text,
        }
    }

    // ---- Codex write side (reactive + user submit) ----

    /// Borrow the Codex accumulator, if this is a Codex session.
    fn codex_ref(&self) -> Option<&CodexAccumulator> {
        match &self.accumulator {
            ProviderAccumulator::Codex(codex) => Some(codex),
            _ => None,
        }
    }

    /// Mutably borrow the Codex accumulator, if this is a Codex session.
    fn codex_mut(&mut self) -> Option<&mut CodexAccumulator> {
        match &mut self.accumulator {
            ProviderAccumulator::Codex(codex) => Some(codex),
            _ => None,
        }
    }

    /// Begin the Codex handshake: queue the initial `initialize` request.
    ///
    /// Mirrors Swift `CodexAppServerSession.start()` (the one unconditional write
    /// before any read). A no-op for non-Codex sessions. The store calls this once
    /// after spawning, so the frame is drained + written before any stdout arrives.
    pub fn begin_codex_handshake(&mut self) {
        if let Some(frame) = self.codex_mut().map(CodexAccumulator::initialize) {
            self.pending_outbound.push(encode_line(&frame));
        }
    }

    /// Handle a user `writeLine` for a Codex session (Swift `submit`).
    ///
    /// Reproduces the guard order exactly: empty text is a silent success; a
    /// startup failure or an in-flight turn rejects; before the thread exists the
    /// input is queued (subject to [`CodexAccumulator::can_queue_input`]) and, if
    /// `initialize` has already resolved, a `thread/start` is (re)issued (spec
    /// §3.4); once the thread exists the `turn/start` frame is queued immediately.
    /// Built frames land in `pending_outbound`.
    pub fn codex_submit(
        &mut self,
        permission_mode: PermissionMode,
        text: &str,
    ) -> Result<(), SubmitRejection> {
        if text.is_empty() {
            return Ok(());
        }
        let attempt = match self.codex_mut() {
            Some(codex) => codex.turn_start(text, permission_mode),
            // Not a Codex session — nothing to do (the store routes by provider).
            None => return Ok(()),
        };
        match attempt {
            Ok(frame) => {
                self.pending_outbound.push(encode_line(&frame));
                Ok(())
            }
            Err(SubmitRejection::ThreadNotReady) => {
                if !self.codex_ref().is_some_and(|codex| codex.can_queue_input(text)) {
                    return Err(SubmitRejection::ThreadNotReady);
                }
                self.codex_queue
                    .push_back((text.to_string(), permission_mode));
                if let Some(codex) = self.codex_mut() {
                    codex.note_queued(text);
                }
                // Swift: if already initialized when a prompt is queued, kick a
                // (re)`thread/start` so the queue can eventually drain.
                if self.codex_ref().is_some_and(CodexAccumulator::did_initialize) {
                    if let Some(frame) = self.codex_mut().and_then(CodexAccumulator::thread_start) {
                        self.pending_outbound.push(encode_line(&frame));
                    }
                }
                Ok(())
            }
            Err(other) => Err(other),
        }
    }

    /// Drain the queued input(s) into `turn/start` frames (Swift
    /// `drainCodexAppServerQueuedInputs`). Called when the thread id first becomes
    /// known. `MAX_QUEUED_INPUT_COUNT == 1`, so at most one input is drained.
    fn drain_codex_queue(&mut self) {
        let mut frames = Vec::new();
        while let Some((text, permission_mode)) = self.codex_queue.pop_front() {
            if let Some(frame) = self
                .codex_mut()
                .and_then(|codex| codex.turn_start(&text, permission_mode).ok())
            {
                frames.push(encode_line(&frame));
            }
        }
        if let Some(codex) = self.codex_mut() {
            codex.clear_queue();
        }
        self.pending_outbound.extend(frames);
    }

    /// Fail + clear the queued inputs (Swift `failQueuedInputs`). Called from the
    /// startup-failure edge; the pending prompts never get sent.
    fn fail_codex_queue(&mut self) {
        self.codex_queue.clear();
        if let Some(codex) = self.codex_mut() {
            codex.clear_queue();
        }
    }

    // ---- reactive pending-action accessors (drained by the store) ----

    /// Take the queued stdin frames (each already `encode_line`-terminated).
    pub fn take_pending_outbound(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_outbound)
    }

    /// Take + clear the Codex startup-failure teardown flag.
    pub fn take_pending_terminate(&mut self) -> bool {
        std::mem::take(&mut self.pending_terminate)
    }

    /// Take + clear the OpenCode "create loopback session" flag.
    pub fn take_pending_opencode_create(&mut self) -> bool {
        std::mem::take(&mut self.pending_opencode_create)
    }

    // ---- OpenCode SSE feeding ----

    /// Feed a raw SSE line from the OpenCode `/event` stream.
    ///
    /// Frames the line through the owned [`OpenCodeEventStreamParser`] and routes
    /// each dispatched event through [`consume_opencode_event`](Self::consume_opencode_event).
    /// A no-op for non-OpenCode sessions.
    pub fn consume_opencode_sse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let events = match &mut self.accumulator {
            ProviderAccumulator::OpenCode { parser, .. } => parser.consume_line(line),
            _ => return Vec::new(),
        };
        let mut out = Vec::new();
        for event in events {
            out.extend(self.consume_opencode_event(&event));
        }
        out
    }

    /// Feed one parsed OpenCode SSE event, mapping assistant text + turn
    /// completion to [`AgentEvent`]s.
    ///
    /// Faithful port of `AgentSessionProcessStore.handleOpenCodeEvent` (scoped to
    /// the pure accumulator work): the event's session id must match the created
    /// OpenCode session id. A no-op for non-OpenCode sessions or before the
    /// loopback session exists.
    pub fn consume_opencode_event(&mut self, event: &OpenCodeEvent) -> Vec<AgentEvent> {
        let Some(opencode_session_id) = self.opencode_session_id.clone() else {
            return Vec::new();
        };
        let provider_id = self.provider_id;
        // Match on the OpenCode loopback id; stamp the emitted renderer events with
        // this session's cmux id (what the frontend routes on).
        let emit_session_id = self.session_id.clone();
        match &mut self.accumulator {
            ProviderAccumulator::OpenCode { text, .. } => text.consume_event_to_events(
                event,
                &opencode_session_id,
                &emit_session_id,
                provider_id,
            ),
            _ => Vec::new(),
        }
    }
}
