//! Port of the markdown viewer typography domain logic:
//! `MarkdownFontSizeSettings`, `MarkdownMaxWidthSettings`, `MarkdownFontFamily`,
//! and `MarkdownTypographyDefaults` (`Sources/Panels/Markdown*.swift`).
//!
//! This is the pure clamp/step/zoom/normalize/escape layer only. Two things in
//! the Swift sources are intentionally OUT of scope for a headless crate and are
//! not ported:
//! - `MarkdownFontFamily.availableFamilies()` / `MarkdownFontFamilyCache` — these
//!   enumerate installed fonts via CoreText (`CTFontManagerCopyAvailableFontFamilyNames`),
//!   which is platform GUI state, not headless logic.
//! - The `resolvedDefault` / `setDefault` / `resetDefault` `UserDefaults`
//!   persistence — there is no `UserDefaults` here. The persisted shape already
//!   lives in [`cmux_config::MarkdownConfig`] (`font_size: i64`,
//!   `font_family: String`, `max_width: i64`, defaults `15` / `""` / `980`); this
//!   module supplies the value logic that reads/writes those fields. The numeric
//!   defaults MUST stay in sync with `cmux-config` and
//!   `web/data/cmux.schema.json` — a `#[test]` below guards the `cmux-config` side.
//!
//! Swift uses `Double` throughout; the persisted config stores integers, so the
//! clamp/zoom math is done in `f64` and rounded to `i64` only when producing a
//! stored value (`Int(clamp(v).rounded())`), exactly as Swift `setDefault` does.

/// `markdown.fontSize` — the `.markdown-body` font size in points.
pub mod font_size {
    /// UserDefaults / cmux.json key.
    pub const KEY: &str = "markdown.fontSize";
    pub const DEFAULT_POINT_SIZE: f64 = 15.0;
    pub const MINIMUM_POINT_SIZE: f64 = 8.0;
    pub const MAXIMUM_POINT_SIZE: f64 = 96.0;
    pub const STEP_POINT_SIZE: f64 = 1.0;
    /// Intrinsic `.markdown-body` font size baked into `shell.html`, in CSS px.
    /// MUST stay in sync with the `.markdown-body { font-size: … }` rule the
    /// viewer-core `assets` module serves (`Resources/markdown-viewer/shell.html`).
    pub const BASE_RENDER_POINT_SIZE: f64 = 15.0;

    /// Clamps a requested point size into the supported range (port of `clamp` =
    /// Swift `min(max(value, min), max)`; inputs are never NaN — they come from a
    /// config `i64` or the settings stepper).
    pub fn clamp(value: f64) -> f64 {
        value.clamp(MINIMUM_POINT_SIZE, MAXIMUM_POINT_SIZE)
    }

    /// The persistent default point size given the raw stored value (port of
    /// `resolvedDefault`): clamp a present value, else fall back to the default.
    pub fn resolved_default(raw: Option<f64>) -> f64 {
        raw.map(clamp).unwrap_or(DEFAULT_POINT_SIZE)
    }

    /// The integer value `setDefault` persists: `Int(clamp(points).rounded())`.
    pub fn store_value(points: f64) -> i64 {
        clamp(points).round() as i64
    }

    /// The WKWebView `pageZoom` factor that renders the body at `point_size`
    /// (port of `pageZoom(forPointSize:)`).
    pub fn page_zoom(point_size: f64) -> f64 {
        clamp(point_size) / BASE_RENDER_POINT_SIZE
    }
}

/// `markdown.maxWidth` — the rendered `.markdown-body` `max-width` in CSS px.
pub mod max_width {
    pub const KEY: &str = "markdown.maxWidth";
    pub const DEFAULT_CSS_PIXELS: f64 = 980.0;
    pub const MINIMUM_CSS_PIXELS: f64 = 320.0;
    pub const MAXIMUM_CSS_PIXELS: f64 = 2400.0;
    pub const STEP_CSS_PIXELS: f64 = 20.0;

    /// Port of `clamp` (`min(max(value, min), max)`).
    pub fn clamp(value: f64) -> f64 {
        value.clamp(MINIMUM_CSS_PIXELS, MAXIMUM_CSS_PIXELS)
    }

    /// Port of `resolvedDefault`.
    pub fn resolved_default(raw: Option<f64>) -> f64 {
        raw.map(clamp).unwrap_or(DEFAULT_CSS_PIXELS)
    }

