//! Port of the pure record-management core of app `Sources/ClosedItemHistory.swift`.
//!
//! Covers push + capacity trim, insert-with-protected-record capacity eviction,
//! `restore_first_restorable` candidate selection (closedAt desc, offset desc,
//! newer-than cutoff, id exclusions), the UUID remap/removal transforms, loaded
//! record de-dup merge, menu-snapshot limiting, and closed-window restore
//! validation.
//!
//! DIVERGENCE (self-contained snapshots): the Swift store references
//! `SessionPanelSnapshot` / `SessionWorkspaceSnapshot` / `SessionWindowSnapshot`
//! from the app target (not in cmux-core). To stay decoupled, this module models
//! its OWN minimal [`PanelSnapshot`] / [`WorkspaceSnapshot`] / [`WindowSnapshot`]
//! value types carrying only the fields the ported logic reads.
//!
//! DIVERGENCE (no ambient clock/id): Swift defaults `closedAt = Date()` and
//! `id = UUID()`. The port takes both explicitly (`closed_at: i64` millis-style
//! ordinal, `id: Uuid`) so every operation is deterministic and testable.
//!
//! EXCLUDED (host wiring): file save/load, the persistence actor, async
//! load-then-merge sequencing, and the `revision`-gated async persistence. The
//! `revision` counter is kept (it is pure bookkeeping) but nothing is written to
//! disk. The full localized menu-title projection is simplified — the portable
//! remap/dedup/capacity/sort core is complete.

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

/// Ordinal timestamp for a closed record (higher = more recently closed).
/// Mirrors Swift's `Date` only in its ordering role.
pub type ClosedAt = i64;

/// Split orientation for a closed panel's fallback placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitOrientation {
    /// Split along the horizontal axis (side-by-side panes).
    Horizontal,
    /// Split along the vertical axis (stacked panes).
    Vertical,
}

/// Where a closed panel should be re-inserted when its original pane is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedPanelSplitPlacement {
    /// The split axis to recreate.
    pub orientation: SplitOrientation,
    /// Whether the restored panel should become the first child of the split.
    pub insert_first: bool,
    /// The panel to anchor the recreated split against, if still present.
    pub anchor_panel_id: Option<Uuid>,
}

/// Minimal panel content snapshot (title/directory only). The pure core never
/// reads more than this; richer per-panel state is host wiring.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelSnapshot {
    /// A user-assigned custom title, if any.
    pub custom_title: Option<String>,
    /// The panel's process/derived title, if any.
    pub title: Option<String>,
    /// The panel's working directory, if any.
    pub directory: Option<String>,
}

/// Minimal workspace content snapshot (title/directory only).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    /// A user-assigned custom title, if any.
    pub custom_title: Option<String>,
    /// The workspace's process-derived title.
    pub process_title: String,
    /// The workspace's current working directory.
    pub current_directory: String,
}

/// Minimal window content snapshot (workspace count + restorability flag).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowSnapshot {
    /// The number of workspaces the window held.
    pub workspace_count: usize,
    /// Whether the window snapshot carries any restorable panels.
    pub has_restorable_panels: bool,
}

/// A closed-panel history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedPanelHistoryEntry {
    /// The owning workspace's id.
    pub workspace_id: Uuid,
    /// The closed pane's id.
    pub pane_id: Uuid,
    /// The pane the closed panel was anchored to, if any.
    pub pane_anchor_panel_id: Option<Uuid>,
    /// Whether restore should target the original pane.
    pub restore_in_original_pane: bool,
    /// The tab index the panel occupied.
    pub tab_index: i64,
    /// The panel's content snapshot.
    pub snapshot: PanelSnapshot,
    /// Fallback split placement when the original pane is gone.
    pub fallback_split_placement: Option<ClosedPanelSplitPlacement>,
}

/// A closed-workspace history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedWorkspaceHistoryEntry {
    /// The closed workspace's id.
    pub workspace_id: Uuid,
    /// The owning window's id, if known.
    pub window_id: Option<Uuid>,
    /// The workspace index within its window.
    pub workspace_index: i64,
    /// The workspace's content snapshot.
    pub snapshot: WorkspaceSnapshot,
}

/// A closed-window history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedWindowHistoryEntry {
    /// The closed window's id, if known.
    pub window_id: Option<Uuid>,
    /// The window's content snapshot.
    pub snapshot: WindowSnapshot,
    /// The workspace ids the window held.
    pub workspace_ids: Vec<Uuid>,
}

/// A closed item: a panel, a workspace, or a whole window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosedItemHistoryEntry {
    /// A closed panel.
    Panel(ClosedPanelHistoryEntry),
    /// A closed workspace.
    Workspace(ClosedWorkspaceHistoryEntry),
    /// A closed window.
    Window(ClosedWindowHistoryEntry),
}

