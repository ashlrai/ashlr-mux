# Milestone-loop log

One line per completed slice (milestone/result/commit/next). Newest last.

- M4 WS5 — wire `classify_command` into `cmux-cli` dispatch (pure `plan()` +
  thin executor); `rpc` preserved, generic v1 forward + side-effecting no-socket
  actions return honest "not yet ported" errors. Tested (cmux-cli 47, workspace
  357, clippy clean) → simplify (collapsed 15 not-ported arms) → retested green.
  Commit: `1a04263`. Next: server-contract map for the v1 generic forward, OR
  Go daemon lifecycle (WS1-3).
- M4 WS5 — v1 client wire codec in `cmux-ipc` (`shell_quote` +
  `build_v1_command_line` + `interpret_v1_response` + `V1ResponseError`), dual of
  the v2 codec; parity-pinned to Swift. Understand-workflow first PROVED there is
  no generic v1 forward (per-command bespoke handlers + v1/v2 split + handle
  resolution → blocked on app/server; see DECISIONS.md). Tested (cmux-ipc 76,
  workspace green, clippy clean) → simplify (collapsed dual `.map`) → retested
  green. Commit: <pending>. Next: Go daemon lifecycle (WS1-3) — the unblocked
  remaining half of M4.
- Phase 1 (React foundation + host bridge) — moved desktop app off vanilla-TS +
  `Bun.build` onto React 19 + Vite 7 + Tailwind 4; added `webviews` to the bun
  workspace (single hoisted React, verified). Built the host bridge: generic
  `host.invoke/on` for shell code + `installMacHostShims()` presenting the macOS
  WKWebView contract (`webkit.messageHandlers.{agentSession,cmuxDiffComments,
  cmuxLib}` returning RAW `NativeReply` via new `invokeRaw`; `cmux://agent-event`
  → `cmuxAgentBridge.receive` push). Ported terminal to React `<TerminalSurface>`.
  Reuse proven with `@cmux/webviews` `Icon` (build + `react-dom/server` runtime
  test). Wired Tauri↔Vite HMR (`beforeDevCommand`, `devUrl:1420`, strictPort).
  Removed `build-desktop-web.mjs` + its CI ref. Tested: 32 web tests pass, `tsc
  --noEmit` clean, `vite build` emits dist. Commit: <pending>. Deferred to P3/P4:
  the Rust `agent_session_rpc`/`diff_comments_rpc`/`cmux_lib_rpc` backends.
  Remaining P1 acceptance: live WebView2 run (needs GUI). Next: Phase 2
  (workspace shell — tabs + splits) once the GUI run is confirmed.
- Phase 1 — VISUAL ACCEPTANCE PASSED. `npx @tauri-apps/cli dev` launched
  cmux-desktop.exe (cargo built 6.81s, zero panics); live PowerShell terminal,
  React header rendered, HMR update to App.tsx/styles.css applied live (R7
  cleared). Fixed reused-`Icon` invisibility: webviews icons are stroked line-art
  needing `stroke:currentcolor;fill:none;stroke-width:1.6` — added canonical rule
  to desktop styles.css (`.cmux-icon svg`). NOTE dev box has no `cargo tauri`;
  use `npx @tauri-apps/cli dev`.
- Phase 2 slice 1 (R2 split-pane spike) — DECIDED: lightweight custom React
  renderer over the typed model, NOT bonsplit (Swift/AppKit, not checked out) nor
  a heavy React lib. Session model already in `cmux-core::session` + generated to
  `@cmux/core-types` (binary tree, `{type:pane|split}` union, one f64
  `divider_position` + orientation per split). Built pure `session/splitLayout.ts`
  (clampDivider 0.1–0.9, resizeDivider Δpx/axis, leaf-weighted equalizeDivider,
  countLeaves, immutable setDividerAtPath — all macOS bonsplit-parity) + recursive
  `components/SplitTree.tsx` (flex nesting, pointer-drag dividers). 52 web tests
  pass (+21), tsc clean. NO backend/GUI dep — pure + SSR-tested. Commit: <pending>.
  Next (needs go-ahead — larger, GUI-coupled): slice 2 Rust session-state manager
  (hold AppSessionSnapshot, mutate open/close/split/focus/move/resize, one ConPTY
  per pane, emit `cmux://session-changed`), then wire SplitTree → live snapshot.
