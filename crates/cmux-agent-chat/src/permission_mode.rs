//! Composer permission mode + codex per-turn override assembly.
//!
//! Ported verbatim from the canonical macOS Swift `AgentSessionPermissionMode`
//! (`Sources/Panels/AgentSessionPermissionMode.swift`). The raw string values
//! match the renderer's `ComposerPermissionMode`
//! (`default | auto-review | full-access | custom`).

use serde_json::{json, Value};

/// The permission mode selected in the composer.
///
/// `Standard` is the raw value `"default"` (Swift `.standard`); the remaining
/// variants map to their kebab-case raw strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// Raw `"default"` — the fallback mode.
    Standard,
    /// Raw `"auto-review"`.
    AutoReview,
    /// Raw `"full-access"`.
    FullAccess,
    /// Raw `"custom"` — emits no codex overrides.
    Custom,
}

impl PermissionMode {
    /// Parse from the renderer's raw string, returning `None` for unknown values.
    pub fn from_raw(raw: &str) -> Option<Self> {
        match raw {
            "default" => Some(Self::Standard),
            "auto-review" => Some(Self::AutoReview),
            "full-access" => Some(Self::FullAccess),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// The raw wire string for this mode.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Standard => "default",
            Self::AutoReview => "auto-review",
            Self::FullAccess => "full-access",
            Self::Custom => "custom",
        }
    }

    /// The codex per-turn override object.
    ///
    /// Mirrors the Swift `codexTurnOverrides` exactly, including the JSON `null`
    /// entries where macOS used `NSNull()`. `Custom` returns an empty object so
    /// the caller applies no overrides.
    pub fn codex_turn_overrides(&self) -> Value {
        match self {
            Self::Standard => json!({
                "approvalPolicy": "never",
                "approvalsReviewer": Value::Null,
                "sandboxPolicy": Value::Null,
            }),
            Self::Custom => json!({}),
            Self::AutoReview => json!({
                "approvalPolicy": "on-request",
                "approvalsReviewer": "auto_review",
                "sandboxPolicy": Value::Null,
            }),
            Self::FullAccess => json!({
                "approvalPolicy": "never",
                "approvalsReviewer": "user",
                "sandboxPolicy": { "type": "dangerFullAccess" },
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_raw_round_trips_all_variants() {
        for mode in [
            PermissionMode::Standard,
            PermissionMode::AutoReview,
            PermissionMode::FullAccess,
            PermissionMode::Custom,
        ] {
            assert_eq!(PermissionMode::from_raw(mode.as_str()), Some(mode));
        }
    }

    #[test]
    fn from_raw_rejects_unknown() {
        assert_eq!(PermissionMode::from_raw("nope"), None);
        assert_eq!(PermissionMode::from_raw(""), None);
    }

    #[test]
    fn standard_overrides_include_json_null() {
        let v = PermissionMode::Standard.codex_turn_overrides();
        assert_eq!(v["approvalPolicy"], json!("never"));
        assert_eq!(v["approvalsReviewer"], Value::Null);
        assert_eq!(v["sandboxPolicy"], Value::Null);
        // NSNull must serialize to a present `null`, not an omitted key.
        assert!(v.as_object().unwrap().contains_key("approvalsReviewer"));
        assert!(v.as_object().unwrap().contains_key("sandboxPolicy"));
    }

    #[test]
    fn auto_review_overrides() {
        let v = PermissionMode::AutoReview.codex_turn_overrides();
        assert_eq!(v["approvalPolicy"], json!("on-request"));
        assert_eq!(v["approvalsReviewer"], json!("auto_review"));
        assert_eq!(v["sandboxPolicy"], Value::Null);
    }

    #[test]
    fn full_access_overrides() {
        let v = PermissionMode::FullAccess.codex_turn_overrides();
        assert_eq!(v["approvalPolicy"], json!("never"));
        assert_eq!(v["approvalsReviewer"], json!("user"));
        assert_eq!(v["sandboxPolicy"], json!({ "type": "dangerFullAccess" }));
    }

    #[test]
    fn custom_overrides_are_empty() {
        assert_eq!(PermissionMode::Custom.codex_turn_overrides(), json!({}));
    }
}
