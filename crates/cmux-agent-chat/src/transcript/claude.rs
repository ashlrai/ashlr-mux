//! Converts Claude Code session JSONL lines into [`ChatMessage`] values.
//!
//! Ports `Parsing/ClaudeTranscriptParser.swift`. Reads the format written
//! under `~/.claude/projects/<encoded-cwd>/` as of Claude Code 2.1: `user`
//! and `assistant` lines carry content; everything else is skipped. The
//! parser is stateless and fails open: malformed or unknown lines are dropped
//! silently. Pairing of `tool_use` blocks with their later `tool_result`
//! works across parse calls through [`ChatTranscriptParseState`].
//!
//! NAMING: this is `transcript::claude`, distinct from the crate's top-level
//! `claude` module (the live stream-json accumulator).

use std::sync::OnceLock;

use regex::Regex;

use super::batch_assembler::TranscriptBatchAssembler;
use super::diff_builder::TranscriptDiffBuilder;
use super::json_value::{parse_json_line, TranscriptJson};
use super::model::{
    ChatFileEdit, ChatFileEditOperation, ChatMessage, ChatMessageKind, ChatProse, ChatQuestion,
    ChatQuestionOption, ChatRole, ChatTerminalCapture, ChatThought, ChatToolUse,
};
use super::parse_state::{ChatTranscriptParseResult, ChatTranscriptParseState};
use super::text_budget::TranscriptTextBudget;
use super::timestamp::{Timestamp, TranscriptTimestampParser};
use super::tool_completion::TranscriptToolCompletion;

const USER_NOISE_PREFIXES: [&str; 3] = ["<command-name>", "<local-command", "<system-reminder"];
const EDIT_TOOL_NAMES: [&str; 3] = ["Edit", "MultiEdit", "NotebookEdit"];
const SUMMARY_ARGUMENT_KEYS: [&str; 11] = [
    "file_path",
    "notebook_path",
    "path",
    "pattern",
    "command",
    "query",
    "url",
    "description",
    "prompt",
    "skill",
    "name",
];

/// Matches an `Exit code[:] N` header in a tool result.
fn exit_code_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"Exit code:? (-?\d+)").expect("valid exit-code regex"))
}

/// Converts Claude Code session JSONL lines into [`ChatMessage`] values.
#[derive(Debug, Default, Clone)]
pub struct ClaudeTranscriptParser {
    budget: TranscriptTextBudget,
    timestamps: TranscriptTimestampParser,
    diffs: TranscriptDiffBuilder,
}

impl ClaudeTranscriptParser {
    /// Creates a Claude transcript parser.
    pub fn new() -> Self {
        ClaudeTranscriptParser::default()
    }

