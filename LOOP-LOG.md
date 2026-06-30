# Milestone-loop log

One line per completed slice (milestone/result/commit/next). Newest last.

- M4 WS5 — wire `classify_command` into `cmux-cli` dispatch (pure `plan()` +
  thin executor); `rpc` preserved, generic v1 forward + side-effecting no-socket
  actions return honest "not yet ported" errors. Tested (cmux-cli 47, workspace
  357, clippy clean) → simplify (collapsed 15 not-ported arms) → retested green.
  Commit: <pending>. Next: server-contract map for the v1 generic forward, OR
  Go daemon lifecycle (WS1-3).
