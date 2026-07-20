# Windows parity checkpoint

Checkpoint commits:

- Current canonical audit: `ecebdbb64b3532b0308650280ae4b83f30becf2a`
- Frozen differential canonical: `e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`
- Latest complete window behavior captured: `a51ddd28763bad2549d3903de78bc7b59b988e36`
- Latest workspace behavior captured: `4cfbc03030967813dcd23a1637ec2832c363e46a`
- Latest workspace-navigation behavior captured: `aa3f74c799079290f25441e950761c67c3c56026`
- Latest workspace-ordering behavior captured: `46f4cedf1a77fda23baff9b73b5d8ef7b96eb187`
- Latest workspace-group behavior captured: `357ebc0c662df446b93d842b3a7d24964e993dfa`
- Latest Windows code checkpoint: `72be49b35300bec3c359650231e210ed00db38c5`
- Latest terminal-title behavior checkpoint: `ece79b413abe1298f2160223920c47176fdc741e`
- Latest exact startup differential evidence: `parity/diff-lane@215737999ac0579682b6b2a2d230d7a0d064a51d`
- Latest window harness checkpoint: `parity/diff-lane@9257db511b975335e2ee79bc4bb143c04bfa95f8`
- Latest canonical window capture: workflow run `29686515273`
- Latest workspace harness/manifest checkpoint: `parity/diff-lane@3ff4aee606b6f129df017694b6af2309775f6fe2`
- Latest canonical workspace capture: workflow run `29690475786`
- Latest navigation evidence: `parity/diff-lane@2ff911b7699ac07c3186cff37d3962a7caadfecd`
- Latest canonical navigation capture: workflow run `29694877733`
- Latest ordering evidence: `parity/diff-lane@1e7a5e0a662a0c20f67aaa80cbdd5ddc6e232da3`
- Latest canonical ordering capture: workflow run `29696270467`
- Latest workspace-group evidence: `parity/diff-lane@32dd8da8256e35dd1ae42e41fa267c3753634833`
- Latest canonical workspace-group capture: workflow run `29697573532`
- Latest pane-management evidence: `parity/diff-lane@6cf4b270e2ed390c61225f89eb2ef6e7c445480e`
- Latest canonical pane-management capture: workflow run `29715304196`

The Windows desktop and CLI build successfully. The retained pane/surface and
startup-restore evidence remains valid. The exact environment-matched window
pair contains all 68 cases with zero capture errors, zero missing cases, and
zero unsatisfied settles. Its normalized comparison is valid: all 68 cases are
exact or reviewed platform-equivalences, with zero strict deltas. See
`evidence/window_lifecycle_2026-07-19.json` for hashes,
strict lanes, and provenance.

Windows now rejects ConPTY's initial full Windows PowerShell executable path as
a surface title only while that surface has no prior runtime title. Canonical's
`Terminal` default therefore survives the real PowerShell boot path in a
custom-titled, multi-pane workspace; subsequent OSC titles, including the same
path after a real title, remain accepted. RED commit `8a801e6d55` and fix
`ece79b413a` retain this behavior. The focused test passed twice, all 334
`cmux-core` tests passed, the desktop gate passed 1,113 tests with one ignored,
the frontend passed 1,255 tests plus typecheck and production build, and the
parity harness passed 30 tests. A headless real-app probe focused and booted the
PowerShell pane, swapped it, and observed `Terminal` rather than the executable
path with zero capture or visible-window errors.

The pane-management family is promoted at `parity/diff-lane@6cf4b270e2`.
Both captures have zero capture errors, missing cases, or unsatisfied settles.
The final guarded Windows run stayed headless, left no capture process or
named-pipe state, closed port 1420, and reported no visible-window violation.
All 30 cases are exact after reviewed pointers isolate native working
directories, eager background ConPTY grid materialization, and an equivalent
49.5-point floating serialization tail. The lane covers v2 and CLI list,
surface-list, focus, last, swap, break, join, creation, and resizing behavior,
including help, output, ambient scope, terminal/browser creation, payloads,
errors, state, refs, rendered geometry, grid metrics, and lifecycle-event
order. `cli:new-pane` and `cli:resize-pane` are now verified; their v2 methods
were already verified.

