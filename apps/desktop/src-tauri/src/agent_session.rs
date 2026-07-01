//! Agent-session host bridge (Phase 3 GUI-wiring slice).
//!
//! Drives the reused `webviews/agent-session` React app: it is the concrete,
//! Windows-side implementation behind the single request seam
//! (`agent_session_rpc`, the Tauri command the `host.ts` shim maps the
//! `agentSession` webkit channel onto) and the single push seam
//! (`cmux://agent-event`, forwarded to `window.cmuxAgentBridge.receive`).
//!
//! ## Architecture — a single-owner actor, no tokio
//!
//! [`cmux_agent_chat::ProcessStore`] is single-threaded by construction (its
//! event sink is `FnMut(AgentEvent)`, not `Send`), and `cmux-process` is pure
//! blocking `std` (a spawned child's stdout/stderr arrive on a blocking
//! `mpsc::Receiver`, and there is no wait/exit-code API — channel disconnect is
//! the only exit signal). So this bridge owns the `ProcessStore` in ONE actor
//! thread and funnels three message kinds to it over an `mpsc` channel:
//!
//! * [`ActorMsg::Rpc`] — a renderer request (`agent_session_rpc`), replied to
//!   over a per-request oneshot channel.
//! * [`ActorMsg::Feed`] — a stdout/stderr chunk (empty = EOF) from a per-session
//!   reader thread.
//! * [`ActorMsg::Exit`] — the child exited (reader thread saw the pipe close).
//!
//! Draining them serially on one thread gives the exact event ordering the macOS
//! serial `MainActor` guarantees, which the renderer's start/exit state machine
//! depends on. This mirrors `terminal.rs` (a `std::thread` pump + `app.emit`),
//! adding no async runtime.
//!
//! The concrete [`AgentTransport`] resolves the executable through `cmux-agent`
//! (`AgentExecutableResolver` → `to_spawn_spec`), spawns it Job-Object-supervised
//! via `cmux-process`, and pumps its framed output back into the store. This
//! slice wires **Claude** end-to-end (its stream-json write path needs no
//! handshake); Codex (app-server JSON-RPC handshake) and OpenCode (HTTP-loopback
//! plus SSE) reuse this same plumbing in follow-on slices and error clearly
//! until then.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::opencode_http::{self, OpenCodeContext, OpenCodeContexts, StreamEnd};
use cmux_agent::{
    AgentExecutableResolver, AgentSessionProviderId, ClaudeConfigContext, OpenCodeServerAuth,
};
use cmux_agent_chat::process_store::{SpawnRequest, SpawnedSession, TransportError};
use cmux_process::SpawnSpec;
use cmux_agent_chat::{
    handle, write_claude_stream_json, AgentEvent, AgentTransport, BridgeRequest, DispatchContext,
    PermissionMode, ProcessStore, ProviderId, ProviderStream, TransportAction,
};
use cmux_process::{
    AgentStream, JobObjectSupervisor, ProcessSupervisor, SessionId, TerminateMode,
};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

/// The push-seam event name the `host.ts` shim forwards to
/// `window.cmuxAgentBridge.receive`.
const AGENT_EVENT: &str = "cmux://agent-event";

// ---------------------------------------------------------------------------
// Actor plumbing
// ---------------------------------------------------------------------------

/// A message to the single actor thread that owns the [`ProcessStore`].
enum ActorMsg {
    /// A renderer request `{id, method, params}`; the reply envelope is sent back
    /// over `reply`.
    Rpc {
        message: Value,
        reply: Sender<Value>,
    },
    /// A raw stdout/stderr chunk for a session (an empty `data` = stream EOF).
    Feed {
        session_id: String,
        stream: ProviderStream,
        data: Vec<u8>,
    },
    /// The child of `session_id` exited (reader thread saw the pipe close).
    Exit { session_id: String, status: i32 },
    /// OpenCode `POST /session` succeeded with the created loopback session id
    /// (the create worker thread reports back).
    OpenCodeSessionCreated {
        session_id: String,
        opencode_session_id: String,
    },
    /// OpenCode `POST /session` failed (missing id, non-2xx, or network error).
    OpenCodeSessionCreateFailed { session_id: String },
    /// One raw line from the OpenCode `/event` SSE stream.
    OpenCodeSse { session_id: String, line: String },
    /// The OpenCode `/event` stream ended (`errored` = a failure, not a clean EOF).
    OpenCodeStreamEnded { session_id: String, errored: bool },
}

/// Managed Tauri state: a lazily-started actor thread, addressed by its sender.
///
/// The actor can only be created once we have an [`AppHandle`] (its event sink
/// emits `cmux://agent-event`), which first arrives on a command call — so it is
/// started on demand and memoized.
#[derive(Default)]
pub struct AgentSessionState {
    sender: Mutex<Option<Sender<ActorMsg>>>,
}

impl AgentSessionState {
    /// Return the actor's sender, starting the actor thread on first use.
    fn ensure(&self, app: &AppHandle) -> Sender<ActorMsg> {
        let mut guard = self.sender.lock().expect("agent session sender poisoned");
        if let Some(sender) = guard.as_ref() {
            return sender.clone();
        }
        let (tx, rx) = channel::<ActorMsg>();
        // The supervisor + live-child map are shared between the transport (which
        // spawns + registers children and writes the Claude path) and the actor
        // (which executes TransportActions: raw Codex stdin frames + teardown).
        let supervisor = Arc::new(JobObjectSupervisor::new());
        let sessions: LiveChildren = Arc::new(Mutex::new(HashMap::new()));
        let opencode: OpenCodeContexts = Arc::new(Mutex::new(HashMap::new()));
        let transport = ClaudeAgentTransport::new(
            supervisor.clone(),
            tx.clone(),
            sessions.clone(),
            opencode.clone(),
        );
        let host = ActorHost {
            supervisor,
            sessions,
            opencode,
            feedback: tx.clone(),
        };
        let app = app.clone();
        std::thread::Builder::new()
            .name("cmux-agent-session-actor".to_string())
            .spawn(move || run_actor(rx, transport, app, host))
            .expect("spawn agent session actor thread");
        *guard = Some(tx.clone());
        tx
    }
}

