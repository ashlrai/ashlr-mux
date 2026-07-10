use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub mod authorization;
pub mod badge;
pub mod coalescer;
pub mod menu_snapshot;
pub mod policy;
pub mod reconcile;
pub mod settings;
pub mod sidebar_unread;
pub mod sound;
pub mod superseded_buffer;

pub use authorization::{
    cached_delivery_authorization_decision, fallback_effects, NotificationAuthorizationState,
};
pub use badge::dock_badge_label;
pub use coalescer::NotificationBurstCoalescer;
pub use menu_snapshot::{
    badge_text, make as make_menu_snapshot, plain_title, state_hint_kind, NotificationMenuSnapshot,
    StateHintKind, DEFAULT_INLINE_NOTIFICATION_LIMIT,
};
pub use policy::{
    delivery_decision, has_any_notification_effect, should_suppress_external_delivery,
    DeliveryDecision, TerminalNotificationPolicyEffects,
};
pub use reconcile::DismissedTombstoneRing;
pub use settings::NotificationGates;
pub use sidebar_unread::{
    build_sidebar_unread_summaries, SidebarApplyChanges, SidebarSurfaceUnreadKey,
    SidebarUnreadModel, SidebarWorkspaceUnreadSummary,
};
pub use sound::NotificationSound;
pub use superseded_buffer::SupersededPhoneDismissBuffer;

fn default_pane_flash() -> bool {
    true
}

/// What clicking a delivered notification should do.
///
/// Ported from Swift `TerminalNotificationClickAction`
/// (`TerminalNotificationStore.swift` 146-174). Swift's case is
/// `revealInFinder`; on Windows the equivalent reveals the path in Explorer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NotificationClickAction {
    RevealInExplorer { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalNotification {
    pub id: String,
    pub tab_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panel_id: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub created_at: i64,
    #[serde(default)]
    pub is_read: bool,
    #[serde(default = "default_pane_flash")]
    pub pane_flash: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_action: Option<NotificationClickAction>,
}

impl TerminalNotification {
    /// Whether this notification belongs to the given tab and surface, treating
    /// the surface match as surface-OR-panel.
    ///
    /// Ported verbatim from Swift `TerminalNotification.matches`
    /// (`TerminalNotificationStore.swift` 215-221): a `None` target surface
    /// matches only a notification with no surface and no panel; otherwise the
    /// target matches either the surface id or the panel id.
    pub fn matches(&self, tab_id: &str, surface_id: Option<&str>) -> bool {
        if self.tab_id != tab_id {
            return false;
        }
        match surface_id {
            None => self.surface_id.is_none() && self.panel_id.is_none(),
            Some(target) => {
                self.surface_id.as_deref() == Some(target)
                    || self.panel_id.as_deref() == Some(target)
            }
        }
    }
}

/// Whether `lhs` sorts before `rhs` in the notification list: newest
/// `created_at` first, ties broken by ascending id.
///
/// Ported from Swift `notificationSortPrecedes`
/// (`TerminalNotificationStore.swift` 2045-2050). Swift breaks ties on
/// `id.uuidString`; the Rust model already stores ids as `String`, so the
/// comparison is the same lexicographic order.
pub fn notification_sort_precedes(lhs: &TerminalNotification, rhs: &TerminalNotification) -> bool {
    if lhs.created_at != rhs.created_at {
        return lhs.created_at > rhs.created_at;
    }
    lhs.id < rhs.id
}

fn notification_order(
    lhs: &TerminalNotification,
    rhs: &TerminalNotification,
) -> std::cmp::Ordering {
    rhs.created_at
        .cmp(&lhs.created_at)
        .then_with(|| lhs.id.cmp(&rhs.id))
}

/// Legacy flat reducer retained for backward compatibility. New callers should
/// use [`NotificationStore`], which ports the full Swift state machine.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NotificationState {
    pub notifications: Vec<TerminalNotification>,
    pub unread_count: usize,
}

impl NotificationState {
    pub fn record(&mut self, notification: TerminalNotification) {
        self.notifications
            .retain(|existing| existing.id != notification.id);
        self.notifications.push(notification);
        self.notifications.sort_by(notification_order);
        self.refresh();
    }

    pub fn mark_read(&mut self, id: &str) {
        if let Some(notification) = self.notifications.iter_mut().find(|item| item.id == id) {
            notification.is_read = true;
        }
        self.refresh();
    }

    pub fn mark_unread(&mut self, id: &str) {
        if let Some(notification) = self.notifications.iter_mut().find(|item| item.id == id) {
            notification.is_read = false;
        }
        self.refresh();
    }

    pub fn remove(&mut self, id: &str) {
        self.notifications
            .retain(|notification| notification.id != id);
        self.refresh();
    }

    pub fn clear(&mut self) {
        self.notifications.clear();
        self.refresh();
    }

    fn refresh(&mut self) {
        self.unread_count = self
            .notifications
            .iter()
            .filter(|notification| !notification.is_read)
            .count();
    }
}

/// Key into the per-tab/per-surface unread index. Mirrors Swift
/// `TerminalNotificationStore.TabSurfaceKey`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TabSurfaceKey {
    tab_id: String,
    surface_id: Option<String>,
}

/// Derived unread indexes rebuilt on every store mutation.
///
/// Mirrors Swift `TerminalNotificationStore.NotificationIndexes`
/// (`TerminalNotificationStore.swift` 231-237), populated by
/// [`build_indexes`].
#[derive(Debug, Clone, Default)]
struct NotificationIndexes {
    unread_count: usize,
    unread_count_by_tab_id: HashMap<String, usize>,
    unread_by_tab_surface: HashSet<TabSurfaceKey>,
    latest_unread_by_tab_id: HashMap<String, TerminalNotification>,
    latest_by_tab_id: HashMap<String, TerminalNotification>,
}

/// A captured cooldown reservation so a not-yet-committed debounce slot can be
/// rolled back when a notification produces no deliverable effect.
///
/// Mirrors Swift `NotificationCooldownReservation`
/// (`TerminalNotificationStore.swift` 979-982).
#[derive(Debug, Clone)]
struct CooldownReservation {
    key: String,
    previous_date: Option<i64>,
}

/// The pure inputs needed to materialize one notification (no cooldown).
#[derive(Debug, Clone)]
pub struct NotificationRequest {
    pub id: String,
    pub tab_id: String,
    pub surface_id: Option<String>,
    pub panel_id: Option<String>,
    pub title: String,
    pub subtitle: String,
    pub body: String,
    pub created_at: i64,
    pub click_action: Option<NotificationClickAction>,
}

/// A notification request plus the cooldown-debounce parameters consumed by
/// [`NotificationStore::add_notification`].
#[derive(Debug, Clone)]
pub struct AddNotificationRequest {
    pub request: NotificationRequest,
    pub cooldown_key: Option<String>,
    /// Cooldown interval, in the same time unit as `created_at`. Mirrors Swift's
    /// `TimeInterval`; only a finite positive value is honored.
    pub cooldown_interval: Option<i64>,
}

/// The outcome of applying a notification, describing the delivery decision and
/// any superseded ids the caller must clear from the OS delivery seam — without
/// performing any OS work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub delivery: DeliveryDecision,
    /// Whether the workspace should be reordered to the top (the decision flag
    /// only; the settings read + tab move are deferred to the tab manager).
    pub reorder_workspace: bool,
    /// Whether the notification was recorded into the store.
    pub recorded: bool,
    /// Ids of superseded notifications removed by a record, so the delivery seam
    /// can clear their banners.
    pub cleared_ids: Vec<String>,
}

