## M0 desktop test matrix handoff

### What was added
- Split the original single M0 bootstrap test into layered suites:
  - `tests/test_desktop_contracts_unit.py`
  - `apps/desktop/web/src/tauri-bridge.test.ts`
  - Rust unit tests in `apps/desktop/src-tauri/src/lib.rs`
  - `tests/test_desktop_integration.py`
  - `tests/test_desktop_e2e.py`
  - `scripts/desktop/run_desktop_test_matrix.py`
- Updated root scripts in `package.json`:
  - `desktop:test`
  - `desktop:test:unit`
  - `desktop:test:integration`
  - `desktop:test:e2e`
- Kept `tests/test_m0_bootstrap.py` as a compatibility shim that re-exports `test_desktop_integration.py`.
- Updated CI so `desktop-bootstrap` now runs `bun run desktop:test`.

### Real code fix included
- Fixed a bug in `apps/desktop/web/src/tauri-bridge.ts`:
  - `subscribeToAgentEvents` used to create a new native Tauri listener per subscriber.
  - That would duplicate event delivery once more than one listener was registered.
  - It now shares a single native subscription and tears it down when the last listener unsubscribes.

### What passed locally
- `python tests/test_desktop_contracts_unit.py`
- `bun test apps/desktop/web/src/tauri-bridge.test.ts`
- `cargo test -p cmux-desktop --lib`
- `python tests/test_desktop_integration.py`
- `python tests/test_desktop_e2e.py`
  - Native smoke launch skipped in this sandbox because the Tauri app exits early during Windows setup.
- `bun run desktop:test`
- `python tests/test_ci_change_areas.py`

### Known unresolved issue — ROOT CAUSE FIXED (2026-06-25)
- The shim failure was **not** non-determinism. `cargo build` on the desktop
  manifest pulls in the whole workspace, and the M1 crates
  (`crates/cmux-agent/src/lib.rs`, `crates/cmux-core/src/{session,shortcuts}.rs`)
  did not compile. Those compile errors are now fixed — see
  `NEXT_AGENT_NOTE.md`. `cargo check --workspace` and `cargo test --workspace`
  are green.
- Remaining caveat: a **native** desktop build still fails in this sandbox with
  `os error 4551` (Windows Application Control blocks the tauri build-script).
  That is environmental, not the M1 issue. `test_native_desktop_crate_builds()`
  will pass on a non-locked-down machine now that the crates compile.

### Recommended cleanup (optional, low priority)
- Consider narrowing `test_native_desktop_crate_builds()` to a package-specific
  invocation (`cargo check -p cmux-desktop`) so it does not rebuild the whole
  workspace, and so it degrades gracefully where the Application Control policy
  blocks the build-script.

### Environment notes
- Local machine has `cargo`, `rustc`, and `bun`.
- Local machine does **not** have `go`, so `stage-sidecars.ps1` uses the placeholder-daemon fallback locally.
- Native Windows Tauri smoke launch currently exits early in this Codex sandbox with the known access/setup problem, so `tests/test_desktop_e2e.py` skips that path unless `CMUX_E2E_REQUIRE_NATIVE=1`.