    /// The integer value `setDefault` persists: `Int(clamp(pixels).rounded())`.
    pub fn store_value(pixels: f64) -> i64 {
        clamp(pixels).round() as i64
    }
}

/// `markdown.fontFamily` — the body prose font; `""` is the System default.
pub mod font_family {
    pub const KEY: &str = "markdown.fontFamily";
    /// Sentinel for the System default (inherits the GitHub stack); clears the
    /// inline `font-family` override.
    pub const SYSTEM_DEFAULT: &str = "";

    /// Normalizes user/config input before persisting or applying it (port of
    /// `normalized`): collapse CR/LF to spaces (so a malformed multiline config
    /// value cannot emit invalid CSS), then trim surrounding whitespace.
    pub fn normalized(family: &str) -> String {
        family.replace(['\r', '\n'], " ").trim().to_string()
    }

    /// The CSS `font-family` value to apply, or `None` for the System default
    /// (port of `cssValue(for:)`). The name is backslash/quote-escaped and quoted
    /// so multi-word names resolve correctly.
    pub fn css_value(family: &str) -> Option<String> {
        let trimmed = normalized(family);
        if trimmed.is_empty() {
            return None;
        }
        let escaped = trimmed.replace('\\', "\\\\").replace('"', "\\\"");
        Some(format!("\"{escaped}\""))
    }

    /// The persistent default family given the raw stored value (port of
    /// `resolvedDefault`): normalize the present value or the System default.
    pub fn resolved_default(raw: Option<&str>) -> String {
        normalized(raw.unwrap_or(SYSTEM_DEFAULT))
    }

    /// The value `setDefault` persists: `None` (remove the key / System default)
    /// for an empty family, else the normalized name.
    pub fn store_value(family: &str) -> Option<String> {
        let trimmed = normalized(family);
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }
}

/// The persisted markdown typography triple, mirroring the stored shape of
/// [`cmux_config::MarkdownConfig`] (an empty `font_family` is the System
/// default). This is the headless analog of writing the three `markdown.*`
/// `UserDefaults` keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownTypography {
    pub font_size: i64,
    pub font_family: String,
    pub max_width: i64,
}

impl Default for MarkdownTypography {
    fn default() -> Self {
        MarkdownTypography {
            font_size: font_size::store_value(font_size::DEFAULT_POINT_SIZE),
            font_family: font_family::SYSTEM_DEFAULT.to_string(),
            max_width: max_width::store_value(max_width::DEFAULT_CSS_PIXELS),
        }
    }
}

impl MarkdownTypography {
    /// Port of `MarkdownTypographyDefaults.setDefault`: clamp + round each field
    /// into its stored form (an empty/whitespace family becomes the System
    /// default `""`).
    pub fn apply(font_size_pt: f64, font_family_name: &str, max_content_width: f64) -> Self {
        MarkdownTypography {
            font_size: font_size::store_value(font_size_pt),
            font_family: font_family::store_value(font_family_name).unwrap_or_default(),
            max_width: max_width::store_value(max_content_width),
        }
    }

