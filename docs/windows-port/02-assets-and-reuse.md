# 02 — Asset inventory & reuse-vs-rebuild

## Rust core (crates/) — built & tested, headless

| Crate | Role | Status |
|---|---|---|
| `cmux-core` | Session/window/tab/split model + notification store; ts-rs source | Done (M1, M10) |
| `cmux-terminal` | ConPTY wrapper, engine, surface, OSC-133, links | Done (M2 engine layer) |
| `cmux-process` | Job-Object supervisor, zero-orphan tree-kill, NDJSON framer, captured stdio | Done (M3) |
| `cmux-agent` | Executable resolver (PATHEXT/env-policy), `LaunchPlan`→`SpawnSpec`, OpenCode auth | Done (M3) |
| `cmux-ipc` | Control socket: named-pipe server, per-conn auth gate, client, v1/v2 codecs | Done (M4 core) |
| `cmux-cli` | `cmux` CLI: invocation parsing, socket/password resolution, `rpc`, classify/dispatch | Foundation done; per-command socket layer **blocked** on the app/server contract |
| `cmux-windowing` | Window geometry + session restore | Done (M5 geometry) |
| `cmux-golden` | Golden-parity harness vs the Swift app | Done |
| `cmuxd` (Go) | Remote daemon sidecar | Compiles on Windows; lifecycle WS1–3 pending |

## Reusable web — `cmux/webviews` (`@cmux/webviews`)

Stack: **React 19 + TanStack Router + Vite 7 + Tailwind 4 + React Compiler**,
plus `@pierre/diffs`/`@pierre/trees` and ProseMirror. Builds to static bundles
loaded into **WKWebView** on macOS ⇒ Chromium/WebView2-portable. Key trees under
`webviews/src/`:

- `agent-session/` — the agent chat renderer (React; a Solid variant also exists
  under `Resources/agent-session-solid/`).
- `surfaces/` — surface hosts; `comments/` — diff comments.
- Diff viewer via `@pierre/diffs` + `@pierre/trees` + `worker-pool.ts`.
- Prompt editor (ProseMirror); router in `router.tsx`; `App.tsx`, `main.tsx`.

Plus `Resources/markdown-viewer/` — a **static** markdown/mermaid/vega renderer
(`marked.min.js`, `highlight.js`, `mermaid.min.js`, `vega*`), loaded via
`Sources/Panels/MarkdownWebRenderer.swift` / `CmuxWebView.swift`. Trivially
portable.

> Note: `cmux/web` (Next.js) is the **marketing/docs site + cloud control plane**,
> **not** the app UI — reuse it only as a component/styling reference.

## Windows shell (apps/desktop) — MVP done

- `src-tauri/src/terminal.rs` — ConPTY bridge (`terminal_open/write/resize/close`,
  base64 output events). `withGlobalTauri` on; `windows_subsystem="windows"` in
  release.
- `web/` — current **vanilla-TS** xterm frontend + `tauri-bridge.ts`
  (`callNative`/`listenNative`). Upgraded to React in Phase 1.
- `packages/core-types` — the ts-rs Rust→TS bridge.
- `dev/sandbox/` — Windows Sandbox harness for the unsigned dev build.

## Dev/build infra (done)

- **Static-CRT** (`.cargo/config.toml` `+crt-static`) — self-contained binaries,
  no VC++ redistributable dependency.
- **Windows Sandbox harness** (`apps/desktop/dev/sandbox/launch.cmd`).
- **Windowed release** build; **debug keeps a console** for logs.

## Reuse-vs-rebuild matrix

| UI area | macOS today | Verdict for the Windows/WebView2 app |
|---|---|---|
| Terminal surface | native Ghostty/Metal | **Rebuilt** — xterm.js + ConPTY (done) |
| Tabs / split panes | SwiftUI + bonsplit | **Rebuild** in React over the typed session model |
| Sidebar (workspaces, file explorer) | SwiftUI | **Rebuild** in React |
| Command palette | SwiftUI | **Rebuild** in React |
| Settings | SwiftUI | **Rebuild** in React; config via `cmux.json` |
| **Agent chat (rendering)** | WKWebView (React/Solid) | **Reuse** — `webviews/agent-session`, swap host bridge |
| Agent chat (parsing/model) | Swift `CmuxAgentChat` (57 files) | **Port** to Rust (co-locate with `cmux-agent`) |
| **Diff viewer** | WKWebView (`@pierre/diffs`) | **Reuse** near-wholesale |
| **Markdown / mermaid viewer** | static WKWebView bundle | **Reuse** near-wholesale |
| Browser panes | native WKWebView host | Rebuild host on WebView2 (later/optional) |

**Biggest reuse wins:** the `webviews/` React app (chat + diff + prompt editor)
and the static markdown viewer, both plugged into the Phase-1 host bridge; and
`@cmux/core-types` as the session-model contract.

**Must-rebuild:** the window shell (tabs, splits, sidebar, palette, settings) and
the terminal (already done). Plus porting the `CmuxAgentChat` transcript parsers
from Swift to Rust.
