//! Port of `State/CommandPaletteWindowStore.swift` (pure per-window state
//! machine) plus the co-portable `Snapshot/CommandPaletteDebugSnapshot.swift`
//! and `Snapshot/CommandPaletteDebugResultRow.swift` value structs.
//!
//! Owns the per-window command-palette state for every main window, keyed by a
//! window identifier ([`Uuid`]). Holds visibility, pending-open,
//! escape-suppression, selection, and debug-snapshot state.
//!
//! Timing is expressed in `ProcessInfo.processInfo.systemUptime` seconds,
//! passed in by callers as `now` so the logic stays pure and testable
//! (Swift `CommandPaletteWindowStore.swift:12-13`).
//!
//! DIVERGENCE (host-bound, omitted): the Swift type is `@MainActor
//! @Observable` so SwiftUI observes mutations. This headless port drops both —
//! it is a plain owned struct with no observation and no AppKit; the windowing
//! host resolves `NSWindow` → `Uuid` and drives this store.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

/// A single rendered result row captured for command-palette debug inspection.
///
/// Port of `Snapshot/CommandPaletteDebugResultRow.swift:5-31`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPaletteDebugResultRow {
    /// Stable identifier of the command backing the row.
    pub command_id: String,
    /// User-visible title of the row.
    pub title: String,
    /// Optional shortcut hint shown on the trailing edge of the row.
    pub shortcut_hint: Option<String>,
    /// Optional trailing label (for example a scope or status badge).
    pub trailing_label: Option<String>,
    /// Fuzzy-match score that ordered the row in the result list.
    pub score: i64,
}

impl CommandPaletteDebugResultRow {
    /// Creates a debug result row (Swift `init`, `CommandPaletteDebugResultRow.swift:18-30`).
    pub fn new(
        command_id: String,
        title: String,
        shortcut_hint: Option<String>,
        trailing_label: Option<String>,
        score: i64,
    ) -> Self {
        Self {
            command_id,
            title,
            shortcut_hint,
            trailing_label,
            score,
        }
    }
}

/// A point-in-time snapshot of the command-palette contents for a window.
///
/// Port of `Snapshot/CommandPaletteDebugSnapshot.swift:5-22`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPaletteDebugSnapshot {
    /// The query string currently driving result matching.
    pub query: String,
    /// The active palette mode (for example `commands` or `rename_input`).
    pub mode: String,
    /// The rendered result rows in display order.
    pub results: Vec<CommandPaletteDebugResultRow>,
}

impl CommandPaletteDebugSnapshot {
    /// Creates a debug snapshot (Swift `init`, `CommandPaletteDebugSnapshot.swift:14-18`).
    pub fn new(query: String, mode: String, results: Vec<CommandPaletteDebugResultRow>) -> Self {
        Self {
            query,
            mode,
            results,
        }
    }

    /// An empty snapshot used as the default for windows with no palette state.
    ///
    /// Swift `static let empty` (`CommandPaletteDebugSnapshot.swift:21`):
    /// `query: ""`, `mode: "commands"`, `results: []`.
    pub fn empty() -> Self {
        Self {
            query: String::new(),
            mode: "commands".to_string(),
            results: Vec::new(),
        }
    }
}

/// The outcome of pruning a single stale pending-open entry, for debug logging.
///
/// Port of Swift `enum PrunedPendingOpen` (`CommandPaletteWindowStore.swift:70-75`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PrunedPendingOpen {
    /// The entry was pruned because it had no recorded request timestamp.
    MissingTimestamp {
        /// The window whose pending-open entry was pruned.
        window_id: Uuid,
    },
    /// The entry was pruned because it exceeded `pending_open_max_age`.
    Stale {
        /// The window whose pending-open entry was pruned.
        window_id: Uuid,
        /// The age (`now - requested_at`) that exceeded the max.
        age: f64,
    },
}

