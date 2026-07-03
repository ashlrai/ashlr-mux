//! `cmux-sync-protocol` — a faithful 1:1 Rust port of the macOS Swift `sync/v1`
//! layer: the wire codec plus the client-side frame-application state machine.
//!
//! # Swift source map
//! - [`codec`] (**PART A**) ports
//!   `Packages/Shared/CmuxSyncStore/Sources/CmuxSyncStore/SyncProtocol.swift`
//!   (the `SyncFrameCodec` / `SyncWireRecord` / `SyncServerFrame` /
//!   `SyncFrameParseError` wire types, verbatim).
//! - [`applier`] (**PART B**) ports
//!   `Packages/Shared/CmuxSyncStore/Sources/CmuxSyncStore/SyncFrameApplier.swift`
//!   (the pure snapshot-paging / concurrent-delta fold) plus the observable
//!   store semantics of `CmuxSyncStore.swift`'s `applyDelta` / `applySnapshot` /
//!   `cursor` / `epoch` (as an in-memory record store).
//!
//! Behavior is preserved EXACTLY; the only sanctioned divergences are the
//! platform swaps, each documented in the module it applies to:
//! - **`JSONSerialization` → `serde_json`** and Swift `Data` opaque payloads →
//!   `Vec<u8>` re-serialized from the parsed value (identical JSON semantics;
//!   byte formatting may differ). See [`codec`].
//! - **Swift `actor` / `async` → a synchronous `&mut self` struct**, and the
//!   injected raw-SQLite `any CmuxSyncStoring` → an in-memory store reproducing
//!   its observable semantics; `teamID` dropped (one applier serves one team);
//!   `throws` → `Result<_, SyncFrameParseError>`. See [`applier`].

mod applier;
mod codec;

pub use codec::{
    HelloCollection, SyncFrameCodec, SyncFrameParseError, SyncServerFrame, SyncWireRecord,
    SYNC_PROTOCOL_V1, SYNC_SCHEMA_VERSION,
};

pub use applier::{
    StoredSyncRecord, SyncFrameApplier, DEFAULT_MAX_BUFFERED_RECORDS,
    DEFAULT_MAX_QUEUED_DELTA_FRAMES, DEFAULT_MAX_QUEUED_DELTA_RECORDS,
};
