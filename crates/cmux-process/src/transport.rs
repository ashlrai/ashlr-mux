//! Stdio transport framing for the agent wire protocols (M3 WS3).
//!
//! Claude (`stdio-jsonl`) and Codex (`stdio-jsonrpc`) both speak
//! **newline-delimited JSON** over stdio — confirmed against the macOS readers:
//! `AgentSessionOutputLineBuffer` splits stdout on `0x0A`, and
//! `CodexAppServerSession` accumulates a line buffer and dispatches one JSON
//! object per line (NOT LSP `Content-Length` framing). So a single incremental
//! line framer serves both stdio transports; the JSON-RPC request/response
//! correlation layered on top is provider-specific and lives elsewhere.
//!
//! ## Strict UTF-8, fail loud (cross-cutting rule 5)
//!
//! The macOS side decodes lossily (`String(decoding:as:UTF8.self)`), silently
//! substituting U+FFFD. The Windows port must instead decode frames as **strict
//! UTF-8 and surface decode errors**, because a single mis-decoded byte (e.g.
//! CP437 mojibake from a mis-configured child console) corrupts a JSON-RPC /
//! stream-json frame invisibly. This is safe to do at the line boundary: `\n`
//! (0x0A) is ASCII and can never appear inside a multibyte UTF-8 sequence, so a
//! completed line is always a whole sequence of characters — invalid UTF-8
//! within it is genuine corruption, not a chunk boundary artifact.
//!
//! The framer therefore yields `Result<String, FrameError>` per line: `Ok` for a
//! clean frame, `Err(InvalidUtf8)` for corruption, `Err(Oversized)` when an
//! unterminated line exceeds the buffer cap (the safety valve mirroring the
//! macOS 1 MiB cap that prevents unbounded buffering of a never-terminated
//! line).

/// Default maximum bytes buffered for a single unterminated line before the
/// framer gives up and emits [`FrameError::Oversized`]. Mirrors the macOS
/// `AgentSessionOutputLineBuffer.maxBufferedBytes`.
pub const DEFAULT_MAX_LINE_BYTES: usize = 1024 * 1024;

/// A framing failure for one line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// The completed line was not valid UTF-8 — likely console code-page
    /// corruption (rule 5). Carries the line length and the prefix length that
    /// *was* valid, for diagnostics.
    #[error("frame is not valid UTF-8 ({byte_count} bytes, valid up to {valid_up_to})")]
    InvalidUtf8 {
        /// Total bytes in the offending line.
        byte_count: usize,
        /// Number of leading bytes that formed valid UTF-8.
        valid_up_to: usize,
    },
    /// An unterminated line exceeded the buffer cap and was dropped. Carries the
    /// number of bytes discarded.
    #[error("unterminated line exceeded {byte_count}-byte cap and was dropped")]
    Oversized {
        /// Bytes discarded from the over-long line.
        byte_count: usize,
    },
}

/// Incremental newline-delimited frame splitter.
///
/// Feed arbitrary byte chunks from a child's stdout via [`push`](Self::push); it
/// returns the frames that completed in this chunk, buffering any trailing
/// partial line until the next chunk. Call [`flush`](Self::flush) at EOF to emit
/// a final unterminated line, if any.
#[derive(Debug)]
pub struct LineFramer {
    buffer: Vec<u8>,
    max_line_bytes: usize,
}

impl Default for LineFramer {
    fn default() -> Self {
        Self::new()
    }
}

impl LineFramer {
    /// Create a framer with the default [`DEFAULT_MAX_LINE_BYTES`] cap.
    pub fn new() -> Self {
        Self::with_max_line_bytes(DEFAULT_MAX_LINE_BYTES)
    }

    /// Create a framer with a custom unterminated-line cap (must be ≥ 1).
    pub fn with_max_line_bytes(max_line_bytes: usize) -> Self {
        Self {
            buffer: Vec::new(),
            max_line_bytes: max_line_bytes.max(1),
        }
    }

    /// Bytes currently buffered for the in-progress (unterminated) line.
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    /// Feed a chunk of stdout bytes; returns every line that completed within
    /// this chunk (in order), each strict-UTF-8 decoded.
    pub fn push(&mut self, mut bytes: &[u8]) -> Vec<Result<String, FrameError>> {
        let mut frames = Vec::new();
        while let Some(newline) = bytes.iter().position(|&byte| byte == b'\n') {
            let (head, rest) = bytes.split_at(newline);
            bytes = &rest[1..]; // drop the '\n'
            self.append_bounded(head, &mut frames);
            let line = std::mem::take(&mut self.buffer);
            frames.push(decode_line(line));
        }
        // Remaining bytes have no newline yet — buffer them (bounded).
        self.append_bounded(bytes, &mut frames);
        frames
    }

    /// Emit any buffered unterminated line (at EOF). Returns `None` if the
    /// buffer is empty.
    pub fn flush(&mut self) -> Option<Result<String, FrameError>> {
        if self.buffer.is_empty() {
            return None;
        }
        let line = std::mem::take(&mut self.buffer);
        Some(decode_line(line))
    }

