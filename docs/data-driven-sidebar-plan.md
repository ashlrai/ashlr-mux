# Fully data-driven custom sidebars: data, commands, hooks

Goal for the release: an interpreted sidebar can **read all cmux runtime state**, **invoke any cmux command**, and **react to any cmux event/hook**. This documents the real surface (grounded in the v2 dispatcher + event bus) and the architecture/waves to get there.

## The realization: one protocol already backs all three

cmux's v2 socket dispatcher (`TerminalController.processV2Command`, `Sources/TerminalController.swift:3335`) is the single source of truth for both reads and writes, and `CmuxEventBus` is the single source of truth for hooks. So we do not hand-maintain three parallel surfaces; we project the dispatcher + bus into the interpreter:

- **Commands (write)** — `cmux(method, params)` already routes through `runV2CommandLine` → the dispatcher. Windows/Tauri authored-sidebar actions now pass through a safe default allowlist in `custom_sidebar_action_invoke`, the safe-default action subset has a capabilities-advertised method schema gate, and the constrained Swift renderer now preserves basic JSON-compatible param types (numbers, booleans, nulls, arrays, ternaries, and resolved live-data fields). What's still missing is generated full-dispatcher discoverability/coercion and per-sidebar trust/capability manifests.
- **Data (read)** — the dispatcher's query methods already assemble rich payloads. Project those into a `data` value tree instead of the current hand-built 4-key context.
- **Hooks (events)** — `CmuxEventBus` already emits every lifecycle event with a uniform schema. Subscribe to all of it and surface it to the interpreter.

Windows/Tauri now exposes the documented `extension.sidebar.snapshot` read method (plus `sidebar.snapshot` alias) as a rich bootstrap payload built from the same workspace/surface summaries used by `workspace.list` and `surface.list`. It includes selected workspace metadata, ordered workspaces, custom-sidebar authoring aliases (`selectedId`, `selectedTitle`, `workspaceCount`, `unreadTotal`, per-workspace `tabs`, `ports`, `branch`, `pr`/`prs`, `progress`, etc.), the raw runtime fields already surfaced through the session/control-socket path, an interpreter-ready `data` tree mirroring the macOS `CustomSidebarDataContextBuilder` shape, an optional `assets` map for host-minted custom-sidebar image URLs, and an `events.*` bootstrap context (`latest`, `recent`, category/name counts, cursor fields). Windows/Tauri also now advertises a live `events.stream`, routes `cmux events` incrementally, records session/workspace/pane/surface/sidebar-derived events from the existing session-change notification path, appends those events to `~/.cmuxterm/events.jsonl` with one rotated archive, emits an in-process `cmux://events-changed` notification consumed by open authored sidebars, routes `cmux sidebar list` / `cmux sidebar validate [name]` to structured sidebars-directory validation with adjacent `<name>.manifest.json` capability metadata, implements `cmux sidebar open <name>` as a focused-pane `custom-sidebar` surface backed by live session data, renders `.json` custom sidebars through a small block renderer, dispatches JSON-authored action objects through `custom_sidebar_action_invoke` with a safe default capability scope and manifest-aware denial context, renders a constrained `.swift` subset (`VStack`/`HStack`/`Text`/`Divider`/`Spacer`/`Button`/`Label`/`Image`/`AsyncImage`/`ProgressView`/`ScrollView`/`List`/`Section`/`ZStack`/`Grid`/`GridRow`/`Menu`, stack `spacing:`, `Button(role:)`, `.onTapGesture { cmux(...) }`, basic shapes, `ForEach(workspaces)`, `ForEach(workspaces.indices)`, `ForEach(Array(workspaces.enumerated()), id: \.offset)` tuple-style params, simple `for` ranges, `if let`, `workspaces[i]` reads, array helpers like `.first`, `.last`, `.contains`, `.reversed()`, `.prefix(n)`, `.suffix(n)`, `.dropFirst(n)`, `.dropLast(n)`, `.enumerated()`, and common visual modifiers like `.font`, `.bold`, `.italic`, `.monospaced`, `.lineLimit`, `.truncationMode`, `.multilineTextAlignment`, `.textCase`, `.underline`, `.strikethrough`, `.foregroundColor`, `.opacity`, `.fixedSize`, `.disabled`, `.help`, `.padding`, `.background`, `.cornerRadius`, `.layoutPriority`, `.offset`, `.zIndex`, `.aspectRatio`, `.scaledToFit`, `.scaledToFill`, `.clipShape`, `.clipped`, `.shadow`, `.border`, `.stroke`, `.blur`, `.brightness`, `.contrast`, `.saturation`, `.grayscale`, `.rotationEffect`, `.scaleEffect`, `.listStyle`, `.scrollContentBackground`, `.imageScale`, `.symbolRenderingMode`, `.symbolVariant`, `.listRowBackground`, `.listRowSeparator`, `.fill`, `.tint`, and `.frame(maxWidth: .infinity)`) with SwiftUI-authored `cmux(...)` action capture, implements `cmux sidebar reload [name]` as validation plus a targeted `cmux://custom-sidebar-reload` event consumed by open custom-sidebar panes, and implements `cmux sidebar select <name>` as validation plus a `cmux://custom-sidebar-select` event consumed by the left-sidebar custom host. Authored panes poll their source file for save-time hot reload. Full macOS SwiftUI interpreter coverage, manifest-granted privilege expansion UX, broader macOS event-catalog coverage, native asset-catalog lookup beyond host-provided sidebar image URLs, and stateful inputs still need their own completion audit.

