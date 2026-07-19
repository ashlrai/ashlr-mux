# Windows parity program

This directory tracks canonical cmux behavior and the Windows/Tauri port. It
contains two deliberately different views:

- `baseline.json` and `parity-matrix.json` are a frozen acceptance snapshot.
- `current-audit.json` is the latest rolling comparison of current canonical
  and Windows commits.

The frozen matrix is reproducible evidence, but it is not a current completion
percentage. Its rows mix public commands, internal commands, release methods,
debug methods, and broad product umbrellas. Rows are not deduplicated user
features and are not effort-weighted.

At this checkpoint the frozen validator reports 496 matrix entries and 222
pinned source blobs. Those are historical snapshot-integrity counts, not
"features completed." Current scope and status always come from
`current-audit.json`.

## Current status

Read [STATUS.md](STATUS.md) for the engineering checkpoint and
`current-audit.json` for machine-readable counts and commit provenance. Refresh
the rolling audit only at a clean checkpoint:

```powershell
git fetch --no-write-fetch-head origin <canonical-sha>
python scripts/parity/audit_current_scope.py `
  --canonical-commit <canonical-sha> `
  --windows-commit HEAD
```

The audit generates full catalogs in a temporary directory and writes only the
compact result. This keeps routine upstream refreshes from producing enormous
generated-JSON diffs.

## Status rules

- `missing`: no functional Windows implementation is evidenced.
- `red`: an executable acceptance test demonstrates the gap.
- `implemented_unverified`: code exists, but canonical differential/runtime
  evidence is incomplete.
- `verified`: canonical contract, state effects, errors, output, events, and
  applicable persistence have passing evidence at `latest_verifying_commit`.
- `platform_equivalent`: Windows supplies a tested equivalent with a written
  rationale and verifying commit.
- `not_applicable`: the requirement is genuinely irrelevant on Windows and has
  an explicit rationale.

Catalog matching may promote `missing` only to `implemented_unverified`.
Behavioral or live differential evidence is required for `verified`.

## Tracked artifacts

- `source/canonical_cli.json`: generated canonical CLI extraction checkpoint.
- `source/canonical_v2.json`: generated canonical socket-method extraction checkpoint.
- `source/windows_evidence.json`: generated Windows routing extraction checkpoint.
- `product_domains.json`: coarse cross-cutting product requirements.
- `overrides.json`: reviewed evidence and status decisions.
- `current-overrides.json`: reviewed decisions newer than the frozen snapshot;
  the rolling audit layers these over `overrides.json` without rewriting the
  historical matrix.
- `parity-matrix.json`: generated join for the frozen acceptance snapshot.
- `current-audit.json`: compact rolling scope and freshness audit.

Contract files preserve the discovery checkpoint named by their
`windows_audit_commit`. Their `matrix_status_at_audit` and `windows_gap` fields
are historical inputs, not current claims. Current implementation and live
verification status belongs in `STATUS.md`, `current-audit.json`, and
`evidence/*.json`; do not rewrite canonical contracts after implementation.

Do not hand-edit generated catalogs or matrices. The source catalogs have moved
forward since the matrix freeze, so rebuilding from today's catalogs is expected
to differ and would create thousands of misleading generated-line changes.
Validate the matrix against the Git objects pinned inside each frozen row with:

```powershell
python scripts/parity/validate_matrix_sources.py
```

Use `build_matrix.py` only when intentionally creating a separately reviewed
acceptance snapshot from a coherent set of source catalogs.

## Differential protocol

`scripts/parity/differential_harness.py` sends each case to canonical and
Windows runner adapters. Each adapter reports exit status, stdout, stderr,
error, response, mutated state, events, persistence, selector behavior, and
multiwindow behavior. Ignore a platform difference only when the case records
the exact JSON pointer and rationale.

Desktop compilation and unit tests establish testability, not behavioral
parity. Promote runtime-dependent behavior only with relevant live Windows or
canonical differential evidence.
