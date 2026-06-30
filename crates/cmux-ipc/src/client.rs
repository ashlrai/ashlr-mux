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
}
