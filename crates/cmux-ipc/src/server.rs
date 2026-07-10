//! Control-socket serve loop (M4 WS4).
//!
//! The transport core the Tauri app hosts for its in-app control socket — on
//! Windows over a named pipe, but written against generic async byte streams so
//! it is testable over an in-memory duplex on any OS (the named-pipe binding is
//! a thin Windows-only wrapper that feeds a `NamedPipeServer` into
//! [`serve_connection`]).
//!
//! The wire contract is the **unchanged v2 protocol**: newline-delimited JSON,
//! one request object per line, one response object per line, framed by bare
//! `\n` (a trailing `\r` is tolerated). Requests are parsed with the M1
//! [`ControlRequestParser`] and answered with the M1 [`ControlResponseEncoder`],
//! so the byte-level request/response shapes match the macOS server exactly.
//! Frames are bounded by [`MAX_RPC_FRAME_BYTES`] to mirror the daemon's
//! `maxRPCFrameBytes` and bound per-connection memory.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

use crate::{
    append_line,
    auth::{AuthState, ConnectionAuthenticator, NoAuth, PasswordAuthGate, PasswordVerifier},
    ControlCallResult, ControlRequest, ControlRequestParseError, ControlRequestParser,
    ControlResponseEncoder,
};

/// Maximum bytes in a single newline-framed RPC frame (4 MiB), mirroring the
/// daemon's `maxRPCFrameBytes` (`main.go:126`). A frame exceeding this closes the
/// connection with an error rather than buffering unboundedly.
pub const MAX_RPC_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// Raw NDJSON stream returned by a streaming control method.
pub enum ControlStream {
    /// Emit a bounded list of frames and close the connection.
    Frames(Vec<String>),
    /// Emit initial frames, then keep the connection open for live frames until
    /// the sender side is dropped or the client disconnects.
    Live {
        initial_frames: Vec<String>,
        receiver: mpsc::UnboundedReceiver<String>,
    },
}

/// Handles one parsed control request, producing the result to encode. The app
/// supplies this; the transport stays oblivious to the command surface.
pub trait ControlRequestHandler {
    /// Dispatch `request` and return its result.
    fn handle(&mut self, request: ControlRequest) -> ControlCallResult;

    /// Optionally take over the connection and emit raw NDJSON frames. Most
    /// methods are single-response RPCs; streaming methods such as
    /// `events.stream` use this hook so the transport does not wrap each frame
    /// in a JSON-RPC response envelope.
    fn handle_stream(&mut self, _request: ControlRequest) -> Option<ControlStream> {
        None
    }
}

impl<F> ControlRequestHandler for F
where
    F: FnMut(ControlRequest) -> ControlCallResult,
{
    fn handle(&mut self, request: ControlRequest) -> ControlCallResult {
        self(request)
    }
}

/// Run the serve loop for one accepted connection: read newline-framed request
/// lines, dispatch each through `handler`, and write one framed response line
/// per request. Returns when the peer reaches EOF (`Ok`) or an I/O error occurs.
///
/// A malformed request (bad UTF-8 / JSON / shape) yields a protocol error
/// response and the connection continues, matching the macOS server's
/// per-line resilience.
pub async fn serve_connection<R, W, H>(reader: R, writer: W, handler: H) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    H: ControlRequestHandler,
{
    serve_connection_inner(reader, writer, handler, NoAuth).await
}

/// Like [`serve_connection`] but gates every command behind a password auth
/// handshake (`auth <password>` / `auth.login`). Until the connection
/// authenticates, non-auth commands are answered with an `auth_required`
/// rejection; once it authenticates the flag sticks for the connection's life.
/// Use this when the socket access mode requires a password
/// (`accessMode.requiresPasswordAuth`); otherwise use [`serve_connection`].
pub async fn serve_connection_authenticated<R, W, H, V>(
    reader: R,
    writer: W,
    handler: H,
    gate: PasswordAuthGate<V>,
) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    H: ControlRequestHandler,
    V: PasswordVerifier,
{
    serve_connection_inner(reader, writer, handler, gate).await
}