/// The result of a visibility update, surfacing the prior value and whether the
/// in-flight pending-open request was retained.
///
/// Port of Swift `struct VisibilityUpdate` (`CommandPaletteWindowStore.swift:178-183`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisibilityUpdate {
    /// Whether the palette was visible before this update.
    pub was_visible: bool,
    /// Whether a pending-open request was retained despite a false→false update.
    pub retained_pending: bool,
}

/// Owns the per-window command-palette state for every main window.
///
/// Port of `CommandPaletteWindowStore.swift:16-234`.
#[derive(Debug, Default)]
pub struct CommandPaletteWindowStore {
    visibility_by_window_id: HashMap<Uuid, bool>,
    pending_open_by_window_id: HashMap<Uuid, bool>,
    recent_request_at_by_window_id: HashMap<Uuid, f64>,
    escape_suppression_by_window_id: HashSet<Uuid>,
    escape_suppression_started_at_by_window_id: HashMap<Uuid, f64>,
    selection_by_window_id: HashMap<Uuid, i64>,
    snapshot_by_window_id: HashMap<Uuid, CommandPaletteDebugSnapshot>,
}

impl CommandPaletteWindowStore {
    /// Grace window during which a recent palette request is still considered fresh.
    ///
    /// Swift `requestGraceInterval` (`CommandPaletteWindowStore.swift:18`).
    pub const REQUEST_GRACE_INTERVAL: f64 = 1.25;
    /// Maximum age before a pending-open request is pruned as stale.
    ///
    /// Swift `pendingOpenMaxAge` (`CommandPaletteWindowStore.swift:20`).
    pub const PENDING_OPEN_MAX_AGE: f64 = 8.0;
    /// Window during which a suppressed escape key-up is consumed.
    ///
    /// Swift `escapeSuppressionInterval` (`CommandPaletteWindowStore.swift:22`).
    pub const ESCAPE_SUPPRESSION_INTERVAL: f64 = 0.35;

    /// Creates an empty store (Swift `init`, `CommandPaletteWindowStore.swift:33`).
    pub fn new() -> Self {
        Self::default()
    }

    // MARK: Registration / teardown

    /// Seeds the baseline palette state for a newly registered window.
    ///
    /// Swift `registerWindow` (`CommandPaletteWindowStore.swift:38-42`).
    pub fn register_window(&mut self, window_id: Uuid) {
        self.visibility_by_window_id.insert(window_id, false);
        self.selection_by_window_id.insert(window_id, 0);
        self.snapshot_by_window_id
            .insert(window_id, CommandPaletteDebugSnapshot::empty());
    }

    /// Removes every piece of palette state for a window being torn down.
    ///
    /// Swift `removeWindow` (`CommandPaletteWindowStore.swift:45-53`).
    pub fn remove_window(&mut self, window_id: Uuid) {
        self.visibility_by_window_id.remove(&window_id);
        self.pending_open_by_window_id.remove(&window_id);
        self.recent_request_at_by_window_id.remove(&window_id);
        self.escape_suppression_by_window_id.remove(&window_id);
        self.escape_suppression_started_at_by_window_id
            .remove(&window_id);
        self.selection_by_window_id.remove(&window_id);
        self.snapshot_by_window_id.remove(&window_id);
    }

    // MARK: Pending-open

    /// Marks a window as having requested a palette open at `now`.
    ///
    /// Swift `markOpenRequested` (`CommandPaletteWindowStore.swift:58-61`).
    pub fn mark_open_requested(&mut self, window_id: Uuid, now: f64) {
        self.pending_open_by_window_id.insert(window_id, true);
        self.recent_request_at_by_window_id.insert(window_id, now);
    }

    /// Clears the pending-open request for a window.
    ///
    /// Swift `clearPendingOpen` (`CommandPaletteWindowStore.swift:64-67`).
    pub fn clear_pending_open(&mut self, window_id: Uuid) {
        self.pending_open_by_window_id.remove(&window_id);
        self.recent_request_at_by_window_id.remove(&window_id);
    }

