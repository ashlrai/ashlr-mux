//! Policy values and normalization helpers for the right sidebar width override.
//!
//! Direct port of
//! `Packages/macOS/CmuxSettings/Sources/CmuxSettings/Policies/RightSidebarWidthSettings.swift:1-98`.
//!
//! The whole Swift type is stateless `Double` clamp/round math plus stored
//! key-name constants — there is no I/O, so the entire file is the pure core.
//!
//! Parity notes:
//! - Swift's `Double.rounded()` is round-half-away-from-zero, which matches
//!   Rust's [`f64::round`]. (Swift `RightSidebarWidthSettings.swift:54`.)
//! - `configuredMaximumWidth(from:)` returns `nil` for non-finite or `<= 0`
//!   inputs; ported as [`Option::None`]. (Swift lines 39-44.)
//! - `clampedSettingsEditorMaximumWidth` maps non-finite input to the default
//!   configured width (1200), NOT to the min/max bounds. (Swift lines 50-55.)

/// Policy values and normalization helpers for the right sidebar width override.
///
/// Mirrors the Swift `RightSidebarWidthSettings` struct
/// (`RightSidebarWidthSettings.swift:4`). Stateless: all methods take `&self`
/// only to mirror the Swift instance-method surface.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RightSidebarWidthSettings;

impl RightSidebarWidthSettings {
    /// The `cmux.json` key under `sidebar` that stores the right sidebar
    /// maximum width. (Swift `RightSidebarWidthSettings.swift:9`.)
    pub const JSON_KEY: &'static str = "rightMaxWidth";

    /// The dotted settings path for the right sidebar maximum width override.
    /// (Swift `RightSidebarWidthSettings.swift:12`.)
    pub const SETTINGS_PATH: &'static str = "sidebar.rightMaxWidth";

    /// The `UserDefaults` key that stores the active right sidebar maximum
    /// width override. (Swift `RightSidebarWidthSettings.swift:15`.)
    pub const MAX_WIDTH_KEY: &'static str = "rightSidebarMaxWidth";

    /// The `UserDefaults` key that remembers the previous right sidebar maximum
    /// width override. (Swift `RightSidebarWidthSettings.swift:18`.)
    pub const REMEMBERED_MAX_WIDTH_KEY: &'static str = "rightSidebarRememberedMaxWidth";

    /// The stored sentinel value that means the built-in dynamic width cap is
    /// active. (Swift `RightSidebarWidthSettings.swift:21`.)
    pub const NO_OVERRIDE_VALUE: f64 = -1.0;

    /// The smallest allowed right sidebar width, in points.
    /// (Swift `RightSidebarWidthSettings.swift:24`.)
    pub const MINIMUM_WIDTH: f64 = 276.0;

    /// The built-in right sidebar maximum width, in points, used when no
    /// override is active. (Swift `RightSidebarWidthSettings.swift:27`.)
    pub const BUILT_IN_MAXIMUM_WIDTH: f64 = 1200.0;

    /// The width restored when enabling the override without a remembered
    /// value. (Swift `RightSidebarWidthSettings.swift:30`.)
    pub const DEFAULT_CONFIGURED_MAXIMUM_WIDTH: f64 = Self::BUILT_IN_MAXIMUM_WIDTH;

    /// The largest width accepted by settings editors and imported `cmux.json`
    /// values. (Swift `RightSidebarWidthSettings.swift:33`.)
    pub const SETTINGS_EDITOR_MAXIMUM_WIDTH: f64 = 4096.0;

    /// Creates a stateless right sidebar width policy value.
    /// (Swift `RightSidebarWidthSettings.swift:6`.)
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Returns the effective configured maximum width for a stored value.
    ///
    /// Returns a clamped width when the stored value enables the override, or
    /// `None`. (Swift `RightSidebarWidthSettings.swift:39-44`.)
    #[must_use]
    pub fn configured_maximum_width(&self, stored_value: f64) -> Option<f64> {
        if !(stored_value.is_finite() && stored_value > 0.0) {
            return None;
        }
        Some(self.clamped_settings_editor_maximum_width(stored_value))
    }