/// Shared serve loop for the authenticated and unauthenticated paths. `auth`
/// intercepts each decoded command line: when it returns a response that line is
/// answered directly (and not dispatched); otherwise the request is parsed and
/// handed to `handler`.
async fn serve_connection_inner<R, W, H, A>(
    reader: R,
    mut writer: W,
    mut handler: H,
    auth: A,
) -> std::io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    H: ControlRequestHandler,
    A: ConnectionAuthenticator,
{
    let mut reader = BufReader::new(reader);
    let encoder = ControlResponseEncoder;
    let parser = ControlRequestParser;
    let mut auth_state = AuthState::default();

    while let Some(frame) = read_frame(&mut reader, MAX_RPC_FRAME_BYTES).await? {
        let response = match std::str::from_utf8(&frame) {
            Err(_) => encoder.response_for_parse_error(ControlRequestParseError::InvalidUtf8),
            Ok(line) => match auth.intercept(line, &mut auth_state) {
                Some(auth_response) => auth_response,
                None => match parser.request(line) {
                    Ok(request) => {
                        if let Some(stream) = handler.handle_stream(request.clone()) {
                            write_stream(&mut writer, stream).await?;
                            return Ok(());
                        }
                        let id = request.id.clone();
                        encoder.response(id, handler.handle(request))
                    }
                    Err(error) => encoder.response_for_parse_error(error),
                },
            },
        };
        writer.write_all(append_line(&response).as_bytes()).await?;
        writer.flush().await?;
    }
    Ok(())
}

async fn write_stream<W>(writer: &mut W, stream: ControlStream) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    match stream {
        ControlStream::Frames(frames) => {
            for frame in frames {
                write_frame(writer, &frame).await?;
            }
        }
        ControlStream::Live {
            initial_frames,
            mut receiver,
        } => {
            for frame in initial_frames {
                write_frame(writer, &frame).await?;
            }
            while let Some(frame) = receiver.recv().await {
                write_frame(writer, &frame).await?;
            }
        }
    }
    Ok(())
}

/// Read one newline-framed line (without the `\n`, trailing `\r` stripped) from
/// `reader`, bounded to `max_bytes`. Returns `Ok(None)` at EOF (no trailing
/// partial line is returned — unterminated bytes are discarded, matching the
/// macOS reader). Errors with `InvalidData` if the frame would exceed
/// `max_bytes`.
pub async fn read_frame<R>(reader: &mut R, max_bytes: usize) -> std::io::Result<Option<Vec<u8>>>
where
    R: AsyncRead + Unpin,
{
    let mut line: Vec<u8> = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        let read = reader.read(&mut byte).await?;
        if read == 0 {
            // EOF: discard any unterminated remainder (legacy behavior).
            return Ok(None);
        }
        match byte[0] {
            b'\n' => {
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(line));
            }
            other => {
                if line.len() >= max_bytes {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "control frame exceeds maximum size",
                    ));
                }
                line.push(other);
            }
        }
    }
}

