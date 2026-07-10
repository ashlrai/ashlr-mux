//! Port of the PURE parts of the remote-image *loader* in
//! `Sources/Panels/MarkdownRemoteImageLoader.swift` — the byte-level HTTP/1.1
//! response processing that turns raw socket bytes into an image payload or a
//! redirect decision. The SSRF *gate* itself is already ported in
//! [`crate::remote_image`] and is reused verbatim here.
//!
//! Four pure pieces live here, mirroring the Swift loader:
//!
//! 1. [`decode_chunked_body`] — port of `MarkdownHTTPChunkedBodyDecoder.decode`
//!    (`crlfIndex`) (Swift 601-650): walks hex chunk-size lines, drops chunk
//!    extensions after `;`, parses radix-16, enforces the overflow-safe
//!    cumulative cap and per-chunk CRLF termination, and requires a terminal
//!    `0` chunk.
//! 2. [`parse_headers`] — port of `parseHeaders` (Swift 503-547): parses the
//!    status line, decodes the header block as Latin-1 with lowercased names,
//!    and classifies the response as [`HeaderOutcome::Redirect`],
//!    [`HeaderOutcome::Image`], or [`HeaderOutcome::Fail`].
//! 3. [`RemoteImageAccumulator`] — port of the loader's `process` + `finalOutcome`
//!    (Swift 470-568): a host-fed streaming state machine that splits header
//!    from body at `\r\n\r\n`, caps the header at 64 KiB, enforces a hard body
//!    ceiling, and finalizes non-chunked (exact content-length) or chunked bodies.
//! 4. [`redirect_decision`] — port of the pure parts of
//!    `MarkdownRemoteImageFetcher.fetch`'s redirect branch (Swift 288-323): the
//!    depth-limited, gate-checked, same-consent-host redirect follow decision.
//!
//! The TLS/DNS/socket read loop, `NWConnection`, timeouts, and DNS pinning stay
//! in the host layer — only the pure computation is ported.
//!
//! ## Documented divergences from the canonical Swift
//! - **Latin-1 header decode**: Foundation uses `String(data:encoding:.isoLatin1)`.
//!   Rust has no builtin Latin-1 decoder, so each byte is mapped to its `U+00XX`
//!   code point (identical to Latin-1). The Swift `guard let` can never fail for
//!   Latin-1 (all 256 bytes map), so there is no decode-failure path.
//! - **URL resolution**: `URL(string:relativeTo:)` (Foundation) → [`Url::join`]
//!   (WHATWG). Same as [`crate::remote_image`]; the security predicates are reused
//!   unchanged.
//! - **ASCII chunk-size decode**: `String(bytes:encoding:.ascii)` returns `nil`
//!   for any byte `>= 0x80`; replicated by rejecting such bytes before decoding.

use std::collections::HashMap;
use url::Url;

use crate::remote_image::{
    canonical_image_mime_type, is_potentially_safe_remote_image_url, remote_image_consent_host,
    MAXIMUM_REMOTE_IMAGE_BYTES,
};

/// The successful image payload, mirroring Swift's `MarkdownRemoteImageFetchResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownRemoteImageFetchResult {
    pub data: Vec<u8>,
    pub mime_type: String,
}

/// A terminal outcome of processing an HTTP response (Swift's
/// `MarkdownRemoteImageLoadOutcome`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A fully-received image body with its canonical MIME type.
    Image { data: Vec<u8>, mime: String },
    /// A safe, gate-admitted redirect target to follow.
    Redirect(Url),
}

impl Outcome {
    /// Convenience: the image payload as a [`MarkdownRemoteImageFetchResult`]
    /// (mirrors Swift's `.image(MarkdownRemoteImageFetchResult(...))`); `None`
    /// for a redirect outcome.
    pub fn into_fetch_result(self) -> Option<MarkdownRemoteImageFetchResult> {
        match self {
            Outcome::Image { data, mime } => Some(MarkdownRemoteImageFetchResult {
                data,
                mime_type: mime,
            }),
            Outcome::Redirect(_) => None,
        }
    }
}

/// The result of feeding a chunk of bytes to [`RemoteImageAccumulator::process`]
/// (Swift's private `ProcessResult`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessResult {
    /// More bytes are needed; keep reading.
    Continue,
    /// The response is complete with the given outcome.
    Finish(Outcome),
    /// The response is malformed or violates a limit; abort.
    Fail,
}

/// The classification of a parsed HTTP header block (the three cases Swift's
/// `parseHeaders` folds into its `ProcessResult`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderOutcome {
    /// A 3xx with a `Location` that resolves to a gate-admitted URL.
    Redirect(Url),
    /// A 2xx with a canonicalizable image MIME type.
    Image {
        mime: String,
        chunked: bool,
        content_length: Option<usize>,
    },
    /// Anything else (bad status line, non-image, over-long content-length, …).
    Fail,
}

// --- 1. chunked body decoder -------------------------------------------------

