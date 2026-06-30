//! Shared transcript primitive: OSC 133 command spans (M2 WS5 contract).
//!
//! The terminal engine segments the PTY byte stream into prompt / command /
//! output regions using OSC 133 markers (the same `A`/`B`/`C`/`D` markers the
//! crate-root [`crate::Osc133Parser`] already recognizes). Where the parser
//! produces materialized [`crate::TerminalCommandBlock`]s (command text +
//! folded output) for the renderer's scrollback UI, this module defines the
//! *positional* view: byte-offset [`Osc133Span`]s into the raw stream that
//! downstream consumers (M8 agent transcripts, mobile observers on the byte
//! tee) use to slice the original bytes without re-parsing.
//!
//! This is a versioned, parity-tested wire contract ([`OSC133_SPAN_CONTRACT_VERSION`],
//! cross-cutting rule 1), frozen early like the geometry/theme contracts because
//! M8 depends on its JSON shape.

use serde::{Deserialize, Serialize};

/// Wire-format version for the OSC 133 span contract. Bump on any breaking
/// change to the JSON shape.
pub const OSC133_SPAN_CONTRACT_VERSION: u32 = 1;

/// Which OSC 133 region a span covers.
///
/// Serialized lowercase (`"prompt"` / `"command"` / `"output"`) to match the
/// `kind: prompt|command|output` shape consumed by the web chrome and M8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Osc133SpanKind {
    /// The shell prompt (between `OSC 133 ; A` and `OSC 133 ; B`).
    Prompt,
    /// The entered command line (between `OSC 133 ; B` and `OSC 133 ; C`).
    Command,
    /// The command's output (between `OSC 133 ; C` and `OSC 133 ; D`).
    Output,
}

/// A half-open byte range `[start, end)` into the raw PTY stream, tagged with
/// the OSC 133 region it covers. Offsets are byte indices (UTF-8), so a
/// consumer can slice the original buffer directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Osc133Span {
    pub kind: Osc133SpanKind,
    pub start: usize,
    pub end: usize,
}

impl Osc133Span {
    pub fn new(kind: Osc133SpanKind, start: usize, end: usize) -> Self {
        Self { kind, start, end }
    }

    /// The number of bytes the span covers (`0` if the range is empty or
    /// inverted).
    #[inline]
    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    /// Whether the span covers no bytes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    /// Whether `start <= end` (a well-formed half-open range).
    #[inline]
    pub fn is_valid(&self) -> bool {
        self.start <= self.end
    }

    /// Slice the bytes this span covers out of the raw stream it indexes,
    /// returning `None` if the range is inverted or out of bounds.
    pub fn slice<'a>(&self, stream: &'a [u8]) -> Option<&'a [u8]> {
        if self.start > self.end || self.end > stream.len() {
            return None;
        }
        Some(&stream[self.start..self.end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn len_and_emptiness() {
        let span = Osc133Span::new(Osc133SpanKind::Command, 4, 11);
        assert_eq!(span.len(), 7);
        assert!(!span.is_empty());
        assert!(span.is_valid());

        let empty = Osc133Span::new(Osc133SpanKind::Prompt, 5, 5);
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
        assert!(empty.is_valid());
    }

    #[test]
    fn inverted_range_is_invalid_and_zero_len() {
        let span = Osc133Span::new(Osc133SpanKind::Output, 9, 3);
        assert_eq!(span.len(), 0);
        assert!(!span.is_valid());
    }

    #[test]
    fn slice_extracts_the_covered_bytes() {
        let stream = b"prompt$ echo hi\noutput";
        let cmd = Osc133Span::new(Osc133SpanKind::Command, 8, 15);
        assert_eq!(cmd.slice(stream), Some(&b"echo hi"[..]));
    }

    #[test]
    fn slice_out_of_bounds_is_none() {
        let stream = b"short";
        assert_eq!(
            Osc133Span::new(Osc133SpanKind::Output, 2, 99).slice(stream),
            None
        );
        assert_eq!(
            Osc133Span::new(Osc133SpanKind::Output, 4, 2).slice(stream),
            None
        );
    }

    #[test]
    fn kind_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Osc133SpanKind::Prompt).unwrap(),
            "\"prompt\""
        );
        assert_eq!(
            serde_json::to_string(&Osc133SpanKind::Command).unwrap(),
            "\"command\""
        );
        assert_eq!(
            serde_json::to_string(&Osc133SpanKind::Output).unwrap(),
            "\"output\""
        );
    }

    #[test]
    fn span_json_roundtrip() {
        let json = r#"{ "kind": "output", "start": 16, "end": 22 }"#;
        let span: Osc133Span = serde_json::from_str(json).unwrap();
        assert_eq!(span.kind, Osc133SpanKind::Output);
        assert_eq!(span.start, 16);
        assert_eq!(span.end, 22);
        let back = serde_json::to_string(&span).unwrap();
        let again: Osc133Span = serde_json::from_str(&back).unwrap();
        assert_eq!(span, again);
    }
}
