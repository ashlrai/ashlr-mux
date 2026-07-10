//! PART A — the `sync/v1` wire codec.
//!
//! A faithful 1:1 port of `SyncProtocol.swift` (Foundation-only). The codec
//! decodes one server → client WS frame and encodes the `sync.hello` a client
//! sends after connect. Decoding is DEFENSIVE: a frame the client does not
//! understand is surfaced as [`SyncServerFrame::Unknown`] (never an error), so an
//! old client never crashes on a future frame type and a sync frame interleaved
//! with presence frames on a shared socket is cleanly separable. Only a frame
//! that CLAIMS to be sync but is structurally broken yields
//! [`SyncFrameParseError::Malformed`].
//!
//! # Sanctioned platform divergences (Swift → Rust)
//! - `JSONSerialization` → `serde_json`. The decode logic (type switch, numeric
//!   coercion, required-field guards, all marker strings) is reproduced verbatim.
//! - `Data` (opaque payload bytes) → `Vec<u8>`. Swift stores the payload as
//!   `JSONSerialization.data(withJSONObject:)`; this port re-serializes the
//!   parsed value with `serde_json::to_vec`. The SEMANTIC JSON value is identical;
//!   the exact byte formatting (key ordering / whitespace) differs because the two
//!   serializers differ — an inherent, harmless serializer divergence.
//! - Swift's `Foundation.Date` epoch-ms on the wire is represented as `f64`.
//! - `encodeHello` is declared `throws` in Swift (JSONSerialization can throw);
//!   here `serde_json` serialization of the fixed hello shape is infallible, so
//!   [`SyncFrameCodec::encode_hello`] returns `Vec<u8>` directly.

use serde_json::{Map, Value};

/// Current sync record schema version. Must match `SYNC_SCHEMA_VERSION` in the
/// worker. (Swift: `syncSchemaVersion`.)
pub const SYNC_SCHEMA_VERSION: i64 = 1;

/// The sync protocol identifier sent in `sync.hello`. (Swift: `syncProtocolV1`.)
pub const SYNC_PROTOCOL_V1: &str = "sync/v1";

/// One synced record as it appears on the wire and is stored locally. The
/// `payload_json` is kept as raw JSON bytes so the transport/store never decode
/// it; the typed facade decodes on read. Mirrors Swift `SyncWireRecord`.
#[derive(Debug, Clone, PartialEq)]
pub struct SyncWireRecord {
    pub id: String,
    pub rev: i64,
    /// Epoch ms the DO last wrote this record (tiebreak/debug only; `rev` orders).
    pub updated_at: f64,
    pub deleted: bool,
    pub schema_version: i64,
    /// Opaque collection-typed JSON body, `{}` for tombstones. Stored verbatim.
    pub payload_json: Vec<u8>,
}

/// A server → client sync frame. [`SyncServerFrame::Unknown`] covers any non-sync
/// frame on the shared socket (presence frames) and any future sync frame type,
/// so the dispatcher can ignore it without error. Mirrors Swift `SyncServerFrame`.
#[derive(Debug, Clone, PartialEq)]
pub enum SyncServerFrame {
    /// Full state of a collection as of `snapshot_rev`, in history generation
    /// `epoch`. Paged: commit only on the `complete` page.
    Snapshot {
        collection: String,
        snapshot_rev: i64,
        epoch: i64,
        records: Vec<SyncWireRecord>,
        complete: bool,
    },
    /// Incremental change(s); `rev` is the head this frame advances the cursor to
    /// once fully applied.
    Delta {
        collection: String,
        rev: i64,
        records: Vec<SyncWireRecord>,
    },
    /// Liveness + cursor tick when nothing record-shaped changed.
    Tick { collection: String, rev: i64 },
    /// Not a sync frame this client handles (a presence frame, or a future type).
    Unknown,
}

/// Mirrors Swift `SyncFrameParseError`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SyncFrameParseError {
    /// The bytes were not a JSON object (Swift `.notJSON`).
    #[error("frame is not a JSON object")]
    NotJson,
    /// A frame that claimed to be sync but was structurally broken. The string is
    /// the verbatim Swift diagnostic (Swift `.malformed(String)`).
    #[error("{0}")]
    Malformed(String),
}

