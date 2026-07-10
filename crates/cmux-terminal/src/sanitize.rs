//! External committed-text ANSI sanitizer.
//!
//! External accessibility / dictation tools should commit plain text, but some
//! inject a leading escape sequence first. This strips those bytes on the
//! committed-text path so they can't leak into the PTY as literals.
//!
//! 1:1 port of the pure byte scan in
//! `Sources/GhosttyTerminalView.swift:11350-11454`
//! (`GhosttyNSView.sanitizeExternalCommittedText` +
//! `consumeLeadingEscapeSequence` / `consumeLeadingCSISequence` /
//! `consumeLeadingEscapedStringSequence`).
//!
//! Behavior (over `text.as_bytes()`):
//! - Strips a run of leading ESC-introduced sequences from the front:
//!   - CSI: `ESC [` … parameter/intermediate bytes `0x20..=0x3F`, final byte
//!     `0x40..=0x7E`.
//!   - SS3: `ESC O` + exactly one following byte.
//!   - DCS/OSC/PM/APC: `ESC` `P`/`]`/`^`/`_` … consumed until BEL (`0x07`),
//!     ST (`ESC \`), or any other control byte (`< 0x20` or `0x7F`), or EOF.
//!   - Any other single-character escape: `ESC` + one byte.
//!   - A C1 CSI encoded as the UTF-8 byte pair `C2 9B` (U+009B), treated like
//!     `ESC [`.
//! - Returns the substring after the leading escapes; the input unchanged when
//!   there are no leading escapes; empty when the input is fully consumed.
//! - Leading printable control bytes (`\n`, `\t`, …) are left untouched: the
//!   scan only fires on `ESC` (`0x1B`) or the `C2 9B` pair, so automation that
//!   commits a bare newline/tab survives.
//!
//! Purity boundary — NOT ported here: the AppKit `insertText` committed-text
//! call site, the surrounding IME/marked-text plumbing, and
//! `GhosttyTextInputSupport.swift`'s `isControlCharacterScalar` /
//! `shouldSendText` fallback filter (a separate scalar-level predicate, not the
//! leading-escape byte scan).

use std::borrow::Cow;

/// Strips leading ESC-introduced control sequences from externally committed
/// text so they cannot reach the PTY as literals.
///
/// Returns [`Cow::Borrowed`] pointing at the original string when there is
/// nothing to strip (mirrors Swift `return text`) or at the surviving tail;
/// the result is empty when the input is fully consumed.
///
/// Swift: `GhosttyNSView.sanitizeExternalCommittedText`
/// (`Sources/GhosttyTerminalView.swift:11350-11380`).
pub fn sanitize_external_committed_text(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    // Swift: `guard !bytes.isEmpty else { return text }`.
    if bytes.is_empty() {
        return Cow::Borrowed(text);
    }

    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == 0x1B {
            index = consume_leading_escape_sequence(bytes, index);
            continue;
        }

        if byte == 0xC2 {
            let next = index + 1;
            if next < bytes.len() && bytes[next] == 0x9B {
                // U+009B (C1 CSI) is encoded as the UTF-8 byte pair C2 9B.
                index = consume_leading_csi_sequence(bytes, next + 1);
                continue;
            }
        }

        break;
    }

    // Swift: `if index == 0 { return text }`.
    if index == 0 {
        return Cow::Borrowed(text);
    }

    // Swift: `guard index < bytes.count else { return "" }`.
    if index >= bytes.len() {
        return Cow::Borrowed("");
    }

    // Swift: `return String(decoding: bytes[index...], as: UTF8.self)`.
    //
    // The consumed prefix is NOT guaranteed to end on a UTF-8 scalar boundary.
    // The SS3 arm (`ESC O <byte>`, :104) and the single-character-escape default
    // arm (:108) consume exactly one raw byte after the introducer regardless of
    // its value, so when that byte is a UTF-8 lead or continuation byte `index`
    // lands mid-scalar. A borrowed `&text[index..]` slice would then panic. Swift
    // uses the non-failable `String(decoding:as:)`, which lossily replaces the
    // broken bytes with U+FFFD; `String::from_utf8_lossy` mirrors that exactly
    // (identical maximal-subpart substitution) and still borrows when the tail is
    // already valid UTF-8, which is the common case.
    String::from_utf8_lossy(&bytes[index..])
}