    /// Clamps a settings-editor width to the supported range.
    ///
    /// Returns a finite rounded width within the settings editor bounds; a
    /// non-finite input maps to the default configured width.
    /// (Swift `RightSidebarWidthSettings.swift:50-55`.)
    #[must_use]
    pub fn clamped_settings_editor_maximum_width(&self, value: f64) -> f64 {
        if !value.is_finite() {
            return Self::DEFAULT_CONFIGURED_MAXIMUM_WIDTH;
        }
        Self::SETTINGS_EDITOR_MAXIMUM_WIDTH.min(Self::MINIMUM_WIDTH.max(value.round()))
    }

    /// Returns the remembered width to restore for a persisted value.
    ///
    /// Returns a clamped remembered width, or the default configured width.
    /// (Swift `RightSidebarWidthSettings.swift:61-66`.)
    #[must_use]
    pub fn remembered_maximum_width(&self, stored_value: f64) -> f64 {
        match self.configured_maximum_width(stored_value) {
            Some(configured) => self.clamped_settings_editor_maximum_width(configured),
            None => Self::DEFAULT_CONFIGURED_MAXIMUM_WIDTH,
        }
    }

    /// Returns the width displayed in settings for the active and remembered
    /// values. (Swift `RightSidebarWidthSettings.swift:74-79`.)
    #[must_use]
    pub fn editor_maximum_width(
        &self,
        active_stored_value: f64,
        remembered_stored_value: f64,
    ) -> f64 {
        match self.configured_maximum_width(active_stored_value) {
            Some(configured) => self.clamped_settings_editor_maximum_width(configured),
            None => self.remembered_maximum_width(remembered_stored_value),
        }
    }

    /// Returns the stored value to write when enabling the override.
    /// (Swift `RightSidebarWidthSettings.swift:85-87`.)
    #[must_use]
    pub fn stored_maximum_width_when_enabling(&self, remembered_stored_value: f64) -> f64 {
        self.remembered_maximum_width(remembered_stored_value)
    }

