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
        .invoke_handler(tauri::generate_handler![ping, desktop_core_status])
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
}
