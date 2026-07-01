# 01 — Architecture

## Layers

```
┌──────────────────────────────────────────────────────────────┐
│  WebView2 (Chromium)  —  React 19 + Vite + Tailwind 4 UI      │
│                                                              │
│  Workspace shell (tabs, splits, sidebar, palette, settings)  │  ← rebuilt in React
│  Terminal (xterm.js)   Agent chat   Diff   Markdown          │  ← chat/diff/md REUSED from webviews/
│                     │  host bridge (invoke / events)          │
└─────────────────────┼────────────────────────────────────────┘
                      │  Tauri IPC  (commands + events)
┌─────────────────────┴────────────────────────────────────────┐
│  Rust (Tauri backend)  —  the extracted cross-platform core   │
│                                                              │
│  cmux-terminal (ConPTY/engine)   cmux-process (Job Objects)   │
│  cmux-agent (resolver/launch)    cmux-ipc (control socket)    │
│  cmux-core (session model + notifications)   cmux-windowing   │
│  cmux-cli   cmux-golden                       (+ Go cmuxd)    │
└──────────────────────────────────────────────────────────────┘
        │ ts-rs generates @cmux/core-types (shared wire model)
        └───────────────────────────────────────────────────────►  consumed by the React UI
```

## The three reuse seams

These are what make the plan lean; every phase leans on at least one.

### 1. `@cmux/core-types` — the typed wire model
TypeScript types generated from `crates/cmux-core` via **ts-rs**
(`apps/desktop/packages/core-types`, `scripts/generate.mjs`,
`check-drift.mjs`). Already emits `AppSessionSnapshot`, `SessionWindowSnapshot`,
`SessionSplitLayoutSnapshot`, `SessionTabManagerSnapshot`,
`SessionWorkspace*Snapshot`, etc. The UI renders an **already-typed** model; we
extend this bridge rather than invent an IPC contract. CI runs the drift check so
Rust↔TS never silently diverge.

### 2. The host bridge — Tauri ⟷ reused webviews
cmux's `webviews/` were written for a macOS WKWebView host: a **custom URL
scheme** for asset/data loads plus **`window.webkit.messageHandlers.*`** for
RPC/events (`CmuxWebView.swift`, `DiffCommentsBridge.swift`). To reuse them
unmodified we build a **host adapter** presenting the same shape, backed by Tauri:

- `host.invoke(channel, payload)` → Tauri `invoke` (a.k.a. `callNative`).
- `host.on(event, cb)` → Tauri event `listen` (a.k.a. `listenNative`).
- custom-scheme asset/data loads → Tauri commands returning bytes/JSON (or the
  Tauri asset protocol).

Built once in **Phase 1**, then every reused surface (chat, diff, comments,
markdown) plugs in by swapping only its host adapter. Full detail:
[`phases/phase-1-react-foundation-bridge.md`](./phases/phase-1-react-foundation-bridge.md).

### 3. Tauri commands / events — the template already exists
`apps/desktop/web/src/tauri-bridge.ts` (`callNative` / `listenNative`) is the
working pattern that wires the terminal to `terminal.rs`. Every new surface
follows it: a typed Rust `#[tauri::command]` + `Emitter` events, consumed via the
host bridge.

## Data flow (example: a terminal surface)

1. UI calls `host.invoke("terminal_open", { cols, rows })` → Rust `terminal_open`
   spawns a `ConPty`, starts a pump thread.
2. Pump thread emits `cmux://terminal-output` events (base64 bytes) → `host.on`
   → `term.write`.
3. `term.onData` → `host.invoke("terminal_write", …)`; resize →
   `host.invoke("terminal_resize", …)` (explicit; no SIGWINCH on Windows).

The same request/stream shape generalizes to agents (transcript events), diffs
(diff payloads), and the session model (snapshot events).
