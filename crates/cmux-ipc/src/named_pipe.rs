//! Windows named-pipe transport for the control socket (M4).
//!
//! The macOS app listens on an `AF_UNIX` socket at a per-user path
//! (`SocketControlSettings.socketPath()` → `cmux-<uid>.sock` /
//! `cmux-debug-<tag>.sock`). Windows has no Unix-domain analogue for this, so
//! the port binds a **named pipe** at `\\.\pipe\cmux-<id>` instead. The wire
//! protocol above the transport is unchanged: each accepted connection is
//! handed to [`serve_connection`](crate::serve_connection) /
//! [`serve_connection_authenticated`](crate::serve_connection_authenticated),
//! so the NDJSON framing, the M1 codec, and the password auth gate all apply
//! identically.
//!
//! ## Security
//!
//! The pipe is created with an **explicit current-user DACL** rather than the
//! default named-pipe security descriptor. The default descriptor grants read
//! access to broader groups (and, depending on configuration, anonymous/network
//! callers) — unacceptable for a control channel that can drive the app. The
//! DACL here grants `GENERIC_ALL` to exactly the SID that owns the process and
//! nothing else (`D:P(A;;GA;;;<sid>)`, protected so it inherits nothing):
//! least privilege, and the same SID covers the user whether elevated or not.
//! SYSTEM and Administrators are intentionally omitted; the password auth gate
//! is the second layer on top.

#![cfg(windows)]

use std::ffi::c_void;
use std::io;
use std::mem::size_of;
use std::time::{Duration, Instant};

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE, HLOCAL,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use crate::{
    serve_connection, serve_connection_authenticated, ControlRequestHandler, PasswordAuthGate,
    PasswordVerifier,
};

/// The `\\.\pipe\` namespace prefix for local named pipes.
const PIPE_NAMESPACE: &str = r"\\.\pipe\";

/// Build the control-socket pipe path from a base name — the macOS socket
/// filename without its `.sock` suffix (e.g. `cmux-501`, `cmux-debug-mytag`).
/// Both the server (here) and the client must derive the name the same way,
/// mirroring how macOS shares `SocketControlSettings.socketPath()` between app
/// and CLI.
///
/// The base name is a controlled value the app derives, so an invalid one is a
/// programming error rather than something to paper over: a backslash (the
/// namespace separator) or an over-long name is **rejected**, not silently
/// rewritten. Silently mapping `a\b` → `a_b` (or truncating) could alias two
/// distinct identities onto the same pipe — a real hazard on a control channel
/// that can drive the app.
pub fn control_pipe_path(base_name: &str) -> io::Result<String> {
    // The full path "\\.\pipe\<name>" is bounded at 256 chars by CreateNamedPipe.
    const MAX_PIPE_PATH: usize = 256;
    if base_name.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pipe base name must not be empty",
        ));
    }
    if base_name.contains('\\') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pipe base name must not contain a backslash (the namespace separator)",
        ));
    }
    let path = format!("{PIPE_NAMESPACE}{base_name}");
    if path.len() > MAX_PIPE_PATH {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("pipe path exceeds the {MAX_PIPE_PATH}-character limit"),
        ));
    }
    Ok(path)
}