    /// Append `data` to the line buffer, emitting [`FrameError::Oversized`] and
    /// resetting whenever the buffer would exceed the cap before a newline. This
    /// bounds memory against a never-terminated line (or a single line larger
    /// than the cap).
    fn append_bounded(&mut self, mut data: &[u8], frames: &mut Vec<Result<String, FrameError>>) {
        while self.buffer.len() + data.len() > self.max_line_bytes {
            let take = self.max_line_bytes - self.buffer.len();
            self.buffer.extend_from_slice(&data[..take]);
            frames.push(Err(FrameError::Oversized {
                byte_count: self.buffer.len(),
            }));
            self.buffer.clear();
            data = &data[take..];
        }
        self.buffer.extend_from_slice(data);
    }
}

/// Strict-UTF-8 decode one completed line, trimming a single trailing `\r` so
/// `\r\n`-terminated frames decode cleanly.
fn decode_line(mut bytes: Vec<u8>) -> Result<String, FrameError> {
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    let byte_count = bytes.len();
    String::from_utf8(bytes).map_err(|error| FrameError::InvalidUtf8 {
        byte_count,
        valid_up_to: error.utf8_error().valid_up_to(),
    })
}

/// Encode `line` as one NDJSON wire frame: the content followed by a single
/// `\n`. Used by the stdin write path for both stdio transports.
pub fn encode_line(line: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(line.len() + 1);
    bytes.extend_from_slice(line.as_bytes());
    bytes.push(b'\n');
    bytes
}

/// Scrape the OpenCode HTTP-loopback base URL from a `serve --print-logs` line.
///
/// OpenCode (launched with `serve --hostname 127.0.0.1 --port 0 --print-logs`)
/// chooses an ephemeral port and announces it on stdout as
/// `opencode server listening on <url>`. We extract the first token after that
/// marker and accept it only if it is an `http(s)` URL on a loopback host
/// (`localhost` / `127.0.0.1` / `::1`) — verbatim parity with the macOS
/// `AgentSessionProcessStore.openCodeServerURL` + `agentSessionIsLoopbackURL`.
/// Returns the validated base URL, or `None` if the line isn't the announcement
/// or the host isn't loopback (a guard against connecting off-box).
pub fn opencode_server_url(line: &str) -> Option<String> {
    const MARKER: &str = "opencode server listening on ";
    let marker_at = line.find(MARKER)?;
    let candidate = line[marker_at + MARKER.len()..]
        .split_whitespace()
        .next()?;
    if is_loopback_http_url(candidate) {
        Some(candidate.to_string())
    } else {
        None
    }
}

/// Whether `raw` is an `http`/`https` URL whose host is a loopback address.
fn is_loopback_http_url(raw: &str) -> bool {
    loopback_candidate_host(raw).is_some_and(|host| {
        let host = host.to_ascii_lowercase();
        host == "localhost" || host == "127.0.0.1" || host == "::1"
    })
}

