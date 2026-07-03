// Pure keyboard-cursor reducer for the command palette result list.
//
// The macOS host keeps the palette's highlighted row in view state; on
// Windows the same policy lives here as a pure reducer so the React shell
// stays free of interaction logic (SSR tests fire no handlers). All edges
// — clamp on move (matching the host, which does NOT wrap), clamp when the
// result count shrinks, reset on a query change — are covered by unit tests.
//
// The move policy mirrors Swift `moveCommandPaletteSelection(by:)`
// (`Sources/ContentView.swift`): `min(max(current + delta, 0), count - 1)`.
// Moving down from the last row or up from the top is a no-op (the host
// additionally beeps on an empty list); the cursor never wraps around.

/** The cursor over the current result list. */
export interface PaletteSelectionState {
  /** Index of the highlighted row; `0` when the list is empty. */
  readonly index: number;
  /** Number of rows currently in the list. */
  readonly count: number;
}

/** Events that move or re-anchor the cursor. */
export type PaletteSelectionAction =
  | { readonly type: "moveUp" }
  | { readonly type: "moveDown" }
  /** The result list changed length (same query); clamp the cursor. */
  | { readonly type: "resultsChanged"; readonly count: number }
  /** The query text changed; re-anchor to the top of the fresh list. */
  | { readonly type: "queryChanged"; readonly count: number };

/** The initial cursor for a list of `count` rows (default empty). */
export function initialSelection(count = 0): PaletteSelectionState {
  return { index: 0, count };
}

/** Clamps an index into `[0, count - 1]`; an empty list pins to `0`. */
function clampIndex(index: number, count: number): number {
  if (count <= 0) {
    return 0;
  }
  if (index < 0) {
    return 0;
  }
  if (index > count - 1) {
    return count - 1;
  }
  return index;
}

/** Applies a cursor event, returning the next selection state. */
export function paletteSelectionReducer(
  state: PaletteSelectionState,
  action: PaletteSelectionAction,
): PaletteSelectionState {
  switch (action.type) {
    case "moveUp":
      return { index: clampIndex(state.index - 1, state.count), count: state.count };
    case "moveDown":
      return { index: clampIndex(state.index + 1, state.count), count: state.count };
    case "resultsChanged":
      return { index: clampIndex(state.index, action.count), count: action.count };
    case "queryChanged":
      return { index: 0, count: action.count };
  }
}
