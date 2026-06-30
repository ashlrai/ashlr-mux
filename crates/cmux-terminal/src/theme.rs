//! Terminal color theme payload (M2 WS2).
//!
//! The renderer sources all colors from a [`TerminalTheme`]. The web chrome
//! sends theme updates as a Tauri event (`terminal_set_theme`); M9 owns the
//! settings UI, M2 owns the renderer side. The field set mirrors the colors
//! cmux dispatches through `GhosttyNotificationKey`
//! (`Sources/GhosttyTerminalAppearance.swift:88`–`:104`): foreground,
//! background, cursor, cursor-text, selection background/foreground, plus the
//! ANSI color palette.
//!
//! Colors are carried on the wire as CSS-style hex strings (`#rrggbb` or
//! `#rrggbbaa`) — the format the web chrome and Ghostty theme files already use.
//! The JSON shape is versioned ([`THEME_CONTRACT_VERSION`]) and parity-tested
//! (cross-cutting rule 1).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Wire-format version for the terminal theme contract. Bump on any breaking
/// change to the JSON shape.
pub const THEME_CONTRACT_VERSION: u32 = 1;

/// An 8-bit-per-channel RGBA color. Serialized as a CSS hex string: `#rrggbb`
/// when fully opaque, otherwise `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// Error returned when a hex color string cannot be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorParseError(String);

impl fmt::Display for ColorParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid hex color: {}", self.0)
    }
}

impl std::error::Error for ColorParseError {}

impl FromStr for Color {
    type Err = ColorParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s.strip_prefix('#').unwrap_or(s);
        let parse = |slice: &str| -> Result<u8, ColorParseError> {
            u8::from_str_radix(slice, 16).map_err(|_| ColorParseError(s.to_owned()))
        };
        match hex.len() {
            // #rgb shorthand: each nibble is doubled (f -> ff).
            3 => {
                let nibble = |c: char| -> Result<u8, ColorParseError> {
                    let v = c.to_digit(16).ok_or_else(|| ColorParseError(s.to_owned()))? as u8;
                    Ok(v << 4 | v)
                };
                let mut chars = hex.chars();
                Ok(Color::rgb(
                    nibble(chars.next().unwrap())?,
                    nibble(chars.next().unwrap())?,
                    nibble(chars.next().unwrap())?,
                ))
            }
            6 => Ok(Color::rgb(
                parse(&hex[0..2])?,
                parse(&hex[2..4])?,
                parse(&hex[4..6])?,
            )),
            8 => Ok(Color::rgba(
                parse(&hex[0..2])?,
                parse(&hex[2..4])?,
                parse(&hex[4..6])?,
                parse(&hex[6..8])?,
            )),
            _ => Err(ColorParseError(s.to_owned())),
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.a == 255 {
            write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            write!(
                f,
                "#{:02x}{:02x}{:02x}{:02x}",
                self.r, self.g, self.b, self.a
            )
        }
    }
}

impl Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// The number of ANSI colors in a standard palette (0–15).
pub const ANSI_PALETTE_LEN: usize = 16;
/// The number of colors in a full xterm-256 palette.
pub const FULL_PALETTE_LEN: usize = 256;

/// A complete terminal color theme. `palette` holds either the 16 ANSI colors
/// or a full 256-color palette (validated by [`TerminalTheme::validate`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalTheme {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
    pub cursor_text: Color,
    pub selection_background: Color,
    pub selection_foreground: Color,
    pub palette: Vec<Color>,
}

/// Error returned when a [`TerminalTheme`] fails structural validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeError {
    /// The palette length is neither 16 nor 256.
    PaletteLength(usize),
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ThemeError::PaletteLength(n) => write!(
                f,
                "palette must have {ANSI_PALETTE_LEN} or {FULL_PALETTE_LEN} entries, got {n}"
            ),
        }
    }
}

impl std::error::Error for ThemeError {}

impl TerminalTheme {
    /// Validate the structural invariants of the theme. The palette must hold
    /// exactly 16 (ANSI) or 256 (xterm) colors so palette indexing from the
    /// VT stream can never go out of bounds.
    pub fn validate(&self) -> Result<(), ThemeError> {
        match self.palette.len() {
            ANSI_PALETTE_LEN | FULL_PALETTE_LEN => Ok(()),
            other => Err(ThemeError::PaletteLength(other)),
        }
    }

    /// Look up an ANSI/xterm palette color by index, falling back to the
    /// foreground color when the index is out of range.
    pub fn palette_color(&self, index: usize) -> Color {
        self.palette.get(index).copied().unwrap_or(self.foreground)
    }
}