Pane resize now consumes the workspace's rendered portal geometry instead of
the 1px pre-render fallback, so a two-pixel request moves the captured 95px
split by exactly two pixels. Pane grid projection resolves the authoritative
selected surface kind before legacy pane metadata and uses the captured 4px
horizontal/32px vertical terminal chrome. Browser panes no longer expose
terminal metrics. The implementation also moved dispatch context and the new
projection regression into focused child modules; the affected oversized
files are net 13 lines smaller and their budgets were ratcheted down.

Windows now projects pane frames from the rendered workspace authority and
publishes xterm-measured cell dimensions through the terminal resize boundary.
Headless first activation suppresses grid fields until canonical exposes them;
fresh projections outrank stale retained values, while moved panes retain the
last confirmed grid across runtime handoff. Per-panel caches are pruned against
the active model. The viewport state and resize command live in the 138-line
`terminal/viewport_metrics.rs` child; `terminal.rs` fell from 2,902 to 2,895
lines and `control_socket.rs` remains at its 3,854-line ceiling.

Closed windows now retain canonical recoverable routes. A socket-managed close
appends a strict `visible:false` `window.list` row with stable window and
workspace identity; failed native closes discard staged history, live rows win
identity collisions, and restart clears the in-process history. The harness now
settles on non-visibility rather than incorrectly requiring row absence. The
matched CLI-created rows preserve their selected workspace identity and count.
Repeated close of a committed recoverable route is now idempotent: it returns
the same window id/ref with no duplicate state mutation or lifecycle event.
The exact case's response and error lanes match canonical; its native `key`
leaf is now an exact reviewed platform equivalence.
Selector-less `workspace.list` also remains routed to the active recoverable
TabManager after its native window closes. Explicit selectors and live active
windows retain their existing routes; no closed window is resurrected.
Transient invisible native rows now retain native visibility/key state while
using the committed recoverable workspace payload, so teardown timing cannot
produce empty `window.list` rows. DEV last-window close publishes the canonical
`window.closed`/`window.unkeyed` sequence, closes the request connection without
a response frame, and terminates the app. The full 68-case family is green.

The shared window identity boundary is now repaired. Socket-created windows
use canonical UUID identities end to end, selector-less `window.current`
returns the active session UUID, and CLI focus/close-by-UUID reach the backend
instead of failing transport. Across two full Windows captures, normalized
`window-N` identity occurrences fell from 153 to zero. The three CLI cases now
match in every strict semantic field; their exact native `key` leaves are
reviewed platform equivalences rather than three implementations to patch.

Window creation now publishes the canonical initial lifecycle sequence from
one prepared snapshot: `surface.selected`, `pane.focused`, `surface.focused`,
`workspace.created`, `surface.created`, `workspace.selected`, and
`window.created`, followed by `window.closed` in the exercised create/close
case. Duplicate derived session events are suppressed for socket-managed
create/close, while normal UI window operations retain them. Residual event
differences are limited to native fallback-key identity and platform home paths
in the affected cases.

Window focus and key-window close now publish the canonical AppKit lifecycle
sequence. A focus transfer emits `window.unkeyed`, `window.keyed`, then
`window.focused`; closing that key window emits `window.closed`,
`window.unkeyed`, then `window.keyed` for the fallback. Focusing an already-key
window still emits only `window.focused` before the cleanup-close sequence.
Two 10-case prefixes and full 68-case runs reproduce the exact event names,
order, origins, and key/main flags with zero capture errors. AppKit and the
isolated Windows desktop choose different native fallback key owners; the
contract explicitly classifies real OS key transfer and its keyed/unkeyed
ownership as platform-equivalent, now encoded only at the exact JSON leaves.

Resume bindings now use the signed approval store already shared with the
canonical port. A successful CLI set writes or reuses a manual approval record,
returns `approval_policy: "manual"` plus its UUID, and persists both fields in
session state. Malformed resume selectors are rejected in canonical key order
before routing. The approval and selector payload repairs remain exact. Under
matched UI-test mode, the restart case is also exact; the earlier apparent
resume delta came from comparing unlike environments and is not a product gap.

