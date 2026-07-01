//! Pure, headless foundations of the cmux agent-session host contract.
//!
//! The live agent-session webview talks to its host over two seams:
//!
//! * a **request** seam (renderer → host): [`BridgeRequest`] decodes the
//!   `{id, method, params}` envelope and offers typed parameter getters, with
//!   [`BridgeError`] as the failure vocabulary.
//! * a **push** seam (host → renderer): [`AgentEvent`] serializes to the exact
//!   type-tagged, camelCase wire objects the renderer consumes.
//!
//! This crate is the FIRST SLICE: only the pure serde + parsing pieces that the
//! later provider transports (codex / claude / opencode) and the Tauri command
//! will sit on top of. The wire types are mirrored verbatim from the canonical
//! macOS Swift sources under `Sources/Panels/AgentSession*.swift` and validated
//! against `webviews/src/agent-session/shared/types.ts`.
//!
//! ## Deferred (NOT in this slice)
//!
//! The stateful provider transports (codex JSON-RPC, claude stream-json,
//! opencode SSE / http-loopback), the running-session / process-store host
//! state, app-context / theme assembly, and the separate mobile
//! transcript-history / JSONL subsystem (`Sources/Mobile/AgentChat/*`) all come
//! with later slices, together with their `tokio` / `reqwest` / `cmux-process`
//! / `cmux-agent` dependencies.

pub mod claude;
pub mod codex;
pub mod error;
pub mod event;
pub mod line_buffer;
pub mod opencode;
pub mod permission_mode;
pub mod process_store;
pub mod request;
pub mod running_session;
pub mod transport_action;

pub use claude::{write_claude_stream_json, ClaudeStreamAccumulator};
pub use codex::{
    encode_line, initialize_request, initialized_notification, thread_start_request,
    turn_start_request, unsupported_server_request_error, CodexAccumulator, SubmitRejection,
};
pub use error::BridgeError;
pub use event::{
    ActivityKind, ActivityStatus, AgentEvent, AgentSessionRateLimitRow, AgentSessionTheme,
    ProviderId, ProviderStream, RateLimitRole,
};
pub use line_buffer::OutputLineBuffer;
pub use opencode::{
    opencode_process_output_disposition, OpenCodeEvent, OpenCodeEventStreamParser,
    OpenCodeEventTextAccumulator, OpenCodeProcessOutputDisposition,
};
pub use permission_mode::PermissionMode;
pub use process_store::{
    AgentTransport, ProcessStore, SpawnRequest, SpawnedSession, StartedSession, TransportError,
};
pub use request::BridgeRequest;
pub use running_session::{ProviderAccumulator, RunningSession};
pub use transport_action::TransportAction;

use serde_json::{json, Value};

/// Host context for the request dispatcher.
///
/// The full macOS `app.context` payload (panel/workspace ids, renderer kind,
/// theme, and ~90 localized copy keys) is a host concern assembled by the
/// GUI-wiring slice; the dispatcher only needs the fallback working directory
/// for `provider.start`.
#[derive(Debug, Clone, Default)]
pub struct DispatchContext {
    /// The workspace working directory used when a `provider.start` request omits
    /// its own `workingDirectory` parameter.
    pub working_directory: Option<String>,
}

/// Dispatch one decoded [`BridgeRequest`] against the [`ProcessStore`].
///
/// Ported from the `provider.*` + `provider.list` arms of the Swift
/// `AgentSessionWebRendererCoordinator.handle(_:)` switch. Returns the method's
/// reply value (the renderer wraps it as `{ok:true,value}`), or a
/// [`BridgeError`] (wrapped as `{ok:false,error:{code,userMessage}}`).
///
/// ## Not covered here (host concerns)
///
/// `app.context` and `app.pickFiles` are serviced by the host: `app.context`
/// assembles theme + localized copy, and `app.pickFiles` opens a native file
/// dialog (a Tauri dialog+fs concern in the GUI-wiring slice). They fall through
/// to [`BridgeError::UnsupportedMethod`] here so an unexpected delegation is a
/// well-formed error rather than a panic.
pub fn handle<T, S>(
    request: &BridgeRequest,
    store: &mut ProcessStore<T, S>,
    ctx: &DispatchContext,
) -> Result<Value, BridgeError>
where
    T: AgentTransport,
    S: FnMut(AgentEvent),
{
    match request.method.as_str() {
        "provider.list" => Ok(provider_list_value()),
        "provider.select" => {
            if store.has_active_session() {
                return Err(BridgeError::SessionAlreadyRunning);
            }
            let provider = request.provider_id()?;
            store.select_provider(provider);
            Ok(json!({ "providerId": provider.as_str() }))
        }
        "provider.start" => {
            let provider = request.provider_id()?;
            let working_directory = request
                .string("workingDirectory")
                .or_else(|| ctx.working_directory.clone());
            let started = store.start(provider, working_directory)?;
            Ok(started.to_value())
        }
        "provider.writeLine" => {
            let session_id = request.required_string("sessionId")?;
            let permission_mode = request.permission_mode();
            let text = request.required_raw_string("text")?;
            store.write_line(&session_id, permission_mode, &text)?;
            Ok(json!({ "sent": true }))
        }
        "provider.stop" => {
            let session_id = request.required_string("sessionId")?;
            store.stop(&session_id)?;
            Ok(json!({ "stopped": true }))
        }
        other => Err(BridgeError::UnsupportedMethod(other.to_string())),
    }
}

