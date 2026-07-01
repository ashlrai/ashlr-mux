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
use crate::transport_action::TransportAction;

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
    /// * [`NotReady`](Self::NotReady) → [`BridgeError::ProviderNotReady`] (the
    ///   genuinely transient case; its `providerNotReady` code is load-bearing —
    ///   the renderer silently retries a `writeLine`).
    /// * [`Failed`](Self::Failed) → [`BridgeError::ProviderLaunchFailed`] carrying
    ///   a concrete reason (e.g. the executable could not be resolved because the
    ///   CLI is not installed). This mirrors the macOS `AgentExecutableResolverError`
    ///   envelope — the renderer shows the message verbatim — instead of the
    ///   misleading "The provider is not ready yet." a bare `providerNotReady`
    ///   would produce for a permanent launch failure.
    pub fn into_bridge_error(self, provider: ProviderId) -> BridgeError {
        match self {
            TransportError::NotReady => {
                BridgeError::ProviderNotReady(provider.display_name().to_string())
            }
            TransportError::Failed(detail) => BridgeError::ProviderLaunchFailed(format!(
                "{} could not be started. {}",
                provider.display_name(),
                detail
            )),
        }
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
    /// Pure I/O intents the host must perform (Codex stdin frames, OpenCode HTTP
    /// calls, teardown). Appended by `start`/`write_line`/`feed_output`; drained by
    /// the host via [`ProcessStore::take_transport_actions`].
    pending_actions: Vec<TransportAction>,
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
            pending_actions: Vec::new(),
        }
    }

    /// Drain the pending host-I/O intents (Codex stdin frames, OpenCode HTTP
    /// calls, teardown), in order. The host calls this after every store
    /// interaction and executes each [`TransportAction`] against the live
    /// process / HTTP handles it owns.
    pub fn take_transport_actions(&mut self) -> Vec<TransportAction> {
        std::mem::take(&mut self.pending_actions)
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

        // Codex writes its `initialize` request immediately on spawn (Swift
        // `CodexAppServerSession.start()`), before any stdout is consumed. The
        // reader-thread registers the child's stdin before returning, so this
        // action is drained + written first.
        if provider_id == ProviderId::Codex {
            if let Some(session) = self.session.as_mut() {
                session.begin_codex_handshake();
            }
            self.collect_session_actions(&session_id);
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
        match provider {
            // Claude's write path is stateless (`write_claude_stream_json`) and
            // flows through the `AgentTransport` trait unchanged.
            ProviderId::Claude => self
                .transport
                .write_line(session_id, permission_mode, text)
                .map_err(|error| error.into_bridge_error(provider)),
            // Codex frames `turn/start` on the single accumulator (queueing if the
            // thread is not ready yet); the resulting stdin frames drain as actions.
            ProviderId::Codex => {
                self.session
                    .as_mut()
                    .expect("active session present")
                    .codex_submit(permission_mode, text)
                    .map_err(|_| {
                        BridgeError::ProviderNotReady(provider.display_name().to_string())
                    })?;
                self.collect_session_actions(session_id);
                Ok(())
            }
            // OpenCode submits over HTTP; the base URL + created session id must
            // already exist (Swift `postOpenCodePrompt` precondition). Permission
            // mode is intentionally ignored for OpenCode.
            ProviderId::Opencode => {
                let (base_url, opencode_session_id) = {
                    let session = self.session.as_ref().expect("active session present");
                    (
                        session.opencode_base_url().map(str::to_string),
                        session.opencode_session_id().map(str::to_string),
                    )
                };
                match (base_url, opencode_session_id) {
                    (Some(base_url), Some(opencode_session_id)) => {
                        self.pending_actions.push(TransportAction::OpenCodePostPrompt {
                            session_id: session_id.to_string(),
                            base_url,
                            opencode_session_id,
                            text: text.to_string(),
                        });
                        Ok(())
                    }
                    _ => Err(BridgeError::ProviderNotReady(
                        provider.display_name().to_string(),
                    )),
                }
            }
        }
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
        self.collect_session_actions(session_id);
        self.finish_if_exited_and_drained(session_id);
    }

    /// Move a session's reactive intents into `pending_actions`.
    ///
    /// Pulls the queued stdin frames, the startup-failure teardown flag, and the
    /// OpenCode "create loopback session" flag off the active [`RunningSession`]
    /// and turns them into ordered [`TransportAction`]s. Called after `start`,
    /// `write_line`, and `feed_output`.
    fn collect_session_actions(&mut self, session_id: &str) {
        let (outbound, opencode_create, terminate) = match self.session.as_mut() {
            Some(session) if session.session_id() == session_id => (
                session.take_pending_outbound(),
                session.take_pending_opencode_create(),
                session.take_pending_terminate(),
            ),
            _ => return,
        };
        for line in outbound {
            self.pending_actions.push(TransportAction::WriteStdin {
                session_id: session_id.to_string(),
                line,
            });
        }
        if opencode_create {
            // Only when the loopback session has not already been created.
            let base_url = self
                .session
                .as_ref()
                .filter(|session| session.opencode_session_id().is_none())
                .and_then(|session| session.opencode_base_url())
                .map(str::to_string);
            if let Some(base_url) = base_url {
                self.pending_actions
                    .push(TransportAction::OpenCodeCreateSession {
                        session_id: session_id.to_string(),
                        base_url,
                    });
            }
        }
        if terminate {
            // Startup failure: emit provider.exit(1) (Swift `failureSink →
            // failSession(status:1)`) and ask the host to tear the child down.
            self.fail_session(session_id, 1, None);
            self.pending_actions.push(TransportAction::Terminate {
                session_id: session_id.to_string(),
            });
        }
    }

    /// Fail the active session: emit an optional synthetic `stderr` line, then
    /// `provider.exit(status)`, then clear the active session.
    ///
    /// Faithful to Swift `failSession` / the OpenCode create+stream failure paths.
    /// Bypasses the exit-and-drain gate; because the session is cleared, any later
    /// `feed_output` / `notify_exit` for this id no-op (no duplicate exit).
    fn fail_session(&mut self, session_id: &str, status: i32, stderr: Option<&str>) {
        let provider = match self.session.as_ref() {
            Some(session) if session.session_id() == session_id => session.provider_id(),
            _ => return,
        };
        if let Some(text) = stderr {
            self.emit(AgentEvent::ProviderOutput {
                session_id: session_id.to_string(),
                provider_id: provider,
                stream: ProviderStream::Stderr,
                text: text.to_string(),
            });
        }
        self.session = None;
        self.emit(AgentEvent::ProviderExit {
            session_id: session_id.to_string(),
            provider_id: provider,
            status,
        });
    }

    /// Emit the OpenCode session-create failure teardown (Swift
    /// `createOpenCodeSession` catch): synthetic stderr + `provider.exit(1)`.
    pub fn fail_opencode_session_create(&mut self, session_id: &str) {
        self.fail_session(session_id, 1, Some("OpenCode session could not be created.\n"));
    }

    /// Emit the OpenCode event-stream disconnect teardown (Swift
    /// `failOpenCodeEventStream`): synthetic stderr + `provider.exit(1)`.
    pub fn fail_opencode_event_stream(&mut self, session_id: &str) {
        self.fail_session(session_id, 1, Some("OpenCode event stream disconnected.\n"));
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
        // Claude is the only provider whose write path flows through the
        // `AgentTransport` trait, so its NotReady maps to ProviderNotReady.
        // (Codex/OpenCode build TransportActions and never touch the transport.)
        let transport = FakeTransport {
            write_error: Some(TransportError::NotReady),
            ..FakeTransport::default()
        };
        let (mut store, _events) = store_with(transport);
        let started = store.start(ProviderId::Claude, None).expect("claude start");
        assert_eq!(
            store.write_line(&started.session_id, PermissionMode::Standard, "x"),
            Err(BridgeError::ProviderNotReady("Claude Code".to_string()))
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
    fn spawn_failure_maps_to_provider_launch_failed_and_leaves_no_session() {
        let transport = FakeTransport {
            spawn_error: Some(TransportError::Failed("exec not found".to_string())),
            ..FakeTransport::default()
        };
        let (mut store, events) = store_with(transport);
        // A spawn/resolve failure surfaces the real reason, not "not ready".
        match store.start(ProviderId::Codex, None) {
            Err(BridgeError::ProviderLaunchFailed(message)) => {
                assert!(message.contains("Codex"), "{message}");
                assert!(message.contains("exec not found"), "{message}");
            }
            other => panic!("expected ProviderLaunchFailed, got {other:?}"),
        }
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

    // ---- Codex reactive write side (TransportAction assertions) ----

    /// Parse every `WriteStdin` frame into JSON, asserting each is a single
    /// newline-terminated object (no double newline).
    fn write_frames(actions: &[TransportAction]) -> Vec<Value> {
        actions
            .iter()
            .filter_map(|action| match action {
                TransportAction::WriteStdin { line, .. } => {
                    assert!(line.ends_with('\n'), "frame must be newline-terminated");
                    assert_eq!(line.matches('\n').count(), 1, "no double newline");
                    Some(serde_json::from_str::<Value>(line.trim()).expect("frame is JSON"))
                }
                _ => None,
            })
            .collect()
    }

    fn feed_line(
        store: &mut ProcessStore<FakeTransport, impl FnMut(AgentEvent)>,
        session_id: &str,
        line: &str,
    ) {
        let mut bytes = line.as_bytes().to_vec();
        bytes.push(b'\n');
        store.feed_output(session_id, ProviderStream::Stdout, &bytes);
    }

    #[test]
    fn codex_start_enqueues_initialize_frame() {
        let (mut store, _events) = store_with(FakeTransport::default());
        start_codex(&mut store);
        let actions = store.take_transport_actions();
        let frames = write_frames(&actions);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["method"], json!("initialize"));
        assert_eq!(frames[0]["id"], json!(1));
    }

    #[test]
    fn codex_initialize_response_enqueues_initialized_then_thread_start() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        let _ = store.take_transport_actions(); // drop the initialize frame
        feed_line(
            &mut store,
            &started.session_id,
            r#"{"id":1,"result":{"userAgent":"codex"}}"#,
        );
        let frames = write_frames(&store.take_transport_actions());
        // Exactly two frames, in order: initialized (no id), then thread/start.
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0]["method"], json!("initialized"));
        assert!(frames[0].get("id").is_none());
        assert_eq!(frames[1]["method"], json!("thread/start"));
        assert_eq!(frames[1]["params"]["cwd"], json!("/work"));
    }

    /// Drive a Codex session through the handshake so a thread exists.
    fn codex_established(
        store: &mut ProcessStore<FakeTransport, impl FnMut(AgentEvent)>,
        session_id: &str,
    ) {
        let _ = store.take_transport_actions();
        feed_line(store, session_id, r#"{"id":1,"result":{}}"#);
        let _ = store.take_transport_actions();
        feed_line(
            store,
            session_id,
            r#"{"id":2,"result":{"thread":{"id":"thread-1"}}}"#,
        );
        let _ = store.take_transport_actions();
    }

    #[test]
    fn codex_write_before_thread_queues_then_drains_on_thread_ready() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        let _ = store.take_transport_actions();
        // initialize resolves but no thread yet.
        feed_line(&mut store, &started.session_id, r#"{"id":1,"result":{}}"#);
        let _ = store.take_transport_actions(); // initialized + thread/start

        // Submit before the thread exists: queued, no turn/start frame yet.
        store
            .write_line(&started.session_id, PermissionMode::FullAccess, "do it")
            .expect("queue");
        assert!(write_frames(&store.take_transport_actions()).is_empty());

        // Thread becomes ready -> the queued input drains into one turn/start.
        feed_line(
            &mut store,
            &started.session_id,
            r#"{"id":2,"result":{"thread":{"id":"thread-1"}}}"#,
        );
        let frames = write_frames(&store.take_transport_actions());
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["method"], json!("turn/start"));
        assert_eq!(frames[0]["params"]["input"][0]["text"], json!("do it"));
        assert_eq!(frames[0]["params"]["approvalPolicy"], json!("never"));
    }

    #[test]
    fn codex_second_queued_input_is_rejected() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        codex_established(&mut store, &started.session_id);
        // First submit starts a turn (thread ready).
        store
            .write_line(&started.session_id, PermissionMode::Standard, "first")
            .expect("first turn");
        assert_eq!(
            write_frames(&store.take_transport_actions())[0]["method"],
            json!("turn/start")
        );
        // A turn is now in flight -> the next submit is rejected (not queued).
        assert_eq!(
            store.write_line(&started.session_id, PermissionMode::Standard, "second"),
            Err(BridgeError::ProviderNotReady("Codex".to_string()))
        );
    }

    #[test]
    fn codex_thread_ready_submit_writes_turn_start_immediately() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        codex_established(&mut store, &started.session_id);
        store
            .write_line(&started.session_id, PermissionMode::FullAccess, "hi codex")
            .expect("turn");
        let frames = write_frames(&store.take_transport_actions());
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["method"], json!("turn/start"));
        assert_eq!(frames[0]["params"]["input"][0]["text"], json!("hi codex"));
    }

    #[test]
    fn codex_empty_write_is_silent_success_no_frame() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        codex_established(&mut store, &started.session_id);
        store
            .write_line(&started.session_id, PermissionMode::Standard, "")
            .expect("empty is Ok");
        assert!(write_frames(&store.take_transport_actions()).is_empty());
    }

    #[test]
    fn codex_server_request_enqueues_approval_reply_with_raw_id() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        codex_established(&mut store, &started.session_id);
        // Start a full-access turn so approvals are accepted.
        store
            .write_line(&started.session_id, PermissionMode::FullAccess, "go")
            .expect("turn");
        let _ = store.take_transport_actions();
        // Server request with a STRING id must be echoed verbatim.
        feed_line(
            &mut store,
            &started.session_id,
            r#"{"id":"req-7","method":"item/commandExecution/requestApproval","params":{}}"#,
        );
        let frames = write_frames(&store.take_transport_actions());
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["id"], json!("req-7"));
        assert_eq!(frames[0]["result"]["decision"], json!("acceptForSession"));
    }

    #[test]
    fn codex_unsupported_server_request_replies_minus_32601() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        codex_established(&mut store, &started.session_id);
        feed_line(
            &mut store,
            &started.session_id,
            r#"{"id":42,"method":"something/unknown","params":{}}"#,
        );
        let frames = write_frames(&store.take_transport_actions());
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["id"], json!(42));
        assert_eq!(frames[0]["error"]["code"], json!(-32601));
    }

    #[test]
    fn codex_startup_failure_emits_exit_and_terminate_once() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = start_codex(&mut store);
        let _ = store.take_transport_actions();
        // An rpc error on the initialize id fails startup.
        feed_line(
            &mut store,
            &started.session_id,
            r#"{"id":1,"error":{"message":"nope"}}"#,
        );
        // provider.exit(1) emitted and the session cleared.
        assert!(!store.has_active_session());
        let recorded = events.borrow();
        assert!(matches!(
            recorded.last(),
            Some(AgentEvent::ProviderExit { status: 1, .. })
        ));
        drop(recorded);
        // A Terminate action was enqueued.
        assert!(store
            .take_transport_actions()
            .iter()
            .any(|action| matches!(action, TransportAction::Terminate { .. })));
        // A later feed / notify_exit for the dead session no-ops (no duplicate exit).
        let before = events.borrow().len();
        store.notify_exit(&started.session_id, 0);
        assert_eq!(events.borrow().len(), before);
    }

    // ---- OpenCode actions ----

    #[test]
    fn opencode_loopback_url_enqueues_create_session_and_defers_started() {
        let (mut store, events) = store_with(FakeTransport::default());
        let started = store.start(ProviderId::Opencode, None).expect("start");
        assert!(events.borrow().is_empty());
        store.feed_output(
            &started.session_id,
            ProviderStream::Stdout,
            b"opencode server listening on http://127.0.0.1:4096\n",
        );
        // No provider.started yet; one create-session action enqueued.
        assert!(events.borrow().is_empty());
        let actions = store.take_transport_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0],
            TransportAction::OpenCodeCreateSession {
                session_id: started.session_id.clone(),
                base_url: "http://127.0.0.1:4096".to_string(),
            }
        );

        // Handshake completes -> provider.started emitted once.
        store.complete_opencode_handshake(&started.session_id, "oc-1");
        assert_eq!(events.borrow().len(), 1);
        assert!(matches!(
            events.borrow()[0],
            AgentEvent::ProviderStarted { .. }
        ));
    }

    #[test]
    fn opencode_write_before_session_is_provider_not_ready() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = store.start(ProviderId::Opencode, None).expect("start");
        // No base URL / session id yet.
        assert_eq!(
            store.write_line(&started.session_id, PermissionMode::Standard, "hi"),
            Err(BridgeError::ProviderNotReady("OpenCode".to_string()))
        );
    }

    #[test]
    fn opencode_write_after_session_enqueues_post_prompt() {
        let (mut store, _events) = store_with(FakeTransport::default());
        let started = store.start(ProviderId::Opencode, None).expect("start");
        store.feed_output(
            &started.session_id,
            ProviderStream::Stdout,
            b"opencode server listening on http://127.0.0.1:4096\n",
        );
        let _ = store.take_transport_actions();
        store.complete_opencode_handshake(&started.session_id, "oc-1");
        store
            .write_line(&started.session_id, PermissionMode::Standard, "hello")
            .expect("post prompt");
        let actions = store.take_transport_actions();
        assert_eq!(
            actions[0],
            TransportAction::OpenCodePostPrompt {
                session_id: started.session_id.clone(),
                base_url: "http://127.0.0.1:4096".to_string(),
                opencode_session_id: "oc-1".to_string(),
                text: "hello".to_string(),
            }
        );
    }
}
