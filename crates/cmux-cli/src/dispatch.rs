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
//! - The remaining **side-effecting no-socket commands** (`sessions`, the
//!   sigpipe/diff-viewer probes, `open <path>`, …) each need a subsystem that is
//!   not part of the headless core yet.

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
    /// Run the multi-call tmux compatibility shim.
    RunTmuxCompat(Vec<String>),
    /// Stream reconnectable event frames from the v2 control socket.
    RunEvents(Vec<String>),
    /// Run the multi-step SSH workspace bootstrap/control flow.
    RunSsh(Vec<String>),
    /// Inspect the remote-daemon release manifest and local cache.
    RunRemoteDaemonStatus(Vec<String>),
    /// Bridge this terminal to a Cloud VM PTY WebSocket.
    RunVmPtyConnect(Vec<String>),
    /// Run hidden local git-ref discovery for the diff-viewer branch picker.
    RunDiffViewerRefs(Vec<String>),
    /// Regenerate a branch-base diff page and manifest entries.
    RunDiffViewerBranch(Vec<String>),
    /// Serve token-jailed diff-viewer files over loopback HTTP.
    RunDiffViewerServer(Vec<String>),
    /// Render the canonical no-socket documentation index or topic.
    RunDocs(Vec<String>),
    /// Print the canonical ANSI welcome card without a socket.
    RunWelcome,
    /// Render settings paths/docs/help without a socket.
    RunSettings(Vec<String>),
    /// Render local config help and reference modes without a socket.
    RunConfig(Vec<String>),
    /// Edit a supported Ghostty config key, then best-effort reload the app.
    RunConfigMutation(Vec<String>),
    /// Read or edit the shared DEBUG-window display setting without a socket.
    RunWindowDefaultDisplay(Vec<String>),
    /// Launch the packaged desktop with a validated directory path.
    RunOpenPath(String),
    /// Inspect persisted agent hook sessions without a control socket.
    RunSessions(Vec<String>),
    /// Run the hidden child-process SIGPIPE/default-disposition probe.
    RunSigpipeProbe(Vec<String>),
    /// Verify a child closing stdin does not terminate the CLI writer.
    RunSigpipeStdinPipeProbe,
    /// Inspect the platform's SIGPIPE-equivalent stdio disposition.
    RunSigpipeInspect(Vec<String>),
    /// Run a local hooks installer/uninstaller command.
    RunHooksInstaller { command: String, args: Vec<String> },
    /// Read one agent hook payload from stdin and bridge it through `feed.push`.
    RunFeedHook(Vec<String>),
    /// Run a local Feed helper such as persistent-history clearing.
    RunFeed(Vec<String>),
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
        "config" => Some(crate::config::CONFIG_USAGE),
        "docs" => Some(crate::docs::DOCS_USAGE),
        "remote-daemon-status" => Some(crate::remote_daemon_status::REMOTE_DAEMON_STATUS_USAGE),
        "vm-pty-connect" => Some(crate::vm_pty_connect::VM_PTY_CONNECT_USAGE),
        "sessions" | "session-debug" => Some(crate::sessions::SESSIONS_USAGE),
        "settings" => Some(crate::settings::SETTINGS_USAGE),
        "ping" => Some("Usage:\n  cmux ping\n\nSends a ping to the control socket."),
        "capabilities" => Some(
            "Usage:\n  cmux capabilities\n\nPrints the control socket's supported methods and platform metadata as JSON.",
        ),
        "reload-config" => Some(
            "Usage:\n  cmux reload-config\n\nReloads cmux.json from disk and broadcasts the updated configuration.",
        ),
        "refresh-surfaces" => Some(
            "Usage:\n  cmux refresh-surfaces\n\nRequests mounted surfaces to refresh their layout and terminal fit.",
        ),
        "identify" => Some(
            "Usage:\n  cmux identify\n\nPrints desktop/control-socket identity metadata.",
        ),
        "tab-action" => Some(
            "Usage: cmux tab-action --action <name> [flags]\n\nRuns a tab action through the desktop control socket.\n\nFlags:\n  --action <name>             Action name (or provide it as the first positional)\n  --tab <id|ref|index>        Tab target; takes precedence over --surface\n  --surface <id|ref|index>    Surface target\n  --workspace <id|ref|index>  Workspace context\n  --window <id|ref|index>     Window context\n  --title <text>              Title for rename and related actions\n  --url <url>                 URL for browser-tab actions\n  --focus <true|false>        Focus the result (default: false)\n\nTab context (default: $CMUX_TAB_ID, then $CMUX_SURFACE_ID, then focused tab).",
        ),
        "respawn-pane" => Some(
            "Usage: cmux respawn-pane [--workspace <id|ref|index>] [--surface <id|ref|index>] [--window <id|ref|index>] [--command <cmd> | <cmd>]\n\nRespawns a terminal surface using the native Windows shell.\n\nFlags:\n  --workspace <id|ref|index>  Workspace context\n  --surface <id|ref|index>    Surface context (default: focused surface)\n  --window <id|ref|index>     Window context\n  --command <cmd>             Command to run (or provide trailing command tokens)",
        ),
        "list-windows" => Some("Usage:\n  cmux list-windows\n\nLists desktop windows."),
        "current-window" => Some(
            "Usage:\n  cmux current-window\n\nPrints the active desktop window ID.",
        ),
        "list-notifications" => Some(
            "Usage:\n  cmux list-notifications\n\nLists retained desktop notifications.",
        ),
        "dismiss-notification" => Some(
            "Usage:\n  cmux dismiss-notification (--id ID | --all-read)\n\nDismisses one notification or all read notifications.",
        ),
        "mark-notification-read" => Some(
            "Usage:\n  cmux mark-notification-read (--id ID | --workspace WORKSPACE [--surface SURFACE] | --all)\n\nMarks matching notifications read.",
        ),
        "clear-notifications" => Some(
            "Usage:\n  cmux clear-notifications [--workspace WORKSPACE]\n\nClears all notifications or those for one workspace.",
        ),
        "open-notification" => Some(
            "Usage:\n  cmux open-notification --id ID\n\nOpens the workspace and surface targeted by a notification.",
        ),
        "jump-to-unread" => Some(
            "Usage:\n  cmux jump-to-unread\n\nOpens the latest unread notification target.",
        ),
        "notify" => Some(
            "Usage:\n  cmux notify [--title TITLE] [--subtitle SUBTITLE] [--body BODY] [--workspace WORKSPACE] [--surface SURFACE]\n\nCreates and delivers a notification for the selected target.",
        ),
        "right-sidebar" => Some(
            "Usage:\n  cmux right-sidebar <toggle|show|hide|focus|set|mode|files|find|vault|sessions|feed|dock> [--workspace WORKSPACE] [--window WINDOW] [--no-focus]\n\nControls right-sidebar visibility, mode, and focus. The mode command prints its current state.",
        ),
        "feed" => Some(crate::feed_clear::FEED_USAGE),
        "list-workspaces" => Some(
            "Usage:\n  cmux list-workspaces [--window WINDOW]\n\nLists workspaces from the resolved desktop window.",
        ),
        "current-workspace" => Some(
            "Usage:\n  cmux current-workspace [--window WINDOW]\n\nPrints the selected workspace handle.",
        ),
        "new-workspace" => Some(
            "Usage:\n  cmux new-workspace [--name TITLE] [--description TEXT] [--cwd PATH] [--command CMD] [--env KEY=VALUE] [--env-file PATH] [--layout JSON] [--window WINDOW] [--focus true|false] [--group GROUP] [--group-placement PLACEMENT] [--group-reference WORKSPACE]\n\nCreates a terminal workspace.",
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
            "Usage:\n  cmux close-workspace --workspace WORKSPACE [--window WINDOW]\n\nCloses a workspace by UUID, workspace:N ref, or zero-based index.",
        ),
        "close-workspaces" => Some(
            "Usage:\n  cmux close-workspaces WORKSPACE...\n\nCloses one or more workspaces by workspace:N ref or workspace id. Bare numbers are normalized to workspace:N refs.",
        ),
        "reorder-workspace" => Some(
            "Usage:\n  cmux reorder-workspace [--workspace <id|ref|index> | <id|ref|index>] [flags]\n\nReorders a workspace within its window.\n\nFlags:\n  --index <n>                  Place at this index\n  --before <id|ref|index>      Place before this workspace\n  --before-workspace <handle>  Alias for --before\n  --after <id|ref|index>       Place after this workspace\n  --after-workspace <handle>   Alias for --after\n  --window <id|ref|index>      Window context\n  --dry-run                    Print the resolved final index without applying",
        ),
        "reorder-workspaces" => Some(
            "Usage:\n  cmux reorder-workspaces --order <id|ref|index>,<id|ref|index>,... [flags]\n\nAtomically reorders workspaces within pinned and unpinned groups. Unmentioned workspaces keep their relative order after listed peers in the same group.\n\nFlags:\n  --order <refs>               Comma-separated workspace order\n  --window <id|ref|index>      Window context\n  --dry-run                    Print resolved final indexes without applying",
        ),
        "move-workspace-to-window" => Some(
            "Usage:\n  cmux move-workspace-to-window --workspace <id|ref|index> --window <id|ref|index>\n\nMoves a workspace to a different window.\n\nFlags:\n  --workspace <id|ref|index>   Workspace to move (required)\n  --window <id|ref|index>      Target window (required)",
        ),
        "move-surface" => Some(
            "Usage:\n  cmux move-surface [--surface <id|ref|index> | <id|ref|index>] [flags]\n\nMoves a surface to a pane, workspace, or window.\n\nFlags:\n  --surface <id|ref|index>     Surface to move\n  --pane <id|ref|index>        Destination pane\n  --workspace <id|ref|index>   Destination workspace\n  --window <id|ref|index>      Destination window\n  --index <n>                  Destination insertion index\n  --before <id|ref|index>      Place before this surface\n  --before-surface <handle>    Alias for --before\n  --after <id|ref|index>       Place after this surface\n  --after-surface <handle>     Alias for --after\n  --focus <true|false>         Focus after moving",
        ),
        "split-off" => Some(
            "Usage:\n  cmux split-off --surface <id|ref|index> <left|right|up|down> [flags]\n\nMoves an existing surface into a new split without changing focus by default.\n\nFlags:\n  --surface <id|ref|index>     Surface to move (required)\n  --panel <id|ref|index>       Alias for --surface\n  --workspace <id|ref|index>   Workspace context\n  --window <id|ref|index>      Window context\n  --focus <true|false>         Focus the split-off surface (default: false)",
        ),
        "drag-surface-to-split" => Some(
            "Usage:\n  cmux drag-surface-to-split --surface <id|ref|index> <left|right|up|down> [flags]\n\nDrags a surface into a new split in the given direction.\n\nFlags:\n  --surface <id|ref|index>     Surface to drag (required)\n  --panel <id|ref|index>       Alias for --surface\n  --workspace <id|ref|index>   Workspace context\n  --window <id|ref|index>      Window context\n  --focus <true|false>         Focus the split-off surface (default: false)",
        ),
        "swap-pane" => Some(
            "Usage:\n  cmux swap-pane --pane <id|ref|index> --target-pane <id|ref|index> [flags]\n\nSwaps the selected surfaces of two panes.\n\nFlags:\n  --pane <id|ref|index>         Source pane (required)\n  --target-pane <id|ref|index>  Target pane (required)\n  --workspace <id|ref|index>    Workspace context\n  --window <id|ref|index>       Window context\n  --focus <true|false>          Focus the target pane (default: false)",
        ),
        "break-pane" => Some(
            "Usage:\n  cmux break-pane [--workspace <id|ref|index>] [--pane <id|ref|index>] [--surface <id|ref|index>] [--window <id|ref|index>] [--focus <true|false>] [--no-focus]\n\nMove a pane/surface out into its own pane context.\n\nFlags:\n  --workspace <id|ref|index>   Workspace context\n  --pane <id|ref|index>        Source pane\n  --surface <id|ref|index>     Source surface\n  --window <id|ref|index>      Window context\n  --focus <true|false>         Focus the result (default: false)\n  --no-focus                   Compatibility alias for --focus false",
        ),
        "join-pane" => Some(
            "Usage:\n  cmux join-pane --target-pane <id|ref|index> [--workspace <id|ref|index>] [--pane <id|ref|index>] [--surface <id|ref|index>] [--window <id|ref|index>] [--focus <true|false>] [--no-focus]\n\nJoin a pane/surface into another pane.\n\nFlags:\n  --target-pane <id|ref|index>  Target pane (required)\n  --workspace <id|ref|index>    Workspace context\n  --pane <id|ref|index>         Source pane\n  --surface <id|ref|index>      Source surface\n  --window <id|ref|index>       Window context\n  --focus <true|false>          Focus the result (default: false)\n  --no-focus                    Compatibility alias for --focus false",
        ),
        "last-pane" => Some(
            "Usage:\n  cmux last-pane [--workspace <id|ref|index>] [--window <id|ref|index>]\n\nFocus the previously focused pane in a workspace.\n\nFlags:\n  --workspace <id|ref|index>   Workspace context\n  --window <id|ref|index>      Window context for workspace refs and indexes",
        ),
        "resize-pane" => Some(
            "Usage:\n  cmux resize-pane [--pane <id|ref|index>] [--workspace <id|ref|index>] [--window <id|ref|index>] [-L|-R|-U|-D] [--amount <n>]\n\ntmux-compatible pane resize command.\n\nFlags:\n  --pane <id|ref|index>        Pane to resize (default: focused pane)\n  --workspace <id|ref|index>   Workspace context\n  --window <id|ref|index>      Window context for workspace/pane refs and indexes\n  -L|-R|-U|-D                  Direction (default: -R)\n  --amount <n>                 Resize amount (default: 1)",
        ),
        "last-window" => Some(
            "Usage:\n  cmux last-window [--window <id|ref|index>]\n\nSelects the previously visited workspace.\n\nFlags:\n  --window <id|ref|index>   Window whose workspace history to navigate",
        ),
        "reorder-surface" => Some(
            "Usage:\n  cmux reorder-surface [--surface <id|ref|index> | <id|ref|index>] [flags]\n\nReorders a surface within its pane.\n\nFlags:\n  --surface <id|ref|index>     Surface to reorder\n  --workspace <id|ref|index>   Workspace context\n  --window <id|ref|index>      Window context\n  --index <n>                  Place at this insertion index\n  --before <id|ref|index>      Place before this surface\n  --before-surface <handle>    Alias for --before\n  --after <id|ref|index>       Place after this surface\n  --after-surface <handle>     Alias for --after\n  --focus <true|false>         Focus after reordering (default: false)",
        ),
        "select-workspace" => Some(
            "Usage:\n  cmux select-workspace --workspace WORKSPACE [--window WINDOW]\n\nSelects a workspace by UUID, workspace:N ref, or zero-based index.",
        ),
        "rename-workspace" => Some(
            "Usage:\n  cmux rename-workspace [--workspace WORKSPACE] [--window WINDOW] [--] TITLE\n\nSets a workspace title; all positional tokens form the title.",
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
        "list-panes" => Some(
            "Usage:\n  cmux list-panes [--workspace WORKSPACE] [--window WINDOW]\n\nLists panes in a workspace.",
        ),
        "list-pane-surfaces" => Some(
            "Usage:\n  cmux list-pane-surfaces [--workspace WORKSPACE] [--pane PANE] [--window WINDOW]\n\nLists surfaces in a pane. Defaults to the focused pane.",
        ),
        "list-panels" => Some(
            "Usage:\n  cmux list-panels [--workspace WORKSPACE] [--window WINDOW]\n\nLists surfaces (panels) in a workspace.",
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
        "focus-pane" => Some(
            "Usage:\n  cmux focus-pane [--pane PANE | PANE] [--workspace WORKSPACE] [--window WINDOW]\n\nFocuses the specified pane.",
        ),
        "focus-panel" => Some(
            "Usage:\n  cmux focus-panel --panel PANEL [--workspace WORKSPACE] [--window WINDOW]\n\nFocuses a surface/panel in the selected or scoped workspace.",
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
        "clear-history" => Some(
            "Usage:\n  cmux clear-history [--workspace WORKSPACE] [--surface SURFACE] [--window WINDOW]\n\nClears retained scrollback for the selected terminal surface.",
        ),
        "trigger-flash" => Some(
            "Usage:\n  cmux trigger-flash [--workspace WORKSPACE] [--surface SURFACE] [--window WINDOW]\n\nFlashes the selected surface to draw attention to it.",
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
            "Usage:\n  cmux hooks feed --source AGENT [--event EVENT]\n  cmux hooks AGENT install [--yes|-y]\n  cmux hooks uninstall [AGENT|--agent AGENT]\n  cmux hooks (gemini|grok|copilot|codebuddy|factory|qoder) uninstall\n  cmux hooks (kiro|cursor|codex) uninstall\n  cmux hooks (antigravity|agy) uninstall\n  cmux hooks (rovodev|rovo) uninstall\n  cmux hooks (hermes-agent|hermes) uninstall\n  cmux hooks kimi uninstall\n  cmux hooks (pi|omp|amp) uninstall\n  cmux hooks opencode install [--project] [--yes|-y]\n  cmux hooks opencode uninstall [--project]\n  cmux hooks setup [--agent AGENT] [--uninstall] [--yes|-y]\n\nBridges agent events into Feed or installs hooks for Claude, Codex, Kiro, Gemini, Grok, Copilot, CodeBuddy, Factory, Qoder, Cursor, Antigravity, OpenCode, Pi, OMP, Amp, Rovo Dev, Hermes Agent, and Kimi Code.",
        ),
        _ => None,
    }
}

/// The not-yet-ported failure for a socket-backed command with no explicit v2
/// mapping yet. The message points at the raw escape hatch that always works
/// for available backend methods.
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
    // Every no-socket action has a concrete executor. Socket-backed commands
    // without a typed mapping retain the explicit raw-RPC failure below.
    match action {
        PreSocketAction::BareVersion => DispatchPlan::PrintVersion,
        PreSocketAction::Help => DispatchPlan::PrintTopLevelHelp,
        PreSocketAction::UnknownCommandHelp { command } => {
            DispatchPlan::PrintLine(unknown_command_message(command))
        }
        PreSocketAction::SubcommandHelp { command } => {
            DispatchPlan::PrintLine(subcommand_help_text(command))
        }
        PreSocketAction::NeedsSocket => {
            if command == "rpc" {
                DispatchPlan::RunRpc
            } else if command == "__tmux-compat" {
                DispatchPlan::RunTmuxCompat(args.to_vec())
            } else if command == "events" {
                DispatchPlan::RunEvents(args.to_vec())
            } else if command == "ssh" {
                DispatchPlan::RunSsh(args.to_vec())
            } else if command == "feed-hook" {
                DispatchPlan::RunFeedHook(args.to_vec())
            } else if command == "feed" {
                DispatchPlan::RunFeed(args.to_vec())
            } else if command == "hooks" && args.first().is_some_and(|arg| arg == "feed") {
                DispatchPlan::RunFeedHook(args[1..].to_vec())
            } else if command == "hooks" {
                if let Some(feed_args) = generated_hook_feed_args(args) {
                    DispatchPlan::RunFeedHook(feed_args)
                } else {
                    DispatchPlan::RunHooksInstaller {
                        command: command.to_owned(),
                        args: args.to_vec(),
                    }
                }
            } else if matches!(command, "hooks" | "setup-hooks" | "uninstall-hooks") {
                DispatchPlan::RunHooksInstaller {
                    command: command.to_owned(),
                    args: args.to_vec(),
                }
            } else if command == "config"
                && args.first().is_some_and(|argument| {
                    matches!(
                        argument.to_lowercase().as_str(),
                        "set" | "sidebar-font-size" | "surface-tab-bar-font-size"
                    )
                })
            {
                DispatchPlan::RunConfigMutation(args.to_vec())
            } else if let Some(control) = match control_command_for(command, args) {
                Ok(control) => control,
                Err(error) => return DispatchPlan::Fail(error),
            } {
                DispatchPlan::RunControl(control)
            } else {
                DispatchPlan::Fail(socket_command_not_ported(command))
            }
        }
        PreSocketAction::RemoteDaemonStatus => DispatchPlan::RunRemoteDaemonStatus(args.to_vec()),
        PreSocketAction::VmPtyConnect => DispatchPlan::RunVmPtyConnect(args.to_vec()),
        PreSocketAction::Docs => DispatchPlan::RunDocs(args.to_vec()),
        PreSocketAction::Welcome => DispatchPlan::RunWelcome,
        PreSocketAction::Sessions { debug } => {
            let mut session_args = args.to_vec();
            if *debug {
                session_args.insert(0, "debug".to_string());
            }
            DispatchPlan::RunSessions(session_args)
        }
        PreSocketAction::SigpipeProbe => DispatchPlan::RunSigpipeProbe(args.to_vec()),
        PreSocketAction::SigpipeStdinPipeProbe => DispatchPlan::RunSigpipeStdinPipeProbe,
        PreSocketAction::SigpipeInspect => DispatchPlan::RunSigpipeInspect(args.to_vec()),
        PreSocketAction::DiffViewerServer => DispatchPlan::RunDiffViewerServer(args.to_vec()),
        PreSocketAction::DiffViewerRefs => DispatchPlan::RunDiffViewerRefs(args.to_vec()),
        PreSocketAction::DiffViewerBranch => DispatchPlan::RunDiffViewerBranch(args.to_vec()),
        PreSocketAction::SettingsNoSocket => DispatchPlan::RunSettings(args.to_vec()),
        PreSocketAction::WindowDefaultDisplay => {
            DispatchPlan::RunWindowDefaultDisplay(args.to_vec())
        }
        PreSocketAction::ConfigNoSocket => DispatchPlan::RunConfig(args.to_vec()),
        PreSocketAction::OpenPath { path } => DispatchPlan::RunOpenPath(path.clone()),
    }
}

