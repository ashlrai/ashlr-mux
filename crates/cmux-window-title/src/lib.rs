//! cmux-window-title — window-title template resolver.
//!
//! Headless port of the pure core of the canonical macOS `WindowTitleTemplate`
//! (`Sources/App/WindowTitleTemplate.swift`) and its context value type
//! (`Sources/App/WindowTitleTemplateContext.swift`).
//!
//! Ported (100% pure):
//! - [`WindowTitleTemplateContext`] — the 5-field substitution context
//!   (`WindowTitleTemplateContext.swift:3-9`).
//! - [`WindowTitleTemplate::resolved`] — the single-pass `{key}` scanner
//!   (`WindowTitleTemplate.swift:17-39`). It substitutes each recognized
//!   placeholder exactly once, does **not** recursively expand substituted
//!   values, and leaves unknown placeholders verbatim (braces included).
//! - the 6 replacement mappings (`WindowTitleTemplate.swift:41-50`) and the
//!   8-char [`WindowTitleTemplate::window_token`] (`:52-54`).
//! - [`WindowTitleTemplate::configured_from_raw`] — a pure re-expression of the
//!   trim/disable rule inside `configured(defaults:)`
//!   (`WindowTitleTemplate.swift:9-15`): a blank (whitespace-only) raw value is
//!   treated as disabled (`None`).
//!
//! **Left out:** the `UserDefaults` read in `configured(defaults:)`
//! (`WindowTitleTemplate.swift:9-15`) — the disable predicate is reimplemented
//! here as a pure function over the raw string; the caller supplies the string.

use unicode_segmentation::UnicodeSegmentation;
use uuid::Uuid;

/// The context supplied when resolving a [`WindowTitleTemplate`].
///
/// Ports `WindowTitleTemplateContext` (`WindowTitleTemplateContext.swift:3-9`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowTitleTemplateContext {
    /// The fallback title (`{defaultTitle}`).
    pub default_title: String,
    /// The active workspace name (`{activeWorkspace}`).
    pub active_workspace: String,
    /// The active working directory (`{activeDirectory}`).
    pub active_directory: String,
    /// The window identifier, source of `{windowId}` and `{windowToken}`.
    pub window_id: Uuid,
    /// The application name (`{appName}`).
    pub app_name: String,
}

/// A window-title template: a raw string with `{placeholder}` slots.
///
/// Ports the pure core of `WindowTitleTemplate`
/// (`WindowTitleTemplate.swift:3-55`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowTitleTemplate {
    /// The raw template string.
    pub raw_value: String,
}

impl WindowTitleTemplate {
    /// The `UserDefaults` key under which the raw template is stored.
    ///
    /// Ports `WindowTitleTemplate.userDefaultsKey`
    /// (`WindowTitleTemplate.swift:4`). Retained for parity with the canonical
    /// settings layer; the `UserDefaults` read itself is out of scope.
    pub const USER_DEFAULTS_KEY: &'static str = "windowTitleTemplate";

    /// The default raw value (empty ⇒ disabled).
    ///
    /// Ports `WindowTitleTemplate.defaultRawValue`
    /// (`WindowTitleTemplate.swift:5`).
    pub const DEFAULT_RAW_VALUE: &'static str = "";

    /// Constructs a template from a raw string.
    ///
    /// Mirrors the Swift memberwise initializer `WindowTitleTemplate(rawValue:)`.
    pub fn new(raw_value: impl Into<String>) -> Self {
        Self {
            raw_value: raw_value.into(),
        }
    }

    /// Returns the template if the raw value is non-blank, else `None`.
    ///
    /// Pure re-expression of the trim/disable rule inside `configured(defaults:)`
    /// (`WindowTitleTemplate.swift:9-15`): the raw value is disabled when it is
    /// empty after trimming whitespace and newlines. The Swift code uses
    /// `trimmingCharacters(in: .whitespacesAndNewlines)`; [`char::is_whitespace`]
    /// (the Unicode `White_Space` property) covers the same code points
    /// (space, tab, U+000A–U+000D, U+0085, U+2028, U+2029, and the `Zs`
    /// category). The `UserDefaults` read is the caller's responsibility.
    pub fn configured_from_raw(raw_value: &str) -> Option<Self> {
        if raw_value.trim().is_empty() {
            None
        } else {
            Some(Self::new(raw_value))
        }
    }

