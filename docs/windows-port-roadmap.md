# cmux for Windows — Roadmap

**Architecture:** Tauri 2 (Rust) shell + **React/WebView2 UI** over the extracted
**Rust core**, rendering the terminal with **xterm.js over ConPTY**.

**Status:** active. Branch `windows-port`. Foundation (Rust core + terminal MVP)
is **proven working** — a real Windows window with a live shell.

**Supersedes:** `windows-port-plan/` (the M0–M16 native-port plan), now archived.
See [§8 Parity map](#8-feature-parity-checklist-old-m0m16--new) for where each
old milestone lands.

---

## 0. TL;DR

Build a great Windows cmux **fast and lean** by combining three things that
already exist: (1) the **Rust cross-platform core** we extracted (terminal,
process, IPC, agent resolver, session model), (2) cmux's **existing React
webviews** (agent chat, diff viewer, markdown, prompt editor — already
WKWebView-hosted on macOS, so Chromium/WebView2-portable), and (3) **Tauri** as a
light native shell (system WebView2, not bundled Chromium). The UI is built in
**React for behavior-parity** with macOS cmux — same features, workflows,
keybindings, and data model — not a pixel-faithful SwiftUI reimplementation. We
deliver in **value-sequenced phases**, keeping the app runnable at every step.

**Three goals this optimizes for:**
- **Not clunky** — system WebView2 + xterm.js; no bundled Chromium, no custom GPU
  renderer to babysit.
- **Less code** — reuse the Rust core + cmux's React webviews + the ts-rs type
  bridge instead of reimplementing SwiftUI natively.
- **Quality preserved** — behavior-parity on a mature web stack (React 19,
  Tailwind 4), driven by the same typed session model as macOS.

---

## 1. Strategy & rationale

### The pivot
The original plan (`windows-port-plan/`) already chose a **Tauri shell that reuses
cmux's web UI** — a sound call. Its one expensive commitment was an **XL native
Rust GPU terminal** (alacritty engine + `wgpu`, M2 → forked libghostty with a D3D
renderer, M16). This roadmap's delta:

1. **Drop the native GPU terminal track** (M2 `wgpu`, M16 libghostty) and render
   the terminal with **xterm.js in WebView2** over the already-built
   `cmux-terminal::conpty` ConPTY backend. Reversible later behind the same
   backend if a native renderer is ever justified.
2. **Sequence around a working MVP** (proven) and deliver by user value, not by a
   dependency-ordered milestone graph.
3. **Build the UI in React for behavior-parity**, extending cmux's existing
   webviews rather than reimplementing the SwiftUI shell.

### Fidelity principle
"Canonical fidelity" means **behavior / UX / data-model parity**, *not* SwiftUI
implementation parity. Mirror what cmux *does* (features, workflows, keybindings,
config schema, session model). Sanctioned divergences, both at the
implementation layer only:
- **Renderer:** xterm.js instead of native Ghostty/Metal.
- **UI shell:** React in WebView2 instead of SwiftUI/AppKit.

When behavior is unclear, read the Swift sources (`Sources/`, `Packages/`) and
mirror them; don't invent.

---

## 2. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  WebView2 (Chromium)  —  React 19 + Vite + Tailwind 4 UI      │
│                                                              │
│  Workspace shell (tabs, splits, sidebar, palette, settings)  │  ← rebuilt in React
│  Terminal surface (xterm.js)   Agent chat  Diff  Markdown    │  ← chat/diff/md REUSED from webviews/
│                     │  host bridge (invoke / events)          │
└─────────────────────┼────────────────────────────────────────┘
                      │  Tauri IPC  (commands + events)
┌─────────────────────┴────────────────────────────────────────┐
│  Rust (Tauri backend)  —  the extracted cross-platform core   │
│                                                              │
│  cmux-terminal (ConPTY/engine)   cmux-process (Job Objects)   │
│  cmux-agent (resolver/launch)    cmux-ipc (control socket)    │
│  cmux-core (session model + notifications)  cmux-windowing    │
│  cmux-cli   cmux-golden                      (+ Go cmuxd)     │
└──────────────────────────────────────────────────────────────┘
        │ ts-rs generates @cmux/core-types (shared wire model)
        └──────────────────────────────────────────────────────►  consumed by the React UI
```

**Three seams that make this lean:**
- **`@cmux/core-types`** — TypeScript types generated from `crates/cmux-core` via
  ts-rs (`AppSessionSnapshot`, `SessionWindowSnapshot`,
  `SessionSplitLayoutSnapshot`, `SessionTabManagerSnapshot`,
  `SessionWorkspace*Snapshot`, …). The UI renders an already-typed model; we
  extend this bridge rather than invent an IPC contract.
- **The host bridge** — the Tauri equivalent of macOS's WKWebView
  `WKScriptMessageHandler` + custom-URL-scheme contract (`CmuxWebView.swift`,
  `DiffCommentsBridge.swift`). Built once so every reused webview surface plugs
  in unmodified. See [§6](#6-host-bridge-contract).
- **Tauri commands/events** — `callNative`/`listenNative` (already in
  `apps/desktop/web/src/tauri-bridge.ts`) is the template for every surface.

---

## 3. Asset inventory (what already exists)

### Rust core (crates/) — built & tested, headless
| Crate | Role | Status |
|---|---|---|
| `cmux-core` | Session/window/tab/split model + notification store; ts-rs source | Done (M1, M10) |
| `cmux-terminal` | ConPTY wrapper, engine, surface, OSC-133, links | Done (M2 engine layer) |
| `cmux-process` | Job-Object supervisor, zero-orphan tree-kill, NDJSON, captured stdio | Done (M3) |
| `cmux-agent` | Executable resolver (PATHEXT/env-policy), `LaunchPlan`→`SpawnSpec`, OpenCode auth | Done (M3) |
| `cmux-ipc` | Control socket: named-pipe server, auth gate, client, v1/v2 codecs | Done (M4 core) |
| `cmux-cli` | `cmux` CLI: invocation, socket/password resolution, `rpc`, classify/dispatch | Foundation done; per-command socket layer blocked on app/server contract |
| `cmux-windowing` | Window geometry + session restore | Done (M5 geometry) |
| `cmux-golden` | Golden-parity harness vs Swift | Done |
| `cmuxd` (Go) | Remote daemon sidecar | Compiles on Windows; lifecycle WS1–3 pending |

### Reusable web (cmux/webviews → `@cmux/webviews`)
React 19 + TanStack Router + Vite 7 + Tailwind 4 + React Compiler. Builds to
static bundles loaded into WKWebView on macOS. Contains:
- `agent-session/` — the agent chat renderer (React; a Solid variant also exists).
- `surfaces/` — surface hosts; `comments/` — diff comments.
- Diff viewer via `@pierre/diffs` + `@pierre/trees` + `worker-pool.ts`.
- Prompt editor (ProseMirror). Router in `router.tsx`.
- `Resources/markdown-viewer/` — static markdown/mermaid/vega renderer
  (marked + highlight.js + mermaid + vega); trivially portable.

### Windows shell (apps/desktop) — MVP done
- `src-tauri/` — Tauri app; `terminal.rs` (ConPTY bridge: `terminal_open/write/
  resize/close`, base64 output events). `withGlobalTauri` on;
  `windows_subsystem="windows"` in release.
- `web/` — current **vanilla-TS** xterm frontend + `tauri-bridge.ts`
  (`callNative`/`listenNative`). To be upgraded to React in Phase 1.
- `packages/core-types` — the ts-rs Rust→TS bridge (extend in every phase).
- `dev/sandbox/` — Windows Sandbox harness for running the unsigned dev build.

### Dev/build infra (done)
- **Static-CRT** (`.cargo/config.toml` `+crt-static`) — self-contained binaries,
  no VC++ redistributable.
- **Windows Sandbox harness** (`apps/desktop/dev/sandbox/launch.cmd`) — run the
  unsigned build without disabling Smart App Control.
- **Windowed release** build; **debug keeps a console** for logs.

---

## 4. Reuse vs rebuild

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

---

## 5. Phases

Each phase keeps the app runnable end-to-end. Effort is rough: **S** ≤ a few
days, **M** ~1–2 wk, **L** ~3–6 wk.

### Phase 0 — Foundation ✅ DONE
**Rust core + terminal MVP + dev infra.** A window with a live PowerShell shell
over ConPTY, xterm.js in WebView2. Static-CRT, sandbox harness, windowed release,
host launch (SAC off) all working.
- Commits: `07bebae` (terminal), `d9277e7` (withGlobalTauri), `8294d66`
  (sandbox), `219a0f4` (static-CRT), `ee186b9` (windows_subsystem).
- **Acceptance (met):** app launches on host + in sandbox; type/run/resize work.

### Phase 1 — React UI foundation + host bridge   [M, next]
**Goal:** move onto the real UI stack and build the reusable bridge, with the
terminal behavior unchanged.
- **Tasks**
  1. Stand up **React 19 + Vite 7 + Tailwind 4** in `apps/desktop/web`, aligned
     with the `webviews/` toolchain (shared tsconfig/Tailwind conventions; ideally
     a workspace so `@cmux/webviews` components import cleanly).
  2. Replace the `bun` build script with **Vite build**; point
     `tauri.conf.json` `frontendDist` at Vite's `dist`; use a Vite **dev server +
     HMR** via `devUrl` for iteration.
  3. Port the xterm terminal into a React **`<TerminalSurface>`** component,
     keeping the `callNative`/`listenNative` bridge.
  4. **Define & implement the Tauri host bridge** (TS) that mirrors the macOS
     `WKScriptMessageHandler` + custom-scheme contract, so reused webviews run by
     swapping only their host adapter. Read the macOS bridge first (§6).
- **Reuse:** xterm (done), `webviews/` toolchain config, `core-types`.
- **New:** Vite/React scaffold; host-bridge adapter module.
- **Deliverable:** the same terminal app, now on React+Vite+Tailwind, with the
  host bridge in place and one trivial reused webviews component rendering.
- **Acceptance:** terminal identical; Vite build feeds Tauri; host-bridge unit
  tests pass; HMR works under `tauri dev`.
- **Risks:** Vite↔Tauri config; HMR under WebView2; getting the bridge contract
  faithful to macOS.

### Phase 2 — Workspace shell: tabs + splits   [L]
**Goal:** multiple terminal surfaces in tabs and split panes, rendering the core
session model.
- **Tasks**
  1. Expose the session model over Tauri: read
     `SessionWindow/Split/TabManager` snapshots from `cmux-windowing`/`cmux-core`;
     commands to open/close/split/focus/move/resize surfaces.
  2. Render those snapshots in React using `@cmux/core-types`.
  3. **Split-pane** component with behavior-parity for drag/resize/nesting
     (evaluate porting bonsplit semantics vs a React split lib).
  4. Tab bar + surface lifecycle (spawn a ConPTY per surface); focus/keyboard
     routing.
  5. Session persistence/restore via `cmux-windowing`.
- **Reuse:** `core-types`, `cmux-windowing`, `cmux-terminal`, `cmux-core`.
- **New:** React shell (tab bar, split container, surface host); Tauri session
  commands.
- **Deliverable:** a windowed workspace of tabs + split live shells; layout
  survives restart.
- **Acceptance:** open/close/split/focus/resize; restored layout matches; typing
  latency stays crisp with several surfaces.
- **Risks:** split-pane parity; focus routing; latency with many surfaces.

### Phase 3 — Canonical agent sessions   [L]
**Goal:** run Claude / Codex / OpenCode as **canonical agent sessions** (headless
transport) with the reused chat UI — *not* a raw TUI in a terminal.
- **Tasks**
  1. Spawn agents via `cmux-agent` resolve → `plan.to_spawn_spec()` →
     `cmux-process` (Job-Object supervised), using each provider's transport:
     Codex `stdio-jsonrpc`, Claude `stdio-jsonl`, OpenCode `http-loopback`.
  2. **Port the `CmuxAgentChat` transcript parsers** (Claude JSONL, Codex
     JSON-RPC) from Swift → **Rust** (co-located with `cmux-agent`), covered by
     golden tests; expose state through `core-types`.
  3. Wire the **reused `webviews/agent-session`** React renderer to the host
     bridge (replace the WKWebView bridge); stream transcript/state via events.
  4. Make an **agent session a surface type** in the workspace (a tab is a shell
     *or* an agent).
  5. Provider specifics: OpenCode loopback creds (already minted by `cmux-agent`),
     Codex app-server JSON-RPC client, Claude stream-json client.
- **Reuse:** `cmux-agent`, `cmux-process`, `webviews/agent-session`, `core-types`.
- **New:** transport clients (Rust); transcript-parser port; agent-surface
  wiring; bridge event plumbing.
- **Deliverable:** launch an agent in a tab; the canonical chat UI renders; you
  can converse.
- **Acceptance:** each provider connects, streams, and renders; session lifecycle
  (start/stop, auto-start policy) matches canonical; parser golden tests pass.
- **Risks:** transport parity; parser fidelity (golden-test it); provider auth on
  Windows.

### Phase 4 — Diff & markdown surfaces   [M]
**Goal:** reuse the diff viewer and markdown/mermaid renderer as in-app panels.
- **Tasks**
  1. Build the `@pierre/diffs` diff viewer (from `webviews/`) into the app; wire
     the `worker-pool`; feed diffs via the bridge.
  2. Copy the static `Resources/markdown-viewer/` renderer; render agent/file
     markdown (+ mermaid/vega).
  3. Port the diff-comments bridge (`DiffCommentsBridge`) to Tauri.
- **Reuse:** near-wholesale.
- **New:** bridge adapters; diff data source (git integration scope TBD).
- **Deliverable:** view diffs + rich markdown in-app.
- **Acceptance:** diff renders large files via the worker pool; markdown + mermaid
  render.
- **Risks:** worker-pool under Vite/WebView2; scope of the git/diff data source.

### Phase 5 — Sidebar, command palette, settings, config, shortcuts, i18n   [L]
**Goal:** the surrounding chrome + configuration, canonical.
- **Tasks**
  1. **Sidebar:** workspaces list + file explorer (React); data from Rust (fs +
     workspace model).
  2. **Command palette** (React) with an action registry mirroring cmux commands.
  3. **Settings** UI (React) + **`cmux.json`** config (canonical schema)
     read/write via Rust; **keyboard-shortcut settings** (every cmux-owned
     shortcut editable + documented — see the shortcut policy in `CLAUDE.md`).
  4. **i18n:** en/ja message catalogs (localization is a canonical requirement;
     run the localization audit on every user-facing change).
- **Reuse:** the React component base; `core-types`; the `cmux.json` schema.
- **New:** sidebar/palette/settings React; config Rust layer; i18n wiring.
- **Deliverable:** full app chrome; configurable; localized.
- **Acceptance:** settings persist to `cmux.json`; shortcuts editable; en + ja.
- **Risks:** scope; keeping the localization audit disciplined.

### Phase 6 — Polish, integration & ship   [L]
**Goal:** an installable, signed Windows beta.
- **Tasks:** notifications (`cmux-core` store → Windows toast); session-restore
  polish; theming/appearance; **performance pass** (typing latency, large
  output); **packaging** (Tauri bundler → MSI/NSIS); **code signing** (a
  reputable cert — matters for Smart App Control / SmartScreen); **auto-update**;
  docs; beta.
- **Reuse:** `cmux-core` notifications (done), `cmux-windowing` restore (done).
- **Deliverable:** signed, auto-updating Windows cmux beta.
- **Risks:** signing cert & reputation; auto-update; perf under load.

### Cross-cutting / optional tracks
- **Go daemon (`cmuxd`) lifecycle** — only if remote/daemon features are in
  scope; needs a standalone Job Object without `KILL_ON_JOB_CLOSE`.
- **CLI per-command socket layer** — `cmux-cli` foundation is done; the
  per-command forwarding is **blocked on the app/server contract** (the app must
  define socket command names / arg shapes). Unblocks once Phase 2/3 land a
  control server.
- **Backend/cloud control plane** — deferred; parity reference only unless cloud
  VMs are wanted on Windows.

---

## 6. Host bridge contract

The reused webviews assume a macOS host bridge: a **custom URL scheme** for
asset/data loads plus **`window.webkit.messageHandlers.*`** for RPC/events
(`CmuxWebView.swift`, `DiffCommentsBridge.swift`). The Windows port must
re-implement that *contract* on Tauri — the effort is the host, not the UI.

**Plan:** a small TS `host` module that presents the same shape the webviews
expect, backed by Tauri:
- `host.invoke(channel, payload)` → `@tauri-apps/api` `invoke` (or `callNative`).
- `host.on(event, cb)` → Tauri event `listen`.
- asset/data loads that used the custom scheme → Tauri commands returning the
  bytes/JSON (or Tauri asset protocol).

Build it in **Phase 1**, verify against one reused component, then every later
surface (chat, diff, comments, markdown) plugs in by swapping its host adapter
only. **Action item:** enumerate the exact macOS message-handler channels +
custom-scheme routes before finalizing the shape.

---

## 7. Cross-cutting concerns

- **Dev workflow.** Smart App Control is **off** on the primary dev machine, so
  `cargo run -p cmux-desktop` (debug, keeps a console for logs) launches directly;
  the `dev/sandbox/` harness remains for SAC-on machines. Release =
  windowed/no-console.
- **Data model & IPC.** `@cmux/core-types` (ts-rs) is the single source of truth
  for the wire model; run the drift check (`check-drift.mjs`) in CI. Add new
  Tauri commands with typed payloads.
- **Testing.** Rust unit/integration in each crate; golden parity
  (`cmux-golden`) for anything with a Swift counterpart (esp. transcript
  parsers); web unit tests (`bun test`) for bridge/UI logic. Process-spawning
  tests live in lib `#[cfg(test)]` (the standalone-exe Application-Control
  gotcha).
