//! SSH PTY reattach reconnect input filter — pure byte-stream core.
//!
//! Port of the canonical macOS Swift sources
//! `CLI/SSHPTYAttachReconnectInputFilter.swift` (pure core: lines 4-27
//! constants/state/init, 237-312 `filter`/`finish`/`stopFiltering`/
//! `hasPendingInput`/`isFilteringAtProbeBoundary`/`isFilteringActive`/
//! `flushPendingInput`, 314-435 sequence classifiers) plus the sibling value
//! type `CLI/SSHPTYAttachReconnectInputFilterSequenceMatch.swift`.
//!
//! When the cmux CLI reattaches a persistent SSH PTY session
//! (`cmux ssh-pty-attach`), the local terminal replays the seeded remote
//! screen, which contains terminal probe queries (DSR cursor position, DA,
//! DSR status, kitty-keyboard query, DECRQM, OSC 10/11/12 color queries).
//! The terminal answers those on stdin; without filtering the answer bytes
//! would be forwarded to the remote PTY as if the user typed them. This
//! filter strips a leading run of probe-reply escape sequences from stdin and
//! permanently disables itself at the first byte that is not a probe reply
//! (real user typing), when the caller stops it (first remote output), or —
//! in the host pump layer — when a lone ESC gets no continuation within
//! [`PENDING_PROBE_CONTINUATION_TIMEOUT_MS`].
//!
//! The Swift original operates purely on `Data` / `[UInt8]` byte compares
//! (no string/Unicode APIs), so this port is byte-exact by construction.
//!
//! The I/O shell — `startStdinPump`/`pumpStdin` (lines 29-235), `writeAll`
//! (437-454), `pollStdinPump` (456-485) and the whole
//! `CLI/SSHPTYAttachReconnectInputFilterControl.swift` POSIX stop-signal/ack
//! pipe pair — stays in the host transport layer and is intentionally not
//! ported here.

const ESCAPE: u8 = 0x1B; // Swift: `escape`
const BELL: u8 = 0x07; // Swift: `bell`
const LEFT_BRACKET: u8 = 0x5B; // Swift: `leftBracket` '['
const RIGHT_BRACKET: u8 = 0x5D; // Swift: `rightBracket` ']'
const BACKSLASH: u8 = 0x5C; // Swift: `backslash` '\'
const SEMICOLON: u8 = 0x3B; // Swift: `semicolon` ';'
const QUESTION_MARK: u8 = 0x3F; // Swift: `questionMark` '?'
const DOLLAR: u8 = 0x24; // Swift: `dollar` '$'

/// Swift: `maxPendingProbeBytes` (line 13). An incomplete probe-reply suffix
/// longer than this is passed through instead of buffered.
pub const MAX_PENDING_PROBE_BYTES: usize = 512;

/// Swift: `pendingProbeContinuationTimeoutMilliseconds` (line 15). Terminal
/// ESC disambiguation: bounded so a literal Escape key is not held
/// indefinitely. Used by the (unported) stdin pump's poll timeout; exported
/// so the future transport layer shares one source of truth.
pub const PENDING_PROBE_CONTINUATION_TIMEOUT_MS: u32 = 25;

/// Swift: `enum SSHPTYAttachReconnectInputFilterSequenceMatch`
/// (`CLI/SSHPTYAttachReconnectInputFilterSequenceMatch.swift` lines 1-5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceMatch {
    /// Swift: `.strip(length:)` — a whole probe-reply sequence of this byte
    /// length starts at the classification offset; drop it.
    Strip(usize),
    /// Swift: `.incomplete` — the buffer ends mid-sequence; wait for more.
    Incomplete,
    /// Swift: `.passThrough` — not a probe reply; forward verbatim.
    PassThrough,
}

/// Swift: `final class SSHPTYAttachReconnectInputFilter`
/// (`CLI/SSHPTYAttachReconnectInputFilter.swift` lines 4-27).
///
/// The Swift sibling `SSHPTYAttachReconnectInputFilterState` (a Sendable
/// `{ isFiltering, pending }` snapshot for crossing the Task boundary) is
/// just this pair of fields; in Rust the struct itself serves both roles.
#[derive(Debug)]
pub struct ReconnectInputFilter {
    /// Swift: `isFiltering` (line 17).
    is_filtering: bool,
    /// Swift: `pending` (line 18).
    pending: Vec<u8>,
}

