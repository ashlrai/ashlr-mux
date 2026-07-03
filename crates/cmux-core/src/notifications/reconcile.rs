//! The dismissed-tombstone ring and the phone reconcile-sweep classifier.
//!
//! Ported from `cmux/Sources/TerminalNotificationStore.swift` 276-353: the
//! bounded FIFO tombstone ring (`dismissedTombstoneIDs`/`dismissedTombstoneOrder`,
//! capacity 512) plus `reconcileHandledNotificationIDs`.
//!
//! The Swift store write-through persists the ring to `UserDefaults`. Here that
//! persistence is modeled as pure [`DismissedTombstoneRing::to_ids`] /
//! [`DismissedTombstoneRing::from_ids`] so the host owns storage (the Windows
//! equivalent of the `UserDefaults` seam).
//!
//! DEVIATION: Swift stores `UUID`; the Rust notification model uses `String`
//! ids, so this ring holds `String`. Swift `loadDismissedTombstonesIfNeeded`
//! drops non-parseable stored strings via `UUID(uuidString:)`; there is no id
//! parse here, so [`from_ids`](DismissedTombstoneRing::from_ids) keeps every id
//! (deduped, insertion order preserved) — the classification semantics are
//! unchanged.

use std::collections::HashSet;

/// Recently dismissed/cleared notification ids, kept so a phone's foreground
/// reconcile sweep can classify a banner as handled even after the entry left
/// the store. Bounded ring: oldest evicted past [`Self::CAPACITY`].
#[derive(Debug, Clone, Default)]
pub struct DismissedTombstoneRing {
    ids: HashSet<String>,
    order: Vec<String>,
}

impl DismissedTombstoneRing {
    /// Verbatim from Swift `dismissedTombstoneCapacity`
    /// (`TerminalNotificationStore.swift` 279).
    pub const CAPACITY: usize = 512;

    pub fn new() -> Self {
        Self::default()
    }

    /// Record newly dismissed ids, inserting the new ones and evicting the
    /// oldest past [`Self::CAPACITY`] from the front.
    ///
    /// Verbatim port of Swift `recordDismissTombstones(ids:)`
    /// (`TerminalNotificationStore.swift` 291-307), excluding the `UserDefaults`
    /// write-through (the host persists via [`Self::to_ids`]).
    pub fn record(&mut self, ids: &[String]) {
        for id in ids {
            if self.ids.insert(id.clone()) {
                self.order.push(id.clone());
            }
        }
        let overflow = self.order.len().saturating_sub(Self::CAPACITY);
        if overflow > 0 {
            for stale in &self.order[0..overflow] {
                self.ids.remove(stale);
            }
            self.order.drain(0..overflow);
        }
    }

