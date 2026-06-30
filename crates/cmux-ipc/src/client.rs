//! Control-socket client policy: password resolution + the auth handshake (M4).
//!
//! This is the client counterpart to the server-side gate in [`crate::auth`].
//! Both pieces are transport-agnostic — they operate on already-decoded
//! commands and a generic async byte stream — so they apply equally to the
//! Windows named-pipe client ([`crate::named_pipe::connect_pipe`]) and any
//! other transport, and the precedence/handshake logic is testable on any OS.
//!
//! Swift parity: `CLI/cmux.swift` `SocketPasswordResolver.resolve` and
//! `authenticateSocketClientIfNeeded`.

use std::io;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::{read_frame, write_frame, MAX_RPC_FRAME_BYTES};

/// Characters Swift's `CharacterSet.newlines` trims (U+000A–U+000D, U+0085,
/// U+2028, U+2029). Deliberately **not** spaces/tabs: a password file's trailing
/// `\n` is stripped, but a password with intentional leading/trailing spaces is
/// preserved.
const NEWLINE_CHARS: &[char] = &[
    '\u{000A}', '\u{000B}', '\u{000C}', '\u{000D}', '\u{0085}', '\u{2028}', '\u{2029}',
];

/// Trim surrounding newlines and collapse an empty result to `None`. Mirrors
/// Swift `SocketPasswordResolver.normalized`.
fn normalized(value: &str) -> Option<String> {
    let trimmed = value.trim_matches(|c| NEWLINE_CHARS.contains(&c));
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// The candidate password sources, in priority order. The composition root
/// reads each (CLI flag, `CMUX_SOCKET_PASSWORD` env var, password file, OS
/// keychain) and fills the matching field; absent sources stay `None`.
///
/// Named fields rather than positional args so a call site cannot silently
/// transpose two same-typed sources and invert precedence — the safety Swift's
/// `resolve(explicit:…)` argument labels gave for free.
#[derive(Debug, Default, Clone, Copy)]
pub struct PasswordSources<'a> {
    /// `--password <value>` on the command line.
    pub explicit: Option<&'a str>,
    /// The `CMUX_SOCKET_PASSWORD` environment variable.
    pub env: Option<&'a str>,
    /// The password file the app writes.
    pub file: Option<&'a str>,
    /// The OS keychain / credential store.
    pub keychain: Option<&'a str>,
}

/// Resolve the socket password from the candidate `sources` in priority order
/// (explicit → env → file → keychain). Each source is newline-normalized; the
/// first that survives normalization wins. A source that is absent, or present
/// but blank-after-trim, falls through to the next.
///
/// Mirrors `SocketPasswordResolver.resolve(explicit:socketPath:)`. The actual
/// reads are the composition root's job (see [`PasswordSources`]), keeping the
/// precedence policy pure and testable.
pub fn resolve_password(sources: PasswordSources<'_>) -> Option<String> {
    [sources.explicit, sources.env, sources.file, sources.keychain]
        .into_iter()
        .flatten()
        .find_map(normalized)
}

/// Perform the client auth handshake on a connection: send `auth <password>` as
/// the first frame and validate the reply. Call this only when a password
/// resolved (see [`resolve_password`]), before issuing any commands.
///
/// Mirrors `authenticateSocketClientIfNeeded`: an `ERROR:` reply fails the
/// handshake, **except** the legacy "this server has no auth" reply
/// (`Unknown command 'auth'`), which is tolerated so a password-bearing client
/// can still talk to a server that does not require one. A v2 server that does
/// not require auth answers the non-JSON `auth` line with a JSON parse error
/// (which does not start with `ERROR:`), so that path is tolerated too.
pub async fn authenticate_client<R, W>(
    reader: &mut R,
    writer: &mut W,
    password: &str,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    write_frame(writer, &format!("auth {password}")).await?;
    let frame = read_frame(reader, MAX_RPC_FRAME_BYTES)
        .await?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "no response to auth"))?;
    let response = String::from_utf8(frame)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "auth response was not UTF-8"))?;
    if response.starts_with("ERROR:") && !response.contains("Unknown command 'auth'") {
        return Err(io::Error::other(response));
    }
    Ok(())
}

