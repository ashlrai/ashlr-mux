//! Parses a raw tmux window-layout string into a [`RemoteTmuxLayoutNode`] tree.
//!
//! Ported from `RemoteTmuxRawLayoutParser.swift`.
//!
//! The format (from `#{window_layout}` / `%layout-change`) is a 4-hex-char
//! checksum, a comma, then a recursive node:
//! ```text
//! f92f,120x40,0,0{60x40,0,0,4,59x40,61,0[59x20,61,0,5,59x19,61,21,8]}
//! ```
//! where each node is `WxH,X,Y` followed by one of:
//! - `,<paneId>` — a leaf pane,
//! - `{ … }` — a left-right (horizontal) split of comma-separated child nodes,
//! - `[ … ]` — a top-bottom (vertical) split of comma-separated child nodes.

use crate::models::{RemoteTmuxLayoutContent, RemoteTmuxLayoutNode};

/// Parses a window-layout string (with or without the leading checksum).
///
/// Returns the root layout node, or `None` if the string is malformed.
pub fn parse(raw: &str) -> Option<RemoteTmuxLayoutNode> {
    // Normalize first: the strict `cursor == chars.len()` completion check below
    // would otherwise reject an otherwise-valid layout that carries a trailing
    // newline/space.
    let mut chars: Vec<char> = trim_whitespace_and_newlines(raw).chars().collect();
    // Strip a leading 4-hex-char checksum followed by a comma, if present.
    if chars.len() > 5
        && chars[4] == ','
        // DIVERGENCE: Swift `Character.isHexDigit` also matches Unicode fullwidth
        // hex forms; tmux layout checksums are always ASCII hex, so we scope this
        // to `is_ascii_hexdigit` (the checksum could never be a fullwidth digit).
        && chars[0..4].iter().all(|c| c.is_ascii_hexdigit())
    {
        chars.drain(0..5);
    }
    let mut cursor = 0usize;
    let node = parse_node(&chars, &mut cursor)?;
    if cursor == chars.len() {
        Some(node)
    } else {
        None
    }
}

fn parse_node(chars: &[char], cursor: &mut usize) -> Option<RemoteTmuxLayoutNode> {
    let width = parse_int(chars, cursor)?;
    if !consume(chars, cursor, 'x') {
        return None;
    }
    let height = parse_int(chars, cursor)?;
    if !consume(chars, cursor, ',') {
        return None;
    }
    let x = parse_int(chars, cursor)?;
    if !consume(chars, cursor, ',') {
        return None;
    }
    let y = parse_int(chars, cursor)?;

    if *cursor >= chars.len() {
        return None;
    }
    let content = match chars[*cursor] {
        ',' => {
            *cursor += 1;
            let pane_id = parse_int(chars, cursor)?;
            RemoteTmuxLayoutContent::Pane(pane_id)
        }
        '{' => {
            let children = parse_children(chars, cursor, '{', '}')?;
            RemoteTmuxLayoutContent::Horizontal(children)
        }
        '[' => {
            let children = parse_children(chars, cursor, '[', ']')?;
            RemoteTmuxLayoutContent::Vertical(children)
        }
        _ => return None,
    };
    Some(RemoteTmuxLayoutNode::new(width, height, x, y, content))
}

fn parse_children(
    chars: &[char],
    cursor: &mut usize,
    open: char,
    close: char,
) -> Option<Vec<RemoteTmuxLayoutNode>> {
    if !consume(chars, cursor, open) {
        return None;
    }
    let mut children: Vec<RemoteTmuxLayoutNode> = Vec::new();
    loop {
        let child = parse_node(chars, cursor)?;
        children.push(child);
        if *cursor >= chars.len() {
            return None;
        }
        if chars[*cursor] == close {
            *cursor += 1;
            break;
        }
        if chars[*cursor] == ',' {
            *cursor += 1;
            continue;
        }
        return None;
    }
    // A split node always has at least two children; a one-child `{…}`/`[…]` is
    // malformed.
    if children.len() >= 2 {
        Some(children)
    } else {
        None
    }
}

