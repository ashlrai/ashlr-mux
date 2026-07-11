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
//! via `cmux-process`, and pumps its framed output back into the store. Claude's
//! stream-json prompts write through the transport directly; Codex and OpenCode
//! route their reactive stdin / HTTP writes through drained [`TransportAction`]s
//! so the store stays pure while the actor owns the side effects.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::app_settings::{SettingsStore, SELECTED_PROVIDER_KEY};
use crate::opencode_http::{self, OpenCodeContext, OpenCodeContexts, StreamEnd};
use cmux_agent::{
    AgentExecutableResolver, AgentSessionProviderId, ClaudeConfigContext, OpenCodeServerAuth,
};
use cmux_agent_chat::process_store::{SpawnRequest, SpawnedSession, TransportError};
use cmux_agent_chat::{
    handle, write_claude_stream_json, AgentEvent, AgentTransport, BridgeRequest,
    ChatFileEditOperation, ChatMessage, ChatMessageKind, ChatRole, ChatStatusEvent,
    ChatToolUseStatus, ChatTranscriptParseState, ClaudeTranscriptParser, CodexTranscriptParser,
    DispatchContext, PermissionMode, ProcessStore, ProviderId, ProviderStream, TransportAction,
};
use cmux_process::SpawnSpec;
use cmux_process::{AgentStream, JobObjectSupervisor, ProcessSupervisor, SessionId, TerminateMode};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, State};

/// The push-seam event name the `host.ts` shim forwards to
/// `window.cmuxAgentBridge.receive`.
const AGENT_EVENT: &str = "cmux://agent-event";
const DEFAULT_CLAUDE_WARM_POOL_SIZE: usize = 2;

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
    sessions: LiveChildren,
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
        let sessions = self.sessions.clone();
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
        // The persisted-settings file (provider selection). None if the app data
        // directory cannot be resolved — selection then lives in-memory only.
        let settings = app
            .path()
            .app_data_dir()
            .ok()
            .map(|dir| SettingsStore::new(dir.join("cmux").join("settings.json")));
        let app = app.clone();
        std::thread::Builder::new()
            .name("cmux-agent-session-actor".to_string())
            .spawn(move || run_actor(rx, transport, app, host, settings))
            .expect("spawn agent session actor thread");
        *guard = Some(tx.clone());
        tx
    }

    fn live_root_pids_by_session(&self) -> HashMap<String, u32> {
        self.sessions
            .lock()
            .expect("agent sessions mutex poisoned")
            .iter()
            .map(|(session_id, child)| (session_id.clone(), child.root_pid))
            .collect()
    }
}

