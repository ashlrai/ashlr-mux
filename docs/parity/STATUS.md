# Windows parity checkpoint

Checkpoint commits:

- Current canonical audit: `ecebdbb64b3532b0308650280ae4b83f30becf2a`
- Frozen differential canonical: `e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`
- Windows behavior: `4e6e2fc6dc9b71bdc4c2847d3eea7eefc6aa365e`
- Latest Windows code checkpoint: `cb94cf02e2aa326107e8fe8c931ef08521797b53`
- Latest exact startup differential evidence: `parity/diff-lane@215737999ac0579682b6b2a2d230d7a0d064a51d`
- Latest window harness checkpoint: `parity/diff-lane@d309cf0ceb44cda59d755d557f91189848c98357`
- Latest canonical window capture: workflow run `29678884486`

The Windows desktop and CLI build successfully. The retained pane/surface and
startup-restore evidence remains valid. A fresh window-family capture now
completes on both platforms: canonical and Windows each emitted all 68 cases
with zero capture errors. Their normalized comparison has 40 identical cases
and 28 cases with semantic deltas, so the lane is valid diagnostic evidence but
does not yet promote window-family rows to verified. See
`evidence/window_lifecycle_2026-07-19.json`.

The shared window identity boundary is now repaired. Socket-created windows
use canonical UUID identities end to end, selector-less `window.current`
returns the active session UUID, and CLI focus/close-by-UUID reach the backend
instead of failing transport. Across two full Windows captures, normalized
`window-N` identity occurrences fell from 153 to zero. The overall case count
did not move because all affected cases also contain independent event, state,
or payload deltas; it must not be used to erase this narrower verified gain.

Window creation now publishes the canonical initial lifecycle sequence from
one prepared snapshot: `surface.selected`, `pane.focused`, `surface.focused`,
`workspace.created`, `surface.created`, `workspace.selected`, and
`window.created`, followed by `window.closed` in the exercised create/close
case. Event names, ordering, identities, payloads, and key/main flags match the
canonical capture exactly after normalizing only the platform home-directory
path. Duplicate derived session events are suppressed for socket-managed
create/close, while normal UI window operations retain them. The overall
comparison remains 40 identical and 28 delta cases because independent native
teardown timing, window-list ordering/focus, path, and resume payload deltas
remain.

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
before routing. Two post-simplification 31-case runs were identical for all 15
resume cases on Windows; the full differential moved from 38/30 to 40/28.
Eight approval-bearing cases now match every non-identity leaf. Nine resume
cases remain strict deltas only because earlier native window topology assigns
the retained fixture workspace, pane, and surface different global normalized
UUID labels; even the no-binding clear case carries that same offset. This is
recorded as an upstream identity/topology dependency, not as completed strict
parity.

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

1. The window-lifecycle transport, public UUID routing, resume approval fields,
   and selector validation blockers are closed, but 28 of 68 cases still
   differ. The largest shared cause is now initial/native window topology and
   ordering, which shifts fixture workspace/pane/surface identities through
   refresh, resume, CLI state, and restart observations. AppKit's asynchronous
   closed-window rows, native key/fallback choice, and platform paths remain
   separate equivalences; preserve those distinctions instead of changing
   Windows behavior merely to reduce the case count.
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
  4,396 measured lines (from 23,219), `session.rs` is 6,204 (from 12,059), and
  `CustomSidebarSurface.tsx` is 5,419 (from 12,003). Extracted modules retain
  the same public entry points. The 84-line `session/control_snapshot.rs` owns
  control-worker lifecycle publication policy, and the 85-line
  `session/control_window_registration.rs` owns prepared window registration.
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
- `session_ops.rs` is now 4,991 physical lines (from 6,306 in the prior
  checkpoint). Browser history,
  navigation, developer-tools state, and zoom mutations live behind the
  unchanged public API in the 325-line `session_ops/browser.rs` child module;
  canvas layout and persisted geometry mutations now live behind the same API
  in the 526-line `session_ops/canvas.rs` child module. Workspace grouping and
  reorder operations now live in the 1,116-line
  `session_ops/workspace_ordering.rs` child module. Pane-tree mutation,
  resizing, surface-tab movement, and pane metadata transfer now live in the
  1,320-line `session_ops/pane_layout.rs` child module. The latest extraction
  is a behavior-preserving move with five net lines: 297 core tests pass twice,
  core Clippy is clean, and desktop/CLI dependent targets compile.
- `command_forward.rs` is now 4,550 physical lines (from 7,026). Browser
  routing and parameter construction live in the 1,211-line
  `command_forward/browser.rs` child module; workspace routing, selectors,
  metadata, grouping, and environment parsing live in the 1,299-line
  `command_forward/workspace.rs` child module. The 251-test CLI suite and
  all-target compile check pass after each boundary.
- Seventy source-text tests that asserted filenames, function spelling, or
  substring order were removed. The retained suites execute behavior.
- CI now caps new Windows-owned Rust and TypeScript files at 1,500 lines and
  freezes 40 existing oversized files at their current-or-smaller sizes.
- The next structural priorities are the large in-file test module in
  `crates/cmux-core/src/session_ops.rs`,
  `crates/cmux-cli/src/command_forward.rs`, the remaining terminal
  materialization/input domains, and the custom sidebar Swift parser. Split
  them in isolated maintenance commits, not inside feature slices.

## Next efficient slice

Trace the earliest initial-window topology divergence before changing more
resume code. Start from the first `window.list` in
`window_create.params_ignored_junk`: canonical orders refs `window:3,1,2`
before the temporary window while isolated Windows orders `window:1,3,2`.
Identify the smallest ownership/order boundary that also explains the fixture
workspace UUID offset seen by both `surface.refresh` cases and the nine residual
resume cases. Require a strict improvement in the earliest affected case and
no regression in the now-canonical resume payload leaves. Keep AppKit zombie
rows and real OS key selection explicitly outside that implementation slice.
