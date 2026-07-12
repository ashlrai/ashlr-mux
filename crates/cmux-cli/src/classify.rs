//! No-socket command taxonomy (M4 WS5).
//!
//! `classify_command` routes a parsed command to the no-socket handling it
//! needs (help/version/path-open/settings-docs/config-doctor/…) or to
//! [`PreSocketAction::NeedsSocket`] — a faithful port of the pre-socket dispatch
//! in `CLI/cmux.swift` `run()` (~3144-3216) and its `looksLikePath` /
//! `shouldOpenAsPathArgument` / `settings`/`config` no-socket predicates.
//!
//! This module only ROUTES; the per-command usage text and the side-effecting
//! handlers are a later slice. It is pure and cross-platform-testable: the
//! current directory and path existence are injected via [`ClassifyEnv`], so
//! tests never touch the filesystem.
//!
//! **Windows path adaptation:** `looks_like_path` recognizes `\` separators (and
//! `Path::is_absolute` handles drive-letter / UNC roots) in addition to POSIX
//! `/`, so a Windows user's `.\proj` or `C:\code\x` is detected as a path. The
//! command-routing parity with the macOS CLI is unchanged — only the path
//! *shape* recognition is extended for the port's platform.

use std::path::{Path, PathBuf};

/// Where a command should go before any control-socket connection is attempted.
/// Each variant mirrors one pre-socket branch of the Swift `run()` dispatch; the
/// rendering / side-effecting handlers behind each are a later slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreSocketAction {
    /// `version` (matched by name, before the help gate).
    BareVersion,
    /// A help flag on a command that has a usage entry → print its usage.
    SubcommandHelp {
        command: String,
    },
    /// A help flag on a command with no usage entry → print the unknown-command
    /// line and exit 0.
    UnknownCommandHelp {
        command: String,
    },
    /// The bare `help` command.
    Help,
    RemoteDaemonStatus,
    VmPtyConnect,
    Docs,
    Welcome,
    /// `sessions` / `session-debug` (`debug` prepends a `debug` arg downstream).
    Sessions {
        debug: bool,
    },
    SigpipeProbe,
    SigpipeStdinPipeProbe,
    SigpipeInspect,
    DiffViewerServer,
    DiffViewerRefs,
    DiffViewerBranch,
    /// `settings` in a no-socket mode (help / path / docs subcommands).
    SettingsNoSocket,
    /// `window default-display` (reads/writes a local dev setting only).
    WindowDefaultDisplay,
    /// `config` in a no-socket mode (get / doctor / help / docs / font-size get).
    ConfigNoSocket,
    /// `cmux <path>` file-open (only when no `--socket` is given).
    OpenPath {
        path: String,
    },
    /// Everything else: a socket-backed command.
    NeedsSocket,
}

/// Injected environment for path-open classification, so the policy stays pure
/// and filesystem-free in tests.
pub struct ClassifyEnv<'a> {
    /// The process current directory (live `std::env::current_dir()` in prod).
    pub cwd: &'a Path,
    /// Whether a resolved path exists. `Path::exists` in prod (follows symlinks,
    /// true for files and directories — matching `FileManager.fileExists`).
    pub path_exists: &'a dyn Fn(&Path) -> bool,
}

