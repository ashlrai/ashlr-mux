//! Pure dock/taskbar badge-label formatting.
//!
//! Ported from `cmux/Sources/TerminalNotificationStore.swift`:
//! `dockBadgeLabel` (536-553) and `TaggedRunBadgeSettings.normalizedTag`
//! (69-86, with the 10-character clamp).
//!
//! Only the label *formatter* is in scope. Writing the label onto the real Dock
//! tile / Windows taskbar overlay is deferred to the M10 GUI track.

/// Maximum length of a normalized run tag (Swift `maxTagLength`).
const MAX_TAG_LENGTH: usize = 10;

/// Environment variable carrying the run tag (Swift
/// `TaggedRunBadgeSettings.environmentKey`).
pub const RUN_TAG_ENVIRONMENT_KEY: &str = "CMUX_TAG";

/// Trim, empty-check, and clamp a raw run tag to [`MAX_TAG_LENGTH`] characters.
///
/// Verbatim port of Swift `TaggedRunBadgeSettings.normalizedTag(_:)`
/// (`TerminalNotificationStore.swift` 77-85): trims whitespace + newlines;
/// empty → `None`; otherwise clamps to the first 10 characters.
pub fn normalized_tag(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim_matches(|c: char| c.is_whitespace());
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(MAX_TAG_LENGTH).collect())
}

/// Compute the dock/taskbar badge label for the given unread count and tag.
///
/// Verbatim port of Swift `TerminalNotificationStore.dockBadgeLabel`
/// (`TerminalNotificationStore.swift` 536-553):
/// - the unread label is `nil` unless badges are enabled and the count is > 0;
///   counts above 99 clamp to `"99+"`.
/// - with a normalized tag present, the label becomes `"<tag>:<unread>"` (or
///   just `"<tag>"` when there is no unread label).
/// - with no tag, the result is just the unread label (possibly `None`).
pub fn dock_badge_label(
    unread_count: usize,
    is_enabled: bool,
    run_tag: Option<&str>,
) -> Option<String> {
    let unread_label: Option<String> = if is_enabled && unread_count > 0 {
        Some(if unread_count > 99 {
            "99+".to_string()
        } else {
            unread_count.to_string()
        })
    } else {
        None
    };

    if let Some(tag) = normalized_tag(run_tag) {
        return Some(match unread_label {
            Some(label) => format!("{tag}:{label}"),
            None => tag,
        });
    }

    unread_label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_label_only_when_enabled_and_positive() {
        assert_eq!(dock_badge_label(0, true, None), None);
        assert_eq!(dock_badge_label(3, false, None), None);
        assert_eq!(dock_badge_label(3, true, None), Some("3".to_string()));
    }

    #[test]
    fn clamps_above_ninety_nine() {
        assert_eq!(dock_badge_label(99, true, None), Some("99".to_string()));
        assert_eq!(dock_badge_label(100, true, None), Some("99+".to_string()));
        assert_eq!(dock_badge_label(5000, true, None), Some("99+".to_string()));
    }

    #[test]
    fn tag_combines_with_unread_label() {
        assert_eq!(
            dock_badge_label(4, true, Some("mytag")),
            Some("mytag:4".to_string())
        );
        // tag present, no unread → just the tag
        assert_eq!(
            dock_badge_label(0, true, Some("mytag")),
            Some("mytag".to_string())
        );
        // tag present, badges disabled → just the tag
        assert_eq!(
            dock_badge_label(4, false, Some("mytag")),
            Some("mytag".to_string())
        );
        // tag present, 99+ unread
        assert_eq!(
            dock_badge_label(150, true, Some("mytag")),
            Some("mytag:99+".to_string())
        );
    }

    #[test]
    fn normalized_tag_trims_clamps_and_rejects_empty() {
        assert_eq!(normalized_tag(None), None);
        assert_eq!(normalized_tag(Some("   ")), None);
        assert_eq!(normalized_tag(Some("\n  tag \n")), Some("tag".to_string()));
        // 10-char clamp
        assert_eq!(
            normalized_tag(Some("0123456789abcdef")),
            Some("0123456789".to_string())
        );
    }

    #[test]
    fn tag_clamp_applies_inside_badge_label() {
        assert_eq!(
            dock_badge_label(2, true, Some("0123456789abc")),
            Some("0123456789:2".to_string())
        );
    }
}
