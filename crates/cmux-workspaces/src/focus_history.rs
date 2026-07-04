//! Port of the pure back/forward focus-history state machine from
//! `Packages/macOS/CmuxWorkspaces/Sources/CmuxWorkspaces/Navigation/FocusHistoryModel.swift`
//! (plus its supporting value types `FocusHistoryEntry`, `FocusHistoryRecord`,
//! and the `Navigation/Menu/*` menu types).
//!
//! Covers the `focusHistory` stack + `historyIndex` cursor, the recording
//! suppression depth, the suppressed-selection generation marks, and every
//! record / invalidate / navigate / menu-snapshot / can-navigate computation.
//!
//! DIVERGENCE (host seam threaded per-call, not stored). Swift's
//! `FocusHistoryModel` is `@MainActor @Observable` and holds a `weak var host`
//! set once via `attach(host:)`; the app's per-window `TabManager` is the sole
//! implementer. The pure port drops `@MainActor`/`@Observable`/`attach` and
//! instead threads the host explicitly into each method as a
//! [`FocusHistoryHost`] trait object. This is behaviorally identical for the
//! ported (headless) surface: the only reason Swift needs one isolation domain
//! is the re-entrant `selection didSet` (selecting a workspace synchronously
//! re-enters the model). No headless caller — and none of the ported oracle
//! tests — exercises that re-entrancy, so passing the host per-call preserves
//! the observable interleavings exactly while sidestepping interior mutability.
//! `FocusHistoryHosting.swift:21-60` maps 1:1 to [`FocusHistoryHost`].
//!
//! DIVERGENCE (deterministic `focusedAt`). Swift stamps every new
//! `FocusHistoryRecord` with `Date()` (wall-clock now). The port stamps a
//! monotonically increasing [`FocusedAt`] counter instead — the deterministic
//! analog of "later focus ⇒ larger timestamp" that the recency-merge in
//! [`FocusHistoryMenuSnapshot::recently_focused`] depends on. No model-driven
//! oracle path inspects a model-generated `focused_at` value; only the
//! independently-tested `recently_focused` merge does, and its callers supply
//! explicit timestamps.
//!
//! Signed `i64` indices mirror Swift's `Int` `historyIndex` (which starts at
//! `-1` and is compared against `count - 1`, underflowing to `-1` when empty).

use std::collections::HashSet;

use uuid::Uuid;

/// Ordinal focus timestamp (higher = more recently focused). Mirrors Swift's
/// `Date` only in its ordering role (see module divergence note).
pub type FocusedAt = i64;

/// One focus-history position: a workspace plus the panel that was focused in
/// it (or `None` when only workspace-level focus is known).
///
/// Swift `FocusHistoryEntry.swift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusHistoryEntry {
    /// The workspace the user focused.
    pub workspace_id: Uuid,
    /// The focused panel inside the workspace, when known.
    pub panel_id: Option<Uuid>,
}

impl FocusHistoryEntry {
    /// Creates an entry for a workspace and optional panel.
    pub fn new(workspace_id: Uuid, panel_id: Option<Uuid>) -> Self {
        Self {
            workspace_id,
            panel_id,
        }
    }
}

/// A focus-history entry plus the time the focus landed, as stored in the
/// back/forward stack.
///
/// Swift `FocusHistoryRecord.swift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusHistoryRecord {
    /// The focused workspace/panel position.
    pub entry: FocusHistoryEntry,
    /// When the focus landed (ordinal; see [`FocusedAt`]).
    pub focused_at: FocusedAt,
}

impl FocusHistoryRecord {
    /// Creates a record with an explicit focus timestamp.
    pub fn new(entry: FocusHistoryEntry, focused_at: FocusedAt) -> Self {
        Self { entry, focused_at }
    }
}

/// Which side of the focus-history stack a menu enumerates.
///
/// Swift `Navigation/Menu/FocusHistoryMenuDirection.swift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusHistoryMenuDirection {
    /// Entries older than the current position (the back stack).
    Back,
    /// Entries newer than the current position (the forward stack).
    Forward,
}

/// Whether a menu item sits before or after the current history position.
///
/// Swift `Navigation/Menu/FocusHistoryMenuPosition.swift`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusHistoryMenuPosition {
    /// The item is older than the current position.
    Older,
    /// The item is newer than the current position.
    Newer,
}

/// One navigable focus-history menu row: the underlying entry plus the resolved
/// display titles captured at snapshot time.
///
/// Swift `Navigation/Menu/FocusHistoryMenuItem.swift`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusHistoryMenuItem {
    /// The entry's index in the history stack when the snapshot was taken.
    pub history_index: i64,
    /// The underlying history entry (the raw stored entry, not the resolved one).
    pub entry: FocusHistoryEntry,
    /// The workspace's trimmed display title.
    pub workspace_title: String,
    /// The panel's trimmed display title, when one resolved.
    pub panel_title: Option<String>,
    /// Whether the item is older or newer than the current position.
    pub position: FocusHistoryMenuPosition,
    /// When the focus landed.
    pub focused_at: FocusedAt,
    /// Whether selecting the item can navigate.
    pub is_navigable: bool,
}

/// A point-in-time list of navigable focus-history menu items, with the total
/// count and whether the list was truncated by a `max_item_count`.
///
/// Swift `Navigation/Menu/FocusHistoryMenuSnapshot.swift`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusHistoryMenuSnapshot {
    /// The (possibly truncated) menu items.
    pub items: Vec<FocusHistoryMenuItem>,
    /// The total navigable item count before truncation.
    pub total_item_count: usize,
    /// Whether `items` was truncated to a `max_item_count`.
    pub is_limited: bool,
}

impl FocusHistoryMenuSnapshot {
    /// Creates a snapshot.
    pub fn new(items: Vec<FocusHistoryMenuItem>, total_item_count: usize, is_limited: bool) -> Self {
        Self {
            items,
            total_item_count,
            is_limited,
        }
    }

    /// Merges a back and a forward snapshot into one recency-ordered list (most
    /// recently focused first; ties broken by the later history index),
    /// optionally truncated to `max_item_count`.
    ///
    /// Swift `FocusHistoryMenuSnapshot.recentlyFocused(back:forward:maxItemCount:)`.
    pub fn recently_focused(
        back: &FocusHistoryMenuSnapshot,
        forward: &FocusHistoryMenuSnapshot,
        max_item_count: Option<i64>,
    ) -> FocusHistoryMenuSnapshot {
        let mut items: Vec<FocusHistoryMenuItem> =
            back.items.iter().chain(forward.items.iter()).cloned().collect();
        items.sort_by(|lhs, rhs| {
            if lhs.focused_at == rhs.focused_at {
                // Later history index first.
                rhs.history_index.cmp(&lhs.history_index)
            } else {
                // Larger (more recent) focusedAt first.
                rhs.focused_at.cmp(&lhs.focused_at)
            }
        });
        truncate_items(items, max_item_count)
    }
}

