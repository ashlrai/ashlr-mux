//! Canonical-JSON harness for M1 golden-file parity (WS6).
//!
//! The parity contract (see `M1-core-extraction.md` → "Key technical
//! decisions") is:
//!
//! 1. **UUIDs are emitted uppercase** to match `Foundation.UUID.uuidString`,
//!    which Swift's `JSONEncoder`/`JSONSerialization` produce. Lowercase drift
//!    would silently break byte parity, so any UUID-shaped string is folded to
//!    uppercase during canonicalization.
//! 2. **Object keys are sorted** before byte comparison, because Swift's
//!    `JSONSerialization` / `JSONEncoder` emit keys in an *unordered* fashion.
//!    Sorting on both sides makes "byte-identical" well-defined.
//!
//! Golden files are the contract; the parsers are the implementation. We assert
//! byte-identical *canonical* JSON rather than re-deriving equivalent behavior,
//! so subtle Swift→Rust drift is caught on disk.
//!
//! Everything here operates over `serde_json::Value` so it is agnostic to which
//! port produced the value — swapping in Swift-generated fixtures requires no
//! code change, only replacing the bytes under `fixtures/`.

use serde_json::{Map, Value};

/// Canonicalize a [`serde_json::Value`] in place:
///
/// * recursively sort every object's keys (lexicographic, by Unicode scalar
///   order — matching `BTreeMap`/`String` ordering),
/// * uppercase every UUID-shaped string (`8-4-4-4-12` hex with dashes).
///
/// Numbers, booleans, nulls, and non-UUID strings are left untouched. Array
/// order is preserved (arrays are ordered in both Swift and Rust).
pub fn canonicalize(value: &mut Value) {
    match value {
        Value::Object(map) => {
            // serde_json::Map preserves insertion order when the
            // `preserve_order` feature is on, and is a BTreeMap otherwise. To
            // be deterministic regardless of feature flags, rebuild the map in
            // sorted-key order explicitly.
            let mut entries: Vec<(String, Value)> = std::mem::take(map).into_iter().collect();
            entries.sort_by(|(a, _), (b, _)| a.cmp(b));
            let mut sorted = Map::new();
            for (key, mut child) in entries {
                canonicalize(&mut child);
                sorted.insert(key, child);
            }
            *map = sorted;
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                canonicalize(item);
            }
        }
        Value::String(text) => {
            if let Some(upper) = canonical_uuid(text) {
                *text = upper;
            }
        }
        Value::Bool(_) | Value::Number(_) | Value::Null => {}
    }
}

/// Parse `bytes` as JSON, canonicalize, and re-serialize to a stable byte
/// string. This is the function the harness compares against fixture bytes.
///
/// Returns `None` if the input is not valid JSON.
pub fn canonical_json_bytes(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(bytes).ok()?;
    canonicalize(&mut value);
    // serde_json::to_vec on an already-canonicalized value is deterministic:
    // keys were rebuilt in sorted order above, and serde_json emits them in
    // map-iteration order.
    serde_json::to_vec(&value).ok()
}

/// Canonicalize a value to a stable pretty-printed string. Fixtures are stored
/// pretty-printed (one value per file) so diffs are reviewable; byte comparison
/// is performed against this exact rendering.
pub fn canonical_json_string(value: &Value) -> String {
    let mut value = value.clone();
    canonicalize(&mut value);
    serde_json::to_string_pretty(&value).expect("serde_json::Value always serializes")
}

/// If `text` is a UUID in canonical `8-4-4-4-12` hex form (any case), return the
/// uppercase rendering. Otherwise return `None`.
///
/// We do not use the `uuid` crate's parser for the *detection* because we only
/// want to rewrite strings that already look like a UUID, leaving arbitrary
/// hex-ish strings alone. We do validate via the `uuid` crate to guarantee we
/// match Foundation's notion of a UUID exactly.
fn canonical_uuid(text: &str) -> Option<String> {
    // Fast structural reject: Foundation UUID strings are exactly 36 chars with
    // dashes at positions 8, 13, 18, 23.
    if text.len() != 36 {
        return None;
    }
    let bytes = text.as_bytes();
    if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
        return None;
    }
    // Validate as a real UUID (rejects non-hex). `uuid` is permissive on case.
    let parsed = uuid::Uuid::parse_str(text).ok()?;
    let upper = parsed
        .as_hyphenated()
        .encode_upper(&mut uuid::Uuid::encode_buffer())
        .to_owned();
    if upper == text {
        // Already uppercase canonical form: skip the allocation churn upstream.
        None
    } else {
        Some(upper)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sorts_object_keys_recursively() {
        let mut value = json!({
            "b": 1,
            "a": { "z": true, "y": false },
        });
        canonicalize(&mut value);
        let rendered = serde_json::to_string(&value).unwrap();
        assert_eq!(rendered, r#"{"a":{"y":false,"z":true},"b":1}"#);
    }

    #[test]
    fn uppercases_uuid_strings() {
        let mut value = json!({ "id": "3f2504e0-4f89-41d3-9a0c-0305e82c3301" });
        canonicalize(&mut value);
        assert_eq!(
            value["id"],
            json!("3F2504E0-4F89-41D3-9A0C-0305E82C3301")
        );
    }

    #[test]
    fn leaves_already_uppercase_uuid_untouched() {
        let original = "3F2504E0-4F89-41D3-9A0C-0305E82C3301";
        let mut value = json!({ "id": original });
        canonicalize(&mut value);
        assert_eq!(value["id"], json!(original));
    }

    #[test]
    fn leaves_non_uuid_strings_untouched() {
        let mut value = json!({
            "title": "echo hello-world",
            "hexish": "deadbeef",
            "almost": "3f2504e04f8941d39a0c0305e82c3301", // no dashes
        });
        let before = value.clone();
        canonicalize(&mut value);
        assert_eq!(value["title"], before["title"]);
        assert_eq!(value["hexish"], before["hexish"]);
        assert_eq!(value["almost"], before["almost"]);
    }

    #[test]
    fn canonical_bytes_are_stable_regardless_of_input_key_order() {
        let a = canonical_json_bytes(br#"{"b":1,"a":2}"#).unwrap();
        let b = canonical_json_bytes(br#"{"a":2,"b":1}"#).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn canonical_bytes_reject_invalid_json() {
        assert!(canonical_json_bytes(b"not json").is_none());
    }
}
