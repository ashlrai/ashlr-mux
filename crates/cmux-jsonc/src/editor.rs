//! Port of the Swift `JSONCObjectEditor` namespace (`Sources/JSONCParser.swift`,
//! line 233).
//!
//! Format-preserving edit of a (possibly nested) object property: comments and
//! surrounding whitespace outside the edited span are untouched, an inserted
//! property matches its siblings' indentation, the source's newline style
//! (CRLF vs LF vs CR) is preserved, and existing trailing-comma state is
//! honored.
//!
//! ## Sanctioned platform divergences
//!
//! * Swift `String.Index` -> `usize` into a `Vec<char>` (Unicode scalars). See
//!   [`crate::parser`] module docs for why the grapheme-vs-scalar difference is
//!   benign; all offset arithmetic here is internally consistent within the
//!   scalar model, so results are byte-identical to Swift's `String.Index`
//!   mutation (`distance`/`offsetBy`/`replaceSubrange`/`insert`).
//! * `quotedJSONString` (line 518) uses Foundation's `JSONEncoder`, which by
//!   default escapes `/` as `\/`. [`quoted_json_string`] replicates that exact
//!   escaping (control chars via `\b \f \n \r \t` and lowercase `\u00xx`,
//!   forward slash as `\/`) rather than serde_json's slash-preserving output.
//! * `parseJSONString` decodes a string literal via `JSONDecoder`; here that is
//!   `serde_json::from_str::<String>` over the raw quoted substring.

use crate::parser::is_line_terminator;