## Command surface (248 methods)

| namespace | count | notable |
|---|---|---|
| browser | 84 | navigate/click/eval/screenshot/network/cookies/storage/tabs/... |
| workspace | 44 | list/current/select/new/close/reorder/group.*/remote.*/rename/color |
| surface | 24 | list/current/focus/new/close/send/send-key/report_*/resume.* |
| debug | 39 | (debug-only) |
| notification | 10 | list/create/read/remove/clear/create_for_target |
| pane | 9 | list/focus/split/close/... |
| feed | 6 | list/tui/clear/... |
| system | 6 | — |
| vm | 6 | list/attach_info/ssh_info/new/rm/exec |
| window | 5 | list/current/new/focus/close |
| auth | 4 | status/login/sign_out/begin_sign_in |
| app/feedback/events/extension/file/markdown/session/settings/tab | 1–2 each | — |

Full machine catalog: `cmux capabilities` (method list) — should be generated into the authoring docs + skill so authors/agents know every method and its params.

## Data surface (read)

Query methods returning structured payloads (project all into the `data` tree):
- `workspace.list` / `workspace.current` / `workspace.group.list` — id, ref, title, description, selected, pinned, listening_ports, remote, current_directory, custom_color, latest_conversation_message, latest_submitted_message, latest_submitted_at, index.
- `extension.sidebar.snapshot` (richest) — adds root_path, project_root_path, branch_summary, remote_display_target, remote_connection_state, unread_count, latest_notification_text, pull_request_urls, panel_directories, git_branches. (`TerminalController.swift:5525`)
- `surface.list` / `surface.current` — id, ref, index, type, title, focused, pane_id, working dir, initial_command, resume_binding; browser: developer_tools_visible. (`:8615`)
- `pane.list`, `window.list`/`window.current`, `notification.list`, `feed.list`, `vm.list`, `auth.status`, `workspace.remote.status`.

Underlying model (`Workspace.swift:10243+`) carries even more per-workspace/per-surface state: `gitBranch`/`panelGitBranches` (branch + dirty), `pullRequest`/`panelPullRequests`, `surfaceListeningPorts`, `remote*` (connection state, detected/forwarded/conflicting ports, live SSH session count), `latestConversationMessage`/`latestSubmittedMessage`, `progress`, `logEntries`, `statusEntries`, `metadataBlocks`, `manualUnreadPanelIds`, `panelShellActivityStates`, `agentPIDs`/`agentPIDPanelIdsByKey`.

**Gaps not yet queryable (need new exposure for "fully data-driven"):** no known gaps remain among the listed workspace/per-surface read-model fields after surfacing per-surface shell activity (`panelShellActivityStates`) as `panel_shell_activity` through the session/control-socket path. Terminal process metadata (root PID, descendant PIDs, foreground/leaf PID approximation, process name, and source/error metadata) is now exposed through `debug.terminals`. Per-surface TTY names, per-surface ports, agent PID registrations/agent listening ports, per-surface resume bindings, and browser content state (URL/proxy/history/devtools/focus state) are now exposed through the session/control-socket path and rendered/readable via existing Windows/Tauri surfaces; sidebar progress, status/log entries, rich metadata entries, metadata blocks, PR/review metadata, and shell activity are also exposed end-to-end. A broader parity audit across dispatcher coverage, event hooks, generated types, CLI/socket surfaces, and live desktop behavior is still required before declaring the full goal complete.

## Hook/event surface (CmuxEventBus)

Uniform event schema (`CmuxEventBus.swift:181`): type, protocol, version, boot_id, seq, id, **name**, **category**, source, occurred_at, workspace_id, surface_id, pane_id, window_id, payload.

Emitted names by category (`CmuxEventPublishing.swift`):
- `workspace.*` — created, closed, selected, reordered, prompt_submitted
- `surface.*` — created, closed, selected, focused
- `pane.*` — created, closed, focused
- `notification.*` — created, read, removed, cleared
- `window.*` — lifecycle
- `workstream.*` — start / progress / complete

