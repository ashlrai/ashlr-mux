# Windows-port design decisions (headless harvest)

Running log of non-obvious design decisions made while porting cmux to Windows,
so future iterations (and reviewers) can see the *why*, not just the *what*.
Newest first.

## Phase 3 — Codex + OpenCode transports (the read→write feedback loop)

The crux: Codex (stdio JSON-RPC handshake) and OpenCode (HTTP-loopback + SSE)
both need to *react to reads by writing* — send `initialized`+`thread/start`
after the `initialize` response, drain the queued turn after the thread is ready,
reply to server approval requests, `POST /session` after the loopback URL is
sniffed. The pure store's `feed_output` only produced events; it had no way to
trigger writes, and the `AgentTransport` trait is `{spawn, write_line,
terminate}` (store-initiated only). Decisions (settled by an understand→design
ultracode workflow grounded in the Swift sources):

- **`TransportAction` intent list, NOT a trait extension or an actor-held
  transport.** The single `CodexAccumulator` lives in `RunningSession` and serves
  BOTH read (`consume_line` mutates `thread_id`/`did_initialize`) AND write
  (`initialize`/`thread_start`/`turn_start` builders) — so frame construction MUST
  stay in the store; a second accumulator would desync id allocation. But I/O
  cannot run in the pure crate. Resolution: the store appends pure
  `TransportAction`s (`WriteStdin`/`Terminate`/`OpenCodeCreateSession`/
  `OpenCodePostPrompt`) that the `src-tauri` actor drains via
  `take_transport_actions()` after EVERY message and executes against shared
  handles. This keeps `cmux-agent-chat` dependency-pure + os-4551-safe (unit tests
  assert the emitted `Vec<TransportAction>` with no transport, no spawn), leaves
  the `AgentTransport` trait + Claude path untouched, and honors no-tokio.
- **Codex reactive machine lives in `RunningSession::handle_codex_line`.** After
  `consume_line`, react in spec order: (a) `initialize` resolved → push
  `initialized` THEN `thread/start` (two separate `encode_line` frames); (b)
  `thread_id` first known → drain the queue (`MAX_QUEUED_INPUT_COUNT == 1`); (c)
  the line was a server request (`parse_server_request`, raw id echoed verbatim) →
  push the approval / `-32601` reply; (d) `take_startup_failure_signal()` →
  `fail_codex_queue` + `pending_terminate`. The `&mut accumulator` borrow is
  dropped (bools+events extracted first) before the `self.*` helpers run.
- **`WriteStdin.line` is already `encode_line`-terminated; the actor writes it
  RAW** (no `AgentIo::write_line`, which would double the newline and corrupt the
  JSON-RPC framing).
- **OpenCode HTTP = `ureq` (blocking, no tokio), on worker threads.** `reqwest`
  transitively pulls tokio → rejected. Loopback is plain HTTP → `default-features
  = false` (no TLS). `create_session`/`post_prompt`/`stream_events` run on
  short-lived / long-lived `std::thread`s that report back as new `ActorMsg`
  variants; the actor stays responsive. `provider.started` stays deferred to
  `complete_opencode_handshake` (after `POST /session` returns a valid id). Auth
  header is derived from `spec.env` (the `OPENCODE_SERVER_*` creds `cmux-agent`
  mints) in `spawn`, BEFORE the spec is moved into `spawn_captured`, and stored in
  a host-side `OpenCodeContexts` map — the secret never enters the pure store.
- **Documented divergence: OpenCode `writeLine` is optimistic.** Swift awaits the
  `POST prompt_async` and throws `providerNotReady` on failure; our single-threaded
  actor cannot block up to 30s on the POST without stalling all event ordering, so
  the RPC returns `{sent:true}` and a failed POST surfaces as a synthetic OpenCode
  `stderr` line (emitted verbatim by the store) instead of a synchronous error.
