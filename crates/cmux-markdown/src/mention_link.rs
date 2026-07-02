//! Port of `TextBoxMentionMarkdown` (`Sources/TextBoxMentionMarkdown.swift`) —
//! the pure builder that turns a mention label + file path into a markdown link
//! for insertion into the composer. String→string only, no parsing.
//!
//! Only the mention-link builder is ported. `SidebarMetadataMarkdownRenderer`
//! (the other markdown text file) is intentionally NOT ported: its real work is
//! Apple's `AttributedString(markdown:)` parser, which has no faithful Rust
//! equivalent, and substituting a different parser would break the port's
//! fidelity discipline.

/// Build a markdown link `[label](target)` for a mention (port of `link(label:path:)`).
///
/// The label is escaped for use inside `[...]` and the path is turned into a
/// markdown link target by [`markdown_target`].
pub fn link(label: &str, path: &str) -> String {
    // Escape ORDER is load-bearing: the backslash replacement MUST run first, or
    // the backslashes inserted by the `[`/`]` escapes get double-escaped.
    let escaped_label = label
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    // Only `\`, `[`, `]` are escaped — parentheses are safe inside `[...]`.
    format!("[{escaped_label}]({})", markdown_target(path))
}

/// Turn a file path into a markdown link target (port of `markdownTarget(for:)`).
///
/// If the path contains whitespace/newlines or any of `( ) < >`, it is wrapped in
/// angle brackets with only `<`/`>` percent-encoded (parens are safe inside
/// `<...>`). Otherwise the bare path is returned with `)` percent-encoded.
fn markdown_target(path: &str) -> String {
    // `char::is_whitespace` (Unicode White_Space) stands in for Swift's
    // `.whitespacesAndNewlines`; they agree on the practically-relevant
    // space/tab/newline set and differ only on exotic code points that do not
    // occur in file paths (documented minor divergence).
    let needs_wrapping = path.chars().any(|c| c.is_whitespace())
        || path.contains('(')
        || path.contains(')')
        || path.contains('<')
        || path.contains('>');
    if needs_wrapping {
        let escaped_path = path.replace('<', "%3C").replace('>', "%3E");
        format!("<{escaped_path}>")
    } else {
        // Faithful to Swift: the non-wrapped branch percent-encodes `)`. (Given
        // the guard above already excludes any `)`, this is a no-op in practice,
        // but the port mirrors the canonical source verbatim.)
        path.replace(')', "%29")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_golden() {
        // Verbatim from cmuxTests/TextBoxMentionCompletionTests.swift:473-481.
        assert_eq!(
            link("@Docs/[draft].md", "Docs/roadmap <draft>.md"),
            "[@Docs/\\[draft\\].md](<Docs/roadmap %3Cdraft%3E.md>)"
        );
    }

    #[test]
    fn label_escape_order_no_double_escape() {
        // A pre-existing backslash before a bracket must not be double-escaped by
        // the bracket pass: `a\[b` -> escape `\` first -> `a\\[b` -> escape `[`
        // -> `a\\\[b`.
        assert_eq!(link("a\\[b", "x"), "[a\\\\\\[b](x)");
    }

    #[test]
    fn label_parens_are_not_escaped() {
        assert_eq!(link("fn(x)", "x"), "[fn(x)](x)");
    }

    #[test]
    fn plain_path_emitted_verbatim() {
        assert_eq!(link("x", "docs/a.md"), "[x](docs/a.md)");
    }

    #[test]
    fn path_with_space_is_angle_wrapped() {
        assert_eq!(link("x", "a b.md"), "[x](<a b.md>)");
    }

    #[test]
    fn path_with_parens_is_angle_wrapped_without_encoding_parens() {
        // `(`/`)` trigger wrapping but are left intact inside `<...>`.
        assert_eq!(link("x", "a(b).md"), "[x](<a(b).md>)");
    }

    #[test]
    fn path_with_angles_is_wrapped_and_encoded() {
        assert_eq!(link("x", "a<b>.md"), "[x](<a%3Cb%3E.md>)");
    }
}
