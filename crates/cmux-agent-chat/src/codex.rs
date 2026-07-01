//! Codex `app-server` JSON-RPC request builders + notification accumulator.
//!
//! Ported **faithfully** from the canonical macOS Swift sources:
//!
//! * message handling / mapping — `Sources/Panels/CodexAppServerSession.swift`
//!   (`final class CodexAppServerSession`), and the codex wiring in
//!   `Sources/Panels/AgentSessionProcessStore.swift` (`emitOutput` /
//!   `emitActivity` / `emitTurnComplete` translate the session's sink payloads
//!   into the `provider.*` [`AgentEvent`]s).
//! * per-turn permission overrides — [`PermissionMode::codex_turn_overrides`].
//!
//! The renderer-side shapes this feeds are validated against
//! `webviews/src/agent-session/shared/types.ts` (`AgentEvent`).
//!
//! ## What this slice ports (pure)
//!
//! * **Request-frame builders** — [`initialize_request`],
//!   [`initialized_notification`], [`thread_start_request`],
//!   [`turn_start_request`], and [`encode_line`] reproduce the exact
//!   `{id, method, params}` envelopes the Codex app-server expects, including the
//!   per-turn permission overrides.
//! * **[`CodexAccumulator`]** — the synchronous core of the Swift session:
//!   allocates request ids, builds the handshake / thread / turn frames while
//!   tracking their ids, and consumes one server→client JSON-RPC line at a time,
//!   mapping notifications to [`AgentEvent`]s (`provider.output`,
//!   `provider.activity`, `provider.turnComplete`) exactly as the Swift
//!   `handleNotification` + `AgentSessionProcessStore` sinks do. It tracks the
//!   pure turn state (`is_turn_in_flight`, `active_permission_mode`) and the
//!   startup/thread state, and exposes the pure server-request approval logic via
//!   [`CodexAccumulator::approval_response`].
//!
//! ## Deferred (NOT in this slice — needs the live child process)
//!
//! The actual stdio transport (writing the framed lines, reading stdout), the
//! process spawn, and the **async single-input queue** (Swift `queuedInputs` with
//! its `CheckedContinuation`s, `startThreadIfNeeded` sequencing after
//! `initialized`, and `drainCodexAppServerQueuedInputs`). The queue's *pure*
//! backpressure predicate is provided as [`CodexAccumulator::can_queue_input`],
//! but the continuation bookkeeping and the "send `initialized` then
//! `thread/start` on the initialize response, then drain the queue" orchestration
//! belong to the transport layer.

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use crate::event::{ActivityKind, ActivityStatus, AgentEvent, ProviderId, ProviderStream};
use crate::permission_mode::PermissionMode;

/// Swift `CodexAppServerSession.maxQueuedInputCount`.
const MAX_QUEUED_INPUT_COUNT: usize = 1;
/// Swift `CodexAppServerSession.maxQueuedInputBytes`.
const MAX_QUEUED_INPUT_BYTES: usize = 64 * 1024;

/// English default text for the localized `agentSession.codex.*` strings the
/// Swift session emits on `stderr`. The Rust crate is headless and carries no
/// localization catalog, so the `defaultValue` copy is used verbatim.
const INVALID_JSON_MESSAGE: &str = "Codex app-server response was not valid JSON.";
const RPC_FAILED_MESSAGE: &str = "Codex app-server request failed.";
const UNKNOWN_WARNING_MESSAGE: &str = "Codex app-server reported a warning.";

// ---------------------------------------------------------------------------
// Request-frame builders (pure, stateless)
// ---------------------------------------------------------------------------

/// Build the `initialize` request envelope.
///
/// Mirrors Swift `CodexAppServerSession.start()`: `clientInfo.version` is the
/// caller-supplied app version (`CFBundleShortVersionString`, `"0"` fallback).
pub fn initialize_request(id: i64, client_version: &str) -> Value {
    json!({
        "id": id,
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "cmux",
                "title": "cmux",
                "version": client_version,
            },
            "capabilities": {
                "experimentalApi": true,
                "requestAttestation": false,
            },
        },
    })
}

/// Build the `initialized` notification envelope (no `id`, no `params`).
///
/// Mirrors Swift `sendNotification(method: "initialized")`.
pub fn initialized_notification() -> Value {
    json!({ "method": "initialized" })
}

/// Build the `thread/start` request envelope.
///
/// Mirrors Swift `startThreadIfNeeded()`: `cwd` is included only when a working
/// directory is present.
pub fn thread_start_request(id: i64, working_directory: Option<&str>) -> Value {
    let mut params = Map::new();
    params.insert("serviceName".to_string(), json!("cmux"));
    params.insert("threadSource".to_string(), json!("user"));
    if let Some(cwd) = working_directory {
        params.insert("cwd".to_string(), json!(cwd));
    }
    json!({
        "id": id,
        "method": "thread/start",
        "params": Value::Object(params),
    })
}

/// Build the `turn/start` request envelope, merging the per-turn permission
/// overrides.
///
/// Mirrors Swift `sendTurnStart(threadID:text:permissionMode:)`: the `input`
/// carries a single `text` element with an empty `text_elements` array, and the
/// entries of [`PermissionMode::codex_turn_overrides`] are merged into `params`
/// (so `Custom` adds nothing and the others add `approvalPolicy` etc., including
/// the JSON `null`s).
pub fn turn_start_request(
    id: i64,
    thread_id: &str,
    text: &str,
    permission_mode: PermissionMode,
) -> Value {
    let mut params = Map::new();
    params.insert("threadId".to_string(), json!(thread_id));
    params.insert(
        "input".to_string(),
        json!([{
            "type": "text",
            "text": text,
            "text_elements": [],
        }]),
    );
    if let Value::Object(overrides) = permission_mode.codex_turn_overrides() {
        for (key, value) in overrides {
            params.insert(key, value);
        }
    }
    json!({
        "id": id,
        "method": "turn/start",
        "params": Value::Object(params),
    })
}

/// Frame a JSON-RPC value as a newline-terminated line.
///
/// Mirrors Swift `sendJSONObject`, which serializes the object and appends
/// `0x0A`.
pub fn encode_line(value: &Value) -> String {
    // Serialization of a plain `serde_json::Value` is infallible.
    let mut serialized = serde_json::to_string(value).expect("serialize codex json-rpc line");
    serialized.push('\n');
    serialized
}

// ---------------------------------------------------------------------------
// Submit rejection reasons (pure)
// ---------------------------------------------------------------------------

