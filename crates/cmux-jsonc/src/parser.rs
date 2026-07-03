//! Port of the Swift `JSONCParser` namespace (`Sources/JSONCParser.swift`).
//!
//! Faithful 1:1 transcription of the Foundation string state-machines that
//! turn a raw JSONC config byte stream into strict JSON bytes.
//!
//! ## Sanctioned platform divergences
//!
//! * Swift `Data` -> Rust `&[u8]` / `Vec<u8>`.
//! * Swift `String.Index` (extended-grapheme-cluster cursor) -> `usize` index
//!   into a `Vec<char>` (Unicode scalars). The only grapheme cluster that
//!   combines a JSON-structural character is `"\r\n"` (one `Character` in
//!   Swift, two scalars in Rust). Every algorithm here either (a) appends the
//!   scalars verbatim, producing byte-identical output, or (b) branches on
//!   line terminators via [`is_line_terminator`], whose CR/LF-first-scalar
//!   check makes the split-vs-combined representation indistinguishable. The
//!   scalar model is therefore provably equivalent for these functions.
//! * Swift `String.Encoding` -> the [`Encoding`] enum. The `source` fallback
//!   omits Foundation's `NSString.stringEncoding(for:)` multi-encoding lossy
//!   heuristic (Foundation-only); per spec it defaults to UTF-8 and otherwise
//!   returns [`JsoncError::InvalidTextEncoding`].
//! * Swift `LocalizedError` -> a `thiserror` enum with byte-identical messages.

/// Text encodings detected by [`detected_json_encoding`], mirroring the subset
/// of `String.Encoding` the Swift source inspects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16BigEndian,
    Utf16LittleEndian,
    Utf32BigEndian,
    Utf32LittleEndian,
}

/// Port of the Swift `JSONCError` `LocalizedError`. The `Display` strings are
/// transcribed verbatim from `errorDescription`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JsoncError {
    #[error("config file text encoding is not supported")]
    InvalidTextEncoding,
    #[error("invalid trailing comma")]
    InvalidTrailingComma,
    #[error("unterminated block comment")]
    UnterminatedBlockComment,
}

/// Port of `JSONCParser.preprocess(data:)` (line 4).
///
/// Decodes the source, drops a leading BOM (`U+FEFF`), strips comments, then
/// strips trailing commas, returning strict-JSON UTF-8 bytes.
pub fn preprocess(data: &[u8]) -> Result<Vec<u8>, JsoncError> {
    let (source, _encoding) = source(data)?;
    let without_bom = match source.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => source,
    };
    let stripped = strip_comments(&without_bom)?;
    let normalized = strip_trailing_commas(&stripped)?;
    Ok(normalized.into_bytes())
}

/// Port of `JSONCParser.source(data:)` (line 12).
///
/// Returns the decoded text and the encoding used. See the module docs for the
/// omitted `NSString` heuristic (the final Foundation fallback).
pub fn source(data: &[u8]) -> Result<(String, Encoding), JsoncError> {
    if let Some(encoding) = detected_json_encoding(data) {
        if let Some(text) = decode(data, encoding) {
            return Ok((text, encoding));
        }
    }
    if let Ok(text) = std::str::from_utf8(data) {
        return Ok((text.to_string(), Encoding::Utf8));
    }
    Err(JsoncError::InvalidTextEncoding)
}

/// Port of `JSONCParser.detectedJSONEncoding(for:)` (line 57).
///
/// Inspects the leading (up to) four bytes for a BOM or a zero-byte pattern.
pub fn detected_json_encoding(data: &[u8]) -> Option<Encoding> {
    let bytes = &data[..data.len().min(4)];
    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return Some(Encoding::Utf32BigEndian);
    }
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return Some(Encoding::Utf32LittleEndian);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Some(Encoding::Utf16BigEndian);
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Some(Encoding::Utf16LittleEndian);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Some(Encoding::Utf8);
    }
    if bytes.len() < 4 {
        return None;
    }

    match (bytes[0] == 0, bytes[1] == 0, bytes[2] == 0, bytes[3] == 0) {
        (true, true, true, false) => Some(Encoding::Utf32BigEndian),
        (false, true, true, true) => Some(Encoding::Utf32LittleEndian),
        (true, false, true, false) => Some(Encoding::Utf16BigEndian),
        (false, true, false, true) => Some(Encoding::Utf16LittleEndian),
        _ => None,
    }
}

