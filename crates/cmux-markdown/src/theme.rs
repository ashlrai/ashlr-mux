//! Port of `MarkdownWebTheme` and the `NSColor` markdown color math
//! (`Sources/Panels/MarkdownWebSupport.swift:90-221`) plus the `applyTheme` CSS
//! variable payload (`Sources/Panels/MarkdownWebRenderer.swift:352-361`).
//!
//! Given the viewer's opaque background color, this derives the six GitHub-CSS
//! theme variables the shell injects. Everything is pure math on an sRGB
//! `(r, g, b)` triple — the one input the GUI layer supplies — so it is fully
//! headless and testable. The muted/neutral/border tones are each a translucent
//! white (on a dark background) or black (on a light background) overlay whose
//! alpha is found by an 18-iteration binary search targeting a fixed WCAG
//! contrast ratio against the background, exactly as in Swift.
//!
//! Fidelity notes:
//! - `isDark` uses the simple NTSC luminance `0.299R+0.587G+0.114B > 0.5` on the
//!   gamma-encoded channels (Swift `NSColor.isLightColor` /
//!   `CmuxFoundation`'s `luminance`), NOT the WCAG relative luminance used for
//!   the contrast search — the two are deliberately different.
//! - `NSColor.blended(withFraction:of:)` blends the gamma-encoded sRGB channels
//!   linearly; the port reproduces that per-channel `base*(1-f)+overlay*f`.
//! - The overlay result is the overlay color (pure white or black) at the
//!   searched alpha — the blended candidate is only used to measure contrast.
//! - `--borderColor-neutral-muted` maps to `mutedBorder` (the SAME value as
//!   `--borderColor-muted`), preserving the Swift payload exactly.

/// The resolved markdown viewer theme: six CSS values derived from the
/// background color. Port of `struct MarkdownWebTheme`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownWebTheme {
    pub is_dark: bool,
    /// Always `"transparent"` — the viewer composites over the host surface.
    pub background: String,
    pub muted_background: String,
    pub neutral_muted_background: String,
    pub border: String,
    pub muted_border: String,
}

impl MarkdownWebTheme {
    /// Port of `MarkdownWebTheme.resolve(backgroundColor:)`.
    ///
    /// `background` is the opaque sRGB background color as 8-bit channels.
    pub fn resolve(background: (u8, u8, u8)) -> MarkdownWebTheme {
        let base = to_unit(background);
        let is_dark = !is_light(base);
        // White overlay on dark backgrounds, black overlay on light ones.
        let overlay_u8: (u8, u8, u8) = if is_dark { (255, 255, 255) } else { (0, 0, 0) };
        let overlay = to_unit(overlay_u8);

        let muted = theme_overlay_alpha(base, overlay, if is_dark { 1.09 } else { 1.06 });
        let neutral = theme_overlay_alpha(base, overlay, if is_dark { 1.35 } else { 1.20 });
        let border = theme_overlay_alpha(base, overlay, if is_dark { 1.92 } else { 1.43 });

        MarkdownWebTheme {
            is_dark,
            background: "transparent".to_string(),
            muted_background: css_color(overlay_u8, muted),
            neutral_muted_background: css_color(overlay_u8, neutral),
            border: css_color(overlay_u8, border),
            // mutedBorder = border overlay at 70% of the border alpha.
            muted_border: css_color(overlay_u8, border * 0.70),
        }
    }

