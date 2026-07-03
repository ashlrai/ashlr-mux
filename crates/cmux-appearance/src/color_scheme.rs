//! System interface style + terminal color-scheme preference — port of
//! CmuxTerminalCore `TerminalSystemAppearance.swift` and
//! `TerminalColorSchemePreference.swift`.
//!
//! [`SystemAppearance`] is a pure snapshot of the macOS `AppleInterfaceStyle`
//! value (injected as `Option<String>`; no UserDefaults read here).
//! [`ColorSchemePreference`] is the two-case light/dark preference libghostty
//! theme resolution keys off of — DISTINCT from [`crate::AppearanceMode`] (which
//! also carries `System`/`Auto`) and from `cmux-config::Appearance`. cmux
//! resolves the app appearance mode down to this preference before loading
//! terminal colors.

use serde::{Deserialize, Serialize};

/// A snapshot of the macOS system interface style, read from the
/// `AppleInterfaceStyle` user-defaults key. Mirrors `TerminalSystemAppearance`.
///
/// The raw interface-style value (for example `"Dark"`) is injected; `None`
/// represents light mode / the key being unset. No UserDefaults access lives
/// here — the caller supplies the value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SystemAppearance {
    /// The raw `AppleInterfaceStyle` value, or `None` when unset (light mode).
    pub interface_style: Option<String>,
}

impl SystemAppearance {
    /// The system defaults key macOS sets to `"Dark"` while dark mode is active.
    pub const APPLE_INTERFACE_STYLE_KEY: &'static str = "AppleInterfaceStyle";

    /// The `AppleInterfaceStyle` value that indicates dark mode.
    pub const DARK_INTERFACE_STYLE_VALUE: &'static str = "Dark";

    /// Creates a snapshot from a raw interface-style value.
    pub fn new(interface_style: Option<impl Into<String>>) -> SystemAppearance {
        SystemAppearance {
            interface_style: interface_style.map(Into::into),
        }
    }

    /// Whether the system is currently in dark mode: a case-insensitive match of
    /// the interface style against `"Dark"`. Mirrors
    /// `TerminalSystemAppearance.prefersDark`.
    pub fn prefers_dark(&self) -> bool {
        self.interface_style
            .as_deref()
            .is_some_and(|style| style.eq_ignore_ascii_case(Self::DARK_INTERFACE_STYLE_VALUE))
    }
}

/// The light/dark preference that drives terminal theme selection. Mirrors
/// `TerminalColorSchemePreference` (a.k.a. `GhosttyConfig.ColorSchemePreference`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorSchemePreference {
    Light,
    Dark,
}

impl ColorSchemePreference {
    /// Resolves the terminal color-scheme preference the way cmux's appearance
    /// mode drives Ghostty split-theme selection. Mirrors
    /// `TerminalColorSchemePreference.resolve(appearanceModeRawValue:systemAppearance:)`.
    ///
    /// An explicit `"light"`/`"dark"` raw value short-circuits; any other value
    /// (`"system"`, `"auto"`, unset, or unrecognized) follows the system
    /// interface style. Note this matches the raw string literally — callers who
    /// pass an already-normalized mode see identical behavior because normalize
    /// only ever emits `system`/`light`/`dark`.
    pub fn resolve(appearance_mode_raw: Option<&str>, system: &SystemAppearance) -> ColorSchemePreference {
        match appearance_mode_raw {
            Some("light") => ColorSchemePreference::Light,
            Some("dark") => ColorSchemePreference::Dark,
            _ => {
                if system.prefers_dark() {
                    ColorSchemePreference::Dark
                } else {
                    ColorSchemePreference::Light
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(style: Option<&str>) -> SystemAppearance {
        SystemAppearance::new(style)
    }

    // Mirrors TerminalSystemAppearanceTests
    // `prefersDarkIsTrueOnlyForDarkInterfaceStyle`.
    #[test]
    fn prefers_dark_is_true_only_for_dark_interface_style() {
        assert!(system(Some("Dark")).prefers_dark());
        assert!(system(Some("dark")).prefers_dark());
        assert!(!system(None).prefers_dark());
        assert!(!system(Some("Light")).prefers_dark());
        assert!(!system(Some("")).prefers_dark());
    }

    // Mirrors TerminalColorSchemePreferenceResolutionTests `explicitLightModeWins`.
    #[test]
    fn explicit_light_mode_wins() {
        let dark_system = system(Some("Dark"));
        assert_eq!(
            ColorSchemePreference::resolve(Some("light"), &dark_system),
            ColorSchemePreference::Light
        );
    }

    // Mirrors `explicitDarkModeWins`.
    #[test]
    fn explicit_dark_mode_wins() {
        let light_system = system(None);
        assert_eq!(
            ColorSchemePreference::resolve(Some("dark"), &light_system),
            ColorSchemePreference::Dark
        );
    }

    // Mirrors `systemModeFollowsSystemAppearance`.
    #[test]
    fn system_mode_follows_system_appearance() {
        assert_eq!(
            ColorSchemePreference::resolve(Some("system"), &system(Some("Dark"))),
            ColorSchemePreference::Dark
        );
        assert_eq!(
            ColorSchemePreference::resolve(Some("system"), &system(None)),
            ColorSchemePreference::Light
        );
    }

    // Mirrors `unsetAndUnknownModesFollowSystemAppearance`.
    #[test]
    fn unset_and_unknown_modes_follow_system_appearance() {
        assert_eq!(
            ColorSchemePreference::resolve(None, &system(Some("Dark"))),
            ColorSchemePreference::Dark
        );
        assert_eq!(
            ColorSchemePreference::resolve(Some("totally-unknown"), &system(None)),
            ColorSchemePreference::Light
        );
    }
}
