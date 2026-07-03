//! PART B — the client-side frame-application state machine.
//!
//! A faithful port of `SyncFrameApplier.swift` (the pure fold) plus the in-memory
//! record store it delegates to (`CmuxSyncStore.swift`'s `applyDelta` /
//! `applySnapshot` / `cursor` / `epoch` semantics). The applier drives a record
//! store from a stream of [`SyncServerFrame`]s, handling the snapshot-paging
//! buffer and the concurrent-delta queue so a delete racing a snapshot is never
//! lost, and the per-collection buffer caps that bound memory against a
//! misbehaving producer.
//!
//! # Sanctioned platform divergences (Swift → Rust)
//! - **`actor` / `async` → synchronous plain struct.** The Swift applier is an
//!   `actor` whose every method is `async` (frames arrive serially from the WS
//!   receive loop; the actor serializes store writes). Per the port spec, this is
//!   modelled as a synchronous, single-threaded struct — the serialization the
//!   actor provided is inherent to `&mut self`.
//! - **The injected `any CmuxSyncStoring` (a raw-SQLite `actor`) → an in-memory
//!   [`InMemorySyncStore`].** The spec directs porting ONLY the pure fold, so the
//!   store is a plain in-memory record map that reproduces the SQLite store's
//!   observable semantics: the per-record monotone `local.rev >= r.rev` guard, the
//!   `MAX` cursor advance, the epoch-reset force-apply, and the missing-record
//!   tombstone reconciliation. The `/1000` ms→s `updated_at` unit boundary is
//!   preserved.
//! - **`teamID` is dropped.** One applier serves exactly one team (Swift threads a
//!   constant `teamID` through every store call); the in-memory store is therefore
//!   keyed by collection only.
//! - **`Foundation.Date` `now` → `f64` epoch seconds** (`timeIntervalSince1970`).
//!   It feeds only tombstone `updated_at` and cursor `synced_at`, neither of which
//!   is observable in the pure fold.
//! - **`apply(_:) throws -> Bool` → `apply(..) -> Result<bool, SyncFrameParseError>`.**
//!   The Swift `throws` (buffer-cap overflow and unrequested-collection rejection)
//!   maps to `Err`.
//!
//! ## Return-value contract (faithful to the Swift literal)
//! The Swift `apply` DOCUMENTS the returned `Bool` as "whether a sync commit
//! actually happened (the store was written or the cursor advanced)", and
//! `SyncClient` uses it to gate the UI-invalidation callback. The IMPLEMENTATION,
//! however, returns `true` UNCONDITIONALLY once a non-paging delta or tick reaches
//! `store.applyDelta`: `applyDeltaFrame` runs the store write then `return true`
//! (SyncFrameApplier.swift line 223), and the tick arm likewise `return true`
//! (line 134) — even for a stale/duplicate delta whose store write is a
//! monotone-guard no-op. Only an `.unknown` presence frame, an incomplete
//! (buffered-only) snapshot page, and a delta queued during paging return
//! `false`. This port matches that literal EXACTLY: the store's `apply_delta` /
//! `apply_one_record` / `set_cursor` are `Void` (as in Swift), and no "did
//! anything change" result is consulted to decide the applier's return.

use crate::codec::{SyncFrameParseError, SyncServerFrame, SyncWireRecord};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::cmp::Ordering;

/// A record as the store holds it. Mirrors Swift `StoredSyncRecord` (minus the
/// dropped `team_id`). `updated_at` is epoch SECONDS (the wire ms divided by 1000
/// on write — the single documented unit boundary).
#[derive(Debug, Clone, PartialEq)]
pub struct StoredSyncRecord {
    pub collection: String,
    pub record_id: String,
    pub rev: i64,
    pub updated_at: f64,
    pub sort_key: f64,
    pub deleted: bool,
    pub payload_json: Vec<u8>,
}

/// Default ceiling on records accumulated across snapshot pages before a
/// `complete` page arrives. (Swift `defaultMaxBufferedRecords`.)
pub const DEFAULT_MAX_BUFFERED_RECORDS: usize = 100_000;
/// Default ceiling on total records retained across deltas queued while a snapshot
/// is still paging. (Swift `defaultMaxQueuedDeltaRecords`.)
pub const DEFAULT_MAX_QUEUED_DELTA_RECORDS: usize = 10_000;
/// Default ceiling on the NUMBER of delta frames queued while a snapshot is
/// paging. (Swift `defaultMaxQueuedDeltaFrames`.)
pub const DEFAULT_MAX_QUEUED_DELTA_FRAMES: usize = 10_000;

/// `Fn(&SyncWireRecord) -> f64` — the render sort key for a record.
type SortKeyFn = dyn Fn(&SyncWireRecord) -> f64;
/// `Fn() -> f64` — the current time as epoch seconds.
type NowFn = dyn Fn() -> f64;

/// One `(team, collection)` cursor row. Mirrors the `sync_cursors` row.
#[derive(Debug, Clone, Copy)]
struct CursorRow {
    cursor_rev: i64,
    epoch: i64,
    #[allow(dead_code)] // parity field; not observable in the pure fold
    synced_at: f64,
}

/// The in-memory record store: reproduces the observable semantics of
/// `CmuxSyncStore`'s `sync_records` + `sync_cursors` tables for one team.
#[derive(Debug, Default)]
struct InMemorySyncStore {
    /// collection -> (record_id -> record). A `BTreeMap` for the inner map gives
    /// deterministic id iteration (reconciliation scan + live-read tie-break).
    records: HashMap<String, BTreeMap<String, StoredSyncRecord>>,
    /// collection -> cursor row.
    cursors: HashMap<String, CursorRow>,
}

