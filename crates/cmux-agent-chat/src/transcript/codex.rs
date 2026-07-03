//! Converts Codex CLI rollout JSONL lines into [`ChatMessage`] values.
//!
//! Ports `Parsing/CodexTranscriptParser.swift`. Reads the format written
//! under `~/.codex/sessions/YYYY/MM/DD/` as of Codex CLI 0.139: every line is
//! `{timestamp, type, payload}`. Content lives in `response_item` payloads;
//! `event_msg`, `turn_context`, and token bookkeeping are skipped. The parser
//! is stateless and fails open. Pairing of calls with their `*_output` works
//! across parse calls through [`ChatTranscriptParseState`].
//!
//! NAMING: this is `transcript::codex`, distinct from the crate's top-level
//! `codex` module (the live JSON-RPC accumulator).

use std::sync::OnceLock;

use regex::Regex;

use super::batch_assembler::TranscriptBatchAssembler;
use super::json_value::{parse_json_line, TranscriptJson};
use super::model::{
    ChatMessage, ChatMessageKind, ChatProse, ChatRole, ChatStatusEvent, ChatStatusTransition,
    ChatTerminalCapture, ChatThought, ChatToolUse, ChatToolUseStatus,
};
use super::parse_state::{ChatTranscriptParseResult, ChatTranscriptParseState};
use super::text_budget::TranscriptTextBudget;
use super::timestamp::{Timestamp, TranscriptTimestampParser};
use super::tool_completion::TranscriptToolCompletion;

const USER_NOISE_PREFIXES: [&str; 6] = [
    "<user_instructions",
    "<environment_context",
    "<permissions",
    "<collaboration_mode",
    "<turn_aborted",
    "# AGENTS.md instructions",
];
const SHELL_TOOL_NAMES: [&str; 4] = ["shell", "exec_command", "local_shell_call", "container.exec"];
const SHELL_WRAPPER_BINARIES: [&str; 3] = ["bash", "sh", "zsh"];
const SUMMARY_ARGUMENT_KEYS: [&str; 10] = [
    "path", "file_path", "pattern", "query", "url", "text", "key", "app", "session_id", "plan",
];

/// Matches an exit-code header in a tool output.
fn exit_code_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:Process exited with code|Exit code:?|exited with code) (-?\d+)")
            .expect("valid exit-code regex")
    })
}

/// Matches a `Wall time: S seconds` header in a tool output.
fn wall_time_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"Wall time: ([0-9.]+) seconds").expect("valid wall-time regex"))
}

/// Matches the first patched-file path in an `apply_patch` input.
fn apply_patch_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\*\*\* (?:Update|Add|Delete) File: (.+)").expect("valid apply-patch regex")
    })
}

/// Converts Codex CLI rollout JSONL lines into [`ChatMessage`] values.
#[derive(Debug, Default, Clone)]
pub struct CodexTranscriptParser {
    budget: TranscriptTextBudget,
    timestamps: TranscriptTimestampParser,
}