/// Why [`CodexAccumulator::turn_start`] could not build a `turn/start` frame.
///
/// Mirrors the guard clauses of Swift `submit(_:permissionMode:)`. The Swift
/// code throws `providerNotReady` for [`StartupFailed`](Self::StartupFailed) and
/// [`TurnInFlight`](Self::TurnInFlight), and for [`ThreadNotReady`](Self::ThreadNotReady)
/// it *queues* the input — the queue itself is deferred to the transport (see the
/// module docs), so this port surfaces the reason instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitRejection {
    /// The prompt text was empty (`guard !text.isEmpty`).
    EmptyText,
    /// Startup already failed (`guard !didFailStartup`).
    StartupFailed,
    /// A turn is already in flight (`guard !isTurnInFlight`).
    TurnInFlight,
    /// No thread yet; the transport should queue (subject to [`CodexAccumulator::can_queue_input`]).
    ThreadNotReady,
}

// ---------------------------------------------------------------------------
// Accumulator
// ---------------------------------------------------------------------------

/// Synchronous port of `CodexAppServerSession`'s message core.
///
/// Feed complete server→client JSON-RPC lines to
/// [`consume_line`](Self::consume_line). Build outgoing frames with
/// [`initialize`](Self::initialize) / [`initialized`](Self::initialized) /
/// [`thread_start`](Self::thread_start) / [`turn_start`](Self::turn_start), which
/// allocate request ids and track state exactly as the Swift `sendRequest` did.
#[derive(Debug)]
pub struct CodexAccumulator {
    working_directory: Option<String>,
    client_version: String,
    next_request_id: i64,
    initialize_request_id: Option<i64>,
    did_initialize: bool,
    thread_start_request_id: Option<i64>,
    thread_id: Option<String>,
    did_fail_startup: bool,
    active_permission_mode: PermissionMode,
    is_turn_in_flight: bool,
    turn_start_request_ids: HashSet<i64>,
    queued_input_count: usize,
    queued_input_bytes: usize,
    startup_failure_details: Option<String>,
    startup_failed_signaled: bool,
}

impl CodexAccumulator {
    /// Create an accumulator for a session with the given working directory and
    /// client version (`clientInfo.version`).
    pub fn new(working_directory: Option<String>, client_version: impl Into<String>) -> Self {
        Self {
            working_directory,
            client_version: client_version.into(),
            next_request_id: 1,
            initialize_request_id: None,
            did_initialize: false,
            thread_start_request_id: None,
            thread_id: None,
            did_fail_startup: false,
            active_permission_mode: PermissionMode::Standard,
            is_turn_in_flight: false,
            turn_start_request_ids: HashSet::new(),
            queued_input_count: 0,
            queued_input_bytes: 0,
            startup_failure_details: None,
            startup_failed_signaled: false,
        }
    }

    // ---- outgoing frame builders (allocate ids + track state) ----

    fn allocate_request_id(&mut self) -> i64 {
        let id = self.next_request_id;
        self.next_request_id += 1;
        id
    }

    /// Build the `initialize` request and record its id (Swift `start()`).
    pub fn initialize(&mut self) -> Value {
        let id = self.allocate_request_id();
        self.initialize_request_id = Some(id);
        initialize_request(id, &self.client_version)
    }

    /// Build the `initialized` notification (Swift `sendNotification`).
    pub fn initialized(&self) -> Value {
        initialized_notification()
    }

    /// Build a `thread/start` request if one is warranted, recording its id.
    ///
    /// Mirrors Swift `startThreadIfNeeded()`: returns `None` when startup already
    /// failed, or a thread is present / a thread-start is already in flight.
    pub fn thread_start(&mut self) -> Option<Value> {
        if self.did_fail_startup {
            return None;
        }
        if self.thread_id.is_some() || self.thread_start_request_id.is_some() {
            return None;
        }
        let id = self.allocate_request_id();
        self.thread_start_request_id = Some(id);
        Some(thread_start_request(id, self.working_directory.as_deref()))
    }

    /// Build a `turn/start` request, recording its id and marking the turn
    /// in flight.
    ///
    /// Mirrors the guards of Swift `submit(_:permissionMode:)` +
    /// `sendTurnStart(...)`. On success the active permission mode is stored and
    /// `is_turn_in_flight` is set (both are reset on turn completion / rpc error).
    pub fn turn_start(
        &mut self,
        text: &str,
        permission_mode: PermissionMode,
    ) -> Result<Value, SubmitRejection> {
        if text.is_empty() {
            return Err(SubmitRejection::EmptyText);
        }
        if self.did_fail_startup {
            return Err(SubmitRejection::StartupFailed);
        }
        if self.is_turn_in_flight {
            return Err(SubmitRejection::TurnInFlight);
        }
        let thread_id = match self.thread_id.clone() {
            Some(id) => id,
            None => return Err(SubmitRejection::ThreadNotReady),
        };
        let id = self.allocate_request_id();
        let request = turn_start_request(id, &thread_id, text, permission_mode);
        self.active_permission_mode = permission_mode;
        self.is_turn_in_flight = true;
        self.turn_start_request_ids.insert(id);
        Ok(request)
    }

    // ---- pure state accessors / backpressure ----

    /// Whether a turn is currently awaiting completion.
    pub fn is_turn_in_flight(&self) -> bool {
        self.is_turn_in_flight
    }

    /// Whether the `initialize` response has been observed.
    pub fn did_initialize(&self) -> bool {
        self.did_initialize
    }