impl InMemorySyncStore {
    fn cursor(&self, collection: &str) -> i64 {
        self.cursors.get(collection).map_or(0, |c| c.cursor_rev)
    }

    fn epoch(&self, collection: &str) -> i64 {
        self.cursors.get(collection).map_or(0, |c| c.epoch)
    }

    /// Live (non-tombstone) records of a collection, in render order
    /// (`sort_key DESC`). Mirrors Swift `liveRecords` (`ORDER BY sort_key DESC`).
    /// SQLite leaves the order of rows with an EQUAL `sort_key` UNSPECIFIED, so
    /// Swift itself has no defined tie order here; this port breaks such ties by
    /// `record_id` ascending (a stable sort over the id-ordered `BTreeMap` input).
    /// That is a deterministic REFINEMENT of a Swift-undefined order — it cannot
    /// contradict Swift — not a behavioral divergence.
    fn live_records(&self, collection: &str) -> Vec<StoredSyncRecord> {
        let mut out: Vec<StoredSyncRecord> = self
            .records
            .get(collection)
            .into_iter()
            .flat_map(|map| map.values())
            .filter(|r| !r.deleted)
            .cloned()
            .collect();
        // Stable sort over id-ordered input → deterministic ordering on ties.
        out.sort_by(|a, b| {
            b.sort_key
                .partial_cmp(&a.sort_key)
                .unwrap_or(Ordering::Equal)
        });
        out
    }

    fn record_rev(&self, collection: &str, record_id: &str) -> Option<i64> {
        self.records
            .get(collection)
            .and_then(|map| map.get(record_id))
            .map(|r| r.rev)
    }

    fn stored_record(&self, collection: &str, record_id: &str) -> Option<StoredSyncRecord> {
        self.records
            .get(collection)
            .and_then(|map| map.get(record_id))
            .cloned()
    }

    /// Record ids whose rev is within `[min_rev, max_rev]` (inclusive), including
    /// tombstones. Mirrors `allRecordIDs` (no `deleted` filter).
    fn all_record_ids(&self, collection: &str, min_rev: i64, max_rev: i64) -> Vec<String> {
        self.records
            .get(collection)
            .into_iter()
            .flat_map(|map| map.values())
            .filter(|r| r.rev >= min_rev && r.rev <= max_rev)
            .map(|r| r.record_id.clone())
            .collect()
    }

    /// Apply one wire record under the monotone `local.rev >= r.rev` guard. A
    /// stale/duplicate record (rev not newer) is skipped; otherwise it upserts.
    /// Mirrors Swift `applyOneRecord` (a `Void` method).
    fn apply_one_record(&mut self, collection: &str, record: &SyncWireRecord, sort_key: f64) {
        if let Some(local_rev) = self.record_rev(collection, &record.id) {
            if local_rev >= record.rev {
                return; // stale or duplicate; keep the higher rev we hold
            }
        }
        self.write_record(collection, record, sort_key);
    }

    /// Apply one record UNCONDITIONALLY (no monotone guard), used during a reset
    /// snapshot where the snapshot is the new ground truth. Mirrors
    /// `forceApplyRecord`.
    fn force_apply_record(&mut self, collection: &str, record: &SyncWireRecord, sort_key: f64) {
        self.write_record(collection, record, sort_key);
    }

    /// The shared upsert body of `applyOneRecord` / `forceApplyRecord`. The wire
    /// `updated_at` (epoch ms) is divided by 1000 into epoch seconds — the single
    /// documented unit boundary. A tombstone stores `{}` as its payload.
    fn write_record(&mut self, collection: &str, record: &SyncWireRecord, sort_key: f64) {
        let updated_at_seconds = record.updated_at / 1000.0;
        let payload_json = if record.deleted {
            b"{}".to_vec()
        } else {
            record.payload_json.clone()
        };
        let stored = StoredSyncRecord {
            collection: collection.to_string(),
            record_id: record.id.clone(),
            rev: record.rev,
            updated_at: updated_at_seconds,
            sort_key,
            deleted: record.deleted,
            payload_json,
        };
        self.records
            .entry(collection.to_string())
            .or_default()
            .insert(record.id.clone(), stored);
    }

    /// Write a tombstone for a record at a given rev. Mirrors `tombstoneAt`: on an
    /// existing row it updates rev/updated_at/deleted/payload but PRESERVES
    /// `sort_key`; a fresh insert uses `sort_key = 0`.
    fn tombstone_at(&mut self, collection: &str, record_id: &str, rev: i64, now: f64) {
        let map = self.records.entry(collection.to_string()).or_default();
        match map.get_mut(record_id) {
            Some(existing) => {
                existing.rev = rev;
                existing.updated_at = now;
                existing.deleted = true;
                existing.payload_json = b"{}".to_vec();
                // sort_key intentionally preserved (Swift ON CONFLICT omits it).
            }
            None => {
                map.insert(
                    record_id.to_string(),
                    StoredSyncRecord {
                        collection: collection.to_string(),
                        record_id: record_id.to_string(),
                        rev,
                        updated_at: now,
                        sort_key: 0.0,
                        deleted: true,
                        payload_json: b"{}".to_vec(),
                    },
                );
            }
        }
    }

