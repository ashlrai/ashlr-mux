# WS1 — `@cmux/core-types` TS package + generator (M1)

Status: **complete**. Branch `windows-port/m1-ws1-ts-types`.

Builds a thin TypeScript package whose types are generated from the Rust `serde`
session/layout structs, so the web chrome and the Rust core can never drift on
the session-snapshot wire format (M1 spec WS1; cross-cutting rule #1).

## Package location (and why)

`apps/desktop/packages/core-types` (package name `@cmux/core-types`).

Rationale:
- The bun web app already lives at `apps/desktop/web` (`@cmux/desktop-web`) and
  the root `package.json` declares bun workspaces. The new package is a sibling
  under the desktop app, so it sits next to its only consumer and shares the
  same workspace/toolchain.
- Root-level `packages/` is **not** available for TS — it already holds the
  Swift SPM groups (`packages/{iOS,macOS,Shared}`), and CLAUDE.md documents that
  those folders are the source of truth for the Xcode workspace. Putting a TS
  package there would collide with that convention.
- I added `apps/desktop/packages/*` to the root `workspaces` array so bun picks
  it up. `@cmux/` scope matches the existing `@cmux/desktop-web` naming.

## Files added / changed

Added:
- `apps/desktop/packages/core-types/package.json` — `@cmux/core-types`, scripts.
- `apps/desktop/packages/core-types/tsconfig.json` — strict, bundler resolution.
- `apps/desktop/packages/core-types/README.md`
- `apps/desktop/packages/core-types/src/index.ts` — re-exports the generated barrel.
- `apps/desktop/packages/core-types/src/generated/*.ts` — 11 committed generated
  files (10 types + `index.ts` barrel).
- `apps/desktop/packages/core-types/scripts/lib.mjs` — shared generate logic.
- `apps/desktop/packages/core-types/scripts/generate.mjs` — writes `src/generated`.
- `apps/desktop/packages/core-types/scripts/check-drift.mjs` — drift gate.

Changed:
- `crates/cmux-core/Cargo.toml` — optional `ts-rs` dep behind a new `ts` feature.
- `crates/cmux-core/src/session.rs` — feature-gated `#[cfg_attr(feature="ts",
  derive(TS), ts(export))]` on the 9 snapshot types + the orientation enum, a
  manual `impl TS` + an `export_manual_bindings` test for the hand-serialized
  tagged union, and per-field `ts(optional)` / `ts(type="number")` overrides.
- `package.json` — added `apps/desktop/packages/*` to workspaces and
  `core-types:generate` / `core-types:check-drift` root scripts.
- `Cargo.lock`, `bun.lock` — dependency lockfiles.

## How generation works

ts-rs is an **optional** dependency gated behind the `ts` cargo feature:

```toml
[features]
ts = ["dep:ts-rs"]
```

Every wire type carries `#[cfg_attr(feature = "ts", derive(TS), ts(export))]`, so
the derives (and ts-rs itself) are completely inert in the default build. ts-rs's
`#[ts(export)]` generates, for each marked type, a unit test named
`export_bindings_<type>` that writes `<Type>.ts` into `$TS_RS_EXPORT_DIR` when
`cargo test` runs.

The generator (`scripts/lib.mjs`) runs:

```
TS_RS_EXPORT_DIR=<dest> cargo test -p cmux-core --features ts --quiet
```

then normalizes each file's header to a stable "AUTO-GENERATED … do not edit"
banner. Run it via:

```bash
bun run core-types:generate              # from repo root
bun run generate                         # from the package dir
```

### Wire-shape fidelity (matches the JSON exactly)

- `skip_serializing_if = "Option::is_none"` fields -> `field?: T` (omitted when None).
- `layout` (`#[serde(default)]`, no skip) serializes as `"layout": null` -> typed
  `T | null`, **not** optional.
- `i64` fields are forced to `number` via `#[ts(type = "number")]` (ts-rs would
  otherwise emit `bigint`, which is wrong for JSON numbers parsed by `JSON.parse`).
- `SessionSplitOrientation` -> `"horizontal" | "vertical"` (serde `rename_all = lowercase`).
- The tagged union `SessionWorkspaceLayoutSnapshot` uses a variant-named content
  key (`{type:"pane",pane} | {type:"split",split}`) which ts-rs derive cannot
  express. It has hand-written `serde` impls in Rust; I mirror that with a manual
  `impl TS` (so dependent types compile/import under `--features ts`) plus the
  `ts_export::export_manual_bindings` test that writes that one `.ts` and the
  barrel `index.ts` by hand — kept byte-exact with the Rust serializer.

## How to run the drift check

```bash
bun run core-types:check-drift           # from repo root
bun run check-drift                      # from the package dir
```

It regenerates into a temp dir and compares byte-for-byte against the committed
`src/generated/`. Non-zero exit + a per-file report on any drift; the fix is
`bun run core-types:generate` + commit.

## Action-id extension point left for WS3

WS3 owns `crates/cmux-core/src/shortcuts.rs` and the `Action` id catalog; I did
**not** touch that file. The extension point is:

1. When WS3 lands the id type, add `#[cfg_attr(feature="ts", derive(TS),
   ts(export))]` to it (same pattern as the session types).
2. Add one line to the barrel block inside
   `crates/cmux-core/src/session.rs::ts_export::export_manual_bindings`:
   `export type { ActionId } from "./ActionId";`
   The barrel already carries a placeholder comment marking exactly where.

No other file changes; the package's public surface (`src/index.ts` ->
`src/generated/index.ts`) re-exports the whole barrel automatically.

