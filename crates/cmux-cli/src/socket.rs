//! Control-socket address resolution from `--socket` + environment (M4 WS5).
//!
//! Ports the precedence and conflict policy of `CLISocketPathResolver`
//! (`CLI/cmux.swift`): `--socket` overrides everything; otherwise
//! `CMUX_SOCKET_PATH` (canonical) then `CMUX_SOCKET` (deprecated alias); if
//! neither is set, a caller-supplied default. The env values are injected via
//! [`EnvView`] so the policy is pure and cross-platform-testable; the Windows
//! default pipe address is computed by the caller and passed in as
//! `default_path`.

use crate::invocation::CliError;

/// Exact conflict message (English of `cli.socket.error.conflictingEnvironment`)
/// emitted when `CMUX_SOCKET_PATH` and `CMUX_SOCKET` are both set and differ.
pub const CONFLICTING_ENVIRONMENT_MESSAGE: &str =
    "Refusing to choose socket: CMUX_SOCKET_PATH and CMUX_SOCKET differ. Use CMUX_SOCKET_PATH or unset CMUX_SOCKET.";

/// Where the resolved socket address came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketPathSource {
    /// From the `--socket` flag.
    ExplicitFlag,
    /// From `CMUX_SOCKET_PATH` / `CMUX_SOCKET`.
    Environment,
    /// The computed default (or an env value that equals it).
    ImplicitDefault,
}

/// A resolved socket address plus where it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SocketResolution {
    pub path: String,
    pub source: SocketPathSource,
}

/// The two socket-address environment variables, injected for testability.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvView<'a> {
    /// `CMUX_SOCKET_PATH` — canonical.
    pub socket_path: Option<&'a str>,
    /// `CMUX_SOCKET` — deprecated alias.
    pub socket: Option<&'a str>,
}

/// Resolve the socket address: `explicit` (`--socket`) wins verbatim; otherwise
/// the environment (with the conflict check); otherwise `default_path`.
///
/// An explicit `--socket` value is used as-is and **suppresses the env conflict
/// entirely** (the conflict is only checked when no `--socket` is given), so a
/// caller that names a socket is never blocked by ambiguous ambient env — exact
/// parity with the Swift resolver, which reads the env only for telemetry when
/// `--socket` is present.
///
/// `default_path` must be a non-empty, already-validated address: env values
/// are normalized away when blank, but the default is used verbatim, so a blank
/// default would resolve to an empty address.
pub fn resolve_socket_path(
    explicit: Option<&str>,
    env: EnvView<'_>,
    default_path: &str,
) -> Result<SocketResolution, CliError> {
    debug_assert!(
        !default_path.is_empty(),
        "default_path must be a non-empty socket address"
    );

    if let Some(path) = explicit {
        return Ok(SocketResolution {
            path: path.to_owned(),
            source: SocketPathSource::ExplicitFlag,
        });
    }

    // An env value distinct from the default is Environment-sourced; otherwise
    // (env equals default, or no env at all) fall through to the single default
    // construction below as ImplicitDefault.
    if let Some(path) = env_socket_path(env)? {
        if path != default_path {
            return Ok(SocketResolution {
                path,
                source: SocketPathSource::Environment,
            });
        }
    }

    Ok(SocketResolution {
        path: default_path.to_owned(),
        source: SocketPathSource::ImplicitDefault,
    })
}

/// The env-derived socket address: `CMUX_SOCKET_PATH` if set, else `CMUX_SOCKET`.
/// Errors if both are set (after normalization) and differ.
///
/// The conflict comparison is an **exact, case-sensitive string inequality with
/// no path normalization** — `/tmp/x` and `/private/tmp/x` conflict — matching
/// Swift. (Note: Windows pipe names are case-insensitive at the OS level, but
/// the conflict is a guard against *contradictory configuration*, not a test of
/// path equivalence, so the exact-string comparison is intentional.)
fn env_socket_path(env: EnvView<'_>) -> Result<Option<String>, CliError> {
    let socket_path = normalize_env(env.socket_path);
    let socket = normalize_env(env.socket);
    if let (Some(path), Some(alias)) = (socket_path, socket) {
        if path != alias {
            return Err(CliError::new(CONFLICTING_ENVIRONMENT_MESSAGE));
        }
    }
    // Borrow until here; only the surviving value is allocated (the conflict
    // path allocates nothing).
    Ok(socket_path.or(socket).map(str::to_owned))
}

