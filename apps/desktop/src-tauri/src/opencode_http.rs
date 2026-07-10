//! OpenCode HTTP-loopback client (the concrete transport for the `opencode`
//! provider on Windows).
//!
//! The pure [`cmux_agent_chat`] store discovers the loopback URL from the child's
//! stdout and emits [`TransportAction`](cmux_agent_chat::TransportAction)s
//! describing three HTTP calls; this module performs them with the blocking
//! [`ureq`] client (no tokio), faithfully to the canonical macOS
//! `AgentSessionProcessStore` OpenCode paths:
//!
//! | call | method | path | body | trigger |
//! |------|--------|------|------|---------|
//! | create | POST | `/session` | `{}` | loopback URL first sniffed |
//! | prompt | POST | `/session/{id}/prompt_async` | `{"parts":[{"type":"text","text":…}]}` | each `writeLine` |
//! | events | GET | `/event` | — | after create returns a valid `id` |
//!
//! Every call carries `?directory=<workingDirectory>` when a working directory is
//! set, and an `Authorization` header when the launch env minted OpenCode
//! Basic-auth credentials. All three timings + the URL/percent-encoding shape are
//! ported from Swift `openCodeURL` / `postJSON` / `startOpenCodeEventStream`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

/// Swift `postJSON` timeout (`timeoutInterval = 30`).
const POST_TIMEOUT: Duration = Duration::from_secs(30);
/// Swift event-stream timeout (`timeoutInterval = 3600`).
const EVENT_READ_TIMEOUT: Duration = Duration::from_secs(3600);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Host-side per-session context for the OpenCode HTTP transport.
///
/// Holds the launch-env-derived auth header + working directory (kept OUT of the
/// pure store — secrets never enter `cmux-agent-chat`) and the two liveness flags
/// the event-stream failure rule needs.
pub struct OpenCodeContext {
    /// The `Authorization` header value (`Basic …`), if the launch env minted
    /// credentials (`cmux_agent::OpenCodeServerAuth`).
    pub auth_header: Option<String>,
    /// The working directory attached as `?directory=` to every call.
    pub working_directory: Option<String>,
    /// Set when the session is being torn down (stop / terminate); the event
    /// stream stops without treating the disconnect as a failure.
    pub cancelled: Arc<AtomicBool>,
    /// Cleared by the child's reader thread the instant its pipes close (process
    /// exit). An event-stream EOF while this is still `true` is a real failure
    /// (Swift `openCodeEventStreamEOFRequiresFailure = !isCancelled && processIsRunning`).
    pub process_running: Arc<AtomicBool>,
}

/// The shared registry of OpenCode contexts, keyed by store session id.
pub type OpenCodeContexts = Arc<Mutex<HashMap<String, OpenCodeContext>>>;

/// How a `/event` stream ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamEnd {
    /// The stream reached a clean EOF.
    Eof,
    /// The stream failed (non-2xx status, connect error, or a read error).
    Errored,
}

/// Percent-encode a string, escaping everything outside the RFC 3986 unreserved
/// set (`ALPHA / DIGIT / - . _ ~`). Used for path segments (the loopback session
/// id) and the `directory` query value (Windows paths carry `\`, `:`, spaces).
fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for &byte in input.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Build an OpenCode request URL (Swift `openCodeURL`): `{base}/{path}` with a
/// single `?directory=<encoded wd>` query when a working directory is present.
/// `path` is appended verbatim (its dynamic segments are pre-encoded by the
/// caller).
fn build_url(base_url: &str, path: &str, working_directory: Option<&str>) -> String {
    let base = base_url.trim_end_matches('/');
    let mut url = format!("{base}/{path}");
    if let Some(working_directory) = working_directory {
        url.push_str("?directory=");
        url.push_str(&percent_encode(working_directory));
    }
    url
}

fn post_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(POST_TIMEOUT)
        .timeout_write(POST_TIMEOUT)
        .build()
}

/// `POST` a JSON body (Swift `postJSON`): `Content-Type: application/json`,
/// optional `Authorization`, 2xx-or-error, empty body → `{}`, decode a JSON
/// object (else `{}`).
fn post_json(url: &str, auth: Option<&str>, body: &Value) -> Result<Value, String> {
    let mut request = post_agent()
        .post(url)
        .set("Content-Type", "application/json");
    if let Some(auth) = auth {
        request = request.set("Authorization", auth);
    }
    let body = serde_json::to_string(body).map_err(|error| error.to_string())?;
    match request.send_string(&body) {
        Ok(response) => {
            let text = response.into_string().unwrap_or_default();
            if text.trim().is_empty() {
                return Ok(json!({}));
            }
            match serde_json::from_str::<Value>(&text) {
                Ok(value @ Value::Object(_)) => Ok(value),
                _ => Ok(json!({})),
            }
        }
        // ureq returns `Err(Status)` for any non-2xx — Swift's `200..<300` guard.
        Err(ureq::Error::Status(code, _)) => Err(format!("HTTP {code}")),
        Err(error) => Err(error.to_string()),
    }
}

