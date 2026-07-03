//! The menu-bar notification snapshot and line/badge formatters.
//!
//! Ported from `cmux/Sources/App/MenuBarExtraController.swift` 312-368
//! (`NotificationMenuSnapshot` + `NotificationMenuSnapshotBuilder` +
//! `MenuBarBadgeLabelFormatter`) and 374-390
//! (`MenuBarNotificationLineFormatter.plainTitle`).
//!
//! Excluded (GUI seam): the `AttributedString` / `NSLayoutManager` wrapping and
//! truncation in `menuTitle(...)`, the `NSMenu` item construction, and dock-tile
//! writes. Only the pure snapshot + plain-text line + badge string are here.

use super::TerminalNotification;

/// Default number of notifications shown inline in the menu.
///
/// Verbatim from Swift `defaultInlineNotificationLimit`
/// (`MenuBarExtraController.swift` 327).
pub const DEFAULT_INLINE_NOTIFICATION_LIMIT: usize = 6;

/// Immutable projection the menu bar renders.
///
/// Verbatim port of Swift `NotificationMenuSnapshot`
/// (`MenuBarExtraController.swift` 312-324).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationMenuSnapshot {
    pub unread_count: usize,
    pub has_notifications: bool,
    pub recent_notifications: Vec<TerminalNotification>,
}

impl NotificationMenuSnapshot {
    /// Verbatim port of Swift `hasUnreadNotifications`
    /// (`MenuBarExtraController.swift` 317-319).
    pub fn has_unread_notifications(&self) -> bool {
        self.unread_count > 0
    }
}

/// Build the menu snapshot from the current notifications and workspace unread
/// indicator count.
///
/// Verbatim port of Swift `NotificationMenuSnapshotBuilder.make(...)`
/// (`MenuBarExtraController.swift` 329-346). `max_inline` is the caller's inline
/// cap ([`DEFAULT_INLINE_NOTIFICATION_LIMIT`] in the app).
pub fn make(
    notifications: &[TerminalNotification],
    workspace_unread_indicator_count: usize,
    max_inline: usize,
) -> NotificationMenuSnapshot {
    let unread_count = notifications.iter().filter(|n| !n.is_read).count()
        + workspace_unread_indicator_count;

    NotificationMenuSnapshot {
        unread_count,
        has_notifications: !notifications.is_empty() || workspace_unread_indicator_count > 0,
        recent_notifications: notifications.iter().take(max_inline).cloned().collect(),
    }
}

/// The pluralization bucket for the "N unread notifications" state hint.
///
/// Swift `stateHintTitle(unreadCount:)` (`MenuBarExtraController.swift`
/// 348-357) returns a localized string; per lane spec this returns the bucket
/// and leaves the localized rendering to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateHintKind {
    /// No unread notifications.
    Zero,
    /// Exactly one unread notification.
    One,
    /// Two or more unread notifications.
    Other,
}

/// Classify the unread count into its pluralization bucket.
///
/// Mirrors the `switch` in Swift `stateHintTitle(unreadCount:)`.
pub fn state_hint_kind(unread_count: usize) -> StateHintKind {
    match unread_count {
        0 => StateHintKind::Zero,
        1 => StateHintKind::One,
        _ => StateHintKind::Other,
    }
}

/// The menu-bar icon badge text: `None` when there is nothing unread, `"9+"`
/// above 9, else the count.
///
/// Verbatim port of Swift `MenuBarBadgeLabelFormatter.badgeText(for:)`
/// (`MenuBarExtraController.swift` 360-367).
pub fn badge_text(unread_count: usize) -> Option<String> {
    if unread_count == 0 {
        return None;
    }
    if unread_count > 9 {
        Some("9+".to_string())
    } else {
        Some(unread_count.to_string())
    }
}