/// Connect a client to the control pipe at `addr`, retrying transient failures
/// until `timeout` elapses.
///
/// Two conditions are retried: `ERROR_PIPE_BUSY` (the server is up but all pipe
/// instances are momentarily in use — the canonical named-pipe client wait) and
/// `ERROR_FILE_NOT_FOUND` (the server's first instance is not listening *yet*,
/// e.g. the app is still launching). Any other error, or expiry of `timeout`,
/// returns the error so the caller can decide whether to launch the app or give
/// up. A zero `timeout` makes exactly one attempt.
pub async fn connect_pipe(addr: &str, timeout: Duration) -> io::Result<NamedPipeClient> {
    let options = ClientOptions::new();
    let deadline = Instant::now() + timeout;
    loop {
        match options.open(addr) {
            Ok(client) => return Ok(client),
            Err(error) => {
                let code = error.raw_os_error();
                let retryable = code == Some(ERROR_PIPE_BUSY.0 as i32)
                    || code == Some(ERROR_FILE_NOT_FOUND.0 as i32);
                if !retryable || Instant::now() >= deadline {
                    return Err(error);
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
}

/// The SDDL string for a DACL granting `GENERIC_ALL` to `sid` only, protected
/// from inheritance.
fn user_only_sddl(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
}

/// The string SID of the user that owns the current process (e.g.
/// `S-1-5-21-…`). Read from the process token; used to build the pipe DACL.
fn current_user_sid() -> io::Result<String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(win_err)?;
        let token = OwnedHandle(token);

        // First call sizes the buffer (fails with ERROR_INSUFFICIENT_BUFFER).
        let mut needed = 0u32;
        let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut needed);
        if needed == 0 {
            return Err(io::Error::other(
                "GetTokenInformation returned a zero-length TokenUser buffer",
            ));
        }
        let mut buffer = vec![0u8; needed as usize];
        GetTokenInformation(
            token.0,
            TokenUser,
            Some(buffer.as_mut_ptr() as *mut c_void),
            needed,
            &mut needed,
        )
        .map_err(win_err)?;

        let token_user = &*(buffer.as_ptr() as *const TOKEN_USER);
        let mut sid_string = PWSTR::null();
        ConvertSidToStringSidW(token_user.User.Sid, &mut sid_string).map_err(win_err)?;
        let result = sid_string.to_string().map_err(|error| {
            io::Error::new(io::ErrorKind::InvalidData, format!("invalid SID UTF-16: {error}"))
        });
        let _ = LocalFree(Some(HLOCAL(sid_string.0 as *mut c_void)));
        result
    }
}

/// A `SECURITY_ATTRIBUTES` wrapping a self-relative security descriptor built
/// from an SDDL string. Owns the `LocalAlloc`-ed descriptor and frees it on
/// drop. The struct must not move between [`Self::as_ptr`] and the
/// `CreateNamedPipe` call that consumes the pointer.
struct SecurityAttributes {
    attributes: SECURITY_ATTRIBUTES,
    descriptor: PSECURITY_DESCRIPTOR,
}

// SAFETY: the only non-`Send` members are the raw descriptor pointer and the
// `SECURITY_ATTRIBUTES` that points at it. Both reference a process-wide
// `LocalAlloc`-ed security descriptor that is valid from any thread and is
// owned solely by this value (single owner, freed once on drop). The accept
// loop holds it across `.await`, so the serve future must be `Send` to be
// spawned; moving the owner between threads is sound.
unsafe impl Send for SecurityAttributes {}

impl SecurityAttributes {
    /// Build attributes whose DACL grants the current user alone.
    fn current_user() -> io::Result<Self> {
        let sddl = user_only_sddl(&current_user_sid()?);
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();

        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wide.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .map_err(win_err)?;
        }

        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: false.into(),
        };
        Ok(Self {
            attributes,
            descriptor,
        })
    }

    /// Pointer to the `SECURITY_ATTRIBUTES` for `create_with_security_attributes_raw`.
    fn as_ptr(&self) -> *mut c_void {
        &self.attributes as *const SECURITY_ATTRIBUTES as *mut c_void
    }
}

impl Drop for SecurityAttributes {
    fn drop(&mut self) {
        if !self.descriptor.is_invalid() {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
            }
        }
    }
}

/// A process token (or other) handle closed on drop.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
}

/// Create one named-pipe server instance at `addr` with the current-user DACL.
/// `first` must be true for the very first instance so a foreign process cannot
/// have squatted the name (`FILE_FLAG_FIRST_PIPE_INSTANCE`).
fn create_instance(addr: &str, first: bool, security: &SecurityAttributes) -> io::Result<NamedPipeServer> {
    let mut options = ServerOptions::new();
    options.first_pipe_instance(first);
    // SAFETY: `security` outlives this call (it lives for the whole accept loop),
    // and `as_ptr` points at a valid `SECURITY_ATTRIBUTES` whose descriptor is a
    // valid self-relative SD owned by `security`.
    unsafe { options.create_with_security_attributes_raw(addr, security.as_ptr()) }
}

/// Serve the named pipe at `addr`, accepting connections forever and dispatching
/// each through a fresh handler from `make_handler`. No password auth — use
/// [`serve_named_pipe_authenticated`] when the access mode requires a password.
///
/// Runs until an unrecoverable accept error; each connection is served on its
/// own task so a slow or misbehaving client cannot block new connections.
pub async fn serve_named_pipe<F, H>(addr: &str, make_handler: F) -> io::Result<()>
where
    F: FnMut() -> H,
    H: ControlRequestHandler + Send + 'static,
{
    accept_loop(addr, make_handler, |reader, writer, handler| async move {
        let _ = serve_connection(reader, writer, handler).await;
    })
    .await
}