/// Swift: `consumeLeadingEscapeSequence`
/// (`Sources/GhosttyTerminalView.swift:11382-11403`). `start` points at the
/// `ESC` byte.
fn consume_leading_escape_sequence(bytes: &[u8], start: usize) -> usize {
    let next = start + 1;
    if next >= bytes.len() {
        return bytes.len();
    }

    match bytes[next] {
        // CSI: ESC [ ... final
        0x5B => consume_leading_csi_sequence(bytes, next + 1),
        // SS3: ESC O final
        0x4F => (next + 2).min(bytes.len()),
        // DCS/OSC/PM/APC: consume until BEL/ST or EOF.
        0x50 | 0x5D | 0x5E | 0x5F => consume_leading_escaped_string_sequence(bytes, next + 1),
        // Single-character escape.
        _ => (next + 1).min(bytes.len()),
    }
}

/// Swift: `consumeLeadingCSISequence`
/// (`Sources/GhosttyTerminalView.swift:11405-11425`). `start` points just past
/// the CSI introducer (`ESC [` or `C2 9B`).
fn consume_leading_csi_sequence(bytes: &[u8], start: usize) -> usize {
    let mut index = start;
    while index < bytes.len() {
        let byte = bytes[index];
        if (0x20..=0x3F).contains(&byte) {
            index += 1;
            continue;
        }

        if (0x40..=0x7E).contains(&byte) {
            return index + 1;
        }

        break;
    }

    index
}

