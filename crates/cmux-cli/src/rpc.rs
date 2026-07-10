//! The `rpc` command's CLI-specific argument parsing (M4 WS5).
//!
//! Ports `CLI/cmux.swift` `parseRPCParams` (~18174). The v2 wire codec
//! (request envelope encode + response decode) lives with the other v2 client
//! halves in [`cmux_ipc`] (`build_v2_request` / `interpret_v2_response`); this
//! module only turns the trailing CLI tokens into the params object.

use crate::invocation::CliError;

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

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn empty_params_is_empty_object() {
        assert_eq!(parse_rpc_params(&[]).unwrap(), serde_json::json!({}));
        assert_eq!(
            parse_rpc_params(&strings(&["  "])).unwrap(),
            serde_json::json!({})
        );
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
}
