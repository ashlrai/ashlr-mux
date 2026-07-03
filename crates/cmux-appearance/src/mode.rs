//! Appearance-mode normalization — port of `Sources/AppearanceSettings.swift`.
//!
//! `AppearanceMode` is a DISTINCT 4-case enum (`System`, `Light`, `Dark`,
//! `Auto`) and must NOT be confused with `cmux-config::Appearance` (the 3-case
//! config-file enum) nor with `cmux-terminal`'s terminal theme. This layer
//! *resolves* between them: it normalizes the persisted `appearanceMode`
//! user-defaults string (collapsing `Auto` -> `System`, unknown/None ->
//! `System`) and derives an explicit light/dark color-scheme override.
//!
//! All logic is pure; the persisted `appearanceMode` value is injected as
//! `Option<&str>` (the caller owns the UserDefaults read/write).

use serde::{Deserialize, Serialize};

/// The app-wide appearance mode persisted under the `appearanceMode`
/// user-defaults key. Mirrors Swift `AppearanceMode`.
///
/// Includes `Auto` for wire/back-compat: `Auto` is accepted on read but always
/// collapses to `System` during normalization (mirroring
/// `AppearanceMode.mode(for:)`), so it never survives into resolved state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppearanceMode {
    System,
    Light,
    Dark,
    Auto,
}

/// The concrete light/dark color scheme, mirroring SwiftUI's `ColorScheme`
/// (the type `colorSchemeOverride`/`colorScheme` return). This is DISTINCT from
/// [`crate::ColorSchemePreference`] (the terminal-domain preference), matching
/// Swift's separation of `SwiftUI.ColorScheme` from
/// `GhosttyConfig.ColorSchemePreference`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorScheme {
    Light,
    Dark,
}

impl AppearanceMode {
    /// The default mode when none is stored, mirroring
    /// `AppearanceSettings.defaultMode`.
    pub const DEFAULT: AppearanceMode = AppearanceMode::System;

    /// The persisted user-defaults key, mirroring
    /// `AppearanceSettings.appearanceModeKey`.
    pub const DEFAULTS_KEY: &'static str = "appearanceMode";

    /// The persisted `rawValue`, mirroring Swift's `String`-backed raw values.
    pub fn raw_value(self) -> &'static str {
        match self {
            AppearanceMode::System => "system",
            AppearanceMode::Light => "light",
            AppearanceMode::Dark => "dark",
            AppearanceMode::Auto => "auto",
        }
    }

    /// Exact-match parse of a persisted raw value, mirroring
    /// `AppearanceMode(rawValue:)`. Only the exact lowercase spellings match.
    pub fn from_raw(raw: &str) -> Option<AppearanceMode> {
        match raw {
            "system" => Some(AppearanceMode::System),
            "light" => Some(AppearanceMode::Light),
            "dark" => Some(AppearanceMode::Dark),
            "auto" => Some(AppearanceMode::Auto),
            _ => None,
        }
    }
}

/// Normalizes a persisted `appearanceMode` raw value into a resolved mode,
/// mirroring `AppearanceSettings.mode(for:)`.
///
/// Unknown values and `None` fall back to the `System` default; `Auto`
/// collapses to `System`. The result is therefore always one of
/// `System`/`Light`/`Dark`.
pub fn normalize(raw: Option<&str>) -> AppearanceMode {
    match raw.and_then(AppearanceMode::from_raw) {
        Some(AppearanceMode::Auto) => AppearanceMode::System,
        Some(mode) => mode,
        None => AppearanceMode::DEFAULT,
    }
}

/// Returns the explicit color-scheme override for a persisted raw value, or
/// `None` when the mode follows the system/ambient scheme. Mirrors
/// `AppearanceSettings.colorSchemeOverride(for:)`.
///
/// Only manual `Light`/`Dark` yield an override; `System`, `Auto`, unknown, and
/// `None` all yield `None` (they defer to the ambient scheme).
pub fn color_scheme_override(raw: Option<&str>) -> Option<ColorScheme> {
    match normalize(raw) {
        AppearanceMode::Light => Some(ColorScheme::Light),
        AppearanceMode::Dark => Some(ColorScheme::Dark),
        // `System` (and, via `normalize`, `Auto`/unknown/None) defer to ambient.
        AppearanceMode::System | AppearanceMode::Auto => None,
    }
}

/// Resolves the effective color scheme for a persisted raw value, falling back
/// to `fallback` when the mode has no explicit override. Mirrors
/// `AppearanceSettings.colorScheme(for:fallback:)`.
pub fn color_scheme(raw: Option<&str>, fallback: ColorScheme) -> ColorScheme {
    color_scheme_override(raw).unwrap_or(fallback)
}

