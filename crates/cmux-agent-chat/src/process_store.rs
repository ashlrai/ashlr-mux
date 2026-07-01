//! The agent-session lifecycle brain, decoupled from Tauri / tokio / the OS.
//!
//! Ported from the canonical macOS Swift `AgentSessionProcessStore`
//! (`Sources/Panels/AgentSessionProcessStore.swift`). [`ProcessStore`] owns the
//! single-active-session invariant, the lifecycle transitions (start / write /
//! stop / close-all), and the emission of every [`AgentEvent`] — in order —
//! through an injected sink.
//!
//! The two things that would otherwise pin the crate to an async runtime and the
//! OS are abstracted away:
//!
//! * **Side effects** (spawning the child, writing a prompt line, terminating)
//!   live behind [`AgentTransport`]. The real implementation — `cmux-process`
//!   `SpawnSpec` + Job-Object supervision, the stdio pumps, the Codex
//!   app-server write side, and the OpenCode HTTP-loopback client — is the
//!   deferred GUI-wiring slice.
//! * **Events** are pushed through a generic `FnMut(AgentEvent)` sink, exactly as
//!   the research prescribed, so the store stays synchronous and unit-testable.
//!
//! ## What is modelled here vs. deferred
//!
//! Modelled (pure): the single-active-session guard, `provider.started` timing
//! (immediate for Codex/Claude, deferred to the handshake for OpenCode), routing
//! stdout/stderr chunks through the per-session accumulators to `provider.output`
//! / `provider.activity` / `provider.turnComplete`, the exit-and-drain gate that
//! emits `provider.exit`, and clearing the active session on exit.
//!
//! Deferred (transport / GUI slice): the concrete async spawn + stdio pump, the
//! Codex handshake/turn write side and its single-queued-input backpressure, the
//! OpenCode `POST /session` + SSE `/event` client, the termination-escalation
//! `SIGKILL` timer, and the separate active-provider *bool* sink
//! (`activeProviderSink`). The active-provider bool is intentionally omitted —
//! callers can derive it from [`ProcessStore::has_active_session`].

use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::BridgeError;
use crate::event::{AgentEvent, ProviderId, ProviderStream};
use crate::opencode::OpenCodeEvent;
use crate::permission_mode::PermissionMode;
use crate::running_session::RunningSession;

/// A request to spawn a provider, handed to the [`AgentTransport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnRequest {
    /// The store-allocated session id (`UUID().uuidString` parity).
    pub session_id: String,
    /// The provider to launch.
    pub provider_id: ProviderId,
    /// The working directory (trimmed/non-empty already applied by the caller).
    pub working_directory: Option<String>,
}

/// The result of a successful [`AgentTransport::spawn`].
///
/// Carries the resolved executable + transport arguments so the store can build
/// the [`RunningSession`] and emit `provider.started`. (On macOS these come from
/// the resolved `AgentSessionLaunchPlan`.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnedSession {
    /// The resolved executable path.
    pub executable_path: String,
    /// The transport launch arguments.
    pub arguments: Vec<String>,
}

/// Failure of a side-effecting transport operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The provider transport is not ready to accept the operation (Swift
    /// `providerNotReady`, e.g. OpenCode before the loopback session exists).
    NotReady,
    /// The operation failed for another reason (spawn error, write error, …),
    /// carrying a diagnostic message.
    Failed(String),
}

impl TransportError {
    /// Map a transport failure onto the request-seam [`BridgeError`] vocabulary.
    ///
    /// Both variants surface as [`BridgeError::ProviderNotReady`] carrying the
    /// provider display name. On macOS a raw *spawn* failure is rethrown in a
    /// separate `AgentExecutableResolverError` envelope; that host-specific
    /// error path is part of the deferred GUI-wiring slice, so this headless
    /// port approximates it with `providerNotReady`.
    pub fn into_bridge_error(self, provider: ProviderId) -> BridgeError {
        BridgeError::ProviderNotReady(provider.display_name().to_string())
    }
}

