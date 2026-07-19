# Efficient parity loop

Each iteration owns one observable capability or one structural maintenance
boundary. Do not mix feature work, broad refactors, dependency updates, and
evidence rebaselines in one commit series.

## Selection

1. Refresh `current-audit.json` only when canonical scope or the audited Windows
   checkpoint changes materially.
2. Start from the earliest unexplained differential or an explicitly missing
   user capability. A route count, filename, TODO, or speculative resemblance
   is not enough to authorize production work.
3. Choose the smallest capability batch that can earn behavioral evidence.
4. Prefer verification debt over new implementation when working code can be
   promoted with an existing differential lane.
5. Record the expected user-visible outcome and applicable observation lanes
   before editing.

## Change discipline

- Reuse a shared action path for behavior exposed through multiple entry points.
- Add the smallest behavioral test that fails for the actual gap.
- Avoid speculative adapters, duplicated fixtures, and source-text tests.
- Treat more than 500 changed non-generated lines as a signal to split the
  iteration unless the work is a reviewed mechanical file move.
- Do not rewrite a working subsystem to resemble canonical source structure.
  Match observable behavior through the port's existing ownership boundaries.
- Run `python scripts/windows_port_file_length_budget.py`; do not raise an
  oversized file's budget to make feature work fit.
- Keep generated catalogs out of ordinary feature diffs. The rolling audit is
  intentionally compact.
- Never count added lines, deleted lines, commits, or matrix rows as progress.
  Progress is a newly passing observable capability with retained evidence.

## Churn accounting

- Keep behavior changes and mechanical file moves in separate commits.
- Inspect `git diff --numstat` before every commit. Explain any slice with more
  than 500 additions plus deletions, and separate generated or mechanical churn
  from semantic churn.
- Before an extraction, record the parent size, named responsibility, expected
  destination, and focused behavior suite. Move one responsibility at a time.
- Report mechanical moved lines separately from net new logic. A file split may
  have a large add/delete stat, but its reviewed semantic change must be zero.
- Do not combine several oversized-file splits into one iteration. Checkpoint
  and push each verified boundary before selecting the next one.
- Stop an iteration that is accumulating unrelated rewrites or generated
  output; preserve the smallest passing slice and discard no unknown work.

## Orchestrator context

- Keep active context to the current capability, its contract, the failing
  evidence, and the files that own it. Query generated catalogs narrowly
  instead of loading them wholesale.
- Keep at most one implementation lane and one evidence/audit lane active.
  Preserve quarantined or unknown worktrees without using them as scratch
  space.
- At each pushed checkpoint, write only durable facts to `STATUS.md`: commits,
  exact test/case counts, evidence validity, remaining blocker, and next slice.
- Compact immediately after the worktree is clean and the remote HEAD is
  verified. Resume from the durable checkpoint instead of replaying old logs or
  rediscovering completed work.

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