    /// Returns the remembered value to preserve before disabling the override.
    /// (Swift `RightSidebarWidthSettings.swift:95-97`.)
    #[must_use]
    pub fn stored_remembered_maximum_width(
        &self,
        active_stored_value: f64,
        remembered_stored_value: f64,
    ) -> f64 {
        self.editor_maximum_width(active_stored_value, remembered_stored_value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: RightSidebarWidthSettings = RightSidebarWidthSettings::new();

    // --- Ported verbatim from RightSidebarWidthSettingsTests.swift ---

    /// Swift `RightSidebarWidthSettingsTests.swift:8-14`
    /// `disabledOverrideRestoresRememberedCustomMaximumWhenEnabledAgain`.
    #[test]
    fn disabled_override_restores_remembered_custom_maximum_when_enabled_again() {
        let restored = SETTINGS.stored_maximum_width_when_enabling(1_234.0);
        assert_eq!(restored, 1_234.0);
    }

    /// Swift `RightSidebarWidthSettingsTests.swift:16-22`
    /// `invalidRememberedMaximumFallsBackToDefaultWhenEnabled`.
    #[test]
    fn invalid_remembered_maximum_falls_back_to_default_when_enabled() {
        let restored = SETTINGS
            .stored_maximum_width_when_enabling(RightSidebarWidthSettings::NO_OVERRIDE_VALUE);
        assert_eq!(
            restored,
            RightSidebarWidthSettings::DEFAULT_CONFIGURED_MAXIMUM_WIDTH
        );
    }

    /// Swift `RightSidebarWidthSettingsTests.swift:24-31`
    /// `activeCustomMaximumWinsOverRememberedValueForEditor`.
    #[test]
    fn active_custom_maximum_wins_over_remembered_value_for_editor() {
        let editor_value = SETTINGS.editor_maximum_width(1_500.0, 900.0);
        assert_eq!(editor_value, 1_500.0);
    }

    /// Swift `RightSidebarWidthSettingsTests.swift:33-39`
    /// `configuredMaximumWidthIsClampedToEditorRange`.
    #[test]
    fn configured_maximum_width_is_clamped_to_editor_range() {
        let configured = SETTINGS
            .configured_maximum_width(10_000.0)
            .expect("finite positive input yields Some");
        assert_eq!(
            configured,
            RightSidebarWidthSettings::SETTINGS_EDITOR_MAXIMUM_WIDTH
        );
    }

    /// Swift `RightSidebarWidthSettingsTests.swift:41-47`
    /// `rememberedCustomMaximumIsClampedToEditorRange`.
    #[test]
    fn remembered_custom_maximum_is_clamped_to_editor_range() {
        let restored = SETTINGS.stored_maximum_width_when_enabling(10_000.0);
        assert_eq!(
            restored,
            RightSidebarWidthSettings::SETTINGS_EDITOR_MAXIMUM_WIDTH
        );
    }

    // --- Edge cases the lane notes call out (parity-risk inputs) ---

    /// `configuredMaximumWidth` returns `nil` for non-finite / `<= 0` inputs.
    /// (Swift lines 40-42.)
    #[test]
    fn configured_maximum_width_rejects_non_positive_and_non_finite() {
        assert_eq!(SETTINGS.configured_maximum_width(0.0), None);
        assert_eq!(SETTINGS.configured_maximum_width(-1.0), None);
        assert_eq!(
            SETTINGS.configured_maximum_width(RightSidebarWidthSettings::NO_OVERRIDE_VALUE),
            None
        );
        assert_eq!(SETTINGS.configured_maximum_width(f64::NAN), None);
        assert_eq!(SETTINGS.configured_maximum_width(f64::INFINITY), None);
        assert_eq!(SETTINGS.configured_maximum_width(f64::NEG_INFINITY), None);
    }

    /// A finite positive value below the minimum clamps UP to the minimum.
    /// (Swift line 54: `max(minimumWidth, value.rounded())`.)
    #[test]
    fn configured_maximum_width_clamps_up_to_minimum() {
        assert_eq!(
            SETTINGS.configured_maximum_width(10.0),
            Some(RightSidebarWidthSettings::MINIMUM_WIDTH)
        );
    }

    /// Non-finite input to the clamp maps to the default configured width, NOT
    /// to a min/max bound. (Swift lines 51-53.)
    #[test]
    fn clamped_settings_editor_maximum_width_maps_non_finite_to_default() {
        assert_eq!(
            SETTINGS.clamped_settings_editor_maximum_width(f64::NAN),
            RightSidebarWidthSettings::DEFAULT_CONFIGURED_MAXIMUM_WIDTH
        );
        assert_eq!(
            SETTINGS.clamped_settings_editor_maximum_width(f64::INFINITY),
            RightSidebarWidthSettings::DEFAULT_CONFIGURED_MAXIMUM_WIDTH
        );
    }

    /// `value.rounded()` is round-half-away-from-zero (matches Swift `.rounded()`).
    /// (Swift line 54.)
    #[test]
    fn clamped_settings_editor_maximum_width_rounds_half_away_from_zero() {
        assert_eq!(SETTINGS.clamped_settings_editor_maximum_width(300.5), 301.0);
        assert_eq!(SETTINGS.clamped_settings_editor_maximum_width(301.5), 302.0);
    }

    /// When the active value does not enable the override, the editor falls
    /// back to the remembered value. (Swift lines 75-78.)
    #[test]
    fn editor_maximum_width_falls_back_to_remembered_when_active_disabled() {
        let editor_value = SETTINGS.editor_maximum_width(
            RightSidebarWidthSettings::NO_OVERRIDE_VALUE,
            900.0,
        );
        assert_eq!(editor_value, 900.0);
    }

    /// `storedRememberedMaximumWidth` delegates to `editorMaximumWidth`.
    /// (Swift lines 95-97.)
    #[test]
    fn stored_remembered_maximum_width_matches_editor_maximum_width() {
        assert_eq!(
            SETTINGS.stored_remembered_maximum_width(1_500.0, 900.0),
            SETTINGS.editor_maximum_width(1_500.0, 900.0)
        );
        assert_eq!(
            SETTINGS.stored_remembered_maximum_width(
                RightSidebarWidthSettings::NO_OVERRIDE_VALUE,
                900.0
            ),
            900.0
        );
    }

    /// Named stored-key constants are pinned to the Swift string literals.
    /// (Swift lines 9-18.)
    #[test]
    fn stored_key_constants_match_swift() {
        assert_eq!(RightSidebarWidthSettings::JSON_KEY, "rightMaxWidth");
        assert_eq!(
            RightSidebarWidthSettings::SETTINGS_PATH,
            "sidebar.rightMaxWidth"
        );
        assert_eq!(
            RightSidebarWidthSettings::MAX_WIDTH_KEY,
            "rightSidebarMaxWidth"
        );
        assert_eq!(
            RightSidebarWidthSettings::REMEMBERED_MAX_WIDTH_KEY,
            "rightSidebarRememberedMaxWidth"
        );
    }
}