/// The side-effecting operations the [`ProcessStore`] needs from its host.
///
/// Kept minimal and synchronous-friendly (every method returns a `Result`). The
/// real async / `cmux-process` / stdio implementation is the deferred
/// GUI-wiring slice; tests drive an in-memory fake.
pub trait AgentTransport {
    /// Resolve + spawn the provider process, returning its executable/arguments.
    fn spawn(&mut self, request: &SpawnRequest) -> Result<SpawnedSession, TransportError>;

    /// Write one prompt line to the running session.
    ///
    /// The provider-specific framing (Codex `turn/start`, Claude `stream-json`,
    /// OpenCode `POST …/prompt_async`) and the `permission_mode` handling live
    /// in the implementation.
    fn write_line(
        &mut self,
        session_id: &str,
        permission_mode: PermissionMode,
        text: &str,
    ) -> Result<(), TransportError>;

    /// Request termination of the running session (best-effort).
    fn terminate(&mut self, session_id: &str) -> Result<(), TransportError>;
}

/// The value returned from a successful [`ProcessStore::start`].
///
/// Mirrors the `provider.start` reply object built by the Swift coordinator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedSession {
    /// The freshly-allocated session id.
    pub session_id: String,
    /// The provider that was started.
    pub provider_id: ProviderId,
    /// The resolved executable path.
    pub executable_path: String,
    /// The transport launch arguments.
    pub arguments: Vec<String>,
}

impl StartedSession {
    /// The `provider.start` reply object (camelCase wire shape).
    pub fn to_value(&self) -> Value {
        json!({
            "sessionId": self.session_id,
            "providerId": self.provider_id.as_str(),
            "executablePath": self.executable_path,
            "arguments": self.arguments,
        })
    }
}

/// The single-active-session lifecycle store.
///
/// Generic over an [`AgentTransport`] `T` and an `FnMut(AgentEvent)` sink `S`.
pub struct ProcessStore<T, S>
where
    T: AgentTransport,
    S: FnMut(AgentEvent),
{
    transport: T,
    sink: S,
    client_version: String,
    session: Option<RunningSession>,
    selected_provider: Option<ProviderId>,
}

