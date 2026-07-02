mod agent_session;
mod opencode_http;
mod pick_files;
mod session;
mod terminal;

fn ping_response() -> &'static str {
    "pong"
}

#[derive(serde::Serialize)]
struct DesktopCoreStatus {
    milestone: &'static str,
    platform: &'static str,
    agent_providers: Vec<&'static str>,
    ipc_fixture_request: String,
}

#[tauri::command]
fn ping() -> String {
    ping_response().to_string()
}

#[tauri::command]
fn desktop_core_status() -> DesktopCoreStatus {
    DesktopCoreStatus {
        milestone: cmux_core::milestone(),
        platform: cmux_core::CMUX_PLATFORM,
        agent_providers: cmux_agent::AgentSessionProviderId::ALL
            .into_iter()
            .map(cmux_agent::AgentSessionProviderId::raw_value)
            .collect(),
        ipc_fixture_request: cmux_ipc::append_line(r#"{"id":2,"method":"ping","params":{}}"#),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(terminal::TerminalState::default())
        .manage(session::SessionState::default())
        .manage(agent_session::AgentSessionState::default())
        .invoke_handler(tauri::generate_handler![
            ping,
            desktop_core_status,
            terminal::terminal_open,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_close,
            session::session_snapshot,
            session::session_split,
            session::session_close,
            session::session_set_divider,
            session::session_set_surface_kind,
            agent_session::agent_session_rpc
        ])
        .run(tauri::generate_context!())
        .expect("failed to run cmux desktop bootstrap");
}

#[cfg(test)]
mod tests {
    use super::{desktop_core_status, ping, ping_response};

    #[test]
    fn ping_response_is_stable() {
        assert_eq!(ping_response(), "pong");
    }

    #[test]
    fn ping_command_returns_the_bootstrap_reply() {
        assert_eq!(ping(), "pong");
    }

    #[test]
    fn desktop_status_uses_shared_core() {
        let status = desktop_core_status();
        assert_eq!(status.milestone, "M1");
        assert_eq!(status.platform, "windows-m1-core");
        assert!(status.agent_providers.contains(&"codex"));
        assert!(status.ipc_fixture_request.ends_with('\n'));
    }

    #[test]
    fn desktop_status_lists_every_agent_provider_in_declared_order() {
        let status = desktop_core_status();
        // The web layer renders providers by index, so the order is part of
        // the contract, not an implementation detail.
        assert_eq!(status.agent_providers, vec!["codex", "claude", "opencode"]);
    }

    #[test]
    fn desktop_status_ipc_fixture_matches_the_golden_ping_request() {
        let status = desktop_core_status();
        // Exactly the bootstrap ping request the socket-v2 golden fixture
        // encodes, terminated with the framing newline.
        assert_eq!(
            status.ipc_fixture_request,
            "{\"id\":2,\"method\":\"ping\",\"params\":{}}\n"
        );
        // The request must be a single newline-delimited frame.
        assert_eq!(status.ipc_fixture_request.matches('\n').count(), 1);
        assert!(status.ipc_fixture_request.starts_with('{'));
    }

    #[test]
    fn desktop_status_serializes_to_the_shape_the_web_bridge_consumes() {
        // Contract parity: tauri-bridge.ts unwraps `desktop_core_status` as a
        // bare object (no { ok, value } envelope). Assert the exact JSON keys
        // and value types the web layer relies on, so web/Rust drift is caught
        // without a native launch.
        let status = desktop_core_status();
        let value = serde_json::to_value(&status).expect("status serializes");

        let object = value.as_object().expect("status is a JSON object");

        // Exact key set — no extra, no missing.
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "agent_providers",
                "ipc_fixture_request",
                "milestone",
                "platform",
            ]
        );

        assert_eq!(object["milestone"], serde_json::json!("M1"));
        assert_eq!(object["platform"], serde_json::json!("windows-m1-core"));
        assert_eq!(
            object["agent_providers"],
            serde_json::json!(["codex", "claude", "opencode"])
        );
        assert!(object["agent_providers"].is_array());
        assert!(object["milestone"].is_string());
        assert!(object["platform"].is_string());
        assert!(object["ipc_fixture_request"].is_string());

        // The bare object must NOT look like a NativeReply envelope; otherwise
        // callNative would try to unwrap/throw on it.
        assert!(object.get("ok").is_none());
        assert!(object.get("value").is_none());
        assert!(object.get("error").is_none());
    }

    #[test]
    fn ping_command_is_a_bare_string_not_an_envelope() {
        // callNative passes a bare string straight through. Confirm `ping`
        // serializes to a plain JSON string (no { ok, value } wrapper).
        let value = serde_json::to_value(ping()).expect("ping serializes");
        assert_eq!(value, serde_json::json!("pong"));
        assert!(value.is_string());
    }
}
