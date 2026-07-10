//! OpenCode SSE event parsing + assistant-text accumulation (pure core).
//!
//! Ported **faithfully** from the canonical macOS Swift sources:
//!
//! * SSE framing — `Sources/Panels/OpenCodeEventStreamParser.swift`
//!   (struct `OpenCodeEventStreamParser`).
//! * text accumulation — `Sources/Panels/OpenCodeEventTextAccumulator.swift`
//!   (struct `OpenCodeEventTextAccumulator`).
//! * stdout disposition — `AgentSessionProcessStore.openCodeProcessOutputDisposition`
//!   / `openCodeServerURL` (with the loopback check `agentSessionIsLoopbackURL`
//!   in `Sources/Panels/AgentSessionBridge.swift`), plus the enum
//!   `Sources/Panels/OpenCodeProcessOutputDisposition.swift`.
//! * event mapping — the opencode branch of
//!   `AgentSessionProcessStore.handleOpenCodeEvent` (turn-completion is checked
//!   first, then each accumulated text delta becomes a `provider.output` on
//!   `stdout`, then a `provider.turnComplete` when the event completes the turn).
//!
//! The renderer-side shapes this feeds are validated against
//! `webviews/src/agent-session/shared/types.ts` (`AgentEvent`).
//!
//! ## What this slice ports (pure)
//!
//! * [`OpenCodeEventStreamParser`] — frames raw SSE lines into JSON event
//!   objects (`data:` line accumulation, blank-line dispatch, `\r` trimming, the
//!   1 MiB data-byte cap, and JSON-object decoding on flush).
//! * [`OpenCodeEventTextAccumulator`] — consumes parsed events, tracks per-part
//!   assistant text, and yields de-duplicated text deltas plus turn-completion
//!   detection, mapped to the crate [`AgentEvent`] shapes.
//! * [`opencode_process_output_disposition`] — the pure stdout-line sniff that
//!   classifies a line as a captured server URL, suppressed, or emitted
//!   ([`OpenCodeProcessOutputDisposition`]).
//!
//! ## Deferred (NOT in this slice — needs `reqwest` / async)
//!
//! The http-loopback client itself: creating the OpenCode session
//! (`POST /session`), submitting a prompt (`POST /session/<id>/prompt_async`),
//! and fetching the `/event` SSE stream (`URLSession.bytes(for:)` → feed the
//! parser). The `OpenCodeServerAuth` authorization-header injection (already
//! ported in `cmux-agent`) is wired in by that transport layer.
//!
//! ## Known divergence (intentional, documented)
//!
//! * Swift `String.count` / `dropFirst` / `suffix` count **extended grapheme
//!   clusters**; this port counts **Unicode scalar values** (`str::chars`),
//!   exactly as the sibling `claude` port does. The two agree for all ASCII /
//!   BMP-without-combining text (the overwhelmingly common OpenCode output);
//!   they can differ only for multi-scalar grapheme clusters, and only when a
//!   full-text re-send must be diffed against prior streamed deltas.
//! * [`OpenCodeProcessOutputDisposition::ServerUrl`] carries the **raw captured
//!   URL string** rather than a parsed URL value: the crate is headless and has
//!   no URL type / normalization. The loopback gate (`localhost` / `127.0.0.1`
//!   / `::1`) is still applied before capture, matching Swift.

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::event::{AgentEvent, ProviderId, ProviderStream};

/// A parsed OpenCode SSE event object.
pub type OpenCodeEvent = Map<String, Value>;

// ---------------------------------------------------------------------------
// SSE line parser
// ---------------------------------------------------------------------------

/// Swift `OpenCodeEventStreamParser.maxEventDataBytes` (1 MiB).
const MAX_EVENT_DATA_BYTES: usize = 1024 * 1024;

/// Frames raw Server-Sent-Events lines into JSON event objects.
///
/// Faithful port of Swift `OpenCodeEventStreamParser`: `data:` lines are
/// accumulated (with the leading single space stripped), a blank line dispatches
/// the joined payload as one JSON object, `\r` is trimmed from each line, and a
/// data payload exceeding [`MAX_EVENT_DATA_BYTES`] resets the pending buffer.
#[derive(Debug, Default)]
pub struct OpenCodeEventStreamParser {
    data_lines: Vec<String>,
    data_byte_count: usize,
}

impl OpenCodeEventStreamParser {
    /// Create an empty parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume one raw SSE line, dispatching the pending event on a blank line.
    ///
    /// Mirrors Swift `consumeLine`: the line is `\r`-trimmed; a blank line
    /// flushes; only `data:`-prefixed lines are accumulated (the optional single
    /// space after the colon is removed); anything else is ignored. Non-blank
    /// lines never dispatch, so the return is empty except on the blank-line
    /// flush.
    pub fn consume_line(&mut self, line: &str) -> Vec<OpenCodeEvent> {
        let line = line.trim_matches('\r');
        if line.is_empty() {
            return self.flush();
        }
        let Some(rest) = line.strip_prefix("data:") else {
            return Vec::new();
        };
        let data = rest.strip_prefix(' ').unwrap_or(rest);
        let separator_bytes = if self.data_lines.is_empty() { 0 } else { 1 };
        self.data_byte_count += data.len() + separator_bytes;
        if self.data_byte_count > MAX_EVENT_DATA_BYTES {
            self.reset();
            return Vec::new();
        }
        self.data_lines.push(data.to_string());
        Vec::new()
    }