impl Default for TerminalTheme {
    /// A neutral dark default mirroring the standard xterm 16-color ANSI palette.
    fn default() -> Self {
        Self {
            foreground: Color::rgb(0xe5, 0xe5, 0xe5),
            background: Color::rgb(0x00, 0x00, 0x00),
            cursor: Color::rgb(0xff, 0xff, 0xff),
            cursor_text: Color::rgb(0x00, 0x00, 0x00),
            selection_background: Color::rgb(0x44, 0x47, 0x5a),
            selection_foreground: Color::rgb(0xff, 0xff, 0xff),
            palette: vec![
                Color::rgb(0x00, 0x00, 0x00), // 0 black
                Color::rgb(0xcd, 0x00, 0x00), // 1 red
                Color::rgb(0x00, 0xcd, 0x00), // 2 green
                Color::rgb(0xcd, 0xcd, 0x00), // 3 yellow
                Color::rgb(0x00, 0x00, 0xee), // 4 blue
                Color::rgb(0xcd, 0x00, 0xcd), // 5 magenta
                Color::rgb(0x00, 0xcd, 0xcd), // 6 cyan
                Color::rgb(0xe5, 0xe5, 0xe5), // 7 white
                Color::rgb(0x7f, 0x7f, 0x7f), // 8 bright black
                Color::rgb(0xff, 0x00, 0x00), // 9 bright red
                Color::rgb(0x00, 0xff, 0x00), // 10 bright green
                Color::rgb(0xff, 0xff, 0x00), // 11 bright yellow
                Color::rgb(0x5c, 0x5c, 0xff), // 12 bright blue
                Color::rgb(0xff, 0x00, 0xff), // 13 bright magenta
                Color::rgb(0x00, 0xff, 0xff), // 14 bright cyan
                Color::rgb(0xff, 0xff, 0xff), // 15 bright white
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_six_digit_hex() {
        assert_eq!("#1a2b3c".parse::<Color>().unwrap(), Color::rgb(0x1a, 0x2b, 0x3c));
    }

    #[test]
    fn parse_without_hash() {
        assert_eq!("ffffff".parse::<Color>().unwrap(), Color::rgb(255, 255, 255));
    }

    #[test]
    fn parse_eight_digit_hex_with_alpha() {
        assert_eq!(
            "#11223344".parse::<Color>().unwrap(),
            Color::rgba(0x11, 0x22, 0x33, 0x44)
        );
    }

    #[test]
    fn parse_three_digit_shorthand_expands() {
        assert_eq!("#f0a".parse::<Color>().unwrap(), Color::rgb(0xff, 0x00, 0xaa));
    }

    #[test]
    fn parse_rejects_bad_input() {
        assert!("#xyz123".parse::<Color>().is_err());
        assert!("#12345".parse::<Color>().is_err());
        assert!("".parse::<Color>().is_err());
    }

    #[test]
    fn display_omits_alpha_when_opaque() {
        assert_eq!(Color::rgb(0x1a, 0x2b, 0x3c).to_string(), "#1a2b3c");
    }

    #[test]
    fn display_includes_alpha_when_translucent() {
        assert_eq!(Color::rgba(0x1a, 0x2b, 0x3c, 0x80).to_string(), "#1a2b3c80");
    }

    #[test]
    fn color_hex_roundtrips() {
        for c in [
            Color::rgb(0, 0, 0),
            Color::rgb(255, 255, 255),
            Color::rgba(1, 2, 3, 4),
            Color::rgb(0xab, 0xcd, 0xef),
        ] {
            assert_eq!(c.to_string().parse::<Color>().unwrap(), c);
        }
    }

    #[test]
    fn color_serializes_as_hex_string() {
        let json = serde_json::to_string(&Color::rgb(0xff, 0x00, 0x00)).unwrap();
        assert_eq!(json, "\"#ff0000\"");
        let back: Color = serde_json::from_str("\"#00ff00\"").unwrap();
        assert_eq!(back, Color::rgb(0, 255, 0));
    }

    #[test]
    fn default_theme_is_valid_ansi_16() {
        let theme = TerminalTheme::default();
        assert_eq!(theme.palette.len(), ANSI_PALETTE_LEN);
        assert!(theme.validate().is_ok());
    }

    #[test]
    fn full_256_palette_is_valid() {
        let theme = TerminalTheme {
            palette: vec![Color::rgb(0, 0, 0); FULL_PALETTE_LEN],
            ..Default::default()
        };
        assert!(theme.validate().is_ok());
    }

    #[test]
    fn wrong_palette_length_is_rejected() {
        let mut theme = TerminalTheme::default();
        theme.palette.pop();
        assert_eq!(theme.validate(), Err(ThemeError::PaletteLength(15)));
    }

    #[test]
    fn palette_color_falls_back_to_foreground_out_of_range() {
        let theme = TerminalTheme::default();
        assert_eq!(theme.palette_color(0), Color::rgb(0, 0, 0));
        assert_eq!(theme.palette_color(999), theme.foreground);
    }

    #[test]
    fn theme_json_roundtrip() {
        let theme = TerminalTheme::default();
        let json = serde_json::to_string(&theme).unwrap();
        let back: TerminalTheme = serde_json::from_str(&json).unwrap();
        assert_eq!(theme, back);
        // Colors must be hex strings on the wire, not nested objects.
        assert!(json.contains("\"foreground\":\"#e5e5e5\""));
    }
}
