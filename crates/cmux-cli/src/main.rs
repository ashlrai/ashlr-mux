//! `cmux` CLI entry point. A thin shell over [`cmux_cli`]: parse the
//! invocation, classify the command into its pre-socket action, and execute the
//! resulting [`DispatchPlan`] — printing version/help, running the `rpc`
//! control-socket round-trip, or failing with the correct exit code.

use std::path::Path;
use std::process::ExitCode;

use cmux_cli::{
    classify_command, parse_global_options, plan, ClassifyEnv, CliError, DispatchPlan,
    GlobalOptions, ParseOutcome,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {}", error.message);
            ExitCode::from(error.exit_code.clamp(0, 255) as u8)
        }
    }
}

fn run(args: &[String]) -> Result<(), CliError> {
    match parse_global_options(args)? {
        ParseOutcome::PrintVersion => {
            print_version();
            Ok(())
        }
        ParseOutcome::PrintHelp => {
            print_top_level_help();
            Ok(())
        }
        ParseOutcome::Command {
            options,
            command,
            command_args,
        } => dispatch(&options, &command, &command_args),
    }
}

/// Classify the command into its pre-socket action, then execute the resulting
/// [`DispatchPlan`]. Classification and planning are pure (and unit-tested in
/// the library); this function is the thin I/O executor — it reads the current
/// directory and path existence for path-open classification, prints, or hands
/// off to the `rpc` round-trip.
fn dispatch(options: &GlobalOptions, command: &str, command_args: &[String]) -> Result<(), CliError> {
    let cwd = std::env::current_dir().unwrap_or_default();
    let path_exists = |path: &Path| path.exists();
    let env = ClassifyEnv {
        cwd: &cwd,
        path_exists: &path_exists,
    };
    let action = classify_command(
        command,
        command_args,
        options.explicit_socket_path.as_deref(),
        &env,
    );

    match plan(&action, command) {
        DispatchPlan::PrintVersion => {
            print_version();
            Ok(())
        }
        DispatchPlan::PrintTopLevelHelp => {
            print_top_level_help();
            Ok(())
        }
        DispatchPlan::PrintLine(line) => {
            println!("{line}");
            Ok(())
        }
        DispatchPlan::RunRpc => run_rpc_command(options, command_args),
        DispatchPlan::Fail(error) => Err(error),
    }
}

/// Print the version summary to stdout. The standalone Rust CLI reports the
/// crate version; the macOS CLI's bundle/commit suffix has no analogue here yet.
fn print_version() {
    println!("cmux {}", env!("CARGO_PKG_VERSION"));
}

/// Print the top-level help to stdout. The full macOS `usage()` block (the
/// 150-command listing, with its macOS-specific paths) is a later, platform-
/// adapted slice; for now this is the one-line synopsis.
fn print_top_level_help() {
    println!("Usage: cmux <path>|<command> [options]");
}

/// `cmux rpc <method> [json-params]` — resolve the socket address and password
/// from flags + environment, then round-trip a v2 request over the control pipe
/// and print the result. The address/password resolution and the transport are
/// each unit-tested; this glue only reads the ambient env.
#[cfg(windows)]
fn run_rpc_command(options: &GlobalOptions, command_args: &[String]) -> Result<(), CliError> {
    let method = command_args
        .first()
        .map(|arg| arg.trim())
        .filter(|arg| !arg.is_empty())
        .ok_or_else(|| CliError::new("Usage: cmux rpc <method> [json-params]"))?;
    let params = cmux_cli::parse_rpc_params(&command_args[1..])?;

    let default_addr = cmux_ipc::control_pipe_path("cmux")
        .map_err(|error| CliError::new(format!("invalid default socket name: {error}")))?;
    let env_socket_path = std::env::var("CMUX_SOCKET_PATH").ok();
    let env_socket = std::env::var("CMUX_SOCKET").ok();
    let resolution = cmux_cli::resolve_socket_path(
        options.explicit_socket_path.as_deref(),
        cmux_cli::EnvView {
            socket_path: env_socket_path.as_deref(),
            socket: env_socket.as_deref(),
        },
        &default_addr,
    )?;

    let env_password = std::env::var("CMUX_SOCKET_PASSWORD").ok();
    let local_app_data = std::env::var("LOCALAPPDATA").ok();
    let file_password = cmux_cli::read_password_file(local_app_data.as_deref());
    let password = cmux_ipc::resolve_password(cmux_ipc::PasswordSources {
        explicit: options.socket_password.as_deref(),
        env: env_password.as_deref(),
        file: file_password.as_deref(),
        keychain: None,
    });

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| CliError::new(format!("failed to start async runtime: {error}")))?;
    let result = runtime.block_on(cmux_cli::transport::run_rpc(
        &resolution.path,
        password.as_deref(),
        method,
        &params,
    ))?;
    println!("{}", serde_json::to_string(&result).unwrap_or_default());
    Ok(())
}

/// On non-Windows targets the named-pipe transport is unavailable, so socket
/// commands cannot run. (The CLI is only shipped on Windows for this port; this
/// keeps the crate buildable on other CI targets.)
#[cfg(not(windows))]
fn run_rpc_command(_options: &GlobalOptions, _command_args: &[String]) -> Result<(), CliError> {
    Err(CliError::new(
        "socket commands are only supported on Windows in this build",
    ))
}