/// Create the loopback session (`POST /session` with body `{}`), returning the
/// created session id (top-level non-empty `id`) or an error.
pub fn create_session(
    base_url: &str,
    auth: Option<&str>,
    working_directory: Option<&str>,
) -> Result<String, String> {
    let url = build_url(base_url, "session", working_directory);
    let value = post_json(&url, auth, &json!({}))?;
    value
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "OpenCode session response missing id".to_string())
}

/// Submit a prompt (`POST /session/{id}/prompt_async`) with the single text part.
/// Fire-and-forget: the assistant reply arrives over the `/event` stream.
pub fn post_prompt(
    base_url: &str,
    auth: Option<&str>,
    working_directory: Option<&str>,
    opencode_session_id: &str,
    text: &str,
) -> Result<(), String> {
    let path = format!(
        "session/{}/prompt_async",
        percent_encode(opencode_session_id)
    );
    let url = build_url(base_url, &path, working_directory);
    let body = json!({ "parts": [{ "type": "text", "text": text }] });
    post_json(&url, auth, &body).map(|_| ())
}

/// Open the `/event` SSE stream (`GET /event`) and pump each line to `on_line`
/// until EOF, a read error, or cancellation. Returns how the stream ended.
///
/// Mirrors Swift `startOpenCodeEventStream`: status must be 2xx, the body is read
/// line-by-line (each line handed to the ported SSE parser via `on_line`), and
/// `is_cancelled` is checked before each line so a stop request ends the loop
/// without reporting failure.
pub fn stream_events(
    base_url: &str,
    auth: Option<&str>,
    working_directory: Option<&str>,
    is_cancelled: &dyn Fn() -> bool,
    on_line: &mut dyn FnMut(String),
) -> StreamEnd {
    let url = build_url(base_url, "event", working_directory);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CONNECT_TIMEOUT)
        .timeout_read(EVENT_READ_TIMEOUT)
        .build();
    let mut request = agent.get(&url);
    if let Some(auth) = auth {
        request = request.set("Authorization", auth);
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(_) => return StreamEnd::Errored,
    };
    let reader = BufReader::new(response.into_reader());
    for line in reader.lines() {
        if is_cancelled() {
            return StreamEnd::Eof;
        }
        match line {
            Ok(line) => on_line(line),
            Err(_) => return StreamEnd::Errored,
        }
    }
    StreamEnd::Eof
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::Ordering;
    use std::thread;

    #[test]
    fn percent_encode_escapes_reserved_and_keeps_unreserved() {
        assert_eq!(percent_encode("abcXYZ019-._~"), "abcXYZ019-._~");
        // A Windows path: backslashes, colon, and space are all escaped.
        assert_eq!(percent_encode(r"C:\work dir"), "C%3A%5Cwork%20dir");
    }

    #[test]
    fn build_url_appends_directory_query_only_when_present() {
        assert_eq!(
            build_url("http://127.0.0.1:4096/", "session", None),
            "http://127.0.0.1:4096/session"
        );
        assert_eq!(
            build_url("http://127.0.0.1:4096", "event", Some("/tmp/x")),
            "http://127.0.0.1:4096/event?directory=%2Ftmp%2Fx"
        );
    }

    /// Whether `data` holds a complete HTTP request: the header terminator plus a
    /// body of at least the declared `Content-Length` (0 if none).
    fn request_is_complete(data: &[u8]) -> bool {
        let text = String::from_utf8_lossy(data);
        let Some(header_end) = text.find("\r\n\r\n") else {
            return false;
        };
        let content_length = text
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.trim()
                    .eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        let body_len = data.len() - (header_end + 4);
        body_len >= content_length
    }

    /// A one-shot HTTP/1.1 stub that returns `status`/`body` for the first
    /// connection, capturing the request line + body it received.
    fn serve_once(status: &str, body: &str) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().unwrap();
        let captured = Arc::new(Mutex::new(String::new()));
        let captured_worker = captured.clone();
        let status = status.to_string();
        let body = body.to_string();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Read the whole request: headers may arrive in a separate TCP
                // segment from the body, so loop until the declared Content-Length
                // body has been received (or a short read timeout elapses).
                let _ = stream.set_read_timeout(Some(Duration::from_millis(500)));
                let mut data: Vec<u8> = Vec::new();
                let mut buf = [0u8; 1024];
                loop {
                    match stream.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            data.extend_from_slice(&buf[..n]);
                            if request_is_complete(&data) {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                *captured_worker.lock().unwrap() = String::from_utf8_lossy(&data).to_string();
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (format!("http://{addr}"), captured)
    }

    #[test]
    fn create_session_parses_id_and_sends_empty_body_with_auth() {
        let (base, captured) = serve_once("200 OK", r#"{"id":"oc-42"}"#);
        let id = create_session(&base, Some("Basic zzz"), Some("/w")).expect("create");
        assert_eq!(id, "oc-42");
        let request = captured.lock().unwrap().clone();
        assert!(
            request.starts_with("POST /session?directory=%2Fw "),
            "{request}"
        );
        assert!(request.contains("Authorization: Basic zzz"), "{request}");
        assert!(
            request.contains("Content-Type: application/json"),
            "{request}"
        );
        assert!(request.trim_end().ends_with("{}"), "empty body: {request}");
    }

    #[test]
    fn create_session_missing_id_is_error() {
        let (base, _c) = serve_once("200 OK", r#"{"nope":1}"#);
        assert!(create_session(&base, None, None).is_err());
    }

    #[test]
    fn create_session_non_2xx_is_error() {
        let (base, _c) = serve_once("500 Internal Server Error", "boom");
        assert!(create_session(&base, None, None).is_err());
    }

    #[test]
    fn post_prompt_targets_prompt_async_with_text_part() {
        let (base, captured) = serve_once("200 OK", "");
        post_prompt(&base, None, None, "oc-1", "hello there").expect("prompt");
        let request = captured.lock().unwrap().clone();
        assert!(
            request.starts_with("POST /session/oc-1/prompt_async "),
            "{request}"
        );
        // serde_json serializes object keys alphabetically ("text" before "type").
        assert!(request.contains(r#""text":"hello there""#), "{request}");
        assert!(request.contains(r#""type":"text""#), "{request}");
        assert!(request.contains(r#""parts":["#), "{request}");
    }

    #[test]
    fn stream_events_pumps_lines_until_eof() {
        // Serve an SSE-ish body then close (EOF).
        let (base, _c) = serve_once("200 OK", "data: {\"a\":1}\n\ndata: {\"b\":2}\n\n");
        let mut lines = Vec::new();
        let cancelled = AtomicBool::new(false);
        let end = stream_events(
            &base,
            None,
            None,
            &|| cancelled.load(Ordering::SeqCst),
            &mut |line| lines.push(line),
        );
        assert_eq!(end, StreamEnd::Eof);
        assert!(lines.iter().any(|l| l.contains("\"a\":1")), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("\"b\":2")), "{lines:?}");
    }

    #[test]
    #[ignore = "manual diagnostic: hits a live opencode server on 127.0.0.1:4599"]
    fn live_opencode_streams_incrementally() {
        // base64("opencode:testpass")
        let auth = Some("Basic b3BlbmNvZGU6dGVzdHBhc3M=");
        let base = "http://127.0.0.1:4599";
        let sid = create_session(base, auth, None).expect("create session");
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let cancelled = Arc::new(AtomicBool::new(false));
        let lines_w = lines.clone();
        let cancelled_r = cancelled.clone();
        let base_owned = base.to_string();
        std::thread::spawn(move || {
            let mut on_line = |line: String| lines_w.lock().unwrap().push(line);
            stream_events(
                &base_owned,
                Some("Basic b3BlbmNvZGU6dGVzdHBhc3M="),
                None,
                &|| cancelled_r.load(Ordering::SeqCst),
                &mut on_line,
            )
        });
        std::thread::sleep(Duration::from_millis(500));
        let count_before_prompt = lines.lock().unwrap().len();
        post_prompt(base, auth, None, &sid, "hello").expect("prompt");
        std::thread::sleep(Duration::from_secs(8));
        cancelled.store(true, Ordering::SeqCst);
        let received = lines.lock().unwrap();
        eprintln!(
            "lines before prompt = {count_before_prompt}, total = {}, sample = {:?}",
            received.len(),
            received.iter().take(3).collect::<Vec<_>>()
        );
        assert!(
            received.len() > count_before_prompt,
            "ureq streamed NO new SSE lines after the prompt (buffering the chunked body?)"
        );
    }

    #[test]
    fn stream_events_non_2xx_is_errored() {
        let (base, _c) = serve_once("404 Not Found", "nope");
        let cancelled = AtomicBool::new(false);
        let end = stream_events(
            &base,
            None,
            None,
            &|| cancelled.load(Ordering::SeqCst),
            &mut |_| {},
        );
        assert_eq!(end, StreamEnd::Errored);
    }
}
