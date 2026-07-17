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
- [x] A3 — Add `cmux-workspaces` dep to src-tauri + a `render_items` projection helper. `[S, deps: A1, headless]` — done 2026-07-07 (`4a34e56dd`); pure `src-tauri/src/sidebar_render.rs` `render_items(&SessionTabManagerSnapshot) -> Vec<SidebarWorkspaceRenderItem>` over golden-pinned `cmux_workspaces::render_items`. Group anchor uses the canonical 3-tier restore fallback (`TabManager.swift:6018-6027`). `#[allow(dead_code)]` until A4 wires it. Verify FIXED-1 (anchor fallback was anchor_workspace_id-only). NOTE: workspace_id None → row skipped (deferred divergence: canonical restore mints a fresh UUID once in the stateful restore layer — that belongs in A4, not this stateless projection).
- [x] A4 — Wire `WorkspaceList.tsx` into the live sidebar via `renderItems`. `[M, deps: A3, headless]` — done 2026-07-07 (`5309ca07c`); web-side projection (`sidebar/snapshotProjection.ts`, tested twin of `sidebar_render.rs`) + interactive WorkspaceList (titles/selection/close/anchor-activation) mounted in SidebarView; `ensure_workspace_ids` mints workspace UUIDs in the stateful session layer (closes the A3-deferred divergence). Collapse toggle -> A5.
- [x] A5 — Group collapse/expand (`session_set_group_collapsed`). `[M, deps: A4, headless]` — done 2026-07-07 (`d2a77e669`, ultracode wave-14): pure-data canonical variant (WorkspaceGroupCoordinator.swift:405-412, NO selection move — toggle-variant selection semantics = separate future op), changed-gated emit, chevron button toggle in WorkspaceList.
- [x] A6 — Inline rename (`session_rename_workspace`; empty clears custom_title). `[M, deps: A4, headless]` — done 2026-07-07 (`f0220ddd5`, wave-15): Workspace.setCustomTitle parity (trim/clear/source "user"), double-click inline editor (Enter commits, Escape/blur cancels, prefill select-all).
- [x] A7 — Pin/unpin + pinned-ahead reorder (`session_set_workspace_pinned`). `[M, deps: A1,A4, headless]` — done 2026-07-07 (`daf9718c6`, wave-16): canonical boundary move + selection-follows + Some(true)/None encoding; row pin toggle.
- [x] A8 — Multi-select + shift-click ranges (port `selection.ts` anchor policy). `[M, deps: A4, headless]` — done 2026-07-08: sidebar tracks view-state multi-selection plus shift anchor, `selectionAfterWorkspaceClick` ports range/toggle/focus fallback policy, collapsed group members are range-hidden, and rows/headers render canonical multi-selected state.
- [x] A9 — New-workspace placement (feed `placement.ts` into `session_new_workspace`). `[S, deps: A1, headless]` — done 2026-07-07 (`b3217bade`), completed 2026-07-08: `session_ops::new_workspace_with_placement` over `cmux_workspaces::insertion_index` (TabManager.swift:1483-1506); 2-arg `new_workspace` wrapper delegates with `NewWorkspacePlacement::default()` so the src-tauri caller is untouched (host passes the effective placement). Group-contiguity normalization now runs after insertion via the snapshot mirror, so a fresh ungrouped workspace inserted into a group run is moved out of the run while selection follows it.
- [x] A10 — Drag-reorder rows + drop-on-group (`session_reorder_workspaces`). `[L, deps: A7, headless]` — done 2026-07-08: WorkspaceList drag/drop plans target raw or top-level row space, Sidebar dispatches `session_reorder_workspaces`, and reorder tests cover grouped-child promotion and anchor/group behavior.
- [x] A11 — Per-row + per-group context menus (shared action dispatch). `[L, deps: A6,A7,A8, headless]` — done 2026-07-08: shared context-menu descriptors/actions power workspace and group menus, including pin, rename, close, close-others/group, collapse/expand, and new-workspace dispatch.
- [x] A12 — Tab/group color tinting (bridge `cmux-workspaces` tab_colors). `[M, deps: A11, headless]` — done 2026-07-08: workspace and group custom colors project into Sidebar/WorkspaceList accent styling with workspace color overriding inherited group tint.
- [x] A13 — Git/PR badges data contract + render. `[L, deps: A4, headless]` — done 2026-07-08, extended 2026-07-08: session workspace branch/PR snapshots project into ordered badge descriptors and Sidebar renders branch/pull-request badges with canonical gating and stale styling; the desktop host now periodically refreshes workspace git facts from live `git` status, clears stale badge facts when a workspace no longer has a repo directory, and fills current-branch PR badges when `gh pr view` can resolve one.
- [x] A14 — Right-sidebar Sessions Index skeleton (separate surface). `[L, deps: none, headless]` — done 2026-07-08: the right sidebar now has Files/Find/Vault mode chrome, Find delegates to the directory-search surface, and Vault renders a tested live sessions/workspaces index from the current session snapshot.

