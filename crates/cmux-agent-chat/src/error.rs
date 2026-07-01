//! Bridge error type for the agent-session host contract.
//!
//! Ported verbatim from the canonical macOS Swift `AgentSessionBridgeError`
//! (`Sources/Panels/AgentSessionBridgeError.swift`). The camelCase strings
//! returned by [`BridgeError::code`] are load-bearing: the renderer inspects
//! them to decide behaviour (for example it silently no-ops a `writeLine` when
//! the code is `"providerNotReady"`).

use thiserror::Error;

/// Errors surfaced across the agent-session request seam.
///
/// The associated payloads mirror the Swift enum's associated values (provider
/// id, parameter name, method name, session id, transport name). They are
/// carried for diagnostics but deliberately do **not** appear in the
/// user-facing message — matching the macOS app, whose localized strings ignore
/// the payload.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BridgeError {
    /// The request body was not a well-formed `{id, method, params}` object.
    #[error("Invalid bridge request.")]
    InvalidRequest,

    /// A `providerId` was supplied but does not name a known provider.
    #[error("The selected provider is unavailable.")]
    InvalidProvider(String),

    /// A required parameter was absent (or empty, for trimmed lookups).
    #[error("The request is incomplete.")]
    MissingParameter(String),

    /// The requested method is not part of the host contract.
    #[error("This action is not supported.")]
    UnsupportedMethod(String),

    /// The referenced session id is not (or no longer) live.
    #[error("The agent session is no longer available.")]
    SessionNotFound(String),

    /// A session is already running where a fresh one was requested.
    #[error("An agent session is already running.")]
    SessionAlreadyRunning,

    /// The provider transport exists but is not yet ready to accept input.
    #[error("The provider is not ready yet.")]
    ProviderNotReady(String),

    /// The provider's executable could not be resolved or launched (e.g. the CLI
    /// is not installed / not on PATH). Stands in for the macOS
    /// `AgentExecutableResolverError` envelope, which the renderer coordinator
    /// surfaces as `{ok:false, error:{userMessage: error.message}}` — so, unlike
    /// the other variants, the carried detail IS the user-facing message (it names
    /// the real reason instead of a generic "not ready").
    #[error("{0}")]
    ProviderLaunchFailed(String),

    /// The provider declares a transport kind this host cannot drive.
    #[error("Agent transport is not supported.")]
    UnsupportedTransport(String),
}

impl BridgeError {
    /// The stable camelCase code the renderer matches against.
    ///
    /// These strings are a hard wire contract — see the module docs.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalidRequest",
            Self::InvalidProvider(_) => "invalidProvider",
            Self::MissingParameter(_) => "missingParameter",
            Self::UnsupportedMethod(_) => "unsupportedMethod",
            Self::SessionNotFound(_) => "sessionNotFound",
            Self::SessionAlreadyRunning => "sessionAlreadyRunning",
            Self::ProviderNotReady(_) => "providerNotReady",
            Self::ProviderLaunchFailed(_) => "providerLaunchFailed",
            Self::UnsupportedTransport(_) => "unsupportedTransport",
        }
    }

    /// The user-facing message (mirrors the Swift `errorDescription`).
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_wire_contract() {
        assert_eq!(BridgeError::InvalidRequest.code(), "invalidRequest");
        assert_eq!(BridgeError::InvalidProvider("x".into()).code(), "invalidProvider");
        assert_eq!(BridgeError::MissingParameter("x".into()).code(), "missingParameter");
        assert_eq!(BridgeError::UnsupportedMethod("x".into()).code(), "unsupportedMethod");
        assert_eq!(BridgeError::SessionNotFound("x".into()).code(), "sessionNotFound");
        assert_eq!(BridgeError::SessionAlreadyRunning.code(), "sessionAlreadyRunning");
        assert_eq!(BridgeError::ProviderLaunchFailed("x".into()).code(), "providerLaunchFailed");
        assert_eq!(BridgeError::UnsupportedTransport("x".into()).code(), "unsupportedTransport");
    }

    #[test]
    fn provider_launch_failed_message_is_the_carried_detail() {
        // Unlike the other variants, the detail IS the user-facing message (it
        // mirrors the macOS resolver-error envelope's verbatim `userMessage`).
        let err = BridgeError::ProviderLaunchFailed("Codex could not be started. not found".into());
        assert_eq!(err.user_message(), "Codex could not be started. not found");
    }

    #[test]
    fn provider_not_ready_code_is_load_bearing() {
        // The renderer silently no-ops writeLine on exactly this string.
        assert_eq!(BridgeError::ProviderNotReady("codex".into()).code(), "providerNotReady");
    }

    #[test]
    fn user_message_is_non_empty_and_payload_free() {
        let err = BridgeError::InvalidProvider("codex".into());
        let msg = err.user_message();
        assert!(!msg.is_empty());
        // Payload must not leak into the user-facing message.
        assert!(!msg.contains("codex"));
    }
}
