# M1 / WS6 — Golden-file parity harness

Branch: `windows-port/m1-ws6-golden` (off `windows-port/m1-base`).
Crate added: `crates/cmux-golden` (dev/test only, `publish = false`).

## What this delivers

A Rust parity harness that feeds representative inputs through the **existing
public Rust ports** and asserts **byte-identical canonical JSON** against
committed reference fixtures, for all four M1 contract domains:

| Domain    | Rust port exercised (crate)                          | Fixtures |
|-----------|------------------------------------------------------|----------|
| session   | `encode_session`/`decode_session` + recursive layout union (`cmux-core`) | 4 |
| shortcuts | `StoredShortcut` JSON round-trips, `ShortcutWhenClause::parse` to AST, `evaluate` truth table (`cmux-core`) | 12 |
| osc133    | `Osc133Parser::consume` block segmentation (`cmux-terminal`) | 8 |
| ipc       | `ControlRequestParser::request` incl. every defect class (`cmux-ipc`) | 8 |

`cargo test -p cmux-golden` runs **36 tests** (6 lib unit + 30 golden) — all
green. `cargo clippy -p cmux-golden --all-targets -- -D warnings` is clean.
`cargo test --workspace --exclude cmux-desktop` is green (only a crate was
added; pre-existing counts unchanged: cmux-core 10, cmux-ipc 12, cmux-terminal
9, cmux-agent 5, cmux-cli 0). `cmux-desktop` is excluded because the Tauri
build-script fails with Windows `os error 4551` (Application Control) in this
sandbox — environmental, unrelated to this crate.

## Harness structure

```
crates/cmux-golden/
|-- Cargo.toml                 # dev/test crate; deps: serde_json, uuid; dev-deps: the 3 ports
|-- src/lib.rs                 # the canonicalizer (sorted keys + uppercase UUID) + unit tests
|-- fixtures/                  # committed reference corpus (Rust-seeded placeholders)
|   |-- session/   (4 files)
|   |-- shortcuts/ (12 files)
|   |-- osc133/    (8 files)
|   |-- ipc/       (8 files)
|-- tests/
|   |-- support/mod.rs         # fixture read/write/assert + CMUX_GOLDEN_BLESS bless mode
|   |-- session_golden.rs
|   |-- shortcuts_golden.rs
|   |-- osc133_golden.rs
|   |-- ipc_golden.rs
|-- swift-exporter/            # authoritative macOS exporter: design + reference Swift
    |-- README.md
    |-- Package.swift
    |-- Sources/CmuxGoldenExport/{Canonicalizer,FixtureExporter}.swift
```

Each test: build representative input -> run it through the real Rust port ->
project to a `serde_json::Value` -> `assert_canonical_fixture(domain, name,
value)`. The support helper canonicalizes and compares byte-for-byte against
`fixtures/<domain>/<name>.json`.

## Canonicalization approach (`src/lib.rs`)

The spec pins two normalizations so "byte-identical" is well-defined against
Swift's `JSONEncoder`/`JSONSerialization` (which emit **unordered keys** and
**uppercase** Foundation UUIDs):

1. **Sort object keys** recursively, lexicographically by `String::cmp` (Unicode
   scalar order). The map is rebuilt explicitly so the result is deterministic
   regardless of the `serde_json/preserve_order` feature flag.
2. **Uppercase UUID-shaped strings** — exactly 36 chars, dashes at 8/13/18/23,
   validated via the `uuid` crate (rejects non-hex). Already-uppercase UUIDs and
   any non-UUID string are left untouched. This matches `Foundation.UUID.uuidString`.

Output is `serde_json::to_string_pretty` (2-space indent). Fixtures are stored
pretty-printed (one value per file, trailing newline) so diffs are reviewable;
comparison is byte-for-byte on that exact rendering.

Public API: `canonicalize(&mut Value)`, `canonical_json_bytes(&[u8]) -> Option<Vec<u8>>`, `canonical_json_string(&Value) -> String`.

### Projections for non-`Serialize` types

Two ports expose types that intentionally do not derive `Serialize`
(`ShortcutWhenClause` carries a compiled `regex::Regex`; `ControlRequest`/
`ControlRequestParseError` are plain enums). The harness defines a small,
deterministic **JSON projection** of each public AST/result in the test file
itself, and that projection *is* the canonical form the fixture pins:

- when-clause AST: `{ "node": "and|or|not|atom|key|compare|always", ... }`
  (see `tests/shortcuts_golden.rs::clause_json`).
- IPC result: `{ "outcome": "ok", "id", "method", "params" }` or
  `{ "outcome": "error", "code": "invalidUTF8|invalidJSON|notAnObject|missingMethod", "id"? }`
  (see `tests/ipc_golden.rs`). The `code` strings use the Swift defect-class
  spellings from the spec.

The Swift exporter must reproduce these exact projection shapes; the Rust test
files are the spec for them.

## Coverage detail

- **session** — `full_modern_snapshot` (every optional field, nested split
  layout, canvas panes, workspace groups, UUID workspace ids),
  `legacy_pre_canvas_pre_tab_snapshot` (all newer optionals absent — pins that
  absent fields do NOT resurface as keys), `legacy_no_layout_snapshot` (`layout`
  itself `None` -> serialized as `null`), `empty_snapshot`. Every test does
  encode->decode->re-encode and asserts the round-trip is a fixed point. Plus a
  raw-JSON legacy decode test.
- **shortcuts** — `StoredShortcut` unbound/single-stroke/chord round-trips;
  when-clause parse->AST for precedence (`||` looser than `&&`), `!`+parens,
  int/regex/in-list comparisons, and boolean-literal folding (`== true`->bare
  key, `== false`->`Not`); a `evaluate` **truth table** over an 8-row context
  grid x 5 clauses.
- **osc133** — happy path, non-zero exit, two sequenced commands, running (no
  D mark), CR progress fold, CRLF->LF, alt-screen interactive, noise stripping.
  Every transcript is replayed **whole and byte-at-a-time** and both must yield
  identical blocks before the fixture is asserted (guards split-escape / fold
  rules at chunk boundaries).
- **ipc** — ok envelopes (int id+params, string id no params, null id) and each
  defect class: `invalidJSON`, `notAnObject`, `missingMethod` (with and without
  echoed id), and the `invalidUTF8` variant. Note: the Rust strict parser takes
  `&str`, so invalid UTF-8 cannot reach it — that defect lives at the framing
  layer upstream. The harness pins the projection of the `InvalidUtf8` variant
  itself so the defect class is in the corpus and the enum is fully covered;
  this is documented in `tests/ipc_golden.rs`.

## Fixtures: Rust-seeded placeholders (IMPORTANT)

**The 32 committed fixtures were generated from the Rust side, not Swift.** They
are placeholders. The macOS Swift exporter is the eventual **source of truth**
and MUST regenerate them on CI and treat its output as authoritative. This is
marked in:

- `tests/support/mod.rs` module doc ("the fixtures currently committed are
  Rust-seeded placeholders ... the macOS CI Swift exporter MUST regenerate them"),
- each `tests/*_golden.rs` file header,
- `crates/cmux-golden/swift-exporter/README.md`.

Swapping in authoritative Swift fixtures requires **no code change**: the Swift
exporter writes the same canonical JSON to the same `fixtures/<domain>/<name>.json`
paths, and the Rust tests assert against whatever bytes are on disk.

### Re-blessing (how the placeholders were made)

```bash
CMUX_GOLDEN_BLESS=1 cargo test -p cmux-golden   # writes fixtures from Rust output
cargo test -p cmux-golden                       # asserts against them
```

`CMUX_GOLDEN_BLESS=1` switches `assert_canonical_fixture` from compare-mode to
write-mode. This is also the seam the macOS exporter plugs into.

## Swift fixture exporter — design + macOS CI step

Full design and a reference implementation live in
`crates/cmux-golden/swift-exporter/` (`README.md`, `Package.swift`,
`Sources/CmuxGoldenExport/{Canonicalizer,FixtureExporter}.swift`).

**Not built/run here:** this Windows machine has no Swift toolchain, and the
codecs depend on Foundation + the macOS-only SPM packages, so the exporter is
documented thoroughly rather than executed (per the WS6 brief).

Design:

- An SPM executable `cmux-golden-export <out-dir>` depending on the existing
  products `CmuxControlSocket`, `CmuxAgentChat`, `CmuxSettings`.
- It builds the **same representative inputs** as the Rust tests, runs them
  through the **existing Swift codecs** (`AppSessionSnapshot` Codable via
  `JSONEncoder`; `StoredShortcut` Codable; `ShortcutWhenClause.parse`/`evaluate`;
  `OSC133CommandParser.consume`->`[TerminalCommandBlock]`; `ControlRequestParser.request(fromLine:)`),
  and writes `<out-dir>/<domain>/<name>.json`.
- `Canonicalizer.swift` is a **port of the Rust canonicalizer** (recursive key
  sort + uppercase UUID + serde_json-compatible 2-space pretty printer) so the
  bytes are identical on both sides. (`JSONSerialization.prettyPrinted`/
  `.sortedKeys` alone is insufficient: it does not uppercase UUIDs and its
  spacing/`/`-escaping differs from serde_json.)
- For the non-`Codable` types the exporter reproduces the **same projection
  shapes** defined in the Rust test files (the test files are the spec).
- `FixtureExporter.swift` is a reference skeleton: the canonicalizer + IO are
  complete; the per-fixture *inputs* are marked `TODO(macOS)` to be filled to
  match the Rust tests exactly. `AppSessionSnapshot` lives in the app target;
  if it is not importable from a standalone SPM executable, the README documents
  the fallback of running the exporter as an XCTest inside the app target.

### macOS CI invocation

```bash
# 1. Regenerate authoritative fixtures from the Swift codecs.
swift run --package-path crates/cmux-golden/swift-exporter \
  cmux-golden-export "$PWD/crates/cmux-golden/fixtures"

# 2. Assert the Rust ports match the freshly-exported Swift fixtures.
cargo test -p cmux-golden

# 3. Fail the build if committed fixtures drifted from Swift output.
git diff --exit-code -- crates/cmux-golden/fixtures
```

On the Windows runner only step 2 runs (no Swift): fixtures are the contract,
Rust ports are checked against them. macOS runs 1+3 so any placeholder/port
drift is caught there.

## CI steps to add (NOT applied — `.github/workflows/ci.yml` is off-limits)

Add to the M1 crate matrix (windows-latest **and** macos-latest):

```yaml
# Both runners — assert Rust ports vs committed golden fixtures + lint.
- name: cmux-golden tests
  run: cargo test -p cmux-golden
- name: cmux-golden clippy
  run: cargo clippy -p cmux-golden --all-targets -- -D warnings

# macOS runner ONLY — regenerate authoritative fixtures from Swift, then
# assert Rust matches, then fail on drift. Place BEFORE the test step above
# on macOS so step 2 sees Swift-authored fixtures.
- name: Export golden fixtures from Swift (macOS only)
  if: runner.os == 'macOS'
  run: |
    swift run --package-path crates/cmux-golden/swift-exporter \
      cmux-golden-export "$PWD/crates/cmux-golden/fixtures"
- name: Fail on Swift-vs-committed fixture drift (macOS only)
  if: runner.os == 'macOS'
  run: git diff --exit-code -- crates/cmux-golden/fixtures
```

(The existing M1 crates' `cargo test` + `clippy -D warnings` matrix wiring from
NEXT_AGENT_NOTE item 5 should add `cmux-golden` alongside the other crates.)

## How to swap in authoritative fixtures

1. Implement the `TODO(macOS)` input builders in
   `swift-exporter/Sources/CmuxGoldenExport/FixtureExporter.swift` so they match
   the Rust test inputs one-to-one.
2. Run the exporter (CI step 1 above) to overwrite `crates/cmux-golden/fixtures/`.
3. Run `cargo test -p cmux-golden`. If a Rust port diverges from Swift, the test
   fails with a path + bless hint — fix the port (not the fixture) until green.
4. Commit the Swift-authored fixtures, replacing the Rust-seeded placeholders.
   No harness code changes.

## Blockers / notes

- No Swift toolchain on this machine -> Swift exporter is designed + reference-
  implemented but not executed; per-fixture inputs are `TODO(macOS)`.
- `cmux-desktop` (Tauri) cannot build here (`os error 4551`); excluded from the
  workspace test run. Unrelated to this crate.
- Did not touch `crates/cmux-core/src/{shortcuts,session}.rs`, other crate
  sources, or `.github/workflows/ci.yml`, per the brief. Only edited the
  workspace `Cargo.toml` (added the `crates/cmux-golden` member) and created
  `crates/cmux-golden/**`.

(File paths above are relative to the worktree root
`C:/Users/User/coding/work/ashlr-mux/cmux-wt/m1-ws6`.)
