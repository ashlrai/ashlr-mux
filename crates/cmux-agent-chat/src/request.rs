//! Request-seam parsing: `BridgeRequest` and its typed parameter getters.
//!
//! Ported verbatim from the canonical macOS Swift `AgentSessionBridgeRequest`
//! (`Sources/Panels/AgentSessionBridgeRequest.swift`).
//!
//! The trim distinction between [`BridgeRequest::string`] (trimmed, non-empty)
//! and [`BridgeRequest::raw_string`] (untouched) is a hard contract: the
//! `writeLine` text uses `required_raw_string` so leading/trailing whitespace
//! is preserved, whereas `sessionId` uses `required_string`.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::BridgeError;
use crate::event::ProviderId;
use crate::permission_mode::PermissionMode;

/// A decoded `{id, method, params}` request from the renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Map<String, Value>,
}

impl BridgeRequest {
    /// Build from a raw JSON value, mirroring the Swift `init(body:)`.
    ///
    /// Returns [`BridgeError::InvalidRequest`] when the body is not an object
    /// or is missing a string `id`/`method`. A missing or non-object `params`
    /// defaults to an empty map (matching Swift).
    pub fn from_value(body: Value) -> Result<Self, BridgeError> {
        let object = body.as_object().ok_or(BridgeError::InvalidRequest)?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .ok_or(BridgeError::InvalidRequest)?
            .to_string();
        let method = object
            .get("method")
            .and_then(Value::as_str)
            .ok_or(BridgeError::InvalidRequest)?
            .to_string();
        let params = object
            .get("params")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        Ok(Self { id, method, params })
    }

    /// A trimmed, non-empty string parameter, or `None`.
    ///
    /// Trims leading/trailing whitespace (Swift `.whitespacesAndNewlines`) and
    /// treats the empty result as absent.
    pub fn string(&self, key: &str) -> Option<String> {
        let raw = self.params.get(key).and_then(Value::as_str)?;
        let trimmed = raw.trim_matches(char::is_whitespace);
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }

    /// A required trimmed, non-empty string parameter.
    pub fn required_string(&self, key: &str) -> Result<String, BridgeError> {
        self.string(key)
            .ok_or_else(|| BridgeError::MissingParameter(key.to_string()))
    }

    /// A string parameter with no trimming or emptiness check.
    pub fn raw_string(&self, key: &str) -> Option<String> {
        self.params
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
    }

    /// A required string parameter with no trimming (preserves whitespace).
    pub fn required_raw_string(&self, key: &str) -> Result<String, BridgeError> {
        self.raw_string(key)
            .ok_or_else(|| BridgeError::MissingParameter(key.to_string()))
    }

    /// The `providerId` parameter parsed to a [`ProviderId`].
    pub fn provider_id(&self) -> Result<ProviderId, BridgeError> {
        let raw = self.required_string("providerId")?;
        ProviderId::from_raw(&raw).ok_or(BridgeError::InvalidProvider(raw))
    }

    /// The `permissionMode` parameter, falling back to [`PermissionMode::Standard`].
    pub fn permission_mode(&self) -> PermissionMode {
        self.string("permissionMode")
            .and_then(|raw| PermissionMode::from_raw(&raw))
            .unwrap_or(PermissionMode::Standard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(params: Value) -> BridgeRequest {
        BridgeRequest::from_value(json!({
            "id": "req-1",
            "method": "session.writeLine",
            "params": params,
        }))
        .unwrap()
    }

    #[test]
    fn from_value_parses_fields() {
        let req = request(json!({ "sessionId": "s1" }));
        assert_eq!(req.id, "req-1");
        assert_eq!(req.method, "session.writeLine");
        assert_eq!(req.params.get("sessionId").unwrap(), &json!("s1"));
    }

    #[test]
    fn from_value_defaults_missing_params_to_empty() {
        let req = BridgeRequest::from_value(json!({ "id": "r", "method": "m" })).unwrap();
        assert!(req.params.is_empty());
    }

    #[test]
    fn from_value_rejects_non_object() {
        assert_eq!(
            BridgeRequest::from_value(json!("nope")),
            Err(BridgeError::InvalidRequest)
        );
    }

    #[test]
    fn from_value_rejects_missing_id_or_method() {
        assert_eq!(
            BridgeRequest::from_value(json!({ "method": "m" })),
            Err(BridgeError::InvalidRequest)
        );
        assert_eq!(
            BridgeRequest::from_value(json!({ "id": "r" })),
            Err(BridgeError::InvalidRequest)
        );
    }

    #[test]
    fn trim_distinction_is_enforced() {
        let req = request(json!({ "text": "  hello world  ", "sessionId": "  s1  " }));
        // writeLine text preserves whitespace...
        assert_eq!(req.required_raw_string("text").unwrap(), "  hello world  ");
        assert_eq!(req.raw_string("text").unwrap(), "  hello world  ");
        // ...while sessionId is trimmed.
        assert_eq!(req.required_string("sessionId").unwrap(), "s1");
        assert_eq!(req.string("sessionId").unwrap(), "s1");
    }

    #[test]
    fn string_treats_whitespace_only_as_absent() {
        let req = request(json!({ "sessionId": "   " }));
        assert_eq!(req.string("sessionId"), None);
        assert_eq!(
            req.required_string("sessionId"),
            Err(BridgeError::MissingParameter("sessionId".to_string()))
        );
        // ...but raw_string still returns the untrimmed value.
        assert_eq!(req.raw_string("sessionId").unwrap(), "   ");
    }

    #[test]
    fn missing_raw_string_errors() {
        let req = request(json!({}));
        assert_eq!(
            req.required_raw_string("text"),
            Err(BridgeError::MissingParameter("text".to_string()))
        );
        assert_eq!(req.raw_string("text"), None);
    }

    #[test]
    fn provider_id_parses_and_rejects() {
        let ok = request(json!({ "providerId": "codex" }));
        assert_eq!(ok.provider_id().unwrap(), ProviderId::Codex);

        let bad = request(json!({ "providerId": "gemini" }));
        assert_eq!(
            bad.provider_id(),
            Err(BridgeError::InvalidProvider("gemini".to_string()))
        );

        let missing = request(json!({}));
        assert_eq!(
            missing.provider_id(),
            Err(BridgeError::MissingParameter("providerId".to_string()))
        );
    }

    #[test]
    fn permission_mode_parses_and_defaults() {
        assert_eq!(
            request(json!({ "permissionMode": "full-access" })).permission_mode(),
            PermissionMode::FullAccess
        );
        // Unknown / missing / whitespace-only all fall back to Standard.
        assert_eq!(
            request(json!({ "permissionMode": "bogus" })).permission_mode(),
            PermissionMode::Standard
        );
        assert_eq!(request(json!({})).permission_mode(), PermissionMode::Standard);
    }
}