/// One `(name, cursor, epoch)` collection subscription for `encode_hello`. A
/// named struct (not a bare tuple) so callers cannot silently transpose the two
/// numeric fields. Mirrors Swift's `(name:, cursor:, epoch:)` tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelloCollection {
    pub name: String,
    pub cursor: i64,
    pub epoch: i64,
}

/// Encodes/decodes sync/v1 wire frames. Mirrors Swift `SyncFrameCodec` (an
/// instantiable value, not a static namespace).
#[derive(Debug, Clone, Copy, Default)]
pub struct SyncFrameCodec;

impl SyncFrameCodec {
    pub fn new() -> Self {
        Self
    }

    /// Parse one WS text/data frame. Returns [`SyncServerFrame::Unknown`] for
    /// non-sync frames (so the caller routes presence frames elsewhere) and only
    /// errors on a frame that claims to be sync but is structurally broken.
    ///
    /// Faithful to Swift `parse`: `try? JSONSerialization.jsonObject(...) as?
    /// [String: Any]` fails (→ `.notJSON`) for both non-JSON bytes AND valid JSON
    /// whose top level is not an object (a JSON array/fragment). `serde_json`
    /// parses a fragment as a `Value`, so this reproduces the same outcome by
    /// rejecting any non-object top level as `NotJson`.
    pub fn parse(&self, data: &[u8]) -> Result<SyncServerFrame, SyncFrameParseError> {
        let value: Value = match serde_json::from_slice(data) {
            Ok(v) => v,
            Err(_) => return Err(SyncFrameParseError::NotJson),
        };
        let obj = match value {
            Value::Object(obj) => obj,
            _ => return Err(SyncFrameParseError::NotJson),
        };

        // Missing `type`, or a `type` that is not a string, is a presence/unknown
        // frame — NOT an error (Swift: `else { return .unknown }`).
        let type_str = match obj.get("type").and_then(Value::as_str) {
            Some(s) => s,
            None => return Ok(SyncServerFrame::Unknown),
        };

        match type_str {
            "sync.snapshot" => {
                let (collection, snapshot_rev) = require_collection_and_rev(
                    &obj,
                    "snapshotRev",
                    "sync.snapshot missing collection/snapshotRev",
                )?;
                let complete = matches!(obj.get("complete"), Some(Value::Bool(true)));
                let epoch = int_value(obj.get("epoch")).unwrap_or(0);
                let records = require_records(obj.get("records"), "sync.snapshot", snapshot_rev)?;
                Ok(SyncServerFrame::Snapshot {
                    collection,
                    snapshot_rev,
                    epoch,
                    records,
                    complete,
                })
            }
            "sync.delta" => {
                let (collection, rev) =
                    require_collection_and_rev(&obj, "rev", "sync.delta missing collection/rev")?;
                let records = require_records(obj.get("records"), "sync.delta", rev)?;
                Ok(SyncServerFrame::Delta {
                    collection,
                    rev,
                    records,
                })
            }
            "sync.tick" => {
                let (collection, rev) =
                    require_collection_and_rev(&obj, "rev", "sync.tick missing collection/rev")?;
                Ok(SyncServerFrame::Tick { collection, rev })
            }
            // A presence frame (snapshot/online/offline/seen/routes) or a future
            // sync frame: not ours to apply.
            _ => Ok(SyncServerFrame::Unknown),
        }
    }

    /// Encode the `sync.hello` a client sends after connect to subscribe to
    /// collections with the cursors and epochs it already holds. Mirrors Swift
    /// `encodeHello`.
    pub fn encode_hello(&self, collections: &[HelloCollection]) -> Vec<u8> {
        let cols: Vec<Value> = collections
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "cursor": c.cursor,
                    "epoch": c.epoch,
                })
            })
            .collect();
        let payload = serde_json::json!({
            "type": "sync.hello",
            "protocol": SYNC_PROTOCOL_V1,
            "collections": cols,
        });
        // The hello shape is a fixed object/array/string/number tree; serde_json
        // serialization of it cannot fail, so Swift's `throws` is unreachable.
        serde_json::to_vec(&payload).expect("sync.hello serialization is infallible")
    }
}