- **Localization.** Canonical requirement: all user-facing strings localized
  (en/ja), audited on every UI-touching change.
- **Packaging/signing.** Tauri bundler; a reputable code-signing cert is needed
  for a clean install experience under SAC/SmartScreen.

---

## 8. Feature-parity checklist (old M0–M16 → new)

The archived milestones are retained as a **parity reference**. Mapping:

| Old milestone | New home |
|---|---|
| M0 Bootstrap & Windows CI | Phase 0 (CI green) + Phase 6 (CI hardening) |
| M1 Core extraction | **Phase 0 — done** |
| **M2 Terminal engine v1 (wgpu/alacritty)** | **Dropped** → xterm.js (Phase 0/1) |
| M3 Process & agent lifecycle | **Phase 0 — done** + Phase 3 |
| M4 Daemon/socket/CLI IPC | Phase 0 (core done); CLI per-command → cross-cutting |
| M5 App shell + windowing | Phase 2 (geometry done in Phase 0) |
| M6 Tabs, splits & sidebar (web) | Phase 2 (+ sidebar in Phase 5) |
| M7 Browser pane + webview hosts | Phase 4 (webview hosts) + browser pane later |
| M8 Agent integration | Phase 3 |
| M9 Settings, config, shortcuts, i18n | Phase 5 |
| M10 Notifications | Phase 6 (store done in `cmux-core`) |
| M11 Backend/cloud control plane | Deferred (optional track) |
| M12 Packaging, signing, update | Phase 6 |
| M13 Testing & CI hardening | Cross-cutting + Phase 6 |
| M14 Performance & parity | Phase 6 (perf pass) |
| M15 Beta, docs & release | Phase 6 |
| **M16 Terminal engine v2 (libghostty)** | **Dropped/deferred** (xterm.js is the renderer) |