## Area B — Window chrome + tab strip
Today: OS title bar; `App.tsx` header is a toggle + static label. `cmux-window-title`
ported but unwired.

- [x] B1 — `decorations:false` + custom HTML title bar w/ Windows caption buttons (min/max/close). `[M, deps: none, gui-verify]` — done 2026-07-08: main Tauri window disables native decorations and the React `WindowTitlebar` renders custom minimize/maximize/close caption controls backed by native commands.
- [x] B2 — Draggable title band (`-webkit-app-region`) + no-drag opt-outs + dbl-click-maximize. `[S, deps: B1, gui-verify]` — done 2026-07-08: titlebar marks the center band as the Tauri drag region and leading/trailing controls as no-drag islands.
- [x] B3 — Port WindowChromeMetrics constants (28pt band etc.) to a shared metrics module. `[S, deps: none, headless]` — done 2026-07-08: `window/chromeMetrics.ts` exposes shared titlebar and caption-control metrics consumed by `WindowTitlebar`.
- [x] B4 — Wire `cmux-window-title` → `window.set_title` from live state. `[M, deps: none, headless]` — done 2026-07-08: backend `window_title` resolves default/configured templates, refreshes native titles on session changes, and `useWindowChrome` mirrors title state into the web titlebar.
- [x] B5 — Titlebar control cluster (5 slots, canonical a11y ids). `[M, deps: B1, gui-verify]` — done 2026-07-08: custom titlebar exposes sidebar, file explorer, settings, minimize/maximize/restore, and close controls with tested labels/state.
- [x] B6 — Minimal-mode presentation toggle + hover-reveal state machine. `[L, deps: B3,B5, headless]` — done 2026-07-08: `app.minimalMode` adds shell/titlebar minimal classes, lets the body occupy the full window, and reveals the titlebar on hover/focus while preserving native caption controls.
- [x] B7 — Minimal-mode tab-strip inset geometry (Windows caption side). `[M, deps: B6, headless]` — done 2026-07-08: shared chrome metrics now expose minimal-mode top-strip height plus Windows caption-side safe-area CSS variables, with titlebar caption widths derived from the same tested constants.

## Area C — Splits, dividers, 2D canvas
Splits live + well-tested; several pure ops (equalize, resize, directional) have NO
caller. Canvas ENTIRELY absent from web but `cmux-canvas` + data model exist.

