use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    Double(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(serde_json::Map<String, serde_json::Value>),
}

impl Eq for JsonValue {}

impl TryFrom<serde_json::Value> for JsonValue {
    type Error = &'static str;

    fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
        match value {
            serde_json::Value::Null => Ok(Self::Null),
            serde_json::Value::Bool(value) => Ok(Self::Bool(value)),
            serde_json::Value::Number(number) => {
                if let Some(value) = number.as_i64() {
                    Ok(Self::Int(value))
                } else if let Some(value) = number.as_u64() {
                    match i64::try_from(value) {
                        Ok(value) => Ok(Self::Int(value)),
                        Err(_) => Ok(Self::Double(value as f64)),
                    }
                } else if let Some(value) = number.as_f64() {
                    Ok(Self::Double(value))
                } else {
                    Err("unsupported number")
                }
            }
            serde_json::Value::String(value) => Ok(Self::String(value)),
            serde_json::Value::Array(values) => Ok(Self::Array(
                values
                    .into_iter()
                    .map(JsonValue::try_from)
                    .collect::<Result<Vec<_>, _>>()?,
            )),
            serde_json::Value::Object(values) => Ok(Self::Object(values)),
        }
    }
}

impl From<JsonValue> for serde_json::Value {
    fn from(value: JsonValue) -> Self {
        match value {
            JsonValue::Null => Self::Null,
            JsonValue::Bool(value) => Self::Bool(value),
            JsonValue::Int(value) => Self::Number(value.into()),
            JsonValue::Double(value) => serde_json::Number::from_f64(value)
                .map(Self::Number)
                .unwrap_or(Self::Null),
            JsonValue::String(value) => Self::String(value),
            JsonValue::Array(values) => Self::Array(values.into_iter().map(Self::from).collect()),
            JsonValue::Object(values) => Self::Object(values),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(value: serde_json::Value) -> serde_json::Value {
        serde_json::Value::from(JsonValue::try_from(value).expect("convert"))
    }

    #[test]
    fn scalars_round_trip() {
        for value in [
            serde_json::json!(null),
            serde_json::json!(true),
            serde_json::json!(false),
            serde_json::json!(42),
            serde_json::json!(-7),
            serde_json::json!(2.5),
            serde_json::json!("hello"),
        ] {
            assert_eq!(round_trip(value.clone()), value);
        }
    }

    #[test]
    fn whole_numbers_decode_as_int_not_double() {
        assert!(matches!(
            JsonValue::try_from(serde_json::json!(5)).unwrap(),
            JsonValue::Int(5)
        ));
        assert!(matches!(
            JsonValue::try_from(serde_json::json!(1.5)).unwrap(),
            JsonValue::Double(_)
        ));
    }

    #[test]
    fn u64_above_i64_max_falls_back_to_double() {
        let big = (i64::MAX as u64) + 1;
        assert!(matches!(
            JsonValue::try_from(serde_json::json!(big)).unwrap(),
            JsonValue::Double(_)
        ));
    }

    #[test]
    fn nested_array_and_object_round_trip() {
        let input = serde_json::json!({
            "method": "ping",
            "params": [1, "two", false, null, {"nested": 3}],
        });
        assert_eq!(round_trip(input.clone()), input);
    }
}
