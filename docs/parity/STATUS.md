# Windows parity checkpoint

Checkpoint commits:

- Current canonical audit: `41756f7285a02d2592751438793647f7d4ba9b71`
- Frozen differential canonical: `e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`
- Windows behavior captured: `a51ddd28763bad2549d3903de78bc7b59b988e36`
- Latest Windows code checkpoint: `2c4a5650b383665441d8d916f7137eddb309a68e`
- Latest exact startup differential evidence: `parity/diff-lane@215737999ac0579682b6b2a2d230d7a0d064a51d`
- Latest window harness checkpoint: `parity/diff-lane@9257db511b975335e2ee79bc4bb143c04bfa95f8`
- Latest canonical window capture: workflow run `29686515273`

The Windows desktop and CLI build successfully. The retained pane/surface and
startup-restore evidence remains valid. The exact environment-matched window
pair contains all 68 cases with zero capture errors, zero missing cases, and
zero unsatisfied settles. Its normalized comparison is valid: all 68 cases are
exact or reviewed platform-equivalences, with zero strict deltas. See
`evidence/window_lifecycle_2026-07-19.json` for hashes,
strict lanes, and provenance.

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

## What the current audit says

The rolling catalog contains 503 rows: 159 public CLI commands, 16 internal CLI
contracts, 263 release socket methods, 46 debug socket methods, and 19 coarse
product umbrellas. It currently classifies 22 rows as verified, 3 as reviewed
platform equivalents, 255 as implemented but unverified, and 223 as missing.
The strict resolved count is 25.

The zero-delta window lane promotes 11 entry-point rows. `window.create`,
`window.close`, the three resume methods, their covered CLI commands, and
`surface-resume` are exact. `window.focus`, `focus-window`, and
`surface.refresh` use the contract's tested Win32 foreground/renderer
equivalents. `cli:window` and the broad `product.window_lifecycle` umbrella stay
unverified because the retained headless lane does not perform a successful
physical display move. These current decisions live in
`current-overrides.json`; the frozen matrix and its 14 historical promotions
remain unchanged.

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
  4,396 measured lines (from 23,219), `session.rs` is 5,134 (from 12,059), and
  `CustomSidebarSurface.tsx` is 4,795 (from 12,003). Extracted modules retain
  the same public entry points. The 84-line `session/control_snapshot.rs` owns
  control-worker lifecycle publication policy, and the 85-line
  `session/control_window_registration.rs` owns prepared window registration.
  The 1,076-line `session/commands.rs` now owns Tauri command adapters and URI
  routing; its moved body is byte-equivalent after four scope-preserving
  visibility annotations, and 308 session-scoped tests cover the unchanged API
  boundary.
- Canonical window-event construction now lives in the 319-line
  `control_socket/window_lifecycle/events.rs` child module. The parent remains
  below its frozen file-length ceiling, and the production transition and
  publication executor share prepared window identities and key history.
- `browser.rs` is now 3,284 physical lines. Its control-runtime presence checks
  live in the 37-line `browser/control_state.rs` child module.
- `terminal.rs` is now 2,902 measured lines (from 5,672 at the previous
  checkpoint). Its unchanged 2,772-line inline test body now remains in the
  same `terminal::tests::*` namespace through two include files of 1,425 and
  1,347 lines. Logical reconstruction matches the former file exactly; the
  68-test terminal-filtered run passes before and three times after the move,
  and the desktop all-target check is clean. The commit is +2 net ownership
  lines, with no behavior rewrite. Process-tree snapshots, listening-port
  discovery, and terminal output pumping already live in the 454-line
  `terminal/process_runtime.rs` child module; the Tauri command entry point
  remains in `terminal.rs`.
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
- `customSidebarSwiftParser.ts` is now 5,152 physical lines (from 5,417 at the
  previous checkpoint). Balanced call, closure, delimiter, ternary, and
  top-level operator scanning now lives in the 270-line abstract
  `SwiftSyntaxReader.ts` base. The 268 moved method-body lines are exact after
  normalizing `private` to the required `protected` inheritance boundary; the
  only parent changes are its import, inheritance, and `super()` call. The
  92-test custom-sidebar suite passes before and twice after the extraction,
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
- CLI `main.rs` is now 3,081 physical lines (from 4,059). Control-result text,
  JSON projection, id formatting, and tmux-state pruning live in the 997-line
  `control_output.rs` child module with 11 explicit parent-visible functions.
  The move adds 19 net module/import/visibility lines; the full CLI all-target suite
  passes before the move, after extraction, and after visibility tightening.
- `workspace_control.rs` is now 3,180 physical lines (from 3,711 at this
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
  freezes 39 existing oversized files at their current-or-smaller sizes.
- The next structural priorities are
  the remaining 5,152-line Swift parser, the 4,795-line custom-sidebar
  component, and their 5,975-line test file, followed by the remaining
  session/control and terminal domains. Split one coherent responsibility per
  isolated maintenance commit; do not mix those moves with feature slices.

## Next efficient slice

Map the remaining 5,152-line Swift parser methods and extract one coherent
expression-evaluation responsibility in a behavior-neutral commit. Keep the
new module below 1,500 lines, preserve moved bodies exactly apart from the
smallest required visibility boundary, and run the focused Bun suite before
and twice after simplification, then the full web suite, typecheck, build, and
file-length gate. Do not mix the move with parity behavior changes or generated
catalog churn.