fn decode(data: &[u8], encoding: Encoding) -> Option<String> {
    match encoding {
        Encoding::Utf8 => std::str::from_utf8(data).ok().map(|s| s.to_string()),
        Encoding::Utf16BigEndian => decode_utf16(data, true),
        Encoding::Utf16LittleEndian => decode_utf16(data, false),
        Encoding::Utf32BigEndian => decode_utf32(data, true),
        Encoding::Utf32LittleEndian => decode_utf32(data, false),
    }
}

fn decode_utf16(bytes: &[u8], big_endian: bool) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        let unit = if big_endian {
            u16::from_be_bytes([chunk[0], chunk[1]])
        } else {
            u16::from_le_bytes([chunk[0], chunk[1]])
        };
        units.push(unit);
    }
    String::from_utf16(&units).ok()
}

fn decode_utf32(bytes: &[u8], big_endian: bool) -> Option<String> {
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut result = String::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        let value = if big_endian {
            u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        } else {
            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        };
        match char::from_u32(value) {
            Some(c) => result.push(c),
            None => return None,
        }
    }
    Some(result)
}

/// Port of `JSONCParser.isLineTerminator(_:)` (line 80).
///
/// A Rust `char` is a single Unicode scalar, so Swift's "first scalar of the
/// grapheme cluster" reduces to a direct comparison. CRLF is handled by callers
/// iterating scalars: the `\r` scalar alone already satisfies the check.
pub fn is_line_terminator(character: char) -> bool {
    character == '\n' || character == '\r'
}

