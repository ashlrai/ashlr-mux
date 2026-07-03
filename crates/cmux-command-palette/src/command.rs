//! Port of `Values/CommandPaletteCommand.swift` (value fields only).

/// One runnable palette command: identity, display strings, and search
/// keywords.
///
/// DIVERGENCE (host-bound): the Swift value carries an `action: () -> Void`
/// closure run on activation, and a sibling `CommandPaletteSearchResult` embeds
/// that action. Both are omitted here — command execution is the windowing
/// host's responsibility. This port keeps only the pure, searchable value
/// fields plus [`CommandPaletteCommand::searchable_texts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPaletteCommand {
    /// Stable command identifier.
    pub id: String,
    /// Tie-break rank; lower sorts first at equal score.
    pub rank: i64,
    /// Display title.
    pub title: String,
    /// Display subtitle.
    pub subtitle: String,
    /// Optional keyboard-shortcut hint shown trailing the row.
    pub shortcut_hint: Option<String>,
    /// Optional kind label (for example a switcher row's surface kind).
    pub kind_label: Option<String>,
    /// Additional search keywords.
    pub keywords: Vec<String>,
    /// Whether activating the command dismisses the palette.
    pub dismiss_on_run: bool,
}

impl CommandPaletteCommand {
    /// Creates a command.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        rank: i64,
        title: String,
        subtitle: String,
        shortcut_hint: Option<String>,
        kind_label: Option<String>,
        keywords: Vec<String>,
        dismiss_on_run: bool,
    ) -> Self {
        Self {
            id,
            rank,
            title,
            subtitle,
            shortcut_hint,
            kind_label,
            keywords,
            dismiss_on_run,
        }
    }

    /// Texts the search corpus indexes for this command.
    ///
    /// Swift: `searchableTexts == [title, subtitle] + keywords`.
    pub fn searchable_texts(&self) -> Vec<String> {
        let mut texts = Vec::with_capacity(2 + self.keywords.len());
        texts.push(self.title.clone());
        texts.push(self.subtitle.clone());
        texts.extend(self.keywords.iter().cloned());
        texts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchable_texts_are_title_subtitle_then_keywords() {
        let command = CommandPaletteCommand::new(
            "command.rename".to_string(),
            0,
            "Rename Tab".to_string(),
            "Tab".to_string(),
            Some("⌘R".to_string()),
            None,
            vec!["rename".to_string(), "title".to_string()],
            true,
        );
        assert_eq!(
            command.searchable_texts(),
            vec![
                "Rename Tab".to_string(),
                "Tab".to_string(),
                "rename".to_string(),
                "title".to_string(),
            ]
        );
    }
}
