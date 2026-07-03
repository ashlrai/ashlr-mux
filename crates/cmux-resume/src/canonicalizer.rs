//! Shell-token canonicalization for resume commands.
//!
//! Ported from `SurfaceResumeCommandCanonicalizer` in
//! `Sources/SessionPersistence.swift:636-696` — a faithful char-by-char
//! quote/escape lexer plus cwd normalization and shell-quoting helpers.

/// Tokenizes a resume command into shell-style tokens.
///
/// Faithful port of `SurfaceResumeCommandCanonicalizer.tokens(from:)`
/// (`SessionPersistence.swift:637-677`). Iterates Unicode scalar values
/// (`char`, exactly like Swift's `UnicodeScalar` iteration):
///
/// - Single (`'`) and double (`"`) quotes start/end a quoted run.
/// - A backslash escapes the next scalar only inside a double-quoted run or
///   when unquoted (never inside a single-quoted run).
/// - Unquoted whitespace flushes the current token.
/// - An unbalanced quote yields `None`.
/// - An empty command (no tokens) yields `None`.
pub fn tokens(command: &str) -> Option<Vec<String>> {
    let scalars: Vec<char> = command.chars().collect();
    let mut tokens: Vec<String> = Vec::new();
    let mut token = String::new();
    let mut index = 0usize;
    let mut quote: Option<char> = None;

    // Swift's `flushToken` only appends when the accumulated token is non-empty.
    // A quoted-but-empty token (e.g. `''`) never flushes anything, mirroring
    // Swift where `token` stays empty for `''`.
    macro_rules! flush_token {
        () => {
            if !token.is_empty() {
                tokens.push(std::mem::take(&mut token));
            }
        };
    }

    while index < scalars.len() {
        let scalar = scalars[index];
        if let Some(active_quote) = quote {
            if scalar == active_quote {
                quote = None;
            } else if active_quote == '"' && scalar == '\\' && index + 1 < scalars.len() {
                index += 1;
                token.push(scalars[index]);
            } else {
                token.push(scalar);
            }
        } else if scalar == '\'' || scalar == '"' {
            quote = Some(scalar);
        } else if scalar.is_whitespace() {
            // DIVERGENCE: Swift tests `CharacterSet.whitespacesAndNewlines`;
            // Rust `char::is_whitespace` covers the same ASCII space/tab/newline
            // set (and the common Unicode whitespace) that resume commands ever
            // contain. They differ only on exotic scalars that never appear in a
            // reconstructed resume command line.
            flush_token!();
        } else if scalar == '\\' && index + 1 < scalars.len() {
            index += 1;
            token.push(scalars[index]);
        } else {
            token.push(scalar);
        }
        index += 1;
    }

    if quote.is_some() {
        return None;
    }
    flush_token!();
    if tokens.is_empty() {
        None
    } else {
        Some(tokens)
    }
}

/// Trims and lexically normalizes a working directory, returning `None` for an
/// empty result.
///
/// Faithful port of `SurfaceResumeCommandCanonicalizer.normalizedCWD(_:)`
/// (`SessionPersistence.swift:679-685`).
///
// DIVERGENCE: Swift uses `(rawValue as NSString).expandingTildeInPath` then
// `.standardizingPath`, both Foundation APIs. `expandingTildeInPath` expands a
// leading `~` using `$HOME`, and `standardizingPath` performs a lexical clean
// (collapses `//`, resolves `.`/`..`, strips a trailing slash, keeps `/` for
// root) and *may* touch the filesystem to resolve symlinks when the path
// exists. We deliberately implement a pure, POSIX-style *lexical* normalization
// with no `$HOME`/locale/filesystem dependence: the approval store must produce
// the same canonical cwd on every host so cross-platform signatures validate.
// Fixtures use POSIX-literal absolute paths (no `~`), so tilde expansion is
// intentionally omitted; a leading `~` is left verbatim.
pub fn normalized_cwd(raw_value: Option<&str>) -> Option<String> {
    let raw = raw_value?;
    let trimmed = raw.trim_matches(|c: char| c.is_whitespace());
    if trimmed.is_empty() {
        return None;
    }
    Some(lexical_normalize_posix(trimmed))
}