- **EOF-vs-error rule is actor-side** (`!cancelled && process_running`), needing
  live process state the pure store lacks. `process_running` is an `AtomicBool`
  cleared by the reader thread the instant the child pipes close; the natural-exit
  path also drops the whole context in the actor's `Exit` handler (so a later clean
  `/event` EOF finds no context and declines to fail an already-exited session).
  A narrow mislabel race remains (inherited from Swift's async `Process.isRunning`)
  — cosmetic only, never a double `provider.exit`.
- **Intentional Codex parity divergence:** `{"id":null,...}` is treated as a
  notification (no reply), not a server request. A null request id is malformed per
  JSON-RPC 2.0 and Codex never emits one; matching Swift's NSNull-is-non-nil reply
  would fabricate a reply to a malformed frame.

Verified by a 4-lens adversarial review workflow (9 agents): 4 confirmed findings,
all LOW; the two real context leaks (reader-spawn-failure rollback + natural-exit)
were fixed, the other two documented as inherited/intentional.

Live-run fix: a spawn/resolve failure (e.g. the CLI is not installed) now maps to a
new `BridgeError::ProviderLaunchFailed(detail)` — its detail IS the user-facing
message ("<Provider> could not be started. <reason>"), mirroring the macOS
`AgentExecutableResolverError` envelope (`{ok:false, error:{userMessage}}`, no code;
`AgentSessionWebRendererCoordinator.swift:175`) — instead of the misleading generic
`providerNotReady` ("The provider is not ready yet."). `TransportError::NotReady`
still maps to `providerNotReady` (transient; its code is load-bearing — the renderer
silently retries `writeLine`). Note: `BridgeError` is otherwise a verbatim port of
Swift `AgentSessionBridgeError`; this variant stands in for the *separate* Swift
resolver-error type so the renderer shows the real reason (faithful to the macOS
user experience, not the enum shape). KNOWN LIMITATION surfaced live: one agent
session per WINDOW (singleton `cmuxAgentBridge` + single-active-session store);
macOS is one-per-pane. Lifting it (per-pane bridge routing + multi-session store)
is a deferred feature.

## Phase 3 — agent-session GUI-wiring (Claude live) — non-obvious decisions

- **The concrete transport lives in `src-tauri`, not `cmux-agent-chat`.** The
  `AgentTransport` trait is in `cmux-agent-chat`, but that crate is deliberately
  Tauri/OS/tokio-free (its 168 tests run headless behind a `FakeTransport`, dodging
  os-4551). Putting the real `cmux-process`+`cmux-agent` implementation there would
  add those deps and break that purity. So `ClaudeAgentTransport` is a `src-tauri`
  concern; the crate stays pure and the concrete transport is verified LIVE.
- **No tokio anywhere.** The mapping agents established that `cmux-process` is pure
  blocking `std` (`spawn_captured` → `AgentIo` = a blocking `mpsc::Receiver<Agent
  OutputChunk>` + a stdin writer; NO wait/exit-code API — channel disconnect is the
  only exit signal), and `ProcessStore` is single-threaded by construction (its sink
  is `FnMut(AgentEvent)`, not `Send`). Both point at ONE design: a single **actor
  thread** owning the store, fed by an `mpsc<ActorMsg>` (`Rpc`/`Feed`/`Exit`), exactly
  like `terminal.rs`. Draining serially = the macOS serial-`MainActor` event ordering
  the renderer's state machine needs. The resume doc's "tokio + stdio pump" framing
  was superseded by the actual (blocking) `cmux-process` shape.
- **`AgentIo::into_parts()` added to cmux-process.** The reader thread must OWN the
  receiver (to block on `recv()`), while the actor keeps the stdin writer (for
  `write_line`); `AgentIo` glued them with private fields and no split (`chunks()`
  only lends `&Receiver`). A small, natural `into_parts(self) -> (stdin, receiver)`
  resolves the opposite ownership needs.
- **Windows shim wrapping is mandatory and lives in the transport.** `claude`
  resolves (correct PATHEXT order) to `claude.cmd` (npm shim); `CreateProcessW` can
  only launch a real PE image, so it would fail on `.cmd`/`.bat`/`.ps1`.
  `wrap_windows_shim` rewrites the resolved `SpawnSpec`: `.cmd`/`.bat` → `%ComSpec%
  /C <shim> <args>`, `.ps1` → `powershell -NoProfile -ExecutionPolicy Bypass -File
  <shim> <args>`. The AGENT executable + args are captured BEFORE wrapping so
  `provider.started` shows `claude.cmd`, not `cmd.exe` (canonical). Simple single-
  quoted-path `cmd /C` form is correct because the agent launch args carry no spaces.
- **exit + teardown needs three signals; the reader emits all three.** `ProcessStore`
  clears the session + emits `provider.exit` only after `notify_exit` AND stdout-EOF
  AND stderr-EOF. `cmux-process` merges both pipes onto one receiver and signals a
  single disconnect, so on disconnect the reader sends `Feed(stdout,∅)` +
  `Feed(stderr,∅)` + `Exit`. Exit status is 0 (cmux-process exposes no code — a known
  minor fidelity gap vs the Swift terminationHandler).
- **`app.context` copy has 67 keys, not 68.** The extraction miscounted; the
  authoritative `types.ts AgentSessionCopy` has 67. Built from a `(key,value)` slice
  into a `serde_json::Map` (a 67-entry `json!` literal overflows the macro recursion
  limit). Localization deferred to Phase 5 (source of truth `Localizable.xcstrings`).
- **Canonical surface kind in the session model, not a web-only toggle.** Per "stay
  true to cmux", `surface_kind: Option<String>` rides `SessionPaneLayoutSnapshot`
  (the authoritative snapshot), `skip_serializing_if none` so absent = terminal and
  the Swift-authored golden fixtures stay byte-identical (Swift doesn't emit it yet).
  `session_ops::set_surface_kind` puts the kind on the pane node so it survives splits
  (the pane keeps its side) and closes. The web flat portal keys by `panelId`, so a
  terminal↔agent swap under the same key remounts only that one surface.
- **`agent_session_rpc` returns the RAW `{ok,value|error}` envelope.** The `host.ts`
  shim routes the `agentSession` channel via `invokeRaw` (bridge.ts unwraps
  `{ok,value}` itself), so — unlike the bare-value `session_*`/`terminal_*` commands —
  this command must hand back the envelope. Error `code` is load-bearing
  (`providerNotReady` → the renderer silently retries `writeLine`).
- **Single active agent session (store-enforced).** `ProcessStore` allows one live
  session; the singleton `cmuxAgentBridge` has no per-pane routing. So v1 supports one
  agent pane at a time; a second is safe (start rejects with `sessionAlreadyRunning`).

## Phase 1 — React/Vite/Tailwind foundation + host bridge

The desktop app moved off the Phase-0 vanilla-TS + `Bun.build` shell onto the
real UI stack, and grew the reusable **host bridge**. Non-obvious decisions:

- **`webviews` joined the root bun workspace now** (not deferred). Adding
  `"webviews"` to the root `workspaces` array symlinks `@cmux/webviews` and — via
  bun hoisting — guarantees a **single React instance** (`.bun/react@19.2.3`,
  verified from both packages). This is what lets reused webviews components run
  under the desktop app's React without the dreaded "two Reacts" hook crash. The
  alternative (defer; validate against only the static markdown viewer) was
  rejected by the user to prove deep reuse in Phase 1.
- **The webkit shim returns the RAW `NativeReply` envelope, not the unwrapped
  value.** The reused `webviews/` bridges (`agent-session/shared/bridge.ts`,
  `comments/bridge.ts`) call `webkit.messageHandlers.<name>.postMessage(...)` and
  **unwrap `{ok,value|error}` themselves**. So `installMacHostShims` must hand
  back the envelope verbatim — it cannot route through `callNative` (which
  unwraps). That forced a second transport primitive, `invokeRaw`, alongside
  `callNative`. The shim also never throws: a transport rejection becomes
  `{ok:false,error}` so reused code sees a normal failed reply.
- **Channel→Tauri-command map is the stable seam; the Rust backends are
  deferred.** `agentSession→agent_session_rpc`, `cmuxDiffComments→
  diff_comments_rpc`, `cmuxLib→cmux_lib_rpc`, each taking `{id,method,params}`
  and returning a `NativeReply`. The shim is complete now; the `#[tauri::command]`
  implementations land per-surface in Phases 3–4. Until then those channels
  return `{ok:false}` — and nothing in the Phase-1 build calls them, so that is
  invisible.
- **Native→webview push stays a push, not a listen.** macOS evaluates
  `window.cmuxAgentBridge.receive(event)` into the WKWebView; the shim mirrors
  that by subscribing to the Tauri event `cmux://agent-event` and forwarding each
  payload to `cmuxAgentBridge.receive`. Reinstalling tears down the prior
  subscription first so dev HMR never double-delivers.
- **Shim option types are non-generic (`unknown` in/out).** A generic
  `<T>(...)=>Promise<T>` call signature cannot be satisfied by a concrete test
  double; making `InvokeRaw`/`Listen` non-generic keeps the shim honest (it only
  ever needs `unknown`) and lets plain mocks inject.
- **Vite `strictPort: 1420` + cwd-independent `beforeDevCommand`.** Tauri's
  `devUrl` is fixed, so Vite must fail loudly (not silently pick another port)
  if 1420 is taken. The root `desktop:web:dev` delegator uses `bun run --cwd
  apps/desktop/web dev`; because bun executes a script from the defining
  package.json's dir, this resolves correctly even when Tauri runs it from the
  `src-tauri` cwd (verified). NOTE: `cargo tauri` is not installed on the dev
  box — the working CLI is npm's `@tauri-apps/cli` via `npx`.
- **Removed `scripts/desktop/build-desktop-web.mjs`** (replaced by `vite build`)
  and dropped its now-dead entry from `scripts/ci/detect_ci_change_areas.py`
  (the `apps/desktop/` prefix already covers every new web file).

Reuse proof: the zero-dep `@cmux/webviews` `Icon` renders both at build (bundled)
and at runtime (`react-dom/server` test). Host bridge has 8 unit tests; the
Phase-0 `tauri-bridge` transport + tests are preserved verbatim (one latent CFA
type bug fixed, surfaced by the new `tsc --noEmit` script). Remaining Phase-1
acceptance is the live WebView2 run (`npx @tauri-apps/cli dev`) — needs the GUI.

## M4 WS5 — v1 client wire codec + the "no generic forward" finding

An understand-workflow over `CLI/cmux.swift`'s ~50 socket-command handlers
established the architecture of the socket command layer, and why a generic
forward is not a faithful port:

- **No name-deriving forwarder.** Each handler hardcodes its *socket* command
  name as an underscore_case string literal at the `sendV1Command` call site.
  The hyphen→underscore correspondence (CLI `list-windows` → socket
  `list_windows`) is a hand-typed convention, not a transform; `notify` →
  `notify_target` is a genuine rename. There is no mapping table.
- **Two transports.** Many modern verbs (`send`, `new-split`, `new-workspace`,
  `capture-pane`, `resize-pane`, all `browser.*`) emit **v2 JSON-RPC** with a
  typed params dict, not a v1 line (`send` → `surface.send_text`, `resize-pane`
  → `pane.resize`).
- **Args are not forwarded verbatim.** Handlers do per-flag work: renaming
  (`--name` → `title`), **handle resolution** (`--workspace`/`--surface` refs →
  uuids via *socket round-trips*), env defaulting (`CMUX_WORKSPACE_ID`), and the
  v1/v2 choice. Even the closest-to-verbatim v1 path rewrites `--workspace` →
  `--tab=`, strips `--window`, and injects a default `--tab=`.

**Consequence:** the per-command socket layer must be ported as bespoke handlers,
and handle resolution requires a *server* implementing those methods — i.e. it is
blocked on the app/server side (a HARD BOUNDARY). The headless CLI-side primitives
of M4 are otherwise complete.

**What was built:** the one frozen, reusable artifact the finding supports — the
**v1 client wire codec** in `cmux-ipc/client.rs` (`shell_quote`,
`build_v1_command_line`, `interpret_v1_response`, `V1ResponseError`), the dual of
the v2 codec (`build_v2_request` / `interpret_v2_response`). It is pure and
parity-pinned to exact Swift lines (`shellQuote` 12004-12010; forwarder
16294-16297; `sendV1Command` 5765-5771), so its shape is verifiable without a
caller. The CLI→socket command-name mapping is deliberately the *caller's*
responsibility (kept out of the codec). `V1ResponseError` is a newtype (not an
enum like `V2ResponseError`) because a v1 reply carries exactly one failure bit
(`ERROR:`-prefixed or not) — modeling it as an enum would fabricate distinctions
the protocol lacks. Watch-item: land the first real v1 handler before long so the
frozen primitive gets one end-to-end integration exercise.

## M4 WS5 — wire `classify_command` into dispatch (`cmux-cli`)

- **Two-step `classify → plan → executor` split.** `classify_command`
  (`classify.rs`) is the permanent, complete Swift-parity port of `run()`'s
  pre-socket taxonomy (18 branches, load-bearing order). `plan()` (`dispatch.rs`)
  is the *porting-frontier projection*: it maps each `PreSocketAction` to a
  `DispatchPlan` the thin `main.rs` executor performs. Keeping the frontier churn
  (stopgaps, the `rpc` special-case) out of the parity classifier means a newly
  ported command edits `plan()` + executor, never the stable classifier or its
  parity tests.
- **Generic v1 socket forward DEFERRED.** Swift sends a "needs socket" command as
  a v1 line: `([socketCommand] + args).map(shellQuote).join(" ") + "\n"`. But the
  *socket* command name is bespoke per command (the CLI `list-windows` is sent as
  `list_windows`) — there is **no universal CLI-name → socket-name rule**. A
  generic forward would send wrong frames. So only `rpc` (the one command with a
  complete v2 codec) runs today; every other `NeedsSocket` command returns a clear
  "not yet ported (only 'rpc' wired)" error. Unblocking this needs a server-contract
  map (the per-command socket name + arg shape + response-render table).
- **Side-effecting no-socket commands → `not_yet_ported`.** `docs`, `welcome`,
  `sessions`, `settings`/`config` docs, the sigpipe/diff-viewer probes, `open
  <path>`, `window default-display`, etc. each need a subsystem not in the headless
  core yet. They fail with exit 1 and a descriptive label rather than a guessed
  behavior.
- **Top-level help + per-command usage text DEFERRED.** `print_top_level_help`
  prints a one-line synopsis, not the full macOS `usage()` block. The verbatim port
  is held back because (a) the 150-command listing references macOS-specific paths
  (`~/.local/state/cmux/cmux.sock`, `~/.config/ghostty`) that need Windows
  adaptation, and (b) the per-command `subcommandUsage` text is a ~1989-line switch.
  `SubcommandHelp` prints the faithful `cmux <command>` header + a pointer as a
  stopgap (exit 0, matching Swift).
- **Kept the `command` param on `plan()` rather than enriching
  `PreSocketAction::NeedsSocket { command }`.** The deeper alternative would touch
  the stable parity classifier and its many `NeedsSocket` assertions; "rpc is the
  one wired command" is a property of the porting frontier, so it belongs in
  `plan()`, not the Swift mirror.

## Phase 2 slice 3 — flat portal workspace (terminals survive splits)

- **Flat portal layer, NOT recursive-tree rendering.** Terminals are rendered
  once in an absolutely-positioned layer keyed by stable `panel_id`, positioned
  from geometry computed off the layout tree (`paneRects`). The recursive
  `SplitTree` would move a pane's DOM position on every split → React remount →
  the pane's ConPTY shell dies. Keying by `panel_id` in a flat layer means a
  split/close never changes a surviving pane's React identity, so its
  `<TerminalSurface>` (and its shell) lives on. This mirrors the macOS portal
  approach. `SplitTree.tsx`/`SplitDemo.tsx` are kept (SplitTree still unit-tested;
  may retire later).
- **Pure geometry stays in percentages; divider inset applied at render.**
  `paneRects` returns pure %-rects (panes meet at divider centerlines) so it is
  headless-unit-testable without pixel context. `Workspace.tsx` insets each pane
  by half the divider thickness only on edges NOT on the container boundary
  (derivable from the rect alone: `x>0` ⇒ has a left divider), and draws the 6px
  divider handles as an overlay above the panes. Divider drag reuses the shared
  `resizeDivider`/`setDividerAtPath` math (macOS bonsplit parity).
- **Divider drag = optimistic local, persist on release.** During drag the
  layout is overridden locally (smooth, no round-trips); on pointer-up a single
  `session_set_divider` persists the final ratio and the returned/emitted snapshot
  clears the override. Chosen over per-move backend writes to avoid flooding Tauri
  invokes. Easy to switch to live-persist if desired.

## Ultracode parallel advance — crates + research (2026-07-01)

- **Worktree isolation fails on a detached HEAD.** The first workflow ran the two
  crate-implementation agents with `isolation:'worktree'`; both errored before
  running with `Failed to resolve base branch "HEAD": git rev-parse failed` (the
  repo is at a detached HEAD). Fix: rerun WITHOUT worktree, writing directly to the
  main tree, and SEQUENTIALLY so the two `Cargo.toml` member-line edits don't race.
  New leaf crates don't lock the running `cmux-desktop.exe`, so in-tree builds are
  safe. Lesson for future workflows on this repo: skip worktree isolation until the
  branch has a resolvable name.
- **`cmux-agent-chat` first slice re-scoped by the Phase 3 research.** The resume
  doc scoped it as "transcript record + JSONL parser." The Phase 3 research proved
  the JSONL-transcript reader is the SEPARATE mobile-companion history subsystem
  (`Sources/Mobile/AgentChat/*`, `chat.message` topic) — orthogonal to the live
  webview host contract. So the first slice instead builds the pure foundations the
  live contract needs (`BridgeError` w/ exact `code()` strings, `BridgeRequest`
  w/ the trim vs no-trim getter distinction, `PermissionMode`, `AgentEvent` serde
  matching `types.ts` wire shapes with optionals omitted-not-null, `OutputLineBuffer`).
  The stateful transports (codex/claude/opencode), process_store, context/theme,
  and the history subsystem are honestly deferred.
- **`cmux-config` covers core sections, preserves the rest losslessly.**
  Top-level sections are `Option<T>` and every unmodeled key is captured via
  `#[serde(flatten)]` into `Config.extra`, so decode→encode is lossless at the
  section level (round-trip test asserts a fixed point, not byte-identity — the
  correct invariant for a defaulting model). Not deny_unknown_fields (the schema is
  large; we model a subset). Windows config path = `dirs::config_dir()/cmux/cmux.json`
  (= `%APPDATA%\cmux\cmux.json`). Deferred: enum-value validation, shortcut-binding
  grammar, numeric ranges, color-hex patterns, and the free-form
  actions/ui/commands/vault/workspaceGroups sections (kept in `extra`).
- **i18n source-of-truth finding (Phase 5, overrides the resume doc).**
  `web/messages/en.json`/`ja.json` are the MARKETING website catalog (keys
  ios/meta/home/blog/docs/…) with ZERO app-shell strings. The shell strings
  (`sessionIndex.*`, `settings.*`, `shortcut.*.label`, `menu.*`, `command.*`) live
  in `Resources/Localizable.xcstrings` (EN+JA). ⇒ the React shell must localize
  from a flattened `Localizable.xcstrings` (build-step generated), not web/messages.

## Slice 3 bugfix — Tauri v2 argument casing (camelCase, not snake_case)

Symptom: the workspace's split buttons did nothing while the initial pane
rendered fine. Root cause: **Tauri v2 maps JS camelCase argument keys onto Rust
snake_case command params.** `useSession.ts` was sending `panel_id` (snake_case),
so `session_split`/`session_close` failed deserialization ("missing field
panelId") and the error was swallowed. `session_snapshot` worked because it takes
no args — which is exactly why the single pane rendered but nothing could split.
None of the terminal commands caught this: their args (`id`/`cols`/`rows`/`data`)
are single words, identical in both cases. Fix: send `panelId`; single-word keys
(`path`/`position`/`orientation`) are unaffected. Also replaced the silent
`.catch(() => {})` on the session mutators with `console.error` so a failed
command is visible next time. (The prior resume-doc "snake_case" note was a wrong
guess never exercised until a multi-word-arg command reached the frontend.)

## Phase 4 — diff/markdown surface MOUNT model (swarm-derived, 2026-07-02)

A read-only planning swarm resolved the central Phase-4 open question. **The
headless Rust core is mount-independent — build it first, decide the mount
later.** Evidence (R1, high confidence): the Windows shell renders *every* pane
(terminals AND the reused agent-session app) as **React in ONE document**
(`Workspace.tsx` flat DOM portal); no child webviews exist anywhere. Tauri's
child-webview embedding (`Window::add_child`, `Webview::url/set_position/set_size`)
is `#[cfg(feature = "unstable")]` in tauri 2.11.3 — **semver-exempt and OFF
today** (`Cargo.toml` features = []). But `register_asynchronous_uri_scheme_protocol`
is **stable** and embedding-independent.

**Decision:** build the diff/markdown backend + custom-URI-scheme handlers now;
treat the surface mount as a separate deferred choice. Recommended mount when we
get there: an **`<iframe>` whose `src` is the custom scheme**
(`cmux-diff-viewer://<token>/index.html`) positioned by the existing paneRects
DOM layout — because it (a) keeps the frozen `webviews/src/comments/bridge.ts`
verbatim (that bridge sends only `{repoRoot, comment}`, **no token** — so the
token MUST ride in the frame URL, which an iframe carries but a main-document
mount cannot without editing the frozen seam), (b) needs only the stable scheme
API, (c) is pure DOM (no native rect-sync, no WebView2 airspace occlusion of the
z-10 dividers / z-20 PaneControls that unstable child-webviews would cause). The
one unit-untestable risk to spike on a live WebView2: whether `diff_comments_rpc`
can read the calling **iframe's** frame URL for the token gate (`webview.url()`
may return the main frame, not the sub-iframe). If it can't → fall back to
unstable child-webview (explicit go/no-go), **never** to main-document.

Landed this session: `crates/cmux-diff/src/session.rs` — the token/session
registry (validators + trusted-root jail + 24h expiry), a faithful headless port
of `CmuxDiffViewerURLSchemeHandler` (`BrowserPanel.swift:1904`). 14 tests.

## Findings parked for follow-up (2026-07-02)

- **`dialSocket` has a Windows fail-over gap (real, production).** The two
  `TestDialSocket*` tests in `cmuxd-remote` fail on Windows: a WinSock
  `connectex: ... actively refused` error is NOT matched by `dialSocket`'s
  address-refresh trigger, so `refreshAddr` is called 0 times and the dial does
  not fail over to a refreshed relay address. Gated `//go:build !windows` for now
  (tests-only slice); fix `dialSocket`'s error classification when the Windows
  CLI relay dial is actually ported.
- **Windows sends a meaningless `workingDirectory` (agent chat).** The "IDE
  context" composer chip is NOT an IDE integration — it's the auto-context chip
  (workspace dir → @-mention), gated only on `state.context.workingDirectory`,
  and Windows already matches macOS-React exactly. The real (host-side) gap:
  `agent_session.rs` sends `std::env::current_dir()` (the app/exe launch dir)
  unconditionally, so the chip always shows with a meaningless dir. Faithful fix
  lives in `agent_session.rs`: send the actual workspace cwd, or `None` when
  there is no real workspace (the existing webview gate then hides the chip with
  zero frontend change). No frontend change is correct.

## Swarm-quality lessons (2026-07-02)

- **Verify by RUNNING, not compiling.** The Go-test-port implement agent
  self-reported "PASS" from a compile-only check (`go test -run XXNONE`, "no
  tests to run") — my verify instruction handed it that shortcut and framed the
  goal as "compile." Running the suite surfaced 4 Windows runtime failures its
  scout had explicitly predicted. For test-porting, the acceptance command must
  be "tests green," never "tests compile."
- **Strict StructuredOutput schemas + huge payloads = retry-cap failures.** The
  Phase-4 research agents that had to return byte-level canonical-contract dumps
  (R2/R3/R4) hit the 5-retry StructuredOutput cap (oversized objects failing a
  6-required-field `additionalProperties:false` schema). Keep research-return
  schemas loose (one freeform field) or chunk the extraction.

## Windows errno identity is not portable — verify the predicate fires (2026-07-02)

- **`syscall.ECONNREFUSED` ≠ WinSock `WSAECONNREFUSED` under `errors.Is` on
  Windows/Go 1.26.4.** `syscall.ECONNREFUSED` is a synthetic constant
  (`0x20000016`); a real refused connect carries winsock errno `10061` in its
  `OpError → SyscallError → syscall.Errno` chain, so `errors.Is(err,
  syscall.ECONNREFUSED)` returns false. The match that fires is
  `errors.Is(err, golang.org/x/sys/windows.WSAECONNREFUSED)`. The pre-existing
  `main.go:783` precedent using `syscall.ECONNREFUSED` is itself ineffective on
  Windows for the same reason (latent bug; not fixed in this slice — flagged).
- **Process:** the scout PRESCRIBED the `syscall.ECONNREFUSED` fix; the implement
  agent ran the relocated test on `GOOS=windows`, saw it still fail
  (`refreshCalls==0`), and switched to the x/sys errno. Adversarial "verify by
  RUNNING on the target OS" caught a plausible-but-wrong fix. Do not trust
  `errors.Is` across GOOS on synthetic syscall constants — confirm the predicate
  actually matches a real error from the target platform.

## Swarm partitioning that held (2026-07-02)

- Read-only scout workflow (4 agents) → disjoint-lane specs, then a 2-lane
  implement→verify workflow run in the BACKGROUND while the orchestrator
  hand-built a third lane. Overwrite-free because: the third lane (cmux-markdown)
  owns a brand-new crate + the root Cargo.toml/lock, lane A owns Go files, lane B
  owns cmux-diff/{session,lib,manifest,submission_pool}. The one true conflict
  (typography wants cmux-markdown's lib.rs/Cargo.toml) was SEQUENCED after the
  core, not parallelized. Verify agents correctly flagged the other lanes' files
  as "scope contamination" in the shared tree — expected; committed per-lane.
