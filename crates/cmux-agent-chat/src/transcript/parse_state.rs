//! Carry-over state and the result of one incremental transcript parse call.
//!
//! Ports `Parsing/ChatTranscriptParseState.swift` and
//! `Parsing/ChatTranscriptParseResult.swift`.
//!
//! Agent transcripts are tailed: a tool invocation and its result can land in
//! different parse calls. [`ChatTranscriptParseState`] carries the registry of
//! tool invocations still awaiting a result so a later call can pair them,
//! plus the last-seen timestamp for lines that omit one.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::model::ChatMessage;
use super::timestamp::Timestamp;

/// Carry-over state between incremental transcript parse calls.
///
/// `pending_tool_uses` is keyed by the transcript's tool-call identifier
/// (`tool_use_id` for Claude, `call_id` for Codex); each value is the
/// already-emitted message(s) in running form. Codable snake_case is
/// preserved (`pending_tool_uses`, `last_timestamp`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatTranscriptParseState {
    /// Tool invocations awaiting a result, keyed by tool call identifier.
    #[serde(rename = "pending_tool_uses", default)]
    pub pending_tool_uses: HashMap<String, Vec<ChatMessage>>,

    /// Timestamp of the last line that carried one, the fallback for
    /// subsequent lines that omit a timestamp.
    #[serde(
        rename = "last_timestamp",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub last_timestamp: Option<Timestamp>,
}

impl ChatTranscriptParseState {
    /// Creates empty parse carry-over state.
    pub fn new() -> Self {
        ChatTranscriptParseState::default()
    }
}

/// The outcome of one incremental transcript parse call.
///
/// Seq assignment: every message parsed from the JSONL line at offset `n` of
/// the input gets `seq == starting_seq + n`. When one line yields several
/// messages (a Claude assistant line with multiple content blocks) they all
/// share that line's seq and are disambiguated by id suffixes (`uuid`,
/// `uuid#1`, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct ChatTranscriptParseResult {
    /// Messages newly produced by this parse call, in transcript order.
    pub messages: Vec<ChatMessage>,

    /// Completed re-emissions of messages from *earlier* parse calls whose
    /// tool result arrived in this call. Each carries the original id and seq.
    pub updated_messages: Vec<ChatMessage>,

    /// Carry-over state to pass into the next parse call.
    pub state: ChatTranscriptParseState,
}

impl ChatTranscriptParseResult {
    /// Creates a parse result.
    pub fn new(
        messages: Vec<ChatMessage>,
        updated_messages: Vec<ChatMessage>,
        state: ChatTranscriptParseState,
    ) -> Self {
        ChatTranscriptParseResult {
            messages,
            updated_messages,
            state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::model::{ChatMessageKind, ChatProse, ChatRole};

    #[test]
    fn state_round_trips_snake_case() {
        let mut pending = HashMap::new();
        pending.insert(
            "call-1".to_string(),
            vec![ChatMessage::new(
                "m1",
                3,
                ChatRole::Agent,
                Timestamp::from_millis(1000),
                ChatMessageKind::Prose(ChatProse::new("hi")),
            )],
        );
        let state = ChatTranscriptParseState {
            pending_tool_uses: pending,
            last_timestamp: Some(Timestamp::from_millis(2000)),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("pending_tool_uses"));
        assert!(json.contains("last_timestamp"));
        let decoded: ChatTranscriptParseState = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, state);
    }

    #[test]
    fn empty_state_omits_last_timestamp() {
        let json = serde_json::to_string(&ChatTranscriptParseState::new()).unwrap();
        assert!(!json.contains("last_timestamp"));
    }
}