/// The actor loop: owns the [`ProcessStore`] and drains [`ActorMsg`]s serially.
///
/// After every message it drains the store's [`TransportAction`]s and performs
/// them against the shared `sessions` / `supervisor` handles: raw Codex stdin
/// frames ([`TransportAction::WriteStdin`]) and startup-failure teardown
/// ([`TransportAction::Terminate`]). OpenCode HTTP actions are wired in slice 2.
fn run_actor(rx: Receiver<ActorMsg>, transport: ClaudeAgentTransport, app: AppHandle, host: ActorHost) {
    // The event sink: serialize each AgentEvent to its tagged wire object and
    // push it to the renderer over cmux://agent-event. Emitted from this one
    // thread, preserving order.
    let sink = move |event: AgentEvent| {
        let payload = serde_json::to_value(&event).expect("AgentEvent serializes");
        let _ = app.emit(AGENT_EVENT, payload);
    };
    let mut store = ProcessStore::new(transport, sink, env!("CARGO_PKG_VERSION"));

    // The fallback working directory for provider.start / app.context.
    let ctx = DispatchContext {
        working_directory: std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string()),
    };

    while let Ok(msg) = rx.recv() {
        match msg {
            ActorMsg::Rpc { message, reply } => {
                let envelope = dispatch_message(&mut store, message, &ctx);
                let _ = reply.send(envelope);
            }
            ActorMsg::Feed {
                session_id,
                stream,
                data,
            } => {
                store.feed_output(&session_id, stream, &data);
            }
            ActorMsg::Exit { session_id, status } => {
                store.notify_exit(&session_id, status);
                // Natural exit (the reader saw the child's pipes close): no
                // teardown action runs on this route, so drop any OpenCode HTTP
                // context here to avoid leaking it across sessions. Idempotent with
                // the stop/terminate paths. A later clean `/event` EOF then finds
                // no context and correctly declines to fail the (already-exited)
                // session.
                forget_opencode_context(&host, &session_id);
            }
            ActorMsg::OpenCodeSessionCreated {
                session_id,
                opencode_session_id,
            } => {
                // Records the loopback session id + emits the deferred
                // provider.started, then begins the /event SSE stream.
                store.complete_opencode_handshake(&session_id, opencode_session_id);
                let base_url = store
                    .active_session()
                    .filter(|session| session.session_id() == session_id)
                    .and_then(|session| session.opencode_base_url())
                    .map(str::to_string);
                if let Some(base_url) = base_url {
                    spawn_event_stream(&host, session_id, base_url);
                }
            }
            ActorMsg::OpenCodeSessionCreateFailed { session_id } => {
                // Swift create-catch: synthetic stderr + provider.exit(1) + kill.
                store.fail_opencode_session_create(&session_id);
                terminate_session(&host, &session_id);
            }
            ActorMsg::OpenCodeSse { session_id, line } => {
                store.feed_opencode_sse_line(&session_id, &line);
            }
            ActorMsg::OpenCodeStreamEnded {
                session_id,
                errored,
            } => {
                // Swift `openCodeEventStreamEOFRequiresFailure`: an error always
                // fails; a clean EOF fails only while the child is still running
                // and the session was not cancelled. `fail_opencode_event_stream`
                // itself no-ops if the session already exited (no duplicate exit).
                if errored || should_fail_on_clean_eof(&host, &session_id) {
                    store.fail_opencode_event_stream(&session_id);
                    terminate_session(&host, &session_id);
                }
            }
        }
        // Perform any I/O the store queued while handling this message.
        execute_actions(store.take_transport_actions(), &host);
    }
}

/// Perform the store's queued [`TransportAction`]s against the live children.
///
/// `WriteStdin` writes the already-`encode_line`-framed bytes RAW to the child's
/// stdin (no re-newline). `Terminate` tree-kills + reaps the session. The OpenCode
/// HTTP variants are handled in the OpenCode slice.
fn execute_actions(actions: Vec<TransportAction>, host: &ActorHost) {
    for action in actions {
        match action {
            TransportAction::WriteStdin { session_id, line } => {
                write_child_stdin(&host.sessions, &session_id, line.as_bytes());
            }
            TransportAction::Terminate { session_id } => {
                terminate_session(host, &session_id);
            }
            // OpenCode call A: create the loopback session on a worker thread.
            TransportAction::OpenCodeCreateSession {
                session_id,
                base_url,
            } => spawn_create_session(host, session_id, base_url),
            // OpenCode call C: submit the prompt fire-and-forget.
            TransportAction::OpenCodePostPrompt {
                session_id,
                base_url,
                opencode_session_id,
                text,
            } => spawn_post_prompt(host, session_id, base_url, opencode_session_id, text),
        }
    }
}

/// Write raw bytes to a live session's child stdin (best-effort).
fn write_child_stdin(sessions: &LiveChildren, session_id: &str, bytes: &[u8]) {
    let mut guard = sessions.lock().expect("agent sessions mutex poisoned");
    if let Some(child) = guard.get_mut(session_id) {
        let _ = child
            .stdin
            .write_all(bytes)
            .and_then(|()| child.stdin.flush());
    }
}

/// Tear down a session: cancel its OpenCode event stream (so a resulting EOF is
/// not counted a failure), tree-kill + reap the child, and drop its OpenCode
/// context. Idempotent.
fn terminate_session(host: &ActorHost, session_id: &str) {
    if let Some(context) = host
        .opencode
        .lock()
        .expect("opencode contexts mutex poisoned")
        .get(session_id)
    {
        context.cancelled.store(true, Ordering::SeqCst);
    }
    let handle_id = host
        .sessions
        .lock()
        .expect("agent sessions mutex poisoned")
        .get(session_id)
        .map(|child| child.handle_id);
    if let Some(handle_id) = handle_id {
        let _ = host
            .supervisor
            .terminate(handle_id, TerminateMode::Graceful);
    }
    reap_session(&host.supervisor, &host.sessions, session_id);
    host.opencode
        .lock()
        .expect("opencode contexts mutex poisoned")
        .remove(session_id);
}