    /// The established thread id, once known.
    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }

    /// Whether startup has failed (later submits are rejected).
    pub fn did_fail_startup(&self) -> bool {
        self.did_fail_startup
    }

    /// The permission mode of the in-flight turn (`Standard` when idle).
    pub fn active_permission_mode(&self) -> PermissionMode {
        self.active_permission_mode
    }

    /// Whether `text` could be queued before the thread exists.
    ///
    /// Pure port of Swift `canQueueInput(_:)`: at most
    /// [`MAX_QUEUED_INPUT_COUNT`] queued inputs and
    /// [`MAX_QUEUED_INPUT_BYTES`] total UTF-8 bytes. The queue itself lives in the
    /// transport; [`note_queued`](Self::note_queued) / [`clear_queue`](Self::clear_queue)
    /// let it mirror the counters here.
    pub fn can_queue_input(&self, text: &str) -> bool {
        if self.queued_input_count >= MAX_QUEUED_INPUT_COUNT {
            return false;
        }
        self.queued_input_bytes + text.len() <= MAX_QUEUED_INPUT_BYTES
    }

    /// Record that `text` was queued (transport-side backpressure mirror).
    pub fn note_queued(&mut self, text: &str) {
        self.queued_input_count += 1;
        self.queued_input_bytes += text.len();
    }

    /// Clear the queued-input counters (Swift `queuedInputs.removeAll()`).
    pub fn clear_queue(&mut self) {
        self.queued_input_count = 0;
        self.queued_input_bytes = 0;
    }

    /// The startup-failure details captured on the first failure, if any.
    pub fn startup_failure_details(&self) -> Option<&str> {
        self.startup_failure_details.as_deref()
    }

    /// Whether a startup failure has been signaled (Swift `failureSink` edge).
    ///
    /// Reads and clears the one-shot flag so the caller can drive its equivalent
    /// of `failSession` exactly once.
    pub fn take_startup_failure_signal(&mut self) -> bool {
        std::mem::take(&mut self.startup_failed_signaled)
    }

    // ---- line consumption ----

    /// Consume one server→client JSON-RPC line, mapping it to [`AgentEvent`]s.
    ///
    /// Faithful port of Swift `handleLine`: an empty line is inert; a line that
    /// is not a JSON object yields a single `stderr` [`AgentEvent::ProviderOutput`]
    /// (the `invalidJSON` message); a message with both `method` and `id` is a
    /// server request (no events — use [`approval_response`](Self::approval_response)
    /// to build the reply); a `method`-only message is a notification; otherwise
    /// it is a response / rpc error keyed by `id`.
    ///
    /// `provider_id` is always [`ProviderId::Codex`] for the emitted events.
    pub fn consume_line(&mut self, line: &str, session_id: &str) -> Vec<AgentEvent> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let object = match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Object(map)) => map,
            _ => return vec![self.stderr(session_id, INVALID_JSON_MESSAGE)],
        };

        let method = object.get("method").and_then(Value::as_str);
        let has_id = object.get("id").is_some_and(|v| !v.is_null());

        if let Some(method) = method {
            if has_id {
                // Server request: the approval/error reply is built on demand via
                // `approval_response` and written by the transport (deferred).
                return Vec::new();
            }
            let params = object.get("params").and_then(Value::as_object);
            return self.handle_notification(method, params, session_id);
        }

        let id = match object.get("id").and_then(request_id) {
            Some(id) => id,
            None => return Vec::new(),
        };
        if let Some(error) = object.get("error").and_then(Value::as_object) {
            return self.handle_rpc_error(id, error, session_id);
        }
        self.handle_response(id, object.get("result").and_then(Value::as_object), session_id)
    }

    fn handle_response(
        &mut self,
        id: i64,
        result: Option<&Map<String, Value>>,
        session_id: &str,
    ) -> Vec<AgentEvent> {
        if self.initialize_request_id == Some(id) {
            self.initialize_request_id = None;
            self.did_initialize = true;
            // Swift then sends `initialized` + `thread/start`; that async
            // sequencing is the transport's job (see module docs).
            return Vec::new();
        }

        if self.thread_start_request_id == Some(id) {
            match result
                .and_then(|r| r.get("thread"))
                .and_then(Value::as_object)
                .and_then(|t| t.get("id"))
                .and_then(Value::as_str)
            {
                Some(thread_id) if !thread_id.is_empty() => {
                    self.thread_id = Some(thread_id.to_string());
                    self.thread_start_request_id = None;
                    // Draining queued inputs is the transport's job (deferred).
                    Vec::new()
                }
                _ => self.fail_startup(None, session_id),
            }
        } else {
            // Turn-start responses are acknowledged (Swift removes the id); any
            // other response id is ignored. Neither yields an event.
            self.turn_start_request_ids.remove(&id);
            Vec::new()
        }
    }

    fn handle_rpc_error(
        &mut self,
        id: i64,
        error: &Map<String, Value>,
        session_id: &str,
    ) -> Vec<AgentEvent> {
        let details = error.get("message").and_then(Value::as_str).map(str::to_string);
        if self.initialize_request_id == Some(id) || self.thread_start_request_id == Some(id) {
            return self.fail_startup(details, session_id);
        }
        if self.turn_start_request_ids.remove(&id) {
            self.is_turn_in_flight = false;
            self.active_permission_mode = PermissionMode::Standard;
        }
        vec![self.stderr(session_id, RPC_FAILED_MESSAGE)]
    }

    fn handle_notification(
        &mut self,
        method: &str,
        params: Option<&Map<String, Value>>,
        session_id: &str,
    ) -> Vec<AgentEvent> {
        match method {
            "thread/started" => {
                if self.thread_id.is_none() {
                    if let Some(thread_id) = params
                        .and_then(|p| p.get("thread"))
                        .and_then(Value::as_object)
                        .and_then(|t| t.get("id"))
                        .and_then(Value::as_str)
                    {
                        self.thread_id = Some(thread_id.to_string());
                        self.thread_start_request_id = None;
                        // Drain is the transport's job (deferred).
                    }
                }
                Vec::new()
            }
            "item/agentMessage/delta" => {
                match params.and_then(|p| p.get("delta")).and_then(Value::as_str) {
                    Some(delta) => vec![self.stdout(session_id, delta)],
                    None => Vec::new(),
                }
            }
            "item/agentMessage/completed"
            | "item/agentMessage/complete"
            | "item/agentMessage/finished" => self.complete_turn(session_id),
            "item/started" => match params.and_then(|p| p.get("item")).and_then(Value::as_object) {
                Some(item) => self.activity_for_item(item, ActivityStatus::InProgress, session_id),
                None => Vec::new(),
            },
            "item/completed" => match params.and_then(|p| p.get("item")).and_then(Value::as_object) {
                Some(item) => {
                    if item_is_agent_message(item) {
                        self.complete_turn(session_id)
                    } else {
                        self.activity_for_item(item, ActivityStatus::Completed, session_id)
                    }
                }
                None => Vec::new(),
            },
            "turn/completed" | "turn/complete" | "turn/finished" | "turn/end" | "turn/ended"
            | "turn/stopped" | "turn/failed" | "turn/canceled" | "turn/cancelled" => {
                self.complete_turn(session_id)
            }
            "item/commandExecution/outputDelta" => {
                let Some(item_id) = params.and_then(|p| p.get("itemId")).and_then(Value::as_str)
                else {
                    return Vec::new();
                };
                let output_delta = params
                    .and_then(|p| p.get("delta"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                vec![self.activity(
                    item_id,
                    ActivityKind::Command,
                    ActivityStatus::InProgress,
                    command_action(ActivityStatus::InProgress),
                    None,
                    output_delta,
                    session_id,
                )]
            }
            "item/fileChange/patchUpdated" => {
                let Some(item_id) = params.and_then(|p| p.get("itemId")).and_then(Value::as_str)
                else {
                    return Vec::new();
                };
                let summary = file_change_summary(params.and_then(|p| p.get("changes")));
                vec![self.activity(
                    item_id,
                    ActivityKind::FileChange,
                    ActivityStatus::InProgress,
                    file_change_action(summary.change_type.as_deref(), ActivityStatus::InProgress),
                    summary.path,
                    None,
                    session_id,
                )]
            }
            "error" => {
                let details = params
                    .and_then(|p| p.get("error"))
                    .and_then(Value::as_object)
                    .and_then(|e| e.get("message"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if self.thread_id.is_none()
                    || self.initialize_request_id.is_some()
                    || self.thread_start_request_id.is_some()
                {
                    self.fail_startup(details, session_id)
                } else {
                    vec![self.stderr(session_id, RPC_FAILED_MESSAGE)]
                }
            }
            "warning" | "guardianWarning" | "configWarning" | "deprecationNotice" => {
                let message = codex_message(params).unwrap_or_else(|| UNKNOWN_WARNING_MESSAGE.to_string());
                vec![self.stderr_owned(session_id, message)]
            }
            _ => Vec::new(),
        }
    }

    fn complete_turn(&mut self, session_id: &str) -> Vec<AgentEvent> {
        self.is_turn_in_flight = false;
        self.active_permission_mode = PermissionMode::Standard;
        vec![AgentEvent::ProviderTurnComplete {
            session_id: session_id.to_string(),
            provider_id: ProviderId::Codex,
        }]
    }

    fn fail_startup(&mut self, details: Option<String>, session_id: &str) -> Vec<AgentEvent> {
        // Swift `failStartup` no-ops after the first failure.
        if self.did_fail_startup {
            return Vec::new();
        }
        self.did_fail_startup = true;
        self.initialize_request_id = None;
        self.did_initialize = false;
        self.thread_start_request_id = None;
        self.thread_id = None;
        self.is_turn_in_flight = false;
        self.active_permission_mode = PermissionMode::Standard;
        self.turn_start_request_ids.clear();
        // Failing queued inputs is the transport's job (deferred).
        self.startup_failure_details = details;
        self.startup_failed_signaled = true;
        vec![self.stderr(session_id, RPC_FAILED_MESSAGE)]
    }

    fn activity_for_item(
        &self,
        item: &Map<String, Value>,
        default_status: ActivityStatus,
        session_id: &str,
    ) -> Vec<AgentEvent> {
        let (Some(item_id), Some(item_type)) = (
            item.get("id").and_then(Value::as_str),
            item.get("type").and_then(Value::as_str),
        ) else {
            return Vec::new();
        };
        let status = activity_status(item, default_status);
        match item_type {
            "commandExecution" => vec![self.activity(
                item_id,
                ActivityKind::Command,
                status,
                command_action(status),
                command_text(item),
                None,
                session_id,
            )],
            "fileChange" => {
                let summary = file_change_summary(item.get("changes"));
                vec![self.activity(
                    item_id,
                    ActivityKind::FileChange,
                    status,
                    file_change_action(summary.change_type.as_deref(), status),
                    summary.path,
                    None,
                    session_id,
                )]
            }
            _ => Vec::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn activity(
        &self,
        activity_id: &str,
        kind: ActivityKind,
        status: ActivityStatus,
        action: String,
        detail: Option<String>,
        output_delta: Option<String>,
        session_id: &str,
    ) -> AgentEvent {
        AgentEvent::ProviderActivity {
            session_id: session_id.to_string(),
            provider_id: ProviderId::Codex,
            activity_id: activity_id.to_string(),
            kind,
            status,
            action,
            // Swift drops empty detail / outputDelta from the activity dict.
            detail: detail.filter(|s| !s.is_empty()),
            output_delta: output_delta.filter(|s| !s.is_empty()),
        }
    }

    fn stdout(&self, session_id: &str, text: &str) -> AgentEvent {
        AgentEvent::ProviderOutput {
            session_id: session_id.to_string(),
            provider_id: ProviderId::Codex,
            stream: ProviderStream::Stdout,
            text: text.to_string(),
        }
    }

    fn stderr(&self, session_id: &str, text: &str) -> AgentEvent {
        self.stderr_owned(session_id, text.to_string())
    }

    fn stderr_owned(&self, session_id: &str, text: String) -> AgentEvent {
        AgentEvent::ProviderOutput {
            session_id: session_id.to_string(),
            provider_id: ProviderId::Codex,
            stream: ProviderStream::Stderr,
            text,
        }
    }

    // ---- server-request approvals (pure) ----

    /// Build the reply payload for a server→client approval request.
    ///
    /// Pure port of Swift `handleServerRequest` result building: the decision
    /// depends on [`active_permission_mode`](Self::active_permission_mode).
    /// Returns `None` for unsupported methods (Swift replies with a `-32601`
    /// error — see [`unsupported_server_request_error`]). The actual write is the
    /// transport's job.
    pub fn approval_response(
        &self,
        method: &str,
        params: Option<&Map<String, Value>>,
    ) -> Option<Value> {
        match method {
            "item/commandExecution/requestApproval" => {
                Some(json!({ "decision": self.command_approval_decision() }))
            }
            "item/fileChange/requestApproval" => {
                Some(json!({ "decision": self.command_approval_decision() }))
            }
            "item/permissions/requestApproval" => Some(json!({
                "permissions": self.granted_permissions(params),
                "scope": "turn",
            })),
            "execCommandApproval" | "applyPatchApproval" => {
                Some(json!({ "decision": self.legacy_review_decision() }))
            }
            _ => None,
        }
    }

    fn command_approval_decision(&self) -> &'static str {
        match self.active_permission_mode {
            PermissionMode::FullAccess => "acceptForSession",
            PermissionMode::Standard | PermissionMode::AutoReview | PermissionMode::Custom => {
                "decline"
            }
        }
    }

    fn legacy_review_decision(&self) -> &'static str {
        match self.active_permission_mode {
            PermissionMode::FullAccess => "approved_for_session",
            PermissionMode::Standard | PermissionMode::AutoReview | PermissionMode::Custom => {
                "denied"
            }
        }
    }

    fn granted_permissions(&self, params: Option<&Map<String, Value>>) -> Value {
        if self.active_permission_mode != PermissionMode::FullAccess {
            return json!({});
        }
        params
            .and_then(|p| p.get("permissions"))
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}))
    }
}

