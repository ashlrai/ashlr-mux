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
    authenticate_client, build_v2_request, connect_pipe, interpret_v1_response,
    interpret_v2_response, read_frame, write_frame, MAX_RPC_FRAME_BYTES,
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
    let raw =
        String::from_utf8(frame).map_err(|_| CliError::new("response was not valid UTF-8"))?;
    interpret_v2_response(&raw).map_err(|error| CliError::new(error.to_string()))
}

/// Run one v1 text-protocol round-trip: connect, authenticate when `password`
/// is `Some`, send the raw command `line` (e.g. `focus_window <uuid>`), and
/// return the raw reply body. A reply starting with `ERROR:` fails with the
/// verbatim line as the error message, exactly as the macOS CLI surfaces it
/// (`sendV1Command`, CLI/cmux.swift:5952-5958).
pub async fn run_v1(
    socket_addr: &str,
    password: Option<&str>,
    line: &str,
) -> Result<String, CliError> {
    let client = connect_pipe(socket_addr, CONNECT_TIMEOUT)
        .await
        .map_err(|error| CliError::new(format!("could not connect to {socket_addr}: {error}")))?;
    let (mut reader, mut writer) = tokio::io::split(client);

    if let Some(password) = password {
        authenticate_client(&mut reader, &mut writer, password)
            .await
            .map_err(|error| CliError::new(format!("socket authentication failed: {error}")))?;
    }

    write_frame(&mut writer, line)
        .await
        .map_err(|error| CliError::new(format!("failed to send request: {error}")))?;

    let frame = read_frame(&mut reader, MAX_RPC_FRAME_BYTES)
        .await
        .map_err(|error| CliError::new(format!("failed to read response: {error}")))?
        .ok_or_else(|| CliError::new("connection closed before a response"))?;
    let raw =
        String::from_utf8(frame).map_err(|_| CliError::new("response was not valid UTF-8"))?;
    interpret_v1_response(&raw)
        .map(str::to_owned)
        .map_err(|error| CliError::new(error.0))
}

/// Run a v2 request that takes over the connection and returns raw NDJSON
/// stream frames instead of a single JSON-RPC response envelope.
pub async fn stream_rpc(
    socket_addr: &str,
    password: Option<&str>,
    method: &str,
    params: &serde_json::Value,
) -> Result<Vec<String>, CliError> {
    let mut frames = Vec::new();
    stream_rpc_with_handler(socket_addr, password, method, params, |frame| {
        frames.push(frame.to_string());
        Ok(true)
    })
    .await?;
    Ok(frames)
}

/// Run a streaming v2 request and invoke `on_frame` as each raw NDJSON frame is
/// received. Returning `Ok(false)` stops reading and closes the connection.
pub async fn stream_rpc_with_handler<F>(
    socket_addr: &str,
    password: Option<&str>,
    method: &str,
    params: &serde_json::Value,
    mut on_frame: F,
) -> Result<(), CliError>
where
    F: FnMut(&str) -> Result<bool, CliError>,
{
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

    while let Some(frame) = read_frame(&mut reader, MAX_RPC_FRAME_BYTES)
        .await
        .map_err(|error| CliError::new(format!("failed to read stream frame: {error}")))?
    {
        let raw = String::from_utf8(frame)
            .map_err(|_| CliError::new("stream frame was not valid UTF-8"))?;
        if !on_frame(&raw)? {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cmux_ipc::{
        control_pipe_path, serve_named_pipe, serve_named_pipe_authenticated, ControlCallResult,
        ControlRequest, ControlRequestHandler, ControlStream, JsonValue, PasswordAuthGate,
        PasswordVerifier,
    };

    /// A handler that echoes the request method back as `result.method`, so a
    /// round-trip can assert the request reached the server intact.
    fn echo_method(request: ControlRequest) -> ControlCallResult {
        let mut result = serde_json::Map::new();
        result.insert(
            "method".to_owned(),
            serde_json::Value::String(request.method),
        );
        ControlCallResult::Ok(JsonValue::Object(result))
    }

    struct StreamHandler;

    impl ControlRequestHandler for StreamHandler {
        fn handle(&mut self, request: ControlRequest) -> ControlCallResult {
            echo_method(request)
        }

        fn handle_stream(&mut self, request: ControlRequest) -> Option<ControlStream> {
            (request.method == "events.stream").then(|| {
                ControlStream::Frames(vec![
                    r#"{"type":"ack"}"#.to_string(),
                    r#"{"type":"heartbeat"}"#.to_string(),
                ])
            })
        }
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

    fn spawn_stream(addr: &str) {
        let addr = addr.to_owned();
        tokio::spawn(async move {
            let _ = serve_named_pipe(&addr, || StreamHandler).await;
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

    /// Spawn a raw text server at `addr` that answers every inbound line with
    /// `reply` (v1 protocol shape: one line in, one line out).
    fn spawn_raw_line_server(addr: &str, reply: &'static str) {
        let addr = addr.to_owned();
        tokio::spawn(async move {
            let server = tokio::net::windows::named_pipe::ServerOptions::new()
                .first_pipe_instance(true)
                .create(&addr)
                .expect("create pipe");
            server.connect().await.expect("connect");
            let (reader, mut writer) = tokio::io::split(server);
            let mut reader = tokio::io::BufReader::new(reader);
            while let Ok(Some(_line)) = read_frame(&mut reader, MAX_RPC_FRAME_BYTES).await {
                if write_frame(&mut writer, reply).await.is_err() {
                    break;
                }
            }
        });
    }

    #[tokio::test]
    async fn v1_round_trips_the_raw_reply_body() {
        let addr = test_addr("v1-ok");
        spawn_raw_line_server(&addr, "OK 44444444-4444-4444-8444-444444444444");
        let reply = run_v1(&addr, None, "new_window").await.expect("v1");
        assert_eq!(reply, "OK 44444444-4444-4444-8444-444444444444");
    }

    #[tokio::test]
    async fn v1_error_replies_become_verbatim_cli_errors() {
        let addr = test_addr("v1-err");
        spawn_raw_line_server(&addr, "ERROR: Window not found");
        let error = run_v1(&addr, None, "focus_window nope").await.unwrap_err();
        assert_eq!(error.message, "ERROR: Window not found");
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
    async fn stream_rpc_collects_raw_frames_until_eof() {
        let addr = test_addr("stream");
        spawn_stream(&addr);
        let frames = stream_rpc(&addr, None, "events.stream", &serde_json::json!({}))
            .await
            .expect("stream");
        assert_eq!(
            frames,
            vec![
                r#"{"type":"ack"}"#.to_string(),
                r#"{"type":"heartbeat"}"#.to_string()
            ]
        );
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
        assert!(
            error.message.starts_with("auth_required:"),
            "got: {}",
            error.message
        );
    }
}
