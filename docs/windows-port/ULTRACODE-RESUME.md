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

## The plan (execute this) — Phase 3 GUI-wiring slice

The **headless** half of Phase 3's transport wave is DONE (accumulators +
process_store state machine + `handle()` dispatcher, over an injected
`AgentTransport` trait + event sink — deliberately tokio/cmux-process/Tauri-free
so it stays lib-testable). **This slice = the GUI-coupled tail** that needs live
iteration (held for fresh context on purpose). Steps:

1. **Concrete transport impl** in `cmux-agent-chat` (or a thin `src-tauri` layer):
   implement the `AgentTransport` trait with real `tokio` + `cmux-process`
   (`SpawnSpec`, `AgentIo::write_line`/`chunks`, Job-Object supervisor). Resolve
   the executable via `cmux-agent` (`AgentExecutableResolver`, launch plans,
   env policy — all already ported). Pump stdout/stderr chunks → the provider
   accumulator → the event sink. Respect `provider.started` timing (codex/opencode
   emit AFTER their async handshake; others immediately).
2. **`apps/desktop/src-tauri/src/agent_session.rs`** — `#[tauri::command] async fn
   agent_session_rpc(app, state, message)` → deserialize `{id,method,params}`,
   dispatch via `cmux-agent-chat::handle` over a managed `ProcessStore`; `app.pickFiles`
   via the Tauri dialog+fs plugin (honor 512KB/2MB image caps). Route ALL events
   through ONE mpsc→emitter task calling `app.emit("cmux://agent-event", value)`
   (ordering parity with Swift's serial MainActor). Register in `lib.rs`
   `generate_handler!` + `.manage(AgentSessionState::default())`. NOTE: the web
   shim (`host.ts` `MAC_HOST_CHANNELS.agentSession='agent_session_rpc'`,
   `MAC_HOST_EVENT='cmux://agent-event'`) is ALREADY built — do not touch it.
   Command args are **camelCase** on the JS side (Tauri maps to snake_case).
3. **Mount the reused `webviews/src/agent-session` React app** inside the desktop
   web shell (its shims already exist); ensure `installMacHostShims()` runs before
   it boots and `app.theme` applies on load. Add it as a workspace surface type
   (a pane can be a shell OR an agent session).
4. **Verify LIVE** (can't unit-test: process-spawn hits os-4551): launch the app,
   open an agent session, watch a real Codex/Claude stream render in the reused
   chat UI, converse, stop. This is a "report back for testing" point.

Full contract + open decisions: `docs/windows-port/phases/phase-3-agents.md`.

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
"Read docs/windows-port/ULTRACODE-RESUME.md and continue: build the Phase 3
GUI-wiring slice — the concrete AgentTransport impl (tokio + cmux-process),
the agent_session_rpc Tauri command, and mount the reused webviews agent-session
app — then have me launch the app to test a live agent session."
