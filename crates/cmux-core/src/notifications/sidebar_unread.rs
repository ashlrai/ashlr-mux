//! The coalesced per-workspace unread projection the sidebar observes.
//!
//! Ported from `cmux/Sources/TerminalNotificationStore.swift` 595-615
//! (`buildSidebarUnreadSummaries`) and 2148-2242
//! (`SidebarWorkspaceUnreadSummary`, `SidebarSurfaceUnreadKey`,
//! `SidebarUnreadModel`).
//!
//! DIVERGENCE: Swift's `SidebarUnreadModel` is a `@MainActor` `ObservableObject`
//! that republishes via `@Published` under equality guards. Here it is a plain
//! struct; [`SidebarUnreadModel::apply`] returns the per-field change flags
//! ([`SidebarApplyChanges`]) the host uses to drive whatever publish mechanism
//! replaces `objectWillChange`. Ids are `String` (Swift uses `UUID`).

use super::TerminalNotification;
use std::collections::{HashMap, HashSet};

/// Immutable per-workspace unread projection rendered by the sidebar.
///
/// Verbatim port of Swift `SidebarWorkspaceUnreadSummary`
/// (`TerminalNotificationStore.swift` 2154-2157). `latest_notification_text` is
/// the trimmed body-or-title of the latest notification (read or unread).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SidebarWorkspaceUnreadSummary {
    pub unread_count: usize,
    pub latest_notification_text: Option<String>,
}

/// Workspace + surface pair mirroring the store's per-surface unread set.
///
/// Verbatim port of Swift `SidebarSurfaceUnreadKey`
/// (`TerminalNotificationStore.swift` 2160-2163).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SidebarSurfaceUnreadKey {
    pub workspace_id: String,
    pub surface_id: Option<String>,
}

/// Trim leading/trailing ASCII+Unicode whitespace (Swift `.whitespacesAndNewlines`).
fn trimmed_non_empty(text: &str) -> Option<String> {
    let trimmed = text.trim_matches(|c: char| c.is_whitespace());
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Build the per-workspace unread summaries the sidebar renders.
///
/// Verbatim port of Swift `buildSidebarUnreadSummaries()`
/// (`TerminalNotificationStore.swift` 595-615). Only workspaces with a
/// non-default summary are included; absent entries resolve to `(0, None)` via
/// [`SidebarUnreadModel::summary`].
///
/// The `NotificationStore` indexes are private, so this pure function takes the
/// derived inputs directly (the store composes it, exactly as the existing port
/// split the badge/coalescer/sound helpers out):
/// - `unread_count_by_tab_id`: per-tab unread notification count (Swift
///   `indexes.unreadCountByTabId`).
/// - `latest_by_tab_id`: latest notification per tab, read or unread (Swift
///   `indexes.latestByTabId`).
/// - `workspace_unread_indicator_ids`: the union of manual/panel-derived/
///   restored workspace indicators (Swift `workspaceUnreadIndicatorIds`); a
///   member adds 1 to that tab's count, mirroring `unreadCount(forTabId:)`.
pub fn build_sidebar_unread_summaries(
    unread_count_by_tab_id: &HashMap<String, usize>,
    latest_by_tab_id: &HashMap<String, TerminalNotification>,
    workspace_unread_indicator_ids: &HashSet<String>,
) -> HashMap<String, SidebarWorkspaceUnreadSummary> {
    let mut ids: HashSet<&String> = HashSet::new();
    ids.extend(unread_count_by_tab_id.keys());
    ids.extend(latest_by_tab_id.keys());
    ids.extend(workspace_unread_indicator_ids.iter());

    let mut result: HashMap<String, SidebarWorkspaceUnreadSummary> = HashMap::new();
    for id in ids {
        let has_indicator = workspace_unread_indicator_ids.contains(id);
        let count = unread_count_by_tab_id.get(id).copied().unwrap_or(0) + usize::from(has_indicator);
        let latest_text = latest_by_tab_id.get(id).and_then(|notification| {
            let text = if notification.body.is_empty() {
                &notification.title
            } else {
                &notification.body
            };
            trimmed_non_empty(text)
        });
        if count == 0 && latest_text.is_none() {
            continue;
        }
        result.insert(
            id.clone(),
            SidebarWorkspaceUnreadSummary {
                unread_count: count,
                latest_notification_text: latest_text,
            },
        );
    }
    result
}

/// Which of the five coalesced fields changed on an [`SidebarUnreadModel::apply`].
///
/// Replaces Swift's implicit `@Published` republish: each flag is `true` iff the
/// corresponding field differed from its prior value (equality guard).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SidebarApplyChanges {
    pub total_unread_count: bool,
    pub summaries: bool,
    pub unread_surface_keys: bool,
    pub focused_read_indicator: bool,
    pub manual_unread: bool,
}