    /// The six CSS custom properties the shell injects, in the exact order and
    /// mapping of Swift `applyTheme`. Note `--borderColor-neutral-muted` reuses
    /// `muted_border`.
    pub fn css_variables(&self) -> [(&'static str, &str); 6] {
        [
            ("--bgColor-default", &self.background),
            ("--bgColor-muted", &self.muted_background),
            ("--bgColor-neutral-muted", &self.neutral_muted_background),
            ("--borderColor-default", &self.border),
            ("--borderColor-muted", &self.muted_border),
            ("--borderColor-neutral-muted", &self.muted_border),
        ]
    }
}

fn to_unit((r, g, b): (u8, u8, u8)) -> (f64, f64, f64) {
    (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0)
}

/// Simple NTSC-weighted luminance on gamma-encoded channels; `> 0.5` reads as a
/// light color (port of `NSColor.isLightColor` / `luminance`).
fn is_light((r, g, b): (f64, f64, f64)) -> bool {
    (0.299 * r) + (0.587 * g) + (0.114 * b) > 0.5
}

/// WCAG relative luminance (port of `markdownRelativeLuminance`).
fn relative_luminance((r, g, b): (f64, f64, f64)) -> f64 {
    fn linear(c: f64) -> f64 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    (0.2126 * linear(r)) + (0.7152 * linear(g)) + (0.0722 * linear(b))
}

/// WCAG contrast ratio (port of `markdownContrastRatio`).
fn contrast_ratio(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let lighter = la.max(lb);
    let darker = la.min(lb);
    (lighter + 0.05) / (darker + 0.05)
}

/// Binary-search the overlay alpha that reaches `target_contrast` against the
/// base (port of `markdownThemeOverlay`). Returns the alpha in `0..=1`; the
/// caller applies it to the overlay color.
fn theme_overlay_alpha(
    base: (f64, f64, f64),
    overlay: (f64, f64, f64),
    target_contrast: f64,
) -> f64 {
    let mut low = 0.0_f64;
    let mut high = 1.0_f64;
    let mut result = 1.0_f64;
    for _ in 0..18 {
        let mid = (low + high) / 2.0;
        let candidate = blend(base, overlay, mid);
        if contrast_ratio(candidate, base) < target_contrast {
            low = mid;
        } else {
            high = mid;
            result = mid;
        }
    }
    result
}

/// Linear per-channel blend of gamma-encoded sRGB values (port of
/// `NSColor.blended(withFraction:of:)`).
fn blend(base: (f64, f64, f64), overlay: (f64, f64, f64), fraction: f64) -> (f64, f64, f64) {
    (
        base.0 * (1.0 - fraction) + overlay.0 * fraction,
        base.1 * (1.0 - fraction) + overlay.1 * fraction,
        base.2 * (1.0 - fraction) + overlay.2 * fraction,
    )
}

/// Format an `(r, g, b)` + alpha as `rgba(r, g, b, .3f)` (port of
/// `markdownCSSColor`).
fn css_color((r, g, b): (u8, u8, u8), alpha: f64) -> String {
    let a = alpha.clamp(0.0, 1.0);
    format!("rgba({r}, {g}, {b}, {a:.3})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_white_black_is_21() {
        let ratio = contrast_ratio((1.0, 1.0, 1.0), (0.0, 0.0, 0.0));
        assert!((ratio - 21.0).abs() < 1e-9, "got {ratio}");
    }

    #[test]
    fn relative_luminance_endpoints() {
        assert!((relative_luminance((1.0, 1.0, 1.0)) - 1.0).abs() < 1e-9);
        assert!(relative_luminance((0.0, 0.0, 0.0)).abs() < 1e-12);
    }

    #[test]
    fn is_light_threshold() {
        assert!(is_light((1.0, 1.0, 1.0)));
        assert!(!is_light((0.0, 0.0, 0.0)));
        // GitHub dark background (#0d1117) is dark.
        assert!(!is_light(to_unit((13, 17, 23))));
        // GitHub light background (#ffffff) is light.
        assert!(is_light(to_unit((255, 255, 255))));
    }

    #[test]
    fn css_color_format() {
        assert_eq!(
            css_color((255, 255, 255), 0.5),
            "rgba(255, 255, 255, 0.500)"
        );
        assert_eq!(css_color((0, 0, 0), 0.1234), "rgba(0, 0, 0, 0.123)");
        assert_eq!(css_color((0, 0, 0), 2.0), "rgba(0, 0, 0, 1.000)");
    }

    #[test]
    fn dark_theme_uses_white_overlay_and_transparent_bg() {
        let theme = MarkdownWebTheme::resolve((13, 17, 23));
        assert!(theme.is_dark);
        assert_eq!(theme.background, "transparent");
        assert!(theme.muted_background.starts_with("rgba(255, 255, 255,"));
        assert!(theme.border.starts_with("rgba(255, 255, 255,"));
    }

    #[test]
    fn light_theme_uses_black_overlay() {
        let theme = MarkdownWebTheme::resolve((255, 255, 255));
        assert!(!theme.is_dark);
        assert!(theme.muted_background.starts_with("rgba(0, 0, 0,"));
    }

    #[test]
    fn muted_border_is_seventy_percent_of_border_alpha() {
        // Recover the alphas from the formatted strings and check the ratio.
        let theme = MarkdownWebTheme::resolve((13, 17, 23));
        let border_a = parse_alpha(&theme.border);
        let muted_a = parse_alpha(&theme.muted_border);
        assert!(
            (muted_a - border_a * 0.70).abs() < 1e-3,
            "border={border_a} muted={muted_a}"
        );
    }

    #[test]
    fn overlay_alpha_reaches_target_contrast() {
        let base = to_unit((13, 17, 23));
        let overlay = (1.0, 1.0, 1.0);
        let alpha = theme_overlay_alpha(base, overlay, 1.92);
        let candidate = blend(base, overlay, alpha);
        let ratio = contrast_ratio(candidate, base);
        // 18 iterations converge tightly to the target.
        assert!((ratio - 1.92).abs() < 0.02, "ratio={ratio}");
    }

    #[test]
    fn css_variables_mapping() {
        let theme = MarkdownWebTheme::resolve((13, 17, 23));
        let vars = theme.css_variables();
        assert_eq!(vars[0].0, "--bgColor-default");
        assert_eq!(vars[0].1, "transparent");
        assert_eq!(vars[3].0, "--borderColor-default");
        // neutral-muted reuses muted-border exactly.
        assert_eq!(vars[4].0, "--borderColor-muted");
        assert_eq!(vars[5].0, "--borderColor-neutral-muted");
        assert_eq!(vars[4].1, vars[5].1);
    }

    fn parse_alpha(css: &str) -> f64 {
        // rgba(r, g, b, a)
        let a = css.rsplit(',').next().unwrap().trim().trim_end_matches(')');
        a.parse().unwrap()
    }
}
