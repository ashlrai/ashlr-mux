//! Incremental parser for a tmux control-mode (`tmux -CC`) byte stream.
//!
//! Ported from `RemoteTmuxControlStreamParser.swift`.
//!
//! Feed raw bytes as they arrive from the SSH process; the parser buffers
//! partial lines, strips the `ESC P 1000 p` / `ESC \` DCS framing and the
//! `\r` that the SSH `-tt` pty adds, coalesces `%begin`…`%end` command blocks,
//! and emits structured [`RemoteTmuxControlMessage`] values.
//!
//! The protocol is line-oriented: notifications and command-block content are
//! ASCII (tmux octal-escapes control bytes), so they are decoded to `String`.
//! The exception is `%output`, whose payload carries raw pane bytes — including
//! the high bytes of multi-byte UTF-8 characters, which tmux does NOT escape and
//! can split across two notifications. `%output` is therefore parsed from raw
//! bytes (see [`parse_output`]) so those characters survive for the surface to
//! reassemble; a `String` round-trip would replace each split half with U+FFFD.

use crate::models::RemoteTmuxControlMessage;

/// The DCS sequence tmux emits to enter control mode: `ESC P 1000 p`.
const ENTER_SEQUENCE: [u8; 7] = [0x1b, 0x50, 0x31, 0x30, 0x30, 0x30, 0x70];

/// ASCII bytes of the `%output ` notification prefix (used to detect and parse
/// `%output` lines from raw bytes, before any String decode).
const OUTPUT_PREFIX: &[u8] = b"%output ";

/// Incremental `tmux -CC` control-stream parser. Feed bytes with [`feed`].
///
/// [`feed`]: RemoteTmuxControlStreamParser::feed
#[derive(Debug, Clone)]
pub struct RemoteTmuxControlStreamParser {
    max_buffered_line_bytes: usize,
    max_command_block_bytes: usize,
    buffer: Vec<u8>,
    in_block: bool,
    block_number: i64,
    block_lines: Vec<String>,
    block_buffered_bytes: usize,
}

impl Default for RemoteTmuxControlStreamParser {
    fn default() -> Self {
        Self::new(1_048_576, 16_777_216)
    }
}

impl RemoteTmuxControlStreamParser {
    /// Creates a parser with explicit memory budgets. The Swift initializer
    /// clamps each budget to at least `1`.
    pub fn new(max_buffered_line_bytes: usize, max_command_block_bytes: usize) -> Self {
        Self {
            max_buffered_line_bytes: max_buffered_line_bytes.max(1),
            max_command_block_bytes: max_command_block_bytes.max(1),
            buffer: Vec::new(),
            in_block: false,
            block_number: 0,
            block_lines: Vec::new(),
            block_buffered_bytes: 0,
        }
    }

    /// Feeds a chunk of stream bytes and returns any newly completed messages.
    pub fn feed(&mut self, data: &[u8]) -> Vec<RemoteTmuxControlMessage> {
        let mut messages: Vec<RemoteTmuxControlMessage> = Vec::new();
        for &byte in data {
            if byte == 0x0a {
                let mut line_bytes = std::mem::take(&mut self.buffer);
                if line_bytes.last() == Some(&0x0d) {
                    line_bytes.pop(); // strip pty CR
                }
                let parsed = self.parse_line(line_bytes);
                for message in parsed {
                    let is_stream_error =
                        matches!(message, RemoteTmuxControlMessage::StreamError(_));
                    messages.push(message);
                    if is_stream_error {
                        return messages;
                    }
                }
            } else {
                self.buffer.push(byte);
                if self.buffer.len() > self.max_buffered_line_bytes {
                    let reason =
                        format!("line exceeded {} bytes", self.max_buffered_line_bytes);
                    messages.push(self.stream_error(reason));
                    return messages;
                }
            }
        }
        messages
    }