/// Like [`serve_named_pipe`] but gates every connection behind the password auth
/// handshake described in [`crate::auth`]. `gate` is cloned per connection (each
/// connection tracks its own `authenticated` state internally).
pub async fn serve_named_pipe_authenticated<F, H, V>(
    addr: &str,
    make_handler: F,
    gate: PasswordAuthGate<V>,
) -> io::Result<()>
where
    F: FnMut() -> H,
    H: ControlRequestHandler + Send + 'static,
    V: PasswordVerifier + Clone + Send + 'static,
{
    accept_loop(addr, make_handler, move |reader, writer, handler| {
        let gate = gate.clone();
        async move {
            let _ = serve_connection_authenticated(reader, writer, handler, gate).await;
        }
    })
    .await
}

/// The shared accept loop. `serve` consumes one connection's read/write halves
/// plus its handler and returns the future that drives it to completion.
async fn accept_loop<F, H, S, Fut>(addr: &str, mut make_handler: F, serve: S) -> io::Result<()>
where
    F: FnMut() -> H,
    H: ControlRequestHandler + Send + 'static,
    S: Fn(tokio::io::ReadHalf<NamedPipeServer>, tokio::io::WriteHalf<NamedPipeServer>, H) -> Fut
        + Send
        + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let security = SecurityAttributes::current_user()?;
    // One instance is always listening: we create the next instance *before*
    // serving the one that just connected, so there is no window where a client
    // finds no server.
    let mut server = create_instance(addr, true, &security)?;
    loop {
        server.connect().await?;
        let connected = server;
        server = create_instance(addr, false, &security)?;

        let handler = make_handler();
        let (reader, writer) = tokio::io::split(connected);
        tokio::spawn(serve(reader, writer, handler));
    }
}