/// Port of `MarkdownHTTPChunkedBodyDecoder.decode` (Swift 601-637): decode a
/// chunked-transfer body, or `None` if it is malformed or exceeds `maximum_bytes`.
///
/// Walks hex chunk-size lines (dropping any `;`-delimited chunk extension),
/// enforces the overflow-safe cumulative cap `decoded.len() <= max - size`,
/// requires each chunk to be CRLF-terminated, and requires a terminal `0` chunk
/// (a body that ends without one yields `None`).
pub fn decode_chunked_body(bytes: &[u8], maximum_bytes: usize) -> Option<Vec<u8>> {
    let max = maximum_bytes as i64;
    let mut offset: usize = 0;
    let mut decoded: Vec<u8> = Vec::new();

    while offset < bytes.len() {
        let line_end = crlf_index(bytes, offset)?;
        let size_line_bytes = &bytes[offset..line_end];
        // Swift `String(bytes:encoding:.ascii)` returns nil for any byte >= 0x80.
        if size_line_bytes.iter().any(|&b| b > 0x7f) {
            return None;
        }
        // All bytes are <= 0x7f, so this is valid ASCII (and valid UTF-8).
        let size_line = std::str::from_utf8(size_line_bytes).ok()?;
        let size_token = first_chunk_size_token(size_line);
        // Swift `Int(_:radix:16)` over the whitespace-trimmed token. `.whitespaces`
        // is the Unicode space set + tab (no newlines); the size line is ASCII, so
        // trimming space/tab is equivalent.
        let size =
            i64::from_str_radix(size_token.trim_matches(|c: char| c == ' ' || c == '\t'), 16)
                .ok()?;
        offset = line_end + 2;
        if size == 0 {
            return Some(decoded);
        }
        let remaining_bytes = bytes.len() as i64 - offset as i64;
        if !(size >= 0
            && size <= max
            && (decoded.len() as i64) <= max - size
            && remaining_bytes >= 2
            && size <= remaining_bytes - 2)
        {
            return None;
        }
        let size_us = size as usize;
        let chunk_end = offset + size_us;
        if bytes[chunk_end] != 13 || bytes[chunk_end + 1] != 10 {
            return None;
        }
        decoded.extend_from_slice(&bytes[offset..offset + size_us]);
        if decoded.len() as i64 > max {
            return None;
        }
        offset += size_us + 2;
    }
    None
}