    /// Resolves the template against `context`.
    ///
    /// Single-pass `{key}` scanner ported verbatim from
    /// `WindowTitleTemplate.resolved(context:)`
    /// (`WindowTitleTemplate.swift:17-39`):
    ///
    /// - A `{` with a following `}` delimits a placeholder; the text between is
    ///   the key. A recognized key is replaced by its value; an unrecognized key
    ///   is emitted **verbatim, braces included** (`:33-35`).
    /// - Substituted values are **not** rescanned, so placeholder-looking text
    ///   inside a replacement value is left literal (`:31-32` advances past the
    ///   close brace; the loop never re-reads the appended replacement).
    /// - A `{` with no following `}` is emitted as a literal `{` (`:22-27`,
    ///   the `guard`'s else branch).
    ///
    /// **Unicode granularity:** Swift iterates `rawValue` by `Character`
    /// (extended grapheme cluster), and both `rawValue[index] == "{"` and
    /// `firstIndex(of: "}")` compare whole `Character`s. So a brace fused with a
    /// following combining mark — e.g. `}` + U+0301, the grapheme `"}\u{0301}"` —
    /// is *not* the `Character` `"}"` and cannot open or close a placeholder. To
    /// preserve that behavior this scanner iterates by extended grapheme cluster
    /// (via [`UnicodeSegmentation::graphemes`] with `is_extended = true`, the
    /// UAX #29 rules Swift's `Character` follows) and compares each cluster
    /// against the whole strings `"{"` / `"}"`. A scalar-level scan would find
    /// the lone `}` scalar inside such a cluster and substitute where Swift does
    /// not (see the `combining_mark_*` oracle tests).
    pub fn resolved(&self, context: &WindowTitleTemplateContext) -> String {
        let replacements = self.replacements(context);
        let clusters: Vec<&str> = self.raw_value.graphemes(true).collect();
        let mut resolved = String::new();
        let mut index = 0;

        while index < clusters.len() {
            // guard rawValue[index] == "{", let closeIndex = firstIndex(of: "}")
            if clusters[index] != "{" {
                resolved.push_str(clusters[index]);
                index += 1;
                continue;
            }
            let Some(offset) = clusters[index..].iter().position(|&c| c == "}") else {
                resolved.push_str(clusters[index]);
                index += 1;
                continue;
            };
            let close_index = index + offset;

            let placeholder: String = clusters[index + 1..close_index].concat();
            match replacements.iter().find(|(key, _)| *key == placeholder) {
                Some((_, replacement)) => resolved.push_str(replacement),
                None => resolved.push_str(&clusters[index..=close_index].concat()),
            }
            index = close_index + 1;
        }

        resolved
    }

    /// The ordered `(key, value)` replacement mappings.
    ///
    /// Ports `WindowTitleTemplate.replacements(context:)`
    /// (`WindowTitleTemplate.swift:41-50`). `{windowId}` is the lowercased
    /// hyphenated UUID (`:43`); `{windowToken}` is its 8-char prefix (`:44`).
    fn replacements(&self, context: &WindowTitleTemplateContext) -> Vec<(&'static str, String)> {
        vec![
            ("windowId", context.window_id.hyphenated().to_string()),
            ("windowToken", Self::window_token(context.window_id)),
            ("activeWorkspace", context.active_workspace.clone()),
            ("activeDirectory", context.active_directory.clone()),
            ("defaultTitle", context.default_title.clone()),
            ("appName", context.app_name.clone()),
        ]
    }

