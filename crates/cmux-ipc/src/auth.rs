//! Per-connection password auth gate for the control socket (M4).
//!
//! When the socket runs in a password-protected access mode, every connection
//! must authenticate before its commands are dispatched. A client authenticates
//! with either the v1 text command `auth <password>` or the v2 JSON request
//! `{"method":"auth.login","params":{"password":"…"}}`; once a connection
//! authenticates the flag sticks for the rest of that connection.
//!
//! This is a verbatim port of the gate in
//! `Sources/TerminalController.swift` (`authResponseIfNeeded`,
//! `passwordLoginV1ResponseIfNeeded`, `passwordLoginV2ResponseIfNeeded`,
//! `passwordAuthRequiredResponse`). The byte-level request/response shapes match
//! the macOS server exactly so the same CLI client
//! (`authenticateSocketClientIfNeeded` → `auth <password>`) drives both.
//!
//! The gate is transport-agnostic: it operates on already-decoded command lines,
//! so it is tested over plain strings on any OS and reused by the Windows
//! named-pipe server and any future transport unchanged.

use crate::{json_value::JsonValue, ControlResponseEncoder};

/// Verifies a presented socket password against the configured one. The app
/// injects the concrete store (keychain / file / Settings); the gate stays
/// oblivious to where the password comes from, exactly like the Swift
/// `SocketControlPasswordStore` injection into `TerminalController`.
pub trait PasswordVerifier {
    /// Whether a socket password is configured at all. Mirrors
    /// `passwordStore.hasConfiguredPassword(allowLazyKeychainFallback: true)`.
    fn has_configured_password(&self) -> bool;

    /// Whether `password` matches the configured one. Mirrors
    /// `passwordStore.verify(password:allowLazyKeychainFallback: true)`.
    fn verify(&self, password: &str) -> bool;
}

/// Per-connection auth state. Starts unauthenticated and flips to `true` after a
/// successful `auth` / `auth.login`, where it stays for the connection's life.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AuthState {
    authenticated: bool,
}

impl AuthState {
    /// Whether this connection has authenticated.
    pub(crate) fn authenticated(&self) -> bool {
        self.authenticated
    }
}

/// Intercepts a command line before it reaches the handler. `Some(line)` is a
/// response to send back instead of dispatching the command; `None` lets the
/// command through.
///
/// Internal plumbing for the serve loop — the public surface is
/// [`PasswordVerifier`], [`PasswordAuthGate`], and the `serve_connection*`
/// entrypoints, so this trait stays crate-private.
pub(crate) trait ConnectionAuthenticator {
    /// Inspect `command` for the connection in `state`. Returns `Some` response
    /// to short-circuit (auth handshake or rejection), `None` to dispatch.
    fn intercept(&self, command: &str, state: &mut AuthState) -> Option<String>;
}

/// No-auth pass-through: every command is dispatched. Used when the access mode
/// does not require a password (`accessMode.requiresPasswordAuth == false`).
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct NoAuth;

impl ConnectionAuthenticator for NoAuth {
    fn intercept(&self, _command: &str, _state: &mut AuthState) -> Option<String> {
        None
    }
}

/// Password auth gate. Constructed only when the access mode requires a
/// password — its mere presence in the serve loop represents the Swift
/// `guard socketServer.accessMode.requiresPasswordAuth` being true.
#[derive(Debug, Default, Clone, Copy)]
pub struct PasswordAuthGate<V> {
    verifier: V,
}

impl<V: PasswordVerifier> PasswordAuthGate<V> {
    /// Build a gate over `verifier`.
    pub fn new(verifier: V) -> Self {
        Self { verifier }
    }