/// Port of `crlfIndex` (Swift 639-649): the index of the first `\r\n` at or after
/// `offset`, or `None`.
fn crlf_index(bytes: &[u8], offset: usize) -> Option<usize> {
    if offset >= bytes.len() {
        return None;
    }
    let mut index = offset;
    while index + 1 < bytes.len() {
        if bytes[index] == 13 && bytes[index + 1] == 10 {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// The chunk-size token: everything before the first `;` chunk-extension marker.
///
/// Faithful to Swift's `sizeLine.split(separator: ";", maxSplits: 1).first ?? ""`
/// (`omittingEmptySubsequences` defaults to `true`): empty subsequences are
/// omitted and do not count toward `maxSplits`, so a line starting with one or
/// more `;` yields the first non-empty `;`-delimited token (`";4;ext"` → `"4"`,
/// `";;abc"` → `"abc"`), not the raw remainder.
fn first_chunk_size_token(size_line: &str) -> &str {
    swift_split(size_line, ';', 1, true)
        .first()
        .copied()
        .unwrap_or("")
}

// --- 2. header parser / status classifier ------------------------------------

/// Port of `parseHeaders` (Swift 503-547): classify an HTTP header block.
///
/// - Parses the status line (`split(' ', maxSplits: 2)`, `>= 2` parts, integer
///   status code).
/// - Decodes the header block as Latin-1, keying lowercased header names.
/// - A 3xx whose `Location` resolves (relative to `request_url`) to a
///   gate-admitted URL ⇒ [`HeaderOutcome::Redirect`].
/// - A 2xx with a canonicalizable image `Content-Type` ⇒ [`HeaderOutcome::Image`]
///   (with `Transfer-Encoding: chunked` detection and a validated
///   `Content-Length`).
/// - Everything else ⇒ [`HeaderOutcome::Fail`].
pub fn parse_headers(header_data: &[u8], request_url: &Url, maximum_bytes: usize) -> HeaderOutcome {
    // DIVERGENCE: Foundation decodes with `.isoLatin1`; Rust has no builtin, so
    // map each byte to its U+00XX code point (identical to Latin-1). The Swift
    // guard-let can never fail for Latin-1, so there is no decode-failure path.
    let raw_headers = decode_latin1(header_data);
    let lines: Vec<&str> = raw_headers.split("\r\n").collect();
    // `split` never yields an empty Vec, so `lines.first()` is always `Some`.
    let status_line = lines.first().copied().unwrap_or("");
    let status_parts = swift_split(status_line, ' ', 2, true);
    if status_parts.len() < 2 {
        return HeaderOutcome::Fail;
    }
    let status_code: i64 = match status_parts[1].parse() {
        Ok(code) => code,
        Err(_) => return HeaderOutcome::Fail,
    };

    let mut headers: HashMap<String, String> = HashMap::new();
    for &line in lines.iter().skip(1) {
        if let Some(colon) = line.find(':') {
            // DIVERGENCE: Swift trims `.whitespacesAndNewlines`; Rust
            // `char::is_whitespace` is the Unicode White_Space set, which covers
            // the same ASCII space/tab/CR/LF that appear in HTTP headers.
            let name = line[..colon]
                .trim_matches(|c: char| c.is_whitespace())
                .to_ascii_lowercase();
            let value = line[colon + 1..]
                .trim_matches(|c: char| c.is_whitespace())
                .to_string();
            headers.insert(name, value);
        }
    }

    if (300..400).contains(&status_code) {
        if let Some(location) = headers.get("location") {
            // DIVERGENCE: URL(string:relativeTo:) (Foundation) → Url::join (WHATWG).
            if let Ok(redirect_url) = request_url.join(location) {
                if is_potentially_safe_remote_image_url(&redirect_url) {
                    return HeaderOutcome::Redirect(redirect_url);
                }
            }
        }
    }

    if !(200..300).contains(&status_code) {
        return HeaderOutcome::Fail;
    }
    let mime = match canonical_image_mime_type(headers.get("content-type").map(String::as_str)) {
        Some(mime) => mime,
        None => return HeaderOutcome::Fail,
    };

    let mut chunked = false;
    if let Some(transfer_encoding) = headers.get("transfer-encoding") {
        let lowered = transfer_encoding.to_ascii_lowercase();
        // Swift trims `.whitespaces` (Unicode Zs + tab); the value is Latin-1
        // decoded, and the only Zs characters in U+0000..=U+00FF are space and
        // U+00A0, so this trim set is exactly faithful.
        if swift_split(&lowered, ',', usize::MAX, true)
            .iter()
            .any(|token| {
                token.trim_matches(|c: char| c == ' ' || c == '\t' || c == '\u{A0}') == "chunked"
            })
        {
            chunked = true;
        }
    }

    let mut content_length: Option<usize> = None;
    if let Some(raw) = headers.get("content-length") {
        // Swift `headers["content-length"].flatMap(Int.init)`: a present-but-
        // unparseable value is ignored (no content-length, not a failure).
        if let Ok(length) = raw.parse::<i64>() {
            if length < 0 || length > maximum_bytes as i64 {
                return HeaderOutcome::Fail;
            }
            content_length = Some(length as usize);
        }
    }

    HeaderOutcome::Image {
        mime,
        chunked,
        content_length,
    }
}

/// Decode bytes as Latin-1 (ISO-8859-1): each byte is one code point in
/// `U+0000..=U+00FF`. See the module-level divergence note.
fn decode_latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

/// Faithful port of Swift's
/// `Collection.split(separator:maxSplits:omittingEmptySubsequences:)` for a
/// `char` separator over a `&str` (stdlib algorithm): empty subsequences are
/// omitted when `omit_empty`, they do not count toward `max_splits`, and once
/// `max_splits` splits have been emitted the remainder (including any further
/// separators) is the final subsequence.
fn swift_split(s: &str, sep: char, max_splits: usize, omit_empty: bool) -> Vec<&str> {
    let mut result: Vec<&str> = Vec::new();
    let mut sub_start: usize = 0;

    if max_splits == 0 || s.is_empty() {
        if !(sub_start == s.len() && omit_empty) {
            result.push(&s[sub_start..]);
        }
        return result;
    }

    for (i, c) in s.char_indices() {
        if c == sep {
            let did_append = if sub_start == i && omit_empty {
                false
            } else {
                result.push(&s[sub_start..i]);
                true
            };
            sub_start = i + c.len_utf8();
            if did_append && result.len() == max_splits {
                break;
            }
        }
    }

    if sub_start != s.len() || !omit_empty {
        result.push(&s[sub_start..]);
    }
    result
}

// --- 3. streaming accumulator state machine ----------------------------------

/// Port of the loader's `process` + `finalOutcome` (Swift 470-568): a host-fed
/// state machine that accumulates HTTP response bytes, splits the header from
/// the body at `\r\n\r\n`, and yields a terminal [`Outcome`].
///
/// The host layer owns the socket read loop: it calls [`Self::process`] with
/// each received byte chunk, acting on [`ProcessResult::Finish`] /
/// [`ProcessResult::Fail`], and calls [`Self::final_outcome`] when the peer
/// closes the connection (Swift's `isComplete` branch).
#[derive(Debug, Clone)]
pub struct RemoteImageAccumulator {
    maximum_bytes: usize,
    request_url: Url,
    raw_body: Vec<u8>,
    mime_type: String,
    header_parsed: bool,
    uses_chunked_transfer: bool,
    expected_body_bytes: Option<usize>,
}

impl RemoteImageAccumulator {
    /// A new accumulator for a response to `request_url`, using the canonical
    /// 8 MiB cap ([`MAXIMUM_REMOTE_IMAGE_BYTES`]) — the cap the Swift loader
    /// always uses.
    pub fn new(request_url: Url) -> Self {
        Self::with_maximum_bytes(request_url, MAXIMUM_REMOTE_IMAGE_BYTES)
    }

    /// A new accumulator with an explicit byte cap (for tests / non-default hosts).
    pub fn with_maximum_bytes(request_url: Url, maximum_bytes: usize) -> Self {
        Self {
            maximum_bytes,
            request_url,
            raw_body: Vec::new(),
            // Swift defaults `mimeType` to "image/png" before the header is parsed.
            mime_type: "image/png".to_string(),
            header_parsed: false,
            uses_chunked_transfer: false,
            expected_body_bytes: None,
        }
    }

    /// Feed the next chunk of received bytes (Swift `process(_:)`).
    pub fn process(&mut self, data: &[u8]) -> ProcessResult {
        self.raw_body.extend_from_slice(data);

        if !self.header_parsed {
            let Some(pos) = find_subsequence(&self.raw_body, b"\r\n\r\n") else {
                // 64 KiB header cap before the delimiter is seen.
                return if self.raw_body.len() > 64 * 1024 {
                    ProcessResult::Fail
                } else {
                    ProcessResult::Continue
                };
            };
            let header_data = self.raw_body[..pos].to_vec();
            let remaining = self.raw_body[pos + 4..].to_vec();
            self.raw_body = remaining;
            match parse_headers(&header_data, &self.request_url, self.maximum_bytes) {
                HeaderOutcome::Image {
                    mime,
                    chunked,
                    content_length,
                } => {
                    self.header_parsed = true;
                    self.mime_type = mime;
                    self.uses_chunked_transfer = chunked;
                    self.expected_body_bytes = content_length;
                }
                HeaderOutcome::Redirect(url) => {
                    return ProcessResult::Finish(Outcome::Redirect(url));
                }
                HeaderOutcome::Fail => return ProcessResult::Fail,
            }
        }

        // Hard body ceiling of maximum_bytes + 64 KiB.
        if self.raw_body.len() > self.maximum_bytes + 64 * 1024 {
            return ProcessResult::Fail;
        }
        if !self.uses_chunked_transfer && self.raw_body.len() > self.maximum_bytes {
            return ProcessResult::Fail;
        }
        if !self.uses_chunked_transfer {
            if let Some(expected) = self.expected_body_bytes {
                if self.raw_body.len() >= expected {
                    self.raw_body.truncate(expected);
                    return match self.final_outcome() {
                        Some(outcome) => ProcessResult::Finish(outcome),
                        None => ProcessResult::Fail,
                    };
                }
            }
        }
        ProcessResult::Continue
    }

    /// Finalize the accumulated body (Swift `finalOutcome()`), called by the host
    /// when the peer closes the connection. `None` if no complete image is
    /// available (header never parsed, content-length mismatch, malformed chunked
    /// body, or an over-cap body).
    pub fn final_outcome(&self) -> Option<Outcome> {
        if !self.header_parsed {
            return None;
        }
        let body: Vec<u8> = if self.uses_chunked_transfer {
            decode_chunked_body(&self.raw_body, self.maximum_bytes)?
        } else {
            if let Some(expected) = self.expected_body_bytes {
                if self.raw_body.len() != expected {
                    return None;
                }
            }
            self.raw_body.clone()
        };
        if body.len() > self.maximum_bytes {
            return None;
        }
        Some(Outcome::Image {
            data: body,
            mime: self.mime_type.clone(),
        })
    }
}

/// The index of the first occurrence of `needle` in `haystack` (Swift
/// `Data.range(of:)`), or `None`.
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

// --- 4. redirect-follow decision predicate -----------------------------------

/// Port of the pure parts of `MarkdownRemoteImageFetcher.fetch`'s redirect branch
/// (Swift 288-323): decide whether to follow a redirect, and to what URL.
///
/// Returns the resolved next URL only when:
/// - `redirect_depth <= 3` (Swift's `guard redirectDepth <= 3` at the entry of
///   the recursive fetch; `redirect_depth` is the depth the followed request
///   would run at), and
/// - the `Location` resolves (relative to `request_url`) to a URL whose consent
///   host equals `approved_host` — which, via [`remote_image_consent_host`],
///   also requires the SSRF gate to admit it.
///
/// Cross-host redirects (consent host `!= approved_host`) yield `None`.
pub fn redirect_decision(
    redirect: &Url,
    request_url: &Url,
    approved_host: &str,
    redirect_depth: u32,
) -> Option<Url> {
    if redirect_depth > 3 {
        return None;
    }
    // Swift: URL(string: redirectURL.absoluteString, relativeTo: url)?.absoluteURL.
    // DIVERGENCE: Foundation URL resolution → WHATWG Url::join.
    let resolved = request_url.join(redirect.as_str()).ok()?;
    // `remote_image_consent_host` returns `None` unless the resolved URL passes
    // `is_potentially_safe_remote_image_url`, so the gate is enforced here
    // transitively — matching Swift, which only compares the consent host.
    if remote_image_consent_host(&resolved).as_deref() == Some(approved_host) {
        Some(resolved)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(u: &str) -> Url {
        Url::parse(u).unwrap()
    }

    // --- 1. chunked decode ---------------------------------------------------

    #[test]
    fn chunked_decode_happy() {
        let body = b"4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        assert_eq!(
            decode_chunked_body(body, MAXIMUM_REMOTE_IMAGE_BYTES).as_deref(),
            Some(&b"Wikipedia"[..])
        );
    }

    #[test]
    fn chunked_decode_single_chunk() {
        let body = b"3\r\nabc\r\n0\r\n\r\n";
        assert_eq!(
            decode_chunked_body(body, 1024).as_deref(),
            Some(&b"abc"[..])
        );
    }

    #[test]
    fn chunked_decode_drops_extensions() {
        let body = b"4;foo=bar\r\nWiki\r\n0;end\r\n\r\n";
        assert_eq!(
            decode_chunked_body(body, 1024).as_deref(),
            Some(&b"Wiki"[..])
        );
    }

    #[test]
    fn chunked_decode_uppercase_hex() {
        // 0x1A = 26 bytes.
        let mut body = Vec::new();
        body.extend_from_slice(b"1A\r\n");
        body.extend_from_slice(&[b'x'; 26]);
        body.extend_from_slice(b"\r\n0\r\n\r\n");
        assert_eq!(decode_chunked_body(&body, 1024).unwrap(), vec![b'x'; 26]);
    }

    #[test]
    fn chunked_decode_oversize_chunk_rejected() {
        // A single 4-byte chunk against a 3-byte cap.
        let body = b"4\r\nWiki\r\n0\r\n\r\n";
        assert_eq!(decode_chunked_body(body, 3), None);
    }

    #[test]
    fn chunked_decode_cumulative_cap_rejected() {
        // Two 3-byte chunks (6 bytes) against a 5-byte cap: the second overflows.
        let body = b"3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n";
        assert_eq!(decode_chunked_body(body, 5), None);
        assert_eq!(
            decode_chunked_body(body, 6).as_deref(),
            Some(&b"abcdef"[..])
        );
    }

    #[test]
    fn chunked_decode_missing_terminal_zero_chunk() {
        // Well-formed single chunk but no terminal 0-chunk.
        let body = b"4\r\nWiki\r\n";
        assert_eq!(decode_chunked_body(body, 1024), None);
    }

    #[test]
    fn chunked_decode_missing_crlf_after_chunk() {
        // "Wiki" not followed by CRLF.
        let body = b"4\r\nWikiXX0\r\n\r\n";
        assert_eq!(decode_chunked_body(body, 1024), None);
    }

    #[test]
    fn chunked_decode_bad_hex_rejected() {
        let body = b"zz\r\nWiki\r\n0\r\n\r\n";
        assert_eq!(decode_chunked_body(body, 1024), None);
    }

    #[test]
    fn chunked_decode_non_ascii_size_line_rejected() {
        let body = b"\xff\r\nWiki\r\n0\r\n\r\n";
        assert_eq!(decode_chunked_body(body, 1024), None);
    }

    #[test]
    fn chunked_decode_negative_size_rejected() {
        // Int(radix:16) parses "-1"; the size >= 0 guard rejects it.
        let body = b"-1\r\nWiki\r\n0\r\n\r\n";
        assert_eq!(decode_chunked_body(body, 1024), None);
    }

    #[test]
    fn chunked_decode_empty_input() {
        // No size line at all → no terminal 0-chunk → None.
        assert_eq!(decode_chunked_body(b"", 1024), None);
    }

    #[test]
    fn chunked_decode_immediate_terminal() {
        assert_eq!(
            decode_chunked_body(b"0\r\n\r\n", 1024).as_deref(),
            Some(&b""[..])
        );
    }

    /// Verbatim port of Swift
    /// `testMarkdownRemoteImageChunkedDecoderRejectsOversizedChunks`
    /// (`cmuxTests/MarkdownPanelTests.swift` 1354-1374): same byte inputs, same
    /// `maximumBytes`, same expected outcomes.
    #[test]
    fn chunked_decode_swift_oracle_rejects_oversized_chunks() {
        // decode("3\r\nabc\r\n0\r\n\r\n", maximumBytes: 8) == Data("abc")
        assert_eq!(
            decode_chunked_body(b"3\r\nabc\r\n0\r\n\r\n", 8).as_deref(),
            Some(&b"abc"[..])
        );
        // decode("9\r\nabcdefghi\r\n0\r\n\r\n", maximumBytes: 8) == nil
        assert_eq!(decode_chunked_body(b"9\r\nabcdefghi\r\n0\r\n\r\n", 8), None);
        // decode("7fffffffffffffff\r\n", maximumBytes: 8) == nil.
        // Swift `Int(_, radix: 16)` on 64-bit parses Int64.max successfully and
        // the `size <= maximumBytes` guard rejects it; Rust
        // `i64::from_str_radix` parses it identically.
        assert_eq!(decode_chunked_body(b"7fffffffffffffff\r\n", 8), None);
    }

    #[test]
    fn chunked_decode_i64_boundary_size_lines() {
        // i64::MAX parses in both Swift and Rust; rejected by `size <= max`.
        assert_eq!(
            decode_chunked_body(b"7fffffffffffffff\r\nx\r\n0\r\n\r\n", 8),
            None
        );
        // i64::MAX + 1 overflows: Swift `Int(_, radix:16)` returns nil, Rust
        // `from_str_radix` returns Err — both yield no decode.
        assert_eq!(
            decode_chunked_body(b"8000000000000000\r\nx\r\n0\r\n\r\n", 8),
            None
        );
        // Far past 64 bits also fails the parse (never panics/traps).
        assert_eq!(
            decode_chunked_body(b"ffffffffffffffffff\r\nx\r\n0\r\n\r\n", 8),
            None
        );
        // i64::MIN parses in both (Swift Int.min, Rust i64::MIN); the `size >= 0`
        // guard rejects it BEFORE `max - size` is evaluated — the short-circuit
        // order mirrors Swift's guard-comma order, so `8 - i64::MIN` (which would
        // overflow) is never computed.
        assert_eq!(
            decode_chunked_body(b"-8000000000000000\r\nx\r\n0\r\n\r\n", 8),
            None
        );
    }

    #[test]
    fn chunked_decode_cap_boundary_arithmetic() {
        // A single chunk of exactly `max` bytes is allowed
        // (`decoded.count (0) <= max - size (0)`).
        assert_eq!(
            decode_chunked_body(b"8\r\nabcdefgh\r\n0\r\n\r\n", 8).as_deref(),
            Some(&b"abcdefgh"[..])
        );
        // Two chunks summing to exactly `max` are allowed (4 <= 8 - 4).
        assert_eq!(
            decode_chunked_body(b"4\r\nabcd\r\n4\r\nefgh\r\n0\r\n\r\n", 8).as_deref(),
            Some(&b"abcdefgh"[..])
        );
        // One byte past the cap after a full-cap chunk is rejected (8 > 8 - 1).
        assert_eq!(
            decode_chunked_body(b"8\r\nabcdefgh\r\n1\r\ni\r\n0\r\n\r\n", 8),
            None
        );
        // maximum_bytes == 0: any non-empty chunk is rejected (1 > 0), but the
        // bare terminal chunk still decodes to an empty body.
        assert_eq!(decode_chunked_body(b"1\r\na\r\n0\r\n\r\n", 0), None);
        assert_eq!(
            decode_chunked_body(b"0\r\n\r\n", 0).as_deref(),
            Some(&b""[..])
        );
    }

    #[test]
    fn crlf_index_basic() {
        assert_eq!(crlf_index(b"ab\r\ncd", 0), Some(2));
        assert_eq!(crlf_index(b"ab\r\ncd", 3), None);
        assert_eq!(crlf_index(b"abcd", 0), None);
        assert_eq!(crlf_index(b"", 0), None);
    }

    #[test]
    fn first_chunk_size_token_cases() {
        assert_eq!(first_chunk_size_token("4"), "4");
        assert_eq!(first_chunk_size_token("4;ext"), "4");
        assert_eq!(first_chunk_size_token(""), "");
        // Leading empty subsequences are omitted and do not count toward
        // maxSplits (matches Swift `split(separator:maxSplits:)`).
        assert_eq!(first_chunk_size_token(";0"), "0");
        assert_eq!(first_chunk_size_token(";4;ext"), "4");
        assert_eq!(first_chunk_size_token(";;abc"), "abc");
    }

    #[test]
    fn chunked_decode_leading_semicolon_size_line_matches_swift_split() {
        // Swift `split(";", maxSplits: 1, omittingEmptySubsequences: true)` on
        // ";4;ext" yields ["4", "ext"], so the size token is "4" and the chunk
        // decodes — a naive "everything before the first ';'" would fail here.
        assert_eq!(
            decode_chunked_body(b";4;ext\r\nWiki\r\n0\r\n\r\n", 1024).as_deref(),
            Some(&b"Wiki"[..])
        );
    }

    // --- 2. header parse -----------------------------------------------------

    fn req() -> Url {
        url("https://example.com/a.png")
    }

    #[test]
    fn header_parse_200_png() {
        let headers = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n";
        assert_eq!(
            parse_headers(headers, &req(), 1024),
            HeaderOutcome::Image {
                mime: "image/png".to_string(),
                chunked: false,
                content_length: None,
            }
        );
    }

    #[test]
    fn header_parse_200_with_content_length() {
        let headers = b"HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: 42\r\n";
        assert_eq!(
            parse_headers(headers, &req(), 1024),
            HeaderOutcome::Image {
                mime: "image/jpeg".to_string(),
                chunked: false,
                content_length: Some(42),
            }
        );
    }

    #[test]
    fn header_parse_200_chunked() {
        let headers =
            b"HTTP/1.1 200 OK\r\nContent-Type: image/gif\r\nTransfer-Encoding: gzip, chunked\r\n";
        assert_eq!(
            parse_headers(headers, &req(), 1024),
            HeaderOutcome::Image {
                mime: "image/gif".to_string(),
                chunked: true,
                content_length: None,
            }
        );
    }

    #[test]
    fn header_parse_chunked_token_trims_latin1_nbsp() {
        // Swift trims `.whitespaces` (Zs + tab) around each transfer-encoding
        // token; a Latin-1 0xA0 (U+00A0 NO-BREAK SPACE, in Zs) before "chunked"
        // must therefore still be detected.
        let headers =
            b"HTTP/1.1 200 OK\r\nContent-Type: image/gif\r\nTransfer-Encoding: gzip,\xa0chunked\r\n";
        assert_eq!(
            parse_headers(headers, &req(), 1024),
            HeaderOutcome::Image {
                mime: "image/gif".to_string(),
                chunked: true,
                content_length: None,
            }
        );
    }

    #[test]
    fn header_parse_content_length_over_cap_fails() {
        let headers = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 2000\r\n";
        assert_eq!(parse_headers(headers, &req(), 1024), HeaderOutcome::Fail);
    }

    #[test]
    fn header_parse_content_length_negative_fails() {
        let headers = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: -1\r\n";
        assert_eq!(parse_headers(headers, &req(), 1024), HeaderOutcome::Fail);
    }

    #[test]
    fn header_parse_content_length_unparseable_ignored() {
        // Present but unparseable content-length is ignored, not a failure.
        let headers = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: xyz\r\n";
        assert_eq!(
            parse_headers(headers, &req(), 1024),
            HeaderOutcome::Image {
                mime: "image/png".to_string(),
                chunked: false,
                content_length: None,
            }
        );
    }

    #[test]
    fn header_parse_200_non_image_fails() {
        let headers = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n";
        assert_eq!(parse_headers(headers, &req(), 1024), HeaderOutcome::Fail);
    }

    #[test]
    fn header_parse_200_missing_content_type_fails() {
        let headers = b"HTTP/1.1 200 OK\r\n";
        assert_eq!(parse_headers(headers, &req(), 1024), HeaderOutcome::Fail);
    }

    #[test]
    fn header_parse_404_fails() {
        let headers = b"HTTP/1.1 404 Not Found\r\nContent-Type: image/png\r\n";
        assert_eq!(parse_headers(headers, &req(), 1024), HeaderOutcome::Fail);
    }

    #[test]
    fn header_parse_malformed_status_fails() {
        assert_eq!(
            parse_headers(b"GARBAGE\r\n", &req(), 1024),
            HeaderOutcome::Fail
        );
        assert_eq!(
            parse_headers(b"HTTP/1.1 notanumber OK\r\n", &req(), 1024),
            HeaderOutcome::Fail
        );
        assert_eq!(parse_headers(b"", &req(), 1024), HeaderOutcome::Fail);
    }

    #[test]
    fn header_parse_301_absolute_location_redirects() {
        let headers = b"HTTP/1.1 301 Moved\r\nLocation: https://example.com/b.png\r\n";
        assert_eq!(
            parse_headers(headers, &req(), 1024),
            HeaderOutcome::Redirect(url("https://example.com/b.png"))
        );
    }

    #[test]
    fn header_parse_302_relative_location_resolves() {
        let headers = b"HTTP/1.1 302 Found\r\nLocation: /b.png\r\n";
        assert_eq!(
            parse_headers(headers, &req(), 1024),
            HeaderOutcome::Redirect(url("https://example.com/b.png"))
        );
    }

    #[test]
    fn header_parse_301_missing_location_fails() {
        let headers = b"HTTP/1.1 301 Moved\r\n";
        assert_eq!(parse_headers(headers, &req(), 1024), HeaderOutcome::Fail);
    }

    #[test]
    fn header_parse_301_unsafe_location_fails() {
        // A redirect to a non-HTTPS URL is rejected by the gate → falls through to
        // the 2xx guard → Fail.
        let headers = b"HTTP/1.1 301 Moved\r\nLocation: http://example.com/b.png\r\n";
        assert_eq!(parse_headers(headers, &req(), 1024), HeaderOutcome::Fail);
    }

    #[test]
    fn header_parse_301_private_ip_location_fails() {
        let headers = b"HTTP/1.1 301 Moved\r\nLocation: https://127.0.0.1/b.png\r\n";
        assert_eq!(parse_headers(headers, &req(), 1024), HeaderOutcome::Fail);
    }

    #[test]
    fn header_parse_header_name_case_insensitive() {
        let headers = b"HTTP/1.1 200 OK\r\nCONTENT-TYPE: IMAGE/PNG\r\n";
        assert_eq!(
            parse_headers(headers, &req(), 1024),
            HeaderOutcome::Image {
                mime: "image/png".to_string(),
                chunked: false,
                content_length: None,
            }
        );
    }

    #[test]
    fn swift_split_matches_status_line() {
        assert_eq!(
            swift_split("HTTP/1.1 200 OK", ' ', 2, true),
            vec!["HTTP/1.1", "200", "OK"]
        );
        // maxSplits keeps the remainder intact.
        assert_eq!(
            swift_split("HTTP/1.1 200 OK EXTRA", ' ', 2, true),
            vec!["HTTP/1.1", "200", "OK EXTRA"]
        );
        // Omitted empties do not count toward maxSplits: the leading double space
        // before "200" is collapsed, but once maxSplits is reached after "200" the
        // remainder (including its leading space) is returned verbatim — exactly
        // Swift's stdlib behavior. Only statusParts[1] ("200") is consumed.
        assert_eq!(
            swift_split("HTTP/1.1  200  OK", ' ', 2, true),
            vec!["HTTP/1.1", "200", " OK"]
        );
    }

    // --- 3. accumulator ------------------------------------------------------

    #[test]
    fn accumulator_non_chunked_exact_content_length() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        let response =
            b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\nabcd";
        assert_eq!(
            acc.process(response),
            ProcessResult::Finish(Outcome::Image {
                data: b"abcd".to_vec(),
                mime: "image/png".to_string(),
            })
        );
    }

    #[test]
    fn accumulator_non_chunked_truncates_extra_bytes() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        // Body overshoots content-length; the extra bytes are truncated away.
        let response =
            b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\nabcdEXTRA";
        assert_eq!(
            acc.process(response),
            ProcessResult::Finish(Outcome::Image {
                data: b"abcd".to_vec(),
                mime: "image/png".to_string(),
            })
        );
    }

    #[test]
    fn accumulator_split_across_process_calls() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        assert_eq!(
            acc.process(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n"),
            ProcessResult::Continue
        );
        assert_eq!(
            acc.process(b"Content-Length: 4\r\n\r\nab"),
            ProcessResult::Continue
        );
        assert_eq!(
            acc.process(b"cd"),
            ProcessResult::Finish(Outcome::Image {
                data: b"abcd".to_vec(),
                mime: "image/png".to_string(),
            })
        );
    }

    #[test]
    fn accumulator_content_length_mismatch_final_outcome_none() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        // Content-Length says 5 but the stream ends after 4 bytes.
        assert_eq!(
            acc.process(
                b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 5\r\n\r\nabcd"
            ),
            ProcessResult::Continue
        );
        assert_eq!(acc.final_outcome(), None);
    }

    #[test]
    fn accumulator_no_content_length_finalizes_at_close() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        assert_eq!(
            acc.process(b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\nhello"),
            ProcessResult::Continue
        );
        assert_eq!(
            acc.final_outcome(),
            Some(Outcome::Image {
                data: b"hello".to_vec(),
                mime: "image/png".to_string(),
            })
        );
    }

    #[test]
    fn accumulator_chunked_finalizes_at_close() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        assert_eq!(
            acc.process(
                b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n"
            ),
            ProcessResult::Continue
        );
        assert_eq!(
            acc.final_outcome(),
            Some(Outcome::Image {
                data: b"Wikipedia".to_vec(),
                mime: "image/png".to_string(),
            })
        );
    }

    #[test]
    fn accumulator_redirect_finishes() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        assert_eq!(
            acc.process(b"HTTP/1.1 302 Found\r\nLocation: https://example.com/b.png\r\n\r\n"),
            ProcessResult::Finish(Outcome::Redirect(url("https://example.com/b.png")))
        );
    }

    #[test]
    fn accumulator_bad_status_fails() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        assert_eq!(
            acc.process(b"HTTP/1.1 500 Error\r\nContent-Type: image/png\r\n\r\n"),
            ProcessResult::Fail
        );
    }

    #[test]
    fn accumulator_header_overflow_fails() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        // 64 KiB + 1 bytes with no header delimiter → Fail.
        let blob = vec![b'a'; 64 * 1024 + 1];
        assert_eq!(acc.process(&blob), ProcessResult::Fail);
    }

    #[test]
    fn accumulator_header_under_cap_continues() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        let blob = vec![b'a'; 64 * 1024];
        assert_eq!(acc.process(&blob), ProcessResult::Continue);
    }

    #[test]
    fn accumulator_non_chunked_over_cap_fails() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 10);
        // No content-length; non-chunked body exceeds the cap.
        let mut response = b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\n\r\n".to_vec();
        response.extend_from_slice(&[b'x'; 11]);
        assert_eq!(acc.process(&response), ProcessResult::Fail);
    }

    #[test]
    fn accumulator_chunked_body_ceiling_fails() {
        let mut acc = RemoteImageAccumulator::with_maximum_bytes(req(), 10);
        let mut response =
            b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\n\r\n"
                .to_vec();
        // Exceed maximum_bytes + 64 KiB even though chunked defers content-length.
        response.extend_from_slice(&vec![b'x'; 10 + 64 * 1024 + 1]);
        assert_eq!(acc.process(&response), ProcessResult::Fail);
    }

    #[test]
    fn accumulator_final_outcome_before_header_is_none() {
        let acc = RemoteImageAccumulator::with_maximum_bytes(req(), 1024);
        assert_eq!(acc.final_outcome(), None);
    }

    // --- 4. redirect decision ------------------------------------------------

    #[test]
    fn redirect_decision_same_host_ok() {
        let decision = redirect_decision(
            &url("https://example.com/b.png"),
            &url("https://example.com/a.png"),
            "example.com",
            0,
        );
        assert_eq!(decision, Some(url("https://example.com/b.png")));
    }

    #[test]
    fn redirect_decision_depth_boundary() {
        let redirect = url("https://example.com/b.png");
        let request = url("https://example.com/a.png");
        // depth 3 is allowed; depth 4 is not.
        assert_eq!(
            redirect_decision(&redirect, &request, "example.com", 3),
            Some(url("https://example.com/b.png"))
        );
        assert_eq!(
            redirect_decision(&redirect, &request, "example.com", 4),
            None
        );
    }

    #[test]
    fn redirect_decision_cross_host_rejected() {
        let decision = redirect_decision(
            &url("https://evil.com/b.png"),
            &url("https://example.com/a.png"),
            "example.com",
            0,
        );
        assert_eq!(decision, None);
    }

    #[test]
    fn redirect_decision_gate_reject() {
        // A non-HTTPS redirect target fails the gate (consent host is None).
        let decision = redirect_decision(
            &url("http://example.com/b.png"),
            &url("https://example.com/a.png"),
            "example.com",
            0,
        );
        assert_eq!(decision, None);
    }

    #[test]
    fn redirect_decision_private_ip_rejected() {
        let decision = redirect_decision(
            &url("https://127.0.0.1/b.png"),
            &url("https://example.com/a.png"),
            "example.com",
            0,
        );
        assert_eq!(decision, None);
    }

    // --- fetch result conversion ---------------------------------------------

    #[test]
    fn outcome_into_fetch_result() {
        let outcome = Outcome::Image {
            data: b"abc".to_vec(),
            mime: "image/png".to_string(),
        };
        assert_eq!(
            outcome.into_fetch_result(),
            Some(MarkdownRemoteImageFetchResult {
                data: b"abc".to_vec(),
                mime_type: "image/png".to_string(),
            })
        );
        assert_eq!(
            Outcome::Redirect(url("https://example.com/b.png")).into_fetch_result(),
            None
        );
    }
}
