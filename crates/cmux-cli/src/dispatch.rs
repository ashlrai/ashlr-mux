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
//! Actions with a complete local or v2-control contract are executed today:
//! `version`, the bare/unknown/subcommand help renders, raw `rpc`, and the
//! user-facing commands mapped by [`crate::command_forward`]. Remaining actions
//! are mapped to [`DispatchPlan::Fail`] with a clear "not yet available" message
//! rather than a guessed behavior, because:
//!
//! - The remaining **generic socket command forward** surface still needs
//!   explicit server-contract entries for each command with bespoke argument
//!   shapes. Unmapped commands do not send guessed frames.
//! - The **side-effecting no-socket commands** (`docs`, `welcome`, `sessions`,
//!   `settings`/`config` docs, the sigpipe/diff-viewer probes, `open <path>`,
//!   …) each need a subsystem that is not part of the headless core yet.

use crate::classify::PreSocketAction;
use crate::command_forward::{control_command_for, ControlCommand};
use crate::invocation::CliError;
use crate::ssh::SSH_USAGE_TEXT;

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
    /// Run one mapped user-facing command through the v2 control socket.
    RunControl(ControlCommand),
    /// Stream reconnectable event frames from the v2 control socket.
    RunEvents(Vec<String>),
    /// Run the multi-step SSH workspace bootstrap/control flow.
    RunSsh(Vec<String>),
    /// Run hidden local git-ref discovery for the diff-viewer branch picker.
    RunDiffViewerRefs(Vec<String>),
    /// Regenerate a branch-base diff page and manifest entries.
    RunDiffViewerBranch(Vec<String>),
    /// Run a local hooks installer/uninstaller command.
    RunHooksInstaller { command: String, args: Vec<String> },
    /// Abort with this error and its exit code.
    Fail(CliError),
}

/// The bare-`help` / unknown-command pointer line, verbatim from Swift
/// `CLI/cmux.swift:3157`.
pub fn unknown_command_message(command: &str) -> String {
    format!("Unknown command '{command}'. Run 'cmux help' to see available commands.")
}

/// The subcommand-help render: Swift `dispatchSubcommandHelp` prints
/// `cmux <command>` then a blank line then the per-command usage text. Commands
/// with complete v2-control routes get concrete Windows-port usage text; the
/// larger legacy surface keeps the faithful header and points at the list.
pub fn subcommand_help_text(command: &str) -> String {
    match mapped_subcommand_usage(command) {
        Some(usage) => format!("cmux {command}\n\n{usage}"),
        None => format!(
            "cmux {command}\n\n(detailed usage for '{command}' is not yet ported; \
             run 'cmux help' for the command list)"
        ),
    }
}

