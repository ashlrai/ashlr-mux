# Windows parity checkpoint

Checkpoint commits:

- Current canonical audit: `ecebdbb64b3532b0308650280ae4b83f30becf2a`
- Frozen differential canonical: `e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`
- Windows behavior captured: `55f0afaf4e957a7fd56fe1e7a291492d039edf9f`
- Latest Windows code checkpoint: `1df21ddaf6a1173de84cde12953f547eaacd799a`
- Latest exact startup differential evidence: `parity/diff-lane@215737999ac0579682b6b2a2d230d7a0d064a51d`
- Latest window harness checkpoint: `parity/diff-lane@e23cd72b7807f700a5961b2d8ae44919c810911c`
- Latest completed canonical window capture: workflow run `29685067632`
- Matching replacement canonical capture: workflow run `29686515273` (pending)

The Windows desktop and CLI build successfully. The retained pane/surface and
startup-restore evidence remains valid. The published 57-identical/11-delta
window comparison is diagnostic only: its canonical capture used UI test mode,
but its Windows capture did not. It must not promote parity rows or drive
platform-equivalence decisions. A matching UI-test Windows recapture at harness
`e23cd72b78` completed all 68 cases twice with zero capture errors; exact
canonical workflow run `29686515273` is pending. See
`evidence/window_lifecycle_2026-07-19.json` for hashes and provenance.

Closed windows now match canonical recoverable-route behavior. A successful
close appends a strict `visible:false` `window.list` row with stable window and
workspace identity; failed native closes discard staged history, live rows win
identity collisions, and restart clears the in-process history. The harness now
settles on non-visibility rather than incorrectly requiring row absence. This
removed the old 40/28 settle defect, but the resulting 57/11 pair remains
non-authoritative because of the environment mismatch above.

The shared window identity boundary is now repaired. Socket-created windows
use canonical UUID identities end to end, selector-less `window.current`
returns the active session UUID, and CLI focus/close-by-UUID reach the backend
instead of failing transport. Across two full Windows captures, normalized
`window-N` identity occurrences fell from 153 to zero. The remaining CLI state
deltas share one upstream topology loss (`window:2` disappears on Windows
before those probes); they are not three separate CLI implementations to patch.

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
Two 10-case prefixes and a full 68-case run reproduce the exact event names,
order, origins, and key/main flags with zero capture errors. The strict event
lane remains different because AppKit chooses `window:2` and the isolated
Windows desktop chooses `window:1` as the native prior/fallback key window;
real OS key selection is an explicit platform equivalence in the contract.

Resume bindings now use the signed approval store already shared with the
canonical port. A successful CLI set writes or reuses a manual approval record,
returns `approval_policy: "manual"` plus its UUID, and persists both fields in
session state. Malformed resume selectors are rejected in canonical key order
before routing. The approval and selector payload repairs remain exact. The one
residual resume case is now isolated to restart ownership: canonical reports
the original surface missing, while Windows restores its binding and target.
The differing approval UUID is downstream of that semantic mismatch.

## What the current audit says

The rolling catalog contains 503 rows: 159 public CLI commands, 16 internal CLI
contracts, 263 release socket methods, 46 debug socket methods, and 19 coarse
product umbrellas. It currently classifies 14 rows as verified, 264 as
implemented but unverified, and 225 as missing.

Those numbers are useful for finding API gaps. They are not a percentage of the
user experience. The catalog still needs a deduplicated user-capability layer
before a defensible product-completion percentage exists. See
`current-audit.json` for exact provenance and upstream drift.

The separate frozen snapshot still validates 496 entries against 222 pinned
source blobs. That is the source-integrity result for the old acceptance
baseline, not "222 of 496 complete" and not the rolling catalog.

## Known acceptance blockers

1. The published window pair is not promotable because its UI-test modes differ.
   The repeat-close manifest race is fixed at `65fe6a0242`, and event-reader
   teardown no longer hangs the final Windows case at `e23cd72b78`. Wait for the
   exact matching canonical capture, then regenerate the comparison. Diagnostic
   prefixes locate the earliest live-`window:2` loss near browser-only refresh,
   but that is not yet sufficient evidence for a production change. Restart
   binding ownership and last-window behavior remain candidate semantic gaps.
2. Four differential-remediation unit tests fail unchanged at both pushed
   baseline `0ea973d28d` and behavior checkpoint `286b2d7b67`:
   `closing_an_unselected_tab_suppresses_the_noop_pair`,
   `create_after_explicit_focus_keeps_the_new_tab_selected`,
   `surface_create_without_focus_emits_the_canonical_selection_flip`, and
   `surface_create_without_focus_preserves_pane_selection`. The remaining
   desktop library gate is 1,045 passed, 1 ignored, and 4 explicitly excluded;
   the full gate is red until these expectations and implementation are
   reconciled.
3. Unintegrated terminal/mobile viewport work remains quarantined and is not
   counted as parity progress until its cancellation and runtime behavior are
   re-audited on `windows-port`.
4. Implemented behavior needs evidence promotion in coherent capability batches;
   raw route or help-text presence is not verification.

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
  `CustomSidebarSurface.tsx` is 5,419 (from 12,003). Extracted modules retain
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
- `terminal.rs` is now 5,672 measured lines (from 5,709). Process-tree
  snapshots, listening-port discovery, and terminal output pumping now live in
  the 454-line `terminal/process_runtime.rs` child module; the Tauri command
  entry point remains in `terminal.rs`.
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
- `command_forward.rs` is now 4,550 physical lines (from 7,026). Browser
  routing and parameter construction live in the 1,211-line
  `command_forward/browser.rs` child module; workspace routing, selectors,
  metadata, grouping, and environment parsing live in the 1,299-line
  `command_forward/workspace.rs` child module. The 251-test CLI suite and
  all-target compile check pass after each boundary.
- `workspace_control.rs` is now 3,180 physical lines (from 3,711 at this
  checkpoint). Strict live/recoverable `window.list` projection lives in the
  183-line `workspace_control/window_list.rs` child. Right-sidebar, feed, and
  notification socket controls moved mechanically to the 495-line
  `workspace_control/activity_controls.rs` child; 21 notification, 5 feed, and
  5 right-sidebar tests pass twice after visibility simplification, and the
  desktop all-target check is clean. The extraction commit is +500/-495: five
  net ownership lines, no behavior rewrite.
- Seventy source-text tests that asserted filenames, function spelling, or
  substring order were removed. The retained suites execute behavior.
- CI now caps new Windows-owned Rust and TypeScript files at 1,500 lines and
  freezes 39 existing oversized files at their current-or-smaller sizes.
- The next structural priorities are
  `crates/cmux-cli/src/command_forward.rs`, the remaining terminal
  materialization/input domains, and the custom sidebar Swift parser. Split
  them in isolated maintenance commits, not inside feature slices.

## Next efficient slice

Finish exact environment-matched window evidence: retain the completed 68-case
Windows UI-test capture, download canonical run `29686515273`, and regenerate
the strict normalized comparison. Only then classify path/key equivalences and
instrument the browser-only refresh boundary that first appears to lose live
`window:2`. Change the earliest demonstrated production owner, not the three
downstream CLI probes. Keep native key-window choice and platform paths explicit;
do not hard-code dictionary iteration order.