/// Pure counterpart of `AppearanceSettings.resolvedMode(defaults:)`.
///
/// Given the currently-stored raw value, returns the normalized mode plus
/// whether the store needs rewriting so it matches the resolved `rawValue`
/// (i.e. the stored string was absent, unknown, or `auto`). The caller owns the
/// UserDefaults write — this fn performs no I/O, mirroring how Swift writes
/// `resolved.rawValue` back only when `stored != resolved.rawValue`.
pub fn resolved_mode(stored: Option<&str>) -> (AppearanceMode, bool) {
    let resolved = normalize(stored);
    let needs_rewrite = stored != Some(resolved.raw_value());
    (resolved, needs_rewrite)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mirrors AppearanceSettingsTests
    // `testColorSchemeOverrideIsExplicitOnlyForManualLightAndDarkModes`.
    #[test]
    fn color_scheme_override_is_explicit_only_for_manual_light_and_dark_modes() {
        assert_eq!(
            color_scheme_override(Some(AppearanceMode::Light.raw_value())),
            Some(ColorScheme::Light)
        );
        assert_eq!(
            color_scheme_override(Some(AppearanceMode::Dark.raw_value())),
            Some(ColorScheme::Dark)
        );
        assert_eq!(color_scheme_override(Some(AppearanceMode::System.raw_value())), None);
        assert_eq!(color_scheme_override(Some(AppearanceMode::Auto.raw_value())), None);
        assert_eq!(color_scheme_override(Some("invalid")), None);

        assert_eq!(
            color_scheme(Some(AppearanceMode::Dark.raw_value()), ColorScheme::Light),
            ColorScheme::Dark
        );
        assert_eq!(
            color_scheme(Some(AppearanceMode::System.raw_value()), ColorScheme::Dark),
            ColorScheme::Dark
        );
    }

    // `None` also defers to the ambient fallback (unset appearanceMode).
    #[test]
    fn color_scheme_override_defers_on_none() {
        assert_eq!(color_scheme_override(None), None);
        assert_eq!(color_scheme(None, ColorScheme::Light), ColorScheme::Light);
        assert_eq!(color_scheme(None, ColorScheme::Dark), ColorScheme::Dark);
    }

    // Mirrors `AppearanceMode.mode(for:)`: auto/unknown/None collapse to system.
    #[test]
    fn normalize_collapses_auto_and_unknown_to_system() {
        assert_eq!(normalize(Some("system")), AppearanceMode::System);
        assert_eq!(normalize(Some("light")), AppearanceMode::Light);
        assert_eq!(normalize(Some("dark")), AppearanceMode::Dark);
        assert_eq!(normalize(Some("auto")), AppearanceMode::System);
        assert_eq!(normalize(Some("totally-unknown")), AppearanceMode::System);
        assert_eq!(normalize(None), AppearanceMode::System);
    }

    // normalize() is idempotent: normalizing its own rawValue is a fixed point.
    #[test]
    fn normalize_is_idempotent() {
        for raw in ["system", "light", "dark", "auto", "unknown"] {
            let once = normalize(Some(raw));
            let twice = normalize(Some(once.raw_value()));
            assert_eq!(once, twice);
        }
        // The resolved rawValue is always a stable fixed point of normalize.
        for mode in [AppearanceMode::System, AppearanceMode::Light, AppearanceMode::Dark] {
            assert_eq!(normalize(Some(mode.raw_value())), mode);
        }
    }

    // Mirrors `resolvedMode`: rewrite exactly when stored != resolved.rawValue.
    #[test]
    fn resolved_mode_flags_rewrite_when_stored_differs() {
        // Unset -> default system, needs rewrite.
        assert_eq!(resolved_mode(None), (AppearanceMode::System, true));
        // Auto normalizes to system, so the stored "auto" needs rewriting.
        assert_eq!(resolved_mode(Some("auto")), (AppearanceMode::System, true));
        // Unknown -> system, needs rewrite.
        assert_eq!(resolved_mode(Some("bogus")), (AppearanceMode::System, true));
        // Already-canonical values need no rewrite.
        assert_eq!(resolved_mode(Some("system")), (AppearanceMode::System, false));
        assert_eq!(resolved_mode(Some("light")), (AppearanceMode::Light, false));
        assert_eq!(resolved_mode(Some("dark")), (AppearanceMode::Dark, false));
    }
}