    /// Advance the cursor monotonically (`MAX`, never backward). `epoch = None`
    /// preserves the existing epoch (the delta path); `Some` adopts the server
    /// epoch (a snapshot commit). Mirrors Swift `setCursor` (a `Void` method).
    fn set_cursor(&mut self, collection: &str, rev: i64, epoch: Option<i64>, now: f64) {
        match self.cursors.get_mut(collection) {
            Some(row) => {
                row.cursor_rev = row.cursor_rev.max(rev);
                if let Some(e) = epoch {
                    row.epoch = e;
                }
                row.synced_at = now;
            }
            None => {
                // First insert writes `rev` directly (Swift inserts the excluded
                // value, not MAX(0, rev)); the implicit prior cursor is 0.
                self.cursors.insert(
                    collection.to_string(),
                    CursorRow {
                        cursor_rev: rev,
                        epoch: epoch.unwrap_or(0),
                        synced_at: now,
                    },
                );
            }
        }
    }

    /// Set the cursor UNCONDITIONALLY (no `MAX`) and adopt `epoch`, used on a reset
    /// snapshot to move the cursor DOWN to the new head. Mirrors `forceCursor`.
    fn force_cursor(&mut self, collection: &str, rev: i64, epoch: i64, now: f64) {
        self.cursors.insert(
            collection.to_string(),
            CursorRow {
                cursor_rev: rev,
                epoch,
                synced_at: now,
            },
        );
    }

    /// Apply one delta/tick frame atomically: upsert each record under the
    /// monotone guard, then advance the cursor to the frame head. Mirrors Swift
    /// `applyDelta` (a `Void` method). Whether the caller treats this as a
    /// UI-invalidating "commit" is decided by the applier, which follows Swift's
    /// unconditional `return true` for a non-paging delta/tick (see the
    /// module-level return-value contract).
    fn apply_delta(
        &mut self,
        collection: &str,
        frame_rev: i64,
        records: &[SyncWireRecord],
        sort_key_for: &SortKeyFn,
        now: f64,
    ) {
        for record in records {
            self.apply_one_record(collection, record, sort_key_for(record));
        }
        // The cursor advances to the frame head only after every record committed.
        self.set_cursor(collection, frame_rev, None, now);
    }

    /// Apply a completed snapshot atomically: upsert each record (force-apply on a
    /// reset), tombstone-reconcile authoritative rows absent from it, then set the
    /// cursor + epoch. Mirrors `applySnapshot`, including the reset detection
    /// (`localCursor > snapshotRev || (epoch != 0 && epoch != localEpoch)`) and the
    /// `tombRev = isReset ? max(snapshotRev, localRev) : snapshotRev` watermark.
    fn apply_snapshot(
        &mut self,
        collection: &str,
        snapshot_rev: i64,
        epoch: i64,
        records: &[SyncWireRecord],
        sort_key_for: &SortKeyFn,
        now: f64,
    ) {
        let local_cursor = self.cursor(collection);
        let local_epoch = self.epoch(collection);
        // A nonzero incoming epoch that differs from ours is a reset — including
        // when our local epoch is 0. A cursor ahead of the snapshot's head is also
        // a reset (history rolled back).
        let epoch_changed = epoch != 0 && epoch != local_epoch;
        let is_reset = local_cursor > snapshot_rev || epoch_changed;

        let mut present: HashSet<String> = HashSet::new();
        for record in records {
            let sort_key = sort_key_for(record);
            if is_reset {
                self.force_apply_record(collection, record, sort_key);
            } else {
                self.apply_one_record(collection, record, sort_key);
            }
            present.insert(record.id.clone());
        }

        // Missing-record reconciliation: authoritative rows in [1, maxRev] absent
        // from the snapshot are tombstoned (provisional rev == 0 rows are exempt).
        // On a reset the scan covers all authoritative rows (maxRev = i64::MAX).
        let max_rev = if is_reset { i64::MAX } else { snapshot_rev };
        let existing = self.all_record_ids(collection, 1, max_rev);
        for id in existing {
            if !present.contains(&id) {
                let local_rev = self.record_rev(collection, &id).unwrap_or(0);
                let tomb_rev = if is_reset {
                    snapshot_rev.max(local_rev)
                } else {
                    snapshot_rev
                };
                self.tombstone_at(collection, &id, tomb_rev, now);
            }
        }

        // On a reset the cursor must move DOWN to the new head; force it.
        if is_reset {
            self.force_cursor(collection, snapshot_rev, epoch, now);
        } else {
            self.set_cursor(collection, snapshot_rev, Some(epoch), now);
        }
    }
}

/// Per-collection in-flight snapshot: accumulated pages + the deltas that arrived
/// during paging (queued, applied after the snapshot commits). Mirrors Swift
/// `SnapshotBuild`.
#[derive(Debug)]
struct SnapshotBuild {
    snapshot_rev: i64,
    epoch: i64,
    records: Vec<SyncWireRecord>,
    queued_deltas: Vec<(i64, Vec<SyncWireRecord>)>,
}

impl SnapshotBuild {
    fn new(snapshot_rev: i64, epoch: i64) -> Self {
        Self {
            snapshot_rev,
            epoch,
            records: Vec::new(),
            queued_deltas: Vec::new(),
        }
    }
}

/// The client-side frame-application state machine. Drives an in-memory record
/// store from a stream of [`SyncServerFrame`]s. Mirrors Swift `SyncFrameApplier`
/// (synchronous, single-team). See the module docs for sanctioned divergences.
pub struct SyncFrameApplier {
    store: InMemorySyncStore,
    sort_key_for: Box<SortKeyFn>,
    now: Box<NowFn>,
    /// The collections this applier accepts frames for. Empty = accept any
    /// collection (kept for tests/back-compat); production passes the subscribed
    /// set. Mirrors `allowedCollections`.
    allowed_collections: HashSet<String>,
    /// Per-collection in-flight snapshot builds. Mirrors `builds`.
    builds: HashMap<String, SnapshotBuild>,
    max_buffered_records: usize,
    max_queued_delta_records: usize,
    max_queued_delta_frames: usize,
}