fn mapped_subcommand_usage(command: &str) -> Option<&'static str> {
    match command {
        "ping" => Some("Usage:\n  cmux ping\n\nSends a ping to the control socket."),
        "capabilities" => Some(
            "Usage:\n  cmux capabilities\n\nPrints the control socket's supported methods and platform metadata as JSON.",
        ),
        "identify" => Some(
            "Usage:\n  cmux identify\n\nPrints desktop/control-socket identity metadata.",
        ),
        "list-workspaces" => Some(
            "Usage:\n  cmux list-workspaces\n\nLists workspaces from the active desktop session.",
        ),
        "current-workspace" => Some(
            "Usage:\n  cmux current-workspace\n\nPrints the selected workspace.",
        ),
        "new-workspace" => Some(
            "Usage:\n  cmux new-workspace [--cwd PATH] [--command CMD] [--input TEXT] [--env KEY=VALUE]\n\nCreates and selects a terminal workspace.",
        ),
        "new-browser-workspace" => Some(
            "Usage:\n  cmux new-browser-workspace [URL|--url URL]\n\nCreates and selects a browser workspace.",
        ),
        "restore-previous-launch" => Some(
            "Usage:\n  cmux restore-previous-launch\n\nRestores the previous launch's saved desktop session.",
        ),
        "restore-session" => Some(
            "Usage:\n  cmux restore-session\n\nReopens the previously saved cmux session.",
        ),
        "close-workspace" => Some(
            "Usage:\n  cmux close-workspace WORKSPACE\n\nCloses the workspace identified by workspace:N ref or workspace id.",
        ),
        "close-workspaces" => Some(
            "Usage:\n  cmux close-workspaces WORKSPACE...\n\nCloses one or more workspaces by workspace:N ref or workspace id. Bare numbers are normalized to workspace:N refs.",
        ),
        "select-workspace" => Some(
            "Usage:\n  cmux select-workspace WORKSPACE\n\nSelects a workspace by workspace:N ref or workspace id. Bare numbers are normalized to workspace:N refs.",
        ),
        "rename-workspace" => Some(
            "Usage:\n  cmux rename-workspace [WORKSPACE] TITLE\n\nSets or clears a workspace title.",
        ),
        "rename-window" => Some(
            "Usage:\n  cmux rename-window [WORKSPACE] TITLE\n\nCompatibility alias for `cmux rename-workspace`.",
        ),
        "workspace" => Some(
            "Usage:\n  cmux workspace [list|current|new|close WORKSPACE|select|rename|set-progress|clear-progress|set-status|set-agent-pid|clear-agent-pid|report-pr|report-review|report-meta|report-meta-block|log|sidebar-state|next|previous|pin|unpin|mark-read|mark-unread]\n\nRuns a workspace control command through the desktop control socket.",
        ),
        "set-progress" => Some(
            "Usage:\n  cmux set-progress VALUE [--workspace WORKSPACE] [--label LABEL]\n\nSets sidebar progress for a workspace.",
        ),
        "clear-progress" => Some(
            "Usage:\n  cmux clear-progress [--workspace WORKSPACE]\n\nClears sidebar progress for a workspace.",
        ),
        "set-status" => Some(
            "Usage:\n  cmux set-status KEY VALUE [--workspace WORKSPACE] [--priority N]\n\nSets a sidebar status pill for a workspace.",
        ),
        "clear-status" => Some(
            "Usage:\n  cmux clear-status KEY [--workspace WORKSPACE]\n\nClears a sidebar status pill for a workspace.",
        ),
        "list-status" => Some(
            "Usage:\n  cmux list-status [--workspace WORKSPACE]\n\nLists sidebar status pills for a workspace.",
        ),
        "set-agent-pid" => Some(
            "Usage:\n  cmux set-agent-pid KEY PID [--workspace WORKSPACE]\n\nRegisters an agent root PID so descendant listening ports can appear in the sidebar.",
        ),
        "clear-agent-pid" => Some(
            "Usage:\n  cmux clear-agent-pid KEY [--workspace WORKSPACE]\n\nClears a registered agent root PID and refreshes sidebar agent ports.",
        ),
        "report-tty" => Some(
            "Usage:\n  cmux report-tty TTY [--workspace WORKSPACE] [--panel SURFACE]\n\nReports a terminal TTY for a workspace surface.",
        ),
        "report-shell-state" => Some(
            "Usage:\n  cmux report-shell-state <prompt|running|unknown> [--workspace WORKSPACE] [--panel SURFACE]\n\nReports whether a surface shell is idle at a prompt or running a command.",
        ),
        "report-pr" => Some(
            "Usage:\n  cmux report-pr NUMBER URL [--workspace WORKSPACE] [--panel SURFACE] [--label LABEL] [--state open|merged|closed] [--branch BRANCH] [--stale]\n\nReports pull-request metadata for sidebar display.",
        ),
        "report-review" => Some(
            "Usage:\n  cmux report-review NUMBER URL [--workspace WORKSPACE] [--panel SURFACE] [--label LABEL] [--state open|merged|closed] [--branch BRANCH] [--stale]\n\nReports provider-specific review metadata for sidebar display.",
        ),
        "clear-pr" => Some(
            "Usage:\n  cmux clear-pr [--workspace WORKSPACE] [--panel SURFACE]\n\nClears pull-request metadata for a sidebar surface.",
        ),
        "report-meta" | "set-meta" => Some(
            "Usage:\n  cmux report-meta KEY VALUE [--workspace WORKSPACE] [--icon ICON] [--color COLOR] [--url URL] [--format plain|markdown] [--priority N]\n\nSets a rich sidebar metadata entry for a workspace.",
        ),
        "clear-meta" => Some(
            "Usage:\n  cmux clear-meta KEY [--workspace WORKSPACE]\n\nClears a rich sidebar metadata entry for a workspace.",
        ),
        "list-meta" => Some(
            "Usage:\n  cmux list-meta [--workspace WORKSPACE]\n\nLists rich sidebar metadata entries for a workspace.",
        ),
        "report-meta-block" | "set-meta-block" => Some(
            "Usage:\n  cmux report-meta-block KEY [--workspace WORKSPACE] [--priority N] -- MARKDOWN\n\nSets a freeform sidebar markdown metadata block for a workspace.",
        ),
        "clear-meta-block" => Some(
            "Usage:\n  cmux clear-meta-block KEY [--workspace WORKSPACE]\n\nClears a freeform sidebar markdown metadata block for a workspace.",
        ),
        "list-meta-blocks" => Some(
            "Usage:\n  cmux list-meta-blocks [--workspace WORKSPACE]\n\nLists freeform sidebar markdown metadata blocks for a workspace.",
        ),
        "reset-sidebar" => Some(
            "Usage:\n  cmux reset-sidebar [--workspace WORKSPACE]\n\nClears sidebar status, metadata, progress, and log state for a workspace.",
        ),
        "log" => Some(
            "Usage:\n  cmux log [--workspace WORKSPACE] [--level LEVEL] -- MESSAGE\n\nAppends a sidebar log entry for a workspace.",
        ),
        "clear-log" => Some(
            "Usage:\n  cmux clear-log [--workspace WORKSPACE]\n\nClears sidebar log entries for a workspace.",
        ),
        "list-log" => Some(
            "Usage:\n  cmux list-log [--workspace WORKSPACE] [--limit N]\n\nLists recent sidebar log entries for a workspace.",
        ),
        "sidebar-state" => Some(
            "Usage:\n  cmux sidebar-state [--workspace WORKSPACE]\n\nPrints sidebar state for a workspace.",
        ),
        "sidebar-snapshot" | "extension-sidebar-snapshot" => Some(
            "Usage:\n  cmux sidebar-snapshot\n\nPrints the rich extension sidebar snapshot used by custom sidebars and event-stream catch-up.",
        ),
        "sidebar" => Some(
            "Usage:\n  cmux sidebar [list|validate [name]|reload [name]|select <name>|open <name>]\n\nValidates and manages custom sidebars from ~/.config/cmux/sidebars through the desktop control socket.",
        ),
        "workspace-group" => Some(
            "Usage:\n  cmux workspace group collapse GROUP_ID\n  cmux workspace group expand GROUP_ID\n\nCollapses or expands a workspace group.",
        ),
        "ssh" => Some(SSH_USAGE_TEXT),
        "list-panes" | "list-pane-surfaces" | "list-panels" => Some(
            "Usage:\n  cmux list-pane-surfaces\n\nLists pane/surface metadata for the active desktop session.",
        ),
        "new-split" => Some(
            "Usage:\n  cmux new-split [--panel PANEL] [--direction right|down|left|up]\n\nSplits a pane and creates a terminal surface.",
        ),
        "new-pane" => Some(
            "Usage:\n  cmux new-pane [--panel PANEL] [--direction right|down|left|up]\n\nSplits a pane and creates a terminal surface.",
        ),
        "new-surface" => Some(
            "Usage:\n  cmux new-surface [--panel PANEL] [--command CMD] [--input TEXT] [--env KEY=VALUE]\n\nCreates a terminal tab in the selected pane.",
        ),
        "new-terminal-tab" => Some(
            "Usage:\n  cmux new-terminal-tab [--panel PANEL] [--command CMD] [--input TEXT] [--env KEY=VALUE]\n\nCreates a terminal tab in the selected pane.",
        ),
        "split-browser" => Some(
            "Usage:\n  cmux split-browser [URL|--url URL] [--panel PANEL] [--direction right|down|left|up]\n\nSplits the selected pane with a browser surface.",
        ),
        "close-surface" => Some(
            "Usage:\n  cmux close-surface [SURFACE]\n\nCloses the selected surface, or the surface identified by surface:N ref or id.",
        ),
        "focus-pane" | "focus-panel" => Some(
            "Usage:\n  cmux focus-panel --panel PANEL\n\nFocuses a surface/panel in the selected or scoped workspace.",
        ),
        "surface-health" => Some(
            "Usage:\n  cmux surface-health [--workspace WORKSPACE]\n\nReports surface health for a workspace.",
        ),
        "read-screen" => Some(
            "Usage:\n  cmux read-screen [--workspace WORKSPACE] [--surface SURFACE] [--window WINDOW] [--scrollback] [--lines N]\n\nReads plain text from the selected terminal viewport or retained scrollback.",
        ),
        "capture-pane" => Some(
            "Usage:\n  cmux capture-pane [--workspace WORKSPACE] [--surface SURFACE] [--window WINDOW] [--scrollback] [--lines N]\n\nReads plain text from the selected terminal pane.",
        ),
        "send" => Some(
            "Usage:\n  cmux send [--workspace WORKSPACE] [--surface SURFACE] [--] TEXT\n\nSends literal text to a terminal surface.",
        ),
        "send-key" => Some(
            "Usage:\n  cmux send-key [--workspace WORKSPACE] [--surface SURFACE] KEY\n\nSends a named key to a terminal surface.",
        ),
        "send-panel" => Some(
            "Usage:\n  cmux send-panel --panel PANEL [--workspace WORKSPACE] [--] TEXT\n\nSends literal text to a terminal panel.",
        ),
        "send-key-panel" => Some(
            "Usage:\n  cmux send-key-panel --panel PANEL [--workspace WORKSPACE] KEY\n\nSends a named key to a terminal panel.",
        ),
        "rename-tab" => Some(
            "Usage:\n  cmux rename-tab [SURFACE] TITLE\n\nSets or clears a surface title.",
        ),
        "move-tab-to-new-workspace" => Some(
            "Usage:\n  cmux move-tab-to-new-workspace [SURFACE]\n\nMoves a surface into a new selected workspace.",
        ),
        "surface" => Some(
            "Usage:\n  cmux surface [list|split|new-tab|close|rename|pin|unpin|mark-read|mark-unread|browser|markdown|diff|next|previous]\n\nRuns a surface control command through the desktop control socket.",
        ),
        "browser" => Some(
            "Usage:\n  cmux browser [open URL|split URL|new-workspace URL|back|forward|reload|snapshot|eval|wait|click|fill|get|is|find|cookies|storage|tab|console|state|network|zoom VALUE]\n  cmux browser <surface> <agent-browser-style-command...>\n\nRuns a browser control command through the desktop control socket. Automation commands include snapshot/eval/wait, click/dblclick/hover/focus/type/fill/press/key/check/select/scroll, get/is/find locator families, frame/dialog/download helpers, cookies/storage/tab state, console/errors, highlight, addinitscript/addscript/addstyle, and state save/load. Use `cmux browser network [SURFACE] [--limit N] [--url-contains TEXT] [--method METHOD]` to inspect recorded browser Network records, including URL, method, headers, body previews, status, timing, proxy attribution, and record notes for cleartext or opaque proxy tunnel observations. Use `cmux browser network clear [SURFACE]` to clear retained records.",
        ),
        "open-browser" | "navigate" => Some(
            "Usage:\n  cmux open-browser [URL]\n  cmux navigate [URL]\n\nOpens a URL in the selected browser surface.",
        ),
        "browser-back" => Some(
            "Usage:\n  cmux browser-back [SURFACE]\n\nNavigates the selected browser surface back.",
        ),
        "browser-forward" => Some(
            "Usage:\n  cmux browser-forward [SURFACE]\n\nNavigates the selected browser surface forward.",
        ),
        "browser-reload" => Some(
            "Usage:\n  cmux browser-reload --panel SURFACE\n\nReloads a browser surface. Legacy alias for `cmux browser reload`.",
        ),
        "get-url" => Some(
            "Usage:\n  cmux get-url --panel SURFACE\n\nPrints the current browser URL. Legacy alias for `cmux browser get-url`.",
        ),
        "focus-webview" => Some(
            "Usage:\n  cmux focus-webview --panel SURFACE\n\nFocuses browser web content. Legacy alias for `cmux browser focus-webview`.",
        ),
        "is-webview-focused" => Some(
            "Usage:\n  cmux is-webview-focused --panel SURFACE\n\nPrints whether browser web content is focused. Legacy alias for `cmux browser is-webview-focused`.",
        ),
        "reopen-closed-browser-tab" => Some(
            "Usage:\n  cmux reopen-closed-browser-tab\n\nReopens the most recently closed browser tab.",
        ),
        "diff" => Some(
            "Usage:\n  cmux diff [--path PATH]\n\nOpens the diff surface in the selected pane.",
        ),
        "markdown" => Some(
            "Usage:\n  cmux markdown [--path PATH]\n\nOpens a markdown surface in the selected pane.",
        ),
        "hooks" => Some(
            "Usage:\n  cmux hooks claude install [--yes|-y]\n  cmux hooks setup --agent claude [--yes|-y]\n\nShows the cmux.json diff for enabling Claude Code integration, prompts for confirmation, and applies it.",
        ),
        _ => None,
    }
}