/// Drop a session's OpenCode HTTP context (auth header + liveness flags), if any.
/// Idempotent — a no-op for non-OpenCode sessions or an already-removed entry.
fn forget_opencode_context(host: &ActorHost, session_id: &str) {
    host.opencode
        .lock()
        .expect("opencode contexts mutex poisoned")
        .remove(session_id);
}

/// Whether a clean OpenCode `/event` EOF should fail the session (Swift
/// `openCodeEventStreamEOFRequiresFailure = !isCancelled && processIsRunning`).
///
/// `process_running` is cleared by the reader thread the instant the child pipes
/// close, and the natural-exit path removes the context entirely (via the `Exit`
/// handler), so the common orderings resolve correctly. A narrow race remains: if
/// this fires before the reader observes the pipe close, a clean shutdown can be
/// mislabeled (a synthetic "disconnected" stderr + exit 1 instead of exit 0). This
/// is inherited from the canonical Swift (`Process.isRunning` is likewise updated
/// asynchronously) and is a cosmetic status mislabel, not a lifecycle bug — the
/// cleared session prevents any double `provider.exit`.
fn should_fail_on_clean_eof(host: &ActorHost, session_id: &str) -> bool {
    host.opencode
        .lock()
        .expect("opencode contexts mutex poisoned")
        .get(session_id)
        .map(|context| {
            !context.cancelled.load(Ordering::SeqCst)
                && context.process_running.load(Ordering::SeqCst)
        })
        .unwrap_or(false)
}

/// Read the OpenCode auth header + working directory for a session (cloned out so
/// the caller does not hold the contexts lock).
fn opencode_auth_and_dir(
    opencode: &OpenCodeContexts,
    session_id: &str,
) -> (Option<String>, Option<String>) {
    opencode
        .lock()
        .expect("opencode contexts mutex poisoned")
        .get(session_id)
        .map(|context| {
            (
                context.auth_header.clone(),
                context.working_directory.clone(),
            )
        })
        .unwrap_or((None, None))
}

/// OpenCode call A worker: `POST /session`, reporting the created id (or failure)
/// back to the actor.
fn spawn_create_session(host: &ActorHost, session_id: String, base_url: String) {
    let feedback = host.feedback.clone();
    let (auth, dir) = opencode_auth_and_dir(&host.opencode, &session_id);
    std::thread::spawn(move || {
        let message =
            match opencode_http::create_session(&base_url, auth.as_deref(), dir.as_deref()) {
                Ok(opencode_session_id) => ActorMsg::OpenCodeSessionCreated {
                    session_id,
                    opencode_session_id,
                },
                Err(_) => ActorMsg::OpenCodeSessionCreateFailed { session_id },
            };
        let _ = feedback.send(message);
    });
}

/// OpenCode call C worker: `POST …/prompt_async`, fire-and-forget.
///
/// The `provider.writeLine` RPC already returned optimistically (the actor cannot
/// block up to 30s on the POST without stalling all event ordering — a documented
/// divergence from Swift's awaited `postOpenCodePrompt`), so a failure here is
/// surfaced as an OpenCode `stderr` line (emitted verbatim by the store) rather
/// than a synchronous RPC error.
fn spawn_post_prompt(
    host: &ActorHost,
    session_id: String,
    base_url: String,
    opencode_session_id: String,
    text: String,
) {
    let feedback = host.feedback.clone();
    let (auth, dir) = opencode_auth_and_dir(&host.opencode, &session_id);
    std::thread::spawn(move || {
        if let Err(error) = opencode_http::post_prompt(
            &base_url,
            auth.as_deref(),
            dir.as_deref(),
            &opencode_session_id,
            &text,
        ) {
            let line = format!("OpenCode prompt failed: {error}\n");
            let _ = feedback.send(ActorMsg::Feed {
                session_id,
                stream: ProviderStream::Stderr,
                data: line.into_bytes(),
            });
        }
    });
}

/// OpenCode call B worker: open the `/event` SSE stream, pumping each line to the
/// actor and reporting how it ended.
fn spawn_event_stream(host: &ActorHost, session_id: String, base_url: String) {
    let feedback = host.feedback.clone();
    let (auth, dir) = opencode_auth_and_dir(&host.opencode, &session_id);
    let cancelled = host
        .opencode
        .lock()
        .expect("opencode contexts mutex poisoned")
        .get(&session_id)
        .map(|context| context.cancelled.clone());
    let Some(cancelled) = cancelled else {
        return;
    };
    std::thread::spawn(move || {
        let is_cancelled = || cancelled.load(Ordering::SeqCst);
        let end = {
            let feedback = &feedback;
            let session_id = &session_id;
            let mut on_line = |line: String| {
                let _ = feedback.send(ActorMsg::OpenCodeSse {
                    session_id: session_id.clone(),
                    line,
                });
            };
            opencode_http::stream_events(
                &base_url,
                auth.as_deref(),
                dir.as_deref(),
                &is_cancelled,
                &mut on_line,
            )
        };
        let _ = feedback.send(ActorMsg::OpenCodeStreamEnded {
            session_id,
            errored: end == StreamEnd::Errored,
        });
    });
}

/// Dispatch one renderer message to a reply envelope.
///
/// `app.context` / `app.pickFiles` are host concerns (the store returns
/// `UnsupportedMethod` for them), so they are intercepted here; every
/// `provider.*` method delegates to [`cmux_agent_chat::handle`].
fn dispatch_message(
    store: &mut ProcessStore<ClaudeAgentTransport, impl FnMut(AgentEvent)>,
    message: Value,
    ctx: &DispatchContext,
) -> Value {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    match method.as_str() {
        "app.context" => ok_envelope(app_context_value(ctx)),
        // Native file picking is a later slice (needs a Tauri dialog plugin);
        // report "no files selected" so the composer's attach action no-ops
        // cleanly rather than erroring.
        "app.pickFiles" => ok_envelope(json!({ "files": [] })),
        _ => match BridgeRequest::from_value(message) {
            Ok(request) => match handle(&request, store, ctx) {
                Ok(value) => ok_envelope(value),
                Err(error) => err_envelope(error.code(), &error.user_message()),
            },
            Err(error) => err_envelope(error.code(), &error.user_message()),
        },
    }
}