/// Applies the shared `maxItemCount` truncation contract: when `max_item_count`
/// is present, non-negative, and exceeded, keep the first `max_item_count`
/// items and flag `is_limited`; `total_item_count` is always the untruncated
/// count.
fn truncate_items(
    items: Vec<FocusHistoryMenuItem>,
    max_item_count: Option<i64>,
) -> FocusHistoryMenuSnapshot {
    if let Some(max) = max_item_count {
        if max >= 0 && items.len() as i64 > max {
            let total = items.len();
            let kept = items.into_iter().take(max as usize).collect();
            return FocusHistoryMenuSnapshot::new(kept, total, true);
        }
    }
    let total = items.len();
    FocusHistoryMenuSnapshot::new(items, total, false)
}

/// The window-side seam the focus-history model drives: snapshot reads of
/// workspace/panel existence, titles, and remembered focus, plus the
/// synchronous selection/focus mutations a history navigation performs.
///
/// Mirrors Swift `FocusHistoryHosting` (`FocusHistoryHosting.swift:21-60`).
/// Reads return `false`/`None` when the workspace or panel is gone.
pub trait FocusHistoryHost {
    // --- Selection / workspace reads ---

    /// The window's selected workspace id, if any.
    fn selected_workspace_id(&self) -> Option<Uuid>;
    /// Whether the workspace still exists in this window.
    fn workspace_exists(&self, workspace_id: Uuid) -> bool;
    /// Whether the panel still exists in the workspace.
    fn panel_exists(&self, workspace_id: Uuid, panel_id: Uuid) -> bool;
    /// The workspace's display title, or `None` when the workspace is gone.
    fn workspace_title(&self, workspace_id: Uuid) -> Option<String>;
    /// The panel's display title, when one exists.
    fn panel_title(&self, workspace_id: Uuid, panel_id: Uuid) -> Option<String>;
    /// The window-level remembered focused panel for the workspace.
    fn remembered_focused_panel_id(&self, workspace_id: Uuid) -> Option<Uuid>;
    /// The workspace's own focused panel id.
    fn workspace_focused_panel_id(&self, workspace_id: Uuid) -> Option<Uuid>;
    /// The workspace's first panel id ordered by `uuidString` (the legacy
    /// deterministic fallback).
    fn first_panel_id_sorted_by_uuid_string(&self, workspace_id: Uuid) -> Option<Uuid>;

    // --- Navigation mutations ---

    /// Selects the workspace if it is not already selected.
    fn select_workspace(&mut self, workspace_id: Uuid);
    /// Remembers the focused surface for the workspace.
    fn remember_focused_surface(&mut self, workspace_id: Uuid, surface_id: Uuid);
    /// Focuses the panel in the workspace.
    fn focus_panel(&mut self, workspace_id: Uuid, panel_id: Uuid);
    /// Triggers the focus flash on the panel.
    fn trigger_focus_flash(&mut self, workspace_id: Uuid, panel_id: Uuid);
    /// Focuses the selected workspace's panel (the workspace-level fallback).
    fn focus_selected_workspace_panel(&mut self);

    // --- Change propagation ---

    /// Called after any observable history mutation; the host bumps its
    /// published revision counter.
    fn focus_history_revision_did_change(&mut self);
}

/// Per-window focus-history sub-model: the back/forward stack of
/// workspace/panel focus positions, the recording-suppression depth, and the
/// deferred-selection suppression marks.
///
/// Swift `FocusHistoryModel`.
#[derive(Debug, Clone)]
pub struct FocusHistoryModel {
    focus_history: Vec<FocusHistoryRecord>,
    history_index: i64,
    focus_history_recording_suppression_depth: i64,
    focus_history_suppressed_selection_side_effect_generations: HashSet<u64>,
    max_history_size: usize,
    /// Monotonic clock replacing Swift's `Date()` (see module divergence note).
    focused_at_counter: FocusedAt,
}

impl FocusHistoryModel {
    /// Creates a model. `max_history_size` is the legacy stack cap (50).
    pub fn new(max_history_size: usize) -> Self {
        Self {
            focus_history: Vec::new(),
            history_index: -1,
            focus_history_recording_suppression_depth: 0,
            focus_history_suppressed_selection_side_effect_generations: HashSet::new(),
            max_history_size,
            focused_at_counter: 0,
        }
    }

    fn make_record(&mut self, entry: FocusHistoryEntry) -> FocusHistoryRecord {
        let focused_at = self.focused_at_counter;
        self.focused_at_counter += 1;
        FocusHistoryRecord::new(entry, focused_at)
    }

    // MARK: - Suppression

    /// Whether focus changes are currently recorded (recording is not
    /// suppressed). Swift `shouldRecordFocusHistory`.
    pub fn should_record_focus_history(&self) -> bool {
        self.focus_history_recording_suppression_depth == 0
    }

    /// Runs `body` with focus-history recording suppressed (re-entrant). The
    /// body receives the model and host so it can drive further operations, as
    /// the Swift closure captures both from its environment.
    /// Swift `withFocusHistoryRecordingSuppressed`.
    pub fn with_focus_history_recording_suppressed<R>(
        &mut self,
        host: &mut dyn FocusHistoryHost,
        body: impl FnOnce(&mut Self, &mut dyn FocusHistoryHost) -> R,
    ) -> R {
        self.focus_history_recording_suppression_depth += 1;
        let result = body(self, host);
        // `max(0, depth - 1)`.
        self.focus_history_recording_suppression_depth =
            (self.focus_history_recording_suppression_depth - 1).max(0);
        result
    }

    /// Marks a selection-side-effect generation whose deferred side effects must
    /// run with recording suppressed. Swift
    /// `markSuppressedSelectionSideEffectGeneration`.
    pub fn mark_suppressed_selection_side_effect_generation(&mut self, generation: u64) {
        self.focus_history_suppressed_selection_side_effect_generations
            .insert(generation);
    }

