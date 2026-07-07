// Port of the command-palette results-gating predicates:
//   (a) sync-seed decision on open / query change
//   (b) preserve-empty-while-pending (don't flash an empty list)
//
// Canonical source (parity oracle):
//   Packages/macOS/CmuxCommandPalette/Sources/CmuxCommandPalette/Orchestration/
//     CommandPaletteSearchOrchestrator.swift:11,312-338
//   Consumers/wiring: Sources/ContentView.swift:4939-4982 (sync-seed gate) and
//     :8403-8419 (`commandPaletteShouldShowEmptyState`).
// Rust parity mirror (identical semantics):
//   crates/cmux-command-palette/src/orchestrator.rs:49,296-321 with the truth
//   tables at :436-489.
//
// All predicates are pure and headless: no clock, no Tauri/DOM/globals. The D2
// Tauri bridge only exposes `preview_search_matches` scoring, not these gates,
// so they are re-implemented here in pure TS (the ported Rust fns are the oracle
// for parity, not a runtime dependency).
//
// D4 CAVEAT (BACKLOG.md:99): `hasVisibleResultsForScope` /
// `visibleResultsScopeMatches` / `resolvedResultsAreEmpty` must be fed from the
// D4 host's authoritative "results currently shown" tracker — NEVER inferred
// from `paletteSelection.count`, which drops to 0 between an async query change
// and `applyResults`. They mirror the canonical `commandPaletteVisibleResults`/
// `commandPaletteVisibleResultsScope` state, distinct from selection. Hence
// every input is explicit and keyed by the exact Swift arg name to prevent
// positional-boolean transposition.
//
// The scope-flip visible-results reset (ContentView.swift:3496-3506) is already
// ported in `listScope.ts` (`shouldResetVisibleResults`) and consumed by
// `paletteQuery.ts`; it is deliberately NOT re-ported here — compose with it.

/**
 * Corpus-size ceiling under which results may be seeded synchronously even
 * without a ready search index. Swift `synchronousSeedCorpusLimit`
 * (CommandPaletteSearchOrchestrator.swift:11); Rust
 * `SYNCHRONOUS_SEED_CORPUS_LIMIT` (orchestrator.rs:49). Inclusive: 256 seeds,
 * 257 does not.
 */
export const SYNCHRONOUS_SEED_CORPUS_LIMIT = 256;

/**
 * Whether opening the palette (or a query change) should seed results
 * synchronously instead of waiting for the async search task.
 *
 * Swift `shouldSynchronouslySeedResults` (…Orchestrator.swift:312-318):
 *   `!hasVisibleResultsForScope && (hasSearchIndex || corpusCount <= 256)`.
 */
export function shouldSynchronouslySeedResults(input: {
  hasVisibleResultsForScope: boolean;
  hasSearchIndex: boolean;
  corpusCount: number;
}): boolean {
  return (
    !input.hasVisibleResultsForScope &&
    (input.hasSearchIndex ||
      input.corpusCount <= SYNCHRONOUS_SEED_CORPUS_LIMIT)
  );
}

/**
 * Whether the visible empty state should be preserved while a search is pending,
 * to avoid flashing stale results. True only when ALL five inputs hold.
 *
 * Swift `shouldPreserveEmptyStateWhileSearchPending`
 * (…Orchestrator.swift:322-338); Rust orchestrator.rs:309-321.
 */
export function shouldPreserveEmptyStateWhileSearchPending(input: {
  isSearchPending: boolean;
  visibleResultsScopeMatches: boolean;
  resolvedSearchScopeMatches: boolean;
  resolvedSearchFingerprintMatches: boolean;
  resolvedResultsAreEmpty: boolean;
}): boolean {
  return (
    input.isSearchPending &&
    input.visibleResultsScopeMatches &&
    input.resolvedSearchScopeMatches &&
    input.resolvedSearchFingerprintMatches &&
    input.resolvedResultsAreEmpty
  );
}

/**
 * The three-branch empty-state decision from
 * `commandPaletteShouldShowEmptyState` (ContentView.swift:8407-8419), so the D4
 * host can call one fn:
 *   1. If visible results are non-empty -> not empty-state (8408).
 *   2. Else if the current resolved results are available
 *      (`hasCurrentResolvedResults`) -> show empty-state (8409-8411).
 *   3. Else defer to {@link shouldPreserveEmptyStateWhileSearchPending} (8413-8419)
 *      — while a fresh search is pending, hold the empty view rather than flash.
 *
 * `hasCurrentResolvedResults` mirrors ContentView.swift:8403-8405:
 * `!isSearchPending && resolvedSearchRequestID == searchRequestID` — the D4 host
 * computes it and passes it in.
 *
 * NOTE: the primitive predicates above stay exported for direct oracle parity
 * with the Rust truth-table tests; prefer them when you only need one axis.
 */
export function shouldShowEmptyState(input: {
  hasVisibleResults: boolean;
  hasCurrentResolvedResults: boolean;
  isSearchPending: boolean;
  visibleResultsScopeMatches: boolean;
  resolvedSearchScopeMatches: boolean;
  resolvedSearchFingerprintMatches: boolean;
  resolvedResultsAreEmpty: boolean;
}): boolean {
  if (input.hasVisibleResults) {
    return false;
  }
  if (input.hasCurrentResolvedResults) {
    return true;
  }
  return shouldPreserveEmptyStateWhileSearchPending({
    isSearchPending: input.isSearchPending,
    visibleResultsScopeMatches: input.visibleResultsScopeMatches,
    resolvedSearchScopeMatches: input.resolvedSearchScopeMatches,
    resolvedSearchFingerprintMatches: input.resolvedSearchFingerprintMatches,
    resolvedResultsAreEmpty: input.resolvedResultsAreEmpty,
  });
}
