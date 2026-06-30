//! Windows named-pipe transport for socket-backed CLI commands (M4 WS5).
//!
//! Composes the cmux-ipc client primitives into one round-trip: connect to the
//! control pipe, run the password handshake when a password resolved, send the
//! v2 request frame, and interpret the single response. The socket address and
//! resolved password are supplied by the caller (the composition root reads the
//! env / files), so this stays a pure transport step testable against an
//! in-process [`cmux_ipc::serve_named_pipe`].

#![cfg(windows)]

use std::time::Duration;

use cmux_ipc::{
    authenticate_client, build_v2_request, connect_pipe, interpret_v2_response, read_frame,
    write_frame, MAX_RPC_FRAME_BYTES,
};

use crate::invocation::CliError;

/// How long to wait for the control pipe to accept a connection (covers the
/// app's first-instance startup race).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

/// Run one `rpc` round-trip against the control pipe at `socket_addr`: connect,
/// authenticate when `password` is `Some`, send `method`/`params`, and return
/// the response's `result` (or a [`CliError`] for a transport, auth, or v2-error
/// failure).
pub async fn run_rpc(
    socket_addr: &str,
    password: Option<&str>,
    method: &str,
    params: &serde_json::Value,
) -> Result<serde_json::Value, CliError> {
    let client = connect_pipe(socket_addr, CONNECT_TIMEOUT)
        .await
        .map_err(|error| CliError::new(format!("could not connect to {socket_addr}: {error}")))?;
    let (mut reader, mut writer) = tokio::io::split(client);

    if let Some(password) = password {
        authenticate_client(&mut reader, &mut writer, password)
            .await
            .map_err(|error| CliError::new(format!("socket authentication failed: {error}")))?;
    }

    let request = build_v2_request(method, params);
    write_frame(&mut writer, &request)
        .await
        .map_err(|error| CliError::new(format!("failed to send request: {error}")))?;

    let frame = read_frame(&mut reader, MAX_RPC_FRAME_BYTES)
        .await
        .map_err(|error| CliError::new(format!("failed to read response: {error}")))?
        .ok_or_else(|| CliError::new("connection closed before a response"))?;
    let raw = String::from_utf8(frame)
        .map_err(|_| CliError::new("response was not valid UTF-8"))?;
    interpret_v2_response(&raw).map_err(|error| CliError::new(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_ipc::{
        control_pipe_path, serve_named_pipe, serve_named_pipe_authenticated, ControlCallResult,
        ControlRequest, JsonValue, PasswordAuthGate, PasswordVerifier,
    };

    /// A handler that echoes the request method back as `result.method`, so a
    /// round-trip can assert the request reached the server intact.
    fn echo_method(request: ControlRequest) -> ControlCallResult {
        let mut result = serde_json::Map::new();
        result.insert("method".to_owned(), serde_json::Value::String(request.method));
        ControlCallResult::Ok(JsonValue::Object(result))
    }

    fn test_addr(tag: &str) -> String {
        control_pipe_path(&format!("cmux-cli-transport-{}-{tag}", std::process::id()))
            .expect("valid pipe name")
    }

    #[derive(Clone)]
    struct OnePassword(&'static str);
    impl PasswordVerifier for OnePassword {
        fn has_configured_password(&self) -> bool {
            true
        }
        fn verify(&self, password: &str) -> bool {
            password == self.0
        }
    }

    /// Spawn a no-auth echo server on a fresh pipe at `addr`.
    fn spawn_echo(addr: &str) {
        let addr = addr.to_owned();
        tokio::spawn(async move {
            let _ = serve_named_pipe(&addr, || echo_method).await;
        });
    }

    /// Spawn an auth-gated echo server (password `s3cret`) at `addr`.
    fn spawn_auth_echo(addr: &str) {
        let addr = addr.to_owned();
        tokio::spawn(async move {
            let gate = PasswordAuthGate::new(OnePassword("s3cret"));
            let _ = serve_named_pipe_authenticated(&addr, || echo_method, gate).await;
        });
    }

    #[tokio::test]
    async fn rpc_round_trips_result() {
        let addr = test_addr("ok");
        spawn_echo(&addr);
        let result = run_rpc(&addr, None, "surface.list", &serde_json::json!({}))
            .await
            .expect("rpc");
        assert_eq!(result, serde_json::json!({"method": "surface.list"}));
    }

    #[tokio::test]
    async fn rpc_authenticates_before_sending() {
        let addr = test_addr("auth");
        spawn_auth_echo(&addr);
        // With the right password the command dispatches past the auth gate.
        let result = run_rpc(&addr, Some("s3cret"), "ping", &serde_json::json!({}))
            .await
            .expect("rpc");
        assert_eq!(result, serde_json::json!({"method": "ping"}));
    }

    #[tokio::test]
    async fn rpc_without_password_against_auth_server_is_rejected() {
        let addr = test_addr("noauth");
        spawn_auth_echo(&addr);
        // No password → the gate rejects with an auth_required v2 error.
        let error = run_rpc(&addr, None, "ping", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(error.message.starts_with("auth_required:"), "got: {}", error.message);
    }
}