- [x] C1 — Directional split insertion (thread `SPLIT_DIRECTION.insertFirst` → `session_split`). `[S, deps: none, headless]` — done 2026-07-07 (`48a685739`), wired 2026-07-08: command/ops layer threads `insert_first` through split_pane→apply_split→session_split, and the palette/agent-fork callers map right/down to append-second and top/left fork targets to insert-first.
- [x] C2 — Equalize dividers action (apply `equalizeDividerPlan`). `[S, deps: none, headless]` — done 2026-07-07 (`48a685739`), wired 2026-07-08: `session_equalize_dividers` command over `session_ops::equalize_dividers` whole-tree walker with orientation-aware `span_count` (parity-exact vs CmuxPanes `ExternalTreeNode.spanCount(along:)` — NOT leaf-count weighting; they diverge on mixed-orientation trees), exposed through the command palette.
- [x] C3 — Keyboard divider resize (`resizeDividerAdjustment` + key handler). `[S, deps: none, headless]` — done 2026-07-08: `keyboardDividerResize` ports the canonical 10px arrow-step math, divider handles expose keyboard focus/a11y values, and Workspace/SplitTree key handlers persist the resized ratio.
- [x] C4 — Directional pane focus (needs a web focused-pane concept). `[M, deps: none, headless]` — done 2026-07-08: `focusedPaneStore` tracks pane focus from Workspace capture handlers, `adjacentPanelId` resolves split-pane geometry, and command-palette focus intents dispatch to the adjacent panel.
- [x] C5 — `layout_mode` plumbing + `session_set_layout_mode` (seed canvas from splits). `[M, deps: none, headless]` — done 2026-07-08: active-workspace Tauri command, `useSession` binding, command-palette toggle, lazy split→canvas seeding, and regression tests across core/desktop/web.
- [x] C6 — Port `cmux-canvas` geometry/layout/placer to TS modules. `[L, deps: C5, headless]` — done 2026-07-08: added web `canvasGeometry`, `canvasLayout`, and `canvasPlacer` modules with parity tests for rect math, placement fallthrough/preferred directions, z-order, frame batches, content bounds, and pane/tab hosting.
- [x] C7 — `CanvasSurface` component (floating panes, never-remount invariant). `[L, deps: C6, gui-verify]` — done 2026-07-08 as inline `Workspace` canvas mode: persisted pixel frames, floating pane chrome, move/resize handles, canvas HUD, and no remount of existing pane surfaces.
- [x] C8 — Snap engine + guide lines. `[M, deps: C7, headless]` — done 2026-07-08: web now ports the shared snap rules for move/resize gestures (edge, gap, center, threshold, min-size clamp), persists snapped frames, and renders live vertical/horizontal guide overlays during canvas drags.
- [x] C9 — Aligner commands + one CanvasAction dispatcher. `[M, deps: C6, headless]` — done 2026-07-08: `session_apply_canvas_action` dispatches tidy/align/equalize/distribute over persisted pane frames; palette canvas rows share the same action path. Selection narrowing remains a future enhancement once canvas multi-select exists.
- [x] C10 — Canvas viewport (pan/zoom/overview/reveal). `[L, deps: C7, headless]` — done 2026-07-08: canvas mode uses persisted coordinates with wheel pan, ctrl/cmd-wheel zoom, HUD overview/100%/zoom controls, and reveal-focused-pane.
- [x] C11 — Canvas spatial-nav focus. `[S, deps: C6, headless]` — done 2026-07-08: directional focus commands use persisted canvas pane geometry when `layout_mode === "canvas"` and split geometry otherwise.
- [x] C12 — Canvas session persistence round-trip. `[M, deps: C5, headless]` — done 2026-07-08: drag/resize/action mutations persist through `canvas_panes` in the active session snapshot with core and desktop command tests.
- [x] C13 — CanvasConfig settings (paneGap, snappingEnabled). `[S, deps: C8, headless]` — done 2026-07-08: `canvas.snappingEnabled` now disables live snap/guide behavior, `canvas.paneGap` drives web snap adjacency plus toolbar/palette canvas action spacing, and the desktop command/core action path accepts an optional configured gap.