/// Classify `command` (with its post-presentation `command_args`) into the
/// pre-socket action it warrants. `explicit_socket_path` is the `--socket` flag
/// value (not the resolved address); a present `--socket` suppresses path-open.
///
/// The ordering is load-bearing and matches Swift `run()` exactly: `version` is
/// matched by name *before* the help gate (so `version --help` prints the
/// version), then the help gate, then the name-routed no-socket commands, then
/// the settings / window-default-display / config predicates, then path-open,
/// then the socket fall-through.
pub fn classify_command(
    command: &str,
    command_args: &[String],
    explicit_socket_path: Option<&str>,
    env: &ClassifyEnv<'_>,
) -> PreSocketAction {
    // 1. `version` by name — precedes the help gate.
    if command == "version" {
        return PreSocketAction::BareVersion;
    }

    // 2. Help gate: scan only the tokens before the first `--` for an exact
    //    `--help` / `-h`. `__tmux-compat` is excluded and falls through.
    if command != "__tmux-compat"
        && !(command == "feed" && command_args.first().is_some_and(|arg| arg == "tui"))
    {
        let pre_separator = match command_args.iter().position(|arg| arg == "--") {
            Some(index) => &command_args[..index],
            None => command_args,
        };
        if pre_separator
            .iter()
            .any(|arg| arg == "--help" || arg == "-h")
        {
            return if command_has_usage_entry(command) {
                PreSocketAction::SubcommandHelp {
                    command: command.to_owned(),
                }
            } else {
                PreSocketAction::UnknownCommandHelp {
                    command: command.to_owned(),
                }
            };
        }
    }

    // 3-10. Name-routed no-socket commands.
    match command {
        "help" => return PreSocketAction::Help,
        "remote-daemon-status" => return PreSocketAction::RemoteDaemonStatus,
        "vm-pty-connect" => return PreSocketAction::VmPtyConnect,
        "docs" => return PreSocketAction::Docs,
        "welcome" => return PreSocketAction::Welcome,
        "sessions" => return PreSocketAction::Sessions { debug: false },
        "session-debug" => return PreSocketAction::Sessions { debug: true },
        "__sigpipe-probe" => return PreSocketAction::SigpipeProbe,
        "__sigpipe-stdin-pipe-probe" => return PreSocketAction::SigpipeStdinPipeProbe,
        "__sigpipe-inspect" => return PreSocketAction::SigpipeInspect,
        "diff-viewer-server" => return PreSocketAction::DiffViewerServer,
        "__diff-viewer-refs" => return PreSocketAction::DiffViewerRefs,
        "__diff-viewer-branch" => return PreSocketAction::DiffViewerBranch,
        _ => {}
    }

    // 11. settings (no-socket modes).
    if command == "settings" && settings_command_does_not_need_socket(command_args) {
        return PreSocketAction::SettingsNoSocket;
    }

    // 12. `window default-display` — sits between settings and config.
    if command == "window"
        && command_args
            .first()
            .map(|arg| arg.to_lowercase())
            .as_deref()
            == Some("default-display")
    {
        return PreSocketAction::WindowDefaultDisplay;
    }

    // 13. config (no-socket modes).
    if command == "config" && config_command_does_not_need_socket(command_args) {
        return PreSocketAction::ConfigNoSocket;
    }

    // 14. `cmux <path>` file-open — only without an explicit --socket.
    if explicit_socket_path.is_none() && should_open_as_path_argument(command, env) {
        return PreSocketAction::OpenPath {
            path: command.to_owned(),
        };
    }

    // 15. Socket-backed command (incl. settings/config when their predicate is
    //     false — those re-dispatch on the socket path).
    PreSocketAction::NeedsSocket
}

/// Whether `arg` looks like a path rather than a command. The macOS set
/// (`.`, `..`, `~`-prefix, contains `/`) extended with Windows `\` so native
/// Windows paths are recognized. The explicit `/`-prefix etc. of the Swift
/// source are subsumed by the `contains` checks, so the result is identical for
/// POSIX inputs.
fn looks_like_path(arg: &str) -> bool {
    arg == "." || arg == ".." || arg.starts_with('~') || arg.contains('/') || arg.contains('\\')
}

/// Resolve a bare relative `arg` against `cwd` for the existence check. Absolute
/// inputs are returned as-is (`Path::is_absolute` covers drive-letter / UNC
/// roots on Windows). `~` / separator-bearing inputs never reach here — they
/// short-circuit in [`should_open_as_path_argument`] via `looks_like_path`.
fn resolve_path(arg: &str, cwd: &Path) -> PathBuf {
    let path = Path::new(arg);
    // The `is_absolute` arm is defensive only: every absolute path bears a
    // separator, so `should_open_as_path_argument` already short-circuited it
    // via `looks_like_path`. Reachable inputs here are separator-free relatives.
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(arg)
    }
}

