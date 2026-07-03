//! Accumulates the messages of one parse call and routes tool results.
//!
//! Ports `Parsing/TranscriptBatchAssembler.swift`. In-batch messages are
//! completed in place; messages from earlier calls are re-emitted as updates.

use std::collections::HashMap;

use super::model::ChatMessage;
use super::parse_state::{ChatTranscriptParseResult, ChatTranscriptParseState};
use super::text_budget::TranscriptTextBudget;
use super::timestamp::Timestamp;
use super::tool_completion::TranscriptToolCompletion;

/// Upper bound on tool invocations carried across parse calls awaiting a
/// result. Capping to the most-recent N (by seq) bounds the carried state;
/// dropping the oldest unresolved calls only means an extremely-late result
/// (>N tool calls later) won't back-patch.
pub const MAX_PENDING_TOOL_USES: usize = 256;

/// Accumulates the messages of one parse call.
pub struct TranscriptBatchAssembler {
    messages: Vec<ChatMessage>,
    updated_messages: Vec<ChatMessage>,
    pending: HashMap<String, Vec<ChatMessage>>,
    batch_index_by_message_id: HashMap<String, usize>,
    budget: TranscriptTextBudget,
}

impl TranscriptBatchAssembler {
    /// Creates an assembler seeded with carried-over pending tool uses.
    pub fn new(state: ChatTranscriptParseState, budget: TranscriptTextBudget) -> Self {
        TranscriptBatchAssembler {
            messages: Vec::new(),
            updated_messages: Vec::new(),
            pending: state.pending_tool_uses,
            batch_index_by_message_id: HashMap::new(),
            budget,
        }
    }

    /// Appends a newly parsed message, optionally registering it as a tool
    /// invocation awaiting its result.
    ///
    /// A single tool call can register multiple messages (a multi-question
    /// `AskUserQuestion` emits one card per question); its result must resolve
    /// all of them, so group by call id.
    pub fn append(&mut self, message: ChatMessage, pending_key: Option<&str>) {
        if let Some(key) = pending_key {
            self.batch_index_by_message_id
                .insert(message.id.clone(), self.messages.len());
            self.pending
                .entry(key.to_string())
                .or_default()
                .push(message.clone());
        }
        self.messages.push(message);
    }

    /// Pairs a tool result with its pending invocation, if registered.
    ///
    /// Applies to every message registered under this call id. For questions,
    /// `completion.applied` resolves each by its own prompt, so multi-question
    /// cards each get their correct answer.
    pub fn resolve(&mut self, key: &str, completion: &TranscriptToolCompletion) {
        let Some(pending_messages) = self.pending.remove(key) else {
            return;
        };
        for pending_message in &pending_messages {
            let Some(completed) = completion.applied(pending_message, &self.budget) else {
                continue;
            };
            if let Some(&index) = self.batch_index_by_message_id.get(&completed.id) {
                self.messages[index] = completed;
            } else {
                self.updated_messages.push(completed);
            }
        }
    }

    /// Finalizes the batch into a parse result.
    pub fn result(self, last_timestamp: Option<Timestamp>) -> ChatTranscriptParseResult {
        ChatTranscriptParseResult::new(
            self.messages,
            self.updated_messages,
            ChatTranscriptParseState {
                pending_tool_uses: Self::bounded(self.pending),
                last_timestamp,
            },
        )
    }

    /// Caps carried pending tool uses to the most-recent [`MAX_PENDING_TOOL_USES`]
    /// by their newest message seq, evicting the oldest unresolved calls.
    fn bounded(
        pending: HashMap<String, Vec<ChatMessage>>,
    ) -> HashMap<String, Vec<ChatMessage>> {
        if pending.len() <= MAX_PENDING_TOOL_USES {
            return pending;
        }
        let mut entries: Vec<(String, Vec<ChatMessage>)> = pending.into_iter().collect();
        // Newest first by the group's highest seq. The key is a stable
        // secondary sort so eviction is deterministic on seq ties (the Swift
        // seqs are unique in practice, so this only firms up determinism).
        entries.sort_by(|lhs, rhs| {
            let lhs_seq = lhs.1.iter().map(|m| m.seq).max().unwrap_or(0);
            let rhs_seq = rhs.1.iter().map(|m| m.seq).max().unwrap_or(0);
            rhs_seq.cmp(&lhs_seq).then_with(|| lhs.0.cmp(&rhs.0))
        });
        entries.truncate(MAX_PENDING_TOOL_USES);
        entries.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::model::{ChatMessageKind, ChatRole, ChatToolUse};

    fn tool_use(seq: i64) -> ChatMessage {
        ChatMessage::new(
            format!("m{seq}"),
            seq,
            ChatRole::Agent,
            Timestamp::from_millis(1_781_000_000_000 + seq),
            ChatMessageKind::ToolUse(ChatToolUse::running("Read", format!("s{seq}"), None)),
        )
    }

    #[test]
    fn pending_tool_uses_bounded_to_newest() {
        let mut assembler =
            TranscriptBatchAssembler::new(ChatTranscriptParseState::new(), TranscriptTextBudget::new());
        let total = MAX_PENDING_TOOL_USES + 50;
        for i in 0..total {
            assembler.append(tool_use(i as i64), Some(&format!("call-{i}")));
        }
        let state = assembler.result(None).state;
        assert_eq!(state.pending_tool_uses.len(), MAX_PENDING_TOOL_USES);
        assert!(state.pending_tool_uses.contains_key(&format!("call-{}", total - 1)));
        assert!(!state.pending_tool_uses.contains_key("call-0"));
    }

    #[test]
    fn pending_under_cap_all_retained() {
        let mut assembler =
            TranscriptBatchAssembler::new(ChatTranscriptParseState::new(), TranscriptTextBudget::new());
        for i in 0..10 {
            assembler.append(tool_use(i), Some(&format!("call-{i}")));
        }
        let state = assembler.result(None).state;
        assert_eq!(state.pending_tool_uses.len(), 10);
    }
}
