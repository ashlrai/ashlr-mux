//! Claude `--output-format stream-json` accumulator + input framing.
//!
//! Ported **faithfully** from the canonical macOS Swift sources:
//!
//! * accumulator — `Sources/Panels/ClaudeStreamJSONAccumulator.swift`
//!   (struct `ClaudeStreamJSONAccumulator`).
//! * input framing — `writeClaudeStreamJSON(_:to:)` in
//!   `Sources/Panels/AgentSessionProcessStore.swift`.
//! * event mapping — the `stream == "stdout" && providerID == .claude` branch of
//!   `AgentSessionProcessStore.handleProcessOutput` (deltas → `provider.output`
//!   on `stdout`, then a `provider.turnComplete` when the line completes a turn).
//!
//! The renderer-side shapes this feeds are validated against
//! `webviews/src/agent-session/shared/types.ts` (`AgentEvent`).
//!
//! ## Field / behaviour parity
//!
//! Same field handling and turn-completion detection as the Swift source:
//! `message_start` (assistant) sets the current message id; `content_block_delta`
//! yields raw text deltas; a full `assistant` message is de-duplicated against
//! already-emitted characters (per message id, plus a pending-delta fallback);
//! a bare `result` string is emitted once per turn if no assistant text streamed;
//! and `result` / `message_stop` / `done` complete the turn (resetting tracking).
//!
//! ## Known divergence (intentional, documented)
//!
//! Swift `String.count` / `dropFirst` count **extended grapheme clusters**; this
//! port counts **Unicode scalar values** (`str::chars`). The two agree for all
//! ASCII / BMP-without-combining text (the overwhelmingly common Claude output).
//! They can differ only for grapheme clusters made of multiple scalars (e.g. a
//! base char plus a combining mark, or a ZWJ emoji sequence) *and* only when a
//! full `assistant` message must be diffed against prior streamed deltas — an
//! edge case. Internal consistency (all lengths use the same unit) means the
//! prefix-drop for the normal "deltas then final full message" flow is exact.
//! Pulling in a grapheme-segmentation dependency was deliberately avoided to keep
//! the crate dependency-free beyond serde.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::Value;

use crate::event::{AgentEvent, ProviderId, ProviderStream};

/// Matches the Swift `maxTrackedMessages` cap.
const MAX_TRACKED_MESSAGES: usize = 16;

/// Count of Unicode scalar values — the port's stand-in for Swift `String.count`
/// (see the module-level divergence note).
fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// Serde model for one framed user-input line (see [`write_claude_stream_json`]).
///
/// Field declaration order fixes the emitted key order (`type`, `message`) so the
/// output is deterministic; the Claude CLI parses JSON so order is not load-bearing.
#[derive(Serialize)]
struct ClaudeUserMessageLine<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    message: ClaudeUserMessage<'a>,
}

#[derive(Serialize)]
struct ClaudeUserMessage<'a> {
    role: &'static str,
    content: [ClaudeTextBlock<'a>; 1],
}

#[derive(Serialize)]
struct ClaudeTextBlock<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    text: &'a str,
}

/// Build the newline-terminated `stream-json` user-input line for `text`.
///
/// Verbatim shape from Swift `writeClaudeStreamJSON`:
/// `{"type":"user","message":{"role":"user","content":[{"type":"text","text":<text>}]}}\n`.
pub fn write_claude_stream_json(text: &str) -> String {
    let line = ClaudeUserMessageLine {
        kind: "user",
        message: ClaudeUserMessage {
            role: "user",
            content: [ClaudeTextBlock {
                kind: "text",
                text,
            }],
        },
    };
    // Serialization of these plain owned/borrowed types is infallible.
    let mut serialized = serde_json::to_string(&line).expect("serialize claude user-input line");
    serialized.push('\n');
    serialized
}

/// Stateful accumulator over Claude `stream-json` stdout lines.
///
/// Faithful port of Swift `ClaudeStreamJSONAccumulator`. Feed complete lines to
/// [`consume_line`](Self::consume_line) (raw text deltas) or
/// [`consume_line_to_events`](Self::consume_line_to_events) (renderer events).
#[derive(Debug, Default)]
pub struct ClaudeStreamAccumulator {
    emitted_char_count_by_message_id: HashMap<String, usize>,
    message_id_order: Vec<String>,
    current_message_id: Option<String>,
    pending_delta_char_count: usize,
    emitted_any_assistant_text: bool,
}