/// Write one request/response `line` as a wire frame (append `\n`) to `writer`
/// and flush. A convenience for clients and tests.
pub async fn write_frame<W>(writer: &mut W, line: &str) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    writer.write_all(append_line(line).as_bytes()).await?;
    writer.flush().await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::JsonValue;
    use tokio::io::{duplex, AsyncWriteExt};

    /// One-password verifier for the authenticated serve-loop test.
    struct OnePassword(&'static str);
    impl PasswordVerifier for OnePassword {
        fn has_configured_password(&self) -> bool {
            true
        }
        fn verify(&self, password: &str) -> bool {
            password == self.0
        }
    }

    /// Drive the authenticated serve loop over a duplex and collect responses.
    async fn round_trip_authenticated(
        gate: PasswordAuthGate<OnePassword>,
        requests: &[&str],
    ) -> Vec<String> {
        let (mut client, server) = duplex(64 * 1024);
        let (server_reader, server_writer) = tokio::io::split(server);
        let server_task = tokio::spawn(async move {
            serve_connection_authenticated(server_reader, server_writer, echo_handler, gate).await
        });

        for request in requests {
            write_frame(&mut client, request).await.expect("write");
        }
        client.shutdown().await.expect("shutdown");

        let mut responses = Vec::new();
        while let Some(frame) = read_frame(&mut client, MAX_RPC_FRAME_BYTES)
            .await
            .expect("read")
        {
            responses.push(String::from_utf8(frame).expect("utf8"));
        }
        server_task.await.expect("join").expect("serve");
        responses
    }

    #[tokio::test]
    async fn authenticated_loop_rejects_then_admits_after_auth() {
        let responses = round_trip_authenticated(
            PasswordAuthGate::new(OnePassword("s3cret")),
            &[
                r#"{"id":1,"method":"surface.list"}"#, // before auth → rejected
                "auth s3cret",                         // authenticate
                r#"{"id":2,"method":"ping"}"#,         // after auth → dispatched
            ],
        )
        .await;
        assert_eq!(responses.len(), 3);

        let first: serde_json::Value = serde_json::from_str(&responses[0]).unwrap();
        assert_eq!(first["error"]["code"], serde_json::json!("auth_required"));

        assert_eq!(responses[1], "OK: Authenticated");

        let third: serde_json::Value = serde_json::from_str(&responses[2]).unwrap();
        assert_eq!(third["ok"], serde_json::json!(true));
        assert_eq!(third["result"], serde_json::json!("ping"));
    }

    /// Drive a handler over an in-memory duplex: spawn the server on one end,
    /// send `requests` from the other, and collect the response lines.
    ///
    /// NOTE: we `shutdown()` the client's *write* direction to signal EOF to the
    /// server — dropping a `tokio::io::split` write half would NOT close the
    /// duplex (the read half keeps it open), so the server loop would hang.
    async fn round_trip<H>(handler: H, requests: &[&str]) -> Vec<String>
    where
        H: ControlRequestHandler + Send + 'static,
    {
        let (mut client, server) = duplex(64 * 1024);
        let (server_reader, server_writer) = tokio::io::split(server);
        let server_task =
            tokio::spawn(
                async move { serve_connection(server_reader, server_writer, handler).await },
            );

        for request in requests {
            write_frame(&mut client, request).await.expect("write");
        }
        client.shutdown().await.expect("shutdown"); // half-close → server sees EOF

        let mut responses = Vec::new();
        while let Some(frame) = read_frame(&mut client, MAX_RPC_FRAME_BYTES)
            .await
            .expect("read")
        {
            responses.push(String::from_utf8(frame).expect("utf8"));
        }
        server_task.await.expect("join").expect("serve");
        responses
    }

    fn echo_handler(request: ControlRequest) -> ControlCallResult {
        ControlCallResult::Ok(JsonValue::String(request.method))
    }

    struct StreamHandler;

    impl ControlRequestHandler for StreamHandler {
        fn handle(&mut self, request: ControlRequest) -> ControlCallResult {
            ControlCallResult::Ok(JsonValue::String(request.method))
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

    struct LiveStreamHandler;

    impl ControlRequestHandler for LiveStreamHandler {
        fn handle(&mut self, request: ControlRequest) -> ControlCallResult {
            ControlCallResult::Ok(JsonValue::String(request.method))
        }

        fn handle_stream(&mut self, request: ControlRequest) -> Option<ControlStream> {
            if request.method != "events.stream" {
                return None;
            }
            let (sender, receiver) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                let _ = sender.send(r#"{"type":"event","seq":1}"#.to_string());
            });
            Some(ControlStream::Live {
                initial_frames: vec![r#"{"type":"ack"}"#.to_string()],
                receiver,
            })
        }
    }

    #[tokio::test]
    async fn dispatches_request_and_frames_response() {
        let responses = round_trip(echo_handler, &[r#"{"id":1,"method":"ping"}"#]).await;
        assert_eq!(responses.len(), 1);
        let value: serde_json::Value = serde_json::from_str(&responses[0]).expect("json");
        assert_eq!(value["id"], serde_json::json!(1));
        assert_eq!(value["ok"], serde_json::json!(true));
        assert_eq!(value["result"], serde_json::json!("ping"));
    }

    #[tokio::test]
    async fn streaming_handler_takes_over_connection_with_raw_frames() {
        let responses = round_trip(
            StreamHandler,
            &[r#"{"id":1,"method":"events.stream","params":{}}"#],
        )
        .await;
        assert_eq!(
            responses,
            vec![
                r#"{"type":"ack"}"#.to_string(),
                r#"{"type":"heartbeat"}"#.to_string()
            ]
        );
    }

    #[tokio::test]
    async fn live_stream_handler_keeps_connection_for_receiver_frames() {
        let responses = round_trip(
            LiveStreamHandler,
            &[r#"{"id":1,"method":"events.stream","params":{}}"#],
        )
        .await;
        assert_eq!(
            responses,
            vec![
                r#"{"type":"ack"}"#.to_string(),
                r#"{"type":"event","seq":1}"#.to_string()
            ]
        );
    }

    #[tokio::test]
    async fn handles_multiple_pipelined_requests_in_order() {
        let responses = round_trip(
            echo_handler,
            &[
                r#"{"id":1,"method":"a"}"#,
                r#"{"id":2,"method":"b"}"#,
                r#"{"id":"x","method":"c"}"#,
            ],
        )
        .await;
        let ids: Vec<serde_json::Value> = responses
            .iter()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["id"].clone())
            .collect();
        assert_eq!(
            ids,
            vec![
                serde_json::json!(1),
                serde_json::json!(2),
                serde_json::json!("x")
            ]
        );
    }

    #[tokio::test]
    async fn malformed_json_yields_error_response_and_continues() {
        let responses = round_trip(
            echo_handler,
            &["not json at all", r#"{"id":2,"method":"ok"}"#],
        )
        .await;
        assert_eq!(responses.len(), 2, "both lines answered");
        let first: serde_json::Value = serde_json::from_str(&responses[0]).unwrap();
        assert_eq!(first["ok"], serde_json::json!(false));
        let second: serde_json::Value = serde_json::from_str(&responses[1]).unwrap();
        assert_eq!(second["ok"], serde_json::json!(true));
        assert_eq!(second["result"], serde_json::json!("ok"));
    }

    #[tokio::test]
    async fn handler_error_result_is_encoded() {
        let handler = |_req: ControlRequest| ControlCallResult::Err {
            code: "boom".into(),
            message: "nope".into(),
            data: None,
        };
        let responses = round_trip(handler, &[r#"{"id":7,"method":"x"}"#]).await;
        let value: serde_json::Value = serde_json::from_str(&responses[0]).unwrap();
        assert_eq!(value["id"], serde_json::json!(7));
        assert_eq!(value["ok"], serde_json::json!(false));
        assert_eq!(value["error"]["code"], serde_json::json!("boom"));
    }

    #[tokio::test]
    async fn crlf_framing_is_accepted() {
        let (mut client, server) = duplex(1024);
        let (sr, sw) = tokio::io::split(server);
        let task = tokio::spawn(async move { serve_connection(sr, sw, echo_handler).await });
        client
            .write_all(b"{\"id\":1,\"method\":\"crlf\"}\r\n")
            .await
            .unwrap();
        client.flush().await.unwrap();
        client.shutdown().await.unwrap(); // half-close → server sees EOF
        let frame = read_frame(&mut client, MAX_RPC_FRAME_BYTES)
            .await
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&frame).unwrap();
        assert_eq!(value["result"], serde_json::json!("crlf"));
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected() {
        let (mut a, mut b) = duplex(1024);
        // No newline within the cap → read_frame errors.
        let big = vec![b'x'; 64];
        tokio::spawn(async move {
            let _ = a.write_all(&big).await;
            // keep `a` alive so the reader sees data, then drop to avoid hang
        });
        let result = read_frame(&mut b, 16).await;
        assert!(result.is_err(), "frame over the cap must error");
    }

    #[tokio::test]
    async fn eof_without_newline_returns_none() {
        let (mut a, mut b) = duplex(1024);
        tokio::spawn(async move {
            a.write_all(b"partial-no-newline").await.unwrap();
            drop(a);
        });
        // Drain until EOF; the unterminated remainder is discarded → None.
        while read_frame(&mut b, MAX_RPC_FRAME_BYTES)
            .await
            .unwrap()
            .is_some()
        {}
    }
}
