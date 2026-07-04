//! Closed-form sRGB color math shared by window chrome, hex encoding, and
//! canvas tab fills — merged port of three canonical macOS sources:
//!
//! - `Packages/macOS/CmuxAppKitSupportUI/.../WindowChrome/Color/WindowChromeColorResolver.swift:1-88`
//!   (WCAG relative luminance, contrast ratio, readable color scheme,
//!   alpha-over compositing, separator nudge).
//! - `Packages/macOS/CmuxFoundation/.../Color/NSColor+Hex.swift:28-79`
//!   (`isLightColor`, `darken(by:)`, `hexString(includeAlpha:)`).
//! - `Packages/macOS/CmuxCanvasUI/.../CanvasTabColorMath.swift:1-36`
//!   (Rec. 601 light test, blend-toward-white/black, active/hover tab fills).
//!
//! All math is window/UI-agnostic and operates on sRGB f64 components. This is
//! ADDITIVE: the private HSB helpers inside `cmux-workspaces::tab_colors`
//! (`[u8; 3]`-based, byte-truncating) are deliberately left untouched; this
//! module carries its own f64 HSB helpers so the two lanes stay independent.
//!
//! DIVERGENCE (color space): the Swift methods first convert the receiver to
//! sRGB (`usingColorSpace(.sRGB)`), and `NSColor.blended(withFraction:of:)`
//! interpolates in the calibrated/generic RGB space. This port accepts sRGB
//! f64 input directly and blends in sRGB, mirroring the repo-wide sRGB-f64
//! lane and the already-accepted ±1-byte `NSColor` round-trip divergence (see
//! `cmux-workspaces::tab_colors`). Component math is otherwise identical.

use serde::{Deserialize, Serialize};

use crate::ColorScheme;

/// An sRGB color with straight (non-premultiplied) alpha, all components in
/// `[0, 1]`. Mirrors the sRGB `NSColor` the canonical methods operate on after
/// `usingColorSpace(.sRGB)`; the caller supplies already-sRGB components.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Srgba {
    /// Red component in `[0, 1]`.
    pub red: f64,
    /// Green component in `[0, 1]`.
    pub green: f64,
    /// Blue component in `[0, 1]`.
    pub blue: f64,
    /// Straight alpha in `[0, 1]`.
    pub alpha: f64,
}

impl Srgba {
    /// Creates an sRGB color from straight-alpha components.
    pub fn new(red: f64, green: f64, blue: f64, alpha: f64) -> Srgba {
        Srgba {
            red,
            green,
            blue,
            alpha,
        }
    }

    /// Returns a copy with `alpha` replaced, mirroring
    /// `NSColor.withAlphaComponent(_:)`.
    pub fn with_alpha(self, alpha: f64) -> Srgba {
        Srgba { alpha, ..self }
    }

    // -- WCAG (WindowChromeColorResolver) -----------------------------------

