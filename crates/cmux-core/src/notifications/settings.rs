//! The macOS `UserDefaults`-backed notification gates (dock badge, unread pane
//! ring, pane flash, menu-bar visibility).
//!
//! These four toggles live only in `UserDefaults` on macOS — they are NOT part
//! of the `cmux.json` notifications block (already modeled in `cmux-config`), so
//! they are ported here as a pure [`NotificationGates`] value plus its resolver.
//!
//! The `id` / `userDefaultsKey` constants mirror
//! `Packages/macOS/CmuxSettings/.../Keys/NotificationsCatalogSection.swift` and
//! `cmux/Sources/TerminalNotificationStore.swift` 40-67 / 2138-2145. The
//! `defaults.object(forKey:) == nil ? default : defaults.bool(...)` pattern
//! becomes a pure resolver over `Option<bool>` stored values so the host owns
//! the `UserDefaults` / registry read (the Windows storage seam).

// --- dock badge -----------------------------------------------------------

/// Catalog id for the dock/taskbar badge gate (Swift `notifications.dockBadge`).
pub const DOCK_BADGE_ID: &str = "notifications.dockBadge";
/// `UserDefaults` key for the dock badge gate (Swift `notificationDockBadgeEnabled`).
pub const DOCK_BADGE_USER_DEFAULTS_KEY: &str = "notificationDockBadgeEnabled";
/// Default dock-badge-enabled value (Swift `defaultDockBadgeEnabled`).
pub const DOCK_BADGE_DEFAULT: bool = true;

// --- unread pane ring -----------------------------------------------------

/// Catalog id for the unread pane ring gate (Swift `notifications.unreadPaneRing`).
pub const UNREAD_PANE_RING_ID: &str = "notifications.unreadPaneRing";
/// `UserDefaults` key for the unread pane ring gate (Swift `notificationPaneRingEnabled`).
pub const UNREAD_PANE_RING_USER_DEFAULTS_KEY: &str = "notificationPaneRingEnabled";
/// Default unread-pane-ring-enabled value (Swift `NotificationPaneRingSettings.defaultEnabled`).
pub const UNREAD_PANE_RING_DEFAULT: bool = true;

// --- pane flash -----------------------------------------------------------

/// Catalog id for the pane flash gate (Swift `notifications.paneFlash`).
pub const PANE_FLASH_ID: &str = "notifications.paneFlash";
/// `UserDefaults` key for the pane flash gate (Swift `notificationPaneFlashEnabled`).
pub const PANE_FLASH_USER_DEFAULTS_KEY: &str = "notificationPaneFlashEnabled";
/// Default pane-flash-enabled value (Swift `NotificationPaneFlashSettings.defaultEnabled`).
pub const PANE_FLASH_DEFAULT: bool = true;

// --- menu-bar visibility --------------------------------------------------

/// Catalog id for the menu-bar visibility gate (Swift `notifications.showInMenuBar`).
pub const SHOW_IN_MENU_BAR_ID: &str = "notifications.showInMenuBar";
/// `UserDefaults` key for the menu-bar visibility gate (Swift `showMenuBarExtra`).
pub const SHOW_IN_MENU_BAR_USER_DEFAULTS_KEY: &str = "showMenuBarExtra";
/// Default menu-bar-visible value (Swift `NotificationsCatalogSection.showInMenuBar` default).
pub const SHOW_IN_MENU_BAR_DEFAULT: bool = true;

/// The resolved `UserDefaults`-backed notification gates.
///
/// [`Default`] returns every gate at its documented default (all `true`),
/// matching the Swift `default*Enabled` constants for an unset key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationGates {
    pub dock_badge_enabled: bool,
    pub unread_pane_ring_enabled: bool,
    pub pane_flash_enabled: bool,
    pub show_in_menu_bar: bool,
}

impl Default for NotificationGates {
    fn default() -> Self {
        Self {
            dock_badge_enabled: DOCK_BADGE_DEFAULT,
            unread_pane_ring_enabled: UNREAD_PANE_RING_DEFAULT,
            pane_flash_enabled: PANE_FLASH_DEFAULT,
            show_in_menu_bar: SHOW_IN_MENU_BAR_DEFAULT,
        }
    }
}