    /// The 8-character lowercase window token derived from `window_id`.
    ///
    /// Ports `WindowTitleTemplate.windowToken(for:)`
    /// (`WindowTitleTemplate.swift:52-54`): `String(uuidString.prefix(8))`
    /// lowercased. The hyphenated UUID's first 8 characters are the leading
    /// hex group (no hyphen falls within the first 8), so this is the first 8
    /// hex digits, lowercased. The `uuid` crate already renders lowercase hex,
    /// matching Swift's uppercase `uuidString` followed by `.lowercased()`.
    pub fn window_token(window_id: Uuid) -> String {
        window_id
            .hyphenated()
            .to_string()
            .chars()
            .take(8)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed UUID used by the Swift oracle
    /// (`WindowTitleTemplateTests.swift:18`, `:42`).
    fn oracle_window_id() -> Uuid {
        Uuid::parse_str("01234567-89AB-CDEF-0123-456789ABCDEF").expect("valid uuid")
    }

    /// Ports `resolvesWindowPlaceholdersAndPreservesUnknownPlaceholders`
    /// (`WindowTitleTemplateTests.swift:17-32`).
    #[test]
    fn resolves_window_placeholders_and_preserves_unknown_placeholders() {
        let template = WindowTitleTemplate::new(
            "[cmux:{windowToken}] {activeWorkspace} {activeDirectory} {windowId} {defaultTitle} {appName} {unknown}",
        );

        let resolved = template.resolved(&WindowTitleTemplateContext {
            default_title: "Fallback".to_string(),
            active_workspace: "Build".to_string(),
            active_directory: "/tmp/project".to_string(),
            window_id: oracle_window_id(),
            app_name: "cmux".to_string(),
        });

        assert_eq!(
            resolved,
            "[cmux:01234567] Build /tmp/project 01234567-89ab-cdef-0123-456789abcdef Fallback cmux {unknown}"
        );
    }

    /// Ports `configuredTemplateTreatsBlankDefaultsValueAsDisabled`
    /// (`WindowTitleTemplateTests.swift:34-39`). The `UserDefaults` write is
    /// replaced by passing the raw string directly.
    #[test]
    fn configured_template_treats_blank_value_as_disabled() {
        assert_eq!(WindowTitleTemplate::configured_from_raw("   \n"), None);
    }

    /// Ports `resolverDoesNotExpandPlaceholdersInsideReplacementValues`
    /// (`WindowTitleTemplateTests.swift:41-54`).
    #[test]
    fn resolver_does_not_expand_placeholders_inside_replacement_values() {
        let template = WindowTitleTemplate::new("{activeWorkspace} {appName}");

        let resolved = template.resolved(&WindowTitleTemplateContext {
            default_title: "Fallback".to_string(),
            active_workspace: "{windowId}".to_string(),
            active_directory: "{windowToken}".to_string(),
            window_id: oracle_window_id(),
            app_name: "cmux".to_string(),
        });

        assert_eq!(resolved, "{windowId} cmux");
    }

    // --- Edge cases called out by the port notes (non-recursive substitution
    // + verbatim-unknown-placeholder behavior). ---

    /// A non-blank template is retained by the disable predicate.
    #[test]
    fn configured_template_retains_non_blank_value() {
        assert_eq!(
            WindowTitleTemplate::configured_from_raw("  {appName}  "),
            Some(WindowTitleTemplate::new("  {appName}  "))
        );
    }

    /// The empty default raw value is disabled.
    #[test]
    fn configured_template_treats_empty_value_as_disabled() {
        assert_eq!(
            WindowTitleTemplate::configured_from_raw(WindowTitleTemplate::DEFAULT_RAW_VALUE),
            None
        );
    }

    /// A `{` with no closing `}` is emitted literally (guard else branch,
    /// `WindowTitleTemplate.swift:22-27`).
    #[test]
    fn unclosed_brace_is_literal() {
        let template = WindowTitleTemplate::new("a {appName b");
        let resolved = template.resolved(&WindowTitleTemplateContext {
            default_title: String::new(),
            active_workspace: String::new(),
            active_directory: String::new(),
            window_id: oracle_window_id(),
            app_name: "cmux".to_string(),
        });
        assert_eq!(resolved, "a {appName b");
    }

    /// An empty placeholder `{}` matches no key and is preserved verbatim.
    #[test]
    fn empty_placeholder_is_preserved() {
        let template = WindowTitleTemplate::new("x{}y");
        let resolved = template.resolved(&WindowTitleTemplateContext {
            default_title: String::new(),
            active_workspace: String::new(),
            active_directory: String::new(),
            window_id: oracle_window_id(),
            app_name: "cmux".to_string(),
        });
        assert_eq!(resolved, "x{}y");
    }

    /// A combining mark fused to the closing brace makes the brace part of a
    /// different grapheme cluster, so Swift's `Character`-based
    /// `firstIndex(of: "}")` finds no close brace: the whole template is emitted
    /// literally, unresolved. Pins parity for the exact divergent input from the
    /// grapheme-vs-scalar review finding: `"{appName}"` followed by U+0301
    /// (combining acute accent). A scalar scan would find the `}` scalar at
    /// index 8, match `appName`, and substitute `cmux` — Swift does not.
    #[test]
    fn combining_mark_on_close_brace_leaves_template_literal() {
        let template = WindowTitleTemplate::new("{appName}\u{0301}");
        let resolved = template.resolved(&WindowTitleTemplateContext {
            default_title: String::new(),
            active_workspace: String::new(),
            active_directory: String::new(),
            window_id: oracle_window_id(),
            app_name: "cmux".to_string(),
        });
        // Swift: guard's `firstIndex(of: "}")` is nil (the only `}` is fused into
        // the grapheme "}\u{0301}"), so every Character is appended verbatim.
        assert_eq!(resolved, "{appName}\u{0301}");
    }

    /// Sanity companion: without the combining mark the placeholder resolves
    /// normally, proving the literal-emission above is caused by the fused
    /// grapheme and not by some unrelated parse failure.
    #[test]
    fn plain_close_brace_resolves_placeholder() {
        let template = WindowTitleTemplate::new("{appName}");
        let resolved = template.resolved(&WindowTitleTemplateContext {
            default_title: String::new(),
            active_workspace: String::new(),
            active_directory: String::new(),
            window_id: oracle_window_id(),
            app_name: "cmux".to_string(),
        });
        assert_eq!(resolved, "cmux");
    }

    /// The window token is the lowercased 8-char hex prefix
    /// (`WindowTitleTemplate.swift:52-54`).
    #[test]
    fn window_token_is_lowercase_eight_char_prefix() {
        assert_eq!(
            WindowTitleTemplate::window_token(oracle_window_id()),
            "01234567"
        );
    }
}