/// Records for a delta/snapshot frame. The field is REQUIRED and must be an array
/// of objects: a frame that claims to be sync but whose `records` is missing or
/// the wrong type is structurally broken and errors, so the client resyncs rather
/// than committing an empty frame that would silently advance the cursor.
///
/// Faithful to Swift `requireRecords`: Swift's `value as? [[String: Any]]` cast is
/// ALL-OR-NOTHING — if the value is not an array, OR any element is not an object,
/// the whole cast fails with the same `"<frame> missing or non-array records"`
/// message (checked BEFORE any per-record parse). This is reproduced with a first
/// pass that verifies every element is an object before parsing any record.
///
/// `max_rev` is the frame head (`rev` for a delta, `snapshotRev` for a snapshot).
/// A record with `record.rev > max_rev` is a malformed/forged frame and is
/// rejected to avoid durable local-cache poisoning by a poison-high rev.
/// Extract the required `collection` string plus the frame's required int head
/// field (`snapshotRev` / `rev`), or fail with that frame's verbatim Swift
/// malformed diagnostic. Shared by the three sync frame arms of
/// [`SyncFrameCodec::parse`], which repeat this guard verbatim in Swift.
fn require_collection_and_rev(
    obj: &Map<String, Value>,
    int_key: &str,
    error: &str,
) -> Result<(String, i64), SyncFrameParseError> {
    match (
        obj.get("collection").and_then(Value::as_str),
        int_value(obj.get(int_key)),
    ) {
        (Some(collection), Some(rev)) => Ok((collection.to_string(), rev)),
        _ => Err(SyncFrameParseError::Malformed(error.to_string())),
    }
}

fn require_records(
    value: Option<&Value>,
    frame: &str,
    max_rev: i64,
) -> Result<Vec<SyncWireRecord>, SyncFrameParseError> {
    let array = match value {
        Some(Value::Array(array)) => array,
        _ => {
            return Err(SyncFrameParseError::Malformed(format!(
                "{frame} missing or non-array records"
            )))
        }
    };
    // First pass: the Swift `[[String: Any]]` cast requires EVERY element to be an
    // object, and fails wholesale (with the missing/non-array message) otherwise —
    // before any record-level error can be raised.
    let mut objects: Vec<&Map<String, Value>> = Vec::with_capacity(array.len());
    for item in array {
        match item {
            Value::Object(map) => objects.push(map),
            _ => {
                return Err(SyncFrameParseError::Malformed(format!(
                    "{frame} missing or non-array records"
                )))
            }
        }
    }
    // Second pass: parse each record in order, enforcing the frame-head guard.
    let mut out = Vec::with_capacity(objects.len());
    for map in objects {
        let record = parse_record(map)?;
        if record.rev > max_rev {
            return Err(SyncFrameParseError::Malformed(format!(
                "{frame} record {} rev {} exceeds frame head {}",
                record.id, record.rev, max_rev
            )));
        }
        out.push(record);
    }
    Ok(out)
}

