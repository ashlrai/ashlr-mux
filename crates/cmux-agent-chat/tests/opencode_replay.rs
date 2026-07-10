//! Replays a REAL opencode 1.17.13 `/event` SSE capture through the ported
//! parser + text accumulator to prove the assistant reply is extracted.
//!
//! The fixture was captured live (create session → POST prompt "hello" → GET
//! /event). The assistant streams its chain-of-thought on a `type:"reasoning"`
//! part (which must be hidden) and the actual reply "Hi" on a `type:"text"` part.

use cmux_agent_chat::{AgentEvent, ProviderId, ProviderStream, RunningSession};

const STREAM: &str = include_str!("fixtures/opencode_1_17_event_stream.sse");
const OC_SESSION_ID: &str = "ses_0dc7c068fffek4W91Kz7vw6NtN";

#[test]
fn opencode_1_17_event_stream_yields_assistant_reply() {
    let mut session = RunningSession::new(
        "store-session-1",
        ProviderId::Opencode,
        "opencode.cmd",
        Vec::new(),
        None,
        "1.0.0",
    );
    session.set_opencode_session_id(OC_SESSION_ID);

    let mut text = String::new();
    let mut completed = false;
    for raw in STREAM.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        for event in session.consume_opencode_sse_line(line) {
            match event {
                AgentEvent::ProviderOutput {
                    text: chunk,
                    stream: ProviderStream::Stdout,
                    session_id,
                    ..
                } => {
                    // Regression guard: renderer events must carry the cmux store
                    // session id, NOT the OpenCode loopback id (`ses_…`). Emitting
                    // the loopback id routes the reply to a session the UI does not
                    // know about, so nothing renders.
                    assert_eq!(
                        session_id, "store-session-1",
                        "ProviderOutput leaked the OpenCode loopback session id"
                    );
                    text.push_str(&chunk);
                }
                AgentEvent::ProviderTurnComplete { session_id, .. } => {
                    assert_eq!(session_id, "store-session-1");
                    completed = true;
                }
                _ => {}
            }
        }
    }

    eprintln!("collected assistant text = {text:?}");
    assert!(!text.is_empty(), "expected assistant reply text, got empty");
    // The reasoning ("The user just said ...") must NOT leak into the reply.
    assert!(
        !text.contains("The user just said"),
        "reasoning leaked into the reply: {text:?}"
    );
    assert!(
        text.contains("Hi"),
        "expected reply to contain 'Hi', got {text:?}"
    );
    assert!(completed, "expected a turn-complete (session.idle)");
}
