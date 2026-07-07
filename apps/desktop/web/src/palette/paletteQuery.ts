// The command palette's query-driven state, composing the pure sub-policies:
// scope + matching-query derivation (`listScope`), the keyboard cursor
// (`paletteSelection`), and the scope-flip reset decision. This is the pure
// heart of the palette's "query input" behavior; the React overlay (D4) drives
// it and holds no interaction logic itself.
//
// Parity note (the load-bearing distinction): a QUERY TEXT change re-anchors the
// cursor to the top of the fresh list (`queryChanged`, index 0), while a
// same-query RESULT COUNT change only CLAMPS the cursor (`resultsChanged`).
// Conflating the two would reset the cursor when the macOS host preserves it
// (Swift `ContentView` command-palette handling).

import {
  listScope,
  queryForMatching,
  shouldResetVisibleResults,
  type CommandPaletteListScope,
} from "./listScope";
import {
  initialSelection,
  paletteSelectionReducer,
  type PaletteSelectionState,
} from "./paletteSelection";

/** The palette's query state: the raw query, its derived scope + matcher input,
 *  and the current cursor over the result list. `scope` and `matchingQuery` are
 *  a deliberate denormalized cache of `query` (recomputed on every change) so
 *  the React consumer needn't re-derive them each render. */
export interface PaletteQueryState {
  /** The raw query text (may carry the leading `>` command-scope prefix). */
  readonly query: string;
  /** Which list the query selects: `commands` (`>`-prefixed) or `switcher`. */
  readonly scope: CommandPaletteListScope;
  /** The query the (Rust) matcher should search — scope framing stripped. */
  readonly matchingQuery: string;
  /** The keyboard cursor over the current result list. */
  readonly selection: PaletteSelectionState;
}

/** Builds the initial query state for `query` over a list of `count` rows. */
export function initialQueryState(query = "", count = 0): PaletteQueryState {
  return {
    query,
    scope: listScope(query),
    matchingQuery: queryForMatching(query),
    selection: initialSelection(count),
  };
}

/** The result of a query-text change: the next state plus whether the currently
 *  visible results should be reset for the scope transition. */
export interface QueryChangeResult {
  readonly state: PaletteQueryState;
  /** Reset the shown results when the scope flipped while results were visible. */
  readonly resetVisibleResults: boolean;
}

/**
 * The user edited the query text. Re-derives scope + matching query, re-anchors
 * the cursor to the top of the (new) list, and reports whether the visible
 * results should be reset (scope flip while results were shown).
 *
 * `nextResultCount` is the size of the fresh list when known synchronously (the
 * sync-seed path); when results arrive asynchronously, pass `0` here and call
 * {@link applyResults} once the matcher replies.
 */
export function applyQueryChange(
  prev: PaletteQueryState,
  nextQuery: string,
  nextResultCount = 0,
): QueryChangeResult {
  const resetVisibleResults = shouldResetVisibleResults(
    prev.query,
    nextQuery,
    prev.selection.count > 0,
  );
  return {
    state: {
      query: nextQuery,
      scope: listScope(nextQuery),
      matchingQuery: queryForMatching(nextQuery),
      selection: paletteSelectionReducer(prev.selection, {
        type: "queryChanged",
        count: nextResultCount,
      }),
    },
    resetVisibleResults,
  };
}

/** New match results arrived for the SAME query — clamp the cursor into range
 *  (never re-anchor to the top). */
export function applyResults(prev: PaletteQueryState, count: number): PaletteQueryState {
  return {
    ...prev,
    selection: paletteSelectionReducer(prev.selection, { type: "resultsChanged", count }),
  };
}

/** Keyboard cursor move (no wrap; clamps at the list ends). */
export function applyMove(
  prev: PaletteQueryState,
  direction: "up" | "down",
): PaletteQueryState {
  return {
    ...prev,
    selection: paletteSelectionReducer(prev.selection, {
      type: direction === "up" ? "moveUp" : "moveDown",
    }),
  };
}
