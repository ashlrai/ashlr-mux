//! Pure routing of a classified command to its execution plan (M4 WS5).
//!
//! [`classify_command`](crate::classify::classify_command) decides *what kind*
//! of pre-socket action a command warrants; [`plan`] decides *how the binary
//! should carry it out* — print version, print help, print a one-shot line,
//! run the `rpc` round-trip, or fail with an exit code. Keeping this mapping
//! pure (no I/O, no `println!`, no socket connect) is what makes the routing
//! unit-testable; `main.rs` is the thin executor that performs the I/O each
//! [`DispatchPlan`] describes.
//!
//! ## Port scope (deliberate, see also `DECISIONS.md`)
//!
//! Only the no-socket actions whose behavior is fully determined and
//! platform-independent are executed today: `version`, the bare/unknown/
//! subcommand help renders, and the `rpc` control-socket round-trip (the one
//! command with a complete v2 codec). Every other action is mapped to
//! [`DispatchPlan::Fail`] with a clear "not yet available" message rather than a
//! guessed behavior, because:
//!
//! - The **generic socket command forward** (Swift's v1 line protocol) maps each
//!   command to a bespoke *socket* command name and argument shape (e.g. the CLI
//!   `list-windows` is sent as `list_windows`); there is no universal rule, so a
//!   generic forward would send wrong frames. It waits on a server-contract map.
//! - The **side-effecting no-socket commands** (`docs`, `welcome`, `sessions`,
//!   `settings`/`config` docs, the sigpipe/diff-viewer probes, `open <path>`,
//!   …) each need a subsystem that is not part of the headless core yet.

use crate::classify::PreSocketAction;
use crate::invocation::CliError;

/// How `main` should carry out a classified command. Pure data — the executor
/// turns each variant into stdout/stderr + an exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchPlan {
    /// Print the version summary to stdout and exit 0.
    PrintVersion,
    /// Print the top-level help to stdout and exit 0.
    PrintTopLevelHelp,
    /// Print this exact line to stdout and exit 0 (unknown-command / subcommand
    /// help). The string carries no trailing newline; the executor's `println!`
    /// supplies it, matching Swift's `print`.
    PrintLine(String),
    /// Run the `rpc` control-socket round-trip (reads socket/password from the
    /// ambient options + environment in the executor).
    RunRpc,
    /// Abort with this error and its exit code.
    Fail(CliError),
}

/// The bare-`help` / unknown-command pointer line, verbatim from Swift
/// `CLI/cmux.swift:3157`.
pub fn unknown_command_message(command: &str) -> String {
    format!("Unknown command '{command}'. Run 'cmux help' to see available commands.")
}

/// The subcommand-help render: Swift `dispatchSubcommandHelp` prints
/// `cmux <command>` then a blank line then the per-command usage text. The usage
/// bodies (the ~1989-line `subcommandUsage` switch) are a later slice, so this
/// stopgap prints the faithful header and points at the command list. Exit 0.
pub fn subcommand_help_text(command: &str) -> String {
    format!(
        "cmux {command}\n\n(detailed usage for '{command}' is not yet ported; \
         run 'cmux help' for the command list)"
    )
}

/// A uniform "this action is not part of the Windows headless port yet" failure
/// (exit 1). `label` is the user-facing command spelling.
fn not_yet_ported(label: &str) -> CliError {
    CliError::new(format!(
        "'{label}' is not yet available in the Windows port"
    ))
}

/// The not-yet-ported failure for a socket-backed command other than `rpc`. Kept
/// distinct from [`not_yet_ported`] so the message names the one path that does
/// work today.
fn socket_command_not_ported(command: &str) -> CliError {
    CliError::new(format!(
        "socket command '{command}' is not yet ported (M4 WS5); \
         only 'rpc' is wired to the control socket so far"
    ))
}