impl CodexTranscriptParser {
    /// Creates a Codex transcript parser.
    pub fn new() -> Self {
        CodexTranscriptParser::default()
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
            if let Some(stamped) = self
                .timestamps
                .date(root.get("timestamp").and_then(|v| v.string()))
            {
                last_timestamp = Some(stamped);
            }
            let timestamp = last_timestamp.unwrap_or(Timestamp::EPOCH_ZERO);
            let payload = root.get("payload");
            match root.get("type").and_then(|v| v.string()) {
                Some("session_meta") => self.append_session_start(payload, seq, timestamp, &mut assembler),
                Some("compacted") => assembler.append(
                    ChatMessage::new(
                        format!("line-{seq}"),
                        seq,
                        ChatRole::System,
                        timestamp,
                        ChatMessageKind::Status(ChatStatusTransition::new(
                            ChatStatusEvent::ContextCompacted,
                            None,
                        )),
                    ),
                    None,
                ),
                Some("response_item") => self.append_response_item(payload, seq, timestamp, &mut assembler),
                _ => continue,
            }
        }
        assembler.result(last_timestamp)
    }

    // MARK: - Line kinds

    fn append_session_start(
        &self,
        payload: Option<TranscriptJson<'_>>,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let session_id = payload.and_then(|p| p.get("id")).and_then(|v| v.string());
        let cwd = payload
            .and_then(|p| p.get("cwd"))
            .and_then(|v| v.string())
            .map(str::to_string);
        assembler.append(
            ChatMessage::new(
                session_id.map(|id| format!("session-{id}")).unwrap_or_else(|| format!("line-{seq}")),
                seq,
                ChatRole::System,
                timestamp,
                ChatMessageKind::Status(ChatStatusTransition::new(
                    ChatStatusEvent::SessionStarted,
                    cwd,
                )),
            ),
            None,
        );
    }

    fn append_response_item(
        &self,
        payload: Option<TranscriptJson<'_>>,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let Some(payload) = payload else {
            return;
        };
        match payload.get("type").and_then(|v| v.string()) {
            Some("message") => self.append_message(payload, seq, timestamp, assembler),
            Some("reasoning") => self.append_reasoning(payload, seq, timestamp, assembler),
            Some("function_call") => self.append_function_call(payload, seq, timestamp, assembler),
            Some("custom_tool_call") => self.append_custom_tool_call(payload, seq, timestamp, assembler),
            Some("function_call_output") | Some("custom_tool_call_output") => {
                self.resolve_output(payload, assembler)
            }
            Some("web_search_call") => self.append_web_search(payload, seq, timestamp, assembler),
            _ => (),
        }
    }

    fn append_message(
        &self,
        payload: TranscriptJson<'_>,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let role = match payload.get("role").and_then(|v| v.string()) {
            Some("user") => ChatRole::User,
            Some("assistant") => ChatRole::Agent,
            _ => return, // developer / system context injections
        };
        let blocks = payload.get("content").and_then(|v| v.array()).unwrap_or_default();
        let texts: Vec<&str> = blocks
            .iter()
            .filter_map(|block| {
                let block_type = block.get("type").and_then(|v| v.string())?;
                if block_type != "input_text" && block_type != "output_text" {
                    return None;
                }
                let text = block.get("text").and_then(|v| v.string())?;
                let trimmed = text.trim();
                if trimmed.is_empty() {
                    return None;
                }
                if role == ChatRole::User
                    && USER_NOISE_PREFIXES.iter().any(|prefix| trimmed.starts_with(prefix))
                {
                    return None;
                }
                Some(text)
            })
            .collect();
        if texts.is_empty() {
            return;
        }
        assembler.append(
            ChatMessage::new(
                format!("line-{seq}"),
                seq,
                role,
                timestamp,
                ChatMessageKind::Prose(ChatProse::new(self.budget.body(&texts.join("\n\n")))),
            ),
            None,
        );
    }

    fn append_reasoning(
        &self,
        payload: TranscriptJson<'_>,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let summaries: Vec<&str> = payload
            .get("summary")
            .and_then(|v| v.array())
            .unwrap_or_default()
            .iter()
            .filter_map(|item| item.get("text").and_then(|v| v.string()))
            .collect();
        let text = summaries.join("\n\n");
        if text.trim().is_empty() {
            return;
        }
        assembler.append(
            ChatMessage::new(
                format!("line-{seq}"),
                seq,
                ChatRole::Agent,
                timestamp,
                ChatMessageKind::Thought(ChatThought::new(self.budget.body(&text))),
            ),
            None,
        );
    }

    // MARK: - Tool calls

    fn append_function_call(
        &self,
        payload: TranscriptJson<'_>,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let Some(name) = payload.get("name").and_then(|v| v.string()) else {
            return;
        };
        let call_id = payload.get("call_id").and_then(|v| v.string());
        let arguments = payload.get("arguments").and_then(|v| v.string());
        // The arguments are a JSON-encoded string; parse them into a subtree.
        let parsed_value = arguments.and_then(parse_json_line);
        let parsed_arguments = parsed_value.as_ref().map(TranscriptJson::new);
        let kind = if SHELL_TOOL_NAMES.contains(&name) {
            match shell_command(parsed_arguments, payload) {
                Some(command) => ChatMessageKind::Terminal(ChatTerminalCapture::running(command)),
                None => generic_tool_use_kind(&self.budget, name, parsed_arguments, arguments),
            }
        } else {
            generic_tool_use_kind(&self.budget, name, parsed_arguments, arguments)
        };
        assembler.append(
            ChatMessage::new(
                call_id.map(str::to_string).unwrap_or_else(|| format!("line-{seq}")),
                seq,
                ChatRole::Agent,
                timestamp,
                kind,
            ),
            call_id,
        );
    }

    fn append_custom_tool_call(
        &self,
        payload: TranscriptJson<'_>,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let Some(name) = payload.get("name").and_then(|v| v.string()) else {
            return;
        };
        let call_id = payload.get("call_id").and_then(|v| v.string());
        let input = payload.get("input").and_then(|v| v.string()).unwrap_or("");
        let mut summary = name.to_string();
        if name == "apply_patch" {
            if let Some(path) = first_patched_file(input) {
                summary = format!("{name} {}", self.budget.summary_argument(path));
            }
        }
        let detail = if input.is_empty() {
            None
        } else {
            Some(self.budget.input_detail(input))
        };
        assembler.append(
            ChatMessage::new(
                call_id.map(str::to_string).unwrap_or_else(|| format!("line-{seq}")),
                seq,
                ChatRole::Agent,
                timestamp,
                ChatMessageKind::ToolUse(ChatToolUse::running(name, summary, detail)),
            ),
            call_id,
        );
    }

    fn append_web_search(
        &self,
        payload: TranscriptJson<'_>,
        seq: i64,
        timestamp: Timestamp,
        assembler: &mut TranscriptBatchAssembler,
    ) {
        let Some(query) = payload
            .get("action")
            .and_then(|a| a.get("query"))
            .and_then(|v| v.string())
        else {
            return;
        };
        assembler.append(
            ChatMessage::new(
                format!("line-{seq}"),
                seq,
                ChatRole::Agent,
                timestamp,
                ChatMessageKind::ToolUse(ChatToolUse {
                    tool_name: "web_search".to_string(),
                    summary: format!("Search {}", self.budget.summary_argument(query)),
                    input_detail: None,
                    output: None,
                    status: ChatToolUseStatus::Succeeded,
                }),
            ),
            None,
        );
    }

    // MARK: - Tool outputs

    fn resolve_output(&self, payload: TranscriptJson<'_>, assembler: &mut TranscriptBatchAssembler) {
        let Some(call_id) = payload.get("call_id").and_then(|v| v.string()) else {
            return;
        };
        let completion = completion_from(payload.get("output"));
        assembler.resolve(call_id, &completion);
    }
}