/// A single closed-item record with stable identity and a close ordinal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClosedItemHistoryRecord {
    /// The record's stable id.
    pub id: Uuid,
    /// When the item was closed (ordering only).
    pub closed_at: ClosedAt,
    /// The closed item.
    pub entry: ClosedItemHistoryEntry,
}

impl ClosedItemHistoryRecord {
    /// Creates a record.
    pub fn new(id: Uuid, closed_at: ClosedAt, entry: ClosedItemHistoryEntry) -> Self {
        Self {
            id,
            closed_at,
            entry,
        }
    }
}

/// One row of the recently-closed menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// The originating record's id.
    pub id: Uuid,
    /// The projected primary title.
    pub title: String,
    /// The kind label ("Tab" / "Workspace" / "Window").
    pub detail: String,
    /// The close ordinal.
    pub closed_at: ClosedAt,
}

/// The recently-closed menu projection with limiting bookkeeping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSnapshot {
    /// The projected, most-recent-first menu rows.
    pub items: Vec<MenuItem>,
    /// The total number of records (before limiting).
    pub total_item_count: usize,
    /// Whether the snapshot was truncated to `max_item_count`.
    pub is_limited: bool,
}

// ---- Free-function transforms (Swift `static func recordsBy…`) --------------

fn remap_anchor(panel_id: Option<Uuid>, panel_id_map: &HashMap<Uuid, Uuid>) -> Option<Uuid> {
    panel_id.map(|p| panel_id_map.get(&p).copied().unwrap_or(p))
}

/// Rewrites panel records that belong to `old_workspace_id` onto
/// `new_workspace_id`, remapping anchor panel ids through `panel_id_map` and
/// forcing `restore_in_original_pane = false` (the workspace moved). Returns the
/// new records and whether anything changed.
pub fn records_by_remapping_panel_workspace_ids(
    records: &[ClosedItemHistoryRecord],
    old_workspace_id: Uuid,
    new_workspace_id: Uuid,
    panel_id_map: &HashMap<Uuid, Uuid>,
) -> (Vec<ClosedItemHistoryRecord>, bool) {
    let mut did_update = false;
    let remapped = records
        .iter()
        .map(|record| {
            let ClosedItemHistoryEntry::Panel(panel) = &record.entry else {
                return record.clone();
            };
            if panel.workspace_id != old_workspace_id {
                return record.clone();
            }
            did_update = true;
            let fallback = panel.fallback_split_placement.as_ref().map(|p| {
                ClosedPanelSplitPlacement {
                    orientation: p.orientation,
                    insert_first: p.insert_first,
                    anchor_panel_id: remap_anchor(p.anchor_panel_id, panel_id_map),
                }
            });
            ClosedItemHistoryRecord::new(
                record.id,
                record.closed_at,
                ClosedItemHistoryEntry::Panel(ClosedPanelHistoryEntry {
                    workspace_id: new_workspace_id,
                    pane_id: panel.pane_id,
                    pane_anchor_panel_id: remap_anchor(panel.pane_anchor_panel_id, panel_id_map),
                    restore_in_original_pane: false,
                    tab_index: panel.tab_index,
                    snapshot: panel.snapshot.clone(),
                    fallback_split_placement: fallback,
                }),
            )
        })
        .collect();
    (remapped, did_update)
}

/// Rewrites every panel record's anchor references from `old_panel_id` to
/// `new_panel_id` (both the pane anchor and the fallback-placement anchor).
/// Returns the new records and whether anything changed.
pub fn records_by_remapping_panel_anchor_ids(
    records: &[ClosedItemHistoryRecord],
    old_panel_id: Uuid,
    new_panel_id: Uuid,
) -> (Vec<ClosedItemHistoryRecord>, bool) {
    let mut did_update = false;
    let remapped = records
        .iter()
        .map(|record| {
            let ClosedItemHistoryEntry::Panel(panel) = &record.entry else {
                return record.clone();
            };
            let pane_anchor = if panel.pane_anchor_panel_id == Some(old_panel_id) {
                Some(new_panel_id)
            } else {
                panel.pane_anchor_panel_id
            };
            let fallback = panel.fallback_split_placement.as_ref().map(|placement| {
                let anchor = if placement.anchor_panel_id == Some(old_panel_id) {
                    Some(new_panel_id)
                } else {
                    placement.anchor_panel_id
                };
                ClosedPanelSplitPlacement {
                    orientation: placement.orientation,
                    insert_first: placement.insert_first,
                    anchor_panel_id: anchor,
                }
            });
            let old_fallback_anchor = panel
                .fallback_split_placement
                .as_ref()
                .and_then(|p| p.anchor_panel_id);
            let new_fallback_anchor = fallback.as_ref().and_then(|p| p.anchor_panel_id);
            if pane_anchor != panel.pane_anchor_panel_id
                || new_fallback_anchor != old_fallback_anchor
            {
                did_update = true;
            }
            ClosedItemHistoryRecord::new(
                record.id,
                record.closed_at,
                ClosedItemHistoryEntry::Panel(ClosedPanelHistoryEntry {
                    workspace_id: panel.workspace_id,
                    pane_id: panel.pane_id,
                    pane_anchor_panel_id: pane_anchor,
                    restore_in_original_pane: panel.restore_in_original_pane,
                    tab_index: panel.tab_index,
                    snapshot: panel.snapshot.clone(),
                    fallback_split_placement: fallback,
                }),
            )
        })
        .collect();
    (remapped, did_update)
}