struct PropertyRange {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

struct ObjectRange {
    #[allow(dead_code)]
    open_brace: usize,
    close_brace: usize,
    properties: Vec<PropertyRange>,
}

impl ObjectRange {
    fn property(&self, key: &str) -> Option<&PropertyRange> {
        self.properties.iter().find(|p| p.key == key)
    }
}

/// Port of `JSONCObjectEditor.setNestedObjectProperty(...)` (line 234).
///
/// Sets `parent_key.child_key` to the already-encoded JSON fragment
/// `child_value_json` inside `source`, preserving formatting. Returns `None`
/// when `source` has no parseable root object (mirrors the Swift `-> String?`).
pub fn set_nested_object_property(
    parent_key: &str,
    child_key: &str,
    child_value_json: &str,
    source: &str,
) -> Option<String> {
    let chars: Vec<char> = source.chars().collect();
    let root = root_object(&chars)?;
    let newline = preferred_newline(&chars);

    if let Some(parent) = root.property(parent_key) {
        let parent_value_start = skip_whitespace_and_comments(&chars, parent.value_start);
        if parent_value_start >= chars.len() {
            return None;
        }
        if chars[parent_value_start] == '{' {
            if let Some(parent_object) = parse_object(&chars, parent_value_start) {
                if let Some(child) = parent_object.property(child_key) {
                    let child_indent = indentation_before_line(&chars, child.key_start);
                    let replacement = with_preferred_newline(
                        &value_json_for_property(child_value_json, &child_indent),
                        newline,
                    );
                    return Some(replacing(&chars, child.value_start, child.value_end, &replacement));
                }

                let child_indent = property_indent(&parent_object, &chars);
                let child_property = property_text(child_key, child_value_json, &child_indent);
                return Some(inserting(&child_property, &parent_object, &chars));
            }
        }

        let parent_indent = indentation_before_line(&chars, parent.key_start);
        let child_indent = format!("{parent_indent}  ");
        let child_property = property_text(child_key, child_value_json, &child_indent);
        let replacement = with_preferred_newline(
            &format!("{{\n{child_property}\n{parent_indent}}}"),
            newline,
        );
        return Some(replacing(&chars, parent.value_start, parent.value_end, &replacement));
    }

    let parent_indent = property_indent(&root, &chars);
    let child_indent = format!("{parent_indent}  ");
    let child_property = property_text(child_key, child_value_json, &child_indent);
    let parent_property = format!(
        "{parent_indent}{}: {{\n{child_property}\n{parent_indent}}}",
        quoted_json_string(parent_key)
    );
    Some(inserting(&parent_property, &root, &chars))
}

/// Port of `rootObject(in:)` (line 293).
fn root_object(chars: &[char]) -> Option<ObjectRange> {
    let n = chars.len();
    let mut index = skip_whitespace_and_comments(chars, 0);
    if index < n && chars[index] == '\u{feff}' {
        index += 1;
        index = skip_whitespace_and_comments(chars, index);
    }
    if index >= n || chars[index] != '{' {
        return None;
    }
    parse_object(chars, index)
}

/// Port of `parseObject(in:at:)` (line 303).
fn parse_object(chars: &[char], open_brace: usize) -> Option<ObjectRange> {
    let n = chars.len();
    if open_brace >= n || chars[open_brace] != '{' {
        return None;
    }
    let close_brace = matching_container_end(chars, open_brace)?;

    let mut properties: Vec<PropertyRange> = Vec::new();
    let mut index = open_brace + 1;
    loop {
        index = skip_whitespace_and_comments(chars, index);
        if index >= close_brace {
            return Some(ObjectRange {
                open_brace,
                close_brace,
                properties,
            });
        }
        if chars[index] == ',' {
            index += 1;
            continue;
        }
        if chars[index] != '"' {
            return None;
        }
        let parsed_key = parse_json_string(chars, index)?;

        index = skip_whitespace_and_comments(chars, parsed_key.1);
        if index >= close_brace || chars[index] != ':' {
            return None;
        }
        index += 1;
        let value_start = skip_whitespace_and_comments(chars, index);
        if value_start >= close_brace {
            return None;
        }
        let value_end = skip_value(chars, value_start)?;

        properties.push(PropertyRange {
            key: parsed_key.2,
            key_start: parsed_key.0,
            value_start,
            value_end,
        });
        index = value_end;
    }
}

/// Port of `matchingContainerEnd(in:at:)` (line 342).
fn matching_container_end(chars: &[char], start: usize) -> Option<usize> {
    let n = chars.len();
    let opening = chars[start];
    let closing = if opening == '{' {
        '}'
    } else if opening == '[' {
        ']'
    } else {
        return None;
    };

    let mut stack: Vec<char> = vec![closing];
    let mut index = start + 1;
    while index < n {
        let character = chars[index];
        if character == '"' {
            let string_end = parse_json_string(chars, index)?.1;
            index = string_end;
            continue;
        }
        if let Some(next) = skip_comment_at(chars, index) {
            index = next;
            continue;
        }
        if character == '{' {
            stack.push('}');
        } else if character == '[' {
            stack.push(']');
        } else if stack.last() == Some(&character) {
            stack.pop();
            if stack.is_empty() {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

/// Port of `skipValue(in:from:)` (line 399).
fn skip_value(chars: &[char], start: usize) -> Option<usize> {
    let n = chars.len();
    if start >= n {
        return None;
    }
    let character = chars[start];
    if character == '{' || character == '[' {
        let end = matching_container_end(chars, start)?;
        return Some(end + 1);
    }
    if character == '"' {
        return parse_json_string(chars, start).map(|parsed| parsed.1);
    }

    let mut index = start;
    while index < n {
        let current = chars[index];
        if current == ',' || current == '}' || current == ']' || current.is_whitespace() {
            return Some(index);
        }
        if current == '/' {
            let next = index + 1;
            if next < n && (chars[next] == '/' || chars[next] == '*') {
                return Some(index);
            }
        }
        index += 1;
    }
    Some(index)
}

/// Port of `parseJSONString(in:at:)` (line 427). Returns
/// `(start, end, decoded_value)`.
fn parse_json_string(chars: &[char], start: usize) -> Option<(usize, usize, String)> {
    let n = chars.len();
    if start >= n || chars[start] != '"' {
        return None;
    }
    let mut index = start + 1;
    let mut is_escaped = false;
    while index < n {
        let character = chars[index];
        if is_escaped {
            is_escaped = false;
        } else if character == '\\' {
            is_escaped = true;
        } else if character == '"' {
            let end = index + 1;
            let raw: String = chars[start..end].iter().collect();
            match serde_json::from_str::<String>(&raw) {
                Ok(value) => return Some((start, end, value)),
                Err(_) => return None,
            }
        }
        index += 1;
    }
    None
}

/// Port of `skipWhitespaceAndComments(in:from:)` (line 451).
fn skip_whitespace_and_comments(chars: &[char], start: usize) -> usize {
    let n = chars.len();
    let mut index = start;
    while index < n {
        let character = chars[index];
        if character.is_whitespace() || character == '\u{feff}' {
            index += 1;
            continue;
        }
        if let Some(next) = skip_comment_at(chars, index) {
            index = next;
            continue;
        }
        return index;
    }
    index
}

/// Shared comment-skip used by [`matching_container_end`] and
/// [`skip_whitespace_and_comments`] (both Swift originals repeat this scan
/// verbatim). Returns the index just past a `//` line comment (stopping at the
/// line terminator) or `/* */` block comment (running to end of input when
/// unterminated) starting at `index`, or `None` when `chars[index]` does not
/// start a comment.
fn skip_comment_at(chars: &[char], index: usize) -> Option<usize> {
    let n = chars.len();
    if chars[index] != '/' {
        return None;
    }
    let next = index + 1;
    if next < n && chars[next] == '/' {
        let mut index = next + 1;
        while index < n && !is_line_terminator(chars[index]) {
            index += 1;
        }
        return Some(index);
    }
    if next < n && chars[next] == '*' {
        let mut index = next + 1;
        while index < n {
            let following = index + 1;
            if chars[index] == '*' && following < n && chars[following] == '/' {
                return Some(following + 1);
            }
            index = following;
        }
        return Some(index);
    }
    None
}

/// Port of `propertyIndent(for:in:)` (line 486).
fn property_indent(object: &ObjectRange, chars: &[char]) -> String {
    let mut indent = indentation_before_line(chars, object.close_brace);
    indent.push_str("  ");
    indent
}

/// Port of `indentationBeforeLine(containing:in:)` (line 490).
fn indentation_before_line(chars: &[char], index: usize) -> String {
    let mut line_start = index;
    while line_start > 0 {
        let previous = line_start - 1;
        if is_line_terminator(chars[previous]) {
            break;
        }
        line_start = previous;
    }

    let n = chars.len();
    let mut indentation = String::new();
    let mut cursor = line_start;
    while cursor < n {
        let character = chars[cursor];
        if character == ' ' || character == '\t' {
            indentation.push(character);
            cursor += 1;
            continue;
        }
        break;
    }
    indentation
}

/// Port of `propertyText(key:valueJSON:indent:)` (line 514).
fn property_text(key: &str, value_json: &str, indent: &str) -> String {
    format!(
        "{indent}{}: {}",
        quoted_json_string(key),
        value_json_for_property(value_json, indent)
    )
}

/// Port of `quotedJSONString(_:)` (line 518).
///
/// Matches Foundation `JSONEncoder`'s default escaping, including `/` -> `\/`.
fn quoted_json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for scalar in value.chars() {
        match scalar {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '/' => out.push_str("\\/"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Port of `valueJSONForProperty(_:propertyIndent:)` (line 529).
fn value_json_for_property(value_json: &str, property_indent: &str) -> String {
    let lines = split_on_lf_not_crlf(value_json);
    let first = lines[0];
    let mut result = String::from(first);
    for line in &lines[1..] {
        result.push('\n');
        result.push_str(property_indent);
        result.push_str(line);
    }
    result
}

/// Swift-faithful equivalent of `valueJSON.split(separator: "\n",
/// omittingEmptySubsequences: false)`.
///
/// Swift `split(separator:)` operates on `Character` (extended grapheme
/// clusters). Per Unicode UAX #29 (GB3, CR × LF), a `\r\n` is always a single
/// grapheme-cluster `Character` that is not equal to the `\n` `Character`, so
/// Swift never treats the LF of a CRLF pair as a split boundary — the `\r\n`
/// stays within its segment. Only a standalone `\n` (one not immediately
/// preceded by `\r`) is a boundary.
///
/// Naive `str::split('\n')` would split inside `\r\n`, re-indenting CRLF
/// continuation lines that Swift leaves flush. This splits only on a `\n` that
/// is not the LF of a `\r\n` pair. `\n` and `\r` are ASCII, so byte scanning is
/// safe and every boundary is a UTF-8 char boundary. Always yields at least one
/// element, matching both Swift's `omittingEmptySubsequences: false` and Rust's
/// `str::split`.
fn split_on_lf_not_crlf(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut segments = Vec::new();
    let mut segment_start = 0usize;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' && !(i > 0 && bytes[i - 1] == b'\r') {
            segments.push(&text[segment_start..i]);
            segment_start = i + 1;
        }
    }
    segments.push(&text[segment_start..]);
    segments
}

/// Port of `preferredNewline(in:)` (line 535).
fn preferred_newline(chars: &[char]) -> &'static str {
    let mut has_cr = false;
    for (i, &c) in chars.iter().enumerate() {
        if c == '\r' {
            has_cr = true;
            if chars.get(i + 1) == Some(&'\n') {
                return "\r\n";
            }
        }
    }
    if has_cr {
        "\r"
    } else {
        "\n"
    }
}

/// Port of `withPreferredNewline(_:newline:)` (line 545).
fn with_preferred_newline(text: &str, newline: &str) -> String {
    if newline == "\n" {
        text.to_string()
    } else {
        text.replace('\n', newline)
    }
}

/// Port of `replacing(_:from:to:with:)` (line 550).
fn replacing(chars: &[char], from: usize, to: usize, replacement: &str) -> String {
    let mut result: String = chars[..from].iter().collect();
    result.push_str(replacement);
    result.extend(chars[to..].iter());
    result
}

/// Port of `inserting(_:into:in:)` (line 561).
///
/// Reconstructs the Swift in-place mutation directly: an optional separating
/// comma after the last property, then the new property block before the close
/// brace. See module docs for why the scalar model yields identical bytes.
fn inserting(property_text: &str, object: &ObjectRange, chars: &[char]) -> String {
    let closing_indent = indentation_before_line(chars, object.close_brace);
    let newline = preferred_newline(chars);
    let normalized_property_text = with_preferred_newline(property_text, newline);
    let insert_text = format!("{newline}{normalized_property_text}{newline}{closing_indent}");

    let mut result = String::new();
    match object.properties.last() {
        Some(last) if !has_trailing_comma(chars, last, object.close_brace) => {
            result.extend(chars[..last.value_end].iter());
            result.push(',');
            result.extend(chars[last.value_end..object.close_brace].iter());
        }
        _ => result.extend(chars[..object.close_brace].iter()),
    }
    result.push_str(&insert_text);
    result.extend(chars[object.close_brace..].iter());
    result
}

/// Port of `hasTrailingComma(after:before:in:)` (line 581).
fn has_trailing_comma(chars: &[char], property: &PropertyRange, close_brace: usize) -> bool {
    let index = skip_whitespace_and_comments(chars, property.value_end);
    index < close_brace && chars[index] == ','
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_existing_child_value_preserves_comment() {
        let src = "{\n  \"parent\": {\n    \"child\": 1 // keep\n  }\n}";
        let out = set_nested_object_property("parent", "child", "2", src).unwrap();
        assert_eq!(out, "{\n  \"parent\": {\n    \"child\": 2 // keep\n  }\n}");
    }

    #[test]
    fn replace_existing_child_with_multiline_value_reindents() {
        let src = "{\n  \"parent\": {\n    \"child\": 1\n  }\n}";
        let out = set_nested_object_property("parent", "child", "{\n  \"x\": 2\n}", src).unwrap();
        assert_eq!(
            out,
            "{\n  \"parent\": {\n    \"child\": {\n      \"x\": 2\n    }\n  }\n}"
        );
    }

    #[test]
    fn insert_child_into_populated_parent_adds_comma() {
        let src = "{\n  \"parent\": {\n    \"a\": 1\n  }\n}";
        let out = set_nested_object_property("parent", "b", "2", src).unwrap();
        assert_eq!(
            out,
            "{\n  \"parent\": {\n    \"a\": 1,\n  \n    \"b\": 2\n  }\n}"
        );
    }

    #[test]
    fn insert_child_into_parent_with_existing_trailing_comma_no_double() {
        let src = "{\n  \"parent\": {\n    \"a\": 1,\n  }\n}";
        let out = set_nested_object_property("parent", "b", "2", src).unwrap();
        assert_eq!(
            out,
            "{\n  \"parent\": {\n    \"a\": 1,\n  \n    \"b\": 2\n  }\n}"
        );
    }

    #[test]
    fn insert_parent_into_empty_root() {
        let out = set_nested_object_property("parent", "child", "1", "{}").unwrap();
        assert_eq!(out, "{\n  \"parent\": {\n    \"child\": 1\n  }\n}");
    }

    #[test]
    fn insert_parent_into_populated_root_adds_comma() {
        let src = "{\n  \"a\": 1\n}";
        let out = set_nested_object_property("parent", "child", "2", src).unwrap();
        assert_eq!(
            out,
            "{\n  \"a\": 1,\n\n  \"parent\": {\n    \"child\": 2\n  }\n}"
        );
    }

    #[test]
    fn parent_value_not_object_is_replaced() {
        let src = "{\n  \"parent\": 5\n}";
        let out = set_nested_object_property("parent", "child", "1", src).unwrap();
        assert_eq!(out, "{\n  \"parent\": {\n    \"child\": 1\n  }\n}");
    }

    #[test]
    fn crlf_value_replace_preserves_crlf() {
        let src = "{\r\n  \"parent\": {\r\n    \"child\": 1\r\n  }\r\n}";
        let out = set_nested_object_property("parent", "child", "2", src).unwrap();
        assert_eq!(
            out,
            "{\r\n  \"parent\": {\r\n    \"child\": 2\r\n  }\r\n}"
        );
    }

    #[test]
    fn crlf_insert_converts_newlines_and_preserves_crlf() {
        let out = set_nested_object_property("parent", "child", "1", "{\r\n}").unwrap();
        assert_eq!(
            out,
            "{\r\n\r\n  \"parent\": {\r\n    \"child\": 1\r\n  }\r\n}"
        );
    }

    #[test]
    fn crlf_value_fragment_inner_lines_stay_flush() {
        // Regression: Swift's `split(separator: "\n")` operates on grapheme-cluster
        // Characters, and a `\r\n` is a single Character that is not the `\n`
        // Character, so Swift does NOT split at the LF of a CRLF pair. The interior
        // CRLF lines therefore stay flush (no continuation-line reindent). Naive
        // `str::split('\n')` would split inside `\r\n` and re-indent them.
        // Source is LF-only, so the preferred newline is "\n" and the fragment's
        // CRLFs pass through untouched.
        let src = "{\n  \"parent\": {\n    \"child\": 1\n  }\n}";
        let out =
            set_nested_object_property("parent", "child", "{\r\n  \"x\": 2\r\n}", src).unwrap();
        assert_eq!(
            out,
            "{\n  \"parent\": {\n    \"child\": {\r\n  \"x\": 2\r\n}\n  }\n}"
        );
    }

    #[test]
    fn value_json_for_property_splits_lf_not_crlf() {
        // Only a standalone LF is a split boundary; the LF inside a CRLF stays put,
        // so its following text is not reindented.
        assert_eq!(value_json_for_property("a\r\nb\nc", "  "), "a\r\nb\n  c");
        // Pure-LF fragment reindents every continuation line (unchanged behavior).
        assert_eq!(value_json_for_property("a\nb\nc", "  "), "a\n  b\n  c");
        // Pure-CRLF fragment has no standalone LF, so nothing is reindented.
        assert_eq!(value_json_for_property("a\r\nb\r\nc", "  "), "a\r\nb\r\nc");
        // Single line: returned as-is.
        assert_eq!(value_json_for_property("solo", "  "), "solo");
    }

    #[test]
    fn non_object_root_returns_none() {
        assert_eq!(set_nested_object_property("a", "b", "1", "[1,2]"), None);
        assert_eq!(set_nested_object_property("a", "b", "1", "123"), None);
    }

    #[test]
    fn quoted_key_escapes_forward_slash_like_foundation() {
        // Foundation JSONEncoder default escapes "/" as "\/".
        assert_eq!(quoted_json_string("a/b"), "\"a\\/b\"");
        // Control-char and quote escaping.
        assert_eq!(quoted_json_string("x\"\n\t"), "\"x\\\"\\n\\t\"");
        assert_eq!(quoted_json_string("\u{1}"), "\"\\u0001\"");
        assert_eq!(quoted_json_string("plain"), "\"plain\"");
    }

    #[test]
    fn inserted_slash_key_uses_escaped_slash() {
        let out = set_nested_object_property("a/b", "c", "1", "{}").unwrap();
        assert!(
            out.contains("\"a\\/b\": {"),
            "expected escaped-slash key, got: {out}"
        );
    }

    #[test]
    fn comments_outside_edit_span_are_untouched() {
        let src = "{\n  // leading\n  \"parent\": {\n    /* inner */\n    \"child\": 1\n  }\n}";
        let out = set_nested_object_property("parent", "child", "2", src).unwrap();
        assert_eq!(
            out,
            "{\n  // leading\n  \"parent\": {\n    /* inner */\n    \"child\": 2\n  }\n}"
        );
    }

    #[test]
    fn slashes_inside_string_value_not_treated_as_comment() {
        // The "//" inside the existing string value must not confuse parsing.
        let src = "{\n  \"parent\": {\n    \"url\": \"http://x\"\n  }\n}";
        let out = set_nested_object_property("parent", "url", "\"http://y\"", src).unwrap();
        assert_eq!(
            out,
            "{\n  \"parent\": {\n    \"url\": \"http://y\"\n  }\n}"
        );
    }
}