/// Map a `windows::core::Error` into an `io::Error` preserving the OS code.
fn win_err(error: windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        authenticate_client, read_frame, write_frame, ControlCallResult, ControlRequest, JsonValue,
        MAX_RPC_FRAME_BYTES,
    };

    fn echo_handler(request: ControlRequest) -> ControlCallResult {
        ControlCallResult::Ok(JsonValue::String(request.method))
    }

    /// Per-test pipe name unique to this process (no collisions across the
    /// suite or parallel `cargo test` invocations).
    fn test_addr(tag: &str) -> String {
        control_pipe_path(&format!("cmux-ipc-test-{}-{tag}", std::process::id()))
            .expect("valid pipe name")
    }

    /// Open a client to `addr` via the production connector, allowing time for
    /// the first server instance to come up.
    async fn connect_client(addr: &str) -> NamedPipeClient {
        connect_pipe(addr, Duration::from_secs(2))
            .await
            .expect("connect")
    }

    #[test]
    fn pipe_path_prefixes_namespace_and_rejects_invalid_names() {
        assert_eq!(control_pipe_path("cmux-501").unwrap(), r"\\.\pipe\cmux-501");
        assert_eq!(
            control_pipe_path("cmux-debug-tag").unwrap(),
            r"\\.\pipe\cmux-debug-tag"
        );
        // A backslash would be read as a namespace break — reject rather than
        // silently alias `a\b` and `a_b` onto the same pipe.
        assert!(control_pipe_path(r"a\b").is_err());
        assert!(control_pipe_path("").is_err());
        assert!(control_pipe_path(&"x".repeat(300)).is_err());
    }

    #[test]
    fn user_only_sddl_grants_generic_all_to_just_the_sid() {
        assert_eq!(
            user_only_sddl("S-1-5-21-1-2-3-1001"),
            "D:P(A;;GA;;;S-1-5-21-1-2-3-1001)"
        );
    }

    #[test]
    fn current_user_sid_is_a_well_formed_sid() {
        let sid = current_user_sid().expect("current user sid");
        assert!(sid.starts_with("S-1-"), "unexpected SID form: {sid}");
    }

    #[test]
    fn current_user_security_attributes_build_and_free() {
        // Exercises the SDDL → security-descriptor → SECURITY_ATTRIBUTES path
        // (and the Drop that LocalFrees the descriptor).
        let security = SecurityAttributes::current_user().expect("security attributes");
        assert!(!security.descriptor.is_invalid());
        assert!(!security.as_ptr().is_null());
    }

    #[tokio::test]
    async fn named_pipe_round_trips_a_command() {
        let addr = test_addr("roundtrip");
        let server_addr = addr.clone();
        tokio::spawn(async move {
            let _ = serve_named_pipe(&server_addr, || echo_handler).await;
        });

        let mut client = connect_client(&addr).await;
        write_frame(&mut client, r#"{"id":1,"method":"ping"}"#)
            .await
            .expect("write");
        let frame = read_frame(&mut client, MAX_RPC_FRAME_BYTES)
            .await
            .expect("read")
            .expect("frame");
        let value: serde_json::Value = serde_json::from_slice(&frame).expect("json");
        assert_eq!(value["ok"], serde_json::json!(true));
        assert_eq!(value["result"], serde_json::json!("ping"));
    }

    #[derive(Clone)]
    struct OnePassword(&'static str);
    impl PasswordVerifier for OnePassword {
        fn has_configured_password(&self) -> bool {
            true
        }
        fn verify(&self, password: &str) -> bool {
            password == self.0
        }
    }

    /// Spawn an auth-gated echo server on a fresh pipe and connect a client.
    async fn spawn_authenticated_server(tag: &str) -> NamedPipeClient {
        let addr = test_addr(tag);
        let server_addr = addr.clone();
        tokio::spawn(async move {
            let gate = PasswordAuthGate::new(OnePassword("s3cret"));
            let _ = serve_named_pipe_authenticated(&server_addr, || echo_handler, gate).await;
        });
        connect_client(&addr).await
    }

    #[tokio::test]
    async fn client_handshake_then_command_succeeds() {
        let client = spawn_authenticated_server("client-ok").await;
        let (mut reader, mut writer) = tokio::io::split(client);
        authenticate_client(&mut reader, &mut writer, "s3cret")
            .await
            .expect("auth");

        write_frame(&mut writer, r#"{"id":1,"method":"ping"}"#)
            .await
            .expect("write");
        let frame = read_frame(&mut reader, MAX_RPC_FRAME_BYTES)
            .await
            .expect("read")
            .expect("frame");
        let value: serde_json::Value = serde_json::from_slice(&frame).expect("json");
        assert_eq!(value["result"], serde_json::json!("ping"));
    }

    #[tokio::test]
    async fn client_handshake_rejects_wrong_password() {
        let client = spawn_authenticated_server("client-bad").await;
        let (mut reader, mut writer) = tokio::io::split(client);
        let error = authenticate_client(&mut reader, &mut writer, "wrong")
            .await
            .unwrap_err();
        assert!(error.to_string().contains("Invalid password"));
    }

    #[tokio::test]
    async fn authenticated_named_pipe_requires_auth_then_admits() {
        let addr = test_addr("auth");
        let server_addr = addr.clone();
        tokio::spawn(async move {
            let gate = PasswordAuthGate::new(OnePassword("s3cret"));
            let _ = serve_named_pipe_authenticated(&server_addr, || echo_handler, gate).await;
        });

        let mut client = connect_client(&addr).await;
        // Before auth: rejected.
        write_frame(&mut client, r#"{"id":1,"method":"ping"}"#)
            .await
            .expect("write");
        let frame = read_frame(&mut client, MAX_RPC_FRAME_BYTES)
            .await
            .expect("read")
            .expect("frame");
        let value: serde_json::Value = serde_json::from_slice(&frame).expect("json");
        assert_eq!(value["error"]["code"], serde_json::json!("auth_required"));

        // Authenticate.
        write_frame(&mut client, "auth s3cret").await.expect("write");
        let frame = read_frame(&mut client, MAX_RPC_FRAME_BYTES)
            .await
            .expect("read")
            .expect("frame");
        assert_eq!(String::from_utf8(frame).unwrap(), "OK: Authenticated");

        // After auth: dispatched.
        write_frame(&mut client, r#"{"id":2,"method":"ping"}"#)
            .await
            .expect("write");
        let frame = read_frame(&mut client, MAX_RPC_FRAME_BYTES)
            .await
            .expect("read")
            .expect("frame");
        let value: serde_json::Value = serde_json::from_slice(&frame).expect("json");
        assert_eq!(value["result"], serde_json::json!("ping"));
    }
}