/// Rewrites workspace records whose `window_id == Some(old_window_id)` onto
/// `new_window_id`. Returns the new records and whether anything changed.
pub fn records_by_remapping_workspace_window_ids(
    records: &[ClosedItemHistoryRecord],
    old_window_id: Uuid,
    new_window_id: Uuid,
) -> (Vec<ClosedItemHistoryRecord>, bool) {
    let mut did_update = false;
    let remapped = records
        .iter()
        .map(|record| {
            let ClosedItemHistoryEntry::Workspace(workspace) = &record.entry else {
                return record.clone();
            };
            if workspace.window_id != Some(old_window_id) {
                return record.clone();
            }
            did_update = true;
            ClosedItemHistoryRecord::new(
                record.id,
                record.closed_at,
                ClosedItemHistoryEntry::Workspace(ClosedWorkspaceHistoryEntry {
                    workspace_id: workspace.workspace_id,
                    window_id: Some(new_window_id),
                    workspace_index: workspace.workspace_index,
                    snapshot: workspace.snapshot.clone(),
                }),
            )
        })
        .collect();
    (remapped, did_update)
}

/// Drops panel records whose workspace id is in `workspace_ids`. Returns the new
/// records and whether anything was removed.
pub fn records_by_removing_panel_records(
    records: &[ClosedItemHistoryRecord],
    workspace_ids: &HashSet<Uuid>,
) -> (Vec<ClosedItemHistoryRecord>, bool) {
    let filtered: Vec<ClosedItemHistoryRecord> = records
        .iter()
        .filter(|record| {
            let ClosedItemHistoryEntry::Panel(panel) = &record.entry else {
                return true;
            };
            !workspace_ids.contains(&panel.workspace_id)
        })
        .cloned()
        .collect();
    let did_update = filtered.len() != records.len();
    (filtered, did_update)
}

/// Whether a restored closed-window has usable content: there must be live
/// panels, and if the snapshot carried restorable panels at least one workspace
/// must have restored a panel.
pub fn has_usable_restored_content(
    snapshot_has_restorable_panels: bool,
    restored_panel_ids_by_workspace_index: &[HashMap<Uuid, Uuid>],
    has_live_panels: bool,
) -> bool {
    if !has_live_panels {
        return false;
    }
    if !snapshot_has_restorable_panels {
        return true;
    }
    restored_panel_ids_by_workspace_index
        .iter()
        .any(|m| !m.is_empty())
}

// ---- The store (in-memory, no I/O) ------------------------------------------

/// In-memory closed-item history. A faithful port of the pure state machine of
/// `ClosedItemHistoryStore`, minus persistence/async (see module docs).
#[derive(Debug, Clone, Default)]
pub struct ClosedItemHistory {
    records: Vec<ClosedItemHistoryRecord>,
    capacity: Option<usize>,
    revision: u64,
}

impl ClosedItemHistory {
    /// Creates a history with an optional capacity. Matches Swift's
    /// `capacity.map { max(1, $0) }` (a capacity of 0 becomes 1).
    pub fn new(capacity: Option<usize>) -> Self {
        Self {
            records: Vec::new(),
            capacity: capacity.map(|c| c.max(1)),
            revision: 0,
        }
    }

    /// The current records in insertion order (oldest first).
    pub fn records(&self) -> &[ClosedItemHistoryRecord] {
        &self.records
    }

    /// The mutation counter (bumped on every mutating operation).
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Whether there is anything to reopen.
    pub fn can_reopen(&self) -> bool {
        !self.records.is_empty()
    }

    /// Appends a record, trimming the oldest overflow to capacity.
    pub fn push(&mut self, record: ClosedItemHistoryRecord) {
        self.records.push(record);
        self.trim_to_capacity();
        self.revision = self.revision.wrapping_add(1);
    }

    fn trim_to_capacity(&mut self) {
        if let Some(capacity) = self.capacity {
            if self.records.len() > capacity {
                let overflow = self.records.len() - capacity;
                self.records.drain(0..overflow);
            }
        }
    }

