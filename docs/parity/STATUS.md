# Windows parity checkpoint

Checkpoint commits:

- Canonical cmux: `c9f2d8c4382e29db89a030d80d02d8174ef7f2ac`
- Windows behavior: `758e80606a0ab1e8e2116844f5cf5993beefb78e`
- Latest pushed verification checkpoint: `637d63acb0e82a618fdaee64644a7ac75b2b8a05`

The Windows desktop and CLI build successfully. The broad desktop, web, IPC,
workspace, parity, and contract suites passed on the Windows behavior commit.
An isolated-profile live differential on the exact pushed verification commit
produced 43 identical pane/surface observations with zero deltas. The user
session file remained byte-for-byte unchanged, so nine entries covered by the
lane are now strictly verified.

## What the current audit says

The rolling catalog contains 503 rows: 159 public CLI commands, 16 internal CLI
contracts, 263 release socket methods, 46 debug socket methods, and 19 coarse
product umbrellas. It currently classifies 9 rows as verified, 268 as
implemented but unverified, and 226 as missing.

Those numbers are useful for finding API gaps. They are not a percentage of the
user experience. The catalog still needs a deduplicated user-capability layer
before a defensible product-completion percentage exists. See
`current-audit.json` for exact provenance and upstream drift.

## Known acceptance blockers

1. Terminal and mobile-terminal viewport handling has a quarantined concurrency
   fix that must await accepted-handler cancellation before integration.
2. Implemented behavior needs evidence promotion in coherent capability batches;
   raw route or help-text presence is not verification.

## Maintenance checkpoint

- Historical root handoffs and reports now live under
  `docs/archive/windows-port-legacy/`; they are preserved evidence, not active
  instructions.
- `control_socket.rs` fell from 23,219 to 4,171 measured lines, `session.rs`
  from 12,059 to 5,896, and `CustomSidebarSurface.tsx` from 12,003 to 5,333.
  Extracted modules retain the same public entry points.
- Seventy source-text tests that asserted filenames, function spelling, or
  substring order were removed. The retained suites execute behavior.
- CI now caps new Windows-owned Rust and TypeScript files at 1,500 lines and
  freezes 40 existing oversized files at their current-or-smaller sizes.
- The next structural priorities are `crates/cmux-core/src/session_ops.rs`,
  `crates/cmux-cli/src/command_forward.rs`, `terminal.rs`, and the custom
  sidebar Swift parser. Split them in isolated maintenance commits, not inside
  feature slices.

## Next efficient slice

Finish the pane/surface family without broadening scope: add the missing
restore/multiwindow evidence for surface.list, surface.close, surface.focus,
surface.move, and the product-level lifecycle invariant. Keep that evidence
slice separate from terminal viewport integration and structural refactors.
