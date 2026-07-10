//! Shared string helpers used by both hook-config ports.
//!
//! These mirror the identical Swift helpers that appear (duplicated) in both
//! `HermesAgentHookConfig` and `RovoDevHookConfig`: `leadingWhitespace`,
//! `serialized`, and `yamlDoubleQuoted`, plus the two Foundation
//! `CharacterSet` trims (`.whitespaces` and `.whitespacesAndNewlines`) that the
//! Swift code applies via `trimmingCharacters(in:)`.
//!
//! DIVERGENCE (sanctioned platform swap): Foundation's `.whitespaces`
//! (Unicode general category `Zs` + U+0009 TAB) and `.whitespacesAndNewlines`
//! (that set plus the newline set U+000A–U+000D, U+0085, U+2028, U+2029) are
//! approximated here by their ASCII members only. `trim_ws` trims SPACE and TAB;
//! `trim_ws_nl` additionally trims LF/CR/VT/FF. The config files these transform
//! are ASCII YAML, so the non-ASCII `Zs`/separator members never appear.

/// Foundation `String.prefix { $0 == " " || $0 == "\t" }` — the run of leading
/// SPACE/TAB characters that opens `line`.
pub(crate) fn leading_whitespace(line: &str) -> &str {
    let end = line
        .find(|c: char| c != ' ' && c != '\t')
        .unwrap_or(line.len());
    &line[..end]
}

/// Foundation `trimmingCharacters(in: .whitespaces)` restricted to ASCII
/// SPACE/TAB (see module divergence note).
pub(crate) fn trim_ws(s: &str) -> &str {
    s.trim_matches(|c: char| c == ' ' || c == '\t')
}

/// Foundation `trimmingCharacters(in: .whitespacesAndNewlines)` restricted to
/// ASCII SPACE/TAB plus the ASCII newline family (see module divergence note).
pub(crate) fn trim_ws_nl(s: &str) -> &str {
    s.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0B}' | '\u{0C}'))
}

/// Swift `serialized(_:)`: `lines.isEmpty ? "" : lines.joined(separator: "\n") + "\n"`.
pub(crate) fn serialized(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

/// Swift `yamlDoubleQuoted(_:)`. Escapes backslash, then double-quote, then
/// newline — in that exact order — and wraps in double quotes. The order is
/// load-bearing: backslash MUST be doubled first so the escapes introduced by
/// the later two replacements are not re-escaped.
pub(crate) fn yaml_double_quoted(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// Swift `Array.insert(contentsOf:at:)` — splice `block` into `lines` at `at`.
pub(crate) fn splice_insert(lines: &mut Vec<String>, at: usize, block: Vec<String>) {
    lines.splice(at..at, block);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_whitespace_takes_spaces_and_tabs_only() {
        assert_eq!(leading_whitespace("  \tfoo"), "  \t");
        assert_eq!(leading_whitespace("foo"), "");
        assert_eq!(leading_whitespace("    "), "    ");
        assert_eq!(leading_whitespace(""), "");
        // A non-space/tab whitespace (e.g. CR) is NOT leading whitespace here.
        assert_eq!(leading_whitespace("\rfoo"), "");
    }

    #[test]
    fn trim_ws_trims_space_and_tab_both_ends() {
        assert_eq!(trim_ws("  \tfoo \t "), "foo");
        assert_eq!(trim_ws("foo"), "foo");
        assert_eq!(trim_ws(""), "");
        // Newlines are NOT trimmed by trim_ws.
        assert_eq!(trim_ws("\nfoo\n"), "\nfoo\n");
    }

    #[test]
    fn trim_ws_nl_also_trims_newline_family() {
        assert_eq!(trim_ws_nl("\n\r foo \t\r\n"), "foo");
        assert_eq!(trim_ws_nl("   "), "");
        assert_eq!(trim_ws_nl("\u{0B}\u{0C}x"), "x");
    }

    #[test]
    fn serialized_matches_swift() {
        assert_eq!(serialized(&[]), "");
        assert_eq!(serialized(&["a".to_string()]), "a\n");
        assert_eq!(serialized(&["a".to_string(), "b".to_string()]), "a\nb\n");
    }

    #[test]
    fn yaml_double_quoted_plain() {
        assert_eq!(yaml_double_quoted("plain"), "\"plain\"");
        assert_eq!(yaml_double_quoted(""), "\"\"");
    }

    #[test]
    fn yaml_double_quoted_escapes_quote() {
        // a"b -> "a\"b"
        assert_eq!(
            yaml_double_quoted("a\"b").chars().collect::<Vec<_>>(),
            vec!['"', 'a', '\\', '"', 'b', '"']
        );
    }

    #[test]
    fn yaml_double_quoted_escapes_backslash() {
        // a\b -> "a\\b"
        assert_eq!(
            yaml_double_quoted("a\\b").chars().collect::<Vec<_>>(),
            vec!['"', 'a', '\\', '\\', 'b', '"']
        );
    }

    #[test]
    fn yaml_double_quoted_escapes_newline() {
        // a<LF>b -> "a\nb" (literal backslash-n, not a newline)
        assert_eq!(
            yaml_double_quoted("a\nb").chars().collect::<Vec<_>>(),
            vec!['"', 'a', '\\', 'n', 'b', '"']
        );
    }

    #[test]
    fn yaml_double_quoted_order_backslash_before_quote() {
        // Input: backslash then quote. Backslash doubled FIRST, then the quote
        // is escaped, yielding three backslashes then the escaped quote.
        assert_eq!(
            yaml_double_quoted("\\\"").chars().collect::<Vec<_>>(),
            vec!['"', '\\', '\\', '\\', '"', '"']
        );
    }

    #[test]
    fn splice_insert_places_block_at_index() {
        let mut lines = vec!["a".to_string(), "d".to_string()];
        splice_insert(&mut lines, 1, vec!["b".to_string(), "c".to_string()]);
        assert_eq!(lines, vec!["a", "b", "c", "d"]);
    }
}