    /// Inserts a record at `index`, evicting *other* records to honor capacity
    /// so the just-inserted record is never the one dropped. When every other
    /// record is exhausted it falls back to removing the front (mirrors Swift).
    pub fn insert(&mut self, record: ClosedItemHistoryRecord, index: i64) {
        let clamped = index.max(0).min(self.records.len() as i64) as usize;
        let protected_record_id = record.id;
        self.records.insert(clamped, record);
        if let Some(capacity) = self.capacity {
            if self.records.len() > capacity {
                let overflow = self.records.len() - capacity;
                for _ in 0..overflow {
                    match self
                        .records
                        .iter()
                        .position(|r| r.id != protected_record_id)
                    {
                        Some(removal_index) => {
                            self.records.remove(removal_index);
                        }
                        None => {
                            self.records.remove(0);
                        }
                    }
                }
            }
        }
        self.revision = self.revision.wrapping_add(1);
    }

    /// Removes a record by id, returning it with its former index.
    pub fn remove_record(&mut self, id: Uuid) -> Option<(ClosedItemHistoryRecord, usize)> {
        let index = self.records.iter().position(|r| r.id == id)?;
        let record = self.records.remove(index);
        self.revision = self.revision.wrapping_add(1);
        Some((record, index))
    }

    /// Restores the first restorable candidate. Candidates are the records that
    /// pass `excluding` and the `newer_than` cutoff, sorted by close ordinal
    /// descending then original offset descending. `restore` returns whether the
    /// entry was actually restored; the first success is removed and reported.
    /// Failed candidates invoke `on_failure` and are skipped.
    pub fn restore_first_restorable<R, F>(
        &mut self,
        newer_than: Option<ClosedAt>,
        excluding: &HashSet<Uuid>,
        mut restore: R,
        mut on_failure: F,
    ) -> bool
    where
        R: FnMut(&ClosedItemHistoryEntry) -> bool,
        F: FnMut(Uuid),
    {
        let mut candidates: Vec<(usize, &ClosedItemHistoryRecord)> = self
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| {
                if excluding.contains(&record.id) {
                    return false;
                }
                match newer_than {
                    Some(cutoff) => record.closed_at >= cutoff,
                    None => true,
                }
            })
            .collect();
        candidates.sort_by(|(lhs_offset, lhs), (rhs_offset, rhs)| {
            if lhs.closed_at != rhs.closed_at {
                rhs.closed_at.cmp(&lhs.closed_at)
            } else {
                rhs_offset.cmp(lhs_offset)
            }
        });
        let candidate_ids: Vec<Uuid> = candidates.into_iter().map(|(_, r)| r.id).collect();

