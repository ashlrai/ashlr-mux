//! Port of `Sources/TextBoxMentionCompletionDetector.swift`.
//!
//! The Swift detector walks an `NSString` (UTF-16) backwards from the cursor
//! to find the mention token, so this port operates on UTF-16 code units and
//! reports `NSRange`-style UTF-16 offsets.

use crate::kind::MentionKind;
use crate::query::{MentionQuery, Utf16Range};

/// Swift: `TextBoxMentionCompletionDetector.query(in:selectedRange:)`.
///
/// `selected_location` / `selected_length` are UTF-16 offsets, mirroring the
/// Swift `NSRange` parameter. (Swift also guards
/// `selectedRange.location != NSNotFound`; there is no `NSNotFound` analog in
/// this API.)
pub fn mention_query_in(
    text: &str,
    selected_location: usize,
    selected_length: usize,
) -> Option<MentionQuery> {
    if selected_length != 0 {
        return None;
    }

    let units: Vec<u16> = text.encode_utf16().collect();
    // Swift: `min(max(0, selectedRange.location), nsText.length)`.
    let cursor = selected_location.min(units.len());
    if cursor == 0 {
        return None;
    }

    let mut token_start = cursor;
    while token_start > 0 {
        // Swift takes a 1-UTF-16-unit substring and asks
        // `rangeOfCharacter(from: .whitespacesAndNewlines)`. A lone surrogate
        // half never matches the whitespace set, so scanning continues
        // through it — mirrored here by treating surrogate units as
        // non-whitespace.
        if utf16_unit_is_whitespace(units[token_start - 1]) {
            break;
        }
        token_start -= 1;
    }

    if token_start >= cursor {
        return None;
    }
    let token_range = Utf16Range::new(token_start, cursor - token_start);
    // Swift converts the NSString slice to String, which replaces unpaired
    // surrogates with U+FFFD — `from_utf16_lossy` does the same.
    let token = String::from_utf16_lossy(&units[token_start..cursor]);
    // DIVERGENCE: Swift `token.first` is a grapheme cluster; this takes the
    // first Unicode scalar. Differs only when the trigger scalar is followed
    // by a combining mark (Swift would then see a non-trigger Character).
    let trigger = token.chars().next()?;

    let kind = match trigger {
        '@' => MentionKind::File,
        '/' => MentionKind::Skill,
        '$' => MentionKind::Skill,
        _ => return None,
    };

    // DIVERGENCE: Swift compares grapheme-cluster Characters against the
    // bracket set; this compares Unicode scalars (a bracket + combining mark
    // would pass in Swift but be rejected here).
    if !token
        .chars()
        .all(|character| !matches!(character, '[' | ']' | '(' | ')' | '<' | '>'))
    {
        return None;
    }

    // DIVERGENCE: Swift `token.dropFirst()` drops one grapheme cluster; this
    // drops one Unicode scalar (the trigger, which is always a single scalar
    // for the accepted triggers @ / $ /).
    let query: String = token.chars().skip(1).collect();
    Some(MentionQuery::new(kind, token_range, query, Some(trigger)))
}

fn utf16_unit_is_whitespace(unit: u16) -> bool {
    if (0xD800..=0xDFFF).contains(&unit) {
        return false;
    }
    // `char::is_whitespace` (Unicode White_Space) equals the union of
    // Foundation `.whitespaces` and `.newlines`.
    char::from_u32(u32::from(unit)).is_some_and(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_len(text: &str) -> usize {
        text.encode_utf16().count()
    }

    // Oracle: cmuxTests/TextBoxMentionCompletionTests.swift
    // `testTextBoxMentionCompletionDetectsFileAndSkillTokens`.
    #[test]
    fn detects_file_and_skill_tokens() {
        let file_prompt = "open @Sources/TextBox";
        let file_query = mention_query_in(file_prompt, utf16_len(file_prompt), 0).unwrap();
        assert_eq!(file_query.kind, MentionKind::File);
        assert_eq!(file_query.trigger, '@');
        assert_eq!(file_query.query, "Sources/TextBox");
        assert_eq!(file_query.range(), Utf16Range::new(5, 16));

        let skill_prompt = "use /swift-guidance before editing";
        let cursor = skill_prompt.find(" before").unwrap(); // ASCII: byte == UTF-16 offset
        let skill_query = mention_query_in(skill_prompt, cursor, 0).unwrap();
        assert_eq!(skill_query.kind, MentionKind::Skill);
        assert_eq!(skill_query.trigger, '/');
        assert_eq!(skill_query.query, "swift-guidance");
        assert_eq!(skill_query.range(), Utf16Range::new(4, 15));

        let dollar_skill_prompt = "use $axiom-swift now";
        let dollar_cursor = dollar_skill_prompt.find(" now").unwrap();
        let dollar_skill_query = mention_query_in(dollar_skill_prompt, dollar_cursor, 0).unwrap();
        assert_eq!(dollar_skill_query.kind, MentionKind::Skill);
        assert_eq!(dollar_skill_query.trigger, '$');
        assert_eq!(dollar_skill_query.query, "axiom-swift");
        assert_eq!(dollar_skill_query.range(), Utf16Range::new(4, 12));

        let bare_slash_prompt = "cd /";
        let bare_slash_query =
            mention_query_in(bare_slash_prompt, utf16_len(bare_slash_prompt), 0).unwrap();
        assert_eq!(bare_slash_query.kind, MentionKind::Skill);
        assert_eq!(bare_slash_query.trigger, '/');
        assert_eq!(bare_slash_query.query, "");

        let bare_dollar_prompt = "echo $";
        let bare_dollar_query =
            mention_query_in(bare_dollar_prompt, utf16_len(bare_dollar_prompt), 0).unwrap();
        assert_eq!(bare_dollar_query.kind, MentionKind::Skill);
        assert_eq!(bare_dollar_query.trigger, '$');
        assert_eq!(bare_dollar_query.query, "");

        let email_prompt = "mail lawrence@example.com";
        assert!(mention_query_in(email_prompt, utf16_len(email_prompt), 0).is_none());
    }

    #[test]
    fn rejects_nonzero_selection_length() {
        assert!(mention_query_in("@abc", 4, 1).is_none());
    }

    #[test]
    fn rejects_cursor_at_start_and_clamps_past_end() {
        assert!(mention_query_in("@abc", 0, 0).is_none());
        // Swift clamps the cursor to the NSString length.
        let clamped = mention_query_in("@abc", 99, 0).unwrap();
        assert_eq!(clamped.range(), Utf16Range::new(0, 4));
        assert_eq!(clamped.query, "abc");
    }

    #[test]
    fn rejects_tokens_containing_bracket_characters() {
        let prompt = "@a[b";
        assert!(mention_query_in(prompt, utf16_len(prompt), 0).is_none());
        let markdown = "[@a.txt](/tmp/a.txt)";
        assert!(mention_query_in(markdown, utf16_len(markdown), 0).is_none());
    }

    #[test]
    fn non_bmp_text_uses_utf16_offsets() {
        // "😀 @a" — the emoji is 2 UTF-16 units, so the token starts at 3.
        let prompt = "\u{1F600} @a";
        let query = mention_query_in(prompt, utf16_len(prompt), 0).unwrap();
        assert_eq!(query.range(), Utf16Range::new(3, 2));
        assert_eq!(query.query, "a");
    }
}
