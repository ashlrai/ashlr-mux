use crate::json_value::JsonValue;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlCallResult {
    Ok(JsonValue),
    Err {
        code: String,
        message: String,
        data: Option<JsonValue>,
    },
}
