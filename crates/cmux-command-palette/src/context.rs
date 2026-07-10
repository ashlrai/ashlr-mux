//! Port of `Context/CommandPaletteContextSnapshot.swift` and
//! `Context/CommandPaletteContextKeys.swift`.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// A typed string key for [`CommandPaletteContextSnapshot`] lookups. Carries a
/// `raw_value` so the snapshot can store/read it; the well-known keys are
/// exposed as named constructors whose `raw_value` is byte-identical to the
/// string the snapshot persists.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CommandPaletteContextKeys {
    /// The underlying snapshot dictionary key.
    pub raw_value: String,
}

impl CommandPaletteContextKeys {
    /// Wraps a raw snapshot key string.
    pub fn new(raw_value: impl Into<String>) -> Self {
        Self {
            raw_value: raw_value.into(),
        }
    }

    /// Key for one terminal open-target's availability; `raw_value` is the
    /// target's raw identifier.
    pub fn terminal_open_target_available(raw_value: &str) -> Self {
        Self::new(format!("terminal.openTarget.{raw_value}.available"))
    }
}

/// Declares the well-known context keys as named constructors, each returning a
/// [`CommandPaletteContextKeys`] whose `raw_value` matches the Swift literal.
macro_rules! context_keys {
    ($($(#[$meta:meta])* $name:ident => $raw:literal),+ $(,)?) => {
        impl CommandPaletteContextKeys {
            $(
                $(#[$meta])*
                pub fn $name() -> Self {
                    Self::new($raw)
                }
            )+
        }
    };
}

context_keys! {
    /// Whether a workspace is selected.
    has_workspace => "workspace.hasSelection",
    /// Selected workspace display name.
    workspace_name => "workspace.name",
    /// Whether the workspace has a custom name.
    workspace_has_custom_name => "workspace.hasCustomName",
    /// Whether the workspace has a custom description.
    workspace_has_custom_description => "workspace.hasCustomDescription",
    /// Whether minimal mode is enabled for the workspace.
    workspace_minimal_mode_enabled => "workspace.minimalModeEnabled",
    /// Whether the workspace should offer pinning.
    workspace_should_pin => "workspace.shouldPin",
    /// Whether the workspace has pull requests.
    workspace_has_pull_requests => "workspace.hasPullRequests",
    /// Whether the workspace has splits.
    workspace_has_splits => "workspace.hasSplits",
    /// Whether the workspace uses the canvas layout mode.
    workspace_canvas_layout => "workspace.canvasLayout",
    /// Whether the workspace has sibling workspaces.
    workspace_has_peers => "workspace.hasPeers",
    /// Whether a workspace exists above the selection.
    workspace_has_above => "workspace.hasAbove",
    /// Whether a workspace exists below the selection.
    workspace_has_below => "workspace.hasBelow",
    /// Whether mark-read is available for the workspace.
    workspace_can_mark_read => "workspace.canMarkRead",
    /// Whether mark-unread is available for the workspace.
    workspace_can_mark_unread => "workspace.canMarkUnread",
    /// Whether the sidebar matches the terminal background.
    sidebar_match_terminal_background => "sidebar.matchTerminalBackground",
    /// Whether a panel has focus.
    has_focused_panel => "panel.hasFocus",
    /// Focused panel display name.
    panel_name => "panel.name",
    /// Whether the focused panel is a browser.
    panel_is_browser => "panel.isBrowser",
    /// Whether browser focus mode is active.
    panel_browser_focus_mode_active => "panel.browserFocusModeActive",
    /// Whether the browser omnibar is visible.
    panel_browser_omnibar_visible => "panel.browser.omnibarVisible",
    /// Whether the focused panel is markdown.
    panel_is_markdown => "panel.isMarkdown",
    /// Whether the focused panel is a terminal.
    panel_is_terminal => "panel.isTerminal",
    /// Whether the focused panel sits in a pane.
    panel_has_pane => "panel.hasPane",
    /// Whether the focused panel hosts a forkable agent.
    panel_has_forkable_agent => "panel.hasForkableAgent",
    /// Whether the focused panel has a custom name.
    panel_has_custom_name => "panel.hasCustomName",
    /// Whether the focused panel should offer pinning.
    panel_should_pin => "panel.shouldPin",
    /// Whether the focused panel has unread state.
    panel_has_unread => "panel.hasUnread",
    /// Whether the focused panel can move to a new workspace.
    panel_can_move_to_new_workspace => "panel.canMoveToNewWorkspace",
    /// Whether an app update is available.
    update_has_available => "update.hasAvailable",
    /// Whether the cmux CLI is installed in PATH.
    cli_installed_in_path => "cli.installedInPATH",
    /// Whether cmux is the default terminal.
    default_terminal_is_default => "defaultTerminal.isDefault",
    /// Whether the browser surface is disabled.
    browser_disabled => "browser.disabled",
    /// Whether the user is signed in.
    auth_signed_in => "auth.signedIn",
    /// Whether an auth operation is in flight.
    auth_working => "auth.working",
}

/// Immutable snapshot of the bool/string context values that gate which palette
/// commands are visible and enabled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandPaletteContextSnapshot {
    bool_values: HashMap<String, bool>,
    string_values: HashMap<String, String>,
}

impl CommandPaletteContextSnapshot {
    /// Creates an empty snapshot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a boolean context value.
    pub fn set_bool(&mut self, key: &CommandPaletteContextKeys, value: bool) {
        self.bool_values.insert(key.raw_value.clone(), value);
    }

    /// Sets a string context value; `None` or empty removes the key.
    pub fn set_string(&mut self, key: &CommandPaletteContextKeys, value: Option<&str>) {
        match value {
            Some(value) if !value.is_empty() => {
                self.string_values
                    .insert(key.raw_value.clone(), value.to_string());
            }
            _ => {
                self.string_values.remove(&key.raw_value);
            }
        }
    }

    /// Reads a boolean context value (false when absent).
    pub fn bool(&self, key: &CommandPaletteContextKeys) -> bool {
        self.bool_values
            .get(&key.raw_value)
            .copied()
            .unwrap_or(false)
    }

    /// Reads a string context value.
    pub fn string(&self, key: &CommandPaletteContextKeys) -> Option<&str> {
        self.string_values.get(&key.raw_value).map(String::as_str)
    }

    /// Order-insensitive fingerprint over all context values, used to detect
    /// when the command list must be rebuilt.
    ///
    /// DIVERGENCE: Swift finalizes a per-process `Hasher` whose seed is
    /// randomized per launch, so its values are only meaningful within one
    /// process and are never persisted or compared across runs. This port uses
    /// a stable sorted-key hash (`DefaultHasher` over the sorted keys) so the
    /// same values always fingerprint the same within a build; callers must
    /// treat the value as opaque and only compare equality — never assert exact
    /// numbers (the two implementations do not agree on the concrete integer).
    pub fn fingerprint(&self) -> i64 {
        Self::fingerprint_raw(&self.bool_values, &self.string_values)
    }

    /// Fingerprints raw bool/string context maps.
    pub fn fingerprint_raw(
        bool_values: &HashMap<String, bool>,
        string_values: &HashMap<String, String>,
    ) -> i64 {
        let mut hasher = DefaultHasher::new();
        let mut bool_keys: Vec<&String> = bool_values.keys().collect();
        bool_keys.sort();
        for key in bool_keys {
            key.hash(&mut hasher);
            bool_values
                .get(key)
                .copied()
                .unwrap_or(false)
                .hash(&mut hasher);
        }
        let mut string_keys: Vec<&String> = string_values.keys().collect();
        string_keys.sort();
        for key in string_keys {
            key.hash(&mut hasher);
            string_values
                .get(key)
                .map(String::as_str)
                .unwrap_or("")
                .hash(&mut hasher);
        }
        hasher.finish() as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_known_keys_match_swift_raw_values() {
        assert_eq!(
            CommandPaletteContextKeys::has_workspace().raw_value,
            "workspace.hasSelection"
        );
        assert_eq!(
            CommandPaletteContextKeys::panel_browser_omnibar_visible().raw_value,
            "panel.browser.omnibarVisible"
        );
        assert_eq!(
            CommandPaletteContextKeys::cli_installed_in_path().raw_value,
            "cli.installedInPATH"
        );
        assert_eq!(
            CommandPaletteContextKeys::terminal_open_target_available("vscode").raw_value,
            "terminal.openTarget.vscode.available"
        );
    }

    #[test]
    fn bool_defaults_to_false_and_reads_back() {
        let mut snapshot = CommandPaletteContextSnapshot::new();
        assert!(!snapshot.bool(&CommandPaletteContextKeys::has_workspace()));
        snapshot.set_bool(&CommandPaletteContextKeys::has_workspace(), true);
        assert!(snapshot.bool(&CommandPaletteContextKeys::has_workspace()));
    }

    #[test]
    fn empty_or_none_string_removes_the_key() {
        let mut snapshot = CommandPaletteContextSnapshot::new();
        snapshot.set_string(
            &CommandPaletteContextKeys::workspace_name(),
            Some("Phoenix"),
        );
        assert_eq!(
            snapshot.string(&CommandPaletteContextKeys::workspace_name()),
            Some("Phoenix")
        );
        snapshot.set_string(&CommandPaletteContextKeys::workspace_name(), Some(""));
        assert_eq!(
            snapshot.string(&CommandPaletteContextKeys::workspace_name()),
            None
        );
        snapshot.set_string(
            &CommandPaletteContextKeys::workspace_name(),
            Some("Phoenix"),
        );
        snapshot.set_string(&CommandPaletteContextKeys::workspace_name(), None);
        assert_eq!(
            snapshot.string(&CommandPaletteContextKeys::workspace_name()),
            None
        );
    }

    #[test]
    fn fingerprint_is_order_insensitive_and_value_sensitive() {
        let mut a = CommandPaletteContextSnapshot::new();
        a.set_bool(&CommandPaletteContextKeys::has_workspace(), true);
        a.set_string(
            &CommandPaletteContextKeys::workspace_name(),
            Some("Phoenix"),
        );

        let mut b = CommandPaletteContextSnapshot::new();
        // Same values inserted in the opposite order.
        b.set_string(
            &CommandPaletteContextKeys::workspace_name(),
            Some("Phoenix"),
        );
        b.set_bool(&CommandPaletteContextKeys::has_workspace(), true);

        // Equal content -> equal fingerprint (do not assert the exact number).
        assert_eq!(a.fingerprint(), b.fingerprint());

        // A changed value flips the fingerprint (probabilistically).
        b.set_bool(&CommandPaletteContextKeys::has_workspace(), false);
        assert_ne!(a.fingerprint(), b.fingerprint());
    }
}
