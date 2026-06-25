# M1 — Cross-platform core extraction (status)

## Current state (verified 2026-06-25)
The M1 Rust crates now **compile, test, and lint clean**. The "likely won't
compile" warnings from the previous handoff are resolved.

Verified locally:
- `cargo check --workspace` — clean.
- `cargo test --workspace` — green (39 unit tests: cmux-core 10, cmux-ipc 12,
  cmux-terminal 9, cmux-agent 5, cmux-desktop 3).
- `cargo clippy -p cmux-core -p cmux-ipc -p cmux-terminal -p cmux-agent -p cmux-cli --all-targets -- -D warnings` — clean.

> Note: `cargo clippy --workspace` (and any native build of `cmux-desktop`)
> currently fails in this sandbox with `os error 4551` (Windows Application
> Control policy blocks the tauri build-script). This is environmental, not a
> code defect — same family as the e2e native-launch skip. Validate the desktop
> crate on a non-locked-down machine.

## Compile fixes applied (the "likely issues" from the prior note)
- `crates/cmux-core/src/session.rs`
  - Boxed the recursive `first`/`second` fields of `SessionSplitLayoutSnapshot`
    (`Box<SessionWorkspaceLayoutSnapshot>`) to break the infinite-size cycle.
    Serde shape is unchanged (Box is transparent).
  - Dropped `Eq` from the snapshot structs that transitively reach the `f64`
    `divider_position` via `layout`: `SessionWorkspaceSnapshot`,
    `SessionTabManagerSnapshot`, `SessionWindowSnapshot`, `AppSessionSnapshot`.
    They keep `PartialEq`. (`SessionCanvasPaneSnapshot` /
    `SessionWorkspaceGroupSnapshot` keep `Eq` — no float reachable.)
- `crates/cmux-core/src/shortcuts.rs`
  - `ShortcutRegex` no longer derives `Eq`/`PartialEq` (regex::Regex isn't
    comparable). Manual `PartialEq`/`Eq` defined on `pattern` only, so the whole
    `ShortcutContextOperand` / `ShortcutWhenClause` chain keeps `Eq`.
- `crates/cmux-ipc/src/lib.rs`
  - Test helper `strict_error` rewritten to `.err()` (clippy `manual_ok_err`).
- `crates/cmux-agent/src/lib.rs`
  - `runtime_search_path` `.map(normalize_path)` → closure taking `&Path`
    (parent() yields `&Path`, normalize_path takes `PathBuf`).

## What still genuinely remains for M1 (was never started)
These are real gaps, not compile fallout — safe to hand to workers:
1. **TS `@cmux/core-types` package + generator** (WS1) — emit `.d.ts` from the
   Rust structs (ts-rs/typeshare) for layout types + the `Action` id list; the
   package does not exist yet.
2. **`Action` enum (180+ cases)** (WS3) — generate from
   `Sources/KeyboardShortcutSettings.swift:64`; no catalog exists yet. Do not
   hand-retype raw values (they are on-disk config keys).
3. **Golden-file parity harness** (WS6) — Swift fixture exporter (macOS runner)
   + `cmux-golden` Rust crate asserting byte-identical JSON. None exists yet.
4. **`scripts/desktop/verify_cmux_contracts.py`** — extend to validate the
   generated core types once (1) lands.
5. **CI wiring** — add the four crates' `cargo test` + `clippy -D warnings` to
   the windows-latest + macos matrix.
6. **Parity tightening** — the current session/shortcut/resolver ports are
   intentionally simplified, not full Swift parity. Expand only after the golden
   harness (3) exists so drift is caught.

## Swift reference sources (unchanged)
- `windows-port-plan/milestones/M1-core-extraction.md` (authoritative spec)
- `Packages/macOS/CmuxControlSocket/Sources/CmuxControlSocket/Wire/{JSONValue,ControlRequestParser,ControlResponseEncoder}.swift`
- `Packages/Shared/CmuxAgentChat/Sources/CmuxAgentChat/Parsing/OSC133CommandParser.swift`
- `Packages/macOS/CmuxSettings/Sources/CmuxSettings/Values/ShortcutWhenClause.swift`
- `Sources/{AgentExecutableResolver,AgentSessionLaunchPlan,SessionPersistence}.swift`

## Coordination note for parallel workers
`crates/` and `apps/` are **untracked** in the `cmux` repo (detached HEAD).
WS3 (Action enum / shortcuts) and WS6 (golden harness) both touch
`crates/cmux-core` — sequence or section them to avoid edit collisions.
