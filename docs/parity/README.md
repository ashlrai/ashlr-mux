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

- `source/canonical_cli.json`: generated from the frozen canonical commit.
- `source/canonical_v2.json`: generated frozen socket-method inventory.
- `source/windows_evidence.json`: generated frozen Windows routing evidence.
- `product_domains.json`: coarse cross-cutting product requirements.
- `overrides.json`: reviewed evidence and status decisions.
- `parity-matrix.json`: generated join for the frozen acceptance snapshot.
- `current-audit.json`: compact rolling scope and freshness audit.

Do not hand-edit generated catalogs or matrices. Validate the frozen snapshot
with:

```powershell
python scripts/parity/build_matrix.py --check
python scripts/parity/validate_matrix_sources.py
```

## Differential protocol

`scripts/parity/differential_harness.py` sends each case to canonical and
Windows runner adapters. Each adapter reports exit status, stdout, stderr,
error, response, mutated state, events, persistence, selector behavior, and
multiwindow behavior. Ignore a platform difference only when the case records
the exact JSON pointer and rationale.

Desktop compilation and unit tests establish testability, not behavioral
parity. Promote runtime-dependent behavior only with relevant live Windows or
canonical differential evidence.
