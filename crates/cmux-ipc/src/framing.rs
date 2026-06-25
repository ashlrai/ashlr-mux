pub fn split_lines(buffer: &str) -> Vec<&str> {
    buffer.lines().collect()
}

pub fn append_line(line: &str) -> String {
    let mut framed = String::with_capacity(line.len() + 1);
    framed.push_str(line);
    framed.push('\n');
    framed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_line_terminates_with_newline() {
        assert_eq!(append_line("{\"id\":1}"), "{\"id\":1}\n");
        assert_eq!(append_line(""), "\n");
    }

    #[test]
    fn framed_objects_round_trip() {
        let wire = format!("{}{}", append_line("a"), append_line("b"));
        assert_eq!(wire, "a\nb\n");
        assert_eq!(split_lines(&wire), vec!["a", "b"]);
    }

    #[test]
    fn empty_buffer_yields_no_lines() {
        assert!(split_lines("").is_empty());
    }

    #[test]
    fn blank_lines_are_preserved_as_empty_entries() {
        // A bare "\n\n" frames two empty lines between content.
        assert_eq!(split_lines("a\n\nb\n"), vec!["a", "", "b"]);
    }

    #[test]
    fn crlf_terminators_are_trimmed() {
        // `str::lines` strips the trailing '\r', so CRLF-framed input
        // yields the same logical lines as LF framing.
        assert_eq!(split_lines("a\r\nb\r\n"), vec!["a", "b"]);
    }

    #[test]
    fn unterminated_trailing_content_is_returned_as_a_line() {
        // Contract note: this helper is stateless and treats the final
        // segment as a complete line even without a trailing '\n'. A
        // streaming caller must buffer the remainder until the newline
        // arrives rather than rely on this helper to detect partial frames.
        assert_eq!(split_lines("a\nb"), vec!["a", "b"]);
    }
}
