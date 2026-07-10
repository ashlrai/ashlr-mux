//! Full-fixture replay of the on-disk transcript parsers, mirroring the
//! `tests/opencode_replay.rs` pattern but for the JSONL history subsystem.
//!
//! Rebuilds representative `~/.claude/projects/**` and `~/.codex/sessions/**`
//! JSONL runs and asserts the emitted [`ChatMessage`] role/seq/kind sequence,
//! cross-call tool pairing + back-patching, parse-state carry-over, the
//! isSidechain / isMeta / noise-prefix drops, and the `maxPendingToolUses`
//! newest-by-seq eviction — end-to-end through the public parser API.

use cmux_agent_chat::transcript::batch_assembler::MAX_PENDING_TOOL_USES;
use cmux_agent_chat::{
    ChatMessageKind, ChatRole, ChatStatusEvent, ChatTranscriptParseState, ClaudeTranscriptParser,
    CodexTranscriptParser,
};
use serde_json::{json, Value};

/// One Claude JSONL user line carrying a plain string content.
fn claude_user(uuid: &str, content: &str, timestamp: Option<&str>) -> String {
    let mut object = json!({
        "parentUuid": Value::Null, "isSidechain": false, "type": "user",
        "message": {"role": "user", "content": content},
        "uuid": uuid, "sessionId": "s-1",
    });
    if let Some(ts) = timestamp {
        object["timestamp"] = json!(ts);
    }
    object.to_string()
}

/// One Claude JSONL assistant line carrying content blocks.
fn claude_assistant(uuid: &str, blocks: Value, timestamp: Option<&str>) -> String {
    let mut object = json!({
        "parentUuid": "u-1", "isSidechain": false, "type": "assistant",
        "message": {"role": "assistant", "content": blocks},
        "uuid": uuid, "sessionId": "s-1",
    });
    if let Some(ts) = timestamp {
        object["timestamp"] = json!(ts);
    }
    object.to_string()
}

/// One Claude JSONL tool_result (a user line whose content is a result block).
fn claude_tool_result(tool_use_id: &str, content: Value) -> String {
    json!({
        "parentUuid": "a-1", "isSidechain": false, "type": "user",
        "message": {"role": "user", "content": [
            {"tool_use_id": tool_use_id, "type": "tool_result", "content": content}
        ]},
        "uuid": "r-1", "sessionId": "s-1", "timestamp": "2026-06-12T05:08:23.317Z",
    })
    .to_string()
}

