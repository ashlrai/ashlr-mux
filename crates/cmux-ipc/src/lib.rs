//! Socket v2 RPC codec for the cmux control channel.
//!
//! Owns the newline-delimited JSON-RPC wire contract shared verbatim with the
//! macOS app: the `{ "id", "method", "params" }` request envelope, its lenient
//! and strict parsers, the response encoder, and the NDJSON framing helpers. The
//! transport (AF_UNIX on macOS, named pipes on Windows) sits beneath this codec
//! unchanged.
//!
//! Swift parity sources:
//! - `Packages/macOS/CmuxControlSocket/Sources/CmuxControlSocket/Wire/ControlRequest.swift`
//! - `Packages/macOS/CmuxControlSocket/Sources/CmuxControlSocket/Wire/ControlRequestParser.swift`
//! - `Packages/macOS/CmuxControlSocket/Sources/CmuxControlSocket/Wire/ControlResponseEncoder.swift`

mod control_call_result;
mod control_request;
mod control_request_parse_error;
mod control_request_parser;
mod control_response_encoder;
mod framing;
mod json_value;

pub use control_call_result::ControlCallResult;
pub use control_request::ControlRequest;
pub use control_request_parse_error::ControlRequestParseError;
pub use control_request_parser::ControlRequestParser;
pub use control_response_encoder::ControlResponseEncoder;
pub use framing::{append_line, split_lines};
pub use json_value::JsonValue;

#[cfg(test)]
mod tests {
    use super::*;

    fn strict_error(line: &str) -> Option<ControlRequestParseError> {
        ControlRequestParser.request(line).err()
    }

    #[test]
    fn lenient_parses_full_envelope() {
        let request = ControlRequestParser
            .lenient_request(r#"  {"id":7,"method":" system.ping ","params":{"k":"v"}} "#)
            .expect("request");
        assert_eq!(request.id, Some(JsonValue::Int(7)));
        assert_eq!(request.method, "system.ping");
        assert_eq!(
            request.params,
            serde_json::Map::from_iter([(
                "k".to_owned(),
                serde_json::Value::String("v".to_owned())
            )])
        );
    }

    #[test]
    fn lenient_requires_object_prefix_after_trim() {
        assert!(ControlRequestParser.lenient_request("ping").is_none());
        assert!(ControlRequestParser.lenient_request(r#"[{"method":"x"}]"#).is_none());
        assert!(ControlRequestParser.lenient_request("").is_none());
    }

    #[test]
    fn lenient_rejects_missing_or_empty_method() {
        assert!(ControlRequestParser.lenient_request(r#"{"id":1}"#).is_none());
        assert!(ControlRequestParser.lenient_request(r#"{"method":"  "}"#).is_none());
        assert!(ControlRequestParser.lenient_request(r#"{"method":5}"#).is_none());
    }

    #[test]
    fn lenient_defaults_missing_or_non_object_params() {
        let missing = ControlRequestParser
            .lenient_request(r#"{"method":"m"}"#)
            .expect("missing");
        assert!(missing.params.is_empty());
        assert_eq!(missing.id, None);

        let non_object = ControlRequestParser
            .lenient_request(r#"{"method":"m","params":[1]}"#)
            .expect("non-object");
        assert!(non_object.params.is_empty());
    }

    #[test]
    fn strict_parses_envelope() {
        let request = ControlRequestParser
            .request(r#"{"id":"abc","method":"surface.list","params":{"n":2}}"#)
            .expect("request");
        assert_eq!(request.id, Some(JsonValue::String("abc".to_owned())));
        assert_eq!(request.method, "surface.list");
        assert_eq!(
            request.params,
            serde_json::Map::from_iter([("n".to_owned(), serde_json::Value::Number(2.into()))])
        );
    }

    #[test]
    fn strict_classifies_invalid_json() {
        assert_eq!(strict_error("not json"), Some(ControlRequestParseError::InvalidJson));
        assert_eq!(strict_error(r#"{"method""#), Some(ControlRequestParseError::InvalidJson));
    }

    #[test]
    fn strict_classifies_non_object_top_level() {
        assert_eq!(strict_error("[1,2]"), Some(ControlRequestParseError::NotAnObject));
    }

    #[test]
    fn strict_classifies_missing_method_and_echoes_id() {
        assert_eq!(
            strict_error(r#"{"id":3}"#),
            Some(ControlRequestParseError::MissingMethod {
                id: Some(JsonValue::Int(3))
            })
        );
        assert_eq!(
            strict_error(r#"{"method":""}"#),
            Some(ControlRequestParseError::MissingMethod { id: None })
        );
        assert_eq!(
            strict_error(r#"{"id":null,"method":" "}"#),
            Some(ControlRequestParseError::MissingMethod {
                id: Some(JsonValue::Null)
            })
        );
    }

    #[test]
    fn response_encoder_matches_legacy_shapes() {
        let encoder = ControlResponseEncoder;
        let line = encoder.ok(
            Some(JsonValue::Int(4)),
            JsonValue::Object(serde_json::Map::from_iter([(
                "pong".to_owned(),
                serde_json::Value::Bool(true)
            )])),
        );
        let decoded: serde_json::Value = serde_json::from_str(&line).expect("json");
        assert_eq!(
            decoded,
            serde_json::json!({
                "id": 4,
                "ok": true,
                "result": { "pong": true }
            })
        );
    }

    #[test]
    fn parse_error_responses_match_legacy_strings() {
        let encoder = ControlResponseEncoder;
        let utf8: serde_json::Value = serde_json::from_str(
            &encoder.response_for_parse_error(ControlRequestParseError::InvalidUtf8),
        )
        .expect("utf8");
        assert_eq!(
            utf8,
            serde_json::json!({
                "ok": false,
                "error": { "code": "invalid_utf8", "message": "Invalid UTF-8" }
            })
        );
    }

    #[test]
    fn top_level_fragments_collapse_to_encode_failure() {
        let encoder = ControlResponseEncoder;
        assert_eq!(
            encoder.encode(JsonValue::String("x".to_owned())),
            ControlResponseEncoder::ENCODE_FAILURE_RESPONSE
        );
    }

    #[test]
    fn framing_helpers_are_newline_delimited() {
        assert_eq!(split_lines("a\nb\n"), vec!["a", "b"]);
        assert_eq!(append_line("hello"), "hello\n");
    }
}