## Area D — Command palette + fuzzy switcher  *(all logic ported, zero live)*
- [x] D1 — Port `window_store` visibility/selection/escape state machine to TS. `[M, deps: none, headless]` — done 2026-07-06; `palette/windowStore.ts` class + 18 tests (faithful port). NOTE for D4: `paletteSelection` reducer (clamps `[0,count-1]`) is the selection source of truth, not the store's looser `>=0` clamp.
- [x] D2 — Tauri search bridge (`orchestrator.*_search_matches`; add cmux-command-palette+cmux-mentions deps). `[M, deps: none, headless]` — done 2026-07-06; `src-tauri/src/command_palette.rs` `command_palette_search` command over `preview_search_matches` (scoring stays in the orchestrator); 6 Rust tests. Web-side `host.invoke("command_palette_search", …)` wrapper lands with D4.
- [x] D3 — Query input + scope hook (`listScope` + `paletteSelection`). `[S, deps: none, headless]` — done 2026-07-08: the pure `palette/paletteQuery.ts` model is wired into `useCommandPalette`, deriving scope/matching query/selection for the mounted overlay, re-anchoring on query changes, clamping on async result changes, and resetting shown results on visible scope flips.
- [x] D4 — Live overlay host: mount, open-shortcut, focus, Escape, arrow/click/Enter. `[M, deps: D3,D2, gui-verify]` — done 2026-07-08: `CommandPaletteOverlay` is mounted in `App`, opens via global Ctrl/Cmd+K and Ctrl/Cmd+Shift+P, owns focus/Escape/arrow/Enter handling, and wires hover/click activation through the shared palette row renderer. The async result path already preserves shown results while pending via the D9 gating layer. Right-sidebar mode rows now inject at the canonical splice point and open Files/Find/Vault through the shared host action.
- [x] D5 — Command catalog + activation dispatch (ONE shared action path). `[M, deps: none, headless]` — done 2026-07-07 (`2bf07ee48`); `palette/commandCatalog.ts` ports all 117 canonical contributions (ContentView.swift:6321-7464) in declared order + a single id→intent registry/`dispatchCommand`. Config override structurally gated to the 4 canonical configurable ids. Runtime sub-lists (extension sidebar, canvas, settings toggles, color palette, terminal targets, cmux.json actions) are injectable at exact Swift `contentsOf:` positions — D4 supplies them from live host state for absolute-rank parity.
- [x] D6 — Live switcher-entry producer (`switcherIndex` from workspace state). `[M, deps: none, headless]` — done 2026-07-07 (`2bf07ee48`), extended 2026-07-08: `palette/switcherEntries.ts` (single-window path, ContentView.swift:5249-5358) indexes workspace titles, directories, descriptions, surface facts, git branch fields, and listening-port facts reported through the session/control-socket path; local terminal panels and live agent-owned process trees both kick the Windows PID-tree listener scanner and feed the same workspace port badges/search facts.
- [x] D7 — Render-sequencing guard (monotonic seq + resultsVersion). `[S, deps: D2, headless]` — done 2026-07-07 (`bd3fe5f2b`); pure `palette/renderSequencing.ts` `RenderSequencingGuard` (CommandPaletteOverlay.swift:46-56), two independent clocks, drops stale async batches. Verify SOLID.
- [x] D8 — Scroll-follow + hover selection. `[S, deps: D4, headless]` — done 2026-07-08: command palette rows update selection on hover and the active row scrolls into view with nearest alignment, with headless coverage for row callbacks and the scroll-follow helper.
- [x] D9 — Sync-seed + preserve-empty-while-pending gating. `[S, deps: D2, headless]` — done 2026-07-07 (`bd3fe5f2b`); pure `palette/resultsGating.ts` (seed `<=256`, 5-input preserve AND, 3-branch show-empty; ContentView.swift:8407-8419 + orchestrator.rs:296-321). Params keyed by exact Swift arg names; "results shown" is an explicit host input. Verify SOLID.
- [x] D10 — Settings-toggle palette surface. `[L, deps: D5, headless]` — done 2026-07-08: command palette injects live settings-toggle rows from the current config, including app/automation/browser/terminal/canvas/file/sidebar/notification booleans, gates unavailable rows, and dispatches through the shared Settings config action path.