    /// v2 JSON auth (`{"method":"auth.login","params":{"password":"…"}}`).
    /// Returns `None` when `command` is not an `auth.login` request so the gate
    /// can fall through to the v1 check. Mirrors
    /// `passwordLoginV2ResponseIfNeeded`.
    fn v2_login_response(&self, command: &str, state: &mut AuthState) -> Option<String> {
        // Pre-filter before the full JSON parse. Swift checks `hasPrefix("{")`
        // WITHOUT trimming (so leading whitespace routes to the v1 text path),
        // and only `auth.login` requests are handled here. Every genuine
        // `auth.login` line contains that substring; the rare false positive
        // falls through the `method` check below unchanged. This keeps the
        // serve loop from parsing every non-auth command twice — once here and
        // once for dispatch.
        if !command.starts_with('{') || !command.contains("auth.login") {
            return None;
        }
        let object = json_object(command)?;

        let method = object
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim();
        if method != "auth.login" {
            return None;
        }

        let encoder = ControlResponseEncoder;
        let id = json_id(&object);

        let Some(provided) = object
            .get("params")
            .and_then(serde_json::Value::as_object)
            .and_then(|params| params.get("password"))
            .and_then(serde_json::Value::as_str)
        else {
            return Some(encoder.error(
                id,
                "invalid_params",
                "auth.login requires params.password",
                None,
            ));
        };

        if !self.verifier.has_configured_password() {
            return Some(encoder.error(
                id,
                "auth_unconfigured",
                "Password mode is enabled but no socket password is configured in Settings.",
                None,
            ));
        }
        if !self.verifier.verify(provided) {
            return Some(encoder.error(id, "auth_failed", "Invalid password", None));
        }

        state.authenticated = true;
        Some(encoder.ok(id, authenticated_result()))
    }

    /// v1 text auth (`auth <password>`). Returns `None` when `command` is not an
    /// `auth` command. Mirrors `passwordLoginV1ResponseIfNeeded`.
    fn v1_login_response(&self, command: &str, state: &mut AuthState) -> Option<String> {
        // Match `auth` / `auth ` case-insensitively like Swift's `lowercased()`
        // comparison, but on ASCII bytes so we never allocate a lowercased copy
        // of the (up to 4 MiB) command line just to test a 5-byte keyword.
        let bytes = command.as_bytes();
        let is_bare = bytes.eq_ignore_ascii_case(b"auth");
        let is_prefixed = bytes.len() >= 5 && bytes[..5].eq_ignore_ascii_case(b"auth ");
        if !is_bare && !is_prefixed {
            return None;
        }

        // Unlike the v2 path, v1 checks for a configured password BEFORE
        // validating the presented one (Swift order preserved).
        if !self.verifier.has_configured_password() {
            return Some(
                "ERROR: Password mode is enabled but no socket password is configured in Settings."
                    .to_owned(),
            );
        }

        // `dropFirst(5)` drops the literal "auth " (5 ASCII bytes) and keeps the
        // password with its original casing; the bare "auth" case (len 4) yields
        // `None` here, i.e. an empty password. The matched prefix is ASCII, so
        // byte index 5 is a char boundary and `get` is panic-safe regardless.
        let provided = command.get(5..).unwrap_or_default();
        if provided.is_empty() {
            return Some("ERROR: Missing password. Usage: auth <password>".to_owned());
        }
        if !self.verifier.verify(provided) {
            return Some("ERROR: Invalid password".to_owned());
        }

        state.authenticated = true;
        Some("OK: Authenticated".to_owned())
    }

    /// Rejection for a non-auth command on an unauthenticated connection. v2
    /// (JSON) requests get a JSON `auth_required` error echoing the id; anything
    /// else gets the v1 text rejection. Mirrors `passwordAuthRequiredResponse`.
    fn auth_required_response(&self, command: &str) -> String {
        if let Some(object) = json_object(command) {
            return ControlResponseEncoder.error(
                json_id(&object),
                "auth_required",
                "Authentication required. Send auth <password> first.",
                None,
            );
        }
        "ERROR: Authentication required \u{2014} send auth <password> first".to_owned()
    }
}

impl<V: PasswordVerifier> ConnectionAuthenticator for PasswordAuthGate<V> {
    fn intercept(&self, command: &str, state: &mut AuthState) -> Option<String> {
        if let Some(response) = self.v2_login_response(command, state) {
            return Some(response);
        }
        if let Some(response) = self.v1_login_response(command, state) {
            return Some(response);
        }
        if !state.authenticated() {
            return Some(self.auth_required_response(command));
        }
        None
    }
}