    /// Dispatch any pending data as a single JSON event object.
    ///
    /// Mirrors Swift `flush`: joins the accumulated `data:` lines with `\n`,
    /// resets, and returns the decoded JSON **object** (an empty vec when there
    /// is nothing pending or the payload is not a JSON object).
    pub fn flush(&mut self) -> Vec<OpenCodeEvent> {
        if self.data_lines.is_empty() {
            return Vec::new();
        }
        let data = self.data_lines.join("\n");
        self.reset();
        match serde_json::from_str::<Value>(&data) {
            Ok(Value::Object(object)) => vec![object],
            _ => Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.data_lines.clear();
        self.data_byte_count = 0;
    }
}

// ---------------------------------------------------------------------------
// Text accumulator
// ---------------------------------------------------------------------------

/// Swift `OpenCodeEventTextAccumulator.maxTrackedMessages`.
const MAX_TRACKED_MESSAGES: usize = 16;
/// Swift `OpenCodeEventTextAccumulator.maxTrackedPartTextCharacters` (256 KiB).
const MAX_TRACKED_PART_TEXT_CHARACTERS: usize = 256 * 1024;

/// Count of Unicode scalar values — the port's stand-in for Swift `String.count`
/// (see the module-level divergence note).
fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// Drop the first `n` scalar values of `text` (port of Swift `dropFirst(n)`).
fn drop_first_chars(text: &str, n: usize) -> String {
    text.chars().skip(n).collect()
}

/// Keep the last `n` scalar values of `text` (port of Swift `suffix(n)`).
fn suffix_chars(text: &str, n: usize) -> String {
    let count = char_count(text);
    if count <= n {
        return text.to_string();
    }
    drop_first_chars(text, count - n)
}

/// Stateful accumulator over parsed OpenCode SSE events.
///
/// Faithful port of Swift `OpenCodeEventTextAccumulator`. Feed parsed events to
/// [`consume_event`](Self::consume_event) (raw assistant-text deltas) or
/// [`consume_event_to_events`](Self::consume_event_to_events) (renderer events).
#[derive(Debug, Default)]
pub struct OpenCodeEventTextAccumulator {
    message_role_by_id: HashMap<String, String>,
    message_id_order: Vec<String>,
    message_id_by_part_id: HashMap<String, String>,
    is_text_part_by_id: HashMap<String, bool>,
    text_by_part_id: HashMap<String, String>,
    stored_text_start_offset_by_part_id: HashMap<String, usize>,
    emitted_character_count_by_part_id: HashMap<String, usize>,
}

impl OpenCodeEventTextAccumulator {
    /// Create an empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Total retained part-text scalar count (test/introspection helper).
    ///
    /// Mirrors Swift `retainedTextCharacterCountForTesting`.
    pub fn retained_text_character_count(&self) -> usize {
        self.text_by_part_id.values().map(|t| char_count(t)).sum()
    }

    /// Consume one parsed event, returning the assistant text delta(s) it yields.
    ///
    /// Faithful port of Swift `consumeEvent`: the event is ignored unless it has
    /// a string `type`, an object `properties`, and a session id matching
    /// `session_id`. Only `message.updated`, `message.part.updated`, and
    /// `message.part.delta` produce text.
    pub fn consume_event(&mut self, event: &OpenCodeEvent, session_id: &str) -> Vec<String> {
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            return Vec::new();
        };
        let Some(properties) = event.get("properties").and_then(Value::as_object) else {
            return Vec::new();
        };
        if event_session_id(properties).as_deref() != Some(session_id) {
            return Vec::new();
        }
        match kind {
            "message.updated" => self.consume_message_updated(properties),
            "message.part.updated" => self.consume_part_updated(properties),
            "message.part.delta" => self.consume_part_delta(properties),
            _ => Vec::new(),
        }
    }

    /// Consume one parsed event and map its output to renderer [`AgentEvent`]s.
    ///
    /// Mirrors `AgentSessionProcessStore.handleOpenCodeEvent`: the turn-completion
    /// check runs first (on the raw event), then each delta becomes an
    /// [`AgentEvent::ProviderOutput`] on `stdout`, and finally a
    /// [`AgentEvent::ProviderTurnComplete`] is appended when the event completes
    /// the turn.
    /// `session_id` is the OpenCode loopback session id used to *match* incoming
    /// events; `emit_session_id` is the cmux agent session id stamped onto the
    /// emitted renderer events (the id the frontend routes on). These differ for
    /// OpenCode — the loopback id (`ses_…`) is an internal detail and must never
    /// leak into the renderer, or the reply is delivered to a session the UI does
    /// not know about and silently dropped.
    pub fn consume_event_to_events(
        &mut self,
        event: &OpenCodeEvent,
        session_id: &str,
        emit_session_id: &str,
        provider_id: ProviderId,
    ) -> Vec<AgentEvent> {
        let completes_turn = Self::completes_assistant_turn(event, session_id);
        let mut events = Vec::new();
        for delta in self.consume_event(event, session_id) {
            events.push(AgentEvent::ProviderOutput {
                session_id: emit_session_id.to_string(),
                provider_id,
                stream: ProviderStream::Stdout,
                text: delta,
            });
        }
        if completes_turn {
            events.push(AgentEvent::ProviderTurnComplete {
                session_id: emit_session_id.to_string(),
                provider_id,
            });
        }
        events
    }