/// The plain-text menu line for a notification: an unread dot + title, then the
/// body-or-subtitle, then the tab title, joined by newlines.
///
/// Ported from Swift `MenuBarNotificationLineFormatter.plainTitle(...)`
/// (`MenuBarExtraController.swift` 374-390).
///
/// DIVERGENCE: Swift appends a locale-formatted `.shortened` time to the first
/// line (`notification.createdAt.formatted(...)`). That is non-deterministic
/// (locale + time-zone dependent) and belongs to the GUI seam, so the time is
/// omitted here; the host may re-append a formatted time. Every other part of
/// the line — the dot, title, detail, and tab title — is byte-identical to
/// Swift, so the oracle's `hasPrefix` / `contains` assertions hold.
pub fn plain_title(notification: &TerminalNotification, tab_title: Option<&str>) -> String {
    let dot = if notification.is_read { "  " } else { "● " };
    let mut lines: Vec<String> = vec![format!("{dot}{}", notification.title)];

    let detail = if notification.body.is_empty() {
        &notification.subtitle
    } else {
        &notification.body
    };
    if !detail.is_empty() {
        lines.push(detail.clone());
    }

    if let Some(tab_title) = tab_title {
        if !tab_title.is_empty() {
            lines.push(tab_title.to_string());
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: &str, created_at: i64, is_read: bool) -> TerminalNotification {
        TerminalNotification {
            id: id.to_owned(),
            tab_id: "tab".into(),
            surface_id: None,
            panel_id: None,
            title: format!("N-{id}"),
            subtitle: String::new(),
            body: String::new(),
            created_at,
            is_read,
            pane_flash: true,
            click_action: None,
        }
    }

    /// Oracle: `testSnapshotCountsUnreadAndLimitsRecentItems`.
    #[test]
    fn snapshot_counts_unread_and_limits_recent() {
        let notifications: Vec<TerminalNotification> = (0..8)
            .map(|i| notification(&i.to_string(), i, i % 2 == 0))
            .collect();

        let snapshot = make(&notifications, 0, 3);
        assert_eq!(snapshot.unread_count, 4);
        assert!(snapshot.has_notifications);
        assert!(snapshot.has_unread_notifications());
        assert_eq!(snapshot.recent_notifications.len(), 3);
        let ids: Vec<&str> = snapshot
            .recent_notifications
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(ids, vec!["0", "1", "2"]);
    }

    /// Oracle: `testSnapshotCountsWorkspaceUnreadIndicatorsWithoutNotificationRecords`.
    #[test]
    fn snapshot_counts_workspace_indicators_without_records() {
        let snapshot = make(&[], 2, DEFAULT_INLINE_NOTIFICATION_LIMIT);
        assert_eq!(snapshot.unread_count, 2);
        assert!(snapshot.has_notifications);
        assert!(snapshot.has_unread_notifications());
        assert!(snapshot.recent_notifications.is_empty());
    }

    #[test]
    fn empty_snapshot_has_nothing() {
        let snapshot = make(&[], 0, DEFAULT_INLINE_NOTIFICATION_LIMIT);
        assert_eq!(snapshot.unread_count, 0);
        assert!(!snapshot.has_notifications);
        assert!(!snapshot.has_unread_notifications());
    }

    /// Oracle: `testStateHintTitleHandlesSingularPluralAndZero` (bucket only).
    #[test]
    fn state_hint_kind_buckets() {
        assert_eq!(state_hint_kind(0), StateHintKind::Zero);
        assert_eq!(state_hint_kind(1), StateHintKind::One);
        assert_eq!(state_hint_kind(2), StateHintKind::Other);
        assert_eq!(state_hint_kind(47), StateHintKind::Other);
    }

    /// Oracle: `MenuBarBadgeLabelFormatterTests`.
    #[test]
    fn badge_text_matches_swift() {
        assert_eq!(badge_text(0), None);
        assert_eq!(badge_text(1), Some("1".to_string()));
        assert_eq!(badge_text(9), Some("9".to_string()));
        assert_eq!(badge_text(10), Some("9+".to_string()));
        assert_eq!(badge_text(47), Some("9+".to_string()));
    }

    /// Oracle: `testPlainTitleContainsUnreadDotBodyAndTab`.
    #[test]
    fn plain_title_unread_dot_body_and_tab() {
        let mut n = notification("a", 0, false);
        n.title = "Build finished".into();
        n.body = "All checks passed".into();
        let line = plain_title(&n, Some("workspace-1"));
        assert!(line.starts_with("● Build finished"));
        assert!(line.contains("All checks passed"));
        assert!(line.contains("workspace-1"));
    }

    /// Oracle: `testPlainTitleFallsBackToSubtitleWhenBodyEmpty`.
    #[test]
    fn plain_title_falls_back_to_subtitle_when_body_empty() {
        let mut n = notification("a", 0, true);
        n.title = "Deploy".into();
        n.subtitle = "staging".into();
        n.body = String::new();
        let line = plain_title(&n, None);
        assert!(line.starts_with("  Deploy"));
        assert!(line.contains("staging"));
    }

    #[test]
    fn plain_title_omits_empty_detail_and_tab() {
        let mut n = notification("a", 0, false);
        n.title = "Only title".into();
        n.subtitle = String::new();
        n.body = String::new();
        assert_eq!(plain_title(&n, None), "● Only title");
        assert_eq!(plain_title(&n, Some("")), "● Only title");
    }
}
