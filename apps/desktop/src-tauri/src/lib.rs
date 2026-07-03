mod agent_session;
mod diff;
mod markdown;
mod opencode_http;
mod pick_files;
mod schemes;
mod session;
mod terminal;

use tauri::Manager;

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
        .manage(markdown::MarkdownState::default())
        // `DiffState` needs the resolved `app_data_dir`, so it is constructed with
        // the `AppHandle` in `setup` rather than up front on the builder.
        .setup(|app| {
            app.manage(diff::DiffState::new(app.handle())?);
            Ok(())
        })
        // Phase-4 custom URI schemes (WebView2 `WebResourceRequested` handlers).
        // Each delegates its parse+lookup to the pure `schemes` helpers, then reads
        // bytes and responds. DEFERRED to the UI checkpoint: that hyphenated custom
        // schemes serve bytes through WebView2, and that the pages loading them
        // carry the expected origin/token.
        .register_asynchronous_uri_scheme_protocol("cmux-diff-viewer", diff_viewer_protocol)
        .register_asynchronous_uri_scheme_protocol("cmux-md", markdown_protocol)
        .register_asynchronous_uri_scheme_protocol("cmux-local-image", local_image_protocol)
        .register_asynchronous_uri_scheme_protocol("cmux-remote-image", remote_image_protocol)
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
            agent_session::agent_session_rpc,
            diff::diff_comments_rpc,
            markdown::cmux_lib_rpc,
            markdown::markdown_render,
            markdown::markdown_apply_theme
        ])
        .run(tauri::generate_context!())
        .expect("failed to run cmux desktop bootstrap");
}

// ---------------------------------------------------------------------------
// Custom URI-scheme protocol handlers (Phase 4)
// ---------------------------------------------------------------------------

/// Build a `200 OK` byte response with an explicit `Content-Type`.
fn ok_bytes(mime: &str, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .header(tauri::http::header::CONTENT_TYPE, mime)
        .body(body)
        .expect("well-formed uri-scheme response")
}

/// Build an empty response with `status`.
fn status_only(status: tauri::http::StatusCode) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .body(Vec::new())
        .expect("well-formed uri-scheme response")
}

/// `cmux-diff-viewer://<token>/<path>` — serve a file from the token's registered
/// allowlist (trust-gated by the diff session registry). Untrusted/unknown →
/// `403`; a registered file that vanished on disk → `404`.
fn diff_viewer_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let uri = request.uri().to_string();
    let response = match ctx.app_handle().try_state::<diff::DiffState>() {
        Some(state) => match state.resolve_diff_request(&uri, std::time::SystemTime::now()) {
            Some((path, mime)) => match std::fs::read(&path) {
                Ok(bytes) => ok_bytes(&mime, bytes),
                Err(_) => status_only(tauri::http::StatusCode::NOT_FOUND),
            },
            None => status_only(tauri::http::StatusCode::FORBIDDEN),
        },
        None => status_only(tauri::http::StatusCode::INTERNAL_SERVER_ERROR),
    };
    responder.respond(response);
}

/// `cmux-md://…/<asset>` — serve the viewer shell HTML / lazy libraries / CSS from
/// the bundled markdown-viewer assets.
fn markdown_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let uri = request.uri().to_string();
    let app = ctx.app_handle();
    let assets = app
        .path()
        .resource_dir()
        .ok()
        .and_then(|root| app.try_state::<markdown::MarkdownState>().map(|s| (s, root)))
        .and_then(|(state, root)| state.assets(&root));
    let response = match assets.and_then(|assets| schemes::resolve_md_request(&assets, &uri)) {
        Some((bytes, mime)) => ok_bytes(mime, bytes),
        None => status_only(tauri::http::StatusCode::NOT_FOUND),
    };
    responder.respond(response);
}

/// `cmux-local-image://…?url=<file-url>` — serve an on-disk image jailed to the
/// calling webview's markdown-document directory.
fn local_image_protocol(
    ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let uri = request.uri().to_string();
    let markdown_file = ctx
        .app_handle()
        .try_state::<markdown::MarkdownState>()
        .map(|state| state.markdown_file_for(ctx.webview_label()))
        .unwrap_or_default();
    let response = match schemes::resolve_local_image_request(&uri, &markdown_file) {
        Some((path, mime)) => match std::fs::read(&path) {
            Ok(bytes) => ok_bytes(&mime, bytes),
            Err(_) => status_only(tauri::http::StatusCode::NOT_FOUND),
        },
        None => status_only(tauri::http::StatusCode::FORBIDDEN),
    };
    responder.respond(response);
}

/// `cmux-remote-image://…?url=<https>` — validate the outbound URL against the
/// SSRF gate. DEFERRED (host-layer network I/O): the DNS-pinned TLS fetch of an
/// admitted URL. An admitted URL currently responds `501 Not Implemented`; a
/// rejected URL responds `403`.
fn remote_image_protocol(
    _ctx: tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let uri = request.uri().to_string();
    let response = match schemes::remote_image_request(&uri) {
        Some(_admitted_url) => status_only(tauri::http::StatusCode::NOT_IMPLEMENTED),
        None => status_only(tauri::http::StatusCode::FORBIDDEN),
    };
    responder.respond(response);
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
