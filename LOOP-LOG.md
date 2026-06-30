# Milestone-loop log

One line per completed slice (milestone/result/commit/next). Newest last.

- M4 WS5 — wire `classify_command` into `cmux-cli` dispatch (pure `plan()` +
  thin executor); `rpc` preserved, generic v1 forward + side-effecting no-socket
  actions return honest "not yet ported" errors. Tested (cmux-cli 47, workspace
  357, clippy clean) → simplify (collapsed 15 not-ported arms) → retested green.
  Commit: `1a04263`. Next: server-contract map for the v1 generic forward, OR
  Go daemon lifecycle (WS1-3).
- M4 WS5 — v1 client wire codec in `cmux-ipc` (`shell_quote` +
  `build_v1_command_line` + `interpret_v1_response` + `V1ResponseError`), dual of
  the v2 codec; parity-pinned to Swift. Understand-workflow first PROVED there is
  no generic v1 forward (per-command bespoke handlers + v1/v2 split + handle
  resolution → blocked on app/server; see DECISIONS.md). Tested (cmux-ipc 76,
  workspace green, clippy clean) → simplify (collapsed dual `.map`) → retested
  green. Commit: <pending>. Next: Go daemon lifecycle (WS1-3) — the unblocked
  remaining half of M4.