impl ClaudeStreamAccumulator {
    /// Create an empty accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume one line, returning the assistant text delta(s) it produced.
    ///
    /// Mirrors Swift `consumeLine`: malformed / non-object / empty lines yield an
    /// empty vec; `message_start` primes the current message id; deltas and
    /// de-duplicated full `assistant` messages return their new text; a one-shot
    /// `result` string is emitted when no assistant text streamed; turn-completing
    /// lines reset tracking. At most one string is ever returned (parity with the
    /// Swift `[String]` that only ever holds zero or one element).
    pub fn consume_line(&mut self, line: &str) -> Vec<String> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Vec::new();
        }
        let object = match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Object(map)) => unwrap_stream_event(map),
            _ => return Vec::new(),
        };

        if let Some(message_id) = assistant_message_id_from_message_start(&object) {
            self.remember_message_id(&message_id);
            self.current_message_id = Some(message_id);
            self.pending_delta_char_count = 0;
            return Vec::new();
        }

        if let Some(delta) = self.assistant_text_delta(&object) {
            if !delta.is_empty() {
                self.emitted_any_assistant_text = true;
                match self.current_message_id.clone() {
                    Some(current) => {
                        self.remember_message_id(&current);
                        *self
                            .emitted_char_count_by_message_id
                            .entry(current)
                            .or_insert(0) += char_count(&delta);
                    }
                    None => self.pending_delta_char_count += char_count(&delta),
                }
                return vec![delta];
            }
        }

        if !self.emitted_any_assistant_text
            && object.get("type").and_then(Value::as_str) == Some("result")
        {
            if let Some(result) = object.get("result").and_then(Value::as_str) {
                if !result.is_empty() {
                    self.emitted_any_assistant_text = true;
                    self.reset_turn_tracking();
                    return vec![result.to_string()];
                }
            }
        }

        if completes_assistant_turn_from_object(&object) {
            self.reset_turn_tracking();
        }
        Vec::new()
    }

    /// Consume one line and map its output to renderer [`AgentEvent`]s.
    ///
    /// Mirrors the `AgentSessionProcessStore` claude/stdout branch exactly: the
    /// turn-completion check is evaluated on the raw line *first*, then each delta
    /// becomes a [`AgentEvent::ProviderOutput`] on `stdout`, and finally a
    /// [`AgentEvent::ProviderTurnComplete`] is appended when the line completes a
    /// turn.
    pub fn consume_line_to_events(
        &mut self,
        line: &str,
        session_id: &str,
        provider_id: ProviderId,
    ) -> Vec<AgentEvent> {
        let completes_turn = Self::completes_assistant_turn(line);
        let mut events = Vec::new();
        for delta in self.consume_line(line) {
            events.push(AgentEvent::ProviderOutput {
                session_id: session_id.to_string(),
                provider_id,
                stream: ProviderStream::Stdout,
                text: delta,
            });
        }
        if completes_turn {
            events.push(AgentEvent::ProviderTurnComplete {
                session_id: session_id.to_string(),
                provider_id,
            });
        }
        events
    }

    /// Whether `line` (raw, pre-trim) completes the assistant turn.
    ///
    /// Faithful port of the static Swift `completesAssistantTurn(_:)`: requires a
    /// JSON object with a string `type` of `result`, `message_stop`, or `done`.
    pub fn completes_assistant_turn(line: &str) -> bool {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return false;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(Value::Object(map)) => {
                let map = unwrap_stream_event(map);
                completes_assistant_turn_type(map.get("type").and_then(Value::as_str))
            }
            _ => false,
        }
    }

    fn assistant_text_delta(&mut self, object: &serde_json::Map<String, Value>) -> Option<String> {
        if object.get("type").and_then(Value::as_str) == Some("content_block_delta") {
            if let Some(delta) = object.get("delta").and_then(Value::as_object) {
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    return Some(text.to_string());
                }
            }
        }

        if object.get("type").and_then(Value::as_str) != Some("assistant") {
            return None;
        }
        let message = object
            .get("message")
            .and_then(Value::as_object)
            .unwrap_or(object);
        let full_text = content_text(message.get("content"));
        if full_text.is_empty() {
            return None;
        }

        let message_id = message
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("assistant")
            .to_string();
        self.remember_message_id(&message_id);
        let full_count = char_count(&full_text);
        let previous_char_count = self
            .emitted_char_count_by_message_id
            .get(&message_id)
            .copied()
            .unwrap_or_else(|| self.pending_delta_char_count.min(full_count));
        self.emitted_char_count_by_message_id
            .insert(message_id.clone(), full_count);
        if self.current_message_id.as_deref() == Some(message_id.as_str()) {
            self.current_message_id = None;
        }
        self.pending_delta_char_count = 0;
        if previous_char_count > 0 && full_count >= previous_char_count {
            return Some(full_text.chars().skip(previous_char_count).collect());
        }
        Some(full_text)
    }

    fn remember_message_id(&mut self, message_id: &str) {
        if !self.message_id_order.iter().any(|m| m == message_id) {
            self.message_id_order.push(message_id.to_string());
        }
        while self.message_id_order.len() > MAX_TRACKED_MESSAGES {
            let removed = self.message_id_order.remove(0);
            self.emitted_char_count_by_message_id.remove(&removed);
        }
    }

    fn reset_turn_tracking(&mut self) {
        self.emitted_char_count_by_message_id.clear();
        self.message_id_order.clear();
        self.current_message_id = None;
        self.pending_delta_char_count = 0;
    }
}