    /// Whether `event` completes the assistant turn for `session_id`.
    ///
    /// Faithful port of the static Swift `completesAssistantTurn(_:sessionID:)`.
    pub fn completes_assistant_turn(event: &OpenCodeEvent, session_id: &str) -> bool {
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            return false;
        };
        let Some(properties) = event.get("properties").and_then(Value::as_object) else {
            return false;
        };
        if event_session_id(properties).as_deref() != Some(session_id) {
            return false;
        }
        match kind {
            "session.idle" => true,
            "session.status" => session_status_is_idle(properties.get("status")),
            "message.updated" => {
                let empty = Map::new();
                let info = message_info(properties, &empty);
                if first_string(&[info.get("role"), properties.get("role")]).as_deref()
                    != Some("assistant")
                {
                    return false;
                }
                message_info_has_completed_time(info)
                    || first_string(&[
                        info.get("finish"),
                        info.get("finishedReason"),
                        properties.get("finish"),
                    ])
                    .is_some()
                    || info.get("error").is_some()
            }
            _ => false,
        }
    }

    fn consume_message_updated(&mut self, properties: &Map<String, Value>) -> Vec<String> {
        let empty = Map::new();
        let info = message_info(properties, &empty);
        let Some(message_id) = first_string(&[
            info.get("id"),
            properties.get("messageID"),
            properties.get("messageId"),
        ]) else {
            return Vec::new();
        };
        let Some(role) = first_string(&[info.get("role"), properties.get("role")]) else {
            return Vec::new();
        };

        self.remember_message_id(&message_id);
        self.message_role_by_id
            .insert(message_id.clone(), role.clone());
        if role != "assistant" {
            return Vec::new();
        }
        let part_ids: Vec<String> = self
            .message_id_by_part_id
            .iter()
            .filter(|(_, candidate)| *candidate == &message_id)
            .map(|(part_id, _)| part_id.clone())
            .collect();
        let mut output = Vec::new();
        for part_id in part_ids {
            output.extend(self.flush_part(&part_id));
        }
        if message_info_has_completed_time(info)
            || first_string(&[
                info.get("finish"),
                info.get("finishedReason"),
                properties.get("finish"),
            ])
            .is_some()
            || info.get("error").is_some()
        {
            self.prune_message(&message_id);
        }
        output
    }

    fn consume_part_updated(&mut self, properties: &Map<String, Value>) -> Vec<String> {
        let Some(part) = properties.get("part").and_then(Value::as_object) else {
            return Vec::new();
        };
        let Some(part_id) = part.get("id").and_then(Value::as_str) else {
            return Vec::new();
        };
        let Some(message_id) = part.get("messageID").and_then(Value::as_str) else {
            return Vec::new();
        };
        let part_id = part_id.to_string();
        let message_id = message_id.to_string();

        self.message_id_by_part_id
            .insert(part_id.clone(), message_id.clone());
        self.remember_message_id(&message_id);
        if part.get("type").and_then(Value::as_str) != Some("text")
            || part.get("ignored").and_then(Value::as_bool) == Some(true)
        {
            self.prune_part(&part_id);
            return Vec::new();
        }

        self.is_text_part_by_id.insert(part_id.clone(), true);
        let Some(text) =
            first_content_string(&[part.get("text"), part.get("textDelta"), part.get("content")])
        else {
            return Vec::new();
        };

        if char_count(&text) >= self.source_character_count(&part_id) {
            self.store_bounded_text(&text, 0, &part_id);
        }
        self.flush_full_text(&text, &part_id)
    }

    fn consume_part_delta(&mut self, properties: &Map<String, Value>) -> Vec<String> {
        if properties.get("field").and_then(Value::as_str) != Some("text") {
            return Vec::new();
        }
        let Some(part_id) = properties.get("partID").and_then(Value::as_str) else {
            return Vec::new();
        };
        let Some(message_id) = properties.get("messageID").and_then(Value::as_str) else {
            return Vec::new();
        };
        let Some(delta) = properties.get("delta").and_then(Value::as_str) else {
            return Vec::new();
        };
        if delta.is_empty() {
            return Vec::new();
        }
        let part_id = part_id.to_string();
        let message_id = message_id.to_string();

        self.message_id_by_part_id
            .insert(part_id.clone(), message_id.clone());
        self.remember_message_id(&message_id);
        if self.is_text_part_by_id.get(&part_id) == Some(&true)
            && self.message_role_by_id.get(&message_id).map(String::as_str) == Some("assistant")
        {
            *self
                .emitted_character_count_by_part_id
                .entry(part_id)
                .or_insert(0) += char_count(delta);
            return vec![delta.to_string()];
        }
        let existing = self
            .text_by_part_id
            .get(&part_id)
            .cloned()
            .unwrap_or_default();
        let combined = format!("{existing}{delta}");
        let start_offset = self
            .stored_text_start_offset_by_part_id
            .get(&part_id)
            .copied()
            .unwrap_or(0);
        self.store_bounded_text(&combined, start_offset, &part_id);
        self.flush_part(&part_id)
    }

    fn flush_full_text(&mut self, text: &str, part_id: &str) -> Vec<String> {
        if self.is_text_part_by_id.get(part_id) != Some(&true) {
            return Vec::new();
        }
        let Some(message_id) = self.message_id_by_part_id.get(part_id) else {
            return Vec::new();
        };
        if self.message_role_by_id.get(message_id).map(String::as_str) != Some("assistant") {
            return Vec::new();
        }
        if text.is_empty() {
            return Vec::new();
        }
        let emitted = self
            .emitted_character_count_by_part_id
            .get(part_id)
            .copied()
            .unwrap_or(0);
        let full = char_count(text);
        if full <= emitted {
            return Vec::new();
        }
        self.emitted_character_count_by_part_id
            .insert(part_id.to_string(), full);
        vec![drop_first_chars(text, emitted)]
    }

    fn flush_part(&mut self, part_id: &str) -> Vec<String> {
        if self.is_text_part_by_id.get(part_id) != Some(&true) {
            return Vec::new();
        }
        let Some(message_id) = self.message_id_by_part_id.get(part_id) else {
            return Vec::new();
        };
        if self.message_role_by_id.get(message_id).map(String::as_str) != Some("assistant") {
            return Vec::new();
        }
        let Some(text) = self.text_by_part_id.get(part_id) else {
            return Vec::new();
        };
        if text.is_empty() {
            return Vec::new();
        }
        let emitted = self
            .emitted_character_count_by_part_id
            .get(part_id)
            .copied()
            .unwrap_or(0);
        let stored_start = self
            .stored_text_start_offset_by_part_id
            .get(part_id)
            .copied()
            .unwrap_or(0);
        let stored_end = stored_start + char_count(text);
        if stored_end <= emitted {
            return Vec::new();
        }
        // Swift `max(0, emittedCharacterCount - storedStartOffset)`.
        let relative_start = emitted.saturating_sub(stored_start);
        if relative_start >= char_count(text) {
            return Vec::new();
        }
        let out = drop_first_chars(text, relative_start);
        self.emitted_character_count_by_part_id
            .insert(part_id.to_string(), stored_end);
        vec![out]
    }

    fn source_character_count(&self, part_id: &str) -> usize {
        let emitted = self
            .emitted_character_count_by_part_id
            .get(part_id)
            .copied()
            .unwrap_or(0);
        let stored_start = self
            .stored_text_start_offset_by_part_id
            .get(part_id)
            .copied()
            .unwrap_or(0);
        let stored_len = self
            .text_by_part_id
            .get(part_id)
            .map(|t| char_count(t))
            .unwrap_or(0);
        emitted.max(stored_start + stored_len)
    }

    fn store_bounded_text(&mut self, text: &str, source_start_offset: usize, part_id: &str) {
        let (bounded, offset) = bounded_stored_text(text, source_start_offset);
        self.text_by_part_id.insert(part_id.to_string(), bounded);
        self.stored_text_start_offset_by_part_id
            .insert(part_id.to_string(), offset);
    }

    fn remember_message_id(&mut self, message_id: &str) {
        if !self.message_id_order.iter().any(|m| m == message_id) {
            self.message_id_order.push(message_id.to_string());
        }
        while self.message_id_order.len() > MAX_TRACKED_MESSAGES {
            let oldest = self.message_id_order[0].clone();
            self.prune_message(&oldest);
        }
    }

    fn prune_message(&mut self, message_id: &str) {
        self.message_role_by_id.remove(message_id);
        self.message_id_order.retain(|m| m != message_id);
        let part_ids: Vec<String> = self
            .message_id_by_part_id
            .iter()
            .filter(|(_, candidate)| *candidate == &message_id.to_string())
            .map(|(part_id, _)| part_id.clone())
            .collect();
        for part_id in part_ids {
            self.prune_part(&part_id);
        }
    }

    fn prune_part(&mut self, part_id: &str) {
        self.message_id_by_part_id.remove(part_id);
        self.is_text_part_by_id.remove(part_id);
        self.text_by_part_id.remove(part_id);
        self.stored_text_start_offset_by_part_id.remove(part_id);
        self.emitted_character_count_by_part_id.remove(part_id);
    }
}