fn generic_tool_use_kind(
    budget: &TranscriptTextBudget,
    tool_name: &str,
    arguments: Option<TranscriptJson<'_>>,
    raw_arguments: Option<&str>,
) -> ChatMessageKind {
    let mut summary = tool_name.to_string();
    if let Some(arguments) = arguments {
        for key in SUMMARY_ARGUMENT_KEYS {
            if let Some(value) = arguments.get(key).and_then(|v| v.string()) {
                if !value.trim_matches(|c| c == ' ' || c == '\t').is_empty() {
                    summary = format!("{tool_name} {}", budget.summary_argument(value));
                    break;
                }
            }
        }
    }
    let detail = raw_arguments.and_then(|raw| {
        if raw.is_empty() || raw == "{}" {
            None
        } else {
            Some(budget.input_detail(raw))
        }
    });
    ChatMessageKind::ToolUse(ChatToolUse::running(tool_name, summary, detail))
}

/// Extracts the human-meaningful command line from a shell-style call.
///
/// Handles `{"cmd": "..."}`, `{"command": "..."}`, `{"command": ["bash",
/// "-lc", "actual"]}`, and the `local_shell_call` `action.command` array.
fn shell_command(arguments: Option<TranscriptJson<'_>>, payload: TranscriptJson<'_>) -> Option<String> {
    if let Some(cmd) = arguments.and_then(|a| a.get("cmd")).and_then(|v| v.string()) {
        return Some(cmd.to_string());
    }
    if let Some(cmd) = arguments.and_then(|a| a.get("command")).and_then(|v| v.string()) {
        return Some(cmd.to_string());
    }
    let parts = arguments
        .and_then(|a| a.get("command"))
        .and_then(|v| v.array())
        .or_else(|| {
            payload
                .get("action")
                .and_then(|a| a.get("command"))
                .and_then(|v| v.array())
        })?;
    let strings: Vec<&str> = parts.iter().filter_map(|part| part.string()).collect();
    if strings.is_empty() {
        return None;
    }
    if strings.len() >= 3 {
        let binary = strings[0].rsplit('/').find(|segment| !segment.is_empty());
        if let Some(binary) = binary {
            if SHELL_WRAPPER_BINARIES.contains(&binary)
                && (strings[1] == "-lc" || strings[1] == "-c")
            {
                return Some(strings[2..].join(" "));
            }
        }
    }
    Some(strings.join(" "))
}