    /// Consumes the mark for the generation; returns whether it was set. Swift
    /// `consumeSuppressedSelectionSideEffectGeneration`.
    pub fn consume_suppressed_selection_side_effect_generation(&mut self, generation: u64) -> bool {
        self.focus_history_suppressed_selection_side_effect_generations
            .remove(&generation)
    }

    /// Clears all history state (the window-reset path). Does not bump the host
    /// revision. Swift `reset`.
    pub fn reset(&mut self) {
        self.focus_history.clear();
        self.history_index = -1;
        self.focus_history_recording_suppression_depth = 0;
        self.focus_history_suppressed_selection_side_effect_generations
            .clear();
    }

    // MARK: - Recording

    /// Records a focus landing on the workspace/panel. With
    /// `preserving_forward_branch` the forward stack is kept and the entry is
    /// inserted after the current position (the closed-item restore path).
    /// Swift `recordFocusInHistory(workspaceId:panelId:preservingForwardBranch:)`.
    pub fn record_focus_in_history(
        &mut self,
        host: &mut dyn FocusHistoryHost,
        workspace_id: Uuid,
        panel_id: Option<Uuid>,
        preserving_forward_branch: bool,
    ) {
        if !self.should_record_focus_history() {
            return;
        }
        let entry = FocusHistoryEntry::new(workspace_id, panel_id);
        if !self.focus_history_entry_is_valid(&*host, entry) {
            return;
        }

        if self.history_index >= 0
            && self.history_index < self.focus_history.len() as i64
            && self.focus_history[self.history_index as usize].entry == entry
        {
            return;
        }

        let mut did_mutate_history = false;
        if self.history_index < self.focus_history.len() as i64 - 1 {
            if preserving_forward_branch {
                let insertion_index = (self.history_index + 1).max(0);
                if self.focus_history[insertion_index as usize].entry == entry {
                    let old_history_index = self.history_index;
                    self.history_index = insertion_index;
                    if self.history_index != old_history_index {
                        host.focus_history_revision_did_change();
                    }
                    return;
                }

                let record = self.make_record(entry);
                self.focus_history.insert(insertion_index as usize, record);
                let overflow = (self.focus_history.len() as i64 - self.max_history_size as i64).max(0);
                if overflow > 0 {
                    self.focus_history.drain(0..overflow as usize);
                }
                self.history_index = (insertion_index - overflow).max(-1);
                host.focus_history_revision_did_change();
                return;
            } else {
                // focusHistory = Array(focusHistory.prefix(historyIndex + 1))
                self.focus_history.truncate((self.history_index + 1) as usize);
                did_mutate_history = true;
            }
        }

        if self.focus_history.last().map(|r| r.entry) == Some(entry) {
            self.history_index = self.focus_history.len() as i64 - 1;
            if did_mutate_history {
                host.focus_history_revision_did_change();
            }
            return;
        }

        let record = self.make_record(entry);
        self.focus_history.push(record);
        if self.focus_history.len() > self.max_history_size {
            let overflow = self.focus_history.len() - self.max_history_size;
            self.focus_history.drain(0..overflow);
        }

        self.history_index = self.focus_history.len() as i64 - 1;
        host.focus_history_revision_did_change();
    }

    /// Records the entry when non-`None`; see [`Self::record_focus_in_history`].
    /// Swift `recordFocusInHistory(_:preservingForwardBranch:)`.
    pub fn record_focus_in_history_entry(
        &mut self,
        host: &mut dyn FocusHistoryHost,
        entry: Option<FocusHistoryEntry>,
        preserving_forward_branch: bool,
    ) {
        let Some(entry) = entry else { return };
        self.record_focus_in_history(
            host,
            entry.workspace_id,
            entry.panel_id,
            preserving_forward_branch,
        );
    }

    /// Records an implicit (non-user-initiated) focus: coalesces with the
    /// current entry when it targets the same workspace mid-stack. Swift
    /// `recordImplicitFocusInHistory`.
    pub fn record_implicit_focus_in_history(
        &mut self,
        host: &mut dyn FocusHistoryHost,
        workspace_id: Uuid,
        panel_id: Option<Uuid>,
    ) {
        if !self.should_record_focus_history() {
            return;
        }
        let entry = FocusHistoryEntry::new(workspace_id, panel_id);
        if !self.focus_history_entry_is_valid(&*host, entry) {
            return;
        }

        if self.history_index >= 0
            && self.history_index < self.focus_history.len() as i64 - 1
            && self.focus_history[self.history_index as usize].entry.workspace_id == workspace_id
        {
            if self.focus_history[self.history_index as usize].entry != entry {
                let record = self.make_record(entry);
                self.focus_history[self.history_index as usize] = record;
                host.focus_history_revision_did_change();
            }
            return;
        }

        self.record_focus_in_history(host, workspace_id, panel_id, false);
    }

    // MARK: - Invalidation

    /// Drops the workspace's entries (panel `None`) or bumps the revision so
    /// menus revalidate a panel-level entry. Swift
    /// `invalidateFocusHistoryTarget`.
    pub fn invalidate_focus_history_target(
        &mut self,
        host: &mut dyn FocusHistoryHost,
        workspace_id: Uuid,
        panel_id: Option<Uuid>,
    ) {
        if let Some(panel_id) = panel_id {
            let present = self.focus_history.iter().any(|record| {
                record.entry.workspace_id == workspace_id && record.entry.panel_id == Some(panel_id)
            });
            if !present {
                return;
            }
            host.focus_history_revision_did_change();
            return;
        }

        let old_count = self.focus_history.len();
        if old_count == 0 {
            return;
        }

        let current_index = self.history_index;
        // prefix(max(0, min(currentIndex + 1, oldCount)))
        let prefix_len = (current_index + 1).min(old_count as i64).max(0) as usize;
        let removed_before_or_at_current = self.focus_history[..prefix_len]
            .iter()
            .filter(|record| record.entry.workspace_id == workspace_id)
            .count();
        self.focus_history
            .retain(|record| record.entry.workspace_id != workspace_id);
        if self.focus_history.len() == old_count {
            return;
        }

        self.history_index -= removed_before_or_at_current as i64;
        if self.focus_history.is_empty() {
            self.history_index = -1;
        } else {
            self.history_index = self
                .history_index
                .max(-1)
                .min(self.focus_history.len() as i64 - 1);
        }
        host.focus_history_revision_did_change();
    }

    // MARK: - Resolution