    fn parse_line(&mut self, raw_bytes: Vec<u8>) -> Vec<RemoteTmuxControlMessage> {
        let mut bytes = raw_bytes;
        let mut prefix_messages: Vec<RemoteTmuxControlMessage> = Vec::new();

        // Strip a leading enter DCS (it is prepended to the first %begin line).
        if bytes.starts_with(&ENTER_SEQUENCE) {
            prefix_messages.push(RemoteTmuxControlMessage::Enter);
            bytes.drain(0..ENTER_SEQUENCE.len());
        }
        // Drop ST (ESC \) DCS-teardown framing — but ONLY on notification lines.
        // Command-block content (e.g. `capture-pane -e` output) is raw terminal
        // bytes that can legitimately contain ESC `\` (an OSC String Terminator),
        // and stripping those would corrupt the painted pane. tmux frames the
        // block, so block content is never DCS-framed.
        if !self.in_block {
            bytes = removing_st(&bytes);
        }
        if bytes.is_empty() {
            return prefix_messages;
        }

        // `%output` is the only notification whose payload carries raw, possibly
        // multi-byte UTF-8 pane bytes. Parse it straight from the raw bytes so a
        // character that tmux split across two `%output` notifications (it sends
        // pane bytes raw and chunks PTY reads mid-character) survives intact —
        // a downstream stream parser reassembles split UTF-8 across output
        // calls, but routing each half through a `String` decode first would
        // replace it with U+FFFD before the surface ever sees it.
        if !self.in_block {
            if let Some(output) = parse_output(&bytes) {
                prefix_messages.push(output);
                return prefix_messages;
            }
        }

        // DIVERGENCE: Swift `String(decoding: bytes, as: UTF8.self)` is lossy
        // (invalid UTF-8 → U+FFFD, never fails). `String::from_utf8_lossy` is the
        // exact equivalent. Only `%output` (handled above from raw bytes) carries
        // raw PTY bytes; every line reaching here is tmux-formatted ASCII, so the
        // lossy decode never actually substitutes.
        let line = String::from_utf8_lossy(&bytes).into_owned();

        if self.in_block {
            // Only a %end/%error whose command number matches this block's
            // %begin terminates it. tmux does NOT escape command output inside a
            // block, so a captured pane line like "%end 1 0 0" must be treated as
            // content, not a terminator (otherwise the block truncates and the
            // command-correlation FIFO desyncs permanently).
            if (line.starts_with("%end ") || line.starts_with("%error "))
                && field(&line, 2).and_then(|f| f.parse::<i64>().ok()) == Some(self.block_number)
            {
                let is_error = line.starts_with("%error ");
                let result = RemoteTmuxControlMessage::CommandResult {
                    command_number: self.block_number,
                    lines: std::mem::take(&mut self.block_lines),
                    is_error,
                };
                self.in_block = false;
                self.block_buffered_bytes = 0;
                prefix_messages.push(result);
                return prefix_messages;
            }
            // Block content is always tmux-formatted text — `capture-pane`/
            // `display-message` responses are printable/escaped, never raw PTY
            // bytes split mid-character — so this String round-trip is lossless.
            if self.block_buffered_bytes + bytes.len() + 1 > self.max_command_block_bytes {
                let reason = format!(
                    "command block exceeded {} bytes",
                    self.max_command_block_bytes
                );
                prefix_messages.push(self.stream_error(reason));
                return prefix_messages;
            }
            self.block_buffered_bytes += bytes.len() + 1;
            self.block_lines.push(line);
            return prefix_messages;
        }

        if line.starts_with("%begin ") {
            match field(&line, 2).and_then(|f| f.parse::<i64>().ok()) {
                Some(number) => {
                    self.block_number = number;
                    self.in_block = true;
                    self.block_lines = Vec::new();
                    self.block_buffered_bytes = 0;
                    prefix_messages
                }
                None => {
                    // Malformed `%begin` (missing/non-numeric command number): do
                    // NOT enter block mode — `block_number = 0` would swallow every
                    // later line until a matching `%end ... 0` and wedge the mirror
                    // until reconnect. Treat the bad line as a normal notification.
                    prefix_messages.push(parse_notification(&line));
                    prefix_messages
                }
            }
        } else {
            prefix_messages.push(parse_notification(&line));
            prefix_messages
        }
    }