The retained workspace-lifecycle lane now covers 19 public v2 and CLI cases.
Capture integrity is clean on both platforms: zero capture errors, missing
cases, or unsatisfied settles. The final selected-create repair publishes the
canonical focus trio, `workspace.created`, `surface.created`, and
`workspace.selected` without generic session-model noise. Together with the
earlier selection, rename, and close repairs, the archived comparison is now
19/19 exact after reviewed native-directory pointers. Windows capture SHA-256
is `D2180F84DE5D7B81D0723BF2A433EF7996AAFF0BAD98F7E1F5D77386B0D85A07`;
normalized comparison SHA-256 is
`B2E4E58CB02D900FDA57C2CACAB35EE82C2931E89031C63CD2CE5A913E2D95F0`.
The canonical capture, Windows capture, normalized comparison, manifest, and
provenance are retained at `parity/diff-lane@3ff4aee606`; see
`evidence/workspace_lifecycle_2026-07-19.json`.

Workspace next, previous, and last navigation are now exact across ten retained
v2 and CLI cases. The lane covers explicit-window, workspace-owner, and active
window routing; wraparound; invalid selectors; history and fresh-window
behavior; canonical CLI summaries; and exact lifecycle event order. Both
captures have zero errors, and the normalized comparison is 10/10 exact after
three reviewed native-directory pointers. The Windows capture ran in isolated
headless mode: four Tauri windows were created and none became visible. See
`evidence/workspace_navigation_2026-07-19.json`.

Workspace ordering and cross-window movement are now exact across thirteen
retained v2 and CLI cases. The lane covers explicit-window and workspace-owner
routing, dry-run plans, atomic batch ordering, duplicate and missing-order
errors, no-op event suppression, stable refs, cross-window state, focus intent,
and canonical CLI summaries. Responses, errors, state other than reviewed
platform working directories, multiwindow probes, and lifecycle events are
13/13 exact. The isolated capture created the main and four auxiliary Tauri
windows with zero visible windows before or after the cases. See
`evidence/workspace_ordering_2026-07-19.json`.

Workspace-group lifecycle behavior is now exact across twenty-four retained
cases covering all seventeen public v2 methods and the `workspace-group` CLI
entry point. The lane verifies list, create, rename, collapse/expand,
pin/unpin, membership and anchor changes, new grouped workspaces, color/icon
metadata, ordering, focus, ungroup, delete, canonical errors, CLI output,
multiwindow routing, state, and exact lifecycle-event order. Both captures
have zero errors, missing cases, or unsatisfied settles; the comparison is
24/24 exact after reviewed platform-directory pointers. The isolated Windows
run stayed headless, including the focus path that previously surfaced a
development WebView at `localhost:1420`. See
`evidence/workspace_group_lifecycle_2026-07-19.json`.

## What the current audit says

The rolling catalog contains 503 rows: 159 public CLI commands, 16 internal CLI
contracts, 263 release socket methods, 46 debug socket methods, and 19 coarse
product umbrellas. It currently classifies 82 rows as verified, 3 as reviewed
platform equivalents, 197 as implemented but unverified, and 221 as missing.
The strict resolved count is 85. These are entry-point rows, not a user-facing
completion percentage.

The zero-delta window lane promotes 11 entry-point rows. `window.create`,
`window.close`, the three resume methods, their covered CLI commands, and
`surface-resume` are exact. `window.focus`, `focus-window`, and
`surface.refresh` use the contract's tested Win32 foreground/renderer
equivalents. `cli:window` and the broad `product.window_lifecycle` umbrella stay
unverified because the retained headless lane does not perform a successful
physical display move. These current decisions live in
`current-overrides.json`; the frozen matrix and its 14 historical promotions
remain unchanged.

The zero-delta workspace lanes promote 42 exercised public entry points: the
v2 and CLI list/current/create/select/rename/close pairs; next, previous, and
last navigation; single and batch ordering; cross-window movement; all
seventeen public workspace-group v2 methods; and the `workspace-group` CLI.
The separate mobile-host `workspace.group.action` route remains unverified.
The broad `product.workspace_lifecycle` umbrella remains implemented but
unverified because restore, persistence, remote workspaces, and
`workspace.action` semantics remain outside the retained families.

Those numbers are useful for finding API gaps. They are not a percentage of the
user experience. The catalog still needs a deduplicated user-capability layer
before a defensible product-completion percentage exists. See
`current-audit.json` for exact provenance and upstream drift.

The separate frozen snapshot still validates 496 entries against 222 pinned
source blobs. That is the source-integrity result for the old acceptance
baseline, not "222 of 496 complete" and not the rolling catalog.

## Known acceptance blockers