Subscribe via `CmuxEventBus.subscribe(afterSequence:names:categories:)`; the socket `events` command streams them. Agent lifecycle hooks (`CLI/CMUXCLI+AgentHookDefinitions.swift`) exist for codex/grok/cursor/gemini/kiro/antigravity/hermes — session-start/prompt-submit/stop/notification/session-end/shell-exec — recorded to `~/.cmuxterm/{agent}-hook-sessions.json`.

## Architecture

```
interpreted sidebar (.swift)
        │  reads                         │  writes               │  reacts
        ▼                                ▼                       ▼
   data.* value tree            cmux(method, params)        on(event) / event.*
        │                                │                       │
   DataContextProvider          SidebarActionDispatch      EventBridge
        │  (projects)                    │ (already全)            │ (subscribes all)
        └──────────────► TerminalController v2 dispatcher ◄───── CmuxEventBus
```

- **DataContextProvider** (host): builds the `data` SwiftValue tree each refresh by reusing the same payload builders the v2 query methods use (workspace summary, extension snapshot, surface/pane/window/notification/feed/vm/auth). Exhaustive-by-construction: every field the dispatcher can return is projected. Lives in the app target (touches `TerminalController`/`Workspace`), feeds the package via the existing `dataContext` param.
- **SidebarActionDispatch** (exists): `cmux(method, params)` → `runV2CommandLine`. Windows/Tauri now preserves basic JSON-compatible Swift param values, has a safe default capability scope (allow/deny method globs), and validates the safe-default authored-action subset against a capabilities-advertised method schema before dispatch. Remaining hardening is generated full-dispatcher schema/coercion plus opt-in trust/capability manifests.
- **EventBridge** (host): one `CmuxEventBus.subscribe` over all names/categories; pushes `events.latest`, `events.recent[]`, per-category counts, and agent-hook lifecycle state into the data tree, and triggers a re-walk on each event (replacing/augmenting the 1s TimelineView tick with event-driven refresh). Windows/Tauri open authored sidebars now seed `events.*` from `extension.sidebar.snapshot`, update it from the in-process `cmux://events-changed` bridge, and support a safe `.onEvent("name")` / `.onEvent(category:)` subset for local `@State` assignments plus safe-scoped `cmux(...)` actions.

## Waves

- **Wave A — Full read surface (fully data-driven).** Replace the 4-key `customSidebarDataContext` with the `DataContextProvider` projecting all query payloads + the rich `Workspace` model fields. Additive, low-risk. Personas immediately populate with real data. The previously listed shell-activity read-model gap is now surfaced as structured fields, and Windows/Tauri now has a real `extension.sidebar.snapshot` bootstrap payload; continue auditing the actual interpreter data injection path for any remaining unlisted read-model holes before treating this wave as fully closed.
- **Wave B — Events/hooks reactive.** EventBridge subscribes to the whole bus; expose `events.*` + agent-hook state; event-driven re-render. Surface every event name/category. Windows/Tauri now has the socket/CLI/event-ring/durable-log/live-stream foundation, session/workspace/pane/surface/sidebar event derivation, `events.*` in `extension.sidebar.snapshot`, an in-process `events.*` bridge for open authored sidebars, and a safe author `.onEvent(...)` subset. The remaining macOS event catalog and arbitrary Swift handler semantics are not yet complete.
- **Wave C — State engine + reactivity + input controls.** SwiftUI-surface roadmap Phase 2: a host-owned `@State`/`$binding` engine. Windows/Tauri now supports simple local `@State var` declarations, direct `$name` editable TextField/Toggle/Picker/Slider controls, and `.onEvent(...)` state updates. Richer Binding semantics, persisted state identity, live cmux-data mutation, and write-actions beyond `cmux()` remain the larger lift.
- **Wave D — Command catalog + capability scoping + typed params.** Generate the full `cmux capabilities` catalog into authoring docs and a Swift interpreter knowledge reference; Windows/Tauri now has a safe default allowlist for authored sidebar actions, but per-sidebar capability manifests/trust UX and robust param coercion remain. Security gate for "all commands exposed."
- **Interleave — leaf SwiftUI views/modifiers** from `docs/swiftui-interpreter-surface.md` Phase 1 (List/Section/LazyVStack/Grid/Label/ProgressView/overlay-background/styles) so the richer data has richer views to render into. Windows/Tauri now has a safe CSS-backed gradient subset; richer gradient stops/materials remain part of the generalized `StyleValue` follow-up.

## Open decision: opt-in expansion beyond safe authored-sidebar actions
Exposing every dispatcher method to any `.swift` file a user or in-pane agent drops in includes destructive/sensitive ones (`browser.eval`, debug methods, remote configuration, `workspace.close`). Windows/Tauri now uses option (2) as the baseline: `custom_sidebar_action_invoke` applies a default-safe allowlist and returns `custom_sidebar_capability_denied` for methods outside that scope. The remaining decision is how a user deliberately expands that scope: an opt-in trusted flag per file/dir, a per-sidebar capability manifest, or both.
