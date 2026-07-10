# cmux CLI Contract

This document is the compatibility contract for migrating `CLI/cmux.swift` to
Swift ArgumentParser. The migration should preserve command names, aliases,
global flags, exit behavior, socket routing, and no-socket help behavior unless
a PR explicitly calls out an intentional contract change.

The current implementation is a hand-rolled parser. This spec is deliberately
written around user-visible behavior so the implementation can change behind it.

## Migration Rules

- Keep `cmux --help`, `cmux -h`, `cmux --version`, and `cmux -v` working without
  connecting to the cmux socket.
- Keep documented `cmux <command> --help` probes working without a socket where
  they already do.
- Keep `--socket`, `--password`, and `--window` as global options before the
  command. Keep presentation options `--json` and `--id-format` accepted either
  before or after the command.
- Keep UUIDs, refs such as `workspace:2`, and indexes accepted wherever the
  command accepts a window, workspace, pane, surface, or tab handle.
- Keep text output stable for scripting commands unless a command already
  documents JSON as the scripting interface.
- Keep hidden/internal commands available until their callers have migrated.

## Global Invocation

| Form | Contract |
| --- | --- |
| `cmux <path>` | Open a directory or file parent in cmux through the app's file-open path, without requiring control-socket access. Relative paths resolve from the current working directory. |
| `cmux [global-options] <command> [options]` | Run a named command. Presentation options may appear before or after the command. |
| `cmux --help`, `cmux -h` | Print top-level usage without a socket. |
| `cmux help` | Print top-level usage without a socket. |
| `cmux --version`, `cmux -v`, `cmux version` | Print version summary without a socket. |

Global options:

| Option | Contract |
| --- | --- |
| `--socket <path>` | Override the socket path for this invocation. |
| `--password <value>` | Use an explicit socket password. Takes precedence over `CMUX_SOCKET_PASSWORD`. |
| `--json` | Prefer machine-readable JSON output for commands that support it. |
| `--id-format <refs\|uuids\|both>` | Select handle format in JSON and supported text output. |
| `--window <id\|ref\|index>` | Route the command through a specific window when supported. |

Environment:

| Variable | Contract |
| --- | --- |
| `CMUX_SOCKET_PATH` | Canonical socket path override. |
| `CMUX_SOCKET` | Deprecated compatibility alias for `CMUX_SOCKET_PATH`. New scripts should use `CMUX_SOCKET_PATH`; if both variables are set and differ, the CLI fails before socket commands. |
| `CMUX_SOCKET_PASSWORD` | Socket password fallback when `--password` is absent. |
| `CMUX_WORKSPACE_ID` | Default workspace context inside cmux terminals. |
| `CMUX_SURFACE_ID` | Default surface context inside cmux terminals. |
| `CMUX_TAB_ID` | Default tab context for tab commands. |

## Top-Level Commands

