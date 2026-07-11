# Windows parity program

This directory is the source of truth for parity between canonical cmux and
the Windows/Tauri port. The canonical target is immutable and recorded in
`baseline.json`. Updating `origin/main` does not move that target.

## Status rules

- `missing`: no functional Windows implementation is evidenced.
- `red`: an executable acceptance test demonstrates the gap.
- `implemented_unverified`: code exists, but canonical differential/runtime
  evidence is incomplete.
- `verified`: canonical contract, state effects, errors, output, events and
  applicable persistence have passing evidence at `latest_verifying_commit`.
- `platform_equivalent`: Windows supplies a tested equivalent with a written
  rationale and verifying commit.
- `not_applicable`: the requirement is genuinely irrelevant or impossible on
  Windows and has explicit user approval.

Catalog matching may promote `missing` only to `implemented_unverified`.
`verified`, `platform_equivalent`, and `not_applicable` are manual evidence
decisions enforced by the matrix validator.

## Files

- `source/canonical_cli.json`: generated from the frozen canonical commit.
- `source/canonical_v2.json`: generated release-v2 method inventory.
- `source/windows_evidence.json`: generated Windows routes and evidence hints.
- `product_domains.json`: cross-cutting UI/runtime/product requirements.
- `overrides.json`: reviewed status, acceptance, dependency and commit evidence.
- `parity-matrix.json`: generated joined matrix and exact summary counts.
- `matrix.schema.json`: machine-readable matrix contract.

Generated source catalogs and the matrix must be regenerated rather than
hand-edited. The generator validates pinned counts, unique IDs, source
locations, status evidence and percentage numerators/denominators.

## Differential protocol

`scripts/parity/differential_harness.py` sends each case to canonical and
Windows runner adapters. Each adapter must report all observation lanes:
exit status, stdout, stderr, error, response, mutated state, events,
persistence, selector behavior and multiwindow behavior. Platform differences
are ignored only when a case names the exact JSON pointer and rationale.

## Verification boundary

The known local Windows desktop loader failure occurs before the Rust test
harness. Desktop compile/link/build is useful but cannot promote an entry to
`verified`. A relevant live Windows CI/runtime or differential result is
required whenever the behavior depends on the desktop process.