impl NotificationGates {
    /// Resolve the gates from stored `Option<bool>` values: `None` (key unset)
    /// falls back to the documented default, `Some(v)` uses the stored value.
    ///
    /// Mirrors the Swift `defaults.object(forKey:) == nil ? default :
    /// defaults.bool(forKey:)` idiom shared by `NotificationBadgeSettings`,
    /// `NotificationPaneRingSettings`, `NotificationPaneFlashSettings`, and
    /// `MenuBarExtraSettings`.
    pub fn resolve(
        dock_badge_enabled: Option<bool>,
        unread_pane_ring_enabled: Option<bool>,
        pane_flash_enabled: Option<bool>,
        show_in_menu_bar: Option<bool>,
    ) -> Self {
        Self {
            dock_badge_enabled: dock_badge_enabled.unwrap_or(DOCK_BADGE_DEFAULT),
            unread_pane_ring_enabled: unread_pane_ring_enabled.unwrap_or(UNREAD_PANE_RING_DEFAULT),
            pane_flash_enabled: pane_flash_enabled.unwrap_or(PANE_FLASH_DEFAULT),
            show_in_menu_bar: show_in_menu_bar.unwrap_or(SHOW_IN_MENU_BAR_DEFAULT),
        }
    }

    /// Compute the dock/taskbar badge label using the resolved `dock_badge_enabled`
    /// gate, wiring this settings model to the already-ported badge formatter.
    ///
    /// Mirrors Swift `refreshDockBadge()` (`TerminalNotificationStore.swift`
    /// 2138-2145), which feeds `NotificationBadgeSettings.isDockBadgeEnabled()`
    /// into `dockBadgeLabel(unreadCount:isEnabled:runTag:)`. The dock-tile write
    /// itself remains the GUI seam.
    pub fn dock_badge_label(&self, unread_count: usize, run_tag: Option<&str>) -> Option<String> {
        super::badge::dock_badge_label(unread_count, self.dock_badge_enabled, run_tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Oracle: the `*PreferenceDefaultsToEnabled` / `DefaultsToVisible` tests —
    /// every gate defaults to `true` when its key is unset.
    #[test]
    fn defaults_all_enabled() {
        let gates = NotificationGates::default();
        assert!(gates.dock_badge_enabled);
        assert!(gates.unread_pane_ring_enabled);
        assert!(gates.pane_flash_enabled);
        assert!(gates.show_in_menu_bar);
    }

    #[test]
    fn resolve_none_uses_defaults() {
        let gates = NotificationGates::resolve(None, None, None, None);
        assert_eq!(gates, NotificationGates::default());
    }

    /// Oracle: setting a key false/true reads back that stored value.
    #[test]
    fn resolve_uses_stored_values() {
        let gates = NotificationGates::resolve(Some(false), Some(false), Some(false), Some(false));
        assert!(!gates.dock_badge_enabled);
        assert!(!gates.unread_pane_ring_enabled);
        assert!(!gates.pane_flash_enabled);
        assert!(!gates.show_in_menu_bar);

        let gates = NotificationGates::resolve(Some(true), None, Some(false), None);
        assert!(gates.dock_badge_enabled);
        assert!(gates.unread_pane_ring_enabled); // default
        assert!(!gates.pane_flash_enabled);
        assert!(gates.show_in_menu_bar); // default
    }

    #[test]
    fn keys_mirror_swift_catalog() {
        assert_eq!(DOCK_BADGE_ID, "notifications.dockBadge");
        assert_eq!(DOCK_BADGE_USER_DEFAULTS_KEY, "notificationDockBadgeEnabled");
        assert_eq!(UNREAD_PANE_RING_ID, "notifications.unreadPaneRing");
        assert_eq!(
            UNREAD_PANE_RING_USER_DEFAULTS_KEY,
            "notificationPaneRingEnabled"
        );
        assert_eq!(PANE_FLASH_ID, "notifications.paneFlash");
        assert_eq!(PANE_FLASH_USER_DEFAULTS_KEY, "notificationPaneFlashEnabled");
        assert_eq!(SHOW_IN_MENU_BAR_ID, "notifications.showInMenuBar");
        assert_eq!(SHOW_IN_MENU_BAR_USER_DEFAULTS_KEY, "showMenuBarExtra");
    }

    /// The dock-badge gate feeds the existing badge formatter's `is_enabled`.
    #[test]
    fn dock_badge_label_respects_gate() {
        let enabled = NotificationGates::default();
        assert_eq!(enabled.dock_badge_label(3, None), Some("3".to_string()));

        let disabled = NotificationGates::resolve(Some(false), None, None, None);
        assert_eq!(disabled.dock_badge_label(3, None), None);
        // A run tag still shows even when the badge count is gated off.
        assert_eq!(
            disabled.dock_badge_label(3, Some("tag")),
            Some("tag".to_string())
        );
    }
}