/// Swift `boundedStoredText`: keep only the trailing
/// [`MAX_TRACKED_PART_TEXT_CHARACTERS`] scalars, advancing the start offset.
fn bounded_stored_text(text: &str, source_start_offset: usize) -> (String, usize) {
    let count = char_count(text);
    if count <= MAX_TRACKED_PART_TEXT_CHARACTERS {
        return (text.to_string(), source_start_offset);
    }
    let dropped = count - MAX_TRACKED_PART_TEXT_CHARACTERS;
    (
        suffix_chars(text, MAX_TRACKED_PART_TEXT_CHARACTERS),
        source_start_offset + dropped,
    )
}

/// Swift `info = properties["info"] ?? properties["message"] ?? [:]`.
fn message_info<'a>(
    properties: &'a Map<String, Value>,
    empty: &'a Map<String, Value>,
) -> &'a Map<String, Value> {
    properties
        .get("info")
        .and_then(Value::as_object)
        .or_else(|| properties.get("message").and_then(Value::as_object))
        .unwrap_or(empty)
}

/// Swift `firstString`: first value that is a non-empty (after trim) string,
/// returned trimmed.
fn first_string(values: &[Option<&Value>]) -> Option<String> {
    for value in values {
        if let Some(s) = value.and_then(|v| v.as_str()) {
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Swift `firstContentString`: first non-empty string; else the first empty
/// string encountered; else `None`. Values that are not strings are skipped.
fn first_content_string(values: &[Option<&Value>]) -> Option<String> {
    let mut empty: Option<String> = None;
    for value in values {
        if let Some(s) = value.and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
            if empty.is_none() {
                empty = Some(s.to_string());
            }
        }
    }
    empty
}

/// Swift `eventSessionID`: probe the direct + nested session-id keys.
fn event_session_id(properties: &Map<String, Value>) -> Option<String> {
    let nested = |key: &str, nested_key: &str| -> Option<&Value> {
        properties
            .get(key)
            .and_then(Value::as_object)
            .and_then(|o| o.get(nested_key))
    };
    first_string(&[
        properties.get("sessionID"),
        properties.get("sessionId"),
        properties.get("session_id"),
        nested("info", "sessionID"),
        nested("info", "sessionId"),
        nested("info", "session_id"),
        nested("message", "sessionID"),
        nested("message", "sessionId"),
        nested("message", "session_id"),
        nested("part", "sessionID"),
        nested("part", "sessionId"),
        nested("part", "session_id"),
    ])
}

/// Swift `sessionStatusIsIdle`.
fn session_status_is_idle(value: Option<&Value>) -> bool {
    if let Some(string) = first_string(&[value]) {
        return string == "idle";
    }
    let Some(status) = value.and_then(Value::as_object) else {
        return false;
    };
    first_string(&[
        status.get("type"),
        status.get("status"),
        status.get("state"),
    ])
    .as_deref()
        == Some("idle")
}

/// Swift `messageInfoHasCompletedTime`.
fn message_info_has_completed_time(info: &Map<String, Value>) -> bool {
    let Some(time) = info.get("time").and_then(Value::as_object) else {
        return false;
    };
    time.get("completed").is_some()
        || time.get("completedAt").is_some()
        || time.get("end").is_some()
        || time.get("ended").is_some()
}

// ---------------------------------------------------------------------------
// stdout disposition sniff
// ---------------------------------------------------------------------------

/// How a raw OpenCode stdout/stderr line should be handled.
///
/// Port of the Swift `OpenCodeProcessOutputDisposition` enum (the associated
/// value is the raw captured URL string — see the module-level divergence note).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenCodeProcessOutputDisposition {
    /// Forward the line to the renderer as normal output.
    Emit,
    /// Drop the line (opencode stdout noise once the server URL is known).
    Suppress,
    /// The line announced the loopback server URL; capture it (raw string).
    ServerUrl(String),
}

/// Classify one OpenCode process output line.
///
/// Faithful port of Swift `openCodeProcessOutputDisposition(text:stream:)`: a
/// loopback server-URL announcement is captured on either stream; otherwise a
/// `stdout` line is suppressed and a `stderr` line is emitted.
pub fn opencode_process_output_disposition(
    text: &str,
    stream: ProviderStream,
) -> OpenCodeProcessOutputDisposition {
    if let Some(url) = opencode_server_url(text) {
        return OpenCodeProcessOutputDisposition::ServerUrl(url);
    }
    match stream {
        ProviderStream::Stdout => OpenCodeProcessOutputDisposition::Suppress,
        // INTENTIONAL DIVERGENCE from canonical macOS (which emits ALL stderr):
        // opencode >= 1.17 routes its `--print-logs` structured diagnostics to
        // stderr (`timestamp=… level=INFO … message=…`), which would flood the
        // transcript and bury the assistant reply. We pass `--print-logs` only so
        // the loopback URL is announced (captured above), so suppress the INFO/
        // DEBUG log noise while still surfacing WARN/ERROR + any unstructured
        // stderr (real failures). The assistant reply arrives over the `/event`
        // SSE stream, not this path, so it is unaffected.
        ProviderStream::Stderr if is_opencode_server_log_noise(text) => {
            OpenCodeProcessOutputDisposition::Suppress
        }
        ProviderStream::Stderr => OpenCodeProcessOutputDisposition::Emit,
    }
}

/// Whether a stderr line is one of opencode's structured `--print-logs` INFO/DEBUG
/// diagnostics (noise), as opposed to a genuine error worth surfacing.
///
/// opencode's structured logs are `key=value` records carrying both a
/// `level=INFO`/`level=DEBUG` token and a `message=` field. Matching on that shape
/// (rather than any line containing "INFO") avoids suppressing real error output,
/// which is either unstructured or carries `level=WARN`/`level=ERROR`.
fn is_opencode_server_log_noise(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.contains("level=INFO") || trimmed.contains("level=DEBUG"))
        && trimmed.contains("message=")
}

