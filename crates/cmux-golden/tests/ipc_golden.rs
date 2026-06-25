//! Golden-file parity for `cmux-ipc` `ControlRequest` parsing.
//!
//! Pins the strict-parser results — both successful envelopes AND each defect
//! class (`invalidUtf8` / `invalidJson` / `notAnObject` / `missingMethod`) — as
//! canonical JSON. `ControlRequest` and `ControlRequestParseError` have no
//! `Serialize` impl, so this file defines a deterministic JSON projection of the
//! public types; that projection is the canonical form the fixtures pin.
//!
//! Note on `invalidUtf8`: the Rust `ControlRequestParser::request` takes `&str`,
//! so invalid UTF-8 cannot reach it — the UTF-8 boundary is upstream (the byte
//! framing layer that decodes a line before parsing). We therefore project the
//! `InvalidUtf8` *error variant itself* (the same variant the Swift parser
//! returns) so the defect class is represented in the corpus and the
//! `ControlRequestParseError` enum is covered end-to-end.
//!
//! Fixtures are Rust-seeded placeholders; the macOS Swift exporter
//! (`ControlRequestParser` in `Packages/macOS/CmuxControlSocket`) is
//! authoritative.

mod support;

use cmux_ipc::{ControlRequest, ControlRequestParseError, ControlRequestParser, JsonValue};
use serde_json::{json, Value};
use support::assert_canonical_fixture;

const DOMAIN: &str = "ipc";

/// Deterministic JSON projection of `JsonValue` (the `ControlRequest.id` type).
fn json_value_proj(value: &JsonValue) -> Value {
    match value {
        JsonValue::Null => json!({ "kind": "null" }),
        JsonValue::Bool(b) => json!({ "kind": "bool", "value": b }),
        JsonValue::Int(i) => json!({ "kind": "int", "value": i }),
        JsonValue::Double(d) => json!({ "kind": "double", "value": d }),
        JsonValue::String(s) => json!({ "kind": "string", "value": s }),
        JsonValue::Array(items) => json!({
            "kind": "array",
            "value": items.iter().map(json_value_proj).collect::<Vec<_>>(),
        }),
        JsonValue::Object(map) => json!({ "kind": "object", "value": Value::Object(map.clone()) }),
    }
}

fn request_proj(request: &ControlRequest) -> Value {
    json!({
        "outcome": "ok",
        "id": request.id.as_ref().map(json_value_proj),
        "method": request.method,
        "params": Value::Object(request.params.clone()),
    })
}

fn error_proj(error: &ControlRequestParseError) -> Value {
    match error {
        ControlRequestParseError::InvalidUtf8 => json!({ "outcome": "error", "code": "invalidUTF8" }),
        ControlRequestParseError::InvalidJson => json!({ "outcome": "error", "code": "invalidJSON" }),
        ControlRequestParseError::NotAnObject => {
            json!({ "outcome": "error", "code": "notAnObject" })
        }
        ControlRequestParseError::MissingMethod { id } => json!({
            "outcome": "error",
            "code": "missingMethod",
            "id": id.as_ref().map(json_value_proj),
        }),
    }
}

fn parse_proj(line: &str) -> Value {
    match ControlRequestParser.request(line) {
        Ok(request) => request_proj(&request),
        Err(error) => error_proj(&error),
    }
}

#[test]
fn ok_full_envelope() {
    let value = parse_proj(r#"{"id":7,"method":"system.ping","params":{"k":"v"}}"#);
    assert_canonical_fixture(DOMAIN, "ok_full_envelope", &value);
}

#[test]
fn ok_string_id_no_params() {
    let value = parse_proj(r#"{"id":"abc","method":"surface.list"}"#);
    assert_canonical_fixture(DOMAIN, "ok_string_id", &value);
}

#[test]
fn ok_null_id() {
    let value = parse_proj(r#"{"id":null,"method":"noop","params":{}}"#);
    assert_canonical_fixture(DOMAIN, "ok_null_id", &value);
}

#[test]
fn defect_invalid_json() {
    let value = parse_proj("not json");
    assert_canonical_fixture(DOMAIN, "defect_invalid_json", &value);
    // Sanity: confirm the exact variant, not just the projection.
    assert_eq!(
        ControlRequestParser.request("not json").unwrap_err(),
        ControlRequestParseError::InvalidJson
    );
}

#[test]
fn defect_not_an_object() {
    let value = parse_proj("[1,2,3]");
    assert_canonical_fixture(DOMAIN, "defect_not_an_object", &value);
    assert_eq!(
        ControlRequestParser.request("[1,2,3]").unwrap_err(),
        ControlRequestParseError::NotAnObject
    );
}

#[test]
fn defect_missing_method_echoes_id() {
    let value = parse_proj(r#"{"id":3}"#);
    assert_canonical_fixture(DOMAIN, "defect_missing_method_with_id", &value);
    assert_eq!(
        ControlRequestParser.request(r#"{"id":3}"#).unwrap_err(),
        ControlRequestParseError::MissingMethod {
            id: Some(JsonValue::Int(3))
        }
    );
}

#[test]
fn defect_missing_method_empty() {
    let value = parse_proj(r#"{"method":"   "}"#);
    assert_canonical_fixture(DOMAIN, "defect_missing_method_empty", &value);
}

#[test]
fn defect_invalid_utf8_variant() {
    // The Rust strict parser cannot observe invalid UTF-8 (it takes &str); the
    // defect lives at the framing layer. We pin the projection of the variant
    // itself so the defect class is represented and the enum is fully covered.
    let value = error_proj(&ControlRequestParseError::InvalidUtf8);
    assert_canonical_fixture(DOMAIN, "defect_invalid_utf8", &value);
}