    /// Prunes pending-open entries older than `PENDING_OPEN_MAX_AGE`.
    ///
    /// Returns the entries pruned, so the caller can emit debug logs that match
    /// the previous inline behavior.
    ///
    /// Swift `pruneExpiredPendingOpenStates` (`CommandPaletteWindowStore.swift:81-98`).
    pub fn prune_expired_pending_open_states(&mut self, now: f64) -> Vec<PrunedPendingOpen> {
        let mut pruned: Vec<PrunedPendingOpen> = Vec::new();
        // Swift iterates over `Array(pendingOpenByWindowId.keys)`, a snapshot of
        // keys taken before mutation.
        let window_ids: Vec<Uuid> = self.pending_open_by_window_id.keys().copied().collect();
        for window_id in window_ids {
            if self.pending_open_by_window_id.get(&window_id) != Some(&true) {
                continue;
            }
            let Some(&requested_at) = self.recent_request_at_by_window_id.get(&window_id) else {
                self.pending_open_by_window_id.remove(&window_id);
                pruned.push(PrunedPendingOpen::MissingTimestamp { window_id });
                continue;
            };
            let age = now - requested_at;
            if age <= Self::PENDING_OPEN_MAX_AGE {
                continue;
            }
            self.pending_open_by_window_id.remove(&window_id);
            self.recent_request_at_by_window_id.remove(&window_id);
            pruned.push(PrunedPendingOpen::Stale { window_id, age });
        }
        pruned
    }

    /// Whether a window has a live pending-open request after pruning stale entries.
    ///
    /// Swift `isPendingOpen` (`CommandPaletteWindowStore.swift:101-104`).
    pub fn is_pending_open(&mut self, window_id: Uuid, now: f64) -> bool {
        let _ = self.prune_expired_pending_open_states(now);
        self.pending_open_by_window_id.get(&window_id) == Some(&true)
    }

    /// Raw pending-open flag without pruning.
    ///
    /// Swift `isPendingOpenRaw` (`CommandPaletteWindowStore.swift:107-109`).
    pub fn is_pending_open_raw(&self, window_id: Uuid) -> bool {
        self.pending_open_by_window_id.get(&window_id) == Some(&true)
    }

    /// The age of a recent, still-fresh palette request, or `None` when none applies.
    ///
    /// Swift `recentRequestAge` (`CommandPaletteWindowStore.swift:112-127`).
    pub fn recent_request_age(&mut self, window_id: Uuid, now: f64) -> Option<f64> {
        let _ = self.prune_expired_pending_open_states(now);
        if self.pending_open_by_window_id.get(&window_id) != Some(&true) {
            self.recent_request_at_by_window_id.remove(&window_id);
            return None;
        }
        let Some(&started_at) = self.recent_request_at_by_window_id.get(&window_id) else {
            self.pending_open_by_window_id.remove(&window_id);
            return None;
        };
        let age = now - started_at;
        if age <= Self::REQUEST_GRACE_INTERVAL {
            return Some(age);
        }
        None
    }

    /// The first window id with a live pending-open request, if any.
    ///
    /// Swift `firstPendingOpenWindowId` (`CommandPaletteWindowStore.swift:130-132`).
    ///
    /// DIVERGENCE (unordered, parity-preserving): Swift's
    /// `Dictionary.first(where:)` has an unspecified iteration order; Rust's
    /// `HashMap` likewise, so "first" is equally arbitrary — matching Swift's
    /// observable contract (defined only for the empty / single-entry cases the
    /// callers rely on).
    pub fn first_pending_open_window_id(&self) -> Option<Uuid> {
        self.pending_open_by_window_id
            .iter()
            .find(|(_, &open)| open)
            .map(|(&id, _)| id)
    }

    /// Test seam: forces a window's pending-open request to a given age.
    ///
    /// Swift `setPendingOpenAge` (`CommandPaletteWindowStore.swift:135-138`).
    pub fn set_pending_open_age(&mut self, window_id: Uuid, now: f64, age: f64) {
        self.pending_open_by_window_id.insert(window_id, true);
        self.recent_request_at_by_window_id
            .insert(window_id, now - age.max(0.0));
    }

