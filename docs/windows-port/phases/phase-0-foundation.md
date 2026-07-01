# Phase 0 — Foundation ✅ DONE

**Goal:** a runnable Windows shell — a window with a live terminal — plus the
extracted Rust core and the dev/build infra everything else stands on.

## What was delivered

- **Rust cross-platform core** (headless, tested): `cmux-core` (session model +
  notification store), `cmux-terminal` (ConPTY), `cmux-process` (Job-Object
  supervision), `cmux-agent` (resolver), `cmux-ipc` (control socket),
  `cmux-windowing` (geometry/restore), `cmux-cli` (foundation), `cmux-golden`.
- **Terminal MVP:** `apps/desktop/src-tauri/src/terminal.rs` — `terminal_open/
  write/resize/close` over `cmux-terminal::conpty::ConPty`, base64 output events
  (`cmux://terminal-output` / `-exit`) rendered by xterm.js in the WebView2
  webview.
- **Dev/build infra:** static-CRT (`+crt-static`, no VC++ redist), Windows
  Sandbox harness (`apps/desktop/dev/sandbox/`), `withGlobalTauri`,
  `windows_subsystem="windows"` in release.

## Commits

`07bebae` terminal · `d9277e7` withGlobalTauri · `8294d66` sandbox harness ·
`219a0f4` static-CRT · `ee186b9` windows_subsystem. (An agent-in-terminal
launcher was reverted in `de10cc9` as non-canonical.)

## Acceptance — met

- App launches on the host (SAC off) and inside Windows Sandbox.
- A live PowerShell shell renders; typing, running commands, and resize all work
  over ConPTY.
- Static-CRT verified via `dumpbin /dependents` (no `VCRUNTIME*`/UCRT imports);
  `cargo build --workspace` + `cargo test --workspace` green.

## Carry-forward into Phase 1

- The current frontend is **vanilla TS** — it becomes React in Phase 1.
- The `tauri-bridge.ts` (`callNative`/`listenNative`) pattern is the template for
  the host bridge.
- The terminal commands stay; they get wrapped in a React `<TerminalSurface>`.
