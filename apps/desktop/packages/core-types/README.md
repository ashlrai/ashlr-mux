# @cmux/core-types

TypeScript bindings for the cmux Rust **core session/layout wire models**.

These types are **generated** from the `serde` structs/enums in
`crates/cmux-core/src/session.rs` via [`ts-rs`](https://github.com/Aleph-Alpha/ts-rs).
They exist so the web chrome and the Rust core can never drift on the session
snapshot wire format (M1 spec WS1; cross-cutting rule #1).

> Do **not** hand-edit anything under `src/generated/`. Regenerate instead.

## Usage

```ts
import type { AppSessionSnapshot } from "@cmux/core-types";
```

## Regenerate

From the repo root (or this package dir):

```bash
bun run core-types:generate        # repo root
# or
bun run generate                   # inside apps/desktop/packages/core-types
```

This runs `cargo test -p cmux-core --features ts` with `TS_RS_EXPORT_DIR`
pointed at `src/generated/`, then normalizes the file headers.

## Drift check (CI)

```bash
bun run core-types:check-drift     # repo root
# or
bun run check-drift                # inside the package
```

Regenerates into a temp dir and fails (exit 1) if the committed
`src/generated/` differs. Run `generate` and commit the result to fix.

## What is generated

| Rust type | TS type |
| --- | --- |
| `SessionPaneLayoutSnapshot` | object |
| `SessionSplitOrientation` | `"horizontal" \| "vertical"` |
| `SessionSplitLayoutSnapshot` | object (recursive) |
| `SessionWorkspaceLayoutSnapshot` | tagged union `{type:"pane",pane} \| {type:"split",split}` |
| `SessionCanvasPaneSnapshot` | object |
| `SessionWorkspaceSnapshot` | object |
| `SessionWorkspaceGroupSnapshot` | object |
| `SessionTabManagerSnapshot` | object |
| `SessionWindowSnapshot` | object |
| `AppSessionSnapshot` | object |

### Wire-shape fidelity notes

- Fields with `#[serde(skip_serializing_if = "Option::is_none")]` are emitted as
  optional (`field?: T`) — omitted from JSON when `None`.
- `layout` uses `#[serde(default)]` **without** `skip_serializing_if`, so it
  serializes as `"layout": null` and is typed `T | null` (not optional).
- `i64` fields are typed `number` (not ts-rs's default `bigint`) because the
  JSON wire shape is a plain number consumed via `JSON.parse`.
- The `SessionWorkspaceLayoutSnapshot` union has hand-written `serde` impls with
  a variant-named content key (`pane`/`split`) that ts-rs derive can't express;
  its `.ts` and the barrel are written by the `ts_export` test in
  `crates/cmux-core/src/session.rs`, and a manual `impl TS` keeps dependents
  compiling and importing correctly.

### WS3 extension point

The keyboard `Action` id catalog (WS3, `crates/cmux-core/src/shortcuts.rs`) is
**not** generated here yet. When WS3 lands, derive `TS` + `#[ts(export)]` on its
id type and add one `export type { ActionId } from "./ActionId";` line to the
barrel block in `crates/cmux-core/src/session.rs::ts_export` (the barrel already
carries a placeholder comment). No other file needs to change.