    fn focus_history_entry_is_valid(
        &self,
        host: &dyn FocusHistoryHost,
        entry: FocusHistoryEntry,
    ) -> bool {
        if !host.workspace_exists(entry.workspace_id) {
            return false;
        }
        let Some(panel_id) = entry.panel_id else {
            return true;
        };
        host.panel_exists(entry.workspace_id, panel_id)
    }

    /// Resolves the entry's panel against the workspace's current panels using
    /// the legacy fallback chain (entry panel, remembered panel,
    /// workspace-focused panel, deterministic first). Swift
    /// `resolvedFocusHistoryPanelId(for:)`.
    pub fn resolved_focus_history_panel_id(
        &self,
        host: &dyn FocusHistoryHost,
        entry: FocusHistoryEntry,
    ) -> Option<Uuid> {
        let workspace_id = entry.workspace_id;

        if let Some(panel_id) = entry.panel_id {
            if host.panel_exists(workspace_id, panel_id) {
                return Some(panel_id);
            }
        }

        if let Some(remembered) = host.remembered_focused_panel_id(workspace_id) {
            if host.panel_exists(workspace_id, remembered) {
                return Some(remembered);
            }
        }

        if let Some(workspace_panel) = host.workspace_focused_panel_id(workspace_id) {
            if host.panel_exists(workspace_id, workspace_panel) {
                return Some(workspace_panel);
            }
        }

        host.first_panel_id_sorted_by_uuid_string(workspace_id)
    }

    /// The entry for the current selection, if any workspace is selected. Swift
    /// `currentFocusHistoryEntry`.
    pub fn current_focus_history_entry(
        &self,
        host: &dyn FocusHistoryHost,
    ) -> Option<FocusHistoryEntry> {
        let selected = host.selected_workspace_id()?;
        Some(FocusHistoryEntry::new(
            selected,
            host.remembered_focused_panel_id(selected),
        ))
    }

    fn resolved_focus_history_entry(
        &self,
        host: &dyn FocusHistoryHost,
        entry: FocusHistoryEntry,
    ) -> Option<FocusHistoryEntry> {
        if !host.workspace_exists(entry.workspace_id) {
            return None;
        }
        // Closed panels still leave a useful workspace-level history entry.
        Some(FocusHistoryEntry::new(
            entry.workspace_id,
            self.resolved_focus_history_panel_id(host, entry),
        ))
    }

    fn focus_history_entry_resolves_to_current(
        &self,
        host: &dyn FocusHistoryHost,
        entry: FocusHistoryEntry,
        current_entry: Option<FocusHistoryEntry>,
    ) -> bool {
        let Some(current_entry) = current_entry else {
            return false;
        };
        let Some(resolved) = self.resolved_focus_history_entry(host, entry) else {
            return false;
        };
        resolved == current_entry
    }

    fn focus_history_entry_is_navigable(
        &self,
        host: &dyn FocusHistoryHost,
        entry: FocusHistoryEntry,
        current_entry: Option<FocusHistoryEntry>,
    ) -> bool {
        if self.resolved_focus_history_entry(host, entry).is_none() {
            return false;
        }
        if self.focus_history_entry_resolves_to_current(host, entry, current_entry) {
            return false;
        }
        true
    }

    // MARK: - Menu snapshots

    /// Builds the back or forward menu snapshot, optionally truncated. Swift
    /// `focusHistoryMenuSnapshot(direction:maxItemCount:)`.
    pub fn focus_history_menu_snapshot(
        &self,
        host: &dyn FocusHistoryHost,
        direction: FocusHistoryMenuDirection,
        max_item_count: Option<i64>,
    ) -> FocusHistoryMenuSnapshot {
        let current_entry = self.current_focus_history_entry(host);
        let count = self.focus_history.len() as i64;
        let history_indices: Vec<i64> = match direction {
            FocusHistoryMenuDirection::Back => {
                let last_back_index = self.history_index.min(count) - 1;
                if last_back_index >= 0 {
                    (0..=last_back_index).rev().collect()
                } else {
                    Vec::new()
                }
            }
            FocusHistoryMenuDirection::Forward => {
                if self.history_index < count - 1 {
                    ((self.history_index + 1)..count).collect()
                } else {
                    Vec::new()
                }
            }
        };

        let items: Vec<FocusHistoryMenuItem> = history_indices
            .into_iter()
            .filter_map(|index| {
                let record = self.focus_history[index as usize];
                let entry = record.entry;
                let resolved_entry = self.resolved_focus_history_entry(host, entry)?;
                let raw_workspace_title = host.workspace_title(resolved_entry.workspace_id)?;
                if !self.focus_history_entry_is_navigable(host, entry, current_entry) {
                    return None;
                }

                let workspace_title = raw_workspace_title.trim().to_string();
                let panel_title = resolved_entry
                    .panel_id
                    .and_then(|panel_id| host.panel_title(resolved_entry.workspace_id, panel_id))
                    .map(|title| title.trim().to_string())
                    .filter(|title| !title.is_empty());
                let position = match direction {
                    FocusHistoryMenuDirection::Back => FocusHistoryMenuPosition::Older,
                    FocusHistoryMenuDirection::Forward => FocusHistoryMenuPosition::Newer,
                };

                Some(FocusHistoryMenuItem {
                    history_index: index,
                    entry,
                    workspace_title,
                    panel_title,
                    position,
                    focused_at: record.focused_at,
                    is_navigable: true,
                })
            })
            .collect();

        truncate_items(items, max_item_count)
    }

    // MARK: - Navigation

    fn restore_focus_history_entry(
        &self,
        host: &mut dyn FocusHistoryHost,
        entry: FocusHistoryEntry,
    ) -> bool {
        if !host.workspace_exists(entry.workspace_id) {
            return false;
        }

        host.select_workspace(entry.workspace_id);

        let target_panel_id = self.resolved_focus_history_panel_id(&*host, entry);

        if let Some(target_panel_id) = target_panel_id {
            host.remember_focused_surface(entry.workspace_id, target_panel_id);
            host.focus_panel(entry.workspace_id, target_panel_id);
            host.trigger_focus_flash(entry.workspace_id, target_panel_id);
        } else {
            host.focus_selected_workspace_panel();
        }

        true
    }

    fn navigate_to_focus_history_entry(
        &mut self,
        host: &mut dyn FocusHistoryHost,
        entry: FocusHistoryEntry,
        target_index: i64,
    ) -> bool {
        let did_restore = self.with_focus_history_recording_suppressed(host, |model, host| {
            model.restore_focus_history_entry(host, entry)
        });
        if !did_restore {
            return false;
        }
        self.history_index = target_index;
        host.focus_history_revision_did_change();
        true
    }

