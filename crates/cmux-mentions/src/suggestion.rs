//! Port of `Sources/TextBoxMentionSuggestion.swift`.

/// Swift: `struct TextBoxMentionSuggestion: Identifiable, Equatable, Sendable`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionSuggestion {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub insertion_text: String,
    pub system_image_name: String,
}
