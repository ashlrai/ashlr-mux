# Phase 4 — Diff & markdown surfaces   [M]

**Goal:** reuse cmux's **diff viewer** and **markdown/mermaid renderer** as in-app
panels — two of the highest-reuse assets, near-wholesale.

**Depends on:** Phase 1 (host bridge). Can run in parallel with Phase 2/3.

## Context

Both already run in WKWebView on macOS, so they are WebView2-portable. The work is
the **host contract**, not the UI.

## Tasks

1. **Diff viewer** — build the `@pierre/diffs` + `@pierre/trees` viewer (from
   `webviews/`) into the app; wire the **`worker-pool`** (large-diff performance);
   feed diffs through the host bridge.
2. **Markdown/mermaid viewer** — copy the static `Resources/markdown-viewer/`
   renderer (marked + highlight.js + mermaid + vega); render agent/file markdown.
3. **Diff comments** — port the `DiffCommentsBridge` contract to Tauri (the
   comments channel used by the diff UI).
4. **Data sources** — define where diffs come from (git integration scope TBD;
   start with explicit diff payloads, add a git provider later).

## Reuse
Near-wholesale: `webviews/` diff viewer + worker pool, `Resources/markdown-viewer`.

## New code
Host-bridge adapters for the diff/comments channels; the diff data feed.

## Deliverable
View diffs (incl. large files) and rich markdown (incl. mermaid/vega) in-app.

## Acceptance
- Diff renders large files smoothly via the worker pool.
- Markdown + mermaid + vega render.
- Diff comments round-trip through the bridge.

## Risks
- Worker-pool behavior under Vite/WebView2 (bundling web workers).
- Scope of the git/diff **data source** — keep the first cut minimal.

## Touchpoints
`cmux/webviews/src` (diff, `worker-pool.ts`, `pierre-options.ts`, `comments/`),
`cmux/Resources/markdown-viewer`, `Sources/Panels/MarkdownWebRenderer.swift` +
`DiffCommentsBridge.swift` (contract reference), `apps/desktop/src-tauri`.