/// `{ ok: true, value }` — the `NativeReply` success envelope the reused
/// `bridge.ts` unwraps.
fn ok_envelope(value: Value) -> Value {
    json!({ "ok": true, "value": value })
}

/// `{ ok: false, error: { code, userMessage } }` — the failure envelope. `code`
/// is load-bearing (`providerNotReady` makes the renderer silently retry
/// `writeLine`).
fn err_envelope(code: &str, user_message: &str) -> Value {
    json!({
        "ok": false,
        "error": { "code": code, "userMessage": user_message },
    })
}

// ---------------------------------------------------------------------------
// The Tauri command (single request seam)
// ---------------------------------------------------------------------------

/// The single request seam: the `agentSession` webkit channel → this command.
///
/// Returns the RAW `NativeReply` envelope (the shim routes it via `invokeRaw`,
/// and `bridge.ts` unwraps `{ok,value|error}` itself). The `message` argument is
/// the `{id, method, params}` object (single-word arg key, so no camelCase
/// mapping surprises).
#[tauri::command]
pub fn agent_session_rpc(
    app: AppHandle,
    state: State<'_, AgentSessionState>,
    message: Value,
) -> Value {
    let sender = state.ensure(&app);
    let (reply_tx, reply_rx) = channel::<Value>();
    if sender
        .send(ActorMsg::Rpc {
            message,
            reply: reply_tx,
        })
        .is_err()
    {
        return err_envelope("actorUnavailable", "Agent session host is not running.");
    }
    reply_rx
        .recv()
        .unwrap_or_else(|_| err_envelope("actorUnavailable", "Agent session host stopped."))
}

// ---------------------------------------------------------------------------
// Concrete transport (Claude wired; Codex/OpenCode reserved)
// ---------------------------------------------------------------------------

/// A live child: the supervisor session id (for termination), its provider (for
/// framing writes), and its stdin writer.
struct LiveChild {
    handle_id: SessionId,
    provider: ProviderId,
    stdin: Box<dyn Write + Send>,
}

/// The set of live children, keyed by session id. Shared (`Arc<Mutex>`) between
/// the actor thread (spawn/write/stop) and each session's reader thread, which
/// reaps its own entry when the child exits on its own.
type LiveChildren = Arc<Mutex<HashMap<String, LiveChild>>>;

/// The concrete [`AgentTransport`]: resolves + spawns the provider via
/// `cmux-agent` + `cmux-process`, and pumps framed output back to the actor.
struct ClaudeAgentTransport {
    supervisor: Arc<JobObjectSupervisor>,
    /// A clone of the actor's inbox, handed to each session's reader thread so it
    /// can push `Feed`/`Exit` messages back.
    feedback: Sender<ActorMsg>,
    sessions: LiveChildren,
    /// Per-session OpenCode HTTP context (auth header + working dir + liveness
    /// flags), created on spawn for OpenCode sessions and read by the HTTP workers.
    opencode: OpenCodeContexts,
}

impl ClaudeAgentTransport {
    fn new(
        supervisor: Arc<JobObjectSupervisor>,
        feedback: Sender<ActorMsg>,
        sessions: LiveChildren,
        opencode: OpenCodeContexts,
    ) -> Self {
        Self {
            supervisor,
            feedback,
            sessions,
            opencode,
        }
    }
}

/// The handles the actor uses to execute [`TransportAction`]s: the child-process
/// supervisor + live-child map (shared with the transport) and the OpenCode HTTP
/// context registry + a feedback sender the HTTP worker threads report back on.
struct ActorHost {
    supervisor: Arc<JobObjectSupervisor>,
    sessions: LiveChildren,
    opencode: OpenCodeContexts,
    feedback: Sender<ActorMsg>,
}

/// Reap a finished session: drop its `LiveChild` (closing the child's stdin) and
/// release its supervisor handles. `reap` closes the job handle, which fires
/// `KILL_ON_JOB_CLOSE` — so any straggler tree is torn down even on the reader's
/// natural-exit path. Idempotent — safe to call from the reader thread on natural
/// exit AND from the stop path, whichever runs first.
fn reap_session(supervisor: &JobObjectSupervisor, sessions: &LiveChildren, session_id: &str) {
    let child = sessions
        .lock()
        .expect("agent sessions mutex poisoned")
        .remove(session_id);
    if let Some(child) = child {
        supervisor.reap(child.handle_id);
    }
}

