use crate::{
    control_call_result::ControlCallResult,
    control_request_parse_error::ControlRequestParseError,
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
            ("code".to_owned(), serde_json::Value::String(code.to_owned())),
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
                id.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null),
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
