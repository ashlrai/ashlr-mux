//! Port of `Sources/TextBoxMentionKind.swift`.

/// Which corpus a mention query completes against.
///
/// Swift: `enum TextBoxMentionKind: Equatable, Sendable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MentionKind {
    File,
    Skill,
}

impl MentionKind {
    /// Swift: `var defaultTrigger: Character`.
    pub fn default_trigger(self) -> char {
        match self {
            MentionKind::File => '@',
            MentionKind::Skill => '/',
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_triggers_match_swift() {
        assert_eq!(MentionKind::File.default_trigger(), '@');
        assert_eq!(MentionKind::Skill.default_trigger(), '/');
    }
}
