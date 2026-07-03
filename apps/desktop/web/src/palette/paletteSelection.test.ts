import { describe, expect, test } from "bun:test";

import {
  initialSelection,
  paletteSelectionReducer,
  type PaletteSelectionState,
} from "./paletteSelection";

const at = (index: number, count: number): PaletteSelectionState => ({ index, count });

describe("initialSelection", () => {
  test("defaults to the top of an empty list", () => {
    expect(initialSelection()).toEqual({ index: 0, count: 0 });
  });

  test("keeps the given count", () => {
    expect(initialSelection(5)).toEqual({ index: 0, count: 5 });
  });
});

describe("paletteSelectionReducer move", () => {
  test("moveDown advances one row", () => {
    expect(paletteSelectionReducer(at(0, 3), { type: "moveDown" })).toEqual(at(1, 3));
  });

  test("moveUp retreats one row", () => {
    expect(paletteSelectionReducer(at(2, 3), { type: "moveUp" })).toEqual(at(1, 3));
  });

  test("moveDown from the last row stays put (host clamps, no wrap)", () => {
    expect(paletteSelectionReducer(at(2, 3), { type: "moveDown" })).toEqual(at(2, 3));
  });

  test("moveUp from the top stays put (host clamps, no wrap)", () => {
    expect(paletteSelectionReducer(at(0, 3), { type: "moveUp" })).toEqual(at(0, 3));
  });

  test("moves are no-ops over an empty list", () => {
    expect(paletteSelectionReducer(at(0, 0), { type: "moveDown" })).toEqual(at(0, 0));
    expect(paletteSelectionReducer(at(0, 0), { type: "moveUp" })).toEqual(at(0, 0));
  });

  test("a single-row list pins to its only row", () => {
    expect(paletteSelectionReducer(at(0, 1), { type: "moveDown" })).toEqual(at(0, 1));
    expect(paletteSelectionReducer(at(0, 1), { type: "moveUp" })).toEqual(at(0, 1));
  });
});

describe("paletteSelectionReducer resultsChanged", () => {
  test("clamps a now-out-of-range cursor to the last row", () => {
    expect(paletteSelectionReducer(at(4, 5), { type: "resultsChanged", count: 2 })).toEqual(
      at(1, 2),
    );
  });

  test("keeps an in-range cursor while adopting the new count", () => {
    expect(paletteSelectionReducer(at(1, 5), { type: "resultsChanged", count: 8 })).toEqual(
      at(1, 8),
    );
  });

  test("pins to the top when the list becomes empty", () => {
    expect(paletteSelectionReducer(at(3, 5), { type: "resultsChanged", count: 0 })).toEqual(
      at(0, 0),
    );
  });
});

describe("paletteSelectionReducer queryChanged", () => {
  test("re-anchors to the top and adopts the new count", () => {
    expect(paletteSelectionReducer(at(3, 5), { type: "queryChanged", count: 7 })).toEqual(
      at(0, 7),
    );
  });

  test("re-anchors to the top even when the new list is empty", () => {
    expect(paletteSelectionReducer(at(3, 5), { type: "queryChanged", count: 0 })).toEqual(
      at(0, 0),
    );
  });
});