    /// Port of `MarkdownTypographyDefaults.resetToBuiltInDefaults`: the built-in
    /// `15` / `""` / `980`.
    pub fn reset_to_builtin() -> Self {
        MarkdownTypography::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_size_clamp() {
        assert_eq!(font_size::clamp(4.0), 8.0);
        assert_eq!(font_size::clamp(200.0), 96.0);
        assert_eq!(font_size::clamp(15.0), 15.0);
        assert_eq!(font_size::clamp(8.0), 8.0);
        assert_eq!(font_size::clamp(96.0), 96.0);
    }

    #[test]
    fn max_width_clamp() {
        assert_eq!(max_width::clamp(100.0), 320.0);
        assert_eq!(max_width::clamp(5000.0), 2400.0);
        assert_eq!(max_width::clamp(980.0), 980.0);
    }

    #[test]
    fn constants_match_canonical() {
        assert_eq!(font_size::DEFAULT_POINT_SIZE, 15.0);
        assert_eq!(font_size::MINIMUM_POINT_SIZE, 8.0);
        assert_eq!(font_size::MAXIMUM_POINT_SIZE, 96.0);
        assert_eq!(font_size::STEP_POINT_SIZE, 1.0);
        assert_eq!(font_size::BASE_RENDER_POINT_SIZE, 15.0);
        assert_eq!(max_width::DEFAULT_CSS_PIXELS, 980.0);
        assert_eq!(max_width::MINIMUM_CSS_PIXELS, 320.0);
        assert_eq!(max_width::MAXIMUM_CSS_PIXELS, 2400.0);
        assert_eq!(max_width::STEP_CSS_PIXELS, 20.0);
        assert_eq!(font_family::SYSTEM_DEFAULT, "");
        assert_eq!(font_size::KEY, "markdown.fontSize");
        assert_eq!(max_width::KEY, "markdown.maxWidth");
        assert_eq!(font_family::KEY, "markdown.fontFamily");
    }

    #[test]
    fn page_zoom_scales_and_clamps() {
        assert!((font_size::page_zoom(15.0) - 1.0).abs() < 1e-9);
        assert!((font_size::page_zoom(30.0) - 2.0).abs() < 1e-9);
        assert!((font_size::page_zoom(96.0) - 6.4).abs() < 1e-9);
        // Input is clamped first: 200 -> 96 -> 96/15.
        assert!((font_size::page_zoom(200.0) - (96.0 / 15.0)).abs() < 1e-9);
    }

    #[test]
    fn resolved_default_behaviour() {
        assert_eq!(font_size::resolved_default(None), 15.0);
        assert_eq!(font_size::resolved_default(Some(4.0)), 8.0);
        assert_eq!(font_size::resolved_default(Some(20.0)), 20.0);
        assert_eq!(max_width::resolved_default(None), 980.0);
        assert_eq!(max_width::resolved_default(Some(10_000.0)), 2400.0);
    }

    #[test]
    fn store_value_rounds_then_clamps() {
        assert_eq!(font_size::store_value(15.6), 16);
        assert_eq!(font_size::store_value(8.4), 8);
        assert_eq!(font_size::store_value(3.2), 8); // clamped up first
        assert_eq!(max_width::store_value(980.4), 980);
    }

    #[test]
    fn font_family_normalized_collapses_newlines() {
        assert_eq!(font_family::normalized("  Comic Sans  "), "Comic Sans");
        assert_eq!(font_family::normalized("a\r\nb"), "a  b");
        assert_eq!(font_family::normalized("a\nb"), "a b");
        assert_eq!(font_family::normalized("   "), "");
    }

    #[test]
    fn font_family_css_value_escaping() {
        assert_eq!(font_family::css_value(""), None);
        assert_eq!(font_family::css_value("   "), None);
        assert_eq!(font_family::css_value("Comic Sans").as_deref(), Some("\"Comic Sans\""));
        assert_eq!(
            font_family::css_value("say \"hi\"").as_deref(),
            Some("\"say \\\"hi\\\"\"")
        );
        assert_eq!(
            font_family::css_value("back\\slash").as_deref(),
            Some("\"back\\\\slash\"")
        );
    }

    #[test]
    fn font_family_store_value() {
        assert_eq!(font_family::store_value(""), None);
        assert_eq!(font_family::store_value("  "), None);
        assert_eq!(font_family::store_value("  Inter  ").as_deref(), Some("Inter"));
    }

    #[test]
    fn typography_apply_and_reset() {
        let applied = MarkdownTypography::apply(15.6, "  Inter  ", 100.0);
        assert_eq!(applied.font_size, 16);
        assert_eq!(applied.font_family, "Inter");
        assert_eq!(applied.max_width, 320);

        let reset = MarkdownTypography::reset_to_builtin();
        assert_eq!(reset, MarkdownTypography { font_size: 15, font_family: String::new(), max_width: 980 });

        // Empty family in apply() collapses to the System default.
        let no_font = MarkdownTypography::apply(15.0, "  ", 980.0);
        assert_eq!(no_font.font_family, "");
    }

    #[test]
    fn defaults_stay_in_sync_with_cmux_config() {
        // Guards against numeric drift between this domain layer and the serde
        // persistence model in cmux-config (and, transitively, cmux.schema.json).
        let cfg = cmux_config::MarkdownConfig::default();
        let ours = MarkdownTypography::default();
        assert_eq!(ours.font_size, cfg.font_size);
        assert_eq!(ours.font_family, cfg.font_family);
        assert_eq!(ours.max_width, cfg.max_width);
    }
}
