//! A tool result observed in the transcript, applied to the pending
//! running-state message its invocation produced.
//!
//! Ports `Parsing/TranscriptToolCompletion.swift`.

use super::model::{
    ChatMessage, ChatMessageKind, ChatQuestion, ChatTerminalCapture, ChatToolUse, ChatToolUseStatus,
};
use super::text_budget::TranscriptTextBudget;

/// A tool result observed in the transcript.
#[derive(Debug, Clone)]
pub struct TranscriptToolCompletion {
    /// The result text, already extracted from the transcript shape.
    pub output: Option<String>,
    /// Whether the transcript flagged the result as an error.
    pub is_error: bool,
    /// The exit code, when one was parseable from the result.
    pub exit_code: Option<i64>,
    /// Wall-clock duration in seconds, when one was parseable.
    pub duration_seconds: Option<f64>,
}

impl TranscriptToolCompletion {
    /// Creates a completion.
    pub fn new(
        output: Option<String>,
        is_error: bool,
        exit_code: Option<i64>,
        duration_seconds: Option<f64>,
    ) -> Self {
        TranscriptToolCompletion {
            output,
            is_error,
            exit_code,
            duration_seconds,
        }
    }

    /// Produces the completed copy of a pending tool message.
    ///
    /// Returns `None` when the result does not change how the message renders
    /// (file edits, unanswered questions), mirroring the Swift `applied(to:)`.
    pub fn applied(
        &self,
        message: &ChatMessage,
        budget: &TranscriptTextBudget,
    ) -> Option<ChatMessage> {
        match &message.kind {
            ChatMessageKind::Terminal(capture) => {
                let completed = ChatTerminalCapture {
                    command: capture.command.clone(),
                    output: self.output.as_deref().map(|text| budget.body(text)),
                    exit_code: Some(self.exit_code.unwrap_or(if self.is_error { 1 } else { 0 })),
                    duration_seconds: self.duration_seconds,
                    is_running: false,
                };
                Some(message.replacing_kind(ChatMessageKind::Terminal(completed)))
            }
            ChatMessageKind::ToolUse(tool_use) => {
                let failed = self.is_error || self.exit_code.unwrap_or(0) != 0;
                let completed = ChatToolUse {
                    tool_name: tool_use.tool_name.clone(),
                    summary: tool_use.summary.clone(),
                    input_detail: tool_use.input_detail.clone(),
                    output: self.output.as_deref().map(|text| budget.body(text)),
                    status: if failed {
                        ChatToolUseStatus::Failed
                    } else {
                        ChatToolUseStatus::Succeeded
                    },
                };
                Some(message.replacing_kind(ChatMessageKind::ToolUse(completed)))
            }
            ChatMessageKind::Question(question) => {
                let answer = self.answer_for_prompt(&question.prompt)?;
                let answered = ChatQuestion {
                    prompt: question.prompt.clone(),
                    options: question.options.clone(),
                    selected_option_label: Some(answer),
                };
                Some(message.replacing_kind(ChatMessageKind::Question(answered)))
            }
            _ => None,
        }
    }

    /// Extracts the chosen answer for a question prompt from the
    /// `Your questions have been answered: "Q"="A"...` result text.
    fn answer_for_prompt(&self, prompt: &str) -> Option<String> {
        let output = self.output.as_deref()?;
        let needle = format!("\"{prompt}\"=\"");
        let start = output.find(&needle)?;
        let tail = &output[start + needle.len()..];
        let end = tail.find('"')?;
        let answer = &tail[..end];
        if answer.is_empty() {
            None
        } else {
            Some(answer.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::model::{ChatProse, ChatQuestionOption, ChatRole};
    use crate::transcript::timestamp::Timestamp;

    fn message(kind: ChatMessageKind) -> ChatMessage {
        ChatMessage::new("m1", 0, ChatRole::Agent, Timestamp::EPOCH_ZERO, kind)
    }

    #[test]
    fn terminal_completion_sets_output_and_exit() {
        let msg = message(ChatMessageKind::Terminal(ChatTerminalCapture::running(
            "ls",
        )));
        let completion = TranscriptToolCompletion::new(Some("out".into()), false, None, Some(1.5));
        let completed = completion
            .applied(&msg, &TranscriptTextBudget::new())
            .unwrap();
        let ChatMessageKind::Terminal(capture) = completed.kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.output.as_deref(), Some("out"));
        assert_eq!(capture.exit_code, Some(0));
        assert_eq!(capture.duration_seconds, Some(1.5));
        assert!(!capture.is_running);
    }

    #[test]
    fn error_terminal_without_exit_defaults_to_one() {
        let msg = message(ChatMessageKind::Terminal(ChatTerminalCapture::running(
            "ls",
        )));
        let completion = TranscriptToolCompletion::new(None, true, None, None);
        let completed = completion
            .applied(&msg, &TranscriptTextBudget::new())
            .unwrap();
        let ChatMessageKind::Terminal(capture) = completed.kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.exit_code, Some(1));
    }

    #[test]
    fn tool_use_fails_on_nonzero_exit() {
        let msg = message(ChatMessageKind::ToolUse(ChatToolUse::running(
            "Grep", "Grep x", None,
        )));
        let completion =
            TranscriptToolCompletion::new(Some("no match".into()), false, Some(2), None);
        let completed = completion
            .applied(&msg, &TranscriptTextBudget::new())
            .unwrap();
        let ChatMessageKind::ToolUse(tool) = completed.kind else {
            panic!("expected toolUse");
        };
        assert_eq!(tool.status, ChatToolUseStatus::Failed);
        assert_eq!(tool.output.as_deref(), Some("no match"));
    }

    #[test]
    fn question_selects_answer_by_prompt() {
        let question = ChatQuestion {
            prompt: "Which path?".into(),
            options: vec![
                ChatQuestionOption::new("Fast", None),
                ChatQuestionOption::new("Slow", None),
            ],
            selected_option_label: None,
        };
        let msg = message(ChatMessageKind::Question(question));
        let completion = TranscriptToolCompletion::new(
            Some("Your questions have been answered: \"Which path?\"=\"Slow\". Continue.".into()),
            false,
            None,
            None,
        );
        let completed = completion
            .applied(&msg, &TranscriptTextBudget::new())
            .unwrap();
        let ChatMessageKind::Question(q) = completed.kind else {
            panic!("expected question");
        };
        assert_eq!(q.selected_option_label.as_deref(), Some("Slow"));
    }

    #[test]
    fn unanswered_question_yields_none() {
        let question = ChatQuestion {
            prompt: "Which path?".into(),
            options: vec![],
            selected_option_label: None,
        };
        let msg = message(ChatMessageKind::Question(question));
        let completion =
            TranscriptToolCompletion::new(Some("unrelated output".into()), false, None, None);
        assert!(completion
            .applied(&msg, &TranscriptTextBudget::new())
            .is_none());
    }

    #[test]
    fn prose_is_not_completable() {
        let msg = message(ChatMessageKind::Prose(ChatProse::new("hi")));
        let completion = TranscriptToolCompletion::new(Some("x".into()), false, None, None);
        assert!(completion
            .applied(&msg, &TranscriptTextBudget::new())
            .is_none());
    }
}