impl ReconnectInputFilter {
    /// Swift: `init(enabled:)` (lines 20-22).
    pub fn new(enabled: bool) -> Self {
        Self {
            is_filtering: enabled,
            pending: Vec::new(),
        }
    }

    /// Swift: `func filter(_ data: Data) -> Data` (lines 237-275).
    ///
    /// Strips a leading run of probe-reply sequences from `pending ++ data`.
    /// The first non-probe byte permanently disables filtering (it never
    /// re-enables); an incomplete trailing sequence of at most
    /// [`MAX_PENDING_PROBE_BYTES`] is buffered for the next call, while a
    /// longer one is passed through and disables filtering.
    pub fn filter(&mut self, data: &[u8]) -> Vec<u8> {
        if !self.is_filtering || data.is_empty() {
            return data.to_vec();
        }

        let mut bytes = std::mem::take(&mut self.pending);
        bytes.extend_from_slice(data);

        let mut output = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] != ESCAPE {
                self.is_filtering = false;
                output.extend_from_slice(&bytes[index..]);
                return output;
            }

            match reconnect_probe_reply_sequence(&bytes, index) {
                SequenceMatch::Strip(length) => {
                    index += length;
                }
                SequenceMatch::Incomplete => {
                    let suffix = &bytes[index..];
                    if suffix.len() > MAX_PENDING_PROBE_BYTES {
                        self.is_filtering = false;
                        output.extend_from_slice(suffix);
                        return output;
                    }
                    self.pending.extend_from_slice(suffix);
                    return output;
                }
                SequenceMatch::PassThrough => {
                    self.is_filtering = false;
                    output.extend_from_slice(&bytes[index..]);
                    return output;
                }
            }
        }

        output
    }

    /// Swift: `func finish() -> Data` (lines 277-284). Drains and returns any
    /// buffered pending bytes without changing the filtering flag.
    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }

    /// Swift: `func stopFiltering() -> Data` (lines 286-290). Drains pending
    /// and permanently disables filtering.
    pub fn stop_filtering(&mut self) -> Vec<u8> {
        let input = self.finish();
        self.is_filtering = false;
        input
    }

    /// Swift: `var hasPendingInput` (lines 292-294).
    pub fn has_pending_input(&self) -> bool {
        self.is_filtering && !self.pending.is_empty()
    }

    /// Swift: `var isFilteringAtProbeBoundary` (lines 296-298).
    pub fn is_filtering_at_probe_boundary(&self) -> bool {
        self.is_filtering && self.pending.is_empty()
    }

    /// Swift: `var isFilteringActive` (lines 300-302).
    pub fn is_filtering_active(&self) -> bool {
        self.is_filtering
    }

    /// Swift: `func flushPendingInput() -> Data` (lines 304-312). Unlike
    /// [`Self::stop_filtering`], this is a no-op unless the filter is BOTH
    /// active and holding pending bytes; when it fires it drains pending and
    /// disables filtering.
    pub fn flush_pending_input(&mut self) -> Vec<u8> {
        if !self.has_pending_input() {
            return Vec::new();
        }
        self.is_filtering = false;
        std::mem::take(&mut self.pending)
    }
}

/// Swift: `static func reconnectProbeReplySequence(in:at:)` (lines 314-334).
fn reconnect_probe_reply_sequence(bytes: &[u8], start: usize) -> SequenceMatch {
    if start >= bytes.len() || bytes[start] != ESCAPE {
        return SequenceMatch::PassThrough;
    }
    if start + 1 >= bytes.len() {
        // read() can split immediately after ESC; wait for one more byte
        // before deciding.
        return SequenceMatch::Incomplete;
    }

    match bytes[start + 1] {
        RIGHT_BRACKET => osc_color_reply_sequence(bytes, start),
        LEFT_BRACKET => csi_probe_reply_sequence(bytes, start),
        _ => SequenceMatch::PassThrough,
    }
}

