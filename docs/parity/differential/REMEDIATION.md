# Pane/Surface Lifecycle — Differential Remediation Contract

Input contract for the remediation slice, derived from the first live
differential: frozen canonical `e1825d40d` vs Windows `be9cb3819`
(`captures/pane_surface_lifecycle.normalized-diff.e1825d40d-vs-be9cb3819.json`,
43 cases: 23 identical, 20 deltas). Every validation/error-path case and the
frozen `tab-action`/`respawn-pane` `--help` texts are already byte-identical.

## Root decisions (2026-07-13)

1. **Sanctioned normalization — timing nondeterminism ONLY.** `occurred_at`
   timestamps, `boot_id` (already covered by UUID symbolization), and
   seq-derived event ids (`<boot uuid>-<seq>`) are symbolized by stable
   first-seen order (`<ts-N>` / `<event-id-N>`): `TimingSymbolizer` in
   `scripts/parity/capture_driver.py`, applied at capture time for new
   captures and idempotently at load time by
   `scripts/parity/compare_captures.py` (so the frozen canonical NDJSON is
   never rewritten). Frame COUNTS, event NAMES, frame ORDER, payload keys,
   and the ack's `after_seq`/replay-default/`resume` counters stay STRICT —
   those are real divergences (items 8a/8b below).
2. **Divergences 1–3 (identity bootstrap cluster) are deliberately
   deferred**: the Batch 1B backend builder is implementing real window
   records; the fix lands after 1B integrates. Divergence 9 may self-heal via
   the 1B list-row plain-key contract; re-diff after 1B.

## Root divergences

| # | Divergence | Cases (delta lanes) | Owner | Status |
|---|-----------|---------------------|-------|--------|
| 1 | **`window_id: "main"` literal vs canonical UUID** — contaminates the response lane of ~17 delta cases (every success response). | all response-lane deltas | backend | deferred to post-1B (window records) |
| 2 | **Bootstrap surface ids `surface-N` literals vs UUID** — surface.list/current rows for the fixture's initial surfaces (`surface-2`, `surface-3`, `surface-5`). | pane_create.direction_right_happy, surface_create.terminal_happy, surface_list.rows_shape, surface_current.workspace_selector_routing, surface_respawn.focused_fallback_explicit_command | backend | deferred to post-1B |
| 3 | **Ref-mint off-by-one** (`workspace:1/pane:1/surface:3` vs canonical `workspace:2/pane:2/surface:4`) — Windows does not register the bootstrap default workspace in the handle registry before the first mint (canonical does). Bootstrap-sensitive. | nearly all success-response cases | backend | deferred to post-1B |
| 4 | **`pane.resize` semantics/echo** — Windows omits the canonical `direction`+`amount` echo keys and uses pixel-fraction steps (0.5→0.4444; 0.9→0.8990) vs canonical step semantics (0.5→0.1 for amount 2; 0.9→0.1). Clamp itself matches (0.99→0.9). | pane_resize.relative_happy, pane_create.divider_clamped_high (state probe) | backend | open |
| 5 | **`surface.move` dual-anchor validation missing** — canonical `invalid_params: "Specify at most one of before_surface_id or after_surface_id"`; Windows succeeds and performs the move. | surface_move.both_anchors_rejected (error+response) | backend | open |
| 6 | **`requested_working_directory: null`** vs canonical inherited creator cwd — presence-vs-null semantics divergence (path *text* would be approved-differencable; null is not). | pane_create.direction_right_happy, surface_create.terminal_happy, surface_list.rows_shape, surface_respawn.focused_fallback_explicit_command (state/response rows) | backend | open |
| 7 | **`selected_in_pane` false on the bootstrap surface** where canonical has true — pane-selection semantics divergence. | pane_create.direction_right_happy, surface_list.rows_shape (rows) | backend | open |
| 8a | **`events.stream` default replay** — Windows defaults to full replay from seq 0 (`after_seq: 0`, whole-session replay: 19/49/72/100 frames) vs canonical no-replay (`after_seq: null`, 2–6 frames). | pane_create.direction_right_happy, surface_create.terminal_happy, surface_close.happy, surface_action.rename_trims_title (events) | backend | open |
| 8b | **Extra noncanonical event emissions** — `session.changed`, `pane.focused` on non-focus create, double `workspace.created` at bootstrap. | same events cases as 8a | backend | open |
| 9 | **`cli tab-action --surface <uuid>`** — Windows CLI exits 1 `Error: not_found: Tab not found` vs canonical `OK action=pin tab=tab:26 workspace=workspace:2`; target resolution does not accept the `--surface <uuid>` form. | cli.tab_action_pin (exit/stdout/stderr) | cli-or-backend | may self-heal via 1B list-row plain-key contract; re-diff after 1B |

**Cascade artifact (not a divergence, no owner):** UUID symbol-number offsets
in echoed error data (`surface_respawn.bogus_surface_id_not_found`, symbol
swap in `pane_create.source_ref_not_honored_quirk`) — creation-order
symbolization drifts once divergences 1–3 change how many UUIDs each side
exposes. Self-heals when 1–3 are fixed.

## Promotion bar

Zero unexplained deltas across all 43 cases (and the contract's remaining
acceptance surface) on the exact pushed commit under
`scripts/parity/compare_captures.py` with only the manifest's JSON-pointer
approved differences and the sanctioned timing normalization above. Delta
count after the sanctioned normalization: **20** (unchanged — every events
case also carries real divergences; the normalization removed only timestamp
and seq-id noise from the events detail).