/// The request id used by the v2 client. A single request is issued per
/// connection, so a fixed id suffices — the server echoes it and nothing
/// correlates on it.
const V2_REQUEST_ID: i64 = 1;

/// Build the one-line v2 request envelope (`{"id","method","params"}`) for
/// `method` + `params`. `serde_json` emits compact single-line JSON with any
/// embedded newlines escaped, so the result is a valid NDJSON frame body. This
/// is the client counterpart to [`crate::ControlResponseEncoder`].
pub fn build_v2_request(method: &str, params: &serde_json::Value) -> String {
    serde_json::json!({
        "id": V2_REQUEST_ID,
        "method": method,
        "params": params,
    })
    .to_string()
}

/// A v2 response that did not yield a result. `Display` is the exact
/// human-facing message; a CLI wraps it in its own error type for an exit code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum V2ResponseError {
    /// A plain-text `ERROR:` reply, surfaced verbatim (e.g. from a server that
    /// rejected before the JSON protocol started).
    #[error("{0}")]
    PlainText(String),
    /// The reply was not parseable JSON, or not a JSON object.
    #[error("Invalid v2 response: {0}")]
    Invalid(String),
    /// `{"ok":false}` carrying an `error` object: `<code>: <message>`.
    #[error("{code}: {message}")]
    Failed { code: String, message: String },
    /// `{"ok":false}` with no usable `error` object.
    #[error("v2 request failed")]
    Unspecified,
}

/// Interpret a v2 response line into the result value, or a [`V2ResponseError`].
///
/// Mirrors the macOS CLI's `sendV2` tail: a plain-text `ERROR:` reply is
/// surfaced verbatim; a non-object / unparseable reply is "Invalid v2
/// response"; `{"ok":true}` yields its `result` (an empty object if absent);
/// `{"ok":false}` with an `error` becomes `<code>: <message>`.
///
/// A non-object `result` is returned **as-is** rather than coerced to `{}` — the
/// caller (e.g. the `rpc` passthrough) prints whatever the method returned (the
/// Swift `[String: Any]` coercion is a type artifact, not intent).
pub fn interpret_v2_response(raw: &str) -> Result<serde_json::Value, V2ResponseError> {
    if raw.starts_with("ERROR:") {
        return Err(V2ResponseError::PlainText(raw.to_owned()));
    }

    // An unparseable reply and a non-object reply mean the same thing here.
    let invalid = || V2ResponseError::Invalid(raw.to_owned());
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
            .unwrap_or("error")
            .to_owned();
        let message = error
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Unknown v2 error")
            .to_owned();
        return Err(V2ResponseError::Failed { code, message });
    }

    Err(V2ResponseError::Unspecified)
}