fn first_patched_file(patch: &str) -> Option<&str> {
    apply_patch_regex()
        .captures(patch)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str())
}

/// Builds a completion from an output payload, which is a plain string, a
/// JSON-encoded `{"output": ..., "metadata": {"exit_code": ...}}` string, or
/// that object inline; exit code and wall time also appear as text headers.
fn completion_from(value: Option<TranscriptJson<'_>>) -> TranscriptToolCompletion {
    let mut text: Option<String> = value.and_then(|v| v.string()).map(str::to_string);
    let mut exit_code = value
        .and_then(|v| v.get("metadata"))
        .and_then(|m| m.get("exit_code"))
        .and_then(|v| v.int());
    let mut duration = value
        .and_then(|v| v.get("metadata"))
        .and_then(|m| m.get("duration_seconds"))
        .and_then(|v| v.double());
    if text.is_none() && value.is_some_and(|v| v.is_object()) {
        text = value
            .and_then(|v| v.get("output"))
            .and_then(|v| v.string())
            .map(str::to_string);
    }
    if let Some(raw) = text.as_deref() {
        if let Some(nested_value) = parse_json_line(raw) {
            let nested = TranscriptJson::new(&nested_value);
            if let Some(inner) = nested.get("output").and_then(|v| v.string()) {
                text = Some(inner.to_string());
                exit_code = nested
                    .get("metadata")
                    .and_then(|m| m.get("exit_code"))
                    .and_then(|v| v.int())
                    .or(exit_code);
                duration = nested
                    .get("metadata")
                    .and_then(|m| m.get("duration_seconds"))
                    .and_then(|v| v.double())
                    .or(duration);
            }
        }
    }
    if exit_code.is_none() {
        if let Some(text) = text.as_deref() {
            let head: String = text.chars().take(400).collect();
            if let Some(captures) = exit_code_regex().captures(&head) {
                exit_code = captures.get(1).and_then(|m| m.as_str().parse().ok());
            }
        }
    }
    if duration.is_none() {
        if let Some(text) = text.as_deref() {
            let head: String = text.chars().take(400).collect();
            if let Some(captures) = wall_time_regex().captures(&head) {
                duration = captures.get(1).and_then(|m| m.as_str().parse().ok());
            }
        }
    }
    TranscriptToolCompletion::new(text, exit_code.unwrap_or(0) != 0, exit_code, duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn parser() -> CodexTranscriptParser {
        CodexTranscriptParser::new()
    }

    fn parse(lines: &[String], starting_seq: i64) -> ChatTranscriptParseResult {
        parser().parse(lines.iter(), starting_seq, ChatTranscriptParseState::new())
    }

    fn line(kind: &str, payload: Value, timestamp: &str) -> String {
        json!({ "timestamp": timestamp, "type": kind, "payload": payload }).to_string()
    }

    fn line_default(kind: &str, payload: Value) -> String {
        line(kind, payload, "2026-06-11T21:38:05.381Z")
    }

    fn message_line(role: &str, texts: &[&str]) -> String {
        let block_type = if role == "assistant" { "output_text" } else { "input_text" };
        let content: Vec<Value> = texts.iter().map(|t| json!({"type": block_type, "text": t})).collect();
        line_default("response_item", json!({"type": "message", "role": role, "content": content}))
    }

    fn function_call_line(name: &str, arguments: &str, call_id: &str) -> String {
        line_default(
            "response_item",
            json!({"type": "function_call", "name": name, "arguments": arguments, "call_id": call_id}),
        )
    }

    fn output_line(call_id: &str, output: &str) -> String {
        line_default(
            "response_item",
            json!({"type": "function_call_output", "call_id": call_id, "output": output}),
        )
    }

    #[test]
    fn session_meta() {
        let meta = line_default(
            "session_meta",
            json!({
                "id": "019eb89e-aaaa", "timestamp": "2026-06-11T21:38:03.916Z",
                "cwd": "/repo", "originator": "codex-tui", "cli_version": "0.139.0",
            }),
        );
        let result = parse(&[meta], 0);
        assert_eq!(result.messages.len(), 1);
        let message = &result.messages[0];
        assert_eq!(message.role, ChatRole::System);
        assert_eq!(
            message.kind,
            ChatMessageKind::Status(ChatStatusTransition::new(
                ChatStatusEvent::SessionStarted,
                Some("/repo".to_string())
            ))
        );
        assert_eq!(message.id, "session-019eb89e-aaaa");
    }

    #[test]
    fn prose_mapping() {
        let lines = vec![
            message_line("developer", &["<permissions instructions>\nstuff"]),
            message_line("user", &[
                "# AGENTS.md instructions for /repo\n<INSTRUCTIONS>...",
                "<environment_context>\n  <cwd>/repo</cwd>\n</environment_context>",
            ]),
            message_line("user", &["fix the parser"]),
            message_line("assistant", &["On it."]),
        ];
        let result = parse(&lines, 0);
        assert_eq!(result.messages.len(), 2);
        assert_eq!(result.messages[0].role, ChatRole::User);
        assert_eq!(result.messages[0].kind, ChatMessageKind::Prose(ChatProse::new("fix the parser")));
        assert_eq!(result.messages[0].seq, 2);
        assert_eq!(result.messages[1].role, ChatRole::Agent);
        assert_eq!(result.messages[1].kind, ChatMessageKind::Prose(ChatProse::new("On it.")));
    }

    #[test]
    fn reasoning() {
        let lines = vec![
            line_default("response_item", json!({"type": "reasoning", "summary": [], "encrypted_content": "gAAAA"})),
            line_default("response_item", json!({
                "type": "reasoning",
                "summary": [
                    {"type": "summary_text", "text": "Inspect the file"},
                    {"type": "summary_text", "text": "Then run tests"},
                ],
            })),
        ];
        let result = parse(&lines, 0);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(
            result.messages[0].kind,
            ChatMessageKind::Thought(ChatThought::new("Inspect the file\n\nThen run tests"))
        );
    }

    #[test]
    fn exec_command() {
        let call = function_call_line(
            "exec_command",
            r#"{"cmd":"rg -n \"foo\" .","workdir":"/repo","yield_time_ms":10000}"#,
            "call_1",
        );
        let result = parse(&[call], 7);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].id, "call_1");
        assert_eq!(result.messages[0].seq, 7);
        let ChatMessageKind::Terminal(capture) = &result.messages[0].kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.command, r#"rg -n "foo" ."#);
        assert!(capture.is_running);
    }

    #[test]
    fn shell_command_array() {
        let call = function_call_line("shell", r#"{"command":["bash","-lc","echo hi"],"timeout_ms":5000}"#, "call_1");
        let result = parse(&[call], 0);
        let ChatMessageKind::Terminal(capture) = &result.messages[0].kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.command, "echo hi");
    }

    #[test]
    fn output_same_call() {
        let lines = vec![
            function_call_line("exec_command", r#"{"cmd":"swift build"}"#, "call_1"),
            output_line("call_1", "Chunk ID: 8f9491\nWall time: 1.5000 seconds\nProcess exited with code 0\nOutput:\nBuild complete!"),
        ];
        let result = parse(&lines, 0);
        assert_eq!(result.messages.len(), 1);
        assert!(result.updated_messages.is_empty());
        let ChatMessageKind::Terminal(capture) = &result.messages[0].kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.exit_code, Some(0));
        assert_eq!(capture.duration_seconds, Some(1.5));
        assert!(!capture.is_running);
        assert!(capture.output.as_deref().unwrap().contains("Build complete!"));
    }

    #[test]
    fn output_across_calls() {
        let first = parser().parse(
            [function_call_line("exec_command", r#"{"cmd":"make"}"#, "call_z")].iter(),
            3,
            ChatTranscriptParseState::new(),
        );
        assert_eq!(first.state.pending_tool_uses.len(), 1);
        let second = parser().parse(
            [output_line("call_z", "Process exited with code 1\nOutput:\nerror")].iter(),
            4,
            first.state,
        );
        assert!(second.messages.is_empty());
        assert_eq!(second.updated_messages.len(), 1);
        let updated = &second.updated_messages[0];
        assert_eq!(updated.id, "call_z");
        assert_eq!(updated.seq, 3);
        let ChatMessageKind::Terminal(capture) = &updated.kind else {
            panic!("expected terminal");
        };
        assert_eq!(capture.exit_code, Some(1));
        assert!(second.state.pending_tool_uses.is_empty());
    }

    #[test]
    fn generic_function_call() {
        let lines = vec![
            function_call_line("update_plan", r#"{"plan":[{"step":"do it","status":"pending"}]}"#, "call_p"),
            output_line("call_p", r#"{"output":"plan rejected","metadata":{"exit_code":2,"duration_seconds":0.1}}"#),
        ];
        let result = parse(&lines, 0);
        let ChatMessageKind::ToolUse(tool) = &result.messages[0].kind else {
            panic!("expected toolUse");
        };
        assert_eq!(tool.tool_name, "update_plan");
        assert_eq!(tool.output.as_deref(), Some("plan rejected"));
        assert_eq!(tool.status, ChatToolUseStatus::Failed);
        assert!(tool.input_detail.as_deref().unwrap().contains("plan"));
    }

    #[test]
    fn apply_patch() {
        let patch = "*** Begin Patch\n*** Update File: /repo/Sources/App.swift\n@@\n-old\n+new\n*** End Patch";
        let lines = vec![
            line_default("response_item", json!({
                "type": "custom_tool_call", "status": "completed",
                "call_id": "call_ap", "name": "apply_patch", "input": patch,
            })),
            line_default("response_item", json!({
                "type": "custom_tool_call_output", "call_id": "call_ap",
                "output": "Exit code: 0\nWall time: 0 seconds\nOutput:\nSuccess.",
            })),
        ];
        let result = parse(&lines, 0);
        let ChatMessageKind::ToolUse(tool) = &result.messages[0].kind else {
            panic!("expected toolUse");
        };
        assert_eq!(tool.tool_name, "apply_patch");
        assert_eq!(tool.summary, "apply_patch /repo/Sources/App.swift");
        assert_eq!(tool.status, ChatToolUseStatus::Succeeded);
    }

    #[test]
    fn noise_and_seq() {
        let lines = vec![
            line_default("event_msg", json!({"type": "task_started", "turn_id": "t-1"})),
            line_default("turn_context", json!({"turn_id": "t-1", "cwd": "/repo"})),
            line_default("event_msg", json!({"type": "token_count", "info": {"total_token_usage": {"input_tokens": 5}}})),
            "garbage {".to_string(),
            message_line("user", &["hello"]),
        ];
        let result = parse(&lines, 100);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].seq, 104);
        assert_eq!(result.messages[0].id, "line-104");
    }

    #[test]
    fn compacted() {
        let result = parse(&[line_default("compacted", json!({"message": "history replaced"}))], 0);
        assert_eq!(result.messages.len(), 1);
        assert_eq!(
            result.messages[0].kind,
            ChatMessageKind::Status(ChatStatusTransition::new(ChatStatusEvent::ContextCompacted, None))
        );
    }

    #[test]
    fn truncation() {
        let huge = format!("Process exited with code 0\nOutput:\n{}", "y".repeat(40_000));
        let lines = vec![
            function_call_line("exec_command", r#"{"cmd":"cat big"}"#, "call_1"),
            output_line("call_1", &huge),
        ];
        let result = parse(&lines, 0);
        let ChatMessageKind::Terminal(capture) = &result.messages[0].kind else {
            panic!("expected terminal");
        };
        assert!(capture.output.as_ref().map(|o| o.chars().count()).unwrap_or(0) <= 16_385);
        assert!(capture.output.as_deref().unwrap().ends_with('…'));
    }
}
