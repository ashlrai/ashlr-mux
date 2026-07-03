// Mirrors the `#[cfg(test)] mod tests` cases in
// `crates/cmux-workspaces/src/selection_sync.rs` 1:1. Rust uses
// `Uuid::new_v4()`; the port substitutes fixed UUID strings for determinism.

import { describe, expect, test } from "bun:test";

import {
  anchorIndex,
  anchorIndexAfterWorkspaceClick,
  anchorIndexAfterWorkspaceReorder,
  anchorWorkspaceId,
  reconciledSelection,
  shiftClickAnchorIndex,
} from "./selection";

const U = {
  a: "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
  b: "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
  c: "cccccccc-cccc-cccc-cccc-cccccccccccc",
  fallback: "ffffffff-ffff-ffff-ffff-ffffffffffff",
  stale: "0e0e0e0e-0e0e-0e0e-0e0e-0e0e0e0e0e0e",
  missingFallback: "12121212-1212-1212-1212-121212121212",
  focused: "0f0c0f0c-0f0c-0f0c-0f0c-0f0c0f0c0f0c",
} as const;

function set(ids: readonly string[]): Set<string> {
  return new Set(ids);
}

describe("selection_sync", () => {
  test("reconciled_selection_drops_stale_ids", () => {
    // Restore edge case: ids missing from the live list are dropped.
    const { a, b, stale } = U;
    const live = [a, b];
    const out = reconciledSelection(set([a, stale]), live, undefined);
    expect(out).toEqual(set([a]));
  });

  test("reconciled_selection_falls_back_when_nothing_survives", () => {
    const { a, fallback, stale, missingFallback } = U;
    const live = [a, fallback];
    // No previous id survives → fall back to the live fallback.
    expect(reconciledSelection(set([stale]), live, fallback)).toEqual(
      set([fallback]),
    );
    // Fallback also missing → empty.
    expect(
      reconciledSelection(set([stale]), live, missingFallback).size,
    ).toBe(0);
    // No fallback → empty.
    expect(reconciledSelection(set([stale]), live, undefined).size).toBe(0);
  });

  test("anchor_index_prefers_preferred_then_first_selected", () => {
    const { a, b, c } = U;
    const live = [a, b, c];
    // Preferred is selected → its index.
    expect(anchorIndex(c, set([b, c]), live)).toBe(2);
    // Preferred not selected → first selected in live order (b at 1).
    expect(anchorIndex(a, set([b, c]), live)).toBe(1);
    // No preferred → first selected.
    expect(anchorIndex(undefined, set([c]), live)).toBe(2);
    // None selected → undefined.
    expect(anchorIndex(undefined, set([]), live)).toBeUndefined();
  });

  test("anchor_workspace_id_respects_bounds", () => {
    const { a, b } = U;
    const live = [a, b];
    expect(anchorWorkspaceId(1, live)).toBe(b);
    expect(anchorWorkspaceId(2, live)).toBeUndefined();
    expect(anchorWorkspaceId(-1, live)).toBeUndefined();
    expect(anchorWorkspaceId(undefined, live)).toBeUndefined();
  });

  test("shift_click_anchor_index_derivation_order", () => {
    const { a, b, focused } = U;
    const live = [a, b, focused];
    // Existing valid index wins.
    expect(shiftClickAnchorIndex(1, set([]), undefined, live)).toBe(1);
    // Stale existing index → derive from single selection.
    expect(shiftClickAnchorIndex(99, set([b]), undefined, live)).toBe(1);
    // Multi-selection ignored → fall to focus.
    expect(shiftClickAnchorIndex(undefined, set([a, b]), focused, live)).toBe(
      2,
    );
    // Nothing to derive from → undefined.
    expect(
      shiftClickAnchorIndex(undefined, set([a, b]), undefined, live),
    ).toBeUndefined();
  });

  test("anchor_index_after_click_switches_on_shift", () => {
    expect(anchorIndexAfterWorkspaceClick(false, 3, 5)).toBe(5);
    expect(anchorIndexAfterWorkspaceClick(true, 3, 5)).toBe(3);
    // Shift with no resolved anchor → clicked index.
    expect(anchorIndexAfterWorkspaceClick(true, undefined, 5)).toBe(5);
  });

  test("anchor_index_after_reorder_prefers_preferred", () => {
    const { a, b } = U;
    const live = [a, b];
    // Preferred selected → its post-reorder index.
    expect(
      anchorIndexAfterWorkspaceReorder(b, set([a, b]), undefined, live),
    ).toBe(1);
    // Preferred not selected → fall to focus/first-selected path.
    expect(anchorIndexAfterWorkspaceReorder(a, set([b]), b, live)).toBe(1);
  });
});