impl AgentTransport for ClaudeAgentTransport {
    fn spawn(&mut self, request: &SpawnRequest) -> Result<SpawnedSession, TransportError> {
        let provider = request.provider_id;
        let agent_provider = to_agent_provider(provider);

        // Resolve the executable + curated env + rewritten PATH, then a SpawnSpec.
        let resolver = AgentExecutableResolver::default();
        let plan = resolver
            .resolve(agent_provider)
            .map_err(|error| TransportError::Failed(error.to_string()))?;
        let claude_ctx = ClaudeConfigContext::from_process_env(&plan.environment);
        let spec = plan.to_spawn_spec(request.working_directory.as_deref(), None, &claude_ctx);

        // Capture the resolved AGENT executable for the provider.started event
        // (the renderer shows the agent, e.g. `claude.cmd`, not the interpreter)
        // BEFORE the spec is rewritten to launch through cmd.exe/powershell.
        let executable_path = spec.program.to_string_lossy().to_string();
        let arguments = spec.args.clone();

        // OpenCode: derive the loopback Basic-auth header from the launch env
        // (`OPENCODE_SERVER_*`, minted by `cmux-agent`) BEFORE `spec` is moved
        // into `spawn_captured`. The secret stays host-side (out of the store).
        let opencode_auth = if provider == ProviderId::Opencode {
            OpenCodeServerAuth::from_environment(&spec.env)
                .map(|auth| auth.authorization_header)
        } else {
            None
        };

        // Windows npm/pnpm agents resolve to `.cmd`/`.bat`/`.ps1` shims (here
        // `claude.cmd`), which `CreateProcessW` cannot execute directly — only a
        // real PE image. Wrap the shim with its interpreter so the real spawn
        // succeeds.
        let spec = wrap_windows_shim(spec);

        let (handle, io) = self
            .supervisor
            .spawn_captured(spec)
            .map_err(|error| TransportError::Failed(error.to_string()))?;
        let (stdin, chunks) = io.into_parts();

        // Register the live child (holds stdin) before starting its reader, so a
        // reader-spawn failure has a tracked entry to roll back.
        self.sessions.lock().expect("agent sessions mutex poisoned").insert(
            request.session_id.clone(),
            LiveChild {
                handle_id: handle.id,
                provider,
                stdin,
            },
        );

        // OpenCode: register the HTTP context (auth header + working dir + the
        // liveness flags the event-stream failure rule needs). `process_running`
        // is shared with the reader thread, which clears it the instant the child
        // pipes close (so an event-stream EOF then is a real disconnect).
        let process_running: Option<Arc<AtomicBool>> = if provider == ProviderId::Opencode {
            let flag = Arc::new(AtomicBool::new(true));
            self.opencode
                .lock()
                .expect("opencode contexts mutex poisoned")
                .insert(
                    request.session_id.clone(),
                    OpenCodeContext {
                        auth_header: opencode_auth,
                        working_directory: request.working_directory.clone(),
                        cancelled: Arc::new(AtomicBool::new(false)),
                        process_running: flag.clone(),
                    },
                );
            Some(flag)
        } else {
            None
        };

        // A dedicated reader thread forwards framed output (and EOF/exit) to the
        // actor, then reaps its own session on the natural-exit path (cmux-process
        // has no exit-code API — channel disconnect is the only exit signal — so
        // we report status 0). The store clears its session on notify_exit, but
        // the transport's entry + supervisor handles are released here.
        let feedback = self.feedback.clone();
        let supervisor = Arc::clone(&self.supervisor);
        let sessions = Arc::clone(&self.sessions);
        let session_id = request.session_id.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("cmux-agent-reader-{session_id}"))
            .spawn(move || {
                reader_loop(chunks, session_id, feedback, supervisor, sessions, process_running)
            });
        if let Err(error) = spawned {
            // Roll back the just-confined child: no reader means no drain/exit
            // signal would ever fire, so tear it down now. Also drop the OpenCode
            // context registered above (the child/handle rollback via reap_session
            // does not touch it), mirroring the terminate paths.
            reap_session(&self.supervisor, &self.sessions, &request.session_id);
            self.opencode
                .lock()
                .expect("opencode contexts mutex poisoned")
                .remove(&request.session_id);
            return Err(TransportError::Failed(error.to_string()));
        }

        Ok(SpawnedSession {
            executable_path,
            arguments,
        })
    }

    fn write_line(
        &mut self,
        session_id: &str,
        _permission_mode: PermissionMode,
        text: &str,
    ) -> Result<(), TransportError> {
        let mut sessions = self.sessions.lock().expect("agent sessions mutex poisoned");
        let child = sessions.get_mut(session_id).ok_or(TransportError::NotReady)?;
        let framed = match child.provider {
            // Claude stream-json: one `{"type":"user",...}` line (already
            // newline-terminated) per prompt. No handshake, no permission mode.
            ProviderId::Claude => write_claude_stream_json(text),
            // Codex needs the app-server turn/start framing + handshake; OpenCode
            // writes over HTTP. Both are follow-on slices.
            ProviderId::Codex | ProviderId::Opencode => {
                return Err(TransportError::Failed(
                    "provider write path not yet ported".to_string(),
                ))
            }
        };
        child
            .stdin
            .write_all(framed.as_bytes())
            .map_err(|error| TransportError::Failed(error.to_string()))?;
        child
            .stdin
            .flush()
            .map_err(|error| TransportError::Failed(error.to_string()))?;
        Ok(())
    }

    fn terminate(&mut self, session_id: &str) -> Result<(), TransportError> {
        // Graceful stop first (Ctrl-Break + 500ms grace + whole-tree kill), then
        // release the child + supervisor handles. The reader thread will still
        // observe EOF and drive the store's drain/exit; its own reap is then an
        // idempotent no-op.
        // Cancel any OpenCode event stream first, so its disconnect EOF is not
        // treated as a failure, then drop the context.
        if let Some(context) = self
            .opencode
            .lock()
            .expect("opencode contexts mutex poisoned")
            .get(session_id)
        {
            context.cancelled.store(true, Ordering::SeqCst);
        }
        let handle_id = self
            .sessions
            .lock()
            .expect("agent sessions mutex poisoned")
            .get(session_id)
            .map(|child| child.handle_id);
        if let Some(handle_id) = handle_id {
            let _ = self.supervisor.terminate(handle_id, TerminateMode::Graceful);
        }
        reap_session(&self.supervisor, &self.sessions, session_id);
        self.opencode
            .lock()
            .expect("opencode contexts mutex poisoned")
            .remove(session_id);
        Ok(())
    }
}