/// Port of `JSONCParser.stripComments(from:)` (line 88).
///
/// String-and-escape-aware: `//` and `/* */` inside a JSON string literal are
/// left intact.
pub fn strip_comments(source: &str) -> Result<String, JsoncError> {
    let chars: Vec<char> = source.chars().collect();
    let n = chars.len();
    let mut result = String::new();
    let mut index = 0usize;
    let mut in_string = false;
    let mut is_escaped = false;

    while index < n {
        let character = chars[index];

        if in_string {
            result.push(character);
            if is_escaped {
                is_escaped = false;
            } else if character == '\\' {
                is_escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if character == '"' {
            in_string = true;
            result.push(character);
            index += 1;
            continue;
        }

        if character == '/' {
            let next_index = index + 1;
            if next_index < n {
                let next = chars[next_index];
                if next == '/' {
                    index = next_index + 1;
                    while index < n && !is_line_terminator(chars[index]) {
                        index += 1;
                    }
                    continue;
                }
                if next == '*' {
                    index = next_index + 1;
                    let mut did_close = false;
                    while index < n {
                        let current = chars[index];
                        let following_index = index + 1;
                        if current == '*' && following_index < n && chars[following_index] == '/' {
                            index = following_index + 1;
                            did_close = true;
                            break;
                        }
                        index = following_index;
                    }
                    if !did_close {
                        return Err(JsoncError::UnterminatedBlockComment);
                    }
                    continue;
                }
            }
        }

        result.push(character);
        index += 1;
    }

    Ok(result)
}

/// Port of `JSONCParser.stripTrailingCommas(from:)` (line 156).
///
/// Removes a comma that is followed only by whitespace before a `}` or `]`;
/// throws [`JsoncError::InvalidTrailingComma`] when the comma has no preceding
/// value (an empty container / doubled comma).
pub fn strip_trailing_commas(source: &str) -> Result<String, JsoncError> {
    let chars: Vec<char> = source.chars().collect();
    let n = chars.len();
    let mut result = String::new();
    let mut index = 0usize;
    let mut in_string = false;
    let mut is_escaped = false;
    let mut last_significant_character: Option<char> = None;

    while index < n {
        let character = chars[index];

        if in_string {
            result.push(character);
            if is_escaped {
                is_escaped = false;
            } else if character == '\\' {
                is_escaped = true;
            } else if character == '"' {
                in_string = false;
                last_significant_character = Some(character);
            }
            index += 1;
            continue;
        }

        if character == '"' {
            in_string = true;
            result.push(character);
            index += 1;
            continue;
        }

        if character == ',' {
            let mut lookahead = index + 1;
            while lookahead < n && chars[lookahead].is_whitespace() {
                lookahead += 1;
            }
            if lookahead < n && (chars[lookahead] == '}' || chars[lookahead] == ']') {
                if last_significant_character.is_none()
                    || last_significant_character == Some(',')
                    || last_significant_character == Some('{')
                    || last_significant_character == Some('[')
                    || last_significant_character == Some(':')
                {
                    return Err(JsoncError::InvalidTrailingComma);
                }
                index += 1;
                continue;
            }
        }

        result.push(character);
        if !character.is_whitespace() {
            last_significant_character = Some(character);
        }
        index += 1;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_terminator_membership() {
        assert!(is_line_terminator('\n'));
        assert!(is_line_terminator('\r'));
        assert!(!is_line_terminator(' '));
        assert!(!is_line_terminator('\t'));
        assert!(!is_line_terminator('a'));
    }

    #[test]
    fn encoding_bom_detection() {
        assert_eq!(
            detected_json_encoding(&[0xEF, 0xBB, 0xBF, 0x7B]),
            Some(Encoding::Utf8)
        );
        assert_eq!(
            detected_json_encoding(&[0xFE, 0xFF]),
            Some(Encoding::Utf16BigEndian)
        );
        // FF FE 41 00 is a UTF-16LE BOM (not the FF FE 00 00 UTF-32LE BOM).
        assert_eq!(
            detected_json_encoding(&[0xFF, 0xFE, 0x41, 0x00]),
            Some(Encoding::Utf16LittleEndian)
        );
        assert_eq!(
            detected_json_encoding(&[0x00, 0x00, 0xFE, 0xFF]),
            Some(Encoding::Utf32BigEndian)
        );
        // The UTF-32LE BOM must win over the UTF-16LE BOM (checked first).
        assert_eq!(
            detected_json_encoding(&[0xFF, 0xFE, 0x00, 0x00]),
            Some(Encoding::Utf32LittleEndian)
        );
    }

    #[test]
    fn encoding_zero_pattern_detection() {
        // "{" in each encoding, no BOM.
        assert_eq!(
            detected_json_encoding(&[0x00, 0x00, 0x00, 0x7B]),
            Some(Encoding::Utf32BigEndian)
        );
        assert_eq!(
            detected_json_encoding(&[0x7B, 0x00, 0x00, 0x00]),
            Some(Encoding::Utf32LittleEndian)
        );
        // 00 7B 00 22 -> (true,false,true,false) -> UTF-16BE
        assert_eq!(
            detected_json_encoding(&[0x00, 0x7B, 0x00, 0x22]),
            Some(Encoding::Utf16BigEndian)
        );
        // 7B 00 22 00 -> (false,true,false,true) -> UTF-16LE
        assert_eq!(
            detected_json_encoding(&[0x7B, 0x00, 0x22, 0x00]),
            Some(Encoding::Utf16LittleEndian)
        );
    }

    #[test]
    fn encoding_none_for_plain_and_short() {
        assert_eq!(detected_json_encoding(b"{\"a\":1}"), None);
        assert_eq!(detected_json_encoding(&[0x7B]), None);
        assert_eq!(detected_json_encoding(&[]), None);
    }

    #[test]
    fn source_plain_utf8() {
        let (text, enc) = source(b"{}").unwrap();
        assert_eq!(text, "{}");
        assert_eq!(enc, Encoding::Utf8);
    }

    #[test]
    fn source_utf8_bom_retains_feff() {
        // The detected UTF-8 encoding decodes the whole buffer, so the FEFF
        // survives as U+FEFF (preprocess is what strips it).
        let (text, enc) = source(&[0xEF, 0xBB, 0xBF, 0x7B, 0x7D]).unwrap();
        assert_eq!(text, "\u{feff}{}");
        assert_eq!(enc, Encoding::Utf8);
    }

    #[test]
    fn source_utf16_be_bom() {
        // FE FF 00 7B 00 7D = BOM + "{}" in UTF-16BE.
        let (text, enc) = source(&[0xFE, 0xFF, 0x00, 0x7B, 0x00, 0x7D]).unwrap();
        assert_eq!(text, "\u{feff}{}");
        assert_eq!(enc, Encoding::Utf16BigEndian);
    }

    #[test]
    fn strip_line_comment() {
        assert_eq!(strip_comments("{\"a\":1} // hi").unwrap(), "{\"a\":1} ");
    }

    #[test]
    fn strip_block_comment() {
        assert_eq!(strip_comments("{/* x */\"a\":1}").unwrap(), "{\"a\":1}");
    }

    #[test]
    fn strip_comment_like_text_inside_strings_preserved() {
        // // inside a string value
        assert_eq!(
            strip_comments("{\"a\":\"http://x\"}").unwrap(),
            "{\"a\":\"http://x\"}"
        );
        // /* */ inside a string value
        assert_eq!(
            strip_comments("{\"a\":\"/* not */\"}").unwrap(),
            "{\"a\":\"/* not */\"}"
        );
        // escaped quote then // still inside the string
        assert_eq!(
            strip_comments("{\"a\":\"x\\\"//y\"}").unwrap(),
            "{\"a\":\"x\\\"//y\"}"
        );
    }

    #[test]
    fn strip_line_comment_stops_at_crlf() {
        // The line comment runs to the CR; the CRLF and following token remain.
        assert_eq!(
            strip_comments("{\"a\":1}// c\r\n\"b\"").unwrap(),
            "{\"a\":1}\r\n\"b\""
        );
    }

    #[test]
    fn strip_comments_unterminated_block() {
        assert_eq!(
            strip_comments("{/* x").unwrap_err(),
            JsoncError::UnterminatedBlockComment
        );
    }

    #[test]
    fn strip_trailing_comma_object_and_array() {
        assert_eq!(strip_trailing_commas("{\"a\":1,}").unwrap(), "{\"a\":1}");
        assert_eq!(strip_trailing_commas("[1,2,]").unwrap(), "[1,2]");
    }

    #[test]
    fn strip_trailing_comma_nested() {
        assert_eq!(
            strip_trailing_commas("{\"a\":[1,2,],}").unwrap(),
            "{\"a\":[1,2]}"
        );
    }

    #[test]
    fn strip_trailing_comma_keeps_interior_whitespace() {
        // Only the comma is dropped; the newline before the bracket stays.
        assert_eq!(strip_trailing_commas("[1,\n]").unwrap(), "[1\n]");
    }

    #[test]
    fn strip_trailing_comma_not_trailing_when_more_content() {
        assert_eq!(
            strip_trailing_commas("{\"a\":1,\"b\":2}").unwrap(),
            "{\"a\":1,\"b\":2}"
        );
    }

    #[test]
    fn strip_trailing_comma_invalid_cases() {
        assert_eq!(
            strip_trailing_commas("[,]").unwrap_err(),
            JsoncError::InvalidTrailingComma
        );
        assert_eq!(
            strip_trailing_commas("{,}").unwrap_err(),
            JsoncError::InvalidTrailingComma
        );
        assert_eq!(
            strip_trailing_commas("[1,,]").unwrap_err(),
            JsoncError::InvalidTrailingComma
        );
    }

    #[test]
    fn preprocess_strips_utf8_bom() {
        let out = preprocess(&[0xEF, 0xBB, 0xBF, 0x7B, 0x7D]).unwrap();
        assert_eq!(out, b"{}");
    }

    #[test]
    fn preprocess_full_pipeline() {
        // BOM + block comment + trailing comma all removed.
        let mut input = vec![0xEF, 0xBB, 0xBF];
        input.extend_from_slice(b"{ /* c */ \"a\": 1, }");
        let out = preprocess(&input).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "{  \"a\": 1 }");
    }

    #[test]
    fn preprocess_line_comment_and_trailing_comma() {
        let out = preprocess(b"{\n  \"a\": 1, // note\n}").unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "{\n  \"a\": 1 \n}");
    }
}
