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
