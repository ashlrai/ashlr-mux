# Windows parity checkpoint

Checkpoint commits:

- Current canonical audit: `c9f2d8c4382e29db89a030d80d02d8174ef7f2ac`
- Frozen differential canonical: `e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`
- Windows behavior: `99bd001c6eddaf91c3460dd1114ffda5093f82a4`
- Latest Windows code checkpoint: `99bd001c6eddaf91c3460dd1114ffda5093f82a4`
- Latest exact startup differential evidence: `parity/diff-lane@215737999ac0579682b6b2a2d230d7a0d064a51d`

The Windows desktop and CLI build successfully. The broad desktop, web, IPC,
workspace, parity, and contract suites passed on the Windows behavior commit.
An isolated-profile live differential on the exact pushed verification commit
produced 43 identical pane/surface observations with zero deltas. The user
session file remained byte-for-byte unchanged, so nine entries covered by the
lane are now strictly verified.

## What the current audit says

The rolling catalog contains 503 rows: 159 public CLI commands, 16 internal CLI
contracts, 263 release socket methods, 46 debug socket methods, and 19 coarse
product umbrellas. It currently classifies 14 rows as verified, 264 as
implemented but unverified, and 225 as missing.

Those numbers are useful for finding API gaps. They are not a percentage of the
user experience. The catalog still needs a deduplicated user-capability layer
before a defensible product-completion percentage exists. See
`current-audit.json` for exact provenance and upstream drift.

## Known acceptance blockers

1. Terminal and mobile-terminal viewport handling has a quarantined concurrency
   fix that must await accepted-handler cancellation before integration.
2. Implemented behavior needs evidence promotion in coherent capability batches;
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
  4,396 measured lines (from 23,219), `session.rs` is 6,284 (from 12,059), and
  `CustomSidebarSurface.tsx` is 5,419 (from 12,003). Extracted modules retain
  the same public entry points.
- `terminal.rs` is now 5,672 measured lines (from 5,709). Process-tree
  snapshots, listening-port discovery, and terminal output pumping now live in
  the 454-line `terminal/process_runtime.rs` child module; the Tauri command
  entry point remains in `terminal.rs`.
- `session_ops.rs` is now 7,923 measured lines. Browser history,
  navigation, developer-tools state, and zoom mutations now live behind the
  unchanged public API in the 325-line `session_ops/browser.rs` child module.
- Seventy source-text tests that asserted filenames, function spelling, or
  substring order were removed. The retained suites execute behavior.
- CI now caps new Windows-owned Rust and TypeScript files at 1,500 lines and
  freezes 40 existing oversized files at their current-or-smaller sizes.
- The next structural priorities are the remaining layout/workspace domains in
  `crates/cmux-core/src/session_ops.rs`,
  `crates/cmux-cli/src/command_forward.rs`, the remaining terminal
  materialization/input domains, and the custom sidebar Swift parser. Split
  them in isolated maintenance commits, not inside feature slices.

## Next efficient slice

Select one priority-zero implemented-but-unverified capability that already has
a retained canonical lane, and spend the next slice converting verification
debt into a zero-delta promotion. Keep structural extraction separate; the next
maintenance candidate is the largest still-frozen Windows-owned file whose
coherent domain can be moved without changing behavior.