    /// Whether the ring currently holds `id`.
    pub fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }

    /// The persisted ring contents in insertion order (Swift
    /// `dismissedTombstoneOrder.map(\.uuidString)`).
    pub fn to_ids(&self) -> Vec<String> {
        self.order.clone()
    }

    /// Rehydrate the ring from persisted ids, deduping while preserving order.
    ///
    /// Mirrors Swift `loadDismissedTombstonesIfNeeded`
    /// (`TerminalNotificationStore.swift` 282-289).
    pub fn from_ids(ids: Vec<String>) -> Self {
        let mut ring = Self::default();
        for id in ids {
            if ring.ids.insert(id.clone()) {
                ring.order.push(id);
            }
        }
        ring
    }

    /// Classify which delivered banner ids this host has handled: a known
    /// notification is handled iff read; an unknown one is handled iff
    /// tombstoned. An unread known id is never handled even when an older
    /// tombstone exists (a `markUnread` after a dismiss resurrects it).
    ///
    /// Verbatim port of Swift `reconcileHandledNotificationIDs(deliveredIDs:)`
    /// (`TerminalNotificationStore.swift` 338-353). Since the Rust
    /// `NotificationStore` owns the entries, the caller supplies the derived
    /// `known_ids` / `read_ids` sets (Swift builds them inline from
    /// `notifications`).
    pub fn reconcile_handled(
        &self,
        delivered_ids: &[String],
        known_ids: &HashSet<String>,
        read_ids: &HashSet<String>,
    ) -> Vec<String> {
        delivered_ids
            .iter()
            .filter(|id| {
                if known_ids.contains(*id) {
                    read_ids.contains(*id)
                } else {
                    self.ids.contains(*id)
                }
            })
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(values: &[&str]) -> HashSet<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    /// Oracle: `mobileNotificationReconcileClassifiesHandledAndReportsUnreadCount`
    /// — known-read and unknown-tombstoned report handled; unread and foreign
    /// ids are left alone, in delivered order.
    #[test]
    fn classifies_known_read_and_unknown_tombstoned() {
        let mut ring = DismissedTombstoneRing::new();
        ring.record(&ids(&["removed"])); // user-driven removal tombstones the id

        let known = set(&["read", "unread"]);
        let read = set(&["read"]);
        let delivered = ids(&["read", "unread", "removed", "foreign"]);

        assert_eq!(
            ring.reconcile_handled(&delivered, &known, &read),
            ids(&["read", "removed"])
        );
    }

    /// Oracle: `mobileNotificationReconcileEmptyDeliveredIsBadgeOnlySync`.
    #[test]
    fn empty_delivered_reports_nothing_handled() {
        let ring = DismissedTombstoneRing::new();
        let known = set(&["unread"]);
        let read = HashSet::new();
        assert_eq!(
            ring.reconcile_handled(&[], &known, &read),
            Vec::<String>::new()
        );
    }

    /// Oracle: `reconcileUnreadEntryBeatsStaleDismissTombstone` — markRead
    /// tombstones, then a currently-unread id is never handled even with a
    /// stale tombstone.
    #[test]
    fn unread_entry_beats_stale_tombstone() {
        let mut ring = DismissedTombstoneRing::new();
        ring.record(&ids(&["n"])); // markRead recorded a tombstone

        // Still read-in-store: handled.
        assert_eq!(
            ring.reconcile_handled(&ids(&["n"]), &set(&["n"]), &set(&["n"])),
            ids(&["n"])
        );

        // markUnread: known but not read → not handled despite the tombstone.
        assert_eq!(
            ring.reconcile_handled(&ids(&["n"]), &set(&["n"]), &HashSet::new()),
            Vec::<String>::new()
        );
    }

    /// Oracle: `dismissTombstonesSurviveStoreReload` — a persisted-and-reloaded
    /// tombstone still reconciles when the entry left the store entirely.
    #[test]
    fn tombstones_survive_reload() {
        let mut ring = DismissedTombstoneRing::new();
        ring.record(&ids(&["n"]));
        let persisted = ring.to_ids();

        // The analogue of a relaunch: drop the in-memory ring, re-read persisted.
        let reloaded = DismissedTombstoneRing::from_ids(persisted);

        // Entry left the store: unknown + tombstoned → handled.
        assert_eq!(
            reloaded.reconcile_handled(&ids(&["n"]), &HashSet::new(), &HashSet::new()),
            ids(&["n"])
        );
    }

    #[test]
    fn ring_is_bounded_fifo_at_capacity() {
        let mut ring = DismissedTombstoneRing::new();
        let all: Vec<String> = (0..DismissedTombstoneRing::CAPACITY + 10)
            .map(|n| format!("id-{n}"))
            .collect();
        ring.record(&all);

        assert_eq!(ring.to_ids().len(), DismissedTombstoneRing::CAPACITY);
        // Oldest 10 evicted from the front.
        assert!(!ring.contains("id-0"));
        assert!(!ring.contains("id-9"));
        assert!(ring.contains("id-10"));
        assert!(ring.contains(&format!("id-{}", DismissedTombstoneRing::CAPACITY + 9)));
        assert_eq!(ring.to_ids().first().map(String::as_str), Some("id-10"));
    }

    #[test]
    fn record_dedupes_without_reordering() {
        let mut ring = DismissedTombstoneRing::new();
        ring.record(&ids(&["a", "b"]));
        ring.record(&ids(&["a", "c"])); // "a" already present → not re-appended
        assert_eq!(ring.to_ids(), ids(&["a", "b", "c"]));
    }

    #[test]
    fn from_ids_dedupes_preserving_order() {
        let ring = DismissedTombstoneRing::from_ids(ids(&["a", "b", "a", "c"]));
        assert_eq!(ring.to_ids(), ids(&["a", "b", "c"]));
    }
}
