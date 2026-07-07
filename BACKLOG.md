# Desktop-web UI parity backlog (canonical cmux → ashlrmux)

Derived from a 7-agent parity map (workflow `wj6yyvzuk`, 2026-07-06). Drives the
milestone loop: pick the first unchecked slice whose deps are met, build →
test → `/simplify` → retest → commit.

## The shape of the work

The Rust logic crates and most pure TS modules are **already ported and tested**;
even several React components exist. The remaining frontier is almost entirely
**wiring**: register a Tauri command, add a crate dep to `apps/desktop/src-tauri`,
mount a component, thread data through `useSession`. Most slices are S/M and
**headless-testable at the logic layer**. GUI-only verification (`headlessTestable:
false`) is flagged per slice — those get logic tests + a deferred manual-verify note.

## Parallelism & contention (how many milestones can run at once)

The seven areas are **largely file-disjoint and can proceed in parallel**, but they
funnel through a few shared files. Treat these as **serialize zones** — never edit
from two concurrent worktrees:

- `apps/desktop/src-tauri/src/lib.rs` — the `invoke_handler!` registry + `Cargo.toml`
  deps. EVERY Tauri-wiring slice touches it. (Land command registrations one at a time.)
- `apps/desktop/src-tauri/src/session.rs` + `crates/cmux-core/src/session.rs` +
  `crates/cmux-golden` — sidebar, splits, canvas, placement all mutate these.
- `apps/desktop/web/src/hooks/useSession.ts` — sidebar + splits + canvas.
- `apps/desktop/web/src/components/Workspace.tsx` — splits + agent + diff surface switch.
- `apps/desktop/web/src/App.tsx` + `styles.css` — window-chrome + sidebar. Namespace
  new CSS under a per-area prefix.

**Independent (safe-to-parallelize) area cores:** Settings (`settings/*`,
`SettingsPane.tsx`, `config.rs`, cmux-config/appearance/settings-search) · Command
palette (`palette/*`, `CommandPalette*`) · Markdown (`markdown.rs`, cmux-markdown) ·
Agent-session (`agent_session.rs`, `AgentSessionSurface.tsx`, cmux-agent*) · Window
chrome (titlebar, cmux-window-title). Each still lands its Tauri command through the
`lib.rs` serialize zone.

Per-iteration strategy: build one coherent slice serially, OR fan out 2–4
**disjoint-file** slices via a worktree-isolated workflow and integrate the `lib.rs`
registrations last.

---

## Area A — Sidebar / workspace list  *(highest visible gap)*
Live today: flat single-select list + new/close (shipped 2026-07-06). Rich port
(`cmux-workspaces`, `sidebar/*`, `WorkspaceList.tsx`) exists but unwired; data model
lacks `group_id`/`is_pinned`.

- [x] A1 — Extend `SessionWorkspaceSnapshot` with `group_id: Option<String>` +
  `is_pinned: bool`; regen core-types; golden stays byte-stable. `[S, deps: none, headless]` — done 2026-07-07 (`23b8cfc29`); modeled `is_pinned` as `Option<bool>` omit-when-none (not bare `bool`) to keep Swift-authored golden fixtures byte-identical (verified SOLID vs `SessionPersistence.swift:1833-1834`). If strict `bool` typing is later required: `pub is_pinned: bool` + `skip_serializing_if="std::ops::Not::not"`.
- [x] A2 — Hide the ✕ on the sole workspace row (canonical disables close on last tab). `[XS, deps: none, headless]` — done 2026-07-06; split `Sidebar` into `SidebarView`(pure)+container, added `Sidebar.test.tsx`.
- [ ] A3 — Add `cmux-workspaces` dep to src-tauri + a `render_items` projection helper. `[S, deps: A1, headless]`
- [ ] A4 — Wire `WorkspaceList.tsx` into the live sidebar via `renderItems`. `[M, deps: A3, headless]`
- [ ] A5 — Group collapse/expand (`session_set_group_collapsed`). `[M, deps: A4, headless]`
- [ ] A6 — Inline rename (`session_rename_workspace`; empty clears custom_title). `[M, deps: A4, headless]`
- [ ] A7 — Pin/unpin + pinned-ahead reorder (`session_set_workspace_pinned`). `[M, deps: A1,A4, headless]`
- [ ] A8 — Multi-select + shift-click ranges (port `selection.ts` anchor policy). `[M, deps: A4, headless]`
- [ ] A9 — New-workspace placement (feed `placement.ts` into `session_new_workspace`). `[S, deps: A1, headless]`
- [ ] A10 — Drag-reorder rows + drop-on-group (`session_reorder_workspaces`). `[L, deps: A7, headless]`
- [ ] A11 — Per-row + per-group context menus (shared action dispatch). `[L, deps: A6,A7,A8, headless]`
- [ ] A12 — Tab/group color tinting (bridge `cmux-workspaces` tab_colors). `[M, deps: A11, headless]`
- [ ] A13 — Git/PR badges data contract + render (live polling deferred). `[L, deps: A4, headless]`
- [ ] A14 — Right-sidebar Sessions Index skeleton (separate surface). `[L, deps: none, headless]`