## Area E — Settings / config UI  *(17/17 pane anchors live)*
- [x] E1 — `config_load` Tauri cmd (add cmux-config dep; preserve unmodeled sections via `Config::extra`). `[M, deps: none, headless]` — done 2026-07-08: `config_load` decodes cmux.json through `cmux-config`, materializes Settings-visible defaults, and caches the raw JSON tree so unknown sections survive later saves.
- [x] E2 — `config_save` via dotted-JSONPath set/remove onto raw tree (NOT typed re-serialize). `[M, deps: E1, headless]` — done 2026-07-08: `config_save` applies one dotted `JsonPath` mutation to the cached raw tree, validates via the typed decoder, and writes pretty JSON while preserving advanced/unmodeled sections.
- [x] E3 — Config-delta representation (ConfigAction → dotted path). `[M, deps: E2, headless]` — done 2026-07-08: `configMutationFromAction` maps Settings actions to default-aware dotted set/remove mutations across app, sidebar, notifications, automation, browser, terminal, markdown, canvas, file, diff, shortcut, and workspace-color settings.
- [x] E4 — Wire `SettingsPane` into the app (load on mount, persist onChange). `[M, deps: E2,E3, gui-verify]` — done 2026-07-08: `App` loads settings on mount, mounts `SettingsOverlay`, routes SettingsPane actions through `configReducer`, persists mutations through `config_save`, and refreshes applied appearance/status surfaces.
- [x] E5 — Live cmux.json reload (notify watcher → config-changed event). `[M, deps: E1, gui-verify]` — done 2026-07-08: desktop starts a config-file watcher loop that reloads changed cmux.json into the raw cache and emits `cmux://config-changed`; the web app subscribes, refreshes Settings state/config-extension status, and reapplies appearance.
- [x] E6 — SettingsPane shortcuts list uses `shortcutFormat.ts`. `[S, deps: none, headless]` — done 2026-07-08: shortcut rows render display strings through `shortcutBindingDisplayString`/`shortcutFormat.ts`, with tests for valid, invalid, chord, and clear-state bindings.
- [x] E7 — Editable shortcut capture (key recorder → `setShortcutBinding`). `[L, deps: E3, headless]` — done 2026-07-08: shortcut inputs still allow raw text edits, but now capture modified keystrokes, F-keys, arrows, enter/tab/space, and deletion-key clears into canonical config binding strings routed through `setShortcutBinding`.
- [x] E8 — Settings search box wired to `cmux-settings-search`. `[M, deps: none, headless]` — done 2026-07-08: `SettingsOverlay` owns the search query and `SettingsPane` consumes `settingsEntriesMatching`/section projection to filter and navigate the rendered settings sections.
- [x] E9 — Apply appearance (`appearanceMode.ts` → document theme + persist). `[S, deps: none, headless]` — done 2026-07-08: config load/save and palette/Settings actions flow through `useAppearance`, applying the resolved appearance to the document theme and persisting non-default choices.
- [x] E10 — Raw settings.json editor pane. `[L, deps: E2, gui-verify]` — done 2026-07-08: Settings cmux.json can load/edit/save the raw file in-app through validated native read/write commands, refreshing typed settings state after successful saves.
- [x] E11 — Remaining canonical panes (terminal/browser/automation/…). `[L, deps: E3,E4, headless]` — done 2026-07-08: SettingsPane now renders the remaining canonical surfaces, including automation, browser, terminal/TextBox, workspace colors, platform status/action panes, app integrations, shortcuts, cmux.json, and reset, with search navigation targeting each pane anchor.
- [x] E12 — Browser import selection wizard + start command. `[M, deps: E4, headless]` — done 2026-07-08: the Import Browser Data pane now exposes canonical source-profile selection, cookies/history/additional-data scope controls, separate-vs-merge planning, destination selectors, and a `browser_import_start` Tauri command that validates/captures the selected import plan. Remaining follow-up: actual browser data migration into a Windows browser-profile store once that destination store is ported.
- [x] E13 — Blank browser import hint. `[S, deps: E12, headless]` — done 2026-07-08: blank browser panes now render the canonical import hint affordances (`BrowserImportHintImportButton`, `BrowserImportHintSettingsButton`, `BrowserImportHintDismissButton`) when `browser.showImportHintOnBlankTabs` is enabled; Import/Settings open the browser-import settings section, and Dismiss persists the existing config flag off through the shared config-save path.
- [x] E14 — Browser import UI-test fixture/capture hooks. `[S, deps: E12, headless]` — done 2026-07-08: `browser_import_profiles` now honors `CMUX_UI_TEST_BROWSER_IMPORT_FIXTURE` (`browserName` + `profiles`) for deterministic detected-profile tests, and `browser_import_start` writes the canonical capture JSON shape (`sourceProfiles`, `destinationKind`, `destinationName`) to `CMUX_UI_TEST_BROWSER_IMPORT_CAPTURE_PATH`.
- [x] E15 — Browser import destination profile parity. `[S, deps: E12, headless]` — done 2026-07-08: desktop now exposes `browser_import_destination_profiles`, honors `CMUX_UI_TEST_BROWSER_IMPORT_DESTINATIONS`, and the Settings import controls load backend destination profiles for merge/single-destination imports while separate-profile imports still default to create-new profiles with optional existing-profile choices.
- [x] E16 — Settings-open UI-test capture hook. `[S, deps: E13, headless]` — done 2026-07-08: desktop now exposes `settings_open_capture`, writes `{opened,target,used_open_window_override}` to `CMUX_UI_TEST_SETTINGS_OPEN_CAPTURE_PATH`, and the shared Settings opener invokes it for browser-import hint Settings clicks and other targeted Settings opens.
- [x] E17 — Browser import Next-driven wizard flow. `[S, deps: E12, headless]` — done 2026-07-08: the Import Browser Data pane now starts as a canonical `Next`-driven flow, advancing from detected browser introduction to source-profile selection and then scope/mode/destination/start controls, so blank-tab Import clicks satisfy the expected two-step source/options sequence while preserving the existing Settings entry point and backend capture path.

