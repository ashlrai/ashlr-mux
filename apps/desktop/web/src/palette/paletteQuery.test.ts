import { describe, expect, test } from "bun:test";

import {
  applyMove,
  applyQueryChange,
  applyResults,
  initialQueryState,
} from "./paletteQuery";

describe("paletteQuery", () => {
  test("initial state derives scope + matching query (switcher)", () => {
    const s = initialQueryState("hello", 3);
    expect(s.query).toBe("hello");
    expect(s.scope).toBe("switcher");
    expect(s.matchingQuery).toBe("hello");
    expect(s.selection).toEqual({ index: 0, count: 3 });
  });

  test("initial state strips the > prefix + surrounding whitespace in commands scope", () => {
    const s = initialQueryState(">  rename  ");
    expect(s.scope).toBe("commands");
    expect(s.matchingQuery).toBe("rename");
  });

  test("a query change re-anchors the cursor to the top and re-derives scope", () => {
    const start = applyResults(initialQueryState("a", 5), 5);
    const moved = applyMove(applyMove(start, "down"), "down"); // cursor at 2
    expect(moved.selection.index).toBe(2);

    const { state } = applyQueryChange(moved, ">find", 4);
    // queryChanged always re-anchors to 0 (not a clamp).
    expect(state.selection).toEqual({ index: 0, count: 4 });
    expect(state.scope).toBe("commands");
    expect(state.matchingQuery).toBe("find");
  });

  test("resetVisibleResults fires only on a scope flip while results are shown", () => {
    const shown = applyResults(initialQueryState("a", 2), 2); // switcher, results visible
    // switcher -> commands with results shown: reset.
    expect(applyQueryChange(shown, ">a", 0).resetVisibleResults).toBe(true);
    // switcher -> switcher (same scope): no reset.
    expect(applyQueryChange(shown, "ab", 0).resetVisibleResults).toBe(false);

    // Same scope flip but with NO results shown: no reset.
    const empty = initialQueryState("a", 0);
    expect(applyQueryChange(empty, ">a", 0).resetVisibleResults).toBe(false);
  });

  test("applyResults clamps the cursor without re-anchoring", () => {
    const start = applyResults(initialQueryState("a", 5), 5);
    const atEnd = applyMove(applyMove(applyMove(applyMove(applyMove(start, "down"), "down"), "down"), "down"), "down");
    expect(atEnd.selection.index).toBe(4); // clamped at last row, no wrap

    // List shrinks to 2 rows: cursor clamps to 1, does NOT reset to 0.
    const shrunk = applyResults(atEnd, 2);
    expect(shrunk.selection).toEqual({ index: 1, count: 2 });
  });

  test("applyMove does not wrap at either end", () => {
    const s = applyResults(initialQueryState("a", 3), 3);
    expect(applyMove(s, "up").selection.index).toBe(0); // already at top
    const bottom = applyMove(applyMove(applyMove(s, "down"), "down"), "down");
    expect(bottom.selection.index).toBe(2); // pinned at last, no wrap past end
  });
});