/// Pure POSIX lexical path clean (mirrors Go `path.Clean` / Foundation
/// `standardizingPath` for absolute string paths):
///
/// - collapses repeated `/`,
/// - drops `.` elements,
/// - resolves inner `..` against the preceding non-`..` element,
/// - drops `..` at the root of a rooted path,
/// - strips a trailing slash (root stays `/`).
fn lexical_normalize_posix(path: &str) -> String {
    let rooted = path.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for component in path.split('/') {
        match component {
            "" | "." => continue,
            ".." => {
                match out.last() {
                    Some(&last) if last != ".." => {
                        out.pop();
                    }
                    _ => {
                        if !rooted {
                            out.push("..");
                        }
                        // A `..` at the root of a rooted path is dropped.
                    }
                }
            }
            other => out.push(other),
        }
    }
    let joined = out.join("/");
    if rooted {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// The allowed unquoted character set for `shellQuoted`.
const SHELL_QUOTED_ALLOWED: &str =
    "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_+-=./:@%";

/// Shell-quotes a value only when it contains characters outside the allowed
/// set. Faithful port of `SurfaceResumeCommandCanonicalizer.shellQuoted(_:)`
/// (`SessionPersistence.swift:687-694`).
pub fn shell_quoted(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    if value.chars().all(|c| SHELL_QUOTED_ALLOWED.contains(c)) {
        return value.to_string();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tokens_plain_words() {
        assert_eq!(tokens("claude --resume main"), Some(toks(&["claude", "--resume", "main"])));
    }

    #[test]
    fn tokens_collapses_runs_of_whitespace() {
        assert_eq!(
            tokens("  claude\t\t--resume \n main "),
            Some(toks(&["claude", "--resume", "main"]))
        );
    }

    #[test]
    fn tokens_single_quotes_preserve_whitespace_and_backslash() {
        // Inside single quotes, backslash is literal.
        assert_eq!(tokens("'a b'"), Some(toks(&["a b"])));
        assert_eq!(tokens(r"'a\nb'"), Some(toks(&[r"a\nb"])));
    }

    #[test]
    fn tokens_double_quotes_process_backslash_escape() {
        // Inside double quotes, backslash escapes the next scalar.
        assert_eq!(tokens(r#""a b""#), Some(toks(&["a b"])));
        assert_eq!(tokens(r#""a\"b""#), Some(toks(&["a\"b"])));
        assert_eq!(tokens(r#""a\\b""#), Some(toks(&[r"a\b"])));
    }

    #[test]
    fn tokens_unquoted_backslash_escapes_next() {
        assert_eq!(tokens(r"a\ b"), Some(toks(&["a b"])));
        assert_eq!(tokens(r"a\\b"), Some(toks(&[r"a\b"])));
    }

    #[test]
    fn tokens_adjacent_quoted_and_unquoted_join_into_one_token() {
        assert_eq!(tokens(r#"foo"bar"'baz'"#), Some(toks(&["foobarbaz"])));
    }

    #[test]
    fn tokens_quoted_empty_string_yields_no_token() {
        // `''` accumulates nothing → flushToken appends nothing → empty → None.
        assert_eq!(tokens("''"), None);
        assert_eq!(tokens(r#""""#), None);
    }

    #[test]
    fn tokens_unbalanced_single_quote_is_none() {
        assert_eq!(tokens("'unterminated"), None);
    }

    #[test]
    fn tokens_unbalanced_double_quote_is_none() {
        assert_eq!(tokens(r#""unterminated"#), None);
    }

    #[test]
    fn tokens_empty_command_is_none() {
        assert_eq!(tokens(""), None);
        assert_eq!(tokens("   \t\n "), None);
    }

    #[test]
    fn tokens_trailing_unquoted_backslash_is_literal_dropped() {
        // A trailing `\` with no following scalar: the `index + 1 < count`
        // guard fails, so it falls through and the backslash is appended.
        assert_eq!(tokens(r"a\"), Some(toks(&[r"a\"])));
    }

    #[test]
    fn shell_quoted_allowed_passthrough() {
        assert_eq!(shell_quoted("claude"), "claude");
        assert_eq!(shell_quoted("a-b_c.d/e:f@g%h+i=j"), "a-b_c.d/e:f@g%h+i=j");
    }

    #[test]
    fn shell_quoted_wraps_disallowed() {
        assert_eq!(shell_quoted("a b"), "'a b'");
        assert_eq!(shell_quoted("it's"), "'it'\\''s'");
    }

    #[test]
    fn shell_quoted_empty_is_double_single_quote() {
        assert_eq!(shell_quoted(""), "''");
    }

    #[test]
    fn normalized_cwd_trims_and_nils_empty() {
        assert_eq!(normalized_cwd(None), None);
        assert_eq!(normalized_cwd(Some("   ")), None);
        assert_eq!(normalized_cwd(Some("")), None);
    }

    #[test]
    fn normalized_cwd_lexical_clean() {
        assert_eq!(normalized_cwd(Some("  /home/user/project  ")).as_deref(), Some("/home/user/project"));
        assert_eq!(normalized_cwd(Some("/home/user/project/")).as_deref(), Some("/home/user/project"));
        assert_eq!(normalized_cwd(Some("/home//user/./project")).as_deref(), Some("/home/user/project"));
        assert_eq!(normalized_cwd(Some("/home/user/foo/../project")).as_deref(), Some("/home/user/project"));
        assert_eq!(normalized_cwd(Some("/..")).as_deref(), Some("/"));
        assert_eq!(normalized_cwd(Some("/")).as_deref(), Some("/"));
    }

    #[test]
    fn normalized_cwd_relative_keeps_leading_dotdot() {
        assert_eq!(normalized_cwd(Some("a/../../b")).as_deref(), Some("../b"));
    }
}