    /// Parses a contiguous run of JSONL lines into chat messages.
    ///
    /// Each parsed message gets `seq == starting_seq + line_offset`. Pass the
    /// returned [`ChatTranscriptParseResult::state`] into the next call.
    pub fn parse<I, S>(
        &self,
        lines: I,
        starting_seq: i64,
        state: ChatTranscriptParseState,
    ) -> ChatTranscriptParseResult
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut assembler = TranscriptBatchAssembler::new(state.clone(), self.budget);
        let mut last_timestamp = state.last_timestamp;
        for (offset, line) in lines.into_iter().enumerate() {
            let seq = starting_seq + offset as i64;
            let Some(value) = parse_json_line(line.as_ref()) else {
                continue;
            };
            let root = TranscriptJson::new(&value);
            if !root.is_object() {
                continue;
            }
            // Task-subagent traffic shares the session JSONL with
            // `isSidechain: true`; skip it BEFORE touching last_timestamp (the
            // seq is still consumed) so a subagent line's timestamp can't leak
            // into a later visible line that lacks one.
            if root.get("isSidechain").and_then(|v| v.bool()) == Some(true) {
                continue;
            }
            if let Some(stamped) = self
                .timestamps
                .date(root.get("timestamp").and_then(|v| v.string()))
            {
                last_timestamp = Some(stamped);
            }
            let timestamp = last_timestamp.unwrap_or(Timestamp::EPOCH_ZERO);
            match root.get("type").and_then(|v| v.string()) {
                Some("user") => self.append_user_line(root, seq, timestamp, &mut assembler),
                Some("assistant") => {
                    self.append_assistant_line(root, seq, timestamp, &mut assembler)
                }
                _ => continue,
            }
        }
        assembler.result(last_timestamp)
    }

    // MARK: - User lines

    fn append_user_line(
        &self,
        root: TranscriptJson<'_>,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        if root.get("isMeta").and_then(|v| v.bool()) == Some(true) {
            return;
        }
        let Some(content) = root.get("message").and_then(|m| m.get("content")) else {
            return;
        };
        let line_id = root
            .get("uuid")
            .and_then(|v| v.string())
            .map(str::to_string)
            .unwrap_or_else(|| format!("line-{seq}"));
        let mut emitted = 0;
        if let Some(text) = content.string() {
            self.append_user_prose(text, &line_id, &mut emitted, seq, timestamp, assembler);
            return;
        }
        for block in content.array().unwrap_or_default() {
            match block.get("type").and_then(|v| v.string()) {
                Some("text") => {
                    let text = block.get("text").and_then(|v| v.string()).unwrap_or("");
                    self.append_user_prose(text, &line_id, &mut emitted, seq, timestamp, assembler);
                }
                Some("tool_result") => self.resolve_tool_result(block, assembler),
                _ => continue,
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn append_user_prose(
        &self,
        text: &str,
        line_id: &str,
        emitted: &mut i64,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }
        if USER_NOISE_PREFIXES
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
        {
            return;
        }
        assembler.append(
            ChatMessage::new(
                block_id(line_id, *emitted),
                seq,
                ChatRole::User,
                timestamp,
                ChatMessageKind::Prose(ChatProse::new(self.budget.body(text))),
            ),
            None,
        );
        *emitted += 1;
    }

    fn resolve_tool_result(
        &self,
        block: TranscriptJson<'_>,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let Some(call_id) = block.get("tool_use_id").and_then(|v| v.string()) else {
            return;
        };
        let output = result_text(block.get("content"));
        let is_error = block
            .get("is_error")
            .and_then(|v| v.bool())
            .unwrap_or(false);
        let exit_code = parsed_exit_code(output.as_deref());
        assembler.resolve(
            call_id,
            &TranscriptToolCompletion::new(output, is_error, exit_code, None),
        );
    }

    // MARK: - Assistant lines

    fn append_assistant_line(
        &self,
        root: TranscriptJson<'_>,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let Some(content) = root.get("message").and_then(|m| m.get("content")) else {
            return;
        };
        let line_id = root
            .get("uuid")
            .and_then(|v| v.string())
            .map(str::to_string)
            .unwrap_or_else(|| format!("line-{seq}"));
        let mut emitted = 0;
        if let Some(text) = content.string() {
            self.append_agent_prose(text, &line_id, &mut emitted, seq, timestamp, assembler);
            return;
        }
        for block in content.array().unwrap_or_default() {
            match block.get("type").and_then(|v| v.string()) {
                Some("text") => {
                    let text = block.get("text").and_then(|v| v.string()).unwrap_or("");
                    self.append_agent_prose(
                        text,
                        &line_id,
                        &mut emitted,
                        seq,
                        timestamp,
                        assembler,
                    );
                }
                Some("thinking") => {
                    let text = block.get("thinking").and_then(|v| v.string()).unwrap_or("");
                    self.append_thought(text, &line_id, &mut emitted, seq, timestamp, assembler);
                }
                Some("tool_use") => {
                    self.append_tool_use(block, &line_id, &mut emitted, seq, timestamp, assembler);
                }
                _ => continue,
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn append_agent_prose(
        &self,
        text: &str,
        line_id: &str,
        emitted: &mut i64,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        if text.trim().is_empty() {
            return;
        }
        assembler.append(
            ChatMessage::new(
                block_id(line_id, *emitted),
                seq,
                ChatRole::Agent,
                timestamp,
                ChatMessageKind::Prose(ChatProse::new(self.budget.body(text))),
            ),
            None,
        );
        *emitted += 1;
    }

    #[allow(clippy::too_many_arguments)]
    fn append_thought(
        &self,
        text: &str,
        line_id: &str,
        emitted: &mut i64,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        if text.trim().is_empty() {
            return;
        }
        assembler.append(
            ChatMessage::new(
                block_id(line_id, *emitted),
                seq,
                ChatRole::Agent,
                timestamp,
                ChatMessageKind::Thought(ChatThought::new(self.budget.body(text))),
            ),
            None,
        );
        *emitted += 1;
    }

    // MARK: - Tool use blocks

    #[allow(clippy::too_many_arguments)]
    fn append_tool_use(
        &self,
        block: TranscriptJson<'_>,
        line_id: &str,
        emitted: &mut i64,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let Some(tool_name) = block.get("name").and_then(|v| v.string()) else {
            return;
        };
        let call_id = block.get("id").and_then(|v| v.string());
        let input = block.get("input");
        for kind in self.tool_use_kinds(tool_name, input) {
            assembler.append(
                ChatMessage::new(
                    block_id(line_id, *emitted),
                    seq,
                    ChatRole::Agent,
                    timestamp,
                    kind,
                ),
                call_id,
            );
            *emitted += 1;
        }
    }

    /// Maps one `tool_use` block to its message payload(s).
    fn tool_use_kinds(
        &self,
        tool_name: &str,
        input: Option<TranscriptJson<'_>>,
    ) -> Vec<ChatMessageKind> {
        if tool_name == "Bash" {
            if let Some(command) = input
                .and_then(|i| i.get("command"))
                .and_then(|v| v.string())
            {
                return vec![ChatMessageKind::Terminal(ChatTerminalCapture::running(
                    command,
                ))];
            }
        }
        if tool_name == "Write" || EDIT_TOOL_NAMES.contains(&tool_name) {
            if let Some(edit) = self.file_edit_kind(tool_name, input) {
                return vec![edit];
            }
        }
        if tool_name == "AskUserQuestion" {
            let questions = question_kinds(input);
            if !questions.is_empty() {
                return questions;
            }
        }
        vec![self.generic_tool_use_kind(tool_name, input)]
    }

    fn file_edit_kind(
        &self,
        tool_name: &str,
        input: Option<TranscriptJson<'_>>,
    ) -> Option<ChatMessageKind> {
        let input = input?;
        let file_path = input
            .get("file_path")
            .and_then(|v| v.string())
            .or_else(|| input.get("notebook_path").and_then(|v| v.string()))?;
        let (operation, change) = match tool_name {
            "Write" => (
                ChatFileEditOperation::Write,
                self.diffs
                    .creation(input.get("content").and_then(|v| v.string()).unwrap_or("")),
            ),
            "MultiEdit" => {
                let edits: Vec<_> = input
                    .get("edits")
                    .and_then(|v| v.array())
                    .unwrap_or_default()
                    .iter()
                    .map(|edit| {
                        self.diffs.replacement(
                            edit.get("old_string")
                                .and_then(|v| v.string())
                                .unwrap_or(""),
                            edit.get("new_string")
                                .and_then(|v| v.string())
                                .unwrap_or(""),
                        )
                    })
                    .collect();
                (ChatFileEditOperation::Edit, self.diffs.combined(&edits))
            }
            _ => (
                ChatFileEditOperation::Edit,
                self.diffs.replacement(
                    input
                        .get("old_string")
                        .and_then(|v| v.string())
                        .unwrap_or(""),
                    input
                        .get("new_string")
                        .and_then(|v| v.string())
                        .or_else(|| input.get("new_source").and_then(|v| v.string()))
                        .unwrap_or(""),
                ),
            ),
        };
        Some(ChatMessageKind::FileEdit(ChatFileEdit {
            file_path: file_path.to_string(),
            operation,
            additions: Some(change.additions),
            deletions: Some(change.deletions),
            unified_diff: if change.diff.is_empty() {
                None
            } else {
                Some(self.budget.body(&change.diff))
            },
        }))
    }

    fn generic_tool_use_kind(
        &self,
        tool_name: &str,
        input: Option<TranscriptJson<'_>>,
    ) -> ChatMessageKind {
        let mut summary = tool_name.to_string();
        if let Some(input) = input {
            for key in SUMMARY_ARGUMENT_KEYS {
                if let Some(value) = input.get(key).and_then(|v| v.string()) {
                    if !value.trim_matches(|c| c == ' ' || c == '\t').is_empty() {
                        summary = format!("{tool_name} {}", self.budget.summary_argument(value));
                        break;
                    }
                }
            }
        }
        let detail = input.map(|i| self.budget.input_detail(&i.compact_json_string()));
        ChatMessageKind::ToolUse(ChatToolUse::running(tool_name, summary, detail))
    }
}

fn question_kinds(input: Option<TranscriptJson<'_>>) -> Vec<ChatMessageKind> {
    let questions = input
        .and_then(|i| i.get("questions"))
        .and_then(|v| v.array());
    questions
        .unwrap_or_default()
        .iter()
        .filter_map(|question| {
            let prompt = question.get("question").and_then(|v| v.string())?;
            let options = question
                .get("options")
                .and_then(|v| v.array())
                .unwrap_or_default()
                .iter()
                .filter_map(|option| {
                    option.get("label").and_then(|v| v.string()).map(|label| {
                        ChatQuestionOption::new(
                            label,
                            option
                                .get("description")
                                .and_then(|v| v.string())
                                .map(str::to_string),
                        )
                    })
                })
                .collect();
            Some(ChatMessageKind::Question(ChatQuestion {
                prompt: prompt.to_string(),
                options,
                selected_option_label: None,
            }))
        })
        .collect()
}

/// Extracts text from a `tool_result` content payload: a plain string, or an
/// array of blocks where only `text` blocks carry renderable text.
fn result_text(content: Option<TranscriptJson<'_>>) -> Option<String> {
    let content = content?;
    if let Some(text) = content.string() {
        return Some(text.to_string());
    }
    let blocks = content.array()?;
    let texts: Vec<&str> = blocks
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(|v| v.string()) == Some("text") {
                block.get("text").and_then(|v| v.string())
            } else {
                None
            }
        })
        .collect();
    if texts.is_empty() {
        None
    } else {
        Some(texts.join("\n"))
    }
}