fn parse_int(chars: &[char], cursor: &mut usize) -> Option<i64> {
    let start = *cursor;
    // DIVERGENCE: Swift `Character.isNumber` matches Unicode numerics broadly;
    // tmux emits only ASCII digits in layout strings, so we scope to
    // `is_ascii_digit` (an `Int(String(...))` of any non-ASCII digit would fail
    // to parse anyway).
    while *cursor < chars.len() && chars[*cursor].is_ascii_digit() {
        *cursor += 1;
    }
    if *cursor <= start {
        return None;
    }
    let s: String = chars[start..*cursor].iter().collect();
    s.parse::<i64>().ok()
}

fn consume(chars: &[char], cursor: &mut usize, expected: char) -> bool {
    if *cursor < chars.len() && chars[*cursor] == expected {
        *cursor += 1;
        true
    } else {
        false
    }
}

/// Trims leading/trailing characters in Foundation's
/// `CharacterSet.whitespacesAndNewlines` (space, tab, the newline family, and
/// Unicode separators). Mirrors Swift `trimmingCharacters(in: .whitespacesAndNewlines)`.
fn trim_whitespace_and_newlines(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_pane_with_checksum() {
        let node = parse("b2f1,80x24,0,0,1").unwrap();
        assert_eq!(
            node,
            RemoteTmuxLayoutNode::new(80, 24, 0, 0, RemoteTmuxLayoutContent::Pane(1))
        );
    }

    #[test]
    fn single_pane_without_checksum() {
        let node = parse("80x24,0,0,1").unwrap();
        assert_eq!(node.content, RemoteTmuxLayoutContent::Pane(1));
    }

    #[test]
    fn nested_horizontal_and_vertical_splits() {
        // From the doc-comment example.
        let raw = "f92f,120x40,0,0{60x40,0,0,4,59x40,61,0[59x20,61,0,5,59x19,61,21,8]}";
        let node = parse(raw).unwrap();
        assert_eq!(node.width, 120);
        assert_eq!(node.height, 40);
        match &node.content {
            RemoteTmuxLayoutContent::Horizontal(children) => {
                assert_eq!(children.len(), 2);
                assert_eq!(children[0].content, RemoteTmuxLayoutContent::Pane(4));
                match &children[1].content {
                    RemoteTmuxLayoutContent::Vertical(inner) => {
                        assert_eq!(inner.len(), 2);
                        assert_eq!(inner[0].content, RemoteTmuxLayoutContent::Pane(5));
                        assert_eq!(inner[1].content, RemoteTmuxLayoutContent::Pane(8));
                    }
                    other => panic!("expected vertical, got {other:?}"),
                }
            }
            other => panic!("expected horizontal, got {other:?}"),
        }
        assert_eq!(node.pane_ids_in_order(), vec![4, 5, 8]);
    }

    #[test]
    fn trailing_whitespace_is_tolerated() {
        assert!(parse("b2f1,80x24,0,0,1\n").is_some());
        assert!(parse("  b2f1,80x24,0,0,1  ").is_some());
    }

    #[test]
    fn split_with_single_child_is_rejected() {
        // `{…}` with only one child is malformed (a split needs >= 2 children).
        assert!(parse("80x24,0,0{80x24,0,0,1}").is_none());
    }

    #[test]
    fn trailing_garbage_is_rejected() {
        assert!(parse("80x24,0,0,1garbage").is_none());
    }

    #[test]
    fn missing_dimensions_is_rejected() {
        assert!(parse("80x,0,0,1").is_none());
        assert!(parse("x24,0,0,1").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn checksum_only_stripped_when_four_hex_then_comma() {
        // "zzzz,..." is not hex, so the leading token is NOT a checksum and the
        // whole thing must parse as a node (which it cannot) → None.
        assert!(parse("zzzz,80x24,0,0,1").is_none());
    }

    #[test]
    fn horizontal_split_three_panes() {
        let node = parse("120x40,0,0{40x40,0,0,1,40x40,41,0,2,38x40,82,0,3}").unwrap();
        assert_eq!(node.pane_ids_in_order(), vec![1, 2, 3]);
    }
}
