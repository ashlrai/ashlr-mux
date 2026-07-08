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

## 2026-07-03 (batch 2) — 7 more headless crates (transcript + workspaces + notifications + appearance + sidebar-args + git + browser-history)

`/loop ultracode` continued. Scout-2 (5 Explore agents + partitioner, `wgrjddlr0`)
DISPROVED the "frontier exhausted" note a second time — found 7 disjoint oracle-backed
headless lanes. Pre-seeded 5 new-crate members in root Cargo.toml (`b8eea0883`), then
a 7-lane implement→adversarial-verify workflow. The session usage limit (resets 6am ET)
cut the first run off mid-flight: 3 lanes finished green and were salvaged/committed
(`1d218327b`: sidebar-args 10 / git 13 / browser-history 7 tests); the other 4 had
partial writes. After the limit reset, a scoped re-run (`w04t8yz9x`, treating the
partial files as drafts) finished all 4 SOLID:

- `55f55c143` **cmux-agent-chat transcript/** — on-disk Claude/Codex session-JSONL
  history parser (model + json_value + text_budget + timestamp + diff_builder +
  tool_completion + parse_state + batch_assembler + claude/codex parsers), DISTINCT
  from the live provider transports. 262 tests incl. fixture replay.
- `bdb080999` **cmux-workspaces** (NEW) — pure workspace/tab/group ordering +
  group-invariants + batch-reorder + sidebar render projection + selection-sync +
  placement + closed-item history (session-restore edge cases). 59 tests.
- `df3d35740` **cmux-core notification sub-models** — the six pure models behind the
  delivery seam (superseded buffer, dismiss-tombstone ring, authorization, menu
  snapshot, sidebar-unread, UserDefaults gates). cmux-core now 142 tests.
- `690450971` **cmux-appearance** (NEW) — appearance-mode + color-scheme + theme-name
  selection codec resolution layer. 29 tests.

Gate GREEN: cargo check --workspace + clippy --workspace --all-targets clean, no
ts-rs drift (no new TS-exported types). All 4 verify verdicts SOLID (zero bugs).
KEY OPERATIONAL LESSON: the session-usage limit is a hard mid-workflow failure mode —
committing each lane the moment it is green (not batching the commit to end-of-wave)
is what saved the 3 completed lanes; the pre-seed's disjoint file sets made the
partial-4 trivially resumable as drafts. NET for the two batches: 7 new crates +
2 major cmux-core model ports (~450 new tests), all faithful Swift ports. The
remaining mainline is now genuinely host-wiring / UI-checkpoint work (Phase-4 mount
live-verify, renderer, window chrome, live agents, packaging). NEXT clean break =
the Phase-4 UI test (docs/windows-port/UI-TEST-QUEUE.md).

## 2026-07-03 (batch 3) — cmux-canvas + ts-rs codegen + 5 headless-verifiable web lanes

`/loop ultracode` continued after a course-correction: the loop had stopped early by
mislabeling the Phase-4 mount layer as a "UI checkpoint" when its frontend was never
wired — nothing built so far actually needed the UI (all test-verified backend). So
the scout scope was WIDENED to include web/frontend slices that are headless-verifiable
via `bun test src` + `bun run typecheck` (SSR render tests + pure reducers), with the
genuine live-WebView2 tail explicitly BATCHED for one future UI session (see
UI-TEST-QUEUE.md) rather than stopping the loop. Ran a scout (`w1kubd0f7`) + a
concurrent read-only red-team (`wcc48ubdb`, 8 agents) + a 7-lane implement→verify wave
(`wp3pfjp01`, 14 agents) to keep capacity saturated. 10 commits (scaffold + 8 lanes + docs):

- `c4546902a` **cmux-canvas** (NEW) — pure free-canvas geometry/layout/snap/placer/
  aligner/spatial-nav/viewport (~1400 LOC, 66 oracle @Test cases 1:1). 67 tests. SOLID.
- `70010b1f6` **timestamp fail-open fix** — the red-team's ONE finding (7/8 targets
  clean): parse_iso8601 didn't bound the year, so a corrupt 6+ digit year overflowed
  base_seconds*1000 (panic/wrap, breaking fail-open). Year bound 0..=9999 + checked_*
  arithmetic + regression test.
- `0c10f243f` **core-types ts-rs** — added cmux-diff to the generation loop (typed
  DiffComment; ts(type=number) so start/end_line emit number not bigint), which also
  resynced ~30 previously-drifted cmux-config binding types. check-drift + tsc clean.
- `b1798d23c` **web split-geometry** — pure equalize/keyboard-resize planners; FIXED a
  real bug (equalizeDivider weighted by countLeaves, should be orientation-aware
  spanCount; corrected the test that asserted the wrong 2/3->1/2) + activeLayout extract.
- `9308d7659` **web settings**, `4e12f8b02` **web sidebar**, `c199a03d2` **web
  command-palette** (verify FIXED a wrap->clamp cursor bug + its mis-asserting tests),
  `1cf20b9d0` **web diff/markdown surface routing scaffold** — Phase-5 chrome + Phase-4
  surface wiring, all greenfield/disjoint, driven by the already-ported Rust models.

Gate GREEN: cargo check --workspace + clippy clean; cmux-canvas 67 / cmux-agent-chat
263 / cmux-diff 55; core-types check-drift clean; web `bun test src` 185 pass / 0 fail
+ tsc --noEmit clean. Two workflows found+fixed 3 REAL bugs (timestamp overflow,
palette wrap, equalize spanCount) — the adversarial passes keep earning their keep.
PUSHED to fork/windows-port. Backend pure-oracle frontier is now THINNING (remaining
candidates are Bonsplit-blocked / Ghostty-coupled / Windows-updater-caveated); the bulk
of what's left is the batched live-WebView2 UI session + genuinely UI-coupled wiring.

## 2026-07-03 — 7-crate headless batch (agent-launch/sync/jsonc/guardrail/settings) + red-team fixes

Resumed after the /ashlr:loop detour (that slash-command runs the ashlr FLEET CONDUCTOR, not
this coding loop — its inbox is 1229 junk test-fixtures; the real coding loop is workflow-driven).

Scout (wbhju2idd, 4 agents) DISPROVED the "frontier exhausted" note AGAIN: 7 disjoint NEW-crate
lanes, all Foundation-only. Pre-seeded 7 skeletons + root Cargo.toml, `cargo check` green, then an
implement->verify->fix wave. Session-limit killed the first wave mid-run; resumed with credits and
gated the on-disk state: 6 crates had been written but several were ORPHANED (module files present,
but lib.rs left a 1-line placeholder with NO `mod` decls, so 0 tests compiled/ran), and sanitizer
was still a bare placeholder. Finish workflow (w8tju0dvt, 10 agents) implemented sanitizer + wired
hook-config/sync-protocol/jsonc + adversarially verified all six. LANDED (cargo test + clippy -D
warnings green), commit 92828014e:
  cmux-agent-launch-sanitizer 60, cmux-agent-resume-argv 40, cmux-agent-hook-config 48,
  cmux-sync-protocol 59, cmux-jsonc 36, cmux-pane-guardrail 21, cmux-settings-search 45  (~309 tests).

Red-team (wbx2dzmny, 10 agents) over batch-3 code -> 5 CONFIRMED LOW findings; fixes in commit
b1fac5203: cmux-diff UUID canonical-form (delete+save gate on 36-char hyphenated form like Swift
UUID(uuidString:)), web equalizeDivider clamp drop (Swift emits unclamped span ratio), web listScope
Swift whitespacesAndNewlines trim (U+0085 yes, U+FEFF no). One finding correctly ruled FALSE-POSITIVE
under the parity mandate (claude emitted_any_assistant_text is NEVER reset in the canonical Swift
accumulator either -> the Rust is faithful; a "fix" would DIVERGE). jsonc CRLF-split parity fix also
landed (grapheme-aware value split so a \r\n is not split on its interior LF; +2 tests).

Pushed 8cd2c9f7b..b1fac5203 -> fork/windows-port. HEAD b1fac5203. Working tree clean.

OPEN (next session, do first):
 (1) sync-protocol apply()-return-value parity nit — the parity-nits workflow (wnce7kg9m) was STOPPED
     mid-run for a session pause (I finished only its bool->double deref). Re-read Swift
     SyncFrameApplier.applyDeltaFrame and make apply() return Swift-faithful for stale/duplicate-rev
     deltas (Swift may return true unconditionally once it reaches store.applyDelta), or confirm the
     current false is faithful and delete the misleading doc-comment. LOW.
 (2) /simplify was SKIPPED this batch (wrap-up under a session pause) — run it on the 7 new crates +
     re-test before the next scout.
 (3) sanitizer grapheme-vs-scalar codex-session-id doc-note (LOW, unreachable ASCII domain) may not
     have landed (that lane was in the stopped wnce7kg9m) — verify/add.

FRONTIER STILL NOT EXHAUSTED — partitioner flagged ~8 more headless lanes for the next scout. These
are single-crate-lib lanes (run at most ONE per crate concurrently to stay write-disjoint):
cmux-agent capture_trust / spawn_identity / feed_event / hook_payload; cmux-ssh
reconnect_input_filter; cmux-terminal top_label; cmux-workspaces tab_colors. Lower-confidence:
cmux-remote-shell-commands (couples to FileManager + unported SSH deps). Web-slice ports:
shortcutFormat / placement / reorder / switcherIndex (share `bun test src`; sequence them).

- Frontier batch-4 (5 headless lanes) — ported into existing crates + web via a
  scout→implement→red-team→fix ultracode workflow (Fable), the 2 red-team lanes
  that hit the Fable limit re-run on Opus 4.8. Landed: cmux-agent `capture_trust`
  + `spawn_identity` (47 tests); cmux-ssh `reconnect_input_filter` (66); cmux-
  terminal `top_label` (113, +1 red-team fix: tab_sync/tmux_rename both-or-neither
  gate); cmux-workspaces `tab_colors` (88); web `shortcutFormat`/`placement`/
  `reorder`/`switcherIndex` (308, +2 red-team fixes: switcherIndex ICU `\s` class
  dropped stray VT+NEL so interior whitespace matches Swift; reorder
  `batchReorderFinalIds` now traps on duplicate current ids per Swift
  `Dictionary(uniqueKeysWithValues:)`). All clippy -D clean. Commit `c3c1ef212`.
  Owed /simplify from the PRIOR 7-crate batch also cleared first: 11 verified
  cleanups, commit `0262b398c` (pushed). SKIPPED (need serde/serde_json the crate
  lacks — flagged, not silently dep-added): cmux-agent `feed_event` +
  `hook_payload` (Workstream event/payload pure core). Next: run the frontier-4
  /simplify (in flight), then either land the Workstream lanes by adding
  serde.workspace deps to cmux-agent (or a new cmux-agent-workstream crate), OR
  re-scout the remaining headless frontier.

- Workstream port (cmux-agent feed_event + hook_payload) — the deferred serde
  lanes. Added serde/serde_json to cmux-agent only (root already declares them).
  5 new modules: workstream_json (AnyJSON over serde_json::Value), _source,
  _context, feed_event (WorkstreamEvent + HookEventName), hook_payload
  (Payload/Status/Decision/Item + pure event->item mapping). scout→implement→
  red-team→fix workflow; red-team hardened decode to throw typeMismatch on
  present wrong-typed optionals (Swift decodeIfPresent). Adjudicated the one
  execution-unverifiable finding: WorkstreamDecision is synthesized Codable so
  nil exitPlan feedback emits explicit "feedback":null (synthesized enum coders
  skip struct encodeIfPresent; corroborated by the sibling WorkstreamPayload
  hand-writing encodeIfPresent) — dropped skip_serializing_if, pinned a test.
  87 tests, clippy clean. Commit `632a8d649`. /simplify: 3 cleanups (dead
  telemetry branch, 2 allocs) commit `9fd558c3b`. Both pushed. Store
  I/O/actor/persistence/redaction remain out of scope (need the app). NEXT:
  re-scout the headless frontier for the next batch.

- Frontier batch-5 (7 of 8 headless lanes) — scout (5 area surveyors -> ranked
  8-wide write-disjoint backlog) then implement->red-team->fix (Opus, high
  effort). Landed + committed `f76d33a01` (pushed): cmux-git `git_index` (DIRC
  v2/v3/v4 + FNV-1a, 27), cmux-scrub `noise_filter` (errno classifier; Unicode \s
  to match ICU [:space:] + NBSP pin, 56), NEW crate cmux-window-title
  (WindowTitleTemplate; red-team fix grapheme-cluster iteration, 10), cmux-config
  `right_sidebar_width` (clamp/round, 36), cmux-agent `auto_naming_agent_catalog`
  (summarizer decision matrix, 98), NEW crate cmux-panes (ExternalTreeNode +
  spatial order, 7), cmux-workspaces `focus_history` (back/forward stack, 103).
  3 red-team findings (1 fixed grapheme, 1 hardened NBSP, 1 ruled non-divergent).
  /simplify (commit pending): 2 cleanups (window-title trim, scrub doc). 8th lane
  cmux-ssh-url (L) FAILED (agent derailed, produced nothing) — re-running solo.
  Frontier NOT exhausted: full 2nd wave queued (per-crate, sequenced). NEXT: land
  ssh-url, then start the 2nd wave.

- Frontier wave-2 (8 lanes, 4 write-disjoint crate-tracks, sibling lanes
  sequenced) — scout (9 slugs -> concrete specs; chat-ansi-sanitizer dropped:
  already ported + no Swift oracle) then implement->red-team->fix (Opus, high
  effort). Landed + committed per-track (pushed): cmux-config
  `5da106ee4` notification_hooks (resolveNotificationHooks + ActionTrust SHA-256
  fingerprint) + json_path (80 tests); cmux-git `c776ba6a3` pr_selection +
  repo_resolution (55); cmux-panes `0cfcdc6d2` tmux_overlay + surface_map (28);
  cmux-workspaces `418ec597b` surface_list + session_restore_policy (150). 2
  confirmed red-team fixes: (1) notification fingerprint now escapes '/' as '\/'
  to match Foundation JSONEncoder(.sortedKeys) — was breaking the trusted-actions
  auth contract byte-identity; (2) git RFC3339 parse now does Foundation ISO8601
  calendar validation (4-digit year, no leap-second, leap-year days_in_month).
  sha2 added to cmux-config. /simplify in flight. NEXT: land the wave-2 simplify,
  then re-scout the frontier (wave-3) or pivot to the MVP GUI-mount work.

- Frontier wave-3 (10 of 11 lanes, 9 crate-tracks) - scout reported frontier
  THINNING (1 candidate already-ported collision; 0/5 areas self-exhausted but
  tail is medium/low value). implement->red-team->fix (Opus, high effort), landed
  + committed per-track (pushed fe7ebd1b8..a77d2d354): cmux-appearance color_math
  (WCAG/sRGB, 52), cmux-workspaces mount_plan + avatar djb2 slots (181),
  cmux-canvas minimap (78), cmux-command-palette window_store (40), cmux-terminal
  sanitize (134), cmux-browser-history session_history (33), cmux-panes
  sidebar_drop (61), cmux-ssh ssh_batch (126), NEW crate cmux-notifications
  delivery (21). 1 CONFIRMED red-team fix: cmux-terminal sanitizer returned a
  BORROWED text[index..] slice that PANICS when index lands mid-scalar (SS3 /
  single-char-escape consuming a UTF-8 lead byte) -> from_utf8_lossy (Swift
  String(decoding:) parity) + 3 panic-repro pins. mention-candidate lane was a
  NO-OP (already ported in index_store.rs 04c3c1329 - a thinning signal;
  implementer correctly refused to duplicate). /simplify in flight. FRONTIER NOW
  GENUINELY THIN per scout - next survey (wave-4) should be tightly scoped and
  expect mostly-empty; remaining port work is the GUI/GPU-mount frontier that
  needs the running app (HARD BOUNDARY, not headless-verifiable). NEXT: land
  wave-3 simplify, run a final tight wave-4 confirmation scout, then likely
  declare the headless harvest complete for this milestone.

- Frontier wave-4 (final 3 lanes) - the wave-4 CONFIRMATION scout returned
  "nearly exhausted": only 3 genuine oracle-backed lanes survived, all
  write-disjoint. implement->red-team->fix (Opus, high effort), 0 confirmed
  red-team findings this round, landed + committed (pushed a61d2f1ad..768beb077):
  cmux-terminal `9ec41be59` copy_mode (vim-style scrollback key-resolution state
  machine, 10 CopyMode/*.swift files, full oracle incl. Hangul fallback +
  caps-lock invariance, 166 tests); cmux-agent-launch-sanitizer `cea27411d`
  hermes_codex_config (TOML/URL derivation filling the app-target closure seam,
  70); cmux-agent `768beb077` prompt_extraction (WorkstreamEvent prompt/assistant
  accessors + grapheme-truncated preview, 112). /simplify in flight.
  === HEADLESS FRONTIER EXHAUSTED === After 5 port waves (batch-4, workstream,
  batch-5, wave-2, wave-3, wave-4) the pure-logic surface is harvested. Remaining
  Windows-port work is the GUI/GPU/transport-MOUNT frontier (HARD BOUNDARY, needs
  the running app, NOT headless-verifiable): Tauri command wiring over the
  headless cores (diff/markdown/schemes/notifications/copy-mode/etc), WebView2
  surface mount, live agent-transport verification, window chrome/HWND/focus/IME,
  wgpu renderer, packaging/signing. The autonomous headless loop has reached its
  natural boundary. See DECISIONS.md "Headless frontier exhaustion".

- Re-invocation re-audit (2026-07-04) - loop re-fired after the exhaustion call;
  did NOT reflexively re-stop. Re-derived state from disk (tree clean, HEAD
  197d62281 == pushed) and took the fresh look the mount frontier deserved:
  read apps/desktop/src-tauri/src/lib.rs (generate_handler! + 4 uri schemes) and
  the web bridge apps/desktop/web/src/tauri-bridge.ts. FINDING: the web->native
  command surface is small, closed, and FULLY wired (ping, desktop_core_status,
  terminal_*, session_*, agent_session_rpc, diff_comments_rpc, markdown_* +
  cmux-diff-viewer/cmux-md/cmux-local-image/cmux-remote-image schemes). The
  apparent run_agent / connect gaps are test-only fixtures in
  tauri-bridge.test.ts, not real call sites. The shipped-but-unwired headless
  cores (cmux-notifications, copy_mode, panes/sidebar_drop, appearance/color_math,
  canvas/minimap, browser-history) have NO web caller yet - their UI isn't built -
  so wiring them now would be speculative, contract-less, parity-unsafe guesswork.
  VERDICT unchanged and reinforced: no parity-safe headless work remains; the
  mount-command layer is fully wired for what the web invokes today. Remaining
  work is UI-build + host-I/O seams (remote_image DNS-pinned TLS fetch) + live
  transports + window chrome/wgpu + packaging - all need the running app. Loop
  stops here; resume via the running app (npx @tauri-apps/cli dev).
- UI buildout #1 — live **workspace sidebar** for desktop-web (canonical cmux
  chrome parity). New `Sidebar.tsx` + shell layout (`App.tsx`) + `useSession`
  workspace wiring + `session_new/select/close_workspace` Tauri commands.
  /simplify pass (4 agents): moved the workspace lifecycle into
  `cmux_core::session_ops` (append/select/close on `SessionTabManagerSnapshot`,
  canonical `guard count>1` no-op — dropped the "replace-last-with-fresh"
  divergence); reused `fresh_terminal_workspace` (3 sites) + `.cmux-icon svg`
  (dropped 2 duplicated stroke blocks); collapsed `workspaceTitle`. Tested
  (cmux-core session_ops 22, cmux-desktop 21, web 308, clippy clean, tsc clean)
  → simplify → retested green. Next: hide ✕ on sole workspace, then next parity
  slice (workspace rename / tab strip / command-palette host).
  NOTE: git push blocked this session. origin (manaflow-ai/cmux) denies MasonStation (403); auto-mode classifier blocks the fork remote (ashlrai/ashlr-mux) as it was not named at session start. Local commits are the durable checkpoint; user can push via `! git push fork windows-port`.
- UI buildout #2 — parity MAP (7-agent workflow wj6yyvzuk) -> BACKLOG.md: 7 areas, ~60 slices, first parallel wave of ~11 disjoint-file slices; key finding = frontier is mostly WIRING already-ported logic. Then shipped A2: hide the per-row close on the sole workspace (canonical TabManager.closeWorkspace no-op parity), refactoring Sidebar into presentational SidebarView + thin container to make it testable; +Sidebar.test.tsx (5 tests). Tested (web 313) -> /simplify (softened an overstated WorkspaceList-parity comment; noted a cross-file test-helper dup for later) -> retested green. Next: build an entrypoint layer (command palette D1-D4 / shortcuts) that unblocks the C1-C4 split/canvas action slices, or A1 data-model (group_id/is_pinned) to unblock the sidebar richness chain.
- UI buildout #3 — D1: ported the command-palette per-window state machine `crates/cmux-command-palette/src/window_store.rs` -> `apps/desktop/web/src/palette/windowStore.ts` (visibility, pending-open expiry [8s max / 1.25s grace], escape suppression [0.35s], selection clamp, debug snapshot; `now` injected, pure). 18 tests ported 1:1 from the Rust suite. Tested (web 331, tsc clean) -> /simplify (2-agent quality pass: faithful clean port, zero fixes; noted paletteSelection is the selection SoT for D4) -> green. Unblocks D-area command-palette wiring (D2 search bridge, D3 query hook, D4 live overlay). Next: D3 query/scope hook or D2 Tauri search bridge.
- UI buildout #4 — D3 (pure model): `apps/desktop/web/src/palette/paletteQuery.ts` composes listScope + paletteSelection into the palette query-driven state — scope/matching-query derivation, cursor re-anchor on query change (queryChanged) vs clamp on results change (resultsChanged), scope-flip reset decision. +6 tests. Tested (web 337, tsc clean) -> /simplify (clean composition, zero fixes; added a denormalized-cache doc note; recorded a D4 in-flight-reset caveat on the backlog) -> green. The thin React hook lands with D4 (live overlay). Next: D2 Tauri search bridge (add cmux-command-palette + cmux-mentions deps to src-tauri) or D5 command catalog.
- UI buildout #5 — D2: command-palette search bridge. Added cmux-command-palette + cmux-mentions deps to apps/desktop/src-tauri; new `command_palette.rs` `command_palette_search` Tauri command over `CommandPaletteSearchOrchestrator::preview_search_matches` (pure `run_search` + thin wrapper; scoring/history-boost stays in the orchestrator). 6 Rust tests (commands/switcher scope, candidate restriction, limit-0 short-circuit, ascending highlight indices). Tested (cmux-desktop command_palette 6, clippy clean) -> /simplify (applied 3: dropped UsageEntryInput dup [reuse CommandPaletteUsageEntry], gated the heavy corpus_by_id clone to switcher scope only, take request by value to move fields) -> retested green. Web `host.invoke("command_palette_search")` wrapper lands with D4. Next: D5 command catalog (pure) or D6 switcher-entry producer, then D4 live overlay.
- UI buildout #6 — 2 disjoint headless lanes in ONE ultracode wave (`/loop ultracode`, workflow `wdom5lsoz`: scout→implement→adversarial-verify per lane, 6 agents; then simplify workflow `wd96m4hk0`, 2 agents). Lanes chosen file-disjoint on different toolchains (Rust vs bun) so zero build contention — the honest max parallelism for one iteration given the session/lib serialize zones + shared cargo lock.
  - **A1** (`23b8cfc29`) — `SessionWorkspaceSnapshot` gains `group_id: Option<String>` + `is_pinned: Option<bool>` (omit-when-none, `surface_kind` precedent). Regen core-types (no drift); closed the golden seam (`..Default::default()` on the two exhaustive `session_golden.rs` literals) so Swift-authored fixtures stay byte-identical. Verify SOLID vs `SessionPersistence.swift:1833-1834` (is_pinned modeled optional not bare-bool for byte-stability — flagged alternative if strict typing later needed). Unblocks Area-A A3-A14.
  - **D5+D6** (`2bf07ee48`) — pure `palette/commandCatalog.ts` (117 canonical contributions in declared order + one shared id→intent dispatch path; config override structurally gated to the 4 canonical configurable ids; runtime sub-lists injectable at exact Swift `contentsOf:` positions) + `palette/switcherEntries.ts` (single-window switcher producer, ContentView.swift:5249-5358). Adversarial verify FIXED-1: config override was applied beyond the 4 canonical ids (invented behavior) — now structurally gated. Simplify: 2 parity-safe cleanups (dead `undefined` branch; factor `switcherCorpus` onto shared `searchableTexts`).
  Gate GREEN: session_golden 5/5 byte-stable, cmux-core 152, clippy + `cargo check --workspace` clean, no ts-rs drift, web `bun test src` 373, typecheck 0. Pushed `1a25ef091..2bf07ee48` → fork/windows-port. NEXT: D4 (live overlay host — GUI-verify: mount + open-shortcut + focus + Escape + arrow/click/Enter, wiring windowStore/paletteQuery/commandCatalog/switcherEntries/command_palette_search together) is the natural next slice but is gui-verify; headless alternatives: A3 (`cmux-workspaces` dep + `render_items` projection, now unblocked by A1), or A9 (new-workspace placement via `placement.ts`), or C1/C2 (directional split + equalize, pure-module wiring).
- UI buildout #7 — 2 disjoint headless lanes (`/loop ultracode`, workflow `w15fas9pd`: scout→implement→adversarial-verify, 6 agents; simplify `wnkarz7ih`, 2 agents, zero changes — code already parity-minimal). Same Rust∥bun disjoint shape.
  - **A3** (`4a34e56dd`) — pure `src-tauri/src/sidebar_render.rs` `render_items(&SessionTabManagerSnapshot) -> Vec<SidebarWorkspaceRenderItem>` over golden-pinned `cmux_workspaces::render_items`; added cmux-workspaces + uuid deps + `mod` line (only lib.rs edit). Group anchor = canonical 3-tier restore fallback (TabManager.swift:6018-6027). `#[allow(dead_code)]` until A4. Verify FIXED-1: anchor resolution was anchor_workspace_id-only (dropped anchorless groups + leaked phantom-workspace headers) → corrected to index→stored-if-member→members[0], drop only member-less groups, de-dup keep-first. Deferred divergence (workspace_id None → skip vs canonical fresh-UUID mint): belongs in the stateful A4 restore layer, not this stateless projection. cmux-desktop 77, clippy clean.
  - **D7+D9** (`bd3fe5f2b`) — pure `palette/renderSequencing.ts` (RenderSequencingGuard, two independent seq/resultsVersion clocks, drops stale async batches; CommandPaletteOverlay.swift:46-56) + `palette/resultsGating.ts` (seed `<=256` / 5-input preserve AND / 3-branch show-empty; ContentView.swift:8407-8419 + orchestrator.rs:296-321). Params keyed by exact Swift arg names (anti-transposition); "results shown" is an explicit host input, never paletteSelection.count. Verify SOLID.
  Gate GREEN: cmux-desktop 77, clippy clean, web `bun test src` 391, typecheck 0. Pushed `0dbf5b6c2..bd3fe5f2b` → fork/windows-port. Area D pure-model layer (D1/D2/D3/D5/D6/D7/D9) is now COMPLETE — everything the live overlay composes is ported+tested. NEXT: D4 is the natural convergence but GUI-verify (needs `npx @tauri-apps/cli dev`) → flag for user, don't blind-build. Remaining HEADLESS: A9 (new-workspace placement via placement.ts → session_new_workspace), C1/C2 (directional split + equalize, pure-module wiring into session_split — touches session serialize zone, run solo), C3 (keyboard divider resize), E6/E9 (settings shortcut-format + appearance apply), G1/G5 (markdown doc feed + diff-comments shim), F5 (provider.select persistence). A4 (wire WorkspaceList via render_items) is headless-testable at the projection layer but the live mount is GUI.
- UI buildout #8 — 2 disjoint headless lanes (`/loop ultracode`, workflow `wa8iumt9p`: scout→implement→adversarial-verify, 6 agents; simplify `wtdbfd9ca`, 2 agents, zero changes). Rust∥bun disjoint.
  - **A9** (`b3217bade`) — `session_ops::new_workspace_with_placement(tabs, panel_id, placement)` over golden-pinned `cmux_workspaces::insertion_index` (TabManager.swift:1156/1340/1483-1506); 2-arg `new_workspace` kept as a wrapper delegating with `NewWorkspacePlacement::default()` (AfterCurrent) so the src-tauri caller stays untouched (host passes the effective placement to the `_with_placement` variant). Added cmux-workspaces dep to cmux-core. Verify SOLID. Two disclosed divergences (doc-commented): AfterCurrent-no-selection (proven unreachable in the single-list model) + group-contiguity normalization (Swift `normalizeWorkspaceGroupContiguity` post-insert — port consumes WorkspaceRow/WorkspaceGroup not snapshot types → FUTURE cross-crate slice; A9 places by flat index, new ws inherits no group_id). cmux-core 158, golden 9 byte-stable, clippy clean.
  - **E8** (`b3217bade`) — pure `settings/settingsSearch.ts` `settingsEntriesMatching(query)` over a 132-entry corpus (16 sections + 116 settings). Byte-faithful TS re-port of the Rust crate cmux-settings-search (index/aliases/entry/target) + corpus transcribed from SettingsNavigation.swift:304-590 (the crate does NOT port the corpus). Scalar-domain semantics (codepoint iteration, curated diacritic strip not NFD, exact ASCII delimiter set with +/= kept in tokens), score-tier ladder, load-bearing Swift row order (offset breaks score ties). Pure/headless; SettingsPane search-box wiring is a later slice. Verify SOLID (re-derived row-by-row vs 4 oracles).
  Gate GREEN: cmux-core 158, golden 9 byte-stable, clippy clean, web `bun test src` 444, typecheck 0. Pushed `11c935166..b3217bade` → fork/windows-port. NEXT headless (all still open): C1/C2/C3 (split ops — session serialize zone, run solo), E6 (shortcut-format in SettingsPane), E9 (appearance apply — thin GUI tail), G1/G5 (markdown doc feed / diff-comments shim), F5 (provider.select persistence). GUI-verify (flag for user, don't blind-build): D4 live overlay, A4 live WorkspaceList mount, window chrome, live agents.
- UI buildout #9 — 2 disjoint headless lanes (`/loop ultracode`, workflow `w7n0axyxy`: 6 agents; simplify `w9svsf0e1`, 2 agents, zero changes). Rust∥bun disjoint (deliberately NOT two Rust lanes — concurrent same-crate cargo builds race on the target lock + mid-edit compiles; the Rust∥bun split shares no build state). F5 deferred to a later solo iteration for that reason.
  - **G1** (`adf9171f9`) — `markdown_set_document` + `MarkdownState::set_document(label, path)`: the MISSING WRITER for `PanelCtx.file_path`. The `cmux-local-image://` jail (`local_image_protocol`) already READ `markdown_file_for(label)` but nothing wrote it → every image request jailed against an empty path and 403'd. Ports Swift `Coordinator.bind` filePath assign (MarkdownWebRenderer.swift:190-194), field-scoped, stored raw (jail standardizes at request time). Set-before-render doc-contract (Swift binds at updateNSView:99 before update(markdown:):106). `webview.eval` render push = deferred GUI tail. Verify SOLID — this was a real latent bug, not just a port. cmux-desktop 83 (+6), clippy clean.
  - **E9** (`adf9171f9`) — pure `settings/appearanceResolve.ts` `resolveAppliedAppearance(stored, systemColorScheme) -> AppliedAppearance` (mode/colorScheme/followsSystem/documentColorScheme/persistedRawValue/needsRewrite). Port of `AppearanceSettings.applicationAppearance` (:198-213) COMPOSING the ported appearanceMode.ts fns (no light/dark/system re-impl = the documented duplication trap avoided). duringLaunch is host-only → represented by followsSystem + injected systemColorScheme. document mutation + defaults write-back = deferred thin caller. Verify SOLID. web bun test src 455 (+11), typecheck 0.
  Gate GREEN: cmux-desktop 83, clippy clean, web 455, typecheck 0. Pushed `c8f43c1fa..adf9171f9` → fork/windows-port.
  === HEADLESS UI FRONTIER THINNING (2026-07-07) === 4 iterations this session (#6-#9, 8 slices: A1/A3/A9 + D5/D6/D7/D9 + E8/E9 + G1). Area D pure layer COMPLETE; Area A data/projection/placement foundation in; Area E settings-search + appearance-resolve in; Area G markdown writer in. Remaining slices are increasingly (a) GUI-verify live-mount work needing `npx @tauri-apps/cli dev` (D4 overlay, A4 WorkspaceList mount, window chrome B1-B7, live agents F1-F6, canvas C6-C13) — DO NOT blind-build per user directive; or (b) headless slices that TOUCH SESSION SERIALIZE ZONES and must run SOLO not paired (C1/C2/C3 split ops into session.rs/useSession/Workspace; A4-A8 sidebar richness); or (c) small host-wiring in src-tauri that races if paired as two Rust lanes (F5 provider persistence, G4/G5 diff/markdown wiring). NEXT: either solo-lane iterations for the session-zone C/A slices, OR pause and hand the user the prioritized GUI-verify list. Per user's "stop and notify if you can't do it without me": the highest-value remaining work (live UI) is GUI-gated → surface it rather than churn low-value headless tails.
- UI buildout #10 — solo Rust lane C1+C2 split/divider commands (`/loop ultracode`, workflow `w3icd1n66` scout→implement→verify; simplify `w0hs1ltu1`). Solo (not paired) because it touches the session serialize zone.
  - **C1** — already complete: scout CONFIRMED `insert_first` is threaded end-to-end (split_pane→apply_split→session_split, default false=append-second). No code change. JS caller direction-map (left/up=first, right/down=second, camelCase insertFirst) = deferred GUI.
  - **C2** (`48a685739`) — `session_equalize_dividers` command + new `session_ops::equalize_dividers` whole-tree walker using orientation-aware `span_count` (parity-exact port of CmuxPanes `ExternalTreeNode.spanCount(along:)` — pane=1, nested split recurses only when orientation==axis else 1). NOT the pre-existing leaf-count `equalize_divider` (diverges on mixed-orientation trees: H(V(a,b),c)→root 0.5 span vs 2/3 leaf; canonical=span). foundSplit-style bool (single pane→no-op). Command mirrors session_set_divider (lock→apply→clone→emit cmux://session-changed). Verify SOLID (span semantics + clamp-folds-plan+apply equivalence + no default-regression + golden byte-stable). Simplify removed one unreachable `total_span==0` guard (span_count always ≥1; canonical has no guard → parity-improving). cmux-core 161, cmux-desktop 86, golden 9 byte-stable, clippy clean.
  Pushed `68bc6dbf7..48a685739` → fork/windows-port.
  === LOOP CHECKPOINT (2026-07-07, after iter #10) === 5 iterations this session (#6-#10): A1/A3/A9, D5/D6/D7/D9, E8/E9, G1, C1(confirmed)/C2 = 11 slices, all pushed green, 4 real defects caught by adversarial verify (config-override scope, phantom-group anchor, G1 live 403 bug, +). The HEADLESS command/ops/pure-model frontier for the touched areas is now largely harvested. What REMAINS is dominantly the DEFERRED GUI-WIRING TAIL — each headless slice above left a thin caller (JS invoke / DOM mutation / keyboard-menu trigger / webview.eval render push) that needs the running app to build+verify. Consolidated GUI-verify queue for the user's next `npx @tauri-apps/cli dev` session (highest value first): (1) D4 live command-palette overlay — compose windowStore+paletteQuery+commandCatalog+switcherEntries+renderSequencing+resultsGating+command_palette_search + open-shortcut/focus/Escape/arrow/click/Enter; (2) A4 mount rich WorkspaceList via render_items (groups/pins/collapse) + wire selection; (3) directional-split + equalize UI triggers (C1 insertFirst arg from split buttons/keys; C2 equalize keyboard/menu/palette action) — commands are LIVE, just need callers; (4) markdown doc-feed caller (await markdown_set_document before markdown_render) + appearance apply (resolveAppliedAppearance→document color-scheme + persist) + settings-search box (settingsEntriesMatching→SettingsPane); (5) window chrome B1-B7; (6) live agents F1-F6 (Codex/OpenCode installed). Remaining PURE-headless candidates are thinner: C3 (resizeDividerAdjustment key handler — mostly GUI), C4 (directional pane focus — needs a web focused-pane concept), E6 (shortcut-format wiring — GUI), A5-A8/A10-A14 (sidebar richness — mostly GUI-mount). Recommend: pause autonomous headless churn; do a GUI session next. LOOP CONTINUES only if more genuinely-headless slices are worth it — else await user for the live-verify phase.

- UI buildout #11-#12 (2026-07-07, /loop resumed) — #11 was D4 (live
  command-palette overlay, `d348176ee`) + terminal top-label feed
  (`77b3d72d7`), landed before this log entry. #12 this iteration:
  (a) recovered + finished the uncommitted in-flight agent-session slice
  (`acc0071ee`, rebased over fork PR #2): queued provider switch while a
  session runs (pendingProviderId + advanceProviderSwitch stop→wait→apply→
  re-arm auto-start; picker shows the queued provider immediately), composer
  single-line/multiline oscillation fix (trust width only when measured in
  single-line layout), overflow-wrap:anywhere on the composer. webviews 173
  tests green.
  (b) **A4** (`5309ca07c`) — rich WorkspaceList mounted in the live sidebar.
  `ensure_workspace_ids` in session.rs mints workspace UUIDs (stateful layer
  owns id synthesis; closes the A3-deferred divergence — fresh_terminal_workspace
  stays pure/id-less). Web `sidebar/snapshotProjection.ts` = tested twin of
  sidebar_render.rs (uuid validate+lowercase, member-less drop, dup keep-first,
  3-tier anchor fallback) composed with ported renderItems; WorkspaceList now
  interactive (titles, selection, close w/ sole-workspace no-op parity, header
  click activates anchor — the header IS the anchor's row); SidebarView renders
  through it (groups/pins/collapse LIVE); id→index translation for the
  index-addressed session commands. Gate: web 480 + tsc clean, cmux-desktop 88
  + clippy clean. sidebar_render.rs note corrected (projection lives web-side;
  Rust twin stays as oracle + future native consumer).
  USER DIRECTIVE UPDATE: user re-invoked /loop asking for the FULL UI parity
  buildout ("nothing is in the ui yet... keep working until completely
  finished; I will look once everything is implemented") — this supersedes the
  earlier "don't blind-build GUI" pause. GUI-wiring tail is now IN SCOPE for
  the autonomous loop; verify headlessly (tests/tsc/clippy) and flag anything
  only a live run can prove. NEXT queue: A5 (group collapse command + chevron)
  / A6 (inline rename) / A7 (pin) — session-zone, run solo; C1 insertFirst +
  C2 equalize UI triggers (commands live, need callers); markdown doc-feed
  caller + appearance apply + settings-search box wiring; D-overlay polish.

- UI buildout #13 (`7bc95d651`) — palette INTENT EXECUTION: D4's activateAt
  went from console.info stub to a real dispatch path. Pure
  `palette/intentPlan.ts` (CommandIntentKind + session shape → executable
  plan) + thin executor in useCommandPalette. LIVE now: newWorkspace,
  closeWorkspace, nextWorkspace/previousWorkspace (canonical WRAP,
  TabManager.swift:3451-3485), terminalSplitRight/Down (targets first-leaf
  active pane until C4 focus tracking; insertFirst threaded through
  useSession.split = C1 plumbing), equalizeSplits (new
  useSession.equalizeDividers = C2 caller), toggleSidebar (App-owned
  hostActions bundle through CommandPaletteOverlay). Unmapped kinds still
  log (never silently no-op). New splitLayout.firstActivePanelId.
  Gate: web 492 + tsc clean. C1/C2 UI triggers = DONE via palette; keyboard
  shortcuts for splits = separate slice (needs the shortcut system, E6-adjacent).
  NEXT: A5 group-collapse command (session zone, solo), markdown doc-feed
  caller + appearance apply + settings-search box, or renameWorkspace intent
  (needs inline-rename UI = A6).

- UI buildout #14 (ultracode wave, workflow wsjcw9mug: 12 agents — 4 lanes x
  scout→implement→adversarial-verify, ALL 4 verdicts SOLID, 0 blocking) — the
  first wave under the user's new "ultracode + multiple sub agents" /loop
  directive. Lanes file-disjoint (1 Rust ∥ 3 TS), Rust+TS halves of A5 built
  in parallel against an agreed command contract:
  - **A5** (`d2a77e669`) — group collapse end-to-end. KEY oracle finding
    (scout): canonical has TWO variants — pure-data setWorkspaceGroupCollapsed
    (WorkspaceGroupCoordinator.swift:405-412, no selection move; socket/CLI
    paths) vs UI toggleWorkspaceGroupCollapsed (:367-403, selects anchor when
    collapsing hides the selected member). Ported the PURE variant; the
    toggle-variant selection semantics are a documented future op. Emit gated
    on changed (parity with willSet suppression). Chevron = separate button
    tap target (stopPropagation), header click still selects anchor.
  - **G1 caller** (`397337d95`) — markdown_set_document await-before-render in
    MarkdownSurface + markdownBridge; closes the live local-image 403 found in
    buildout #9.
  - **E8 tail** (`6cf84cce2`) — SettingsPane search box over
    settingsEntriesMatching (score order preserved, section navigation, pure
    settingsSearchResults.ts projection).
  Gate (run by orchestrator over the merged tree): cmux-core 173 (+12),
  cmux-desktop 90 (+2), goldens byte-stable, clippy clean; web 512 (+20), tsc
  clean. NEXT wave candidates: A6 inline rename (Rust session_rename_workspace
  ∥ TS sidebar editor), A7 pin/unpin + pinned-ahead reorder, E9 appearance
  apply tail, renameWorkspace/toggleWorkspacePin palette intents (dep A6/A7).

- UI buildout #15 (ultracode wave, workflow wqhaxwqe7: 12 agents, 4 lanes
  scout→implement→adversarial-verify, ALL SOLID, 0 blocking, 6 minor — 2
  applied by orchestrator [rename prefill select-all per ContentView:14954;
  Shift added to the divider-key modifier guard], rest = documented interims
  [C4 first-leaf target, appearance shell colors, execCommand return]):
  - **A6** (`f0220ddd5`) — inline rename e2e. Oracle Workspace.swift:4390-4407
    setCustomTitle: trim, empty/whitespace clears custom_title+source, else
    sets source "user" (golden-pinned). Unknown index no-op. Blur CANCELS
    (canonical modal commits only on explicit affirmation). Double-click row
    label edits; useSession.renameWorkspace(index, title).
  - **E9 caller** (`4e2e728ca`) — useAppearance mounts in App: composes
    resolveAppliedAppearance over stored value + matchMedia, stamps :root
    color-scheme + data attr, needsRewrite persistence. INTERIM localStorage
    store (divergence documented until settings persistence lands).
  - **Palette intents r2** (`99fe55ec7`) — copyWorkspaceID/copyPaneID/
    copySurfaceID/copyIdentifiers with oracle-cited formats; state the port
    lacks stays unhandled (no invented formats). hasFocusedPanel feeds catalog
    ctx so display and activation resolve identically.
  - **C3** (`dd7d9518b`) — keyboard divider resize: focusable separator
    dividers, canonical 10px resize_split step through drag's resizeDivider
    math; cross-axis arrows null; modified arrows (incl Shift) excluded.
  Gate: cmux-core 181 (+8), cmux-desktop 93 (+3), goldens stable, clippy 0;
  web 544 (+32), tsc clean. NEXT wave-16: A7 pin e2e, C4 focused-pane
  tracking (fixes split-target + copySurfaceID interims), E6 shortcut-format
  in SettingsPane, G5 diff-comments shim.

- UI buildout #16 (ultracode wave, workflow w976jxrf5: 12 agents, 4 lanes,
  ALL SOLID, 8 minor — orchestrator applied 3 [A7 dangling-group-id boundary
  fallback to own flag per Ordering.swift:201-207 + regression test; Pin/Unpin
  Workspace label casing; Object.freeze(UNBOUND_SHORTCUT)]; noted-not-applied:
  C4 capture-order close transient (self-corrects via fallback), C4
  selected-panel nuance (multi-tab panes don't render yet), G5 reply-listener
  origin gate (hardening pass), Workspace.test.tsx mock.module process-global
  leak (watch for cross-file flakes)):
  - **A7** (`daf9718c6`) — pin/unpin e2e. Oracle: WorkspaceReorderCoordinator
    :467-472 + reorderTabForPinnedState :529-539 — ungrouped remove-then-insert
    at leading global-pinned boundary (pin→end of pinned prefix, unpin→front
    of unpinned segment, stable partitions); grouped = flag-only (contiguity
    normalization documented gap). is_pinned Some(true)/None keeps goldens
    byte-stable. Index selection follows the moved workspace.
  - **C4** (`e353793ef`) — focused-pane store + resolveActivePanelId with
    layout validation + first-leaf fallback; palette splits/copy now target
    the REAL focused pane (retires the interim two verifiers flagged).
  - **E6** (`ae0974d00`) — Settings keyboard-shortcut rows via ported
    shortcutBinding/shortcutFormat (Windows chords), display-only.
  - **G5** (`ee519486a`) — diffCommentsRelay: viewer-bundle comment RPCs ↔
    diff_comments_rpc with relayId reply routing; GUI tail = viewer visuals.
  Gate: cmux-core 194 (+13 incl regression), cmux-desktop 95 (+2), goldens
  stable, clippy 0; web 660 (+116), tsc clean. Sidebar richness A4-A7 now
  COMPLETE headlessly. NEXT: A8 multi-select/shift-ranges, A10 drag-reorder,
  toggleWorkspacePin/renameWorkspace palette intents, A11 context menus, F5
  provider persistence, window-chrome B-lanes (GUI-verify tails accumulating —
  a live `npx @tauri-apps/cli dev` verification session is increasingly
  valuable before more UI stacking).

- UI buildout #17 (ultracode wave, workflow w2hbn2ajb: 12 agents, 4 lanes,
  ALL SOLID, 7 minor — noted-not-applied: F5 per-panel-vs-global persistence
  divergence (documented in commit), settings write on the actor thread
  (perf, revisit if RPC latency shows), palette workspaceName vs
  workspaceDisplayName subtitle nuance, A8 placeholder-key sweep in shift
  ranges (corrupted-row edge), A10 normalize gate mirror-vs-model groups
  emptiness nuance):
  - **A10 backend** (`2cc228a79`) — reorder op + command. Scout's key find:
    ALL planning primitives already golden-pinned in cmux-workspaces
    (clamps/normalize/sync) — the op is a positional snapshot<->Uuid mirror
    adapter + router (anchors → top-level path, members → in-section clamp,
    ungrouped → pin-tier clamp). Drag-inference (isDragOperation=true) =
    the future drag-UI lane.
  - **A8** (`fc74baaa0`) — sidebar multi-select via ported selection.ts;
    modifiers ride the row-click callbacks; is-multi-selected exclusive of
    is-selected (canonical isActive-first).
  - **Palette r3** (`2faf3b082`) — toggleWorkspacePin + clearWorkspaceName;
    closeOthers stays unhandled (canonical confirm guards not portable yet).
  - **F5** (`26b6bb1a2`) — provider.select persists via NEW app_settings.rs
    (atomic JSON store in app-config dir; corrupt→defaults) = the settings-
    persistence FOUNDATION (appearance localStorage interim can migrate).
  Gate: cmux-core 206 (+12), cmux-desktop 108 (+13), goldens stable, clippy
  0; web 678 (+18), tsc clean. NEXT wave-18: A11 context menus, A13 badges
  contract, notification-config wiring, agent-session replay groundwork.