/// Whether a bare first token should open as a path. Path-shaped tokens open
/// unconditionally (before the command/flag guard, so e.g. `./open` opens even
/// though `open` is a command); a leading `-` or a known command never opens; an
/// otherwise-bare token opens only if it exists.
fn should_open_as_path_argument(arg: &str, env: &ClassifyEnv<'_>) -> bool {
    if looks_like_path(arg) {
        return true;
    }
    if arg.starts_with('-') || is_top_level_command(arg) {
        return false;
    }
    (env.path_exists)(&resolve_path(arg, env.cwd))
}

/// Split `args` at the first `--` into the pre-separator head and the derived
/// `arguments` list (`head` with `--json` removed, then the post-`--` tail).
/// Mirrors the shared `parsedDocsSettingsArguments` helper.
fn docs_settings_arguments(args: &[String]) -> (&[String], Vec<String>) {
    let separator = args.iter().position(|arg| arg == "--");
    let (head, tail): (&[String], &[String]) = match separator {
        Some(index) => (&args[..index], &args[index + 1..]),
        None => (args, &[]),
    };
    let arguments: Vec<String> = head
        .iter()
        .filter(|arg| *arg != "--json")
        .chain(tail.iter())
        .cloned()
        .collect();
    (head, arguments)
}

/// Whether the pre-separator `head` carries a help request: an exact `--help` /
/// `-h` token, or a leading positional `help` (with `--json` ignored for the
/// positional check). Mirrors the shared `helpRequested` helper.
fn has_help_request(head: &[String]) -> bool {
    let has_flag = head.iter().any(|arg| arg == "--help" || arg == "-h");
    let leading_positional_help = head
        .iter()
        .find(|arg| *arg != "--json")
        .map(|arg| arg.to_lowercase())
        .as_deref()
        == Some("help");
    has_flag || leading_positional_help
}

/// Whether a `settings` invocation can run without a socket: a help request, or
/// a `path`/`paths`/`docs`/`documentation` subcommand. The default (no
/// subcommand) is `open`, which needs a socket.
fn settings_command_does_not_need_socket(args: &[String]) -> bool {
    let (head, arguments) = docs_settings_arguments(args);
    let subcommand = arguments
        .first()
        .map(|arg| arg.to_lowercase())
        .unwrap_or_else(|| "open".to_owned());
    has_help_request(head)
        || matches!(
            subcommand.as_str(),
            "path" | "paths" | "docs" | "documentation"
        )
}

/// Whether a `config` invocation can run without a socket. The font-size keys
/// short-circuit (no-socket only as a one-arg `get`), *before* the help check —
/// but note the top-level help gate fires earlier still, so `config
/// sidebar-font-size --help` routes to `SubcommandHelp`, not here. The default
/// (no subcommand) is `help`, which needs no socket; `set` / `reload` do.
fn config_command_does_not_need_socket(args: &[String]) -> bool {
    let (head, arguments) = docs_settings_arguments(args);
    let subcommand = arguments
        .first()
        .map(|arg| arg.to_lowercase())
        .unwrap_or_else(|| "help".to_owned());
    if subcommand == "get" {
        return true;
    }
    if subcommand == "sidebar-font-size" || subcommand == "surface-tab-bar-font-size" {
        return arguments.len() == 1;
    }
    has_help_request(head)
        || matches!(
            subcommand.as_str(),
            "help" | "path" | "paths" | "docs" | "documentation" | "doctor" | "check" | "validate"
        )
}

/// Exact, case-sensitive membership in the top-level command set.
fn is_top_level_command(name: &str) -> bool {
    TOP_LEVEL_COMMAND_NAMES.contains(&name)
}

/// Whether `command` has a subcommand usage entry (→ `SubcommandHelp`; absent →
/// `UnknownCommandHelp`). Independent of [`TOP_LEVEL_COMMAND_NAMES`].
fn command_has_usage_entry(command: &str) -> bool {
    SUBCOMMAND_USAGE_COMMANDS.contains(&command)
}