    // MARK: Escape suppression

    /// Begins escape suppression for a window at `now`.
    ///
    /// Swift `beginEscapeSuppression` (`CommandPaletteWindowStore.swift:143-146`).
    pub fn begin_escape_suppression(&mut self, window_id: Uuid, now: f64) {
        self.escape_suppression_by_window_id.insert(window_id);
        self.escape_suppression_started_at_by_window_id
            .insert(window_id, now);
    }

    /// Ends escape suppression for a window.
    ///
    /// Swift `endEscapeSuppression` (`CommandPaletteWindowStore.swift:149-152`).
    pub fn end_escape_suppression(&mut self, window_id: Uuid) {
        self.escape_suppression_by_window_id.remove(&window_id);
        self.escape_suppression_started_at_by_window_id
            .remove(&window_id);
    }

    /// Whether a suppressed escape should be consumed for a window at `now`.
    ///
    /// When suppression has expired the entry is cleaned up as a fallback for a
    /// lost key-up, matching the previous inline behavior.
    ///
    /// Swift `shouldConsumeSuppressedEscape` (`CommandPaletteWindowStore.swift:158-166`).
    pub fn should_consume_suppressed_escape(&mut self, window_id: Uuid, now: f64) -> bool {
        if !self.escape_suppression_by_window_id.contains(&window_id) {
            return false;
        }
        // Swift: `escapeSuppressionStartedAtByWindowId[windowId] ?? 0`.
        let started_at = self
            .escape_suppression_started_at_by_window_id
            .get(&window_id)
            .copied()
            .unwrap_or(0.0);
        if now - started_at <= Self::ESCAPE_SUPPRESSION_INTERVAL {
            return true;
        }
        self.end_escape_suppression(window_id);
        false
    }

    /// Clears escape suppression for every window (fallback when no window resolves).
    ///
    /// Swift `clearAllEscapeSuppression` (`CommandPaletteWindowStore.swift:169-172`).
    pub fn clear_all_escape_suppression(&mut self) {
        self.escape_suppression_by_window_id.clear();
        self.escape_suppression_started_at_by_window_id.clear();
    }

    // MARK: Visibility

    /// Updates a window's visibility, clearing pending-open state on open/close.
    ///
    /// Opening (`false`→`true`) and closing (`true`→`false`) both resolve any
    /// pending-open request. Repeated `false` updates are ignored so a stale
    /// sync cannot erase an in-flight open request.
    ///
    /// Swift `setVisible` (`CommandPaletteWindowStore.swift:191-199`).
    pub fn set_visible(&mut self, visible: bool, window_id: Uuid) -> VisibilityUpdate {
        // Swift `updateValue` returns the OLD value (or nil → false).
        let was_visible = self
            .visibility_by_window_id
            .insert(window_id, visible)
            .unwrap_or(false);
        if visible || was_visible {
            self.pending_open_by_window_id.remove(&window_id);
            self.recent_request_at_by_window_id.remove(&window_id);
        }
        let retained_pending = !visible
            && !was_visible
            && self.pending_open_by_window_id.get(&window_id) == Some(&true);
        VisibilityUpdate {
            was_visible,
            retained_pending,
        }
    }

    /// Whether the palette is marked visible for a window.
    ///
    /// Swift `isVisible` (`CommandPaletteWindowStore.swift:202-204`).
    pub fn is_visible(&self, window_id: Uuid) -> bool {
        self.visibility_by_window_id
            .get(&window_id)
            .copied()
            .unwrap_or(false)
    }

    /// The first window id with the palette currently visible, if any.
    ///
    /// Swift `firstVisibleWindowId` (`CommandPaletteWindowStore.swift:207-209`).
    ///
    /// DIVERGENCE (unordered, parity-preserving): see
    /// [`first_pending_open_window_id`](Self::first_pending_open_window_id).
    pub fn first_visible_window_id(&self) -> Option<Uuid> {
        self.visibility_by_window_id
            .iter()
            .find(|(_, &visible)| visible)
            .map(|(&id, _)| id)
    }