/// Parse one wire record object. Mirrors Swift `parseRecord`.
fn parse_record(obj: &Map<String, Value>) -> Result<SyncWireRecord, SyncFrameParseError> {
    let id = match obj.get("id").and_then(Value::as_str) {
        Some(id) => id.to_string(),
        None => {
            return Err(SyncFrameParseError::Malformed(
                "record missing id/rev".to_string(),
            ))
        }
    };
    let rev = match int_value(obj.get("rev")) {
        Some(rev) => rev,
        None => {
            return Err(SyncFrameParseError::Malformed(
                "record missing id/rev".to_string(),
            ))
        }
    };
    let updated_at = double_value(obj.get("updatedAt")).unwrap_or(0.0);
    let deleted = matches!(obj.get("deleted"), Some(Value::Bool(true)));
    let schema_version = int_value(obj.get("schemaVersion")).unwrap_or(SYNC_SCHEMA_VERSION);
    let payload_json: Vec<u8> = if deleted {
        // A tombstone legitimately carries `{}`; its payload is never read.
        b"{}".to_vec()
    } else {
        // A LIVE record must carry a SERIALIZABLE payload. Faithful to Swift's
        // `JSONSerialization.data(withJSONObject:)` (no `.fragmentsAllowed`): the
        // payload must be a JSON object or array. A missing payload, or a payload
        // that is a bare fragment (string/number/bool/null), is unserializable and
        // rejected — do NOT silently store `{}` (which the facade cannot decode,
        // hiding the row while the cursor advances past it).
        match obj.get("payload") {
            Some(payload @ (Value::Object(_) | Value::Array(_))) => serde_json::to_vec(payload)
                .map_err(|_| {
                    SyncFrameParseError::Malformed(format!(
                        "live record {id} missing/unserializable payload"
                    ))
                })?,
            _ => {
                return Err(SyncFrameParseError::Malformed(format!(
                    "live record {id} missing/unserializable payload"
                )))
            }
        }
    };
    Ok(SyncWireRecord {
        id,
        rev,
        updated_at,
        deleted,
        schema_version,
        payload_json,
    })
}

/// Parse a NON-NEGATIVE integer from a JSON value. `rev`/`snapshotRev`/`cursor`/
/// `epoch` are all non-negative, so a negative or non-integral value is malformed.
/// A JSON boolean is rejected (so `rev: true` does not parse as 1). Returns `None`
/// for anything invalid so the caller surfaces `.malformed`. Mirrors Swift
/// `intValue`.
fn int_value(value: Option<&Value>) -> Option<i64> {
    let value = value?;
    // Reject JSON booleans (Swift rejects a CFBoolean-backed NSNumber up front).
    if let Value::Bool(_) = value {
        return None;
    }
    let number = match value {
        Value::Number(n) => n,
        _ => return None,
    };
    // Prefer the exact integer; fall back to the bounds-checked double conversion
    // so a huge value (e.g. 1e100 or a u64 above i64::MAX) yields None instead of
    // trapping on an out-of-range conversion.
    let parsed = if let Some(i) = number.as_i64() {
        Some(i)
    } else {
        number.as_f64().and_then(int_from_double)
    };
    match parsed {
        Some(result) if result >= 0 => Some(result),
        _ => None,
    }
}

/// Convert a JSON double to i64 only when it is finite, integral, and within i64
/// range; otherwise `None`. Mirrors Swift `intFromDouble`, including the exact
/// power-of-two bounds: compare against `2^63` (exactly representable, `> i64::MAX`)
/// with a STRICT `<`, and against `-2^63` (`== i64::MIN`) with `>=`.
fn int_from_double(d: f64) -> Option<i64> {
    let two_to_63 = 9_223_372_036_854_775_808.0_f64; // 2^63, exact in f64; > i64::MAX
    if d.is_finite() && d == d.trunc() && d >= -two_to_63 && d < two_to_63 {
        Some(d as i64)
    } else {
        None
    }
}