/// Swift: `consumeLeadingEscapedStringSequence`
/// (`Sources/GhosttyTerminalView.swift:11427-11454`). `start` points just past
/// the DCS/OSC/PM/APC introducer byte.
fn consume_leading_escaped_string_sequence(bytes: &[u8], start: usize) -> usize {
    let mut index = start;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == 0x07 {
            return index + 1;
        }

        if byte == 0x1B {
            let next = index + 1;
            if next < bytes.len() && bytes[next] == 0x5C {
                return next + 1;
            }
            return index;
        }

        if byte < 0x20 || byte == 0x7F {
            return index + 1;
        }

        index += 1;
    }

    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Ported oracle cases -------------------------------------------------
    // `cmuxTests/CJKIMEInputTests.swift:945-984`
    // `ExternalCommittedTextSanitizationTests`.

    /// Swift: `testStripsLeadingCSISequenceFromExternalCommittedText` (:946).
    #[test]
    fn strips_leading_csi_sequence() {
        assert_eq!(sanitize_external_committed_text("\u{1B}[Chello"), "hello");
    }

    /// Swift: `testStripsLeadingC1CSISequenceFromExternalCommittedText` (:953).
    /// U+009B encodes as the UTF-8 pair `C2 9B`.
    #[test]
    fn strips_leading_c1_csi_sequence() {
        assert_eq!(
            sanitize_external_committed_text("\u{009B}1;5Chello"),
            "hello"
        );
    }

    /// Swift: `testStripsMultipleLeadingControlAndEscapeSequences` (:960).
    #[test]
    fn strips_multiple_leading_sequences() {
        assert_eq!(
            sanitize_external_committed_text("\u{1B}[1;5C\u{1B}OChello"),
            "hello"
        );
    }

    /// Swift: `testLeavesLiteralBracketPrefixedTextUntouched` (:967).
    #[test]
    fn leaves_literal_bracket_prefixed_text_untouched() {
        assert_eq!(
            sanitize_external_committed_text("[Code] review"),
            "[Code] review"
        );
    }

    /// Swift: `testPreservesLeadingControlBytesUsedByAutomation` (:974).
    #[test]
    fn preserves_leading_control_bytes() {
        assert_eq!(sanitize_external_committed_text("\n"), "\n");
        assert_eq!(sanitize_external_committed_text("\tfoo"), "\tfoo");
    }

    // --- Hand-computed edge cases (Swift formula, deterministic) --------------

    /// `guard !bytes.isEmpty else { return text }` — empty stays empty.
    #[test]
    fn empty_input_returns_empty() {
        assert_eq!(sanitize_external_committed_text(""), "");
    }

    /// `guard index < bytes.count else { return "" }` — a CSI that consumes the
    /// whole input yields empty. `1B 5B 43`: final `C` (0x43) closes the CSI at
    /// index 3 == len.
    #[test]
    fn fully_consumed_csi_returns_empty() {
        assert_eq!(sanitize_external_committed_text("\u{1B}[C"), "");
    }

    /// SS3 `ESC O <one>` consumes exactly the introducer plus one byte.
    #[test]
    fn ss3_consumes_two_bytes() {
        assert_eq!(sanitize_external_committed_text("\u{1B}OCrest"), "rest");
    }

    /// SS3 truncated at EOF: `min(count, next+2)` clamps to len → empty.
    #[test]
    fn ss3_truncated_at_eof_returns_empty() {
        assert_eq!(sanitize_external_committed_text("\u{1B}O"), "");
    }

    /// Lone trailing ESC: `next >= count` → `return bytes.count` → empty.
    #[test]
    fn lone_trailing_escape_returns_empty() {
        assert_eq!(sanitize_external_committed_text("\u{1B}"), "");
    }

    /// Single-character escape `ESC X`: default branch consumes `next + 1`.
    #[test]
    fn single_char_escape_stripped() {
        assert_eq!(sanitize_external_committed_text("\u{1B}Xrest"), "rest");
    }

    /// OSC (`ESC ]`) consumed until BEL (`0x07`).
    #[test]
    fn osc_consumed_until_bel() {
        assert_eq!(
            sanitize_external_committed_text("\u{1B}]0;title\u{07}rest"),
            "rest"
        );
    }

    /// OSC consumed until ST (`ESC \`): terminator is two bytes past the body.
    #[test]
    fn osc_consumed_until_st() {
        assert_eq!(
            sanitize_external_committed_text("\u{1B}]0;t\u{1B}\\rest"),
            "rest"
        );
    }

    /// DCS (`ESC P`) consumed until an embedded control byte (`< 0x20`); the
    /// control byte itself is consumed (`return index + 1`).
    #[test]
    fn escaped_string_consumed_until_control_byte() {
        // 1B 50 (ESC P) 78 (x) 0A (LF, < 0x20) → consumed through the LF.
        assert_eq!(sanitize_external_committed_text("\u{1B}Px\nrest"), "rest");
    }

    /// A `C2` byte NOT followed by `9B` is not a C1 CSI — leave it untouched
    /// (`©` = `C2 A9`). Scan breaks at index 0 → input unchanged.
    #[test]
    fn c2_not_followed_by_9b_is_untouched() {
        assert_eq!(sanitize_external_committed_text("\u{00A9}x"), "\u{00A9}x");
    }

    /// Mixed leading run: `ESC [` CSI then a `C2 9B` C1 CSI, then text.
    #[test]
    fn mixed_csi_and_c1_csi_run() {
        assert_eq!(
            sanitize_external_committed_text("\u{1B}[1m\u{009B}2Kfoo"),
            "foo"
        );
    }

    /// The surviving tail may contain multi-byte scalars; the split lands on a
    /// UTF-8 boundary so they are preserved intact.
    #[test]
    fn preserves_multibyte_tail() {
        assert_eq!(sanitize_external_committed_text("\u{1B}[Ccafé"), "café");
    }

    // --- Mid-scalar tail: lossy U+FFFD decode (Swift parity) -----------------
    // Swift `String(decoding: bytes[index...], as: UTF8.self)` is non-failable
    // and lossily replaces broken bytes with U+FFFD. The SS3 and single-char
    // escape arms consume one raw byte regardless of value, so `index` can land
    // mid-scalar; these inputs pin that behavior against a borrowed-slice panic.

    /// SS3 (`ESC O <byte>`) swallows the first UTF-8 lead byte of `é`
    /// (`1B 4F C3 A9`): `index` lands on the lone continuation byte `A9`, which
    /// Swift decodes lossily to a single U+FFFD. A `&text[index..]` slice would
    /// panic mid-scalar.
    #[test]
    fn ss3_consuming_utf8_lead_byte_decodes_lossily() {
        assert_eq!(
            sanitize_external_committed_text("\u{1B}O\u{00E9}"),
            "\u{FFFD}"
        );
    }

    /// Single-character escape default arm (`ESC <byte>`) swallows the lead byte
    /// of `é` (`1B C3 A9`): `index` lands on the continuation byte `A9` →
    /// one U+FFFD, no panic.
    #[test]
    fn single_char_escape_consuming_utf8_lead_byte_decodes_lossily() {
        assert_eq!(
            sanitize_external_committed_text("\u{1B}\u{00E9}"),
            "\u{FFFD}"
        );
    }

    /// SS3 before a 4-byte emoji (`1B 4F F0 9F 98 80`): `index` lands on the
    /// first continuation byte `9F`, leaving three dangling continuation bytes
    /// `9F 98 80` → three U+FFFD (maximal-subpart substitution), matching Swift.
    #[test]
    fn ss3_consuming_multibyte_lead_decodes_each_continuation_byte() {
        assert_eq!(
            sanitize_external_committed_text("\u{1B}O\u{1F600}"),
            "\u{FFFD}\u{FFFD}\u{FFFD}"
        );
    }

    /// Interior escapes (not at the front) are left alone — the scan stops at
    /// the first non-escape byte.
    #[test]
    fn interior_escape_not_stripped() {
        assert_eq!(
            sanitize_external_committed_text("hi\u{1B}[Cthere"),
            "hi\u{1B}[Cthere"
        );
    }
}