#[test]
fn claude_full_transcript_replays_roles_seqs_and_kinds() {
    let parser = ClaudeTranscriptParser::new();

    // A realistic tail: prompt, a noise line, a sidechain subagent line, then
    // an assistant turn that thinks, speaks, and runs Bash — the tool_result
    // arriving only in the SECOND parse call.
    let first_batch = [
        claude_user("u-1", "add a test", Some("2026-06-12T10:00:00.000Z")),
        claude_user(
            "noise",
            "<system-reminder>ignore me</system-reminder>",
            Some("2026-06-12T10:00:01.000Z"),
        ),
        // Sidechain line: dropped, but its seq is still consumed and its late
        // timestamp must NOT leak into the following no-timestamp line.
        json!({
            "parentUuid": Value::Null, "isSidechain": true, "type": "assistant",
            "message": {"role": "assistant", "content": [{"type": "text", "text": "subagent"}]},
            "uuid": "side", "sessionId": "s-1", "timestamp": "2026-06-12T23:59:00.000Z",
        })
        .to_string(),
        claude_assistant(
            "a-1",
            json!([
                {"type": "thinking", "thinking": "plan it", "signature": "x"},
                {"type": "text", "text": "Running the build."},
                {"type": "tool_use", "id": "toolu_build", "name": "Bash", "input": {"command": "swift build"}},
            ]),
            None, // no timestamp: inherits the visible 10:00:01, not the sidechain 23:59
        ),
    ];
    let first = parser.parse(first_batch.iter(), 0, ChatTranscriptParseState::new());

    // seqs: u-1=0, noise=1 (dropped), sidechain=2 (dropped), assistant=3 (x3 blocks).
    assert_eq!(first.messages.len(), 4);
    let roles_kinds: Vec<(ChatRole, &str)> = first
        .messages
        .iter()
        .map(|m| (m.role, kind_tag(&m.kind)))
        .collect();
    assert_eq!(
        roles_kinds,
        vec![
            (ChatRole::User, "prose"),
            (ChatRole::Agent, "thought"),
            (ChatRole::Agent, "prose"),
            (ChatRole::Agent, "terminal"),
        ]
    );
    // The user prompt kept its own seq 0; the three assistant blocks share
    // seq 3 and are id-suffixed.
    assert_eq!(first.messages[0].seq, 0);
    assert_eq!(first.messages[1].seq, 3);
    assert_eq!(first.messages[1].id, "a-1");
    assert_eq!(first.messages[2].id, "a-1#1");
    assert_eq!(first.messages[3].id, "a-1#2");

    // The no-timestamp assistant line inherited the visible 10:00:01, proving
    // the sidechain's 23:59 was skipped before the timestamp update.
    let inherited = first.messages[3].timestamp;
    assert_eq!(
        inherited,
        cmux_agent_chat::transcript::TranscriptTimestampParser::new()
            .date(Some("2026-06-12T10:00:01Z"))
            .unwrap()
    );

    // Bash is still running; its call id is carried in the pending state.
    let ChatMessageKind::Terminal(capture) = &first.messages[3].kind else {
        panic!("expected terminal");
    };
    assert!(capture.is_running);
    assert_eq!(first.state.pending_tool_uses.len(), 1);
    assert!(first.state.pending_tool_uses.contains_key("toolu_build"));

    // Second parse call: the result arrives, back-patching the earlier message
    // via updated_messages (not messages), preserving id + seq.
    let second = parser.parse(
        [claude_tool_result(
            "toolu_build",
            json!("Build complete\nExit code: 0"),
        )]
        .iter(),
        4,
        first.state,
    );
    assert!(second.messages.is_empty());
    assert_eq!(second.updated_messages.len(), 1);
    let updated = &second.updated_messages[0];
    assert_eq!(updated.id, "a-1#2");
    assert_eq!(updated.seq, 3);
    let ChatMessageKind::Terminal(capture) = &updated.kind else {
        panic!("expected terminal");
    };
    assert!(!capture.is_running);
    assert_eq!(capture.exit_code, Some(0));
    assert!(capture
        .output
        .as_deref()
        .unwrap()
        .contains("Build complete"));
    assert!(second.state.pending_tool_uses.is_empty());
}

#[test]
fn claude_eviction_keeps_newest_max_pending_by_seq() {
    let parser = ClaudeTranscriptParser::new();
    // One assistant line per unresolved Bash call; more than the cap, no
    // results ever arrive.
    let total = MAX_PENDING_TOOL_USES + 40;
    let lines: Vec<String> = (0..total)
        .map(|i| {
            claude_assistant(
                &format!("a-{i}"),
                json!([{"type": "tool_use", "id": format!("call-{i}"), "name": "Bash", "input": {"command": format!("cmd {i}")}}]),
                Some("2026-06-12T10:00:00.000Z"),
            )
        })
        .collect();
    let result = parser.parse(lines.iter(), 0, ChatTranscriptParseState::new());
    assert_eq!(result.messages.len(), total);
    // Carried pending state is bounded to the newest (highest-seq) calls.
    assert_eq!(result.state.pending_tool_uses.len(), MAX_PENDING_TOOL_USES);
    assert!(result
        .state
        .pending_tool_uses
        .contains_key(&format!("call-{}", total - 1)));
    assert!(!result.state.pending_tool_uses.contains_key("call-0"));
}