impl SyncFrameApplier {
    /// Construct an applier with the default caps, no collection allowlist, and a
    /// `now` of 0. `sort_key_for` maps a record to its render sort key.
    pub fn new(sort_key_for: impl Fn(&SyncWireRecord) -> f64 + 'static) -> Self {
        Self {
            store: InMemorySyncStore::default(),
            sort_key_for: Box::new(sort_key_for),
            now: Box::new(|| 0.0),
            allowed_collections: HashSet::new(),
            builds: HashMap::new(),
            max_buffered_records: DEFAULT_MAX_BUFFERED_RECORDS,
            max_queued_delta_records: DEFAULT_MAX_QUEUED_DELTA_RECORDS,
            max_queued_delta_frames: DEFAULT_MAX_QUEUED_DELTA_FRAMES,
        }
    }

    /// Override the `now` clock (epoch seconds).
    pub fn with_now(mut self, now: impl Fn() -> f64 + 'static) -> Self {
        self.now = Box::new(now);
        self
    }

    /// Restrict the applier to a set of collections; a frame for any other
    /// collection is rejected as [`SyncFrameParseError::Malformed`].
    pub fn with_allowed_collections<I, S>(mut self, collections: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_collections = collections.into_iter().map(Into::into).collect();
        self
    }

    /// Override the three buffer caps.
    pub fn with_caps(
        mut self,
        max_buffered_records: usize,
        max_queued_delta_records: usize,
        max_queued_delta_frames: usize,
    ) -> Self {
        self.max_buffered_records = max_buffered_records;
        self.max_queued_delta_records = max_queued_delta_records;
        self.max_queued_delta_frames = max_queued_delta_frames;
        self
    }

    /// The cursor to send in the next `sync.hello` for a collection.
    pub fn cursor(&self, collection: &str) -> i64 {
        self.store.cursor(collection)
    }

    /// The history epoch to send in the next `sync.hello` for a collection.
    pub fn epoch(&self, collection: &str) -> i64 {
        self.store.epoch(collection)
    }

    /// Live (non-tombstone) records of a collection, in render order.
    pub fn live_records(&self, collection: &str) -> Vec<StoredSyncRecord> {
        self.store.live_records(collection)
    }

    /// The stored record (including a tombstone) for an id, or `None` if the store
    /// has never seen it. Exposed for assertions.
    pub fn stored_record(&self, collection: &str, record_id: &str) -> Option<StoredSyncRecord> {
        self.store.stored_record(collection, record_id)
    }

    /// Whether a snapshot is currently paging for a collection.
    pub fn has_in_flight_snapshot(&self, collection: &str) -> bool {
        self.builds.contains_key(collection)
    }

    /// Reject a frame for a collection this applier was not configured to accept.
    /// Mirrors `requireAllowed`.
    fn require_allowed(&self, collection: &str) -> Result<(), SyncFrameParseError> {
        if !self.allowed_collections.is_empty() && !self.allowed_collections.contains(collection) {
            return Err(SyncFrameParseError::Malformed(format!(
                "frame for unrequested collection {collection}"
            )));
        }
        Ok(())
    }

    /// Apply one server frame. Snapshot pages buffer until `complete`; deltas
    /// received mid-paging are queued and drained after the snapshot commits;
    /// deltas/ticks outside paging apply immediately. [`SyncServerFrame::Unknown`]
    /// (a presence frame) is ignored. Returns `true` when a non-paging delta/tick
    /// reaches the store or a snapshot's `complete` page commits; `false` for an
    /// unknown frame, a buffered-only page, or a queued delta (see the
    /// module-level return-value contract). Mirrors Swift `apply`.
    pub fn apply(&mut self, frame: SyncServerFrame) -> Result<bool, SyncFrameParseError> {
        match frame {
            SyncServerFrame::Snapshot {
                collection,
                snapshot_rev,
                epoch,
                records,
                complete,
            } => {
                self.require_allowed(&collection)?;
                self.apply_snapshot_page(collection, snapshot_rev, epoch, records, complete)
            }
            SyncServerFrame::Delta {
                collection,
                rev,
                records,
            } => {
                self.require_allowed(&collection)?;
                self.apply_delta_frame(collection, rev, records)
            }
            SyncServerFrame::Tick { collection, rev } => {
                self.require_allowed(&collection)?;
                // A tick advances the cursor when nothing record-shaped changed.
                // During paging it is ignored (the snapshot commit sets the
                // cursor); otherwise apply it as an empty delta. Swift returns
                // `true` unconditionally for the non-paging tick (line 134).
                if self.builds.contains_key(&collection) {
                    Ok(false)
                } else {
                    let now = (self.now)();
                    self.store
                        .apply_delta(&collection, rev, &[], self.sort_key_for.as_ref(), now);
                    Ok(true)
                }
            }
            SyncServerFrame::Unknown => Ok(false), // presence frame or future type
        }
    }

    /// Discard any in-flight snapshot build for a collection on a stream drop, so a
    /// half-applied snapshot never commits. Mirrors `resetInFlight`.
    pub fn reset_in_flight(&mut self) {
        self.builds.clear();
    }