/// Map a classified `action` (for `command`) to the executor's plan. `command`
/// is needed because [`PreSocketAction::NeedsSocket`] does not carry the command
/// name, and only `rpc` has a working socket path today.
pub fn plan(action: &PreSocketAction, command: &str) -> DispatchPlan {
    // The wired paths return directly; every remaining variant is a no-socket
    // command whose subsystem is not ported yet, so it falls through to a single
    // `Fail(not_yet_ported(label))` with its own user-facing label.
    let label = match action {
        PreSocketAction::BareVersion => return DispatchPlan::PrintVersion,
        PreSocketAction::Help => return DispatchPlan::PrintTopLevelHelp,
        PreSocketAction::UnknownCommandHelp { command } => {
            return DispatchPlan::PrintLine(unknown_command_message(command))
        }
        PreSocketAction::SubcommandHelp { command } => {
            return DispatchPlan::PrintLine(subcommand_help_text(command))
        }
        PreSocketAction::NeedsSocket => {
            return if command == "rpc" {
                DispatchPlan::RunRpc
            } else {
                DispatchPlan::Fail(socket_command_not_ported(command))
            }
        }
        PreSocketAction::RemoteDaemonStatus => "remote-daemon-status",
        PreSocketAction::VmPtyConnect => "vm-pty-connect",
        PreSocketAction::Docs => "docs",
        PreSocketAction::Welcome => "welcome",
        PreSocketAction::Sessions { debug } => {
            if *debug {
                "session-debug"
            } else {
                "sessions"
            }
        }
        PreSocketAction::SigpipeProbe => "__sigpipe-probe",
        PreSocketAction::SigpipeStdinPipeProbe => "__sigpipe-stdin-pipe-probe",
        PreSocketAction::SigpipeInspect => "__sigpipe-inspect",
        PreSocketAction::DiffViewerServer => "diff-viewer-server",
        PreSocketAction::DiffViewerRefs => "__diff-viewer-refs",
        PreSocketAction::DiffViewerBranch => "__diff-viewer-branch",
        PreSocketAction::SettingsNoSocket => "settings",
        PreSocketAction::WindowDefaultDisplay => "window default-display",
        PreSocketAction::ConfigNoSocket => "config",
        PreSocketAction::OpenPath { .. } => "open",
    };
    DispatchPlan::Fail(not_yet_ported(label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_help_actions_map_to_print_plans() {
        assert_eq!(plan(&PreSocketAction::BareVersion, "version"), DispatchPlan::PrintVersion);
        assert_eq!(plan(&PreSocketAction::Help, "help"), DispatchPlan::PrintTopLevelHelp);
    }

    #[test]
    fn unknown_command_help_prints_exact_pointer_line() {
        let plan = plan(
            &PreSocketAction::UnknownCommandHelp { command: "bogus".to_owned() },
            "bogus",
        );
        assert_eq!(
            plan,
            DispatchPlan::PrintLine(
                "Unknown command 'bogus'. Run 'cmux help' to see available commands.".to_owned()
            )
        );
    }

    #[test]
    fn subcommand_help_prints_header_and_pointer() {
        let plan = plan(
            &PreSocketAction::SubcommandHelp { command: "send".to_owned() },
            "send",
        );
        match plan {
            DispatchPlan::PrintLine(text) => {
                assert!(text.starts_with("cmux send\n\n"), "got: {text:?}");
                assert!(text.contains("run 'cmux help'"));
            }
            other => panic!("expected PrintLine, got {other:?}"),
        }
    }

    #[test]
    fn rpc_is_the_only_socket_command_that_runs() {
        assert_eq!(plan(&PreSocketAction::NeedsSocket, "rpc"), DispatchPlan::RunRpc);
        match plan(&PreSocketAction::NeedsSocket, "list-workspaces") {
            DispatchPlan::Fail(error) => {
                assert_eq!(error.exit_code, 1);
                assert!(error.message.contains("list-workspaces"));
                assert!(error.message.contains("only 'rpc'"));
            }
            other => panic!("expected Fail, got {other:?}"),
        }
    }

    #[test]
    fn side_effecting_no_socket_actions_fail_with_not_ported() {
        for (action, needle) in [
            (PreSocketAction::Docs, "docs"),
            (PreSocketAction::Welcome, "welcome"),
            (PreSocketAction::Sessions { debug: false }, "sessions"),
            (PreSocketAction::Sessions { debug: true }, "session-debug"),
            (PreSocketAction::SettingsNoSocket, "settings"),
            (PreSocketAction::ConfigNoSocket, "config"),
            (PreSocketAction::WindowDefaultDisplay, "window default-display"),
            (PreSocketAction::OpenPath { path: "p".to_owned() }, "open"),
        ] {
            match plan(&action, "x") {
                DispatchPlan::Fail(error) => {
                    assert_eq!(error.exit_code, 1, "{action:?}");
                    assert!(error.message.contains(needle), "{action:?}: {}", error.message);
                    assert!(error.message.contains("not yet available"), "{action:?}");
                }
                other => panic!("expected Fail for {action:?}, got {other:?}"),
            }
        }
    }
}
