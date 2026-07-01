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