/// The full unread-index state machine ported from Swift
/// `TerminalNotificationStore`.
///
/// All OS delivery (toasts, sound, dock/taskbar badge writes, phone-push
/// mirroring) lives behind the delivery seam and is intentionally excluded;
/// methods return the pure data the seam needs (cleared ids, delivery
/// decisions). Ids are `String` and timestamps are `i64` to match the Rust
/// model (Swift uses `UUID`/`Date`).
#[derive(Debug, Clone, Default)]
pub struct NotificationStore {
    notifications: Vec<TerminalNotification>,
    indexes: NotificationIndexes,
    manual_unread_workspace_ids: HashSet<String>,
    panel_derived_unread_workspace_ids: HashSet<String>,
    restored_unread_workspace_ids: HashSet<String>,
    focused_read_indicator_by_tab_id: HashMap<String, String>,
    last_notification_date_by_cooldown_key: HashMap<String, i64>,
}

impl NotificationStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Immutable read of the current notifications (newest first).
    pub fn notifications(&self) -> &[TerminalNotification] {
        &self.notifications
    }

    // --- index building -----------------------------------------------------

    /// Mirrors Swift `buildIndexes(for:)`
    /// (`TerminalNotificationStore.swift` 2021-2043).
    fn build_indexes(notifications: &[TerminalNotification]) -> NotificationIndexes {
        let mut indexes = NotificationIndexes::default();
        for notification in notifications {
            indexes
                .latest_by_tab_id
                .entry(notification.tab_id.clone())
                .or_insert_with(|| notification.clone());
            if notification.is_read {
                continue;
            }
            indexes.unread_count += 1;
            *indexes
                .unread_count_by_tab_id
                .entry(notification.tab_id.clone())
                .or_insert(0) += 1;
            indexes.unread_by_tab_surface.insert(TabSurfaceKey {
                tab_id: notification.tab_id.clone(),
                surface_id: notification.surface_id.clone(),
            });
            if let Some(panel_id) = &notification.panel_id {
                if Some(panel_id) != notification.surface_id.as_ref() {
                    indexes.unread_by_tab_surface.insert(TabSurfaceKey {
                        tab_id: notification.tab_id.clone(),
                        surface_id: Some(panel_id.clone()),
                    });
                }
            }
            indexes
                .latest_unread_by_tab_id
                .entry(notification.tab_id.clone())
                .or_insert_with(|| notification.clone());
        }
        indexes
    }

    fn rebuild_indexes(&mut self) {
        self.indexes = Self::build_indexes(&self.notifications);
    }

    // --- record / apply -----------------------------------------------------

    /// Record a notification with Swift supersede semantics: remove every
    /// existing notification for the same tab+surface, capture their ids,
    /// front-insert the newcomer, clear any stale focused-read indicator,
    /// optionally set a focused-read indicator when suppressed-while-unread, and
    /// clear the tab's manual unread flag.
    ///
    /// Mirrors the pure parts of Swift `recordNotification`
    /// (`TerminalNotificationStore.swift` 1147-1237); the `center.remove*` and
    /// phone-push lines are the deferred delivery seam, surfaced here only as
    /// the returned cleared-id list. `mark_unread` is derived from
    /// `!notification.is_read` (Swift builds the notification with
    /// `isRead = !effects.markUnread`).
    pub fn record(
        &mut self,
        notification: TerminalNotification,
        should_suppress_external_delivery: bool,
    ) -> Vec<String> {
        let mark_unread = !notification.is_read;
        let tab_id = notification.tab_id.clone();
        let surface_id = notification.surface_id.clone();

        let mut ids_to_clear: Vec<String> = Vec::new();
        self.notifications.retain(|existing| {
            if existing.tab_id == tab_id && existing.surface_id == surface_id {
                ids_to_clear.push(existing.id.clone());
                false
            } else {
                true
            }
        });

        if let Some(existing_surface) = self.focused_read_indicator_by_tab_id.get(&tab_id) {
            if Some(existing_surface.as_str()) != surface_id.as_deref() {
                self.focused_read_indicator_by_tab_id.remove(&tab_id);
            }
        }

        if should_suppress_external_delivery && mark_unread {
            self.set_focused_read_indicator(&tab_id, surface_id.as_deref());
        }

        self.notifications.insert(0, notification);
        self.set_workspace_manual_unread(false, &tab_id);
        self.rebuild_indexes();
        ids_to_clear
    }

    /// Build a notification from `request` + `effects` and either record it or
    /// take the effects-only path, returning the delivery decision.
    ///
    /// Mirrors Swift `applyNotification(request:effects:...)`
    /// (`TerminalNotificationStore.swift` 1089-1145) without any OS delivery.
    pub fn apply_notification(
        &mut self,
        request: NotificationRequest,
        effects: &TerminalNotificationPolicyEffects,
        should_suppress_external_delivery: bool,
    ) -> ApplyOutcome {
        let notification = TerminalNotification {
            id: request.id,
            tab_id: request.tab_id,
            surface_id: request.surface_id,
            panel_id: request.panel_id,
            title: request.title,
            subtitle: request.subtitle,
            body: request.body,
            created_at: request.created_at,
            is_read: !effects.mark_unread,
            pane_flash: effects.pane_flash,
            click_action: request.click_action,
        };
        let delivery = delivery_decision(effects, should_suppress_external_delivery);
        let reorder_workspace = effects.reorder_workspace;

        if effects.record {
            let cleared_ids = self.record(notification, should_suppress_external_delivery);
            ApplyOutcome {
                delivery,
                reorder_workspace,
                recorded: true,
                cleared_ids,
            }
        } else {
            ApplyOutcome {
                delivery,
                reorder_workspace,
                recorded: false,
                cleared_ids: Vec::new(),
            }
        }
    }

    /// Cooldown-debounced entrypoint. Skips (returns `None`) when an identical
    /// cooldown key fired within the interval; otherwise reserves the slot,
    /// applies the notification, and commits or rolls back the reservation based
    /// on whether any deliverable effect occurred.
    ///
    /// Mirrors Swift `addNotification(...)`
    /// (`TerminalNotificationStore.swift` 877-977, the pure cooldown + reserve /
    /// commit / restore path). `created_at` doubles as Swift's `now`.
    pub fn add_notification(
        &mut self,
        add: AddNotificationRequest,
        effects: &TerminalNotificationPolicyEffects,
        should_suppress_external_delivery: bool,
    ) -> Option<ApplyOutcome> {
        let now = add.request.created_at;
        let resolved_interval = add.cooldown_interval.filter(|interval| *interval > 0);

        if let (Some(key), Some(interval)) = (add.cooldown_key.as_deref(), resolved_interval) {
            if let Some(&last) = self.last_notification_date_by_cooldown_key.get(key) {
                if now - last < interval {
                    return None;
                }
            }
        }

        let reservation =
            self.make_cooldown_reservation(add.cooldown_key.as_deref(), resolved_interval);
        if let Some(reservation) = &reservation {
            self.last_notification_date_by_cooldown_key
                .insert(reservation.key.clone(), now);
        }

        let outcome =
            self.apply_notification(add.request, effects, should_suppress_external_delivery);

        // The record path always commits the reservation (already set to `now`).
        // The effects-only path commits when a deliverable effect occurred and
        // rolls back otherwise (Swift `applyNotification` 1135-1139).
        if !outcome.recorded && !has_any_notification_effect(effects) {
            self.restore_cooldown_reservation(reservation.as_ref());
        }

        Some(outcome)
    }

    fn make_cooldown_reservation(
        &self,
        key: Option<&str>,
        interval: Option<i64>,
    ) -> Option<CooldownReservation> {
        match (key, interval) {
            (Some(key), Some(_)) => Some(CooldownReservation {
                key: key.to_string(),
                previous_date: self
                    .last_notification_date_by_cooldown_key
                    .get(key)
                    .copied(),
            }),
            _ => None,
        }
    }

    fn restore_cooldown_reservation(&mut self, reservation: Option<&CooldownReservation>) {
        let Some(reservation) = reservation else {
            return;
        };
        match reservation.previous_date {
            Some(previous) => {
                self.last_notification_date_by_cooldown_key
                    .insert(reservation.key.clone(), previous);
            }
            None => {
                self.last_notification_date_by_cooldown_key
                    .remove(&reservation.key);
            }
        }
    }

    // --- mark read / unread -------------------------------------------------

    /// Mark one notification read. Returns its id when it transitioned from
    /// unread → read (the cleared id), else `None`.
    ///
    /// Mirrors Swift `markRead(id:)` (`TerminalNotificationStore.swift`
    /// 1347-1362), excluding the delivery seam.
    pub fn mark_read(&mut self, id: &str) -> Option<String> {
        let index = self.notifications.iter().position(|item| item.id == id)?;
        if self.notifications[index].is_read {
            return None;
        }
        self.notifications[index].is_read = true;
        self.rebuild_indexes();
        Some(id.to_string())
    }

    /// Mark one notification unread, deferring any manual/restored workspace
    /// unread for the same tab to the now-concrete notification.
    ///
    /// Mirrors Swift `markUnread(id:)` (`TerminalNotificationStore.swift`
    /// 1364-1378).
    pub fn mark_unread(&mut self, id: &str) {
        let Some(index) = self.notifications.iter().position(|item| item.id == id) else {
            return;
        };
        let mut notification = self.notifications.remove(index);
        if !notification.is_read {
            self.notifications.insert(index, notification);
            return;
        }
        let tab_id = notification.tab_id.clone();
        notification.is_read = false;
        self.notifications.insert(0, notification);
        self.rebuild_indexes();
        self.set_workspace_manual_unread(false, &tab_id);
        self.set_workspace_restored_unread(false, &tab_id);
    }

    /// Mark every unread notification for a tab read. Returns the cleared ids.
    ///
    /// Mirrors Swift `markRead(forTabId:)` (`TerminalNotificationStore.swift`
    /// 1380-1404). The `clearWorkspacePanelUnread` AppDelegate side effect is
    /// deferred.
    pub fn mark_read_for_tab(&mut self, tab_id: &str) -> Vec<String> {
        let mut ids_to_clear = Vec::new();
        for notification in &mut self.notifications {
            if notification.tab_id == tab_id && !notification.is_read {
                notification.is_read = true;
                ids_to_clear.push(notification.id.clone());
            }
        }
        if !ids_to_clear.is_empty() {
            self.rebuild_indexes();
        }
        self.clear_focused_read_indicator(tab_id, None);
        self.set_workspace_manual_unread(false, tab_id);
        self.set_panel_derived_workspace_unread(false, tab_id);
        self.set_workspace_restored_unread(false, tab_id);
        ids_to_clear
    }

    /// Mark every unread notification matching a tab+surface read. Returns the
    /// cleared ids.
    ///
    /// Mirrors Swift `markRead(forTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 1406-1439).
    pub fn mark_read_for_tab_surface(
        &mut self,
        tab_id: &str,
        surface_id: Option<&str>,
    ) -> Vec<String> {
        let mut ids_to_clear = Vec::new();
        for notification in &mut self.notifications {
            if notification.matches(tab_id, surface_id) && !notification.is_read {
                notification.is_read = true;
                ids_to_clear.push(notification.id.clone());
            }
        }
        if !ids_to_clear.is_empty() {
            self.rebuild_indexes();
        }
        self.clear_focused_read_indicator(tab_id, surface_id);
        if surface_id.is_none() {
            self.set_panel_derived_workspace_unread(false, tab_id);
            self.set_workspace_restored_unread(false, tab_id);
        }
        ids_to_clear
    }

    /// Mark a whole tab unread by setting the manual workspace indicator.
    ///
    /// Mirrors Swift `markUnread(forTabId:)` (`TerminalNotificationStore.swift`
    /// 1441-1444).
    pub fn mark_unread_for_tab(&mut self, tab_id: &str) {
        self.set_workspace_manual_unread(true, tab_id);
        self.set_workspace_restored_unread(false, tab_id);
    }

    /// Re-mark the latest matching notification as the oldest unread one,
    /// moving it just past the last unread entry. Returns the affected id.
    ///
    /// Mirrors Swift `markLatestNotificationAsOldestUnread(forTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 1446-1463).
    pub fn mark_latest_as_oldest_unread(
        &mut self,
        tab_id: &str,
        surface_id: Option<&str>,
    ) -> Option<String> {
        let Some(index) = self.latest_notification_index(tab_id, surface_id) else {
            if surface_id.is_none() && !self.workspace_is_unread(tab_id) {
                self.set_workspace_manual_unread(true, tab_id);
            }
            return None;
        };

        let mut notification = self.notifications.remove(index);
        notification.is_read = false;
        let insertion_index = self
            .notifications
            .iter()
            .rposition(|item| !item.is_read)
            .map(|i| i + 1)
            .unwrap_or(self.notifications.len());
        let id = notification.id.clone();
        self.notifications.insert(insertion_index, notification);
        self.set_workspace_manual_unread(false, tab_id);
        self.rebuild_indexes();
        Some(id)
    }

    /// Mirrors Swift `latestNotificationIndex(forTabId:surfaceId:in:)`
    /// (`TerminalNotificationStore.swift` 1465-1474).
    fn latest_notification_index(&self, tab_id: &str, surface_id: Option<&str>) -> Option<usize> {
        if let Some(index) = self
            .notifications
            .iter()
            .position(|item| item.matches(tab_id, surface_id))
        {
            return Some(index);
        }
        if surface_id.is_some() {
            if let Some(index) = self
                .notifications
                .iter()
                .position(|item| item.tab_id == tab_id && item.surface_id.is_none())
            {
                return Some(index);
            }
        }
        self.notifications
            .iter()
            .position(|item| item.tab_id == tab_id)
    }

    /// Mark every unread notification read. Returns the cleared ids.
    ///
    /// Mirrors Swift `markAllRead()` (`TerminalNotificationStore.swift`
    /// 1494-1520). The per-tab panel-unread AppDelegate side effect is deferred.
    pub fn mark_all_read(&mut self) -> Vec<String> {
        let mut ids_to_clear = Vec::new();
        for notification in &mut self.notifications {
            if !notification.is_read {
                notification.is_read = true;
                ids_to_clear.push(notification.id.clone());
            }
        }
        if !ids_to_clear.is_empty() {
            self.rebuild_indexes();
        }
        self.clear_workspace_manual_unread();
        self.clear_panel_derived_workspace_unread();
        self.clear_workspace_restored_unread();
        ids_to_clear
    }

    // --- remove / clear -----------------------------------------------------

    /// Remove one notification by id, clearing any focused-read indicator it
    /// owned. Returns whether anything was removed.
    ///
    /// Mirrors Swift `remove(id:)` (`TerminalNotificationStore.swift`
    /// 1522-1542).
    pub fn remove(&mut self, id: &str) -> bool {
        let Some(index) = self.notifications.iter().position(|item| item.id == id) else {
            return false;
        };
        let removed = self.notifications.remove(index);
        self.rebuild_indexes();
        self.clear_focused_read_indicator(&removed.tab_id, removed.surface_id.as_deref());
        true
    }

    /// Remove the latest notification for a tab.
    ///
    /// Mirrors Swift `clearLatestNotification(forTabId:)`
    /// (`TerminalNotificationStore.swift` 868-871).
    pub fn clear_latest_notification(&mut self, tab_id: &str) {
        if let Some(latest) = self.indexes.latest_by_tab_id.get(tab_id) {
            let id = latest.id.clone();
            self.remove(&id);
        }
    }

    /// Clear every notification for a tab+surface plus a matching focused/
    /// restored indicator. Returns the cleared ids.
    ///
    /// Mirrors Swift `clearNotifications(forTabId:surfaceId:...)`
    /// (`TerminalNotificationStore.swift` 1621-1662).
    pub fn clear_for_tab_surface(&mut self, tab_id: &str, surface_id: Option<&str>) -> Vec<String> {
        let had_focused_read_indicator = self
            .focused_read_indicator_by_tab_id
            .get(tab_id)
            .map(|s| Some(s.as_str()) == surface_id)
            .unwrap_or(false);
        let had_restored_workspace_unread =
            surface_id.is_none() && self.restored_unread_workspace_ids.contains(tab_id);

        let ids_to_clear: Vec<String> = self
            .notifications
            .iter()
            .filter(|item| item.matches(tab_id, surface_id))
            .map(|item| item.id.clone())
            .collect();

        if ids_to_clear.is_empty() && !had_focused_read_indicator && !had_restored_workspace_unread
        {
            return Vec::new();
        }
        if !ids_to_clear.is_empty() {
            self.notifications
                .retain(|item| !item.matches(tab_id, surface_id));
            self.rebuild_indexes();
        }
        if surface_id.is_none() {
            self.set_workspace_restored_unread(false, tab_id);
        }
        self.clear_focused_read_indicator(tab_id, surface_id);
        ids_to_clear
    }

    /// Clear every notification for a tab (any surface). Returns the cleared
    /// ids.
    ///
    /// Mirrors Swift `clearNotifications(forTabId:...)`
    /// (`TerminalNotificationStore.swift` 1700-1731). The
    /// `clearWorkspacePanelUnread` AppDelegate side effect is deferred.
    pub fn clear_for_tab(&mut self, tab_id: &str) -> Vec<String> {
        let had_focused_read_indicator = self.focused_read_indicator_by_tab_id.contains_key(tab_id);
        let ids_to_clear: Vec<String> = self
            .notifications
            .iter()
            .filter(|item| item.tab_id == tab_id)
            .map(|item| item.id.clone())
            .collect();

        self.set_workspace_manual_unread(false, tab_id);
        self.set_panel_derived_workspace_unread(false, tab_id);
        self.set_workspace_restored_unread(false, tab_id);

        if ids_to_clear.is_empty() && !had_focused_read_indicator {
            return Vec::new();
        }
        if !ids_to_clear.is_empty() {
            self.notifications.retain(|item| item.tab_id != tab_id);
            self.rebuild_indexes();
        }
        self.clear_focused_read_indicator(tab_id, None);
        ids_to_clear
    }

    /// Clear all notifications and every workspace/focused indicator. Returns
    /// the cleared notification ids.
    ///
    /// Mirrors Swift `clearAll(...)` (`TerminalNotificationStore.swift`
    /// 1600-1619). The per-tab panel-unread AppDelegate side effect is deferred.
    pub fn clear_all(&mut self) -> Vec<String> {
        let has_state = !self.notifications.is_empty()
            || !self.focused_read_indicator_by_tab_id.is_empty()
            || !self.manual_unread_workspace_ids.is_empty()
            || !self.panel_derived_unread_workspace_ids.is_empty()
            || !self.restored_unread_workspace_ids.is_empty();
        if !has_state {
            return Vec::new();
        }
        let ids: Vec<String> = self
            .notifications
            .iter()
            .map(|item| item.id.clone())
            .collect();
        self.notifications.clear();
        self.clear_workspace_manual_unread();
        self.clear_panel_derived_workspace_unread();
        self.clear_workspace_restored_unread();
        self.focused_read_indicator_by_tab_id.clear();
        self.rebuild_indexes();
        ids
    }

    /// Reassign a surface's notifications from one tab to another.
    ///
    /// Mirrors Swift `rebindSurfaceNotifications(fromTabId:toTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 1664-1698).
    pub fn rebind_surface(
        &mut self,
        source_tab_id: &str,
        destination_tab_id: &str,
        surface_id: &str,
    ) {
        if source_tab_id == destination_tab_id {
            return;
        }
        let mut did_move = false;
        for notification in &mut self.notifications {
            if notification.matches(source_tab_id, Some(surface_id)) {
                notification.tab_id = destination_tab_id.to_string();
                did_move = true;
            }
        }
        if did_move {
            self.rebuild_indexes();
        }
        if self
            .focused_read_indicator_by_tab_id
            .get(source_tab_id)
            .map(|s| s.as_str())
            == Some(surface_id)
        {
            self.focused_read_indicator_by_tab_id.remove(source_tab_id);
            if !self
                .focused_read_indicator_by_tab_id
                .contains_key(destination_tab_id)
            {
                self.focused_read_indicator_by_tab_id
                    .insert(destination_tab_id.to_string(), surface_id.to_string());
            }
        }
    }

    /// Replace a tab's notifications with restored session entries, reassigning
    /// any colliding ids and re-sorting the whole list.
    ///
    /// Mirrors Swift `restoreSessionNotifications(_:forTabId:)`
    /// (`TerminalNotificationStore.swift` 1544-1568). The
    /// `TerminalMutationBus` discard and delivery seam are out of scope.
    pub fn restore_session(
        &mut self,
        restored_notifications: Vec<TerminalNotification>,
        tab_id: &str,
    ) {
        let mut used_ids: HashSet<String> = self
            .notifications
            .iter()
            .filter(|item| item.tab_id != tab_id)
            .map(|item| item.id.clone())
            .collect();

        let mut restored_for_tab: Vec<TerminalNotification> = restored_notifications
            .into_iter()
            .filter(|item| item.tab_id == tab_id)
            .collect();
        restored_for_tab.sort_by(notification_order);
        let restored_for_tab: Vec<TerminalNotification> = restored_for_tab
            .into_iter()
            .map(|item| Self::notification_with_unique_id(item, &mut used_ids))
            .collect();

        let kept: Vec<TerminalNotification> = self
            .notifications
            .iter()
            .filter(|item| item.tab_id != tab_id)
            .cloned()
            .collect();

        let mut next: Vec<TerminalNotification> =
            restored_for_tab.into_iter().chain(kept).collect();
        next.sort_by(notification_order);

        if next != self.notifications {
            self.notifications = next;
            self.rebuild_indexes();
        }
        self.clear_focused_read_indicator(tab_id, None);
    }

    /// Mirrors Swift `notificationWithUniqueId(_:usedIds:)`
    /// (`TerminalNotificationStore.swift` 1570-1596).
    ///
    /// DEVIATION: Swift mints a fresh random `UUID` on collision; the Rust model
    /// uses `String` ids with no UUID generator available, so a deterministic
    /// `"<id>#<n>"` suffix is appended until unique. Uniqueness within the
    /// `used_ids` set is preserved, which is all the caller relies on.
    fn notification_with_unique_id(
        mut notification: TerminalNotification,
        used_ids: &mut HashSet<String>,
    ) -> TerminalNotification {
        if used_ids.insert(notification.id.clone()) {
            return notification;
        }
        let base = notification.id.clone();
        let mut counter: u64 = 1;
        loop {
            let candidate = format!("{base}#{counter}");
            if used_ids.insert(candidate.clone()) {
                notification.id = candidate;
                return notification;
            }
            counter += 1;
        }
    }

    // --- focused-read indicators -------------------------------------------

    /// Mirrors Swift `setFocusedReadIndicator(forTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 1476-1480).
    pub fn set_focused_read_indicator(&mut self, tab_id: &str, surface_id: Option<&str>) {
        let Some(surface_id) = surface_id else {
            return;
        };
        if self
            .focused_read_indicator_by_tab_id
            .get(tab_id)
            .map(|s| s.as_str())
            == Some(surface_id)
        {
            return;
        }
        self.focused_read_indicator_by_tab_id
            .insert(tab_id.to_string(), surface_id.to_string());
    }

    /// Mirrors Swift `clearFocusedReadIndicator(forTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 1482-1486). A `None` surface clears
    /// unconditionally; a `Some` surface clears only on an exact match.
    pub fn clear_focused_read_indicator(&mut self, tab_id: &str, surface_id: Option<&str>) {
        let Some(existing) = self.focused_read_indicator_by_tab_id.get(tab_id) else {
            return;
        };
        if surface_id.is_none() || Some(existing.as_str()) == surface_id {
            self.focused_read_indicator_by_tab_id.remove(tab_id);
        }
    }

    /// Mirrors Swift `clearFocusedReadIndicatorIfSurfaceChanged(forTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 1488-1492).
    pub fn clear_focused_read_indicator_if_surface_changed(
        &mut self,
        tab_id: &str,
        surface_id: Option<&str>,
    ) {
        let Some(existing) = self.focused_read_indicator_by_tab_id.get(tab_id) else {
            return;
        };
        if Some(existing.as_str()) != surface_id {
            self.focused_read_indicator_by_tab_id.remove(tab_id);
        }
    }

    /// Mirrors Swift `focusedReadIndicatorSurfaceId(forTabId:)`
    /// (`TerminalNotificationStore.swift` 873-875).
    pub fn focused_read_indicator_surface_id(&self, tab_id: &str) -> Option<&str> {
        self.focused_read_indicator_by_tab_id
            .get(tab_id)
            .map(|s| s.as_str())
    }

    // --- workspace-indicator setters ---------------------------------------

    fn set_workspace_manual_unread(&mut self, is_unread: bool, tab_id: &str) -> bool {
        if is_unread {
            self.manual_unread_workspace_ids.insert(tab_id.to_string())
        } else {
            self.manual_unread_workspace_ids.remove(tab_id)
        }
    }

    fn clear_workspace_manual_unread(&mut self) {
        self.manual_unread_workspace_ids.clear();
    }

    fn set_panel_derived_workspace_unread(&mut self, is_unread: bool, tab_id: &str) -> bool {
        if is_unread {
            self.panel_derived_unread_workspace_ids
                .insert(tab_id.to_string())
        } else {
            self.panel_derived_unread_workspace_ids.remove(tab_id)
        }
    }

    fn clear_panel_derived_workspace_unread(&mut self) {
        self.panel_derived_unread_workspace_ids.clear();
    }

    fn set_workspace_restored_unread(&mut self, is_unread: bool, tab_id: &str) -> bool {
        if is_unread {
            self.restored_unread_workspace_ids
                .insert(tab_id.to_string())
        } else {
            self.restored_unread_workspace_ids.remove(tab_id)
        }
    }

    fn clear_workspace_restored_unread(&mut self) {
        self.restored_unread_workspace_ids.clear();
    }

    /// Mirrors Swift `setPanelDerivedUnread(_:forTabId:)`
    /// (`TerminalNotificationStore.swift` 802-805).
    pub fn set_panel_derived_unread(&mut self, is_unread: bool, tab_id: &str) -> bool {
        self.set_panel_derived_workspace_unread(is_unread, tab_id)
    }

    /// Mirrors Swift `restoreUnreadIndicator(forTabId:)`
    /// (`TerminalNotificationStore.swift` 807-810).
    pub fn restore_unread_indicator(&mut self, tab_id: &str) -> bool {
        self.set_workspace_restored_unread(true, tab_id)
    }

    /// Mirrors Swift `clearRestoredUnreadIndicator(forTabId:)`
    /// (`TerminalNotificationStore.swift` 812-815).
    pub fn clear_restored_unread_indicator(&mut self, tab_id: &str) -> bool {
        self.set_workspace_restored_unread(false, tab_id)
    }

    /// Mirrors Swift `clearManualUnread(forTabId:)`
    /// (`TerminalNotificationStore.swift` 817-820).
    pub fn clear_manual_unread(&mut self, tab_id: &str) -> bool {
        self.set_workspace_manual_unread(false, tab_id)
    }

    // --- queries ------------------------------------------------------------

    /// Mirrors Swift `hasManualUnread(forTabId:)`.
    pub fn has_manual_unread(&self, tab_id: &str) -> bool {
        self.manual_unread_workspace_ids.contains(tab_id)
    }

    /// Mirrors Swift `hasPanelDerivedUnread(forTabId:)`.
    pub fn has_panel_derived_unread(&self, tab_id: &str) -> bool {
        self.panel_derived_unread_workspace_ids.contains(tab_id)
    }

    /// Mirrors Swift `hasRestoredUnreadIndicator(forTabId:)`.
    pub fn has_restored_unread_indicator(&self, tab_id: &str) -> bool {
        self.restored_unread_workspace_ids.contains(tab_id)
    }

    /// The union of all workspace-level unread indicator ids.
    ///
    /// Mirrors Swift `workspaceUnreadIndicatorIds`
    /// (`TerminalNotificationStore.swift` 559-563).
    pub fn workspace_unread_indicator_ids(&self) -> HashSet<String> {
        self.manual_unread_workspace_ids
            .union(&self.panel_derived_unread_workspace_ids)
            .cloned()
            .collect::<HashSet<String>>()
            .union(&self.restored_unread_workspace_ids)
            .cloned()
            .collect()
    }

    fn workspace_unread_indicator_count(&self) -> usize {
        self.workspace_unread_indicator_ids().len()
    }

    /// The number of unread notification entries (the phone-badge count).
    ///
    /// Mirrors Swift `unreadNotificationCount`
    /// (`TerminalNotificationStore.swift` 263).
    pub fn unread_notification_count(&self) -> usize {
        self.indexes.unread_count
    }

    /// The total unread count: notification entries plus workspace indicators.
    ///
    /// Mirrors Swift `unreadCount` (`TerminalNotificationStore.swift` 555-557).
    pub fn unread_count(&self) -> usize {
        self.indexes.unread_count + self.workspace_unread_indicator_count()
    }

    /// Per-tab unread count: unread notifications for the tab plus one when the
    /// tab carries any workspace-level unread indicator.
    ///
    /// Mirrors Swift `unreadCount(forTabId:)`
    /// (`TerminalNotificationStore.swift` 824-829).
    pub fn unread_count_for_tab(&self, tab_id: &str) -> usize {
        let has_workspace_unread_indicator = self.manual_unread_workspace_ids.contains(tab_id)
            || self.panel_derived_unread_workspace_ids.contains(tab_id)
            || self.restored_unread_workspace_ids.contains(tab_id);
        self.indexes
            .unread_count_by_tab_id
            .get(tab_id)
            .copied()
            .unwrap_or(0)
            + usize::from(has_workspace_unread_indicator)
    }

    /// Mirrors Swift `workspaceIsUnread(forTabId:)`
    /// (`TerminalNotificationStore.swift` 831-833).
    pub fn workspace_is_unread(&self, tab_id: &str) -> bool {
        self.unread_count_for_tab(tab_id) > 0
    }

    /// Mirrors Swift `canMarkWorkspaceRead(forTabIds:)`
    /// (`TerminalNotificationStore.swift` 835-837).
    pub fn can_mark_workspace_read(&self, tab_ids: &[&str]) -> bool {
        tab_ids.iter().any(|id| self.workspace_is_unread(id))
    }

    /// Mirrors Swift `canMarkWorkspaceUnread(forTabIds:)`
    /// (`TerminalNotificationStore.swift` 839-841).
    pub fn can_mark_workspace_unread(&self, tab_ids: &[&str]) -> bool {
        tab_ids.iter().any(|id| !self.workspace_is_unread(id))
    }

    /// Mirrors Swift `hasUnreadNotification(forTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 843-845).
    pub fn has_unread_notification(&self, tab_id: &str, surface_id: Option<&str>) -> bool {
        self.indexes.unread_by_tab_surface.contains(&TabSurfaceKey {
            tab_id: tab_id.to_string(),
            surface_id: surface_id.map(|s| s.to_string()),
        })
    }

    /// Mirrors Swift `hasUnreadNotificationRequiringPaneFlash(forTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 847-853).
    pub fn has_unread_requiring_pane_flash(&self, tab_id: &str, surface_id: Option<&str>) -> bool {
        self.notifications.iter().any(|notification| {
            notification.matches(tab_id, surface_id)
                && !notification.is_read
                && notification.pane_flash
        })
    }

    /// Mirrors Swift `hasVisibleNotificationIndicator(forTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 855-858).
    pub fn has_visible_notification_indicator(
        &self,
        tab_id: &str,
        surface_id: Option<&str>,
    ) -> bool {
        self.has_unread_notification(tab_id, surface_id)
            || self
                .focused_read_indicator_by_tab_id
                .get(tab_id)
                .map(|s| Some(s.as_str()) == surface_id)
                .unwrap_or(false)
    }

    /// Mirrors Swift `latestNotification(forTabId:)`
    /// (`TerminalNotificationStore.swift` 860-862).
    pub fn latest_notification_for_tab(&self, tab_id: &str) -> Option<&TerminalNotification> {
        self.indexes.latest_by_tab_id.get(tab_id)
    }

    /// Mirrors Swift `notifications(forTabId:surfaceId:)`
    /// (`TerminalNotificationStore.swift` 864-866).
    pub fn notifications_for_tab_surface(
        &self,
        tab_id: &str,
        surface_id: Option<&str>,
    ) -> Vec<&TerminalNotification> {
        self.notifications
            .iter()
            .filter(|item| item.matches(tab_id, surface_id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification(id: &str, created_at: i64) -> TerminalNotification {
        TerminalNotification {
            id: id.to_owned(),
            tab_id: "tab".into(),
            surface_id: Some("surface".into()),
            panel_id: None,
            title: "Title".into(),
            subtitle: "Subtitle".into(),
            body: "Body".into(),
            created_at,
            is_read: false,
            pane_flash: true,
            click_action: None,
        }
    }

    fn unread(id: &str, tab: &str, surface: Option<&str>, created_at: i64) -> TerminalNotification {
        TerminalNotification {
            id: id.to_owned(),
            tab_id: tab.to_owned(),
            surface_id: surface.map(|s| s.to_owned()),
            panel_id: None,
            title: "Title".into(),
            subtitle: String::new(),
            body: "Body".into(),
            created_at,
            is_read: false,
            pane_flash: true,
            click_action: None,
        }
    }

    // --- legacy reducer (unchanged behavior) --------------------------------

    #[test]
    fn reducer_tracks_unread_counts() {
        let mut state = NotificationState::default();
        state.record(notification("a", 1));
        state.record(notification("b", 2));
        assert_eq!(state.unread_count, 2);
        assert_eq!(state.notifications[0].id, "b");

        state.mark_read("b");
        assert_eq!(state.unread_count, 1);

        state.remove("a");
        assert_eq!(state.unread_count, 0);
    }

    // --- TerminalNotification.matches ---------------------------------------

    #[test]
    fn matches_surface_or_panel() {
        let mut n = unread("a", "tab", Some("s1"), 1);
        n.panel_id = Some("p1".into());
        assert!(n.matches("tab", Some("s1")));
        assert!(n.matches("tab", Some("p1")));
        assert!(!n.matches("tab", Some("other")));
        assert!(!n.matches("other", Some("s1")));
        // None target requires both surface and panel to be nil
        assert!(!n.matches("tab", None));

        let workspace = unread("b", "tab", None, 1);
        assert!(workspace.matches("tab", None));
    }

    // --- record / supersede -------------------------------------------------

    #[test]
    fn record_supersedes_same_tab_surface() {
        let mut store = NotificationStore::new();
        let cleared = store.record(unread("a", "tab", Some("s"), 1), false);
        assert!(cleared.is_empty());
        let cleared = store.record(unread("b", "tab", Some("s"), 2), false);
        assert_eq!(cleared, vec!["a".to_string()]);
        // only the newest survives, front-inserted
        assert_eq!(store.notifications().len(), 1);
        assert_eq!(store.notifications()[0].id, "b");
        assert_eq!(store.unread_notification_count(), 1);
    }

    #[test]
    fn record_keeps_distinct_surfaces() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s1"), 1), false);
        store.record(unread("b", "tab", Some("s2"), 2), false);
        assert_eq!(store.notifications().len(), 2);
        // newest front-inserted
        assert_eq!(store.notifications()[0].id, "b");
        assert_eq!(store.unread_count_for_tab("tab"), 2);
    }

    #[test]
    fn record_clears_manual_unread_for_tab() {
        let mut store = NotificationStore::new();
        store.mark_unread_for_tab("tab");
        assert!(store.has_manual_unread("tab"));
        store.record(unread("a", "tab", Some("s"), 1), false);
        assert!(!store.has_manual_unread("tab"));
    }

    #[test]
    fn record_suppressed_unread_sets_focused_indicator() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s"), 1), true);
        assert_eq!(store.focused_read_indicator_surface_id("tab"), Some("s"));
        // a record for a different surface clears the stale indicator
        store.record(unread("b", "tab", Some("s2"), 2), false);
        assert_eq!(store.focused_read_indicator_surface_id("tab"), None);
    }

    // --- build_indexes ------------------------------------------------------

    #[test]
    fn index_counts_panel_surface_separately() {
        let mut store = NotificationStore::new();
        let mut n = unread("a", "tab", Some("s"), 1);
        n.panel_id = Some("p".into());
        store.record(n, false);
        assert!(store.has_unread_notification("tab", Some("s")));
        assert!(store.has_unread_notification("tab", Some("p")));
        assert!(!store.has_unread_notification("tab", Some("other")));
    }

    #[test]
    fn latest_by_tab_includes_read_entries() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s1"), 1), false);
        store.record(unread("b", "tab", Some("s2"), 2), false);
        // b is newest and front; latest is b
        assert_eq!(store.latest_notification_for_tab("tab").unwrap().id, "b");
    }

    // --- mark read / unread -------------------------------------------------

    #[test]
    fn mark_read_guards_and_returns_cleared_id() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s"), 1), false);
        assert_eq!(store.mark_read("a"), Some("a".to_string()));
        assert_eq!(store.unread_notification_count(), 0);
        // already read → no cleared id
        assert_eq!(store.mark_read("a"), None);
        assert_eq!(store.mark_read("missing"), None);
    }

    #[test]
    fn mark_unread_clears_manual_and_restored() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s"), 1), false);
        store.mark_read("a");
        store.restore_unread_indicator("tab");
        store.mark_unread("a");
        assert_eq!(store.unread_notification_count(), 1);
        assert!(!store.has_restored_unread_indicator("tab"));
    }

    #[test]
    fn mark_unread_moves_the_notification_to_the_front() {
        let mut store = NotificationStore::new();
        store.record(unread("old", "tab", Some("s1"), 1), false);
        store.record(unread("new", "tab", Some("s2"), 2), false);
        store.mark_read("old");

        store.mark_unread("old");

        let ids: Vec<&str> = store
            .notifications()
            .iter()
            .map(|notification| notification.id.as_str())
            .collect();
        assert_eq!(ids, vec!["old", "new"]);
    }

    #[test]
    fn mark_read_for_tab_clears_all_and_indicators() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s1"), 1), false);
        store.record(unread("b", "tab", Some("s2"), 2), false);
        store.set_focused_read_indicator("tab", Some("s1"));
        let cleared = store.mark_read_for_tab("tab");
        assert_eq!(cleared.len(), 2);
        assert_eq!(store.unread_notification_count(), 0);
        assert_eq!(store.focused_read_indicator_surface_id("tab"), None);
    }

    #[test]
    fn mark_read_for_tab_surface_only_matches() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s1"), 1), false);
        store.record(unread("b", "tab", Some("s2"), 2), false);
        let cleared = store.mark_read_for_tab_surface("tab", Some("s1"));
        assert_eq!(cleared, vec!["a".to_string()]);
        assert!(store.has_unread_notification("tab", Some("s2")));
        assert!(!store.has_unread_notification("tab", Some("s1")));
    }

    #[test]
    fn mark_all_read_clears_workspace_indicators() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "t1", Some("s"), 1), false);
        store.mark_unread_for_tab("t2");
        let cleared = store.mark_all_read();
        assert_eq!(cleared, vec!["a".to_string()]);
        assert_eq!(store.unread_count(), 0);
        assert!(!store.has_manual_unread("t2"));
    }

    #[test]
    fn mark_latest_as_oldest_unread_reorders() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s1"), 1), false);
        store.record(unread("b", "tab", Some("s2"), 2), false);
        store.mark_read("a");
        store.mark_read("b");
        // now both read; re-mark latest matching s2 as oldest unread
        let id = store.mark_latest_as_oldest_unread("tab", Some("s2"));
        assert_eq!(id, Some("b".to_string()));
        assert_eq!(store.unread_notification_count(), 1);
    }

    #[test]
    fn mark_latest_as_oldest_unread_without_match_sets_manual() {
        let mut store = NotificationStore::new();
        let id = store.mark_latest_as_oldest_unread("tab", None);
        assert_eq!(id, None);
        assert!(store.has_manual_unread("tab"));
    }

    // --- remove / clear -----------------------------------------------------

    #[test]
    fn remove_clears_focused_indicator() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s"), 1), true);
        assert_eq!(store.focused_read_indicator_surface_id("tab"), Some("s"));
        assert!(store.remove("a"));
        assert_eq!(store.focused_read_indicator_surface_id("tab"), None);
        assert!(!store.remove("a"));
    }

    #[test]
    fn clear_for_tab_surface_returns_cleared_ids() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s1"), 1), false);
        store.record(unread("b", "tab", Some("s2"), 2), false);
        let cleared = store.clear_for_tab_surface("tab", Some("s1"));
        assert_eq!(cleared, vec!["a".to_string()]);
        assert_eq!(store.notifications().len(), 1);
        // nothing to clear → empty
        assert!(store
            .clear_for_tab_surface("tab", Some("missing"))
            .is_empty());
    }

    #[test]
    fn clear_for_tab_clears_everything_for_tab() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s1"), 1), false);
        store.record(unread("b", "tab", Some("s2"), 2), false);
        store.record(unread("c", "other", Some("s"), 3), false);
        let cleared = store.clear_for_tab("tab");
        assert_eq!(cleared.len(), 2);
        assert_eq!(store.notifications().len(), 1);
        assert_eq!(store.notifications()[0].id, "c");
    }

    #[test]
    fn clear_all_resets_indicators() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "tab", Some("s"), 1), true);
        store.mark_unread_for_tab("t2");
        let cleared = store.clear_all();
        assert_eq!(cleared, vec!["a".to_string()]);
        assert_eq!(store.unread_count(), 0);
        assert_eq!(store.focused_read_indicator_surface_id("tab"), None);
        assert!(!store.has_manual_unread("t2"));
        // idempotent: nothing left
        assert!(store.clear_all().is_empty());
    }

    // --- rebind / restore ---------------------------------------------------

    #[test]
    fn rebind_surface_moves_notifications_and_indicator() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "src", Some("s"), 1), true);
        store.rebind_surface("src", "dst", "s");
        assert!(store.has_unread_notification("dst", Some("s")));
        assert!(!store.has_unread_notification("src", Some("s")));
        assert_eq!(store.focused_read_indicator_surface_id("src"), None);
        assert_eq!(store.focused_read_indicator_surface_id("dst"), Some("s"));
    }

    #[test]
    fn restore_session_reassigns_duplicate_ids() {
        let mut store = NotificationStore::new();
        // existing notification in a different tab with id "x"
        store.record(unread("x", "other", Some("s"), 5), false);
        // restored set for "tab" collides with id "x"
        let restored = vec![
            unread("x", "tab", Some("s"), 1),
            unread("y", "tab", Some("s2"), 2),
        ];
        store.restore_session(restored, "tab");
        // the colliding id was reassigned; all ids stay unique
        let ids: HashSet<&str> = store
            .notifications()
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(ids.len(), store.notifications().len());
        assert!(store
            .notifications()
            .iter()
            .any(|n| n.id == "x" && n.tab_id == "other"));
        assert!(store
            .notifications()
            .iter()
            .any(|n| n.id == "x#1" && n.tab_id == "tab"));
    }

    #[test]
    fn restore_session_sorts_newest_first() {
        let mut store = NotificationStore::new();
        let restored = vec![
            unread("a", "tab", Some("s1"), 1),
            unread("b", "tab", Some("s2"), 3),
            unread("c", "tab", Some("s3"), 2),
        ];
        store.restore_session(restored, "tab");
        let ids: Vec<&str> = store
            .notifications()
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert_eq!(ids, vec!["b", "c", "a"]);
    }

    // --- focused-read indicator transitions ---------------------------------

    #[test]
    fn focused_indicator_set_clear_and_visible() {
        let mut store = NotificationStore::new();
        store.set_focused_read_indicator("tab", Some("s"));
        assert!(store.has_visible_notification_indicator("tab", Some("s")));
        // None surface never sets
        store.set_focused_read_indicator("t2", None);
        assert_eq!(store.focused_read_indicator_surface_id("t2"), None);
        // clear with mismatched surface is a no-op
        store.clear_focused_read_indicator("tab", Some("other"));
        assert_eq!(store.focused_read_indicator_surface_id("tab"), Some("s"));
        store.clear_focused_read_indicator("tab", Some("s"));
        assert_eq!(store.focused_read_indicator_surface_id("tab"), None);
    }

    #[test]
    fn clear_focused_if_surface_changed() {
        let mut store = NotificationStore::new();
        store.set_focused_read_indicator("tab", Some("s"));
        store.clear_focused_read_indicator_if_surface_changed("tab", Some("s"));
        assert_eq!(store.focused_read_indicator_surface_id("tab"), Some("s"));
        store.clear_focused_read_indicator_if_surface_changed("tab", Some("other"));
        assert_eq!(store.focused_read_indicator_surface_id("tab"), None);
    }

    // --- counts -------------------------------------------------------------

    #[test]
    fn total_unread_count_includes_workspace_indicators() {
        let mut store = NotificationStore::new();
        store.record(unread("a", "t1", Some("s"), 1), false);
        store.mark_unread_for_tab("t2");
        // 1 notification entry + 1 workspace indicator
        assert_eq!(store.unread_notification_count(), 1);
        assert_eq!(store.unread_count(), 2);
        assert_eq!(store.unread_count_for_tab("t2"), 1);
        assert!(store.workspace_is_unread("t2"));
    }

    #[test]
    fn pane_flash_requirement() {
        let mut store = NotificationStore::new();
        let mut n = unread("a", "tab", Some("s"), 1);
        n.pane_flash = false;
        store.record(n, false);
        assert!(!store.has_unread_requiring_pane_flash("tab", Some("s")));
        store.record(unread("b", "tab", Some("s2"), 2), false);
        assert!(store.has_unread_requiring_pane_flash("tab", Some("s2")));
    }

    // --- cooldown -----------------------------------------------------------

    fn add_request(
        id: &str,
        created_at: i64,
        key: Option<&str>,
        interval: Option<i64>,
    ) -> AddNotificationRequest {
        AddNotificationRequest {
            request: NotificationRequest {
                id: id.to_owned(),
                tab_id: "tab".into(),
                surface_id: Some("s".into()),
                panel_id: None,
                title: "T".into(),
                subtitle: String::new(),
                body: "B".into(),
                created_at,
                click_action: None,
            },
            cooldown_key: key.map(|k| k.to_owned()),
            cooldown_interval: interval,
        }
    }

    #[test]
    fn add_notification_debounces_within_interval() {
        let mut store = NotificationStore::new();
        let effects = TerminalNotificationPolicyEffects::default();
        assert!(store
            .add_notification(add_request("a", 100, Some("k"), Some(50)), &effects, false)
            .is_some());
        // within the interval → skipped
        assert!(store
            .add_notification(add_request("b", 120, Some("k"), Some(50)), &effects, false)
            .is_none());
        // past the interval → allowed
        assert!(store
            .add_notification(add_request("c", 200, Some("k"), Some(50)), &effects, false)
            .is_some());
    }

    #[test]
    fn add_notification_records_and_reports_delivery() {
        let mut store = NotificationStore::new();
        let effects = TerminalNotificationPolicyEffects::default();
        let outcome = store
            .add_notification(add_request("a", 1, None, None), &effects, false)
            .unwrap();
        assert!(outcome.recorded);
        assert_eq!(outcome.delivery, DeliveryDecision::Desktop);
        assert_eq!(store.unread_notification_count(), 1);

        // suppressed delivery
        let outcome = store
            .add_notification(add_request("b", 2, None, None), &effects, true)
            .unwrap();
        assert_eq!(outcome.delivery, DeliveryDecision::Suppressed);
    }

    #[test]
    fn effects_only_no_effect_rolls_back_cooldown() {
        let mut store = NotificationStore::new();
        // effects with nothing on → effects-only path, no deliverable effect
        let effects = TerminalNotificationPolicyEffects {
            record: false,
            mark_unread: false,
            reorder_workspace: false,
            desktop: false,
            sound: false,
            command: false,
            pane_flash: false,
        };
        let outcome = store
            .add_notification(add_request("a", 100, Some("k"), Some(50)), &effects, false)
            .unwrap();
        assert!(!outcome.recorded);
        assert_eq!(outcome.delivery, DeliveryDecision::None);
        // the reservation rolled back, so an immediate second add is NOT skipped
        let second =
            store.add_notification(add_request("b", 110, Some("k"), Some(50)), &effects, false);
        assert!(second.is_some());
    }

    #[test]
    fn apply_effects_only_does_not_record() {
        let mut store = NotificationStore::new();
        let effects = TerminalNotificationPolicyEffects {
            record: false,
            ..TerminalNotificationPolicyEffects::default()
        };
        let outcome = store.apply_notification(
            NotificationRequest {
                id: "a".into(),
                tab_id: "tab".into(),
                surface_id: Some("s".into()),
                panel_id: None,
                title: "T".into(),
                subtitle: String::new(),
                body: "B".into(),
                created_at: 1,
                click_action: None,
            },
            &effects,
            false,
        );
        assert!(!outcome.recorded);
        assert!(outcome.reorder_workspace);
        assert_eq!(store.notifications().len(), 0);
    }

    // --- sort precedes ------------------------------------------------------

    #[test]
    fn sort_precedes_newest_then_id() {
        let a = unread("a", "tab", None, 2);
        let b = unread("b", "tab", None, 1);
        assert!(notification_sort_precedes(&a, &b)); // a newer
        let c = unread("c", "tab", None, 5);
        let d = unread("d", "tab", None, 5);
        assert!(notification_sort_precedes(&c, &d)); // tie → id asc
        assert!(!notification_sort_precedes(&d, &c));
    }

    // --- click action serde -------------------------------------------------

    #[test]
    fn click_action_round_trips() {
        let action = NotificationClickAction::RevealInExplorer {
            path: "C:/x".into(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("reveal_in_explorer"));
        let decoded: NotificationClickAction = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, action);
    }

    #[test]
    fn notification_serde_defaults_pane_flash_true() {
        // a legacy payload without pane_flash/click_action still decodes
        let json = r#"{"id":"a","tab_id":"t","title":"T","subtitle":"","body":"B","created_at":1}"#;
        let n: TerminalNotification = serde_json::from_str(json).unwrap();
        assert!(n.pane_flash);
        assert_eq!(n.click_action, None);
        assert!(!n.is_read);
    }
}