/// Decode `command` into its JSON object map, or `None` if it doesn't textually
/// start with `{` (no trim, for Swift `hasPrefix("{")` parity), isn't valid
/// JSON, or isn't a JSON object. Moves the map out of the parsed value so no
/// clone is made. Shared by the `auth.login` and `auth_required` paths.
fn json_object(command: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    if !command.starts_with('{') {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(command).ok()? {
        serde_json::Value::Object(map) => Some(map),
        _ => None,
    }
}

/// Extract a request `id` from a decoded JSON object as the encoder's
/// `Option<JsonValue>`. A missing id and a present-but-unconvertible id both
/// collapse to `None`, which the encoder renders as `null` — matching Swift's
/// `dict["id"]` (absent → nil → null). Intentionally method-agnostic (unlike
/// `ControlRequestParser`, which drops the id when the method is empty), so the
/// `auth_required` path can echo the id of a method-less line like `{"id":9}`.
fn json_id(object: &serde_json::Map<String, serde_json::Value>) -> Option<JsonValue> {
    object
        .get("id")
        .cloned()
        .and_then(|value| JsonValue::try_from(value).ok())
}

/// The `{ "authenticated": true }` success payload.
fn authenticated_result() -> JsonValue {
    JsonValue::Object(serde_json::Map::from_iter([(
        "authenticated".to_owned(),
        serde_json::Value::Bool(true),
    )]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A verifier with one fixed password. `configured == false` models the
    /// "password mode enabled but nothing set in Settings" state.
    struct FixedVerifier {
        configured: bool,
        password: &'static str,
    }

    impl PasswordVerifier for FixedVerifier {
        fn has_configured_password(&self) -> bool {
            self.configured
        }
        fn verify(&self, password: &str) -> bool {
            self.configured && password == self.password
        }
    }

    fn gate() -> PasswordAuthGate<FixedVerifier> {
        PasswordAuthGate::new(FixedVerifier {
            configured: true,
            password: "s3cret",
        })
    }

    fn json(line: &str) -> serde_json::Value {
        serde_json::from_str(line).expect("json")
    }

    #[test]
    fn v1_correct_password_authenticates() {
        let mut state = AuthState::default();
        let response = gate()
            .intercept("auth s3cret", &mut state)
            .expect("response");
        assert_eq!(response, "OK: Authenticated");
        assert!(state.authenticated());
    }

    #[test]
    fn v1_command_keyword_is_case_insensitive_but_password_is_not() {
        let mut state = AuthState::default();
        // Keyword may be upper-cased; the password keeps its exact casing.
        let response = gate()
            .intercept("AUTH s3cret", &mut state)
            .expect("response");
        assert_eq!(response, "OK: Authenticated");
        assert!(state.authenticated());

        let mut wrong = AuthState::default();
        let response = gate()
            .intercept("auth S3CRET", &mut wrong)
            .expect("response");
        assert_eq!(response, "ERROR: Invalid password");
        assert!(!wrong.authenticated());
    }

    #[test]
    fn v1_wrong_password_rejected() {
        let mut state = AuthState::default();
        let response = gate().intercept("auth nope", &mut state).expect("response");
        assert_eq!(response, "ERROR: Invalid password");
        assert!(!state.authenticated());
    }

    #[test]
    fn v1_missing_password_rejected() {
        let mut state = AuthState::default();
        assert_eq!(
            gate().intercept("auth", &mut state).expect("response"),
            "ERROR: Missing password. Usage: auth <password>"
        );
        // Trailing space, still no password.
        assert_eq!(
            gate().intercept("auth ", &mut state).expect("response"),
            "ERROR: Missing password. Usage: auth <password>"
        );
        assert!(!state.authenticated());
    }

    #[test]
    fn v1_password_preserves_internal_spaces() {
        let spaced = PasswordAuthGate::new(FixedVerifier {
            configured: true,
            password: " pad ed ",
        });
        let mut state = AuthState::default();
        // Everything after "auth " is the password verbatim, including spaces.
        let response = spaced
            .intercept("auth  pad ed ", &mut state)
            .expect("response");
        assert_eq!(response, "OK: Authenticated");
    }

    #[test]
    fn v1_unconfigured_password_reports_settings_error() {
        let unconfigured = PasswordAuthGate::new(FixedVerifier {
            configured: false,
            password: "",
        });
        let mut state = AuthState::default();
        let response = unconfigured
            .intercept("auth anything", &mut state)
            .expect("response");
        assert_eq!(
            response,
            "ERROR: Password mode is enabled but no socket password is configured in Settings."
        );
        assert!(!state.authenticated());
    }

    #[test]
    fn v2_correct_password_authenticates() {
        let mut state = AuthState::default();
        let response = gate()
            .intercept(
                r#"{"id":3,"method":"auth.login","params":{"password":"s3cret"}}"#,
                &mut state,
            )
            .expect("response");
        assert_eq!(
            json(&response),
            serde_json::json!({"id": 3, "ok": true, "result": {"authenticated": true}})
        );
        assert!(state.authenticated());
    }

    #[test]
    fn v2_wrong_password_is_auth_failed() {
        let mut state = AuthState::default();
        let response = gate()
            .intercept(
                r#"{"id":"x","method":"auth.login","params":{"password":"nope"}}"#,
                &mut state,
            )
            .expect("response");
        let value = json(&response);
        assert_eq!(value["id"], serde_json::json!("x"));
        assert_eq!(value["ok"], serde_json::json!(false));
        assert_eq!(value["error"]["code"], serde_json::json!("auth_failed"));
        assert_eq!(
            value["error"]["message"],
            serde_json::json!("Invalid password")
        );
        assert!(!state.authenticated());
    }

    #[test]
    fn v2_missing_password_param_is_invalid_params() {
        let mut state = AuthState::default();
        let response = gate()
            .intercept(r#"{"id":1,"method":"auth.login","params":{}}"#, &mut state)
            .expect("response");
        let value = json(&response);
        assert_eq!(value["error"]["code"], serde_json::json!("invalid_params"));
        assert_eq!(
            value["error"]["message"],
            serde_json::json!("auth.login requires params.password")
        );
    }

    #[test]
    fn v2_unconfigured_password_is_auth_unconfigured() {
        let unconfigured = PasswordAuthGate::new(FixedVerifier {
            configured: false,
            password: "",
        });
        let mut state = AuthState::default();
        let response = unconfigured
            .intercept(
                r#"{"id":1,"method":"auth.login","params":{"password":"x"}}"#,
                &mut state,
            )
            .expect("response");
        assert_eq!(
            json(&response)["error"]["code"],
            serde_json::json!("auth_unconfigured")
        );
    }

    #[test]
    fn unauthenticated_v2_command_gets_json_auth_required_with_echoed_id() {
        let mut state = AuthState::default();
        let response = gate()
            .intercept(r#"{"id":9,"method":"surface.list"}"#, &mut state)
            .expect("response");
        let value = json(&response);
        assert_eq!(value["id"], serde_json::json!(9));
        assert_eq!(value["ok"], serde_json::json!(false));
        assert_eq!(value["error"]["code"], serde_json::json!("auth_required"));
        assert_eq!(
            value["error"]["message"],
            serde_json::json!("Authentication required. Send auth <password> first.")
        );
        assert!(!state.authenticated());
    }

    #[test]
    fn unauthenticated_v1_command_gets_text_auth_required() {
        let mut state = AuthState::default();
        let response = gate()
            .intercept("list-workspaces", &mut state)
            .expect("response");
        assert_eq!(
            response,
            "ERROR: Authentication required \u{2014} send auth <password> first"
        );
    }

    #[test]
    fn leading_whitespace_before_brace_routes_to_text_rejection() {
        // hasPrefix("{") is not trimmed, so a space before `{` is treated as a
        // v1 (text) command and gets the text rejection, not the JSON one.
        let mut state = AuthState::default();
        let response = gate()
            .intercept(r#" {"id":9,"method":"surface.list"}"#, &mut state)
            .expect("response");
        assert_eq!(
            response,
            "ERROR: Authentication required \u{2014} send auth <password> first"
        );
    }

    #[test]
    fn malformed_json_object_on_unauthed_connection_gets_text_rejection() {
        let mut state = AuthState::default();
        // Starts with `{` but is not valid JSON → falls back to text rejection.
        let response = gate().intercept("{not json", &mut state).expect("response");
        assert_eq!(
            response,
            "ERROR: Authentication required \u{2014} send auth <password> first"
        );
    }

    #[test]
    fn once_authenticated_commands_pass_through() {
        let gate = gate();
        let mut state = AuthState::default();
        gate.intercept("auth s3cret", &mut state).expect("auth");
        assert!(state.authenticated());
        // A normal command now passes through (intercept returns None).
        assert!(gate.intercept("list-workspaces", &mut state).is_none());
        assert!(gate
            .intercept(r#"{"id":1,"method":"surface.list"}"#, &mut state)
            .is_none());
    }

    #[test]
    fn no_auth_passes_everything_through() {
        let mut state = AuthState::default();
        assert!(NoAuth.intercept("list-workspaces", &mut state).is_none());
        assert!(NoAuth.intercept("auth whatever", &mut state).is_none());
        assert!(!state.authenticated());
    }
}
