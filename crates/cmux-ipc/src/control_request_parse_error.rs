use crate::json_value::JsonValue;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ControlRequestParseError {
    #[error("invalid utf-8")]
    InvalidUtf8,
    #[error("invalid json")]
    InvalidJson,
    #[error("expected a top-level object")]
    NotAnObject,
    #[error("missing method")]
    MissingMethod { id: Option<JsonValue> },
}