/// The authoritative top-level command names (verbatim from
/// `cmux.swift` `topLevelCommandNames`, 156 entries).
const TOP_LEVEL_COMMAND_NAMES: &[&str] = &[
    "__codex-teams-watch",
    "__tmux-compat",
    "agent-hibernation",
    "auth",
    "bind-key",
    "break-pane",
    "browser",
    "browser-back",
    "browser-forward",
    "browser-reload",
    "browser-status",
    "capabilities",
    "capture-pane",
    "claude-hook",
    "claude-teams",
    "clear-agent-pid",
    "clear-history",
    "clear-log",
    "clear-meta",
    "clear-meta-block",
    "clear-pr",
    "clear-notifications",
    "clear-progress",
    "clear-status",
    "close-surface",
    "close-window",
    "close-workspace",
    "close-workspaces",
    "cloud",
    "codex",
    "codex-hook",
    "codex-teams",
    "config",
    "copy-mode",
    "current-window",
    "current-workspace",
    "debug-terminals",
    "detach-tab",
    "diff",
    "disable-browser",
    "dismiss-notification",
    "display-message",
    "docs",
    "drag-surface-to-split",
    "enable-browser",
    "events",
    "extension-sidebar-snapshot",
    "feedback",
    "feed",
    "feed-hook",
    "find-window",
    "focus-pane",
    "focus-panel",
    "focus-webview",
    "focus-window",
    "get-url",
    "help",
    "hooks",
    "identify",
    "is-webview-focused",
    "join-pane",
    "jump-to-unread",
    "last-pane",
    "last-window",
    "list-buffers",
    "list-log",
    "list-meta",
    "list-meta-blocks",
    "list-notifications",
    "list-pane-surfaces",
    "list-panels",
    "list-panes",
    "list-status",
    "list-windows",
    "list-workspaces",
    "log",
    "login",
    "logout",
    "markdown",
    "mark-notification-read",
    "memory",
    "mobile",
    "move-surface",
    "move-tab-to-new-workspace",
    "move-workspace-to-window",
    "navigate",
    "new-browser-workspace",
    "new-pane",
    "new-split",
    "new-surface",
    "new-terminal-tab",
    "new-window",
    "new-workspace",
    "next-window",
    "notify",
    "omc",
    "omo",
    "omx",
    "open",
    "open-browser",
    "open-notification",
    "paste-buffer",
    "ping",
    "pipe-pane",
    "popup",
    "previous-window",
    "read-screen",
    "refresh-surfaces",
    "reload-config",
    "report-meta",
    "report-meta-block",
    "report-pr",
    "report-review",
    "report-shell-state",
    "report-tty",
    "remote-daemon-status",
    "rename-tab",
    "rename-window",
    "rename-workspace",
    "reorder-surface",
    "reorder-workspace",
    "reorder-workspaces",
    "resize-pane",
    "respawn-pane",
    "reopen-closed-browser-tab",
    "restore-session",
    "restore-previous-launch",
    "right-sidebar",
    "rpc",
    "reset-sidebar",
    "select-workspace",
    "send",
    "send-key",
    "send-key-panel",
    "send-panel",
    "set-app-focus",
    "set-agent-pid",
    "set-buffer",
    "set-hook",
    "set-meta",
    "set-meta-block",
    "set-progress",
    "set-status",
    "settings",
    "setup-hooks",
    "shortcuts",
    "sidebar",
    "sidebar-state",
    "sidebar-snapshot",
    "simulate-app-active",
    "split-off",
    "split-browser",
    "ssh",
    "ssh-pty-attach",
    "ssh-session-attach",
    "ssh-session-cleanup",
    "ssh-session-end",
    "ssh-session-list",
    "ssh-tmux",
    "surface",
    "surface-health",
    "surface-resume",
    "swap-pane",
    "tab-action",
    "themes",
    "top",
    "tree",
    "trigger-flash",
    "unbind-key",
    "uninstall-hooks",
    "version",
    "vm",
    "vm-pty-attach",
    "vm-pty-connect",
    "vm-ssh-attach",
    "wait-for",
    "welcome",
    "workspace",
    "workspace-action",
    "workspace-group",
];