/// Drain a session's framed output, forwarding each line to the actor; on pipe
/// close (both streams EOF), signal EOF for both streams + exit, then reap the
/// session (release its `LiveChild` + supervisor handles). The natural-exit
/// counterpart to the stop path's `terminate` — without this, a session that
/// finishes on its own would orphan its stdin pipe + supervisor handles.
fn reader_loop(
    chunks: Receiver<cmux_process::AgentOutputChunk>,
    session_id: String,
    feedback: Sender<ActorMsg>,
    supervisor: Arc<JobObjectSupervisor>,
    sessions: LiveChildren,
    process_running: Option<Arc<AtomicBool>>,
) {
    loop {
        match chunks.recv() {
            Ok(chunk) => {
                let stream = match chunk.stream {
                    AgentStream::Stdout => ProviderStream::Stdout,
                    AgentStream::Stderr => ProviderStream::Stderr,
                };
                // cmux-process frames one decoded line per chunk (no trailing
                // newline); the store's line buffer re-splits on `\n`, so add it
                // back. Framing errors (invalid utf-8 / oversize) are skipped.
                let line = match chunk.frame {
                    Ok(line) => line,
                    Err(_) => continue,
                };
                let mut data = line.into_bytes();
                data.push(b'\n');
                if feedback
                    .send(ActorMsg::Feed {
                        session_id: session_id.clone(),
                        stream,
                        data,
                    })
                    .is_err()
                {
                    break;
                }
            }
            Err(_) => {
                // Both pipes reached EOF: the child has exited. Clear the OpenCode
                // liveness flag FIRST (before any exit message races an event-stream
                // EOF) so a concurrent stream EOF is treated as a benign shutdown.
                if let Some(flag) = &process_running {
                    flag.store(false, Ordering::SeqCst);
                }
                // Flush both streams + report exit so the store emits provider.exit
                // and clears its session.
                let _ = feedback.send(ActorMsg::Feed {
                    session_id: session_id.clone(),
                    stream: ProviderStream::Stdout,
                    data: Vec::new(),
                });
                let _ = feedback.send(ActorMsg::Feed {
                    session_id: session_id.clone(),
                    stream: ProviderStream::Stderr,
                    data: Vec::new(),
                });
                let _ = feedback.send(ActorMsg::Exit {
                    session_id: session_id.clone(),
                    status: 0,
                });
                break;
            }
        }
    }
    // Release the transport's entry + supervisor handles for this session (the
    // store cleared its own on notify_exit; this is the transport-side cleanup).
    reap_session(&supervisor, &sessions, &session_id);
}

/// Map the chat-crate provider id onto the `cmux-agent` resolver's provider id.
/// (The two enums are hand-kept in lockstep but have no shared type; note the
/// spelling difference `Opencode` vs `OpenCode`.)
fn to_agent_provider(provider: ProviderId) -> AgentSessionProviderId {
    match provider {
        ProviderId::Codex => AgentSessionProviderId::Codex,
        ProviderId::Claude => AgentSessionProviderId::Claude,
        ProviderId::Opencode => AgentSessionProviderId::OpenCode,
    }
}

/// Look up an environment value case-insensitively (Windows env keys vary in
/// casing — `ComSpec`, `Path`, …).
fn lookup_env_ci<'a>(
    env: &'a std::collections::BTreeMap<String, String>,
    key: &str,
) -> Option<&'a str> {
    env.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v.as_str())
}

/// Rewrite a spawn spec that targets a Windows shim (`.cmd`/`.bat`/`.ps1`) to
/// run through its interpreter, since `CreateProcessW` only executes real PE
/// images. A `.exe` (or extensionless) program is returned unchanged.
///
/// * `.cmd` / `.bat` → `%ComSpec% /C <shim> <args…>`
/// * `.ps1` → `powershell.exe -NoProfile -ExecutionPolicy Bypass -File <shim> <args…>`
///
/// The interpreter is given as an ABSOLUTE path: `cmux-process` passes
/// `spec.program` to `CreateProcessW` as `lpApplicationName`, which is NOT
/// PATH-searched (a bare `cmd.exe`/`powershell.exe` would be looked up only in
/// the child's cwd and fail). The shim path is a single command-line token
/// (quoted by `cmux-process`'s argv builder); the agent launch arguments here
/// contain no spaces, so the simple `cmd /C "<path>" <args>` form is correct.
fn wrap_windows_shim(spec: SpawnSpec) -> SpawnSpec {
    let ext = spec
        .program
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let program = spec.program.to_string_lossy().to_string();
    match ext.as_deref() {
        Some("cmd") | Some("bat") => {
            // %ComSpec% is an absolute path (an ESSENTIAL_WINDOWS_ENV_KEY, so it
            // is in the curated env); fall back to the canonical System32 path.
            let comspec = lookup_env_ci(&spec.env, "ComSpec")
                .map(str::to_string)
                .unwrap_or_else(|| system32_path(&spec.env, "cmd.exe"));
            let mut args = Vec::with_capacity(spec.args.len() + 2);
            args.push("/C".to_string());
            args.push(program);
            args.extend(spec.args.iter().cloned());
            SpawnSpec {
                program: comspec.into(),
                args,
                ..spec
            }
        }
        Some("ps1") => {
            let powershell =
                system32_path(&spec.env, r"WindowsPowerShell\v1.0\powershell.exe");
            let mut args = vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                program,
            ];
            args.extend(spec.args.iter().cloned());
            SpawnSpec {
                program: powershell.into(),
                args,
                ..spec
            }
        }
        _ => spec,
    }
}

/// Build an absolute `System32` path for `tail` (e.g. `cmd.exe`,
/// `WindowsPowerShell\v1.0\powershell.exe`) from `%SystemRoot%` (an
/// ESSENTIAL_WINDOWS_ENV_KEY), falling back to the conventional `C:\Windows`.
fn system32_path(env: &std::collections::BTreeMap<String, String>, tail: &str) -> String {
    let root = lookup_env_ci(env, "SystemRoot")
        .filter(|r| !r.is_empty())
        .unwrap_or(r"C:\Windows");
    let root = root.trim_end_matches('\\');
    format!(r"{root}\System32\{tail}")
}

// ---------------------------------------------------------------------------
// app.context assembly (theme + localized copy)
// ---------------------------------------------------------------------------

/// The `app.context` reply the reused app requests on boot: renderer kind,
/// initial provider, working directory, the localized `copy` dictionary, and the
/// theme. `provider.list` is a separate call (serviced by the dispatcher).
fn app_context_value(ctx: &DispatchContext) -> Value {
    json!({
        "panelId": "agent-session",
        "workspaceId": "workspace-1",
        "renderer": "react",
        // Claude is auto-start=false, so the user clicks Start; selecting it here
        // just seeds the composer's provider dropdown.
        "initialProviderId": "claude",
        "workingDirectory": ctx.working_directory,
        "copy": copy_value(),
        "theme": theme_value(),
    })
}

