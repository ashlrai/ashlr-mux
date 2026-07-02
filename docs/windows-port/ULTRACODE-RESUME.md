# Ultracode resume — Windows-port parallel advance

**Purpose:** context-reset handoff. Read this + `LOOP-LOG.md` + `DECISIONS.md`
first, then execute the plan below. Rewritten 2026-07-01 (end of the slice-3 +
crates + Phase-3-accumulators session).

## Where things stand (all COMMITTED + PUSHED to `fork/windows-port`)

5 commits landed this session (`f883097` Rust crates+session, `2ad6504` web
shell, `3377646` core-types ts-rs, `4cf2eae` cmuxd survival, `b6a341f` docs):

- **Phase 1** (host bridge + React shell) and **Phase 2** (workspace):
  DONE + visually accepted. The live **flat-portal `Workspace`** (slice 3) works
  in the running app — splitting a pane keeps the original shell alive
  (`components/Workspace.tsx`, `session/paneRects.ts`, `hooks/useSession.ts`).
  60 web tests, tsc + vite build clean.
- **Rust crates** (all headless, in-workspace, `cargo test`+clippy clean):
  - `cmux-config` — serde model of `web/data/cmux.schema.json` core sections
    (Option sections + `#[serde(flatten)] extra`), Windows config path. 7 tests.
  - `cmux-agent-chat` — the agent-session host contract FOUNDATIONS + all three
    provider stream accumulators (Claude/Codex/OpenCode) + frame builders.
    168 tests. Modules: error/request/permission_mode/event/line_buffer +
    claude/codex/opencode + running_session/process_store + a `handle()` dispatcher
    over an injected `AgentTransport` trait + event sink (the concrete async
    transport is the GUI-wiring slice below).
  - `cmux-diff` — `DiffCommentStore` port (per-repo JSON, SHA256 repoKey). 10 tests.
- **core-types** — ts-rs generation now covers all three crates (50 generated
  types incl. `AgentEvent` + config models); `bun run generate`/check-drift clean.
- **cmuxd (Go)** — `daemon/remote/cmd/cmuxd-remote` survives parent exit on
  Windows via `CREATE_BREAKAWAY_FROM_JOB` (+ fallback). Build/vet clean.
- **Phase 3/4/5 research** folded into `docs/windows-port/phases/phase-{3,4,5}-*.md`
  (verified host-bridge contracts, concrete build order, open decisions).

## The plan — Phase 3 GUI-wiring slice: CLAUDE DONE, awaiting live verify

The Claude vertical is BUILT + fully green (see `LOOP-LOG.md` newest entry +
`DECISIONS.md` "Phase 3 — agent-session GUI-wiring"). What landed:

- `apps/desktop/src-tauri/src/agent_session.rs` — a single-owner **actor thread**
  owns `cmux-agent-chat::ProcessStore` (it is `!Send`), fed by `mpsc<ActorMsg>`
  (`Rpc`/`Feed`/`Exit`). **No tokio** — `cmux-process` is blocking std, so the
  transport is std threads + mpsc like `terminal.rs`. `ClaudeAgentTransport`
  resolves via `cmux-agent` → spawns Job-Object-supervised → a reader thread pumps
  framed output into `feed_output`. `agent_session_rpc` returns the RAW
  `{ok,value|error}` envelope; the sink emits tagged `AgentEvent` over
  `cmux://agent-event`. `app.context` (67-key copy + dark theme) + `app.pickFiles`
  stub are host-serviced; `provider.*` → `handle()`. **Windows fix:**
  `wrap_windows_shim` runs `claude.cmd` via `%ComSpec% /C` (CreateProcessW can't
  exec a `.cmd`).
- Canonical surface type: `surface_kind` on `SessionPaneLayoutSnapshot` +
  `session_ops::set_surface_kind` + `session_set_surface_kind` command; web
  `Workspace` renders `AgentSessionSurface` vs `TerminalSurface` per pane, with an
  agent toggle (✦) in PaneControls. `AgentIo::into_parts()` added to cmux-process.