        for candidate_id in candidate_ids {
            let Some(entry) = self
                .records
                .iter()
                .find(|r| r.id == candidate_id)
                .map(|r| r.entry.clone())
            else {
                continue;
            };
            if !restore(&entry) {
                on_failure(candidate_id);
                continue;
            }
            if let Some(index) = self.records.iter().position(|r| r.id == candidate_id) {
                self.records.remove(index);
                self.revision = self.revision.wrapping_add(1);
            }
            return true;
        }
        false
    }

    /// Merges loaded persisted records ahead of the current ones, de-duplicating
    /// by id, then trims to capacity. No-op for an empty input.
    pub fn merge_loaded_persisted_records(&mut self, loaded_records: &[ClosedItemHistoryRecord]) {
        if loaded_records.is_empty() {
            return;
        }
        if self.records.is_empty() {
            self.records = loaded_records.to_vec();
        } else {
            let mut seen: HashSet<Uuid> = self.records.iter().map(|r| r.id).collect();
            let missing: Vec<ClosedItemHistoryRecord> = loaded_records
                .iter()
                .filter(|r| seen.insert(r.id))
                .cloned()
                .collect();
            if missing.is_empty() {
                return;
            }
            let mut merged = missing;
            merged.append(&mut self.records);
            self.records = merged;
        }
        self.trim_to_capacity();
        self.revision = self.revision.wrapping_add(1);
    }

    /// The recently-closed menu projection (most recent first), optionally
    /// limited to the `max_item_count` most recent records.
    pub fn menu_snapshot(&self, max_item_count: Option<usize>) -> MenuSnapshot {
        let total = self.records.len();
        if let Some(max) = max_item_count {
            if total > max {
                let items = self.records[total - max..]
                    .iter()
                    .rev()
                    .map(menu_item_for)
                    .collect();
                return MenuSnapshot {
                    items,
                    total_item_count: total,
                    is_limited: true,
                };
            }
        }
        MenuSnapshot {
            items: self.records.iter().rev().map(menu_item_for).collect(),
            total_item_count: total,
            is_limited: false,
        }
    }

    // -- Store-level remap/removal wrappers (guard old != new, then apply). --

    /// Remaps panel records from one workspace id to another (see the free fn).
    pub fn remap_panel_workspace_ids(
        &mut self,
        old_workspace_id: Uuid,
        new_workspace_id: Uuid,
        panel_id_map: &HashMap<Uuid, Uuid>,
    ) {
        if old_workspace_id == new_workspace_id {
            return;
        }
        let (records, did_update) = records_by_remapping_panel_workspace_ids(
            &self.records,
            old_workspace_id,
            new_workspace_id,
            panel_id_map,
        );
        if did_update {
            self.records = records;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Remaps panel anchor references from one panel id to another.
    pub fn remap_panel_anchor_ids(&mut self, old_panel_id: Uuid, new_panel_id: Uuid) {
        if old_panel_id == new_panel_id {
            return;
        }
        let (records, did_update) =
            records_by_remapping_panel_anchor_ids(&self.records, old_panel_id, new_panel_id);
        if did_update {
            self.records = records;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Remaps workspace records from one window id to another.
    pub fn remap_workspace_window_ids(&mut self, old_window_id: Uuid, new_window_id: Uuid) {
        if old_window_id == new_window_id {
            return;
        }
        let (records, did_update) =
            records_by_remapping_workspace_window_ids(&self.records, old_window_id, new_window_id);
        if did_update {
            self.records = records;
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Removes panel records for the given workspace ids.
    pub fn remove_panel_records(&mut self, workspace_ids: &HashSet<Uuid>) {
        if workspace_ids.is_empty() {
            return;
        }
        let (records, did_update) =
            records_by_removing_panel_records(&self.records, workspace_ids);
        if did_update {
            self.records = records;
            self.revision = self.revision.wrapping_add(1);
        }
    }
}

/// Simplified menu-title projection (see module docs — the full localized
/// projection is deferred). Picks the first non-empty title candidate; else a
/// kind-based default.
fn menu_item_for(record: &ClosedItemHistoryRecord) -> MenuItem {
    let (title, detail) = match &record.entry {
        ClosedItemHistoryEntry::Panel(entry) => {
            let candidates = [
                entry.snapshot.custom_title.clone(),
                entry.snapshot.title.clone(),
                entry.snapshot.directory.as_deref().map(last_path_component),
            ];
            (
                first_non_empty(&candidates).unwrap_or_else(|| "Terminal".to_string()),
                "Tab".to_string(),
            )
        }
        ClosedItemHistoryEntry::Workspace(entry) => {
            let candidates = [
                entry.snapshot.custom_title.clone(),
                Some(entry.snapshot.process_title.clone()),
                directory_title_candidate(&entry.snapshot.current_directory),
            ];
            (
                first_non_empty(&candidates).unwrap_or_else(|| "Untitled Workspace".to_string()),
                "Workspace".to_string(),
            )
        }
        ClosedItemHistoryEntry::Window(_) => ("Window".to_string(), "Window".to_string()),
    };
    MenuItem {
        id: record.id,
        title,
        detail,
        closed_at: record.closed_at,
    }
}

fn first_non_empty(candidates: &[Option<String>]) -> Option<String> {
    candidates
        .iter()
        .filter_map(|c| c.as_ref())
        .map(|c| c.trim().to_string())
        .find(|c| !c.is_empty())
}

fn directory_title_candidate(directory: &str) -> Option<String> {
    let trimmed = directory.trim();
    if trimmed.is_empty() || trimmed == "." {
        return None;
    }
    Some(last_path_component(trimmed))
}

/// Last path component, string-only (no filesystem stat). Handles both `/` and
/// `\` separators so it is correct cross-platform.
fn last_path_component(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    match trimmed.rsplit(['/', '\\']).next() {
        Some(last) if !last.is_empty() => last.to_string(),
        _ => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panel_entry(workspace_id: Uuid) -> ClosedItemHistoryEntry {
        ClosedItemHistoryEntry::Panel(ClosedPanelHistoryEntry {
            workspace_id,
            pane_id: Uuid::new_v4(),
            pane_anchor_panel_id: None,
            restore_in_original_pane: true,
            tab_index: 0,
            snapshot: PanelSnapshot::default(),
            fallback_split_placement: None,
        })
    }

    fn record(id: Uuid, closed_at: ClosedAt, entry: ClosedItemHistoryEntry) -> ClosedItemHistoryRecord {
        ClosedItemHistoryRecord::new(id, closed_at, entry)
    }

    #[test]
    fn push_trims_oldest_to_capacity() {
        let mut history = ClosedItemHistory::new(Some(2));
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        history.push(record(a, 1, panel_entry(Uuid::new_v4())));
        history.push(record(b, 2, panel_entry(Uuid::new_v4())));
        history.push(record(c, 3, panel_entry(Uuid::new_v4())));
        // Oldest (a) evicted.
        assert_eq!(
            history.records().iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![b, c]
        );
    }

    #[test]
    fn capacity_zero_becomes_one() {
        let mut history = ClosedItemHistory::new(Some(0));
        history.push(record(Uuid::new_v4(), 1, panel_entry(Uuid::new_v4())));
        let last = Uuid::new_v4();
        history.push(record(last, 2, panel_entry(Uuid::new_v4())));
        assert_eq!(history.records().len(), 1);
        assert_eq!(history.records()[0].id, last);
    }

    #[test]
    fn insert_protects_just_inserted_record_from_eviction() {
        // Capacity 2, already full; inserting a third at the front must evict an
        // *other* record, never the just-inserted one.
        let mut history = ClosedItemHistory::new(Some(2));
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        history.push(record(a, 1, panel_entry(Uuid::new_v4())));
        history.push(record(b, 2, panel_entry(Uuid::new_v4())));
        let protected = Uuid::new_v4();
        history.insert(record(protected, 3, panel_entry(Uuid::new_v4())), 0);
        assert_eq!(history.records().len(), 2);
        assert!(history.records().iter().any(|r| r.id == protected));
        // The front (oldest other) got evicted.
        assert!(!history.records().iter().any(|r| r.id == a));
    }

    #[test]
    fn insert_clamps_index() {
        let mut history = ClosedItemHistory::new(None);
        let a = Uuid::new_v4();
        history.push(record(a, 1, panel_entry(Uuid::new_v4())));
        let b = Uuid::new_v4();
        // Out-of-range index clamps to the end.
        history.insert(record(b, 2, panel_entry(Uuid::new_v4())), 99);
        assert_eq!(history.records()[1].id, b);
        let c = Uuid::new_v4();
        // Negative index clamps to the front.
        history.insert(record(c, 3, panel_entry(Uuid::new_v4())), -5);
        assert_eq!(history.records()[0].id, c);
    }

    #[test]
    fn restore_first_restorable_orders_by_closed_at_then_offset_desc() {
        let mut history = ClosedItemHistory::new(None);
        let older = Uuid::new_v4();
        let newer_first = Uuid::new_v4();
        let newer_second = Uuid::new_v4();
        history.push(record(older, 1, panel_entry(Uuid::new_v4())));
        history.push(record(newer_first, 5, panel_entry(Uuid::new_v4())));
        history.push(record(newer_second, 5, panel_entry(Uuid::new_v4())));
        // Same closedAt tie broken by higher offset (last pushed) first.
        let ok = history.restore_first_restorable(
            None,
            &HashSet::new(),
            |_entry| true,
            |_id| {},
        );
        assert!(ok);
        // newer_second (offset 2, closedAt 5) should have been restored+removed.
        assert!(!history.records().iter().any(|r| r.id == newer_second));
        assert_eq!(history.records().len(), 2);
        let _ = older;
        let _ = newer_first;
    }

    #[test]
    fn restore_first_restorable_honors_cutoff_and_exclusions_and_failure() {
        let mut history = ClosedItemHistory::new(None);
        let too_old = Uuid::new_v4();
        let excluded = Uuid::new_v4();
        let target = Uuid::new_v4();
        history.push(record(too_old, 1, panel_entry(Uuid::new_v4())));
        history.push(record(excluded, 10, panel_entry(Uuid::new_v4())));
        history.push(record(target, 8, panel_entry(Uuid::new_v4())));

        let excluding: HashSet<Uuid> = HashSet::from([excluded]);
        let mut failures: Vec<Uuid> = Vec::new();
        // Cutoff 5 drops too_old; excluded is skipped; the first (highest
        // closedAt among survivors) is `target`.
        let ok = history.restore_first_restorable(
            Some(5),
            &excluding,
            |_entry| true,
            |id| failures.push(id),
        );
        assert!(ok);
        assert!(failures.is_empty());
        assert!(!history.records().iter().any(|r| r.id == target));
        // too_old and excluded remain.
        assert_eq!(history.records().len(), 2);
    }

    #[test]
    fn restore_first_restorable_skips_failed_candidates() {
        // Tag each panel's workspace id so the restore closure can identify which
        // candidate it is inspecting. The highest-closedAt candidate fails; the
        // next one succeeds and is removed.
        let first_ws = Uuid::new_v4();
        let second_ws = Uuid::new_v4();
        let mut history = ClosedItemHistory::new(None);
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        history.push(record(first, 2, panel_entry(first_ws)));
        history.push(record(second, 1, panel_entry(second_ws)));
        let mut failures: Vec<Uuid> = Vec::new();
        let ok = history.restore_first_restorable(
            None,
            &HashSet::new(),
            |entry| match entry {
                // Fail the first (higher closedAt) candidate, accept the second.
                ClosedItemHistoryEntry::Panel(p) => p.workspace_id != first_ws,
                _ => true,
            },
            |id| failures.push(id),
        );
        assert!(ok);
        assert_eq!(failures, vec![first]);
        // `second` restored+removed; `first` remains.
        assert!(!history.records().iter().any(|r| r.id == second));
        assert!(history.records().iter().any(|r| r.id == first));
    }

    #[test]
    fn restore_first_restorable_returns_false_when_all_fail() {
        let mut history = ClosedItemHistory::new(None);
        history.push(record(Uuid::new_v4(), 2, panel_entry(Uuid::new_v4())));
        history.push(record(Uuid::new_v4(), 1, panel_entry(Uuid::new_v4())));
        let mut failures = 0;
        let ok = history.restore_first_restorable(
            None,
            &HashSet::new(),
            |_entry| false,
            |_id| failures += 1,
        );
        assert!(!ok);
        assert_eq!(failures, 2);
        // Nothing removed.
        assert_eq!(history.records().len(), 2);
    }

    #[test]
    fn remap_panel_workspace_ids_rewrites_and_forces_non_original_pane() {
        let old_ws = Uuid::new_v4();
        let new_ws = Uuid::new_v4();
        let old_anchor = Uuid::new_v4();
        let new_anchor = Uuid::new_v4();
        let rec = ClosedItemHistoryRecord::new(
            Uuid::new_v4(),
            1,
            ClosedItemHistoryEntry::Panel(ClosedPanelHistoryEntry {
                workspace_id: old_ws,
                pane_id: Uuid::new_v4(),
                pane_anchor_panel_id: Some(old_anchor),
                restore_in_original_pane: true,
                tab_index: 3,
                snapshot: PanelSnapshot::default(),
                fallback_split_placement: Some(ClosedPanelSplitPlacement {
                    orientation: SplitOrientation::Vertical,
                    insert_first: true,
                    anchor_panel_id: Some(old_anchor),
                }),
            }),
        );
        let map: HashMap<Uuid, Uuid> = HashMap::from([(old_anchor, new_anchor)]);
        let (records, did_update) =
            records_by_remapping_panel_workspace_ids(&[rec], old_ws, new_ws, &map);
        assert!(did_update);
        let ClosedItemHistoryEntry::Panel(panel) = &records[0].entry else {
            panic!("expected panel");
        };
        assert_eq!(panel.workspace_id, new_ws);
        assert!(!panel.restore_in_original_pane);
        assert_eq!(panel.pane_anchor_panel_id, Some(new_anchor));
        assert_eq!(
            panel.fallback_split_placement.as_ref().unwrap().anchor_panel_id,
            Some(new_anchor)
        );
        // Non-matching workspace → no change.
        let (_, did_update2) =
            records_by_remapping_panel_workspace_ids(&records, old_ws, new_ws, &map);
        assert!(!did_update2);
    }

    #[test]
    fn remap_panel_anchor_ids_updates_both_anchors() {
        let old_anchor = Uuid::new_v4();
        let new_anchor = Uuid::new_v4();
        let rec = ClosedItemHistoryRecord::new(
            Uuid::new_v4(),
            1,
            ClosedItemHistoryEntry::Panel(ClosedPanelHistoryEntry {
                workspace_id: Uuid::new_v4(),
                pane_id: Uuid::new_v4(),
                pane_anchor_panel_id: Some(old_anchor),
                restore_in_original_pane: true,
                tab_index: 0,
                snapshot: PanelSnapshot::default(),
                fallback_split_placement: Some(ClosedPanelSplitPlacement {
                    orientation: SplitOrientation::Horizontal,
                    insert_first: false,
                    anchor_panel_id: Some(old_anchor),
                }),
            }),
        );
        let (records, did_update) =
            records_by_remapping_panel_anchor_ids(&[rec], old_anchor, new_anchor);
        assert!(did_update);
        let ClosedItemHistoryEntry::Panel(panel) = &records[0].entry else {
            panic!("expected panel");
        };
        assert_eq!(panel.pane_anchor_panel_id, Some(new_anchor));
        assert_eq!(
            panel.fallback_split_placement.as_ref().unwrap().anchor_panel_id,
            Some(new_anchor)
        );
        // Idempotent second pass reports no update.
        let (_, again) = records_by_remapping_panel_anchor_ids(&records, old_anchor, new_anchor);
        assert!(!again);
    }

    #[test]
    fn remap_workspace_window_ids_matches_some_old() {
        let old_window = Uuid::new_v4();
        let new_window = Uuid::new_v4();
        let rec = ClosedItemHistoryRecord::new(
            Uuid::new_v4(),
            1,
            ClosedItemHistoryEntry::Workspace(ClosedWorkspaceHistoryEntry {
                workspace_id: Uuid::new_v4(),
                window_id: Some(old_window),
                workspace_index: 2,
                snapshot: WorkspaceSnapshot::default(),
            }),
        );
        let (records, did_update) =
            records_by_remapping_workspace_window_ids(&[rec], old_window, new_window);
        assert!(did_update);
        let ClosedItemHistoryEntry::Workspace(ws) = &records[0].entry else {
            panic!("expected workspace");
        };
        assert_eq!(ws.window_id, Some(new_window));
        // A None window id never matches.
        let none_rec = ClosedItemHistoryRecord::new(
            Uuid::new_v4(),
            1,
            ClosedItemHistoryEntry::Workspace(ClosedWorkspaceHistoryEntry {
                workspace_id: Uuid::new_v4(),
                window_id: None,
                workspace_index: 0,
                snapshot: WorkspaceSnapshot::default(),
            }),
        );
        let (_, none_update) =
            records_by_remapping_workspace_window_ids(&[none_rec], old_window, new_window);
        assert!(!none_update);
    }

    #[test]
    fn removing_panel_records_filters_matching_workspaces() {
        let ws = Uuid::new_v4();
        let keep_ws = Uuid::new_v4();
        let records = vec![
            record(Uuid::new_v4(), 1, panel_entry(ws)),
            record(Uuid::new_v4(), 2, panel_entry(keep_ws)),
            record(
                Uuid::new_v4(),
                3,
                ClosedItemHistoryEntry::Workspace(ClosedWorkspaceHistoryEntry {
                    workspace_id: ws,
                    window_id: None,
                    workspace_index: 0,
                    snapshot: WorkspaceSnapshot::default(),
                }),
            ),
        ];
        let (filtered, did_update) =
            records_by_removing_panel_records(&records, &HashSet::from([ws]));
        assert!(did_update);
        // The panel for `ws` is gone; the panel for keep_ws and the workspace
        // record survive.
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|r| match &r.entry {
            ClosedItemHistoryEntry::Panel(p) => p.workspace_id != ws,
            _ => true,
        }));
    }

    #[test]
    fn merge_loaded_dedups_by_id_and_prepends() {
        let mut history = ClosedItemHistory::new(None);
        let shared = Uuid::new_v4();
        let existing = Uuid::new_v4();
        history.push(record(existing, 5, panel_entry(Uuid::new_v4())));
        history.push(record(shared, 6, panel_entry(Uuid::new_v4())));
        let loaded_new = Uuid::new_v4();
        let loaded = vec![
            record(loaded_new, 1, panel_entry(Uuid::new_v4())),
            record(shared, 6, panel_entry(Uuid::new_v4())), // dup id → dropped
        ];
        history.merge_loaded_persisted_records(&loaded);
        // loaded_new prepended, shared not duplicated.
        assert_eq!(
            history.records().iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![loaded_new, existing, shared]
        );
    }

    #[test]
    fn merge_loaded_empty_is_noop() {
        let mut history = ClosedItemHistory::new(None);
        history.push(record(Uuid::new_v4(), 1, panel_entry(Uuid::new_v4())));
        let before = history.revision();
        history.merge_loaded_persisted_records(&[]);
        assert_eq!(history.revision(), before);
    }

    #[test]
    fn menu_snapshot_limits_to_most_recent() {
        let mut history = ClosedItemHistory::new(None);
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        history.push(record(a, 1, panel_entry(Uuid::new_v4())));
        history.push(record(b, 2, panel_entry(Uuid::new_v4())));
        history.push(record(c, 3, panel_entry(Uuid::new_v4())));
        let snap = history.menu_snapshot(Some(2));
        assert!(snap.is_limited);
        assert_eq!(snap.total_item_count, 3);
        // Most recent first: c, b (a dropped).
        assert_eq!(snap.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![c, b]);

        let full = history.menu_snapshot(None);
        assert!(!full.is_limited);
        assert_eq!(full.items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![c, b, a]);
    }

    #[test]
    fn has_usable_restored_content_rules() {
        // No live panels → never usable.
        assert!(!has_usable_restored_content(true, &[], false));
        // Live panels, snapshot had no restorable panels → usable.
        assert!(has_usable_restored_content(false, &[], true));
        // Restorable panels but nothing actually restored → not usable.
        assert!(!has_usable_restored_content(true, &[HashMap::new()], true));
        // Restorable panels and at least one restored → usable.
        let restored = vec![HashMap::from([(Uuid::new_v4(), Uuid::new_v4())])];
        assert!(has_usable_restored_content(true, &restored, true));
    }

    #[test]
    fn last_path_component_handles_both_separators() {
        assert_eq!(last_path_component("/home/user/proj"), "proj");
        assert_eq!(last_path_component("C:\\Users\\me\\proj"), "proj");
        assert_eq!(last_path_component("/home/user/proj/"), "proj");
    }
}
