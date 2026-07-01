# 90 — Cross-cutting concerns & risks

## Cross-cutting concerns

### Dev workflow
- **Smart App Control is off** on the primary dev machine, so
  `cargo run -p cmux-desktop` (debug — keeps a console for logs) launches
  directly. The `apps/desktop/dev/sandbox/` harness remains for SAC-on machines.
- **Release** = windowed / no console (`windows_subsystem="windows"`). **Debug** =
  console attached (for logs); launch debug from a real interactive terminal
  (a GUI app through a piped shell exits immediately).
- Static-CRT means the binary is self-contained — no VC++ redistributable needed.

### Data model & IPC
- `@cmux/core-types` (ts-rs from `cmux-core`) is the **single source of truth** for
  the wire model. Run the drift check (`check-drift.mjs`) in CI.
- Add Tauri commands with **typed payloads**; extend `core-types` rather than
  hand-writing TS interfaces.

### Testing
- Rust unit/integration per crate.
- **Golden parity** (`cmux-golden`) for anything with a Swift counterpart —
  especially the agent transcript parsers (Phase 3).
- Web unit tests (`bun test`) for bridge/UI logic.
- Process-spawning tests live in lib `#[cfg(test)]` (the standalone-exe
  Application-Control gotcha; and `session_golden` flakes under heavy parallel
  `cargo test --workspace`).

### Localization
- Canonical requirement: **all user-facing strings localized** (currently en/ja).
- Run the **localization audit** on every UI-touching change (labels, menus,
  settings, shortcuts, errors, tooltips). `defaultValue`/English fallback does not
  count. See the localization rules in the repo `CLAUDE.md`.

### Keyboard shortcuts
- Every cmux-owned shortcut must be in the shortcut settings, editable in
  Settings, supported in `cmux.json`, and documented (shortcut policy in
  `CLAUDE.md`). Enforced in Phase 5.

### Packaging & signing
- Tauri bundler → MSI/NSIS. A **reputable code-signing certificate** is needed for
  a clean install under Smart App Control / SmartScreen (Phase 6).

## Optional tracks (kept off the critical path)

- **Go daemon (`cmuxd`) lifecycle** — only if remote/daemon features are in scope;
  needs a standalone Job Object *without* `KILL_ON_JOB_CLOSE` so the daemon
  survives its launcher.
- **CLI per-command socket layer** — `cmux-cli` foundation is done, but
  per-command forwarding is **blocked on the app/server contract** (the app must
  define socket command names, arg shapes, and response rendering). Unblocks once
  Phase 2/3 stand up a control server.
- **Backend / cloud control plane** — deferred; parity reference only unless cloud
  VMs are wanted on Windows.

## Risk register

| # | Risk | Impact | Mitigation |
|---|---|---|---|
| R1 | **Host-bridge fidelity** — reused webviews depend on the macOS host contract | Stalls chat/diff/markdown reuse | Enumerate channels/routes first; build & unit-test the bridge in Phase 1 against one real component |
| R2 | **Split-pane parity** — bonsplit nesting/drag/ratios are subtle | Clunky shell | Spike early in Phase 2; decide port-vs-library on evidence |
| R3 | **Transcript-parser fidelity** — Swift `CmuxAgentChat` port must match exactly | Wrong agent rendering | Port to Rust with `cmux-golden` golden tests |
| R4 | **Signing / reputation** — unsigned/low-rep binaries hit SAC / SmartScreen | Bad install UX | Budget a reputable cert in Phase 6 |
| R5 | **Performance / latency** — a webview terminal must stay crisp | Feels clunky | Latency pass in Phase 6; xterm tuning; no per-keystroke allocations in the bridge |
| R6 | **Scope creep** — backend/cloud + browser panes can balloon | Slips the beta | Keep them optional tracks, off the critical path |
| R7 | **Vite ↔ Tauri / HMR** friction under WebView2 | Slows Phase 1 | Validate `devUrl` HMR early; fall back to static `dist` if needed |
