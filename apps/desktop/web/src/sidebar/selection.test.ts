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
  selectionAfterWorkspaceClick,
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

// updateSelection() branch coverage (ContentView.swift:14252-14319).
describe("selectionAfterWorkspaceClick", () => {
  const NONE = new Set<string>();

  function click(
    overrides: Partial<Parameters<typeof selectionAfterWorkspaceClick>[0]> &
      Pick<
        Parameters<typeof selectionAfterWorkspaceClick>[0],
        "clickedIndex" | "modifiers" | "liveWorkspaceIds"
      >,
  ) {
    return selectionAfterWorkspaceClick({
      existingAnchorIndex: undefined,
      selectedWorkspaceIds: NONE,
      focusedWorkspaceId: undefined,
      hiddenWorkspaceIds: NONE,
      ...overrides,
    });
  }

  test("shift_range_replaces_selection", () => {
    const { a, b, c, stale } = U;
    const out = click({
      clickedIndex: 2,
      modifiers: { shift: true, toggle: false },
      existingAnchorIndex: 0,
      selectedWorkspaceIds: set([stale]),
      liveWorkspaceIds: [a, b, c],
    });
    // Range [anchor..clicked]; the previous selection is replaced.
    expect(out.selectedWorkspaceIds).toEqual(set([a, b, c]));
    // Shift preserves the resolved anchor.
    expect(out.anchorIndex).toBe(0);
  });

  test("shift_toggle_unions_range_into_selection", () => {
    const { a, b, c, stale } = U;
    const out = click({
      clickedIndex: 1,
      modifiers: { shift: true, toggle: true },
      existingAnchorIndex: 0,
      selectedWorkspaceIds: set([stale]),
      liveWorkspaceIds: [a, b, c],
    });
    // formUnion (ContentView.swift:14301): previous ids survive.
    expect(out.selectedWorkspaceIds).toEqual(set([a, b, stale]));
    expect(out.anchorIndex).toBe(0);
  });

  test("shift_range_works_upward_from_anchor", () => {
    const { a, b, c } = U;
    const out = click({
      clickedIndex: 0,
      modifiers: { shift: true, toggle: false },
      existingAnchorIndex: 2,
      liveWorkspaceIds: [a, b, c],
    });
    // Clicked above the anchor → same inclusive range.
    expect(out.selectedWorkspaceIds).toEqual(set([a, b, c]));
    expect(out.anchorIndex).toBe(2);
  });

  test("shift_range_skips_hidden_collapsed_members", () => {
    const { a, b, c } = U;
    // b is a collapsed group's non-anchor member; a is that group's anchor
    // (anchors stay rangeable — ContentView.swift:14284-14299).
    const out = click({
      clickedIndex: 2,
      modifiers: { shift: true, toggle: false },
      existingAnchorIndex: 0,
      liveWorkspaceIds: [a, b, c],
      hiddenWorkspaceIds: set([b]),
    });
    expect(out.selectedWorkspaceIds).toEqual(set([a, c]));
  });

  test("toggle_adds_then_removes_and_may_empty", () => {
    const { a, b } = U;
    const live = [a, b];
    const added = click({
      clickedIndex: 1,
      modifiers: { shift: false, toggle: true },
      selectedWorkspaceIds: set([a]),
      liveWorkspaceIds: live,
    });
    expect(added.selectedWorkspaceIds).toEqual(set([a, b]));
    // Toggle moves the anchor to the clicked row.
    expect(added.anchorIndex).toBe(1);
    // Removal may leave the set empty — canonical allows it
    // (ContentView.swift:14306-14307); no auto-refill.
    const removed = click({
      clickedIndex: 0,
      modifiers: { shift: false, toggle: true },
      selectedWorkspaceIds: set([a]),
      liveWorkspaceIds: live,
    });
    expect(removed.selectedWorkspaceIds.size).toBe(0);
    expect(removed.anchorIndex).toBe(0);
  });

  test("plain_click_selects_only_the_clicked_row", () => {
    const { a, b, c } = U;
    const out = click({
      clickedIndex: 1,
      modifiers: { shift: false, toggle: false },
      existingAnchorIndex: 2,
      selectedWorkspaceIds: set([a, c]),
      liveWorkspaceIds: [a, b, c],
    });
    expect(out.selectedWorkspaceIds).toEqual(set([b]));
    // Plain click moves the anchor to the clicked row.
    expect(out.anchorIndex).toBe(1);
  });

  test("shift_without_resolvable_anchor_falls_through", () => {
    const { a, b } = U;
    // Empty selection + no focus + no anchor → shift cannot resolve; falls to
    // the plain branch, and the anchor maps nil→clickedIndex
    // (ContentView.swift:14267-14274 + anchorIndexAfterWorkspaceClick).
    const plain = click({
      clickedIndex: 1,
      modifiers: { shift: true, toggle: false },
      liveWorkspaceIds: [a, b],
    });
    expect(plain.selectedWorkspaceIds).toEqual(set([b]));
    expect(plain.anchorIndex).toBe(1);
    // Same fall-through with toggle held → the cmd branch toggles.
    const toggled = click({
      clickedIndex: 1,
      modifiers: { shift: true, toggle: true },
      liveWorkspaceIds: [a, b],
    });
    expect(toggled.selectedWorkspaceIds).toEqual(set([b]));
    expect(toggled.anchorIndex).toBe(1);
  });

  test("shift_derives_anchor_from_focus_when_no_index", () => {
    const { a, b, c } = U;
    // No stored anchor, multi-selection → derive from the focused workspace
    // (shiftClickAnchorIndex derivation order).
    const out = click({
      clickedIndex: 2,
      modifiers: { shift: true, toggle: false },
      selectedWorkspaceIds: set([a, c]),
      focusedWorkspaceId: b,
      liveWorkspaceIds: [a, b, c],
    });
    expect(out.selectedWorkspaceIds).toEqual(set([b, c]));
    expect(out.anchorIndex).toBe(1);
  });
});