- Phase 2 slice 2 (Rust session-state backend) — DONE + tested. Step 1:
  `cmux-core/src/session_ops.rs` — pure split-tree ops (clamp_divider 0.1–0.9,
  count_leaves, equalize_divider, contains_panel, split_pane w/ insert_first,
  close_panel collapsing emptied splits into sibling + CloseOutcome, immutable-ish
  set_divider_at_path, SplitChild first/second serde-lowercase). Authoritative
  mirror of web splitLayout.ts + macOS CmuxPanes. 17 tests, clippy clean (cmux-core
  89 total). Step 2: `apps/desktop/src-tauri/src/session.rs` — `SessionState`
  (Mutex<AppSessionSnapshot> + AtomicU64 panel counter), thin `#[tauri::command]`s
  `session_snapshot`/`session_split`/`session_close`/`session_set_divider` over
  pure apply_* helpers (active_layout_slot navigates window→selected workspace),
  emits `cmux://session-changed`. KEY ARCH: session layer owns STRUCTURE ONLY —
  NOT ConPTY; each pane's terminal lifecycle stays with `<TerminalSurface>` mount/
  unmount keyed by panel_id (keeps session layer headless-testable, dodges os-4551).
  7 tests, clippy clean; registered in lib.rs. `tauri dev` hot-recompiled clean
  (14.4s), app running with commands live. Commit: <pending>. NEXT (slice 3, GUI):
  wire frontend to live snapshot — `useSession` hook (snapshot + session-changed
  listen), `Workspace` component rendering SplitTree with a TerminalSurface per
  pane. DESIGN FORK to resolve first: terminals must persist across layout changes
  (a split moves a pane's tree position → naive flex reconciliation would remount
  & KILL its shell). Plan: flat terminal layer positioned by geometry computed from
  the tree, keyed by panel_id (macOS portal approach), dividers as an overlay.
- Phase 2 slice 3 (live workspace, flat portal) — DONE + tested (automated).
  Replaced the mock `SplitDemo` with a snapshot-driven `Workspace`. New:
  `session/paneRects.ts` (pure: `paneRects` panel_id→%-rect + `dividerHandles`;
  8 tests), `hooks/useSession.ts` (session_snapshot + `cmux://session-changed`
  subscribe; split/close/setDivider), `components/Workspace.tsx` (one absolutely-
  positioned `<TerminalSurface>` per panel_id — STABLE key so a split never
  remounts a surviving shell; divider overlay w/ optimistic drag persisted on
  release; per-pane split-H/V + close controls). Swapped `App.tsx`. Kept
  SplitDemo/SplitTree. 60 web tests pass, `tsc` clean, `vite build` clean.
  Commit: <pending>. Live-test checkpoint: user to confirm shells survive splits
  in the running window (HMR-pushed). Next: Phase 3 transports OR Go daemon WS1-3.
- Ultracode parallel advance (workflow `w5kvotmvq` + retry `wgj21xmq0`) — DONE.
  3/3 research specs (Phase 3 agent-session contract, Phase 4 diff+markdown,
  Phase 5 chrome) → folded into `docs/windows-port/phases/phase-3/4/5-*.md`. Two
  crates landed IN-TREE (worktree isolation failed on detached HEAD → rebuilt
  without worktree, sequential): `crates/cmux-config` (serde model of
  web/data/cmux.schema.json core sections; dirs::config_dir()/cmux/cmux.json;
  `#[serde(flatten)] extra` preserves unmodeled sections; 7 tests) and
  `crates/cmux-agent-chat` FIRST SLICE (pure foundations: BridgeError/BridgeRequest/
  PermissionMode/AgentEvent/OutputLineBuffer, ts-rs export; 31 tests). Both added
  to workspace members; `cargo test`+`clippy` clean (38 tests total). Commit:
  <pending>. Next: Phase 3 transports build on cmux-agent-chat.