/// Parse a double from a JSON value. Mirrors Swift `doubleValue` (accepts a JSON
/// number — integer or float). Unlike `int_value`, Swift's `doubleValue` has NO
/// CFBoolean guard, so a JSON boolean bridges to a CFBoolean-backed `NSNumber`
/// whose `doubleValue` is `1.0` (`true`) / `0.0` (`false`) and coerces through —
/// e.g. `updatedAt: true` → 1.0. Any other non-number yields `None`.
fn double_value(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(n) => n.as_f64(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codec() -> SyncFrameCodec {
        SyncFrameCodec::new()
    }

    #[test]
    fn constants_match_worker() {
        assert_eq!(SYNC_SCHEMA_VERSION, 1);
        assert_eq!(SYNC_PROTOCOL_V1, "sync/v1");
    }

    #[test]
    fn parse_snapshot_frame() {
        let bytes = br#"{
            "type": "sync.snapshot",
            "collection": "devices",
            "snapshotRev": 7,
            "epoch": 2,
            "complete": true,
            "records": [
                {"id": "a", "rev": 5, "updatedAt": 1000, "payload": {"name": "A"}},
                {"id": "b", "rev": 7, "deleted": true}
            ]
        }"#;
        let frame = codec().parse(bytes).unwrap();
        match frame {
            SyncServerFrame::Snapshot {
                collection,
                snapshot_rev,
                epoch,
                records,
                complete,
            } => {
                assert_eq!(collection, "devices");
                assert_eq!(snapshot_rev, 7);
                assert_eq!(epoch, 2);
                assert!(complete);
                assert_eq!(records.len(), 2);
                assert_eq!(records[0].id, "a");
                assert_eq!(records[0].rev, 5);
                assert_eq!(records[0].updated_at, 1000.0);
                assert!(!records[0].deleted);
                assert_eq!(records[0].schema_version, SYNC_SCHEMA_VERSION);
                assert_eq!(records[0].payload_json, br#"{"name":"A"}"#.to_vec());
                assert_eq!(records[1].id, "b");
                assert!(records[1].deleted);
                assert_eq!(records[1].payload_json, b"{}".to_vec());
            }
            other => panic!("expected snapshot, got {other:?}"),
        }
    }

    #[test]
    fn parse_delta_frame() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":9,"records":[]}"#;
        let frame = codec().parse(bytes).unwrap();
        assert_eq!(
            frame,
            SyncServerFrame::Delta {
                collection: "devices".to_string(),
                rev: 9,
                records: vec![],
            }
        );
    }

    #[test]
    fn parse_tick_frame() {
        let bytes = br#"{"type":"sync.tick","collection":"devices","rev":11}"#;
        let frame = codec().parse(bytes).unwrap();
        assert_eq!(
            frame,
            SyncServerFrame::Tick {
                collection: "devices".to_string(),
                rev: 11,
            }
        );
    }

    #[test]
    fn unknown_type_is_unknown_not_error() {
        let bytes = br#"{"type":"presence.snapshot","foo":1}"#;
        assert_eq!(codec().parse(bytes).unwrap(), SyncServerFrame::Unknown);
    }

    #[test]
    fn missing_type_is_unknown() {
        let bytes = br#"{"collection":"devices","rev":1}"#;
        assert_eq!(codec().parse(bytes).unwrap(), SyncServerFrame::Unknown);
    }

    #[test]
    fn non_string_type_is_unknown() {
        let bytes = br#"{"type":5}"#;
        assert_eq!(codec().parse(bytes).unwrap(), SyncServerFrame::Unknown);
    }

    #[test]
    fn non_json_is_not_json_error() {
        assert_eq!(
            codec().parse(b"not json at all {").unwrap_err(),
            SyncFrameParseError::NotJson
        );
    }

    #[test]
    fn top_level_json_array_is_not_json() {
        // Valid JSON, but not a top-level object: Swift's `as? [String: Any]` fails.
        assert_eq!(
            codec().parse(b"[]").unwrap_err(),
            SyncFrameParseError::NotJson
        );
    }

    #[test]
    fn top_level_json_fragment_is_not_json() {
        // JSONSerialization (no .fragmentsAllowed) rejects a bare number; the port
        // rejects any non-object top level as NotJson.
        assert_eq!(
            codec().parse(b"42").unwrap_err(),
            SyncFrameParseError::NotJson
        );
    }

    #[test]
    fn snapshot_missing_collection_is_malformed() {
        let bytes = br#"{"type":"sync.snapshot","snapshotRev":3,"records":[]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.snapshot missing collection/snapshotRev".into())
        );
    }

    #[test]
    fn snapshot_missing_rev_is_malformed() {
        let bytes = br#"{"type":"sync.snapshot","collection":"devices","records":[]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.snapshot missing collection/snapshotRev".into())
        );
    }

    #[test]
    fn delta_missing_fields_is_malformed() {
        let bytes = br#"{"type":"sync.delta","records":[]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.delta missing collection/rev".into())
        );
    }

    #[test]
    fn tick_missing_fields_is_malformed() {
        let bytes = br#"{"type":"sync.tick","collection":"devices"}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.tick missing collection/rev".into())
        );
    }

    #[test]
    fn records_field_missing_is_malformed() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.delta missing or non-array records".into())
        );
    }

    #[test]
    fn records_field_non_array_is_malformed() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,"records":{}}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.delta missing or non-array records".into())
        );
    }

    #[test]
    fn records_array_with_non_object_element_is_malformed_wholesale() {
        // Swift's `[[String: Any]]` cast fails wholesale before any per-record
        // parse: even though element 0 would parse, element 1 (a string) makes the
        // whole cast fail with the missing/non-array message.
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":1,"payload":{}}, "oops"]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.delta missing or non-array records".into())
        );
    }

    #[test]
    fn record_rev_exceeding_frame_head_is_malformed() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":9,"payload":{}}]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.delta record a rev 9 exceeds frame head 3".into())
        );
    }

    #[test]
    fn record_rev_equal_to_frame_head_is_allowed() {
        // The guard is `record.rev > maxRev`; rev == head is fine.
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":3,"payload":{}}]}"#;
        let frame = codec().parse(bytes).unwrap();
        match frame {
            SyncServerFrame::Delta { records, .. } => assert_eq!(records[0].rev, 3),
            other => panic!("expected delta, got {other:?}"),
        }
    }

    #[test]
    fn record_missing_id_is_malformed() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"rev":1,"payload":{}}]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("record missing id/rev".into())
        );
    }

    #[test]
    fn record_missing_rev_is_malformed() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","payload":{}}]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("record missing id/rev".into())
        );
    }

    #[test]
    fn live_record_missing_payload_is_malformed() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":1}]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("live record a missing/unserializable payload".into())
        );
    }

    #[test]
    fn live_record_fragment_payload_is_malformed() {
        // A bare-string payload is not serializable via JSONSerialization without
        // .fragmentsAllowed → unserializable.
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":1,"payload":"just a string"}]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("live record a missing/unserializable payload".into())
        );
    }

    #[test]
    fn deleted_record_needs_no_payload() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":2,"deleted":true}]}"#;
        let frame = codec().parse(bytes).unwrap();
        match frame {
            SyncServerFrame::Delta { records, .. } => {
                assert!(records[0].deleted);
                assert_eq!(records[0].payload_json, b"{}".to_vec());
            }
            other => panic!("expected delta, got {other:?}"),
        }
    }

    #[test]
    fn record_field_defaults() {
        // updatedAt defaults to 0, deleted to false, schemaVersion to the constant.
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":1,"payload":{}}]}"#;
        let frame = codec().parse(bytes).unwrap();
        match frame {
            SyncServerFrame::Delta { records, .. } => {
                assert_eq!(records[0].updated_at, 0.0);
                assert!(!records[0].deleted);
                assert_eq!(records[0].schema_version, SYNC_SCHEMA_VERSION);
            }
            other => panic!("expected delta, got {other:?}"),
        }
    }

    #[test]
    fn int_value_accepts_float_with_integral_value() {
        // rev: 5.0 coerces to 5 (Swift routes NSNumber through the double path).
        let bytes = br#"{"type":"sync.tick","collection":"devices","rev":5.0}"#;
        assert_eq!(
            codec().parse(bytes).unwrap(),
            SyncServerFrame::Tick {
                collection: "devices".to_string(),
                rev: 5,
            }
        );
    }

    #[test]
    fn int_value_rejects_boolean_rev() {
        // rev: true must NOT parse as 1 → record missing id/rev.
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":true,"payload":{}}]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("record missing id/rev".into())
        );
    }

    #[test]
    fn int_value_rejects_negative() {
        // A negative snapshotRev is not a valid non-negative int → malformed.
        let bytes =
            br#"{"type":"sync.snapshot","collection":"devices","snapshotRev":-1,"records":[]}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.snapshot missing collection/snapshotRev".into())
        );
    }

    #[test]
    fn int_value_rejects_huge_out_of_range_float() {
        // 1e100 is finite but far above 2^63 → not an int → malformed.
        let bytes = br#"{"type":"sync.tick","collection":"devices","rev":1e100}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.tick missing collection/rev".into())
        );
    }

    #[test]
    fn int_value_rejects_non_integral_float() {
        let bytes = br#"{"type":"sync.tick","collection":"devices","rev":5.5}"#;
        assert_eq!(
            codec().parse(bytes).unwrap_err(),
            SyncFrameParseError::Malformed("sync.tick missing collection/rev".into())
        );
    }

    #[test]
    fn updated_at_accepts_integer() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":1,"updatedAt":1700000000000,"payload":{}}]}"#;
        let frame = codec().parse(bytes).unwrap();
        match frame {
            SyncServerFrame::Delta { records, .. } => {
                assert_eq!(records[0].updated_at, 1_700_000_000_000.0);
            }
            other => panic!("expected delta, got {other:?}"),
        }
    }

    #[test]
    fn updated_at_boolean_coerces_like_swift_doublevalue() {
        // Swift's doubleValue has NO CFBoolean guard (unlike intValue): a JSON
        // boolean bridges to a CFBoolean-backed NSNumber whose doubleValue is 1.0
        // (true) / 0.0 (false). So updatedAt: true parses as 1.0 at the wire level
        // (the applier would later store 1.0 / 1000 = 0.001s).
        let true_bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":1,"updatedAt":true,"payload":{}}]}"#;
        match codec().parse(true_bytes).unwrap() {
            SyncServerFrame::Delta { records, .. } => assert_eq!(records[0].updated_at, 1.0),
            other => panic!("expected delta, got {other:?}"),
        }
        // updatedAt: false parses as 0.0.
        let false_bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":1,"updatedAt":false,"payload":{}}]}"#;
        match codec().parse(false_bytes).unwrap() {
            SyncServerFrame::Delta { records, .. } => assert_eq!(records[0].updated_at, 0.0),
            other => panic!("expected delta, got {other:?}"),
        }
    }

    #[test]
    fn payload_is_re_serialized_from_value() {
        let bytes = br#"{"type":"sync.delta","collection":"devices","rev":3,
            "records":[{"id":"a","rev":1,"payload":{"z":1,"a":[true,null]}}]}"#;
        let frame = codec().parse(bytes).unwrap();
        match frame {
            SyncServerFrame::Delta { records, .. } => {
                // serde_json (BTreeMap Map) re-serializes with sorted keys.
                assert_eq!(
                    records[0].payload_json,
                    br#"{"a":[true,null],"z":1}"#.to_vec()
                );
            }
            other => panic!("expected delta, got {other:?}"),
        }
    }

    #[test]
    fn encode_hello_round_trips() {
        let bytes = codec().encode_hello(&[
            HelloCollection {
                name: "devices".to_string(),
                cursor: 12,
                epoch: 3,
            },
            HelloCollection {
                name: "builds".to_string(),
                cursor: 0,
                epoch: 0,
            },
        ]);
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["type"], "sync.hello");
        assert_eq!(value["protocol"], "sync/v1");
        let cols = value["collections"].as_array().unwrap();
        assert_eq!(cols.len(), 2);
        assert_eq!(cols[0]["name"], "devices");
        assert_eq!(cols[0]["cursor"], 12);
        assert_eq!(cols[0]["epoch"], 3);
        assert_eq!(cols[1]["name"], "builds");
        assert_eq!(cols[1]["cursor"], 0);
        assert_eq!(cols[1]["epoch"], 0);
    }

    #[test]
    fn encode_hello_empty_collections() {
        let bytes = codec().encode_hello(&[]);
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["type"], "sync.hello");
        assert_eq!(value["protocol"], "sync/v1");
        assert!(value["collections"].as_array().unwrap().is_empty());
    }
}