/// Unwrap the Claude CLI's `stream_event` envelope.
///
/// With `--include-partial-messages`, the current `claude` CLI wraps each raw
/// Anthropic streaming event in `{"type":"stream_event","event":{…}}` — e.g.
/// `{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":
/// "text_delta","text":"…"}}}`. The inner `event` object is exactly the shape the
/// accumulator's `message_start` / `content_block_delta` / `message_stop`
/// handling expects, so we substitute it. Non-enveloped lines (the top-level
/// `assistant` full message, `result`) pass through unchanged. Without this, the
/// wrapped deltas are ignored and only the final full `assistant` message emits —
/// i.e. the whole reply arrives at once instead of streaming.
fn unwrap_stream_event(
    object: serde_json::Map<String, Value>,
) -> serde_json::Map<String, Value> {
    if object.get("type").and_then(Value::as_str) == Some("stream_event") {
        if let Some(event) = object.get("event").and_then(Value::as_object) {
            return event.clone();
        }
    }
    object
}

fn completes_assistant_turn_type(type_field: Option<&str>) -> bool {
    matches!(type_field, Some("result" | "message_stop" | "done"))
}

fn completes_assistant_turn_from_object(object: &serde_json::Map<String, Value>) -> bool {
    completes_assistant_turn_type(object.get("type").and_then(Value::as_str))
}

fn assistant_message_id_from_message_start(
    object: &serde_json::Map<String, Value>,
) -> Option<String> {
    if object.get("type").and_then(Value::as_str) != Some("message_start") {
        return None;
    }
    let message = object.get("message").and_then(Value::as_object)?;
    if message.get("role").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let message_id = message.get("id").and_then(Value::as_str)?;
    if message_id.is_empty() {
        return None;
    }
    Some(message_id.to_string())
}