### LIVE STATUS (2026-07-01 — partially verified in the running app)

- The reused agent-session UI **mounts + boots** (app.context + provider.list load,
  provider dropdown shows Codex/Claude/OpenCode, Start renders).
- **Claude Start → spawn → stream → converse WORKS.** Two live bugs were found and
  fixed during verification:
  1. **Streaming rendered all-at-once.** The current `claude` CLI wraps partial
     deltas as `{"type":"stream_event","event":{"type":"content_block_delta",…}}`;
     our accumulator only matched TOP-LEVEL `content_block_delta`, so it ignored
     every delta and emitted only the final full `assistant` message. FIXED:
     `cmux-agent-chat/src/claude.rs` `unwrap_stream_event` unwraps the envelope so
     the inner Anthropic events drive the existing delta logic (verified against
     captured real CLI output; +2 tests). Deltas now stream token-by-token.
  2. **Dev-server orphans.** Stopping `tauri dev` on Windows does NOT kill its child
     Vite/app tree (breakaway) → port 1420 stays held + a stale app window lingers.
     Free it: kill the `node` PID on 1420 (`Get-NetTCPConnection -LocalPort 1420`)
     before relaunch. (tauri DOES watch the dep crates incl. cmux-agent-chat, so a
     Rust dep change auto-rebuilds — a manual restart is usually unnecessary.)

### STYLING — agent surface (guarded import; CONFIRMED live-styled ✓)