/// Swift `openCodeServerURL(from:)`: extract the announced loopback URL, if any.
fn opencode_server_url(text: &str) -> Option<String> {
    const MARKER: &str = "opencode server listening on ";
    let index = text.find(MARKER)?;
    let after = &text[index + MARKER.len()..];
    let trimmed = after.trim();
    // Swift `split(separator: " ")` omits empty subsequences; take the first.
    let candidate = trimmed.split(' ').find(|s| !s.is_empty())?;
    if is_loopback_url(candidate) {
        Some(candidate.to_string())
    } else {
        None
    }
}

/// Swift `agentSessionIsLoopbackURL`: true when the URL host is a loopback host.
fn is_loopback_url(url: &str) -> bool {
    matches!(
        url_host(url).as_deref(),
        Some("localhost" | "127.0.0.1" | "::1")
    )
}

/// Extract the lowercased host from an absolute `scheme://` URL string.
///
/// Returns `None` for a URL without a `scheme://authority` (parity with Swift
/// `URL(string:)` producing a `nil` `host`).
fn url_host(url: &str) -> Option<String> {
    let (_scheme, rest) = url.split_once("://")?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, hp)| hp);
    if let Some(after_bracket) = host_port.strip_prefix('[') {
        let close = after_bracket.find(']')?;
        return Some(after_bracket[..close].to_lowercase());
    }
    let host = host_port.split(':').next().unwrap_or(host_port);
    if host.is_empty() {
        None
    } else {
        Some(host.to_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(value: Value) -> OpenCodeEvent {
        value.as_object().unwrap().clone()
    }

    // ---- SSE parser: framing ----

    #[test]
    fn single_data_line_dispatches_on_blank_line() {
        let mut p = OpenCodeEventStreamParser::new();
        assert!(p
            .consume_line(r#"data: {"type":"session.idle","properties":{}}"#)
            .is_empty());
        let events = p.consume_line("");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], json!("session.idle"));
    }

    #[test]
    fn leading_space_after_colon_is_stripped_only_once() {
        // "data:  {..}" -> one space removed, payload still has a leading space,
        // which JSON tolerates.
        let mut p = OpenCodeEventStreamParser::new();
        p.consume_line(r#"data:  {"type":"x","properties":{}}"#);
        let events = p.consume_line("");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], json!("x"));
    }

    #[test]
    fn data_without_leading_space_is_accepted() {
        let mut p = OpenCodeEventStreamParser::new();
        p.consume_line(r#"data:{"type":"y","properties":{}}"#);
        let events = p.consume_line("");
        assert_eq!(events[0]["type"], json!("y"));
    }

    #[test]
    fn multiple_data_lines_join_with_newline() {
        let mut p = OpenCodeEventStreamParser::new();
        p.consume_line(r#"data: {"type":"z","#);
        p.consume_line(r#"data: "properties":{}}"#);
        let events = p.consume_line("");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], json!("z"));
    }

    #[test]
    fn non_data_lines_are_ignored() {
        let mut p = OpenCodeEventStreamParser::new();
        assert!(p.consume_line("event: message").is_empty());
        assert!(p.consume_line(": comment").is_empty());
        assert!(p.consume_line("id: 42").is_empty());
        // Nothing pending -> blank line yields nothing.
        assert!(p.consume_line("").is_empty());
    }

    #[test]
    fn carriage_returns_are_trimmed() {
        let mut p = OpenCodeEventStreamParser::new();
        p.consume_line("data: {\"type\":\"cr\",\"properties\":{}}\r");
        // A bare CR line trims to empty -> flush.
        let events = p.consume_line("\r");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], json!("cr"));
    }

    #[test]
    fn partial_event_completes_across_chunks() {
        // Simulate three separate reader chunks before the dispatch blank line.
        let mut p = OpenCodeEventStreamParser::new();
        assert!(p
            .consume_line(r#"data: {"type":"message.part.delta","#)
            .is_empty());
        assert!(p
            .consume_line(r#"data: "properties":{"field":"text","#)
            .is_empty());
        assert!(p.consume_line(r#"data: "delta":"hi"}}"#).is_empty());
        let events = p.consume_line("");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], json!("message.part.delta"));
        assert_eq!(events[0]["properties"]["delta"], json!("hi"));
    }

    #[test]
    fn flush_of_empty_parser_is_empty() {
        let mut p = OpenCodeEventStreamParser::new();
        assert!(p.flush().is_empty());
    }

    #[test]
    fn explicit_flush_dispatches_pending_without_blank_line() {
        let mut p = OpenCodeEventStreamParser::new();
        p.consume_line(r#"data: {"type":"tail","properties":{}}"#);
        let events = p.flush();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], json!("tail"));
        // Buffer was reset.
        assert!(p.flush().is_empty());
    }

    #[test]
    fn malformed_json_payload_yields_no_event() {
        let mut p = OpenCodeEventStreamParser::new();
        p.consume_line("data: not json");
        assert!(p.consume_line("").is_empty());
    }

    #[test]
    fn non_object_json_payload_yields_no_event() {
        let mut p = OpenCodeEventStreamParser::new();
        p.consume_line("data: [1,2,3]");
        assert!(p.consume_line("").is_empty());
    }

    #[test]
    fn oversized_data_resets_and_parser_recovers() {
        let mut p = OpenCodeEventStreamParser::new();
        // One data line over the 1 MiB cap forces a reset (returns nothing).
        let huge = "x".repeat(MAX_EVENT_DATA_BYTES + 10);
        assert!(p.consume_line(&format!("data: {huge}")).is_empty());
        // Nothing pending after reset.
        assert!(p.consume_line("").is_empty());
        // A fresh valid event still parses.
        p.consume_line(r#"data: {"type":"ok","properties":{}}"#);
        let events = p.consume_line("");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], json!("ok"));
    }

    // ---- accumulator: session filtering ----

    #[test]
    fn event_for_other_session_is_ignored() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        let event = obj(json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": "other",
                "field": "text",
                "partID": "p1",
                "messageID": "m1",
                "delta": "hi"
            }
        }));
        assert!(acc.consume_event(&event, "sess").is_empty());
    }

    #[test]
    fn event_missing_type_or_properties_is_ignored() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        assert!(acc
            .consume_event(&obj(json!({"properties": {"sessionID": "s"}})), "s")
            .is_empty());
        assert!(acc
            .consume_event(&obj(json!({"type": "message.updated"})), "s")
            .is_empty());
    }

    // ---- accumulator: streaming deltas ----

    fn message_updated_assistant(session: &str, message_id: &str) -> OpenCodeEvent {
        obj(json!({
            "type": "message.updated",
            "properties": {
                "sessionID": session,
                "info": {"id": message_id, "role": "assistant"}
            }
        }))
    }

    fn part_updated_text(
        session: &str,
        part_id: &str,
        message_id: &str,
        text: &str,
    ) -> OpenCodeEvent {
        obj(json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": session,
                "part": {"id": part_id, "messageID": message_id, "type": "text", "text": text}
            }
        }))
    }

    fn part_delta(session: &str, part_id: &str, message_id: &str, delta: &str) -> OpenCodeEvent {
        obj(json!({
            "type": "message.part.delta",
            "properties": {
                "sessionID": session,
                "field": "text",
                "partID": part_id,
                "messageID": message_id,
                "delta": delta
            }
        }))
    }

    #[test]
    fn streaming_deltas_emit_each_delta() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        acc.consume_event(&part_updated_text("s", "p1", "m1", ""), "s");
        assert_eq!(
            acc.consume_event(&part_delta("s", "p1", "m1", "Hel"), "s"),
            vec!["Hel"]
        );
        assert_eq!(
            acc.consume_event(&part_delta("s", "p1", "m1", "lo"), "s"),
            vec!["lo"]
        );
    }

    #[test]
    fn delta_before_text_part_known_is_buffered_not_emitted_twice() {
        // Without a prior text part.updated + assistant message.updated, a delta
        // takes the buffered path; flushPart still requires the text-part/assistant
        // guards, so nothing is emitted until those are known.
        let mut acc = OpenCodeEventTextAccumulator::new();
        assert!(acc
            .consume_event(&part_delta("s", "p1", "m1", "ignored"), "s")
            .is_empty());
    }

    #[test]
    fn empty_delta_yields_nothing() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        acc.consume_event(&part_updated_text("s", "p1", "m1", ""), "s");
        assert!(acc
            .consume_event(&part_delta("s", "p1", "m1", ""), "s")
            .is_empty());
    }

    // ---- accumulator: full-text (part.updated) de-dup ----

    #[test]
    fn part_updated_full_text_without_deltas_emits_full_text() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        let out = acc.consume_event(&part_updated_text("s", "p1", "m1", "Complete answer"), "s");
        assert_eq!(out, vec!["Complete answer"]);
    }

    #[test]
    fn part_updated_full_text_emits_only_uncovered_tail() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        assert_eq!(
            acc.consume_event(&part_updated_text("s", "p1", "m1", "Hello"), "s"),
            vec!["Hello"]
        );
        // Re-send grows the text; only the new tail is emitted.
        assert_eq!(
            acc.consume_event(&part_updated_text("s", "p1", "m1", "Hello, world"), "s"),
            vec![", world"]
        );
    }

    #[test]
    fn part_updated_full_text_fully_covered_yields_nothing() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        acc.consume_event(&part_delta("s", "p1", "m1", ""), "s"); // no-op empty
        acc.consume_event(&part_updated_text("s", "p1", "m1", "Hi"), "s");
        // Same text again -> nothing new.
        assert!(acc
            .consume_event(&part_updated_text("s", "p1", "m1", "Hi"), "s")
            .is_empty());
    }

    #[test]
    fn non_text_part_is_pruned_and_silent() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        let tool = obj(json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "s",
                "part": {"id": "p1", "messageID": "m1", "type": "tool"}
            }
        }));
        assert!(acc.consume_event(&tool, "s").is_empty());
        // A subsequent delta for the pruned part is not on the fast path.
        assert!(acc
            .consume_event(&part_delta("s", "p1", "m1", "x"), "s")
            .is_empty());
    }

    #[test]
    fn ignored_text_part_is_pruned() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        let ignored = obj(json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "s",
                "part": {"id": "p1", "messageID": "m1", "type": "text", "ignored": true, "text": "nope"}
            }
        }));
        assert!(acc.consume_event(&ignored, "s").is_empty());
    }

    #[test]
    fn message_updated_flushes_buffered_part_text() {
        // Deltas arrive before we know the message role; a later assistant
        // message.updated flushes the buffered part text.
        let mut acc = OpenCodeEventTextAccumulator::new();
        // part.updated establishes text-part but role unknown yet.
        acc.consume_event(&part_updated_text("s", "p1", "m1", ""), "s");
        // delta buffers (fast path blocked: role unknown).
        assert!(acc
            .consume_event(&part_delta("s", "p1", "m1", "buffered"), "s")
            .is_empty());
        // Now the assistant message.updated arrives and flushes.
        let out = acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        assert_eq!(out, vec!["buffered"]);
    }

    // ---- turn completion ----

    #[test]
    fn session_idle_completes_turn() {
        let event = obj(json!({"type": "session.idle", "properties": {"sessionID": "s"}}));
        assert!(OpenCodeEventTextAccumulator::completes_assistant_turn(
            &event, "s"
        ));
        assert!(!OpenCodeEventTextAccumulator::completes_assistant_turn(
            &event, "other"
        ));
    }

    #[test]
    fn session_status_idle_string_and_object() {
        let string_status = obj(json!({
            "type": "session.status",
            "properties": {"sessionID": "s", "status": "idle"}
        }));
        assert!(OpenCodeEventTextAccumulator::completes_assistant_turn(
            &string_status,
            "s"
        ));

        let object_status = obj(json!({
            "type": "session.status",
            "properties": {"sessionID": "s", "status": {"type": "idle"}}
        }));
        assert!(OpenCodeEventTextAccumulator::completes_assistant_turn(
            &object_status,
            "s"
        ));

        let running = obj(json!({
            "type": "session.status",
            "properties": {"sessionID": "s", "status": "running"}
        }));
        assert!(!OpenCodeEventTextAccumulator::completes_assistant_turn(
            &running, "s"
        ));
    }

    #[test]
    fn message_updated_completes_turn_on_completed_time_finish_or_error() {
        let completed = obj(json!({
            "type": "message.updated",
            "properties": {"sessionID": "s", "info": {"role": "assistant", "time": {"completed": 1}}}
        }));
        assert!(OpenCodeEventTextAccumulator::completes_assistant_turn(
            &completed, "s"
        ));

        let finished = obj(json!({
            "type": "message.updated",
            "properties": {"sessionID": "s", "info": {"role": "assistant", "finish": "stop"}}
        }));
        assert!(OpenCodeEventTextAccumulator::completes_assistant_turn(
            &finished, "s"
        ));

        let errored = obj(json!({
            "type": "message.updated",
            "properties": {"sessionID": "s", "info": {"role": "assistant", "error": {"m": 1}}}
        }));
        assert!(OpenCodeEventTextAccumulator::completes_assistant_turn(
            &errored, "s"
        ));
    }

    #[test]
    fn message_updated_non_assistant_does_not_complete() {
        let user = obj(json!({
            "type": "message.updated",
            "properties": {"sessionID": "s", "info": {"role": "user", "time": {"completed": 1}}}
        }));
        assert!(!OpenCodeEventTextAccumulator::completes_assistant_turn(
            &user, "s"
        ));
    }

    #[test]
    fn in_progress_assistant_message_does_not_complete() {
        let running = obj(json!({
            "type": "message.updated",
            "properties": {"sessionID": "s", "info": {"role": "assistant", "time": {"created": 1}}}
        }));
        assert!(!OpenCodeEventTextAccumulator::completes_assistant_turn(
            &running, "s"
        ));
    }

    // ---- event mapping ----

    #[test]
    fn consume_event_to_events_maps_delta_to_stdout_output() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        acc.consume_event(&part_updated_text("s", "p1", "m1", ""), "s");
        // Match on the OpenCode loopback id "s"; emit under the distinct cmux id.
        let out = acc.consume_event_to_events(
            &part_delta("s", "p1", "m1", "chunk"),
            "s",
            "cmux-session",
            ProviderId::Opencode,
        );
        assert_eq!(
            out,
            vec![AgentEvent::ProviderOutput {
                session_id: "cmux-session".into(),
                provider_id: ProviderId::Opencode,
                stream: ProviderStream::Stdout,
                text: "chunk".into(),
            }]
        );
    }

    #[test]
    fn consume_event_to_events_appends_turn_complete() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        let idle = obj(json!({"type": "session.idle", "properties": {"sessionID": "s"}}));
        let out = acc.consume_event_to_events(&idle, "s", "cmux-session", ProviderId::Opencode);
        assert_eq!(
            out,
            vec![AgentEvent::ProviderTurnComplete {
                session_id: "cmux-session".into(),
                provider_id: ProviderId::Opencode,
            }]
        );
    }

    #[test]
    fn full_streaming_turn_flow() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        acc.consume_event(&message_updated_assistant("s", "m1"), "s");
        acc.consume_event(&part_updated_text("s", "p1", "m1", ""), "s");
        let mut collected = String::new();
        let mut completed = false;
        let events = [
            part_delta("s", "p1", "m1", "The "),
            part_delta("s", "p1", "m1", "answer "),
            part_delta("s", "p1", "m1", "is 42."),
            obj(json!({"type": "session.idle", "properties": {"sessionID": "s"}})),
        ];
        for event in &events {
            for ev in acc.consume_event_to_events(event, "s", "cmux-session", ProviderId::Opencode)
            {
                match ev {
                    AgentEvent::ProviderOutput { text, .. } => collected.push_str(&text),
                    AgentEvent::ProviderTurnComplete { .. } => completed = true,
                    other => panic!("unexpected event {other:?}"),
                }
            }
        }
        assert_eq!(collected, "The answer is 42.");
        assert!(completed);
    }

    // ---- message-id tracking cap ----

    #[test]
    fn tracked_message_ids_are_capped() {
        let mut acc = OpenCodeEventTextAccumulator::new();
        for i in 0..(MAX_TRACKED_MESSAGES + 8) {
            acc.consume_event(&message_updated_assistant("s", &format!("m{i}")), "s");
        }
        assert!(acc.message_id_order.len() <= MAX_TRACKED_MESSAGES);
        assert!(acc.message_role_by_id.len() <= MAX_TRACKED_MESSAGES);
    }

    #[test]
    fn nested_session_id_lookup_matches() {
        // sessionID lives under info, not at the top of properties.
        let mut acc = OpenCodeEventTextAccumulator::new();
        let event = obj(json!({
            "type": "message.updated",
            "properties": {"info": {"sessionID": "s", "id": "m1", "role": "assistant"}}
        }));
        // Consumed (no panic, role stored) -> a following part flushes text.
        assert!(acc.consume_event(&event, "s").is_empty());
        acc.consume_event(&part_updated_text("s", "p1", "m1", "hi"), "s");
        // Confirm the message role was recorded via nested session id.
        assert_eq!(
            acc.message_role_by_id.get("m1").map(String::as_str),
            Some("assistant")
        );
    }

    // ---- stdout disposition ----

    #[test]
    fn server_url_captured_on_stdout_and_stderr() {
        for stream in [ProviderStream::Stdout, ProviderStream::Stderr] {
            let line = "opencode server listening on http://127.0.0.1:4096\n";
            assert_eq!(
                opencode_process_output_disposition(line, stream),
                OpenCodeProcessOutputDisposition::ServerUrl("http://127.0.0.1:4096".into())
            );
        }
    }

    #[test]
    fn server_url_localhost_and_ipv6_loopback() {
        assert_eq!(
            opencode_process_output_disposition(
                "opencode server listening on http://localhost:1234",
                ProviderStream::Stdout
            ),
            OpenCodeProcessOutputDisposition::ServerUrl("http://localhost:1234".into())
        );
        assert_eq!(
            opencode_process_output_disposition(
                "opencode server listening on http://[::1]:1234",
                ProviderStream::Stdout
            ),
            OpenCodeProcessOutputDisposition::ServerUrl("http://[::1]:1234".into())
        );
    }

    #[test]
    fn non_loopback_server_url_is_not_captured() {
        // A non-loopback host falls through to the stream default.
        assert_eq!(
            opencode_process_output_disposition(
                "opencode server listening on http://example.com:80",
                ProviderStream::Stdout
            ),
            OpenCodeProcessOutputDisposition::Suppress
        );
        assert_eq!(
            opencode_process_output_disposition(
                "opencode server listening on http://10.0.0.5:80",
                ProviderStream::Stderr
            ),
            OpenCodeProcessOutputDisposition::Emit
        );
    }

    #[test]
    fn plain_stdout_is_suppressed_and_stderr_emitted() {
        assert_eq!(
            opencode_process_output_disposition("some log line", ProviderStream::Stdout),
            OpenCodeProcessOutputDisposition::Suppress
        );
        assert_eq!(
            opencode_process_output_disposition("some log line", ProviderStream::Stderr),
            OpenCodeProcessOutputDisposition::Emit
        );
    }

    #[test]
    fn structured_info_debug_server_logs_are_suppressed_on_stderr() {
        for line in [
            r#"timestamp=2026-07-02T15:38:01.593Z level=INFO run=d8c76528 message="loop session.id=ses_x step=0""#,
            "timestamp=2026-07-02T15:33:59.516Z level=INFO run=d8c76528 message=init",
            "level=DEBUG message=\"tracking hash=abc\"",
        ] {
            assert_eq!(
                opencode_process_output_disposition(line, ProviderStream::Stderr),
                OpenCodeProcessOutputDisposition::Suppress,
                "should suppress: {line}"
            );
        }
    }

    #[test]
    fn genuine_errors_and_warnings_still_emit() {
        for line in [
            "level=ERROR message=\"failed to reach provider\"",
            "level=WARN message=\"deprecated flag\"",
            "panic: runtime error: nil pointer",
            "Error: connection refused",
        ] {
            assert_eq!(
                opencode_process_output_disposition(line, ProviderStream::Stderr),
                OpenCodeProcessOutputDisposition::Emit,
                "should emit: {line}"
            );
        }
    }

    #[test]
    fn server_url_check_precedes_the_noise_filter() {
        // A clean announcement on stderr must still be captured as ServerUrl (the
        // URL check runs before the INFO/DEBUG suppression), not dropped as noise.
        assert!(matches!(
            opencode_process_output_disposition(
                "opencode server listening on http://127.0.0.1:4096",
                ProviderStream::Stderr,
            ),
            OpenCodeProcessOutputDisposition::ServerUrl(_)
        ));
    }

    #[test]
    fn marker_takes_first_token_and_ignores_trailing_words() {
        assert_eq!(
            opencode_process_output_disposition(
                "opencode server listening on http://127.0.0.1:9 (press ctrl-c)",
                ProviderStream::Stdout
            ),
            OpenCodeProcessOutputDisposition::ServerUrl("http://127.0.0.1:9".into())
        );
    }

    #[test]
    fn missing_scheme_url_is_not_loopback() {
        assert_eq!(
            opencode_process_output_disposition(
                "opencode server listening on 127.0.0.1:4096",
                ProviderStream::Stdout
            ),
            OpenCodeProcessOutputDisposition::Suppress
        );
    }

    #[test]
    fn url_host_extraction() {
        assert_eq!(
            url_host("http://127.0.0.1:4096").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            url_host("http://Localhost:80/path").as_deref(),
            Some("localhost")
        );
        assert_eq!(
            url_host("http://user:pw@127.0.0.1:5/x").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(url_host("http://[::1]:5").as_deref(), Some("::1"));
        assert_eq!(url_host("127.0.0.1:4096"), None);
        assert_eq!(url_host("http:///path"), None);
    }
}
