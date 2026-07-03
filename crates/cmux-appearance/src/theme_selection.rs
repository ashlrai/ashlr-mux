//! Terminal theme-NAME selection codec — port of the theme-string logic in
//! CmuxTerminalCore `GhosttyConfig.swift` plus `CLI/CMUXCLI+Themes.swift` and
//! `CLI/CMUXCLI+ThemeSupport.swift`.
//!
//! This is the string codec that maps a raw ghostty `theme` directive value —
//! which may carry conditional `light:.../dark:...` tokens — to a concrete
//! theme name for a resolved [`ColorSchemePreference`]. Pure string logic; no
//! disk or UserDefaults access. It sits above `cmux-config::Appearance` and
//! feeds `cmux-terminal`'s terminal theme, but owns neither.

use crate::color_scheme::ColorSchemePreference;

/// The light/dark theme names parsed from a raw `theme` value. Mirrors the
/// `light`/`dark` fields of CLI `CMUXCLI.ThemeSelection` (host-only fields
/// `rawValue`/`sourcePath` are excluded — they carry file-wiring state).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThemeSelection {
    /// The theme name for light mode, defaulting to the bare-token fallback.
    pub light: Option<String>,
    /// The theme name for dark mode, defaulting to the bare-token fallback.
    pub dark: Option<String>,
}

/// Error returned by [`validate_theme_name`], mirroring the two `CLIError`
/// messages Swift's `validatedThemeName` throws.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeValidationError {
    /// The trimmed theme name was empty.
    Empty,
    /// The theme name did not match any available theme (with a non-empty
    /// available list). Carries the trimmed name for the message.
    Unknown(String),
}

impl std::fmt::Display for ThemeValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeValidationError::Empty => write!(f, "Theme name cannot be empty"),
            ThemeValidationError::Unknown(name) => write!(
                f,
                "Unknown theme '{name}'. Run 'cmux themes' to list available themes."
            ),
        }
    }
}

impl std::error::Error for ThemeValidationError {}

/// Faithful port of Swift `Collection.split(separator:maxSplits:1,
/// omittingEmptySubsequences:true)` for a single-`char` separator.
///
/// This is NOT the same as Rust's `str::splitn(2, sep)`: Swift omits empty
/// subsequences AND only counts a *non-empty* emitted piece against `maxSplits`.
/// Consequences that matter to the theme codec:
/// - `"light:"` -> `["light"]` (len 1, a bare token) — the trailing empty is
///   omitted and never forms a second piece.
/// - `":dark"` -> `["dark"]` (len 1) — the leading empty is skipped and does
///   not consume the single split.
/// - `"a:b:c"` -> `["a", "b:c"]` (len 2).
fn split_max1_omitting_empty(s: &str, sep: char) -> Vec<&str> {
    const MAX_SPLITS: usize = 1;
    let mut result: Vec<&str> = Vec::new();
    let mut sub_start = 0usize;

    for (byte_idx, ch) in s.char_indices() {
        if ch != sep {
            continue;
        }
        // appendSubsequence(end: byte_idx): skip when empty (omitting).
        let did_append = if sub_start != byte_idx {
            result.push(&s[sub_start..byte_idx]);
            true
        } else {
            false
        };
        sub_start = byte_idx + ch.len_utf8();
        if did_append && result.len() == MAX_SPLITS {
            break;
        }
    }

    // Trailing subsequence: appended unless empty (omitting).
    if sub_start != s.len() {
        result.push(&s[sub_start..]);
    }
    result
}

/// Iterates the comma-separated tokens of a raw theme value the way Swift
/// `String.split(separator: ",")` does (unbounded splits, empty subsequences
/// omitted).
fn comma_tokens(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(',').filter(|token| !token.is_empty())
}