impl SidebarApplyChanges {
    /// Whether any field changed (the host publishes iff so).
    pub fn any(&self) -> bool {
        self.total_unread_count
            || self.summaries
            || self.unread_surface_keys
            || self.focused_read_indicator
            || self.manual_unread
    }
}

/// The coalesced unread model the workspace sidebar observes instead of the
/// full `NotificationStore`.
///
/// Port of Swift `SidebarUnreadModel` (`TerminalNotificationStore.swift`
/// 2174-2242). Query methods mirror the equivalent `NotificationStore` reads
/// exactly so callers can switch source without behavior change.
#[derive(Debug, Clone, Default)]
pub struct SidebarUnreadModel {
    total_unread_count: usize,
    summary_by_workspace_id: HashMap<String, SidebarWorkspaceUnreadSummary>,
    unread_surface_keys: HashSet<SidebarSurfaceUnreadKey>,
    focused_read_indicator_by_workspace_id: HashMap<String, String>,
    manual_unread_workspace_ids: HashSet<String>,
}

impl SidebarUnreadModel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply a fresh coalesced snapshot, updating only the fields that changed.
    ///
    /// Verbatim port of Swift `apply(...)` (`TerminalNotificationStore.swift`
    /// 2182-2204), returning the per-field change flags instead of publishing.
    pub fn apply(
        &mut self,
        total_unread_count: usize,
        summaries: HashMap<String, SidebarWorkspaceUnreadSummary>,
        unread_surface_keys: HashSet<SidebarSurfaceUnreadKey>,
        focused_read_indicator_by_workspace_id: HashMap<String, String>,
        manual_unread_workspace_ids: HashSet<String>,
    ) -> SidebarApplyChanges {
        let mut changes = SidebarApplyChanges::default();

        if self.total_unread_count != total_unread_count {
            self.total_unread_count = total_unread_count;
            changes.total_unread_count = true;
        }
        if self.summary_by_workspace_id != summaries {
            self.summary_by_workspace_id = summaries;
            changes.summaries = true;
        }
        if self.unread_surface_keys != unread_surface_keys {
            self.unread_surface_keys = unread_surface_keys;
            changes.unread_surface_keys = true;
        }
        if self.focused_read_indicator_by_workspace_id != focused_read_indicator_by_workspace_id {
            self.focused_read_indicator_by_workspace_id = focused_read_indicator_by_workspace_id;
            changes.focused_read_indicator = true;
        }
        if self.manual_unread_workspace_ids != manual_unread_workspace_ids {
            self.manual_unread_workspace_ids = manual_unread_workspace_ids;
            changes.manual_unread = true;
        }
        changes
    }

    /// The current total unread count (Swift `totalUnreadCount`).
    pub fn total_unread_count(&self) -> usize {
        self.total_unread_count
    }

    /// Verbatim port of Swift `summary(forWorkspaceId:)`
    /// (`TerminalNotificationStore.swift` 2206-2208): absent → default `(0, None)`.
    pub fn summary(&self, workspace_id: &str) -> SidebarWorkspaceUnreadSummary {
        self.summary_by_workspace_id
            .get(workspace_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Verbatim port of Swift `unreadCount(forWorkspaceId:)`.
    pub fn unread_count(&self, workspace_id: &str) -> usize {
        self.summary(workspace_id).unread_count
    }

    /// Verbatim port of Swift `latestNotificationText(forWorkspaceId:)`.
    pub fn latest_notification_text(&self, workspace_id: &str) -> Option<String> {
        self.summary(workspace_id).latest_notification_text
    }

    /// Verbatim port of Swift `workspaceIsUnread(forWorkspaceId:)`.
    pub fn workspace_is_unread(&self, workspace_id: &str) -> bool {
        self.unread_count(workspace_id) > 0
    }

    /// Verbatim port of Swift `hasManualUnread(forWorkspaceId:)`.
    pub fn has_manual_unread(&self, workspace_id: &str) -> bool {
        self.manual_unread_workspace_ids.contains(workspace_id)
    }

    /// Verbatim port of Swift `hasUnreadNotification(forWorkspaceId:surfaceId:)`.
    pub fn has_unread_notification(&self, workspace_id: &str, surface_id: Option<&str>) -> bool {
        self.unread_surface_keys.contains(&SidebarSurfaceUnreadKey {
            workspace_id: workspace_id.to_string(),
            surface_id: surface_id.map(str::to_string),
        })
    }

    /// Verbatim port of Swift `hasVisibleNotificationIndicator(forWorkspaceId:surfaceId:)`.
    pub fn has_visible_notification_indicator(
        &self,
        workspace_id: &str,
        surface_id: Option<&str>,
    ) -> bool {
        self.has_unread_notification(workspace_id, surface_id)
            || self
                .focused_read_indicator_by_workspace_id
                .get(workspace_id)
                .map(|s| Some(s.as_str()) == surface_id)
                .unwrap_or(false)
    }

    /// Verbatim port of Swift `canMarkWorkspaceRead(forWorkspaceIds:)`.
    pub fn can_mark_workspace_read(&self, workspace_ids: &[&str]) -> bool {
        workspace_ids.iter().any(|id| self.workspace_is_unread(id))
    }

    /// Verbatim port of Swift `canMarkWorkspaceUnread(forWorkspaceIds:)`.
    pub fn can_mark_workspace_unread(&self, workspace_ids: &[&str]) -> bool {
        workspace_ids.iter().any(|id| !self.workspace_is_unread(id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: &str, tab: &str, body: &str, created_at: i64) -> TerminalNotification {
        TerminalNotification {
            id: id.to_owned(),
            tab_id: tab.to_owned(),
            surface_id: None,
            panel_id: None,
            title: format!("title-{id}"),
            subtitle: String::new(),
            body: body.to_owned(),
            created_at,
            is_read: false,
            pane_flash: true,
            click_action: None,
        }
    }

    fn set(values: &[&str]) -> HashSet<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn summaries(
        pairs: Vec<(&str, usize, Option<&str>)>,
    ) -> HashMap<String, SidebarWorkspaceUnreadSummary> {
        pairs
            .into_iter()
            .map(|(id, count, text)| {
                (
                    id.to_string(),
                    SidebarWorkspaceUnreadSummary {
                        unread_count: count,
                        latest_notification_text: text.map(str::to_string),
                    },
                )
            })
            .collect()
    }

    // --- build_sidebar_unread_summaries ------------------------------------

    #[test]
    fn build_summaries_counts_and_latest_text() {
        let unread_by_tab: HashMap<String, usize> =
            [("t1".to_string(), 2usize)].into_iter().collect();
        let latest: HashMap<String, TerminalNotification> = [
            ("t1".to_string(), notification("a", "t1", "latest body", 3)),
            // Body empty → falls back to title; only-read/latest still included.
            ("t2".to_string(), notification("b", "t2", "", 1)),
        ]
        .into_iter()
        .collect();
        let indicators = set(&["t3"]); // indicator-only workspace

        let result = build_sidebar_unread_summaries(&unread_by_tab, &latest, &indicators);

        assert_eq!(result["t1"].unread_count, 2);
        assert_eq!(
            result["t1"].latest_notification_text.as_deref(),
            Some("latest body")
        );
        // t2: no unread count but has latest → included, text from title.
        assert_eq!(result["t2"].unread_count, 0);
        assert_eq!(
            result["t2"].latest_notification_text.as_deref(),
            Some("title-b")
        );
        // t3: indicator adds 1, no notification → count 1, no text.
        assert_eq!(result["t3"].unread_count, 1);
        assert_eq!(result["t3"].latest_notification_text, None);
    }

    #[test]
    fn build_summaries_skips_empty_workspaces() {
        // A tab present only via a zero unread count and no latest is skipped.
        let unread_by_tab: HashMap<String, usize> =
            [("t1".to_string(), 0usize)].into_iter().collect();
        let latest: HashMap<String, TerminalNotification> = HashMap::new();
        let indicators = HashSet::new();
        let result = build_sidebar_unread_summaries(&unread_by_tab, &latest, &indicators);
        assert!(result.is_empty());
    }

    #[test]
    fn build_summaries_indicator_adds_to_notification_count() {
        let unread_by_tab: HashMap<String, usize> =
            [("t1".to_string(), 2usize)].into_iter().collect();
        let latest: HashMap<String, TerminalNotification> = HashMap::new();
        let indicators = set(&["t1"]);
        let result = build_sidebar_unread_summaries(&unread_by_tab, &latest, &indicators);
        // 2 notifications + 1 indicator = 3.
        assert_eq!(result["t1"].unread_count, 3);
    }

    #[test]
    fn build_summaries_whitespace_only_body_trims_to_none_and_skips() {
        // Swift `buildSidebarUnreadSummaries` (595-615) uses
        // `body.isEmpty ? title : body`: a whitespace-only body is NOT empty, so
        // it is chosen (not the title), then trimmed to empty → nil. With no
        // unread count or indicator, the workspace is skipped entirely — it does
        // NOT fall back to the title (that only happens on an exactly-empty body).
        let latest: HashMap<String, TerminalNotification> =
            [("t1".to_string(), notification("a", "t1", "   \n ", 1))]
                .into_iter()
                .collect();
        let result = build_sidebar_unread_summaries(&HashMap::new(), &latest, &HashSet::new());
        assert!(result.is_empty());
    }

    #[test]
    fn build_summaries_exactly_empty_body_falls_back_to_title() {
        // Contrast with the whitespace case: an exactly-empty body DOES fall back
        // to the title (Swift `body.isEmpty ? title : body`), and a non-empty
        // trimmed title keeps the workspace present.
        let latest: HashMap<String, TerminalNotification> =
            [("t1".to_string(), notification("a", "t1", "", 1))]
                .into_iter()
                .collect();
        let result = build_sidebar_unread_summaries(&HashMap::new(), &latest, &HashSet::new());
        assert_eq!(
            result["t1"].latest_notification_text.as_deref(),
            Some("title-a")
        );
    }

    // --- SidebarUnreadModel.apply change flags -----------------------------

    #[test]
    fn apply_reports_per_field_changes() {
        let mut model = SidebarUnreadModel::new();
        let changes = model.apply(
            3,
            summaries(vec![("t1", 2, Some("hi"))]),
            [SidebarSurfaceUnreadKey {
                workspace_id: "t1".into(),
                surface_id: Some("s1".into()),
            }]
            .into_iter()
            .collect(),
            [("t1".to_string(), "s1".to_string())].into_iter().collect(),
            set(&["t9"]),
        );
        // First apply: everything changed.
        assert!(changes.total_unread_count);
        assert!(changes.summaries);
        assert!(changes.unread_surface_keys);
        assert!(changes.focused_read_indicator);
        assert!(changes.manual_unread);
        assert!(changes.any());
    }

    #[test]
    fn apply_is_idempotent_when_unchanged() {
        let mut model = SidebarUnreadModel::new();
        let build = || {
            (
                3usize,
                summaries(vec![("t1", 2, Some("hi"))]),
                HashSet::<SidebarSurfaceUnreadKey>::new(),
                HashMap::<String, String>::new(),
                set(&["t9"]),
            )
        };
        let (a, b, c, d, e) = build();
        model.apply(a, b, c, d, e);
        let (a, b, c, d, e) = build();
        let changes = model.apply(a, b, c, d, e);
        assert!(!changes.any());
        assert!(!changes.total_unread_count);
        assert!(!changes.summaries);
    }

    #[test]
    fn apply_reports_only_the_changed_field() {
        let mut model = SidebarUnreadModel::new();
        model.apply(
            1,
            summaries(vec![("t1", 1, None)]),
            HashSet::new(),
            HashMap::new(),
            HashSet::new(),
        );
        // Only the total changes.
        let changes = model.apply(
            2,
            summaries(vec![("t1", 1, None)]),
            HashSet::new(),
            HashMap::new(),
            HashSet::new(),
        );
        assert!(changes.total_unread_count);
        assert!(!changes.summaries);
        assert!(!changes.unread_surface_keys);
        assert!(!changes.focused_read_indicator);
        assert!(!changes.manual_unread);
    }

    // --- query methods ------------------------------------------------------

    #[test]
    fn query_methods_mirror_store() {
        let mut model = SidebarUnreadModel::new();
        model.apply(
            5,
            summaries(vec![("t1", 2, Some("body")), ("t2", 0, None)]),
            [SidebarSurfaceUnreadKey {
                workspace_id: "t1".into(),
                surface_id: Some("s1".into()),
            }]
            .into_iter()
            .collect(),
            [("t1".to_string(), "s2".to_string())].into_iter().collect(),
            set(&["t1"]),
        );

        assert_eq!(model.total_unread_count(), 5);
        assert_eq!(model.unread_count("t1"), 2);
        assert_eq!(model.latest_notification_text("t1").as_deref(), Some("body"));
        assert!(model.workspace_is_unread("t1"));
        assert!(!model.workspace_is_unread("t2"));
        assert!(!model.workspace_is_unread("absent"));
        assert!(model.has_manual_unread("t1"));
        assert!(!model.has_manual_unread("t2"));

        // per-surface unread key
        assert!(model.has_unread_notification("t1", Some("s1")));
        assert!(!model.has_unread_notification("t1", Some("other")));

        // visible indicator: unread surface OR focused-read match
        assert!(model.has_visible_notification_indicator("t1", Some("s1"))); // unread
        assert!(model.has_visible_notification_indicator("t1", Some("s2"))); // focused-read
        assert!(!model.has_visible_notification_indicator("t1", Some("s3")));

        // can-mark predicates
        assert!(model.can_mark_workspace_read(&["t1", "t2"]));
        assert!(!model.can_mark_workspace_read(&["t2"]));
        assert!(model.can_mark_workspace_unread(&["t2"]));
        assert!(!model.can_mark_workspace_unread(&["t1"]));
    }

    #[test]
    fn summary_defaults_for_absent_workspace() {
        let model = SidebarUnreadModel::new();
        let summary = model.summary("absent");
        assert_eq!(summary, SidebarWorkspaceUnreadSummary::default());
        assert_eq!(model.unread_count("absent"), 0);
        assert_eq!(model.latest_notification_text("absent"), None);
    }
}