    // MARK: Selection

    /// Sets the clamped selection index for a window.
    ///
    /// Swift `setSelectionIndex` (`CommandPaletteWindowStore.swift:214-216`).
    pub fn set_selection_index(&mut self, index: i64, window_id: Uuid) {
        self.selection_by_window_id
            .insert(window_id, index.max(0));
    }

    /// The selection index for a window, defaulting to zero.
    ///
    /// Swift `selectionIndex` (`CommandPaletteWindowStore.swift:219-221`).
    pub fn selection_index(&self, window_id: Uuid) -> i64 {
        self.selection_by_window_id
            .get(&window_id)
            .copied()
            .unwrap_or(0)
    }

    // MARK: Snapshot

    /// Stores the debug snapshot for a window.
    ///
    /// Swift `setSnapshot` (`CommandPaletteWindowStore.swift:226-228`).
    pub fn set_snapshot(&mut self, snapshot: CommandPaletteDebugSnapshot, window_id: Uuid) {
        self.snapshot_by_window_id.insert(window_id, snapshot);
    }

    /// The debug snapshot for a window, defaulting to empty.
    ///
    /// Swift `snapshot` (`CommandPaletteWindowStore.swift:231-233`).
    pub fn snapshot(&self, window_id: Uuid) -> CommandPaletteDebugSnapshot {
        self.snapshot_by_window_id
            .get(&window_id)
            .cloned()
            .unwrap_or_else(CommandPaletteDebugSnapshot::empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ports of `CommandPaletteWindowStoreTests.swift:8-123` (11 @Test cases),
    // pinned to the Swift-derived expectations.

    /// Swift `registerSeedsBaseline` (`CommandPaletteWindowStoreTests.swift:8-17`).
    #[test]
    fn register_seeds_baseline() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.register_window(id);
        assert!(!store.is_visible(id));
        assert_eq!(store.selection_index(id), 0);
        assert_eq!(store.snapshot(id).mode, "commands");
        assert!(store.snapshot(id).results.is_empty());
    }

    /// Swift `removeClearsState` (`CommandPaletteWindowStoreTests.swift:19-34`).
    #[test]
    fn remove_clears_state() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.register_window(id);
        store.set_visible(true, id);
        store.mark_open_requested(id, 100.0);
        store.begin_escape_suppression(id, 100.0);
        store.set_selection_index(3, id);
        store.remove_window(id);
        assert!(!store.is_visible(id));
        assert!(!store.is_pending_open_raw(id));
        assert_eq!(store.selection_index(id), 0);
        assert_eq!(store.first_visible_window_id(), None);
        assert_eq!(store.first_pending_open_window_id(), None);
    }