1. Unintegrated terminal/mobile viewport work remains quarantined and is not
   counted as parity progress until its cancellation and runtime behavior are
   re-audited on `windows-port`.
2. Implemented behavior needs evidence promotion in coherent capability batches;
   raw route or help-text presence is not verification.

The differential-remediation blocker is closed at `246172de0d`. Surface create
now uses only canonical bonsplit `focusedPaneId` authority instead of inferring
explicit pane focus from the workspace's selected surface. The same one-line
predicate repair restores the non-focus select/revert event sequence, preserves
the prior pane selection, and suppresses the later no-op close pair. All 47
remediation tests pass twice; the full desktop library gate is 1,058 passed,
1 ignored, and 0 failed.

The normal-startup restore blocker is closed. Exact Windows capture
`99bd001c6e` matches retained canonical run `29674972316` in all semantic
fields: two-window ownership and routing, target refs `5,6,4`, moved-surface
identity and focus on `surface:6`, default terminal titles, remote payload
shape, and non-null restored directories. Ten filesystem strings are explicit
platform-path equivalences. The normalized comparison is 1/1 identical with
zero unexplained deltas, so `surface.list/close/focus/move` and
`product.pane_surface_lifecycle` are now verified. See
`evidence/startup_restore_2026-07-19.json`.

## Maintenance checkpoint

- Historical root handoffs and reports now live under
  `docs/archive/windows-port-legacy/`; they are preserved evidence, not active
  instructions.
- The previously monolithic files remain split: `control_socket.rs` is now
  3,874 physical lines (from 23,219), `session.rs` is 5,079 (from 12,059), and
  `CustomSidebarSurface.tsx` is 4,795 (from 12,003). Extracted modules retain
  the same public entry points. The 106-line `session/control_snapshot.rs` owns
  control-worker lifecycle publication policy, and the 85-line
  `session/control_window_registration.rs` owns prepared window registration.
  Workspace ordering now uses focused 175-line payload, 67-line request-parser,
  and 75-line transaction modules. The tracked `payloads.rs` and
  `workspace_control.rs` ceilings fell to 2,946 and 2,914 lines without raising
  any file budget.
  The 1,076-line `session/commands.rs` now owns Tauri command adapters and URI
  routing; its moved body is byte-equivalent after four scope-preserving
  visibility annotations, and 308 session-scoped tests cover the unchanged API
  boundary.
- Custom-sidebar action policy, schema validation, and native bridge reply
  shaping now live in the 554-line
  `control_socket/custom_sidebar_action.rs` module. The control-socket root
  retains only the Tauri command wrapper. Six focused behavior tests pass
  before and twice after extraction; the full desktop library remains 1,059
  passed and 1 ignored, and the all-target check is green.
- Workspace lifecycle event construction/publication now lives in the
  366-line `control_socket/workspace_control/events.rs` child. This restores
  `event_stream.rs` to its 1,558-line ceiling and lowers
  `workspace_control.rs` to 2,914 lines. Workspace selection's pure transaction
  candidate lives in the 25-line `session/workspace_selection.rs` child, and
  its payload test moved from the catch-all control-socket test root into the
  existing workspace-action suite. The focused selection tests pass twice,
  the full desktop library remains green, and no behavior path changed.
- Workspace rename request handling now lives in the 61-line
  `workspace_control/rename.rs` child. Changed and unchanged renames retain
  persistence and frontend publication while reseeding, rather than emitting,
  generic derived-session events; both v2 and CLI paths publish one canonical
  `socket.v2` event. Three focused tests pass twice, the full desktop library
  remains green, and the live differential makes both rename cases exact.
- Workspace close request handling now lives in the 58-line
  `workspace_control/close.rs` child. The shared v2/CLI path suppresses only
  generic derived-session events, preserves normal snapshot publication and
  runtime teardown, then publishes the canonical two-event close sequence.
  Five focused close tests and the full 1,060-test desktop library pass; one
  test remains ignored, all targets compile, and both live close cases are
  exact after native-directory review.
- Workspace-create directory, environment, and layout validation now lives in
  the 74-line `workspace_control/create_params.rs` child. Selected and
  background creates share one canonical event-spec path; four focused create
  tests and the full 1,062-test desktop library pass, one test remains ignored,
  and the live selected-create case is exact.
