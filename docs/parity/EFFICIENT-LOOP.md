# Efficient parity loop

Each iteration owns one observable capability or one structural maintenance
boundary. Do not mix feature work, broad refactors, dependency updates, and
evidence rebaselines in one commit series.

## Selection

1. Refresh `current-audit.json` only when canonical scope or the audited Windows
   checkpoint changes materially.
2. Choose the smallest capability batch that can earn behavioral evidence.
3. Prefer verification debt over new implementation when working code can be
   promoted with an existing differential lane.
4. Record the expected user-visible outcome and applicable observation lanes
   before editing.

## Change discipline

- Reuse a shared action path for behavior exposed through multiple entry points.
- Add the smallest behavioral test that fails for the actual gap.
- Avoid speculative adapters, duplicated fixtures, and source-text tests.
- Treat more than 500 changed non-generated lines as a signal to split the
  iteration unless the work is a reviewed mechanical file move.
- Run `python scripts/windows_port_file_length_budget.py`; do not raise an
  oversized file's budget to make feature work fit.
- Keep generated catalogs out of ordinary feature diffs. The rolling audit is
  intentionally compact.
- Never count added lines, deleted lines, commits, or matrix rows as progress.
  Progress is a newly passing observable capability with retained evidence.

## Verification sequence

1. Run the focused failing test or differential.
2. Implement the narrowest production change.
3. Run the focused test twice.
4. Simplify names, ownership, duplication, and error paths.
5. Run the focused test twice again.
6. Run adjacent package/frontend tests and compile checks.
7. Run broad gates only once the slice is stable.
8. Confirm the diff contains no task logs, temporary evidence, generated noise,
   unrelated formatting, or stale progress claims.

## Completion record

A completed slice records the canonical commit, Windows commit, exact cases and
observation lanes, test commands, result counts, known exclusions, and the next
smallest unresolved capability. Commit and push only that reviewed slice.