/// Splits a comma/`key:value` theme string into its `light`/`dark`/fallback
/// buckets. Shared scan behind [`parse_theme_selection`] and
/// [`resolve_theme_name`], mirroring the identical loop in Swift's
/// `parseThemeSelection` and `GhosttyConfig.resolveThemeName`.
///
/// `first-wins` per key; bare tokens (no `key:value`) become the fallback;
/// entries with an empty value are skipped.
fn scan_theme_buckets(raw: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut fallback: Option<String> = None;
    let mut light: Option<String> = None;
    let mut dark: Option<String> = None;

    for token in comma_tokens(raw) {
        let entry = token.trim();
        if entry.is_empty() {
            continue;
        }

        let parts = split_max1_omitting_empty(entry, ':');
        if parts.len() != 2 {
            if fallback.is_none() {
                fallback = Some(entry.to_string());
            }
            continue;
        }

        let key = parts[0].trim().to_lowercase();
        let value = parts[1].trim();
        if value.is_empty() {
            continue;
        }

        match key.as_str() {
            "light" => {
                if light.is_none() {
                    light = Some(value.to_string());
                }
            }
            "dark" => {
                if dark.is_none() {
                    dark = Some(value.to_string());
                }
            }
            _ => {
                if fallback.is_none() {
                    fallback = Some(value.to_string());
                }
            }
        }
    }

    (fallback, light, dark)
}

/// Parses a raw `theme` value into a [`ThemeSelection`]. Mirrors CLI
/// `parseThemeSelection(rawValue:sourcePath:)`.
///
/// The raw value is trimmed; `None`/empty yields both sides `None`. Each side
/// defaults to the bare-token fallback when it has no explicit `light:`/`dark:`
/// token.
pub fn parse_theme_selection(raw: Option<&str>) -> ThemeSelection {
    let trimmed = raw.map(str::trim).filter(|value| !value.is_empty());
    let Some(raw) = trimmed else {
        return ThemeSelection { light: None, dark: None };
    };

    let (fallback, light, dark) = scan_theme_buckets(raw);
    let resolved_dark = dark.or_else(|| fallback.clone());
    let resolved_light = light.or(fallback);
    ThemeSelection {
        light: resolved_light,
        dark: resolved_dark,
    }
}

/// Encodes explicit light/dark theme names into a raw `theme` directive value.
/// Mirrors CLI `encodedThemeValue(light:dark:)`.
///
/// Values are trimmed; empty (or `None`) sides are dropped. Produces
/// `light:X,dark:Y` / `light:X` / `dark:Y` / `None`.
pub fn encode_theme_value(light: Option<&str>, dark: Option<&str>) -> Option<String> {
    let normalized_light = light.map(str::trim).filter(|value| !value.is_empty());
    let normalized_dark = dark.map(str::trim).filter(|value| !value.is_empty());

    match (normalized_light, normalized_dark) {
        (Some(light), Some(dark)) => Some(format!("light:{light},dark:{dark}")),
        (Some(light), None) => Some(format!("light:{light}")),
        (None, Some(dark)) => Some(format!("dark:{dark}")),
        (None, None) => None,
    }
}

/// Resolves a raw `theme` value to the concrete theme name for
/// `preferred_color_scheme`. Mirrors `GhosttyConfig.resolveThemeName(from:preferredColorScheme:)`.
///
/// The requested side wins; otherwise it falls back to the bare token, then the
/// opposite side, and finally the trimmed raw value.
pub fn resolve_theme_name(raw: &str, preferred_color_scheme: ColorSchemePreference) -> String {
    let (fallback, light, dark) = scan_theme_buckets(raw);

    let (preferred, other) = match preferred_color_scheme {
        ColorSchemePreference::Light => (light, dark),
        ColorSchemePreference::Dark => (dark, light),
    };

    if let Some(preferred) = preferred {
        return preferred;
    }
    if let Some(fallback) = fallback {
        return fallback;
    }
    if let Some(other) = other {
        return other;
    }
    raw.trim().to_string()
}

/// Returns the value of the LAST non-empty `theme = ...` directive in the given
/// config contents, or `None`. Mirrors `GhosttyConfig.lastThemeDirective(in:)`.
///
/// Blank lines and `#` comments are skipped; each line is split on the first
/// `=`; the key must be exactly `theme`; the value is trimmed and has any
/// surrounding double-quotes stripped.
pub fn last_theme_directive(contents: &str) -> Option<String> {
    let mut last_value: Option<String> = None;

    for line in contents.split(is_newline) {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let parts = split_max1_omitting_empty(trimmed, '=');
        if parts.len() != 2 {
            continue;
        }
        if parts[0].trim() != "theme" {
            continue;
        }

        let value = parts[1].trim().trim_matches('"');
        if !value.is_empty() {
            last_value = Some(value.to_string());
        }
    }

    last_value
}