- The legacy workspace-alias notice now lives in the 21-line CLI
  `legacy_alias.rs` module while retaining the same broken-pipe-safe stderr
  writer. `main.rs` is 3,071 lines; the exact executable notice test passes
  twice, all CLI targets pass, and the file-length guard is green.
- Canonical window-event construction now lives in the 319-line
  `control_socket/window_lifecycle/events.rs` child module. The parent remains
  below its frozen file-length ceiling, and the production transition and
  publication executor share prepared window identities and key history.
- `browser.rs` is now 3,284 physical lines. Its control-runtime presence checks
  live in the 37-line `browser/control_state.rs` child module.
- `terminal.rs` is now 2,895 measured lines (from 5,672 at the previous
  checkpoint). Its unchanged 2,772-line inline test body now remains in the
  same `terminal::tests::*` namespace through two include files of 1,425 and
  1,347 lines. Logical reconstruction matches the former file exactly; the
  68-test terminal-filtered run passes before and three times after the move,
  and the desktop all-target check is clean. Process-tree snapshots, listening-port
  discovery, and terminal output pumping already live in the 454-line
  `terminal/process_runtime.rs` child module. Viewport measurement, retained
  pane-grid state, and the resize command now live in the 138-line
  `terminal/viewport_metrics.rs` child module.
- `session_ops.rs` is now 1,316 physical lines (from 6,306 before its staged
  extractions). Browser history,
  navigation, developer-tools state, and zoom mutations live behind the
  unchanged public API in the 325-line `session_ops/browser.rs` child module;
  canvas layout and persisted geometry mutations now live behind the same API
  in the 526-line `session_ops/canvas.rs` child module. Workspace grouping and
  reorder operations now live in the 1,116-line
  `session_ops/workspace_ordering.rs` child module. Pane-tree mutation,
  resizing, surface-tab movement, and pane metadata transfer now live in the
  1,320-line `session_ops/pane_layout.rs` child module. Its former 3,675-line
  inline test module now keeps the same `session_ops::tests::*` namespace
  through a four-line include host and four focused files ranging from 537 to
  1,244 lines. That move is exactly net-zero: 3,678 additions and 3,678
  deletions. The 297 core tests pass before and twice after simplification,
  core Clippy is clean, and desktop/CLI dependent targets compile.
- `command_forward.rs` is now 3,599 physical lines (from 7,026). Browser
  routing and parameter construction live in the 1,209-line
  `command_forward/browser.rs` child module; workspace routing, selectors,
  metadata, grouping, and environment parsing live in the 1,299-line
  `command_forward/workspace.rs` child module. Shared frozen-option parsing,
  selector precedence, terminal startup/environment parsing, and `ParsedArgs`
  now live in the 952-line `command_forward/arguments.rs` include. Expanding
  the include reconstructs all 4,550 former lines exactly; the move is one net
  ownership line with no visibility changes. The 251-test CLI suite passes
  before the move and twice after it, and the full all-target matrix and compile
  check are green.
- `customSidebarSwiftParser.ts` is now 3,719 physical lines (from 5,417 at the
  previous checkpoint). Balanced call, closure, delimiter, ternary, and
  top-level operator scanning now lives in the 270-line abstract
  `SwiftSyntaxReader.ts` base. The 268 moved method-body lines are exact after
  normalizing `private` to the required `protected` inheritance boundary; the
  only parent changes are its import, inheritance, and `super()` call.
  Expression evaluation, collection transforms, formatting, date/measurement
  handling, and geometry built-ins now live in the 1,466-line abstract
  `SwiftExpressionEvaluator.ts` base. Its 1,426 moved implementation lines are
  exact after normalizing `protected` back to `private`; eight explicit hooks
  retain the parser-owned view/function helpers, and seven obsolete parent type
  imports were removed. The 92-test custom-sidebar suite passes before and
  twice after the extraction,
  the full web suite passes 1,253 tests across 81 files, and typecheck, web
  production build, whitespace, and the 39-file Windows length budget are
  clean. No user-facing strings or localization resources changed.