/// Swift: `static func oscColorReplySequence(in:at:)` (lines 336-382).
///
/// Matches replies to OSC 10/11/12 color queries:
/// `ESC ] (10|11|12) ; <payload> (BEL | ESC \)`.
fn osc_color_reply_sequence(bytes: &[u8], start: usize) -> SequenceMatch {
    let mut cursor = start + 2;
    let mut command: Vec<u8> = Vec::new();

    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte == SEMICOLON {
            break;
        }
        if !(0x30..=0x39).contains(&byte) || command.len() >= 2 {
            return SequenceMatch::PassThrough;
        }
        command.push(byte);
        cursor += 1;
    }

    if cursor >= bytes.len() {
        return if is_osc_color_reply_command_prefix(&command) {
            SequenceMatch::Incomplete
        } else {
            SequenceMatch::PassThrough
        };
    }
    if bytes[cursor] != SEMICOLON {
        return SequenceMatch::PassThrough;
    }
    if command != [0x31, 0x30] && command != [0x31, 0x31] && command != [0x31, 0x32] {
        return SequenceMatch::PassThrough;
    }

    cursor += 1;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if byte == BELL {
            return SequenceMatch::Strip(cursor - start + 1);
        }
        if byte == ESCAPE {
            if cursor + 1 >= bytes.len() {
                return SequenceMatch::Incomplete;
            }
            if bytes[cursor + 1] == BACKSLASH {
                return SequenceMatch::Strip(cursor - start + 2);
            }
            // A payload ESC not followed by backslash is skipped as payload
            // (Swift falls through to `cursor += 1`).
        }
        cursor += 1;
    }
    SequenceMatch::Incomplete
}

/// Swift: `static func csiProbeReplySequence(in:at:)` (lines 384-402).
fn csi_probe_reply_sequence(bytes: &[u8], start: usize) -> SequenceMatch {
    let mut cursor = start + 2;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if (0x40..=0x7E).contains(&byte) {
            return if should_strip_csi_reply(bytes, start + 2, cursor) {
                SequenceMatch::Strip(cursor - start + 1)
            } else {
                SequenceMatch::PassThrough
            };
        }
        if !(0x20..=0x3F).contains(&byte) {
            return SequenceMatch::PassThrough;
        }
        cursor += 1;
    }
    SequenceMatch::Incomplete
}

/// Swift: `static func isOSCColorReplyCommandPrefix(_:)` (lines 404-410).
fn is_osc_color_reply_command_prefix(command: &[u8]) -> bool {
    command.is_empty()
        || command == [0x31]
        || command == [0x31, 0x30]
        || command == [0x31, 0x31]
        || command == [0x31, 0x32]
}