    /// Returns `true` once the snapshot's `complete` page commits; an incomplete
    /// page only buffers and returns `false`. Mirrors `applySnapshotPage`.
    fn apply_snapshot_page(
        &mut self,
        collection: String,
        snapshot_rev: i64,
        epoch: i64,
        records: Vec<SyncWireRecord>,
        complete: bool,
    ) -> Result<bool, SyncFrameParseError> {
        // Take ownership of any prior build (Swift reads a value-type copy then
        // re-stores; taking + re-inserting is equivalent).
        let mut build = self
            .builds
            .remove(&collection)
            .unwrap_or_else(|| SnapshotBuild::new(snapshot_rev, epoch));
        // A snapshotRev or epoch change mid-paging means the server restarted the
        // snapshot; discard the stale buffer.
        if build.snapshot_rev != snapshot_rev || build.epoch != epoch {
            build = SnapshotBuild::new(snapshot_rev, epoch);
        }
        // Bound the buffer before appending. On overflow the build is already
        // removed from the map; surface a malformed frame so the transport
        // tears down and re-hellos.
        if build.records.len() + records.len() > self.max_buffered_records {
            return Err(SyncFrameParseError::Malformed(format!(
                "snapshot for {collection} exceeded {} buffered records before completing",
                self.max_buffered_records
            )));
        }
        build.records.extend(records);
        if !complete {
            self.builds.insert(collection, build);
            return Ok(false); // buffered only, nothing committed yet
        }
        // Commit the full snapshot atomically, then drain the deltas that raced
        // the paging.
        let now = (self.now)();
        self.store.apply_snapshot(
            &collection,
            snapshot_rev,
            build.epoch,
            &build.records,
            self.sort_key_for.as_ref(),
            now,
        );
        for (rev, delta_records) in build.queued_deltas {
            let now = (self.now)();
            self.store.apply_delta(
                &collection,
                rev,
                &delta_records,
                self.sort_key_for.as_ref(),
                now,
            );
        }
        Ok(true)
    }