/// The canonical dark [`AgentSessionTheme`] (Option A — the shipped web `:root`
/// dark defaults, matching the Swift `AgentSessionWebTheme` dark derivation).
fn theme_value() -> Value {
    json!({
        "isDark": true,
        "pageBackground": "transparent",
        "surfaceBackground": "rgba(28, 31, 27, 0.34)",
        "surfaceElevatedBackground": "rgba(28, 31, 27, 0.48)",
        "inputBackground": "rgba(8, 10, 8, 0.36)",
        "border": "rgba(233, 231, 216, 0.12)",
        "borderStrong": "rgba(233, 231, 216, 0.22)",
        "text": "#f1f0e8",
        "mutedText": "rgba(241, 240, 232, 0.58)",
        "softText": "rgba(241, 240, 232, 0.78)",
        "accent": "#8ab4f8",
        "accentSoft": "rgba(138, 180, 248, 0.2)",
        "danger": "#ff8d7e",
        "shadow": "rgba(0, 0, 0, 0.2)",
    })
}

/// The English [`AgentSessionCopy`] dictionary (all 67 keys). Sourced from the
/// reused app's own copy fixture. Localization (JA etc.) is a Phase 5 concern —
/// the i18n source of truth is `Resources/Localizable.xcstrings`, not
/// `web/messages`. Format specifiers (`%@`, `%d`) are consumed verbatim by the
/// renderer's `formatTemplate`, so they are preserved.
///
/// Built from a `(key, value)` table into a `serde_json::Map` rather than one
/// large `json!` literal (68 entries overflows the macro's recursion limit).
fn copy_value() -> Value {
    const COPY: [(&str, &str); 67] = [
        ("start", "Start"),
        ("stop", "Stop"),
        ("send", "Send"),
        ("provider", "Provider"),
        ("rateLimits", "Rate limits"),
        ("rateLimitUsageRemaining", "Usage remaining"),
        ("rateLimitPrimary", "Primary"),
        ("rateLimitSecondary", "Secondary"),
        ("rateLimitWeekly", "Weekly"),
        ("rateLimitMonthly", "Monthly"),
        ("rateLimitDaysFormat", "%@d"),
        ("rateLimitHoursFormat", "%@h"),
        ("rateLimitMinutesFormat", "%@m"),
        ("rateLimitResets", "resets"),
        ("voiceInput", "Voice input"),
        ("promptPlaceholder", "Ask anything"),
        ("attachFile", "Attach file"),
        ("addFilesAndMore", "Add files and more"),
        ("addPhotosAndFiles", "Add photos & files"),
        ("removeAttachment", "Remove attachment"),
        ("copyOutput", "Copy output"),
        ("copyAssistantMessage", "Copy"),
        ("copiedAssistantMessage", "Copied"),
        ("copyUserMessage", "Copy message"),
        ("copiedUserMessage", "Copied"),
        ("shellLabel", "Shell"),
        ("copyShellContents", "Copy shell contents"),
        ("copiedShellContents", "Copied shell contents"),
        ("collapseShell", "Collapse shell"),
        ("shellSuccess", "Success"),
        ("showMore", "Show more"),
        ("showLess", "Show less"),
        ("browseWeb", "Browse web"),
        ("autoContext", "Context"),
        ("includeIdeContext", "Include IDE context"),
        ("ideContext", "IDE context"),
        ("tools", "Tools"),
        ("changePermissions", "Change permissions"),
        ("permissionsDefault", "Default permissions"),
        ("permissionsFullAccess", "Full access"),
        ("permissionsAutoReview", "Auto-review"),
        ("permissionsCustom", "Custom (config.toml)"),
        ("reasoningEffortHigh", "High"),
        ("mentionMenuTitle", "Mention"),
        ("mentionCurrentWorkspace", "Current workspace"),
        ("skillMenuTitle", "Skills"),
        ("composerNoResults", "No results"),
        ("planMode", "Plan mode"),
        ("planSuggestionAction", "Use plan mode"),
        ("planSuggestionDismiss", "Dismiss suggestion"),
        ("planSuggestionShortcut", "Shift + Tab"),
        ("planSuggestionTitle", "Create a plan"),
        ("skillPlan", "Plan"),
        ("skillCodeReview", "Code review"),
        ("skillResearch", "Research"),
        ("loadingStatus", "Loading"),
        ("idleStatus", "Idle"),
        ("startingStatus", "Starting"),
        ("runningStatus", "Running"),
        ("stoppingStatus", "Stopping"),
        ("failedStatus", "Failed"),
        ("rendererReadyFormat", "%@ ready"),
        ("stopped", "Stopped"),
        ("sentCharsFormat", "Sent %d chars"),
        ("providerStarted", "Provider started"),
        ("providerExitedFormat", "Provider exited %d"),
        ("requestFailed", "Native bridge request failed."),
    ];
    let map: serde_json::Map<String, Value> = COPY
        .iter()
        .map(|(key, value)| ((*key).to_string(), Value::String((*value).to_string())))
        .collect();
    Value::Object(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_id_maps_to_agent_resolver_id() {
        assert_eq!(
            to_agent_provider(ProviderId::Codex),
            AgentSessionProviderId::Codex
        );
        assert_eq!(
            to_agent_provider(ProviderId::Claude),
            AgentSessionProviderId::Claude
        );
        assert_eq!(
            to_agent_provider(ProviderId::Opencode),
            AgentSessionProviderId::OpenCode
        );
    }

    #[test]
    fn wrap_shim_wraps_cmd_with_comspec_and_preserves_args() {
        let mut env = std::collections::BTreeMap::new();
        env.insert("ComSpec".to_string(), r"C:\Windows\System32\cmd.exe".to_string());
        let spec = SpawnSpec::new(r"C:\npm\claude.cmd")
            .args(["-p", "--output-format", "stream-json"])
            .env(env);
        let wrapped = wrap_windows_shim(spec);
        assert_eq!(wrapped.program.to_string_lossy(), r"C:\Windows\System32\cmd.exe");
        assert_eq!(
            wrapped.args,
            vec![
                "/C".to_string(),
                r"C:\npm\claude.cmd".to_string(),
                "-p".to_string(),
                "--output-format".to_string(),
                "stream-json".to_string(),
            ]
        );
    }

    #[test]
    fn wrap_shim_wraps_ps1_with_absolute_powershell() {
        let mut env = std::collections::BTreeMap::new();
        env.insert("SystemRoot".to_string(), r"C:\Windows".to_string());
        let spec = SpawnSpec::new(r"C:\npm\opencode.ps1").args(["serve"]).env(env);
        let wrapped = wrap_windows_shim(spec);
        // Absolute path — CreateProcessW's lpApplicationName is not PATH-searched.
        assert_eq!(
            wrapped.program.to_string_lossy(),
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"
        );
        assert_eq!(
            wrapped.args,
            vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                r"C:\npm\opencode.ps1".to_string(),
                "serve".to_string(),
            ]
        );
    }

    #[test]
    fn wrap_shim_leaves_a_real_exe_untouched() {
        let spec = SpawnSpec::new(r"C:\bin\codex.exe").args(["app-server"]);
        let wrapped = wrap_windows_shim(spec);
        assert_eq!(wrapped.program.to_string_lossy(), r"C:\bin\codex.exe");
        assert_eq!(wrapped.args, vec!["app-server".to_string()]);
    }

    #[test]
    fn wrap_shim_cmd_falls_back_to_absolute_system32_when_env_missing() {
        // No ComSpec and no SystemRoot → conventional C:\Windows\System32\cmd.exe
        // (still absolute, so CreateProcessW can resolve it).
        let spec = SpawnSpec::new(r"C:\npm\claude.cmd");
        let wrapped = wrap_windows_shim(spec);
        assert_eq!(wrapped.program.to_string_lossy(), r"C:\Windows\System32\cmd.exe");
        assert_eq!(wrapped.args[0], "/C");
        assert_eq!(wrapped.args[1], r"C:\npm\claude.cmd");
    }

    #[test]
    fn ok_and_err_envelopes_match_native_reply_shape() {
        let ok = ok_envelope(json!({ "sessionId": "s1" }));
        assert_eq!(ok["ok"], json!(true));
        assert_eq!(ok["value"]["sessionId"], json!("s1"));

        let err = err_envelope("providerNotReady", "Claude Code is not ready.");
        assert_eq!(err["ok"], json!(false));
        assert_eq!(err["error"]["code"], json!("providerNotReady"));
        assert_eq!(err["error"]["userMessage"], json!("Claude Code is not ready."));
    }

    #[test]
    fn app_context_has_required_fields_and_full_copy() {
        let ctx = DispatchContext {
            working_directory: Some("C:/work".to_string()),
        };
        let value = app_context_value(&ctx);
        assert_eq!(value["renderer"], json!("react"));
        assert_eq!(value["initialProviderId"], json!("claude"));
        assert_eq!(value["workingDirectory"], json!("C:/work"));
        // Theme: every one of the 14 fields present, isDark true.
        let theme = value["theme"].as_object().expect("theme object");
        assert_eq!(theme.len(), 14);
        assert_eq!(theme["isDark"], json!(true));
        // Copy: all 67 keys present, format specifiers preserved.
        let copy = value["copy"].as_object().expect("copy object");
        assert_eq!(copy.len(), 67);
        assert_eq!(copy["start"], json!("Start"));
        assert_eq!(copy["sentCharsFormat"], json!("Sent %d chars"));
        assert_eq!(copy["requestFailed"], json!("Native bridge request failed."));
    }

    #[test]
    fn app_context_is_wrapped_in_ok_envelope_by_dispatch() {
        // Build a throwaway store to exercise dispatch_message's app.context arm.
        let (tx, _rx) = channel::<ActorMsg>();
        let transport = ClaudeAgentTransport::new(
            Arc::new(JobObjectSupervisor::new()),
            tx,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        );
        let mut store = ProcessStore::new(transport, |_event: AgentEvent| {}, "9.9.9");
        let ctx = DispatchContext::default();
        let reply = dispatch_message(&mut store, json!({ "method": "app.context" }), &ctx);
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["value"]["renderer"], json!("react"));
    }

    #[test]
    fn pick_files_stub_returns_empty_selection() {
        let (tx, _rx) = channel::<ActorMsg>();
        let transport = ClaudeAgentTransport::new(
            Arc::new(JobObjectSupervisor::new()),
            tx,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        );
        let mut store = ProcessStore::new(transport, |_event: AgentEvent| {}, "9.9.9");
        let ctx = DispatchContext::default();
        let reply = dispatch_message(&mut store, json!({ "method": "app.pickFiles" }), &ctx);
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["value"]["files"], json!([]));
    }

    #[test]
    fn provider_list_flows_through_dispatch_wrapped() {
        let (tx, _rx) = channel::<ActorMsg>();
        let transport = ClaudeAgentTransport::new(
            Arc::new(JobObjectSupervisor::new()),
            tx,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        );
        let mut store = ProcessStore::new(transport, |_event: AgentEvent| {}, "9.9.9");
        let ctx = DispatchContext::default();
        let reply = dispatch_message(
            &mut store,
            json!({ "id": "1", "method": "provider.list", "params": {} }),
            &ctx,
        );
        assert_eq!(reply["ok"], json!(true));
        let list = reply["value"].as_array().expect("provider list array");
        assert_eq!(list.len(), 3);
        assert_eq!(list[1]["id"], json!("claude"));
        assert_eq!(list[1]["autoStart"], json!(false));
    }

    #[test]
    fn unknown_method_maps_to_unsupported_error_envelope() {
        let (tx, _rx) = channel::<ActorMsg>();
        let transport = ClaudeAgentTransport::new(
            Arc::new(JobObjectSupervisor::new()),
            tx,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        );
        let mut store = ProcessStore::new(transport, |_event: AgentEvent| {}, "9.9.9");
        let ctx = DispatchContext::default();
        let reply = dispatch_message(
            &mut store,
            json!({ "id": "1", "method": "provider.bogus", "params": {} }),
            &ctx,
        );
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(reply["error"]["code"], json!("unsupportedMethod"));
    }
}