## Area B — Window chrome + tab strip
Today: OS title bar; `App.tsx` header is a toggle + static label. `cmux-window-title`
ported but unwired.

- [ ] B1 — `decorations:false` + custom HTML title bar w/ Windows caption buttons (min/max/close). `[M, deps: none, gui-verify]`
- [ ] B2 — Draggable title band (`-webkit-app-region`) + no-drag opt-outs + dbl-click-maximize. `[S, deps: B1, gui-verify]`
- [ ] B3 — Port WindowChromeMetrics constants (28pt band etc.) to a shared metrics module. `[S, deps: none, headless]`
- [ ] B4 — Wire `cmux-window-title` → `window.set_title` from live state. `[M, deps: none, headless]`
- [ ] B5 — Titlebar control cluster (5 slots, canonical a11y ids). `[M, deps: B1, gui-verify]`
- [ ] B6 — Minimal-mode presentation toggle + hover-reveal state machine. `[L, deps: B3,B5, headless]`
- [ ] B7 — Minimal-mode tab-strip inset geometry (Windows caption side). `[M, deps: B6, headless]`

## Area C — Splits, dividers, 2D canvas
Splits live + well-tested; several pure ops (equalize, resize, directional) have NO
caller. Canvas ENTIRELY absent from web but `cmux-canvas` + data model exist.

- [ ] C1 — Directional split insertion (thread `SPLIT_DIRECTION.insertFirst` → `session_split`). `[S, deps: none, headless]`
- [ ] C2 — Equalize dividers action (apply `equalizeDividerPlan`). `[S, deps: none, headless]`
- [ ] C3 — Keyboard divider resize (`resizeDividerAdjustment` + key handler). `[S, deps: none, headless]`
- [ ] C4 — Directional pane focus (needs a web focused-pane concept). `[M, deps: none, headless]`
- [ ] C5 — `layout_mode` plumbing + `session_set_layout_mode` (seed canvas from splits). `[M, deps: none, headless]`
- [ ] C6 — Port `cmux-canvas` geometry/layout/placer to TS modules. `[L, deps: C5, headless]`
- [ ] C7 — `CanvasSurface` component (floating panes, never-remount invariant). `[L, deps: C6, gui-verify]`
- [ ] C8 — Snap engine + guide lines. `[M, deps: C7, headless]`
- [ ] C9 — Aligner commands + one CanvasAction dispatcher. `[M, deps: C6, headless]`
- [ ] C10 — Canvas viewport (pan/zoom/overview/reveal). `[L, deps: C7, headless]`
- [ ] C11 — Canvas spatial-nav focus. `[S, deps: C6, headless]`
- [ ] C12 — Canvas session persistence round-trip. `[M, deps: C5, headless]`
- [ ] C13 — CanvasConfig settings (paneGap, snappingEnabled). `[S, deps: C8, headless]`

## Area D — Command palette + fuzzy switcher  *(all logic ported, zero live)*
- [x] D1 — Port `window_store` visibility/selection/escape state machine to TS. `[M, deps: none, headless]` — done 2026-07-06; `palette/windowStore.ts` class + 18 tests (faithful port). NOTE for D4: `paletteSelection` reducer (clamps `[0,count-1]`) is the selection source of truth, not the store's looser `>=0` clamp.
- [x] D2 — Tauri search bridge (`orchestrator.*_search_matches`; add cmux-command-palette+cmux-mentions deps). `[M, deps: none, headless]` — done 2026-07-06; `src-tauri/src/command_palette.rs` `command_palette_search` command over `preview_search_matches` (scoring stays in the orchestrator); 6 Rust tests. Web-side `host.invoke("command_palette_search", …)` wrapper lands with D4.
- [~] D3 — Query input + scope hook (`listScope` + `paletteSelection`). `[S, deps: none, headless]` — pure model `palette/paletteQuery.ts` done 2026-07-06 (scope/matching derivation, query-change re-anchor, results clamp, scope-flip reset; +6 tests). The thin React hook lands with D4 wiring.
- [ ] D4 — Live overlay host: mount, open-shortcut, focus, Escape, arrow/click/Enter. `[M, deps: D3,D2, gui-verify]` — NOTE: track "results currently shown" independently of `paletteQuery` `selection.count` (which drops to 0 between an async query change and `applyResults`), else a scope-flip reset can be missed mid-query.
- [x] D5 — Command catalog + activation dispatch (ONE shared action path). `[M, deps: none, headless]` — done 2026-07-07 (`2bf07ee48`); `palette/commandCatalog.ts` ports all 117 canonical contributions (ContentView.swift:6321-7464) in declared order + a single id→intent registry/`dispatchCommand`. Config override structurally gated to the 4 canonical configurable ids. Runtime sub-lists (extension sidebar, canvas, settings toggles, color palette, terminal targets, cmux.json actions) are injectable at exact Swift `contentsOf:` positions — D4 supplies them from live host state for absolute-rank parity.
- [x] D6 — Live switcher-entry producer (`switcherIndex` from workspace state). `[M, deps: none, headless]` — done 2026-07-07 (`2bf07ee48`); `palette/switcherEntries.ts` (single-window path, ContentView.swift:5249-5358). Uses only existing snapshot fields; branch/ports/description keywords await Lane-A future fields (git/PR badge data, A13).
- [ ] D7 — Render-sequencing guard (monotonic seq + resultsVersion). `[S, deps: D2, headless]`
- [ ] D8 — Scroll-follow + hover selection. `[S, deps: D4, headless]`
- [ ] D9 — Sync-seed + preserve-empty-while-pending gating. `[S, deps: D2, headless]`
- [ ] D10 — Settings-toggle palette surface. `[L, deps: D5, headless]`

