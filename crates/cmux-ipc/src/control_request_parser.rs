use crate::{
    control_request::ControlRequest,
    control_request_parse_error::ControlRequestParseError,
    json_value::JsonValue,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct ControlRequestParser;

impl ControlRequestParser {
    pub fn lenient_request(&self, line: &str) -> Option<ControlRequest> {
        let trimmed = line.trim();
        if !trimmed.starts_with('{') {
            return None;
        }

        let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
        let object = value.as_object()?;
        let request = Self::request_from_object(object)?;
        if request.method.is_empty() {
            return None;
        }
        Some(request)
    }

    pub fn request(&self, line: &str) -> Result<ControlRequest, ControlRequestParseError> {
        let value: serde_json::Value =
            serde_json::from_str(line).map_err(|_| ControlRequestParseError::InvalidJson)?;
        let object = value
            .as_object()
            .ok_or(ControlRequestParseError::NotAnObject)?;
        let request = Self::request_from_object(object)
            .ok_or(ControlRequestParseError::MissingMethod { id: None })?;
        if request.method.is_empty() {
            return Err(ControlRequestParseError::MissingMethod { id: request.id });
        }
        Ok(request)
    }

    fn request_from_object(
        object: &serde_json::Map<String, serde_json::Value>,
    ) -> Option<ControlRequest> {
        let id = object.get("id").cloned().map(JsonValue::try_from).transpose().ok()?;
        let method = object
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned();
        let params = object
            .get("params")
            .and_then(serde_json::Value::as_object)
            .cloned()
            .unwrap_or_default();
        Some(ControlRequest::new(id, method, params))
    }
}