/// Commands that have a subcommand usage entry (verbatim from the
/// `subcommandUsage` switch labels; aliases share an entry). Independent of
/// [`TOP_LEVEL_COMMAND_NAMES`] — e.g. `canvas` / `simulate-sidebar-drag` appear
/// here but not there.
const SUBCOMMAND_USAGE_COMMANDS: &[&str] = &[
    "remotes",
    "remote",
    "ping",
    "capabilities",
    "canvas",
    "events",
    "auth",
    "login",
    "logout",
    "vm",
    "cloud",
    "rpc",
    "help",
    "docs",
    "settings",
    "config",
    "welcome",
    "shortcuts",
    "disable-browser",
    "enable-browser",
    "browser-status",
    "agent-hibernation",
    "restore-session",
    "sessions",
    "session-debug",
    "feedback",
    "feed",
    "hooks",
    "themes",
    "claude-teams",
    "codex-teams",
    "omo",
    "omx",
    "omc",
    "identify",
    "list-windows",
    "current-window",
    "new-window",
    "focus-window",
    "close-window",
    "move-workspace-to-window",
    "move-surface",
    "reorder-surface",
    "reorder-workspace",
    "reorder-workspaces",
    "simulate-sidebar-drag",
    "workspace-action",
    "tab-action",
    "move-tab-to-new-workspace",
    "detach-tab",
    "rename-tab",
    "close-workspaces",
    "new-browser-workspace",
    "new-terminal-tab",
    "split-browser",
    "reopen-closed-browser-tab",
    "restore-previous-launch",
    "new-workspace",
    "list-workspaces",
    "workspace",
    "workspace-group",
    "ssh",
    "ssh-tmux",
    "ssh-session-list",
    "ssh-session-attach",
    "ssh-session-cleanup",
    "remote-daemon-status",
    "new-split",
    "list-panes",
    "list-pane-surfaces",
    "tree",
    "top",
    "memory",
    "focus-pane",
    "new-pane",
    "new-surface",
    "close-surface",
    "drag-surface-to-split",
    "split-off",
    "refresh-surfaces",
    "reload-config",
    "surface-health",
    "surface",
    "surface-resume",
    "debug-terminals",
    "trigger-flash",
    "list-panels",
    "focus-panel",
    "close-workspace",
    "select-workspace",
    "rename-workspace",
    "rename-window",
    "current-workspace",
    "capture-pane",
    "resize-pane",
    "pipe-pane",
    "wait-for",
    "swap-pane",
    "break-pane",
    "join-pane",
    "next-window",
    "previous-window",
    "last-window",
    "last-pane",
    "find-window",
    "clear-history",
    "set-hook",
    "popup",
    "bind-key",
    "unbind-key",
    "copy-mode",
    "set-buffer",
    "paste-buffer",
    "list-buffers",
    "respawn-pane",
    "display-message",
    "read-screen",
    "send",
    "send-key",
    "send-panel",
    "send-key-panel",
    "notify",
    "list-notifications",
    "dismiss-notification",
    "mark-notification-read",
    "open-notification",
    "jump-to-unread",
    "clear-notifications",
    "set-agent-pid",
    "clear-agent-pid",
    "set-status",
    "clear-status",
    "list-status",
    "report-pr",
    "report-review",
    "report-shell-state",
    "report-tty",
    "clear-pr",
    "report-meta",
    "set-meta",
    "clear-meta",
    "list-meta",
    "report-meta-block",
    "set-meta-block",
    "clear-meta-block",
    "list-meta-blocks",
    "reset-sidebar",
    "set-progress",
    "clear-progress",
    "log",
    "clear-log",
    "list-log",
    "sidebar-state",
    "sidebar-snapshot",
    "extension-sidebar-snapshot",
    "right-sidebar",
    "sidebar",
    "set-app-focus",
    "simulate-app-active",
    "claude-hook",
    "codex",
    "browser",
    "open-browser",
    "navigate",
    "browser-back",
    "browser-forward",
    "browser-reload",
    "get-url",
    "focus-webview",
    "is-webview-focused",
    "open",
    "diff",
    "markdown",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn args(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|s| (*s).to_owned()).collect()
    }

    /// A classify env over `/w` whose path-existence answer is fixed.
    fn classify(
        command: &str,
        tokens: &[&str],
        socket: Option<&str>,
        exists: bool,
    ) -> PreSocketAction {
        let cwd = Path::new("/w");
        let path_exists = move |_: &Path| exists;
        let env = ClassifyEnv {
            cwd,
            path_exists: &path_exists,
        };
        classify_command(command, &args(tokens), socket, &env)
    }

    #[test]
    fn looks_like_path_matches_posix_and_windows_shapes() {
        for yes in [
            ".", "..", "/abs", "./rel", "../rel", "~", "~/p", "a/b", ".\\rel", "a\\b", "C:\\x",
        ] {
            assert!(looks_like_path(yes), "{yes:?} should look like a path");
        }
        for no in ["...", "a.b", "foo.txt", "open", "-h", "version"] {
            assert!(!looks_like_path(no), "{no:?} should not look like a path");
        }
    }

    #[test]
    fn resolve_path_joins_relative_keeps_absolute() {
        assert_eq!(resolve_path("rel", Path::new("/w")), Path::new("/w/rel"));
        assert_eq!(resolve_path("/abs", Path::new("/w")), Path::new("/abs"));
    }

    #[test]
    fn should_open_respects_command_and_flag_guards() {
        let cwd = Path::new("/w");
        let always = |_: &Path| true;
        let never = |_: &Path| false;
        let env_exists = ClassifyEnv {
            cwd,
            path_exists: &always,
        };
        let env_missing = ClassifyEnv {
            cwd,
            path_exists: &never,
        };

        // Path-shaped token short-circuits before the command guard.
        assert!(should_open_as_path_argument("./open", &env_missing));
        // Known command / flag never open.
        assert!(!should_open_as_path_argument("open", &env_exists));
        assert!(!should_open_as_path_argument("-h", &env_exists));
        // Bare token opens only if it exists (case-sensitive: "Open" is not a command).
        assert!(should_open_as_path_argument("Open", &env_exists));
        assert!(!should_open_as_path_argument("bogus", &env_missing));
        assert!(should_open_as_path_argument("bogus", &env_exists));
    }

    #[test]
    fn version_precedes_help_gate() {
        assert_eq!(
            classify("version", &["--help"], None, false),
            PreSocketAction::BareVersion
        );
    }

    #[test]
    fn help_gate_routes_known_and_unknown() {
        assert_eq!(
            classify("docs", &["--help"], None, false),
            PreSocketAction::SubcommandHelp {
                command: "docs".to_owned()
            }
        );
        assert_eq!(
            classify("send", &["-h"], None, false),
            PreSocketAction::SubcommandHelp {
                command: "send".to_owned()
            }
        );
        assert_eq!(
            classify("bogus", &["--help"], None, false),
            PreSocketAction::UnknownCommandHelp {
                command: "bogus".to_owned()
            }
        );
    }

    #[test]
    fn feed_tui_help_reaches_the_nested_parser() {
        assert_eq!(
            classify("feed", &["tui", "--help"], None, false),
            PreSocketAction::NeedsSocket
        );
    }

    #[test]
    fn help_gate_excludes_tmux_compat_and_after_separator_and_inexact() {
        // __tmux-compat is excluded from the gate.
        assert_eq!(
            classify("__tmux-compat", &["--help"], None, false),
            PreSocketAction::NeedsSocket
        );
        // A help flag after `--` does not fire the gate (docs has no other pre-`--` route here).
        assert_eq!(
            classify("send", &["--", "--help"], None, false),
            PreSocketAction::NeedsSocket
        );
        // Inexact flag does not match (docs falls to its name route).
        assert_eq!(
            classify("docs", &["--help=true"], None, false),
            PreSocketAction::Docs
        );
    }

    #[test]
    fn settings_no_socket_predicate_and_routing() {
        assert!(!settings_command_does_not_need_socket(&args(&[]))); // default "open"
        assert!(settings_command_does_not_need_socket(&args(&["path"])));
        assert!(settings_command_does_not_need_socket(&args(&[
            "--", "docs"
        ]))); // subcommand from tail
        assert_eq!(
            classify("settings", &[], None, false),
            PreSocketAction::NeedsSocket
        );
        assert_eq!(
            classify("settings", &["path"], None, false),
            PreSocketAction::SettingsNoSocket
        );
        // Help gate precedes the predicate.
        assert_eq!(
            classify("settings", &["--help"], None, false),
            PreSocketAction::SubcommandHelp {
                command: "settings".to_owned()
            }
        );
        assert_eq!(
            classify("settings", &["account"], None, false),
            PreSocketAction::NeedsSocket
        );
    }

    #[test]
    fn config_no_socket_predicate_and_routing() {
        assert!(config_command_does_not_need_socket(&args(&[]))); // default "help"
        assert!(config_command_does_not_need_socket(&args(&["get", "k"])));
        assert!(config_command_does_not_need_socket(&args(&[
            "--json", "get"
        ]))); // --json stripped
        assert!(config_command_does_not_need_socket(&args(&[
            "sidebar-font-size"
        ]))); // one-arg get
        assert!(!config_command_does_not_need_socket(&args(&[
            "sidebar-font-size",
            "14"
        ]))); // set
        assert!(!config_command_does_not_need_socket(&args(&[
            "set", "k", "v"
        ])));
        assert!(config_command_does_not_need_socket(&args(&["--", "help"])));

        assert_eq!(
            classify("config", &[], None, false),
            PreSocketAction::ConfigNoSocket
        );
        assert_eq!(
            classify("config", &["set", "k", "v"], None, false),
            PreSocketAction::NeedsSocket
        );
    }

    #[test]
    fn font_size_help_gate_wins_over_predicate() {
        // Predicate in isolation: font-size + extra arg → needs socket.
        assert!(!config_command_does_not_need_socket(&args(&[
            "sidebar-font-size",
            "--help"
        ])));
        // Integrated: the help gate fires first → SubcommandHelp (config has a usage entry).
        assert_eq!(
            classify("config", &["sidebar-font-size", "--help"], None, false),
            PreSocketAction::SubcommandHelp {
                command: "config".to_owned()
            }
        );
    }

    #[test]
    fn window_default_display_is_between_settings_and_config() {
        assert_eq!(
            classify("window", &["default-display"], None, false),
            PreSocketAction::WindowDefaultDisplay
        );
        assert_eq!(
            classify("window", &["Default-Display"], None, false),
            PreSocketAction::WindowDefaultDisplay
        );
        assert_eq!(
            classify("window", &["list"], None, false),
            PreSocketAction::NeedsSocket
        );
    }

    #[test]
    fn path_open_gated_on_explicit_socket() {
        // Existing path with no --socket → OpenPath.
        assert_eq!(
            classify("myproj", &[], None, true),
            PreSocketAction::OpenPath {
                path: "myproj".to_owned()
            }
        );
        // Same with --socket present → NeedsSocket (path-open suppressed).
        assert_eq!(
            classify("myproj", &[], Some("\\\\.\\pipe\\x"), true),
            PreSocketAction::NeedsSocket
        );
        // Path-shaped token opens even when it doesn't exist (no --socket).
        assert_eq!(
            classify("./open", &[], None, false),
            PreSocketAction::OpenPath {
                path: "./open".to_owned()
            }
        );
        // ...but with --socket, even a path-shaped token needs the socket path.
        assert_eq!(
            classify("./x", &[], Some("/s"), false),
            PreSocketAction::NeedsSocket
        );
    }

    #[test]
    fn sessions_debug_flag() {
        assert_eq!(
            classify("sessions", &[], None, false),
            PreSocketAction::Sessions { debug: false }
        );
        assert_eq!(
            classify("session-debug", &[], None, false),
            PreSocketAction::Sessions { debug: true }
        );
    }

    #[test]
    fn mapped_control_commands_need_socket_even_when_a_same_named_path_exists() {
        assert_eq!(
            classify("list-workspaces", &[], None, false),
            PreSocketAction::NeedsSocket
        );
        for command in [
            "close-workspaces",
            "new-browser-workspace",
            "new-terminal-tab",
            "reopen-closed-browser-tab",
            "restore-previous-launch",
            "split-browser",
        ] {
            assert_eq!(
                classify(command, &[], None, true),
                PreSocketAction::NeedsSocket,
                "{command}"
            );
        }
    }

    #[test]
    fn command_sets_are_independent() {
        // canvas has a usage entry but is not a top-level command.
        assert!(command_has_usage_entry("canvas"));
        assert!(!is_top_level_command("canvas"));
        // A top-level command without a usage entry.
        assert!(is_top_level_command("__codex-teams-watch"));
        assert!(!command_has_usage_entry("__codex-teams-watch"));
    }
}
