# Swift golden-fixture exporter (macOS, authoritative)

This directory holds the **design and reference implementation** of the Swift
fixture exporter for the M1 golden-file parity harness (WS6).

## Status

The exporter is **not built/run on the Windows porting machine** (no Swift
toolchain; the codecs depend on Foundation/AppKit and the macOS-only SPM
packages). The fixtures currently committed under
`crates/cmux-golden/fixtures/` are **Rust-seeded placeholders**. On macOS CI the
exporter below MUST regenerate them; its output is **authoritative**, and the
Rust `cmux-golden` tests assert byte-identical canonical JSON against it.

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
