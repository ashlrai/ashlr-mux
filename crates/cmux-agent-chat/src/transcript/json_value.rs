//! A dynamically-shaped JSON value walker for transcript JSONL lines.
//!
//! Ports `Parsing/TranscriptJSONValue.swift`. Transcript lines have loose,
//! evolving schemas, so the parsers walk a typed JSON tree instead of
//! decoding fixed shapes. Unknown or missing keys read as `None` and the
//! parsers fail open (skip the line).
//!
//! DIVERGENCE: The Swift original is a hand-rolled `enum` that copies every
//! subscript by value. Here the walker is a zero-copy borrowing view over a
//! `serde_json::Value` (`serde_json::Value` already models exactly the same
//! six JSON shapes), so chained subscripts never clone the parsed tree.

use serde_json::Value;

/// Decodes one JSONL line, returning `None` for malformed JSON.
///
/// Mirrors the Swift `TranscriptJSONValue(jsonLine:)` failable initializer.
pub fn parse_json_line(line: &str) -> Option<Value> {
    serde_json::from_str(line).ok()
}

/// A borrowing view over one node of a decoded transcript JSON tree.
///
/// Copy-cheap (`&Value` inside): subscripting walks the shared tree without
/// cloning, matching the Swift value-type `subscript`/accessor ergonomics.
#[derive(Debug, Clone, Copy)]
pub struct TranscriptJson<'a> {
    value: &'a Value,
}

impl<'a> TranscriptJson<'a> {
    /// Wraps a decoded JSON value.
    pub fn new(value: &'a Value) -> Self {
        TranscriptJson { value }
    }

    /// The string payload, or `None` when this is not a string.
    pub fn string(&self) -> Option<&'a str> {
        self.value.as_str()
    }

    /// The boolean payload, or `None` when this is not a boolean.
    pub fn bool(&self) -> Option<bool> {
        self.value.as_bool()
    }

    /// The numeric payload as a double, or `None` when not a number.
    ///
    /// Integers decode to their double value, exactly like the Swift walker
    /// (which represents every JSON number as a `Double`).
    pub fn double(&self) -> Option<f64> {
        self.value.as_f64()
    }

    /// The numeric payload as an integer, or `None` when not a number.
    ///
    /// Truncates toward zero, mirroring the Swift `Int(double)` conversion.
    pub fn int(&self) -> Option<i64> {
        self.value
            .as_i64()
            .or_else(|| self.value.as_u64().map(|u| u as i64))
            .or_else(|| self.value.as_f64().map(|f| f as i64))
    }

    /// Whether this value is a JSON object (the Swift `object != nil` guard).
    pub fn is_object(&self) -> bool {
        self.value.is_object()
    }

    /// The array elements as borrowing views, or `None` when not an array.
    pub fn array(&self) -> Option<Vec<TranscriptJson<'a>>> {
        self.value
            .as_array()
            .map(|items| items.iter().map(TranscriptJson::new).collect())
    }

    /// Looks up a key on an object value; `None` for other shapes.
    ///
    /// Mirrors the Swift `subscript(key:)` that reads `object?[key]`.
    pub fn get(&self, key: &str) -> Option<TranscriptJson<'a>> {
        self.value
            .as_object()
            .and_then(|map| map.get(key))
            .map(TranscriptJson::new)
    }

    /// Renders the value as compact JSON with sorted keys and unescaped
    /// slashes (the Swift `compactJSONString()` with `[.sortedKeys,
    /// .withoutEscapingSlashes]`).
    pub fn compact_json_string(&self) -> String {
        compact_json_string(self.value)
    }
}

/// Renders a JSON value as compact text with lexicographically sorted object
/// keys and unescaped slashes.
///
/// Object-key sorting is done explicitly here rather than relying on
/// `serde_json`'s `Map` ordering, so the output is stable regardless of
/// whether the `preserve_order` feature is enabled anywhere in the workspace.
/// `serde_json` never escapes `/`, matching Foundation's
/// `.withoutEscapingSlashes`.
pub fn compact_json_string(value: &Value) -> String {
    let mut out = String::new();
    write_compact(value, &mut out);
    out
}

fn write_compact(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(*key).expect("scalar JSON serialization"));
                out.push(':');
                write_compact(&map[*key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_compact(item, out);
            }
            out.push(']');
        }
        scalar => write_scalar(scalar, out),
    }
}

/// Serializes a JSON scalar (string / number / bool / null) via `serde_json`,
/// which already emits compact form and leaves slashes unescaped.
fn write_scalar(value: &Value, out: &mut String) {
    // A scalar `Value` always serializes; the unwrap cannot fail.
    out.push_str(&serde_json::to_string(value).expect("scalar JSON serialization"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn malformed_line_is_none() {
        assert!(parse_json_line("not json").is_none());
        assert!(parse_json_line("{\"a\": truncated").is_none());
        // Valid JSON that is not an object still decodes (the parsers apply
        // the object guard themselves).
        assert!(parse_json_line("[1, 2, 3]").is_some());
    }

    #[test]
    fn accessors_read_each_shape() {
        let value = json!({
            "s": "hi", "b": true, "n": 42, "f": 1.5,
            "arr": [1, 2], "obj": {"k": "v"}
        });
        let root = TranscriptJson::new(&value);
        assert!(root.is_object());
        assert_eq!(root.get("s").and_then(|v| v.string()), Some("hi"));
        assert_eq!(root.get("b").and_then(|v| v.bool()), Some(true));
        assert_eq!(root.get("n").and_then(|v| v.int()), Some(42));
        assert_eq!(root.get("f").and_then(|v| v.double()), Some(1.5));
        assert_eq!(root.get("f").and_then(|v| v.int()), Some(1));
        assert_eq!(root.get("arr").and_then(|v| v.array()).unwrap().len(), 2);
        assert_eq!(
            root.get("obj").and_then(|v| v.get("k")).and_then(|v| v.string()),
            Some("v")
        );
        // Subscripting a non-object reads as None, like the Swift walker.
        assert!(root.get("s").and_then(|v| v.get("nope")).is_none());
    }

    #[test]
    fn compact_json_sorts_keys_and_keeps_slashes() {
        let value = json!({ "zebra": 1, "alpha": "/repo/main.swift", "middle": true });
        assert_eq!(
            compact_json_string(&value),
            r#"{"alpha":"/repo/main.swift","middle":true,"zebra":1}"#
        );
    }

    #[test]
    fn compact_json_sorts_nested_keys() {
        let value = json!({ "b": { "y": 2, "x": 1 }, "a": [ { "d": 4, "c": 3 } ] });
        assert_eq!(
            compact_json_string(&value),
            r#"{"a":[{"c":3,"d":4}],"b":{"x":1,"y":2}}"#
        );
    }
}