/// Build the `-32601` error reply for an unsupported server request.
///
/// Mirrors the `default` arm of Swift `handleServerRequest`. `id` is the raw id
/// value echoed from the request.
pub fn unsupported_server_request_error(id: Value, method: &str) -> Value {
    json!({
        "id": id,
        "error": {
            "code": -32601,
            "message": format!("Request from Codex app-server is not supported: {method}"),
        },
    })
}

// ---------------------------------------------------------------------------
// Pure helpers (ported from CodexAppServerSession statics)
// ---------------------------------------------------------------------------

fn request_id(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64(),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

fn item_is_agent_message(item: &Map<String, Value>) -> bool {
    matches!(
        item.get("type").and_then(Value::as_str),
        Some("agentMessage" | "assistantMessage" | "message")
    )
}

fn activity_status(item: &Map<String, Value>, default_status: ActivityStatus) -> ActivityStatus {
    if let Some(false) = item
        .get("parsedCmd")
        .and_then(Value::as_object)
        .and_then(|c| c.get("isFinished"))
        .and_then(Value::as_bool)
    {
        return ActivityStatus::InProgress;
    }
    let raw_status = item
        .get("executionStatus")
        .and_then(Value::as_str)
        .or_else(|| item.get("status").and_then(Value::as_str));
    match raw_status.map(str::to_ascii_lowercase).as_deref() {
        Some("interrupted" | "canceled" | "cancelled" | "stopped" | "declined" | "denied"
        | "rejected") => ActivityStatus::Stopped,
        Some("failed" | "failure" | "error") => ActivityStatus::Failed,
        Some("inprogress" | "in_progress" | "running" | "started") => ActivityStatus::InProgress,
        Some("completed" | "complete" | "succeeded" | "success") => ActivityStatus::Completed,
        _ => default_status,
    }
}

fn command_action(status: ActivityStatus) -> String {
    match status {
        ActivityStatus::InProgress => "Running".to_string(),
        ActivityStatus::Stopped => "Stopped".to_string(),
        _ => "Ran".to_string(),
    }
}

fn file_change_action(change_type: Option<&str>, status: ActivityStatus) -> String {
    match (change_type, status) {
        (Some("add"), ActivityStatus::InProgress) => "Creating".to_string(),
        (Some("add"), _) => "Created".to_string(),
        (Some("delete"), ActivityStatus::InProgress) => "Deleting".to_string(),
        (Some("delete"), _) => "Deleted".to_string(),
        (_, ActivityStatus::InProgress) => "Editing".to_string(),
        (_, ActivityStatus::Stopped) => "Stopped".to_string(),
        _ => "Edited".to_string(),
    }
}

fn command_text(item: &Map<String, Value>) -> Option<String> {
    if let Some(parsed) = item.get("parsedCmd").and_then(Value::as_object) {
        for key in ["cmd", "command", "name"] {
            if let Some(value) = non_empty_string(parsed.get(key)) {
                return Some(value);
            }
        }
    }
    for key in ["command", "cmd", "commandText", "name"] {
        if let Some(value) = non_empty_string(item.get(key)) {
            return Some(value);
        }
    }
    if let Some(command) = item.get("command").and_then(Value::as_array) {
        let text = command
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// A file-change path + change-type summary (Swift `fileChangeSummary`).
struct FileChangeSummary {
    path: Option<String>,
    change_type: Option<String>,
}

fn file_change_summary(value: Option<&Value>) -> FileChangeSummary {
    match value {
        Some(Value::Object(changes)) => {
            // Swift sorts the keys and returns the first entry.
            let mut keys: Vec<&String> = changes.keys().collect();
            keys.sort();
            if let Some(&key) = keys.first() {
                let change = changes.get(key).and_then(Value::as_object);
                return FileChangeSummary {
                    path: Some(key.clone()),
                    change_type: file_change_type(change),
                };
            }
            FileChangeSummary { path: None, change_type: None }
        }
        Some(Value::Array(changes)) => {
            if let Some(first) = changes.first().and_then(Value::as_object) {
                let path = non_empty_string(first.get("path"))
                    .or_else(|| non_empty_string(first.get("filePath")))
                    .or_else(|| non_empty_string(first.get("name")));
                FileChangeSummary { path, change_type: file_change_type(Some(first)) }
            } else {
                FileChangeSummary { path: None, change_type: None }
            }
        }
        _ => FileChangeSummary { path: None, change_type: None },
    }
}

fn file_change_type(change: Option<&Map<String, Value>>) -> Option<String> {
    let change = change?;
    if let Some(type_str) = non_empty_string(change.get("type")) {
        return Some(type_str);
    }
    if let Some(kind) = change.get("kind").and_then(Value::as_object) {
        return non_empty_string(kind.get("type"));
    }
    non_empty_string(change.get("kind"))
}

fn non_empty_string(value: Option<&Value>) -> Option<String> {
    let text = value.and_then(Value::as_str)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn codex_message(params: Option<&Map<String, Value>>) -> Option<String> {
    let params = params?;
    if let Some(message) = params.get("message").and_then(Value::as_str) {
        return Some(message.to_string());
    }
    if let Some(warning) = params.get("warning").and_then(Value::as_str) {
        return Some(warning.to_string());
    }
    params
        .get("error")
        .and_then(Value::as_object)
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc() -> CodexAccumulator {
        CodexAccumulator::new(Some("/tmp/cmux".to_string()), "1.2.3")
    }

    /// Drive an accumulator through the handshake so a turn can start.
    fn established() -> CodexAccumulator {
        let mut a = acc();
        let init = a.initialize();
        let init_id = init["id"].as_i64().unwrap();
        a.consume_line(
            &format!(r#"{{"id":{init_id},"result":{{"userAgent":"codex"}}}}"#),
            "s",
        );
        let thread = a.thread_start().unwrap();
        let thread_id = thread["id"].as_i64().unwrap();
        a.consume_line(
            &format!(r#"{{"id":{thread_id},"result":{{"thread":{{"id":"thread-1"}}}}}}"#),
            "s",
        );
        a
    }

    // ---- request builders ----

    #[test]
    fn initialize_request_shape() {
        let v = initialize_request(1, "9.9.9");
        assert_eq!(v["id"], json!(1));
        assert_eq!(v["method"], json!("initialize"));
        assert_eq!(v["params"]["clientInfo"]["name"], json!("cmux"));
        assert_eq!(v["params"]["clientInfo"]["title"], json!("cmux"));
        assert_eq!(v["params"]["clientInfo"]["version"], json!("9.9.9"));
        assert_eq!(v["params"]["capabilities"]["experimentalApi"], json!(true));
        assert_eq!(v["params"]["capabilities"]["requestAttestation"], json!(false));
    }

    #[test]
    fn initialized_notification_has_no_id_or_params() {
        let v = initialized_notification();
        assert_eq!(v["method"], json!("initialized"));
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("id"));
        assert!(!obj.contains_key("params"));
    }

    #[test]
    fn thread_start_includes_cwd_when_present() {
        let v = thread_start_request(2, Some("/work/dir"));
        assert_eq!(v["method"], json!("thread/start"));
        assert_eq!(v["params"]["serviceName"], json!("cmux"));
        assert_eq!(v["params"]["threadSource"], json!("user"));
        assert_eq!(v["params"]["cwd"], json!("/work/dir"));
    }

    #[test]
    fn thread_start_omits_cwd_when_absent() {
        let v = thread_start_request(2, None);
        assert!(!v["params"].as_object().unwrap().contains_key("cwd"));
    }

    #[test]
    fn turn_start_full_access_merges_overrides_and_input() {
        let v = turn_start_request(5, "thread-1", "hello codex", PermissionMode::FullAccess);
        assert_eq!(v["id"], json!(5));
        assert_eq!(v["method"], json!("turn/start"));
        assert_eq!(v["params"]["threadId"], json!("thread-1"));
        assert_eq!(v["params"]["input"][0]["type"], json!("text"));
        assert_eq!(v["params"]["input"][0]["text"], json!("hello codex"));
        assert_eq!(v["params"]["input"][0]["text_elements"], json!([]));
        assert_eq!(v["params"]["approvalPolicy"], json!("never"));
        assert_eq!(v["params"]["approvalsReviewer"], json!("user"));
        assert_eq!(v["params"]["sandboxPolicy"]["type"], json!("dangerFullAccess"));
    }

    #[test]
    fn turn_start_standard_includes_null_overrides() {
        let v = turn_start_request(5, "t", "hi", PermissionMode::Standard);
        assert_eq!(v["params"]["approvalPolicy"], json!("never"));
        assert!(v["params"].as_object().unwrap().contains_key("approvalsReviewer"));
        assert_eq!(v["params"]["approvalsReviewer"], Value::Null);
        assert_eq!(v["params"]["sandboxPolicy"], Value::Null);
    }

    #[test]
    fn turn_start_custom_adds_no_overrides() {
        let v = turn_start_request(5, "t", "hi", PermissionMode::Custom);
        let params = v["params"].as_object().unwrap();
        assert!(!params.contains_key("approvalPolicy"));
        assert!(!params.contains_key("approvalsReviewer"));
        assert!(!params.contains_key("sandboxPolicy"));
    }

    #[test]
    fn encode_line_is_newline_terminated_json() {
        let line = encode_line(&json!({"method":"initialized"}));
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
        let parsed: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(parsed["method"], json!("initialized"));
    }

    // ---- handshake / request-id tracking ----

    #[test]
    fn initialize_records_id_and_response_marks_initialized() {
        let mut a = acc();
        let init = a.initialize();
        assert_eq!(init["id"], json!(1));
        assert!(!a.did_initialize());
        let out = a.consume_line(r#"{"id":1,"result":{"userAgent":"codex"}}"#, "s");
        assert!(out.is_empty());
        assert!(a.did_initialize());
    }

    #[test]
    fn thread_start_response_sets_thread_id() {
        let mut a = acc();
        a.initialize();
        a.consume_line(r#"{"id":1,"result":{}}"#, "s");
        let thread = a.thread_start().unwrap();
        assert_eq!(thread["id"], json!(2));
        assert_eq!(a.thread_id(), None);
        a.consume_line(r#"{"id":2,"result":{"thread":{"id":"thread-9"}}}"#, "s");
        assert_eq!(a.thread_id(), Some("thread-9"));
    }

    #[test]
    fn thread_start_guards_against_duplicate() {
        let mut a = acc();
        a.initialize();
        a.consume_line(r#"{"id":1,"result":{}}"#, "s");
        assert!(a.thread_start().is_some());
        // A thread-start is already in flight -> no second request.
        assert!(a.thread_start().is_none());
    }

    #[test]
    fn thread_start_missing_thread_fails_startup() {
        let mut a = acc();
        a.initialize();
        a.consume_line(r#"{"id":1,"result":{}}"#, "s");
        a.thread_start();
        let out = a.consume_line(r#"{"id":2,"result":{}}"#, "s");
        assert_eq!(out.len(), 1);
        assert!(a.did_fail_startup());
        assert!(a.take_startup_failure_signal());
    }

    // ---- turn submission ----

    #[test]
    fn turn_start_before_thread_is_thread_not_ready() {
        let mut a = acc();
        a.initialize();
        assert_eq!(
            a.turn_start("hi", PermissionMode::Standard),
            Err(SubmitRejection::ThreadNotReady)
        );
    }

    #[test]
    fn turn_start_empty_text_rejected() {
        let mut a = established();
        assert_eq!(a.turn_start("", PermissionMode::Standard), Err(SubmitRejection::EmptyText));
    }

    #[test]
    fn turn_start_marks_in_flight_and_blocks_reentry() {
        let mut a = established();
        let turn = a.turn_start("do it", PermissionMode::FullAccess).unwrap();
        assert_eq!(turn["method"], json!("turn/start"));
        assert_eq!(turn["params"]["input"][0]["text"], json!("do it"));
        assert!(a.is_turn_in_flight());
        assert_eq!(a.active_permission_mode(), PermissionMode::FullAccess);
        assert_eq!(
            a.turn_start("again", PermissionMode::Standard),
            Err(SubmitRejection::TurnInFlight)
        );
    }

    #[test]
    fn turn_completion_notification_resets_turn_state() {
        let mut a = established();
        a.turn_start("do it", PermissionMode::FullAccess).unwrap();
        let out = a.consume_line(r#"{"method":"turn/completed","params":{"threadId":"thread-1"}}"#, "s");
        assert_eq!(
            out,
            vec![AgentEvent::ProviderTurnComplete {
                session_id: "s".into(),
                provider_id: ProviderId::Codex,
            }]
        );
        assert!(!a.is_turn_in_flight());
        assert_eq!(a.active_permission_mode(), PermissionMode::Standard);
    }

    #[test]
    fn turn_start_after_startup_failure_rejected() {
        let mut a = acc();
        a.initialize();
        a.consume_line(r#"{"id":1,"error":{"message":"nope"}}"#, "s");
        assert!(a.did_fail_startup());
        assert_eq!(
            a.turn_start("hi", PermissionMode::Standard),
            Err(SubmitRejection::StartupFailed)
        );
    }

    // ---- notification -> event mapping ----

    #[test]
    fn agent_message_delta_maps_to_stdout() {
        let mut a = acc();
        let out = a.consume_line(
            r#"{"method":"item/agentMessage/delta","params":{"delta":"partial answer"}}"#,
            "sess",
        );
        assert_eq!(
            out,
            vec![AgentEvent::ProviderOutput {
                session_id: "sess".into(),
                provider_id: ProviderId::Codex,
                stream: ProviderStream::Stdout,
                text: "partial answer".into(),
            }]
        );
    }

    #[test]
    fn agent_message_completed_marks_turn_complete() {
        let mut a = acc();
        for method in [
            "item/agentMessage/completed",
            "item/agentMessage/complete",
            "item/agentMessage/finished",
        ] {
            let out = a.consume_line(&format!(r#"{{"method":"{method}"}}"#), "s");
            assert_eq!(
                out,
                vec![AgentEvent::ProviderTurnComplete {
                    session_id: "s".into(),
                    provider_id: ProviderId::Codex,
                }]
            );
        }
    }

    #[test]
    fn item_completed_agent_message_completes_turn() {
        let mut a = acc();
        let out = a.consume_line(
            r#"{"method":"item/completed","params":{"item":{"id":"m1","type":"agentMessage"}}}"#,
            "s",
        );
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], AgentEvent::ProviderTurnComplete { .. }));
    }

    #[test]
    fn command_execution_item_completed_maps_to_activity() {
        let mut a = acc();
        let out = a.consume_line(
            r#"{"method":"item/completed","params":{"item":{"id":"cmd-1","type":"commandExecution","status":"completed","command":"ls -la"}}}"#,
            "s",
        );
        match &out[0] {
            AgentEvent::ProviderActivity {
                activity_id,
                kind,
                status,
                action,
                detail,
                ..
            } => {
                assert_eq!(activity_id, "cmd-1");
                assert_eq!(*kind, ActivityKind::Command);
                assert_eq!(*status, ActivityStatus::Completed);
                assert_eq!(action, "Ran");
                assert_eq!(detail.as_deref(), Some("ls -la"));
            }
            other => panic!("expected activity, got {other:?}"),
        }
    }

    #[test]
    fn file_change_kind_maps_to_specific_action() {
        let mut a = acc();
        let created = a.consume_line(
            r#"{"method":"item/completed","params":{"item":{"id":"file-1","type":"fileChange","status":"completed","changes":[{"path":"Created.swift","kind":{"type":"add"}}]}}}"#,
            "s",
        );
        match &created[0] {
            AgentEvent::ProviderActivity { action, detail, kind, .. } => {
                assert_eq!(*kind, ActivityKind::FileChange);
                assert_eq!(action, "Created");
                assert_eq!(detail.as_deref(), Some("Created.swift"));
            }
            other => panic!("expected activity, got {other:?}"),
        }

        let updated = a.consume_line(
            r#"{"method":"item/fileChange/patchUpdated","params":{"itemId":"file-2","changes":[{"path":"Deleted.swift","kind":{"type":"delete"}}]}}"#,
            "s",
        );
        match &updated[0] {
            AgentEvent::ProviderActivity { action, detail, status, .. } => {
                // patchUpdated is always inProgress -> delete+inProgress = "Deleting".
                assert_eq!(*status, ActivityStatus::InProgress);
                assert_eq!(action, "Deleting");
                assert_eq!(detail.as_deref(), Some("Deleted.swift"));
            }
            other => panic!("expected activity, got {other:?}"),
        }
    }

    #[test]
    fn declined_items_render_as_stopped() {
        let mut a = acc();
        let cmd = a.consume_line(
            r#"{"method":"item/completed","params":{"item":{"id":"cmd-1","type":"commandExecution","status":"declined","command":"danger"}}}"#,
            "s",
        );
        match &cmd[0] {
            AgentEvent::ProviderActivity { status, action, kind, .. } => {
                assert_eq!(*kind, ActivityKind::Command);
                assert_eq!(*status, ActivityStatus::Stopped);
                assert_eq!(action, "Stopped");
            }
            other => panic!("expected activity, got {other:?}"),
        }

        let file = a.consume_line(
            r#"{"method":"item/completed","params":{"item":{"id":"file-1","type":"fileChange","status":"declined","changes":[{"path":"README.md","type":"update"}]}}}"#,
            "s",
        );
        match &file[0] {
            AgentEvent::ProviderActivity { status, action, kind, .. } => {
                assert_eq!(*kind, ActivityKind::FileChange);
                assert_eq!(*status, ActivityStatus::Stopped);
                assert_eq!(action, "Stopped");
            }
            other => panic!("expected activity, got {other:?}"),
        }
    }

    #[test]
    fn parsed_cmd_unfinished_forces_in_progress() {
        let mut a = acc();
        let out = a.consume_line(
            r#"{"method":"item/started","params":{"item":{"id":"cmd-1","type":"commandExecution","status":"completed","parsedCmd":{"isFinished":false,"cmd":"sleep 5"}}}}"#,
            "s",
        );
        match &out[0] {
            AgentEvent::ProviderActivity { status, action, detail, .. } => {
                assert_eq!(*status, ActivityStatus::InProgress);
                assert_eq!(action, "Running");
                assert_eq!(detail.as_deref(), Some("sleep 5"));
            }
            other => panic!("expected activity, got {other:?}"),
        }
    }

    #[test]
    fn command_output_delta_carries_output_and_omits_empty_detail() {
        let mut a = acc();
        let out = a.consume_line(
            r#"{"method":"item/commandExecution/outputDelta","params":{"itemId":"cmd-1","delta":"line of output"}}"#,
            "s",
        );
        match &out[0] {
            AgentEvent::ProviderActivity { output_delta, detail, action, status, .. } => {
                assert_eq!(*status, ActivityStatus::InProgress);
                assert_eq!(action, "Running");
                assert_eq!(output_delta.as_deref(), Some("line of output"));
                assert!(detail.is_none());
            }
            other => panic!("expected activity, got {other:?}"),
        }
    }

    #[test]
    fn turn_lifecycle_synonyms_all_complete() {
        for method in [
            "turn/completed",
            "turn/complete",
            "turn/finished",
            "turn/end",
            "turn/ended",
            "turn/stopped",
            "turn/failed",
            "turn/canceled",
            "turn/cancelled",
        ] {
            let mut a = acc();
            let out = a.consume_line(&format!(r#"{{"method":"{method}"}}"#), "s");
            assert_eq!(out.len(), 1, "{method} should complete the turn");
            assert!(matches!(out[0], AgentEvent::ProviderTurnComplete { .. }));
        }
    }

    // ---- warnings + errors -> stderr ----

    #[test]
    fn warning_emits_message_on_stderr() {
        let mut a = acc();
        let out = a.consume_line(
            r#"{"method":"warning","params":{"message":"disk almost full"}}"#,
            "s",
        );
        assert_eq!(
            out,
            vec![AgentEvent::ProviderOutput {
                session_id: "s".into(),
                provider_id: ProviderId::Codex,
                stream: ProviderStream::Stderr,
                text: "disk almost full".into(),
            }]
        );
    }

    #[test]
    fn warning_without_message_uses_default() {
        let mut a = acc();
        let out = a.consume_line(r#"{"method":"configWarning","params":{}}"#, "s");
        match &out[0] {
            AgentEvent::ProviderOutput { stream, text, .. } => {
                assert_eq!(*stream, ProviderStream::Stderr);
                assert_eq!(text, UNKNOWN_WARNING_MESSAGE);
            }
            other => panic!("expected stderr output, got {other:?}"),
        }
    }

    #[test]
    fn error_notification_before_thread_fails_startup() {
        let mut a = acc();
        a.initialize();
        // initialize_request_id is set -> startup failure path.
        let out = a.consume_line(
            r#"{"method":"error","params":{"error":{"message":"boom"}}}"#,
            "s",
        );
        assert_eq!(out.len(), 1);
        assert!(a.did_fail_startup());
        assert_eq!(a.startup_failure_details(), Some("boom"));
    }

    #[test]
    fn error_notification_after_thread_is_rpc_failure() {
        let mut a = established();
        let out = a.consume_line(
            r#"{"method":"error","params":{"error":{"message":"transient"}}}"#,
            "s",
        );
        match &out[0] {
            AgentEvent::ProviderOutput { stream, text, .. } => {
                assert_eq!(*stream, ProviderStream::Stderr);
                assert_eq!(text, RPC_FAILED_MESSAGE);
            }
            other => panic!("expected stderr output, got {other:?}"),
        }
        assert!(!a.did_fail_startup());
    }

    #[test]
    fn turn_start_rpc_error_resets_turn_and_emits_stderr() {
        let mut a = established();
        let turn = a.turn_start("go", PermissionMode::FullAccess).unwrap();
        let turn_id = turn["id"].as_i64().unwrap();
        let out = a.consume_line(&format!(r#"{{"id":{turn_id},"error":{{"message":"rejected"}}}}"#), "s");
        match &out[0] {
            AgentEvent::ProviderOutput { stream, text, .. } => {
                assert_eq!(*stream, ProviderStream::Stderr);
                assert_eq!(text, RPC_FAILED_MESSAGE);
            }
            other => panic!("expected stderr output, got {other:?}"),
        }
        assert!(!a.is_turn_in_flight());
        assert_eq!(a.active_permission_mode(), PermissionMode::Standard);
    }

    // ---- malformed tolerance ----

    #[test]
    fn empty_and_whitespace_lines_are_inert() {
        let mut a = acc();
        for line in ["", "   ", "\n", "  \t "] {
            assert!(a.consume_line(line, "s").is_empty());
        }
    }

    #[test]
    fn non_object_json_emits_invalid_json_stderr() {
        let mut a = acc();
        for line in ["not json", "[1,2,3]", "42", "\"str\"", "null"] {
            let out = a.consume_line(line, "s");
            assert_eq!(out.len(), 1, "{line:?} should emit one stderr event");
            match &out[0] {
                AgentEvent::ProviderOutput { stream, text, .. } => {
                    assert_eq!(*stream, ProviderStream::Stderr);
                    assert_eq!(text, INVALID_JSON_MESSAGE);
                }
                other => panic!("expected stderr output, got {other:?}"),
            }
        }
    }

    #[test]
    fn server_request_yields_no_events() {
        let mut a = acc();
        let out = a.consume_line(
            r#"{"id":"cmd-1","method":"item/commandExecution/requestApproval","params":{}}"#,
            "s",
        );
        assert!(out.is_empty());
    }

    #[test]
    fn unknown_notification_is_ignored() {
        let mut a = acc();
        assert!(a.consume_line(r#"{"method":"item/reasoning/delta","params":{}}"#, "s").is_empty());
    }

    #[test]
    fn line_with_surrounding_whitespace_is_trimmed() {
        let mut a = acc();
        let out = a.consume_line(
            "  {\"method\":\"item/agentMessage/delta\",\"params\":{\"delta\":\"x\"}}  \n",
            "s",
        );
        assert_eq!(out.len(), 1);
    }

    // ---- approvals (pure) ----

    #[test]
    fn approvals_decline_unless_full_access() {
        let mut a = established();
        a.turn_start("go", PermissionMode::Standard).unwrap();
        assert_eq!(
            a.approval_response("item/commandExecution/requestApproval", None),
            Some(json!({"decision":"decline"}))
        );
        assert_eq!(
            a.approval_response("item/fileChange/requestApproval", None),
            Some(json!({"decision":"decline"}))
        );
        assert_eq!(
            a.approval_response("execCommandApproval", None),
            Some(json!({"decision":"denied"}))
        );
    }

    #[test]
    fn approvals_accept_for_full_access() {
        let mut a = established();
        a.turn_start("go", PermissionMode::FullAccess).unwrap();
        assert_eq!(
            a.approval_response("item/commandExecution/requestApproval", None),
            Some(json!({"decision":"acceptForSession"}))
        );
        assert_eq!(
            a.approval_response("applyPatchApproval", None),
            Some(json!({"decision":"approved_for_session"}))
        );
    }

    #[test]
    fn permission_approval_echoes_permissions_only_for_full_access() {
        let mut a = established();
        a.turn_start("go", PermissionMode::FullAccess).unwrap();
        let params: Map<String, Value> = serde_json::from_str(
            r#"{"permissions":{"network":{"enabled":true}}}"#,
        )
        .unwrap();
        let response = a
            .approval_response("item/permissions/requestApproval", Some(&params))
            .unwrap();
        assert_eq!(response["scope"], json!("turn"));
        assert_eq!(response["permissions"]["network"]["enabled"], json!(true));

        // Standard mode returns empty permissions.
        let mut b = established();
        b.turn_start("go", PermissionMode::Standard).unwrap();
        let response = b
            .approval_response("item/permissions/requestApproval", Some(&params))
            .unwrap();
        assert_eq!(response["permissions"], json!({}));
    }

    #[test]
    fn unsupported_server_request_has_no_approval() {
        let a = acc();
        assert_eq!(a.approval_response("something/unknown", None), None);
        let err = unsupported_server_request_error(json!("req-1"), "something/unknown");
        assert_eq!(err["id"], json!("req-1"));
        assert_eq!(err["error"]["code"], json!(-32601));
    }

    // ---- backpressure predicate ----

    #[test]
    fn can_queue_input_respects_count_and_byte_caps() {
        let mut a = acc();
        assert!(a.can_queue_input("first"));
        a.note_queued("first");
        // One already queued -> at count cap.
        assert!(!a.can_queue_input("second"));
        a.clear_queue();
        assert!(a.can_queue_input("second"));
        // Oversized single input is rejected.
        assert!(!a.can_queue_input(&"x".repeat(MAX_QUEUED_INPUT_BYTES + 1)));
    }

    // ---- integration ----

    #[test]
    fn full_turn_flow_streams_output_then_completes() {
        let mut a = established();
        a.turn_start("what is 2+2", PermissionMode::Standard).unwrap();
        let mut text = String::new();
        let mut completed = false;
        let lines = [
            r#"{"method":"item/agentMessage/delta","params":{"delta":"The "}}"#,
            r#"{"method":"item/agentMessage/delta","params":{"delta":"answer "}}"#,
            r#"{"method":"item/agentMessage/delta","params":{"delta":"is 4."}}"#,
            r#"{"method":"turn/completed","params":{"threadId":"thread-1"}}"#,
        ];
        for line in lines {
            for ev in a.consume_line(line, "s") {
                match ev {
                    AgentEvent::ProviderOutput { text: t, stream: ProviderStream::Stdout, .. } => {
                        text.push_str(&t)
                    }
                    AgentEvent::ProviderTurnComplete { .. } => completed = true,
                    other => panic!("unexpected event {other:?}"),
                }
            }
        }
        assert_eq!(text, "The answer is 4.");
        assert!(completed);
        assert!(!a.is_turn_in_flight());
    }
}