    /// Swift `pendingOpenPruning` (`CommandPaletteWindowStoreTests.swift:36-43`).
    #[test]
    fn pending_open_is_live_within_max_age_and_pruned_after() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.mark_open_requested(id, 100.0);
        assert!(store.is_pending_open(id, 100.0 + CommandPaletteWindowStore::PENDING_OPEN_MAX_AGE));
        assert!(
            !store.is_pending_open(id, 100.0 + CommandPaletteWindowStore::PENDING_OPEN_MAX_AGE + 0.01)
        );
    }

    /// Swift `recentRequestAgeWithinGrace` (`CommandPaletteWindowStoreTests.swift:45-53`).
    #[test]
    fn recent_request_age_returns_age_only_within_grace_interval() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.mark_open_requested(id, 100.0);
        let age =
            store.recent_request_age(id, 100.0 + CommandPaletteWindowStore::REQUEST_GRACE_INTERVAL);
        assert_eq!(age, Some(CommandPaletteWindowStore::REQUEST_GRACE_INTERVAL));
        assert_eq!(
            store.recent_request_age(
                id,
                100.0 + CommandPaletteWindowStore::REQUEST_GRACE_INTERVAL + 0.01
            ),
            None
        );
    }

    /// Swift `setPendingOpenAgeSeam` (`CommandPaletteWindowStoreTests.swift:55-63`).
    #[test]
    fn set_pending_open_age_seam_drives_recent_request_age() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.set_pending_open_age(id, 200.0, 1.0);
        assert_eq!(store.recent_request_age(id, 200.0), Some(1.0));
        store.set_pending_open_age(id, 200.0, 6.25);
        assert_eq!(store.recent_request_age(id, 200.0), None);
    }

    /// Swift `escapeSuppression` (`CommandPaletteWindowStoreTests.swift:65-75`).
    #[test]
    fn escape_suppression_consumed_only_within_suppression_window() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.begin_escape_suppression(id, 100.0);
        assert!(store.should_consume_suppressed_escape(
            id,
            100.0 + CommandPaletteWindowStore::ESCAPE_SUPPRESSION_INTERVAL
        ));
        store.begin_escape_suppression(id, 100.0);
        // Past the window: not consumed and cleaned up.
        assert!(!store.should_consume_suppressed_escape(
            id,
            100.0 + CommandPaletteWindowStore::ESCAPE_SUPPRESSION_INTERVAL + 0.01
        ));
        assert!(!store.should_consume_suppressed_escape(id, 100.0));
    }

    /// Swift `falseVisibilityRetainsPending` (`CommandPaletteWindowStoreTests.swift:77-86`).
    #[test]
    fn repeated_false_visibility_retains_an_in_flight_pending_open() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.mark_open_requested(id, 100.0);
        let update = store.set_visible(false, id);
        assert!(!update.was_visible);
        assert!(update.retained_pending);
        assert!(store.is_pending_open_raw(id));
    }

    /// Swift `openCloseClearsPending` (`CommandPaletteWindowStoreTests.swift:88-99`).
    #[test]
    fn opening_then_closing_clears_pending_open() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.mark_open_requested(id, 100.0);
        let open = store.set_visible(true, id);
        assert!(!open.was_visible);
        assert!(!store.is_pending_open_raw(id));
        let close = store.set_visible(false, id);
        assert!(close.was_visible);
        assert!(!store.is_visible(id));
    }

    /// Swift `selectionClamped` (`CommandPaletteWindowStoreTests.swift:101-109`).
    #[test]
    fn selection_index_is_clamped_to_zero() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.set_selection_index(-5, id);
        assert_eq!(store.selection_index(id), 0);
        store.set_selection_index(7, id);
        assert_eq!(store.selection_index(id), 7);
    }

    /// Swift `pruneOutcomes` (`CommandPaletteWindowStoreTests.swift:111-123`).
    #[test]
    fn prune_reports_missing_timestamp_and_stale_outcomes() {
        let mut store = CommandPaletteWindowStore::new();
        let stale = Uuid::new_v4();
        store.mark_open_requested(stale, 0.0);
        let pruned =
            store.prune_expired_pending_open_states(CommandPaletteWindowStore::PENDING_OPEN_MAX_AGE + 1.0);
        assert_eq!(pruned.len(), 1);
        match pruned[0] {
            PrunedPendingOpen::Stale { window_id, .. } => assert_eq!(window_id, stale),
            other => panic!("expected stale outcome, got {other:?}"),
        }
    }

    // Parity-risk edge cases flagged in the lane notes, with hand-computed
    // expectations derived from the Swift formulas.

    /// A stale prune removes BOTH the pending flag and the request timestamp
    /// (`CommandPaletteWindowStore.swift:93-94`). NOTE: the sibling
    /// `.missingTimestamp` branch (`:86-89`) is unreachable through the public
    /// API — every route that sets the pending flag (`mark_open_requested`,
    /// `set_pending_open_age`) also writes a timestamp — so it is not
    /// separately test-covered here, matching the Swift oracle which also never
    /// exercises it.
    #[test]
    fn stale_prune_removes_flag_and_timestamp() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.mark_open_requested(id, 0.0);
        let pruned = store.prune_expired_pending_open_states(9.0);
        assert_eq!(pruned.len(), 1);
        // After a stale prune both the flag and the timestamp are gone, so a
        // fresh recentRequestAge sees nothing.
        assert!(!store.is_pending_open_raw(id));
        assert_eq!(store.recent_request_age(id, 9.0), None);
    }

    /// Boundary: at exactly `PENDING_OPEN_MAX_AGE` the entry is NOT pruned
    /// (Swift guard `age > pendingOpenMaxAge`, `:92`).
    #[test]
    fn pending_open_survives_at_exact_max_age_boundary() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.mark_open_requested(id, 100.0);
        let pruned = store
            .prune_expired_pending_open_states(100.0 + CommandPaletteWindowStore::PENDING_OPEN_MAX_AGE);
        assert!(pruned.is_empty());
        assert!(store.is_pending_open_raw(id));
    }

    /// `recentRequestAge` prunes the pending entry first, so a stale request
    /// (older than `PENDING_OPEN_MAX_AGE`) returns `None` and clears state
    /// (`CommandPaletteWindowStore.swift:113-117`).
    #[test]
    fn recent_request_age_is_none_after_stale_prune() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.mark_open_requested(id, 0.0);
        assert_eq!(store.recent_request_age(id, 100.0), None);
        assert!(!store.is_pending_open_raw(id));
    }

    /// `setVisible(true)` on an already-visible window still clears pending and
    /// reports `was_visible == true` with `retained_pending == false`
    /// (`CommandPaletteWindowStore.swift:192-198`).
    #[test]
    fn set_visible_true_when_already_visible_clears_pending() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        store.set_visible(true, id);
        store.mark_open_requested(id, 100.0);
        let update = store.set_visible(true, id);
        assert!(update.was_visible);
        assert!(!update.retained_pending);
        assert!(!store.is_pending_open_raw(id));
    }

    /// `shouldConsumeSuppressedEscape` returns false for an unknown window
    /// (`CommandPaletteWindowStore.swift:159`).
    #[test]
    fn should_consume_suppressed_escape_false_for_unknown_window() {
        let mut store = CommandPaletteWindowStore::new();
        assert!(!store.should_consume_suppressed_escape(Uuid::new_v4(), 0.0));
    }

    /// `clearAllEscapeSuppression` drops suppression for every window
    /// (`CommandPaletteWindowStore.swift:169-172`).
    #[test]
    fn clear_all_escape_suppression_clears_every_window() {
        let mut store = CommandPaletteWindowStore::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        store.begin_escape_suppression(a, 0.0);
        store.begin_escape_suppression(b, 0.0);
        store.clear_all_escape_suppression();
        assert!(!store.should_consume_suppressed_escape(a, 0.0));
        assert!(!store.should_consume_suppressed_escape(b, 0.0));
    }

    /// The snapshot round-trips through the store unchanged
    /// (`CommandPaletteWindowStore.swift:226-233`).
    #[test]
    fn snapshot_round_trips() {
        let mut store = CommandPaletteWindowStore::new();
        let id = Uuid::new_v4();
        let snapshot = CommandPaletteDebugSnapshot::new(
            "query".to_string(),
            "rename_input".to_string(),
            vec![CommandPaletteDebugResultRow::new(
                "command.rename".to_string(),
                "Rename".to_string(),
                Some("R".to_string()),
                None,
                42,
            )],
        );
        store.set_snapshot(snapshot.clone(), id);
        assert_eq!(store.snapshot(id), snapshot);
    }

    /// An unregistered window yields the empty snapshot default
    /// (`CommandPaletteWindowStore.swift:231-233`).
    #[test]
    fn snapshot_defaults_to_empty() {
        let store = CommandPaletteWindowStore::new();
        assert_eq!(
            store.snapshot(Uuid::new_v4()),
            CommandPaletteDebugSnapshot::empty()
        );
    }
}
