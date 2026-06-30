# Windows-port design decisions (headless harvest)

Running log of non-obvious design decisions made while porting cmux to Windows,
so future iterations (and reviewers) can see the *why*, not just the *what*.
Newest first.

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
