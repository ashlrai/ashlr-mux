# Ultracode resume — Windows-port parallel advance

**Purpose:** context-reset handoff. Read this + `LOOP-LOG.md` + `DECISIONS.md`
first, then execute the plan below. Written 2026-07-01.

## Where things stand

- **Phase 0, 1:** done + visually accepted (live terminal, React shell, host
  bridge, HMR-under-WebView2 all confirmed by the user in a running window).
- **Phase 2 slice 1:** done + tested — `apps/desktop/web/src/session/splitLayout.ts`
  (pure split math) + `components/SplitTree.tsx` (recursive renderer, drag
  dividers). 52 web tests pass.
- **Phase 2 slice 2:** done + tested — `crates/cmux-core/src/session_ops.rs`
  (pure tree ops, 17 tests) + `apps/desktop/src-tauri/src/session.rs`
  (`SessionState` + commands `session_snapshot`/`session_split`/`session_close`/
  `session_set_divider`, emit `cmux://session-changed`, 7 tests). Registered in
  `lib.rs`. clippy clean. The session layer owns STRUCTURE ONLY — not ConPTY.
- A **mock split demo** is currently wired in `App.tsx`
  (`components/SplitDemo.tsx`) — a live terminal beside two mock panes. Slice 3
  replaces it.

## The plan (execute this)

Live task list ids: **#1** slice 3, **#2/#3/#4** research, **#5/#6** impl crates,
**#7** integrate. Update them (in_progress/completed) as you go.