/// Shell-quote a single v1 line token the way the macOS CLI's `shellQuote`
/// does (`CLI/cmux.swift:12004-12010`): a token made entirely of the safe set
/// `[A-Za-z0-9_@%+=:,./-]` is emitted verbatim; anything else (including the
/// empty string, spaces, and embedded quotes) is wrapped in POSIX single quotes
/// with each `'` rewritten as `'\''`, so the server tokenizes the line
/// shell-style and recovers the exact token.
fn shell_quote(value: &str) -> String {
    let is_safe = !value.is_empty()
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-')
        });
    if is_safe {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// Build the v1 command line for `command` + `args`: each token is
/// [`shell_quote`]d and joined with single spaces, matching the macOS CLI's
/// generic forwarder (`([command] + args).map(shellQuote).joined(separator:
/// " ")`, `CLI/cmux.swift:16294-16297`). The transport appends the trailing
/// `\n` (the NDJSON frame delimiter), so this returns the bare line.
///
/// Note: `command` is the *socket* command name, which the macOS CLI hardcodes
/// per handler (e.g. the CLI `list-windows` is sent as `list_windows`); there
/// is no universal CLI→socket transform, so the caller supplies the resolved
/// name. This is the v1 counterpart to [`build_v2_request`].
pub fn build_v1_command_line(command: &str, args: &[String]) -> String {
    std::iter::once(command)
        .chain(args.iter().map(String::as_str))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A v1 command that the server rejected: the reply began with `ERROR:`.
/// `Display` is the verbatim reply line (including the `ERROR:` prefix), exactly
/// as the macOS CLI surfaces it (`sendV1Command` throws `CLIError(message:
/// response)`, `CLI/cmux.swift:5765-5771`). A CLI wraps this for an exit code.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct V1ResponseError(pub String);

/// Interpret a v1 response line: a reply beginning with `ERROR:` is a failure
/// surfaced verbatim; anything else is the success body, returned as-is for the
/// caller to print. The transport has already stripped the single trailing
/// newline. This is the v1 counterpart to [`interpret_v2_response`]; unlike v2,
/// the success body is **not** parsed as JSON — v1 replies are opaque text the
/// command handler prints or reformats.
pub fn interpret_v1_response(raw: &str) -> Result<&str, V1ResponseError> {
    if raw.starts_with("ERROR:") {
        Err(V1ResponseError(raw.to_owned()))
    } else {
        Ok(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, AsyncWriteExt};

    #[test]
    fn explicit_password_wins() {
        assert_eq!(
            resolve_password(PasswordSources {
                explicit: Some("explicit"),
                env: Some("env"),
                file: Some("file"),
                keychain: Some("kc"),
            })
            .as_deref(),
            Some("explicit")
        );
    }

    #[test]
    fn falls_through_sources_in_priority_order() {
        assert_eq!(
            resolve_password(PasswordSources {
                env: Some("env"),
                file: Some("file"),
                ..Default::default()
            })
            .as_deref(),
            Some("env")
        );
        assert_eq!(
            resolve_password(PasswordSources {
                file: Some("file"),
                keychain: Some("kc"),
                ..Default::default()
            })
            .as_deref(),
            Some("file")
        );
        assert_eq!(
            resolve_password(PasswordSources {
                keychain: Some("kc"),
                ..Default::default()
            })
            .as_deref(),
            Some("kc")
        );
        assert_eq!(resolve_password(PasswordSources::default()), None);
    }

    #[test]
    fn blank_after_newline_trim_falls_through() {
        // An explicit value that is only newlines is treated as absent.
        assert_eq!(
            resolve_password(PasswordSources {
                explicit: Some("\n"),
                env: Some("env"),
                ..Default::default()
            })
            .as_deref(),
            Some("env")
        );
        assert_eq!(
            resolve_password(PasswordSources {
                explicit: Some("\r\n"),
                ..Default::default()
            }),
            None
        );
    }

    #[test]
    fn newline_trim_preserves_internal_and_space_padding() {
        // Trailing file newline stripped; intentional spaces kept; inner spaces kept.
        assert_eq!(
            resolve_password(PasswordSources {
                file: Some("p4ss word\n"),
                ..Default::default()
            })
            .as_deref(),
            Some("p4ss word")
        );
        assert_eq!(
            resolve_password(PasswordSources {
                explicit: Some("  spaced  "),
                ..Default::default()
            })
            .as_deref(),
            Some("  spaced  ")
        );
    }

    async fn run_handshake(server_reply: &'static str) -> io::Result<()> {
        let (mut client, mut server) = duplex(4096);
        // Fake server: read the auth line, send the canned reply.
        let server_task = tokio::spawn(async move {
            let _ = read_frame(&mut server, MAX_RPC_FRAME_BYTES).await;
            let _ = write_frame(&mut server, server_reply).await;
            server.shutdown().await.ok();
        });
        let (mut reader, mut writer) = tokio::io::split(&mut client);
        let result = authenticate_client(&mut reader, &mut writer, "pw").await;
        server_task.await.unwrap();
        result
    }

    #[tokio::test]
    async fn handshake_accepts_ok_reply() {
        assert!(run_handshake("OK: Authenticated").await.is_ok());
    }

    #[tokio::test]
    async fn handshake_tolerates_legacy_unknown_auth_command() {
        assert!(run_handshake("ERROR: Unknown command 'auth'").await.is_ok());
    }

    #[tokio::test]
    async fn handshake_tolerates_v2_parse_error_from_no_auth_server() {
        // A v2-only server answers the non-JSON `auth` line with a JSON error,
        // which does not start with "ERROR:" → not a handshake failure.
        assert!(run_handshake(r#"{"ok":false,"error":{"code":"parse_error"}}"#)
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn handshake_fails_on_invalid_password() {
        let error = run_handshake("ERROR: Invalid password").await.unwrap_err();
        assert!(error.to_string().contains("Invalid password"));
    }

    #[test]
    fn v2_request_envelope_shape() {
        let line = build_v2_request("surface.list", &serde_json::json!({"n": 2}));
        let value: serde_json::Value = serde_json::from_str(&line).expect("json");
        assert_eq!(value["id"], serde_json::json!(1));
        assert_eq!(value["method"], serde_json::json!("surface.list"));
        assert_eq!(value["params"], serde_json::json!({"n": 2}));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn v2_ok_response_yields_result() {
        let result = interpret_v2_response(r#"{"id":1,"ok":true,"result":{"pong":true}}"#).unwrap();
        assert_eq!(result, serde_json::json!({"pong": true}));
    }

    #[test]
    fn v2_ok_response_without_result_is_empty_object() {
        assert_eq!(
            interpret_v2_response(r#"{"ok":true}"#).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn v2_non_object_result_returned_as_is() {
        // Deliberate divergence from Swift's [String: Any] coercion to {}.
        assert_eq!(
            interpret_v2_response(r#"{"ok":true,"result":[1,2]}"#).unwrap(),
            serde_json::json!([1, 2])
        );
    }

    #[test]
    fn v2_error_response_formats_code_and_message() {
        let error =
            interpret_v2_response(r#"{"ok":false,"error":{"code":"auth_required","message":"need auth"}}"#)
                .unwrap_err();
        assert_eq!(error, V2ResponseError::Failed {
            code: "auth_required".to_owned(),
            message: "need auth".to_owned(),
        });
        assert_eq!(error.to_string(), "auth_required: need auth");
    }

    #[test]
    fn v2_plain_text_error_is_surfaced_verbatim() {
        let error = interpret_v2_response("ERROR: Access denied").unwrap_err();
        assert_eq!(error.to_string(), "ERROR: Access denied");
    }

    #[test]
    fn v2_unparseable_and_non_object_are_invalid() {
        assert_eq!(
            interpret_v2_response("not json").unwrap_err().to_string(),
            "Invalid v2 response: not json"
        );
        assert_eq!(
            interpret_v2_response("[1,2]").unwrap_err().to_string(),
            "Invalid v2 response: [1,2]"
        );
    }

    #[test]
    fn v2_ok_false_without_error_is_unspecified() {
        assert_eq!(
            interpret_v2_response(r#"{"ok":false}"#).unwrap_err(),
            V2ResponseError::Unspecified
        );
    }

    #[test]
    fn shell_quote_passes_safe_tokens_verbatim() {
        for safe in ["list_windows", "ws-1", "a.b/c", "user@host", "k=v", "100%", "a,b", "x:y", "_"] {
            assert_eq!(shell_quote(safe), safe, "{safe:?} should pass verbatim");
        }
    }

    #[test]
    fn shell_quote_wraps_unsafe_tokens_and_escapes_quotes() {
        // Space, empty, and embedded single quote all force single-quote wrapping.
        assert_eq!(shell_quote("echo hi"), "'echo hi'");
        assert_eq!(shell_quote(""), "''");
        // `'` becomes `'\''`: close-quote, escaped quote, reopen-quote.
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn build_v1_command_line_joins_quoted_tokens() {
        // Zero-arg command (e.g. the literal `list_windows`).
        assert_eq!(build_v1_command_line("list_windows", &[]), "list_windows");
        // Mixed safe + unsafe args are quoted independently.
        assert_eq!(
            build_v1_command_line("set_status", &["busy".to_owned(), "on a call".to_owned()]),
            "set_status busy 'on a call'",
        );
    }

    #[test]
    fn interpret_v1_response_passes_body_and_flags_error() {
        assert_eq!(interpret_v1_response("OK: 3 windows").unwrap(), "OK: 3 windows");
        // A bare success body is returned as-is (not JSON-parsed, unlike v2).
        assert_eq!(interpret_v1_response("[1,2]").unwrap(), "[1,2]");
        let error = interpret_v1_response("ERROR: no such window").unwrap_err();
        assert_eq!(error.to_string(), "ERROR: no such window");
    }
}
