# Phase 1 — React UI foundation + host bridge   [M · next]

**Goal:** move onto the real UI stack (React + Vite + Tailwind, aligned with
`webviews/`) and build the **reusable host bridge**, with the terminal's behavior
unchanged. This is the platform every later phase builds on.

**Depends on:** Phase 0. **Unblocks:** Phases 2–5.

## Context

The MVP frontend is vanilla TS. To reuse cmux's `webviews/` (agent chat, diff,
markdown, prompt editor) and to build the shell efficiently, we need the same
stack they use — **React 19 + Vite 7 + Tailwind 4 + TanStack Router** — and a host
adapter that presents the macOS WKWebView contract on top of Tauri.

## Tasks

1. **Enumerate the macOS host-bridge contract first.** Read `CmuxWebView.swift`,
   `DiffCommentsBridge.swift`, the custom-URL-scheme handler, and how
   `webviews/` calls `window.webkit.messageHandlers.*`. Produce a list of
   channels (RPC names + payload shapes) and custom-scheme routes. This defines
   the adapter's surface.
2. **Scaffold React + Vite + Tailwind** in `apps/desktop/web`:
   - React 19 + Vite 7 + Tailwind 4, matching `webviews/` tsconfig/Tailwind
     conventions. Prefer a workspace layout so `@cmux/webviews` components import
     cleanly (shared deps, one React instance).
   - Replace the `bun scripts/desktop/build-desktop-web.mjs` build with **Vite
     build**; point `tauri.conf.json` `frontendDist` at Vite's `dist`.
   - Wire a **Vite dev server + HMR** via `tauri.conf.json` `build.devUrl` for
     fast iteration under `tauri dev`.
3. **Port the terminal to a React `<TerminalSurface>`** component — same xterm +
   FitAddon + base64 decode + `callNative`/`listenNative`, no behavior change.
   Prove the stack end-to-end.
4. **Implement the host-bridge adapter** (`host` module, TS): `host.invoke(channel,
   payload)` → Tauri `invoke`; `host.on(event, cb)` → Tauri `listen`;
   custom-scheme asset/data loads → Tauri commands (or the asset protocol).
   Design it so a reused `webviews/` component runs by swapping only its host
   adapter.
5. **Validate the bridge** against one real reused component (e.g. the markdown
   viewer or a trivial `webviews/` surface) rendering in WebView2.

## Reuse
xterm (done), `webviews/` toolchain conventions, `@cmux/core-types`.

## New code
Vite/React/Tailwind scaffold; `<TerminalSurface>`; the `host` bridge adapter
module + its tests.

## Deliverable
The same terminal app, now on React + Vite + Tailwind, with the host bridge in
place and one reused `webviews/` component rendering through it.

## Acceptance
- Terminal behaves identically to Phase 0 (type/run/resize).
- `tauri dev` runs with Vite HMR; release build feeds Tauri from Vite `dist`.
- Host-bridge unit tests pass; the reused component renders via the bridge.
- Web tests (`bun test`) green.

## Risks
- Vite ↔ Tauri config and HMR under WebView2 (R7) — validate early.
- Getting the bridge contract faithful to macOS (R1) — enumerate channels before
  finalizing the shape.

## Touchpoints
`apps/desktop/web/*`, `apps/desktop/src-tauri/tauri.conf.json`,
`scripts/desktop/build-desktop-web.mjs` (replaced), `cmux/webviews/*` (reference),
`Sources/*CmuxWebView*.swift` (contract reference).