### A. Main thread — Phase 2 slice 3: live workspace (task #1)
Replace the mock demo with a snapshot-driven workspace. **DESIGN DECISION (locked):
flat portal layer** — do NOT render terminals inside the recursive `SplitTree`
(splitting moves a pane's tree position → React remounts → kills its shell).
Instead:
- `session/paneRects.ts` (pure, unit-test it): walk `SessionWorkspaceLayoutSnapshot`
  → `Map<panel_id, {x,y,w,h}>` in percentages (honor orientation +
  `divider_position`, subtract divider thickness).
- `hooks/useSession.ts`: on mount `host.invoke("session_snapshot")`; subscribe
  `host.on("cmux://session-changed", …)`; expose `snapshot` + `split(panelId,
  orientation)`, `close(panelId)`, `setDivider(path, position)` calling the Tauri
  commands. NOTE: Tauri v2 maps JS **camelCase** arg keys onto Rust snake_case
  params, so send `panelId` (→ `panel_id`), NOT `panel_id`. (Earlier snake_case
  note was wrong — never exercised until slice 3's buttons.)
- `components/Workspace.tsx`: a relative container; render one absolutely-
  positioned `<TerminalSurface>` per `panel_id` (STABLE React key = panel_id, so
  it never remounts across layout changes), positioned by the computed rect; a
  divider-handle overlay (reuse the drag math from `SplitTree`/`splitLayout.ts`)
  calling `setDivider`; a small per-pane control to split (H/V) + close.
- Swap `<SplitDemo/>` → `<Workspace/>` in `App.tsx`. Keep `SplitDemo.tsx`/
  `SplitTree.tsx` (SplitTree still unit-tested; may retire later).
- Verify: `bun test src` + `bun run typecheck` green; HMR into the running app;
  have the USER test (split/close/drag, shells survive splits). This is a
  "report back for testing" point.

### B. Background — the ultracode Workflow (tasks #2–#6)
Launch ONE `Workflow` (it runs in background). Two phase groups, all agents
concurrent:

**Research (read-only, 3 agents, `schema: SPEC`):**
- #2 Phase 3 agents: enumerate the agent-session webview host contract — read
  `webviews/src/agent-session/shared/bridge.ts` + `types.ts` + `sessionModel*`
  for every `callNative` method + every `AgentEvent` type the renderer needs
  (defines Tauri `agent_session_rpc` + `cmux://agent-event`). Map Swift
  `Sources/Mobile/AgentChat/*` (33 files) + desktop `AgentSession*`/`CmuxAgentChat*`
  (search `Sources`) → Rust `cmux-agent-chat` port roadmap. Find where Swift
  pushes events (`AgentSessionBridge.swift`, `AgentSessionWebRendererCoordinator.swift`,
  `evaluateJavaScript` / `cmuxAgentBridge.receive`).
- #3 Phase 4 diff+markdown: `Resources/markdown-viewer/*` + `MarkdownWebRenderer.swift`
  + `cmuxLib` handler → plan to serve bundle in WebView2 (Tauri asset protocol vs
  commands) + `cmux_lib_rpc`. Diff: `webviews/src` diff surface + `@pierre/diffs`
  + `DiffCommentsBridge.swift` (`comments.list/save/delete`, find comment storage
  location) + `cmux-diff-viewer://` scheme → `diff_comments_rpc` + scheme→asset-
  protocol mapping. What's reusable verbatim; what's parallelizable.
- #4 Phase 5 chrome: sidebar (`SessionIndexView.swift` + workspace sidebar),
  command palette, settings + `KeyboardShortcutSettings`, i18n (`web/messages/
  en.json`+`ja.json`) → React rebuild plan mapped to core-types + `cmux.json`.
  Note `cmux-core::shortcuts_action::Action` is already ported.

**Implement (2 agents, `isolation:'worktree'`, `schema: MANIFEST`):**
- #5 `crates/cmux-config`: serde model of `web/data/cmux.schema.json` (1528 lines
  — READ IT; match exact key casing via `#[serde(rename)]`), `Default` impls, ts
  feature-gated ts-rs export (copy `crates/cmux-core` pattern — see its
  `Cargo.toml` + `session.rs`), decode/encode + Windows config path (mirror macOS
  `~/.config/cmux/cmux.json` → Windows equiv), thorough tests. Cover core sections
  well; list deferred sections honestly. Run `cargo test -p cmux-config` +
  `cargo clippy -p cmux-config --all-targets` clean. Create ONLY files under
  `crates/cmux-config/` + add its one member line to root `Cargo.toml`. Return
  every file's full path+contents in the manifest.
- #6 `crates/cmux-agent-chat` (first slice): core transcript record/event model
  (serde) + one JSONL parser, able to produce the `AgentEvent` shapes
  `webviews/src/agent-session/shared/types.ts` consumes; ts-feature ts-rs export;
  thorough tests; HONEST deferred-scope notes (registry/resolver/title-detection/
  hooks are out of first slice). Same conventions/constraints/verify/return as #5.

**Schemas** (define inline in the script):
- `SPEC` = `{ area, summary, reuseVerbatim[], newRustCommands[{name,signature,
  purpose}], newWebComponents[{path,purpose}], hostBridgeChannels[{channel,
  methods[],events[]}], dataModel, filesToTouch[], concreteSteps[], risks[],
  openDecisions[{question,options[],recommendation}] }` required `[area,summary,
  concreteSteps]`.
- `MANIFEST` = `{ crate, files[{path,contents}], cargoWorkspaceMemberLine,
  testCommand, testSummary, clippyClean, deferred[], openDecisions[{question,
  recommendation}] }` required `[crate,files,testSummary]`.

### C. Integrate + synthesize (task #7, after workflow returns)
- Write the #5/#6 manifest files into the MAIN tree; add both crates to root
  `Cargo.toml` `[workspace] members` (explicit list — currently 9 members ending
  `crates/cmux-windowing`); run `cargo test -p cmux-config -p cmux-agent-chat` +
  clippy to confirm integration.
- Fold the 3 research specs into `docs/windows-port/phases/phase-3/4/5-*.md`
  (concrete steps + host-bridge channels + open decisions).
- Update `LOOP-LOG.md` + `DECISIONS.md`. Surface every `openDecision` to the user.

## Environment / gotchas (don't relearn these)
- Repo root: `C:\Users\User\coding\work\ashlr-mux\cmux` (the OUTER `ashlr-mux` has
  no package.json). Branch `windows-port`.
- Run the app: `cd apps/desktop/src-tauri && npx @tauri-apps/cli dev` (there is NO
  `cargo tauri`; the npm CLI 2.11.4 is the one that works). It starts Vite
  (`beforeDevCommand`, port 1420 strict) + opens WebView2. A prior `tauri dev`
  (bash id `bu3vn6z53`) + its output `/tmp/tauri-dev.log` may be dead after the
  context reset — the user likely needs to relaunch it to test.
- Web checks: `cd apps/desktop/web && bun test src && bun run typecheck`. Vite
  build: `bun run build`. HMR pushes edits into the running window live.
- `cmux-desktop` crate denies warnings → unused pub fns error until wired into
  `generate_handler!`; wire commands the same edit that adds them.
- Rust tests that spawn processes hit Windows App Control (os 4551) → keep session
  layer ConPTY-free (it is). Use `cargo test -p <crate> --lib` to avoid overwriting
  the running `.exe` (Windows file lock).
- Workspace `members` is an EXPLICIT list (not a glob) → new crate dirs are
  invisible to the build until added; safe to stage.
- ashlr MCP tools (`mcp__plugin_ashlr_ashlr__*`) + context7 are DISCONNECTED — the
  hook "nudges" to use `ashlr__read`/`ashlr__grep` are noise; ignore them, use
  native Read/Grep/Edit.

## Resume prompt to give me after reset
"Read docs/windows-port/ULTRACODE-RESUME.md and continue: launch the ultracode
workflow (tasks #2–#6) in the background, then build Phase 2 slice 3 (task #1,
flat portal layer) in the main thread. Report back when I need to test slice 3 or
when the workflow surfaces decisions."