    fn stream_error(&mut self, reason: String) -> RemoteTmuxControlMessage {
        self.buffer = Vec::new();
        self.in_block = false;
        self.block_number = 0;
        self.block_lines = Vec::new();
        self.block_buffered_bytes = 0;
        RemoteTmuxControlMessage::StreamError(reason)
    }
}

/// Parses an `%output %<pane> <octal-escaped data…>` line directly from its raw
/// bytes, preserving the data's multi-byte UTF-8 exactly. Returns `None` if the
/// line is not a well-formed `%output` notification, so the caller falls back to
/// the String-based notification parser.
///
/// Only the prefix and pane id (pure ASCII) are interpreted as text; the data
/// after the second space is unescaped from raw bytes, so a multi-byte character
/// split across two `%output` notifications is never replaced with U+FFFD.
fn parse_output(bytes: &[u8]) -> Option<RemoteTmuxControlMessage> {
    if !bytes.starts_with(OUTPUT_PREFIX) {
        return None;
    }
    let mut i = OUTPUT_PREFIX.len();
    if !(i < bytes.len() && bytes[i] == b'%') {
        return None;
    }
    i += 1;
    let digits_start = i;
    while i < bytes.len() && bytes[i] >= b'0' && bytes[i] <= b'9' {
        i += 1;
    }
    if !(i > digits_start && i < bytes.len() && bytes[i] == b' ') {
        return None;
    }
    let pane_id: i64 = std::str::from_utf8(&bytes[digits_start..i])
        .ok()?
        .parse()
        .ok()?;
    let data = unescape_output(&bytes[(i + 1)..]);
    Some(RemoteTmuxControlMessage::Output { pane_id, data })
}