fn parsed_exit_code(output: Option<&str>) -> Option<i64> {
    let output = output?;
    let head: String = output.chars().take(200).collect();
    let captures = exit_code_regex().captures(&head)?;
    captures.get(1)?.as_str().parse().ok()
}

fn block_id(line_id: &str, emitted: i64) -> String {
    if emitted == 0 {
        line_id.to_string()
    } else {
        format!("{line_id}#{emitted}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::model::{ChatFileEditOperation, ChatToolUseStatus};
    use serde_json::{json, Value};

    fn parser() -> ClaudeTranscriptParser {
        ClaudeTranscriptParser::new()
    }

    fn parse(lines: &[String], starting_seq: i64) -> ChatTranscriptParseResult {
        parser().parse(lines.iter(), starting_seq, ChatTranscriptParseState::new())
    }

    fn user_line(
        uuid: &str,
        content: Value,
        is_meta: Option<bool>,
        timestamp: Option<&str>,
    ) -> String {
        let mut object = json!({
            "parentUuid": Value::Null, "isSidechain": false, "type": "user",
            "message": {"role": "user", "content": content},
            "uuid": uuid, "cwd": "/tmp/x", "sessionId": "s-1", "version": "2.1.175",
        });
        if let Some(meta) = is_meta {
            object["isMeta"] = json!(meta);
        }
        if let Some(ts) = timestamp {
            object["timestamp"] = json!(ts);
        }
        object.to_string()
    }

    fn user_text(uuid: &str, text: &str) -> String {
        user_line(uuid, json!(text), None, Some("2026-06-12T05:07:51.103Z"))
    }

    fn assistant_line(uuid: &str, blocks: Value, timestamp: &str) -> String {
        json!({
            "parentUuid": "u-1", "isSidechain": false, "type": "assistant",
            "message": {
                "model": "claude-fable-5", "id": "msg_01X", "type": "message",
                "role": "assistant", "content": blocks, "stop_reason": "tool_use",
            },
            "uuid": uuid, "timestamp": timestamp, "sessionId": "s-1",
        })
        .to_string()
    }

    fn assistant(blocks: Value) -> String {
        assistant_line("a-1", blocks, "2026-06-12T05:08:20.730Z")
    }

    fn tool_result_line(
        uuid: &str,
        tool_use_id: &str,
        content: Value,
        is_error: Option<bool>,
    ) -> String {
        let mut block =
            json!({"tool_use_id": tool_use_id, "type": "tool_result", "content": content});
        if let Some(err) = is_error {
            block["is_error"] = json!(err);
        }
        json!({
            "parentUuid": "a-1", "isSidechain": false, "type": "user",
            "message": {"role": "user", "content": [block]},
            "uuid": uuid, "timestamp": "2026-06-12T05:08:23.317Z", "sessionId": "s-1",
        })
        .to_string()
    }

    #[test]
    fn user_prose() {
        let result = parse(&[user_text("u-9", "fix the bug")], 41);
        assert_eq!(result.messages.len(), 1);
        let message = &result.messages[0];
        assert_eq!(message.id, "u-9");
        assert_eq!(message.seq, 41);
        assert_eq!(message.role, ChatRole::User);
        assert_eq!(
            message.kind,
            ChatMessageKind::Prose(ChatProse::new("fix the bug"))
        );
        assert!(result.updated_messages.is_empty());
    }

    #[test]
    fn noise_skipped() {
        let lines = vec![
            user_line(
                "u-1",
                json!("<local-command-caveat>Caveat: ...</local-command-caveat>"),
                Some(true),
                Some("2026-06-12T05:07:51.103Z"),
            ),
            user_text(
                "u-1",
                "<command-name>/model</command-name>\n<command-message>model</command-message>",
            ),
            user_text(
                "u-1",
                "<local-command-stdout>Set model</local-command-stdout>",
            ),
            user_text("u-1", "<system-reminder>noise</system-reminder>"),
            r#"{"type": "mode", "mode": "normal", "sessionId": "s-1"}"#.to_string(),
            r#"{"type": "summary", "summary": "Earlier conversation", "leafUuid": "x"}"#
                .to_string(),
            r#"{"type": "ai-title", "aiTitle": "Build a thing", "sessionId": "s-1"}"#.to_string(),
            user_text("u-real", "real prompt"),
        ];
        let result = parse(&lines, 0);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].id, "u-real");
        assert_eq!(result.messages[0].seq, 7);
    }

    #[test]
    fn assistant_text_and_thinking() {
        let lines = vec![
            assistant_line(
                "a-t",
                json!([{"type": "thinking", "thinking": "", "signature": "CAIS"}]),
                "2026-06-12T05:08:20.730Z",
            ),
            assistant_line(
                "a-u",
                json!([{"type": "thinking", "thinking": "weighing options", "signature": "CAIS"}]),
                "2026-06-12T05:08:20.730Z",
            ),
            assistant_line(
                "a-v",
                json!([{"type": "text", "text": "Here is the plan."}]),
                "2026-06-12T05:08:20.730Z",
            ),
        ];
        let result = parse(&lines, 0);
        assert_eq!(result.messages.len(), 2);
        assert_eq!(
            result.messages[0].kind,
            ChatMessageKind::Thought(ChatThought::new("weighing options"))
        );
        assert_eq!(result.messages[1].role, ChatRole::Agent);
        assert_eq!(
            result.messages[1].kind,
            ChatMessageKind::Prose(ChatProse::new("Here is the plan."))
        );
    }

    #[test]
    fn multi_block_line() {
        let line = assistant_line(
            "a-m",
            json!([
                {"type": "text", "text": "Running it now."},
                {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}},
            ]),
            "2026-06-12T05:08:20.730Z",
        );
        let result = parse(&[line], 5);
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].id, "a-m");
        assert_eq!(result.messages[1].id, "a-m#1");
        assert_eq!(result.messages[0].seq, 5);
        assert_eq!(result.messages[1].seq, 5);
    }

    #[test]
    fn bash_tool_use() {
        let line = assistant(json!([
            {"type": "tool_use", "id": "toolu_b", "name": "Bash",
             "input": {"command": "swift test", "description": "Run tests"}},
        ]));
        let result = parse(&[line], 0);
        let ChatMessageKind::Terminal(capture) = &result.messages[0].kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.command, "swift test");
        assert!(capture.is_running);
        assert_eq!(capture.output, None);
    }

    #[test]
    fn bash_result_same_call() {
        let lines = vec![
            assistant(
                json!([{"type": "tool_use", "id": "toolu_b", "name": "Bash", "input": {"command": "ls"}}]),
            ),
            tool_result_line("r-1", "toolu_b", json!("file-a\nfile-b"), None),
        ];
        let result = parse(&lines, 0);
        assert_eq!(result.messages.len(), 1);
        assert!(result.updated_messages.is_empty());
        let ChatMessageKind::Terminal(capture) = &result.messages[0].kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.output.as_deref(), Some("file-a\nfile-b"));
        assert_eq!(capture.exit_code, Some(0));
        assert!(!capture.is_running);
        assert!(result.state.pending_tool_uses.is_empty());
    }

    #[test]
    fn bash_result_across_calls() {
        let first = parser().parse(
            [assistant_line("a-b", json!([{"type": "tool_use", "id": "toolu_x", "name": "Bash", "input": {"command": "make"}}]), "2026-06-12T05:08:20.730Z")].iter(),
            10,
            ChatTranscriptParseState::new(),
        );
        assert_eq!(first.state.pending_tool_uses.len(), 1);
        let second = parser().parse(
            [tool_result_line("r-1", "toolu_x", json!("done"), None)].iter(),
            11,
            first.state,
        );
        assert!(second.messages.is_empty());
        assert_eq!(second.updated_messages.len(), 1);
        let updated = &second.updated_messages[0];
        assert_eq!(updated.id, "a-b");
        assert_eq!(updated.seq, 10);
        let ChatMessageKind::Terminal(capture) = &updated.kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.output.as_deref(), Some("done"));
        assert!(!capture.is_running);
        assert!(second.state.pending_tool_uses.is_empty());
    }

    #[test]
    fn error_result() {
        let lines = vec![
            assistant(json!([
                {"type": "tool_use", "id": "toolu_e", "name": "Bash", "input": {"command": "false"}},
                {"type": "tool_use", "id": "toolu_g", "name": "Grep", "input": {"pattern": "needle", "path": "/tmp"}},
            ])),
            tool_result_line("r-e", "toolu_e", json!("Exit code 2\nboom"), Some(true)),
            tool_result_line("r-g", "toolu_g", json!("No matches"), Some(true)),
        ];
        let result = parse(&lines, 0);
        let ChatMessageKind::Terminal(capture) = &result.messages[0].kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.exit_code, Some(2));
        let ChatMessageKind::ToolUse(grep) = &result.messages[1].kind else {
            panic!("expected toolUse");
        };
        assert_eq!(grep.status, ChatToolUseStatus::Failed);
        assert_eq!(grep.output.as_deref(), Some("No matches"));
    }

    #[test]
    fn edit_tool() {
        let line = assistant(json!([
            {"type": "tool_use", "id": "toolu_ed", "name": "Edit",
             "input": {"file_path": "/repo/App.swift", "old_string": "let a = 1",
                       "new_string": "let a = 2\nlet b = 3", "replace_all": false}},
        ]));
        let result = parse(&[line], 0);
        let ChatMessageKind::FileEdit(edit) = &result.messages[0].kind else {
            panic!("expected fileEdit");
        };
        assert_eq!(edit.file_path, "/repo/App.swift");
        assert_eq!(edit.operation, ChatFileEditOperation::Edit);
        assert_eq!(edit.additions, Some(2));
        assert_eq!(edit.deletions, Some(1));
        assert_eq!(
            edit.unified_diff.as_deref(),
            Some("-let a = 1\n+let a = 2\n+let b = 3")
        );
    }

    #[test]
    fn write_tool() {
        let line = assistant(json!([
            {"type": "tool_use", "id": "toolu_w", "name": "Write",
             "input": {"file_path": "/repo/New.swift", "content": "one\ntwo\nthree"}},
        ]));
        let result = parse(&[line], 0);
        let ChatMessageKind::FileEdit(edit) = &result.messages[0].kind else {
            panic!("expected fileEdit");
        };
        assert_eq!(edit.operation, ChatFileEditOperation::Write);
        assert_eq!(edit.additions, Some(3));
        assert_eq!(edit.deletions, Some(0));
    }

    #[test]
    fn generic_tool() {
        let line = assistant(json!([
            {"type": "tool_use", "id": "toolu_r", "name": "Read", "input": {"file_path": "/repo/main.swift"}},
        ]));
        let result = parse(&[line], 0);
        let ChatMessageKind::ToolUse(tool) = &result.messages[0].kind else {
            panic!("expected toolUse");
        };
        assert_eq!(tool.tool_name, "Read");
        assert_eq!(tool.summary, "Read /repo/main.swift");
        assert!(tool.input_detail.as_deref().unwrap().contains("file_path"));
        assert_eq!(tool.status, ChatToolUseStatus::Running);
    }

    #[test]
    fn ask_user_question() {
        let lines = vec![
            assistant(json!([
                {"type": "tool_use", "id": "toolu_q", "name": "AskUserQuestion",
                 "input": {"questions": [{
                    "question": "Which path?", "header": "Path", "multiSelect": false,
                    "options": [
                        {"label": "Fast", "description": "Quick but rough"},
                        {"label": "Slow", "description": "Thorough"},
                    ],
                 }]}},
            ])),
            tool_result_line("r-1", "toolu_q",
                json!("Your questions have been answered: \"Which path?\"=\"Slow\". You can now continue with these answers in mind."),
                None),
        ];
        let result = parse(&lines, 0);
        assert_eq!(result.messages.len(), 1);
        let ChatMessageKind::Question(question) = &result.messages[0].kind else {
            panic!("expected question");
        };
        assert_eq!(question.prompt, "Which path?");
        assert_eq!(
            question
                .options
                .iter()
                .map(|o| o.label.as_str())
                .collect::<Vec<_>>(),
            vec!["Fast", "Slow"]
        );
        assert_eq!(
            question.options[0].detail.as_deref(),
            Some("Quick but rough")
        );
        assert_eq!(question.selected_option_label.as_deref(), Some("Slow"));
    }

    #[test]
    fn multi_question_ask_user_question() {
        let lines = vec![
            assistant(json!([
                {"type": "tool_use", "id": "toolu_multi", "name": "AskUserQuestion",
                 "input": {"questions": [
                    {"question": "Which path?", "header": "Path", "multiSelect": false,
                     "options": [{"label": "Fast", "description": "rough"}, {"label": "Slow", "description": "thorough"}]},
                    {"question": "Which env?", "header": "Env", "multiSelect": false,
                     "options": [{"label": "Dev", "description": "local"}, {"label": "Prod", "description": "live"}]},
                 ]}},
            ])),
            tool_result_line("r-1", "toolu_multi",
                json!("Your questions have been answered: \"Which path?\"=\"Slow\", \"Which env?\"=\"Dev\". Continue."),
                None),
        ];
        let result = parse(&lines, 0);
        let questions: Vec<&ChatQuestion> = result
            .messages
            .iter()
            .filter_map(|m| match &m.kind {
                ChatMessageKind::Question(q) => Some(q),
                _ => None,
            })
            .collect();
        assert_eq!(questions.len(), 2);
        assert_eq!(
            questions
                .iter()
                .find(|q| q.prompt == "Which path?")
                .unwrap()
                .selected_option_label
                .as_deref(),
            Some("Slow")
        );
        assert_eq!(
            questions
                .iter()
                .find(|q| q.prompt == "Which env?")
                .unwrap()
                .selected_option_label
                .as_deref(),
            Some("Dev")
        );
    }

    #[test]
    fn sidechain_timestamp_does_not_leak() {
        let lines = vec![
            user_line("u-real", json!("first"), None, Some("2026-06-12T10:00:00.000Z")),
            json!({
                "parentUuid": Value::Null, "isSidechain": true, "type": "assistant",
                "message": {"role": "assistant", "content": [{"type": "text", "text": "subagent work"}]},
                "uuid": "side-x", "sessionId": "s-1", "timestamp": "2026-06-12T23:59:59.000Z",
            }).to_string(),
            json!({
                "parentUuid": Value::Null, "isSidechain": false, "type": "assistant",
                "message": {"role": "assistant", "content": [{"type": "text", "text": "real reply"}]},
                "uuid": "a-real", "sessionId": "s-1",
            }).to_string(),
        ];
        let result = parse(&lines, 0);
        let reply = result
            .messages
            .iter()
            .find(|m| matches!(&m.kind, ChatMessageKind::Prose(p) if p.text == "real reply"))
            .expect("missing real reply");
        let expected = TranscriptTimestampParser::new()
            .date(Some("2026-06-12T10:00:00Z"))
            .unwrap();
        assert_eq!(reply.timestamp, expected);
    }

    #[test]
    fn tool_result_array_content() {
        let lines = vec![
            assistant(
                json!([{"type": "tool_use", "id": "toolu_a", "name": "Read", "input": {"file_path": "/x"}}]),
            ),
            tool_result_line(
                "r-1",
                "toolu_a",
                json!([
                    {"type": "text", "text": "first"},
                    {"type": "tool_reference", "tool_name": "TaskCreate"},
                    {"type": "text", "text": "second"},
                ]),
                None,
            ),
        ];
        let result = parse(&lines, 0);
        let ChatMessageKind::ToolUse(tool) = &result.messages[0].kind else {
            panic!("expected toolUse");
        };
        assert_eq!(tool.output.as_deref(), Some("first\nsecond"));
        assert_eq!(tool.status, ChatToolUseStatus::Succeeded);
    }

    #[test]
    fn sidechain_lines_consume_seq() {
        let lines = vec![
            json!({
                "parentUuid": Value::Null, "isSidechain": true, "type": "user",
                "message": {"role": "user", "content": "injected subagent prompt"},
                "uuid": "side-1", "sessionId": "s-1", "timestamp": "2026-06-12T05:07:51.103Z",
            })
            .to_string(),
            user_text("u-real", "the human's prompt"),
        ];
        let result = parse(&lines, 10);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].seq, 11);
        let ChatMessageKind::Prose(prose) = &result.messages[0].kind else {
            panic!("expected prose");
        };
        assert_eq!(prose.text, "the human's prompt");
    }

    #[test]
    fn malformed_lines_skipped() {
        let lines = vec![
            "not json at all".to_string(),
            "{\"type\": \"user\", truncated".to_string(),
            "[1, 2, 3]".to_string(),
            user_text("u-ok", "still works"),
        ];
        let result = parse(&lines, 0);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].seq, 3);
    }

    #[test]
    fn truncation() {
        let huge = "x".repeat(40_000);
        let lines = vec![
            user_text("u-1", &huge),
            assistant(
                json!([{"type": "tool_use", "id": "toolu_t", "name": "Bash", "input": {"command": "cat big"}}]),
            ),
            tool_result_line("r-1", "toolu_t", json!(huge), None),
        ];
        let result = parse(&lines, 0);
        let ChatMessageKind::Prose(prose) = &result.messages[0].kind else {
            panic!("expected prose");
        };
        assert!(prose.text.chars().count() <= 16_385);
        assert!(prose.text.ends_with('…'));
        let ChatMessageKind::Terminal(capture) = &result.messages[1].kind else {
            panic!("expected terminal");
        };
        assert!(
            capture
                .output
                .as_ref()
                .map(|o| o.chars().count())
                .unwrap_or(0)
                <= 16_385
        );
    }

    #[test]
    fn timestamp_fallback() {
        let lines = vec![
            user_line(
                "u-a",
                json!("first"),
                None,
                Some("2026-06-12T05:07:51.103Z"),
            ),
            user_line("u-b", json!("second"), None, None),
        ];
        let result = parse(&lines, 0);
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[1].timestamp, result.messages[0].timestamp);
        let expected = TranscriptTimestampParser::new()
            .date(Some("2026-06-12T05:07:51.103Z"))
            .unwrap();
        assert_eq!(result.messages[0].timestamp, expected);
    }
}
