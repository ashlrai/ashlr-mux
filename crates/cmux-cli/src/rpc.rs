//! The `rpc` command's pure pieces: parse the optional JSON params, build the
//! v2 request line, and interpret the v2 response (M4 WS5).
//!
//! Ports `CLI/cmux.swift` `parseRPCParams` (~18174) and `sendV2` (~2547). These
//! are transport-agnostic and cross-platform-testable; the named-pipe round-trip
//! that carries the request lives in [`crate::transport`].

use crate::invocation::CliError;

/// The request id. A CLI invocation issues exactly one request per connection,
/// so a fixed id suffices (the server echoes it back; nothing correlates on it).
const RPC_REQUEST_ID: i64 = 1;

/// Parse the optional trailing `rpc` arguments into a JSON params object. The
/// arguments are joined with spaces and trimmed (so the params may be a single
/// quoted JSON argument or several tokens); empty input yields an empty object.
/// Mirrors `parseRPCParams`: the value must be a JSON **object**.
pub fn parse_rpc_params(args: &[String]) -> Result<serde_json::Value, CliError> {
    let raw = args.join(" ");
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(serde_json::json!({}));
    }
    let value: serde_json::Value = serde_json::from_str(raw)
        .map_err(|error| CliError::new(format!("rpc params must be valid JSON: {error}")))?;
    if !value.is_object() {
        return Err(CliError::new("rpc params must be a JSON object"));
    }
    Ok(value)
}

/// Build the one-line v2 request envelope for `method` + `params`
/// (`{"id":1,"method":…,"params":…}`). `serde_json` emits compact single-line
/// JSON with any embedded newlines escaped, so the result is a valid NDJSON
/// frame body.
pub fn build_rpc_request(method: &str, params: &serde_json::Value) -> String {
    serde_json::json!({
        "id": RPC_REQUEST_ID,
        "method": method,
        "params": params,
    })
    .to_string()
}

/// Interpret a v2 response line into the result value, or a [`CliError`].
///
/// Mirrors `sendV2`'s tail: a plain-text `ERROR:` reply is surfaced verbatim; a
/// non-object / unparseable reply is an "Invalid v2 response"; `{"ok":true}`
/// yields its `result` (an empty object if absent); `{"ok":false}` with an
/// `error` becomes `"<code>: <message>"` (the Rust server emits only
/// code/message, so the macOS `action`/`reason`/`details` formatting is moot).
///
/// Unlike the Swift `sendV2`, a non-object `result` is returned as-is rather than
/// coerced to `{}` — the `rpc` passthrough should print whatever the method
/// returned (the Swift coercion is a `[String: Any]` type artifact, not intent).
pub fn interpret_rpc_response(raw: &str) -> Result<serde_json::Value, CliError> {
    if raw.starts_with("ERROR:") {
        return Err(CliError::new(raw.to_owned()));
    }

    // An unparseable reply and a non-object reply mean the same thing here.
    let invalid = || CliError::new(format!("Invalid v2 response: {raw}"));
    let response: serde_json::Value = serde_json::from_str(raw).map_err(|_| invalid())?;
    let object = response.as_object().ok_or_else(invalid)?;

    if object.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(object
            .get("result")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})));
    }

    if let Some(error) = object.get("error").and_then(serde_json::Value::as_object) {
        let code = error
            .get("code")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("error");
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Unknown v2 error");
        return Err(CliError::new(format!("{code}: {message}")));
    }

    Err(CliError::new("v2 request failed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn empty_params_is_empty_object() {
        assert_eq!(parse_rpc_params(&[]).unwrap(), serde_json::json!({}));
        assert_eq!(parse_rpc_params(&strings(&["  "])).unwrap(), serde_json::json!({}));
    }

    #[test]
    fn params_join_across_tokens() {
        let value = parse_rpc_params(&strings(&[r#"{"a":"#, "1}"])).unwrap();
        assert_eq!(value, serde_json::json!({"a": 1}));
    }

    #[test]
    fn invalid_json_params_error() {
        let error = parse_rpc_params(&strings(&["{bad"])).unwrap_err();
        assert_eq!(error.exit_code, 1);
        assert!(error.message.starts_with("rpc params must be valid JSON"));
    }

    #[test]
    fn non_object_params_error() {
        let error = parse_rpc_params(&strings(&["[1,2]"])).unwrap_err();
        assert_eq!(error.message, "rpc params must be a JSON object");
    }

    #[test]
    fn request_envelope_shape() {
        let line = build_rpc_request("surface.list", &serde_json::json!({"n": 2}));
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["id"], serde_json::json!(1));
        assert_eq!(value["method"], serde_json::json!("surface.list"));
        assert_eq!(value["params"], serde_json::json!({"n": 2}));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn ok_response_yields_result() {
        let result = interpret_rpc_response(r#"{"id":1,"ok":true,"result":{"pong":true}}"#).unwrap();
        assert_eq!(result, serde_json::json!({"pong": true}));
    }

    #[test]
    fn ok_response_without_result_is_empty_object() {
        let result = interpret_rpc_response(r#"{"id":1,"ok":true}"#).unwrap();
        assert_eq!(result, serde_json::json!({}));
    }

    #[test]
    fn ok_response_non_object_result_returned_as_is() {
        let result = interpret_rpc_response(r#"{"ok":true,"result":[1,2]}"#).unwrap();
        assert_eq!(result, serde_json::json!([1, 2]));
    }

    #[test]
    fn error_response_formats_code_and_message() {
        let error =
            interpret_rpc_response(r#"{"id":1,"ok":false,"error":{"code":"auth_required","message":"need auth"}}"#)
                .unwrap_err();
        assert_eq!(error.message, "auth_required: need auth");
    }

    #[test]
    fn plain_text_error_is_surfaced_verbatim() {
        let error = interpret_rpc_response("ERROR: Access denied").unwrap_err();
        assert_eq!(error.message, "ERROR: Access denied");
    }

    #[test]
    fn unparseable_response_is_invalid() {
        let error = interpret_rpc_response("not json").unwrap_err();
        assert!(error.message.starts_with("Invalid v2 response:"));
    }
}
