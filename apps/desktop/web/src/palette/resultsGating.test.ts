import { describe, expect, test } from "bun:test";

import {
  SYNCHRONOUS_SEED_CORPUS_LIMIT,
  shouldPreserveEmptyStateWhileSearchPending,
  shouldShowEmptyState,
  shouldSynchronouslySeedResults,
} from "./resultsGating";

// Mirrors crates/cmux-command-palette/src/orchestrator.rs:436-489.
describe("shouldSynchronouslySeedResults (D9a)", () => {
  test("index ready, no visible results -> true", () => {
    expect(
      shouldSynchronouslySeedResults({
        hasVisibleResultsForScope: false,
        hasSearchIndex: true,
        corpusCount: 5000,
      }),
    ).toBe(true);
  });

  test("corpus at the inclusive limit (256) -> true", () => {
    expect(SYNCHRONOUS_SEED_CORPUS_LIMIT).toBe(256);
    expect(
      shouldSynchronouslySeedResults({
        hasVisibleResultsForScope: false,
        hasSearchIndex: false,
        corpusCount: 256,
      }),
    ).toBe(true);
  });

  test("corpus just over the limit (257) -> false", () => {
    expect(
      shouldSynchronouslySeedResults({
        hasVisibleResultsForScope: false,
        hasSearchIndex: false,
        corpusCount: 257,
      }),
    ).toBe(false);
  });

  test("already have visible results for scope -> false", () => {
    expect(
      shouldSynchronouslySeedResults({
        hasVisibleResultsForScope: true,
        hasSearchIndex: true,
        corpusCount: 5000,
      }),
    ).toBe(false);
  });
});

// Mirrors orchestrator.rs:454-489 (the pending-empty-state truth table).
describe("shouldPreserveEmptyStateWhileSearchPending (D9b)", () => {
  const allTrue = {
    isSearchPending: true,
    visibleResultsScopeMatches: true,
    resolvedSearchScopeMatches: true,
    resolvedSearchFingerprintMatches: true,
    resolvedResultsAreEmpty: true,
  };

  test("all five hold -> true", () => {
    expect(shouldPreserveEmptyStateWhileSearchPending(allTrue)).toBe(true);
  });

  test("each single condition false -> false", () => {
    const keys = Object.keys(allTrue) as (keyof typeof allTrue)[];
    for (const key of keys) {
      expect(
        shouldPreserveEmptyStateWhileSearchPending({ ...allTrue, [key]: false }),
      ).toBe(false);
    }
  });
});

// Mirrors ContentView.swift:8407-8419 three-branch logic.
describe("shouldShowEmptyState (D9 combined)", () => {
  const preserveTrue = {
    isSearchPending: true,
    visibleResultsScopeMatches: true,
    resolvedSearchScopeMatches: true,
    resolvedSearchFingerprintMatches: true,
    resolvedResultsAreEmpty: true,
  };

  test("non-empty visible results -> false (branch 1)", () => {
    expect(
      shouldShowEmptyState({
        hasVisibleResults: true,
        hasCurrentResolvedResults: true,
        ...preserveTrue,
      }),
    ).toBe(false);
  });

  test("empty + hasCurrentResolvedResults -> true (branch 2)", () => {
    expect(
      shouldShowEmptyState({
        hasVisibleResults: false,
        hasCurrentResolvedResults: true,
        // preserve inputs are ignored on this branch.
        isSearchPending: false,
        visibleResultsScopeMatches: false,
        resolvedSearchScopeMatches: false,
        resolvedSearchFingerprintMatches: false,
        resolvedResultsAreEmpty: false,
      }),
    ).toBe(true);
  });

  test("empty + not-resolved -> delegates to preserve predicate (true sub-case)", () => {
    expect(
      shouldShowEmptyState({
        hasVisibleResults: false,
        hasCurrentResolvedResults: false,
        ...preserveTrue,
      }),
    ).toBe(true);
  });

  test("empty + not-resolved -> delegates to preserve predicate (false sub-case)", () => {
    expect(
      shouldShowEmptyState({
        hasVisibleResults: false,
        hasCurrentResolvedResults: false,
        ...preserveTrue,
        isSearchPending: false, // breaks the AND -> preserve is false
      }),
    ).toBe(false);
  });
});
