# Phase 3 — Canonical agent sessions   [L]

**Goal:** run **Claude / Codex / OpenCode** as *canonical* agent sessions —
spawned over their **headless transport** and rendered with the **reused
`webviews/` chat UI** — not as a raw TUI in a terminal.

**Depends on:** Phase 1 (bridge), Phase 2 (surfaces). **Canonical note:** an
earlier MVP shortcut that launched an agent's TUI in the terminal was reverted
(`de10cc9`); this phase does it the way macOS cmux does.

## How macOS cmux does it (mirror this)

- Resolve the agent executable, then spawn it with **transport args**
  (`AgentSessionProviderId::launch_arguments`): Codex `app-server --listen
  stdio://` (`stdio-jsonrpc`), Claude `-p --output-format stream-json …`
  (`stdio-jsonl`), OpenCode `serve …` (`http-loopback`).
- Drive it programmatically; render the conversation in the **web** agent-session
  UI. Transcript parsing/model is Swift `CmuxAgentChat` (57 files) — logic only.

## Tasks

1. **Spawn agents canonically.** `cmux-agent` resolve → `plan.to_spawn_spec()` →
   `cmux-process` (Job-Object supervised). Curated env + rewritten PATH already
   handled by `cmux-agent`.
2. **Transport clients (Rust):** a Codex app-server **JSON-RPC** client, a Claude
   **stream-json (JSONL)** client, an OpenCode **HTTP-loopback** client (creds
   already minted by `cmux-agent`).
3. **Port the transcript parsers** `CmuxAgentChat` (Claude JSONL, Codex JSON-RPC)
   Swift → **Rust**, co-located with `cmux-agent`; expose state via
   `@cmux/core-types`. Cover with **`cmux-golden`** golden tests.
4. **Wire the reused `webviews/agent-session` React renderer** to the host bridge
   (replace the WKWebView bridge); stream transcript/state via events.
5. **Agent = a surface type** in the workspace — a tab can be a shell *or* an
   agent session (integrates with Phase 2).
6. **Session lifecycle** — start/stop, and the auto-start policy
   (`should_auto_start_session`: Codex/OpenCode yes, Claude no).

## Reuse
`cmux-agent`, `cmux-process`, `webviews/agent-session` (React), `@cmux/core-types`.

## New code
Per-provider transport clients (Rust); transcript-parser port; agent-surface
wiring; bridge event plumbing for transcripts.

## Deliverable
Launch an agent in a tab; the canonical chat UI renders; you can converse.

## Acceptance
- Each provider connects, streams, and renders correctly.
- Session lifecycle + auto-start policy match canonical.
- Transcript-parser golden tests pass (`cmux-golden`).

## Risks
- Transport parity per provider.
- Parser fidelity (R3) — golden-test it.
- Provider auth on Windows (claude is a `.ps1` shim; codex/opencode may be absent).

## Touchpoints
`crates/cmux-agent`, `crates/cmux-process`, `crates/cmux-golden`,
`cmux/webviews/src/agent-session`, `Packages/Shared/CmuxAgentChat` (parser
reference), `apps/desktop/src-tauri` (agent commands/events).

---

## Research-derived plan (2026-07-01, ultracode workflow `w5kvotmvq`)

**Contract (verified from source).** The agent-session webview
(`webviews/src/agent-session`) talks to its host over exactly **one request
seam** and **one push seam**:

- Request: renderer calls
  `window.webkit.messageHandlers.agentSession.postMessage({id,method,params})`
  → the Windows shim (`apps/desktop/web/src/host/host.ts`, already built) maps
  channel `agentSession` → Tauri command **`agent_session_rpc`**, returning the
  `NativeReply` envelope (`{ok:true,value}` | `{ok:false,error:{code?,userMessage?}}`).
- Push: host serializes an `AgentEvent` to JSON and calls
  `window.cmuxAgentBridge.receive(event)` → the shim forwards the Tauri event
  **`cmux://agent-event`** into `cmuxAgentBridge.receive`.

**7 methods:** `app.context`, `provider.list`, `provider.start`,
`provider.select`, `provider.writeLine`, `provider.stop`, `app.pickFiles`.
**7 events:** `app.theme`, `app.rateLimitRows`, `provider.started`,
`provider.output`, `provider.activity`, `provider.turnComplete`,
`provider.exit`.

