# Windows-port design decisions (headless harvest)

Running log of non-obvious design decisions made while porting cmux to Windows,
so future iterations (and reviewers) can see the *why*, not just the *what*.
Newest first.

## M4 WS5 — v1 client wire codec + the "no generic forward" finding

An understand-workflow over `CLI/cmux.swift`'s ~50 socket-command handlers
established the architecture of the socket command layer, and why a generic
forward is not a faithful port:

- **No name-deriving forwarder.** Each handler hardcodes its *socket* command
  name as an underscore_case string literal at the `sendV1Command` call site.
  The hyphen→underscore correspondence (CLI `list-windows` → socket
  `list_windows`) is a hand-typed convention, not a transform; `notify` →
  `notify_target` is a genuine rename. There is no mapping table.
- **Two transports.** Many modern verbs (`send`, `new-split`, `new-workspace`,
  `capture-pane`, `resize-pane`, all `browser.*`) emit **v2 JSON-RPC** with a
  typed params dict, not a v1 line (`send` → `surface.send_text`, `resize-pane`
  → `pane.resize`).
- **Args are not forwarded verbatim.** Handlers do per-flag work: renaming
  (`--name` → `title`), **handle resolution** (`--workspace`/`--surface` refs →
  uuids via *socket round-trips*), env defaulting (`CMUX_WORKSPACE_ID`), and the
  v1/v2 choice. Even the closest-to-verbatim v1 path rewrites `--workspace` →
  `--tab=`, strips `--window`, and injects a default `--tab=`.

**Consequence:** the per-command socket layer must be ported as bespoke handlers,
and handle resolution requires a *server* implementing those methods — i.e. it is
blocked on the app/server side (a HARD BOUNDARY). The headless CLI-side primitives
of M4 are otherwise complete.

**What was built:** the one frozen, reusable artifact the finding supports — the
**v1 client wire codec** in `cmux-ipc/client.rs` (`shell_quote`,
`build_v1_command_line`, `interpret_v1_response`, `V1ResponseError`), the dual of
the v2 codec (`build_v2_request` / `interpret_v2_response`). It is pure and
parity-pinned to exact Swift lines (`shellQuote` 12004-12010; forwarder
16294-16297; `sendV1Command` 5765-5771), so its shape is verifiable without a
caller. The CLI→socket command-name mapping is deliberately the *caller's*
responsibility (kept out of the codec). `V1ResponseError` is a newtype (not an
enum like `V2ResponseError`) because a v1 reply carries exactly one failure bit
(`ERROR:`-prefixed or not) — modeling it as an enum would fabricate distinctions
the protocol lacks. Watch-item: land the first real v1 handler before long so the
frozen primitive gets one end-to-end integration exercise.

## M4 WS5 — wire `classify_command` into dispatch (`cmux-cli`)

- **Two-step `classify → plan → executor` split.** `classify_command`
  (`classify.rs`) is the permanent, complete Swift-parity port of `run()`'s
  pre-socket taxonomy (18 branches, load-bearing order). `plan()` (`dispatch.rs`)
  is the *porting-frontier projection*: it maps each `PreSocketAction` to a
  `DispatchPlan` the thin `main.rs` executor performs. Keeping the frontier churn
  (stopgaps, the `rpc` special-case) out of the parity classifier means a newly
  ported command edits `plan()` + executor, never the stable classifier or its
  parity tests.
- **Generic v1 socket forward DEFERRED.** Swift sends a "needs socket" command as
  a v1 line: `([socketCommand] + args).map(shellQuote).join(" ") + "\n"`. But the
  *socket* command name is bespoke per command (the CLI `list-windows` is sent as
  `list_windows`) — there is **no universal CLI-name → socket-name rule**. A
  generic forward would send wrong frames. So only `rpc` (the one command with a
  complete v2 codec) runs today; every other `NeedsSocket` command returns a clear
  "not yet ported (only 'rpc' wired)" error. Unblocking this needs a server-contract
  map (the per-command socket name + arg shape + response-render table).
- **Side-effecting no-socket commands → `not_yet_ported`.** `docs`, `welcome`,
  `sessions`, `settings`/`config` docs, the sigpipe/diff-viewer probes, `open
  <path>`, `window default-display`, etc. each need a subsystem not in the headless
  core yet. They fail with exit 1 and a descriptive label rather than a guessed
  behavior.
- **Top-level help + per-command usage text DEFERRED.** `print_top_level_help`
  prints a one-line synopsis, not the full macOS `usage()` block. The verbatim port
  is held back because (a) the 150-command listing references macOS-specific paths
  (`~/.local/state/cmux/cmux.sock`, `~/.config/ghostty`) that need Windows
  adaptation, and (b) the per-command `subcommandUsage` text is a ~1989-line switch.
  `SubcommandHelp` prints the faithful `cmux <command>` header + a pointer as a
  stopgap (exit 0, matching Swift).
- **Kept the `command` param on `plan()` rather than enriching
  `PreSocketAction::NeedsSocket { command }`.** The deeper alternative would touch
  the stable parity classifier and its many `NeedsSocket` assertions; "rpc is the
  one wired command" is a property of the porting frontier, so it belongs in
  `plan()`, not the Swift mirror.