- `CustomSidebarSurface.tsx` is now 4,795 physical lines (from 5,419 at the
  previous checkpoint). Its JSON block renderer, workspace filtering, row
  actions, and limits now live in the 278-line `CustomSidebarJsonView.tsx`
  child. All 266 moved implementation lines are exact after normalizing the
  child export; the parent change is one import and two retired type imports.
  Its accessibility and Swift data-attribute projection now lives in the
  360-line `SwiftAccessibilityProps.ts` child behind a type-only dependency;
  all 358 moved implementation lines are exact after normalizing the exported
  function and inferred presentation type boundary.
  The 92-test custom-sidebar suite passes before and twice after simplification,
  the full web suite passes 1,253 tests across 81 files, and typecheck, web
  production build, whitespace, and the 39-file Windows length budget are
  clean. All rendered strings moved unchanged, so no localization resources
  changed.
- CLI `main.rs` is now 3,071 physical lines (from 4,059). Control-result text,
  JSON projection, id formatting, and tmux-state pruning live in the 997-line
  `control_output.rs` child module with 11 explicit parent-visible functions.
  The move adds 19 net module/import/visibility lines; the full CLI all-target suite
  passes before the move, after extraction, and after visibility tightening.
- `workspace_control.rs` is now 3,021 physical lines (from 3,711 at this
  checkpoint). Strict live/recoverable `window.list` projection and read-only
  recoverable active routing live in the 299-line
  `workspace_control/window_list.rs` child. Right-sidebar, feed, and
  notification socket controls moved mechanically to the 495-line
  `workspace_control/activity_controls.rs` child; 21 notification, 5 feed, and
  5 right-sidebar tests pass twice after visibility simplification, and the
  desktop all-target check is clean. The extraction commit is +500/-495: five
  net ownership lines, no behavior rewrite.
- Control window enumeration now uses native top-level windows, so adding a
  child browser WebView cannot erase its owning window from socket state. The
  isolated before/after capture retains refs `window:1,2,3`; two 68-case runs
  complete without capture errors and retain `window:2` through the CLI probes.
- Seventy source-text tests that asserted filenames, function spelling, or
  substring order were removed. The retained suites execute behavior.
- CI now caps new Windows-owned Rust and TypeScript files at 1,500 lines and
  freezes 38 existing oversized files at their current-or-smaller sizes.
- The next structural priorities are
  the 5,975-line custom-sidebar test file, 5,026-line session root,
  4,795-line custom-sidebar component, and 4,349-line pane/surface lifecycle
  module. The Swift parser is now 3,719 lines. Split one coherent
  responsibility per isolated maintenance commit only when it unblocks feature
  work; do not turn structural cleanup into the parity metric.
- `pane.break` and `pane.join` now commit through the explicit pane/surface
  lifecycle transaction instead of the legacy derived-session path
  (`078b8f26c7`, `3dffa36a42`, `55b9503bf6`). Canonical detach/fallback/attach,
  selection/focus, and socket completion events are pinned for both focused and
  already-focused targets, including public refs and explicit surfaces. The
  production cleanup at `6bec4dcc47` removed the unreachable legacy handlers:
  390 deleted lines versus 36 added coverage lines. The 22-case live lane at
  that checkpoint is valid (no missing, capture-error, or unsettled cases),
  keeps 12 exact cases and 10 geometry-only deltas, and makes all four direct
  and CLI break/join event lanes exact. The owned capture reported no visible
  window violation; its desktop, Vite, listener, and port 1420 were stopped.
- The same slice reduced the oversized roots without behavioral rewrites at
  `65e48faddf`: `surface_move` is now a 142-line child, manual-restore event
  projection is a 29-line child, `pane_surface_control.rs` fell from 3,030 to
  2,840 lines, `pane_surface_lifecycle.rs` from 4,494 to 4,357, and
  `event_stream.rs` from 1,561 to 1,537. The 39-file Windows length budget was
  fully green. Verification is 5/5 break/join tests, 9/9 move tests, 10/10
  restore tests, 1,117 passed plus 1 ignored desktop library tests, all three
  desktop process tests, 30/30 repository parity tests, and 120/120
  differential-lane tests.

## Next efficient slice

Build one retained notification-family lane covering the seven implemented
v2 methods and their seven CLI entry points: create/notify, list, mark-read,
dismiss, clear, open, and jump-to-unread. This promotes up to fourteen rows in
one fixture and exercises shared routing, target identity, unread state,
ordering, mutation events, canonical errors, and CLI output. Keep the three
specialized create variants and reconcile route outside the lane until their
currently missing implementations are addressed; do not infer their behavior
from the covered public methods.