/// The characters in Swift `CharacterSet.newlines`, so `components(separatedBy:)`
/// line-splitting matches (`\r\n` yields an empty middle piece, which the
/// blank-line guard drops).
fn is_newline(ch: char) -> bool {
    matches!(
        ch,
        '\u{000A}' | '\u{000B}' | '\u{000C}' | '\u{000D}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
    )
}

/// Validates a raw theme name against the available themes. Mirrors CLI
/// `validatedThemeName(_:availableThemes:)`.
///
/// Returns the canonically-cased match when found (case-insensitive); passes the
/// trimmed name through when the available list is empty; otherwise rejects.
/// An empty (post-trim) name is always rejected.
pub fn validate_theme_name(
    raw: &str,
    available: &[String],
) -> Result<String, ThemeValidationError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ThemeValidationError::Empty);
    }
    if let Some(matched) = available
        .iter()
        .find(|candidate| candidate.eq_ignore_ascii_case(trimmed))
    {
        return Ok(matched.clone());
    }
    if available.is_empty() {
        return Ok(trimmed.to_string());
    }
    Err(ThemeValidationError::Unknown(trimmed.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color_scheme::{ColorSchemePreference, SystemAppearance};

    const CATPPUCCIN: &str = "light:Catppuccin Latte,dark:Apple System Colors";
    const MONOKAI: &str = "light:Monokai Pro Light,dark:Monokai Pro Machine";

    fn light_named(name: &str) -> Option<String> {
        Some(name.to_string())
    }

    // Mirrors GhosttySplitThemePreferenceTests / AppearanceSettingsTests split
    // resolution over "light:Catppuccin Latte,dark:Apple System Colors".
    #[test]
    fn resolves_catppuccin_split_theme_per_scheme() {
        assert_eq!(
            resolve_theme_name(CATPPUCCIN, ColorSchemePreference::Light),
            "Catppuccin Latte"
        );
        assert_eq!(
            resolve_theme_name(CATPPUCCIN, ColorSchemePreference::Dark),
            "Apple System Colors"
        );
    }

    // Mirrors the split resolution over the Monokai Pro pair.
    #[test]
    fn resolves_monokai_split_theme_per_scheme() {
        assert_eq!(
            resolve_theme_name(MONOKAI, ColorSchemePreference::Light),
            "Monokai Pro Light"
        );
        assert_eq!(
            resolve_theme_name(MONOKAI, ColorSchemePreference::Dark),
            "Monokai Pro Machine"
        );
    }

    // Full flow mirroring GhosttySplitThemePreferenceTests: appearance mode +
    // system appearance resolve a preference which then drives theme selection.
    #[test]
    fn stored_and_system_modes_drive_split_theme_selection() {
        // stored dark, stale-light system -> dark theme
        let pref = ColorSchemePreference::resolve(Some("dark"), &SystemAppearance { interface_style: None });
        assert_eq!(pref, ColorSchemePreference::Dark);
        assert_eq!(resolve_theme_name(CATPPUCCIN, pref), "Apple System Colors");

        // stored light, stale-dark system -> light theme
        let pref = ColorSchemePreference::resolve(
            Some("light"),
            &SystemAppearance { interface_style: Some("Dark".into()) },
        );
        assert_eq!(pref, ColorSchemePreference::Light);
        assert_eq!(resolve_theme_name(CATPPUCCIN, pref), "Catppuccin Latte");

        // system mode, light system -> light theme
        let pref = ColorSchemePreference::resolve(Some("system"), &SystemAppearance { interface_style: None });
        assert_eq!(pref, ColorSchemePreference::Light);
        assert_eq!(resolve_theme_name(MONOKAI, pref), "Monokai Pro Light");

        // system mode, dark system -> dark theme
        let pref = ColorSchemePreference::resolve(
            Some("system"),
            &SystemAppearance { interface_style: Some("Dark".into()) },
        );
        assert_eq!(pref, ColorSchemePreference::Dark);
        assert_eq!(resolve_theme_name(MONOKAI, pref), "Monokai Pro Machine");
    }

    // A bare token (no key:value) becomes the fallback for BOTH sides.
    #[test]
    fn bare_token_is_shared_fallback() {
        assert_eq!(
            resolve_theme_name("Solarized Dark", ColorSchemePreference::Light),
            "Solarized Dark"
        );
        assert_eq!(
            resolve_theme_name("Solarized Dark", ColorSchemePreference::Dark),
            "Solarized Dark"
        );

        let selection = parse_theme_selection(Some("Solarized Dark"));
        assert_eq!(selection.light, light_named("Solarized Dark"));
        assert_eq!(selection.dark, light_named("Solarized Dark"));
    }

    // A conditional side falls back to the bare token when its side is absent.
    #[test]
    fn missing_side_falls_back_to_bare_token() {
        let raw = "Zenburn,dark:Nord";
        assert_eq!(resolve_theme_name(raw, ColorSchemePreference::Light), "Zenburn");
        assert_eq!(resolve_theme_name(raw, ColorSchemePreference::Dark), "Nord");

        let selection = parse_theme_selection(Some(raw));
        assert_eq!(selection.light, light_named("Zenburn"));
        assert_eq!(selection.dark, light_named("Nord"));
    }

    // First value wins per key; later duplicates are ignored.
    #[test]
    fn first_wins_per_key() {
        let raw = "light:First,light:Second,dark:D1,dark:D2";
        assert_eq!(resolve_theme_name(raw, ColorSchemePreference::Light), "First");
        assert_eq!(resolve_theme_name(raw, ColorSchemePreference::Dark), "D1");

        let selection = parse_theme_selection(Some(raw));
        assert_eq!(selection.light, light_named("First"));
        assert_eq!(selection.dark, light_named("D1"));
    }

    // Because each comma-token is trimmed BEFORE the colon split, a token like
    // "light: " collapses to "light:" and (per the omitting-empty split) parses
    // as a single bare fallback token — NOT a light key with an empty value.
    // (The value-empty guard in the port is faithful but unreachable for
    // single-colon entries.)
    #[test]
    fn trailing_whitespace_value_collapses_to_bare_token() {
        let selection = parse_theme_selection(Some("light: ,dark:Only"));
        // The bare "light:" token seeds the fallback, so the light side inherits
        // it; the explicit dark token wins for dark.
        assert_eq!(selection.light, light_named("light:"));
        assert_eq!(selection.dark, light_named("Only"));
    }

    // Faithful Swift-split quirk: "light:" has its trailing empty omitted, so it
    // parses as a single bare token (NOT a light key with empty value).
    #[test]
    fn trailing_colon_is_a_bare_token() {
        // "light:" -> ["light"] (len 1) -> bare fallback token "light:".
        assert_eq!(resolve_theme_name("light:", ColorSchemePreference::Light), "light:");
        // ":dark" -> ["dark"] (len 1) -> bare fallback token ":dark".
        assert_eq!(resolve_theme_name(":dark", ColorSchemePreference::Dark), ":dark");
    }

    // Whitespace around keys/values and between tokens is trimmed.
    #[test]
    fn whitespace_around_tokens_is_trimmed() {
        let raw = "  light : Rose Pine Dawn , dark : Rose Pine  ";
        assert_eq!(resolve_theme_name(raw, ColorSchemePreference::Light), "Rose Pine Dawn");
        assert_eq!(resolve_theme_name(raw, ColorSchemePreference::Dark), "Rose Pine");
    }

    // None / empty raw -> both sides None.
    #[test]
    fn parse_empty_yields_no_sides() {
        assert_eq!(parse_theme_selection(None), ThemeSelection { light: None, dark: None });
        assert_eq!(parse_theme_selection(Some("   ")), ThemeSelection { light: None, dark: None });
    }

    // Mirrors CLI encodedThemeValue's four cases.
    #[test]
    fn encode_theme_value_covers_all_cases() {
        assert_eq!(
            encode_theme_value(Some("Latte"), Some("Frappe")),
            Some("light:Latte,dark:Frappe".to_string())
        );
        assert_eq!(encode_theme_value(Some("Latte"), None), Some("light:Latte".to_string()));
        assert_eq!(encode_theme_value(None, Some("Frappe")), Some("dark:Frappe".to_string()));
        assert_eq!(encode_theme_value(None, None), None);
        // Empty / whitespace-only sides are dropped.
        assert_eq!(encode_theme_value(Some("  "), Some("Frappe")), Some("dark:Frappe".to_string()));
        assert_eq!(encode_theme_value(Some(""), Some("")), None);
    }

    // encode(parse(s)) is stable for canonical `light:X,dark:Y` forms.
    #[test]
    fn encode_parse_round_trip_is_stable_for_canonical_forms() {
        for raw in [CATPPUCCIN, MONOKAI, "light:Only Light", "dark:Only Dark"] {
            let selection = parse_theme_selection(Some(raw));
            let encoded = encode_theme_value(selection.light.as_deref(), selection.dark.as_deref());
            assert_eq!(encoded.as_deref(), Some(raw), "round-trip mismatch for {raw:?}");
        }
    }

    // last_theme_directive: last non-empty wins, comments/blanks skipped, quotes
    // and whitespace stripped, CRLF handled.
    #[test]
    fn last_theme_directive_scans_config_lines() {
        let contents = "\
# a comment
theme = First

  theme =   \"Second Theme\"  \r
font-size = 12
theme = Last One
";
        assert_eq!(last_theme_directive(contents), Some("Last One".to_string()));
    }

    #[test]
    fn last_theme_directive_strips_surrounding_quotes_and_ignores_non_theme_keys() {
        assert_eq!(last_theme_directive("theme = \"Quoted\""), Some("Quoted".to_string()));
        assert_eq!(last_theme_directive("theme=Bare"), Some("Bare".to_string()));
        // Non-theme keys and comments never set a value.
        assert_eq!(last_theme_directive("# theme = Commented\nfont = x"), None);
        // A `theme =` with an empty value is ignored.
        assert_eq!(last_theme_directive("theme = "), None);
        // `themex` is not `theme`.
        assert_eq!(last_theme_directive("themex = Nope"), None);
    }

    // last_theme_directive keeps an earlier value when a later one is empty.
    #[test]
    fn last_theme_directive_keeps_last_non_empty() {
        assert_eq!(
            last_theme_directive("theme = Keep\ntheme = \"\""),
            Some("Keep".to_string())
        );
    }

    // Mirrors CLI validatedThemeName.
    #[test]
    fn validate_theme_name_matches_case_insensitively() {
        let available = vec!["Catppuccin Latte".to_string(), "Nord".to_string()];
        assert_eq!(
            validate_theme_name("catppuccin latte", &available),
            Ok("Catppuccin Latte".to_string())
        );
        assert_eq!(validate_theme_name("  NORD  ", &available), Ok("Nord".to_string()));
    }

    #[test]
    fn validate_theme_name_passes_through_when_available_is_empty() {
        assert_eq!(validate_theme_name("Anything", &[]), Ok("Anything".to_string()));
        assert_eq!(validate_theme_name("  Trimmed Me  ", &[]), Ok("Trimmed Me".to_string()));
    }

    #[test]
    fn validate_theme_name_rejects_empty_and_unknown() {
        let available = vec!["Nord".to_string()];
        assert_eq!(validate_theme_name("   ", &available), Err(ThemeValidationError::Empty));
        assert_eq!(
            validate_theme_name("Bogus", &available),
            Err(ThemeValidationError::Unknown("Bogus".to_string()))
        );
        // Error messages mirror the Swift CLIError text.
        assert_eq!(ThemeValidationError::Empty.to_string(), "Theme name cannot be empty");
        assert_eq!(
            ThemeValidationError::Unknown("Bogus".to_string()).to_string(),
            "Unknown theme 'Bogus'. Run 'cmux themes' to list available themes."
        );
    }

    // Direct exercise of the faithful maxSplits=1 omitting-empty splitter.
    #[test]
    fn split_max1_omitting_empty_matches_swift() {
        assert_eq!(split_max1_omitting_empty("a:b:c", ':'), vec!["a", "b:c"]);
        assert_eq!(split_max1_omitting_empty("light:", ':'), vec!["light"]);
        assert_eq!(split_max1_omitting_empty(":dark", ':'), vec!["dark"]);
        assert_eq!(split_max1_omitting_empty("plain", ':'), vec!["plain"]);
        let empty: Vec<&str> = Vec::new();
        assert_eq!(split_max1_omitting_empty("::", ':'), empty);
    }
}