/// The actor loop: owns the [`ProcessStore`] and drains [`ActorMsg`]s serially.
///
/// After every message it drains the store's [`TransportAction`]s and performs
/// them against the shared process / HTTP handles: raw Codex stdin frames
/// ([`TransportAction::WriteStdin`]), OpenCode loopback calls, and teardown
/// ([`TransportAction::Terminate`]).
fn run_actor(
    rx: Receiver<ActorMsg>,
    transport: ClaudeAgentTransport,
    app: AppHandle,
    host: ActorHost,
    settings: Option<SettingsStore>,
) {
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

    // The provider `app.context` seeds the composer dropdown with: the persisted
    // last selection when valid, else the port default. Mutable because canonical
    // serves the LIVE initialProviderID (coordinator :350) — a later app.context
    // re-request must reflect the latest select/start.
    let mut initial_provider = load_initial_provider(settings.as_ref());

    while let Ok(msg) = rx.recv() {
        match msg {
            ActorMsg::Rpc { message, reply } => {
                let method = message
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                let copy_locale = if method == "app.context" {
                    current_agent_copy_locale()
                } else {
                    AgentCopyLocale::En
                };
                let envelope =
                    dispatch_message(&mut store, message, &ctx, initial_provider, copy_locale);
                // Canonical sets initialProviderID on BOTH provider.select and
                // provider.start (coordinator :569/:585); an err envelope (e.g.
                // select-while-running) persists nothing.
                if let Some(provider) = provider_update(&method, &envelope) {
                    initial_provider = provider;
                    if let Some(settings) = &settings {
                        settings.set_string(SELECTED_PROVIDER_KEY, provider.as_str());
                    }
                }
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
                    .session(&session_id)
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
/// stdin (no re-newline). `Terminate` tree-kills + reaps the session. OpenCode
/// variants run the loopback HTTP calls on worker threads.
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

/// Load the persisted provider selection, falling back to the port default.
///
/// The raw string is validated through [`ProviderId::from_raw`] — an unknown or
/// renamed id must NOT reach the webview, whose `ProviderId` is a closed union.
/// A missing/corrupt settings file (or an unresolvable settings path) falls back
/// to `claude`, the port's wired default (see [`app_context_value`]).
fn load_initial_provider(settings: Option<&SettingsStore>) -> ProviderId {
    settings
        .and_then(|store| store.get_string(SELECTED_PROVIDER_KEY))
        .and_then(|raw| ProviderId::from_raw(&raw))
        .unwrap_or(ProviderId::Claude)
}

/// The provider a just-dispatched RPC committed, if any: the `providerId` from
/// the ok-envelope of exactly `provider.select` / `provider.start` (canonical
/// sets `initialProviderID` on both, coordinator :569/:585). An err envelope
/// (e.g. select-while-running `SessionAlreadyRunning`) or any other method
/// yields `None`.
fn provider_update(method: &str, reply: &Value) -> Option<ProviderId> {
    if !matches!(method, "provider.select" | "provider.start") {
        return None;
    }
    if reply.get("ok") != Some(&Value::Bool(true)) {
        return None;
    }
    reply
        .get("value")?
        .get("providerId")?
        .as_str()
        .and_then(ProviderId::from_raw)
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
    initial_provider: ProviderId,
    copy_locale: AgentCopyLocale,
) -> Value {
    let method = message
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    match method.as_str() {
        "app.context" => ok_envelope(app_context_value(
            ctx,
            initial_provider,
            copy_locale,
            message_param_string(&message, "panelId").as_deref(),
            message_param_string(&message, "workspaceId").as_deref(),
        )),
        "provider.warmClaude" | "provider.warmPool.prepareClaude" => {
            let working_directory = message_param_string(&message, "workingDirectory")
                .or_else(|| ctx.working_directory.clone());
            let max_warm_sessions = message_param_usize(&message, "maxWarmSessions")
                .unwrap_or(DEFAULT_CLAUDE_WARM_POOL_SIZE);
            match store.warm_claude_session(working_directory, max_warm_sessions) {
                Ok(session) => ok_envelope(json!({
                    "prepared": session.to_value(),
                    "warmSessions": warm_sessions_value(store),
                })),
                Err(error) => err_envelope(error.code(), &error.user_message()),
            }
        }
        "provider.warmPool.status" => ok_envelope(json!({
            "warmSessions": warm_sessions_value(store),
            "defaultMaxWarmSessions": DEFAULT_CLAUDE_WARM_POOL_SIZE,
        })),
        "provider.warmPool.clear" => {
            store.clear_warm_sessions();
            ok_envelope(json!({
                "warmSessions": warm_sessions_value(store),
            }))
        }
        // `app.pickFiles` is normally intercepted in `agent_session_rpc` (it needs
        // the AppHandle for the native dialog + must not block the actor). This arm
        // is only a defensive fallback if it ever reaches the actor: no selection.
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

fn message_param_string(message: &Value, key: &str) -> Option<String> {
    let value = message.get("params")?.get(key)?.as_str()?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn message_param_usize(message: &Value, key: &str) -> Option<usize> {
    let value = message.get("params")?.get(key)?;
    if let Some(raw) = value.as_u64() {
        usize::try_from(raw).ok()
    } else {
        value
            .as_str()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
    }
}

fn warm_sessions_value(
    store: &ProcessStore<ClaudeAgentTransport, impl FnMut(AgentEvent)>,
) -> Value {
    Value::Array(
        store
            .warm_sessions()
            .iter()
            .map(|session| session.to_value())
            .collect(),
    )
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
pub async fn agent_session_rpc(
    app: AppHandle,
    state: State<'_, AgentSessionState>,
    session_state: State<'_, crate::session::SessionState>,
    message: Value,
) -> Result<Value, ()> {
    if message.get("method").and_then(Value::as_str) == Some("app.transcript") {
        return Ok(ok_envelope(app_transcript_reply(&session_state, &message)));
    }

    // `app.pickFiles` opens a native modal file dialog. It MUST NOT run on the
    // command's own thread: Tauri drives sync commands on the main/UI thread, so a
    // blocking dialog there freezes the whole window ("not responding"). Make the
    // command async and run the blocking picker on the blocking thread pool, so the
    // main event loop keeps pumping while the dialog is open.
    if message.get("method").and_then(Value::as_str) == Some("app.pickFiles") {
        let app = app.clone();
        let reply = tauri::async_runtime::spawn_blocking(move || pick_local_files_reply(&app))
            .await
            .unwrap_or_else(|_| json!({ "files": [] }));
        return Ok(ok_envelope(reply));
    }

    let start_scope = started_agent_scope(&message);
    let sender = state.ensure(&app);
    let (reply_tx, reply_rx) = channel::<Value>();
    if sender
        .send(ActorMsg::Rpc {
            message,
            reply: reply_tx,
        })
        .is_err()
    {
        return Ok(err_envelope(
            "actorUnavailable",
            "Agent session host is not running.",
        ));
    }
    let reply = reply_rx
        .recv()
        .unwrap_or_else(|_| err_envelope("actorUnavailable", "Agent session host stopped."));
    if let Some(started) = start_scope.and_then(|scope| started_agent_snapshot(&scope, &reply)) {
        crate::session::record_started_agent_session(&app, &session_state, started);
    }
    Ok(reply)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentListeningPortsWorkspace {
    pub workspace_index: usize,
    pub workspace_id: Option<String>,
    pub root_pids: Vec<u32>,
    pub ports: Vec<u16>,
}

#[tauri::command]
pub fn agent_scan_listening_ports(
    app: AppHandle,
    state: State<'_, AgentSessionState>,
    session_state: State<'_, crate::session::SessionState>,
) -> Result<Vec<AgentListeningPortsWorkspace>, String> {
    scan_agent_listening_ports(&app, &state, &session_state)
}

fn scan_agent_listening_ports(
    app: &AppHandle,
    state: &AgentSessionState,
    session_state: &crate::session::SessionState,
) -> Result<Vec<AgentListeningPortsWorkspace>, String> {
    let live_roots = state.live_root_pids_by_session();
    let snapshot = crate::session::current_session_snapshot(session_state);
    let Some(window) = snapshot.windows.first() else {
        return Ok(Vec::new());
    };

    let mut updates = Vec::new();
    for (workspace_index, workspace) in window.tab_manager.workspaces.iter().enumerate() {
        let mut root_pids: Vec<u32> = workspace
            .restorable_agent_snapshots
            .as_ref()
            .into_iter()
            .flat_map(|entries| entries.iter())
            .filter_map(|entry| live_roots.get(&entry.snapshot.session_id).copied())
            .collect();
        root_pids.extend(
            workspace
                .agent_pids
                .as_ref()
                .into_iter()
                .flat_map(|entries| entries.iter())
                .map(|entry| entry.pid),
        );
        root_pids.sort_unstable();
        root_pids.dedup();

        let mut ports = Vec::new();
        for root_pid in &root_pids {
            ports.extend(crate::terminal::scan_listening_ports_for_root_pid(
                *root_pid,
            )?);
        }
        ports.sort_unstable();
        ports.dedup();

        crate::session::set_workspace_agent_listening_ports_for_control(
            app,
            session_state,
            workspace_index,
            &ports,
        );
        updates.push(AgentListeningPortsWorkspace {
            workspace_index,
            workspace_id: workspace.workspace_id.clone(),
            root_pids,
            ports,
        });
    }
    Ok(updates)
}

struct StartedAgentScope {
    panel_id: String,
    workspace_id: Option<String>,
    working_directory: Option<String>,
}

fn started_agent_scope(message: &Value) -> Option<StartedAgentScope> {
    if message.get("method").and_then(Value::as_str) != Some("provider.start") {
        return None;
    }
    Some(StartedAgentScope {
        panel_id: message_param_string(message, "panelId")?,
        workspace_id: message_param_string(message, "workspaceId"),
        working_directory: message_param_string(message, "workingDirectory"),
    })
}

fn started_agent_snapshot(
    scope: &StartedAgentScope,
    reply: &Value,
) -> Option<crate::session::StartedAgentSessionSnapshot> {
    if reply.get("ok").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let value = reply.get("value")?;
    Some(crate::session::StartedAgentSessionSnapshot {
        panel_id: scope.panel_id.clone(),
        workspace_id: scope.workspace_id.clone(),
        provider_id: value.get("providerId")?.as_str()?.to_string(),
        session_id: value.get("sessionId")?.as_str()?.to_string(),
        executable_path: value
            .get("executablePath")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        arguments: value
            .get("arguments")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        working_directory: scope.working_directory.clone(),
    })
}

/// Open the native "Add photos & files" picker and map the selection to the
/// `app.pickFiles` reply (`{ files: [...] }`). A cancelled dialog yields an empty
/// selection. Mirrors macOS `pickLocalFiles`; the per-file byte→`data:` URL
/// mapping + shared 2MB image budget live in the pure [`crate::pick_files`].
///
/// Blocking: the caller runs this on the blocking thread pool (never the UI
/// thread) — see [`agent_session_rpc`].
fn pick_local_files_reply(app: &AppHandle) -> Value {
    use tauri_plugin_dialog::DialogExt;

    let picked = app
        .dialog()
        .file()
        .set_title("Add photos & files")
        .blocking_pick_files();

    match picked {
        Some(entries) => {
            let paths: Vec<std::path::PathBuf> = entries
                .into_iter()
                .filter_map(|entry| entry.into_path().ok())
                .collect();
            crate::pick_files::picked_files_value(paths)
        }
        None => json!({ "files": [] }),
    }
}

fn app_transcript_reply(state: &crate::session::SessionState, message: &Value) -> Value {
    let Some(panel_id) = message_param_string(message, "panelId") else {
        return Value::Null;
    };
    let workspace_id = message_param_string(message, "workspaceId");
    let snapshot = crate::session::current_session_snapshot(state);
    app_transcript_reply_for_snapshot(
        &snapshot,
        &panel_id,
        workspace_id.as_deref(),
        &home_directory(),
    )
}

fn app_transcript_reply_for_snapshot(
    snapshot: &cmux_core::session::AppSessionSnapshot,
    panel_id: &str,
    workspace_id: Option<&str>,
    home: &Path,
) -> Value {
    let Some(restorable) = restorable_agent_for_panel(snapshot, panel_id, workspace_id) else {
        return Value::Null;
    };
    let Some(provider_id) = ProviderId::from_raw(&restorable.kind) else {
        return Value::Null;
    };
    let entries = transcript_entries_for_restorable(&restorable, home);
    json!({
        "providerId": provider_id.as_str(),
        "sessionId": restorable.session_id,
        "suppressAutoStart": true,
        "entries": entries,
    })
}

fn restorable_agent_for_panel(
    snapshot: &cmux_core::session::AppSessionSnapshot,
    panel_id: &str,
    workspace_id: Option<&str>,
) -> Option<cmux_core::session::SessionRestorableAgentSnapshot> {
    for window in &snapshot.windows {
        for workspace in &window.tab_manager.workspaces {
            if let Some(workspace_id) = workspace_id {
                if workspace.workspace_id.as_deref() != Some(workspace_id) {
                    continue;
                }
            }
            let Some(entries) = &workspace.restorable_agent_snapshots else {
                continue;
            };
            if let Some(entry) = entries.iter().find(|entry| entry.panel_id == panel_id) {
                return Some(entry.snapshot.clone());
            }
        }
    }
    None
}

fn transcript_entries_for_restorable(
    restorable: &cmux_core::session::SessionRestorableAgentSnapshot,
    home: &Path,
) -> Vec<Value> {
    let Some(path) = transcript_path_for_restorable(restorable, home) else {
        return Vec::new();
    };
    let messages = parse_transcript_messages(&restorable.kind, &path);
    messages
        .iter()
        .filter_map(|message| transcript_entry_value(message, &restorable.session_id))
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn parse_transcript_messages(kind: &str, path: &Path) -> Vec<ChatMessage> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let lines = BufReader::new(file).lines().map_while(Result::ok);
    match kind {
        "codex" => {
            CodexTranscriptParser::new()
                .parse(lines, 0, ChatTranscriptParseState::default())
                .messages
        }
        "claude" => {
            ClaudeTranscriptParser::new()
                .parse(lines, 0, ChatTranscriptParseState::default())
                .messages
        }
        _ => Vec::new(),
    }
}

fn transcript_path_for_restorable(
    restorable: &cmux_core::session::SessionRestorableAgentSnapshot,
    home: &Path,
) -> Option<PathBuf> {
    match restorable.kind.as_str() {
        "codex" => codex_transcript_path(home, &restorable.session_id),
        "claude" => restorable
            .working_directory
            .as_deref()
            .and_then(|cwd| claude_transcript_path(home, cwd, &restorable.session_id)),
        _ => None,
    }
}

fn codex_transcript_path(home: &Path, session_id: &str) -> Option<PathBuf> {
    let root = home.join(".codex").join("sessions");
    let needle = session_id.to_ascii_lowercase();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name.to_ascii_lowercase().contains(&needle) {
                return Some(path);
            }
        }
    }
    None
}

fn claude_transcript_path(
    home: &Path,
    working_directory: &str,
    session_id: &str,
) -> Option<PathBuf> {
    if session_id.is_empty() || session_id == "." || session_id == ".." {
        return None;
    }
    if session_id.contains('/') || session_id.contains('\\') {
        return None;
    }
    for cwd in claude_cwd_candidates(working_directory) {
        let path = home
            .join(".claude")
            .join("projects")
            .join(encode_claude_project_dir(&cwd))
            .join(format!("{session_id}.jsonl"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn claude_cwd_candidates(working_directory: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    push_unique(&mut candidates, working_directory.to_string());
    if let Ok(normalized) = PathBuf::from(working_directory).canonicalize() {
        push_unique(&mut candidates, normalized.to_string_lossy().to_string());
    }
    candidates
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !value.is_empty() && !values.iter().any(|existing| existing == &value) {
        values.push(value);
    }
}

fn encode_claude_project_dir(path: &str) -> String {
    path.replace(['/', '\\', '.', ':'], "-")
}

fn transcript_entry_value(message: &ChatMessage, session_id: &str) -> Option<Value> {
    let id = format!("restored-{}", message.id);
    let sent_at_ms = message.timestamp.millis;
    let with_time = |mut value: Value| {
        if sent_at_ms > 0 {
            value["sentAtMs"] = json!(sent_at_ms);
        }
        value
    };
    match &message.kind {
        ChatMessageKind::Prose(prose) => match message.role {
            ChatRole::User => Some(with_time(json!({
                "id": id,
                "role": "user",
                "text": prose.text,
                "sessionId": session_id,
                "isComplete": true,
            }))),
            ChatRole::Agent => Some(with_time(json!({
                "id": id,
                "role": "assistant",
                "text": prose.text,
                "sessionId": session_id,
                "isComplete": true,
            }))),
            ChatRole::System => Some(with_time(json!({
                "id": id,
                "role": "notice",
                "text": prose.text,
                "sessionId": session_id,
            }))),
        },
        ChatMessageKind::Thought(thought) => Some(with_time(json!({
            "id": id,
            "role": "assistant",
            "text": format!("Thinking\n\n{}", thought.text),
            "sessionId": session_id,
            "isComplete": true,
        }))),
        ChatMessageKind::ToolUse(tool) => {
            let mut value = json!({
                "id": id,
                "role": "activity",
                "text": tool.summary,
                "sessionId": session_id,
                "activityId": message.id,
                "activityKind": "other",
                "activityStatus": tool_status(tool.status),
            });
            if let Some(detail) = &tool.input_detail {
                value["detail"] = json!(detail);
            }
            if let Some(output) = &tool.output {
                value["output"] = json!(output);
            }
            Some(with_time(value))
        }
        ChatMessageKind::Terminal(terminal) => {
            let mut value = json!({
                "id": id,
                "role": "activity",
                "text": terminal.command,
                "sessionId": session_id,
                "activityId": message.id,
                "activityKind": "command",
                "activityStatus": terminal_status(terminal.is_running, terminal.exit_code),
            });
            if let Some(output) = &terminal.output {
                value["output"] = json!(output);
            }
            Some(with_time(value))
        }
        ChatMessageKind::FileEdit(edit) => {
            let mut value = json!({
                "id": id,
                "role": "activity",
                "text": file_edit_action(edit.operation),
                "detail": edit.file_path,
                "sessionId": session_id,
                "activityId": message.id,
                "activityKind": "fileChange",
                "activityStatus": "completed",
            });
            if let Some(output) = &edit.unified_diff {
                value["output"] = json!(output);
            }
            Some(with_time(value))
        }
        ChatMessageKind::Question(question) => Some(with_time(json!({
            "id": id,
            "role": "assistant",
            "text": question.prompt,
            "sessionId": session_id,
            "isComplete": question.selected_option_label.is_some(),
        }))),
        ChatMessageKind::Status(status) => Some(with_time(json!({
            "id": id,
            "role": "notice",
            "text": status_text(status.event, status.detail.as_deref()),
            "sessionId": session_id,
        }))),
        ChatMessageKind::PermissionRequest(permission) => Some(with_time(json!({
            "id": id,
            "role": "notice",
            "text": format!("{} {}", permission.title, permission.subject).trim(),
            "sessionId": session_id,
        }))),
        ChatMessageKind::Attachment(_) | ChatMessageKind::Unsupported(_) => None,
    }
}

fn tool_status(status: ChatToolUseStatus) -> &'static str {
    match status {
        ChatToolUseStatus::Running => "inProgress",
        ChatToolUseStatus::Succeeded => "completed",
        ChatToolUseStatus::Failed => "failed",
    }
}

fn terminal_status(is_running: bool, exit_code: Option<i64>) -> &'static str {
    if is_running {
        "inProgress"
    } else if exit_code.unwrap_or(0) == 0 {
        "completed"
    } else {
        "failed"
    }
}

fn file_edit_action(operation: ChatFileEditOperation) -> &'static str {
    match operation {
        ChatFileEditOperation::Edit => "Edited",
        ChatFileEditOperation::Write => "Wrote",
        ChatFileEditOperation::Delete => "Deleted",
    }
}

fn status_text(event: ChatStatusEvent, detail: Option<&str>) -> String {
    let label = match event {
        ChatStatusEvent::SessionStarted => "Session started",
        ChatStatusEvent::SessionEnded => "Session ended",
        ChatStatusEvent::Interrupted => "Interrupted",
        ChatStatusEvent::ContextCompacted => "Context compacted",
    };
    match detail.filter(|detail| !detail.is_empty()) {
        Some(detail) => format!("{label}: {detail}"),
        None => label.to_string(),
    }
}

fn home_directory() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// Concrete transport (spawn + Claude direct writes; Codex/OpenCode use actions)
// ---------------------------------------------------------------------------

/// A live child: the supervisor session id (for termination), its provider (for
/// framing writes), and its stdin writer.
struct LiveChild {
    handle_id: SessionId,
    root_pid: u32,
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
            OpenCodeServerAuth::from_environment(&spec.env).map(|auth| auth.authorization_header)
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
        self.sessions
            .lock()
            .expect("agent sessions mutex poisoned")
            .insert(
                request.session_id.clone(),
                LiveChild {
                    handle_id: handle.id,
                    root_pid: handle.root_pid,
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
                reader_loop(
                    chunks,
                    session_id,
                    feedback,
                    supervisor,
                    sessions,
                    process_running,
                )
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
        let child = sessions
            .get_mut(session_id)
            .ok_or(TransportError::NotReady)?;
        let framed = match child.provider {
            // Claude stream-json: one `{"type":"user",...}` line (already
            // newline-terminated) per prompt. No handshake, no permission mode.
            ProviderId::Claude => write_claude_stream_json(text),
            // The store bypasses AgentTransport::write_line for these providers:
            // Codex frames stdin via TransportAction::WriteStdin and OpenCode
            // submits prompts via TransportAction::OpenCodePostPrompt.
            ProviderId::Codex | ProviderId::Opencode => {
                return Err(TransportError::Failed(
                    "provider write path is handled by transport actions".to_string(),
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
            let _ = self
                .supervisor
                .terminate(handle_id, TerminateMode::Graceful);
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
            let powershell = system32_path(&spec.env, r"WindowsPowerShell\v1.0\powershell.exe");
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
fn app_context_value(
    ctx: &DispatchContext,
    initial_provider: ProviderId,
    copy_locale: AgentCopyLocale,
    panel_id: Option<&str>,
    workspace_id: Option<&str>,
) -> Value {
    json!({
        "panelId": panel_id.unwrap_or("agent-session"),
        "workspaceId": workspace_id.unwrap_or("workspace-1"),
        "renderer": "react",
        // The persisted last selection (default: claude — auto-start=false, so
        // the user clicks Start; a deliberate divergence from canonical .codex
        // since Claude is the wired transport). Seeds the composer's provider
        // dropdown; if the selection has autoStart=true (codex) the renderer's
        // auto-start effect re-arms, exactly as canonical.
        "initialProviderId": initial_provider.as_str(),
        "workingDirectory": ctx.working_directory,
        "copy": copy_value(copy_locale),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentCopyLocale {
    En,
    Ja,
}

type CopyTable = [(&'static str, &'static str); 67];

const EN_COPY: CopyTable = [
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

const JA_COPY: CopyTable = [
    ("start", "開始"),
    ("stop", "停止"),
    ("send", "送信"),
    ("provider", "プロバイダー"),
    ("rateLimits", "レート制限"),
    ("rateLimitUsageRemaining", "残り使用量"),
    ("rateLimitPrimary", "プライマリ"),
    ("rateLimitSecondary", "セカンダリ"),
    ("rateLimitWeekly", "週間"),
    ("rateLimitMonthly", "月間"),
    ("rateLimitDaysFormat", "%@日"),
    ("rateLimitHoursFormat", "%@時間"),
    ("rateLimitMinutesFormat", "%@分"),
    ("rateLimitResets", "リセット"),
    ("voiceInput", "音声入力"),
    ("promptPlaceholder", "何でも聞いてください"),
    ("attachFile", "ファイルを添付"),
    ("addFilesAndMore", "ファイルなどを追加"),
    ("addPhotosAndFiles", "写真とファイルを追加"),
    ("removeAttachment", "添付ファイルを削除"),
    ("copyOutput", "出力をコピー"),
    ("copyAssistantMessage", "コピー"),
    ("copiedAssistantMessage", "コピーしました"),
    ("copyUserMessage", "メッセージをコピー"),
    ("copiedUserMessage", "コピーしました"),
    ("shellLabel", "シェル"),
    ("copyShellContents", "シェルの内容をコピー"),
    ("copiedShellContents", "シェルの内容をコピーしました"),
    ("collapseShell", "シェルを折りたたむ"),
    ("shellSuccess", "成功"),
    ("showMore", "もっと表示"),
    ("showLess", "表示を減らす"),
    ("browseWeb", "ウェブを閲覧"),
    ("autoContext", "コンテキスト"),
    ("includeIdeContext", "IDE コンテキストを含める"),
    ("ideContext", "IDE コンテキスト"),
    ("tools", "ツール"),
    ("changePermissions", "権限を変更"),
    ("permissionsDefault", "デフォルト権限"),
    ("permissionsFullAccess", "フルアクセス"),
    ("permissionsAutoReview", "自動レビュー"),
    ("permissionsCustom", "カスタム (config.toml)"),
    ("reasoningEffortHigh", "高"),
    ("mentionMenuTitle", "メンション"),
    ("mentionCurrentWorkspace", "現在のワークスペース"),
    ("skillMenuTitle", "スキル"),
    ("composerNoResults", "結果がありません"),
    ("planMode", "計画モード"),
    ("planSuggestionAction", "計画モードを使用"),
    ("planSuggestionDismiss", "提案を閉じる"),
    ("planSuggestionShortcut", "Shift + Tab"),
    ("planSuggestionTitle", "計画を作成"),
    ("skillPlan", "計画"),
    ("skillCodeReview", "コードレビュー"),
    ("skillResearch", "リサーチ"),
    ("loadingStatus", "読み込み中"),
    ("idleStatus", "待機中"),
    ("startingStatus", "開始中"),
    ("runningStatus", "実行中"),
    ("stoppingStatus", "停止中"),
    ("failedStatus", "失敗"),
    ("rendererReadyFormat", "%@ の準備ができました"),
    ("stopped", "停止しました"),
    ("sentCharsFormat", "%d 文字を送信しました"),
    ("providerStarted", "プロバイダーを開始しました"),
    ("providerExitedFormat", "プロバイダーが終了しました %d"),
    (
        "requestFailed",
        "ネイティブブリッジリクエストに失敗しました。",
    ),
];

/// The localized [`AgentSessionCopy`] dictionary (all 67 keys). The English
/// table is sourced from the reused web copy fixture; the Japanese table mirrors
/// the canonical AgentChat wording where the Swift string catalog overlaps and
/// covers the remaining host-only keys in the same key order. Format specifiers
/// (`%@`, `%d`) are consumed verbatim by the renderer's `formatTemplate`, so
/// they are preserved.
fn copy_value(locale: AgentCopyLocale) -> Value {
    let copy = match locale {
        AgentCopyLocale::En => &EN_COPY,
        AgentCopyLocale::Ja => &JA_COPY,
    };
    let map: serde_json::Map<String, Value> = copy
        .iter()
        .map(|(key, value)| ((*key).to_string(), Value::String((*value).to_string())))
        .collect();
    Value::Object(map)
}

fn current_agent_copy_locale() -> AgentCopyLocale {
    let configured = crate::config::current_app_language();
    agent_copy_locale_for_language(&configured, system_language_hint().as_deref())
}

fn agent_copy_locale_for_language(configured: &str, system_hint: Option<&str>) -> AgentCopyLocale {
    let configured = configured.trim();
    if locale_tag_is_ja(configured) {
        return AgentCopyLocale::Ja;
    }
    if configured.eq_ignore_ascii_case("system") || configured.is_empty() {
        if system_hint.is_some_and(locale_tag_is_ja) {
            return AgentCopyLocale::Ja;
        }
    }
    AgentCopyLocale::En
}

fn locale_tag_is_ja(value: &str) -> bool {
    let normalized = value.trim().replace('_', "-").to_ascii_lowercase();
    normalized == "ja" || normalized.starts_with("ja-")
}

fn system_language_hint() -> Option<String> {
    ["LANGUAGE", "LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
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
        env.insert(
            "ComSpec".to_string(),
            r"C:\Windows\System32\cmd.exe".to_string(),
        );
        let spec = SpawnSpec::new(r"C:\npm\claude.cmd")
            .args(["-p", "--output-format", "stream-json"])
            .env(env);
        let wrapped = wrap_windows_shim(spec);
        assert_eq!(
            wrapped.program.to_string_lossy(),
            r"C:\Windows\System32\cmd.exe"
        );
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
        let spec = SpawnSpec::new(r"C:\npm\opencode.ps1")
            .args(["serve"])
            .env(env);
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
        assert_eq!(
            wrapped.program.to_string_lossy(),
            r"C:\Windows\System32\cmd.exe"
        );
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
        assert_eq!(
            err["error"]["userMessage"],
            json!("Claude Code is not ready.")
        );
    }

    #[test]
    fn app_context_has_required_fields_and_full_copy() {
        let ctx = DispatchContext {
            working_directory: Some("C:/work".to_string()),
        };
        let value = app_context_value(&ctx, ProviderId::Claude, AgentCopyLocale::En, None, None);
        assert_eq!(value["renderer"], json!("react"));
        assert_eq!(value["panelId"], json!("agent-session"));
        assert_eq!(value["workspaceId"], json!("workspace-1"));
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
        assert_eq!(
            copy["requestFailed"],
            json!("Native bridge request failed.")
        );
    }

    #[test]
    fn app_context_can_emit_full_japanese_copy() {
        let ctx = DispatchContext {
            working_directory: Some("C:/work".to_string()),
        };
        let value = app_context_value(&ctx, ProviderId::Claude, AgentCopyLocale::Ja, None, None);
        let copy = value["copy"].as_object().expect("copy object");
        assert_eq!(copy.len(), 67);
        assert_eq!(copy["start"], json!("開始"));
        assert_eq!(copy["send"], json!("送信"));
        assert_eq!(copy["copyAssistantMessage"], json!("コピー"));
        assert_eq!(copy["rateLimitDaysFormat"], json!("%@日"));
        assert_eq!(copy["sentCharsFormat"], json!("%d 文字を送信しました"));
        assert_eq!(
            copy["requestFailed"],
            json!("ネイティブブリッジリクエストに失敗しました。")
        );
    }

    #[test]
    fn agent_copy_locale_respects_explicit_and_system_japanese() {
        assert_eq!(
            agent_copy_locale_for_language("ja", Some("en-US")),
            AgentCopyLocale::Ja
        );
        assert_eq!(
            agent_copy_locale_for_language("ja-JP", Some("en-US")),
            AgentCopyLocale::Ja
        );
        assert_eq!(
            agent_copy_locale_for_language("system", Some("ja_JP.UTF-8")),
            AgentCopyLocale::Ja
        );
        assert_eq!(
            agent_copy_locale_for_language("en", Some("ja-JP")),
            AgentCopyLocale::En
        );
        assert_eq!(
            agent_copy_locale_for_language("system", Some("fr-FR")),
            AgentCopyLocale::En
        );
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
        let reply = dispatch_message(
            &mut store,
            json!({ "method": "app.context" }),
            &ctx,
            ProviderId::Claude,
            AgentCopyLocale::En,
        );
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["value"]["renderer"], json!("react"));
    }

    #[test]
    fn app_context_uses_message_scope_panel_and_workspace() {
        let mut store = make_store();
        let ctx = DispatchContext::default();
        let reply = dispatch_message(
            &mut store,
            json!({
                "method": "app.context",
                "params": {
                    "panelId": "panel-agent-9",
                    "workspaceId": "workspace-9"
                }
            }),
            &ctx,
            ProviderId::Claude,
            AgentCopyLocale::En,
        );
        assert_eq!(reply["ok"], json!(true));
        assert_eq!(reply["value"]["panelId"], json!("panel-agent-9"));
        assert_eq!(reply["value"]["workspaceId"], json!("workspace-9"));
    }

    #[test]
    fn pick_files_actor_fallback_returns_empty_selection() {
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
            json!({ "method": "app.pickFiles" }),
            &ctx,
            ProviderId::Claude,
            AgentCopyLocale::En,
        );
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
            ProviderId::Claude,
            AgentCopyLocale::En,
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
            ProviderId::Claude,
            AgentCopyLocale::En,
        );
        assert_eq!(reply["ok"], json!(false));
        assert_eq!(reply["error"]["code"], json!("unsupportedMethod"));
    }

    /// A throwaway store wired to a dummy channel (the ClaudeAgentTransport test
    /// pattern; nothing is spawned).
    fn make_store() -> ProcessStore<ClaudeAgentTransport, impl FnMut(AgentEvent)> {
        let (tx, _rx) = channel::<ActorMsg>();
        let transport = ClaudeAgentTransport::new(
            Arc::new(JobObjectSupervisor::new()),
            tx,
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(HashMap::new())),
        );
        ProcessStore::new(transport, |_event: AgentEvent| {}, "9.9.9")
    }

    #[test]
    fn provider_update_extracts_provider_from_select_and_start_ok_envelopes() {
        let ok = ok_envelope(json!({ "providerId": "opencode" }));
        assert_eq!(
            provider_update("provider.select", &ok),
            Some(ProviderId::Opencode)
        );
        assert_eq!(
            provider_update("provider.start", &ok),
            Some(ProviderId::Opencode)
        );
    }

    #[test]
    fn provider_update_ignores_err_envelopes_and_other_methods() {
        // Err envelope (e.g. select-while-running): nothing persists.
        let err = err_envelope("sessionAlreadyRunning", "A session is already running.");
        assert_eq!(provider_update("provider.select", &err), None);
        // Other methods never update, even with a providerId-shaped value.
        let ok = ok_envelope(json!({ "providerId": "codex" }));
        assert_eq!(provider_update("provider.list", &ok), None);
        assert_eq!(provider_update("app.context", &ok), None);
        // A malformed/unknown providerId never reaches the webview.
        let bogus = ok_envelope(json!({ "providerId": "gemini" }));
        assert_eq!(provider_update("provider.select", &bogus), None);
    }

    #[test]
    fn codex_transcript_path_finds_nested_session_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("07")
            .join("08")
            .join("rollout-2026-07-08-codex-session-abc.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(&transcript, b"").unwrap();

        assert_eq!(
            codex_transcript_path(dir.path(), "codex-session"),
            Some(transcript)
        );
    }

    #[test]
    fn claude_transcript_path_uses_encoded_cwd_and_rejects_unsafe_session_ids() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = r"C:\work\project.one";
        let transcript = dir
            .path()
            .join(".claude")
            .join("projects")
            .join(encode_claude_project_dir(cwd))
            .join("claude-session.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(&transcript, b"").unwrap();

        assert_eq!(
            claude_transcript_path(dir.path(), cwd, "claude-session"),
            Some(transcript)
        );
        assert_eq!(claude_transcript_path(dir.path(), cwd, "../escape"), None);
        assert_eq!(claude_transcript_path(dir.path(), cwd, r"..\escape"), None);
    }

    #[test]
    fn transcript_reply_for_snapshot_suppresses_autostart_even_without_file() {
        use cmux_core::session::{
            AppSessionSnapshot, SessionPanelRestorableAgentSnapshot,
            SessionRestorableAgentSnapshot, SessionTabManagerSnapshot, SessionWindowSnapshot,
            SessionWorkspaceSnapshot,
        };

        let dir = tempfile::tempdir().unwrap();
        let snapshot = AppSessionSnapshot {
            version: 1,
            created_at: 0,
            windows: vec![SessionWindowSnapshot {
                window_id: Some("window-1".to_string()),
                selected_workspace_id: None,
                tab_manager: SessionTabManagerSnapshot {
                    selected_workspace_index: Some(0),
                    workspaces: vec![SessionWorkspaceSnapshot {
                        workspace_id: Some("workspace-1".to_string()),
                        process_title: "agent".to_string(),
                        restorable_agent_snapshots: Some(vec![
                            SessionPanelRestorableAgentSnapshot {
                                panel_id: "panel-1".to_string(),
                                snapshot: SessionRestorableAgentSnapshot {
                                    kind: "codex".to_string(),
                                    session_id: "codex-session".to_string(),
                                    working_directory: None,
                                    launch_command: None,
                                    resume_command: None,
                                    fork_command: None,
                                },
                            },
                        ]),
                        ..Default::default()
                    }],
                    workspace_groups: None,
                },
            }],
        };

        let reply = app_transcript_reply_for_snapshot(
            &snapshot,
            "panel-1",
            Some("workspace-1"),
            dir.path(),
        );

        assert_eq!(reply["providerId"], json!("codex"));
        assert_eq!(reply["sessionId"], json!("codex-session"));
        assert_eq!(reply["suppressAutoStart"], json!(true));
        assert_eq!(reply["entries"], json!([]));
        assert_eq!(
            app_transcript_reply_for_snapshot(
                &snapshot,
                "panel-2",
                Some("workspace-1"),
                dir.path()
            ),
            Value::Null
        );
        assert_eq!(
            app_transcript_reply_for_snapshot(
                &snapshot,
                "panel-1",
                Some("workspace-2"),
                dir.path()
            ),
            Value::Null
        );
    }

    #[test]
    fn transcript_entry_value_maps_core_chat_shapes() {
        let timestamp = cmux_agent_chat::Timestamp::from_millis(1_785_000_000_123);
        let user = ChatMessage::new(
            "user-1",
            1,
            ChatRole::User,
            timestamp,
            ChatMessageKind::Prose(cmux_agent_chat::ChatProse::new("hello")),
        );
        let user_value = transcript_entry_value(&user, "session-1").unwrap();
        assert_eq!(user_value["id"], json!("restored-user-1"));
        assert_eq!(user_value["role"], json!("user"));
        assert_eq!(user_value["text"], json!("hello"));
        assert_eq!(user_value["sessionId"], json!("session-1"));
        assert_eq!(user_value["isComplete"], json!(true));
        assert_eq!(user_value["sentAtMs"], json!(1_785_000_000_123i64));

        let terminal = ChatMessage::new(
            "terminal-1",
            2,
            ChatRole::Agent,
            timestamp,
            ChatMessageKind::Terminal(cmux_agent_chat::ChatTerminalCapture {
                command: "cargo test".to_string(),
                output: None,
                exit_code: None,
                duration_seconds: None,
                is_running: true,
            }),
        );
        let terminal_value = transcript_entry_value(&terminal, "session-1").unwrap();
        assert_eq!(terminal_value["role"], json!("activity"));
        assert_eq!(terminal_value["activityKind"], json!("command"));
        assert_eq!(terminal_value["activityStatus"], json!("inProgress"));
        assert!(!terminal_value.as_object().unwrap().contains_key("output"));

        let edit = ChatMessage::new(
            "edit-1",
            3,
            ChatRole::Agent,
            timestamp,
            ChatMessageKind::FileEdit(cmux_agent_chat::ChatFileEdit {
                file_path: "src/main.rs".to_string(),
                operation: ChatFileEditOperation::Edit,
                additions: Some(2),
                deletions: Some(1),
                unified_diff: Some("@@ diff".to_string()),
            }),
        );
        let edit_value = transcript_entry_value(&edit, "session-1").unwrap();
        assert_eq!(edit_value["activityKind"], json!("fileChange"));
        assert_eq!(edit_value["text"], json!("Edited"));
        assert_eq!(edit_value["detail"], json!("src/main.rs"));
        assert_eq!(edit_value["output"], json!("@@ diff"));

        let status = ChatMessage::new(
            "status-1",
            4,
            ChatRole::System,
            timestamp,
            ChatMessageKind::Status(cmux_agent_chat::ChatStatusTransition::new(
                ChatStatusEvent::ContextCompacted,
                Some("summary written".to_string()),
            )),
        );
        let status_value = transcript_entry_value(&status, "session-1").unwrap();
        assert_eq!(status_value["role"], json!("notice"));
        assert_eq!(
            status_value["text"],
            json!("Context compacted: summary written")
        );
    }

    #[test]
    fn app_context_reflects_non_default_provider() {
        let ctx = DispatchContext::default();
        let value = app_context_value(&ctx, ProviderId::Opencode, AgentCopyLocale::En, None, None);
        assert_eq!(value["initialProviderId"], json!("opencode"));
    }

    #[test]
    fn selected_provider_survives_a_simulated_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cmux").join("settings.json");

        // Session 1: renderer selects opencode; the Rpc arm persists it.
        let mut store = make_store();
        let ctx = DispatchContext::default();
        let envelope = dispatch_message(
            &mut store,
            json!({ "id": "1", "method": "provider.select", "params": { "providerId": "opencode" } }),
            &ctx,
            ProviderId::Claude,
            AgentCopyLocale::En,
        );
        let selected = provider_update("provider.select", &envelope).expect("selection committed");
        SettingsStore::new(path.clone()).set_string(SELECTED_PROVIDER_KEY, selected.as_str());

        // Session 2 (restart): a fresh store over the same path restores it.
        let restored = load_initial_provider(Some(&SettingsStore::new(path)));
        assert_eq!(restored, ProviderId::Opencode);
        let value = app_context_value(&ctx, restored, AgentCopyLocale::En, None, None);
        assert_eq!(value["initialProviderId"], json!("opencode"));
    }

    #[test]
    fn initial_provider_falls_back_to_claude_when_settings_absent_or_corrupt() {
        // No settings store at all (unresolvable app data dir).
        assert_eq!(load_initial_provider(None), ProviderId::Claude);
        // Absent file.
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));
        assert_eq!(load_initial_provider(Some(&store)), ProviderId::Claude);
        // Corrupt file.
        std::fs::write(dir.path().join("settings.json"), b"{corrupt").unwrap();
        assert_eq!(load_initial_provider(Some(&store)), ProviderId::Claude);
        // Valid JSON but an unknown provider id (renamed/foreign): still claude.
        std::fs::write(
            dir.path().join("settings.json"),
            br#"{"agentSession.selectedProviderId": "gemini"}"#,
        )
        .unwrap();
        assert_eq!(load_initial_provider(Some(&store)), ProviderId::Claude);
    }
}
