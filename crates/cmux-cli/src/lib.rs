//! `cmux` CLI library: the invocation parser and socket/password resolution
//! foundation (M4 WS5). The `cmux` binary (`main.rs`) is a thin shell over this;
//! all parsing/resolution logic lives here so it is unit-testable.
//!
//! Pieces:
//! - [`invocation`]: argv parsing (global + presentation options) and [`CliError`].
//! - [`socket`]: `--socket` / `CMUX_SOCKET_PATH` / `CMUX_SOCKET` address resolution.
//! - [`password`]: socket-password source assembly over [`cmux_ipc::resolve_password`].
//!
//! The control-socket connect + command dispatch (`classify_command`, the
//! Windows named-pipe transport) build on top of this and are added next.

pub mod classify;
pub mod invocation;
pub mod password;
pub mod rpc;
pub mod socket;
#[cfg(windows)]
pub mod transport;

pub use classify::{classify_command, ClassifyEnv, PreSocketAction};
pub use invocation::{parse_global_options, CliError, GlobalOptions, ParseOutcome};
pub use password::{password_file_path, read_password_file};
pub use rpc::parse_rpc_params;
pub use socket::{
    resolve_socket_path, EnvView, SocketPathSource, SocketResolution,
    CONFLICTING_ENVIRONMENT_MESSAGE,
};