/// The `provider.list` reply: the `ProviderInfo` wire objects for every
/// provider, in canonical order (see `types.ts` `ProviderInfo`).
///
/// Mirrors the Swift `provider.list` arm (and the sibling
/// `cmux_agent::AgentSessionProviderId` metadata, which is the canonical source a
/// future slice could delegate to instead of the mirrored copy on
/// [`ProviderId`]).
fn provider_list_value() -> Value {
    Value::Array(
        ProviderId::ALL
            .iter()
            .map(|provider| {
                json!({
                    "id": provider.as_str(),
                    "displayName": provider.display_name(),
                    "executableName": provider.executable_name(),
                    "transportKind": provider.transport_kind(),
                    "arguments": provider.launch_arguments(),
                    "autoStart": provider.should_auto_start_session(),
                })
            })
            .collect(),
    )
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct FakeTransport;

    impl AgentTransport for FakeTransport {
        fn spawn(&mut self, request: &SpawnRequest) -> Result<SpawnedSession, TransportError> {
            Ok(SpawnedSession {
                executable_path: format!("/bin/{}", request.provider_id.as_str()),
                arguments: request.provider_id.launch_arguments(),
            })
        }

        fn write_line(
            &mut self,
            _session_id: &str,
            _permission_mode: PermissionMode,
            _text: &str,
        ) -> Result<(), TransportError> {
            Ok(())
        }

        fn terminate(&mut self, _session_id: &str) -> Result<(), TransportError> {
            Ok(())
        }
    }

    type Events = Rc<RefCell<Vec<AgentEvent>>>;

    fn make_store() -> (ProcessStore<FakeTransport, impl FnMut(AgentEvent)>, Events) {
        let events: Events = Rc::new(RefCell::new(Vec::new()));
        let sink_events = events.clone();
        let store = ProcessStore::new(
            FakeTransport,
            move |event| sink_events.borrow_mut().push(event),
            "9.9.9",
        );
        (store, events)
    }

    fn request(method: &str, params: Value) -> BridgeRequest {
        BridgeRequest::from_value(json!({
            "id": "req-1",
            "method": method,
            "params": params,
        }))
        .unwrap()
    }

    #[test]
    fn provider_list_returns_all_three_providers() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        let value = handle(&request("provider.list", json!({})), &mut store, &ctx).unwrap();
        let list = value.as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0]["id"], json!("codex"));
        assert_eq!(list[0]["displayName"], json!("Codex"));
        assert_eq!(list[0]["transportKind"], json!("stdio-jsonrpc"));
        assert_eq!(list[0]["autoStart"], json!(true));
        assert_eq!(list[1]["id"], json!("claude"));
        assert_eq!(list[1]["displayName"], json!("Claude Code"));
        assert_eq!(list[1]["autoStart"], json!(false));
        assert_eq!(list[2]["id"], json!("opencode"));
        assert_eq!(list[2]["transportKind"], json!("http-loopback"));
    }

    #[test]
    fn provider_start_starts_session_and_returns_reply() {
        let (mut store, events) = make_store();
        let ctx = DispatchContext {
            working_directory: Some("/fallback".to_string()),
        };
        let value = handle(
            &request("provider.start", json!({ "providerId": "codex" })),
            &mut store,
            &ctx,
        )
        .unwrap();
        assert_eq!(value["providerId"], json!("codex"));
        assert_eq!(value["executablePath"], json!("/bin/codex"));
        assert!(value["sessionId"].as_str().is_some());
        assert!(store.has_active_session());
        // provider.started emitted immediately for codex.
        assert_eq!(events.borrow().len(), 1);
    }

    #[test]
    fn provider_start_twice_is_session_already_running() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        handle(
            &request("provider.start", json!({ "providerId": "codex" })),
            &mut store,
            &ctx,
        )
        .unwrap();
        assert_eq!(
            handle(
                &request("provider.start", json!({ "providerId": "claude" })),
                &mut store,
                &ctx,
            ),
            Err(BridgeError::SessionAlreadyRunning)
        );
    }

    #[test]
    fn provider_start_invalid_provider() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        assert_eq!(
            handle(
                &request("provider.start", json!({ "providerId": "gemini" })),
                &mut store,
                &ctx,
            ),
            Err(BridgeError::InvalidProvider("gemini".to_string()))
        );
    }

    #[test]
    fn provider_start_missing_provider() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        assert_eq!(
            handle(&request("provider.start", json!({})), &mut store, &ctx),
            Err(BridgeError::MissingParameter("providerId".to_string()))
        );
    }

    #[test]
    fn provider_select_updates_selection_and_guards_active() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        let value = handle(
            &request("provider.select", json!({ "providerId": "opencode" })),
            &mut store,
            &ctx,
        )
        .unwrap();
        assert_eq!(value["providerId"], json!("opencode"));
        assert_eq!(store.selected_provider(), Some(ProviderId::Opencode));

        // Once a session is running, select is rejected.
        handle(
            &request("provider.start", json!({ "providerId": "codex" })),
            &mut store,
            &ctx,
        )
        .unwrap();
        assert_eq!(
            handle(
                &request("provider.select", json!({ "providerId": "claude" })),
                &mut store,
                &ctx,
            ),
            Err(BridgeError::SessionAlreadyRunning)
        );
    }

    #[test]
    fn provider_write_line_flows_through_dispatch() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        let started = handle(
            &request("provider.start", json!({ "providerId": "codex" })),
            &mut store,
            &ctx,
        )
        .unwrap();
        let session_id = started["sessionId"].as_str().unwrap().to_string();
        let value = handle(
            &request(
                "provider.writeLine",
                json!({ "sessionId": session_id, "text": "  keep spaces  " }),
            ),
            &mut store,
            &ctx,
        )
        .unwrap();
        assert_eq!(value, json!({ "sent": true }));
    }

    #[test]
    fn provider_write_line_missing_session_id() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        assert_eq!(
            handle(
                &request("provider.writeLine", json!({ "text": "x" })),
                &mut store,
                &ctx,
            ),
            Err(BridgeError::MissingParameter("sessionId".to_string()))
        );
    }

    #[test]
    fn provider_write_line_missing_text() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        let started = handle(
            &request("provider.start", json!({ "providerId": "codex" })),
            &mut store,
            &ctx,
        )
        .unwrap();
        let session_id = started["sessionId"].as_str().unwrap().to_string();
        assert_eq!(
            handle(
                &request("provider.writeLine", json!({ "sessionId": session_id })),
                &mut store,
                &ctx,
            ),
            Err(BridgeError::MissingParameter("text".to_string()))
        );
    }

    #[test]
    fn provider_stop_flows_through_dispatch() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        let started = handle(
            &request("provider.start", json!({ "providerId": "codex" })),
            &mut store,
            &ctx,
        )
        .unwrap();
        let session_id = started["sessionId"].as_str().unwrap().to_string();
        let value = handle(
            &request("provider.stop", json!({ "sessionId": session_id })),
            &mut store,
            &ctx,
        )
        .unwrap();
        assert_eq!(value, json!({ "stopped": true }));
    }

    #[test]
    fn provider_stop_unknown_session() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        assert_eq!(
            handle(
                &request("provider.stop", json!({ "sessionId": "nope" })),
                &mut store,
                &ctx,
            ),
            Err(BridgeError::SessionNotFound("nope".to_string()))
        );
    }

    #[test]
    fn unsupported_method_is_rejected() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        assert_eq!(
            handle(&request("provider.bogus", json!({})), &mut store, &ctx),
            Err(BridgeError::UnsupportedMethod("provider.bogus".to_string()))
        );
    }

    #[test]
    fn host_only_methods_fall_through_to_unsupported() {
        let (mut store, _events) = make_store();
        let ctx = DispatchContext::default();
        for method in ["app.context", "app.pickFiles"] {
            assert_eq!(
                handle(&request(method, json!({})), &mut store, &ctx),
                Err(BridgeError::UnsupportedMethod(method.to_string()))
            );
        }
    }

    #[test]
    fn full_dispatch_streaming_turn_orders_events_through_sink() {
        let (mut store, events) = make_store();
        let ctx = DispatchContext::default();
        let started = handle(
            &request("provider.start", json!({ "providerId": "codex" })),
            &mut store,
            &ctx,
        )
        .unwrap();
        let session_id = started["sessionId"].as_str().unwrap().to_string();
        for line in [
            "{\"method\":\"item/agentMessage/delta\",\"params\":{\"delta\":\"Yo\"}}\n",
            "{\"method\":\"turn/completed\",\"params\":{}}\n",
        ] {
            store.feed_output(&session_id, ProviderStream::Stdout, line.as_bytes());
        }
        let recorded = events.borrow();
        assert!(matches!(recorded[0], AgentEvent::ProviderStarted { .. }));
        assert!(matches!(recorded[1], AgentEvent::ProviderOutput { .. }));
        assert!(matches!(recorded[2], AgentEvent::ProviderTurnComplete { .. }));
    }
}