/// Swift: `static func shouldStripCSIReply(bytes:bodyStart:finalIndex:)`
/// (lines 412-435).
///
/// Parameters are bytes in `0x30..=0x3F` (digits `; < = > ?`), which must all
/// precede the intermediates in `0x20..=0x2F`; then per final byte:
/// - `R`/`c`/`n`: strip iff no intermediates (cursor-position report
///   `ESC[<r>;<c>R`, DA reply `ESC[?…c`, DSR reply `ESC[<n>n`).
/// - `u`: strip iff no intermediates and the first parameter byte is `?`
///   (kitty keyboard-protocol query reply `ESC[?<flags>u`; a keypress like
///   `ESC[13;2u` has no `?` and is NOT stripped).
/// - `y`: strip iff the intermediates are exactly `$` (DECRPM report
///   `ESC[?<ps>;<pm>$y`).
fn should_strip_csi_reply(bytes: &[u8], body_start: usize, final_index: usize) -> bool {
    let mut parameter_end = body_start;
    while parameter_end < final_index && (0x30..=0x3F).contains(&bytes[parameter_end]) {
        parameter_end += 1;
    }
    if !bytes[parameter_end..final_index]
        .iter()
        .all(|b| (0x20..=0x2F).contains(b))
    {
        return false;
    }

    let parameters = &bytes[body_start..parameter_end];
    let intermediates = &bytes[parameter_end..final_index];
    let final_byte = bytes[final_index];

    match final_byte {
        0x52 | 0x63 | 0x6E => intermediates.is_empty(),
        0x75 => intermediates.is_empty() && parameters.first() == Some(&QUESTION_MARK),
        0x79 => intermediates == [DOLLAR],
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Ported verbatim from cmuxTests/SSHPTYAttachReconnectInputFilterTests.swift
    // === (pure-core tests, lines 6-86). The four stdin-pump tests (lines
    // === 88-204) exercise the unported POSIX fd pump and are skipped.

    /// Swift: `keepsFilteringAcrossProbeOnlyReadsUntilFirstNormalInput`
    /// (lines 6-17).
    #[test]
    fn keeps_filtering_across_probe_only_reads_until_first_normal_input() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(b"\x1b[1;1R\x1b[?1;2c\x1b[?0u"), b"");
        assert_eq!(filter.filter(b"\x1b]11;rgb:e5e5/e9e9/f0f0\x07"), b"");
        assert_eq!(filter.filter(b"\x1b]12;rgb:ffff/ffff/ffff\x07"), b"");

        let normal_input = b"printf keep\n";
        assert_eq!(filter.filter(normal_input), normal_input);

        let later_reply = b"\x1b[2;2R";
        assert_eq!(filter.filter(later_reply), later_reply);
    }

    /// Swift: `keepsFilteringAtIdleProbeBoundaryUntilNormalInput`
    /// (lines 19-30).
    #[test]
    fn keeps_filtering_at_idle_probe_boundary_until_normal_input() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(b"\x1b[1;1R"), b"");
        assert!(filter.is_filtering_at_probe_boundary());

        let live_reply = b"\x1b[2;2R";
        assert_eq!(filter.filter(live_reply), b"");

        let normal_input = b"printf keep\n";
        assert_eq!(filter.filter(normal_input), normal_input);
        assert_eq!(filter.filter(live_reply), live_reply);
    }

    /// Swift: `stopFilteringPreservesLaterProbeLikeInput` (lines 32-39).
    #[test]
    fn stop_filtering_preserves_later_probe_like_input() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(b"\x1b[1;1R"), b"");
        assert_eq!(filter.stop_filtering(), b"");

        let live_reply = b"\x1b[2;2R";
        assert_eq!(filter.filter(live_reply), live_reply);
    }

    /// Swift: `buffersRecognizedSplitOSCColorReplyWithinInitialDrain`
    /// (lines 41-47).
    #[test]
    fn buffers_recognized_split_osc_color_reply_within_initial_drain() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(b"\x1b]11;rgb:e5e5/e9e9"), b"");

        let normal_input = b"printf keep\n";
        let mut second = b"/f0f0\x1b\\".to_vec();
        second.extend_from_slice(normal_input);
        assert_eq!(filter.filter(&second), normal_input);
    }

    /// Swift: `buffersOSCColorReplySplitBeforeCommandSeparator`
    /// (lines 49-56).
    #[test]
    fn buffers_osc_color_reply_split_before_command_separator() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(b"\x1b]1"), b"");
        assert_eq!(filter.filter(b"2"), b"");

        let normal_input = b"printf keep\n";
        let mut second = b";rgb:e5e5/e9e9/f0f0\x07".to_vec();
        second.extend_from_slice(normal_input);
        assert_eq!(filter.filter(&second), normal_input);
    }

    /// Swift: `buffersInitialEscapeUntilProbeContinuationArrives`
    /// (lines 58-65).
    #[test]
    fn buffers_initial_escape_until_probe_continuation_arrives() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(&[0x1B]), b"");

        let normal_input = b"printf keep\n";
        let mut second = b"]11;rgb:e5e5/e9e9/f0f0\x07".to_vec();
        second.extend_from_slice(normal_input);
        assert_eq!(filter.filter(&second), normal_input);
    }

    /// Swift: `passesThroughAmbiguousEscapeAfterNonProbeContinuation`
    /// (lines 67-75).
    #[test]
    fn passes_through_ambiguous_escape_after_non_probe_continuation() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(&[0x1B]), b"");
        assert_eq!(filter.filter(b"x"), b"\x1bx");

        let key_input = b"\x1b[13;2u";
        assert_eq!(filter.filter(key_input), key_input);
    }

    /// Swift: `flushesPendingInputWhenNoContinuationArrives` (lines 77-86).
    #[test]
    fn flushes_pending_input_when_no_continuation_arrives() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(&[0x1B]), b"");
        assert!(filter.has_pending_input());
        assert_eq!(filter.flush_pending_input(), [0x1B]);

        let key_input = b"\x1b[13;2u";
        assert_eq!(filter.filter(key_input), key_input);
    }

    // === Additional Rust-side oracles (derived, byte-pinned expectations).

    /// A fresh filter strips each recognized probe-reply class and stays at
    /// the probe boundary afterwards.
    fn assert_stripped(input: &[u8]) {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(input), b"", "expected {input:?} stripped");
        assert!(
            filter.is_filtering_at_probe_boundary(),
            "expected probe boundary after {input:?}"
        );
    }

    /// A fresh filter passes `input` through verbatim and disables itself.
    fn assert_passed_through(input: &[u8]) {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(input), input, "expected {input:?} verbatim");
        assert!(
            !filter.is_filtering_active(),
            "expected filtering disabled after {input:?}"
        );
    }

    #[test]
    fn classifier_strips_known_probe_replies() {
        // DECRPM report ESC[?<ps>;<pm>$y.
        assert_stripped(b"\x1b[?2026;1$y");
        // DSR status reply ESC[0n.
        assert_stripped(b"\x1b[0n");
        // Kitty keyboard-protocol query reply ESC[?<flags>u.
        assert_stripped(b"\x1b[?1u");
        // Cursor-position report with no params still strips (Swift places
        // no constraint on parameters for R/c/n).
        assert_stripped(b"\x1b[R");
        // OSC 10 with an embedded payload ESC not followed by backslash is
        // skipped as payload; the BEL terminates and strips.
        assert_stripped(b"\x1b]10;rgb:\x1bxff\x07");
        // ST-terminated OSC 11 reply.
        assert_stripped(b"\x1b]11;rgb:e5e5/e9e9/f0f0\x1b\\");
    }

    #[test]
    fn classifier_passes_through_non_probe_sequences() {
        // Shifted-Enter style keypress ESC[13;2u: no '?' parameter.
        assert_passed_through(b"\x1b[13;2u");
        // CSI with an intermediate before R (0x20 space intermediate).
        assert_passed_through(b"\x1b[1 R");
        // 'y' final without the '$' intermediate.
        assert_passed_through(b"\x1b[?2026;1y");
        // OSC 13 is not in the color-query reply set {10,11,12}.
        assert_passed_through(b"\x1b]13;rgb:ffff/ffff/ffff\x07");
        // Three command digits exceed the two-digit cap.
        assert_passed_through(b"\x1b]112;rgb:ffff/ffff/ffff\x07");
        // ESC followed by neither '[' nor ']' (alt-key chord).
        assert_passed_through(b"\x1bOP");
        // CSI body byte outside 0x20..=0x3F before any final.
        assert_passed_through(b"\x1b[1;\x07R");
    }

    /// Swift `filter` line 260: `suffix.count <= maxPendingProbeBytes` — an
    /// incomplete suffix of exactly 512 bytes is still buffered; 513 bytes
    /// passes through and permanently disables filtering.
    #[test]
    fn pending_cap_boundary() {
        // 512 total: ESC ] 1 1 ; + 507 payload bytes, no terminator.
        let mut input = b"\x1b]11;".to_vec();
        input.extend(std::iter::repeat_n(b'a', 507));
        assert_eq!(input.len(), MAX_PENDING_PROBE_BYTES);
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(&input), b"");
        assert!(filter.has_pending_input());

        // 513 total: one more payload byte tips over the cap.
        let mut input = b"\x1b]11;".to_vec();
        input.extend(std::iter::repeat_n(b'a', 508));
        assert_eq!(input.len(), MAX_PENDING_PROBE_BYTES + 1);
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(&input), input);
        assert!(!filter.is_filtering_active());
    }

    /// The cap applies to the accumulated pending ++ data suffix, not just
    /// the latest read: pending from a prior call counts toward the 512.
    #[test]
    fn pending_cap_accumulates_across_calls() {
        let mut filter = ReconnectInputFilter::new(true);
        let mut first = b"\x1b]11;".to_vec();
        first.extend(std::iter::repeat_n(b'a', 400));
        assert_eq!(filter.filter(&first), b"");
        // 405 pending + 108 new = 513 > 512 → the whole suffix passes through.
        let second = vec![b'a'; 108];
        let mut expected = first.clone();
        expected.extend_from_slice(&second);
        assert_eq!(filter.filter(&second), expected);
        assert!(!filter.is_filtering_active());
    }

    /// Swift `filter` line 238 guard: empty input returns empty and leaves
    /// state untouched.
    #[test]
    fn empty_input_is_identity() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(b""), b"");
        assert!(filter.is_filtering_at_probe_boundary());

        // Also with pending held: empty read does not disturb the buffer.
        assert_eq!(filter.filter(&[0x1B]), b"");
        assert_eq!(filter.filter(b""), b"");
        assert!(filter.has_pending_input());
    }

    /// Swift `init(enabled: false)`: a disabled filter is a pure pass-through,
    /// including for probe replies.
    #[test]
    fn disabled_filter_passes_everything_verbatim() {
        let mut filter = ReconnectInputFilter::new(false);
        assert!(!filter.is_filtering_active());
        assert!(!filter.is_filtering_at_probe_boundary());
        let probe = b"\x1b[1;1R";
        assert_eq!(filter.filter(probe), probe);
        assert_eq!(filter.filter(b"printf keep\n"), b"printf keep\n");
    }

    /// Swift `finish` (lines 277-284) drains pending without changing the
    /// filtering flag, unlike `stopFiltering`.
    #[test]
    fn finish_drains_pending_but_keeps_filtering() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(&[0x1B]), b"");
        assert_eq!(filter.finish(), [0x1B]);
        assert!(filter.is_filtering_active());
        // Still filtering: a fresh probe reply is stripped.
        assert_eq!(filter.filter(b"\x1b[1;1R"), b"");
    }

    /// Swift `flushPendingInput` (lines 304-312) is a no-op unless BOTH
    /// filtering and pending; `stopFiltering` fires regardless.
    #[test]
    fn flush_pending_input_requires_pending() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.flush_pending_input(), b"");
        assert!(filter.is_filtering_active());

        let mut disabled = ReconnectInputFilter::new(false);
        assert_eq!(disabled.flush_pending_input(), b"");
    }

    /// Incomplete CSI split across calls resumes correctly.
    #[test]
    fn split_csi_reply_across_calls() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(b"\x1b[1;"), b"");
        assert!(filter.has_pending_input());
        assert_eq!(filter.filter(b"1R"), b"");
        assert!(filter.is_filtering_at_probe_boundary());
    }

    /// OSC payload split right after a payload ESC (Swift lines 371-374:
    /// `cursor + 1 >= bytes.count` → incomplete) then completed by `\` (ST).
    #[test]
    fn split_osc_st_terminator_across_calls() {
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(b"\x1b]10;rgb:ffff\x1b"), b"");
        assert!(filter.has_pending_input());
        assert_eq!(filter.filter(b"\\"), b"");
        assert!(filter.is_filtering_at_probe_boundary());
    }

    /// OSC command that stops being a {10,11,12} prefix mid-buffer passes
    /// through instead of buffering (Swift line 356).
    #[test]
    fn osc_non_color_prefix_at_buffer_end_passes_through() {
        // "\x1b]2" — command byte '2' alone is not a prefix of 10/11/12.
        assert_passed_through(b"\x1b]2");
        // But "\x1b]1" is a prefix → buffered.
        let mut filter = ReconnectInputFilter::new(true);
        assert_eq!(filter.filter(b"\x1b]1"), b"");
        assert!(filter.has_pending_input());
    }

    /// OSC command followed by non-digit non-semicolon passes through
    /// (Swift line 348), e.g. window-title-like OSC.
    #[test]
    fn osc_non_digit_command_byte_passes_through() {
        assert_passed_through(b"\x1b]10a;x\x07");
        // Exact-command check: '1'/';' with a one-digit command is not in
        // {10,11,12} (Swift line 361).
        assert_passed_through(b"\x1b]1;payload\x07");
    }

    /// Mixed buffer: leading probe replies are stripped, then the first
    /// non-probe byte flips filtering off and the remainder is verbatim.
    #[test]
    fn strips_leading_run_then_passes_remainder() {
        let mut filter = ReconnectInputFilter::new(true);
        let mut input = b"\x1b[1;1R\x1b[?1;2c".to_vec();
        input.extend_from_slice(b"ls -la\n\x1b[9;9R");
        assert_eq!(filter.filter(&input), b"ls -la\n\x1b[9;9R");
        assert!(!filter.is_filtering_active());
    }
}