#[test]
fn codex_full_transcript_replays_session_reasoning_prose_and_tool_pairing() {
    let parser = CodexTranscriptParser::new();

    fn codex_line(kind: &str, payload: Value) -> String {
        json!({ "timestamp": "2026-06-11T21:38:05.381Z", "type": kind, "payload": payload })
            .to_string()
    }

    let first_batch = [
        codex_line("session_meta", json!({"id": "sess-1", "cwd": "/repo"})),
        // Injected context user block is noise-dropped.
        codex_line(
            "response_item",
            json!({
                "type": "message", "role": "user",
                "content": [{"type": "input_text", "text": "<environment_context>\n</environment_context>"}],
            }),
        ),
        codex_line(
            "response_item",
            json!({
                "type": "message", "role": "user",
                "content": [{"type": "input_text", "text": "port the parser"}],
            }),
        ),
        codex_line(
            "response_item",
            json!({
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "look first"}, {"type": "summary_text", "text": "then edit"}],
            }),
        ),
        codex_line(
            "response_item",
            json!({
                "type": "function_call", "name": "exec_command",
                "arguments": r#"{"cmd":"cargo test"}"#, "call_id": "call_run",
            }),
        ),
    ];
    let first = parser.parse(first_batch.iter(), 0, ChatTranscriptParseState::new());

    // session(0), noise(1 dropped), prose(2), reasoning(3), function_call(4).
    assert_eq!(first.messages.len(), 4);
    let summary: Vec<(ChatRole, &str, i64)> = first
        .messages
        .iter()
        .map(|m| (m.role, kind_tag(&m.kind), m.seq))
        .collect();
    assert_eq!(
        summary,
        vec![
            (ChatRole::System, "status", 0),
            (ChatRole::User, "prose", 2),
            (ChatRole::Agent, "thought", 3),
            (ChatRole::Agent, "terminal", 4),
        ]
    );
    let ChatMessageKind::Status(status) = &first.messages[0].kind else {
        panic!("expected status");
    };
    assert_eq!(status.event, ChatStatusEvent::SessionStarted);
    assert_eq!(status.detail.as_deref(), Some("/repo"));
    assert_eq!(first.messages[0].id, "session-sess-1");
    assert_eq!(first.messages[3].id, "call_run");
    assert_eq!(first.state.pending_tool_uses.len(), 1);

    // The output lands in a later parse call and back-patches by call id.
    let second = parser.parse(
        [codex_line(
            "response_item",
            json!({
                "type": "function_call_output", "call_id": "call_run",
                "output": "Wall time: 2.0 seconds\nProcess exited with code 0\nOutput:\nok",
            }),
        )]
        .iter(),
        5,
        first.state,
    );
    assert!(second.messages.is_empty());
    assert_eq!(second.updated_messages.len(), 1);
    let ChatMessageKind::Terminal(capture) = &second.updated_messages[0].kind else {
        panic!("expected terminal");
    };
    assert_eq!(second.updated_messages[0].id, "call_run");
    assert_eq!(capture.exit_code, Some(0));
    assert_eq!(capture.duration_seconds, Some(2.0));
    assert!(!capture.is_running);
    assert!(second.state.pending_tool_uses.is_empty());
}

/// Names a message kind's wire discriminator for compact sequence assertions.
fn kind_tag(kind: &ChatMessageKind) -> &'static str {
    match kind {
        ChatMessageKind::Prose(_) => "prose",
        ChatMessageKind::Thought(_) => "thought",
        ChatMessageKind::ToolUse(_) => "tool_use",
        ChatMessageKind::Terminal(_) => "terminal",
        ChatMessageKind::FileEdit(_) => "file_edit",
        ChatMessageKind::PermissionRequest(_) => "permission_request",
        ChatMessageKind::Question(_) => "question",
        ChatMessageKind::Status(_) => "status",
        ChatMessageKind::Attachment(_) => "attachment",
        ChatMessageKind::Unsupported(_) => "unsupported",
    }
}