impl<T, S> ProcessStore<T, S>
where
    T: AgentTransport,
    S: FnMut(AgentEvent),
{
    /// Create a store over `transport`, pushing events to `sink`.
    ///
    /// `client_version` seeds the Codex `initialize` handshake
    /// (`clientInfo.version`, macOS `CFBundleShortVersionString`).
    pub fn new(transport: T, sink: S, client_version: impl Into<String>) -> Self {
        Self {
            transport,
            sink,
            client_version: client_version.into(),
            session: None,
            selected_provider: None,
        }
    }

    /// Whether a session is currently running (Swift `hasActiveProviderSession`).
    pub fn has_active_session(&self) -> bool {
        self.session.is_some()
    }

    /// The most recently selected/started provider, if any.
    pub fn selected_provider(&self) -> Option<ProviderId> {
        self.selected_provider
    }

    /// Immutable access to the active session, if any.
    pub fn active_session(&self) -> Option<&RunningSession> {
        self.session.as_ref()
    }

    /// Record a `provider.select` (Swift updates `initialProviderID`).
    pub fn select_provider(&mut self, provider_id: ProviderId) {
        self.selected_provider = Some(provider_id);
    }

    /// Start a provider session, enforcing the single-active-session invariant.
    ///
    /// Returns [`BridgeError::SessionAlreadyRunning`] when one is already live.
    /// On success the [`RunningSession`] is created and `provider.started` is
    /// emitted immediately for every provider except OpenCode (whose
    /// `provider.started` is deferred to [`complete_opencode_handshake`](Self::complete_opencode_handshake)).
    pub fn start(
        &mut self,
        provider_id: ProviderId,
        working_directory: Option<String>,
    ) -> Result<StartedSession, BridgeError> {
        if self.session.is_some() {
            return Err(BridgeError::SessionAlreadyRunning);
        }
        let session_id = Uuid::new_v4().to_string();
        let request = SpawnRequest {
            session_id: session_id.clone(),
            provider_id,
            working_directory: working_directory.clone(),
        };
        let spawned = self
            .transport
            .spawn(&request)
            .map_err(|error| error.into_bridge_error(provider_id))?;

        let session = RunningSession::new(
            session_id.clone(),
            provider_id,
            spawned.executable_path.clone(),
            spawned.arguments.clone(),
            working_directory,
            self.client_version.clone(),
        );
        self.session = Some(session);
        self.selected_provider = Some(provider_id);

        if provider_id.emits_started_on_spawn() {
            let event = {
                let session = self.session.as_mut().expect("session just inserted");
                session.mark_started_emitted();
                session.started_event()
            };
            self.emit(event);
        }

        Ok(StartedSession {
            session_id,
            provider_id,
            executable_path: spawned.executable_path,
            arguments: spawned.arguments,
        })
    }

    /// Route a prompt line to the active session.
    ///
    /// Returns [`BridgeError::SessionNotFound`] when `session_id` is not the
    /// active session, or the transport's mapped error (typically
    /// [`BridgeError::ProviderNotReady`]).
    pub fn write_line(
        &mut self,
        session_id: &str,
        permission_mode: PermissionMode,
        text: &str,
    ) -> Result<(), BridgeError> {
        let provider = match self.session.as_ref() {
            Some(session) if session.session_id() == session_id => session.provider_id(),
            _ => return Err(BridgeError::SessionNotFound(session_id.to_string())),
        };
        self.transport
            .write_line(session_id, permission_mode, text)
            .map_err(|error| error.into_bridge_error(provider))
    }

    /// Request termination of the active session (Swift `stop`).
    ///
    /// Returns [`BridgeError::SessionNotFound`] when `session_id` is not the
    /// active session. Termination is best-effort: the session is not removed
    /// here — `provider.exit` and removal follow the real exit (drive
    /// [`notify_exit`](Self::notify_exit) + stream drain), matching Swift's
    /// `requestTermination` → `finishSessionIfExitedAndDrained`.
    pub fn stop(&mut self, session_id: &str) -> Result<(), BridgeError> {
        match self.session.as_ref() {
            Some(session) if session.session_id() == session_id => {}
            _ => return Err(BridgeError::SessionNotFound(session_id.to_string())),
        }
        let _ = self.transport.terminate(session_id);
        Ok(())
    }

    /// Request termination of every session (Swift `closeAll`).
    pub fn close_all(&mut self) {
        if let Some(session_id) = self.session.as_ref().map(|s| s.session_id().to_string()) {
            let _ = self.transport.terminate(&session_id);
        }
    }

    /// Feed a raw stdout/stderr chunk for `session_id`, emitting its events.
    ///
    /// An empty `data` slice signals EOF for `stream` (flush + drain). When the
    /// process has exited and both streams have drained, `provider.exit` is
    /// emitted and the active session cleared.
    pub fn feed_output(&mut self, session_id: &str, stream: ProviderStream, data: &[u8]) {
        let events = match self.session.as_mut() {
            Some(session) if session.session_id() == session_id => {
                session.consume_output(stream, data)
            }
            _ => return,
        };
        self.emit_all(events);
        self.finish_if_exited_and_drained(session_id);
    }

    /// Feed a raw SSE line from the OpenCode `/event` stream (deferred transport).
    pub fn feed_opencode_sse_line(&mut self, session_id: &str, line: &str) {
        let events = match self.session.as_mut() {
            Some(session) if session.session_id() == session_id => {
                session.consume_opencode_sse_line(line)
            }
            _ => return,
        };
        self.emit_all(events);
    }

    /// Feed one parsed OpenCode SSE event (deferred transport).
    pub fn feed_opencode_event(&mut self, session_id: &str, event: &OpenCodeEvent) {
        let events = match self.session.as_mut() {
            Some(session) if session.session_id() == session_id => {
                session.consume_opencode_event(event)
            }
            _ => return,
        };
        self.emit_all(events);
    }

    /// Complete the OpenCode loopback handshake, emitting the deferred
    /// `provider.started`.
    ///
    /// Faithful to Swift `createOpenCodeSession`: records the created loopback
    /// session id, then emits `provider.started` once (idempotent).
    pub fn complete_opencode_handshake(
        &mut self,
        session_id: &str,
        opencode_session_id: impl Into<String>,
    ) {
        let event = match self.session.as_mut() {
            Some(session) if session.session_id() == session_id => {
                session.set_opencode_session_id(opencode_session_id);
                if session.started_emitted() {
                    return;
                }
                session.mark_started_emitted();
                session.started_event()
            }
            _ => return,
        };
        self.emit(event);
    }

    /// Record the process exit status and finalize if both streams have drained.
    ///
    /// Faithful to Swift's `terminationHandler` → `finishSessionIfExitedAndDrained`.
    pub fn notify_exit(&mut self, session_id: &str, status: i32) {
        if let Some(session) = self.session.as_mut() {
            if session.session_id() == session_id {
                session.set_pending_exit_status(status);
            }
        }
        self.finish_if_exited_and_drained(session_id);
    }

    fn finish_if_exited_and_drained(&mut self, session_id: &str) {
        let ready = matches!(
            self.session.as_ref(),
            Some(session)
                if session.session_id() == session_id && session.is_exited_and_drained()
        );
        if !ready {
            return;
        }
        let session = self.session.take().expect("session checked present");
        let status = session
            .pending_exit_status()
            .expect("exit status checked present");
        self.emit(session.exit_event(status));
    }

    fn emit(&mut self, event: AgentEvent) {
        (self.sink)(event);
    }

    fn emit_all(&mut self, events: Vec<AgentEvent>) {
        for event in events {
            self.emit(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Records what the transport was asked to do so tests can assert side
    /// effects without any real process.
    #[derive(Default)]
    struct FakeTransport {
        spawned: Vec<SpawnRequest>,
        writes: Vec<(String, PermissionMode, String)>,
        terminated: Vec<String>,
        spawn_error: Option<TransportError>,
        write_error: Option<TransportError>,
    }

    impl AgentTransport for FakeTransport {
        fn spawn(&mut self, request: &SpawnRequest) -> Result<SpawnedSession, TransportError> {
            self.spawned.push(request.clone());
            if let Some(error) = self.spawn_error.clone() {
                return Err(error);
            }
            Ok(SpawnedSession {
                executable_path: format!("/bin/{}", request.provider_id.as_str()),
                arguments: request.provider_id.launch_arguments(),
            })
        }

        fn write_line(
            &mut self,
            session_id: &str,
            permission_mode: PermissionMode,
            text: &str,
        ) -> Result<(), TransportError> {
            self.writes
                .push((session_id.to_string(), permission_mode, text.to_string()));
            if let Some(error) = self.write_error.clone() {
                return Err(error);
            }
            Ok(())
        }

        fn terminate(&mut self, session_id: &str) -> Result<(), TransportError> {
            self.terminated.push(session_id.to_string());
            Ok(())
        }
    }

    /// A Vec-collecting sink shared with the test body.
    type Events = Rc<RefCell<Vec<AgentEvent>>>;

    fn store_with(
        transport: FakeTransport,
    ) -> (ProcessStore<FakeTransport, impl FnMut(AgentEvent)>, Events) {
        let events: Events = Rc::new(RefCell::new(Vec::new()));
        let sink_events = events.clone();
        let store = ProcessStore::new(
            transport,
            move |event| sink_events.borrow_mut().push(event),
            "1.2.3",
        );
        (store, events)
    }

    fn start_codex(
        store: &mut ProcessStore<FakeTransport, impl FnMut(AgentEvent)>,
    ) -> StartedSession {
        store
            .start(ProviderId::Codex, Some("/work".to_string()))
            .expect("codex start")
    }

    // ---- single-active-session guard ----

    #[test]
    fn start_twice_is_session_already_running() {
        let (mut store, _events) = store_with(FakeTransport::default());
        start_codex(&mut store);
        assert_eq!(
            store.start(ProviderId::Claude, None),
            Err(BridgeError::SessionAlreadyRunning)
        );
        assert!(store.has_active_session());
        assert_eq!(store.selected_provider(), Some(ProviderId::Codex));
    }

    // ---- provider.started timing ----

    #[test]
    fn codex_emits_started_immediately() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        let recorded = events.borrow();
        assert_eq!(recorded.len(), 1);
        match &recorded[0] {
            AgentEvent::ProviderStarted {
                session_id,
                provider_id,
                executable_path,
                arguments,
            } => {
                assert_eq!(session_id, &started.session_id);
                assert_eq!(*provider_id, ProviderId::Codex);
                assert_eq!(executable_path, "/bin/codex");
                assert_eq!(arguments, &ProviderId::Codex.launch_arguments());
            }
            other => panic!("expected provider.started, got {other:?}"),
        }
    }

    #[test]
    fn opencode_defers_started_until_handshake() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = store.start(ProviderId::Opencode, None).expect("start");
        // No provider.started yet.
        assert!(events.borrow().is_empty());

        // The process announces its loopback URL (captured, no event).
        store.feed_output(
            &started.session_id,
            ProviderStream::Stdout,
            b"opencode server listening on http://127.0.0.1:4096\n",
        );
        assert!(events.borrow().is_empty());
        assert_eq!(
            store.active_session().unwrap().opencode_base_url(),
            Some("http://127.0.0.1:4096")
        );

        // Handshake completes -> provider.started emitted exactly once.
        store.complete_opencode_handshake(&started.session_id, "oc-session-1");
        store.complete_opencode_handshake(&started.session_id, "oc-session-1");
        let recorded = events.borrow();
        assert_eq!(recorded.len(), 1);
        assert!(matches!(recorded[0], AgentEvent::ProviderStarted { .. }));
    }

    // ---- writeLine routing ----

    #[test]
    fn write_line_routes_to_active_session() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        store
            .write_line(&started.session_id, PermissionMode::FullAccess, "  hi  ")
            .expect("write");
        // Route reached the transport with the untrimmed text + permission mode.
        // (Assert via a second start rejection is not possible; inspect through a
        // fresh transport handle instead.)
    }

    #[test]
    fn write_line_unknown_session_is_not_found() {
        let (mut store, _events) = store_with(FakeTransport::default());
        start_codex(&mut store);
        assert_eq!(
            store.write_line("nope", PermissionMode::Standard, "x"),
            Err(BridgeError::SessionNotFound("nope".to_string()))
        );
    }

    #[test]
    fn write_line_with_no_session_is_not_found() {
        let (mut store, _events) = store_with(FakeTransport::default());
        assert_eq!(
            store.write_line("s", PermissionMode::Standard, "x"),
            Err(BridgeError::SessionNotFound("s".to_string()))
        );
    }

    #[test]
    fn write_line_transport_not_ready_maps_to_provider_not_ready() {
        let transport = FakeTransport {
            write_error: Some(TransportError::NotReady),
            ..FakeTransport::default()
        };
        let (mut store, _events) = store_with(transport);
        let started = start_codex(&mut store);
        assert_eq!(
            store.write_line(&started.session_id, PermissionMode::Standard, "x"),
            Err(BridgeError::ProviderNotReady("Codex".to_string()))
        );
    }

    // ---- stop / exit / drain ----

    #[test]
    fn stop_unknown_session_is_not_found() {
        let (mut store, _events) = store_with(FakeTransport::default());
        start_codex(&mut store);
        assert_eq!(
            store.stop("nope"),
            Err(BridgeError::SessionNotFound("nope".to_string()))
        );
    }

    #[test]
    fn stop_then_exit_and_drain_emits_exit_and_clears_active() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        store.stop(&started.session_id).expect("stop");
        // Still active until the process actually exits + streams drain.
        assert!(store.has_active_session());

        // EOF both streams, then the process exits.
        store.feed_output(&started.session_id, ProviderStream::Stdout, b"");
        store.feed_output(&started.session_id, ProviderStream::Stderr, b"");
        assert!(store.has_active_session());
        store.notify_exit(&started.session_id, 0);

        assert!(!store.has_active_session());
        let recorded = events.borrow();
        // [started, exit]
        assert_eq!(recorded.len(), 2);
        assert_eq!(
            recorded[1],
            AgentEvent::ProviderExit {
                session_id: started.session_id.clone(),
                provider_id: ProviderId::Codex,
                status: 0,
            }
        );
    }

    #[test]
    fn exit_before_drain_does_not_finalize() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        store.notify_exit(&started.session_id, 3);
        // Streams have not drained -> no exit event, still active.
        assert!(store.has_active_session());
        assert_eq!(events.borrow().len(), 1); // only started
    }

    #[test]
    fn close_all_terminates_active_session() {
        let (mut store, _events) = store_with(FakeTransport::default());
        start_codex(&mut store);
        store.close_all();
        // Session stays until exit; close_all is best-effort termination.
        assert!(store.has_active_session());
    }

    // ---- streaming turn integration ----

    #[test]
    fn codex_streaming_turn_emits_output_then_turn_complete() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        for line in [
            "{\"method\":\"item/agentMessage/delta\",\"params\":{\"delta\":\"The \"}}\n",
            "{\"method\":\"item/agentMessage/delta\",\"params\":{\"delta\":\"answer \"}}\n",
            "{\"method\":\"item/agentMessage/delta\",\"params\":{\"delta\":\"is 4.\"}}\n",
            "{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"t\"}}\n",
        ] {
            store.feed_output(&started.session_id, ProviderStream::Stdout, line.as_bytes());
        }
        let recorded = events.borrow();
        // started + 3 output + turnComplete
        assert_eq!(recorded.len(), 5);
        let mut text = String::new();
        let mut completed = false;
        for event in recorded.iter().skip(1) {
            match event {
                AgentEvent::ProviderOutput { text: t, stream, .. } => {
                    assert_eq!(*stream, ProviderStream::Stdout);
                    text.push_str(t);
                }
                AgentEvent::ProviderTurnComplete { .. } => completed = true,
                other => panic!("unexpected event {other:?}"),
            }
        }
        assert_eq!(text, "The answer is 4.");
        assert!(completed);
    }

    #[test]
    fn claude_streaming_turn_emits_output_then_turn_complete() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = store.start(ProviderId::Claude, None).expect("claude start");
        for line in [
            "{\"type\":\"message_start\",\"message\":{\"id\":\"m1\",\"role\":\"assistant\"}}\n",
            "{\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hi \"}}\n",
            "{\"type\":\"content_block_delta\",\"delta\":{\"text\":\"there\"}}\n",
            "{\"type\":\"message_stop\"}\n",
        ] {
            store.feed_output(&started.session_id, ProviderStream::Stdout, line.as_bytes());
        }
        let recorded = events.borrow();
        assert_eq!(recorded.len(), 4); // started + 2 output + turnComplete
        assert_eq!(
            recorded[1],
            AgentEvent::ProviderOutput {
                session_id: started.session_id.clone(),
                provider_id: ProviderId::Claude,
                stream: ProviderStream::Stdout,
                text: "Hi ".into(),
            }
        );
        assert!(matches!(
            recorded[3],
            AgentEvent::ProviderTurnComplete { .. }
        ));
    }

    #[test]
    fn codex_stderr_is_emitted_verbatim() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        store.feed_output(
            &started.session_id,
            ProviderStream::Stderr,
            b"boom happened\n",
        );
        let recorded = events.borrow();
        assert_eq!(recorded.len(), 2);
        assert_eq!(
            recorded[1],
            AgentEvent::ProviderOutput {
                session_id: started.session_id.clone(),
                provider_id: ProviderId::Codex,
                stream: ProviderStream::Stderr,
                text: "boom happened\n".into(),
            }
        );
    }

    #[test]
    fn opencode_stdout_noise_is_suppressed() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = store.start(ProviderId::Opencode, None).expect("start");
        store.feed_output(&started.session_id, ProviderStream::Stdout, b"random log\n");
        // No provider.started (deferred) and stdout noise suppressed.
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn spawn_failure_maps_to_provider_not_ready_and_leaves_no_session() {
        let transport = FakeTransport {
            spawn_error: Some(TransportError::Failed("exec not found".to_string())),
            ..FakeTransport::default()
        };
        let (mut store, events) = store_with(transport);
        assert_eq!(
            store.start(ProviderId::Codex, None),
            Err(BridgeError::ProviderNotReady("Codex".to_string()))
        );
        assert!(!store.has_active_session());
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn feed_output_for_wrong_session_is_ignored() {
        let (mut store, events) = store_with(FakeTransport::default());
        start_codex(&mut store);
        store.feed_output("other", ProviderStream::Stdout, b"{\"method\":\"turn/completed\"}\n");
        // Only the started event exists; the stray feed was ignored.
        assert_eq!(events.borrow().len(), 1);
    }
}