## Area E — Settings / config UI  *(4 of 17 panes; nothing live)*
- [ ] E1 — `config_load` Tauri cmd (add cmux-config dep; preserve unmodeled sections via `Config::extra`). `[M, deps: none, headless]`
- [ ] E2 — `config_save` via dotted-JSONPath set/remove onto raw tree (NOT typed re-serialize). `[M, deps: E1, headless]`
- [ ] E3 — Config-delta representation (ConfigAction → dotted path). `[M, deps: E2, headless]`
- [ ] E4 — Wire `SettingsPane` into the app (load on mount, persist onChange). `[M, deps: E2,E3, gui-verify]`
- [ ] E5 — Live cmux.json reload (notify watcher → config-changed event). `[M, deps: E1, gui-verify]`
- [ ] E6 — SettingsPane shortcuts list uses `shortcutFormat.ts`. `[S, deps: none, headless]`
- [ ] E7 — Editable shortcut capture (key recorder → `setShortcutBinding`). `[L, deps: E3, headless]`
- [ ] E8 — Settings search box wired to `cmux-settings-search`. `[M, deps: none, headless]`
- [ ] E9 — Apply appearance (`appearanceMode.ts` → document theme + persist). `[S, deps: none, headless]`
- [ ] E10 — Raw settings.json editor pane. `[L, deps: E2, gui-verify]`
- [ ] E11 — Remaining canonical panes (terminal/browser/automation/…). `[L, deps: E3,E4, headless]`

## Area F — Agent session / chat  *(chat UI reused verbatim; port is the host)*
Claude end-to-end live; Codex/OpenCode route through the store. Missing host emissions.
- [ ] F1 — Codex end-to-end live verify + enable (handshake drain, approvals echo). `[M, deps: none, headless]`
- [ ] F2 — Emit `app.rateLimitRows` (parse Codex usage → RateLimitFooter). `[L, deps: F1, headless]`
- [ ] F3 — Dynamic `app.theme` push (light/dark from shell). `[M, deps: none, headless]`
- [ ] F4 — Attention/flash on turnComplete/exit while unfocused. `[M, deps: none, headless]`
- [ ] F5 — `provider.select` persistence + seed `initialProviderId`. `[S, deps: none, headless]`
- [ ] F6 — Session restore / transcript replay (no double auto-start). `[L, deps: F1, headless]`
- [ ] F7 — JA localization of the 67-key copy dict. `[M, deps: none, headless]`

## Area G — Diff / Markdown / Browser
- [ ] G1 — Markdown doc feed: `markdown_set_document` sets `PanelCtx.file_path` THEN pushes render. `[M, deps: none, headless]`
- [ ] G2 — Remote-image host layer (DNS-pin SSRF gate + TLS fetch). `[M, deps: none, headless]`
- [ ] G3 — Markdown typography controls. `[M, deps: none, headless]`
- [ ] G4 — Markdown link-open (`openMarkdownFile` → new surface in owning pane). `[M, deps: G1, gui-verify]`
- [ ] G5 — Diff comments WebView2 shim (`cmuxDiffComments.postMessage` → invoke). `[S, deps: none, headless]`
- [ ] G6 — Port `__diff-viewer-refs/-branch` CLI (git + token + manifest jail). `[L, deps: none, headless]`
- [ ] G7 — Build+serve the `webviews/` diff app as `cmux-diff-viewer` assets. `[L, deps: G6, gui-verify]`
- [ ] G8 — Live diff token wiring (replace `token={null}`). `[S, deps: G6, headless]`
- [ ] G9 — submission_pool workspace-id glue. `[M, deps: G5, headless]`
- [ ] G10 — Wire `cmux-browser-history` (sanitizer + nav availability). `[M, deps: none, headless]`
- [ ] G11 — Browser surface skeleton (WebView2 child, omnibar). `[L, deps: G10, gui-verify]`

---

## Suggested first parallel wave (disjoint files, all headless, deps met)
Land in separate iterations or one worktree-isolated fan-out; register each
Tauri command through `lib.rs` last:
- **A1** (data model) · **A2** (✕ hide) — Area A foundation
- **C1 + C2** (directional split + equalize) — Area C, pure-module wiring
- **G1** (markdown doc feed) · **G5** (diff comments shim) — Area G
- **B3 + B4** (chrome metrics + window title) — Area B
- **E9 + E6** (appearance apply + shortcut format) — Area E
- **F5** (provider.select persistence) — Area F
- **D1 + D3** (palette window-store + query hook) — Area D

Each is small, independent, and headless-testable — a clean per-iteration win.