## Verification results

- `cargo test -p cmux-core` (DEFAULT build) — **10 passed**, ts-rs not compiled.
- `cargo clippy -p cmux-core --all-targets -- -D warnings` (DEFAULT) — **clean**.
- `cargo clippy -p cmux-core --all-targets --features ts -- -D warnings` — **clean**
  (the `failed to parse serde attribute` lines are ts-rs proc-macro notes about
  `skip_serializing_if`'s string value, not clippy warnings; the build finishes OK).
- `bun install` + `bun run typecheck` (tsc --noEmit) in the package — **pass**.
- `bun run core-types:check-drift` — **no drift** against committed output.

Known environmental note: `cargo test` for `cmux-ipc` (and any native
`cmux-desktop`/Tauri build) fails here with `os error 4551` ("Application Control
policy has blocked this file" — the test binary is *never executed*). This is the
sandbox's Windows Application Control policy, identical to the family documented
in `NEXT_AGENT_NOTE.md`, not a code defect. `cmux-core` (the only crate WS1
touches) runs fine. Validate the blocked binaries on a non-locked-down runner
(CI does).

## Exact CI steps to wire (I did NOT edit `.github/workflows/ci.yml`)

The `desktop bootstrap (${{ matrix.os_name }})` job (`ci.yml`, ~line 345) already
runs on the `windows-latest` + `macos-latest` matrix, sets up Rust + Bun, runs
`bun install --frozen-lockfile`, and runs `cargo test -p cmux-core ...`. Add **one
step** to that job — right after "Verify desktop contracts" (line ~381):

```yaml
      - name: Check core-types drift (Rust <-> TS)
        run: bun run core-types:check-drift
```

This requires no new tooling: ts-rs is pulled only by the `--features ts` cargo
invocation inside the script, and the step both proves the Rust crate builds with
the `ts` feature and that the committed bindings are current. It runs on both
Windows and macOS via the existing matrix, satisfying the cross-platform intent.

(Optional, if a Linux-only fast gate is wanted instead of/in addition to the
matrix: add the same `bun run core-types:check-drift` step to the existing
`desktop-web` job around line 248, which already does `bun install` on the Linux
runner and has Rust available via the toolchain. The matrix step above is the
minimal, sufficient wiring.)
