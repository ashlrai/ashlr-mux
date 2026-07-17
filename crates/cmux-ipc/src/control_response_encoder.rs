use crate::{
    control_call_result::ControlCallResult, control_request_parse_error::ControlRequestParseError,
    json_value::JsonValue,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct ControlResponseEncoder;

impl ControlResponseEncoder {
    pub const ENCODE_FAILURE_RESPONSE: &'static str =
        "{\"ok\":false,\"error\":{\"code\":\"encode_error\",\"message\":\"Failed to encode JSON\"}}";

    pub fn ok(&self, id: Option<JsonValue>, result: JsonValue) -> String {
        self.encode_value(serde_json::json!({
            "id": id.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null),
            "ok": true,
            "result": serde_json::Value::from(result),
        }))
    }

    pub fn error(
        &self,
        id: Option<JsonValue>,
        code: &str,
        message: &str,
        data: Option<JsonValue>,
    ) -> String {
        let mut error = serde_json::Map::from_iter([
            (
                "code".to_owned(),
                serde_json::Value::String(code.to_owned()),
            ),
            (
                "message".to_owned(),
                serde_json::Value::String(message.to_owned()),
            ),
        ]);
        if let Some(data) = data {
            error.insert("data".to_owned(), data.into());
        }

        self.encode_value(serde_json::Value::Object(serde_json::Map::from_iter([
            (
                "id".to_owned(),
                id.map(serde_json::Value::from)
                    .unwrap_or(serde_json::Value::Null),
            ),
            ("ok".to_owned(), serde_json::Value::Bool(false)),
            ("error".to_owned(), serde_json::Value::Object(error)),
        ])))
    }

    pub fn response(&self, id: Option<JsonValue>, result: ControlCallResult) -> String {
        match result {
            ControlCallResult::Ok(payload) => self.ok(id, payload),
            ControlCallResult::Err {
                code,
                message,
                data,
            } => self.error(id, &code, &message, data),
        }
    }

    pub fn response_for_parse_error(&self, error: ControlRequestParseError) -> String {
        match error {
            ControlRequestParseError::InvalidUtf8 => self.encode_value(serde_json::json!({
                "ok": false,
                "error": {
                    "code": "invalid_utf8",
                    "message": "Invalid UTF-8",
                }
            })),
            ControlRequestParseError::InvalidJson => self.encode_value(serde_json::json!({
                "ok": false,
                "error": {
                    "code": "parse_error",
                    "message": "Invalid JSON",
                }
            })),
            ControlRequestParseError::NotAnObject => self.encode_value(serde_json::json!({
                "ok": false,
                "error": {
                    "code": "invalid_request",
                    "message": "Expected JSON object",
                }
            })),
            ControlRequestParseError::MissingMethod { id } => {
                self.error(id, "invalid_request", "Missing method", None)
            }
        }
    }

    pub fn encode(&self, value: JsonValue) -> String {
        match value {
            JsonValue::Array(_) | JsonValue::Object(_) => self.encode_value(value.into()),
            _ => Self::ENCODE_FAILURE_RESPONSE.to_owned(),
        }
    }

    fn encode_value(&self, value: serde_json::Value) -> String {
        if !value.is_array() && !value.is_object() {
            return Self::ENCODE_FAILURE_RESPONSE.to_owned();
        }
        match serde_json::to_string(&value) {
            Ok(serialized) => serialized.replace('\n', "\\n").replace('\r', "\\r"),
            Err(_) => Self::ENCODE_FAILURE_RESPONSE.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_rejects_non_container_values() {
        let encoder = ControlResponseEncoder;
        assert_eq!(
            encoder.encode(JsonValue::Int(1)),
            ControlResponseEncoder::ENCODE_FAILURE_RESPONSE
        );
        assert_eq!(
            encoder.encode(JsonValue::String("x".into())),
            ControlResponseEncoder::ENCODE_FAILURE_RESPONSE
        );
        assert!(encoder
            .encode(JsonValue::Array(vec![JsonValue::Int(1)]))
            .contains("[1]"));
    }

    #[test]
    fn encoded_responses_never_contain_raw_newlines() {
        // NDJSON framing puts one response per line, so any CR/LF reaching the
        // wire must be escaped rather than emitted raw (guards against a future
        // switch to pretty-printed output, which would break framing).
        let encoder = ControlResponseEncoder;
        let encoded = encoder.ok(
            Some(JsonValue::Int(7)),
            JsonValue::String("line1\nline2\rend".into()),
        );
        assert!(!encoded.contains('\n'));
        assert!(!encoded.contains('\r'));
        assert!(encoded.contains("\\n"));
        assert!(encoded.contains("\\r"));
    }

    #[test]
    fn nonfinite_error_data_collapses_to_canonical_encode_failure() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                ControlResponseEncoder.error(
                    Some(JsonValue::Int(7)),
                    "invalid_params",
                    "font_size must be a positive number of points",
                    Some(JsonValue::Double(value)),
                ),
                ControlResponseEncoder::ENCODE_FAILURE_RESPONSE
            );
        }
    }
}