## Area F — Agent session / chat  *(chat UI reused verbatim; port is the host)*
Claude end-to-end live; Codex/OpenCode route through the store. Missing host emissions.
- [x] F1 — Codex end-to-end live verify + enable (handshake drain, approvals echo). `[M, deps: none, headless]` — done 2026-07-08: verified the Codex actor/store path drains `initialize` → `initialized` → `thread/start`, queues and drains pre-thread prompts, writes `turn/start` once a thread exists, and echoes approval/unsupported server-request replies with raw JSON-RPC ids; focused cmux-agent-chat and desktop agent-session tests pass.
- [x] F2 — Emit `app.rateLimitRows` (parse Codex usage → RateLimitFooter). `[L, deps: F1, headless]` — done 2026-07-08: Codex startup now requests `account/rateLimits/read`, sparse `account/rateLimits/updated` notifications trigger a full snapshot refetch, camel/snake payloads parse into primary/secondary `AgentSessionRateLimitRow`s, and the existing footer reducer consumes the emitted `app.rateLimitRows` event.
- [x] F3 — Dynamic `app.theme` push (light/dark from shell). `[M, deps: none, headless]` — done 2026-07-08: the desktop shell maps the active appearance color scheme to tested `AgentSessionTheme` tokens and pushes `app.theme` through the reused `cmuxAgentBridge`, with the shared session model updating context theme live.
- [x] F4 — Attention/flash on turnComplete/exit while unfocused. `[M, deps: none, headless]` — done 2026-07-08: Workspace listens to agent provider completion/exit events, resolves the owning panel through restorable-agent session ids, and flashes/marks it unread unless that panel is already focused in a focused document.
- [x] F5 — `provider.select` persistence + seed `initialProviderId`. `[S, deps: none, headless]` — done 2026-07-08: agent-session RPC persists successful `provider.select`/`provider.start` provider ids to the app settings store, restores valid selections on actor startup, and seeds `app.context.initialProviderId` for the reused chat UI.
- [x] F6 — Session restore / transcript replay (no double auto-start). `[L, deps: F1, headless]` — done 2026-07-08: `app.transcript` now resolves pane-scoped restorable Codex/Claude snapshots, replays up to 200 saved transcript entries when present, returns an empty suppressing restore for missing transcript files, hydrates the reused chat UI without overwriting live turns, and marks the restored provider as auto-start-attempted to prevent duplicate launches.
- [x] F7 — JA localization of the 67-key copy dict. `[M, deps: none, headless]` — done 2026-07-08: agent-session app.context now resolves the configured app language (including system Japanese hints) and emits a full 67-key Japanese copy table with preserved renderer format specifiers, while English remains the fallback/default.

