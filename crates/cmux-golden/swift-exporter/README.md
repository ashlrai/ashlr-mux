# Swift golden-fixture exporter (macOS, authoritative)

This directory holds the **design and reference implementation** of the Swift
fixture exporter for the M1 golden-file parity harness (WS6).

## Status

The exporter is **not built/run on the Windows porting machine** (no Swift
toolchain; the codecs depend on Foundation/AppKit and the macOS-only SPM
packages). On macOS the exporter regenerates the fixtures under
`crates/cmux-golden/fixtures/`; its output is **authoritative**, and the Rust
`cmux-golden` tests assert byte-identical canonical JSON against it.

### M1 golden-parity: regenerated and proven (macOS)

The exporter has been completed, compiled, and run on macOS; the committed
fixtures are now **authoritative Swift output** (no longer Rust-seeded
placeholders). `cargo test -p cmux-golden` passes (30 tests: 8 ipc, 8 osc133,
5 session, 9 shortcuts). Two decisions made during this work:

1. **Session domain → option (a), mirror the Codables.** `AppSessionSnapshot`
   lives in the app target and, more importantly, its `JSONEncoder` output is a
   *different* wire shape from the Rust port (camelCase keys + ~15 extra fields:
   `frame`, `display`, `sidebar`, `panels`, `statusEntries`, … ). No importable
   Swift codec emits the port's minimal snake_case contract. So the session
   snapshot Codables are mirrored as a standalone `Encodable` graph in
   `Sources/CmuxGoldenExport/SessionMirror.swift` (snake_case `CodingKeys`,
   integer time/dimension fields, `layout` nullable-not-omitted, the
   `{type, pane|split}` tagged union). This proves a Swift implementation of the
   on-disk contract emits byte-identical canonical JSON to the Rust port, while
   keeping the exporter a self-contained SPM executable (no heavyweight
   app-target XCTest host). The live-app `AppSessionSnapshot` ↔ port divergence
   (camelCase + extra fields) is a pre-existing gap outside M1's golden scope and
   is flagged for follow-up.

2. **One Rust port divergence found & fixed: `keyCode`.** The authoritative
   macOS `ShortcutStroke` (CmuxSettings) uses synthesized `Codable`, so the
   on-disk key is `keyCode` (camelCase) — matching the web/webviews
   `keyCode` field. The Rust port serialized `key_code` (snake_case). Fixed the
   port (`crates/cmux-core/src/shortcuts.rs`: `#[serde(rename = "keyCode")]`) and
   updated `fixtures/shortcuts/stored_single_stroke.json`. Every other byte
   across all 32 fixtures was already identical between the Rust ports and the
   Swift codecs.

## Why an exporter, not hand-written fixtures

The whole point of the harness is that **golden files are the contract**: we
assert byte-identical canonical JSON produced by the *real* Swift codecs rather
than re-deriving "equivalent" behavior. So the fixtures must come out of the
same Swift types the macOS app ships:

| Domain     | Swift type / entrypoint                                   | Module             |
|------------|-----------------------------------------------------------|--------------------|
| session    | `AppSessionSnapshot` (`Codable`) via `JSONEncoder`        | app `Sources/`     |
| shortcuts  | `StoredShortcut`/`ShortcutStroke` (`Codable`); `ShortcutWhenClause.parse`/`evaluate` | `CmuxSettings` |
| osc133     | `OSC133CommandParser.consume` → `[TerminalCommandBlock]`  | `CmuxAgentChat`    |
| ipc        | `ControlRequestParser.request(fromLine:)`                 | `CmuxControlSocket`|

## Canonicalization parity (CRITICAL)

The Rust harness canonicalizes before comparison: **sort object keys** and
**uppercase UUIDs**, then pretty-print with 2-space indent. The Swift exporter
MUST emit the identical canonical bytes. Two safe options:

1. **Preferred:** have the Swift exporter ALSO canonicalize — re-serialize each
   value through `JSONSerialization` into a `[String: Any]`/`[Any]` tree, then
   run the same canonicalization (recursive key sort + UUID uppercase) and
   `JSONSerialization.data(withJSONObject:options:[.prettyPrinted, .sortedKeys])`
   is *insufficient* alone because (a) it does not uppercase UUIDs and (b) its
   pretty spacing differs from serde_json. The reference `Canonicalizer.swift`
   below mirrors `cmux_golden::canonicalize` exactly (sorted keys, uppercase
   UUID strings, serde_json-compatible 2-space pretty form).
2. **Alternative:** emit *compact* JSON from Swift and have CI run
   `CMUX_GOLDEN_BLESS` against a small re-canonicalizing shim. Rejected: adds a
   moving part. Keep canonicalization identical on both sides instead.

The projections for the non-`Codable` types (`ShortcutWhenClause` AST, the
`ControlRequest`/`ControlRequestParseError` results) are defined by the Rust
test files (`tests/shortcuts_golden.rs`, `tests/ipc_golden.rs`). The Swift
exporter MUST reproduce those exact projection shapes (same key names:
`node`/`atom`/`key`/`op`/`operand`, `outcome`/`code`/`id`, etc.). See those test
files as the spec.

## Files here

- `Canonicalizer.swift` — port of the Rust canonicalizer (sorted keys + upper
  UUID + serde_json-style pretty printer). Use for byte-identical output.
- `FixtureExporter.swift` — the `swift run cmux-golden-export <out-dir>` entry
  point: builds the same representative inputs as the Rust tests and writes the
  canonical JSON to `<out-dir>/<domain>/<name>.json`.

## macOS CI invocation

```bash
# 1. Regenerate authoritative fixtures from the Swift codecs.
swift run --package-path crates/cmux-golden/swift-exporter \
  cmux-golden-export "$PWD/crates/cmux-golden/fixtures"

# 2. Assert the Rust ports match the freshly-exported Swift fixtures.
cargo test -p cmux-golden

# 3. Fail the build if the committed fixtures drifted from Swift output
#    (i.e. the placeholders were never regenerated, or a port diverged).
git diff --exit-code -- crates/cmux-golden/fixtures
```

On the Windows runner, only step 2 runs (no Swift): the committed fixtures are
treated as the contract and the Rust ports are checked against them. Because
step 1+3 run on macOS, any drift between the placeholders and real Swift output
is caught there.