**Both web seams already exist** (host.ts shim + `MAC_HOST_CHANNELS.agentSession`
+ `MAC_HOST_EVENT`). Do not rewrite them — Phase 3 is the Rust/Tauri side.

### `cmux-agent-chat` crate — first slice DONE (2026-07-01)
Landed as the pure headless foundations (31 tests, clippy clean, in-tree):
`error.rs` (8-variant `BridgeError`, exact `code()` strings — `providerNotReady`
is load-bearing), `request.rs` (`BridgeRequest` + typed getters; the
`required_string` TRIM vs `required_raw_string` NO-TRIM distinction is enforced),
`permission_mode.rs` (+ `codex_turn_overrides()`), `event.rs` (`AgentEvent` serde
enum → exact camelCase wire shapes, optionals omitted-not-null, ts-rs export),
`line_buffer.rs` (byte→line split + flush).

### Remaining Phase 3 build order (concrete)
1. `codex.rs` — Codex app-server **JSON-RPC over stdio** (initialize → thread →
   turn submit w/ permission overrides; notifications → output/activity/turnComplete;
   single-queued-input backpressure). Largest unit; fixture-test off captured frames.
2. `claude.rs` — Claude **stream-json**: input framing `writeClaudeStreamJSON`
   + `ClaudeStreamJSONAccumulator` (locate the Swift source first — not among the
   enumerated Panels files).
3. `opencode.rs` — SSE parser + text accumulator + server-URL sniff/suppress +
   HTTP-loopback client (reuse `cmux-agent::opencode::OpenCodeServerAuth`).
4. `running_session.rs` + `process_store.rs` — lifecycle brain over `cmux-process`
   (`SpawnSpec`, `AgentIo::write_line`/`chunks`, Job-Object supervisor); single
   active session (`sessionAlreadyRunning`); emit every event through an injected
   `Fn(AgentEvent)` sink so the crate stays Tauri-free/testable.
5. `context.rs` + `theme.rs` — `app.context` assembly (~90 localized copy keys) +
   14-field theme dict.
6. `apps/desktop/src-tauri/src/agent_session.rs` — `#[tauri::command]
   agent_session_rpc`; `app.pickFiles` via Tauri dialog+fs (honor 512KB/2MB image
   caps); route all events through ONE mpsc→emitter task (ordering parity with the
   Swift serial MainActor). Register in `lib.rs generate_handler!` + `.manage`.

Deferred to a later milestone: `Sources/Mobile/AgentChat/*` (mobile-companion
transcript-history/JSONL subsystem, `chat.message` topic) — orthogonal to the
live webview contract; maps to a future `cmux-agent-chat::history` module.

### New Rust commands
- `agent_session_rpc(app, state, message) -> NativeReply<Value>` — the single
  request seam; dispatch 7 methods; state owns the `ProcessStore` whose event sink
  is `app.emit("cmux://agent-event", value)`.

### Open decisions
- **Renderer kind:** ship **react only** (keep `AppContext.renderer='react'` for
  wire parity); macOS also has a solid shell — revisit if needed.
- **Theme fidelity:** minimal Rust port emitting the identical 14 fields from a
  base background + `isDark`; accept reduced NSColor-blend fidelity for v1.
- **Request `id`:** parse for shape only, ignore server-side (Tauri's invoke
  promise already correlates); keep the field for wire parity.
- **`app.pickFiles`:** Tauri dialog+fs honoring exact caps + `isImage`/`mimeType`.

### Key risks
- `providerNotReady` code string is a hard contract (renderer silently no-ops
  `writeLine` on it). ✅ asserted in the landed slice.
- `writeLine` text must NOT be trimmed (`required_raw_string`). ✅ enforced.
- AgentEvent optionals (`detail`/`outputDelta`) OMITTED not null. ✅ enforced.
- Three distinct stateful protocols (~1000+ Swift lines) — fixture-test each.
- `provider.started` emission timing differs by provider (codex/opencode AFTER
  handshake, others immediately) — the UI start/exit state machine depends on it.
- Event ordering: route all emits through one mpsc + single emitter task.
- os-4551: process-spawning tests must live in lib `#[cfg(test)]`.
