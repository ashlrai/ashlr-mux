//! Port of `Sources/TextBoxMentionCandidate.swift`.

use crate::markdown;
use crate::suggestion::MentionSuggestion;

/// Swift: `struct TextBoxMentionCandidate: Sendable`.
#[derive(Debug, Clone)]
pub struct MentionCandidate {
    pub title: String,
    pub subtitle: String,
    pub target_path: String,
    pub system_image_name: String,
    pub search_key: String,
    pub priority: i64,
}

impl MentionCandidate {
    /// Swift: `func suggestion(trigger: Character) -> TextBoxMentionSuggestion`.
    pub fn suggestion(&self, trigger: char) -> MentionSuggestion {
        let display_title = if (trigger == '/' || trigger == '$')
            && (self.title.starts_with('/') || self.title.starts_with('$'))
        {
            // DIVERGENCE: Swift `title.dropFirst()` drops one grapheme
            // cluster; this drops one Unicode scalar. The dropped character
            // is always the single-scalar "/" or "$" prefix here.
            let mut title = String::with_capacity(self.title.len());
            title.push(trigger);
            title.extend(self.title.chars().skip(1));
            title
        } else {
            self.title.clone()
        };

        let insertion_text = if trigger == '$' {
            // The $ trigger intentionally inserts the bare skill reference
            // (e.g. "$skill-name") as a plain-text shorthand. The / and @
            // triggers insert a markdown link instead.
            display_title.clone()
        } else {
            markdown::link(&display_title, &self.target_path)
        };

        MentionSuggestion {
            id: format!("{trigger}:{}", self.target_path),
            title: display_title.clone(),
            subtitle: self.subtitle.clone(),
            insertion_text,
            system_image_name: self.system_image_name.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill_candidate(name: &str) -> MentionCandidate {
        MentionCandidate {
            title: format!("/{name}"),
            subtitle: format!("/tmp/skills/{name}/SKILL.md"),
            target_path: format!("/tmp/skills/{name}/SKILL.md"),
            system_image_name: "sparkle.magnifyingglass".into(),
            search_key: name.into(),
            priority: 0,
        }
    }

    // Oracle values from cmuxTests/TextBoxMentionCompletionTests.swift
    // `testTextBoxMentionSkillSuggestionsUseTypedDollarTrigger`: the $ trigger
    // swaps the display prefix and inserts the bare skill reference.
    #[test]
    fn dollar_trigger_inserts_bare_skill_reference() {
        let suggestion = skill_candidate("sample-dollar-skill").suggestion('$');
        assert_eq!(suggestion.title, "$sample-dollar-skill");
        assert_eq!(suggestion.system_image_name, "sparkle.magnifyingglass");
        assert_eq!(suggestion.insertion_text, "$sample-dollar-skill");
        assert_eq!(suggestion.id, "$:/tmp/skills/sample-dollar-skill/SKILL.md");
    }

    // Oracle: `testTextBoxMentionSkillSuggestionsUseTypedSlashTriggerForEmptyQuery`.
    #[test]
    fn slash_trigger_inserts_markdown_link() {
        let suggestion = skill_candidate("sample-slash-skill").suggestion('/');
        assert_eq!(suggestion.title, "/sample-slash-skill");
        assert!(suggestion
            .insertion_text
            .starts_with("[/sample-slash-skill]("));
    }

    // Oracle: `testTextBoxMentionFileSuggestionsUseCommandPaletteSearchIndex`
    // (file candidates keep their title and link-insert).
    #[test]
    fn file_trigger_keeps_title_and_links() {
        let candidate = MentionCandidate {
            title: "@Sources/TextBoxInput.swift".into(),
            subtitle: "~/proj/Sources/TextBoxInput.swift".into(),
            target_path: "/proj/Sources/TextBoxInput.swift".into(),
            system_image_name: "doc".into(),
            search_key: "sources/textboxinput.swift textboxinput.swift".into(),
            priority: 3,
        };
        let suggestion = candidate.suggestion('@');
        assert_eq!(suggestion.title, "@Sources/TextBoxInput.swift");
        assert_eq!(suggestion.system_image_name, "doc");
        assert!(suggestion
            .insertion_text
            .starts_with("[@Sources/TextBoxInput.swift]("));
    }

    #[test]
    fn dollar_trigger_rewrites_slash_titles() {
        // A "/name" title shown for a typed "$" trigger swaps the prefix.
        let suggestion = skill_candidate("iterate-pr").suggestion('$');
        assert_eq!(suggestion.title, "$iterate-pr");
        assert_eq!(suggestion.insertion_text, "$iterate-pr");
    }
}