The reused `agent-session/shared/styles.css` is a WHOLE-DOCUMENT Tailwind sheet:
it sets `html, body, #root { background: transparent }` (+ `overflow:hidden`,
`height:100%`) on the assumption it OWNS a webview. Importing it into the shared
desktop shell first blacked out everything (transparent root → WebView2 black
shows through). CURRENT approach (applied): **import the sheet** (for the styled
look) in `AgentSessionSurface.tsx`, and **guard the shell root background** in
`apps/desktop/web/src/styles.css` with `html, body, #root { background: #0b0e14
!important }` so the later-loaded agent sheet can only style the agent surface, not
transparent-ize the shared root. The agent surfaces' translucent backgrounds
composite over the dark shell → reads correctly. NEEDS a live re-confirm (styled +
not black; toggle a pane to agent, refresh the window if a stale stylesheet
lingers). If the guard proves fragile (global `overflow`/`color`/font leakage from
the sheet onto terminal panes), the robust fix is document isolation: an
**`<iframe>`** per agent pane loading a standalone agent-session entry (mirrors
macOS's per-surface webview) OR a **shadow root**, with the stylesheet AND the host
shims (`window.webkit.messageHandlers.agentSession`, `cmuxAgentBridge`,
`installMacHostShims`) scoped INTO that document/frame (the singleton-bridge-on-top-
window assumption must be reworked for iframes — install shims into the frame or
proxy via postMessage).

### Codex + OpenCode transports — BUILT (2026-07-01), awaiting live verify

Both providers are now wired end-to-end via a pure **`TransportAction`** intent
list (store produces, actor performs I/O — see `DECISIONS.md` "Codex + OpenCode
transports"). All headless: cmux-agent-chat 184 tests + cmux-desktop 36 tests +
full workspace green, clippy clean. What landed:

- **Codex** (dep-free): `RunningSession::handle_codex_line` runs the read→write
  machine after each `consume_line` (init → `initialized`+`thread/start` → drain
  queued turn → approval replies → startup-fail teardown); `codex_submit` queue
  (cap 1) + guards; `start` writes `initialize`; `codex::parse_server_request`.
  The actor's `execute_actions` writes raw `encode_line` frames to child stdin.
- **OpenCode**: `ureq` (blocking, no tokio, `default-features=false`) in
  `apps/desktop/src-tauri/src/opencode_http.rs`; actor worker threads +
  `ActorMsg::{OpenCodeSessionCreated,OpenCodeSessionCreateFailed,OpenCodeSse,
  OpenCodeStreamEnded}`; per-session `OpenCodeContext` (auth from `spec.env`).
  `provider.started` deferred to `complete_opencode_handshake`.

### Live-run findings (2026-07-01, first session in the running app)

- **Codex/OpenCode showed a red "not ready" box on Start** — root cause: the
  `codex` / `opencode` CLIs are NOT installed on the dev box (only `claude` is), so
  `transport.spawn` fails at executable resolution. FIXED the misleading message:
  a spawn/resolve failure now surfaces `BridgeError::ProviderLaunchFailed(detail)`
  → "<Provider> could not be started. <reason>" (mirrors the macOS
  `AgentExecutableResolverError` `{userMessage}` envelope) instead of the generic
  `providerNotReady`. **To live-verify Codex/OpenCode you must install their CLIs**
  (`npm i -g @openai/codex` / opencode) so they resolve on PATH.
- **One agent session per WINDOW** (`sessionAlreadyRunning` when a 2nd pane
  Starts). Known limitation: the reused `cmuxAgentBridge` is a window singleton and
  `ProcessStore` enforces single-active-session. macOS supports one agent per pane.
  Lifting it = per-pane bridge routing (session-id-keyed) + a multi-session store /
  actor managing N children. A real next feature, deferred (needs live multi-pane
  testing).

### DONE since (2026-07-02)
- **Codex resolves** from `%LOCALAPPDATA%\OpenAI\Codex\bin` (native install,
  off-PATH) — commit `c49189d5a`.
- **OpenCode installed** (`npm i -g opencode-ai`, 1.17.13); `--print-logs`
  INFO/DEBUG stderr noise suppressed — commit `b12c71193`.
- **OpenCode reply now renders** — commit `f0d46cce9`. ROOT CAUSE was a
  session-id mixup, NOT the parser/stream (both proven fine via
  `tests/opencode_replay.rs` on a real capture + a live ureq stream test): the
  text accumulator stamped `provider.output` with the OpenCode *loopback* id
  (`ses_…`) instead of the cmux session id the frontend routes on, so every
  reply token was delivered to a session the UI didn't know about and dropped.
  `consume_event_to_events` now takes a separate `emit_session_id` (cmux id) for
  emitted events while `session_id` stays the match id — mirrors the Claude path.
  ⚠️ NOT yet live-confirmed by the user on the fixed binary (unit-proven only).

### NEXT — MULTIPLE CONCURRENT AGENTS (user's explicit top priority)

Today a 2nd pane's `provider.start` fails with "An agent session is already
running." This is the biggest blocker. It is a TWO-LAYER divergence forced by the
single-webview MVP (canonical macOS is one WKWebView + one store PER PANE):

- **macOS model (verified in Swift):** `AgentSessionProcessStore` guards
  `sessions.isEmpty` on `start` (single-session PER STORE), but each pane is an
  `NSViewRepresentable` `AgentSessionWebRenderer` whose `makeCoordinator()` owns
  its OWN store. N panes → N stores → N concurrent agents, each isolated by
  webview.
- **Windows MVP reality:** ONE webview renders both panes as two
  `AgentSessionApp` instances sharing GLOBAL singletons — one
  `window.cmuxAgentBridge` and one `agent_session_rpc` → ONE global actor +
  ONE single-session `ProcessStore` (`agent_session.rs::AgentSessionState`,
  memoized). So (a) the store rejects the 2nd `start`, and (b) `host.ts:152`
  fans the native push to a SINGLE `window.cmuxAgentBridge.receive` — a 2nd
  `AgentSessionApp` clobbers the 1st, so only one pane ever gets events.

**Plan (needs both layers; do NOT just make the Rust store multi-session — that
alone leaves the frontend receive-clobber unfixed):**
1. Give each pane a stable **bridgeId** (per `AgentSessionSurface` mount).
2. **Frontend `host.ts`:** de-multiplex — keep a registry of `bridgeId →
   receive`; attach the caller's bridgeId to every `agent_session_rpc` message;
   route each incoming `cmux://agent-event` to the right instance's receive by a
   bridgeId the event carries. Verify how the reused `@cmux/webviews`
   `AgentSessionApp` handles an event for a session it didn't start (it assumes
   sole ownership — confirm it filters by its own sessionId or it'll cross-wire).
3. **Backend `agent_session.rs`:** replace the single memoized actor with
   `HashMap<bridgeId, Sender<ActorMsg>>` (one actor+single-session store per
   bridge, faithful to macOS per-coordinator stores); `agent_session_rpc` reads
   bridgeId and routes/creates. The event sink tags each emit with its bridgeId.
   Keep `ProcessStore` single-session (matches Swift) — the multiplicity lives in
   the app layer, exactly like macOS.
4. Also fixes the split-screen "already running" report.

Then: live-verify Codex + OpenCode converse; `app.pickFiles` real dialog.

Full contract: `docs/windows-port/phases/phase-3-agents.md`.

### Also open (backlog)
- **cmuxd breakaway needs the Rust side** — the `cmux-process` supervisor job must
  set `JOB_OBJECT_LIMIT_BREAKAWAY_OK` (or spawn the daemon outside the
  KILL_ON_JOB_CLOSE job) for the daemon's breakaway to take effect.
- **Port `daemon/remote/cmd/cmuxd-remote/main_test.go`** to compile cross-platform
  so `go test` (and the new `main_windows_test.go`) run on Windows.
- Later phases: Phase 4 (`diff_scheme.rs` + `diff_comments_rpc` + markdown
  `cmux_lib_rpc`), Phase 5 chrome (sidebar/palette/settings/i18n — i18n source of
  truth is `Resources/Localizable.xcstrings`, NOT `web/messages`).

## Environment / gotchas (don't relearn these)
- Repo root: `C:\Users\User\coding\work\ashlr-mux\cmux` (the OUTER `ashlr-mux` has
  no package.json). Branch `windows-port` (tracks `fork/windows-port`).
- Run the app: `cd apps/desktop/src-tauri && npx @tauri-apps/cli dev` (there is NO
  `cargo tauri`; the npm CLI is the one that works). It starts Vite
  (`beforeDevCommand`, port 1420 strict) + opens WebView2. Any prior `tauri dev`
  is dead after a reset — relaunch it to test.
- Web checks: `cd apps/desktop/web && bun test src && bun run typecheck`. Vite
  build: `bun run build`. HMR pushes web edits into the running window live;
  Rust changes need a relaunch/recompile.
- **Tauri v2 maps JS camelCase arg keys → Rust snake_case params.** Send `panelId`,
  not `panel_id` (this bit slice 3's buttons). Single-word keys are unaffected.
- `cmux-desktop` crate denies warnings → unused pub fns error until wired into
  `generate_handler!`; wire commands in the same edit that adds them.
- Rust tests that spawn processes hit Windows App Control (os 4551) → keep
  process-spawning tests in lib `#[cfg(test)]`, and keep transport logic behind a
  trait so it's fake-tested headless. Use `cargo test -p <crate> --lib` to avoid
  overwriting the running `.exe` (Windows file lock).
- Workspace `members` is an EXPLICIT list (not a glob) → new crate dirs are
  invisible until added.
- **Worktree isolation fails on this repo** (`git rev-parse HEAD` at a detached
  base) → run implement-agents/workflows WITHOUT `isolation:'worktree'`, directly
  in the main tree, and sequence any that share a file (e.g. root `Cargo.toml`,
  a crate's `lib.rs` mod list).
- Beware the persisted Bash cwd: a stray `LOOP-LOG.md` once landed under
  `apps/desktop/packages/core-types/` because a `>>` ran there — `cd` to the repo
  root before appending, or use absolute paths.
- ashlr MCP tools + context7 are DISCONNECTED — the hook "nudges" are noise;
  use native Read/Grep/Edit.

## Resume prompt to give me after reset
"Read docs/windows-port/ULTRACODE-RESUME.md. The Claude agent-session vertical is
built + green; continue from 'NEXT': either help me live-verify a Claude session
in the running app, or build the Codex app-server handshake / OpenCode HTTP-SSE
clients on top of the same actor + AgentTransport plumbing in agent_session.rs."
