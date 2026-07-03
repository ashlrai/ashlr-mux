//! Port of `Sources/TextBoxMentionMarkdown.swift`.

/// Swift: `TextBoxMentionMarkdown.link(label:path:)`.
pub fn link(label: &str, path: &str) -> String {
    let escaped_label = label
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    format!("[{escaped_label}]({})", markdown_target(path))
}

/// Swift: `TextBoxMentionMarkdown.markdownTarget(for:)`.
fn markdown_target(path: &str) -> String {
    // Swift: `path.rangeOfCharacter(from: .whitespacesAndNewlines) != nil`.
    // Rust `char::is_whitespace` (Unicode White_Space) is exactly the union
    // of Foundation's `.whitespaces` (Zs + U+0009) and `.newlines`
    // (U+000A-U+000D, U+0085, U+2028, U+2029).
    if path.chars().any(char::is_whitespace)
        || path.contains('(')
        || path.contains(')')
        || path.contains('<')
        || path.contains('>')
    {
        let escaped_path = path.replace('<', "%3C").replace('>', "%3E");
        return format!("<{escaped_path}>");
    }
    path.replace(')', "%29")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Oracle: cmuxTests/TextBoxMentionCompletionTests.swift
    // `testTextBoxMentionMarkdownEscapesAngleTargetDelimiters`.
    #[test]
    fn escapes_angle_target_delimiters() {
        let link = link("@Docs/[draft].md", "Docs/roadmap <draft>.md");
        assert_eq!(link, "[@Docs/\\[draft\\].md](<Docs/roadmap %3Cdraft%3E.md>)");
    }

    #[test]
    fn plain_path_stays_bare() {
        assert_eq!(link("@a.txt", "/tmp/a.txt"), "[@a.txt](/tmp/a.txt)");
    }

    #[test]
    fn backslash_in_label_is_escaped_first() {
        // Swift replaces "\\" before "[" / "]", so a literal backslash never
        // merges with the bracket escapes.
        assert_eq!(link("a\\b", "p"), "[a\\\\b](p)");
    }

    #[test]
    fn whitespace_path_uses_angle_target() {
        assert_eq!(link("l", "a b"), "[l](<a b>)");
    }
}