fn parse_notification(line: &str) -> RemoteTmuxControlMessage {
    if line == "%exit" || line.starts_with("%exit ") {
        let reason = if line == "%exit" {
            None
        } else {
            Some(line["%exit ".len()..].to_string())
        };
        return RemoteTmuxControlMessage::Exit { reason };
    }
    // NOTE: `%output` is parsed earlier from raw bytes (see `parse_output`),
    // never here — routing its payload through this String would corrupt
    // multi-byte UTF-8 split across notifications. A malformed `%output` that
    // `parse_output` rejects falls through to the `%`-prefix catch-all below as
    // `IgnoredNotification`.
    if line.starts_with("%session-changed ") {
        let Some(id) = field_id(line, 1, '$') else {
            return RemoteTmuxControlMessage::Unparsed(line.to_string());
        };
        // Session names may contain spaces; join the remaining fields.
        return RemoteTmuxControlMessage::SessionChanged {
            session_id: id,
            name: fields_from(line, 2),
        };
    }
    if line.starts_with("%session-renamed ") {
        // tmux emits this for `rename-session` (NOT `%session-changed`, which
        // fires on an attached-session switch). The man page documents only a
        // name, while tmux 3.6a emits "$<id> <name>"; accept both forms.
        if let Some(id) = field_id(line, 1, '$') {
            let name = fields_from(line, 2);
            if !name.is_empty() {
                return RemoteTmuxControlMessage::SessionRenamed {
                    session_id: Some(id),
                    name: fields_from(line, 1),
                    id_bearing_name: Some(name),
                };
            }
        }
        return RemoteTmuxControlMessage::SessionRenamed {
            session_id: None,
            name: fields_from(line, 1),
            id_bearing_name: None,
        };
    }
    if line == "%sessions-changed" {
        return RemoteTmuxControlMessage::SessionsChanged;
    }
    if line.starts_with("%window-add ") {
        let Some(id) = field_id(line, 1, '@') else {
            return RemoteTmuxControlMessage::Unparsed(line.to_string());
        };
        return RemoteTmuxControlMessage::WindowAdd { window_id: id };
    }
    if line.starts_with("%window-close ") || line.starts_with("%unlinked-window-close ") {
        let Some(id) = field_id(line, 1, '@') else {
            return RemoteTmuxControlMessage::Unparsed(line.to_string());
        };
        return RemoteTmuxControlMessage::WindowClose { window_id: id };
    }
    if line.starts_with("%window-renamed ") {
        let Some(id) = field_id(line, 1, '@') else {
            return RemoteTmuxControlMessage::Unparsed(line.to_string());
        };
        let name = fields_from(line, 2);
        return RemoteTmuxControlMessage::WindowRenamed {
            window_id: id,
            name,
        };
    }
    if line.starts_with("%layout-change ") {
        let (Some(id), Some(layout)) = (field_id(line, 1, '@'), field(line, 2)) else {
            return RemoteTmuxControlMessage::Unparsed(line.to_string());
        };
        return RemoteTmuxControlMessage::LayoutChange {
            window_id: id,
            layout: layout.to_string(),
        };
    }
    if line.starts_with("%window-pane-changed ") {
        let (Some(id), Some(pane)) = (field_id(line, 1, '@'), field_id(line, 2, '%')) else {
            return RemoteTmuxControlMessage::Unparsed(line.to_string());
        };
        return RemoteTmuxControlMessage::WindowPaneChanged {
            window_id: id,
            pane_id: pane,
        };
    }
    if line.starts_with("%session-window-changed ") {
        let (Some(sid), Some(wid)) = (field_id(line, 1, '$'), field_id(line, 2, '@')) else {
            return RemoteTmuxControlMessage::Unparsed(line.to_string());
        };
        return RemoteTmuxControlMessage::SessionWindowChanged {
            session_id: sid,
            window_id: wid,
        };
    }
    if line.starts_with("%subscription-changed ") {
        let Some(name) = field(line, 1) else {
            return RemoteTmuxControlMessage::IgnoredNotification(line.to_string());
        };
        // The value is everything after the first " : " separator. The middle
        // fields (session/window/pane/flags) vary by tmux version, so key off the
        // subscription name instead of a fixed field index.
        let value = match line.find(" : ") {
            Some(idx) => line[(idx + " : ".len())..].to_string(),
            None => String::new(),
        };
        return RemoteTmuxControlMessage::SubscriptionChanged {
            name: name.to_string(),
            value,
        };
    }
    if line.starts_with('%') {
        return RemoteTmuxControlMessage::IgnoredNotification(line.to_string());
    }
    RemoteTmuxControlMessage::Unparsed(line.to_string())
}

// MARK: - Helpers

// DIVERGENCE: Swift compares strings by canonical equivalence over grapheme
// clusters (`hasPrefix`, `==`, `split(separator:)`), while the notification
// parsing above and the helpers below compare bytes/scalars. They disagree
// only when a combining mark immediately follows a prefix or separator
// boundary — e.g. Swift does NOT treat "%begin \u{301}5 1" as having prefix
// "%begin " (the grapheme " ́" ≠ " ") and files it as an ignored notification,
// while this port matches the prefix bytewise. tmux protocol framing is pure
// ASCII, so no real control line reaches the difference.

/// Returns the whitespace-separated field at `index` (0-based).
///
// DIVERGENCE: Swift `split(separator: " ", omittingEmptySubsequences: false)`
// keeps empty fields between consecutive spaces. Rust `str::split(' ')` has the
// identical behavior (it does not omit empties), so consecutive spaces map to
// empty fields exactly as Swift.
fn field(line: &str, index: usize) -> Option<&str> {
    line.split(' ').nth(index)
}

/// All fields from `index` onward, rejoined with spaces (for names that may
/// contain spaces).
fn fields_from(line: &str, index: usize) -> String {
    let parts: Vec<&str> = line.split(' ').collect();
    if index >= parts.len() {
        return String::new();
    }
    parts[index..].join(" ")
}

/// Parses the field at `index` as a sigil-prefixed tmux id (`$`/`@`/`%`).
fn field_id(line: &str, index: usize, sigil: char) -> Option<i64> {
    let token = field(line, index)?;
    id(token, sigil)
}