/// Trim surrounding whitespace (incl. newlines); a blank result becomes `None`,
/// so an empty or whitespace-only env var never participates in the conflict and
/// falls through to the next source. Borrows from the input — no allocation.
fn normalize_env(value: Option<&str>) -> Option<&str> {
    let trimmed = value?.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULT: &str = "\\\\.\\pipe\\cmux";

    #[test]
    fn explicit_flag_wins_and_suppresses_env_conflict() {
        // Conflicting env vars present, but --socket is given → no error, used verbatim.
        let resolution = resolve_socket_path(
            Some("\\\\.\\pipe\\chosen"),
            EnvView {
                socket_path: Some("a"),
                socket: Some("b"),
            },
            DEFAULT,
        )
        .expect("explicit suppresses conflict");
        assert_eq!(resolution.path, "\\\\.\\pipe\\chosen");
        assert_eq!(resolution.source, SocketPathSource::ExplicitFlag);
    }

    #[test]
    fn env_conflict_errors_with_exact_message_and_exit_1() {
        let error = resolve_socket_path(
            None,
            EnvView {
                socket_path: Some("/tmp/a"),
                socket: Some("/tmp/b"),
            },
            DEFAULT,
        )
        .unwrap_err();
        assert_eq!(error.exit_code, 1);
        assert_eq!(error.message, CONFLICTING_ENVIRONMENT_MESSAGE);
    }

    #[test]
    fn identical_env_vars_do_not_conflict() {
        let resolution = resolve_socket_path(
            None,
            EnvView {
                socket_path: Some("/tmp/same"),
                socket: Some("/tmp/same"),
            },
            DEFAULT,
        )
        .expect("identical values are fine");
        assert_eq!(resolution.path, "/tmp/same");
        assert_eq!(resolution.source, SocketPathSource::Environment);
    }

    #[test]
    fn conflict_uses_exact_string_not_path_equivalence() {
        // /tmp/x vs /private/tmp/x are the same file on macOS but conflict here.
        let error = resolve_socket_path(
            None,
            EnvView {
                socket_path: Some("/tmp/x"),
                socket: Some("/private/tmp/x"),
            },
            DEFAULT,
        )
        .unwrap_err();
        assert_eq!(error.message, CONFLICTING_ENVIRONMENT_MESSAGE);
    }

    #[test]
    fn blank_socket_path_falls_through_to_alias() {
        // Empty/whitespace CMUX_SOCKET_PATH does not conflict and falls through.
        let resolution = resolve_socket_path(
            None,
            EnvView {
                socket_path: Some("   "),
                socket: Some("/tmp/alias"),
            },
            DEFAULT,
        )
        .expect("blank path falls through");
        assert_eq!(resolution.path, "/tmp/alias");
        assert_eq!(resolution.source, SocketPathSource::Environment);
    }

    #[test]
    fn canonical_socket_path_takes_priority_over_alias() {
        let resolution = resolve_socket_path(
            None,
            EnvView {
                socket_path: Some("/tmp/canonical"),
                socket: None,
            },
            DEFAULT,
        )
        .unwrap();
        assert_eq!(resolution.path, "/tmp/canonical");
    }

    #[test]
    fn nothing_set_uses_default_as_implicit() {
        let resolution = resolve_socket_path(None, EnvView::default(), DEFAULT).unwrap();
        assert_eq!(resolution.path, DEFAULT);
        assert_eq!(resolution.source, SocketPathSource::ImplicitDefault);
    }

    #[test]
    fn env_equal_to_default_is_classified_implicit() {
        let resolution = resolve_socket_path(
            None,
            EnvView {
                socket_path: Some(DEFAULT),
                socket: None,
            },
            DEFAULT,
        )
        .unwrap();
        assert_eq!(resolution.source, SocketPathSource::ImplicitDefault);
    }
}