    /// Navigates to a menu item; returns whether navigation happened. Swift
    /// `navigateToFocusHistoryMenuItem`.
    pub fn navigate_to_focus_history_menu_item(
        &mut self,
        host: &mut dyn FocusHistoryHost,
        item: &FocusHistoryMenuItem,
    ) -> bool {
        let current_entry = self.current_focus_history_entry(&*host);
        if !self.focus_history_entry_is_navigable(&*host, item.entry, current_entry) {
            return false;
        }
        let target_index = item.history_index;
        if target_index >= 0
            && target_index < self.focus_history.len() as i64
            && self.focus_history[target_index as usize].entry == item.entry
        {
            let entry = self.focus_history[target_index as usize].entry;
            return self.navigate_to_focus_history_entry(host, entry, target_index);
        }

        // The index moved; fall back to the last stack slot holding the entry.
        let Some(fallback_index) = self
            .focus_history
            .iter()
            .rposition(|record| record.entry == item.entry)
        else {
            return false;
        };
        self.navigate_to_focus_history_entry(host, item.entry, fallback_index as i64)
    }

    /// Navigates one step back; returns whether navigation happened. Swift
    /// `navigateBack`.
    pub fn navigate_back(&mut self, host: &mut dyn FocusHistoryHost) -> bool {
        if self.history_index <= 0 {
            return false;
        }

        let current_entry = self.current_focus_history_entry(&*host);
        let mut target_index = self.history_index - 1;
        while target_index >= 0 {
            let entry = self.focus_history[target_index as usize].entry;
            if !host.workspace_exists(entry.workspace_id) {
                self.focus_history.remove(target_index as usize);
                self.history_index -= 1;
                target_index -= 1;
                host.focus_history_revision_did_change();
                continue;
            }
            if self.focus_history_entry_resolves_to_current(&*host, entry, current_entry) {
                target_index -= 1;
                continue;
            }
            if self.navigate_to_focus_history_entry(host, entry, target_index) {
                return true;
            }
            self.focus_history.remove(target_index as usize);
            self.history_index -= 1;
            target_index -= 1;
            host.focus_history_revision_did_change();
        }
        false
    }

    /// Navigates one step forward; returns whether navigation happened. Swift
    /// `navigateForward`.
    pub fn navigate_forward(&mut self, host: &mut dyn FocusHistoryHost) -> bool {
        if self.history_index >= self.focus_history.len() as i64 - 1 {
            return false;
        }

        let current_entry = self.current_focus_history_entry(&*host);
        let mut target_index = self.history_index + 1;
        while target_index < self.focus_history.len() as i64 {
            let entry = self.focus_history[target_index as usize].entry;
            if !host.workspace_exists(entry.workspace_id) {
                self.focus_history.remove(target_index as usize);
                host.focus_history_revision_did_change();
                continue;
            }
            if self.focus_history_entry_resolves_to_current(&*host, entry, current_entry) {
                target_index += 1;
                continue;
            }
            if self.navigate_to_focus_history_entry(host, entry, target_index) {
                return true;
            }
            self.focus_history.remove(target_index as usize);
            host.focus_history_revision_did_change();
        }
        false
    }

    /// Whether any back entry is navigable from the current position. Swift
    /// `canNavigateBack`.
    pub fn can_navigate_back(&self, host: &dyn FocusHistoryHost) -> bool {
        let current_entry = self.current_focus_history_entry(host);
        if self.history_index <= 0 {
            return false;
        }
        let prefix_len = (self.history_index as usize).min(self.focus_history.len());
        self.focus_history[..prefix_len]
            .iter()
            .any(|record| self.focus_history_entry_is_navigable(host, record.entry, current_entry))
    }