/// Parses a sigil-prefixed tmux id token, e.g. `@4` → 4, `%8` → 8, `$2` → 2.
pub fn id(token: &str, sigil: char) -> Option<i64> {
    let mut chars = token.chars();
    if chars.next() != Some(sigil) {
        return None;
    }
    chars.as_str().parse::<i64>().ok()
}

/// Removes any `ESC \` (ST) sequences from a line's bytes.
fn removing_st(bytes: &[u8]) -> Vec<u8> {
    if !bytes.contains(&0x1b) {
        return bytes.to_vec();
    }
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == 0x5c {
            i += 2;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Octal-unescapes raw `%output` data bytes (`\ooo` → byte). Any byte that is
/// not part of a `\ooo` escape — including the raw high bytes of a multi-byte
/// UTF-8 character — passes through unchanged, so split or whole UTF-8 text
/// survives intact for the surface to decode.
pub fn unescape_output(bytes: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let is_octal = |b: u8| (0x30..=0x37).contains(&b);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x5c // backslash
            && i + 3 < bytes.len()
            && is_octal(bytes[i + 1])
            && is_octal(bytes[i + 2])
            && is_octal(bytes[i + 3])
        {
            // Compute in i32 to avoid a u8 overflow on malformed escapes like
            // \777; emit literally if out of byte range.
            let value = (bytes[i + 1] - 0x30) as i32 * 64
                + (bytes[i + 2] - 0x30) as i32 * 8
                + (bytes[i + 3] - 0x30) as i32;
            if value <= 0xFF {
                out.push(value as u8);
                i += 4;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_str(p: &mut RemoteTmuxControlStreamParser, s: &str) -> Vec<RemoteTmuxControlMessage> {
        p.feed(s.as_bytes())
    }

    // NOTE: tmux control-mode framing is `%begin <timestamp> <command-number>
    // <flags>` — the command number is the THIRD token (0-based field index 2),
    // the second token is the timestamp. The tests below reflect that layout.

    #[test]
    fn enter_sequence_emits_enter_then_begins_block() {
        let mut p = RemoteTmuxControlStreamParser::default();
        let mut bytes = ENTER_SEQUENCE.to_vec();
        bytes.extend_from_slice(b"%begin 1700000000 5 1\n");
        let msgs = p.feed(&bytes);
        assert_eq!(msgs, vec![RemoteTmuxControlMessage::Enter]);
        // %begin consumed silently; %end closes the block.
        let done = feed_str(&mut p, "hello\n%end 1700000000 5 1\n");
        assert_eq!(
            done,
            vec![RemoteTmuxControlMessage::CommandResult {
                command_number: 5,
                lines: vec!["hello".to_string()],
                is_error: false,
            }]
        );
    }

    #[test]
    fn begin_end_coalesces_multiple_lines() {
        let mut p = RemoteTmuxControlStreamParser::default();
        let msgs = feed_str(&mut p, "%begin 100 7 0\nline-a\nline-b\n%end 100 7 0\n");
        assert_eq!(
            msgs,
            vec![RemoteTmuxControlMessage::CommandResult {
                command_number: 7,
                lines: vec!["line-a".to_string(), "line-b".to_string()],
                is_error: false,
            }]
        );
    }

    #[test]
    fn error_terminator_sets_is_error() {
        let mut p = RemoteTmuxControlStreamParser::default();
        let msgs = feed_str(&mut p, "%begin 0 3 0\noops\n%error 0 3 0\n");
        assert_eq!(
            msgs,
            vec![RemoteTmuxControlMessage::CommandResult {
                command_number: 3,
                lines: vec!["oops".to_string()],
                is_error: true,
            }]
        );
    }

    #[test]
    fn nonmatching_end_number_is_treated_as_block_content() {
        // A captured pane line like "%end 1 0 0" inside block #5 is CONTENT, not
        // a terminator: only a matching command number (field 2) closes the block.
        let mut p = RemoteTmuxControlStreamParser::default();
        let msgs = feed_str(&mut p, "%begin 0 5 0\n%end 1 0 0\n%end 0 5 0\n");
        assert_eq!(
            msgs,
            vec![RemoteTmuxControlMessage::CommandResult {
                command_number: 5,
                lines: vec!["%end 1 0 0".to_string()],
                is_error: false,
            }]
        );
    }

    #[test]
    fn malformed_begin_is_not_a_block() {
        // Non-numeric command number (field 2) must NOT enter block mode.
        let mut p = RemoteTmuxControlStreamParser::default();
        let msgs = feed_str(&mut p, "%begin 1 xyz 0\n");
        assert_eq!(
            msgs,
            vec![RemoteTmuxControlMessage::IgnoredNotification(
                "%begin 1 xyz 0".to_string()
            )]
        );
    }

    #[test]
    fn output_octal_unescape() {
        let mut p = RemoteTmuxControlStreamParser::default();
        // \033 → ESC (0x1b); " hi" literal.
        let msgs = feed_str(&mut p, "%output %2 \\033 hi\n");
        assert_eq!(
            msgs,
            vec![RemoteTmuxControlMessage::Output {
                pane_id: 2,
                data: vec![0x1b, b' ', b'h', b'i'],
            }]
        );
    }

    #[test]
    fn output_multibyte_utf8_split_across_two_notifications() {
        // U+00E9 (é) is 0xC3 0xA9. tmux may split it across two %output lines,
        // sending each raw byte separately. Byte-oriented parsing preserves both
        // halves so a downstream decoder can reassemble them.
        let mut p = RemoteTmuxControlStreamParser::default();
        let first = p.feed(&[b"%output %1 ".to_vec(), vec![0xC3], b"\n".to_vec()].concat());
        let second = p.feed(&[b"%output %1 ".to_vec(), vec![0xA9], b"\n".to_vec()].concat());
        assert_eq!(
            first,
            vec![RemoteTmuxControlMessage::Output {
                pane_id: 1,
                data: vec![0xC3],
            }]
        );
        assert_eq!(
            second,
            vec![RemoteTmuxControlMessage::Output {
                pane_id: 1,
                data: vec![0xA9],
            }]
        );
        // Reassembled, the bytes decode to "é".
        let mut joined = Vec::new();
        for m in first.into_iter().chain(second) {
            if let RemoteTmuxControlMessage::Output { data, .. } = m {
                joined.extend(data);
            }
        }
        assert_eq!(String::from_utf8(joined).unwrap(), "é");
    }

    #[test]
    fn output_high_bytes_pass_through_unescaped() {
        // A raw high byte (0xE2) must pass through untouched, not be mangled.
        let mut p = RemoteTmuxControlStreamParser::default();
        let msgs = p.feed(&[b"%output %9 ".to_vec(), vec![0xE2, 0x82, 0xAC], b"\n".to_vec()].concat());
        assert_eq!(
            msgs,
            vec![RemoteTmuxControlMessage::Output {
                pane_id: 9,
                data: vec![0xE2, 0x82, 0xAC],
            }]
        );
    }

    #[test]
    fn dcs_st_stripped_on_notification_lines() {
        // ESC \ (ST) framing on a notification line is removed.
        let mut p = RemoteTmuxControlStreamParser::default();
        let msgs = p.feed(&[b"%window-add @4".to_vec(), vec![0x1b, 0x5c], b"\n".to_vec()].concat());
        assert_eq!(msgs, vec![RemoteTmuxControlMessage::WindowAdd { window_id: 4 }]);
    }

    #[test]
    fn dcs_st_not_stripped_inside_block() {
        // Inside a command block, ESC \ is legitimate content (an OSC
        // terminator) and must survive verbatim.
        let mut p = RemoteTmuxControlStreamParser::default();
        let _ = feed_str(&mut p, "%begin 10 1 0\n");
        let msgs = p.feed(&[b"pane".to_vec(), vec![0x1b, 0x5c], b"end\n".to_vec(), b"%end 10 1 0\n".to_vec()].concat());
        assert_eq!(
            msgs,
            vec![RemoteTmuxControlMessage::CommandResult {
                command_number: 1,
                lines: vec![format!("pane{}\\end", '\u{1b}')],
                is_error: false,
            }]
        );
    }

    #[test]
    fn cr_stripped_from_pty() {
        let mut p = RemoteTmuxControlStreamParser::default();
        let msgs = feed_str(&mut p, "%sessions-changed\r\n");
        assert_eq!(msgs, vec![RemoteTmuxControlMessage::SessionsChanged]);
    }

    #[test]
    fn partial_line_buffered_across_chunks() {
        let mut p = RemoteTmuxControlStreamParser::default();
        assert!(feed_str(&mut p, "%window-").is_empty());
        let msgs = feed_str(&mut p, "add @11\n");
        assert_eq!(msgs, vec![RemoteTmuxControlMessage::WindowAdd { window_id: 11 }]);
    }

    #[test]
    fn exit_with_and_without_reason() {
        let mut p = RemoteTmuxControlStreamParser::default();
        assert_eq!(
            feed_str(&mut p, "%exit\n"),
            vec![RemoteTmuxControlMessage::Exit { reason: None }]
        );
        assert_eq!(
            feed_str(&mut p, "%exit server exited\n"),
            vec![RemoteTmuxControlMessage::Exit {
                reason: Some("server exited".to_string())
            }]
        );
    }

    #[test]
    fn session_renamed_id_bearing_and_name_only() {
        let mut p = RemoteTmuxControlStreamParser::default();
        assert_eq!(
            feed_str(&mut p, "%session-renamed $3 newname\n"),
            vec![RemoteTmuxControlMessage::SessionRenamed {
                session_id: Some(3),
                name: "$3 newname".to_string(),
                id_bearing_name: Some("newname".to_string()),
            }]
        );
        // No id → name-only interpretation.
        assert_eq!(
            feed_str(&mut p, "%session-renamed just-a-name\n"),
            vec![RemoteTmuxControlMessage::SessionRenamed {
                session_id: None,
                name: "just-a-name".to_string(),
                id_bearing_name: None,
            }]
        );
    }

    #[test]
    fn session_changed_joins_name_with_spaces() {
        let mut p = RemoteTmuxControlStreamParser::default();
        assert_eq!(
            feed_str(&mut p, "%session-changed $2 my session\n"),
            vec![RemoteTmuxControlMessage::SessionChanged {
                session_id: 2,
                name: "my session".to_string(),
            }]
        );
    }

    #[test]
    fn subscription_changed_value_after_separator() {
        let mut p = RemoteTmuxControlStreamParser::default();
        assert_eq!(
            feed_str(&mut p, "%subscription-changed cwd $1 @2 %3 - : /home/me\n"),
            vec![RemoteTmuxControlMessage::SubscriptionChanged {
                name: "cwd".to_string(),
                value: "/home/me".to_string(),
            }]
        );
    }

    #[test]
    fn window_pane_and_layout_and_session_window_changes() {
        let mut p = RemoteTmuxControlStreamParser::default();
        assert_eq!(
            feed_str(&mut p, "%window-pane-changed @4 %8\n"),
            vec![RemoteTmuxControlMessage::WindowPaneChanged {
                window_id: 4,
                pane_id: 8,
            }]
        );
        assert_eq!(
            feed_str(&mut p, "%layout-change @4 b2f1,80x24,0,0,1\n"),
            vec![RemoteTmuxControlMessage::LayoutChange {
                window_id: 4,
                layout: "b2f1,80x24,0,0,1".to_string(),
            }]
        );
        assert_eq!(
            feed_str(&mut p, "%session-window-changed $2 @5\n"),
            vec![RemoteTmuxControlMessage::SessionWindowChanged {
                session_id: 2,
                window_id: 5,
            }]
        );
    }

    #[test]
    fn unknown_percent_notification_is_ignored() {
        let mut p = RemoteTmuxControlStreamParser::default();
        assert_eq!(
            feed_str(&mut p, "%pane-mode-changed %1\n"),
            vec![RemoteTmuxControlMessage::IgnoredNotification(
                "%pane-mode-changed %1".to_string()
            )]
        );
    }

    #[test]
    fn non_percent_line_is_unparsed() {
        let mut p = RemoteTmuxControlStreamParser::default();
        assert_eq!(
            feed_str(&mut p, "garbage line\n"),
            vec![RemoteTmuxControlMessage::Unparsed("garbage line".to_string())]
        );
    }

    #[test]
    fn oversized_line_produces_stream_error_and_resets() {
        let mut p = RemoteTmuxControlStreamParser::new(8, 16);
        let msgs = p.feed(b"aaaaaaaaaaaaaaaa"); // 16 bytes, no newline
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], RemoteTmuxControlMessage::StreamError(_)));
    }

    #[test]
    fn oversized_command_block_produces_stream_error() {
        let mut p = RemoteTmuxControlStreamParser::new(1_048_576, 8);
        let _ = feed_str(&mut p, "%begin 1 0 0\n");
        let msgs = feed_str(&mut p, "0123456789\n");
        assert_eq!(msgs.len(), 1);
        assert!(matches!(msgs[0], RemoteTmuxControlMessage::StreamError(_)));
    }

    // The two tests below are ported VERBATIM from Swift
    // `RemoteTmuxControlStreamParserBudgetTests.swift` — same constructor
    // budgets, same inputs, same expected messages, and (crucially) the same
    // expectation that the parser RECOVERS after a budget stream error: the
    // very next feed parses normally because `stream_error` resets the line
    // buffer and block state.

    #[test]
    fn pending_line_overflow_emits_stream_error_and_resets_parser() {
        // Swift: RemoteTmuxControlStreamParser(maxBufferedLineBytes: 8,
        //        maxCommandBlockBytes: 1024)
        let mut parser = RemoteTmuxControlStreamParser::new(8, 1024);

        let overflow = parser.feed(b"abcdefghi");
        assert_eq!(
            overflow,
            vec![RemoteTmuxControlMessage::StreamError(
                "line exceeded 8 bytes".to_string()
            )]
        );
        assert_eq!(
            parser.feed(b"%exit\r\n"),
            vec![RemoteTmuxControlMessage::Exit { reason: None }]
        );
    }

    #[test]
    fn command_block_overflow_emits_stream_error_and_resets_parser() {
        // Swift: RemoteTmuxControlStreamParser(maxBufferedLineBytes: 128,
        //        maxCommandBlockBytes: 10)
        let mut parser = RemoteTmuxControlStreamParser::new(128, 10);

        assert!(parser.feed(b"%begin 1700000000 7 1\r\n").is_empty());
        let overflow = parser.feed(b"123456\r\nabcdef\r\n");
        assert_eq!(
            overflow,
            vec![RemoteTmuxControlMessage::StreamError(
                "command block exceeded 10 bytes".to_string()
            )]
        );
        assert_eq!(
            parser.feed(b"%window-add @5\r\n"),
            vec![RemoteTmuxControlMessage::WindowAdd { window_id: 5 }]
        );
    }

    #[test]
    fn id_parses_sigil_prefixed_tokens() {
        assert_eq!(id("@4", '@'), Some(4));
        assert_eq!(id("%8", '%'), Some(8));
        assert_eq!(id("$2", '$'), Some(2));
        assert_eq!(id("4", '@'), None); // missing sigil
        assert_eq!(id("@x", '@'), None); // non-numeric
    }

    #[test]
    fn unescape_output_out_of_range_octal_is_literal() {
        // \777 = 511 > 0xFF → emitted literally (backslash + digits pass through).
        assert_eq!(unescape_output(b"\\777"), b"\\777".to_vec());
        // \101 = 0x41 = 'A'.
        assert_eq!(unescape_output(b"\\101"), b"A".to_vec());
    }
}
