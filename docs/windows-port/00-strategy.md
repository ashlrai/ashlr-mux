# 00 — Strategy & rationale

## Goals (in priority order)

1. **A great app that works well on the laptop — not clunky.** System WebView2 +
   xterm.js; no bundled Chromium (unlike Electron), no custom GPU renderer to
   babysit.
2. **Avoid writing a crap-ton of code — but not at the expense of quality.** Reuse
   the extracted Rust core + cmux's existing React webviews + the ts-rs type
   bridge, instead of reimplementing the SwiftUI shell natively.
3. **Behavior-parity with macOS cmux.** Same features, workflows, keybindings,
   config schema, and session model.

## The pivot

The archived plan (`windows-port-plan/`) already made a good call: a **Tauri
shell that reuses cmux's web UI**. Its one expensive commitment was an **XL native
Rust GPU terminal** — an alacritty engine + `wgpu` renderer (M2), later a forked
**libghostty** with a D3D renderer (M16). This roadmap changes three things:

1. **Drop the native GPU terminal track.** Render the terminal with **xterm.js in
   WebView2** over the already-built `cmux-terminal::conpty` ConPTY backend.
   Reversible later behind the same backend if a native renderer is ever
   justified for performance.
2. **Sequence around a working MVP** (already proven) and deliver by **user
   value**, not by a dependency-ordered milestone graph.
3. **Build the UI in React for behavior-parity**, extending cmux's existing
   `webviews/` rather than reimplementing the SwiftUI shell.

## The fidelity principle

"Canonical fidelity" means **behavior / UX / data-model parity — not SwiftUI
implementation parity.** Mirror what cmux *does*: features, workflows,
keybindings, the `cmux.json` config schema, the session model. Two divergences
are sanctioned, both at the implementation layer only:

- **Renderer:** xterm.js instead of native Ghostty/Metal.
- **UI shell:** React in WebView2 instead of SwiftUI/AppKit.

When a behavior is unclear, **read the Swift sources** (`Sources/`, `Packages/`)
and mirror them. Do not invent divergent behavior. (This rule already caught one
regression: an agent "launcher" that ran a raw TUI in the terminal was reverted
because canonical cmux drives agents over a headless transport + a chat UI.)

## Why this hits all three goals

- **Not clunky** — a light webview + xterm.js is smooth for a terminal-centric
  app; no GPU renderer maintenance.
- **Less code** — the Rust core is done; the biggest UI pieces (agent chat, diff,
  markdown, prompt editor) are reused from `webviews/`; the session model is
  already typed in TS via ts-rs.
- **Quality preserved** — behavior-parity on a mature stack (React 19,
  Tailwind 4), driven by the same typed model as macOS.
