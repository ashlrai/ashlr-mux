//! Opaque JSON value helpers.
//!
//! Swift parity source:
//! `Packages/macOS/CMUXAgentLaunch/Sources/CMUXAgentLaunch/Workstream/WorkstreamEvent.swift:155-283`
//! (the private `indirect enum AnyJSON`).
//!
//! `AnyJSON` in Swift is a recursive AST of opaque JSON values used to pass
//! arbitrary `tool_input` shapes through `Codable` without rebuilding raw bytes.
//! In Rust the exact same role is already filled by [`serde_json::Value`]: it
//! carries the `null` / `bool` / integer-vs-float number / `string` / `array` /
//! `object` distinction that `AnyJSON` encodes, and (because this crate does NOT
//! enable serde_json's `preserve_order` feature) its `Map` is a `BTreeMap`, so
//! serialization emits object keys sorted — matching `AnyJSON.asJSONString`,
//! whose object branch sorts its `"key":value` pieces before joining.
//!
//! So instead of a bespoke enum, this module provides the two free functions
//! that mirror `AnyJSON`'s only two externally-observable operations:
//! `asJSONString` (a value → its stable textual form) and
//! `init?(jsonString:)` (a string → an optional value).
//!
//! SANCTIONED DIVERGENCE (byte-level, round-trip-preserving): Swift's
//! `asJSONString` escapes strings via `JSONSerialization`, which on Apple
//! platforms escapes the forward slash `/` as `\/`. `serde_json` does not escape
//! `/`. Both forms parse back to the identical string (`\/` and `/` are the same
//! JSON character), so every value round-trips identically; only the raw bytes of
//! strings that contain `/` differ. The upstream Swift tests deliberately parse
//! `toolInputJSON` back through a JSON reader rather than substring-matching for
//! exactly this reason (see `WorkstreamEventTests.decodesHookPayload`).

use serde_json::Value;

/// Mirror of `AnyJSON.asJSONString` (`WorkstreamEvent.swift:169-184`): the stable
/// textual form — valid, compact JSON with object keys sorted so persisted
/// output is deterministic. See the module-level divergence note about `/`.
pub fn json_to_string(value: &Value) -> String {
    // `serde_json::to_string` on a `Value` is infallible in practice (a `Value`
    // is always serializable); the fallback keeps this total like the Swift API.
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

/// Mirror of `AnyJSON.init?(jsonString:)` (`WorkstreamEvent.swift:186-193`):
/// parse a JSON string (fragments — scalars, not just objects/arrays — allowed)
/// into a value, or `None` when the text is not valid JSON.
///
/// `serde_json::from_str::<Value>` accepts top-level scalars just like Swift's
/// `JSONSerialization` with `.fragmentsAllowed`, and rejects non-JSON text
/// (e.g. `"plain text"`) with `None`, matching the Swift `guard … else { nil }`.
pub fn json_from_string(text: &str) -> Option<Value> {
    serde_json::from_str::<Value>(text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn object_keys_sort_recursively() {
        // Mirrors AnyJSON.asJSONString sorting object keys (nested too).
        let value = json!({"b": 1, "a": {"d": 2, "c": 3}});
        assert_eq!(json_to_string(&value), r#"{"a":{"c":3,"d":2},"b":1}"#);
    }

    #[test]
    fn scalars_round_trip() {
        assert_eq!(json_to_string(&json!(null)), "null");
        assert_eq!(json_to_string(&json!(true)), "true");
        assert_eq!(json_to_string(&json!(2)), "2");
        assert_eq!(json_to_string(&json!(2.5)), "2.5");
        assert_eq!(json_to_string(&json!("x")), "\"x\"");
    }

    #[test]
    fn fragments_parse_and_invalid_is_none() {
        assert_eq!(json_from_string("null"), Some(json!(null)));
        assert_eq!(json_from_string("42"), Some(json!(42)));
        assert_eq!(json_from_string("\"s\""), Some(json!("s")));
        assert_eq!(json_from_string("{\"a\":1}"), Some(json!({"a": 1})));
        assert_eq!(json_from_string("plain text"), None);
    }

    #[test]
    fn integer_and_float_are_distinguished() {
        // AnyJSON keeps `.int` vs `.double`; serde_json::Value does too.
        assert_eq!(json_to_string(&json_from_string("2").unwrap()), "2");
        assert_eq!(json_to_string(&json_from_string("2.0").unwrap()), "2.0");
    }
}
