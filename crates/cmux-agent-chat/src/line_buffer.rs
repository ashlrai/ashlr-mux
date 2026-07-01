//! Byte-to-line splitting for provider stdio.
//!
//! Ported verbatim from the canonical macOS Swift `AgentSessionOutputLineBuffer`
//! (`Sources/Panels/AgentSessionOutputLineBuffer.swift`). Bytes are accumulated
//! and split on LF (`0x0A`); each complete line is emitted with its trailing
//! `"\n"` reappended. A partial (newline-less) tail is retained across
//! `append` calls until a newline arrives or [`OutputLineBuffer::flush`] is
//! called. CR bytes are preserved verbatim (a CRLF line yields `"...\r\n"`).

/// Matches the Swift `maxBufferedBytes` cap (1 MiB). When the retained buffer
/// reaches this size without a newline, it is force-flushed as one line.
const MAX_BUFFERED_BYTES: usize = 1024 * 1024;

/// Accumulates bytes and yields newline-terminated lines.
#[derive(Debug, Default)]
pub struct OutputLineBuffer {
    buffer: Vec<u8>,
}

impl OutputLineBuffer {
    /// Create an empty buffer.
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of bytes currently retained in the partial-line tail.
    pub fn buffered_byte_count(&self) -> usize {
        self.buffer.len()
    }

    /// Append a chunk of bytes, returning any complete lines it produced.
    ///
    /// Lossy UTF-8 decoding matches Swift's `String(decoding:as:UTF8.self)`.
    pub fn append(&mut self, data: &[u8]) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let mut cursor = 0usize;
        while cursor < data.len() {
            let available = MAX_BUFFERED_BYTES.saturating_sub(self.buffer.len()).max(1);
            let remaining = data.len() - cursor;
            let chunk_end = cursor + available.min(remaining);
            self.buffer.extend_from_slice(&data[cursor..chunk_end]);
            cursor = chunk_end;
            self.drain_buffered_lines(&mut lines);
            if self.buffer.len() >= MAX_BUFFERED_BYTES {
                let mut forced = String::from_utf8_lossy(&self.buffer).into_owned();
                forced.push('\n');
                lines.push(forced);
                self.buffer.clear();
            }
        }
        lines
    }

    /// Emit any retained partial line (without appending a newline) and reset.
    pub fn flush(&mut self) -> Vec<String> {
        if self.buffer.is_empty() {
            return Vec::new();
        }
        let text = String::from_utf8_lossy(&self.buffer).into_owned();
        self.buffer.clear();
        vec![text]
    }

    fn drain_buffered_lines(&mut self, lines: &mut Vec<String>) {
        let mut cursor = 0usize;
        let mut consumed_end: Option<usize> = None;
        while cursor < self.buffer.len() {
            match self.buffer[cursor..].iter().position(|&b| b == 0x0A) {
                Some(rel) => {
                    let newline_index = cursor + rel;
                    let mut line =
                        String::from_utf8_lossy(&self.buffer[cursor..newline_index]).into_owned();
                    line.push('\n');
                    lines.push(line);
                    cursor = newline_index + 1;
                    consumed_end = Some(cursor);
                }
                None => break,
            }
        }
        if let Some(end) = consumed_end {
            self.buffer.drain(0..end);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_complete_lines_and_retains_partial() {
        let mut buf = OutputLineBuffer::new();
        let lines = buf.append(b"alpha\nbeta\ngam");
        assert_eq!(lines, vec!["alpha\n".to_string(), "beta\n".to_string()]);
        // "gam" is retained as a partial line.
        assert_eq!(buf.buffered_byte_count(), 3);
    }

    #[test]
    fn partial_line_completes_across_chunks() {
        let mut buf = OutputLineBuffer::new();
        assert!(buf.append(b"hel").is_empty());
        assert!(buf.append(b"lo").is_empty());
        let lines = buf.append(b" world\n");
        assert_eq!(lines, vec!["hello world\n".to_string()]);
        assert_eq!(buf.buffered_byte_count(), 0);
    }

    #[test]
    fn crlf_carriage_return_is_preserved() {
        let mut buf = OutputLineBuffer::new();
        let lines = buf.append(b"line1\r\nline2\r\n");
        assert_eq!(lines, vec!["line1\r\n".to_string(), "line2\r\n".to_string()]);
    }

    #[test]
    fn flush_emits_trailing_partial_without_newline() {
        let mut buf = OutputLineBuffer::new();
        assert!(buf.append(b"done\ntrailing").len() == 1);
        let flushed = buf.flush();
        assert_eq!(flushed, vec!["trailing".to_string()]);
        // Flushing again is a no-op.
        assert!(buf.flush().is_empty());
    }

    #[test]
    fn flush_of_empty_buffer_is_empty() {
        let mut buf = OutputLineBuffer::new();
        assert!(buf.flush().is_empty());
    }

    #[test]
    fn empty_line_yields_bare_newline() {
        let mut buf = OutputLineBuffer::new();
        let lines = buf.append(b"\n\n");
        assert_eq!(lines, vec!["\n".to_string(), "\n".to_string()]);
    }
}