| Command | Contract |
| --- | --- |
| `welcome` | Print the welcome screen. |
| `docs` | Print canonical docs URLs, raw GitHub resources, and useful commands for a topic. |
| `settings` | Open Settings, print cmux.json paths, or print settings docs. |
| `config` | Validate cmux.json syntax, print config references, or reload config. |
| `shortcuts` | Open Settings to Keyboard Shortcuts. |
| `disable-browser` | Disable cmux browser creation and link interception until re-enabled. |
| `enable-browser` | Re-enable cmux browser creation and link interception. |
| `browser-status` | Print whether cmux browser creation and link interception are enabled. |
| `agent-hibernation` | Enable or disable Agent Hibernation. |
| `restore-session` | Restore the previously saved cmux session. |
| `open` | Open files, directories, or URLs in cmux. |
| `feedback` | Open feedback UI or submit feedback with `--email`, `--body`, and repeated `--image`. |
| `feed` | Open the keyboard-first Feed TUI or manage persisted Feed workstream history. |
| `themes` | List, set, clear, or interactively pick Ghostty themes. |
| `claude-teams` | Launch Claude Code with cmux/tmux-style agent team integration. |
| `codex-teams` | Launch Codex with cmux-managed subagent panes. |
| `omo` | Launch OpenCode with oh-my-openagent integration. |
| `omx` | Launch Oh My Codex with cmux pane integration. |
| `omc` | Launch Oh My Claude Code with cmux pane integration. |
| `hooks` | Install, uninstall, and run agent hook integrations under one namespace. |
| `codex` | Compatibility alias for installing or uninstalling Codex hooks. |
| `ping` | Check socket connectivity. |
| `capabilities` | Print server capabilities as JSON. |
| `events` | Stream reconnectable cmux events as newline-delimited JSON. |
| `sidebar-snapshot`, `extension-sidebar-snapshot` | Print the rich `extension.sidebar.snapshot` payload used by custom sidebars and event-stream catch-up, including the interpreter-ready `data` tree. |
| `auth` | Manage auth status, login, and logout through the app. |
| `vm`, `cloud` | Manage cloud VMs. `cloud` is an alias for `vm`. |
| `remotes`, `remote` | Manage remote Macs in the team device registry so they appear in the iOS app's device list. `remote` is an alias for `remotes`. |
| `rpc` | Call a raw v2 socket method with optional JSON params. |
| `identify` | Print server identity and caller context. |
| `list-windows` | List windows. |
| `current-window` | Print the selected window ID. |
| `new-window` | Create a new window. |
| `focus-window` | Focus a window by handle. |
| `close-window` | Close a window by handle. |
| `window displays` | List connected displays (name, index, main flag). |
| `window display <name\|index>` | Move the instance's window(s) onto a display by name (exact, substring) or index, preserving size. Does not steal focus. With `--window`, targets that window; otherwise moves all main windows. `--list` aliases `window displays`. |
| `window default-display [<name>\|--clear]` | Set, show (no arg), or clear (`--clear`) the shared, cross-tag default display that DEBUG dev builds open new windows on, stored in `~/.config/cmux/cmux.json` under `app.devWindowDisplay`. No running app required; applied at window creation. Also settable in Debug > Debug Windows > Dev Window Display. |
| `move-workspace-to-window` | Move a workspace into a target window. |
| `reorder-workspace` | Reorder a workspace inside a window. |
| `reorder-workspaces` | Atomically reorder workspaces inside pinned and unpinned groups. |
| `workspace-action` | Run workspace context-menu actions from the CLI. |
| `workspace` | Namespace for workspace verbs: `list`, `create`, `env`, `close`, `rename`, `select`, `reconnect`, `disconnect`, `group`. `workspace env` prints a workspace's configured environment variables (see [Workspace environment variables](#workspace-environment-variables)); pass `--mask` to redact the values. `workspace reconnect` manually reconnects a remote (SSH) workspace — including one whose automatic reconnect suspended because the host was unreachable — and `workspace disconnect` stops its remote connection. `env`, `reconnect`, and `disconnect` accept a positional workspace handle or `--workspace <id\|ref\|index>`, defaulting to the caller's workspace, then the selected one. |
| `move-tab-to-new-workspace` | Move a tab or surface into a newly created workspace. |
| `list-workspaces` | List workspaces. |
| `new-workspace` | Create a workspace, optionally with cwd, command, description, layout, and per-workspace environment variables (`--env KEY=VALUE` repeatable, `--env-file <path>`). See [Workspace environment variables](#workspace-environment-variables). |
| `ssh` | Open an SSH-backed workspace. Preserves the caller's live `SSH_AUTH_SOCK` for app-launched OpenSSH processes so `ForwardAgent yes` from ssh_config works normally. Supports `-A` / `--forward-agent` to request forwarding and `-a` / `--no-forward-agent` to disable forwarding for a workspace. Agent forwarding remains opt-in because forwarded agents can be used by processes on the remote host while the SSH session is active. |
| `remote-daemon-status` | Print bundled remote daemon version, asset, checksum, and cache status. |
| `ssh-session-list` | List persisted SSH PTY sessions for one remote workspace or all remote workspaces. Supports `--json`. |
| `ssh-session-attach` | Create a local terminal surface that reattaches to an existing persisted SSH PTY session. |
| `ssh-session-cleanup` | Close one or all persisted SSH PTY sessions. Supports `--json`. |
| `new-split` | Split from a surface in a direction. |
| `list-panes` | List panes in a workspace. |
| `list-pane-surfaces` | List surfaces in a pane. |
| `tree` | Print a window, workspace, pane, and surface tree. |
| `top` | Print process/resource usage for cmux windows, workspaces, panes, and surfaces. |
| `focus-pane` | Focus a pane. |
| `new-pane` | Create a pane with terminal or browser content. |
| `new-surface` | Create a surface inside a pane. |
| `close-surface` | Close a surface. |
| `move-surface` | Move a surface to another pane, workspace, window, or index. |
| `split-off` | Move a surface into a new split without changing focus by default. |
| `reorder-surface` | Reorder a surface within its pane. |
| `tab-action` | Run horizontal tab context-menu actions. |
| `rename-tab` | Rename a tab. Compatibility wrapper for `tab-action rename`. |
| `drag-surface-to-split` | Move a surface into a split direction. |
| `refresh-surfaces` | Ask the app to refresh terminal surfaces. |
| `reload-config` | Ask cmux to reload configuration. |
| `surface-health` | Print terminal surface health information. |
| `debug-terminals` | Print debug terminal state. |
| `trigger-flash` | Trigger a visual flash on a workspace or surface. |
| `list-panels` | List panels. Compatibility alias over pane/surface data. |
| `focus-panel` | Focus a panel. Compatibility alias over surface focus. |
| `close-workspace` | Close a workspace. |
| `select-workspace` | Select a workspace. |
| `rename-workspace`, `rename-window` | Rename a workspace. `rename-window` is a compatibility alias. |
| `current-workspace` | Print current workspace information. |
| `read-screen` | Read terminal text from a surface. |
| `send` | Send text to a terminal surface. |
| `send-key` | Send one key to a terminal surface. |
| `send-panel` | Send text to a panel/surface. |
| `send-key-panel` | Send one key to a panel/surface. |
| `notify` | Send a notification to a workspace/surface. |
| `list-notifications` | List queued notifications, including `created_at` and `tab_title`. |
| `dismiss-notification` | Remove one notification, or remove already-read notifications with `--all-read`. |
| `mark-notification-read` | Mark one notification, a workspace/surface scope, or all notifications read. |
| `open-notification` | Focus the notification's workspace/surface and mark it read. |
| `jump-to-unread` | Focus the latest unread notification. |
| `clear-notifications` | Clear queued notifications. |
| `right-sidebar` | Control right sidebar visibility, mode, focus, and state reads. |
| `set-status` | Set a sidebar status pill. |
| `clear-status` | Remove a sidebar status pill. |
| `list-status` | List sidebar status pills. |
| `set-agent-pid` | Register an agent root PID so descendant listening ports can appear in the sidebar. |
| `clear-agent-pid` | Clear a registered agent root PID and refresh sidebar agent ports. |
| `report-tty` | Report a terminal TTY name for a workspace surface. |
| `report-shell-state` | Report whether a workspace surface shell is idle at a prompt, running a command, or unknown. |
| `report-pr` | Report pull-request metadata for sidebar display. |
| `report-review` | Report provider-specific review metadata for sidebar display. |
| `clear-pr` | Remove pull-request/review metadata from a sidebar surface. |
| `report-meta`, `set-meta` | Set a rich sidebar metadata entry. |
| `clear-meta` | Remove a rich sidebar metadata entry. |
| `list-meta` | List rich sidebar metadata entries. |
| `report-meta-block`, `set-meta-block` | Set a markdown sidebar metadata block. |
| `clear-meta-block` | Remove a markdown sidebar metadata block. |
| `list-meta-blocks` | List markdown sidebar metadata blocks. |
| `reset-sidebar` | Clear sidebar progress, status, metadata, metadata blocks, and log entries. |
| `set-progress` | Set sidebar progress. |
| `clear-progress` | Clear sidebar progress. |
| `log` | Append a sidebar log entry. |
| `clear-log` | Clear sidebar log entries. |
| `list-log` | List sidebar log entries. |
| `sidebar-state` | Dump sidebar metadata state. |
| `claude-hook` | Compatibility alias for Claude Code hook events from stdin JSON. |
| `set-app-focus` | Override app focus state for tests. |
| `simulate-app-active` | Trigger app-active handling for tests. |
| `browser` | Run browser automation commands. |
| `open-browser` | Legacy alias for `browser open`. |
| `navigate` | Legacy alias for `browser navigate`. |
| `browser-back` | Legacy alias for `browser back`. |
| `browser-forward` | Legacy alias for `browser forward`. |
| `browser-reload` | Legacy alias for `browser reload`. |
| `get-url` | Legacy alias for `browser get-url`. |
| `focus-webview` | Legacy alias for `browser focus-webview`. |
| `is-webview-focused` | Legacy alias for `browser is-webview-focused`. |
| `markdown` | Open a markdown file in a formatted viewer panel with live reload. |
| `vm-pty-attach` | Internal VM PTY attach command. |
| `vm-ssh-attach` | Hidden compatibility alias for older VM workspaces. |
| `vm-pty-connect` | Internal helper that connects to a VM PTY from a config file. |
| `ssh-pty-attach` | Internal helper used by SSH terminal startup scripts to bridge a local terminal surface to a remote PTY session. |
| `ssh-session-end` | Internal helper that clears remote SSH session state. |
| `__tmux-compat` | Internal tmux compatibility dispatcher. |

## Command Families

Auth subcommands:

| Command | Contract |
| --- | --- |
| `auth status` | Print signed-in state. Supports `--json`. |
| `auth login` | Begin sign-in through the app and wait for completion. |
| `auth logout` | Clear the current session. |

VM subcommands:

| Command | Contract |
| --- | --- |
| `vm ls`, `vm list` | List VMs. |
| `vm new`, `vm create` | Create a VM. Supports `--image`, `--provider`, `--detach`, and `-d`. |
| `vm shell`, `vm attach` | Open an interactive shell for an existing VM. |
| `vm rm`, `vm destroy`, `vm delete` | Destroy a VM. |
| `vm ssh` | Open a cmux-managed SSH workspace for an existing VM. |
| `vm ssh-info` | Print SSH connection info. |
| `vm ssh-attach` | Internal attach helper. |
| `vm exec` | Run a shell command inside a VM. |

Remotes subcommands:

| Command | Contract |
| --- | --- |
| `remotes list`, `remotes ls` | List the team's registered remotes (name, deviceId, routes, tag, last seen). Supports `--json`. |
| `remotes add <name>` | Register or update a remote with one or more `--route <host:port>`. Supports `--tag` and `--json`. Idempotent on `<name>` (re-adding updates routes). The host must be a Tailscale address the phone can authenticate to (CGNAT `100.64.x.x`-`100.127.x.x` or `*.ts.net`); loopback, plain LAN IPs, and bare hostnames are rejected. |
| `remotes remove <name-or-deviceId>` | Remove a remote you registered. Aliases `rm`, `delete`. Supports `--json`. |

Theme subcommands:

| Command | Contract |
| --- | --- |
| `themes` | In a TTY, open the interactive picker. Outside a TTY, list themes. |
| `themes list` | List available themes and current light/dark defaults. |
| `themes set <theme>` | Set the same theme for light and dark appearance. |
| `themes set --light <theme>` | Set the light appearance theme. |
| `themes set --dark <theme>` | Set the dark appearance theme. |
| `themes clear` | Remove the cmux theme override. |

Workspace and tab action names:

| Command | Actions |
| --- | --- |
| `workspace-action` | `pin`, `unpin`, `rename`, `clear-name`, `set-description`, `clear-description`, `move-up`, `move-down`, `move-top`, `close-others`, `close-above`, `close-below`, `mark-read`, `mark-unread`, `set-color`, `clear-color` |
| `tab-action` | `rename`, `clear-name`, `close-left`, `close-right`, `close-others`, `new-terminal-right`, `new-browser-right`, `reload`, `duplicate`, `pin`, `unpin`, `mark-unread` |

### Workspace environment variables

A workspace can carry a set of user-defined environment variables that every
shell spawned in it inherits.

Setting them:

- CLI: `cmux new-workspace --env KEY=VALUE [--env ...] [--env-file <path>]`
  (and the same flags on `cmux workspace create`). `--env` is repeatable;
  `--env-file` reads `KEY=VALUE` lines (blank lines and `#` comments ignored, an
  optional leading `export ` stripped). When both are given, `--env` overrides a
  value from a file.
- Project config (`cmux.json`): an `env` object on a workspace definition, e.g.
  `{ "name": "Build", "cwd": ".", "env": { "AWS_PROFILE": "prod" } }`.
- Socket: the `workspace_env` param on `workspace.create`.

Inspecting them: `cmux workspace env [<handle>] [--mask] [--json]` prints the
configured set. `--mask` redacts the values so secrets are not echoed in full.
The env set is intentionally omitted from `workspace list` output so a plain
listing never leaks secrets.

Semantics:

- **Inheritance.** The variables apply to the workspace's initial shell and to
  every pane, surface, and split created later in that workspace — no per-pane
  re-export. They are also re-applied to every shell recreated on session
  restore.
- **Persistence.** They are stored on the workspace in the session manifest, so
  they survive app restart, daemon restart, and session restore.
- **Precedence.** Workspace env overlays the inherited process environment. It is
  applied as the shell's startup environment, so it is visible to login-shell
  init files (`~/.zprofile`, `~/.zshrc`) as they run, but any `export` those
  files perform for the same key wins for the interactive session (they run after
  the variable is seeded). An explicit per-surface environment (a layout
  `surfaces[].env`, SSH startup env) overrides the workspace value for that
  surface.
- **Protected `CMUX_*` variables.** Workspace env can never override the managed
  variables cmux injects (e.g. `CMUX_WORKSPACE_ID`, `CMUX_SURFACE_ID`,
  `CMUX_SOCKET_PATH`, `CMUX_SOCKET_PASSWORD`) or the terminal identity variables
  (`TERM`, `COLORTERM`, `TERM_PROGRAM`); those keys are protected at spawn time
  and silently win.
- **Secrets.** Values may be secrets. They are never logged, are masked by
  `--mask`, and are kept out of `workspace list`. Prefer `--env-file` so secrets
  do not land in shell history. Note that values stored in the session manifest
  live on disk in plaintext.

tmux compatibility commands:

| Command | Contract |
| --- | --- |
| `capture-pane` | Read pane text. |
| `resize-pane` | Resize a pane with direction flags. |
| `pipe-pane` | Pipe pane text to a shell command. |
| `wait-for` | Signal or wait on a named synchronization point. |
| `swap-pane` | Swap two panes. |
| `break-pane` | Move a pane into a new workspace. |
| `join-pane` | Join a pane into another pane. |
| `next-window`, `previous-window`, `last-window` | Move workspace selection. |
| `last-pane` | Focus the last pane. |
| `find-window` | Find a workspace by title or content. |
| `clear-history` | Clear terminal scrollback. |
| `set-hook` | Manage tmux-compat hook definitions. |
| `popup` | Placeholder, currently unsupported. |
| `bind-key`, `unbind-key`, `copy-mode` | Placeholders, currently unsupported. |
| `set-buffer` | Set a tmux-compat buffer. |
| `paste-buffer` | Paste a tmux-compat buffer. |
| `list-buffers` | List tmux-compat buffers. |
| `respawn-pane` | Send a restart command to a surface. |
| `display-message` | Print or display a message. |

Browser subcommands:

| Command | Contract |
| --- | --- |
| `browser open`, `browser open-split`, `browser new` | Create or open a browser surface. |
| `browser goto`, `browser navigate` | Navigate to a URL. |
| `browser back`, `browser forward`, `browser reload` | Navigate browser history or reload. |
| `browser url`, `browser get-url` | Print current URL. |
| `browser focus-webview`, `browser is-webview-focused` | Focus or query webview focus. |
| `browser snapshot` | Print a DOM snapshot. |
| `browser eval` | Evaluate JavaScript. |
| `browser wait` | Wait for selector, text, URL, load state, or JS predicate. |
| `browser click`, `browser dblclick`, `browser hover`, `browser focus`, `browser check`, `browser uncheck`, `browser scroll-into-view` | Run element interaction. |
| `browser type`, `browser fill` | Type into or set an input. |
| `browser press`, `browser key`, `browser keydown`, `browser keyup` | Send keyboard input. |
| `browser select` | Select an option. |
| `browser scroll` | Scroll page or element. |
| `browser screenshot` | Save a screenshot. |
| `browser get` | Read URL, title, text, HTML, value, attr, count, box, or styles. |
| `browser is` | Check visible, enabled, or checked state. |
| `browser find` | Find by role, text, label, placeholder, alt, title, testid, first, last, or nth. |
| `browser frame` | Select frame context. |
| `browser dialog` | Accept or dismiss dialogs. |
| `browser download` | Wait for or save downloads. |
| `browser profiles` | List, add, rename, clear, or delete cmux browser profiles. `clear` refuses to wipe active profiles unless `--force` is passed. |
| `browser import` | Open the browser import wizard. In detected coding-agent environments, defaults to non-interactive cookie import; pass `--interactive` to force the wizard. Non-interactive import supports `--from`, `--profile`, `--all-profiles`, `--to-profile`, `--create-profile`, and `--domain`. |
| `browser cookies` | Get, set, or clear cookies. |
| `browser storage` | Get, set, or clear local/session storage. |
| `browser tab` | Create, list, switch, or close browser tabs. |
| `browser console`, `browser errors` | List or clear console messages and errors. |
| `browser highlight` | Highlight an element. |
| `browser state` | Save or load browser state. |
| `browser addinitscript`, `browser addscript`, `browser addstyle` | Inject scripts or CSS. |
| `browser viewport` | Set viewport size. |
| `browser geolocation`, `browser geo` | Set geolocation. |
| `browser offline` | Toggle offline state. |
| `browser trace` | Start or stop trace capture. |
| `browser network` | Route, unroute, or list requests. |
| `browser screencast` | Start or stop screencast. |
| `browser input`, `browser input_mouse`, `browser input_keyboard`, `browser input_touch` | Send low-level input. |
| `browser identify` | Identify browser surface context. |

See `docs/browser-automation.md` for the agent workflow, target terminology, supported automation families, and Windows/Tauri WKWebView hard gaps. On the Windows/Tauri backend, viewport/geolocation/offline overrides, trace/screencast, network route/unroute interception, and raw input injection return explicit `not_supported` errors; Network request inspection remains supported through `browser network`.

Hook subcommands:

| Command | Contract |
| --- | --- |
| `hooks setup` | Install hooks for all supported agents whose binaries are on `PATH`. Supports `--agent <name>`, positional agent filters such as `cmux hooks setup rovo`, and `--yes`. |
| `hooks uninstall` | Remove hooks for all supported agents. Supports `--agent <name>`, positional agent filters such as `cmux hooks uninstall rovo`, and `--yes`. |
| `hooks <agent> install` | Install hooks for one supported agent. `opencode` also supports `--project` for the project-local Feed plugin. |
| `hooks <agent> uninstall` | Remove hooks for one supported agent. |
| `hooks claude <event>` | Handle Claude Code hook events. `claude-hook <event>` remains as the main-compatibility alias. |
| `hooks codex <event>` | Handle Codex hook events. `codex install-hooks` remains as the main-compatibility installer alias. |
| `hooks feed --source <agent>` | Convert agent hook events into Feed context. |
| `hooks <agent> <event>` | Generic hook surface for `grok`, `opencode`, `pi`, `amp`, `cursor`, `gemini`, `kimi`, `rovodev`, `copilot`, `codebuddy`, `factory`, and `qoder`. |

Right sidebar commands:

| Command | Contract |
| --- | --- |
| `right-sidebar toggle`, `right-sidebar show`, `right-sidebar hide` | Change right-sidebar visibility without printing on success. |
| `right-sidebar focus` | Focus the current right-sidebar mode. |
| `right-sidebar set <files\|find\|vault\|sessions\|feed\|dock>` | Show the right sidebar, switch mode, and focus it unless `--no-focus` is passed. |
| `right-sidebar files`, `right-sidebar find`, `right-sidebar vault`, `right-sidebar sessions`, `right-sidebar feed`, `right-sidebar dock` | Short aliases for `right-sidebar set <mode>` with focus. |
| `right-sidebar mode` | Print JSON with `visible` and `mode`. |
| `--workspace <id\|ref\|index>` | Target the window containing a workspace. Refs and indexes resolve before the V1 socket command is sent. |
| `--window <id\|ref\|index>` | Target a window. Refs and indexes resolve before the V1 socket command is sent. |
| `--no-focus` | Only valid with `set`; switches mode without moving focus. |

Custom sidebar commands:

| Command | Contract |
| --- | --- |
| `sidebar validate [name]` | Validate all custom sidebars, or one named sidebar, under `~/.config/cmux/sidebars`. |
| `sidebar reload [name]` | Validate all custom sidebars, then request a reload for every valid one. |
| `sidebar select <name>` | Validate and activate one custom sidebar in the sidebar picker. |
| `sidebar open <name>` | Validate and open one custom sidebar as a normal Bonsplit pane tab, preferring the right-side split from the focused surface. |

Windows/Tauri status: `sidebar list` and `sidebar validate [name]` are routed
through the desktop control socket and return structured validation for
`~/.config/cmux/sidebars` (`CMUX_SIDEBARS_DIR` may override the directory for
tests/tools). `sidebar open <name>` now validates the selected sidebar and binds
the focused pane to a `custom-sidebar` surface backed by live session data.
`.json` sidebars render through the Windows/Tauri JSON block renderer with
interactive rows plus authored action objects that invoke shared control-socket
methods through `custom_sidebar_action_invoke`; `.swift` sidebars render through
a constrained Windows/Tauri Swift subset renderer for common stack/text/button/
`Label`/`Image`/`AsyncImage`/`ProgressView`/`Gauge`/`ScrollView`/`List`/`Section`/`ZStack`/`Grid`/
`GridRow`/constrained `LazyVGrid(columns: [GridItem(...)])`/`LazyHGrid(rows: [GridItem(...)])`/
`Menu`/`Group`/`EmptyView`/`ViewThatFits(in:)`/`AnyView`/`TabView`/`TextField`/`SecureField`/`TextEditor`/`Toggle`/`Slider`/`Stepper`/`DatePicker`/`ColorPicker`/`Picker` authoring, stack `spacing:`,
sidebar-local `Form { ... }` containers,
`HSplitView`/`VSplitView` sidebar-local split panes,
`Button(role:)`, basic shapes (`Circle`, `Ellipse`, `Rectangle`,
`RoundedRectangle`, `UnevenRoundedRectangle`, `Capsule`, `ContainerRelativeShape`,
and constrained `Path(roundedRect:)` / `Path(ellipseIn:)`) with fill/stroke/strokeBorder/trim,
CSS-backed SwiftUI gradient styles (`LinearGradient`, `RadialGradient`,
`AngularGradient`, and `Color.<token>.gradient`) for background/foreground/fill,
CSS-backed Material background tokens (`.ultraThinMaterial`, `.thinMaterial`,
`.regularMaterial`, `.thickMaterial`, `.ultraThickMaterial`, and `.bar`),
`ForEach(workspaces)`,
`ForEach(workspaces.indices)`, constrained `ForEach($localCollection)` element
bindings for local `@State` arrays, simple `for` ranges, `if let`,
local `let` bindings, simple user `func` value helpers and `some View` row
helpers, `workspaces[i]` reads, array helpers (`.first`, `.last`, `.contains`,
`.reversed()`, `.prefix(n)`, `.suffix(n)`, `.dropFirst(n)`, `.dropLast(n)`,
`.enumerated()`, `.filter { ... }`, `.map { ... }`, `.flatMap { ... }`,
`.compactMap { ... }`, `.reduce(initial) { ... }`, `.sorted { ... }`,
`.min(by:)`, `.max(by:)`, `.allSatisfy(...)`,
constrained key-path transforms such as `.map(\.title)`,
`.compactMap(\.progress)`, `.flatMap(\.ports)`, and `.sorted(by: \.title)`),
constrained `Dictionary(grouping: workspaces, by: \.selected)` records,
`ForEach(Array(workspaces.enumerated()), id: \.offset)`
tuple-style closure params plus `$0`/`$1` shorthand closures, string helpers (`.count`, `.hasPrefix`,
`.hasSuffix`, `.contains`, `.uppercased()`, `.lowercased()`,
`.split(separator:)`), common numeric/string builtins (`min`, `max`, `abs`,
`Int`, `Double`, `String`, and constrained `String(format:)` for common
`%d`/`%f`/`%@`/`%s` forms), numeric formatting (`.formatted(.currency(code:))`,
`.formatted(.percent)`, `.formatted(.notation(.compactName))`, and
`.formatted(.byteCount(style: .file))` / `Text(value, format: ...)`), simple
`Measurement(value:unit:).formatted(.measurement(width: ...))` values,
deterministic `Date(timeIntervalSince1970:).formatted(.dateTime...)` values,
constrained `Text(date, style: .date/.time)` and static
`Text(timerInterval: start...end)` timer snapshots,
read-only `@Environment(\.colorScheme)`, `@Environment(\.layoutDirection)`,
and `@Environment(\.locale)` declarations with stable sidebar defaults,
static array `.formatted(.list(type: ...))` values,
arithmetic/comparison/logical expressions
(`+ - * / %`, `== != > >= < <=`, `&& || !`), nested Swift string interpolation,
`Text(verbatim:)`, static inline `Text` markdown for `**bold**`, `*italic*`,
`` `code` ``, and `[label](https://...)` links, constrained
`Text("A") + Text(value)` concatenation, dictionary literals
(`["key": value]`), dynamic keyed subscripts
(`lookup[workspace.id]`), optional-style missing-value checks against `nil`,
common visual modifiers (`.font`, `.fontWeight`, `.fontDesign`, `.fontWidth`, `.dynamicTypeSize`, `.bold`,
`.italic`, `.monospaced`, `.lineLimit`, `.truncationMode`,
`.multilineTextAlignment`, `.textCase`, `.underline`, `.strikethrough`,
`.foregroundColor`/`.foregroundStyle`, `.opacity`, `.hidden`, `.fixedSize`, `.badge`, `.allowsHitTesting`,
`.hoverEffect`, `.defaultHoverEffect`, `.disabled`,
`.help`, `.accessibilityLabel`, `.accessibilityHidden`, `.accessibilityValue`,
`.accessibilityHint`, `.accessibilityAddTraits`, `.accessibilityElement(children:)`,
`.accessibilityAction`, `.accessibilityActivationPoint`, `.accessibilityRepresentation`,
`.accessibilitySortPriority`, `.redacted`,
`.privacySensitive`, `.padding`, edge-set `.padding(.horizontal|.vertical|[...], length)`,
`.background`, `.cornerRadius`, `.layoutPriority`,
`.containerRelativeFrame(...)`,
`.safeAreaPadding(...)`, `.contentMargins(...)`,
`.coordinateSpace(name:)`, `.alignmentGuide(...)`,
`.gridCellColumns`, `.gridColumnAlignment`, `.gridCellAnchor`,
`.tracking`, `.kerning`, `.baselineOffset`,
`.offset`, `.position`, `.zIndex`, `.aspectRatio`, `.scaledToFit`, `.scaledToFill`,
`.clipShape`, `.clipped`, `.compositingGroup`, `.shadow`, `.border`, `.stroke`, `.blur`,
`.brightness`, `.contrast`, `.saturation`, `.grayscale`, `.hueRotation`,
`.blendMode`, `.rotationEffect`, `.scaleEffect`, `.rotation3DEffect`, `.visualEffect`, `.listStyle`, `.labelStyle`, `.labelsHidden`, `.menuStyle`, `.controlGroupStyle`, `.controlSize`,
`.buttonStyle`, `.buttonBorderShape`, `.pickerStyle`, `.tabViewStyle`, `.toggleStyle`,
`.textFieldStyle`, `.scrollContentBackground`, `.scrollIndicators`,
`.scrollClipDisabled`, `.scrollTargetBehavior`, `.scrollTargetLayout`,
`.scrollBounceBehavior`, `.scrollDisabled`, `.scrollPosition`,
`.defaultScrollAnchor`, `.resizable(capInsets:resizingMode:)`,
`.renderingMode`, `.interpolation`, `.antialiased`, `.imageScale`,
`.flipsForRightToLeftLayoutDirection`, `.symbolRenderingMode`, `.symbolVariant`, `.listRowBackground`,
`.listRowSeparator`, `.fill`, `.tint`, `.animation(...)`, `.transition(...)`,
`.contentTransition(...)`, `.symbolEffect(...)`, `.symbolEffectsRemoved()`, constrained
`.preferredColorScheme(...)`, `.environment(\.colorScheme, ...)`,
`.environment(\.layoutDirection, ...)`,
`.frame(width:height:minWidth:minHeight:idealWidth:idealHeight:maxWidth:maxHeight:alignment:)`,
child-bearing `.background { ... }`, `.overlay { ... }`, `.mask { ... }`,
`.safeAreaInset { ... }`, `.contextMenu { ... }`, `.refreshable { ... }`,
`.swipeActions { ... }`, `.accessibilityRepresentation { ... }`,
`.tabItem { ... }`, `.onTapGesture { cmux(...) }`,
`.onTapGesture(count:) { cmux(...) }`, `.onLongPressGesture { cmux(...) }`,
`.onSubmit { ... }`, `.onChange(of:) { ... }`, `.onAppear { ... }`,
`.onDisappear { ... }`, `.task { ... }`, `.task(id:) { ... }`, and
`.onHover { hovering in ... }`, `.onGeometryChange(for:) { ... }`,
`.focusable()`, and `.focused($localBool)`),
static `.navigationTitle(...)`, `.navigationSubtitle(...)`,
`.navigationBarTitleDisplayMode(...)`, `.toolbar { ToolbarItem { ... } }`,
`.toolbarBackground(...)`, `.toolbarColorScheme(...)`,
`.keyboardShortcut(...)`, `.keyboardShortcut(..., modifiers: [...])`,
`.contentShape(...)`, `.allowsHitTesting(...)`, `.id(...)`, static `.draggable(...)`,
`.dropDestination(for:) { cmux(...) }`,
sidebar-local `ControlGroup { ... }` grouped control containers,
sidebar-local `GroupBox("Title") { ... }` / `GroupBox(label: { ... }) { ... }`
labeled card containers,
sidebar-local `DisclosureGroup("Title") { ... }` /
`DisclosureGroup(isExpanded: $localBool) { ... } label: { ... }` collapsible
sections with direct local `@State Bool` expansion write-back,
sidebar-local `NavigationStack { ... }` with static-destination
`NavigationLink("Title") { ... }` / `NavigationLink(destination:) { ... }` plus
a constrained `NavigationLink(value:)` / `.navigationDestination(for:)` route
subset for string-like values, and
sidebar-local `Link("Title", destination: URL(string: "https://...")!)` /
`Link(destination: URL(string: "https://...")!) { ... }` external anchors for
safe `http`/`https` destinations,
sidebar-local `ContentUnavailableView(...)` empty-state panels with constrained
title/system-image, description, and action slots,
sidebar-local `.searchable(text: $localQuery, placement: .sidebar, prompt: "...")`
search fields with direct local `@State String` write-back,
sidebar-local `LabeledContent("Title", value: expr)` /
`LabeledContent("Title") { <value views> }` read-only inspector rows,
sidebar-local `.sheet(isPresented:)`, `.popover(isPresented:)`,
`.fullScreenCover(isPresented:)`, `.alert("Title", isPresented:)`, and
`.confirmationDialog("Title", isPresented:)` panels for simple local
`@State Bool` bindings, constrained `.sheet(item:)`, `.popover(item:)`,
`.fullScreenCover(item:)`, `.alert("Title", item:)`, and
`.confirmationDialog("Title", item:)` panels for optional/string-like item values,
sidebar-local `dismiss()` button/action closures inside presentation content,
`.presentationDetents(...)`, `.presentationDragIndicator(...)`,
`.presentationBackground(...)`, and `.presentationCornerRadius(...)`
panel-chrome breadcrumbs/hints on presented content,
alert/confirmation dialog action rows for direct child `Button(role:)` views,
and authored `cmux(...)` actions.
Static toolbar content preserves constrained `ToolbarItem(placement:)` tokens as
stable placement classes/data, and constrained toolbar background/color-scheme
tokens as sidebar-local nav/toolbar chrome hints plus `data-swift-toolbar-*`
breadcrumbs; native platform toolbar slotting, collapsing, automatic placement,
color propagation, visibility resolution, and customization semantics remain out
of scope.
Constrained `.labelsHidden()` preserves authored label-hiding intent as
`data-swift-labels-hidden` metadata and applies sidebar-local visually-hidden
label chrome for common controls; full inherited SwiftUI label-style propagation
and exact platform label layout remain follow-up.
Constrained `.controlGroupStyle(...)` preserves built-in style tokens as
`data-swift-control-group-style` metadata and sidebar-local grouped-control
chrome hints; custom `ControlGroupStyle` structs and native platform menu/palette
behavior remain follow-up.
Constrained `.groupBoxStyle(...)` preserves built-in style tokens as
`data-swift-group-box-style` metadata and sidebar-local card chrome hints;
custom `GroupBoxStyle` structs and native platform group-box styling remain
follow-up.
`DisclosureGroup` emits stable `data-swift-disclosure-*` breadcrumbs and uses
the existing direct local state path for constrained `isExpanded: $localBool`
bindings; native `DisclosureGroupStyle`, outline/list integration, and broader
binding semantics remain follow-up.
Constrained `.searchable(text:prompt:placement:)` emits stable
`data-swift-search-*` breadcrumbs and uses the existing direct local state path
for `@State String` text bindings; native suggestions, tokens, scopes, platform
search-field placement, and broad `SearchFieldPlacement` semantics remain
follow-up.
Constrained `.contentShape(...)`, counted tap gestures, and long-press gestures
emit stable Swift-style shape/gesture/count/duration breadcrumbs; native
hit-test geometry, gesture priority, composition, and gesture-value semantics
remain out of scope.
Constrained `.allowsHitTesting(...)` preserves authored hit-testing intent as
`data-swift-allows-hit-testing` metadata and maps `false` to a sidebar-local
`pointer-events:none` hint; native SwiftUI hit-test tree behavior, gesture
priority, and keyboard-vs-pointer focus nuance remain follow-up.
Constrained `.hidden()` preserves authored hidden-view intent as
`data-swift-hidden` metadata and maps to sidebar-local `visibility:hidden`, so
layout space is retained while visible chrome is suppressed; SwiftUI identity,
transition, lifecycle, and native layout negotiation semantics remain follow-up.
Constrained `.badge(...)` preserves scalar/text badge values as
`data-swift-badge` metadata and renders a sidebar-local pill on the authored
node; native list-row, tab, toolbar, menu, and accessibility badge placement
semantics remain follow-up.
Constrained `.hoverEffect(...)` and `.defaultHoverEffect(...)` preserve authored
pointer-hover style tokens as stable `data-swift-hover-effect` /
`data-swift-default-hover-effect` metadata and sidebar-local hover chrome hints;
native pointer regions, inherited default hover environments, and exact platform
hover-effect animations remain follow-up.
Static `.draggable(...)` resolves string/scalar payloads into browser draggable
affordances and `data-swift-draggable` breadcrumbs; native `Transferable`,
item-provider, drag preview, and drag-session semantics remain out of scope.
`Label("Title", systemImage:)` and constrained
`Label(title: { Text(...) }, icon: { Image(systemName:) })` forms lower to the
same sidebar-local label row; arbitrary rich label title/icon builders remain
follow-up.
`Image(systemName:)` renders a constrained text-glyph fallback with stable
`data-swift-system-image` and `data-swift-system-image-glyph` breadcrumbs;
native SF Symbol vector paths, weights, variable values, and exact symbol
rendering remain follow-up.
Positional `Image("name")` and `Image(decorative:)` resolve through the
`extension.sidebar.snapshot` `assets` map when the host provides one; accepted
asset URLs are limited to `http`/`https` and host-minted
`cmux-sidebar-asset://` URLs, with missing/unsafe entries rendered as
placeholders. Windows/Tauri mints local asset URLs for image files in a sibling
`<sidebar-name>.assets/` directory, keyed by both relative path and extensionless
relative path. `Image(decorative:)` renders with empty alt text and forced
`aria-hidden`, so authored accessibility labels do not leak onto decorative
assets.
`AsyncImage(url:)` accepts only `http`/`https` remote image URLs; unsafe schemes
render as a placeholder instead of loading local or app-internal resources.
Rendered nodes expose constrained `data-swift-async-image-phase` breadcrumbs and
the accepted safe URL as `data-swift-async-image-url`. Constrained named
`AsyncImage(url:content:placeholder:)` closures can render interpreted success
content plus placeholder breadcrumbs, including common `image in
image.resizable()` parameter bodies. Both named closure arguments and SwiftUI's
`AsyncImage(url:) { image in ... } placeholder: { ... }` spelling are supported;
native phase values and true load lifecycle remain follow-up.
`ViewThatFits(in:)` preserves the requested axis as sidebar-local classes/data
and renders interpreted alternatives, but it does not perform native SwiftUI
fit measurement or hide non-fitting alternatives.
`Group { ... }` renders as a semantic layout-transparent container, while
`EmptyView()` is treated as an intentional no-op; native Group modifier fan-out
remains follow-up.
`@State var name = ...` declarations support direct local `$name` bindings for
editable `TextField`, `SecureField`, `TextEditor`, `Toggle`, `Slider`,
`Stepper`, `DatePicker`, `ColorPicker`, and simple text-option `Picker`
controls inside the sidebar pane. Constrained `ForEach($items) { $item in ... }`
loops over local JSON-like `@State` arrays also support editable element-field
bindings such as `$item.title`, `$item.done`, and `$item.count` for those direct
controls by writing back through the local array index. `Picker` options may use
constrained `.tag(value)` metadata on text/label-like option rows so the visible
label can differ from the written selection value. `LabeledContent` supports
constrained title/value and title/trailing-content rows, but not every SwiftUI
format-style, named-label, or arbitrary initializer overload. Bindings to live cmux/session data and
`.constant(...)` still render accessible read-only controls; attach
`.onTapGesture { cmux(...) }` or `.onLongPressGesture { cmux(...) }` when a
control-shaped row should trigger a backend action. Direct local controls also
support `.onSubmit { ... }` and `.onChange(of:) { ... }` for simple local
`@State` assignments plus safe-scoped `cmux(...)` calls; single-line text fields
submit on Enter and multiline text editors submit on Ctrl/Cmd+Enter. Rendered
nodes also support constrained `.onAppear { ... }` and `.onDisappear { ... }`
lifecycle hooks plus `.task { ... }` / `.task(id:) { ... }` mount/id-change
hooks for the same safe local handler subset. These handlers are intentionally
not arbitrary Swift closure execution, full async/await task runtime,
cancellation/priority/actor semantics, exact SwiftUI lifecycle modeling, or true
value-diffing across re-walks. `.onHover { hovering in ... }` supports
constrained browser mouse enter/leave local state updates and safe actions, but
not full gesture composition, drag/drop values, or native pointer-region
semantics. `.focusable()` makes a rendered node keyboard-focusable, and
`.focused($localBool)` mirrors browser focus/blur into a direct local `@State`
Bool binding; full `@FocusState`, programmatic focus ownership, focus scopes,
and native focus propagation remain follow-up. `.refreshable { cmux(...) }`
renders a sidebar-local Refresh button, and `.swipeActions { ... }` renders a
sidebar-local action tray from authored child buttons; these do not claim native
pull-to-refresh physics or platform swipe gestures. `.dropDestination(for:) { cmux(...) }`
marks a rendered node as a browser drop target and invokes the
authored safe action on drop; typed payload/location binding and native
drag/drop highlighting remain follow-up. `.background(.regularMaterial)` and
related Material tokens render as safe translucent CSS backgrounds with
blur/saturation and stable material classes; native SwiftUI vibrancy/material
blending and host-window sampling remain follow-up. `.onEvent("event.name")` and
`.onEvent(category: "workspace")` run on subsequent live event-bridge updates
and support local `@State` assignments plus safe-scoped `cmux(...)` calls.
Swift-authored action params preserve basic
JSON-compatible types (numbers, booleans, nulls, arrays, ternaries, and resolved
live-data fields) before dispatch.
The safe color/style palette covers hierarchical foreground tokens
(`primary`/`secondary`/`tertiary`/`quaternary`/`quinary`), `accent`/`accentColor`,
and common named colors such as `mint`, `indigo`, and `brown`, with stable
foreground hierarchy classes where applicable. Full inherited SwiftUI
`ShapeStyle` resolution and multi-layer foreground styles remain follow-up.
Typography modifiers lower constrained bold/italic bool forms, font width,
dynamic type size, monospaced-digit, numeric `lineLimit(..., reservesSpace:)`,
tracking/kerning, baseline offset, truncation/alignment/case tokens, and
underline/strikethrough pattern/color values to CSS hints plus stable
breadcrumbs; exact SwiftUI
text-run layout, range/nil line limits, native middle truncation algorithms,
per-decoration styling when underline and strikethrough are combined, variable
font availability, host/user Dynamic Type propagation, attributed-run
preservation across concatenated Text values, OpenType feature availability, and
font-engine metrics remain follow-up.
Visual filters include constrained CSS-backed hue rotation and blend modes via
a fixed safe allow-list, including constrained `Angle` literals for hue and
rotation modifiers; exact SwiftUI compositing groups and color-space matching
remain follow-up.
Redaction/privacy modifiers lower to sidebar-local placeholder/privacy classes
and stable `data-swift-redacted`, `data-swift-redaction-reason`,
`data-swift-privacy-sensitive`, and `data-swift-unredacted` breadcrumbs. Native
inherited redaction environments, system privacy lock-state propagation, and
exact SwiftUI placeholder rendering remain follow-up.
Padding supports the all-side shorthand, constrained edge-set forms for `.top`,
`.bottom`, `.leading`, `.trailing`, `.horizontal`, `.vertical`, `.all`, simple
arrays of those tokens, and `.padding(EdgeInsets(top:leading:bottom:trailing:))`
side-specific values. RTL-aware leading/trailing mirroring and native layout
negotiation remain follow-up.
Frame dimensions lower to safe CSS sizing fields, and `maxWidth: .infinity`
continues to render as the sidebar fill class. Constrained `alignment:` tokens
add stable frame classes plus CSS text/flex alignment hints; full SwiftUI
proposed-size, ideal-size negotiation, and native wrapper placement remain
follow-up. Constrained `.containerRelativeFrame(...)` axis/count/span/spacing/
alignment metadata lowers to stable classes, `data-swift-container-*`
breadcrumbs, and safe CSS sizing hints; native container proposal math and
scroll-target sizing remain follow-up. Constrained `.coordinateSpace(name:)`
preserves the named coordinate-space registration as stable classes and
`data-swift-coordinate-space`; GeometryProxy frame conversion and named-space
lookups remain host-borrowed follow-up. Constrained `.alignmentGuide(...)`
preserves guide metadata and static numeric closure offsets as stable
classes/data breadcrumbs plus a safe CSS margin hint; native `ViewDimensions`
closure evaluation and SwiftUI alignment negotiation remain follow-up.
Constrained `Angle`, `UnitPoint`, `CGPoint`, `CGSize`, and `CGRect` literals are
preserved through the Swift expression evaluator with basic member access, so
geometry values can feed transform/accessibility metadata and string
interpolation. Full CoreGraphics APIs, path construction, affine transforms, and
host `GeometryProxy` values remain follow-up.
Constrained `Path(roundedRect: CGRect(...))` and `Path(ellipseIn: CGRect(...))`
render as sidebar-local shape approximations with `data-swift-path-*`
breadcrumbs and CSS size hints; imperative path builders, Canvas, custom Shape
protocol implementations, and exact native vector drawing remain follow-up.
Constrained `RoundedRectangle(..., style:)` and `Capsule(style:)` preserve
corner-style metadata as `data-swift-shape-style` breadcrumbs and stable classes;
exact native continuous/circular corner geometry remains follow-up.
Constrained `.clipShape(_:style:)` preserves `FillStyle(eoFill:antialiased:)`
metadata as `data-swift-clip-*` breadcrumbs; exact native clipping paths,
fill-rule geometry, and antialias rasterization remain follow-up.
Constrained `.safeAreaPadding(...)` and `.contentMargins(..., for:)` preserve
edge/length/`EdgeInsets` and content-margin placement metadata as
`data-swift-safe-area-padding-*` / `data-swift-content-margins-*` breadcrumbs
with sidebar-local padding hints; native safe-area environment and scroll-margin
layout negotiation remain follow-up.
Image `.scaledToFit()` / `.scaledToFill()` helpers lower to sidebar-local
aspect fit/fill classes; native SwiftUI image proposal sizing and exact
object-fit behavior remain follow-up.
Stack initializer alignment on `VStack`, `HStack`, `ZStack`, `LazyVStack`, and
`LazyHStack` preserves constrained alignment tokens as stable classes and
axis-aware CSS alignment hints. Full SwiftUI alignment guides, baseline
alignment, RTL-aware leading/trailing mirroring, and native layout negotiation
remain follow-up.
`LazyVStack` and `LazyHStack` preserve constrained `pinnedViews:` metadata as
stable classes and `data-swift-pinned-views` breadcrumbs, but Windows/Tauri
still renders them eagerly; native virtualization, sticky section headers or
footers, and pinned scroll physics remain follow-up.
`Section("Title")` and constrained `Section(header: Text(...), footer: Text(...))`
forms render dedicated title/header/body/footer slots. Arbitrary `@ViewBuilder`
header/footer closures and full platform list section chrome remain follow-up.
Constrained `List(data, id:)` forms expand supported sequence expressions
through their trailing row closure and preserve the `id:` token as
`data-swift-list-id`; list selection, edit mode, SwiftUI identity diffing, and
native platform list chrome remain follow-up.
Constrained `ForEach(data, id:)` forms stamp generated top-level rows with
`data-swift-id` breadcrumbs for scalar/key-path identities; SwiftUI row identity
diffing, move animations, and native reconciliation remain follow-up.
`ScrollView(axes, showsIndicators:)` preserves vertical/horizontal/both axis
metadata and the indicator flag as stable classes plus `data-swift-scroll-*`
breadcrumbs; scroll-to-position, native paging/snap targets, bounce behavior,
and exact platform scrollbar semantics remain follow-up.
Scroll chrome modifiers support constrained `.scrollContentBackground(.hidden)`,
`.scrollIndicators(.hidden, axes:)`, `.scrollClipDisabled()`,
`.scrollTargetBehavior(...)`, `.scrollTargetLayout()`,
`.scrollBounceBehavior(...)`, `.scrollDisabled(...)`, `.scrollPosition(id:)`,
and `.defaultScrollAnchor(...)` classes/data breadcrumbs plus safe CSS hints
where possible; native paging/snap targets, bounce physics, scroll-position
binding, `ScrollViewReader.scrollTo`, and exact platform scrollbar semantics
remain follow-up.
Authored sidebar actions are safe-scoped by `custom_sidebar_action_invoke`:
sidebar validation/navigation, workspace selection, surface focus/navigation,
sidebar read methods, and sidebar presentation metadata updates are allowed;
browser automation, debug, remote/SSH configuration, close/delete, and broad
file/system mutation methods are denied with `custom_sidebar_capability_denied`.
Allowed authored actions are also checked against a method-specific schema
before dispatch; required field failures return
`custom_sidebar_action_schema_invalid` with `field`, `expected`, and
`accepted_keys` data, and `capabilities` advertises the current catalog at
`custom_sidebar_actions.schema`.
The left-sidebar custom host surfaces adjacent capability-manifest status with
safe-default/no-manifest, requested/allowed counts, and denied requested methods
so authors can see trust-policy mismatches before a blocked action fires.
`sidebar reload [name]` validates sidebars, emits
`cmux://custom-sidebar-reload`, and returns the targeted valid paths so open
custom-sidebar panes can refresh immediately; authored panes also poll their
source for save-time hot reload. `sidebar select <name>` validates a sidebar,
emits `cmux://custom-sidebar-select`, and activates the left-sidebar custom
host. Full macOS SwiftUI interpreter coverage, manifest-granted privilege UX,
and broader dispatcher-wide action schema generation remain follow-up parity work.

Docs topics:

| Command | Contract |
| --- | --- |
| `docs` | List docs topics without a socket. |
| `docs settings` | Print the configuration docs URL, raw schema URL, cmux.json paths, backup reminder, and reload command. |
| `docs shortcuts` | Print shortcut docs and raw shortcut data resources. |
| `docs api` | Print API docs and raw CLI contract resources. |
| `docs browser` | Print browser automation docs and raw browser skill resources. |
| `docs agents` | Print agent integration docs and raw integration resources. |

Settings subcommands:

| Command | Contract |
| --- | --- |
| `settings` | Open the Settings window, launching cmux if needed. |
| `settings open [target]` | Open Settings to an optional target section. |
| `settings path` | Print cmux.json paths, docs URL, schema URL, backup reminder, and reload command without a socket. |
| `settings docs` | Print the same output as `docs settings` without a socket. |
| `settings <target>` | Open Settings to a target section. Supported aliases include `shortcuts`, `json`, `cmux-json`, `browser`, and `automation`. |

Config subcommands:

| Command | Contract |
| --- | --- |
| `config doctor [--path <file>]`, `config check`, `config validate` | Validate JSONC syntax for config files. When `--path` is absent, default discovery checks the primary config, project-level `.cmux/cmux.json` or `cmux.json`, and legacy config files. `--path <file>` may be repeated to validate multiple explicit files. Exits 0 on success and 1 on any error. Supports `--json`. Works without a socket. |
| `config path`, `config paths` | Print cmux.json paths, docs URL, schema URL, backup reminder, and reload command without a socket. |
| `config docs`, `config documentation` | Print the same output as `docs settings` without a socket. |
| `config reload` | Ask the running cmux app to reload configuration. Requires a socket. |
| `config get sidebar-font-size` | Print the effective sidebar text size. |
| `config set sidebar-font-size <points>` | Write the sidebar text size to cmux's editable Ghostty config and reload the running app when available. |
| `config sidebar-font-size [points]` | Get the sidebar text size, or set it when a point size is provided. |
| `config get surface-tab-bar-font-size` | Print the effective workspace tab bar text size. |
| `config set surface-tab-bar-font-size <points>` | Write the workspace tab bar text size to cmux's editable Ghostty config and reload the running app when available. |
| `config surface-tab-bar-font-size [points]` | Get the workspace tab bar text size, or set it when a point size is provided. |
| `config get <key>`, `config set <key> <points>` | Generic get/set for `sidebar-font-size` and `surface-tab-bar-font-size`. |

`config doctor --json` outputs an object with `ok`, `error_count`,
`findings`, `reload_command`, `docs_url`, and `schema_url`. Each finding includes
`label`, `display_path`, `path`, `status`, `ok`, `keys`, and, when available,
`message` and `bytes`.

Events command:

| Option | Contract |
| --- | --- |
| `--after <seq>`, `--after-seq <seq>` | Subscribe to retained events after a sequence number. |
| `--cursor-file <path>` | Read the starting sequence from a file and update it after every event. |
| `--name <event>` | Filter by event name. Repeatable. |
| `--category <name>` | Filter by category. Repeatable. |
| `--reconnect` | Reconnect and resume from the last received sequence until interrupted. |
| `--limit <n>` | Exit after printing `n` event frames. |
| `--no-ack` | Suppress the initial ack frame in stdout. |
| `--no-heartbeat`, `--no-heartbeats` | Suppress heartbeat frames in stdout. |

`events.stream` is a v2 socket method advertised by `capabilities`. The first
response frame is an `ack`; sequence resume metadata lives under `ack.resume` as
`after_seq`, `oldest_seq`, `latest_seq`, `next_seq`, and `gap`. Event frames
carry a process-local monotonic `seq` and a stable `id` for dedupe. Clients
should persist `seq` after processing each event and reconnect with that value.
Windows/Tauri currently supports this method and `cmux events` for retained
replay plus live session-derived event delivery; it prints ack/event/heartbeat
NDJSON frames, appends those events to `~/.cmuxterm/events.jsonl` with one
rotated archive, and keeps the stream open for matching live events and
heartbeats. The full macOS event catalog remains tracked parity work.
See [events.md](events.md) for the full protocol and event catalog. Every emitted event is also appended to
`~/.cmuxterm/events.jsonl`, including model lifecycle events for window
creation, close, focus, key-window state, workspace selection, pane focus, and
surface selection, focus, creation, or closure. The stream is bounded: cmux keeps
4,096 replay events in memory, caps each encoded event frame at 16 KiB, closes
slow subscribers after 1,024 pending events, and rotates `events.jsonl` with one
16 MiB archive at `events.jsonl.1`.

## No-Socket Help Probes

The following probes are executable contract checks. They must exit 0 and print
the expected text without connecting to a cmux socket.

<!-- cli-contract-help-probes:start -->
- `cmux --help` -> `cmux - control cmux via Unix socket`
- `cmux --help` -> `open <path-or-url>...`
- `cmux help` -> `cmux - control cmux via Unix socket`
- `cmux ping --help` -> `Usage: cmux ping`
- `cmux capabilities --help` -> `Usage: cmux capabilities`
- `cmux events --help` -> `Usage: cmux events [options]`
- `cmux auth --help` -> `Usage: cmux auth <status|login|logout>`
- `cmux vm --help` -> `Usage: cmux vm <base|new|ls|status|snapshot|fork|restore|rm|exec|shell|attach|ssh|ssh-info> [args...]`
- `cmux cloud --help` -> `Usage: cmux cloud <base|new|ls|status|snapshot|fork|restore|rm|exec|shell|attach|ssh|ssh-info> [args...]`
- `cmux remotes --help` -> `Usage: cmux remotes <list|add|remove> [options]`
- `cmux remote --help` -> `Usage: cmux remotes <list|add|remove> [options]`
- `cmux rpc --help` -> `Usage: cmux rpc <method> [json-params]`
- `cmux help --help` -> `Usage: cmux help`
- `cmux docs --help` -> `Usage: cmux docs [settings|shortcuts|api|browser|agents|dock]`
- `cmux docs` -> `Topics:`
- `cmux docs settings` -> `Config files:`
- `cmux docs dock` -> `dock: Custom right-sidebar terminal controls`
- `cmux settings --help` -> `Usage: cmux settings [open [target]|path|docs|<target>]`
- `cmux settings path` -> `Config files:`
- `cmux settings docs` -> `Config files:`
- `cmux config --help` -> `Usage: cmux config <doctor|check|validate|path|paths|docs|documentation|reload|get|set|sidebar-font-size|surface-tab-bar-font-size>`
- `cmux config path` -> `Config files:`
- `cmux config docs` -> `Config files:`
- `cmux welcome --help` -> `Usage: cmux welcome`
- `cmux welcome` -> `Toggle Left Sidebar`
- `cmux welcome` -> `Toggle Right Sidebar`
- `cmux shortcuts --help` -> `Usage: cmux shortcuts`
- `cmux disable-browser --help` -> `Usage: cmux disable-browser [--json]`
- `cmux enable-browser --help` -> `Usage: cmux enable-browser [--json]`
- `cmux browser-status --help` -> `Usage: cmux browser-status [--json]`
- `cmux agent-hibernation --help` -> `Usage: cmux agent-hibernation <on|off> [--json]`
- `cmux restore-session --help` -> `Usage: cmux restore-session`
- `cmux open --help` -> `Usage: cmux open <path-or-url>...`
- `cmux feedback --help` -> `Usage: cmux feedback`
- `cmux feed --help` -> `Usage: cmux feed tui [--opentui|--legacy]`
- `cmux hooks --help` -> `Usage: cmux hooks setup [agent] [--agent <name>] [--yes|-y]`
- `cmux codex --help` -> `Usage: cmux codex <install-hooks|uninstall-hooks>`
- `cmux themes --help` -> `Usage: cmux themes`
- `cmux omo --help` -> `Usage: cmux omo [opencode-args...]`
- `cmux omx --help` -> `Usage: cmux omx [omx-args...]`
- `cmux omc --help` -> `Usage: cmux omc [omc-args...]`
- `cmux identify --help` -> `Usage: cmux identify`
- `cmux list-windows --help` -> `Usage: cmux list-windows`
- `cmux current-window --help` -> `Usage: cmux current-window`
- `cmux new-window --help` -> `Usage: cmux new-window`
- `cmux focus-window --help` -> `Usage: cmux focus-window --window <id|ref|index>`
- `cmux close-window --help` -> `Usage: cmux close-window --window <id|ref|index>`
- `cmux move-workspace-to-window --help` -> `Usage: cmux move-workspace-to-window`
- `cmux move-surface --help` -> `Usage: cmux move-surface`
- `cmux split-off --help` -> `Usage: cmux split-off`
- `cmux reorder-surface --help` -> `Usage: cmux reorder-surface`
- `cmux reorder-workspace --help` -> `Usage: cmux reorder-workspace`
- `cmux reorder-workspaces --help` -> `Usage: cmux reorder-workspaces`
- `cmux workspace-action --help` -> `Usage: cmux workspace-action --action <name>`
- `cmux move-tab-to-new-workspace --help` -> `Usage: cmux move-tab-to-new-workspace`
- `cmux tab-action --help` -> `Usage: cmux tab-action --action <name>`
- `cmux rename-tab --help` -> `Usage: cmux rename-tab`
- `cmux new-workspace --help` -> `Usage: cmux new-workspace`
- `cmux list-workspaces --help` -> `Usage: cmux list-workspaces`
- `cmux ssh --help` -> `Usage: cmux ssh <destination>`
- `cmux ssh --help` -> `--forward-agent`
- `cmux ssh-session-list --help` -> `Usage: cmux ssh-session-list`
- `cmux ssh-session-attach --help` -> `Usage: cmux ssh-session-attach --session-id <id>`
- `cmux ssh-session-cleanup --help` -> `Usage: cmux ssh-session-cleanup`
- `cmux new-split --help` -> `Usage: cmux new-split`
- `cmux list-panes --help` -> `Usage: cmux list-panes`
- `cmux list-pane-surfaces --help` -> `Usage: cmux list-pane-surfaces`
- `cmux tree --help` -> `Usage: cmux tree`
- `cmux top --help` -> `Usage: cmux top`
- `cmux focus-pane --help` -> `Usage: cmux focus-pane`
- `cmux new-pane --help` -> `Usage: cmux new-pane`
- `cmux new-surface --help` -> `Usage: cmux new-surface`
- `cmux close-surface --help` -> `Usage: cmux close-surface`
- `cmux drag-surface-to-split --help` -> `Usage: cmux drag-surface-to-split`
- `cmux refresh-surfaces --help` -> `Usage: cmux refresh-surfaces`
- `cmux reload-config --help` -> `Usage: cmux reload-config`
- `cmux surface-health --help` -> `Usage: cmux surface-health`
- `cmux debug-terminals --help` -> `Usage: cmux debug-terminals`
- `cmux trigger-flash --help` -> `Usage: cmux trigger-flash`
- `cmux list-panels --help` -> `Usage: cmux list-panels`
- `cmux focus-panel --help` -> `Usage: cmux focus-panel`
- `cmux close-workspace --help` -> `Usage: cmux close-workspace`
- `cmux select-workspace --help` -> `Usage: cmux select-workspace`
- `cmux rename-workspace --help` -> `Usage: cmux rename-workspace`
- `cmux rename-window --help` -> `Usage: cmux rename-workspace`
- `cmux current-workspace --help` -> `Usage: cmux current-workspace`
- `cmux capture-pane --help` -> `Usage: cmux capture-pane`
- `cmux resize-pane --help` -> `Usage: cmux resize-pane`
- `cmux pipe-pane --help` -> `Usage: cmux pipe-pane`
- `cmux wait-for --help` -> `Usage: cmux wait-for`
- `cmux swap-pane --help` -> `Usage: cmux swap-pane`
- `cmux break-pane --help` -> `Usage: cmux break-pane`
- `cmux join-pane --help` -> `Usage: cmux join-pane`
- `cmux next-window --help` -> `Usage: cmux next-window`
- `cmux previous-window --help` -> `Usage: cmux previous-window`
- `cmux last-window --help` -> `Usage: cmux last-window`
- `cmux last-pane --help` -> `Usage: cmux last-pane`
- `cmux find-window --help` -> `Usage: cmux find-window`
- `cmux clear-history --help` -> `Usage: cmux clear-history`
- `cmux set-hook --help` -> `Usage: cmux set-hook`
- `cmux popup --help` -> `Usage: cmux popup`
- `cmux bind-key --help` -> `Usage: cmux bind-key`
- `cmux unbind-key --help` -> `Usage: cmux unbind-key`
- `cmux copy-mode --help` -> `Usage: cmux copy-mode`
- `cmux set-buffer --help` -> `Usage: cmux set-buffer`
- `cmux paste-buffer --help` -> `Usage: cmux paste-buffer`
- `cmux list-buffers --help` -> `Usage: cmux list-buffers`
- `cmux respawn-pane --help` -> `Usage: cmux respawn-pane`
- `cmux display-message --help` -> `Usage: cmux display-message`
- `cmux read-screen --help` -> `Usage: cmux read-screen`
- `cmux send --help` -> `Usage: cmux send`
- `cmux send-key --help` -> `Usage: cmux send-key`
- `cmux send-panel --help` -> `Usage: cmux send-panel`
- `cmux send-key-panel --help` -> `Usage: cmux send-key-panel`
- `cmux notify --help` -> `Usage: cmux notify`
- `cmux list-notifications --help` -> `Usage: cmux list-notifications`
- `cmux dismiss-notification --help` -> `Usage: cmux dismiss-notification`
- `cmux mark-notification-read --help` -> `Usage: cmux mark-notification-read`
- `cmux open-notification --help` -> `Usage: cmux open-notification`
- `cmux jump-to-unread --help` -> `Usage: cmux jump-to-unread`
- `cmux clear-notifications --help` -> `Usage: cmux clear-notifications`
- `cmux right-sidebar --help` -> `Usage: cmux right-sidebar <command> [flags]`
- `cmux set-status --help` -> `Usage: cmux set-status`
- `cmux clear-status --help` -> `Usage: cmux clear-status`
- `cmux list-status --help` -> `Usage: cmux list-status`
- `cmux set-agent-pid --help` -> `Usage: cmux set-agent-pid`
- `cmux clear-agent-pid --help` -> `Usage: cmux clear-agent-pid`
- `cmux report-pr --help` -> `Usage: cmux report-pr`
- `cmux report-review --help` -> `Usage: cmux report-review`
- `cmux clear-pr --help` -> `Usage: cmux clear-pr`
- `cmux report-meta --help` -> `Usage: cmux report-meta`
- `cmux clear-meta --help` -> `Usage: cmux clear-meta`
- `cmux list-meta --help` -> `Usage: cmux list-meta`
- `cmux report-meta-block --help` -> `Usage: cmux report-meta-block`
- `cmux clear-meta-block --help` -> `Usage: cmux clear-meta-block`
- `cmux list-meta-blocks --help` -> `Usage: cmux list-meta-blocks`
- `cmux reset-sidebar --help` -> `Usage: cmux reset-sidebar`
- `cmux set-progress --help` -> `Usage: cmux set-progress`
- `cmux clear-progress --help` -> `Usage: cmux clear-progress`
- `cmux log --help` -> `Usage: cmux log`
- `cmux clear-log --help` -> `Usage: cmux clear-log`
- `cmux list-log --help` -> `Usage: cmux list-log`
- `cmux sidebar-state --help` -> `Usage: cmux sidebar-state`
- `cmux set-app-focus --help` -> `Usage: cmux set-app-focus`
- `cmux simulate-app-active --help` -> `Usage: cmux simulate-app-active`
- `cmux claude-hook --help` -> `Usage: cmux claude-hook`
- `cmux browser --help` -> `Usage: cmux browser`
- `cmux open-browser --help` -> `Legacy alias for 'cmux browser open'`
- `cmux navigate --help` -> `Legacy alias for 'cmux browser navigate'`
- `cmux browser-back --help` -> `Legacy alias for 'cmux browser back'`
- `cmux browser-forward --help` -> `Legacy alias for 'cmux browser forward'`
- `cmux browser-reload --help` -> `Legacy alias for 'cmux browser reload'`
- `cmux get-url --help` -> `Legacy alias for 'cmux browser get-url'`
- `cmux focus-webview --help` -> `Legacy alias for 'cmux browser focus-webview'`
- `cmux is-webview-focused --help` -> `Legacy alias for 'cmux browser is-webview-focused'`
- `cmux markdown --help` -> `Usage: cmux markdown open <path>`
<!-- cli-contract-help-probes:end -->

## No-Socket Negative Help Probes

The following probes must not print help. They protect argument forwarding after
`--`, where a forwarded `--help` token belongs to the command payload.

<!-- cli-contract-negative-help-probes:start -->
- `cmux vm exec demo -- --help` !> `Usage: cmux vm`
<!-- cli-contract-negative-help-probes:end -->

## Current Help Caveats

These are current contracts to preserve until a follow-up PR intentionally
changes them:

- `cmux version --help` currently prints the version summary because `version`
  is handled before subcommand help dispatch.
- `cmux claude-teams --help` is handled by the command launcher, not by the
  pre-socket help dispatcher.
- `cmux codex-teams --help` is handled by the command launcher, not by the
  pre-socket help dispatcher.
- `cmux remote-daemon-status --help` currently prints status because the command
  runs before subcommand help dispatch.

## ArgumentParser Migration Sequence

1. Keep this contract file and `tests/test_cli_contract_help.py` green.
2. Add Swift ArgumentParser as a dependency without changing behavior.
3. Introduce a parse-only facade that maps ArgumentParser command structs onto
   existing `CMUXCLI` runner methods.
4. Move one command family at a time into small files, starting with no-socket
   commands (`version`, `themes`, hook installers), then socket commands, then
   browser and tmux compatibility.
5. After each family moves, run the contract probes plus targeted socket tests in
   GitHub Actions.
6. When all command families are migrated, remove the manual global parser and
   legacy helper code that no longer owns behavior.