/// Flatten Claude message `content` (string, single block, or array) to its text.
///
/// Faithful port of Swift `contentText(from:)`: a string is returned as-is; a
/// single block object returns its `text` (empty when its `type` is present and
/// not `"text"`); an array joins the flattened parts; anything else yields `""`.
fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(part)) => {
            if let Some(type_str) = part.get("type").and_then(Value::as_str) {
                if type_str != "text" {
                    return String::new();
                }
            }
            part.get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        }
        Some(Value::Array(parts)) => parts.iter().map(|p| content_text(Some(p))).collect(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- input framing ----

    #[test]
    fn write_claude_stream_json_shape_and_newline() {
        let line = write_claude_stream_json("hello world");
        assert!(line.ends_with('\n'), "must be newline-terminated");
        assert_eq!(line.matches('\n').count(), 1, "exactly one trailing newline");
        let parsed: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(
            parsed,
            json!({
                "type": "user",
                "message": {
                    "role": "user",
                    "content": [{ "type": "text", "text": "hello world" }]
                }
            })
        );
    }

    #[test]
    fn write_claude_stream_json_escapes_special_chars() {
        let line = write_claude_stream_json("quote \" and \n newline");
        let parsed: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(parsed["message"]["content"][0]["text"], "quote \" and \n newline");
    }

    #[test]
    fn write_claude_stream_json_empty_text() {
        let line = write_claude_stream_json("");
        let parsed: Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(parsed["message"]["content"][0]["text"], "");
    }

    // ---- consume_line: deltas ----

    #[test]
    fn content_block_delta_returns_text() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out = acc.consume_line(
            r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hel"}}"#,
        );
        assert_eq!(out, vec!["Hel".to_string()]);
        let out = acc.consume_line(
            r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"lo"}}"#,
        );
        assert_eq!(out, vec!["lo".to_string()]);
    }

    #[test]
    fn empty_delta_text_yields_nothing() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out =
            acc.consume_line(r#"{"type":"content_block_delta","delta":{"text":""}}"#);
        assert!(out.is_empty());
    }

    #[test]
    fn content_block_delta_without_text_yields_nothing() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out = acc.consume_line(r#"{"type":"content_block_delta","delta":{"foo":"bar"}}"#);
        assert!(out.is_empty());
    }

    // ---- consume_line: full assistant message de-dup ----

    #[test]
    fn full_assistant_message_dedups_after_deltas() {
        let mut acc = ClaudeStreamAccumulator::new();
        acc.consume_line(r#"{"type":"message_start","message":{"id":"m1","role":"assistant"}}"#);
        assert_eq!(
            acc.consume_line(
                r#"{"type":"content_block_delta","delta":{"text":"Hello, "}}"#
            ),
            vec!["Hello, ".to_string()]
        );
        assert_eq!(
            acc.consume_line(r#"{"type":"content_block_delta","delta":{"text":"world"}}"#),
            vec!["world".to_string()]
        );
        // The final full message repeats the whole text; only the un-emitted tail
        // should be returned. Here deltas already covered all of it -> empty.
        let out = acc.consume_line(
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Hello, world"}]}}"#,
        );
        assert!(out.is_empty(), "fully-covered text yields no new tail, got {out:?}");
    }

    #[test]
    fn full_assistant_message_emits_uncovered_tail() {
        let mut acc = ClaudeStreamAccumulator::new();
        acc.consume_line(r#"{"type":"message_start","message":{"id":"m1","role":"assistant"}}"#);
        acc.consume_line(r#"{"type":"content_block_delta","delta":{"text":"Hello"}}"#);
        // Final message has more than was streamed.
        let out = acc.consume_line(
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Hello, world"}]}}"#,
        );
        assert_eq!(out, vec![", world".to_string()]);
    }

    #[test]
    fn full_assistant_message_without_prior_deltas_emits_full_text() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out = acc.consume_line(
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"Complete answer"}]}}"#,
        );
        assert_eq!(out, vec!["Complete answer".to_string()]);
    }

    #[test]
    fn assistant_content_as_plain_string() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out =
            acc.consume_line(r#"{"type":"assistant","message":{"id":"m1","content":"raw string"}}"#);
        assert_eq!(out, vec!["raw string".to_string()]);
    }

    #[test]
    fn assistant_content_array_joins_text_blocks_and_skips_non_text() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out = acc.consume_line(
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"a"},{"type":"tool_use","name":"x"},{"type":"text","text":"b"}]}}"#,
        );
        assert_eq!(out, vec!["ab".to_string()]);
    }

    // ---- consume_line: result one-shot ----

    #[test]
    fn result_string_emitted_once_when_no_assistant_text() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out = acc.consume_line(r#"{"type":"result","result":"final text"}"#);
        assert_eq!(out, vec!["final text".to_string()]);
    }

    #[test]
    fn result_string_suppressed_after_assistant_text() {
        let mut acc = ClaudeStreamAccumulator::new();
        assert_eq!(
            acc.consume_line(r#"{"type":"content_block_delta","delta":{"text":"streamed"}}"#),
            vec!["streamed".to_string()]
        );
        let out = acc.consume_line(r#"{"type":"result","result":"streamed"}"#);
        assert!(out.is_empty(), "result must not double-emit after streamed text");
    }

    #[test]
    fn empty_result_string_yields_nothing() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out = acc.consume_line(r#"{"type":"result","result":""}"#);
        assert!(out.is_empty());
    }

    // ---- turn completion ----

    #[test]
    fn completes_assistant_turn_types() {
        for ty in ["result", "message_stop", "done"] {
            let line = format!(r#"{{"type":"{ty}"}}"#);
            assert!(
                ClaudeStreamAccumulator::completes_assistant_turn(&line),
                "{ty} should complete turn"
            );
        }
    }

    #[test]
    fn non_completing_types_and_malformed_do_not_complete() {
        for line in [
            r#"{"type":"content_block_delta","delta":{"text":"x"}}"#,
            r#"{"type":"message_start"}"#,
            r#"{"no_type":true}"#,
            "not json",
            "",
            "   ",
            "[1,2,3]",
        ] {
            assert!(
                !ClaudeStreamAccumulator::completes_assistant_turn(line),
                "{line:?} should not complete turn"
            );
        }
    }

    #[test]
    fn message_stop_resets_dedup_tracking() {
        let mut acc = ClaudeStreamAccumulator::new();
        acc.consume_line(r#"{"type":"content_block_delta","delta":{"text":"turn one"}}"#);
        assert!(acc.consume_line(r#"{"type":"message_stop"}"#).is_empty());
        // After reset, emitted_any_assistant_text is NOT cleared by reset, but a
        // fresh result in a new turn: emitted_any_assistant_text stays true, so a
        // subsequent result would be suppressed. Verify tracking state reset lets
        // a new full message with same id re-emit from scratch.
        let out = acc.consume_line(
            r#"{"type":"assistant","message":{"id":"turnOne","content":[{"type":"text","text":"again"}]}}"#,
        );
        assert_eq!(out, vec!["again".to_string()]);
    }

    // ---- malformed tolerance ----

    #[test]
    fn malformed_and_non_object_lines_tolerated() {
        let mut acc = ClaudeStreamAccumulator::new();
        for line in ["", "   ", "not json", "[1,2,3]", "42", "\"a string\"", "null"] {
            assert!(acc.consume_line(line).is_empty(), "{line:?} should be inert");
        }
    }

    #[test]
    fn line_with_surrounding_whitespace_is_trimmed() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out = acc.consume_line("  {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"hi\"}}  \n");
        assert_eq!(out, vec!["hi".to_string()]);
    }

    // ---- event mapping ----

    #[test]
    fn consume_line_to_events_emits_output_then_turn_complete() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out = acc.consume_line_to_events(
            r#"{"type":"content_block_delta","delta":{"text":"chunk"}}"#,
            "sess-1",
            ProviderId::Claude,
        );
        assert_eq!(
            out,
            vec![AgentEvent::ProviderOutput {
                session_id: "sess-1".into(),
                provider_id: ProviderId::Claude,
                stream: ProviderStream::Stdout,
                text: "chunk".into(),
            }]
        );

        // A result line: emits the one-shot output AND completes the turn.
        let out = acc.consume_line_to_events(
            r#"{"type":"result","result":"chunk"}"#,
            "sess-1",
            ProviderId::Claude,
        );
        // Assistant text already streamed -> result suppressed, only turnComplete.
        assert_eq!(
            out,
            vec![AgentEvent::ProviderTurnComplete {
                session_id: "sess-1".into(),
                provider_id: ProviderId::Claude,
            }]
        );
    }

    #[test]
    fn consume_line_to_events_result_emits_output_and_turn_complete() {
        let mut acc = ClaudeStreamAccumulator::new();
        let out = acc.consume_line_to_events(
            r#"{"type":"result","result":"the answer"}"#,
            "sess-9",
            ProviderId::Claude,
        );
        assert_eq!(out.len(), 2, "output delta + turnComplete");
        assert_eq!(
            out[0],
            AgentEvent::ProviderOutput {
                session_id: "sess-9".into(),
                provider_id: ProviderId::Claude,
                stream: ProviderStream::Stdout,
                text: "the answer".into(),
            }
        );
        assert_eq!(
            out[1],
            AgentEvent::ProviderTurnComplete {
                session_id: "sess-9".into(),
                provider_id: ProviderId::Claude,
            }
        );
    }

    #[test]
    fn consume_line_to_events_inert_line_yields_no_events() {
        let mut acc = ClaudeStreamAccumulator::new();
        assert!(acc
            .consume_line_to_events("garbage", "s", ProviderId::Claude)
            .is_empty());
    }

    // ---- message id tracking cap ----

    #[test]
    fn tracked_message_ids_are_capped() {
        let mut acc = ClaudeStreamAccumulator::new();
        // Emit full assistant messages for many distinct ids.
        for i in 0..(MAX_TRACKED_MESSAGES + 8) {
            let line = format!(
                r#"{{"type":"assistant","message":{{"id":"m{i}","content":[{{"type":"text","text":"x"}}]}}}}"#
            );
            let _ = acc.consume_line(&line);
        }
        assert!(acc.message_id_order.len() <= MAX_TRACKED_MESSAGES);
        assert!(acc.emitted_char_count_by_message_id.len() <= MAX_TRACKED_MESSAGES);
    }

    // ---- stream_event envelope (real claude CLI shape) ----

    #[test]
    fn stream_event_wrapped_deltas_stream_incrementally() {
        // The exact shapes the current `claude --include-partial-messages` CLI
        // emits: partial events wrapped in {"type":"stream_event","event":{…}},
        // then an unwrapped top-level full `assistant` message, then `result`.
        // Deltas must stream one-by-one; the final full message must de-dup to
        // nothing; the result must not double-emit.
        let mut acc = ClaudeStreamAccumulator::new();
        let lines = [
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"msg_1","role":"assistant","content":[]}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello there,"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" fellow human friend."}}}"#,
            r#"{"type":"assistant","message":{"id":"msg_1","content":[{"type":"text","text":"Hello there, fellow human friend."}]}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#,
            r#"{"type":"stream_event","event":{"type":"message_stop"}}"#,
            r#"{"type":"result","subtype":"success","result":"Hello there, fellow human friend."}"#,
        ];
        let mut outputs: Vec<String> = Vec::new();
        let mut completed = false;
        for line in lines {
            for ev in acc.consume_line_to_events(line, "s", ProviderId::Claude) {
                match ev {
                    AgentEvent::ProviderOutput { text, .. } => outputs.push(text),
                    AgentEvent::ProviderTurnComplete { .. } => completed = true,
                    other => panic!("unexpected event {other:?}"),
                }
            }
        }
        // Two incremental deltas — NOT one lump — and the full message + result
        // added nothing on top.
        assert_eq!(
            outputs,
            vec!["Hello there,".to_string(), " fellow human friend.".to_string()]
        );
        assert_eq!(outputs.concat(), "Hello there, fellow human friend.");
        assert!(completed, "the turn must complete (result / message_stop)");
    }

    #[test]
    fn wrapped_message_stop_completes_turn() {
        assert!(ClaudeStreamAccumulator::completes_assistant_turn(
            r#"{"type":"stream_event","event":{"type":"message_stop"}}"#
        ));
    }

    // ---- full streaming turn integration ----

    #[test]
    fn full_streaming_turn_flow() {
        let mut acc = ClaudeStreamAccumulator::new();
        let lines = [
            r#"{"type":"message_start","message":{"id":"msg_1","role":"assistant"}}"#,
            r#"{"type":"content_block_start","index":0}"#,
            r#"{"type":"content_block_delta","delta":{"text":"The "}}"#,
            r#"{"type":"content_block_delta","delta":{"text":"answer "}}"#,
            r#"{"type":"content_block_delta","delta":{"text":"is 42."}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"message_stop"}"#,
        ];
        let mut collected = String::new();
        let mut completed = false;
        for line in lines {
            for ev in acc.consume_line_to_events(line, "s", ProviderId::Claude) {
                match ev {
                    AgentEvent::ProviderOutput { text, .. } => collected.push_str(&text),
                    AgentEvent::ProviderTurnComplete { .. } => completed = true,
                    other => panic!("unexpected event {other:?}"),
                }
            }
        }
        assert_eq!(collected, "The answer is 42.");
        assert!(completed, "message_stop should complete the turn");
    }
}