---

## 9. Risks & open questions

- **Host-bridge fidelity** — the reused webviews depend on the macOS host
  contract; mis-modeling it stalls chat/diff reuse. *Mitigate:* enumerate the
  channels/routes first; build & test the bridge in Phase 1.
- **Split-pane parity** — bonsplit behavior (nesting, drag, ratios) is subtle.
  *Mitigate:* spike early in Phase 2; decide port-vs-library on evidence.
- **Transcript-parser fidelity** — porting `CmuxAgentChat` parsers must match
  Swift exactly. *Mitigate:* golden tests via `cmux-golden`.
- **Signing / reputation** — unsigned or low-reputation binaries hit SAC /
  SmartScreen. *Mitigate:* budget a reputable cert in Phase 6.
- **Performance/latency** — a webview terminal must stay crisp. *Mitigate:*
  latency pass in Phase 6; xterm tuning; avoid per-keystroke allocations in the
  bridge.
- **Scope creep** — backend/cloud + browser panes can balloon. *Mitigate:* keep
  them optional tracks, off the critical path.

---

## 10. Immediate next actions (Phase 1)

1. Read the macOS host-bridge contract (`CmuxWebView.swift`, `DiffCommentsBridge`,
   the custom-scheme handler) and enumerate its channels/routes.
2. Scaffold React 19 + Vite 7 + Tailwind 4 in `apps/desktop/web` aligned with
   `webviews/`; switch Tauri to the Vite `dist` + `devUrl` HMR.
3. Port the terminal to a React `<TerminalSurface>` (no behavior change).
4. Implement the host-bridge adapter and validate it against one reused
   `webviews/` component.
