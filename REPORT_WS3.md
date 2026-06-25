# WS3 — `Action` enum port (Swift → Rust)

M1 (cross-platform core extraction), workstream WS3. Branch
`windows-port/m1-ws3-action-enum` off `windows-port/m1-base`.

## Deliverable

- **`crates/cmux-core/src/shortcuts_action.rs`** — new file, the generated Rust
  `Action` enum (109 variants) + helpers + tests. Marked `@generated`.
- **`crates/cmux-core/src/lib.rs`** — minimal additive change only:
  `pub mod shortcuts_action;` and `pub use shortcuts_action::Action;`. No
  restructuring of `shortcuts.rs` (left untouched for WS1).
- **`crates/cmux-core/scripts/extract_action_enum.py`** — committed extraction
  script (provenance of the generated enum). Re-run when the Swift source
  changes; `--check` mode asserts the on-disk file is up to date.

## How the cases were extracted (no hand-retyping)

Every `Action` raw value is an on-disk `cmux.json` config key, so raw values must
not be hand-typed. The Python script mechanically parses the Swift source:

1. Finds `enum Action: String` in `Sources/KeyboardShortcutSettings.swift`
   (declaration at line 64) and walks its body by brace-depth tracking.
2. Collects the leading `case` lines only. Stops at the first computed member
   (`var id`/`var label`/`func`/…); after that, `case` lines are `switch` arms
   (`case .foo:`), not variant declarations.
3. Per declaration line: strips `// …` comments, splits on commas (handling
   multi-case lines like `case splitDown, toggleSplitZoom`), and matches each
   piece as `name` or `name = "rawValue"`.
4. Raw value = explicit `"…"` if present, else the case name (Swift's implicit
   `String` raw value == case name).
5. Emits the Rust enum: variant = UpperCamelCase of the Swift case name, carrying
   `#[serde(rename = "<exactRawValue>")]`.

Run:
```
python3 crates/cmux-core/scripts/extract_action_enum.py          # (re)generate
python3 crates/cmux-core/scripts/extract_action_enum.py --check   # verify in sync
```

## Variant count and how it maps to Swift

- **109 variants.**
- The Swift enum has 109 `case` items (not 109 lines — one line declares two:
  `case splitDown, toggleSplitZoom`). Script and an independent shell count both
  report 109:
  ```
  awk 'NR>=64 && NR<=186' Sources/KeyboardShortcutSettings.swift \
    | grep -E '^\s*case ' | sed 's|//.*||' | grep -oE 'case .*' \
    | tr ',' '\n' | grep -E '[A-Za-z]' | wc -l    # -> 109
  ```
- Enforced two ways in tests:
  - `variant_count_matches_swift` asserts `Action::COUNT == 109` (the number the
    script derived and baked into the generated file).
  - `swift_source_case_count_matches_if_present` re-parses the live Swift file at
    test time (mirroring the script's parse rules) and asserts the count still
    equals `Action::COUNT`, catching drift. Skips silently if the Swift source is
    not reachable from the test cwd.

## Raw values that needed special handling

- **Explicit rename (1):** `case toggleRightSidebar = "toggleFileExplorer"` →
  `Action::ToggleRightSidebar` with `#[serde(rename = "toggleFileExplorer")]`.
  The only case where the Swift case name differs from the on-disk key. Covered
  in `spot_check_known_raw_values` (incl. `from_raw("toggleRightSidebar") == None`).
- **Multi-case line:** `case splitDown, toggleSplitZoom` → two variants, each with
  an implicit raw value equal to its name. Spot-checked.
- The other 107 cases use implicit raw values (raw value == case name).

## Helpers / API

- `Action::raw_value(&self) -> &'static str`
- `Action::from_raw(&str) -> Option<Action>`
- `Action::ALL: [Action; 109]` — all variants in Swift declaration order.
- `Action::COUNT: usize` — 109.
- serde: each variant (de)serializes to/from its bare-string raw value
  (`serde_json::to_string(&Action::Quit) == "\"quit\""`).

## Tests (all green)

`cargo test -p cmux-core` -> 17 passed (6 new in `shortcuts_action::tests`):
- `variant_count_matches_swift` — `COUNT == ALL.len() == 109`.
- `every_variant_round_trips_via_helpers` — `from_raw(raw_value(a)) == Some(a)`.
- `every_variant_round_trips_via_serde` — serde to/from matches raw value.
- `raw_values_are_unique` — 109 distinct raw values.
- `from_raw_rejects_unknown` — unknown/empty -> `None`.
- `spot_check_known_raw_values` — known values incl. explicit-rename case.
- `swift_source_case_count_matches_if_present` — live Swift re-parse cross-check.

## Lint

`cargo clippy -p cmux-core --all-targets -- -D warnings` -> clean.

## CI steps (NOT applied — ci.yml off-limits for WS3)

Add to the windows-latest + macos core-crate job (see NEXT_AGENT_NOTE.md item 5):

```yaml
      # Generated-enum freshness guard: fails if shortcuts_action.rs drifts from
      # the Swift source of truth. Pure Python; no Rust toolchain needed.
      - name: Check Action enum is up to date with Swift source
        run: python3 crates/cmux-core/scripts/extract_action_enum.py --check

      - name: cmux-core tests
        run: cargo test -p cmux-core

      - name: cmux-core clippy
        run: cargo clippy -p cmux-core --all-targets -- -D warnings
```

Notes:
- The `--check` step is the WS3-specific guard: ensures nobody hand-edits the
  generated file or lets it drift from `KeyboardShortcutSettings.swift` without
  regenerating.
- It runs on any OS with Python 3; needs only the checked-in Swift source, not the
  Swift toolchain. Put it on at least one matrix leg.

## Coordination

- Only added two lines to `lib.rs`; `shortcuts.rs` untouched (WS1 reads it).
- `crates/` is untracked in the detached-HEAD `cmux` repo; this branch commits the
  new files.