/// A uniform "this action is not part of the Windows headless port yet" failure
/// (exit 1). `label` is the user-facing command spelling.
fn not_yet_ported(label: &str) -> CliError {
    CliError::new(format!(
        "'{label}' is not yet available in the Windows port"
    ))
}

/// The not-yet-ported failure for a socket-backed command with no explicit v2
/// mapping yet. Kept distinct from [`not_yet_ported`] so the message points at
/// the raw escape hatch that always works for available backend methods.
fn socket_command_not_ported(command: &str) -> CliError {
    CliError::new(format!(
        "socket command '{command}' is not yet ported (M4 WS5); \
         use 'rpc' for raw v2 control-socket calls"
    ))
}

/// Map a classified `action` (for `command`) to the executor's plan. `command`
/// is needed because [`PreSocketAction::NeedsSocket`] does not carry the command
/// name, and only `rpc` has a working socket path today.
pub fn plan(action: &PreSocketAction, command: &str) -> DispatchPlan {
    plan_with_args(action, command, &[])
}

/// Map a classified `action` (for `command` plus its command-specific args) to
/// the executor's plan.
pub fn plan_with_args(action: &PreSocketAction, command: &str, args: &[String]) -> DispatchPlan {
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
            } else if command == "events" {
                DispatchPlan::RunEvents(args.to_vec())
            } else if command == "ssh" {
                DispatchPlan::RunSsh(args.to_vec())
            } else if matches!(command, "hooks" | "setup-hooks" | "uninstall-hooks") {
                DispatchPlan::RunHooksInstaller {
                    command: command.to_owned(),
                    args: args.to_vec(),
                }
            } else if let Some(control) = match control_command_for(command, args) {
                Ok(control) => control,
                Err(error) => return DispatchPlan::Fail(error),
            } {
                DispatchPlan::RunControl(control)
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
        PreSocketAction::DiffViewerRefs => {
            return DispatchPlan::RunDiffViewerRefs(args.to_vec());
        }
        PreSocketAction::DiffViewerBranch => {
            return DispatchPlan::RunDiffViewerBranch(args.to_vec());
        }
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
        assert_eq!(
            plan(&PreSocketAction::BareVersion, "version"),
            DispatchPlan::PrintVersion
        );
        assert_eq!(
            plan(&PreSocketAction::Help, "help"),
            DispatchPlan::PrintTopLevelHelp
        );
    }

    #[test]
    fn unknown_command_help_prints_exact_pointer_line() {
        let plan = plan(
            &PreSocketAction::UnknownCommandHelp {
                command: "bogus".to_owned(),
            },
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
    fn unmapped_subcommand_help_prints_header_and_pointer() {
        let plan = plan(
            &PreSocketAction::SubcommandHelp {
                command: "notify".to_owned(),
            },
            "notify",
        );
        match plan {
            DispatchPlan::PrintLine(text) => {
                assert!(text.starts_with("cmux notify\n\n"), "got: {text:?}");
                assert!(text.contains("run 'cmux help'"));
                assert!(text.contains("not yet ported"));
            }
            other => panic!("expected PrintLine, got {other:?}"),
        }
    }

    #[test]
    fn mapped_socket_command_help_prints_usage() {
        let plan = plan(
            &PreSocketAction::SubcommandHelp {
                command: "list-workspaces".to_owned(),
            },
            "list-workspaces",
        );
        match plan {
            DispatchPlan::PrintLine(text) => {
                assert!(
                    text.starts_with("cmux list-workspaces\n\n"),
                    "got: {text:?}"
                );
                assert!(text.contains("Usage:\n  cmux list-workspaces"));
                assert!(text.contains("active desktop session"));
                assert!(!text.contains("not yet ported"));
            }
            other => panic!("expected PrintLine, got {other:?}"),
        }
    }

    #[test]
    fn capabilities_help_and_route_are_concrete() {
        let help = subcommand_help_text("capabilities");
        assert!(help.contains("Usage:\n  cmux capabilities"));
        assert!(!help.contains("not yet ported"));

        match plan(&PreSocketAction::NeedsSocket, "capabilities") {
            DispatchPlan::RunControl(control) => {
                assert_eq!(control.method, "system.capabilities");
                assert_eq!(control.params, serde_json::json!({}));
            }
            other => panic!("expected RunControl, got {other:?}"),
        }
    }

    #[test]
    fn mapped_socket_command_help_is_concrete_for_control_aliases() {
        for command in [
            "browser-reload",
            "close-workspaces",
            "focus-webview",
            "get-url",
            "is-webview-focused",
            "new-browser-workspace",
            "new-terminal-tab",
            "rename-window",
            "reopen-closed-browser-tab",
            "restore-session",
            "read-screen",
            "capture-pane",
            "restore-previous-launch",
            "split-browser",
        ] {
            let plan = plan(
                &PreSocketAction::SubcommandHelp {
                    command: command.to_owned(),
                },
                command,
            );
            match plan {
                DispatchPlan::PrintLine(text) => {
                    assert!(text.starts_with(&format!("cmux {command}\n\n")));
                    assert!(text.contains("Usage:\n  cmux"));
                    assert!(!text.contains("not yet ported"), "{command}: {text}");
                }
                other => panic!("expected PrintLine for {command}, got {other:?}"),
            }
        }
    }

    #[test]
    fn browser_help_advertises_network_observability_fields() {
        let plan = plan(
            &PreSocketAction::SubcommandHelp {
                command: "browser".to_owned(),
            },
            "browser",
        );
        match plan {
            DispatchPlan::PrintLine(text) => {
                assert!(text.starts_with("cmux browser\n\n"), "got: {text:?}");
                assert!(text.contains("agent-browser-style-command"));
                assert!(text.contains("snapshot/eval/wait"));
                assert!(text.contains("get/is/find"));
                assert!(text.contains("cookies/storage/tab"));
                assert!(text.contains("addinitscript/addscript/addstyle"));
                assert!(text.contains("cmux browser network"));
                assert!(text.contains("headers"));
                assert!(text.contains("body previews"));
                assert!(text.contains("status"));
                assert!(text.contains("timing"));
                assert!(text.contains("proxy attribution"));
                assert!(text.contains("record notes"));
                assert!(text.contains("opaque proxy tunnel"));
                assert!(text.contains("network clear"));
                assert!(!text.contains("not yet ported"));
            }
            other => panic!("expected PrintLine, got {other:?}"),
        }
    }

    #[test]
    fn rpc_and_mapped_socket_commands_run() {
        assert_eq!(
            plan(&PreSocketAction::NeedsSocket, "rpc"),
            DispatchPlan::RunRpc
        );
        match plan_with_args(
            &PreSocketAction::NeedsSocket,
            "events",
            &["--limit".into(), "1".into()],
        ) {
            DispatchPlan::RunEvents(args) => assert_eq!(args, vec!["--limit", "1"]),
            other => panic!("expected RunEvents, got {other:?}"),
        }
        match plan_with_args(&PreSocketAction::NeedsSocket, "list-workspaces", &[]) {
            DispatchPlan::RunControl(control) => {
                assert_eq!(control.method, "workspace.list");
                assert_eq!(control.params, serde_json::json!({}));
            }
            other => panic!("expected RunControl, got {other:?}"),
        }
        match plan(&PreSocketAction::NeedsSocket, "read-screen") {
            DispatchPlan::RunControl(control) => {
                assert_eq!(control.method, "surface.read_text");
                assert_eq!(control.params, serde_json::json!({}));
            }
            other => panic!("expected RunControl, got {other:?}"),
        }
    }

    #[test]
    fn hooks_commands_run_local_installer() {
        match plan_with_args(
            &PreSocketAction::NeedsSocket,
            "hooks",
            &["claude".to_string(), "install".to_string()],
        ) {
            DispatchPlan::RunHooksInstaller { command, args } => {
                assert_eq!(command, "hooks");
                assert_eq!(args, vec!["claude", "install"]);
            }
            other => panic!("expected RunHooksInstaller, got {other:?}"),
        }
    }

    #[test]
    fn mapped_socket_commands_can_use_command_args() {
        let args = vec!["2".to_string()];
        match plan_with_args(&PreSocketAction::NeedsSocket, "select-workspace", &args) {
            DispatchPlan::RunControl(control) => {
                assert_eq!(control.method, "workspace.select");
                assert_eq!(
                    control.params,
                    serde_json::json!({"workspace_ref": "workspace:2"})
                );
            }
            other => panic!("expected RunControl, got {other:?}"),
        }
    }

    #[test]
    fn ssh_command_runs_multi_step_executor() {
        let args = vec![
            "dev.example.com".to_string(),
            "--port".to_string(),
            "2222".to_string(),
        ];
        match plan_with_args(&PreSocketAction::NeedsSocket, "ssh", &args) {
            DispatchPlan::RunSsh(planned_args) => assert_eq!(planned_args, args),
            other => panic!("expected RunSsh, got {other:?}"),
        }
    }

    #[test]
    fn ssh_help_prints_concrete_usage() {
        let plan = plan(
            &PreSocketAction::SubcommandHelp {
                command: "ssh".to_string(),
            },
            "ssh",
        );
        match plan {
            DispatchPlan::PrintLine(text) => {
                assert!(text.starts_with("cmux ssh\n\n"), "got: {text:?}");
                assert!(text.contains("Create a new workspace"));
                assert!(!text.contains("not yet ported"));
            }
            other => panic!("expected PrintLine, got {other:?}"),
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
            (
                PreSocketAction::WindowDefaultDisplay,
                "window default-display",
            ),
            (
                PreSocketAction::OpenPath {
                    path: "p".to_owned(),
                },
                "open",
            ),
        ] {
            match plan(&action, "x") {
                DispatchPlan::Fail(error) => {
                    assert_eq!(error.exit_code, 1, "{action:?}");
                    assert!(
                        error.message.contains(needle),
                        "{action:?}: {}",
                        error.message
                    );
                    assert!(error.message.contains("not yet available"), "{action:?}");
                }
                other => panic!("expected Fail for {action:?}, got {other:?}"),
            }
        }
    }
}