    /// Returns `true` once the delta reaches `store.apply_delta`; `false` ONLY when
    /// it is queued during paging (committed later when the snapshot completes).
    /// Mirrors Swift `applyDeltaFrame`, which returns `true` UNCONDITIONALLY after
    /// the store write (SyncFrameApplier.swift line 223) — even for a
    /// stale/duplicate delta whose store write is a monotone-guard no-op.
    fn apply_delta_frame(
        &mut self,
        collection: String,
        rev: i64,
        records: Vec<SyncWireRecord>,
    ) -> Result<bool, SyncFrameParseError> {
        if let Some(build) = self.builds.get(&collection) {
            // Mid-paging: queue, do not apply yet. Bound the queue on TWO
            // independent axes (total retained records AND total frames) so a
            // never-completing snapshot cannot grow memory without limit.
            let queued_frames = build.queued_deltas.len();
            let queued_records: usize =
                build.queued_deltas.iter().map(|(_, r)| r.len()).sum();
            if queued_frames + 1 > self.max_queued_delta_frames
                || queued_records + records.len() > self.max_queued_delta_records
            {
                self.builds.remove(&collection);
                return Err(SyncFrameParseError::Malformed(format!(
                    "queued deltas for {collection} exceeded the queue bound (frames {}, records {}) while snapshot never completed",
                    self.max_queued_delta_frames, self.max_queued_delta_records
                )));
            }
            self.builds
                .get_mut(&collection)
                .expect("build present in this branch")
                .queued_deltas
                .push((rev, records));
            return Ok(false);
        }
        let now = (self.now)();
        self.store
            .apply_delta(&collection, rev, &records, self.sort_key_for.as_ref(), now);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a live wire record with the given id/rev and a payload carrying `n`
    /// (so payload changes are observable), updatedAt fixed.
    fn live(id: &str, rev: i64, n: i64) -> SyncWireRecord {
        SyncWireRecord {
            id: id.to_string(),
            rev,
            updated_at: 1000.0,
            deleted: false,
            schema_version: 1,
            payload_json: format!("{{\"n\":{n}}}").into_bytes(),
        }
    }

    fn tombstone(id: &str, rev: i64) -> SyncWireRecord {
        SyncWireRecord {
            id: id.to_string(),
            rev,
            updated_at: 1000.0,
            deleted: true,
            schema_version: 1,
            payload_json: b"{}".to_vec(),
        }
    }

    fn snapshot(
        collection: &str,
        rev: i64,
        epoch: i64,
        records: Vec<SyncWireRecord>,
        complete: bool,
    ) -> SyncServerFrame {
        SyncServerFrame::Snapshot {
            collection: collection.to_string(),
            snapshot_rev: rev,
            epoch,
            records,
            complete,
        }
    }

    fn delta(collection: &str, rev: i64, records: Vec<SyncWireRecord>) -> SyncServerFrame {
        SyncServerFrame::Delta {
            collection: collection.to_string(),
            rev,
            records,
        }
    }

    /// An applier whose sort key is the record rev (distinct keys → deterministic
    /// order for assertions).
    fn applier() -> SyncFrameApplier {
        SyncFrameApplier::new(|r| r.rev as f64)
    }

    fn live_ids(a: &SyncFrameApplier, collection: &str) -> Vec<String> {
        a.live_records(collection)
            .into_iter()
            .map(|r| r.record_id)
            .collect()
    }

    #[test]
    fn single_page_snapshot_commits() {
        let mut a = applier();
        let committed = a
            .apply(snapshot("c", 5, 0, vec![live("a", 5, 1), live("b", 5, 2)], true))
            .unwrap();
        assert!(committed);
        assert_eq!(a.cursor("c"), 5);
        assert_eq!(a.epoch("c"), 0);
        // sort_key = rev; both rev 5 → tie broken by id ascending (a before b).
        assert_eq!(live_ids(&a, "c"), vec!["a", "b"]);
    }

    #[test]
    fn incomplete_snapshot_page_buffers_then_commits() {
        let mut a = applier();
        let first = a
            .apply(snapshot("c", 6, 0, vec![live("a", 6, 1)], false))
            .unwrap();
        assert!(!first, "incomplete page commits nothing");
        assert!(a.has_in_flight_snapshot("c"));
        assert!(a.live_records("c").is_empty(), "nothing visible mid-paging");
        assert_eq!(a.cursor("c"), 0);

        let second = a
            .apply(snapshot("c", 6, 0, vec![live("b", 6, 2)], true))
            .unwrap();
        assert!(second);
        assert!(!a.has_in_flight_snapshot("c"));
        assert_eq!(a.cursor("c"), 6);
        assert_eq!(live_ids(&a, "c"), vec!["a", "b"]);
    }

    #[test]
    fn snapshot_then_two_deltas_sequence() {
        let mut a = applier();
        assert!(a
            .apply(snapshot("c", 5, 0, vec![live("a", 5, 1), live("b", 5, 2)], true))
            .unwrap());
        assert_eq!(a.cursor("c"), 5);

        // delta rev 6: add c, update a to rev 6 (payload n=9).
        assert!(a
            .apply(delta("c", 6, vec![live("c", 6, 3), live("a", 6, 9)]))
            .unwrap());
        assert_eq!(a.cursor("c"), 6);

        // delta rev 7: tombstone b.
        assert!(a.apply(delta("c", 7, vec![tombstone("b", 7)])).unwrap());
        assert_eq!(a.cursor("c"), 7);

        // Final: a (updated, rev6), c present; b tombstoned (excluded from live).
        assert_eq!(live_ids(&a, "c"), vec!["a", "c"]);
        assert_eq!(a.stored_record("c", "a").unwrap().rev, 6);
        assert_eq!(a.stored_record("c", "a").unwrap().payload_json, b"{\"n\":9}".to_vec());
        assert!(a.stored_record("c", "b").unwrap().deleted);
    }

    #[test]
    fn epoch_reset_drops_old_records() {
        let mut a = applier();
        // First history: epoch 1, rev 10, records a & b.
        assert!(a
            .apply(snapshot("c", 10, 1, vec![live("a", 10, 1), live("b", 10, 2)], true))
            .unwrap());
        assert_eq!(a.cursor("c"), 10);
        assert_eq!(a.epoch("c"), 1);
        assert_eq!(live_ids(&a, "c"), vec!["a", "b"]);

        // New history: epoch 2 (!= 1) at a LOWER rev 3, only record d. This is a
        // reset: old records dropped, cursor forced DOWN, epoch adopted.
        assert!(a
            .apply(snapshot("c", 3, 2, vec![live("d", 3, 5)], true))
            .unwrap());
        assert_eq!(a.cursor("c"), 3, "cursor forced down to new head");
        assert_eq!(a.epoch("c"), 2);
        assert_eq!(live_ids(&a, "c"), vec!["d"], "old records reconciled away");
        // a & b were tombstoned at max(snapshotRev=3, localRev=10) = 10.
        assert!(a.stored_record("c", "a").unwrap().deleted);
        assert_eq!(a.stored_record("c", "a").unwrap().rev, 10);
        assert!(a.stored_record("c", "b").unwrap().deleted);
    }

    #[test]
    fn stale_rev_delta_returns_true_per_swift_literal_but_store_is_noop() {
        let mut a = applier();
        assert!(a
            .apply(snapshot("c", 5, 0, vec![live("a", 5, 1)], true))
            .unwrap());
        assert_eq!(a.cursor("c"), 5);

        // delta rev 5 (== cursor, not strictly increasing) carrying a duplicate
        // record a rev 5. Swift's applyDeltaFrame reaches store.applyDelta and
        // returns `true` UNCONDITIONALLY (SyncFrameApplier.swift line 223), even
        // though the monotone guard skips the record and the MAX cursor does not
        // advance. The RETURN is true; the STORE is a no-op.
        let committed = a
            .apply(delta("c", 5, vec![live("a", 5, 999)]))
            .unwrap();
        assert!(committed, "a non-paging delta is an unconditional true in Swift");
        // Store unchanged: cursor stays 5, record not overwritten (n stayed 1).
        assert_eq!(a.cursor("c"), 5);
        assert_eq!(a.stored_record("c", "a").unwrap().rev, 5);
        assert_eq!(a.stored_record("c", "a").unwrap().payload_json, b"{\"n\":1}".to_vec());
    }

    #[test]
    fn older_rev_delta_returns_true_per_swift_literal_but_store_is_noop() {
        let mut a = applier();
        assert!(a
            .apply(snapshot("c", 5, 0, vec![live("a", 5, 1)], true))
            .unwrap());
        // delta rev 4 (< cursor) with a stale record: Swift returns `true`
        // unconditionally; the store no-ops (MAX cursor stays 5).
        assert!(a.apply(delta("c", 4, vec![live("a", 3, 7)])).unwrap());
        assert_eq!(a.cursor("c"), 5);
    }

    #[test]
    fn delta_advancing_only_cursor_returns_true() {
        // A delta whose records are all stale but whose rev advances the cursor.
        // Swift returns `true` unconditionally (as for ANY non-paging delta); this
        // also verifies the cursor advances and the stale record is not written.
        let mut a = applier();
        assert!(a
            .apply(snapshot("c", 5, 0, vec![live("a", 5, 1)], true))
            .unwrap());
        let committed = a.apply(delta("c", 6, vec![live("a", 3, 7)])).unwrap();
        assert!(committed);
        assert_eq!(a.cursor("c"), 6);
        // The stale record did not overwrite a.
        assert_eq!(a.stored_record("c", "a").unwrap().rev, 5);
    }

    #[test]
    fn unknown_frame_is_ignored() {
        let mut a = applier();
        assert!(!a.apply(SyncServerFrame::Unknown).unwrap());
        assert!(a.live_records("c").is_empty());
        assert_eq!(a.cursor("c"), 0);
    }

    #[test]
    fn tick_outside_paging_advances_cursor_and_returns_true() {
        let mut a = applier();
        assert!(a
            .apply(snapshot("c", 5, 0, vec![live("a", 5, 1)], true))
            .unwrap());
        let committed = a
            .apply(SyncServerFrame::Tick {
                collection: "c".to_string(),
                rev: 8,
            })
            .unwrap();
        assert!(committed);
        assert_eq!(a.cursor("c"), 8);
    }

    #[test]
    fn stale_tick_still_returns_true_per_swift_literal() {
        // Swift's non-paging tick returns `true` unconditionally (line 134), even
        // when the MAX cursor does not advance. Faithful to the literal.
        let mut a = applier();
        assert!(a
            .apply(snapshot("c", 7, 0, vec![live("a", 7, 1)], true))
            .unwrap());
        let committed = a
            .apply(SyncServerFrame::Tick {
                collection: "c".to_string(),
                rev: 3,
            })
            .unwrap();
        assert!(committed, "tick is an unconditional true in Swift");
        assert_eq!(a.cursor("c"), 7, "MAX cursor does not move backward");
    }

    #[test]
    fn tick_during_paging_is_ignored() {
        let mut a = applier();
        assert!(!a
            .apply(snapshot("c", 6, 0, vec![live("a", 6, 1)], false))
            .unwrap());
        let committed = a
            .apply(SyncServerFrame::Tick {
                collection: "c".to_string(),
                rev: 9,
            })
            .unwrap();
        assert!(!committed, "a tick during paging commits nothing");
        assert_eq!(a.cursor("c"), 0);
    }

    #[test]
    fn delta_during_paging_is_queued_then_drained() {
        let mut a = applier();
        // Start paging (incomplete).
        assert!(!a
            .apply(snapshot("c", 6, 0, vec![live("a", 6, 1)], false))
            .unwrap());
        // A delta arrives mid-paging: queued, not applied yet → false.
        let queued = a.apply(delta("c", 7, vec![tombstone("a", 7)])).unwrap();
        assert!(!queued);
        assert!(a.live_records("c").is_empty());

        // Complete the snapshot: commits the page, then drains the queued delta
        // (which tombstones a). Net live set is empty.
        let committed = a
            .apply(snapshot("c", 6, 0, vec![live("b", 6, 2)], true))
            .unwrap();
        assert!(committed);
        assert_eq!(live_ids(&a, "c"), vec!["b"]);
        assert!(a.stored_record("c", "a").unwrap().deleted, "queued delete applied after commit");
        assert_eq!(a.cursor("c"), 7, "cursor advanced by the drained delta");
    }

    #[test]
    fn unrequested_collection_is_rejected() {
        let mut a = applier().with_allowed_collections(["devices"]);
        // Allowed collection passes.
        assert!(a
            .apply(snapshot("devices", 1, 0, vec![live("a", 1, 1)], true))
            .is_ok());
        // Unrequested collection is malformed.
        let err = a
            .apply(snapshot("builds", 1, 0, vec![live("a", 1, 1)], true))
            .unwrap_err();
        assert_eq!(
            err,
            SyncFrameParseError::Malformed("frame for unrequested collection builds".into())
        );
    }

    #[test]
    fn empty_allowlist_accepts_any_collection() {
        let mut a = applier(); // empty allowlist
        assert!(a
            .apply(snapshot("anything", 1, 0, vec![live("a", 1, 1)], true))
            .is_ok());
    }

    #[test]
    fn snapshot_buffer_cap_overflow_is_malformed_and_clears_build() {
        let mut a = applier().with_caps(2, 10_000, 10_000);
        // 3 records in one incomplete page > cap 2 → malformed.
        let err = a
            .apply(snapshot(
                "c",
                9,
                0,
                vec![live("a", 1, 1), live("b", 2, 2), live("d", 3, 3)],
                false,
            ))
            .unwrap_err();
        assert_eq!(
            err,
            SyncFrameParseError::Malformed(
                "snapshot for c exceeded 2 buffered records before completing".into()
            )
        );
        assert!(!a.has_in_flight_snapshot("c"), "build discarded on overflow");
    }

    #[test]
    fn snapshot_buffer_cap_overflow_accumulates_across_pages() {
        let mut a = applier().with_caps(2, 10_000, 10_000);
        // Page 1: 2 records, incomplete (at the cap, allowed).
        assert!(!a
            .apply(snapshot("c", 9, 0, vec![live("a", 1, 1), live("b", 2, 2)], false))
            .unwrap());
        // Page 2: 1 more record → 3 total > cap 2 → malformed.
        let err = a
            .apply(snapshot("c", 9, 0, vec![live("d", 3, 3)], false))
            .unwrap_err();
        assert_eq!(
            err,
            SyncFrameParseError::Malformed(
                "snapshot for c exceeded 2 buffered records before completing".into()
            )
        );
        assert!(!a.has_in_flight_snapshot("c"));
    }

    #[test]
    fn queued_delta_frame_cap_overflow_is_malformed() {
        let mut a = applier().with_caps(10_000, 10_000, 1);
        assert!(!a
            .apply(snapshot("c", 6, 0, vec![live("a", 6, 1)], false))
            .unwrap());
        // First queued delta is fine (queued_frames 0 + 1 == cap 1).
        assert!(!a.apply(delta("c", 7, vec![])).unwrap());
        // Second queued delta: 1 + 1 > cap 1 → malformed, build discarded.
        let err = a.apply(delta("c", 8, vec![])).unwrap_err();
        assert_eq!(
            err,
            SyncFrameParseError::Malformed(
                "queued deltas for c exceeded the queue bound (frames 1, records 10000) while snapshot never completed".into()
            )
        );
        assert!(!a.has_in_flight_snapshot("c"));
    }

    #[test]
    fn queued_delta_record_cap_overflow_is_malformed() {
        let mut a = applier().with_caps(10_000, 1, 10_000);
        assert!(!a
            .apply(snapshot("c", 6, 0, vec![live("a", 6, 1)], false))
            .unwrap());
        // A single queued delta carrying 2 records: 0 + 2 > cap 1 → malformed.
        let err = a
            .apply(delta("c", 7, vec![live("x", 6, 1), live("y", 6, 2)]))
            .unwrap_err();
        assert_eq!(
            err,
            SyncFrameParseError::Malformed(
                "queued deltas for c exceeded the queue bound (frames 10000, records 1) while snapshot never completed".into()
            )
        );
        assert!(!a.has_in_flight_snapshot("c"));
    }

    #[test]
    fn reset_in_flight_discards_paging_build() {
        let mut a = applier();
        assert!(!a
            .apply(snapshot("c", 6, 0, vec![live("a", 6, 1)], false))
            .unwrap());
        assert!(a.has_in_flight_snapshot("c"));
        a.reset_in_flight();
        assert!(!a.has_in_flight_snapshot("c"));
        // A delta now applies immediately (not queued) since paging was reset.
        let committed = a.apply(delta("c", 7, vec![live("b", 7, 2)])).unwrap();
        assert!(committed);
        assert_eq!(live_ids(&a, "c"), vec!["b"]);
        assert_eq!(a.cursor("c"), 7);
    }

    #[test]
    fn snapshot_page_rev_mismatch_discards_stale_buffer() {
        let mut a = applier();
        // Page for snapshotRev 5 (incomplete) buffers a.
        assert!(!a
            .apply(snapshot("c", 5, 0, vec![live("a", 5, 1)], false))
            .unwrap());
        // A page for a DIFFERENT snapshotRev 6 arrives: the stale buffer (a) is
        // discarded and paging restarts fresh with b.
        assert!(!a
            .apply(snapshot("c", 6, 0, vec![live("b", 6, 2)], false))
            .unwrap());
        // Complete at rev 6: only b (and any rev-6 records) survive; a was dropped.
        assert!(a
            .apply(snapshot("c", 6, 0, vec![live("d", 6, 3)], true))
            .unwrap());
        assert_eq!(live_ids(&a, "c"), vec!["b", "d"]);
        assert!(a.stored_record("c", "a").is_none(), "stale buffered record never committed");
        assert_eq!(a.cursor("c"), 6);
    }

    #[test]
    fn non_reset_snapshot_reconciles_missing_records() {
        let mut a = applier();
        // Snapshot rev 5 with a & b.
        assert!(a
            .apply(snapshot("c", 5, 0, vec![live("a", 5, 1), live("b", 5, 2)], true))
            .unwrap());
        assert_eq!(live_ids(&a, "c"), vec!["a", "b"]);
        // Snapshot rev 6, same epoch 0, cursor 5 < 6 (NOT a reset), with only a
        // (rev 6). b is absent and in [1, 6] → tombstoned at 6.
        assert!(a
            .apply(snapshot("c", 6, 0, vec![live("a", 6, 9)], true))
            .unwrap());
        assert_eq!(live_ids(&a, "c"), vec!["a"], "b reconciled away");
        assert!(a.stored_record("c", "b").unwrap().deleted);
        assert_eq!(a.stored_record("c", "b").unwrap().rev, 6, "tombstone at snapshotRev");
        assert_eq!(a.stored_record("c", "a").unwrap().rev, 6);
        assert_eq!(a.cursor("c"), 6);
    }

    #[test]
    fn cursor_ahead_of_snapshot_head_is_a_reset() {
        // Reset via the cursor-ahead branch (epoch stays 0 on both sides).
        let mut a = applier();
        assert!(a
            .apply(snapshot("c", 10, 0, vec![live("a", 10, 1), live("b", 10, 2)], true))
            .unwrap());
        assert_eq!(a.cursor("c"), 10);
        // A snapshot at a LOWER head 4 with epoch 0: localCursor 10 > 4 → reset.
        assert!(a
            .apply(snapshot("c", 4, 0, vec![live("d", 4, 3)], true))
            .unwrap());
        assert_eq!(a.cursor("c"), 4, "cursor forced down on reset");
        assert_eq!(live_ids(&a, "c"), vec!["d"]);
        assert!(a.stored_record("c", "a").unwrap().deleted);
    }

    #[test]
    fn updated_at_ms_is_divided_to_seconds_on_write() {
        let mut a = applier();
        let record = SyncWireRecord {
            id: "a".to_string(),
            rev: 5,
            updated_at: 5000.0, // epoch ms
            deleted: false,
            schema_version: 1,
            payload_json: b"{}".to_vec(),
        };
        assert!(a.apply(snapshot("c", 5, 0, vec![record], true)).unwrap());
        // Stored as epoch SECONDS: 5000 / 1000 = 5.
        assert_eq!(a.stored_record("c", "a").unwrap().updated_at, 5.0);
    }

    #[test]
    fn live_records_ordered_by_sort_key_desc() {
        // sort_key = rev; higher rev renders first.
        let mut a = applier();
        assert!(a
            .apply(snapshot(
                "c",
                9,
                0,
                vec![live("low", 1, 1), live("high", 9, 2), live("mid", 5, 3)],
                true,
            ))
            .unwrap());
        assert_eq!(live_ids(&a, "c"), vec!["high", "mid", "low"]);
    }
}
