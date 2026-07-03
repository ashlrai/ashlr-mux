//! Port of `Request/CommandPaletteRequestKind.swift` — the window-agnostic
//! policy for a command-palette open request.

/// The window-agnostic policy for a command-palette open request.
///
/// Each case names one way the palette can be requested (open the command list,
/// the workspace switcher, or one of the rename/edit prompts). The per-kind
/// policy that decides which notification to post and whether the request marks
/// a pending-open lives here so it stays pure and testable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandPaletteRequestKind {
    /// Opens the command list palette.
    Commands,
    /// Opens the workspace switcher palette.
    Switcher,
    /// Opens the rename-tab prompt.
    RenameTab,
    /// Opens the rename-workspace prompt.
    RenameWorkspace,
    /// Opens the edit-workspace-description prompt.
    EditWorkspaceDescription,
}

impl CommandPaletteRequestKind {
    /// Every case, in declaration order (Swift `CaseIterable.allCases`).
    pub const ALL: [CommandPaletteRequestKind; 5] = [
        CommandPaletteRequestKind::Commands,
        CommandPaletteRequestKind::Switcher,
        CommandPaletteRequestKind::RenameTab,
        CommandPaletteRequestKind::RenameWorkspace,
        CommandPaletteRequestKind::EditWorkspaceDescription,
    ];

    /// Swift `CaseIterable.allCases`.
    pub fn all_cases() -> [CommandPaletteRequestKind; 5] {
        Self::ALL
    }

    /// The raw notification name posted for this request.
    ///
    /// These strings are byte-identical to the `cmux.*` `Notification.Name`
    /// literals the macOS app posts, so observers keyed on the existing names
    /// are unaffected.
    pub fn notification_name(self) -> &'static str {
        match self {
            CommandPaletteRequestKind::Commands => "cmux.commandPaletteRequested",
            CommandPaletteRequestKind::Switcher => "cmux.commandPaletteSwitcherRequested",
            CommandPaletteRequestKind::RenameTab => "cmux.commandPaletteRenameTabRequested",
            CommandPaletteRequestKind::RenameWorkspace => {
                "cmux.commandPaletteRenameWorkspaceRequested"
            }
            CommandPaletteRequestKind::EditWorkspaceDescription => {
                "cmux.commandPaletteEditWorkspaceDescriptionRequested"
            }
        }
    }

    /// Whether posting this request should mark the target window pending-open.
    ///
    /// Every request kind marks pending-open today; the method keeps the policy
    /// with the kind so a future non-marking request kind is a local change
    /// rather than a call-site edit.
    pub fn marks_pending(self) -> bool {
        match self {
            CommandPaletteRequestKind::Commands
            | CommandPaletteRequestKind::Switcher
            | CommandPaletteRequestKind::RenameTab
            | CommandPaletteRequestKind::RenameWorkspace
            | CommandPaletteRequestKind::EditWorkspaceDescription => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // Swift: `notificationNamesMatchLegacyLiterals`.
    #[test]
    fn notification_names_match_legacy_literals() {
        assert_eq!(
            CommandPaletteRequestKind::Commands.notification_name(),
            "cmux.commandPaletteRequested"
        );
        assert_eq!(
            CommandPaletteRequestKind::Switcher.notification_name(),
            "cmux.commandPaletteSwitcherRequested"
        );
        assert_eq!(
            CommandPaletteRequestKind::RenameTab.notification_name(),
            "cmux.commandPaletteRenameTabRequested"
        );
        assert_eq!(
            CommandPaletteRequestKind::RenameWorkspace.notification_name(),
            "cmux.commandPaletteRenameWorkspaceRequested"
        );
        assert_eq!(
            CommandPaletteRequestKind::EditWorkspaceDescription.notification_name(),
            "cmux.commandPaletteEditWorkspaceDescriptionRequested"
        );
    }

    // Swift: `everyKindMarksPending`.
    #[test]
    fn every_kind_marks_pending() {
        for kind in CommandPaletteRequestKind::all_cases() {
            assert!(kind.marks_pending());
        }
    }

    // Swift: `notificationNamesAreDistinct`.
    #[test]
    fn notification_names_are_distinct() {
        let names: HashSet<&str> = CommandPaletteRequestKind::all_cases()
            .iter()
            .map(|kind| kind.notification_name())
            .collect();
        assert_eq!(names.len(), CommandPaletteRequestKind::all_cases().len());
    }
}