    /// Whether any forward entry is navigable from the current position. Swift
    /// `canNavigateForward`.
    pub fn can_navigate_forward(&self, host: &dyn FocusHistoryHost) -> bool {
        let current_entry = self.current_focus_history_entry(host);
        if self.history_index >= self.focus_history.len() as i64 - 1 {
            return false;
        }
        let start = (self.history_index + 1).max(0) as usize;
        self.focus_history[start..]
            .iter()
            .any(|record| self.focus_history_entry_is_navigable(host, record.entry, current_entry))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// In-memory window host mirroring the Swift `FakeFocusHistoryHost`: a
    /// dictionary of workspaces/panels plus counters for the mutations a
    /// navigation performs.
    #[derive(Default)]
    struct FakeFocusHistoryHost {
        workspaces: HashMap<Uuid, WorkspaceState>,
        selected_workspace_id: Option<Uuid>,
        revision_bumps: usize,
        focused_panels: Vec<(Uuid, Uuid)>,
        flashed_panels: Vec<(Uuid, Uuid)>,
        focus_selected_workspace_panel_calls: usize,
    }

    #[derive(Clone, Default)]
    struct WorkspaceState {
        title: String,
        panels: HashMap<Uuid, String>,
        remembered_focused_panel_id: Option<Uuid>,
        focused_panel_id: Option<Uuid>,
    }

    impl FakeFocusHistoryHost {
        fn add_workspace(&mut self, title: &str, panels: &[(Uuid, &str)]) -> Uuid {
            let id = Uuid::new_v4();
            let panels = panels
                .iter()
                .map(|(k, v)| (*k, v.to_string()))
                .collect::<HashMap<_, _>>();
            self.workspaces.insert(
                id,
                WorkspaceState {
                    title: title.to_string(),
                    panels,
                    remembered_focused_panel_id: None,
                    focused_panel_id: None,
                },
            );
            id
        }

        fn set_remembered(&mut self, workspace_id: Uuid, panel_id: Uuid) {
            if let Some(ws) = self.workspaces.get_mut(&workspace_id) {
                ws.remembered_focused_panel_id = Some(panel_id);
            }
        }
    }

    impl FocusHistoryHost for FakeFocusHistoryHost {
        fn selected_workspace_id(&self) -> Option<Uuid> {
            self.selected_workspace_id
        }

        fn workspace_exists(&self, workspace_id: Uuid) -> bool {
            self.workspaces.contains_key(&workspace_id)
        }

        fn panel_exists(&self, workspace_id: Uuid, panel_id: Uuid) -> bool {
            self.workspaces
                .get(&workspace_id)
                .is_some_and(|ws| ws.panels.contains_key(&panel_id))
        }

        fn workspace_title(&self, workspace_id: Uuid) -> Option<String> {
            self.workspaces.get(&workspace_id).map(|ws| ws.title.clone())
        }

        fn panel_title(&self, workspace_id: Uuid, panel_id: Uuid) -> Option<String> {
            self.workspaces
                .get(&workspace_id)
                .and_then(|ws| ws.panels.get(&panel_id).cloned())
        }

        fn remembered_focused_panel_id(&self, workspace_id: Uuid) -> Option<Uuid> {
            self.workspaces
                .get(&workspace_id)
                .and_then(|ws| ws.remembered_focused_panel_id)
        }

        fn workspace_focused_panel_id(&self, workspace_id: Uuid) -> Option<Uuid> {
            self.workspaces
                .get(&workspace_id)
                .and_then(|ws| ws.focused_panel_id)
        }

        fn first_panel_id_sorted_by_uuid_string(&self, workspace_id: Uuid) -> Option<Uuid> {
            self.workspaces.get(&workspace_id).and_then(|ws| {
                let mut ids: Vec<Uuid> = ws.panels.keys().copied().collect();
                ids.sort_by_key(|id| id.to_string());
                ids.first().copied()
            })
        }

        fn select_workspace(&mut self, workspace_id: Uuid) {
            if self.selected_workspace_id != Some(workspace_id) {
                self.selected_workspace_id = Some(workspace_id);
            }
        }

        fn remember_focused_surface(&mut self, workspace_id: Uuid, surface_id: Uuid) {
            if let Some(ws) = self.workspaces.get_mut(&workspace_id) {
                ws.remembered_focused_panel_id = Some(surface_id);
            }
        }

        fn focus_panel(&mut self, workspace_id: Uuid, panel_id: Uuid) {
            self.focused_panels.push((workspace_id, panel_id));
        }

        fn trigger_focus_flash(&mut self, workspace_id: Uuid, panel_id: Uuid) {
            self.flashed_panels.push((workspace_id, panel_id));
        }

        fn focus_selected_workspace_panel(&mut self) {
            self.focus_selected_workspace_panel_calls += 1;
        }

        fn focus_history_revision_did_change(&mut self) {
            self.revision_bumps += 1;
        }
    }

    fn make_model(max_history_size: usize) -> (FocusHistoryModel, FakeFocusHistoryHost) {
        (
            FocusHistoryModel::new(max_history_size),
            FakeFocusHistoryHost::default(),
        )
    }

    // Swift `recordAndNavigateBackForwardAcrossWorkspaces`.
    #[test]
    fn record_and_navigate_back_forward_across_workspaces() {
        let (mut model, mut host) = make_model(50);
        let panel_a = Uuid::new_v4();
        let panel_b = Uuid::new_v4();
        let ws_a = host.add_workspace("A", &[(panel_a, "pa")]);
        let ws_b = host.add_workspace("B", &[(panel_b, "pb")]);

        host.selected_workspace_id = Some(ws_a);
        host.set_remembered(ws_a, panel_a);
        model.record_focus_in_history(&mut host, ws_a, Some(panel_a), false);

        host.selected_workspace_id = Some(ws_b);
        host.set_remembered(ws_b, panel_b);
        model.record_focus_in_history(&mut host, ws_b, Some(panel_b), false);

        assert!(model.can_navigate_back(&host));
        assert!(!model.can_navigate_forward(&host));

        assert!(model.navigate_back(&mut host));
        assert_eq!(host.selected_workspace_id, Some(ws_a));
        assert!(model.can_navigate_forward(&host));

        assert!(model.navigate_forward(&mut host));
        assert_eq!(host.selected_workspace_id, Some(ws_b));
    }

    // Swift `recordingDropsOldestEntriesBeyondCap`.
    #[test]
    fn recording_drops_oldest_entries_beyond_cap() {
        let (mut model, mut host) = make_model(3);
        let mut workspace_ids: Vec<Uuid> = Vec::new();
        for index in 0..5 {
            let panel = Uuid::new_v4();
            let ws = host.add_workspace(&format!("ws{index}"), &[(panel, "p")]);
            workspace_ids.push(ws);
            host.selected_workspace_id = Some(ws);
            host.set_remembered(ws, panel);
            model.record_focus_in_history(&mut host, ws, Some(panel), false);
        }

        // Cap 3: only ws2 and ws3 remain behind the current ws4 position.
        assert!(model.navigate_back(&mut host));
        assert_eq!(host.selected_workspace_id, Some(workspace_ids[3]));
        assert!(model.navigate_back(&mut host));
        assert_eq!(host.selected_workspace_id, Some(workspace_ids[2]));
        assert!(!model.navigate_back(&mut host));
    }

    // Swift `duplicateRecordAtCurrentPositionIsNoOp`.
    #[test]
    fn duplicate_record_at_current_position_is_no_op() {
        let (mut model, mut host) = make_model(50);
        let panel = Uuid::new_v4();
        let ws = host.add_workspace("ws", &[(panel, "p")]);
        host.selected_workspace_id = Some(ws);

        model.record_focus_in_history(&mut host, ws, Some(panel), false);
        let bumps = host.revision_bumps;
        model.record_focus_in_history(&mut host, ws, Some(panel), false);
        assert_eq!(host.revision_bumps, bumps);
    }

    // Swift `recordingIgnoresUnknownWorkspacesAndPanels`.
    #[test]
    fn recording_ignores_unknown_workspaces_and_panels() {
        let (mut model, mut host) = make_model(50);
        let ws = host.add_workspace("ws", &[]);

        model.record_focus_in_history(&mut host, Uuid::new_v4(), None, false);
        model.record_focus_in_history(&mut host, ws, Some(Uuid::new_v4()), false);
        assert_eq!(host.revision_bumps, 0);
        assert!(!model.can_navigate_back(&host));
    }

    // Swift `preservingForwardBranchInsertsAfterCurrentWithoutDroppingForwardStack`.
    #[test]
    fn preserving_forward_branch_inserts_after_current_without_dropping_forward_stack() {
        let (mut model, mut host) = make_model(50);
        for index in 0..3 {
            let panel = Uuid::new_v4();
            let ws = host.add_workspace(&format!("ws{index}"), &[(panel, "p")]);
            host.selected_workspace_id = Some(ws);
            host.set_remembered(ws, panel);
            model.record_focus_in_history(&mut host, ws, Some(panel), false);
        }

        // Step back to ws1 so ws2 sits on the forward stack.
        assert!(model.navigate_back(&mut host));
        assert!(model.can_navigate_forward(&host));

        // A restore-style record keeps the forward stack.
        let restored_panel = Uuid::new_v4();
        let restored_ws = host.add_workspace("restored", &[(restored_panel, "p")]);
        host.set_remembered(restored_ws, restored_panel);
        host.selected_workspace_id = Some(restored_ws);
        model.record_focus_in_history(&mut host, restored_ws, Some(restored_panel), true);

        assert!(model.can_navigate_forward(&host));
    }

    // Swift `implicitFocusCoalescesWithCurrentMidStackEntryForSameWorkspace`.
    #[test]
    fn implicit_focus_coalesces_with_current_mid_stack_entry_for_same_workspace() {
        let (mut model, mut host) = make_model(50);
        let panel_a = Uuid::new_v4();
        let panel_b = Uuid::new_v4();
        let other_panel = Uuid::new_v4();
        let ws = host.add_workspace("ws", &[(panel_a, "a"), (panel_b, "b")]);
        let other = host.add_workspace("other", &[(other_panel, "p")]);

        host.selected_workspace_id = Some(ws);
        model.record_focus_in_history(&mut host, ws, Some(panel_a), false);
        host.selected_workspace_id = Some(other);
        host.set_remembered(other, other_panel);
        model.record_focus_in_history(&mut host, other, Some(other_panel), false);

        // Back to the mid-stack ws entry, then an implicit focus on another
        // panel of the same workspace rewrites the entry in place.
        host.set_remembered(ws, panel_a);
        assert!(model.navigate_back(&mut host));
        let bumps = host.revision_bumps;
        model.record_implicit_focus_in_history(&mut host, ws, Some(panel_b));
        assert_eq!(host.revision_bumps, bumps + 1);
        // Forward stack intact: implicit focus never truncates it.
        assert!(model.can_navigate_forward(&host));
    }

    // Swift `invalidateWorkspaceRemovesItsEntriesAndClampsIndex`.
    #[test]
    fn invalidate_workspace_removes_its_entries_and_clamps_index() {
        let (mut model, mut host) = make_model(50);
        let panel_a = Uuid::new_v4();
        let panel_b = Uuid::new_v4();
        let ws_a = host.add_workspace("A", &[(panel_a, "a")]);
        let ws_b = host.add_workspace("B", &[(panel_b, "b")]);

        host.selected_workspace_id = Some(ws_a);
        host.set_remembered(ws_a, panel_a);
        model.record_focus_in_history(&mut host, ws_a, Some(panel_a), false);
        host.selected_workspace_id = Some(ws_b);
        host.set_remembered(ws_b, panel_b);
        model.record_focus_in_history(&mut host, ws_b, Some(panel_b), false);

        host.workspaces.remove(&ws_a);
        model.invalidate_focus_history_target(&mut host, ws_a, None);

        assert!(!model.can_navigate_back(&host));
        assert!(!model.navigate_back(&mut host));
    }

    // Swift `invalidatePanelOnlyBumpsRevisionWhenPanelIsInHistory`.
    #[test]
    fn invalidate_panel_only_bumps_revision_when_panel_is_in_history() {
        let (mut model, mut host) = make_model(50);
        let panel = Uuid::new_v4();
        let ws = host.add_workspace("ws", &[(panel, "p")]);
        host.selected_workspace_id = Some(ws);
        model.record_focus_in_history(&mut host, ws, Some(panel), false);

        let bumps = host.revision_bumps;
        model.invalidate_focus_history_target(&mut host, ws, Some(Uuid::new_v4()));
        assert_eq!(host.revision_bumps, bumps);
        model.invalidate_focus_history_target(&mut host, ws, Some(panel));
        assert_eq!(host.revision_bumps, bumps + 1);
    }

    // Swift `menuSnapshotListsBackEntriesMostRecentFirstAndTruncates`.
    #[test]
    fn menu_snapshot_lists_back_entries_most_recent_first_and_truncates() {
        let (mut model, mut host) = make_model(50);
        for index in 0..4 {
            let panel = Uuid::new_v4();
            let ws = host.add_workspace(&format!("ws{index}"), &[(panel, &format!("panel{index}"))]);
            host.selected_workspace_id = Some(ws);
            host.set_remembered(ws, panel);
            model.record_focus_in_history(&mut host, ws, Some(panel), false);
        }

        let snapshot =
            model.focus_history_menu_snapshot(&host, FocusHistoryMenuDirection::Back, None);
        assert_eq!(
            snapshot
                .items
                .iter()
                .map(|i| i.workspace_title.clone())
                .collect::<Vec<_>>(),
            vec!["ws2", "ws1", "ws0"]
        );
        assert_eq!(snapshot.total_item_count, 3);
        assert!(!snapshot.is_limited);

        let limited =
            model.focus_history_menu_snapshot(&host, FocusHistoryMenuDirection::Back, Some(2));
        assert_eq!(limited.items.len(), 2);
        assert_eq!(limited.total_item_count, 3);
        assert!(limited.is_limited);
        assert!(limited
            .items
            .iter()
            .all(|i| i.position == FocusHistoryMenuPosition::Older));
    }

    // Swift `menuSnapshotResolvesClosedPanelToWorkspaceLevelEntry`.
    #[test]
    fn menu_snapshot_resolves_closed_panel_to_workspace_level_entry() {
        let (mut model, mut host) = make_model(50);
        let panel_a = Uuid::new_v4();
        let panel_b = Uuid::new_v4();
        let other_panel = Uuid::new_v4();
        let ws = host.add_workspace("A", &[(panel_a, "pa"), (panel_b, "pb")]);
        let other = host.add_workspace("B", &[(other_panel, "p")]);

        host.selected_workspace_id = Some(ws);
        model.record_focus_in_history(&mut host, ws, Some(panel_a), false);
        host.selected_workspace_id = Some(other);
        host.set_remembered(other, other_panel);
        model.record_focus_in_history(&mut host, other, Some(other_panel), false);

        // Close panel A; the entry resolves through the remembered panel.
        if let Some(ws_state) = host.workspaces.get_mut(&ws) {
            ws_state.panels.remove(&panel_a);
            ws_state.remembered_focused_panel_id = Some(panel_b);
        }

        let snapshot =
            model.focus_history_menu_snapshot(&host, FocusHistoryMenuDirection::Back, None);
        assert_eq!(snapshot.items.len(), 1);
        assert_eq!(
            snapshot.items.first().and_then(|i| i.panel_title.clone()),
            Some("pb".to_string())
        );
    }

    // Swift `navigateToMenuItemFallsBackToLastMatchingEntryWhenIndexMoved`.
    #[test]
    fn navigate_to_menu_item_falls_back_to_last_matching_entry_when_index_moved() {
        let (mut model, mut host) = make_model(50);
        let panel_a = Uuid::new_v4();
        let panel_b = Uuid::new_v4();
        let ws_a = host.add_workspace("A", &[(panel_a, "pa")]);
        let ws_b = host.add_workspace("B", &[(panel_b, "pb")]);

        host.selected_workspace_id = Some(ws_a);
        host.set_remembered(ws_a, panel_a);
        model.record_focus_in_history(&mut host, ws_a, Some(panel_a), false);
        host.selected_workspace_id = Some(ws_b);
        host.set_remembered(ws_b, panel_b);
        model.record_focus_in_history(&mut host, ws_b, Some(panel_b), false);

        let snapshot =
            model.focus_history_menu_snapshot(&host, FocusHistoryMenuDirection::Back, None);
        let item = snapshot.items.first().expect("expected a back item");
        let shifted_item = FocusHistoryMenuItem {
            history_index: item.history_index + 7,
            entry: item.entry,
            workspace_title: item.workspace_title.clone(),
            panel_title: item.panel_title.clone(),
            position: item.position,
            focused_at: item.focused_at,
            is_navigable: item.is_navigable,
        };
        assert!(model.navigate_to_focus_history_menu_item(&mut host, &shifted_item));
        assert_eq!(host.selected_workspace_id, Some(ws_a));
    }

    // Swift `recentlyFocusedMergesBackAndForwardByRecency`.
    #[test]
    fn recently_focused_merges_back_and_forward_by_recency() {
        let older = FocusHistoryMenuItem {
            history_index: 0,
            entry: FocusHistoryEntry::new(Uuid::new_v4(), None),
            workspace_title: "older".to_string(),
            panel_title: None,
            position: FocusHistoryMenuPosition::Older,
            focused_at: 100,
            is_navigable: true,
        };
        let newer = FocusHistoryMenuItem {
            history_index: 2,
            entry: FocusHistoryEntry::new(Uuid::new_v4(), None),
            workspace_title: "newer".to_string(),
            panel_title: None,
            position: FocusHistoryMenuPosition::Newer,
            focused_at: 200,
            is_navigable: true,
        };
        let back = FocusHistoryMenuSnapshot::new(vec![older.clone()], 1, false);
        let forward = FocusHistoryMenuSnapshot::new(vec![newer.clone()], 1, false);
        let merged = FocusHistoryMenuSnapshot::recently_focused(&back, &forward, None);
        assert_eq!(
            merged
                .items
                .iter()
                .map(|i| i.workspace_title.clone())
                .collect::<Vec<_>>(),
            vec!["newer", "older"]
        );

        let limited = FocusHistoryMenuSnapshot::recently_focused(&back, &forward, Some(1));
        assert_eq!(
            limited
                .items
                .iter()
                .map(|i| i.workspace_title.clone())
                .collect::<Vec<_>>(),
            vec!["newer"]
        );
        assert!(limited.is_limited);
        assert_eq!(limited.total_item_count, 2);
    }

    // Swift `suppressionBlocksRecordingAndGenerationsAreConsumedOnce`.
    #[test]
    fn suppression_blocks_recording_and_generations_are_consumed_once() {
        let (mut model, mut host) = make_model(50);
        let panel = Uuid::new_v4();
        let ws = host.add_workspace("ws", &[(panel, "p")]);
        host.selected_workspace_id = Some(ws);

        model.with_focus_history_recording_suppressed(&mut host, |model, host| {
            assert!(!model.should_record_focus_history());
            model.record_focus_in_history(host, ws, Some(panel), false);
        });
        assert!(model.should_record_focus_history());
        assert_eq!(host.revision_bumps, 0);

        model.mark_suppressed_selection_side_effect_generation(7);
        assert!(model.consume_suppressed_selection_side_effect_generation(7));
        assert!(!model.consume_suppressed_selection_side_effect_generation(7));
    }

    // Swift `navigateBackPrunesEntriesWhoseWorkspaceIsGone`.
    #[test]
    fn navigate_back_prunes_entries_whose_workspace_is_gone() {
        let (mut model, mut host) = make_model(50);
        let panel_a = Uuid::new_v4();
        let panel_b = Uuid::new_v4();
        let panel_c = Uuid::new_v4();
        let ws_a = host.add_workspace("A", &[(panel_a, "pa")]);
        let ws_b = host.add_workspace("B", &[(panel_b, "pb")]);
        let ws_c = host.add_workspace("C", &[(panel_c, "pc")]);

        for (ws, panel) in [(ws_a, panel_a), (ws_b, panel_b), (ws_c, panel_c)] {
            host.selected_workspace_id = Some(ws);
            host.set_remembered(ws, panel);
            model.record_focus_in_history(&mut host, ws, Some(panel), false);
        }

        // Drop wsB without invalidating: navigateBack prunes it inline and
        // lands on wsA.
        host.workspaces.remove(&ws_b);
        assert!(model.navigate_back(&mut host));
        assert_eq!(host.selected_workspace_id, Some(ws_a));
    }

    // Swift `resetClearsAllState`.
    #[test]
    fn reset_clears_all_state() {
        let (mut model, mut host) = make_model(50);
        let panel_a = Uuid::new_v4();
        let panel_b = Uuid::new_v4();
        let ws_a = host.add_workspace("A", &[(panel_a, "a")]);
        let ws_b = host.add_workspace("B", &[(panel_b, "b")]);
        host.selected_workspace_id = Some(ws_a);
        host.set_remembered(ws_a, panel_a);
        model.record_focus_in_history(&mut host, ws_a, Some(panel_a), false);
        host.selected_workspace_id = Some(ws_b);
        host.set_remembered(ws_b, panel_b);
        model.record_focus_in_history(&mut host, ws_b, Some(panel_b), false);
        model.mark_suppressed_selection_side_effect_generation(1);

        let bumps = host.revision_bumps;
        model.reset();
        // reset() itself never bumps; the window-reset path owns that bump.
        assert_eq!(host.revision_bumps, bumps);
        assert!(!model.can_navigate_back(&host));
        assert!(!model.can_navigate_forward(&host));
        assert!(!model.consume_suppressed_selection_side_effect_generation(1));
        assert_eq!(
            model.current_focus_history_entry(&host).map(|e| e.workspace_id),
            Some(ws_b)
        );
    }
}
