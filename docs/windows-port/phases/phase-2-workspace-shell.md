# Phase 2 — Workspace shell: tabs + splits   [L]

**Goal:** multiple terminal surfaces arranged in **tabs and split panes**,
rendering the core session model — the "real app" core loop.

**Depends on:** Phase 1. **Unblocks:** Phase 3 (agent surfaces), Phase 5 (sidebar).

## Context

macOS cmux arranges surfaces with SwiftUI + **bonsplit** and a native tab manager.
The layout is already a **typed data model** exposed via `@cmux/core-types`
(`SessionWindowSnapshot`, `SessionSplitLayoutSnapshot`,
`SessionTabManagerSnapshot`, `SessionWorkspace*Snapshot`). So this phase is mostly
**rendering an existing model** and wiring mutations — not designing one.

## Tasks

1. **Expose the session model over Tauri.** Read window/tab/split snapshots from
   `cmux-windowing`/`cmux-core`; add commands to open/close/split/focus/move/
   resize surfaces; emit snapshot-changed events.
2. **Render the snapshots in React** using `@cmux/core-types` — a window → tabs →
   split tree → surfaces.
3. **Split-pane component** with behavior-parity for drag-resize, nesting, and
   ratios. Decide **port bonsplit semantics vs. a React split library** after a
   short spike (R2).
4. **Tab bar + surface lifecycle** — spawn a ConPTY per terminal surface; create/
   close/reorder tabs; focus and keyboard routing (which surface receives input).
5. **Persistence/restore** via `cmux-windowing` — layout survives app restart.

## Reuse
`@cmux/core-types`, `cmux-windowing`, `cmux-terminal`, `cmux-core`.

## New code
React shell — tab bar, split container, surface host; Tauri session commands +
snapshot events.

## Deliverable
A windowed workspace of tabs and split live shells; the layout persists across
restarts.

## Acceptance
- Open/close/split/focus/move/resize all work.
- Restored layout matches the pre-quit state.
- Typing latency stays crisp with several concurrent surfaces (R5).

## Risks
- Split-pane parity (R2) — spike first.
- Focus/keyboard routing across surfaces.
- Latency with many surfaces (R5).

## Touchpoints
`crates/cmux-windowing`, `crates/cmux-core`, `apps/desktop/src-tauri` (session
commands), `apps/desktop/web` (shell components), `Packages/macOS/CmuxPanes`/
`CmuxWorkspaces`/bonsplit (behavior reference).