fn generated_hook_feed_args(args: &[String]) -> Option<Vec<String>> {
    let source = args.first()?;
    let event = args.get(1)?;
    if !matches!(
        event.as_str(),
        "session-start"
            | "prompt-submit"
            | "stop"
            | "notification"
            | "notify"
            | "agent-response"
            | "approval-response"
            | "shell-exec"
            | "shell-done"
            | "session-end"
            | "session-finalize"
    ) {
        return None;
    }
    Some(vec![
        "--source".to_string(),
        source.to_string(),
        "--event".to_string(),
        event.to_string(),
    ])
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
                command: "new-window".to_owned(),
            },
            "new-window",
        );
        match plan {
            DispatchPlan::PrintLine(text) => {
                assert!(text.starts_with("cmux new-window\n\n"), "got: {text:?}");
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
                assert!(text.contains("resolved desktop window"));
                assert!(!text.contains("not yet ported"));
            }
            other => panic!("expected PrintLine, got {other:?}"),
        }
    }

    #[test]
    fn pane_list_command_help_describes_distinct_scopes() {
        assert!(subcommand_help_text("list-panes")
            .contains("cmux list-panes [--workspace WORKSPACE] [--window WINDOW]"));
        assert!(subcommand_help_text("list-pane-surfaces").contains(
            "cmux list-pane-surfaces [--workspace WORKSPACE] [--pane PANE] [--window WINDOW]"
        ));
        assert!(subcommand_help_text("list-panels")
            .contains("cmux list-panels [--workspace WORKSPACE] [--window WINDOW]"));
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
    fn tmux_compat_routes_to_its_multi_call_executor() {
        let args = vec!["resize-pane".to_string(), "-L".to_string()];
        assert_eq!(
            plan_with_args(&PreSocketAction::NeedsSocket, "__tmux-compat", &args),
            DispatchPlan::RunTmuxCompat(args)
        );
    }

    #[test]
    fn mapped_socket_command_help_is_concrete_for_control_aliases() {
        for command in [
            "browser-reload",
            "clear-history",
            "clear-notifications",
            "close-workspaces",
            "current-window",
            "dismiss-notification",
            "focus-webview",
            "get-url",
            "is-webview-focused",
            "jump-to-unread",
            "list-windows",
            "list-notifications",
            "mark-notification-read",
            "open-notification",
            "new-browser-workspace",
            "new-terminal-tab",
            "notify",
            "reload-config",
            "refresh-surfaces",
            "move-workspace-to-window",
            "move-surface",
            "reorder-workspace",
            "reorder-workspaces",
            "reorder-surface",
            "right-sidebar",
            "rename-window",
            "reopen-closed-browser-tab",
            "restore-session",
            "trigger-flash",
            "read-screen",
            "capture-pane",
            "restore-previous-launch",
            "split-browser",
            "split-off",
            "drag-surface-to-split",
            "swap-pane",
            "break-pane",
            "join-pane",
            "last-pane",
            "last-window",
            "resize-pane",
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
        assert!(
            subcommand_help_text("hooks").contains("cmux hooks uninstall [AGENT|--agent AGENT]")
        );
        assert!(subcommand_help_text("hooks").contains("cmux hooks (pi|omp|amp) uninstall"));
        assert!(subcommand_help_text("hooks")
            .contains("cmux hooks (gemini|grok|copilot|codebuddy|factory|qoder) uninstall"));
        assert!(subcommand_help_text("hooks").contains("cmux hooks (kiro|cursor|codex) uninstall"));
        assert!(subcommand_help_text("hooks").contains("cmux hooks (antigravity|agy) uninstall"));
        assert!(subcommand_help_text("hooks").contains("cmux hooks (rovodev|rovo) uninstall"));
        assert!(subcommand_help_text("hooks").contains("cmux hooks kimi uninstall"));
        assert!(
            subcommand_help_text("hooks").contains("cmux hooks (hermes-agent|hermes) uninstall")
        );
        assert!(subcommand_help_text("hooks").contains("cmux hooks opencode uninstall [--project]"));
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
    fn feed_hook_spellings_run_the_local_bridge() {
        for (command, args, expected) in [
            (
                "hooks",
                vec![
                    "feed".to_string(),
                    "--source".to_string(),
                    "claude".to_string(),
                ],
                vec!["--source".to_string(), "claude".to_string()],
            ),
            (
                "feed-hook",
                vec!["--source".to_string(), "claude".to_string()],
                vec!["--source".to_string(), "claude".to_string()],
            ),
        ] {
            match plan_with_args(&PreSocketAction::NeedsSocket, command, &args) {
                DispatchPlan::RunFeedHook(planned) => assert_eq!(planned, expected),
                other => panic!("expected RunFeedHook, got {other:?}"),
            }
        }
    }

    #[test]
    fn generated_agent_lifecycle_hooks_run_through_the_feed_bridge() {
        for (agent, action) in [
            ("kiro", "session-start"),
            ("gemini", "prompt-submit"),
            ("copilot", "stop"),
        ] {
            let args = vec![agent.to_string(), action.to_string()];
            match plan_with_args(&PreSocketAction::NeedsSocket, "hooks", &args) {
                DispatchPlan::RunFeedHook(planned) => assert_eq!(
                    planned,
                    vec!["--source", agent, "--event", action]
                        .into_iter()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                ),
                other => panic!("expected lifecycle Feed bridge, got {other:?}"),
            }
        }
    }

    #[test]
    fn feed_commands_run_the_local_feed_executor() {
        let args = vec!["clear".to_string(), "--yes".to_string()];
        match plan_with_args(&PreSocketAction::NeedsSocket, "feed", &args) {
            DispatchPlan::RunFeed(planned) => assert_eq!(planned, args),
            other => panic!("expected RunFeed, got {other:?}"),
        }
    }

    #[test]
    fn docs_command_runs_the_local_docs_executor() {
        let args = vec!["configuration".to_string(), "--json".to_string()];
        assert_eq!(
            plan_with_args(&PreSocketAction::Docs, "docs", &args),
            DispatchPlan::RunDocs(args)
        );
    }

    #[test]
    fn diff_viewer_server_runs_locally_with_all_arguments() {
        let args = vec!["--root".to_string(), "C:\\viewer".to_string()];
        assert_eq!(
            plan_with_args(
                &PreSocketAction::DiffViewerServer,
                "diff-viewer-server",
                &args,
            ),
            DispatchPlan::RunDiffViewerServer(args)
        );
    }

    #[test]
    fn remote_daemon_status_runs_locally_with_all_arguments() {
        let args = vec![
            "--os".to_string(),
            "linux".to_string(),
            "--arch=arm64".to_string(),
        ];
        assert_eq!(
            plan_with_args(
                &PreSocketAction::RemoteDaemonStatus,
                "remote-daemon-status",
                &args,
            ),
            DispatchPlan::RunRemoteDaemonStatus(args)
        );
        let help = subcommand_help_text("remote-daemon-status");
        assert!(help.contains("Usage: cmux remote-daemon-status"));
        assert!(help.contains("checksum verification state"));
        assert!(!help.contains("not yet ported"));
    }

    #[test]
    fn vm_pty_connect_runs_locally_with_all_arguments() {
        let args = vec![
            "--config".to_string(),
            "C:\\Temp\\vm.json".to_string(),
            "--id=vm-123".to_string(),
        ];
        assert_eq!(
            plan_with_args(&PreSocketAction::VmPtyConnect, "vm-pty-connect", &args),
            DispatchPlan::RunVmPtyConnect(args)
        );
        let help = subcommand_help_text("vm-pty-connect");
        assert!(help.contains(crate::vm_pty_connect::VM_PTY_CONNECT_USAGE));
        assert!(!help.contains("not yet ported"));
    }

    #[test]
    fn welcome_command_runs_the_local_welcome_executor() {
        assert_eq!(
            plan_with_args(&PreSocketAction::Welcome, "welcome", &[]),
            DispatchPlan::RunWelcome
        );
    }

    #[test]
    fn settings_no_socket_runs_the_local_settings_executor() {
        let args = vec!["path".to_string(), "--json".to_string()];
        assert_eq!(
            plan_with_args(&PreSocketAction::SettingsNoSocket, "settings", &args),
            DispatchPlan::RunSettings(args)
        );
    }

    #[test]
    fn config_no_socket_runs_the_local_config_executor() {
        let args = vec!["path".to_string(), "--json".to_string()];
        assert_eq!(
            plan_with_args(&PreSocketAction::ConfigNoSocket, "config", &args),
            DispatchPlan::RunConfig(args)
        );
    }

    #[test]
    fn config_reload_routes_to_the_existing_reload_control_method() {
        let args = vec!["reload".to_string()];
        match plan_with_args(&PreSocketAction::NeedsSocket, "config", &args) {
            DispatchPlan::RunControl(control) => {
                assert_eq!(control.method, "config.reload");
                assert_eq!(control.params, serde_json::json!({}));
            }
            other => panic!("expected RunControl, got {other:?}"),
        }

        let extra = vec!["reload".to_string(), "extra".to_string()];
        match plan_with_args(&PreSocketAction::NeedsSocket, "config", &extra) {
            DispatchPlan::Fail(error) => assert_eq!(error.message, "Usage: cmux config reload"),
            other => panic!("expected Fail, got {other:?}"),
        }

        let set = vec![
            "set".to_string(),
            "sidebar-font-size".to_string(),
            "14".to_string(),
        ];
        match plan_with_args(&PreSocketAction::NeedsSocket, "config", &set) {
            DispatchPlan::RunConfigMutation(planned) => assert_eq!(planned, set),
            other => panic!("expected RunConfigMutation, got {other:?}"),
        }
    }

    #[test]
    fn config_set_routes_to_the_local_mutation_executor() {
        let args = vec![
            "set".to_string(),
            "sidebar-font-size".to_string(),
            "14".to_string(),
        ];
        assert_eq!(
            plan_with_args(&PreSocketAction::NeedsSocket, "config", &args),
            DispatchPlan::RunConfigMutation(args)
        );
    }

    #[test]
    fn window_default_display_runs_the_local_setting_executor() {
        let args = vec!["default-display".to_string(), "Display 2".to_string()];
        assert_eq!(
            plan_with_args(&PreSocketAction::WindowDefaultDisplay, "window", &args),
            DispatchPlan::RunWindowDefaultDisplay(args)
        );
    }

    #[test]
    fn bare_path_runs_the_local_desktop_launcher() {
        assert_eq!(
            plan_with_args(
                &PreSocketAction::OpenPath {
                    path: "project".to_string(),
                },
                "project",
                &[]
            ),
            DispatchPlan::RunOpenPath("project".to_string())
        );
    }

    #[test]
    fn mapped_socket_commands_can_use_command_args() {
        let args = vec!["--workspace".to_string(), "2".to_string()];
        match plan_with_args(&PreSocketAction::NeedsSocket, "select-workspace", &args) {
            DispatchPlan::RunControl(control) => {
                assert_eq!(control.method, "workspace.select");
                assert_eq!(control.params, serde_json::json!({"workspace_index": 2}));
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
    fn sessions_commands_run_locally_without_a_socket() {
        for (action, command, expected_args) in [
            (
                PreSocketAction::Sessions { debug: false },
                "sessions",
                Vec::<String>::new(),
            ),
            (
                PreSocketAction::Sessions { debug: true },
                "session-debug",
                vec!["debug".to_string()],
            ),
        ] {
            match plan_with_args(&action, command, &[]) {
                DispatchPlan::RunSessions(args) => assert_eq!(args, expected_args),
                other => panic!("expected RunSessions for {action:?}, got {other:?}"),
            }
        }
    }

    #[test]
    fn sigpipe_diagnostics_run_locally_without_a_socket() {
        let args = vec!["probe".to_string()];
        assert_eq!(
            plan_with_args(&PreSocketAction::SigpipeProbe, "hidden", &args),
            DispatchPlan::RunSigpipeProbe(args.clone())
        );
        assert_eq!(
            plan_with_args(&PreSocketAction::SigpipeStdinPipeProbe, "hidden", &args),
            DispatchPlan::RunSigpipeStdinPipeProbe
        );
        assert_eq!(
            plan_with_args(&PreSocketAction::SigpipeInspect, "hidden", &args),
            DispatchPlan::RunSigpipeInspect(args)
        );
    }
}
