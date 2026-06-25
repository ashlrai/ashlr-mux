use crate::json_value::JsonValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlRequest {
    pub id: Option<JsonValue>,
    pub method: String,
    pub params: serde_json::Map<String, serde_json::Value>,
}

impl ControlRequest {
    pub fn new(
        id: Option<JsonValue>,
        method: impl Into<String>,
        params: serde_json::Map<String, serde_json::Value>,
    ) -> Self {
        Self {
            id,
            method: method.into(),
            params,
        }
    }
}