/// Extract the host from an `http(s)://[user@]host[:port][/…]` URL, handling a
/// bracketed IPv6 literal (`[::1]`). `None` if the scheme isn't http(s).
fn loopback_candidate_host(raw: &str) -> Option<&str> {
    let rest = raw
        .strip_prefix("http://")
        .or_else(|| raw.strip_prefix("https://"))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    let authority = authority.rsplit('@').next().unwrap_or(authority); // drop userinfo
    let host = if let Some(after_bracket) = authority.strip_prefix('[') {
        after_bracket.split(']').next().unwrap_or(after_bracket) // IPv6 literal
    } else {
        authority.split(':').next().unwrap_or(authority) // strip :port
    };
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oks(frames: Vec<Result<String, FrameError>>) -> Vec<String> {
        frames.into_iter().map(|frame| frame.expect("frame")).collect()
    }

    #[test]
    fn splits_complete_lines_in_one_chunk() {
        let mut framer = LineFramer::new();
        let frames = framer.push(b"{\"a\":1}\n{\"b\":2}\n");
        assert_eq!(oks(frames), vec!["{\"a\":1}", "{\"b\":2}"]);
        assert_eq!(framer.buffered_len(), 0);
    }

    #[test]
    fn buffers_partial_line_across_chunks() {
        let mut framer = LineFramer::new();
        assert!(framer.push(b"{\"hel").is_empty());
        assert!(framer.push(b"lo\":").is_empty());
        let frames = framer.push(b"true}\n");
        assert_eq!(oks(frames), vec!["{\"hello\":true}"]);
    }

    #[test]
    fn trims_carriage_return_for_crlf_frames() {
        let mut framer = LineFramer::new();
        let frames = framer.push(b"line-a\r\nline-b\r\n");
        assert_eq!(oks(frames), vec!["line-a", "line-b"]);
    }

    #[test]
    fn flush_emits_unterminated_tail() {
        let mut framer = LineFramer::new();
        assert!(framer.push(b"no-newline-yet").is_empty());
        let tail = framer.flush().expect("tail").expect("ok");
        assert_eq!(tail, "no-newline-yet");
        assert!(framer.flush().is_none(), "flush is idempotent when empty");
    }

    #[test]
    fn empty_lines_are_preserved() {
        let mut framer = LineFramer::new();
        let frames = framer.push(b"\n\nx\n");
        assert_eq!(oks(frames), vec!["", "", "x"]);
    }

    #[test]
    fn multibyte_utf8_split_across_chunks_roundtrips() {
        // CJK "日本語" + emoji, deliberately split mid-codepoint between pushes.
        let payload = "こんにちは🌍日本語".as_bytes().to_vec();
        let split = 4; // mid multibyte sequence
        let mut framer = LineFramer::new();
        assert!(framer.push(&payload[..split]).is_empty());
        let mut tail = payload[split..].to_vec();
        tail.push(b'\n');
        let frames = framer.push(&tail);
        assert_eq!(oks(frames), vec!["こんにちは🌍日本語"]);
    }

    #[test]
    fn invalid_utf8_line_fails_loud() {
        let mut framer = LineFramer::new();
        // 0xFF is never valid UTF-8 (CP437-style corruption).
        let frames = framer.push(b"ok-prefix\xff\xfetail\n");
        assert_eq!(frames.len(), 1);
        match &frames[0] {
            Err(FrameError::InvalidUtf8 {
                byte_count,
                valid_up_to,
            }) => {
                assert_eq!(*valid_up_to, "ok-prefix".len());
                assert_eq!(*byte_count, "ok-prefix".len() + 2 + "tail".len());
            }
            other => panic!("expected InvalidUtf8, got {other:?}"),
        }
    }

    #[test]
    fn one_bad_line_does_not_poison_following_lines() {
        let mut framer = LineFramer::new();
        let frames = framer.push(b"good1\n\xffbad\ngood2\n");
        assert!(frames[0].is_ok());
        assert!(frames[1].is_err());
        assert_eq!(frames[2].as_ref().unwrap(), "good2");
    }

    #[test]
    fn oversized_unterminated_line_is_capped_and_dropped() {
        let mut framer = LineFramer::with_max_line_bytes(8);
        // 20 bytes with no newline → two Oversized flushes (8 + 8) then 4 buffered.
        let frames = framer.push(&[b'a'; 20]);
        assert_eq!(frames.len(), 2);
        assert!(frames
            .iter()
            .all(|frame| matches!(frame, Err(FrameError::Oversized { byte_count: 8 }))));
        assert_eq!(framer.buffered_len(), 4);
    }

    #[test]
    fn encode_line_appends_single_newline() {
        assert_eq!(encode_line("{\"x\":1}"), b"{\"x\":1}\n");
    }

    #[test]
    fn encode_then_frame_roundtrips() {
        let mut framer = LineFramer::new();
        let wire = encode_line("{\"jsonrpc\":\"2.0\",\"id\":1}");
        let frames = framer.push(&wire);
        assert_eq!(oks(frames), vec!["{\"jsonrpc\":\"2.0\",\"id\":1}"]);
    }

    #[test]
    fn scrapes_loopback_url_from_announcement() {
        assert_eq!(
            opencode_server_url("INFO  opencode server listening on http://127.0.0.1:54321"),
            Some("http://127.0.0.1:54321".to_string())
        );
    }

    #[test]
    fn scrapes_url_ignoring_trailing_text() {
        assert_eq!(
            opencode_server_url("opencode server listening on http://127.0.0.1:8080 (ctrl-c to quit)"),
            Some("http://127.0.0.1:8080".to_string())
        );
    }

    #[test]
    fn accepts_localhost_and_ipv6_loopback() {
        assert_eq!(
            opencode_server_url("opencode server listening on http://localhost:1234"),
            Some("http://localhost:1234".to_string())
        );
        assert_eq!(
            opencode_server_url("opencode server listening on http://[::1]:9999"),
            Some("http://[::1]:9999".to_string())
        );
    }

    #[test]
    fn rejects_non_loopback_host() {
        assert_eq!(
            opencode_server_url("opencode server listening on http://8.8.8.8:80"),
            None
        );
        assert_eq!(
            opencode_server_url("opencode server listening on http://example.com:3000"),
            None
        );
    }

    #[test]
    fn rejects_lines_without_marker_or_scheme() {
        assert_eq!(opencode_server_url("some unrelated log line"), None);
        assert_eq!(
            opencode_server_url("opencode server listening on 127.0.0.1:54321"),
            None,
            "no scheme → not accepted"
        );
    }

    #[test]
    fn ignores_userinfo_when_checking_host() {
        assert_eq!(
            opencode_server_url("opencode server listening on http://user:pw@127.0.0.1:7000"),
            Some("http://user:pw@127.0.0.1:7000".to_string())
        );
    }
}
