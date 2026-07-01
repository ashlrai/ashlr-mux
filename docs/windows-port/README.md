# cmux for Windows — Roadmap

**Architecture (one line):** a **Tauri 2 (Rust) shell** with a **React/WebView2
UI** over the extracted **Rust core**, rendering the terminal with **xterm.js
over ConPTY**. Behavior-parity with macOS cmux; the UI layer is web, not SwiftUI.

**Status:** active. Branch `windows-port`. **Phase 0 is done and proven** — a real
Windows window with a live shell over ConPTY.

This file is the **thin index**; depth lives in the linked docs.

## Read in this order

1. [`00-strategy.md`](./00-strategy.md) — goals, the pivot, the fidelity
   principle, and what changed vs the archived plan.
2. [`01-architecture.md`](./01-architecture.md) — the layers, the three reuse
   seams, and the host bridge.
3. [`02-assets-and-reuse.md`](./02-assets-and-reuse.md) — what already exists
   (Rust crates, cmux webviews, `core-types`) and the reuse-vs-rebuild matrix.
4. [`90-cross-cutting-and-risks.md`](./90-cross-cutting-and-risks.md) — dev
   workflow, data model, testing, localization, packaging, and the risk register.
5. [`91-parity-map.md`](./91-parity-map.md) — the archived M0–M16 milestones →
   new-phase mapping.
6. The phase roadmap below (each phase is its own doc under `phases/`).

## Phase roadmap

Effort: **S** ≤ a few days · **M** ~1–2 wk · **L** ~3–6 wk.

| # | Phase | Effort | Depends on | Status |
|---|---|---|---|---|
| 0 | [Foundation](./phases/phase-0-foundation.md) | — | — | ✅ done |
| 1 | [React UI foundation + host bridge](./phases/phase-1-react-foundation-bridge.md) | M | 0 | **next** |
| 2 | [Workspace shell: tabs + splits](./phases/phase-2-workspace-shell.md) | L | 1 | |
| 3 | [Canonical agent sessions](./phases/phase-3-agents.md) | L | 1, 2 | |
| 4 | [Diff + markdown surfaces](./phases/phase-4-diff-markdown.md) | M | 1 | |
| 5 | [Sidebar, palette, settings, config, i18n](./phases/phase-5-chrome-config.md) | L | 2 | |
| 6 | [Polish, integration & ship](./phases/phase-6-polish-ship.md) | L | 2–5 | |

## Critical path & phasing

- **0 → 1 → 2** is the spine: foundation, then the React stack + host bridge, then
  the tab/split workspace shell.
- **3 (agents)** and **4 (diff/markdown)** run in parallel off 1/2 once the host
  bridge exists.
- **5 (chrome/config)** off 2; **6 (polish/ship)** last.
- **Optional tracks** — Go daemon (`cmuxd`) lifecycle, the CLI per-command socket
  layer, backend/cloud — stay **off the critical path** (see `90`).

## Provenance

Supersedes `windows-port-plan/` (the M0–M16 native-port plan, **archived**). The
native Rust GPU terminal track (M2 `wgpu`/alacritty → M16 libghostty) is dropped
in favor of xterm.js; delivery is re-sequenced around the working MVP by user
value. See [`91-parity-map.md`](./91-parity-map.md).
