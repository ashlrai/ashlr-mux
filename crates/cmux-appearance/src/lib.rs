//! cmux-appearance — appearance / theme resolution layer.
//!
//! Headless port of the resolution logic that sits between the already-ported
//! `cmux-config::Appearance` (the 3-case config-file enum) and `cmux-terminal`'s
//! terminal theme. Nothing here duplicates those types — it *resolves between*
//! them:
//!
//! - [`AppearanceMode`] is a DISTINCT 4-case enum (`System`/`Light`/`Dark`/`Auto`,
//!   including `Auto`) mirroring Swift `AppearanceMode`. It is neither
//!   `cmux-config::Appearance` (3 cases, config-file shape) nor
//!   `cmux-terminal::TerminalTheme` (the resolved terminal theme). This crate
//!   normalizes the persisted `appearanceMode` string and derives the effective
//!   [`ColorScheme`] / [`ColorSchemePreference`].
//! - [`ColorScheme`] mirrors SwiftUI's `ColorScheme`; [`ColorSchemePreference`]
//!   mirrors `GhosttyConfig.ColorSchemePreference` (the terminal-domain
//!   preference). They are intentionally kept as two distinct light/dark enums,
//!   matching the Swift separation.
//!
//! All logic is pure string/enum work. UserDefaults reads (the persisted
//! `appearanceMode` and `AppleInterfaceStyle`) are injected as `Option<&str>` /
//! `Option<String>`; the caller owns every read and write. AppKit/GPU/WebView
//! host wiring (window/sidebar tinting, live-apply, the defaults observer) is
//! intentionally NOT ported here.

mod color_scheme;
mod mode;
mod theme_selection;

pub use color_scheme::{ColorSchemePreference, SystemAppearance};
pub use mode::{
    color_scheme, color_scheme_override, normalize, resolved_mode, AppearanceMode, ColorScheme,
};
pub use theme_selection::{
    encode_theme_value, last_theme_directive, parse_theme_selection, resolve_theme_name,
    validate_theme_name, ThemeSelection, ThemeValidationError,
};
