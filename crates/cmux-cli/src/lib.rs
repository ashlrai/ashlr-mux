//! `cmux` CLI library: the invocation parser and socket/password resolution
//! foundation (M4 WS5). The `cmux` binary (`main.rs`) is a thin shell over this;
//! all parsing/resolution logic lives here so it is unit-testable.
//!
//! Pieces:
//! - [`invocation`]: argv parsing (global + presentation options) and [`CliError`].
//! - [`socket`]: `--socket` / `CMUX_SOCKET_PATH` / `CMUX_SOCKET` address resolution.
//! - [`password`]: socket-password source assembly over [`cmux_ipc::resolve_password`].
//!
//! User-facing socket commands are mapped through [`command_forward`], then the
//! Windows named-pipe transport performs the v2 control-socket round-trip.

pub mod classify;
pub mod command_forward;
pub mod config;
pub mod diff_viewer_cli;
pub mod dispatch;
pub mod docs;
pub mod feed_clear;
pub mod feed_hook;
pub mod feed_tui;
pub mod hooks_installer;
pub mod invocation;
pub mod password;
pub mod path_open;
pub mod rpc;
pub mod sessions;
pub mod settings;
pub mod socket;
pub mod ssh;
#[cfg(windows)]
pub mod transport;
pub mod welcome;
pub mod window_default_display;

pub use classify::{classify_command, ClassifyEnv, PreSocketAction};
pub use command_forward::{control_command_for, ControlCommand, CMUX_WORKSPACE_ID_ENV};
pub use dispatch::{plan, plan_with_args, DispatchPlan};
pub use invocation::{parse_global_options, CliError, GlobalOptions, ParseOutcome};
pub use password::{password_file_path, read_password_file};
pub use rpc::parse_rpc_params;
pub use socket::{
    resolve_socket_path, EnvView, SocketPathSource, SocketResolution,
    CONFLICTING_ENVIRONMENT_MESSAGE,
};
pub use ssh::{build_ssh_command_plan, SshCommandBuildOptions, SshCommandPlan, SSH_USAGE_TEXT};