    /// WCAG relative luminance in sRGB, gamma-linearizing each channel with the
    /// `0.03928 / 12.92` split and `((c + 0.055) / 1.055)^2.4` curve, weighted
    /// `0.2126 / 0.7152 / 0.0722`. Alpha is ignored. Mirrors
    /// `WindowChromeColorResolver.relativeLuminance` /`.linearized`
    /// (`WindowChromeColorResolver.swift:68-87`).
    ///
    /// DISTINCT from the Rec. 601 `luminance` used by
    /// [`Srgba::is_light_color`]: this is the gamma-linearized perceptual
    /// luminance used only for contrast decisions.
    pub fn relative_luminance(self) -> f64 {
        fn linearized(component: f64) -> f64 {
            if component <= 0.03928 {
                component / 12.92
            } else {
                ((component + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linearized(self.red)
            + 0.7152 * linearized(self.green)
            + 0.0722 * linearized(self.blue)
    }

    // -- Rec. 601 light test (NSColor+Hex / CanvasTabColorMath) -------------

    /// Whether the color reads as light: Rec. 601 luma `0.299r + 0.587g +
    /// 0.114b > 0.5`. Alpha is ignored. Mirrors `NSColor.isLightColor` /
    /// `NSColor.luminance` (`NSColor+Hex.swift:28-43`).
    pub fn is_light_color(self) -> bool {
        rec601_luma(self) > 0.5
    }

    /// Rec. 601 light test used by canvas tab fills. Numerically identical to
    /// [`Srgba::is_light_color`] but ported from a distinct source. Mirrors
    /// `NSColor.cmuxCanvasIsLight` (`CanvasTabColorMath.swift:9-14`).
    pub fn cmux_canvas_is_light(self) -> bool {
        rec601_luma(self) > 0.5
    }

    // -- HSB darken (NSColor+Hex) -------------------------------------------

    /// Returns a darkened copy, scaling HSB brightness by `1 - amount` and
    /// clamping to at most `1` (no lower clamp), preserving hue, saturation, and
    /// alpha. Mirrors `NSColor.darken(by:)` (`NSColor+Hex.swift:47-59`).
    pub fn darken(self, amount: f64) -> Srgba {
        let (hue, saturation, brightness) = rgb_to_hsb(self);
        let darkened = (brightness * (1.0 - amount)).min(1.0);
        let (red, green, blue) = hsb_to_rgb(hue, saturation, darkened);
        Srgba {
            red,
            green,
            blue,
            alpha: self.alpha,
        }
    }

    // -- Hex encoding (NSColor+Hex) -----------------------------------------

    /// The `#`-prefixed, uppercased sRGB hex string: `#RRGGBB`, or `#RRGGBBAA`
    /// when `include_alpha` is `true`. Each byte is `Int(component * 255)`
    /// (truncated toward zero) clamped to `[0, 255]`. Mirrors
    /// `NSColor.hexString(includeAlpha:)` (`NSColor+Hex.swift:64-79`).
    pub fn hex_string(self, include_alpha: bool) -> String {
        let red = component_to_byte(self.red);
        let green = component_to_byte(self.green);
        let blue = component_to_byte(self.blue);
        if include_alpha {
            let alpha = component_to_byte(self.alpha);
            format!("#{red:02X}{green:02X}{blue:02X}{alpha:02X}")
        } else {
            format!("#{red:02X}{green:02X}{blue:02X}")
        }
    }

    // -- Canvas tab fills (CanvasTabColorMath) ------------------------------

    /// Lightens toward opaque white by `amount` (`0...1`), a per-component
    /// linear blend `self * (1 - amount) + white * amount` including alpha.
    /// Mirrors `NSColor.cmuxCanvasLighten(by:)` = `blended(withFraction:of:
    /// .white)` (`CanvasTabColorMath.swift:17-19`).
    pub fn cmux_canvas_lighten(self, amount: f64) -> Srgba {
        self.blended(amount, Srgba::new(1.0, 1.0, 1.0, 1.0))
    }

    /// Darkens toward opaque black by `amount` (`0...1`), a per-component linear
    /// blend `self * (1 - amount) + black * amount` including alpha. Mirrors
    /// `NSColor.cmuxCanvasDarken(by:)` = `blended(withFraction:of:.black)`
    /// (`CanvasTabColorMath.swift:22-24`).
    pub fn cmux_canvas_darken(self, amount: f64) -> Srgba {
        self.blended(amount, Srgba::new(0.0, 0.0, 0.0, 1.0))
    }

    /// The active (selected) tab fill for a bar of this background color: light
    /// bars darken by `0.065`, dark bars lighten by `0.12`. Mirrors
    /// `NSColor.cmuxCanvasActiveTabFill` (`CanvasTabColorMath.swift:27-29`).
    pub fn cmux_canvas_active_tab_fill(self) -> Srgba {
        if self.cmux_canvas_is_light() {
            self.cmux_canvas_darken(0.065)
        } else {
            self.cmux_canvas_lighten(0.12)
        }
    }

    /// The hovered (unselected) tab fill for a bar of this background color:
    /// light bars darken by `0.03`, dark bars lighten by `0.07`, then alpha is
    /// forced to `0.78`. Mirrors `NSColor.cmuxCanvasHoverTabFill`
    /// (`CanvasTabColorMath.swift:32-35`).
    pub fn cmux_canvas_hover_tab_fill(self) -> Srgba {
        let adjusted = if self.cmux_canvas_is_light() {
            self.cmux_canvas_darken(0.03)
        } else {
            self.cmux_canvas_lighten(0.07)
        };
        adjusted.with_alpha(0.78)
    }

    /// Per-component linear blend `self * (1 - fraction) + other * fraction`,
    /// including alpha, in sRGB. The sRGB counterpart of
    /// `NSColor.blended(withFraction:of:)`.
    fn blended(self, fraction: f64, other: Srgba) -> Srgba {
        let keep = 1.0 - fraction;
        Srgba {
            red: self.red * keep + other.red * fraction,
            green: self.green * keep + other.green * fraction,
            blue: self.blue * keep + other.blue * fraction,
            alpha: self.alpha * keep + other.alpha * fraction,
        }
    }
}

/// Rec. 601 luma over gamma-encoded sRGB components: `0.299r + 0.587g +
/// 0.114b`. Alpha is ignored. Shared by `isLightColor`, `cmuxCanvasIsLight`,
/// and the separator nudge.
fn rec601_luma(color: Srgba) -> f64 {
    0.299 * color.red + 0.587 * color.green + 0.114 * color.blue
}

/// `Int(component * 255)` (truncated toward zero) clamped to `[0, 255]`,
/// matching Swift `min(255, max(0, Int(component * 255)))`
/// (`NSColor+Hex.swift:71-76`).
fn component_to_byte(component: f64) -> u8 {
    ((component * 255.0) as i64).clamp(0, 255) as u8
}

/// WCAG contrast ratio `(lighter + 0.05) / (darker + 0.05)` over two relative
/// luminances. Mirrors `WindowChromeColorResolver.contrastRatio`
/// (`WindowChromeColorResolver.swift:62-66`).
pub fn contrast_ratio(lhs: f64, rhs: f64) -> f64 {
    let lighter = lhs.max(rhs);
    let darker = lhs.min(rhs);
    (lighter + 0.05) / (darker + 0.05)
}

/// The color scheme with stronger contrast against `background`: `.dark` when
/// white contrasts at least as well as black, else `.light`. Mirrors
/// `WindowChromeColorResolver.readableColorScheme(for:)`
/// (`WindowChromeColorResolver.swift:54-60`).
pub fn readable_color_scheme(background: Srgba) -> ColorScheme {
    let background_luminance = background.relative_luminance();
    let white_contrast = contrast_ratio(background_luminance, 1.0);
    let black_contrast = contrast_ratio(background_luminance, 0.0);
    if white_contrast >= black_contrast {
        ColorScheme::Dark
    } else {
        ColorScheme::Light
    }
}

/// Composites `foreground` over `background` in sRGB (straight alpha-over),
/// returning an opaque color. The foreground alpha is clamped to `[0, 1]`; the
/// background alpha is ignored. Mirrors
/// `WindowChromeColorResolver.compositedColor(_:over:)`
/// (`WindowChromeColorResolver.swift:30-52`).
pub fn composited_color(foreground: Srgba, background: Srgba) -> Srgba {
    let alpha = foreground.alpha.clamp(0.0, 1.0);
    Srgba {
        red: foreground.red * alpha + background.red * (1.0 - alpha),
        green: foreground.green * alpha + background.green * (1.0 - alpha),
        blue: foreground.blue * alpha + background.blue * (1.0 - alpha),
        alpha: 1.0,
    }
}

/// A separator color readable against the given chrome background: Rec. 601
/// light backgrounds nudge each component by `-0.12` at alpha `0.26`, dark
/// backgrounds by `+0.16` at alpha `0.36`, with components clamped to
/// `[0, 1]`. Mirrors `WindowChromeColorResolver.separatorColor(forChromeBackground:)`
/// (`WindowChromeColorResolver.swift:10-27`).
pub fn separator_color(chrome: Srgba) -> Srgba {
    let is_light = rec601_luma(chrome) > 0.5;
    let amount = if is_light { -0.12 } else { 0.16 };
    let separator_alpha = if is_light { 0.26 } else { 0.36 };
    Srgba {
        red: (chrome.red + amount).clamp(0.0, 1.0),
        green: (chrome.green + amount).clamp(0.0, 1.0),
        blue: (chrome.blue + amount).clamp(0.0, 1.0),
        alpha: separator_alpha,
    }
}

/// RGB → HSB (hue in `[0, 1)` turns), matching `NSColor.getHue`: brightness =
/// max component; saturation = `(max - min) / max` (0 when max is 0); hue by
/// the standard sextant formula. f64 counterpart of the byte-based helper in
/// `cmux-workspaces::tab_colors` (kept independent per the additive mandate).
fn rgb_to_hsb(color: Srgba) -> (f64, f64, f64) {
    let (r, g, b) = (color.red, color.green, color.blue);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let brightness = max;
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        (((g - b) / delta).rem_euclid(6.0)) / 6.0
    } else if max == g {
        (((b - r) / delta) + 2.0) / 6.0
    } else {
        (((r - g) / delta) + 4.0) / 6.0
    };
    (hue, saturation, brightness)
}

/// HSB (hue in `[0, 1)` turns) → RGB in f64 (no byte truncation, unlike the
/// `tab_colors` helper), matching `NSColor(hue:saturation:brightness:)`.
fn hsb_to_rgb(hue: f64, saturation: f64, brightness: f64) -> (f64, f64, f64) {
    if saturation <= 0.0 {
        return (brightness, brightness, brightness);
    }
    let h6 = hue.rem_euclid(1.0) * 6.0;
    let sector = h6.floor();
    let f = h6 - sector;
    let p = brightness * (1.0 - saturation);
    let q = brightness * (1.0 - saturation * f);
    let t = brightness * (1.0 - saturation * (1.0 - f));
    match sector as i64 {
        0 => (brightness, t, p),
        1 => (q, brightness, p),
        2 => (p, brightness, t),
        3 => (p, q, brightness),
        4 => (t, p, brightness),
        _ => (brightness, p, q),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    fn assert_srgba(actual: Srgba, expected: Srgba) {
        assert_close(actual.red, expected.red);
        assert_close(actual.green, expected.green);
        assert_close(actual.blue, expected.blue);
        assert_close(actual.alpha, expected.alpha);
    }

    // -- hexString oracle: NSColorHexStringTests (NSColor+HexStringTests.swift)

    // `opaquePrimaryEncodesAsUppercaseRGB`.
    #[test]
    fn opaque_primary_encodes_as_uppercase_rgb() {
        let red = Srgba::new(1.0, 0.0, 0.0, 1.0);
        assert_eq!(red.hex_string(false), "#FF0000");
    }

    // `midGrayRoundsComponentsToBytes`: 0.5 * 255 = 127.5 -> Int truncates ->
    // 127 -> 0x7F.
    #[test]
    fn mid_gray_rounds_components_to_bytes() {
        let gray = Srgba::new(0.5, 0.5, 0.5, 1.0);
        assert_eq!(gray.hex_string(false), "#7F7F7F");
    }

    // `includeAlphaAppendsAlphaByte`: 0.5 alpha -> 0x7F.
    #[test]
    fn include_alpha_appends_alpha_byte() {
        let translucent = Srgba::new(0.0, 0.0, 1.0, 0.5);
        assert_eq!(translucent.hex_string(true), "#0000FF7F");
    }

    // `alphaIsOmittedByDefault`.
    #[test]
    fn alpha_is_omitted_by_default() {
        let translucent = Srgba::new(0.0, 1.0, 0.0, 0.25);
        assert_eq!(translucent.hex_string(false), "#00FF00");
    }

    // Byte truncation + clamp edges (component_to_byte parity with Swift
    // min(255, max(0, Int(component * 255)))).
    #[test]
    fn hex_string_clamps_and_truncates() {
        // Opaque alpha -> FF; 0.25 * 255 = 63.75 -> 63 -> 0x3F.
        assert_eq!(Srgba::new(1.0, 1.0, 1.0, 1.0).hex_string(true), "#FFFFFFFF");
        assert_eq!(Srgba::new(0.0, 0.0, 0.0, 0.25).hex_string(true), "#0000003F");
        // Out-of-range components clamp to the byte bounds.
        assert_eq!(Srgba::new(1.5, -0.5, 0.0, 1.0).hex_string(false), "#FF0000");
        // 0.999 * 255 = 254.745 -> 254 -> 0xFE (truncation, not rounding).
        assert_eq!(Srgba::new(0.999, 0.0, 0.0, 1.0).hex_string(false), "#FE0000");
    }

    // -- WCAG relative luminance + contrast (WindowChromeColorResolver) ------

    #[test]
    fn relative_luminance_endpoints() {
        assert_close(Srgba::new(1.0, 1.0, 1.0, 1.0).relative_luminance(), 1.0);
        assert_close(Srgba::new(0.0, 0.0, 0.0, 1.0).relative_luminance(), 0.0);
    }

    // Hand-computed from the linearization curve: 0.5 > 0.03928 ->
    // ((0.5 + 0.055) / 1.055)^2.4 = 0.214041140..., times (0.2126+0.7152+0.0722)
    // = 1.0 for a neutral gray.
    #[test]
    fn relative_luminance_mid_gray() {
        let expected = ((0.5 + 0.055) / 1.055_f64).powf(2.4);
        assert_close(
            Srgba::new(0.5, 0.5, 0.5, 1.0).relative_luminance(),
            expected,
        );
    }

    // Black/white contrast is the canonical WCAG maximum of 21.
    #[test]
    fn contrast_ratio_black_white_is_21() {
        assert_close(contrast_ratio(1.0, 0.0), 21.0);
        assert_close(contrast_ratio(0.0, 1.0), 21.0);
        // Identical luminances -> ratio of 1.
        assert_close(contrast_ratio(0.3, 0.3), 1.0);
    }

    // readableColorScheme: white background reads as .light (dark text wins),
    // black background reads as .dark (white text wins).
    #[test]
    fn readable_color_scheme_endpoints() {
        assert_eq!(
            readable_color_scheme(Srgba::new(1.0, 1.0, 1.0, 1.0)),
            ColorScheme::Light
        );
        assert_eq!(
            readable_color_scheme(Srgba::new(0.0, 0.0, 0.0, 1.0)),
            ColorScheme::Dark
        );
    }

    // -- compositedColor (WindowChromeColorResolver) -------------------------

    #[test]
    fn composited_color_opaque_foreground_wins() {
        let fg = Srgba::new(1.0, 0.0, 0.0, 1.0);
        let bg = Srgba::new(0.0, 0.0, 1.0, 1.0);
        assert_srgba(composited_color(fg, bg), Srgba::new(1.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn composited_color_transparent_foreground_shows_background() {
        let fg = Srgba::new(1.0, 0.0, 0.0, 0.0);
        let bg = Srgba::new(0.0, 0.0, 1.0, 1.0);
        // Result is always opaque; background alpha is ignored.
        assert_srgba(composited_color(fg, bg), Srgba::new(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn composited_color_half_alpha_is_midpoint() {
        let fg = Srgba::new(1.0, 1.0, 1.0, 0.5);
        let bg = Srgba::new(0.0, 0.0, 0.0, 1.0);
        assert_srgba(composited_color(fg, bg), Srgba::new(0.5, 0.5, 0.5, 1.0));
    }

    #[test]
    fn composited_color_clamps_foreground_alpha() {
        let bg = Srgba::new(0.0, 0.0, 1.0, 1.0);
        // alpha > 1 clamps to 1 -> foreground wins.
        let opaque = Srgba::new(1.0, 0.0, 0.0, 2.0);
        assert_srgba(composited_color(opaque, bg), Srgba::new(1.0, 0.0, 0.0, 1.0));
        // alpha < 0 clamps to 0 -> background wins.
        let clear = Srgba::new(1.0, 0.0, 0.0, -1.0);
        assert_srgba(composited_color(clear, bg), Srgba::new(0.0, 0.0, 1.0, 1.0));
    }

    // -- separatorColor (WindowChromeColorResolver) --------------------------

    #[test]
    fn separator_color_light_chrome_darkens_at_low_alpha() {
        // White is light: amount -0.12, alpha 0.26.
        assert_srgba(
            separator_color(Srgba::new(1.0, 1.0, 1.0, 1.0)),
            Srgba::new(0.88, 0.88, 0.88, 0.26),
        );
    }

    #[test]
    fn separator_color_dark_chrome_lightens_at_higher_alpha() {
        // Black is dark: amount +0.16, alpha 0.36.
        assert_srgba(
            separator_color(Srgba::new(0.0, 0.0, 0.0, 1.0)),
            Srgba::new(0.16, 0.16, 0.16, 0.36),
        );
    }

    // -- isLightColor / cmuxCanvasIsLight (Rec. 601) -------------------------

    #[test]
    fn is_light_color_thresholds_at_rec601_luma() {
        assert!(Srgba::new(1.0, 1.0, 1.0, 1.0).is_light_color());
        assert!(!Srgba::new(0.0, 0.0, 0.0, 1.0).is_light_color());
        // Pure green luma 0.587 > 0.5 -> light.
        assert!(Srgba::new(0.0, 1.0, 0.0, 1.0).is_light_color());
        // Pure blue luma 0.114 <= 0.5 -> not light.
        assert!(!Srgba::new(0.0, 0.0, 1.0, 1.0).is_light_color());
        // cmuxCanvasIsLight is numerically identical.
        assert_eq!(
            Srgba::new(0.0, 1.0, 0.0, 1.0).cmux_canvas_is_light(),
            Srgba::new(0.0, 1.0, 0.0, 1.0).is_light_color()
        );
    }

    // -- darken (HSB, NSColor+Hex) -------------------------------------------

    #[test]
    fn darken_scales_hsb_brightness() {
        // White (achromatic): brightness 1 -> 0.5, hue/sat 0.
        assert_srgba(
            Srgba::new(1.0, 1.0, 1.0, 1.0).darken(0.5),
            Srgba::new(0.5, 0.5, 0.5, 1.0),
        );
        // Saturated red: brightness 1 -> 0.5, hue/sat preserved.
        assert_srgba(
            Srgba::new(1.0, 0.0, 0.0, 1.0).darken(0.5),
            Srgba::new(0.5, 0.0, 0.0, 1.0),
        );
    }

    #[test]
    fn darken_by_zero_is_identity_and_preserves_alpha() {
        let color = Srgba::new(0.2, 0.6, 0.9, 0.4);
        assert_srgba(color.darken(0.0), color);
        // Alpha survives the HSB round-trip.
        assert_close(Srgba::new(1.0, 0.0, 0.0, 0.4).darken(0.5).alpha, 0.4);
    }

    // -- canvas blends (CanvasTabColorMath) ----------------------------------

    #[test]
    fn cmux_canvas_lighten_blends_toward_white() {
        // (0.2, 0.4, 0.6) lightened by 0.25 -> each * 0.75 + 0.25.
        assert_srgba(
            Srgba::new(0.2, 0.4, 0.6, 1.0).cmux_canvas_lighten(0.25),
            Srgba::new(0.4, 0.55, 0.7, 1.0),
        );
    }

    #[test]
    fn cmux_canvas_darken_blends_toward_black() {
        // (0.2, 0.4, 0.6) darkened by 0.25 -> each * 0.75; alpha 1*0.75 + 0.25 = 1.
        assert_srgba(
            Srgba::new(0.2, 0.4, 0.6, 1.0).cmux_canvas_darken(0.25),
            Srgba::new(0.15, 0.3, 0.45, 1.0),
        );
    }

    #[test]
    fn cmux_canvas_active_tab_fill_directional() {
        // Light bar (white) darkens by 0.065 toward black.
        assert_srgba(
            Srgba::new(1.0, 1.0, 1.0, 1.0).cmux_canvas_active_tab_fill(),
            Srgba::new(0.935, 0.935, 0.935, 1.0),
        );
        // Dark bar (black) lightens by 0.12 toward white.
        assert_srgba(
            Srgba::new(0.0, 0.0, 0.0, 1.0).cmux_canvas_active_tab_fill(),
            Srgba::new(0.12, 0.12, 0.12, 1.0),
        );
    }

    #[test]
    fn cmux_canvas_hover_tab_fill_forces_alpha() {
        // Light bar darkens by 0.03, then alpha forced to 0.78.
        assert_srgba(
            Srgba::new(1.0, 1.0, 1.0, 1.0).cmux_canvas_hover_tab_fill(),
            Srgba::new(0.97, 0.97, 0.97, 0.78),
        );
        // Dark bar lightens by 0.07, then alpha forced to 0.78.
        assert_srgba(
            Srgba::new(0.0, 0.0, 0.0, 1.0).cmux_canvas_hover_tab_fill(),
            Srgba::new(0.07, 0.07, 0.07, 0.78),
        );
    }

    #[test]
    fn with_alpha_replaces_only_alpha() {
        let color = Srgba::new(0.1, 0.2, 0.3, 1.0);
        assert_srgba(color.with_alpha(0.78), Srgba::new(0.1, 0.2, 0.3, 0.78));
    }
}
