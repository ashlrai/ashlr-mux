//! Port of `Sources/TextBoxMentionQuery.swift`.

use crate::kind::MentionKind;

/// An `NSRange` analog: `location`/`length` counted in UTF-16 code units,
/// exactly as the Swift detector produces them from `NSString`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utf16Range {
    pub location: usize,
    pub length: usize,
}

impl Utf16Range {
    pub fn new(location: usize, length: usize) -> Self {
        Self { location, length }
    }
}

/// Swift: `struct TextBoxMentionQuery: Equatable, Sendable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionQuery {
    pub kind: MentionKind,
    /// UTF-16 location of the token (Swift `location: Int` from `NSRange`).
    pub location: usize,
    /// UTF-16 length of the token (Swift `length: Int` from `NSRange`).
    pub length: usize,
    /// The token text after the trigger character.
    pub query: String,
    pub trigger: char,
}

impl MentionQuery {
    /// Swift: `init(kind:range:query:trigger: Character? = nil)` — a nil
    /// trigger falls back to `kind.defaultTrigger`.
    pub fn new(kind: MentionKind, range: Utf16Range, query: String, trigger: Option<char>) -> Self {
        Self {
            kind,
            location: range.location,
            length: range.length,
            query,
            trigger: trigger.unwrap_or_else(|| kind.default_trigger()),
        }
    }

    /// Swift: `var range: NSRange`.
    pub fn range(&self) -> Utf16Range {
        Utf16Range::new(self.location, self.length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nil_trigger_uses_kind_default() {
        let file = MentionQuery::new(MentionKind::File, Utf16Range::new(0, 2), "a".into(), None);
        assert_eq!(file.trigger, '@');
        let skill = MentionQuery::new(MentionKind::Skill, Utf16Range::new(0, 2), "a".into(), None);
        assert_eq!(skill.trigger, '/');
    }

    #[test]
    fn explicit_trigger_is_kept() {
        let q = MentionQuery::new(
            MentionKind::Skill,
            Utf16Range::new(4, 12),
            "axiom-swift".into(),
            Some('$'),
        );
        assert_eq!(q.trigger, '$');
        assert_eq!(q.range(), Utf16Range::new(4, 12));
    }
}