## Area G — Diff / Markdown / Browser
- [x] G1 — Markdown doc feed: `markdown_set_document` sets `PanelCtx.file_path` THEN pushes render. `[M, deps: none, headless]` — done 2026-07-07 (`adf9171f9`); the missing WRITER for `PanelCtx.file_path` (the `cmux-local-image://` jail already read it → every image 403'd). Ports Swift `Coordinator.bind` filePath assign; set-before-render doc-contract; `webview.eval` render push is the deferred GUI tail. Verify SOLID (real-bug fix). cmux-desktop 83.
- [x] G2 — Remote-image host layer (DNS-pin SSRF gate + TLS fetch). `[M, deps: none, headless]` — done 2026-07-08: the `cmux-remote-image` Tauri protocol validates admitted HTTPS image URLs, rejects any DNS result with blocked/private addresses, fetches over TLS with timeouts, and streams through the size-limited markdown remote-image accumulator.
- [x] G3 — Markdown typography controls. `[M, deps: none, headless]` — done 2026-07-08: Settings-backed markdown font size, font family, and max-width now flow into live markdown iframes through `markdown_apply_typography`, using the shared clamp/escape typography model; the shell applies CSS variables for body size, family, and reading width, and reset zoom returns to the configured font size.
- [x] G4 — Markdown link-open (`openMarkdownFile` → new surface in owning pane). `[M, deps: G1, gui-verify]` — done 2026-07-08: markdown shell clicks send `openMarkdownFile`, the native cmuxLib bridge resolves markdown-like links relative to the current document, and the owning pane is switched to a markdown surface with the resolved file path.
- [x] G5 — Diff comments WebView2 shim (`cmuxDiffComments.postMessage` → invoke). `[S, deps: none, headless]` — done 2026-07-08: the diff surface installs the parent/iframe `cmuxDiffComments` relay, forwards token and panel scope to `diff_comments_rpc`, and the native command trust-gates active diff-viewer sessions before delegating to `cmux-diff`.
- [x] G6 — Port `__diff-viewer-refs/-branch` CLI (git + token + manifest jail). `[L, deps: none, headless]` — done 2026-07-08: the Rust CLI now executes the hidden diff-viewer refs/branch helpers locally, validates tokens through `cmux-diff`, discovers git refs for the branch picker contract, regenerates branch patch/html files under the trusted diff-viewer root, and upserts manifest entries that the registry can restore.
- [x] G7 — Build+serve the `webviews/` diff app as `cmux-diff-viewer` assets. `[L, deps: G6, gui-verify]` — done 2026-07-08: the `cmux-diff-viewer` protocol now serves session-registered files first and safely falls back to bundled `markdown-viewer/webviews-app` assets (`main.mjs`, chunks, CSS/wasm) through a jailed resolver; starter sessions now boot the real diff app shell with a status payload.
- [x] G8 — Live diff token wiring (replace `token={null}`). `[S, deps: G6, headless]` — done 2026-07-08: Workspace resolves pane-local `diff_viewer_token` / request path snapshots into `DiffSurface`, registered sessions render the trusted `cmux-diff-viewer://<token>/<path>` iframe, and missing-token panes remain guarded with the starter-session recovery action.
- [x] G9 — submission_pool workspace-id glue. `[M, deps: G5, headless]` — done 2026-07-08: diff comment save/delete side effects resolve the relay panel id back to its workspace and keep the pending submission pool scoped by workspace, with native coverage for save/delete updates.
- [x] G10 — Wire `cmux-browser-history` (sanitizer + nav availability). `[M, deps: none, headless]`
- [x] G11 — Browser surface skeleton (WebView2 child, omnibar). `[L, deps: G10, gui-verify]` — done 2026-07-08: desktop registers pane-scoped Tauri child WebViews behind the React browser slot, repositions/zooms/shows/hides them from measured pane bounds, emits child navigation back into session browser history, and keeps iframe rendering only as SSR/plain-browser fallback; live Windows smoke launched cmux against Vite, opened `https://example.com`, verified the omnibar/sidebar remained visible while the native WebView2 child rendered the page, and observed history state update from the child navigation event.

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