- Concurrent Phase 3/4 + daemon tracks (agents, 2026-07-01) — three isolated
  background tracks, zero file conflicts:
  (A) `cmux-agent-chat/src/{codex,opencode}.rs` — pure accumulators ported
  faithfully from Sources/Panels/{CodexAppServerSession,OpenCodeEventStreamParser,
  OpenCodeEventTextAccumulator,OpenCodeProcessOutputDisposition}.swift. codex:
  JSON-RPC request builders (initialize/thread_start/turn_start w/ permission
  overrides) + `CodexAccumulator` (handshake id-tracking, turn guards, notification
  →AgentEvent, pure backpressure state); opencode: SSE line parser + text
  accumulator + stdout server-URL disposition. Deferred: stdio transport/spawn,
  async queue continuations, http-loopback client. cmux-agent-chat now 137 lib
  tests, clippy clean.
  (B) new crate `crates/cmux-diff` — `DiffCommentStore` port (per-repo JSON at
  dirs::data_dir()/cmux/diff-comments/<repoKey>.json; repoKey=hex(SHA256(canonical
  root))[..24] w/ Windows \?\-strip + drive-case fold; deterministic sorted-key
  output via alphabetical field order; upsert-preserves-createdAt, idempotent
  delete). 10 tests, clippy clean, in workspace. Deferred: cmux-diff-viewer://
  scheme/token model, submission pool, diff_comments_rpc.
  (C) Go daemon `daemon/remote/cmd/cmuxd-remote` — WS2 survive-parent-exit on
  Windows via `CREATE_BREAKAWAY_FROM_JOB` + ERROR_ACCESS_DENIED fallback (never
  regresses launch); pure non-spawning flag tests; go build/vet cross-platform
  clean (1 pre-existing unrelated vet error). FOLLOW-UPS: Rust launcher must set
  JOB_OBJECT_LIMIT_BREAKAWAY_OK / spawn outside the kill-on-close job (#13); port
  Unix-only daemon main_test.go cross-platform to unblock `go test` on Windows (#14).
  Combined Rust gate: 154 tests green (cmux-config 7 + cmux-agent-chat 137 +
  cmux-diff 10), clippy clean, ts-rs no drift. Commit: <pending>.
- Concurrent forward slices (agents, 2026-07-01) — (a) ts-rs generation wired for
  the two new crates: `core-types/scripts/lib.mjs` now loops
  `cargo test -p <crate> --features ts` over cmux-core+cmux-config+cmux-agent-chat
  into one TS_RS_EXPORT_DIR and assembles the barrel from the file listing → 50
  generated types (was 11), incl. `AgentEvent` + config models; `bun run generate`/
  typecheck (both pkgs)/check-drift all clean. (b) Phase 3 pure slice:
  `cmux-agent-chat/src/claude.rs` — Claude stream-json accumulator + input framing,
  ported faithfully from `Sources/Panels/ClaudeStreamJSONAccumulator.swift` (+ the
  writeClaudeStreamJSON/handleProcessOutput call sites); `write_claude_stream_json`
  + `ClaudeStreamAccumulator` (consume_line/consume_line_to_events/
  completes_assistant_turn). One documented divergence: scalar- vs grapheme-count
  in full-message de-dup. cmux-agent-chat now 55 tests (+24), clippy clean; combined
  crates gate 62 tests green. Commit: <pending>. Next: codex + opencode transports,
  then process_store + the agent_session_rpc Tauri command.
- Phase 3 headless process-store slice (agent, 2026-07-01) — `cmux-agent-chat`
  gained `running_session.rs` (RunningSession + ProviderAccumulator enum; per-
  provider chunk routing mirroring handleOutputLine) + `process_store.rs`
  (ProcessStore<T,S> over an injected `AgentTransport` trait + `FnMut(AgentEvent)`
  sink; single-active-session invariant; start/select/writeLine/stop/close_all/
  feed_output/notify_exit; OpenCode handshake hook) + a `handle()` dispatcher for
  provider.list/select/start/writeLine/stop. provider.started timing matches
  canonical (`if provider != opencode` → immediate; opencode deferred to handshake).
  Tauri/OS/async-free (only new dep: uuid); 168 lib tests (+31, FakeTransport +
  Vec sink), clippy clean -D warnings, ts-rs no drift. Commit: <pending>. DEFERRED
  to the GUI-wiring slice: concrete async AgentTransport (tokio + cmux-process spawn
  + stdio pump, codex/opencode write side + backpressure, SIGKILL timer),
  agent_session_rpc Tauri command, app.context/pickFiles, webview agent-session mount.
- Phase 3 GUI-wiring slice — Claude agent session live end-to-end (agent,
  2026-07-01). Wired the reused `webviews/agent-session` React app to a concrete
  Windows host. NEW `apps/desktop/src-tauri/src/agent_session.rs`: a single-owner
  ACTOR thread owns `cmux-agent-chat::ProcessStore` (it is !Send — sink is FnMut)
  and drains one `mpsc<ActorMsg>` — `Rpc` (per-request oneshot reply), `Feed`
  (stdout/stderr chunk, empty=EOF), `Exit` — giving serial event ordering (macOS
  MainActor parity). NO tokio: `cmux-process` is blocking std (AgentIo →
  `mpsc::Receiver<AgentOutputChunk>` + stdin), so the transport = std threads +
  mpsc, mirroring `terminal.rs`. `ClaudeAgentTransport` impl AgentTransport:
  resolve via `cmux-agent` (`AgentExecutableResolver`→`to_spawn_spec`), spawn
  Job-Object-supervised via `cmux-process::spawn_captured`, per-session reader
  thread pushes framed lines (re-adds `\n`) → `feed_output`; on pipe disconnect
  sends both-stream EOF + Exit(0). `write_line` (Claude) = `write_claude_stream_json`
  → stdin; `terminate` = Graceful tree-kill. app.context (full 67-key English copy
  + canonical dark 14-field theme) + app.pickFiles stub serviced by the host;
  provider.* delegate to `handle()`. Command returns RAW `{ok,value|error}`
  envelope (invokeRaw path); sink emits tagged AgentEvent over `cmux://agent-event`.
  KEY WINDOWS FIX: `wrap_windows_shim` — `claude` resolves to `claude.cmd` (npm
  shim) which `CreateProcessW` can't run; rewrap `.cmd/.bat`→`%ComSpec% /C …`,
  `.ps1`→`powershell -File …` (agent exe still shown in provider.started).
  Canonical surface-type in the session model: `surface_kind: Option<String>` on
  `SessionPaneLayoutSnapshot` (skip-if-none → golden/Swift parity preserved) +
  pure `session_ops::set_surface_kind` (rides the pane node, survives splits) +
  `session_set_surface_kind` command; web `Workspace` branches TerminalSurface vs
  new `AgentSessionSurface` per pane via `paneRects::surfaceKinds`, PaneControls
  gains an agent toggle. Added `AgentIo::into_parts()` to cmux-process (split
  stdin/receiver for the reader thread). Gate GREEN: cmux-core + cmux-process(38,
  +into_parts) + cmux-agent-chat(168) + cmux-golden + cmux-desktop(28, +9 agent
  tests) all pass; clippy clean; core-types regen + drift clean; web typecheck +
  60 tests + vite build clean (reused app bundles, 66 modules). Commit: <pending>.
  LIVE CHECKPOINT: relaunch `cd apps/desktop/src-tauri && npx @tauri-apps/cli dev`,
  toggle a pane to agent (✦ control), select Claude, Start, converse. Next:
  Codex app-server handshake + OpenCode HTTP/SSE reuse this plumbing; app.pickFiles
  dialog; then backlog (#13 breakaway, #14 daemon go-test).
- Phase 3 GUI-wiring — post-build adversarial review + fixes (agent, 2026-07-01).
  13-agent review workflow (`wuiambv3t`) over the Claude slice → 3 REAL
  leak/robustness bugs, all FIXED (false positives — actor-hang, two-pane orphan,
  constant panelId — correctly rejected). (1) Natural-exit teardown: a
  self-exiting child (turn done / crash) left `ClaudeAgentTransport.sessions` +
  the supervisor's job/process HANDLEs orphaned — only the user Stop path reaped.
  Fix: `sessions` is now `Arc<Mutex<HashMap>>` shared with each reader thread,
  which reaps its own `LiveChild` (drops stdin) + supervisor handles on pipe-close
  via a new idempotent `JobObjectSupervisor::reap(id)` (removes the entry +
  CloseHandle both handles; DISTINCT from `terminate`, which stays idempotent so
  the existing tree-kill / active_process_count contract holds). (2)
  `wrap_windows_shim` emitted bare `powershell.exe`/`cmd.exe`; `CreateProcessW`
  does NOT PATH-search `lpApplicationName`, so `.ps1` (OpenCode) + the
  ComSpec-missing fallback would fail — now absolute `%SystemRoot%\System32\…`
  paths. (3) reader-thread-spawn failure now rolls back the confined child.
  cmux-process 39 tests (+`reap`), cmux-desktop green, clippy clean. Claude live
  path unaffected (its `.cmd` + full ComSpec already worked).
- Phase 3 GUI-wiring — LIVE verify + two fixes (agent, 2026-07-01). Ran the app;
  the reused agent-session UI mounts and Claude spawns/streams/converses. (1)
  STREAMING was all-at-once: the current `claude` CLI wraps partial deltas as
  `{"type":"stream_event","event":{"type":"content_block_delta",…}}`, but our
  accumulator matched only TOP-LEVEL `content_block_delta` → ignored every delta,
  emitted only the final full `assistant` message. FIXED in
  `cmux-agent-chat/src/claude.rs` (`unwrap_stream_event` unwraps the envelope in
  `consume_line` + `completes_assistant_turn`); verified against captured real CLI
  output (+2 tests: `stream_event_wrapped_deltas_stream_incrementally`,
  `wrapped_message_stop_completes_turn`); 27 claude tests green. (2) STYLING: the
  reused `agent-session/shared/styles.css` is a whole-document sheet
  (`html,body,#root{background:transparent}` + overflow/height) — importing it into
  the shared desktop shell blacked out the whole window (transparent root → WebView2
  black). Reverted the import; surface renders functional-but-unstyled. Proper fix =
  isolate the agent app in its own document (iframe/shadow root with styles + host
  shims scoped in) — the FIRST task next session (see ULTRACODE-RESUME OUTSTANDING).
  Also learned: stopping `tauri dev` orphans its Vite/app tree on Windows (port 1420
  stays held) — kill the node PID on 1420 before relaunch; tauri watches dep crates
  so Rust dep changes auto-rebuild.
- Phase 3 GUI-wiring — agent surface styling FIXED + live-confirmed (agent,
  2026-07-01). Importing the reused whole-document `agent-session/shared/styles.css`
  blacked out the shell (it sets `html,body,#root{background:transparent}` → the
  WebView2 window's black showed through the shared root). FIX: keep the import (for
  the styled look) in `AgentSessionSurface.tsx` + guard the shell root in
  `apps/desktop/web/src/styles.css` (`html,body,#root{background:#0b0e14!important}`)
  so the later-loaded agent sheet can only style the agent surface, not clobber the
  shared root. User confirmed live: the agent chat UI now renders properly styled
  (terminal-like) and not black. Long-term-robust alternative (iframe/shadow-root
  document isolation) documented in ULTRACODE-RESUME as optional polish. Claude
  vertical now spawns + streams (token-by-token) + converses + Stops + renders
  styled — the Phase 3 Claude slice is functionally complete pending only polish.
- Phase 3 — Codex + OpenCode transports (ultracode, 2026-07-01). Built the
  read→write feedback loop both providers need, via a NEW pure `TransportAction`
  intent list: the store (cmux-agent-chat) appends `WriteStdin`/`Terminate`/
  `OpenCodeCreateSession`/`OpenCodePostPrompt`; the src-tauri actor drains them
  (`take_transport_actions`) after every message and performs the I/O. Trait +
  Claude path UNCHANGED; crate stays dependency-pure + os-4551-safe. CODEX (dep-
  free): `RunningSession::handle_codex_line` reacts after `consume_line` (init→
  `initialized`+`thread/start`→queue-drain→approval-reply→startup-fail, in order);
  single-input queue (`codex_queue`, cap 1); `codex_submit` guard order; `start`
  writes `initialize`; `parse_server_request` (raw id echoed) added to codex.rs.
  OPENCODE: `ureq` (blocking, no tokio, no-TLS) in new `opencode_http.rs`
  (`build_url`+percent-encode, `post_json`, `create_session`, `post_prompt`, SSE
  `stream_events`); actor worker threads + 4 new `ActorMsg` variants; per-session
  `OpenCodeContext` (auth from `spec.env`, `cancelled`+`process_running` flags);
  `provider.started` deferred to `complete_opencode_handshake`; EOF-vs-error rule;
  optimistic-writeLine divergence documented. Understand→design workflow (6
  agents) settled the architecture; 4-lens adversarial review (9 agents) → 4 LOW
  findings, 2 real leaks FIXED (spawn-rollback + natural-exit context cleanup), 2
  documented (inherited EOF race + intentional null-id). Gate GREEN: cmux-agent-
  chat 185 (+17), cmux-desktop 36 (+8), full workspace tests pass, clippy clean
  workspace-wide. Also FIXED (live-run feedback): a spawn/resolve failure now maps
  to a new `BridgeError::ProviderLaunchFailed(detail)` ("<Provider> could not be
  started. <reason>") instead of the misleading `providerNotReady` ("The provider
  is not ready yet.") — mirrors the macOS `AgentExecutableResolverError` envelope
  (`{userMessage: error.message}`, no code). Surfaced because Codex/OpenCode CLIs
  are not installed on this box (only `claude` is), so their Start correctly fails
  now with a clear reason. KNOWN LIMITATION (not a regression): one agent session
  per WINDOW — the singleton `cmuxAgentBridge` + single-active-session `ProcessStore`
  reject a second pane's Start with `sessionAlreadyRunning`; macOS supports one per
  pane. Lifting it needs per-pane bridge routing + a multi-session store (future
  slice). Commit: <pending>. NEXT: install codex/opencode to live-verify their
  converse; multi-session-per-window; `app.pickFiles`; backlog (#13/#14).

## 2026-07-02 — Phase-4 headless core: cmux-markdown + dialSocket + cmux-diff restore (3 concurrent lanes)

`/loop ultracode`. Scouted 4 candidate headless lanes with a read-only workflow
(4 Explore agents → implementation-ready specs w/ canonical Swift file:line
refs), partitioned into provably-disjoint file sets, then ran the two disjoint
lanes as a background implement→adversarial-verify workflow WHILE hand-building
the markdown core (which shares its crate's lib.rs/Cargo.toml with the deferred
typography lane, so those two were sequenced). Zero file overwrite. Three commits:

- `92cffcf56` **cmux-markdown** (NEW crate, hand-ported by me): five pure modules
  — file_link (MarkdownPanelFileLinkResolver), local_image_jail
  (cmux-local-image:// path jail, security-critical, trailing-sep prefix +
  canonicalize like the diff session jail), theme (MarkdownWebTheme + WCAG
  luminance/contrast + 18-iter binary-search overlay + applyTheme 6-token map),
  assets (MarkdownViewerAssets: 6 {{token}} shell.html subs, deflate-preferred
  loader, lazy cache, 10-key localizedStringsJSON), typography (font-size/
  max-width/font-family clamp+zoom+css-escape + defaults orchestration, cross-
  crate default-sync test vs cmux_config::MarkdownConfig). 52 tests, clippy clean.
  Windows path/URL adaptations documented inline. Excluded (needs GUI): CoreText
  font enumeration, UserDefaults persistence, live renderer wiring.
- `7e34981e6` **cmux-diff manifest+pool** (workflow lane B, verify SOLID): the
  deferred manifest session-restore (session.rs now keeps raw trusted_root +
  register_from_manifest + has_active_session/registered_file manifest fallbacks,
  reusing register()'s full jail) + DiffCommentSubmissionPool (standalone, NOT
  wired into rpc.rs — dispatch has no workspace id). No 1024 cap on the manifest
  path (cap is RPC-ingest-only). 55 tests (+20), clippy clean.
- `c507355ee` **dialSocket Windows refused fix** (workflow lane A, verify SOLID):
  isConnectionRefused now matches the refused errno via a platform-split
  isRefusedErrno helper + retained substring fallback; 2 gated tests relocated to
  cross-platform cli_test.go, pass on Windows.

KEY SWARM WIN (adversarial verify earned its keep): the dialSocket implement
agent EMPIRICALLY DISPROVED the scout's prescribed fix — `errors.Is(err,
syscall.ECONNREFUSED)` does NOT fire on Windows/Go 1.26.4 (syscall.ECONNREFUSED
is a synthetic 0x20000016, not the real winsock 10061 in the error chain); only
`golang.org/x/sys/windows.WSAECONNREFUSED` matches. The "obvious" one-line fix
would have silently no-op'd. Lesson: platform errno identity is not portable —
verify the predicate actually fires on the target OS, don't trust errors.Is
across GOOS.

Gate: cmux-markdown 52 + cmux-diff 55 tests, clippy clean on both; go build+vet
clean; workspace check green. NEXT: the Tauri wiring slice (diff/markdown
commands + register_asynchronous_uri_scheme_protocol handlers + generate_handler!
+ tauri.conf.json bundle.resources) then the live-WebView2 token-gate spike — the
remaining Phase-4 pieces are mount-DEPENDENT so they need the running app.

## 2026-07-02 (iteration 2) — remote-image SSRF gate + mention-link + config sections + main.go errno

`/loop ultracode` continued. main.go:784 had the SAME latent Windows errno bug as
dialSocket (bare errors.Is(err, syscall.ECONNREFUSED) never fires on Windows) —
fixed via the isRefusedErrno helper + a cross-platform regression test that forces
a real refused connect. Then scouted 3 more headless lanes (read-only workflow);
built the two disjoint ones — remote_image myself, config-sections as a concurrent
background implement→verify workflow (SOLID) — plus mention_link. Commits:

- `af6ff20cb` main.go refused-dial errno fix + TestShouldRemovePersistentSocketAfterRefusedDial.
- `b584fbf05` **cmux-markdown/remote_image.rs** — the remote-image SSRF security
  gate (port of MarkdownRemoteImageSecurity): cmux-remote-image:// scheme gate,
  HTTPS-only/no-userinfo/default-port, IPv4+IPv6 private/reserved-range blocklists
  (incl. v4-mapped delegation) + is_allowed_resolved_ip for the DNS layer, MIME
  allowlist (incl svg+xml), HTTP request framing, header-injection guard. DIVERGENCE
  (documented + tested): WHATWG `url` crate parsing is STRICTER than Swift inet_pton
  — classifies decimal/hex IPv4 forms (https://2130706433/) as literals, closing an
  SSRF bypass. DNS + TLS fetch stay in host layer. 72 tests.
- `ed31721bd` **cmux-markdown/mention_link.rs** — TextBoxMentionMarkdown port (label
  escaping order + path angle-wrap/percent-encode); canonical golden test verbatim.
  79 tests total for cmux-markdown (7 modules now).
- `bede7b191` **cmux-config** vault + workspaceGroups + newWorkspaceCommand typed
  serde structs (untagged sessionIdSource, VaultAgent flatten, reused
  NewWorkspacePlacement). 9/49(ts) tests.

Gate ALL GREEN: cmux-markdown 79 + cmux-config 9(+49 ts) + cmux-diff 55 tests,
clippy clean, cargo check --workspace clean (incl cmux-desktop), go build+vet+test
clean. NOTE crates/cmux-config/bindings/ is gitignored ts-rs output (not committed).
NEXT: the mount-DEPENDENT Tauri layer (needs `tauri dev`) OR more headless ports
(remote-image chunked-body decoder + redirect decision; more cmux-config sections).
- Fidelity fix-pass + mentions port (ultracode, 10-agent workflow: 5 implement +
  5 adversarial verify) — applied all queued verify findings across cmux-config
  (duplicate-id/blank-key/trim-collision decode errors, contextMenu-over-
  rightClick lazy ??, blank newWorkspaceCommand, lazy agent key),
  cmux-ssh (Swift split(maxSplits:) whitespace-only 5th field FIXED, grapheme
  DIVERGENCE anchor), cmux-tmux (verbatim budget-oracle recovery tests,
  explicit-null decodeIfPresent parity, encodeIfPresent None omission, snapshot
  decode re-normalization), cmux-markdown (verbatim chunked oracle incl.
  Int64.max size line, swift_split size-token fix, NBSP transfer-encoding trim);
  NEW crate cmux-mentions — pure TextBoxMention* family (detector on UTF-16
  NSRange semantics, candidate index + verbatim CmuxCommandPalette fuzzy/engine
  subset, index-store pure half, all Swift oracle tests). 354 tests across the
  five crates, clippy -D warnings clean. Commits: `46b8057d9` + `04c3c1329`.
  Next: the Tauri mount layer (diff/markdown/schemes commands + URI-scheme
  handlers) — mount-DEPENDENT, needs `tauri dev` + user UI verification. Loop
  stop condition reached.

## 2026-07-03 — 4 concurrent headless lanes (shortcut-model + cmux-resume + command-palette + phase4-tauri)

`/loop ultracode` (milestone-loop). Read-only scout workflow (6 Explore agents +
partitioner, `w2nc6u9ny`) DISPROVED the stale "headless frontier exhausted" note:
found 4 disjoint headless lanes (Go-daemon + cmux-config lanes correctly excluded
as already-complete). Pre-seeded root Cargo.toml + both new-crate skeletons in one
commit so all 4 lanes ran fully parallel on provably-disjoint file sets, then a
4-lane implement→adversarial-verify workflow (`wrfs1rp17`, 8 high-effort agents) +
a parity-guarded simplify workflow (`wgvo8sinq`, 4 agents, zero changes — code
already clean). Five commits:

- `5caab83d1` scaffold: seed cmux-resume + cmux-command-palette crates (root
  Cargo.toml members + skeleton Cargo.toml/lib.rs) so lanes never touch a shared file.
- `f4283883d` **cmux-core shortcut model** — config-string codec + display
  formatting + Action metadata (all 109 default_shortcut entries incl. g-g chord +
  .unbound) + conflict detection + recorder normalization, reconciling the two
  drifted Swift enums. 101 tests. Verify: SOLID (0 divergences).
- `d0105356f` **cmux-resume** (NEW crate) — SurfaceResume approval subsystem:
  shell-token lexer + HMAC-SHA256 signing + longest-prefix matching + 4-branch
  trust decisions. Golden signing-payload byte-parity pinned. 59 lib tests.
  Verify: SOLID (byte-for-byte payload layout verified vs Swift).
- `fc7692036` **cmux-command-palette** (NEW crate) — pure palette model + search
  orchestrator, reuses cmux-mentions palette engine; oracle differential test.
  22 lib + 17 oracle tests. Verify: SOLID.
- `92b69f1de` **desktop Phase-4 mount layer** — diff_comments_rpc + cmux_lib_rpc +
  4 custom URI-scheme handlers over the headless cmux-diff/cmux-markdown cores.
  56 lib tests, cargo check + clippy clean. Verify: FIXED one real security-boundary
  bug (resolve_diff_request now rejects ?query/#fragment, matching Swift
  registeredFile(for:) BrowserPanel.swift:2013-2019).

Cross-cutting gate GREEN: cargo check --workspace clean, clippy --workspace
--all-targets clean, no ts-rs drift (new shortcut types not TS-exported). KEY SWARM
WIN: adversarial verify caught the phase4 query/fragment SSRF-adjacent divergence
that the implement agent had silently introduced by reusing the lenient token-gate
parser for file serving. ⇒ UI-TEST CHECKPOINT reached for Phase-4 (see
docs/windows-port/UI-TEST-QUEUE.md): the mount layer compiles + is unit-tested but
live WebView2 behavior (iframe token via webview.url(), eval delivery, custom
hyphenated schemes serving bytes, bundle.resources at runtime resource_dir) needs
the running app. Loop CONTINUES on remaining headless work; UI items queued for the
morning. NEXT: re-scout for the next disjoint headless batch (Phase-5 chrome logic,
more pure Swift subsystems) or confirm frontier exhausted.
